#!/usr/bin/env python3
"""Run a Covey apply proof with worker/reviewer/apply/closer in separate OS processes."""

from __future__ import annotations

import argparse
import base64
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))

from write_provider_run_id_file_from_env import provider_run_payload
from mutai_digest import blake3_file


COVEY_BIN = "covey"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", default="/data/projects/audio-transcriptify")
    parser.add_argument("--mainline-ref", default="origin/master")
    parser.add_argument("--evidence-dir")
    parser.add_argument("--subtask-id")
    parser.add_argument("--review-subtask-id")
    parser.add_argument(
        "--mission-packet-file",
        help=(
            "Optional Better Droid mission-packet JSON to retain under the "
            "evidence directory and bind into final replay."
        ),
    )
    parser.add_argument(
        "--enforce-promoted-mission-identity-contract",
        action="store_true",
        help=(
            "Pass the retained mission packet to the final verifier and "
            "require its promoted runtime identity contract."
        ),
    )
    parser.add_argument(
        "--feature-patch-from-head",
        action="store_true",
        help="Use `git show --format= --binary HEAD` from --repo as the worker artifact.",
    )
    parser.add_argument(
        "--feature-patch-ref",
        default="HEAD",
        help="Commit ref used with --feature-patch-from-head.",
    )
    parser.add_argument(
        "--provider-run-id-prefix",
        help=(
            "Optional externally supplied provider run id prefix. Use only when "
            "the actor runtime has provider-issued run identifiers to bind."
        ),
    )
    parser.add_argument(
        "--provider-run-id-file",
        help=(
            "Optional JSON file containing exact provider-issued run ids for "
            "worker, reviewer, and apply_gate actors."
        ),
    )
    parser.add_argument(
        "--provider-run-id-public-key",
        help=(
            "Public key used to verify a signed provider-run-id file. "
            "Required when --require-signed-provider-run-ids is set."
        ),
    )
    parser.add_argument(
        "--require-signed-provider-run-ids",
        action="store_true",
        help=(
            "Require --provider-run-id-file to contain an Ed25519 signature "
            "over its canonical provider-run payload."
        ),
    )
    parser.add_argument(
        "--provider-run-id-env",
        action="store_true",
        help=(
            "Read exact provider-issued run ids from MUTAI_* provider-run "
            "environment variables."
        ),
    )
    parser.add_argument(
        "--provider-run-id-issuer",
        default="provider-contract",
        help="Issuer label for provider run ids supplied with --provider-run-id-prefix.",
    )
    parser.add_argument(
        "--require-provider-run-ids",
        action="store_true",
        help="Require provider run ids in the closer replay verifier.",
    )
    parser.add_argument(
        "--promoted-provider-run-ids",
        action="store_true",
        help=(
            "Promoted proof mode: require provider-run IDs from file or env, "
            "require a trusted issuer, and forbid local/test issuers."
        ),
    )
    parser.add_argument(
        "--trusted-provider-run-id-issuer",
        action="append",
        default=[],
        help=(
            "Provider run ID issuer accepted by the closer replay verifier. "
            "May be specified more than once."
        ),
    )
    parser.add_argument(
        "--forbidden-provider-run-id-issuer",
        action="append",
        default=[],
        help=(
            "Provider run ID issuer rejected by the closer replay verifier. "
            "May be specified more than once."
        ),
    )
    parser.add_argument("--actor", choices=["worker", "reviewer", "apply_gate", "closer"])
    parser.add_argument("--db")
    parser.add_argument("--state")
    args = parser.parse_args()

    if args.actor is not None:
        if args.db is None or args.state is None:
            raise SystemExit("--actor requires --db and --state")
        return actor_main(args.actor, Path(args.db), Path(args.state))

    return orchestrate(args)


def orchestrate(args: argparse.Namespace) -> int:
    repo = Path(args.repo).resolve()
    evidence_dir = (
        Path(args.evidence_dir).resolve()
        if args.evidence_dir
        else None
    )

    provider_run_id_file = (
        Path(args.provider_run_id_file).resolve()
        if args.provider_run_id_file
        else None
    )
    provider_run_id_public_key = (
        Path(args.provider_run_id_public_key).resolve()
        if args.provider_run_id_public_key
        else None
    )
    provider_sources = [
        bool(args.provider_run_id_prefix),
        provider_run_id_file is not None,
        bool(args.provider_run_id_env),
    ]
    if sum(provider_sources) > 1:
        raise SystemExit(
            "--provider-run-id-prefix, --provider-run-id-file, and "
            "--provider-run-id-env are mutually exclusive"
        )
    if args.require_signed_provider_run_ids and provider_run_id_file is None:
        raise SystemExit("--require-signed-provider-run-ids requires --provider-run-id-file")
    if args.require_signed_provider_run_ids and provider_run_id_public_key is None:
        raise SystemExit(
            "--require-signed-provider-run-ids requires --provider-run-id-public-key"
        )
    provider_run_id_input = (
        load_provider_run_id_input(
            provider_run_id_file,
            public_key_path=provider_run_id_public_key,
            require_signature=args.require_signed_provider_run_ids,
        )
        if provider_run_id_file is not None
        else None
    )
    mission_packet_file = (
        Path(args.mission_packet_file).resolve()
        if args.mission_packet_file
        else None
    )
    if args.enforce_promoted_mission_identity_contract and mission_packet_file is None:
        raise SystemExit(
            "--enforce-promoted-mission-identity-contract requires "
            "--mission-packet-file"
        )
    if mission_packet_file is not None and not mission_packet_file.is_file():
        raise SystemExit(f"mission packet file is missing: {mission_packet_file}")
    if args.provider_run_id_env:
        try:
            provider_run_id_input = provider_run_payload(dict(os.environ))
        except ValueError as error:
            raise SystemExit(str(error)) from error
    trusted_provider_run_id_issuers = list(args.trusted_provider_run_id_issuer)
    forbidden_provider_run_id_issuers = list(args.forbidden_provider_run_id_issuer)
    require_provider_run_ids = args.require_provider_run_ids
    if args.promoted_provider_run_ids:
        if provider_run_id_input is None:
            raise SystemExit(
                "--promoted-provider-run-ids requires --provider-run-id-file "
                "or --provider-run-id-env"
            )
        require_provider_run_ids = True
        forbidden_provider_run_id_issuers.extend(
            ["mutai-local-proof-runner", "codex-env"]
        )
        issuer = provider_run_id_input["provider_run_id_issuer"]
        if issuer not in trusted_provider_run_id_issuers:
            raise SystemExit(
                "--promoted-provider-run-ids requires "
                f"--trusted-provider-run-id-issuer {issuer}"
            )
        if issuer in forbidden_provider_run_id_issuers:
            raise SystemExit("provider run id issuer is also forbidden")

    if evidence_dir is None:
        evidence_dir = Path(
            tempfile.mkdtemp(prefix="covey-actor-split-proof.", dir=temp_parent())
        )
    elif evidence_dir.exists() and any(evidence_dir.iterdir()):
        raise SystemExit(f"evidence dir is not empty: {evidence_dir}")
    else:
        evidence_dir.mkdir(parents=True, exist_ok=True)
    retained_mission_packet = None
    if mission_packet_file is not None:
        retained_mission_packet = evidence_dir / "mission-packet.json"
        shutil.copyfile(mission_packet_file, retained_mission_packet)

    db = evidence_dir / "covey.db"
    state = {
        "schema": "covey_actor_split_proof_state",
        "repo": str(repo),
        "mainline_ref": args.mainline_ref,
        "evidence_dir": str(evidence_dir),
        "db": str(db),
        "base_rev": git(repo, "rev-parse", f"{args.feature_patch_ref}^")
        if args.feature_patch_from_head
        else git(repo, "rev-parse", "HEAD"),
        "subtask_id": args.subtask_id or f"actor_split_proof_{os.getpid()}",
        "review_subtask_id": args.review_subtask_id
        or f"review_actor_split_proof_{os.getpid()}",
        "feature_patch_from_head": args.feature_patch_from_head,
        "feature_patch_ref": args.feature_patch_ref,
        "provider_run_id_prefix": args.provider_run_id_prefix,
        "provider_run_id_issuer": args.provider_run_id_issuer,
        "provider_run_id_input": provider_run_id_input,
        "provider_run_id_input_signature_required": args.require_signed_provider_run_ids,
        "require_provider_run_ids": require_provider_run_ids,
        "trusted_provider_run_id_issuers": trusted_provider_run_id_issuers,
        "forbidden_provider_run_id_issuers": forbidden_provider_run_id_issuers,
        "mission_packet_file": "mission-packet.json"
        if retained_mission_packet is not None
        else None,
        "enforce_promoted_mission_identity_contract": args.enforce_promoted_mission_identity_contract,
    }
    private_key_path = evidence_dir / "host-runtime-claim-private.pem"
    public_key_path = evidence_dir / "host-runtime-claim-public.pem"
    generate_ed25519_keypair(private_key_path, public_key_path)
    state["host_runtime_claim_private_key"] = str(private_key_path)
    state["host_runtime_claim_public_key"] = str(public_key_path)
    state_path = evidence_dir / "state.json"

    orchestrator = register(
        db,
        "actor-split-orchestrator",
        "actor-split-orchestrator-1",
        "orchestrator",
        "orchestrator-register",
    )
    state["orchestrator"] = orchestrator
    meta = covey(
        db,
        "meta",
        "submit",
        "--session-token",
        orchestrator["session_token"],
        "--prompt-text",
        "actor split proof",
        "--idempotency-key",
        "orchestrator-meta",
    )
    state["meta_task_id"] = meta["meta_task_id"]
    created = covey(
        db,
        "subtask",
        "create",
        "--session-token",
        orchestrator["session_token"],
        "--meta-task-id",
        state["meta_task_id"],
        "--subtask-id",
        state["subtask_id"],
        "--title",
        "Actor split proof work",
        "--kind",
        "work",
        "--priority",
        "1",
        "--idempotency-key",
        "orchestrator-create-work",
    )
    state["created_subtask"] = created
    write_json(state_path, state)

    state["worker"] = run_actor("worker", db, state_path)
    sign_runtime_claim(evidence_dir, private_key_path, public_key_path, "worker", state["worker"])
    write_json(state_path, state)

    state["reviewer"] = run_actor("reviewer", db, state_path)
    sign_runtime_claim(
        evidence_dir, private_key_path, public_key_path, "reviewer", state["reviewer"]
    )
    write_json(state_path, state)

    queue = covey(
        db,
        "queue",
        "enqueue",
        "--session-token",
        orchestrator["session_token"],
        "--artifact-digest",
        state["worker"]["artifact_digest"],
        "--subtask-id",
        state["subtask_id"],
        "--idempotency-key",
        "orchestrator-enqueue",
    )
    state["queue_id"] = queue["queue_id"]
    write_json(state_path, state)

    state["apply_gate"] = run_actor("apply_gate", db, state_path)
    sign_runtime_claim(
        evidence_dir,
        private_key_path,
        public_key_path,
        "apply_gate",
        state["apply_gate"],
    )
    state.pop("host_runtime_claim_private_key", None)
    private_key_path.unlink(missing_ok=True)
    write_json(state_path, state)

    state["closer"] = run_actor("closer", db, state_path)
    write_json(state_path, state)

    summary = {
        "accepted": True,
        "evidence_dir": str(evidence_dir),
        "db": str(db),
        "repo": str(repo),
        "mainline_ref": args.mainline_ref,
        "subtask_id": state["subtask_id"],
        "artifact_digest": state["worker"]["artifact_digest"],
        "review_id": state["worker"]["review_id"],
        "queue_id": state["queue_id"],
        "reviewer_findings_digest": state["reviewer"]["findings_digest"],
        "apply_verification_seal_digest": state["apply_gate"][
            "apply_verification_seal_digest"
        ],
        "final_seal_digest": state["closer"]["seal_digest"],
        "host_runtime_claim_public_key_blake3": blake3_file(public_key_path),
        "actor_process_ids": {
            "worker": state["worker"]["pid"],
            "reviewer": state["reviewer"]["pid"],
            "apply_gate": state["apply_gate"]["pid"],
            "closer": state["closer"]["pid"],
        },
    }
    provider_run_ids = {
        role: state[role].get("provider_run_id")
        for role in ("worker", "reviewer", "apply_gate")
        if state[role].get("provider_run_id")
    }
    if provider_run_ids:
        summary["provider_run_ids"] = provider_run_ids
        summary["provider_run_id_issuers"] = {
            role: state[role].get("provider_run_id_issuer")
            for role in ("worker", "reviewer", "apply_gate")
            if state[role].get("provider_run_id_issuer")
        }
    write_json(evidence_dir / "actor-split-summary.json", summary)
    print(json.dumps(summary, indent=2, sort_keys=True))
    print(f"proof retained at {evidence_dir}")
    return 0


def actor_main(actor: str, db: Path, state_path: Path) -> int:
    state = read_json(state_path)
    if actor == "worker":
        result = worker_actor(db, state)
    elif actor == "reviewer":
        result = reviewer_actor(db, state)
    elif actor == "apply_gate":
        result = apply_gate_actor(db, state)
    elif actor == "closer":
        result = closer_actor(db, state)
    else:
        raise AssertionError(actor)
    print(json.dumps(result, sort_keys=True))
    return 0


def worker_actor(db: Path, state: dict[str, Any]) -> dict[str, Any]:
    actor = ActorLog(Path(state["evidence_dir"]), "worker")
    started_at = now_ms()
    session = actor.covey(
        db,
        "session",
        "register",
        "--agent-principal-id",
        "actor-split-worker",
        "--agent-instance-id",
        f"worker-{os.getpid()}",
        "--role",
        "executor",
        "--idempotency-key",
        "worker-register",
    )
    claim = actor.covey(
        db,
        "subtask",
        "claim-next",
        "--session-token",
        session["session_token"],
        "--lease-duration-ms",
        "30000",
        "--idempotency-key",
        "worker-claim",
    )
    actor.covey(
        db,
        "subtask",
        "start",
        "--session-token",
        session["session_token"],
        "--claim-id",
        claim["claim_id"],
        "--fence-seq",
        str(claim["fence_seq"]),
        "--idempotency-key",
        "worker-start",
    )

    artifact_file = Path(state["evidence_dir"]) / "feature.patch"
    if state.get("feature_patch_from_head"):
        artifact_file.write_text(
            git(
                Path(state["repo"]),
                "show",
                "--format=",
                "--binary",
                state["feature_patch_ref"],
            ),
            encoding="utf-8",
        )
    else:
        artifact_file.write_text(
            "actor split proof artifact\n"
            f"subtask={state['subtask_id']}\n"
            f"worker_pid={os.getpid()}\n",
            encoding="utf-8",
        )
    artifact_digest = blake3_file(artifact_file)
    changed_paths_file = Path(state["evidence_dir"]) / "changed-paths.txt"
    if state.get("feature_patch_from_head"):
        changed_paths_file.write_text(
            git(
                Path(state["repo"]),
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "-r",
                state["feature_patch_ref"],
            )
            + "\n",
            encoding="utf-8",
        )
    else:
        changed_paths_file.write_text("feature.patch\n", encoding="utf-8")
    changed_paths_digest = blake3_file(changed_paths_file)
    manifest = {
        "schema": "actor_split_artifact_manifest",
        "artifact_file": "feature.patch",
        "artifact_digest": artifact_digest,
        "changed_paths_digest": changed_paths_digest,
        "base_rev": state["base_rev"],
    }
    manifest_path = Path(state["evidence_dir"]) / "artifact-manifest.json"
    write_json(manifest_path, manifest)
    actor.covey(
        db,
        "artifact",
        "publish",
        "--session-token",
        session["session_token"],
        "--claim-id",
        claim["claim_id"],
        "--fence-seq",
        str(claim["fence_seq"]),
        "--artifact-digest",
        artifact_digest,
        "--artifact-kind",
        "patch-bundle",
        "--base-rev",
        state["base_rev"],
        "--manifest-path",
        str(manifest_path),
        "--changed-paths-digest",
        changed_paths_digest,
        "--idempotency-key",
        "worker-publish",
    )
    review = actor.covey(
        db,
        "review",
        "request",
        "--session-token",
        session["session_token"],
        "--subtask-id",
        state["subtask_id"],
        "--artifact-digest",
        artifact_digest,
        "--review-subtask-id",
        state["review_subtask_id"],
        "--idempotency-key",
        "worker-request-review",
    )
    transcript_digest = actor.write_transcript()
    ended_at = now_ms()
    run_identity = provider_run_identity(state, "worker")
    attest(
        db,
        session["session_token"],
        "codex-local",
        "actor-split-worker",
        run_identity["provider_run_id"],
        run_identity["provider_run_id_issuer"],
        os.getpid(),
        transcript_digest,
        started_at,
        ended_at,
        "worker-attest",
    )
    result = {
        "pid": os.getpid(),
        "session_token": session["session_token"],
        "agent_principal_id": session["agent_principal_id"],
        "agent_instance_id": session["agent_instance_id"],
        "role": session["role"],
        "provider": "codex-local",
        "model": "actor-split-worker",
        "claim_id": claim["claim_id"],
        "fence_seq": claim["fence_seq"],
        "artifact_digest": artifact_digest,
        "changed_paths_digest": changed_paths_digest,
        "review_id": review["review_id"],
        "transcript_digest": transcript_digest,
        "started_at": started_at,
        "ended_at": ended_at,
    }
    result.update(run_identity)
    return result


def reviewer_actor(db: Path, state: dict[str, Any]) -> dict[str, Any]:
    actor = ActorLog(Path(state["evidence_dir"]), "reviewer")
    started_at = now_ms()
    session = actor.covey(
        db,
        "session",
        "register",
        "--agent-principal-id",
        "actor-split-reviewer",
        "--agent-instance-id",
        f"reviewer-{os.getpid()}",
        "--role",
        "reviewer",
        "--idempotency-key",
        "reviewer-register",
    )
    claim = actor.covey(
        db,
        "subtask",
        "claim-next",
        "--session-token",
        session["session_token"],
        "--lease-duration-ms",
        "30000",
        "--idempotency-key",
        "reviewer-claim",
    )
    actor.covey(
        db,
        "subtask",
        "start",
        "--session-token",
        session["session_token"],
        "--claim-id",
        claim["claim_id"],
        "--fence-seq",
        str(claim["fence_seq"]),
        "--idempotency-key",
        "reviewer-start",
    )
    findings = {
        "schema": "actor_split_reviewer_findings",
        "review_id": state["worker"]["review_id"],
        "artifact_digest": state["worker"]["artifact_digest"],
        "reviewer_pid": os.getpid(),
        "verdict": "approve",
        "summary": "reviewed exact artifact digest for actor split proof",
    }
    findings_path = Path(state["evidence_dir"]) / "reviewer-findings.json"
    write_json(findings_path, findings)
    findings_digest = blake3_file(findings_path)
    actor.covey(
        db,
        "review",
        "decide",
        "--session-token",
        session["session_token"],
        "--review-id",
        state["worker"]["review_id"],
        "--claim-id",
        claim["claim_id"],
        "--fence-seq",
        str(claim["fence_seq"]),
        "--verdict",
        "approve",
        "--findings-digest",
        findings_digest,
        "--idempotency-key",
        "reviewer-decide",
    )
    transcript_digest = actor.write_transcript()
    ended_at = now_ms()
    run_identity = provider_run_identity(state, "reviewer")
    attest(
        db,
        session["session_token"],
        "codex-local",
        "actor-split-reviewer",
        run_identity["provider_run_id"],
        run_identity["provider_run_id_issuer"],
        os.getpid(),
        transcript_digest,
        started_at,
        ended_at,
        "reviewer-attest",
    )
    result = {
        "pid": os.getpid(),
        "session_token": session["session_token"],
        "agent_principal_id": session["agent_principal_id"],
        "agent_instance_id": session["agent_instance_id"],
        "role": session["role"],
        "provider": "codex-local",
        "model": "actor-split-reviewer",
        "claim_id": claim["claim_id"],
        "fence_seq": claim["fence_seq"],
        "findings_digest": findings_digest,
        "transcript_digest": transcript_digest,
        "started_at": started_at,
        "ended_at": ended_at,
    }
    result.update(run_identity)
    return result


def apply_gate_actor(db: Path, state: dict[str, Any]) -> dict[str, Any]:
    actor = ActorLog(Path(state["evidence_dir"]), "apply-gate")
    started_at = now_ms()
    session = actor.covey(
        db,
        "session",
        "register",
        "--agent-principal-id",
        "actor-split-apply-gate",
        "--agent-instance-id",
        f"apply-gate-{os.getpid()}",
        "--role",
        "apply-gate",
        "--idempotency-key",
        "apply-gate-register",
    )
    queue_claim = actor.covey(
        db,
        "queue",
        "claim-next",
        "--session-token",
        session["session_token"],
        "--lease-duration-ms",
        "30000",
        "--idempotency-key",
        "apply-gate-claim",
    )
    verdict = {
        "schema_version": "mutai_apply_gate_check_result.v1",
        "accepted": True,
        "authority_binding": {
            "queue_item_id": state["queue_id"],
            "artifact_digest": state["worker"]["artifact_digest"],
            "review_id": state["worker"]["review_id"],
            "findings_digest": state["reviewer"]["findings_digest"],
            "apply_fence_seq": queue_claim["claim_fence_seq"],
        },
        "blockers": [],
    }
    verdict_path = Path(state["evidence_dir"]) / "apply-gate-output.json"
    write_json(verdict_path, verdict)
    verdict_digest = blake3_file(verdict_path)
    seal_input = {
        "schema": "actor_split_apply_verification_seal_input",
        "queue_id": state["queue_id"],
        "artifact_digest": state["worker"]["artifact_digest"],
        "review_id": state["worker"]["review_id"],
        "findings_digest": state["reviewer"]["findings_digest"],
        "claim_fence_seq": queue_claim["claim_fence_seq"],
        "verdict_digest": verdict_digest,
    }
    seal_input_path = Path(state["evidence_dir"]) / "apply-verification-seal-input.json"
    write_json(seal_input_path, seal_input)
    apply_verification_seal_digest = blake3_file(seal_input_path)
    transcript_digest = actor.write_transcript()
    ended_at = now_ms()
    run_identity = provider_run_identity(state, "apply_gate")
    attest(
        db,
        session["session_token"],
        "codex-local",
        "actor-split-apply-gate",
        run_identity["provider_run_id"],
        run_identity["provider_run_id_issuer"],
        os.getpid(),
        transcript_digest,
        started_at,
        ended_at,
        "apply-gate-attest",
    )
    actor.covey(
        db,
        "queue",
        "record-apply-verification",
        "--session-token",
        session["session_token"],
        "--queue-id",
        state["queue_id"],
        "--artifact-digest",
        state["worker"]["artifact_digest"],
        "--review-id",
        state["worker"]["review_id"],
        "--findings-digest",
        state["reviewer"]["findings_digest"],
        "--claim-fence-seq",
        str(queue_claim["claim_fence_seq"]),
        "--verifier",
        "mutai-rs:settlement-apply-gate",
        "--verdict-digest",
        verdict_digest,
        "--seal-digest",
        apply_verification_seal_digest,
        "--idempotency-key",
        "apply-gate-record-verification",
    )
    actor.covey(
        db,
        "queue",
        "mark-applied",
        "--session-token",
        session["session_token"],
        "--queue-id",
        state["queue_id"],
        "--claim-fence-seq",
        str(queue_claim["claim_fence_seq"]),
        "--idempotency-key",
        "apply-gate-mark-applied",
    )
    result = {
        "pid": os.getpid(),
        "session_token": session["session_token"],
        "agent_principal_id": session["agent_principal_id"],
        "agent_instance_id": session["agent_instance_id"],
        "role": session["role"],
        "provider": "codex-local",
        "model": "actor-split-apply-gate",
        "queue_id": state["queue_id"],
        "claim_fence_seq": queue_claim["claim_fence_seq"],
        "verdict_digest": verdict_digest,
        "apply_verification_seal_digest": apply_verification_seal_digest,
        "transcript_digest": transcript_digest,
        "started_at": started_at,
        "ended_at": ended_at,
    }
    result.update(run_identity)
    return result


def closer_actor(db: Path, state: dict[str, Any]) -> dict[str, Any]:
    evidence_dir = Path(state["evidence_dir"])
    output = evidence_dir / "final-evidence-seal.json"
    transcript = []
    argv = [
        str(COVEY_BIN),
        "proof",
        "apply",
        "verify",
        "--repo",
        state["repo"],
        "--covey-db",
        str(db),
        "--evidence-dir",
        str(evidence_dir),
        "--subtask-id",
        state["subtask_id"],
        "--artifact-digest",
        state["worker"]["artifact_digest"],
        "--review-id",
        state["worker"]["review_id"],
        "--queue-id",
        state["queue_id"],
        "--reviewer-findings-digest",
        state["reviewer"]["findings_digest"],
        "--apply-gate-session-token",
        state["apply_gate"]["session_token"],
        "--verifier",
        "mutai-rs:settlement-apply-gate",
        "--verdict-digest",
        state["apply_gate"]["verdict_digest"],
        "--apply-verification-seal-digest",
        state["apply_gate"]["apply_verification_seal_digest"],
        "--mainline-ref",
        state["mainline_ref"],
        "--artifact-file",
        "feature.patch",
        "--verdict-file",
        "apply-gate-output.json",
        "--require-observed-process-ids",
        "--require-host-signed-runtime-claims",
        "--output",
        str(output),
    ]
    if state.get("require_provider_run_ids"):
        argv.append("--require-provider-run-ids")
    if state.get("mission_packet_file"):
        argv.extend(["--mission-packet-file", state["mission_packet_file"]])
    if state.get("enforce_promoted_mission_identity_contract"):
        argv.append("--enforce-promoted-mission-identity-contract")
    for issuer in state.get("trusted_provider_run_id_issuers") or []:
        argv.extend(["--trusted-provider-run-id-issuer", issuer])
    for issuer in state.get("forbidden_provider_run_id_issuers") or []:
        argv.extend(["--forbidden-provider-run-id-issuer", issuer])
    if state.get("feature_patch_from_head"):
        argv.extend(["--subject-ref", state["feature_patch_ref"]])
    result = subprocess.run(argv, text=True, capture_output=True)
    transcript.append(
        {
            "argv": argv,
            "returncode": result.returncode,
            "stdout": result.stdout,
            "stderr": result.stderr,
        }
    )
    write_json(evidence_dir / "closer-transcript.json", transcript)
    if result.returncode != 0:
        raise RuntimeError(result.stdout or result.stderr)
    payload = json.loads(result.stdout)
    return {
        "pid": os.getpid(),
        "seal": str(output),
        "seal_digest": payload["seal_digest"],
        "transcript_digest": blake3_file(evidence_dir / "closer-transcript.json"),
    }


class ActorLog:
    def __init__(self, evidence_dir: Path, label: str) -> None:
        self.evidence_dir = evidence_dir
        self.label = label
        self.entries: list[dict[str, Any]] = []

    def covey(self, db: Path, *args: str) -> dict[str, Any]:
        argv = [str(COVEY_BIN), "--db", str(db), "--json", *args]
        result = subprocess.run(argv, text=True, capture_output=True)
        entry: dict[str, Any] = {
            "argv": argv,
            "returncode": result.returncode,
            "stdout": result.stdout,
            "stderr": result.stderr,
        }
        if result.stdout:
            try:
                entry["parsed_stdout"] = json.loads(result.stdout)
            except json.JSONDecodeError:
                pass
        self.entries.append(entry)
        if result.returncode != 0:
            raise RuntimeError(result.stderr or result.stdout)
        payload = json.loads(result.stdout)
        if not payload.get("ok"):
            raise RuntimeError(result.stdout)
        return payload["data"]

    def write_transcript(self) -> str:
        path = self.evidence_dir / f"{self.label}-transcript.json"
        write_json(path, self.entries)
        return blake3_file(path)


def run_actor(actor: str, db: Path, state: Path) -> dict[str, Any]:
    result = subprocess.run(
        [
            sys.executable,
            str(Path(__file__).resolve()),
            "--actor",
            actor,
            "--db",
            str(db),
            "--state",
            str(state),
        ],
        text=True,
        capture_output=True,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr or result.stdout)
    return json.loads(result.stdout)


def register(
    db: Path, principal: str, instance: str, role: str, idempotency_key: str
) -> dict[str, Any]:
    return covey(
        db,
        "session",
        "register",
        "--agent-principal-id",
        principal,
        "--agent-instance-id",
        instance,
        "--role",
        role,
        "--idempotency-key",
        idempotency_key,
    )


def attest(
    db: Path,
    session_token: str,
    provider: str,
    model: str,
    provider_run_id: str,
    provider_run_id_issuer: str,
    pid: int,
    transcript_digest: str,
    started_at: int,
    ended_at: int,
    idempotency_key: str,
) -> dict[str, Any]:
    return covey(
        db,
        "session",
        "attest",
        "--session-token",
        session_token,
        "--provider",
        provider,
        "--model",
        model,
        "--provider-run-id",
        provider_run_id,
        "--provider-run-id-issuer",
        provider_run_id_issuer,
        "--process-id",
        str(pid),
        "--command-transcript-digest",
        transcript_digest,
        "--started-at",
        str(started_at),
        "--ended-at",
        str(ended_at),
        "--idempotency-key",
        idempotency_key,
    )


def generate_ed25519_keypair(private_key_path: Path, public_key_path: Path) -> None:
    subprocess.run(
        ["openssl", "genpkey", "-algorithm", "ed25519", "-out", str(private_key_path)],
        check=True,
        capture_output=True,
    )
    subprocess.run(
        [
            "openssl",
            "pkey",
            "-in",
            str(private_key_path),
            "-pubout",
            "-out",
            str(public_key_path),
        ],
        check=True,
        capture_output=True,
    )


def sign_runtime_claim(
    evidence_dir: Path,
    private_key_path: Path,
    public_key_path: Path,
    actor_role: str,
    actor: dict[str, Any],
) -> None:
    payload = {
        "schema": "mutai_host_signed_runtime_claim",
        "actor_role": actor_role,
        "session_token": actor["session_token"],
        "agent_principal_id": actor["agent_principal_id"],
        "agent_instance_id": actor["agent_instance_id"],
        "role": actor["role"],
        "provider": actor["provider"],
        "model": actor["model"],
        "process_id": str(actor["pid"]),
        "container_id": None,
        "command_transcript_digest": actor["transcript_digest"],
        "started_at": int(actor["started_at"]),
        "ended_at": int(actor["ended_at"]),
    }
    if actor.get("provider_run_id"):
        payload["provider_run_id"] = actor["provider_run_id"]
        payload["provider_run_id_issuer"] = actor.get("provider_run_id_issuer", "unknown")
    payload_bytes = canonical_json(payload)
    payload_path = evidence_dir / f"{actor_role.replace('_', '-')}-runtime-claim.payload.json"
    signature_path = evidence_dir / f"{actor_role.replace('_', '-')}-runtime-claim.sig"
    payload_path.write_bytes(payload_bytes)
    subprocess.run(
        [
            "openssl",
            "pkeyutl",
            "-sign",
            "-inkey",
            str(private_key_path),
            "-rawin",
            "-in",
            str(payload_path),
            "-out",
            str(signature_path),
        ],
        check=True,
        capture_output=True,
    )
    claim = {
        "schema": "mutai_host_signed_runtime_claim_envelope",
        "payload": payload,
        "public_key_pem": public_key_path.read_text(encoding="utf-8"),
        "signature_base64": base64.b64encode(signature_path.read_bytes()).decode("ascii"),
    }
    write_json(evidence_dir / f"{actor_role.replace('_', '-')}-runtime-claim.json", claim)
    payload_path.unlink()
    signature_path.unlink()


def provider_run_identity(state: dict[str, Any], actor_role: str) -> dict[str, str]:
    input_payload = state.get("provider_run_id_input")
    if isinstance(input_payload, dict):
        run_ids = input_payload.get("provider_run_ids")
        issuer = input_payload.get("provider_run_id_issuer")
        if isinstance(run_ids, dict) and isinstance(issuer, str) and issuer.strip():
            run_id = run_ids.get(actor_role)
            if isinstance(run_id, str) and run_id.strip():
                return {
                    "provider_run_id": run_id,
                    "provider_run_id_issuer": issuer,
                }
        raise RuntimeError(f"provider run id input is missing actor role {actor_role}")

    prefix = state.get("provider_run_id_prefix")
    if not isinstance(prefix, str) or not prefix.strip():
        prefix = f"local-proof-run:{state['subtask_id']}"
        issuer = "mutai-local-proof-runner"
    else:
        issuer = state.get("provider_run_id_issuer")
        if not isinstance(issuer, str) or not issuer.strip():
            issuer = "unknown"
    return {
        "provider_run_id": f"{prefix}:{actor_role}:{os.getpid()}",
        "provider_run_id_issuer": issuer,
    }


def load_provider_run_id_input(
    path: Path,
    *,
    public_key_path: Path | None,
    require_signature: bool,
) -> dict[str, Any]:
    payload = read_json(path)
    if payload.get("schema") != "mutai_provider_run_ids.v1":
        raise SystemExit("provider run id file schema must be mutai_provider_run_ids.v1")
    issuer = payload.get("provider_run_id_issuer")
    if not isinstance(issuer, str) or not issuer.strip():
        raise SystemExit("provider run id file must contain provider_run_id_issuer")
    run_ids = payload.get("provider_run_ids")
    if not isinstance(run_ids, dict):
        raise SystemExit("provider run id file must contain provider_run_ids object")
    normalized: dict[str, str] = {}
    for role in ("worker", "reviewer", "apply_gate"):
        value = run_ids.get(role)
        if not isinstance(value, str) or not value.strip():
            raise SystemExit(f"provider run id file must contain provider_run_ids.{role}")
        normalized[role] = value
    if len(set(normalized.values())) != len(normalized):
        raise SystemExit("provider run id file must contain distinct run ids per actor role")
    normalized_payload: dict[str, Any] = {
        "schema": "mutai_provider_run_ids.v1",
        "provider_run_id_issuer": issuer,
        "provider_run_ids": normalized,
    }
    supervision = normalized_actor_supervision(payload)
    if supervision is not None:
        normalized_payload["actor_supervision"] = supervision
    signature = payload.get("signature")
    if signature is None:
        if require_signature:
            raise SystemExit("provider run id file must contain signature")
        return normalized_payload
    if not isinstance(signature, dict):
        raise SystemExit("provider run id file signature must be an object")
    if public_key_path is None:
        raise SystemExit("provider run id signature verification requires --provider-run-id-public-key")
    verify_provider_run_id_signature(normalized_payload, signature, public_key_path)
    normalized_payload["signature"] = {
        "algorithm": "ed25519",
        "verified": True,
        "public_key_blake3": blake3_file(public_key_path),
    }
    return normalized_payload


def normalized_actor_supervision(payload: dict[str, Any]) -> dict[str, str] | None:
    supervision = payload.get("actor_supervision")
    if supervision is None:
        return None
    if not isinstance(supervision, dict):
        raise SystemExit("provider run id file actor_supervision must be an object")
    normalized: dict[str, str] = {}
    for field in ("supervisor_id", "supervisor_run_id", "actor_process_model"):
        value = supervision.get(field)
        if not isinstance(value, str) or not value.strip():
            raise SystemExit(f"provider run id file actor_supervision.{field} is required")
        normalized[field] = value.strip()
    if normalized["actor_process_model"] != "separate_provider_runs":
        raise SystemExit(
            "provider run id file actor_supervision.actor_process_model must be separate_provider_runs"
        )
    return normalized


def verify_provider_run_id_signature(
    payload: dict[str, Any], signature: dict[str, Any], public_key_path: Path
) -> None:
    if signature.get("algorithm") != "ed25519":
        raise SystemExit("provider run id signature algorithm must be ed25519")
    signature_base64 = signature.get("signature_base64")
    if not isinstance(signature_base64, str) or not signature_base64.strip():
        raise SystemExit("provider run id signature must contain signature_base64")
    try:
        signature_bytes = base64.b64decode(signature_base64, validate=True)
    except ValueError as error:
        raise SystemExit("provider run id signature_base64 is invalid") from error
    if not public_key_path.is_file():
        raise SystemExit(f"provider run id public key is missing: {public_key_path}")
    with tempfile.TemporaryDirectory(prefix="provider-run-id-signature.", dir=temp_parent()) as tmp:
        root = Path(tmp)
        payload_path = root / "payload.json"
        signature_path = root / "signature.bin"
        payload_path.write_bytes(canonical_json(payload))
        signature_path.write_bytes(signature_bytes)
        result = subprocess.run(
            [
                "openssl",
                "pkeyutl",
                "-verify",
                "-pubin",
                "-inkey",
                str(public_key_path),
                "-rawin",
                "-in",
                str(payload_path),
                "-sigfile",
                str(signature_path),
            ],
            text=True,
            capture_output=True,
        )
    if result.returncode != 0:
        raise SystemExit("provider run id signature verification failed")


def covey(db: Path, *args: str) -> dict[str, Any]:
    result = subprocess.run(
        [str(COVEY_BIN), "--db", str(db), "--json", *args],
        text=True,
        capture_output=True,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr or result.stdout)
    payload = json.loads(result.stdout)
    if not payload.get("ok"):
        raise RuntimeError(result.stdout)
    return payload["data"]


def git(repo: Path, *args: str) -> str:
    return subprocess.check_output(["git", *args], cwd=repo, text=True).strip()


def now_ms() -> int:
    return int(time.time() * 1000)


def write_json(path: Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def canonical_json(payload: Any) -> bytes:
    return json.dumps(payload, sort_keys=True, separators=(",", ":")).encode("utf-8")


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def temp_parent() -> str | None:
    path = Path("/data/tmp")
    if path.is_dir():
        return str(path)
    return None


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(json.dumps({"accepted": False, "error": str(error)}), file=sys.stderr)
        raise

use std::{fs, path::Path, process::Command};

use better_droid::{CompileOptions, compile_change};
use covey::{
    ClaimSubtaskReq, Covey, CoveyError, ImportBdV1Req, MetaTaskState, RegisterSessionReq,
    SessionRole, SubmitMetaTaskReq,
};
use rusqlite::{Connection, params};
use serde_json::Value;
use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_covey")
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin()).args(args).output().expect("run covey")
}

fn run_db(db_path: &Path, args: &[&str]) -> std::process::Output {
    let mut command = Command::new(bin());
    command.arg("--db").arg(db_path).args(args);
    command.output().expect("run covey with db")
}

fn parse_stdout_json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout json")
}

fn parse_stderr_json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stderr).expect("stderr json")
}

fn success_data(output: &std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = parse_stdout_json(output);
    assert_eq!(payload["ok"], Value::Bool(true));
    payload["data"].clone()
}

fn stderr_code(output: &std::process::Output) -> String {
    let payload = parse_stderr_json(output);
    assert_eq!(payload["ok"], Value::Bool(false));
    payload["code"].as_str().expect("error code").to_owned()
}

fn attest_session(db_path: &Path, session_token: &str, role_label: &str) {
    let process_id = format!("pid-{role_label}");
    let provider_run_id = format!("provider-run-{role_label}");
    let transcript_digest = format!("blake3:{role_label}-transcript");
    let idempotency_key = format!("record-runtime-attestation-{role_label}");
    success_data(&run_db(
        db_path,
        &[
            "session",
            "attest",
            "--session-token",
            session_token,
            "--provider",
            "covey-test",
            "--model",
            "test-model",
            "--provider-run-id",
            &provider_run_id,
            "--provider-run-id-issuer",
            "covey-test-provider",
            "--process-id",
            &process_id,
            "--command-transcript-digest",
            &transcript_digest,
            "--started-at",
            "1700000000000",
            "--ended-at",
            "1700000000001",
            "--idempotency-key",
            &idempotency_key,
        ],
    ));
}

fn register_orchestrator(covey: &Covey, principal: &str) -> String {
    covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                principal,
                format!("{principal}-instance"),
                SessionRole::Orchestrator,
                format!("register-{principal}"),
            )
            .expect("valid session registration request"),
        )
        .expect("register orchestrator")
        .session_token
        .to_string()
}

fn seed_ready_queue_item(db_path: &Path, session_token: &str, subtask_id: &str, queue_id: &str) {
    let conn = Connection::open(db_path).expect("open seeded covey db");
    let fixture_now_ms: i64 = conn
        .query_row(
            "SELECT created_at FROM subtasks WHERE subtask_id = ?1",
            params![subtask_id],
            |row| row.get(0),
        )
        .expect("seeded subtask created_at");
    conn.execute(
        r#"
        INSERT INTO artifacts (
            artifact_digest, artifact_kind, base_rev, produced_by_subtask_id,
            produced_by_session, manifest_path, changed_paths_digest, created_at
        ) VALUES (?1, 'patch_bundle', 'base', ?2, ?3, 'manifest.json', 'blake3:paths', ?4)
        "#,
        params![
            "blake3:seeded-queue",
            subtask_id,
            session_token,
            fixture_now_ms
        ],
    )
    .expect("insert artifact fixture");
    conn.execute(
        "UPDATE subtasks SET state = 'ready_for_apply', artifact_digest = 'blake3:seeded-queue', updated_at = ?2 WHERE subtask_id = ?1",
        params![subtask_id, fixture_now_ms],
    )
    .expect("mark subtask fixture ready for apply");
    conn.execute(
        r#"
        INSERT INTO ready_queue (
            queue_id, artifact_digest, subtask_id, settlement_target, state,
            claimed_by_session_token, claim_fence_seq, claim_lease_deadline,
            enqueued_at, updated_at
        ) VALUES (?1, 'blake3:seeded-queue', ?2, 'canonical', 'queued', NULL, NULL, NULL, ?3, ?3)
        "#,
        params![queue_id, subtask_id, fixture_now_ms],
    )
    .expect("insert ready queue fixture");
}

#[test]
fn piped_success_defaults_to_json() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let output = run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "agent-a",
            "--agent-instance-id",
            "run-1",
            "--role",
            "executor",
        ],
    );

    let data = success_data(&output);
    assert_eq!(data["role"], "executor");
    assert!(
        data["session_token"]
            .as_str()
            .expect("token")
            .starts_with("session_")
    );
    assert!(
        output.stderr.is_empty(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn session_register_accepts_apply_gate_role_alias() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let output = run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "apply-gate-alias",
            "--agent-instance-id",
            "apply-gate-alias-run",
            "--role",
            "apply_gate",
        ],
    );

    let data = success_data(&output);
    assert_eq!(data["role"], "apply_gate");
}

#[test]
fn session_active_for_principal_returns_active_session_for_role() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let registered = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "agent-active",
            "--agent-instance-id",
            "run-active",
            "--role",
            "executor",
        ],
    ));

    let active = success_data(&run_db(
        &db,
        &[
            "session",
            "active-for-principal",
            "--agent-principal-id",
            "agent-active",
            "--role",
            "executor",
        ],
    ));
    assert_eq!(active["session_token"], registered["session_token"]);
    assert_eq!(active["agent_principal_id"], "agent-active");
    assert_eq!(active["role"], "executor");

    let mismatch = success_data(&run_db(
        &db,
        &[
            "session",
            "active-for-principal",
            "--agent-principal-id",
            "agent-active",
            "--role",
            "reviewer",
        ],
    ));
    assert_eq!(mismatch, Value::Null);
}

#[test]
fn session_attest_records_runtime_identity_json() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let session = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "agent-attest",
            "--agent-instance-id",
            "run-attest",
            "--role",
            "reviewer",
        ],
    ));
    let session_token = session["session_token"].as_str().expect("session token");

    let output = run_db(
        &db,
        &[
            "session",
            "attest",
            "--session-token",
            session_token,
            "--provider",
            "codex",
            "--model",
            "gpt-test",
            "--provider-run-id",
            "provider-run-123",
            "--provider-run-id-issuer",
            "codex-test-provider",
            "--process-id",
            "pid-123",
            "--command-transcript-digest",
            "blake3:transcript",
            "--started-at",
            "1700000000000",
            "--ended-at",
            "1700000000001",
        ],
    );

    let data = success_data(&output);
    assert_eq!(data["session_token"], session_token);
    assert_eq!(data["agent_principal_id"], "agent-attest");
    assert_eq!(data["role"], "reviewer");
    assert_eq!(data["provider"], "codex");
    assert_eq!(data["provider_run_id"], "provider-run-123");
    assert_eq!(data["provider_run_id_issuer"], "codex-test-provider");
    assert_eq!(data["command_transcript_digest"], "blake3:transcript");
}

#[test]
fn help_defaults_to_json_for_piped_output() {
    let output = run(&["--help"]);
    assert!(output.status.success());
    let payload = parse_stdout_json(&output);
    assert_eq!(payload["ok"], Value::Bool(true));
    assert_eq!(payload["data"]["command"], "covey");
    assert!(
        payload["data"]["help_text"]
            .as_str()
            .expect("help text")
            .contains("Covey local coordination CLI")
    );
}

#[test]
fn no_args_emit_short_json_help_for_piped_output() {
    let output = run(&[]);
    assert!(output.status.success());
    let payload = parse_stdout_json(&output);
    assert_eq!(payload["ok"], Value::Bool(true));
    assert_eq!(payload["data"]["usage"], "covey <group> <command> [flags]");
    assert_eq!(
        payload["data"]["groups"].as_array().expect("groups").len(),
        15
    );
}

#[test]
fn explicit_json_help_and_version_emit_success_envelopes() {
    let help = run(&["--json", "--help"]);
    assert!(help.status.success());
    let help_payload = parse_stdout_json(&help);
    assert_eq!(help_payload["ok"], Value::Bool(true));
    assert_eq!(help_payload["data"]["command"], "covey");

    let version = run(&["--json", "--version"]);
    assert!(version.status.success());
    let version_payload = parse_stdout_json(&version);
    assert_eq!(version_payload["ok"], Value::Bool(true));
    assert_eq!(version_payload["data"]["command"], "covey");
    assert!(version_payload["data"]["version"].as_str().is_some());
}

#[test]
fn queue_mark_in_flight_supersede_and_empty_claim_next_emit_json() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");

    let orch = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "queue-orch",
            "--agent-instance-id",
            "queue-orch-1",
            "--role",
            "orchestrator",
        ],
    ))["session_token"]
        .as_str()
        .expect("orch token")
        .to_owned();
    let gate = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "queue-gate",
            "--agent-instance-id",
            "queue-gate-1",
            "--role",
            "apply-gate",
        ],
    ))["session_token"]
        .as_str()
        .expect("gate token")
        .to_owned();
    let meta_task_id = success_data(&run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "queue command coverage",
        ],
    ))["meta_task_id"]
        .as_str()
        .expect("meta id")
        .to_owned();
    let subtask_id = success_data(&run_db(
        &db,
        &[
            "subtask",
            "create",
            "--session-token",
            &orch,
            "--meta-task-id",
            &meta_task_id,
            "--title",
            "seed queue row",
            "--kind",
            "work",
            "--priority",
            "1",
            "--subtask-id",
            "queue_seed_work",
        ],
    ))["subtask_id"]
        .as_str()
        .expect("subtask id")
        .to_owned();
    seed_ready_queue_item(&db, &orch, &subtask_id, "queue_seed_1");

    let claim = success_data(&run_db(
        &db,
        &[
            "queue",
            "mark-in-flight",
            "--session-token",
            &gate,
            "--queue-id",
            "queue_seed_1",
            "--lease-duration-ms",
            "30000",
        ],
    ));
    assert_eq!(claim["queue_id"], "queue_seed_1");
    assert_eq!(claim["subtask_id"], subtask_id);
    assert!(claim["claim_fence_seq"].as_i64().expect("claim fence") >= 1);

    let superseded = success_data(&run_db(
        &db,
        &[
            "queue",
            "supersede",
            "--session-token",
            &gate,
            "--queue-id",
            "queue_seed_1",
        ],
    ));
    assert_eq!(superseded["operation"], "supersede");
    assert_eq!(superseded["queue_id"], "queue_seed_1");

    let empty_claim = success_data(&run_db(
        &db,
        &[
            "queue",
            "claim-next",
            "--session-token",
            &gate,
            "--lease-duration-ms",
            "30000",
        ],
    ));
    assert_eq!(empty_claim, Value::Null);

    let metrics = success_data(&run_db(&db, &["queue", "metrics"]));
    assert_eq!(metrics["queued_count"], 0);
    assert_eq!(metrics["in_flight_count"], 0);
}

#[test]
fn workflow_commands_emit_json() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");

    let orch = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "orch",
            "--agent-instance-id",
            "orch-1",
            "--role",
            "orchestrator",
        ],
    ))["session_token"]
        .as_str()
        .expect("orch token")
        .to_owned();
    let exec = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "worker",
            "--agent-instance-id",
            "worker-1",
            "--role",
            "executor",
        ],
    ))["session_token"]
        .as_str()
        .expect("exec token")
        .to_owned();
    let reviewer = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "reviewer",
            "--agent-instance-id",
            "reviewer-1",
            "--role",
            "reviewer",
        ],
    ))["session_token"]
        .as_str()
        .expect("reviewer token")
        .to_owned();
    let gate = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "gate",
            "--agent-instance-id",
            "gate-1",
            "--role",
            "apply-gate",
        ],
    ))["session_token"]
        .as_str()
        .expect("gate token")
        .to_owned();
    attest_session(&db, &exec, "workflow-worker");
    attest_session(&db, &reviewer, "workflow-reviewer");
    attest_session(&db, &gate, "workflow-gate");

    let meta_task_id = success_data(&run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "ship cli",
        ],
    ))["meta_task_id"]
        .as_str()
        .expect("meta id")
        .to_owned();

    let subtask_id = success_data(&run_db(
        &db,
        &[
            "subtask",
            "create",
            "--session-token",
            &orch,
            "--meta-task-id",
            &meta_task_id,
            "--title",
            "implement cli",
            "--kind",
            "work",
            "--priority",
            "1",
            "--subtask-id",
            "work_1",
        ],
    ))["subtask_id"]
        .as_str()
        .expect("subtask id")
        .to_owned();

    let claim = success_data(&run_db(
        &db,
        &[
            "subtask",
            "claim-next",
            "--session-token",
            &exec,
            "--lease-duration-ms",
            "30000",
        ],
    ));
    let claim_id = claim["claim_id"].as_str().expect("claim id").to_owned();
    let fence_seq = claim["fence_seq"].as_i64().expect("fence").to_string();

    success_data(&run_db(
        &db,
        &[
            "subtask",
            "start",
            "--session-token",
            &exec,
            "--claim-id",
            &claim_id,
            "--fence-seq",
            &fence_seq,
        ],
    ));

    success_data(&run_db(
        &db,
        &[
            "artifact",
            "publish",
            "--session-token",
            &exec,
            "--claim-id",
            &claim_id,
            "--fence-seq",
            &fence_seq,
            "--artifact-digest",
            "blake3:a",
            "--artifact-kind",
            "patch-bundle",
            "--base-rev",
            "base",
            "--manifest-path",
            "a.json",
            "--changed-paths-digest",
            "blake3:paths_a",
        ],
    ));

    let review_id = success_data(&run_db(
        &db,
        &[
            "review",
            "request",
            "--session-token",
            &exec,
            "--subtask-id",
            &subtask_id,
            "--artifact-digest",
            "blake3:a",
            "--review-subtask-id",
            "review_1",
        ],
    ))["review_id"]
        .as_str()
        .expect("review id")
        .to_owned();

    success_data(&run_db(
        &db,
        &[
            "claim",
            "release",
            "--session-token",
            &exec,
            "--claim-id",
            &claim_id,
            "--fence-seq",
            &fence_seq,
        ],
    ));

    let review_claim = success_data(&run_db(
        &db,
        &[
            "subtask",
            "claim-next",
            "--session-token",
            &reviewer,
            "--lease-duration-ms",
            "30000",
        ],
    ));
    let review_claim_id = review_claim["claim_id"]
        .as_str()
        .expect("review claim id")
        .to_owned();
    let review_fence = review_claim["fence_seq"]
        .as_i64()
        .expect("review fence")
        .to_string();

    success_data(&run_db(
        &db,
        &[
            "subtask",
            "start",
            "--session-token",
            &reviewer,
            "--claim-id",
            &review_claim_id,
            "--fence-seq",
            &review_fence,
        ],
    ));

    success_data(&run_db(
        &db,
        &[
            "review",
            "decide",
            "--session-token",
            &reviewer,
            "--review-id",
            &review_id,
            "--claim-id",
            &review_claim_id,
            "--fence-seq",
            &review_fence,
            "--verdict",
            "approve",
            "--findings-digest",
            "blake3:findings",
        ],
    ));

    let queue_id = success_data(&run_db(
        &db,
        &[
            "queue",
            "enqueue",
            "--session-token",
            &orch,
            "--artifact-digest",
            "blake3:a",
            "--subtask-id",
            &subtask_id,
        ],
    ))["queue_id"]
        .as_str()
        .expect("queue id")
        .to_owned();

    let queue_data = success_data(&run_db(&db, &["queue", "list", "--limit", "10"]));
    assert_eq!(queue_data.as_array().expect("queue list").len(), 1);

    let queue_claim = success_data(&run_db(
        &db,
        &[
            "queue",
            "claim-next",
            "--session-token",
            &gate,
            "--lease-duration-ms",
            "30000",
        ],
    ));
    assert_eq!(queue_claim["queue_id"], queue_id);

    let events = success_data(&run_db(
        &db,
        &[
            "events",
            "list",
            "--after-seq",
            "0",
            "--limit",
            "50",
            "--typed",
        ],
    ));
    assert!(events.as_array().expect("events").len() >= 5);

    let status = success_data(&run_db(
        &db,
        &["subtask", "status", "--subtask-id", &subtask_id],
    ));
    assert_eq!(status["subtask"]["state"], "ready_for_apply");

    let queue_fence = queue_claim["claim_fence_seq"]
        .as_i64()
        .expect("queue claim fence")
        .to_string();
    success_data(&run_db(
        &db,
        &[
            "queue",
            "record-apply-verification",
            "--session-token",
            &gate,
            "--queue-id",
            &queue_id,
            "--artifact-digest",
            "blake3:a",
            "--review-id",
            &review_id,
            "--findings-digest",
            "blake3:findings",
            "--claim-fence-seq",
            &queue_fence,
            "--verifier",
            "mutai-rs",
            "--verdict-digest",
            "blake3:verdict",
            "--seal-digest",
            "blake3:seal",
        ],
    ));
    success_data(&run_db(
        &db,
        &[
            "queue",
            "mark-applied",
            "--session-token",
            &gate,
            "--queue-id",
            &queue_id,
            "--claim-fence-seq",
            &queue_fence,
        ],
    ));
    let metrics = success_data(&run_db(&db, &["queue", "metrics"]));
    assert_eq!(metrics["queued_count"], 0);
}

#[test]
fn blocked_review_decision_records_evidence_without_reclaiming_same_work() {
    let tmp = TempDir::new().expect("temp dir");
    let db = tmp.path().join("covey.sqlite");
    let orch = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "blocked-orch",
            "--agent-instance-id",
            "blocked-orch-1",
            "--role",
            "orchestrator",
        ],
    ))["session_token"]
        .as_str()
        .expect("orch token")
        .to_owned();
    let exec = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "blocked-worker",
            "--agent-instance-id",
            "blocked-worker-1",
            "--role",
            "executor",
        ],
    ))["session_token"]
        .as_str()
        .expect("exec token")
        .to_owned();
    let reviewer = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "blocked-reviewer",
            "--agent-instance-id",
            "blocked-reviewer-1",
            "--role",
            "reviewer",
        ],
    ))["session_token"]
        .as_str()
        .expect("reviewer token")
        .to_owned();
    attest_session(&db, &exec, "blocked-worker");
    attest_session(&db, &reviewer, "blocked-reviewer");

    let meta_task_id = success_data(&run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "blocked review workflow",
        ],
    ))["meta_task_id"]
        .as_str()
        .expect("meta id")
        .to_owned();

    for (subtask_id, title, priority) in [
        ("work_blocked", "first blocked artifact", "1"),
        ("work_next", "next independent task", "2"),
    ] {
        success_data(&run_db(
            &db,
            &[
                "subtask",
                "create",
                "--session-token",
                &orch,
                "--meta-task-id",
                &meta_task_id,
                "--title",
                title,
                "--kind",
                "work",
                "--priority",
                priority,
                "--subtask-id",
                subtask_id,
            ],
        ));
    }

    let claim = success_data(&run_db(
        &db,
        &[
            "subtask",
            "claim",
            "--session-token",
            &exec,
            "--subtask-id",
            "work_blocked",
            "--lease-duration-ms",
            "30000",
        ],
    ));
    let claim_id = claim["claim_id"].as_str().expect("claim id").to_owned();
    let fence_seq = claim["fence_seq"].as_i64().expect("fence").to_string();
    success_data(&run_db(
        &db,
        &[
            "subtask",
            "start",
            "--session-token",
            &exec,
            "--claim-id",
            &claim_id,
            "--fence-seq",
            &fence_seq,
        ],
    ));
    success_data(&run_db(
        &db,
        &[
            "artifact",
            "publish",
            "--session-token",
            &exec,
            "--claim-id",
            &claim_id,
            "--fence-seq",
            &fence_seq,
            "--artifact-digest",
            "blake3:blocking-artifact",
            "--artifact-kind",
            "patch-bundle",
            "--base-rev",
            "base",
            "--manifest-path",
            "blocked.json",
            "--changed-paths-digest",
            "blake3:blocking-paths",
        ],
    ));
    let review_id = success_data(&run_db(
        &db,
        &[
            "review",
            "request",
            "--session-token",
            &exec,
            "--subtask-id",
            "work_blocked",
            "--artifact-digest",
            "blake3:blocking-artifact",
            "--review-subtask-id",
            "review_blocking_artifact",
        ],
    ))["review_id"]
        .as_str()
        .expect("review id")
        .to_owned();
    success_data(&run_db(
        &db,
        &[
            "claim",
            "release",
            "--session-token",
            &exec,
            "--claim-id",
            &claim_id,
            "--fence-seq",
            &fence_seq,
        ],
    ));

    let review_claim = success_data(&run_db(
        &db,
        &[
            "subtask",
            "claim",
            "--session-token",
            &reviewer,
            "--subtask-id",
            "review_blocking_artifact",
            "--lease-duration-ms",
            "30000",
        ],
    ));
    let review_claim_id = review_claim["claim_id"]
        .as_str()
        .expect("review claim id")
        .to_owned();
    let review_fence = review_claim["fence_seq"]
        .as_i64()
        .expect("review fence")
        .to_string();
    success_data(&run_db(
        &db,
        &[
            "subtask",
            "start",
            "--session-token",
            &reviewer,
            "--claim-id",
            &review_claim_id,
            "--fence-seq",
            &review_fence,
        ],
    ));
    let decision = success_data(&run_db(
        &db,
        &[
            "review",
            "decide",
            "--session-token",
            &reviewer,
            "--review-id",
            &review_id,
            "--claim-id",
            &review_claim_id,
            "--fence-seq",
            &review_fence,
            "--verdict",
            "blocked",
            "--findings-digest",
            "blake3:unblock-instructions",
        ],
    ));
    let followup_id = decision["followup_subtask_id"]
        .as_str()
        .expect("blocked review returns follow-up subtask")
        .to_owned();
    assert_eq!(decision["review_id"], review_id);
    assert_eq!(decision["verdict"], "blocked");

    let blocked_status = success_data(&run_db(
        &db,
        &["subtask", "status", "--subtask-id", "work_blocked"],
    ));
    assert_eq!(blocked_status["subtask"]["state"], "blocked");
    assert_eq!(
        blocked_status["subtask"]["artifact_digest"],
        "blake3:blocking-artifact"
    );
    assert_eq!(blocked_status["reviews"][0]["verdict"], "blocked");
    assert_eq!(
        blocked_status["reviews"][0]["findings_digest"],
        "blake3:unblock-instructions"
    );

    let followup = success_data(&run_db(
        &db,
        &["subtask", "status", "--subtask-id", &followup_id],
    ));
    assert_eq!(followup["subtask"]["state"], "available");
    assert_eq!(followup["subtask"]["artifact_digest"], Value::Null);
    assert_eq!(followup["subtask"]["priority"], 1);
    let conn = Connection::open(&db).expect("open covey db");
    let linked: (String, String, String) = conn
        .query_row(
            "SELECT source_subtask_id, source_artifact_digest, findings_digest FROM review_followup_subtasks WHERE review_id = ?1 AND followup_subtask_id = ?2",
            params![review_id.as_str(), followup_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("review follow-up link");
    assert_eq!(linked.0, "work_blocked");
    assert_eq!(linked.1, "blake3:blocking-artifact");
    assert_eq!(linked.2, "blake3:unblock-instructions");

    let next_claim = success_data(&run_db(
        &db,
        &[
            "subtask",
            "claim-next",
            "--session-token",
            &exec,
            "--lease-duration-ms",
            "30000",
        ],
    ));
    assert_eq!(next_claim["subtask_id"], followup_id);
}

#[test]
fn repoops_authority_snapshot_returns_claim_scope_and_lock_facts() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");

    let orch = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "orch-repoops",
            "--agent-instance-id",
            "orch-repoops-1",
            "--role",
            "orchestrator",
        ],
    ))["session_token"]
        .as_str()
        .expect("orch token")
        .to_owned();
    let exec = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "worker-repoops",
            "--agent-instance-id",
            "worker-repoops-1",
            "--role",
            "executor",
        ],
    ))["session_token"]
        .as_str()
        .expect("exec token")
        .to_owned();
    let meta_task_id = success_data(&run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "repoops snapshot",
        ],
    ))["meta_task_id"]
        .as_str()
        .expect("meta id")
        .to_owned();
    let subtask_id = success_data(&run_db(
        &db,
        &[
            "subtask",
            "create",
            "--session-token",
            &orch,
            "--meta-task-id",
            &meta_task_id,
            "--title",
            "repoops work",
            "--kind",
            "work",
            "--priority",
            "1",
            "--subtask-id",
            "repoops_work_1",
        ],
    ))["subtask_id"]
        .as_str()
        .expect("subtask id")
        .to_owned();
    let claim = success_data(&run_db(
        &db,
        &[
            "subtask",
            "claim-next",
            "--session-token",
            &exec,
            "--lease-duration-ms",
            "30000",
        ],
    ));
    let claim_id = claim["claim_id"].as_str().expect("claim id").to_owned();
    let fence_seq = claim["fence_seq"].as_i64().expect("fence").to_string();
    success_data(&run_db(
        &db,
        &[
            "subtask",
            "start",
            "--session-token",
            &exec,
            "--claim-id",
            &claim_id,
            "--fence-seq",
            &fence_seq,
        ],
    ));
    success_data(&run_db(
        &db,
        &[
            "reservation",
            "request",
            "--session-token",
            &orch,
            "--owner-subtask-id",
            &subtask_id,
            "--scope-class",
            "subtree",
            "--scope-key",
            "src",
            "--lease-duration-ms",
            "30000",
        ],
    ));

    let snapshot = success_data(&run_db(
        &db,
        &[
            "repoops",
            "authority-snapshot",
            "--session-token",
            &exec,
            "--claim-id",
            &claim_id,
            "--fence-seq",
            &fence_seq,
            "--paths",
            "src/lib.rs",
        ],
    ));
    assert_eq!(
        snapshot["schema_version"],
        "covey_repoops_authority_snapshot.v1"
    );
    assert_eq!(snapshot["agent_id"], "worker-repoops");
    assert_eq!(snapshot["claim"]["claim_id"], claim_id);
    assert_eq!(snapshot["claim"]["status"], "in_progress");
    assert_eq!(snapshot["scope"]["in"], serde_json::json!(["src/**"]));
    assert_eq!(snapshot["claim"]["scope_in"], serde_json::json!(["src/**"]));
    let ownership_token = snapshot["ownership_token"]
        .as_str()
        .expect("ownership token ref");
    assert!(ownership_token.starts_with("covey-session-token-blake3:"));
    assert_eq!(
        snapshot["claim"]["active_ownership_token"],
        snapshot["ownership_token"]
    );
    assert_eq!(snapshot["git_context"]["ownership_token_required"], true);
    assert_eq!(snapshot["locks"][0]["path"], "src/lib.rs");
    assert_eq!(snapshot["locks"][0]["owner"], "worker-repoops");
    assert_eq!(snapshot["locks"][0]["claim_id"], claim_id);
    assert_eq!(snapshot["locks"][0]["status"], "owned");
    let fact_sources = snapshot["fact_sources"].as_array().expect("fact sources");
    assert!(fact_sources.len() >= 5, "fact_sources={fact_sources:?}");
    let fact_source_values = fact_sources
        .iter()
        .map(|source| source.as_str().expect("fact source string"))
        .collect::<Vec<_>>();
    assert!(
        fact_source_values
            .iter()
            .any(|source| *source == format!("session_token_ref:{ownership_token}"))
    );
    assert!(fact_source_values.contains(&"claims.owner_session_token:token_ref"));
    assert!(
        fact_source_values
            .iter()
            .all(|source| !source.contains(&exec)),
        "fact_sources leaked raw session token: {fact_source_values:?}"
    );
}

#[test]
fn repoops_authority_snapshot_filters_irrelevant_reservations_without_losing_locks() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");

    let orch = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "orch-repoops-filter",
            "--agent-instance-id",
            "orch-repoops-filter-1",
            "--role",
            "orchestrator",
        ],
    ))["session_token"]
        .as_str()
        .expect("orch token")
        .to_owned();
    let exec = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "worker-repoops-filter",
            "--agent-instance-id",
            "worker-repoops-filter-1",
            "--role",
            "executor",
        ],
    ))["session_token"]
        .as_str()
        .expect("exec token")
        .to_owned();
    let meta_task_id = success_data(&run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "repoops filtered snapshot",
        ],
    ))["meta_task_id"]
        .as_str()
        .expect("meta id")
        .to_owned();

    for (subtask_id, title) in [
        ("repoops_filter_work", "repoops filtered work"),
        ("repoops_filter_foreign", "repoops filtered foreign"),
        ("repoops_filter_irrelevant", "repoops filtered irrelevant"),
    ] {
        success_data(&run_db(
            &db,
            &[
                "subtask",
                "create",
                "--session-token",
                &orch,
                "--meta-task-id",
                &meta_task_id,
                "--title",
                title,
                "--kind",
                "work",
                "--priority",
                "1",
                "--subtask-id",
                subtask_id,
            ],
        ));
    }

    let claim = success_data(&run_db(
        &db,
        &[
            "subtask",
            "claim",
            "--session-token",
            &exec,
            "--subtask-id",
            "repoops_filter_work",
            "--lease-duration-ms",
            "30000",
        ],
    ));
    let claim_id = claim["claim_id"].as_str().expect("claim id").to_owned();
    let fence_seq = claim["fence_seq"].as_i64().expect("fence").to_string();
    success_data(&run_db(
        &db,
        &[
            "subtask",
            "start",
            "--session-token",
            &exec,
            "--claim-id",
            &claim_id,
            "--fence-seq",
            &fence_seq,
        ],
    ));

    for args in [
        vec![
            "--owner-subtask-id",
            "repoops_filter_work",
            "--scope-class",
            "subtree",
            "--scope-key",
            "src",
        ],
        vec![
            "--owner-subtask-id",
            "repoops_filter_foreign",
            "--scope-class",
            "exact-path",
            "--scope-key",
            "src/lib.rs",
        ],
        vec![
            "--owner-subtask-id",
            "repoops_filter_foreign",
            "--scope-class",
            "generated-set",
            "--scope-key",
            "generated",
            "--member",
            "generated/out.rs",
        ],
        vec![
            "--owner-subtask-id",
            "repoops_filter_irrelevant",
            "--scope-class",
            "exact-path",
            "--scope-key",
            "docs/readme.md",
        ],
    ] {
        let mut command = vec!["reservation", "request", "--session-token", &orch];
        command.extend(args);
        command.extend(["--lease-duration-ms", "30000"]);
        success_data(&run_db(&db, &command));
    }

    let snapshot = success_data(&run_db(
        &db,
        &[
            "repoops",
            "authority-snapshot",
            "--session-token",
            &exec,
            "--claim-id",
            &claim_id,
            "--fence-seq",
            &fence_seq,
            "--paths",
            "src/lib.rs",
            "generated/out.rs",
        ],
    ));

    assert_eq!(snapshot["scope"]["in"], serde_json::json!(["src/**"]));
    let locks = snapshot["locks"].as_array().expect("locks");
    assert!(locks.iter().any(|lock| {
        lock["path"] == "src/lib.rs"
            && lock["owner"] == "worker-repoops-filter"
            && lock["status"] == "owned"
    }));
    assert!(locks.iter().any(|lock| {
        lock["path"] == "src/lib.rs"
            && lock["owner"] == "subtask:repoops_filter_foreign"
            && lock["status"] == "foreign_owner"
    }));
    assert!(locks.iter().any(|lock| {
        lock["path"] == "generated/out.rs"
            && lock["owner"] == "subtask:repoops_filter_foreign"
            && lock["status"] == "foreign_owner"
    }));
    assert!(
        locks.iter().all(|lock| lock["path"] != "docs/readme.md"),
        "irrelevant reservation leaked into lock facts: {locks:?}"
    );
}

#[test]
fn import_rejects_ambiguous_destination_flags() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let covey = Covey::open(&db).expect("open covey");
    let orch = register_orchestrator(&covey, "orch-import");
    let existing_meta = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "existing destination",
                "submit-existing-meta",
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");

    let both = covey.import_bd_v1_work_subtask(
        &orch,
        Some(&existing_meta),
        Some("new destination"),
        "BD-1",
        "import work item",
        1,
        "import-both-destination-flags",
    );
    assert!(matches!(
        both,
        Err(CoveyError::InvalidPath { path }) if path == "bd import destination selector"
    ));

    let neither = covey.import_bd_v1_work_subtask(
        &orch,
        None,
        None,
        "BD-2",
        "import work item",
        1,
        "import-no-destination-flags",
    );
    assert!(matches!(
        neither,
        Err(CoveyError::InvalidPath { path }) if path == "bd import destination selector"
    ));

    let terminal_meta = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "terminal destination",
                "submit-terminal-meta",
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit terminal meta");
    covey
        .cancel_meta_task(
            covey::CancelMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                terminal_meta.clone(),
                "cancel-terminal-meta",
            )
            .expect("valid cancel-meta-task request"),
        )
        .expect("cancel terminal meta");

    let terminal = covey.import_bd_v1_work_subtask(
        &orch,
        Some(&terminal_meta),
        None,
        "BD-3",
        "import work item",
        1,
        "import-terminal-meta",
    );
    assert!(matches!(
        terminal,
        Err(CoveyError::MetaTaskUnavailable { meta_task_id, state })
            if meta_task_id == terminal_meta && state == MetaTaskState::Cancelled
    ));
}

#[test]
fn import_reports_invalid_source_schema() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let covey = Covey::open(&db).expect("open covey");
    let orch = register_orchestrator(&covey, "orch-import-schema");

    let missing = covey.import_bd_v1(ImportBdV1Req::new_meta_task(
        orch.clone(),
        "/nonexistent/beads.db".to_owned(),
        "new meta".to_owned(),
        "import-missing-db".to_owned(),
    ));
    assert!(matches!(
        missing,
        Err(CoveyError::ImportSourceNotFound { path }) if path == "/nonexistent/beads.db"
    ));

    let bad_db = tmp.path().join("bad_beads.db");
    {
        let conn = rusqlite::Connection::open(&bad_db).expect("open bad db");
        conn.execute("CREATE TABLE other (id INTEGER PRIMARY KEY)", [])
            .expect("create other table");
    }
    let invalid_schema = covey.import_bd_v1(ImportBdV1Req::new_meta_task(
        orch.clone(),
        bad_db.to_string_lossy().to_string(),
        "new meta".to_owned(),
        "import-invalid-schema".to_owned(),
    ));
    assert!(matches!(
        invalid_schema,
        Err(CoveyError::InvalidSourceSchema { path: _, detail }) if detail == "missing issues table"
    ));
}

fn seed_beads_db(db_path: &Path) {
    let conn = Connection::open(db_path).expect("open beads db");
    conn.execute(
        "CREATE TABLE issues (id TEXT PRIMARY KEY, title TEXT NOT NULL, description TEXT, acceptance_criteria TEXT, status TEXT NOT NULL, priority INTEGER NOT NULL, issue_type TEXT NOT NULL, owner TEXT, assignee TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, closed_at TEXT, close_reason TEXT, deleted_at TEXT)",
        [],
    )
    .expect("create issues table");
    conn.execute(
        "CREATE TABLE dependencies (issue_id TEXT NOT NULL, depends_on_id TEXT NOT NULL, type TEXT NOT NULL, created_at TEXT NOT NULL)",
        [],
    )
    .expect("create dependencies table");
    conn.execute(
        "CREATE TABLE labels (issue_id TEXT NOT NULL, label TEXT NOT NULL)",
        [],
    )
    .expect("create labels table");

    let rows = [
        ("BD-1", "first imported work item", "open", 1_i64, "task"),
        ("BD-2", "second imported work item", "open", 2_i64, "task"),
        ("BD-3", "closed work item", "closed", 3_i64, "task"),
        ("BD-4", "epic container", "open", 4_i64, "epic"),
        ("BD-5", "feature work item", "open", 5_i64, "feature"),
        ("BD-6", "review labeled feature", "open", 6_i64, "feature"),
        ("BD-7", "unsupported bug", "open", 7_i64, "bug"),
    ];
    for (id, title, status, priority, issue_type) in rows {
        conn.execute(
            "INSERT INTO issues (id, title, description, acceptance_criteria, status, priority, issue_type, owner, assignee, created_at, updated_at, closed_at, close_reason, deleted_at) VALUES (?1, ?2, '', '', ?3, ?4, ?5, '', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL, '', NULL)",
            params![id, title, status, priority, issue_type],
        )
        .expect("insert issue");
    }
    conn.execute(
        "INSERT INTO dependencies (issue_id, depends_on_id, type, created_at) VALUES (?1, ?2, 'blocks', '2026-01-01T00:00:00Z')",
        params!["BD-2", "BD-1"],
    )
    .expect("insert dependency");
    conn.execute(
        "INSERT INTO labels (issue_id, label) VALUES (?1, ?2)",
        params!["BD-5", "allowed"],
    )
    .expect("insert label");
    conn.execute(
        "INSERT INTO labels (issue_id, label) VALUES (?1, ?2)",
        params!["BD-6", "review"],
    )
    .expect("insert label");
}

fn seed_empty_beads_db(db_path: &Path) {
    let conn = Connection::open(db_path).expect("open beads db");
    conn.execute(
        "CREATE TABLE issues (id TEXT PRIMARY KEY, title TEXT NOT NULL, description TEXT, acceptance_criteria TEXT, status TEXT NOT NULL, priority INTEGER NOT NULL, issue_type TEXT NOT NULL, owner TEXT, assignee TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, closed_at TEXT, close_reason TEXT, deleted_at TEXT)",
        [],
    )
    .expect("create issues table");
    conn.execute(
        "CREATE TABLE dependencies (issue_id TEXT NOT NULL, depends_on_id TEXT NOT NULL, type TEXT NOT NULL, created_at TEXT NOT NULL)",
        [],
    )
    .expect("create dependencies table");
    conn.execute(
        "CREATE TABLE labels (issue_id TEXT NOT NULL, label TEXT NOT NULL)",
        [],
    )
    .expect("create labels table");
}

fn seed_openspec_change(root: &Path, change_id: &str, task_lines: &[&str]) {
    let change_dir = root.join("openspec").join("changes").join(change_id);
    fs::create_dir_all(change_dir.join("specs").join("covey-openspec-import"))
        .expect("create OpenSpec dirs");
    fs::write(change_dir.join(".openspec.yaml"), "schema: better-droid\n").expect("openspec yaml");
    fs::write(change_dir.join("proposal.md"), "## Why\n\nTest proposal.\n").expect("proposal");
    fs::write(change_dir.join("design.md"), "## Context\n\nTest design.\n").expect("design");
    let tasks = task_lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let (task_id, title) = parse_seed_task_line(line);
            let scenario_id = format!("SCN-TEST-{:03}", index + 1);
            format!(
                r#"- [ ] {task_id} {title}
  - **Type:** implementation
  - **Purpose:** Exercise compiled import behavior for REQ-TEST and {scenario_id}.
  - **Dependencies:** none
  - **Allowed Read Paths:** `openspec/changes/**`
  - **Allowed Write Paths:** `covey/src/ops/import/openspec/source.rs`
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Acceptance Criteria:** {title} imports as a compiled task packet.
  - **Validation / Evidence:**
    - **Command / Action:** `cargo test import_openspec --test cli_integration`
    - **Expected Exit Code / Observation:** exits 0
    - **Required Evidence:** stdout, exit code, and changed-file list outside openspec/**
  - **Traceability Refs:** REQ-TEST, {scenario_id}, VAL-TEST-001
  - **Stale If:** source changes
"#
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        change_dir.join("tasks.md"),
        format!("## 1. Work\n\n{tasks}\n"),
    )
    .expect("tasks");
    let scenarios = task_lines
        .iter()
        .enumerate()
        .map(|(index, _)| {
            format!(
                "#### Scenario: SCN-TEST-{number:03} Test scenario {number}\n- **WHEN** imported\n- **THEN** it passes\n",
                number = index + 1
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        change_dir
            .join("specs")
            .join("covey-openspec-import")
            .join("spec.md"),
        format!(
            "## ADDED Requirements\n\n### Requirement: REQ-TEST Test import\nThe system SHALL test compiled import.\n\n{scenarios}"
        ),
    )
    .expect("spec");
    let report = compile_change(&CompileOptions {
        project_root: root.to_path_buf(),
        change_id: change_id.to_owned(),
        output_dir: None,
    })
    .expect("compile OpenSpec fixture");
    assert!(
        report.import_ready,
        "seeded OpenSpec fixture must compile as Covey-import-ready: {:?}",
        report.blockers
    );
}

fn parse_seed_task_line(line: &str) -> (&str, &str) {
    let rest = line
        .trim()
        .strip_prefix("- [ ] ")
        .or_else(|| line.trim().strip_prefix("- [x] "))
        .or_else(|| line.trim().strip_prefix("- [X] "))
        .expect("seed task checkbox");
    rest.split_once(' ').expect("seed task id and title")
}

fn event_count(db_path: &Path) -> i64 {
    let conn = Connection::open(db_path).expect("open db");
    conn.query_row("SELECT COUNT(*) FROM event_log", [], |row| row.get(0))
        .expect("event count")
}

fn scalar_count(db_path: &Path, sql: &str) -> i64 {
    let conn = Connection::open(db_path).expect("open db");
    conn.query_row(sql, [], |row| row.get(0))
        .expect("scalar count")
}

#[test]
fn import_openspec_dry_run_reports_plan_without_events() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    seed_openspec_change(
        tmp.path(),
        "openspec-covey-importer",
        &[
            "- [ ] 1.1 Add OpenSpec importer CLI command",
            "- [ ] 1.2 Parse tasks.md into stable task records",
        ],
    );

    let result = success_data(&run_db(
        &db,
        &[
            "import",
            "openspec",
            "--change",
            "openspec-covey-importer",
            "--project-root",
            tmp.path().to_str().expect("project root"),
            "--dry-run",
            "--json",
        ],
    ));

    assert_eq!(result["operation"], "import_openspec");
    assert_eq!(result["change_id"], "openspec-covey-importer");
    assert_eq!(result["meta_task_id"], "openspec:openspec-covey-importer");
    assert_eq!(result["status"], "covey_import_ready");
    assert_eq!(result["readiness"]["planning_ready"], true);
    assert_eq!(result["readiness"]["covey_import_ready"], true);
    assert_eq!(result["readiness"]["covey_imported"], false);
    assert_eq!(result["readiness"]["implementation_ready"], false);
    assert_eq!(result["readiness"]["execution_ready"], false);
    assert_eq!(result["readiness"]["review_approved"], false);
    assert_eq!(result["readiness"]["apply_queued"], false);
    assert_eq!(result["readiness"]["apply_authorized"], false);
    assert_eq!(result["readiness"]["landed"], false);
    assert_eq!(result["readiness"]["shipped_verified"], false);
    assert_eq!(result["readiness"]["not_imported"], true);
    assert_eq!(result["product_impact"]["product_files_changed"], false);
    assert_eq!(result["product_impact"]["product_tests_run"], false);
    assert_eq!(result["product_impact"]["covey_imported"], false);
    assert_eq!(result["product_impact"]["apply_receipt"], false);
    assert_eq!(result["product_impact"]["shipped_evidence"], false);
    assert_eq!(result["dry_run"], true);
    assert_eq!(result["created"], 3);
    assert_eq!(result["updated"], 0);
    assert_eq!(result["unchanged"], 0);
    assert_eq!(result["conflicts"].as_array().expect("conflicts").len(), 0);
    assert_eq!(result["items"].as_array().expect("items").len(), 3);
    assert_eq!(event_count(&db), 0);
    assert_eq!(scalar_count(&db, "SELECT COUNT(*) FROM meta_tasks"), 0);
    assert_eq!(scalar_count(&db, "SELECT COUNT(*) FROM subtasks"), 0);
}

#[test]
fn import_openspec_write_mode_requires_orchestrator_session() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    seed_openspec_change(
        tmp.path(),
        "openspec-covey-importer",
        &["- [ ] 1.1 Add OpenSpec importer CLI command"],
    );

    let missing_session = run_db(
        &db,
        &[
            "import",
            "openspec",
            "--change",
            "openspec-covey-importer",
            "--project-root",
            tmp.path().to_str().expect("project root"),
            "--json",
        ],
    );
    assert_eq!(missing_session.status.code(), Some(2));
    assert_eq!(stderr_code(&missing_session), "invalid_args");

    let executor = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "exec-openspec-import",
            "--agent-instance-id",
            "exec-openspec-import-1",
            "--role",
            "executor",
        ],
    ))["session_token"]
        .as_str()
        .expect("executor token")
        .to_owned();
    let wrong_role = run_db(
        &db,
        &[
            "import",
            "openspec",
            "--change",
            "openspec-covey-importer",
            "--project-root",
            tmp.path().to_str().expect("project root"),
            "--session-token",
            &executor,
            "--json",
        ],
    );
    assert_eq!(wrong_role.status.code(), Some(3));
    assert_eq!(stderr_code(&wrong_role), "permission_denied");
}

#[test]
fn import_openspec_write_mode_is_idempotent_and_preserves_boundaries() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    seed_openspec_change(
        tmp.path(),
        "openspec-covey-importer",
        &[
            "- [ ] 1.1 Add OpenSpec importer CLI command",
            "- [ ] 1.2 Parse tasks.md into stable task records",
        ],
    );
    let covey = Covey::open(&db).expect("open covey");
    let orch = register_orchestrator(&covey, "orch-openspec-import");

    let first = success_data(&run_db(
        &db,
        &[
            "import",
            "openspec",
            "--change",
            "openspec-covey-importer",
            "--project-root",
            tmp.path().to_str().expect("project root"),
            "--session-token",
            &orch,
            "--json",
        ],
    ));
    assert_eq!(first["status"], "covey_imported");
    assert_eq!(first["readiness"]["covey_imported"], true);
    assert_eq!(first["readiness"]["implementation_ready"], false);
    assert_eq!(first["readiness"]["execution_ready"], true);
    assert_eq!(first["readiness"]["review_approved"], false);
    assert_eq!(first["readiness"]["apply_queued"], false);
    assert_eq!(first["readiness"]["apply_authorized"], false);
    assert_eq!(first["readiness"]["landed"], false);
    assert_eq!(first["readiness"]["shipped_verified"], false);
    assert_eq!(first["product_impact"]["product_files_changed"], false);
    assert_eq!(first["product_impact"]["product_tests_run"], false);
    assert_eq!(first["product_impact"]["covey_imported"], true);
    assert_eq!(first["product_impact"]["apply_receipt"], false);
    assert_eq!(first["product_impact"]["shipped_evidence"], false);
    assert_eq!(first["created"], 3);
    assert_eq!(first["updated"], 0);
    assert_eq!(first["unchanged"], 0);

    let second = success_data(&run_db(
        &db,
        &[
            "import",
            "openspec",
            "--change",
            "openspec-covey-importer",
            "--project-root",
            tmp.path().to_str().expect("project root"),
            "--session-token",
            &orch,
            "--json",
        ],
    ));
    assert_eq!(second["created"], 0);
    assert_eq!(second["updated"], 0);
    assert_eq!(second["unchanged"], 3);
    assert_eq!(
        scalar_count(
            &db,
            "SELECT COUNT(*) FROM subtasks WHERE meta_task_id = 'openspec:openspec-covey-importer'"
        ),
        2
    );
    assert_eq!(
        scalar_count(
            &db,
            "SELECT COUNT(*) FROM subtasks WHERE current_claim_id IS NOT NULL"
        ),
        0
    );
    assert_eq!(scalar_count(&db, "SELECT COUNT(*) FROM ready_queue"), 0);
    assert_eq!(
        scalar_count(
            &db,
            "SELECT COUNT(*) FROM import_provenance WHERE planning_format = 'openspec'"
        ),
        3
    );
}

#[test]
fn import_openspec_updates_unclaimed_task_and_conflicts_on_active_claim_change() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    seed_openspec_change(
        tmp.path(),
        "openspec-covey-importer",
        &[
            "- [ ] 1.1 Add OpenSpec importer CLI command",
            "- [ ] 1.2 Parse tasks.md into stable task records",
        ],
    );
    let covey = Covey::open(&db).expect("open covey");
    let orch = register_orchestrator(&covey, "orch-openspec-update");
    let worker = covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                "worker-openspec-update",
                "worker-openspec-update-1",
                SessionRole::Executor,
                "register-worker-openspec-update",
            )
            .expect("valid session registration request"),
        )
        .expect("register worker")
        .session_token;

    success_data(&run_db(
        &db,
        &[
            "import",
            "openspec",
            "--change",
            "openspec-covey-importer",
            "--project-root",
            tmp.path().to_str().expect("project root"),
            "--session-token",
            &orch,
            "--json",
        ],
    ));

    seed_openspec_change(
        tmp.path(),
        "openspec-covey-importer",
        &[
            "- [ ] 1.1 Add OpenSpec importer CLI command with dry-run",
            "- [ ] 1.2 Parse tasks.md into stable task records",
        ],
    );
    let update = success_data(&run_db(
        &db,
        &[
            "import",
            "openspec",
            "--change",
            "openspec-covey-importer",
            "--project-root",
            tmp.path().to_str().expect("project root"),
            "--session-token",
            &orch,
            "--json",
        ],
    ));
    assert!(update["updated"].as_u64().expect("updated") >= 1);
    let status = success_data(&run_db(
        &db,
        &[
            "subtask",
            "status",
            "--subtask-id",
            "openspec:openspec-covey-importer:1.1",
        ],
    ));
    assert_eq!(
        status["subtask"]["title"],
        "Add OpenSpec importer CLI command with dry-run"
    );

    let _claim = covey
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                worker,
                covey::SubtaskId::parse("openspec:openspec-covey-importer:1.1".to_owned())
                    .expect("valid subtask id"),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                "claim-openspec-imported-task".to_owned(),
            )
            .expect("valid claim-subtask request"),
        )
        .expect("claim imported subtask");
    seed_openspec_change(
        tmp.path(),
        "openspec-covey-importer",
        &[
            "- [ ] 1.1 Add OpenSpec importer CLI command with write-mode",
            "- [ ] 1.2 Parse tasks.md into stable task records",
        ],
    );
    let conflict = success_data(&run_db(
        &db,
        &[
            "import",
            "openspec",
            "--change",
            "openspec-covey-importer",
            "--project-root",
            tmp.path().to_str().expect("project root"),
            "--session-token",
            &orch,
            "--json",
        ],
    ));
    let conflicts = conflict["conflicts"].as_array().expect("conflicts");
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0]["reason"], "active_claim_changed_source");
    assert_eq!(
        conflicts[0]["object_id"],
        "openspec:openspec-covey-importer:1.1"
    );
    let status_after_conflict = success_data(&run_db(
        &db,
        &[
            "subtask",
            "status",
            "--subtask-id",
            "openspec:openspec-covey-importer:1.1",
        ],
    ));
    assert_eq!(
        status_after_conflict["subtask"]["title"],
        "Add OpenSpec importer CLI command with dry-run"
    );
}

#[test]
fn import_json_output_contract() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let covey = Covey::open(&db).expect("open covey");
    let orch = register_orchestrator(&covey, "orch-import-json");

    let beads_db = tmp.path().join("beads.db");
    seed_beads_db(&beads_db);

    let json = success_data(&run_db(
        &db,
        &[
            "import",
            "bd",
            "--session-token",
            &orch,
            "--beads-db",
            beads_db.to_str().expect("beads db path"),
            "--prompt-text",
            "json output contract",
        ],
    ));

    assert_eq!(json["operation"], "import_bd");
    assert!(json["meta_task_id"].as_str().is_some());
    assert_eq!(json["imported_count"], 4);
    assert_eq!(json["skipped_count"], 3);
    let items = json["items"].as_array().expect("items array");
    assert_eq!(items.len(), 7);

    let item_1 = items
        .iter()
        .find(|i| i["source_issue_id"] == "BD-1")
        .expect("BD-1 item");
    assert!(item_1["subtask_id"].as_str().is_some());
    assert_eq!(item_1["skip_reason"], Value::Null);

    let item_3 = items
        .iter()
        .find(|i| i["source_issue_id"] == "BD-3")
        .expect("BD-3 item");
    assert_eq!(item_3["subtask_id"], Value::Null);
    let skip_reason = item_3["skip_reason"]
        .as_object()
        .expect("skip_reason object");
    assert_eq!(
        skip_reason["InvalidRow"]["detail"],
        "unsupported status closed"
    );

    let item_4 = items
        .iter()
        .find(|i| i["source_issue_id"] == "BD-4")
        .expect("BD-4 item");
    assert!(item_4["subtask_id"].as_str().is_some());
    assert_eq!(item_4["skip_reason"], Value::Null);

    let item_5 = items
        .iter()
        .find(|i| i["source_issue_id"] == "BD-5")
        .expect("BD-5 item");
    assert!(item_5["subtask_id"].as_str().is_some());
    assert_eq!(item_5["skip_reason"], Value::Null);

    let item_6 = items
        .iter()
        .find(|i| i["source_issue_id"] == "BD-6")
        .expect("BD-6 item");
    let skip_reason_6 = item_6["skip_reason"]
        .as_object()
        .expect("skip_reason object");
    assert_eq!(
        skip_reason_6["InvalidRow"]["detail"],
        "unsupported labeled issue"
    );

    let item_7 = items
        .iter()
        .find(|i| i["source_issue_id"] == "BD-7")
        .expect("BD-7 item");
    let skip_reason_7 = item_7["skip_reason"]
        .as_object()
        .expect("skip_reason object");
    assert_eq!(
        skip_reason_7["InvalidRow"]["detail"],
        "unsupported issue_type bug"
    );
}

#[test]
fn import_command_into_existing_active_meta_task() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let covey = Covey::open(&db).expect("open covey");
    let orch = register_orchestrator(&covey, "orch-import-existing");
    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "existing active destination",
                "submit-existing-active-meta",
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");

    let beads_db = tmp.path().join("existing-beads.db");
    seed_beads_db(&beads_db);

    let result = success_data(&run_db(
        &db,
        &[
            "import",
            "bd",
            "--session-token",
            &orch,
            "--beads-db",
            beads_db.to_str().expect("beads db path"),
            "--meta-task-id",
            &meta_task_id,
        ],
    ));

    assert_eq!(result["operation"], "import_bd");
    assert_eq!(result["meta_task_id"], meta_task_id);
    assert_eq!(result["imported_count"], 4);
    assert_eq!(result["skipped_count"], 3);
    assert_eq!(result["items"].as_array().expect("items").len(), 7,);
}

#[test]
fn import_command_rejects_terminal_meta_task() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let covey = Covey::open(&db).expect("open covey");
    let orch = register_orchestrator(&covey, "orch-import-terminal-cli");
    let terminal_meta = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "terminal destination",
                "submit-terminal-meta-cli",
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");
    covey
        .cancel_meta_task(
            covey::CancelMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                terminal_meta.clone(),
                "cancel-terminal-meta-cli",
            )
            .expect("valid cancel-meta-task request"),
        )
        .expect("cancel terminal meta");

    let beads_db = tmp.path().join("terminal-beads.db");
    seed_beads_db(&beads_db);

    let output = run_db(
        &db,
        &[
            "import",
            "bd",
            "--session-token",
            &orch,
            "--beads-db",
            beads_db.to_str().expect("beads db path"),
            "--meta-task-id",
            &terminal_meta,
        ],
    );

    assert_eq!(output.status.code(), Some(4));
    assert_eq!(stderr_code(&output), "conflict");
    let payload = parse_stderr_json(&output);
    assert!(
        payload["message"]
            .as_str()
            .expect("message")
            .contains(&terminal_meta)
    );
}

#[test]
fn import_command_reports_structured_missing_and_malformed_source_errors() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let covey = Covey::open(&db).expect("open covey");
    let orch = register_orchestrator(&covey, "orch-import-schema-cli");

    let missing = run_db(
        &db,
        &[
            "import",
            "bd",
            "--session-token",
            &orch,
            "--beads-db",
            "/nonexistent/beads.db",
            "--prompt-text",
            "missing source",
        ],
    );
    assert_eq!(missing.status.code(), Some(1));
    assert_eq!(stderr_code(&missing), "not_found");

    let bad_db = tmp.path().join("bad_beads.db");
    {
        let conn = rusqlite::Connection::open(&bad_db).expect("open bad db");
        conn.execute("CREATE TABLE other (id INTEGER PRIMARY KEY)", [])
            .expect("create other table");
    }

    let malformed = run_db(
        &db,
        &[
            "import",
            "bd",
            "--session-token",
            &orch,
            "--beads-db",
            bad_db.to_str().expect("bad db path"),
            "--prompt-text",
            "malformed source",
        ],
    );
    assert_eq!(malformed.status.code(), Some(2));
    assert_eq!(stderr_code(&malformed), "invalid_args");
    let payload = parse_stderr_json(&malformed);
    assert_eq!(
        payload["message"],
        Value::String(format!(
            "invalid source schema in {}: missing issues table",
            bad_db.to_string_lossy()
        ))
    );
}

#[test]
fn import_command_empty_source_db_returns_zero_counts() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let covey = Covey::open(&db).expect("open covey");
    let orch = register_orchestrator(&covey, "orch-import-empty-cli");

    let beads_db = tmp.path().join("empty-beads.db");
    seed_empty_beads_db(&beads_db);

    let result = success_data(&run_db(
        &db,
        &[
            "import",
            "bd",
            "--session-token",
            &orch,
            "--beads-db",
            beads_db.to_str().expect("beads db path"),
            "--prompt-text",
            "empty source import",
        ],
    ));

    assert_eq!(result["operation"], "import_bd");
    assert!(result["meta_task_id"].as_str().is_some());
    assert_eq!(result["imported_count"], 0);
    assert_eq!(result["skipped_count"], 0);
    assert_eq!(result["items"].as_array().expect("items array").len(), 0,);
}

#[test]
fn import_human_output_summary() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let covey = Covey::open(&db).expect("open covey");
    let orch = register_orchestrator(&covey, "orch-import-human");

    let beads_db = tmp.path().join("beads.db");
    seed_beads_db(&beads_db);

    let result = covey
        .import_bd_v1(ImportBdV1Req::new_meta_task(
            orch.clone(),
            beads_db.to_string_lossy().to_string(),
            "human output summary".to_owned(),
            "import-human-summary".to_owned(),
        ))
        .expect("import should succeed");

    let summary = result.human_summary();
    assert!(
        summary.starts_with("imported 4 subtask(s) into"),
        "unexpected summary start: {summary}"
    );
    assert!(
        summary.contains("(skipped 3)"),
        "unexpected summary: {summary}"
    );
    assert!(
        summary.contains("3 invalid"),
        "unexpected summary: {summary}"
    );

    let repeat = covey
        .import_bd_v1(ImportBdV1Req::existing_meta_task(
            orch,
            beads_db.to_string_lossy().to_string(),
            result.meta_task_id.clone(),
            "import-human-repeat".to_owned(),
        ))
        .expect("repeat import should succeed");

    let repeat_summary = repeat.human_summary();
    assert!(
        repeat_summary.starts_with("imported 0 subtask(s) into"),
        "unexpected repeat summary start: {repeat_summary}"
    );
    assert!(
        repeat_summary.contains("(skipped 7)"),
        "unexpected repeat summary: {repeat_summary}"
    );
    assert!(
        repeat_summary.contains("4 duplicate"),
        "unexpected repeat summary: {repeat_summary}"
    );
    assert!(
        repeat_summary.contains("3 invalid"),
        "unexpected repeat summary: {repeat_summary}"
    );
}

#[test]
fn import_command_help_and_dispatch() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");

    let help = run_db(&db, &["import", "bd", "--help"]);
    assert!(help.status.success());
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(help_text.contains("--session-token"));
    assert!(help_text.contains("--beads-db"));
    assert!(help_text.contains("--meta-task-id"));
    assert!(help_text.contains("--prompt-text"));

    let orch = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "orch",
            "--agent-instance-id",
            "orch-1",
            "--role",
            "orchestrator",
        ],
    ))["session_token"]
        .as_str()
        .expect("orch token")
        .to_owned();

    let beads_db = tmp.path().join("beads.db");
    seed_beads_db(&beads_db);

    let result = success_data(&run_db(
        &db,
        &[
            "import",
            "bd",
            "--session-token",
            &orch,
            "--beads-db",
            beads_db.to_str().expect("beads db path"),
            "--prompt-text",
            "cli import test",
        ],
    ));
    assert!(result["meta_task_id"].as_str().is_some());
    assert_eq!(result["imported_count"], 4);
    assert_eq!(result["skipped_count"], 3);
    assert_eq!(result["operation"], "import_bd");

    let meta_task_id = result["meta_task_id"]
        .as_str()
        .expect("meta_task_id")
        .to_owned();
    let repeat = success_data(&run_db(
        &db,
        &[
            "import",
            "bd",
            "--session-token",
            &orch,
            "--beads-db",
            beads_db.to_str().expect("beads db path"),
            "--meta-task-id",
            &meta_task_id,
        ],
    ));
    assert_eq!(repeat["meta_task_id"], meta_task_id);
    assert_eq!(repeat["imported_count"], 0);
    assert_eq!(repeat["skipped_count"], 7);
}

#[test]
fn import_has_no_claim_mode_v1() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");

    let orch = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "orch",
            "--agent-instance-id",
            "orch-1",
            "--role",
            "orchestrator",
        ],
    ))["session_token"]
        .as_str()
        .expect("orch token")
        .to_owned();

    let beads_db = tmp.path().join("beads.db");
    seed_beads_db(&beads_db);

    let unknown_flag = run_db(
        &db,
        &[
            "import",
            "bd",
            "--session-token",
            &orch,
            "--beads-db",
            beads_db.to_str().expect("beads db path"),
            "--prompt-text",
            "test",
            "--create-held-claims",
        ],
    );
    assert!(!unknown_flag.status.success());
    let stderr = String::from_utf8_lossy(&unknown_flag.stderr);
    assert!(
        stderr.contains("error: unexpected argument '--create-held-claims' found"),
        "stderr: {stderr}"
    );
}

#[test]
fn claim_subtask_has_no_import_composition_mode() {
    // `claim_subtask` exists as a separate CLI primitive, but the importer has no
    // automatic held-claim composition mode. Any future import-and-claim path must
    // compose through `covey subtask claim` after import, not via an importer flag.
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");

    let orch = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "orch",
            "--agent-instance-id",
            "orch-1",
            "--role",
            "orchestrator",
        ],
    ))["session_token"]
        .as_str()
        .expect("orch token")
        .to_owned();

    let beads_db = tmp.path().join("beads.db");
    seed_beads_db(&beads_db);

    let unknown_flag = run_db(
        &db,
        &[
            "import",
            "bd",
            "--session-token",
            &orch,
            "--beads-db",
            beads_db.to_str().expect("beads db path"),
            "--prompt-text",
            "test",
            "--create-held-claims",
        ],
    );
    assert!(!unknown_flag.status.success());
    let stderr = String::from_utf8_lossy(&unknown_flag.stderr);
    assert!(
        stderr.contains("error: unexpected argument '--create-held-claims' found"),
        "stderr: {stderr}"
    );
}

#[test]
fn import_v1_truthfulness_contract() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let covey = Covey::open(&db).expect("open covey");
    let orch = register_orchestrator(&covey, "orch-import-truthfulness");

    let beads_db = tmp.path().join("beads.db");
    seed_beads_db(&beads_db);

    let json = success_data(&run_db(
        &db,
        &[
            "import",
            "bd",
            "--session-token",
            &orch,
            "--beads-db",
            beads_db.to_str().expect("beads db path"),
            "--prompt-text",
            "v1 truthfulness contract",
        ],
    ));

    // V1 result must not imply that imported items are already claimed.
    assert!(json["meta_task_id"].as_str().is_some());
    assert_eq!(json["imported_count"], 4);
    assert_eq!(json["skipped_count"], 3);

    let items = json["items"].as_array().expect("items array");
    for item in items {
        // Each item may have a subtask_id when imported, but never a claim_id.
        // The V1 surface deliberately omits any claim-related field.
        assert!(
            item.get("claim_id").is_none(),
            "V1 import result must not contain claim_id: {:?}",
            item
        );
        // Skip reasons are present for skipped items; imported items have null skip_reason.
        if item["subtask_id"].is_null() {
            assert!(!item["skip_reason"].is_null());
        }
    }

    // CLI help must not advertise a held-claim creation mode.
    let help = run_db(&db, &["import", "bd", "--help"]);
    assert!(help.status.success());
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(
        !help_text.contains("--create-held-claims"),
        "V1 CLI must not advertise held-claim creation"
    );
}

#[test]
fn reservation_and_conflict_commands_emit_json() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");

    let orch = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "orch",
            "--agent-instance-id",
            "orch-1",
            "--role",
            "orchestrator",
        ],
    ))["session_token"]
        .as_str()
        .expect("orch token")
        .to_owned();
    let meta_task_id = success_data(&run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "reserve things",
        ],
    ))["meta_task_id"]
        .as_str()
        .expect("meta id")
        .to_owned();
    let first = success_data(&run_db(
        &db,
        &[
            "subtask",
            "create",
            "--session-token",
            &orch,
            "--meta-task-id",
            &meta_task_id,
            "--title",
            "first",
            "--kind",
            "work",
            "--priority",
            "1",
            "--subtask-id",
            "work_1",
        ],
    ))["subtask_id"]
        .as_str()
        .expect("first")
        .to_owned();
    let second = success_data(&run_db(
        &db,
        &[
            "subtask",
            "create",
            "--session-token",
            &orch,
            "--meta-task-id",
            &meta_task_id,
            "--title",
            "second",
            "--kind",
            "work",
            "--priority",
            "2",
            "--subtask-id",
            "work_2",
        ],
    ))["subtask_id"]
        .as_str()
        .expect("second")
        .to_owned();

    let first_reservation = success_data(&run_db(
        &db,
        &[
            "reservation",
            "request",
            "--session-token",
            &orch,
            "--owner-subtask-id",
            &first,
            "--scope-class",
            "exact-path",
            "--scope-key",
            "src/lib.rs",
            "--lease-duration-ms",
            "30000",
        ],
    ))["reservation_id"]
        .as_str()
        .expect("reservation id")
        .to_owned();

    success_data(&run_db(
        &db,
        &[
            "reservation",
            "request",
            "--session-token",
            &orch,
            "--owner-subtask-id",
            &second,
            "--scope-class",
            "exact-path",
            "--scope-key",
            "src/lib.rs",
            "--lease-duration-ms",
            "30000",
        ],
    ));

    let overlaps = success_data(&run_db(
        &db,
        &[
            "reservation",
            "overlaps",
            "--scope-class",
            "exact-path",
            "--scope-key",
            "src/lib.rs",
        ],
    ));
    assert_eq!(overlaps.as_array().expect("overlaps").len(), 2);

    let conflicts = success_data(&run_db(&db, &["conflict", "list"]));
    let conflict_id = conflicts
        .as_array()
        .expect("conflicts")
        .first()
        .expect("first conflict")["conflict_id"]
        .as_str()
        .expect("conflict id")
        .to_owned();

    success_data(&run_db(
        &db,
        &[
            "conflict",
            "resolve",
            "--session-token",
            &orch,
            "--conflict-id",
            &conflict_id,
            "--resolution-state",
            "acknowledged",
        ],
    ));

    success_data(&run_db(
        &db,
        &[
            "reservation",
            "renew",
            "--session-token",
            &orch,
            "--reservation-id",
            &first_reservation,
            "--extend-by-ms",
            "1000",
        ],
    ));

    success_data(&run_db(
        &db,
        &[
            "reservation",
            "release",
            "--session-token",
            &orch,
            "--reservation-id",
            &first_reservation,
        ],
    ));
}

#[test]
fn exit_codes_cover_robot_contract() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");

    let not_found = run_db(&db, &["session", "status", "--session-token", "missing"]);
    assert_eq!(not_found.status.code(), Some(1));
    assert_eq!(stderr_code(&not_found), "not_found");

    let invalid = run_db(&db, &["session", "register"]);
    assert_eq!(invalid.status.code(), Some(2));
    assert_eq!(stderr_code(&invalid), "invalid_args");

    let orch = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "orch",
            "--agent-instance-id",
            "orch-1",
            "--role",
            "orchestrator",
        ],
    ))["session_token"]
        .as_str()
        .expect("orch token")
        .to_owned();
    let worker = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "worker",
            "--agent-instance-id",
            "worker-1",
            "--role",
            "executor",
        ],
    ))["session_token"]
        .as_str()
        .expect("worker token")
        .to_owned();

    let permission = run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &worker,
            "--prompt-text",
            "not allowed",
        ],
    );
    assert_eq!(permission.status.code(), Some(3));
    assert_eq!(stderr_code(&permission), "permission_denied");

    let conflict = run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "worker",
            "--agent-instance-id",
            "worker-2",
            "--role",
            "executor",
        ],
    );
    assert_eq!(conflict.status.code(), Some(4));
    assert_eq!(stderr_code(&conflict), "conflict");

    let _ = orch;
    let internal_tmp = TempDir::new().expect("internal tmp");
    let internal = run_db(internal_tmp.path(), &["conflict", "list"]);
    assert_eq!(internal.status.code(), Some(5));
    assert_eq!(stderr_code(&internal), "internal_error");
}

#[test]
fn maintenance_commands_emit_json() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");

    let stale = success_data(&run_db(
        &db,
        &["maint", "reap-stale", "--stale-threshold-ms", "1000"],
    ));
    assert_eq!(stale["stale_sessions"], 0);

    let expired_claims = success_data(&run_db(&db, &["maint", "expire-claims"]));
    assert_eq!(expired_claims["expired_count"], 0);

    let expired_reservations = success_data(&run_db(&db, &["maint", "expire-reservations"]));
    assert_eq!(expired_reservations["expired_count"], 0);
}

#[test]
fn mutation_ack_commands_emit_structured_data() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");

    let session = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "agent-a",
            "--agent-instance-id",
            "run-1",
            "--role",
            "executor",
        ],
    ))["session_token"]
        .as_str()
        .expect("session token")
        .to_owned();

    let heartbeat = success_data(&run_db(
        &db,
        &["session", "heartbeat", "--session-token", &session],
    ));
    assert_eq!(heartbeat["operation"], "heartbeat");
    assert_eq!(heartbeat["session_token"], session);
}

#[test]
fn subtask_claim_command_help_and_dispatch() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");

    let help = run_db(&db, &["subtask", "claim", "--help"]);
    assert!(help.status.success());
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(help_text.contains("--session-token"));
    assert!(help_text.contains("--subtask-id"));
    assert!(help_text.contains("--lease-duration-ms"));

    let orch = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "orch",
            "--agent-instance-id",
            "orch-1",
            "--role",
            "orchestrator",
        ],
    ))["session_token"]
        .as_str()
        .expect("orch token")
        .to_owned();

    let exec = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "worker",
            "--agent-instance-id",
            "worker-1",
            "--role",
            "executor",
        ],
    ))["session_token"]
        .as_str()
        .expect("exec token")
        .to_owned();

    let meta_task_id = success_data(&run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "ship targeted claim",
        ],
    ))["meta_task_id"]
        .as_str()
        .expect("meta id")
        .to_owned();

    let subtask_id = success_data(&run_db(
        &db,
        &[
            "subtask",
            "create",
            "--session-token",
            &orch,
            "--meta-task-id",
            &meta_task_id,
            "--title",
            "targeted work",
            "--kind",
            "work",
            "--priority",
            "1",
            "--subtask-id",
            "work_1",
        ],
    ))["subtask_id"]
        .as_str()
        .expect("subtask id")
        .to_owned();

    let claim = success_data(&run_db(
        &db,
        &[
            "subtask",
            "claim",
            "--session-token",
            &exec,
            "--subtask-id",
            &subtask_id,
            "--lease-duration-ms",
            "30000",
        ],
    ));
    assert_eq!(claim["subtask_id"], subtask_id);
    assert!(claim["claim_id"].as_str().is_some());
    assert!(claim["fence_seq"].as_i64().is_some());
    assert!(claim["lease_deadline"].as_i64().is_some());
}

#[test]
fn subtask_claim_emits_structured_json() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");

    let orch = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "orch",
            "--agent-instance-id",
            "orch-1",
            "--role",
            "orchestrator",
        ],
    ))["session_token"]
        .as_str()
        .expect("orch token")
        .to_owned();

    let exec = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "worker",
            "--agent-instance-id",
            "worker-1",
            "--role",
            "executor",
        ],
    ))["session_token"]
        .as_str()
        .expect("exec token")
        .to_owned();

    let meta_task_id = success_data(&run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "ship structured json",
        ],
    ))["meta_task_id"]
        .as_str()
        .expect("meta id")
        .to_owned();

    let subtask_id = success_data(&run_db(
        &db,
        &[
            "subtask",
            "create",
            "--session-token",
            &orch,
            "--meta-task-id",
            &meta_task_id,
            "--title",
            "structured json work",
            "--kind",
            "work",
            "--priority",
            "1",
            "--subtask-id",
            "work_1",
        ],
    ))["subtask_id"]
        .as_str()
        .expect("subtask id")
        .to_owned();

    let claim = success_data(&run_db(
        &db,
        &[
            "subtask",
            "claim",
            "--session-token",
            &exec,
            "--subtask-id",
            &subtask_id,
            "--lease-duration-ms",
            "30000",
            "--idempotency-key",
            "structured-json-key",
        ],
    ));

    assert_eq!(claim["subtask_id"], subtask_id);
    let claim_id = claim["claim_id"].as_str().expect("claim id");
    let fence_seq = claim["fence_seq"].as_i64().expect("fence_seq");
    let lease_deadline = claim["lease_deadline"].as_i64().expect("lease_deadline");
    assert!(!claim_id.is_empty());
    assert!(fence_seq > 0);
    assert!(lease_deadline > 0);

    let replay = success_data(&run_db(
        &db,
        &[
            "subtask",
            "claim",
            "--session-token",
            &exec,
            "--subtask-id",
            &subtask_id,
            "--lease-duration-ms",
            "30000",
            "--idempotency-key",
            "structured-json-key",
        ],
    ));
    assert_eq!(replay["claim_id"], claim_id);
    assert_eq!(replay["fence_seq"], fence_seq);
    assert_eq!(replay["lease_deadline"], lease_deadline);
}

#[test]
fn subtask_claim_reports_typed_failures() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");

    let orch = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "orch",
            "--agent-instance-id",
            "orch-1",
            "--role",
            "orchestrator",
        ],
    ))["session_token"]
        .as_str()
        .expect("orch token")
        .to_owned();

    let exec = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "worker",
            "--agent-instance-id",
            "worker-1",
            "--role",
            "executor",
        ],
    ))["session_token"]
        .as_str()
        .expect("exec token")
        .to_owned();

    let reviewer = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "reviewer",
            "--agent-instance-id",
            "reviewer-1",
            "--role",
            "reviewer",
        ],
    ))["session_token"]
        .as_str()
        .expect("reviewer token")
        .to_owned();

    let meta_task_id = success_data(&run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "ship typed failures",
        ],
    ))["meta_task_id"]
        .as_str()
        .expect("meta id")
        .to_owned();

    let subtask_id = success_data(&run_db(
        &db,
        &[
            "subtask",
            "create",
            "--session-token",
            &orch,
            "--meta-task-id",
            &meta_task_id,
            "--title",
            "typed failure work",
            "--kind",
            "work",
            "--priority",
            "1",
            "--subtask-id",
            "work_1",
        ],
    ))["subtask_id"]
        .as_str()
        .expect("subtask id")
        .to_owned();

    // 1. wrong-role: reviewer cannot claim a work subtask
    let wrong_role = run_db(
        &db,
        &[
            "subtask",
            "claim",
            "--session-token",
            &reviewer,
            "--subtask-id",
            &subtask_id,
            "--lease-duration-ms",
            "30000",
        ],
    );
    assert_eq!(wrong_role.status.code(), Some(3));
    assert_eq!(stderr_code(&wrong_role), "permission_denied");

    // 2. not-found: target subtask does not exist
    let not_found = run_db(
        &db,
        &[
            "subtask",
            "claim",
            "--session-token",
            &exec,
            "--subtask-id",
            "nonexistent_subtask",
            "--lease-duration-ms",
            "30000",
        ],
    );
    assert_eq!(not_found.status.code(), Some(1));
    assert_eq!(stderr_code(&not_found), "not_found");

    // 3. terminal meta-task: cancel meta task, then claim fails
    success_data(&run_db(
        &db,
        &[
            "meta",
            "cancel",
            "--session-token",
            &orch,
            "--meta-task-id",
            &meta_task_id,
        ],
    ));
    let terminal_meta = run_db(
        &db,
        &[
            "subtask",
            "claim",
            "--session-token",
            &exec,
            "--subtask-id",
            &subtask_id,
            "--lease-duration-ms",
            "30000",
        ],
    );
    assert_eq!(terminal_meta.status.code(), Some(4));
    assert_eq!(stderr_code(&terminal_meta), "conflict");
    let terminal_payload = parse_stderr_json(&terminal_meta);
    assert!(
        terminal_payload["message"]
            .as_str()
            .expect("message")
            .contains(&meta_task_id)
    );

    // Reset: create a fresh meta task and subtask for live-claim conflict
    let meta_task_id_2 = success_data(&run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "live claim conflict",
        ],
    ))["meta_task_id"]
        .as_str()
        .expect("meta id 2")
        .to_owned();

    let subtask_id_2 = success_data(&run_db(
        &db,
        &[
            "subtask",
            "create",
            "--session-token",
            &orch,
            "--meta-task-id",
            &meta_task_id_2,
            "--title",
            "conflict work",
            "--kind",
            "work",
            "--priority",
            "1",
            "--subtask-id",
            "work_2",
        ],
    ))["subtask_id"]
        .as_str()
        .expect("subtask id 2")
        .to_owned();

    let exec_2 = success_data(&run_db(
        &db,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "worker-2",
            "--agent-instance-id",
            "worker-2",
            "--role",
            "executor",
        ],
    ))["session_token"]
        .as_str()
        .expect("exec 2 token")
        .to_owned();

    // 4. live-claim conflict: first worker claims, second worker tries same subtask
    success_data(&run_db(
        &db,
        &[
            "subtask",
            "claim",
            "--session-token",
            &exec,
            "--subtask-id",
            &subtask_id_2,
            "--lease-duration-ms",
            "30000",
        ],
    ));
    let live_conflict = run_db(
        &db,
        &[
            "subtask",
            "claim",
            "--session-token",
            &exec_2,
            "--subtask-id",
            &subtask_id_2,
            "--lease-duration-ms",
            "30000",
        ],
    );
    assert_eq!(live_conflict.status.code(), Some(4));
    assert_eq!(stderr_code(&live_conflict), "conflict");
    let conflict_payload = parse_stderr_json(&live_conflict);
    assert!(
        conflict_payload["message"]
            .as_str()
            .expect("message")
            .contains(&subtask_id_2)
    );
}

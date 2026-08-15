---
title: "Better Droid CLI"
description: "Reference for Better Droid mission lint and compile commands"
sidebar:
  order: 13
---

Better Droid provides local mission-readiness tooling for OpenSpec changes that use the `better-droid` schema.

The first implemented surface is source-to-artifact first:

- `better-droid lint CHANGE_ID`
- `better-droid compile CHANGE_ID`
- `better-droid doctor CHANGE_ID`

These commands do not create Covey tasks, claims, reservations, reviews, apply queue entries, git commits, or mutAI settlement records by themselves.

## Usage

```bash
better-droid --project-root /data/projects/mutai lint CHANGE_ID --json
better-droid --project-root /data/projects/mutai compile CHANGE_ID --json
better-droid --project-root /data/projects/mutai doctor CHANGE_ID --json
```

Global flags:

- `--project-root <path>`: repository root. Default: current directory.
- `--json`: force JSON output. Non-TTY output is JSON by default.

## Lint

`lint` reads `openspec/changes/CHANGE_ID/` and reports mission readiness without writing mission artifacts.

The JSON report includes:

- status and import readiness
- checked source paths
- source digests
- hard blockers and warnings
- task counts
- task classifications and task digests

Hard blockers include missing required source files, stale-marked OpenSpec
changes, malformed task IDs, mission-incomplete executable tasks, unmapped
behavioral scenarios, unsafe path policy, and unresolved high or critical
assumption approval.

## Compile

`compile` runs the same readiness checks as lint. Stale-marked changes return a
blocked report and write no mission artifacts. If there are no hard blockers, it
writes the canonical JSON packet set under:

```text
.codex/state/better-droid/CHANGE_ID/mission/
```

Generated files:

- `mission.json`
- `mission-packet.json`
- `traceability.json`
- `validation.json`
- `path-policy.json`
- `review-rubric.json`
- `assumptions.json`
- `compile-report.json`

The command rejects output paths that leave the project or point into
`openspec/changes/**/mission/**`.

`mission-packet.json` is the compiled `mission_packet.v1` input consumed by `authority mission run`.
These files are compiler output and runner input only. They must not be listed
as executor `Allowed Write Paths` or task-level `Generated Paths`, and editing
or regenerating them is not a substitute for completing the source task.

## Doctor

`doctor` compares the root OpenSpec workflow path with Better Droid import
readiness for the same change. It checks that the `better-droid` schema resolves
from the repository root, runs OpenSpec strict validation/status through the
configured `openspec` executable, then runs Better Droid lint. It exits `0` only
when OpenSpec is structurally valid and Better Droid reports
`status: covey_import_ready`, `import_ready: true`, and
`readiness.covey_import_ready: true`. It exits `4` for planning-only,
human-blocked, malformed, or readiness-disagreement packets.

Better Droid readiness is intentionally split:

- Every Better Droid OpenSpec change must explicitly declare
  `planning_class: work_packet`. Missing metadata, `roadmap`, and any other
  value are malformed source. Phase plans, roadmap material, discovery plans,
  and strategy belong in `docs/plans/`, operator notes, or other planning
  documents outside `openspec/changes`.
- `planning_ready`: the non-executable source compiles as planning input only; it is
  not live Covey work and cannot claim execution, landing, or shipped evidence.
- `planning_ready_blocked`: the non-executable source is structurally useful but has a
  human gate such as `blocked-needs-human`.
- `covey_import_ready`: a small `planning_class: work_packet` packet with
  product/apply/migration write scope qualifies for Covey import. This does not
  mean Covey has imported it or that execution has started.
- `implementation_ready`: an implementation packet has concrete product write
  paths, product validation evidence, product behavior acceptance criteria, and
  changed-file evidence outside `openspec/**`. Planning-only, migration, and
  apply-only packets must keep this false.
- `landed` and `shipped_verified`: never claimed by Better Droid compile/doctor;
  these require Covey lifecycle evidence plus apply-gate/landing receipts and
  product verification evidence.

Better Droid compile/doctor must keep `covey_imported`, `execution_ready`,
`review_approved`, `apply_queued`, `apply_authorized`, `landed`, and
`shipped_verified` false. Those gates require downstream Covey, apply-gate, or
product verification evidence.

The repository must expose the Better Droid OpenSpec schema at:

```text
openspec/schemas/better-droid/
```

That root schema mirror is what `$BUN_CACHE/bin/openspec` and the `/opsx:*`
prompts resolve from the repository root. The copy under
`better-droid/openspec/schemas/better-droid/` remains the standalone
Better Droid package surface and is kept aligned by `just planning-surface-check`.

Use `--openspec-bin <path>` when the desired OpenSpec binary is not on `PATH`,
for example:

```bash
better-droid --project-root /data/projects/mutai doctor CHANGE_ID \
  --openspec-bin "$BUN_CACHE/bin/openspec" \
  --json
```

The packet's `runtime` section is compiled planning/runtime input. It does not
declare that workers ran, reviews approved, apply-gate authorization happened,
or product functionality shipped.

## Boundaries

Better Droid lint/compile is not Covey import. It does not dispatch workers, claim work, approve reviews, apply patches, archive OpenSpec changes, or settle mutAI claims. Downstream tools can consume the generated JSON packet set explicitly in later workflow steps.

In the current integrated path, the downstream sequence is:

1. `mutai-scheduler --json orchestrator run-openspec` doctors, compiles, imports, dispatches, observes apply progress, and archives the work packet or reports a named blocker.
2. The import step persists task and provenance state for the compiled change in Covey.
3. `codex-hooks` can resolve imported Better Droid provenance for a claimed subtask.
4. `codex-hooks` runs `authority mission run` against the compiled `mission-packet.json`.
5. The emitted evidence is published into Covey as a `verification-bundle`, then routed through review and archive cleanup without an apply-queue entry.

# Better Droid Architecture

Status: historical proposal / architecture note
Owner: operator-led
Scope: local, inspectable autonomous coding workflow built from OpenSpec, Covey, Codex/OpenCode/Hermes, Superpowers-style skills, and git/CI evidence

> This document is reference material. It predates the current mutAI operator
> loop and must not be read as active workflow authority. The active path for
> live OpenSpec work is `mutai-scheduler --json orchestrator run-openspec`,
> backed by Covey current-work state, Authority evidence, and the apply gate.
> CLI sketches below such as `better-droid import`, `spawn-workers`,
> `apply-next`, `archive`, and `recover` are historical design notes unless a
> current implementation doc points to an exact supported command.

## Executive Summary

"Better Droid" is a local, inspectable replacement for the useful parts of Factory Droid's mission workflow. The goal is not to clone Droid's SaaS product. The goal is to extract the good pattern: missionized coding with explicit contracts, bounded scope, validation, state, and proof.

Droid's useful insight is correct: coding agents need mission contracts and validators, not vibes. The weak part is the opaque autonomous runner. A mission can appear to be running while producing no useful progress, stale state can outlive actual repo completion, and the operator loses the senior-engineer steering loop exactly when the task gets hard.

Better Droid keeps the mission contract and proof discipline, but replaces the black-box runner with local, swappable, observable components:

- OpenSpec owns the artifact/spec layer: proposal, delta specs, design, tasks, verification, archive.
- Covey owns the coordination layer: sessions, claims, leases/fences, subtasks, reservations, artifacts, reviews, apply queue, event log, conflicts.
- Codex/OpenCode/Hermes own execution: local coding, review, debugging, and tool use.
- Superpowers/Hermes skills own procedure: planning, TDD, systematic debugging, subagent-driven execution, code review.
- git/CI own proof: diffs, tests, commits, pushes, remote containment, and reproducible evidence.

The design principle is simple:

Do not make one agent runtime responsible for everything. Keep artifacts, state, execution, review, and proof separate.

## Why Build This

### What Droid Gets Right

Droid missions behave like stateful work contracts. They tend to include:

- mission instructions
- allowed and forbidden paths
- feature/task status
- validation contracts
- autonomous worker and validator roles
- commit/push/PR expectations
- progress logs and mission-local state
- team standards through Blueprints

Those are good ideas. They turn "please code this" into "deliver this bounded outcome and prove it."

### Where Droid Breaks Down

The problem is not the mission abstraction. The problem is the opaque runner and SaaS control loop.

Observed/expected failure modes:

- Long-running missions can spin for hours without useful work.
- Mission state can become stale relative to the actual repo state.
- The operator cannot reliably inspect the internal planner/worker/validator state.
- Headless/PTY execution can wedge on environment prompts such as systemd-inhibit/PolicyKit sleep-lock authentication.
- Resource usage can be high and hard to bound.
- Failures are hard to root-cause because the state machine is owned by the product, not the operator.
- The operator often has to finish the work manually anyway.

For a high-agency solo or small-team workflow, local observability matters more than SaaS polish.

### The Better Droid Thesis

OpenSpec plus Covey can provide a better foundation than Droid for this environment:

- OpenSpec is closer to Droid's mission contract than generic skills are.
- Covey is closer to Droid's mission state than markdown checklists are.
- Codex/OpenCode/Hermes are better local executors because they are observable and swappable.
- Superpowers/Hermes skills provide the execution discipline Droid tries to hide inside the product.

The result is a system where every important question has a local answer:

- What are we building? Read OpenSpec.
- Who owns the work? Ask Covey.
- What changed? Inspect git.
- What evidence exists? Read artifacts, tests, and verification bundles.
- What happened in order? Read Covey events.
- Why did it fail? Read the local logs, prompts, diffs, and state.

## Non-Goals

Better Droid is not:

- a SaaS clone of Droid
- a new model runner
- a replacement for git
- a replacement for CI
- a general project-management system
- a distributed consensus system
- a reason for Covey to become an orchestrator
- a reason for OpenSpec to become an executor
- a reason to hide agent behavior behind another opaque loop

The system should stay decomposed. If one component starts owning too many responsibilities, it becomes the same kind of black box this architecture is trying to avoid.

## Core Architecture

```text
                 human / Hermes planner
                         │
                         ▼
             ┌───────────────────────┐
             │       OpenSpec        │
             │ proposal/spec/design  │
             │ tasks/verify/archive  │
             └───────────┬───────────┘
                         │ import / sync
                         ▼
             ┌───────────────────────┐
             │         Covey         │
             │ sessions, subtasks,   │
             │ claims, fences,       │
             │ reservations, events, │
             │ artifacts, reviews,   │
             │ apply queue           │
             └───────────┬───────────┘
                         │ claim/start/renew/publish/review
       ┌─────────────────┼──────────────────┐
       ▼                 ▼                  ▼
  Codex worker      OpenCode worker      Hermes worker
  + hooks           + wrapper            + native tools
       │                 │                  │
       └─────────────────┼──────────────────┘
                         ▼
             ┌───────────────────────┐
             │       git / CI        │
             │ diffs, tests, commit, │
             │ push, remote proof    │
             └───────────────────────┘
```

Each layer has a narrow job.

| Layer | Component | Responsibility | Must Not Own |
|---|---|---|---|
| Artifact/spec | OpenSpec | Intent, scope, behavior deltas, design, tasks, verification expectations | Worker leases, apply queue, model execution |
| Coordination | Covey | Sessions, claims, fences, reservations, artifacts, reviews, queue, event log | Planning, scheduling policy, patch application, model calls |
| Execution | Codex/OpenCode/Hermes | Implement, debug, inspect, test, produce artifacts | Authoritative task state |
| Procedure | Skills | TDD, debugging discipline, planning, reviews | Persistent mission state |
| Proof | git/CI | Diff, test results, commit, push, remote containment | Planning or task ownership |

## Concept Mapping: Droid to Better Droid

| Droid Concept | Better Droid Equivalent | Notes |
|---|---|---|
| mission.md | OpenSpec `proposal.md` + `design.md` | Captures why, scope, and approach. |
| validation-contract.md | OpenSpec `/opsx:verify` + project verification policy + CI | Validation should be explicit and runnable. |
| features.json | OpenSpec `tasks.md` imported into Covey subtasks | Human-readable tasks become machine-trackable subtasks. |
| state.json | Covey tables | State is transactional, not just a mutable JSON file. |
| progress_log.jsonl | Covey `event_log` + worker logs | Event log is append-only and queryable. |
| worker | Codex/OpenCode/Hermes executor session | Swappable local executor. |
| validator | Hermes reviewer / Codex reviewer / OpenSpec verifier | Reviews bind to exact artifact digests. |
| Blueprints | AGENTS.md + skills + OpenSpec config rules | Standards remain visible and editable. |
| PR delivery | Apply gate + git commit/push + CI | Push/merge is explicit and gated. |
| mission recovery | Read OpenSpec + Covey + git; kill stuck workers safely | No duplicate writers if claims/leases are respected. |

## Component Responsibilities

### OpenSpec: Artifact and Spec Layer

OpenSpec should own what the mission is.

Canonical change layout:

```text
openspec/
  specs/
    <domain>/spec.md
  changes/
    <change-id>/
      proposal.md
      design.md
      tasks.md
      specs/
        <domain>/spec.md
```

OpenSpec responsibilities:

- Capture intent, scope, non-goals, and approach.
- Describe externally visible behavior using requirements and scenarios.
- Keep implementation details in `design.md` and `tasks.md`, not in behavior specs.
- Support fluid iteration: update proposal/spec/design/tasks when implementation reveals reality.
- Verify implementation against artifacts before archive.
- Archive completed changes into the source-of-truth specs.

OpenSpec should not:

- Track live worker leases.
- Decide which worker owns a task.
- Apply patches.
- Replace git history.
- Hide verification behind agent confidence.

Better Droid should treat OpenSpec changes as the mission envelope.

### Covey: Coordination and State Layer

Covey should own live coordination state.

Per Covey's design, it answers four questions:

1. What work exists?
2. Who is doing what right now?
3. What is the status of the artifact a session produced?
4. What happened, in order?

Covey entities:

- `sessions`: active agents and roles
- `meta_tasks`: top-level missions/work packets
- `subtasks`: decomposed work and review tasks
- `claims`: fenced leased ownership of subtasks
- `artifacts`: immutable outputs by digest
- `reviews`: verdicts bound to exact artifact digests
- `reservations`: advisory path-scope hints
- `ready_queue`: approved artifacts awaiting apply
- `event_log`: append-only event stream
- `conflicts`: visible unresolved situations

Important Covey invariants:

- At most one active session per agent principal.
- At most one held claim per subtask.
- Fence tokens increase monotonically per subtask claim lifecycle.
- Artifact digests are immutable and unique.
- Reviews bind to one exact artifact digest.
- New artifacts require new review.
- At most one queued or in-flight queue entry per subtask.
- Every successful mutation appends one event.
- Failed validations append no event and leave no partial state.

Covey should not:

- Plan tasks.
- Schedule models.
- Invoke agents.
- Apply patches.
- Merge branches.
- Become a general application database.

Better Droid should use Covey as a correctness-critical coordination substrate, not as a policy brain.

### Codex/OpenCode/Hermes: Execution Layer

Executors do the actual work. They are replaceable.

Executor responsibilities:

- Claim or receive a Covey subtask.
- Read the relevant OpenSpec artifacts.
- Respect AGENTS.md and project instructions.
- Reserve intended edit scope where needed.
- Implement with TDD where code behavior changes.
- Debug systematically when tests fail.
- Publish an artifact or verification bundle.
- Request review.
- Release or renew claims correctly.

Executors should not:

- Mutate outside claim scope.
- Push or merge directly unless acting as apply gate.
- Treat their own summary as proof.
- Invent task state outside Covey.
- Continue after lease/fence failure.

### Superpowers/Hermes Skills: Procedure Layer

Skills define how agents should work:

- `writing-plans`: decompose work into concrete implementation steps.
- `subagent-driven-development`: execute task-by-task with spec review then quality review.
- `test-driven-development`: enforce red/green/refactor for code behavior changes.
- `systematic-debugging`: find root cause before fixes.
- `code-review`: review correctness, maintainability, and security.
- `factory-droid-mission-recovery`: recover stuck Droid-style missions without duplicate writers.

Skills are procedural memory. They should not be the authoritative state store.

### git/CI: Proof Layer

Proof is not a successful agent message. Proof is evidence.

Proof surfaces:

- `git status --short`
- `git diff --check`
- targeted tests
- package/full tests when warranted
- build checks
- static checks
- generated verification reports
- commit SHA
- push output
- `git ls-remote` remote containment
- CI status

Better Droid should never report completion solely because an agent wrote files. Completion requires evidence.

## Better Droid Mission Lifecycle

### 1. Explore

Use when requirements are unclear.

Inputs:

- operator idea
- existing docs/code
- known constraints
- comparable prior work

Outputs:

- clarified problem
- candidate scope
- risks
- recommended change boundary

Recommended tools:

- OpenSpec `/opsx:explore`
- Hermes research/investigation
- codebase search
- architecture notes

Exit criteria:

- The operator knows whether to start a new change or update an existing one.

### 2. Propose

Create the OpenSpec change.

Outputs:

```text
openspec/changes/<change-id>/proposal.md
openspec/changes/<change-id>/specs/<domain>/spec.md
openspec/changes/<change-id>/design.md
openspec/changes/<change-id>/tasks.md
```

The proposal should define:

- why
- what changes
- capabilities affected
- scope
- non-goals
- impact

Specs should define:

- requirements
- scenarios
- observable behavior
- error cases
- API, security, and reliability constraints

Design should define:

- current state
- chosen approach
- trade-offs
- risks
- data movement and rollback if needed

Tasks should define:

- bite-sized steps
- exact file paths where known
- verification command per step
- review/apply expectations

Exit criteria:

- A human or reviewer can tell what done means before coding starts.

### 3. Import to Covey

Convert OpenSpec tasks into live coordination state.

Desired future wrapper command:

```bash
better-droid import-openspec --change <change-id>
```

Desired future Covey import command if this bridge is implemented inside Covey:

```bash
covey import openspec --change <change-id> --session-token <token>
```

Current manual equivalent:

1. Register a coordinator-client or mission-control session.
2. Create a Covey meta task for the OpenSpec change.
3. Create one Covey work subtask per task/checklist item.
4. Optionally create review subtasks or let workers request them after artifact publication.

Meta task should preserve:

- OpenSpec change id
- path to change directory
- operator prompt
- expected verification commands
- evidence class if applicable

Subtasks should preserve:

- task id (`1.1`, `2.3`, etc.)
- title
- source file path (`tasks.md`)
- expected changed paths if known
- acceptance checks
- dependency hints if known

Exit criteria:

- `covey meta status` and `covey subtask status` can show live work state.

### 4. Claim and Reserve

A worker claims the next available subtask and starts it.

Canonical Covey flow:

```bash
covey --db /data/projects/mutai/.covey/mutai.db --json subtask claim-next \
  --session-token "$EXECUTOR_SESSION" \
  --lease-duration-ms 300000

covey --db /data/projects/mutai/.covey/mutai.db --json subtask start \
  --session-token "$EXECUTOR_SESSION" \
  --claim-id "$CLAIM_ID" \
  --fence-seq "$FENCE_SEQ"
```

If edit paths are known, request reservations:

```bash
covey --db /data/projects/mutai/.covey/mutai.db --json reservation request \
  --session-token "$EXECUTOR_SESSION" \
  --owner-subtask-id "$SUBTASK_ID" \
  --scope-class exact-path \
  --scope-key docs/better-droid-architecture.md \
  --lease-duration-ms 300000
```

Reservations are advisory, but they are valuable for collision detection and planning.

Exit criteria:

- Worker has a valid session, claim id, fence seq, and relevant reservations.

### 5. Execute

The worker implements the claimed task.

Execution rules:

- Read OpenSpec artifacts before editing.
- Read AGENTS.md and project instructions.
- Work only within claimed/reserved scope.
- Use TDD for behavior changes.
- Use systematic debugging for failures.
- Keep changes small.
- Run the narrowest verification that proves the task.
- Record blockers rather than spinning.

For Codex hooks, `Stop` should usually drive continuation:

- heartbeat session
- renew claim
- renew reservations
- if work remains, continue current subtask
- if current subtask is complete, publish artifact or request review
- if no active claim exists, claim next subtask
- if no work exists, stop cleanly

PreToolUse should block dangerous or claim-required operations:

Always deny by default:

- `git push`
- `git merge`
- `git rebase`
- `git reset --hard`
- forced `git clean`
- recursive force delete patterns

Require active claim:

- `git add`
- `git commit`
- file mutation commands such as `sed -i`, `tee`, and write-file equivalents where hooks can observe them

Exit criteria:

- The subtask has concrete output and local verification evidence.

### 6. Publish Artifact

Workers do not just say "done." They publish artifacts.

Artifact kinds supported by Covey include:

- `patch-bundle`
- `isolated-commit-ref`
- `tree-bundle`
- `findings-bundle`
- `verification-bundle`

Recommended Better Droid artifact manifest:

```json
{
  "schema": "better-droid.artifact.v1",
  "openspec_change": "add-dark-mode",
  "subtask_id": "add-dark-mode-1.2",
  "claim_id": "claim_...",
  "fence_seq": 12,
  "artifact_kind": "patch-bundle",
  "base_rev": "main@sha",
  "head_rev": "worker-branch@sha",
  "changed_paths": [
    "src/theme.ts",
    "tests/theme.test.ts"
  ],
  "verification": [
    {
      "command": "npm test -- theme.test.ts",
      "status": "passed",
      "summary": "3 tests passed"
    }
  ],
  "notes": "Implements task 1.2 only; no push performed."
}
```

Publish:

```bash
covey --db /data/projects/mutai/.covey/mutai.db --json artifact publish \
  --session-token "$EXECUTOR_SESSION" \
  --claim-id "$CLAIM_ID" \
  --fence-seq "$FENCE_SEQ" \
  --artifact-digest "$ARTIFACT_DIGEST" \
  --artifact-kind patch-bundle \
  --base-rev "$BASE_REV" \
  --manifest-path artifact.json \
  --changed-paths-digest "$CHANGED_PATHS_DIGEST"
```

Exit criteria:

- Covey records an immutable artifact digest for the subtask.

### 7. Review

Review is a first-class task, not a vibe.

Reviewer responsibilities:

- Read OpenSpec proposal/spec/design/tasks.
- Inspect artifact manifest and changed paths.
- Verify implementation maps to the exact task.
- Check that tests cover relevant scenarios.
- Check for scope creep.
- Check project conventions.
- Check security and operational risks.
- Decide `approve` or `changes-requested` against the exact artifact digest.

Request review:

```bash
covey --db /data/projects/mutai/.covey/mutai.db --json review request \
  --session-token "$EXECUTOR_SESSION" \
  --subtask-id "$SUBTASK_ID" \
  --artifact-digest "$ARTIFACT_DIGEST" \
  --review-subtask-id "$REVIEW_SUBTASK_ID"
```

Decide review:

```bash
covey --db /data/projects/mutai/.covey/mutai.db --json review decide \
  --session-token "$REVIEWER_SESSION" \
  --review-id "$REVIEW_ID" \
  --claim-id "$REVIEW_CLAIM_ID" \
  --fence-seq "$REVIEW_FENCE_SEQ" \
  --verdict approve \
  --findings-digest "$FINDINGS_DIGEST"
```

Important: Covey uses `approve`, not `approved`.

Exit criteria:

- Approved artifacts are eligible for the apply queue.
- Changes-requested artifacts return to work with explicit findings.

### 8. Apply Gate

The apply gate is the only role that should mutate the integration branch or push.

Apply gate responsibilities:

- Claim the approved queue item.
- Verify artifact digest and review status.
- Check worktree cleanliness and unrelated dirty paths.
- Apply patch or merge isolated commit.
- Run required verification.
- Commit if needed.
- Push if required.
- Verify remote containment.
- Mark queue item applied.

Apply gate must not trust worker summaries. It reads Covey and git directly.

Queue flow:

```bash
covey --db /data/projects/mutai/.covey/mutai.db --json queue enqueue \
  --session-token "$ORCH_SESSION" \
  --artifact-digest "$ARTIFACT_DIGEST" \
  --subtask-id "$SUBTASK_ID"

covey --db /data/projects/mutai/.covey/mutai.db --json queue claim-next \
  --session-token "$APPLY_GATE_SESSION" \
  --lease-duration-ms 300000
```

Proof commands commonly include:

```bash
git diff --check
git status --short
# project-specific tests/builds
git rev-parse HEAD
git push origin "$BRANCH"
git ls-remote origin "$BRANCH" | grep "$COMMIT"
```

Exit criteria:

- The approved artifact is integrated and proof is recorded.

### 9. Verify and Archive

After all relevant subtasks are applied, run OpenSpec verification.

Verification dimensions:

- Completeness: tasks done, required work present.
- Correctness: implementation satisfies requirements and scenarios.
- Coherence: implementation follows design decisions and project patterns.
- Evidence: tests/build/static checks/remote containment exist.

If verification passes:

- Archive the OpenSpec change.
- Sync specs into source-of-truth `openspec/specs/`.
- Keep Covey event/artifact/review records as execution evidence.

If verification fails:

- Create follow-up Covey subtasks or mark existing tasks changes-requested.
- Do not archive.

Exit criteria:

- OpenSpec source-of-truth reflects completed behavior.
- git/CI evidence proves the implementation.
- Covey state shows the work lifecycle.

## Local Data Layout

Recommended project-local layout:

```text
/data/projects/mutai/
  openspec/
    specs/
    changes/
  .covey/
    hermes.db
  .codex/
    hooks.json
    hooks/
      session_start.py
      stop.py
      pre_tool.py
      post_tool.py
    state/          # runtime, gitignored
  docs/
    better-droid-architecture.md
```

Use project-local Covey DB paths. Do not accidentally create `./covey.db` in arbitrary directories.

Canonical DB path for this project:

```text
/data/projects/mutai/.covey/mutai.db
```

Canonical Covey binary:

```text
covey
```

All Covey calls should use:

```bash
covey --db /data/projects/mutai/.covey/mutai.db --json ...
```

Do not use `cargo run` for production/hook integration in this environment.

## Codex Hook Architecture

Codex hooks should be thin adapters around Covey. They should not become a second orchestrator.

### SessionStart

Purpose:

- Register or recover a Covey session.
- Persist session token in repo-local hook state.
- Inject task/claim context into Codex.

Inputs:

- Codex session id
- cwd
- role config
- principal identity

Outputs:

- local state file
- optional additional context

### Stop

Purpose:

- Replace manual nudging.
- Continue work only when Covey says there is valid work.

Algorithm:

1. If `stop_hook_active` is true, no-op to prevent recursion.
2. Load local hook state.
3. Heartbeat session.
4. If active claim exists:
   - renew claim
   - renew reservations
   - return continuation prompt to continue current subtask
5. If no active claim:
   - claim next available subtask
   - start it
   - persist claim/fence/subtask
   - return continuation prompt to begin work
6. If no work exists, stop normally.

### PreToolUse

Purpose:

- Prevent unclaimed mutation.
- Prevent unsafe workflow transitions.

Policy:

- Deny push/merge/rebase/reset-hard/forced-clean unless explicitly in apply-gate workflow.
- Require active session, claim, fence, and subtask for mutation commands.
- Optionally check reservations/overlaps for path-scoped commands.
- Keep parsing conservative; if a shell wrapper hides a dangerous command, deny or recursively classify.

### PostToolUse

Purpose:

- Observe side effects.
- Sync changed path hints.
- Prepare artifact/review metadata.
- Surface reservation overlaps.

PostToolUse cannot undo bad changes, so safety belongs in PreToolUse.

## Hermes Integration

Hermes should expose Covey as a native plugin/toolset or bridge, not flatten it into the generic todo system too early.

Recommended Hermes config:

```yaml
covey:
  enabled: true
  binary: covey
  db_path: /data/projects/mutai/.covey/mutai.db
  project_root: /data/projects/mutai
  auto_register_session: true
  default_role: coordinator-client
  default_lease_duration_ms: 300000
  heartbeat_interval_ms: 10000
  claim_renew_interval_ms: 60000
  require_claim_for_delegation: true
  require_reservation_for_edits: true
  event_context_limit: 20
```

Hermes should provide tools/slash commands for:

- session register/status/heartbeat/exit
- meta submit/status/cancel
- subtask create/claim/start/status/stuck/abandon
- claim renew/release/expiring
- reservation request/renew/release/overlaps
- artifact publish
- review request/decide
- queue list/enqueue/claim/mark-in-flight/mark-applied
- events list
- conflicts list/resolve
- maintenance cleanup

Hermes-specific duties:

- Map platform/session identity to Covey `agent_principal_id` and `agent_instance_id`.
- Inject authoritative Covey context into prompts.
- Gate delegation when no claim/reservation exists.
- Verify subagent self-reports by reading Covey/git directly.
- Provide operator-friendly status summaries.
- Keep using Covey's JSON envelope and exit-code contract.

Hermes should not duplicate Covey's state machine.

## Better Droid CLI / UX Sketch

Historical note: this section is a sketch, not the active command surface.
Current operator docs should route normal work through
`mutai-scheduler --json orchestrator run-openspec`. Raw Covey commands and
recovery commands are maintenance tools for named current-work blockers.

A thin wrapper can make the system feel like Droid without hiding state.

Possible commands:

```bash
better-droid init
better-droid propose "add dark mode"
better-droid lint add-dark-mode --json
better-droid compile add-dark-mode --json
better-droid import add-dark-mode
better-droid status add-dark-mode
better-droid spawn-workers add-dark-mode --count 3 --executor codex
better-droid verify add-dark-mode
better-droid apply-next
better-droid archive add-dark-mode
better-droid recover add-dark-mode
```

Command responsibilities:

### `better-droid propose`

- Runs or guides OpenSpec proposal flow.
- Produces OpenSpec artifacts.
- Does not create live worker state unless requested.

### `better-droid lint`

- Reads `openspec/changes/<change-id>/` source authored with the `better-droid` schema.
- Reports source digests, task classifications, blockers, warnings, checked source paths, and import readiness.
- Writes no mission packet files.
- Does not create Covey tasks, claims, reservations, reviews, apply queue entries, git commits, or mutAI settlement records.

### `better-droid compile`

- Runs the same mission-readiness checks as lint.
- Writes the canonical JSON packet set under `.codex/state/better-droid/<change-id>/mission/` only when hard blockers are absent:
  - `mission.json`
  - `traceability.json`
  - `validation.json`
  - `path-policy.json`
  - `review-rubric.json`
  - `assumptions.json`
  - `compile-report.json`
- Keeps JSON as the canonical machine-readable contract. Markdown projections are not part of the first implementation slice.
- Rejects output paths that leave the project or point into `openspec/changes/**/mission/**`.

### `better-droid import`

- Reads OpenSpec tasks.
- Creates Covey meta task and subtasks.
- Adds provenance links back to OpenSpec files.

### `better-droid status`

Shows:

- OpenSpec artifact completeness
- Covey meta/subtask state
- active sessions
- active claims and lease expiry
- reservations and overlaps
- artifacts pending review
- queue items pending apply
- latest events
- git dirty state
- known blockers

### `better-droid spawn-workers`

- Starts Codex/OpenCode/Hermes workers.
- Ensures hooks/config point at the correct Covey DB.
- Enforces fan-out limits.
- Does not bypass claims.

### `better-droid verify`

Runs:

- OpenSpec verification
- project-specific test/build/static checks
- Covey consistency checks
- artifact/review/queue checks
- git diff/status checks

### `better-droid apply-next`

- Claims next apply queue item.
- Applies approved artifact.
- Runs verification.
- Commits/pushes if configured.
- Records evidence.

### `better-droid recover`

- Detects stale sessions and expired claims.
- Shows active workers and latest events.
- Compares Covey state to git state.
- Prevents duplicate writers.
- Suggests safe resume/direct-completion steps.

## State Model

Better Droid should treat Covey as authoritative for live state.

Suggested mission state projection:

```json
{
  "mission_id": "add-dark-mode",
  "openspec_change": "openspec/changes/add-dark-mode",
  "meta_task_id": "meta_...",
  "status": "in_progress",
  "subtasks": {
    "total": 8,
    "available": 2,
    "in_progress": 1,
    "review_pending": 1,
    "approved": 3,
    "applied": 1,
    "blocked": 0
  },
  "active_sessions": [
    {
      "principal": "codex-pane-1",
      "role": "executor",
      "subtask_id": "add-dark-mode-1.2",
      "lease_expires_at": "..."
    }
  ],
  "queue": {
    "ready": 2,
    "in_flight": 0
  },
  "latest_event_seq": 123
}
```

This projection may be generated on demand. Do not make it a separate mutable source of truth.

## Artifact and Evidence Model

Every non-trivial worker output should have an artifact manifest.

Minimum fields:

- schema identifier
- OpenSpec change id
- Covey meta task id
- Covey subtask id
- claim id
- fence seq
- artifact kind
- artifact digest
- base revision
- changed paths
- verification commands and outcomes
- known limitations
- generated files
- review status

Verification bundle fields:

- command
- working directory
- exit code
- stdout/stderr summary
- timestamp
- environment notes
- test count if available
- linked artifact digest

Findings bundle fields:

- reviewer id/session
- artifact digest reviewed
- verdict
- critical issues
- warnings
- suggestions
- exact files/lines where applicable
- required follow-up tasks

## Recovery Model

Better Droid should make stuck work recoverable without guesswork.

Recovery procedure:

1. Read OpenSpec change artifacts.
2. Query Covey meta/subtask status.
3. List active sessions.
4. List active/expired claims.
5. Read latest Covey events.
6. Check git status and diff.
7. Identify unrelated dirty paths.
8. Kill or stop only the workers that own stale claims or are proven stuck.
9. Expire/release claims through Covey, not by editing state files.
10. Resume from the last artifact/review/apply boundary.
11. Re-run verification before declaring completion.

A worker is stuck when:

- claim is active but no heartbeat/progress occurs past threshold
- latest event is old relative to lease policy
- no files changed and no artifact was published
- the worker repeats the same failing action without code changes
- process is blocked on an external prompt
- verifier output shows no structural progress

Recovery should never create duplicate writers for the same subtask.

## Safety and Policy

### Mutation Policy

- No mutation without a claim for autonomous workers.
- No push/merge/rebase outside apply gate.
- No destructive commands without explicit operator approval.
- No unrelated dirty path cleanup unless explicitly scoped.
- No completion report without verification evidence.

### Reservation Policy

- Normalize paths project-root-relative.
- Reject absolute paths outside project root.
- Normalize subtree scopes without trailing slash.
- Sort/dedupe generated-set members.
- Treat reservation overlaps as warnings or hard gates depending on risk.

### Secret Policy

- Run broad scans but classify false positives.
- Run stricter credential-shape scans for real secrets.
- Treat placeholders and domain vocabulary differently from live credentials.

### Resource Policy

- Fan-out should default to a small cap, e.g. three workers.
- One test process per worker by default.
- No endless retries.
- If the same test fails twice without code changes, stop rerunning and debug.
- Prefer narrow tests first; full suite at integration boundaries.

## Verification Policy

Completion requires evidence. A worker or reviewer cannot self-certify.

Minimum for docs-only changes:

```bash
git diff --check
git status --short
```

Minimum for code changes:

```bash
git diff --check
# narrow tests for touched behavior
# package or integration tests where warranted
# build/typecheck/lint where warranted
git status --short
```

Minimum for pushed work:

```bash
git rev-parse HEAD
git push origin "$BRANCH"
git ls-remote origin "$BRANCH" | grep "$COMMIT"
```

OpenSpec verification should check:

- all tasks are complete or intentionally deferred
- each requirement has corresponding implementation
- each scenario has code/test coverage or documented manual validation
- implementation follows design or design was updated to match reality
- no critical warnings remain before archive

Covey verification should check:

- no active stale claims for completed subtasks
- all applied artifacts had approved reviews
- review verdicts bind to the exact artifact digest applied
- queue is empty or remaining items are intentionally deferred
- latest events match the reported lifecycle

## Policy vs Substrate Boundary

Better Droid must be explicit about the difference between coordination substrate and orchestration policy.

Covey may expose operations like `claim-next`, `queue claim-next`, and status queries. Those are atomic state transitions and convenience selectors over eligible rows. They are not scheduling intelligence.

Policy belongs outside Covey:

- The operator, Hermes, or a Better Droid wrapper decides which OpenSpec change matters.
- The importer decides how OpenSpec tasks map into Covey subtasks.
- The launcher decides how many workers to start.
- The worker chooses how to implement within its claimed task.
- The reviewer decides whether an artifact satisfies the spec.
- The apply gate decides whether an approved artifact can safely mutate the integration worktree.

Covey enforces invariants and records events. It should not decide product priority, decomposition strategy, worker count, executor choice, or merge policy.

Use "coordinator client" or "mission-control session" for clients that submit tasks or enqueue artifacts. Avoid implying Covey itself is an orchestrator.

## OpenSpec as mutAI Planning Format, Not a mutAI Rewrite

This architecture should make mutAI more standard at the planning boundary without rewriting mutAI itself.

The desired decision is:

```text
mutAI should use OpenSpec as the preferred planning/specification format.
mutAI should not become OpenSpec.
Authority should not be rewritten around OpenSpec.
```

OpenSpec is the planning artifact format:

- `proposal.md` captures why, scope, impact, and non-goals.
- `design.md` captures approach, trade-offs, data-movement, and rollback thinking.
- `tasks.md` captures implementation breakdown and verification steps.
- `specs/*/spec.md` captures behavioral requirements and scenarios.
- OpenSpec verify/archive closes the planning loop when the implementation is done.

mutAI remains the evidence-bearing change system:

- claim admission
- runtime dispatch
- leases and ownership boundaries
- artifact/review/evidence handling
- apply/landing authority
- settlement
- repo mutation legality
- verifier and proof policy

The integration seam should be upstream of mutAI core:

```text
OpenSpec change
  └─ proposal.md / design.md / tasks.md / specs/*
        │
        ▼
OpenSpec importer / sync adapter
        │
        ▼
Covey meta_task + subtasks, or current task-definition import path
        │
        ▼
mutAI claim/admission/runtime/settlement flow
        │
        ▼
executor work + artifacts + reviews + apply gate + git/CI evidence
```

### Why This Boundary Matters

mutAI's differentiator is not a custom markdown planning DSL. Its differentiator is turning executable work packets into settled, evidence-bearing claims.

Using OpenSpec avoids reinventing:

- proposal format
- design notes
- task breakdown conventions
- behavioral requirement/scenario structure
- verify/archive lifecycle

Keeping OpenSpec outside mutAI core avoids:

- rewriting mutAI around a planning format
- teaching settlement authority to parse markdown
- coupling runtime dispatch to a specific file layout
- creating a second live state source
- making OpenSpec responsible for claims, leases, or apply queues

### Authority Boundary

`/data/projects/mutai/authority/` should stay narrow.

Authority should continue to be described with forensic precision as kernels, contracts, and evidence emitters. Covey owns live coordination; Authority evaluates one Covey-selected claim or apply attempt at a time. Covey owns live coordination; `mutai-rs` evaluates one Covey-selected claim or apply attempt at a time.

Authority may carry opaque OpenSpec/Covey provenance references through typed facts, envelopes, or audit metadata. It should not own OpenSpec parsing as core runtime behavior.

Acceptable Authority involvement:

- carry `openspec_change_id` as opaque provenance
- carry `openspec_task_id` as opaque provenance
- carry proposal/design/tasks/spec digests as audit metadata
- include OpenSpec references in runtime prompt payloads
- bind approved artifact facts to review/apply evidence that originated from OpenSpec-derived tasks

Non-goals for Authority:

- no OpenSpec parser in settlement authority
- no markdown task parser in runtime fleet
- no OpenSpec-owned runtime leases
- no OpenSpec-owned worker scheduling
- no OpenSpec-owned apply queue
- no mutation of `openspec/changes/*` by core settlement code
- no replacement of Covey/task state
- no dual writer over task or claim authority

If a Rust-facing API is needed, it should accept already-normalized planning provenance:

```json
{
  "planning_format": "openspec",
  "openspec_change_id": "add-better-droid-hooks",
  "openspec_change_path": "openspec/changes/add-better-droid-hooks",
  "openspec_task_id": "1.2",
  "openspec_requirement_ids": [
    "REQ-claim-gated-mutation",
    "REQ-review-before-apply"
  ],
  "proposal_digest": "blake3:...",
  "design_digest": "blake3:...",
  "tasks_digest": "blake3:..."
}
```

That lets mutAI preserve auditability without becoming the planner.

### Importer / Adapter Responsibilities

The missing glue should be a thin importer or adapter, not a mutAI rewrite.

Possible names:

- `openspec-mutai-import`
- `better-droid import-openspec`
- `covey import openspec` if the bridge belongs in Covey
- Hermes OpenSpec-to-Covey command

Responsibilities:

- read `openspec/changes/<change-id>/proposal.md`
- read `design.md`
- read `tasks.md`
- read changed specs under `specs/`
- create or update one Covey meta task for the OpenSpec change
- create or update one Covey subtask per stable OpenSpec task id
- preserve provenance links back to proposal, design, tasks, and specs
- preserve digests for audit and invalidation
- detect changed acceptance criteria under active claims
- emit Covey events or conflicts when re-import changes active work
- remain idempotent

The importer should not:

- schedule workers
- hold claims
- mutate apply queues except through explicit coordinator policy
- decide product priority
- apply patches
- archive OpenSpec changes after implementation unless invoked as a separate verified archive step

### Transitional Compatibility

A safe adoption path has two lanes:

Primary future lane:

```text
OpenSpec -> Covey meta_tasks/subtasks -> mutAI runtime/settlement
```

Compatibility lane for current production flow:

```text
OpenSpec -> existing Beads/task-definition creation path -> current mutAI flow
```

This lets the project standardize planning on OpenSpec immediately without blocking on a full Covey/mutAI bridge and without creating a no-dual-writer violation.

### No-Dual-Writer Rule

There must be one authority for each kind of state.

- OpenSpec is the authority for planning artifacts.
- Covey/task-definition storage is the authority for live task/subtask/claim coordination.
- mutAI is the authority for admission, runtime dispatch, verifier policy, settlement, and landing legality.
- git/CI is the authority for committed code and reproducible proof.

Do not let OpenSpec and Covey both mutate live task state. Do not let any alternate runtime or settlement writer race Authority evidence paths. A promoted path must retain one live writer for each state family rather than operating dual writers indefinitely.

## Workspace Isolation Policy

Concurrent workers should not casually share one dirty worktree.

Default policy:

- One worker gets one isolated git worktree or branch.
- The apply gate owns the integration worktree.
- Shared worktrees are allowed only for read-only/review roles or single-worker supervised mode.
- Worker branch names should include mission, subtask, and session identity where practical.
- Dirty worktree state must be checked before worker start.
- Unrelated dirty paths must be recorded as baseline and left untouched.

Suggested worker branch naming:

```text
better-droid/<change-id>/<subtask-id>/<agent-principal-id>
```

Before worker start:

```bash
git status --short
git rev-parse HEAD
```

Before apply gate mutation:

```bash
git status --short
git diff --check
```

If a worker loses its claim or lease, it must stop mutating immediately. It may preserve a local diff for recovery, but it must not publish an artifact unless it regains a valid claim/fence.

## OpenSpec to Covey Sync Rules

Import must be idempotent and provenance-preserving.

Stable rules:

- Each OpenSpec change maps to one Covey meta task.
- Each stable task id in `tasks.md` maps to one deterministic Covey subtask id.
- Re-importing must not duplicate subtasks.
- New tasks create new subtasks.
- Removed tasks are canceled, deferred, or marked obsolete; they are not silently deleted.
- Changed task text updates metadata, but must not silently rewrite the active claim context for an in-progress worker.
- Changed acceptance criteria on an active task should create a Covey event or conflict.
- Dependency changes should be logged.
- If OpenSpec requirements change after artifact approval, the approval should be considered stale unless a reviewer explicitly revalidates it.

Importer output should include a summary like:

```text
created: 3 subtasks
updated metadata: 2 subtasks
unchanged: 7 subtasks
obsolete/deferred: 1 subtask
conflicts: 1 active task changed acceptance criteria
```

Dependency handling should stay simple at first. If tasks include dependencies, the Better Droid wrapper should filter eligible work before invoking worker claim flows. Covey should only enforce explicit dependency constraints if that capability exists as part of its state model.

## Artifact Storage and Digest Rules

An artifact digest is only meaningful if the digest rules are deterministic.

Recommended project-local storage:

```text
.covey/
  artifacts/
    blake3-<digest>/
      manifest.json
      patch.diff
      changed-paths.txt
      verification/
        <command-id>.stdout.txt
        <command-id>.stderr.txt
        summary.json
      findings.json
```

Recommended digest policy:

- Use BLAKE3 for bridge-level artifact bundle digests unless Covey standardizes another digest scheme for the artifact payload.
- Hash canonical JSON for `manifest.json` plus the exact bytes of payload files.
- Include the changed-path list in the digest.
- Include base/head refs in the manifest.
- Keep large logs out of the primary digest if needed, but include their own content digests in the manifest.
- Never mutate files under an existing artifact digest directory.
- If anything material changes, create a new artifact digest.

Artifact kind semantics:

| Kind | Meaning | Apply Behavior |
|---|---|---|
| `patch-bundle` | Patch against an exact base revision | Apply with strict patch/index checks; fail on conflict/fuzz unless explicitly allowed. |
| `isolated-commit-ref` | A commit or branch produced in an isolated worker worktree | Cherry-pick or merge through apply gate after base and review checks. |
| `tree-bundle` | Full tree output or generated tree snapshot | Apply only through controlled tree diff/copy logic owned by apply gate. |
| `findings-bundle` | Reviewer findings | Never applied to worktree. |
| `verification-bundle` | Test/build/proof evidence | Never applied to worktree. |

Retention policy should be explicit before heavy use. Initial deployments can keep artifacts indefinitely under `.covey/artifacts/` and rely on manual cleanup.

## Review Invalidation and Stale Base Policy

A review verdict binds to an exact artifact digest. Approval is invalidated when material facts change.

Approval should be considered stale if:

- artifact digest changes
- changed paths change
- base revision changes materially
- OpenSpec requirements or acceptance criteria change
- verification previously passed but now fails
- apply gate modifies the patch beyond mechanical metadata
- a newer artifact supersedes the reviewed artifact
- security or dependency context changes in a way that affects the task

Stale base handling:

1. Compare artifact `base_rev` to the current integration branch.
2. If base is stale but patch applies cleanly, rerun required verification and record base drift.
3. If apply requires semantic edits, produce a new artifact and new review.
4. If patch conflicts, mark apply conflict and return to worker/recovery flow.
5. Do not silently bless a modified artifact with an old review.

Apply queue metadata belongs to Covey. Worktree mutation belongs to the apply gate. Covey can record states such as queued, claimed, in-flight, applied, superseded, or conflicted; it should not apply patches itself.

## Session, Lease, and Fence Failure Policy

Session identity should be stable and auditable.

Identity model:

- `agent_principal_id`: stable logical worker identity, such as tmux pane or Hermes session role.
- `agent_instance_id`: unique runtime/process/session identity.
- `session_token`: local credential for Covey operations; never commit it.

Token storage:

- Store under `.codex/state/`, Hermes runtime state, or an equivalent gitignored path.
- Restrict filesystem permissions where possible.
- Do not include session tokens in artifact manifests.

On heartbeat failure:

- Stop mutating.
- Report blocked/stale state.
- Do not publish artifacts.

On claim renew/start/publish failure due to stale fence or expired lease:

1. Stop mutation immediately.
2. Snapshot local diff only if safe and clearly mark it as unowned/unpublished.
3. Do not publish a normal artifact under the invalid claim.
4. Record a blocker or conflict.
5. Let the operator/coordinator decide whether to salvage the diff under a new claim.

On restart:

- Recover local state only if project root, principal id, instance metadata, and worktree match expectations.
- Otherwise register a new session and claim fresh work.

## Conflict Model

Conflicts should be visible state, not hidden warnings.

Examples that should create or surface conflicts:

- overlapping reservations on paths that both workers intend to mutate
- OpenSpec task acceptance criteria changed while claimed
- approved artifact has stale base revision
- two artifacts compete for the same subtask
- reviewer approves an older artifact after a newer artifact exists
- apply patch conflicts with integration branch
- dirty path outside reservation appears during apply
- worker tries to mutate without a valid claim
- verification fails after approval

Conflict ownership:

- Covey records conflict state and events.
- Operator/coordinator resolves policy conflicts.
- Apply gate resolves integration conflicts only within explicit apply scope.
- Workers should not self-resolve conflicts by editing state directly.

Resolution should include a reason. Resolution should not mutate immutable artifact or review history.

## Verification Provenance

Verification bundles should be hard to spoof accidentally.

Each verification record should include:

- command argv or exact shell command
- working directory
- start timestamp
- end timestamp
- exit code
- stdout/stderr paths or digests
- git HEAD before command
- git HEAD after command
- executor identity and role: worker, reviewer, apply gate, or CI
- relevant tool versions where cheap to collect
- environment profile: host, container, nix/devshell, or CI job id
- summary and pass/fail classification

A verification summary without command provenance is narrative, not proof.

## Manual Override Policy

The operator can override local automation, but overrides must be attributable.

Allowed overrides:

- cancel/defer subtasks
- release or expire stale claims
- mark conflicts acknowledged/resolved
- supersede queue items
- enqueue/dequeue artifacts with reason
- stop or kill stuck local worker processes

Disallowed overrides:

- editing Covey DB rows manually as the normal path
- mutating artifact digest history
- changing review verdicts without a new event/reason
- silently marking work complete without evidence

Every override should include:

- who did it
- why
- what state changed
- what evidence was inspected
- whether follow-up verification is required

## Security and Runtime Policy

Autonomous workers should run with least practical privilege.

Initial policy:

- No production credentials in worker environments.
- Network access should be explicit for tasks that need it.
- Dependency installation should be reviewed for high-risk projects.
- Generated scripts should be inspected before execution when they affect infrastructure or credentials.
- Workers should not be able to push/merge directly unless they are explicitly the apply gate.
- Environment prompts, password prompts, or PolicyKit prompts are blockers; do not wait indefinitely.
- Tool allow/deny lists should be executor-specific and visible.

Secret scanning:

- Broad scans may flag domain words; classify false positives.
- Credential-shape scans should look for real tokens, keys, private keys, and provider-specific secrets.
- Placeholders and documentation examples should be explicitly classified.

## Loop Controls

Stop-hook continuation is useful, but it can recreate the opaque Droid loop if left unbounded.

Required loop controls:

- max continuations per subtask
- max wall-clock time per subtask
- max no-progress continuations
- stop after repeated identical failure without code changes
- require artifact/checkpoint after a bounded amount of work
- continuation prompt must include subtask id, claim id, fence seq, and exit criteria
- every continuation decision must be reconstructable from hook logs and Covey events

No-progress examples:

- same test failure repeated twice with no code changes
- no changed files and no new artifact after multiple continuations
- worker asks the same question repeatedly
- worker blocks on password/system prompt
- worker exceeds lease renewal budget

## Mission Done Checklist

A Better Droid mission is done only when all relevant evidence exists.

Checklist:

- [ ] OpenSpec proposal/spec/design/tasks reflect final intended behavior.
- [ ] All OpenSpec tasks are complete or explicitly deferred with reasons.
- [ ] All applied artifacts had approved reviews bound to exact artifact digests.
- [ ] No stale active claims remain for completed subtasks.
- [ ] No unresolved conflicts remain.
- [ ] Required tests/build/static checks passed.
- [ ] `git diff --check` passed for relevant changes.
- [ ] Git status is acceptable and unrelated dirty paths are documented.
- [ ] Commit SHA is recorded if committed.
- [ ] Push/remote containment is recorded if pushed.
- [ ] CI status is recorded if CI is required.
- [ ] OpenSpec is archived, or left unarchived with an explicit reason.
- [ ] Final evidence report exists.

## What to Copy from Droid, and What Not to Copy

Copy:

- mission contract
- scoped allowed/forbidden paths
- validator mindset
- progress/evidence trail
- recovery boundary
- team standards as explicit editable policy

Do not copy:

- opaque long-running planner
- hidden state machine
- SaaS-only state
- non-local evidence
- worker self-certification
- stale mission status independent of git
- unbounded compute loops

## Rollout Plan

### Phase 0: Manual Better Droid

Goal: prove the workflow without automation.

Steps:

1. Use OpenSpec for proposal/spec/design/tasks.
2. Manually create Covey meta task and subtasks.
3. Use Hermes/Codex manually as workers.
4. Publish simple artifact manifests.
5. Review manually.
6. Apply manually.
7. Verify and archive.

Success criteria:

- The operator can inspect all state.
- No task needs Droid to make progress.
- Completion evidence is better than a Droid mission summary.

### Phase 1: OpenSpec to Covey Importer

Goal: reduce manual task creation.

Build:

- `covey import openspec` or `better-droid import`
- parser for OpenSpec `tasks.md`
- provenance mapping back to proposal/spec/design/tasks
- idempotent re-import behavior

Success criteria:

- Re-running import does not duplicate subtasks.
- Task IDs remain stable.
- Covey status links back to OpenSpec artifacts.

### Phase 2: Codex SessionStart + Stop Hooks

Goal: replace manual nudging.

Build:

- SessionStart registration
- Stop heartbeat/renew/claim-next/continue
- loop guard for `stop_hook_active`
- repo-local hook state

Success criteria:

- A Codex worker can claim and continue work without repeated operator prompts.
- No recursive hook loops.
- Claims renew and expire predictably.

### Phase 3: PreToolUse Guardrails

Goal: prevent unclaimed or unsafe mutation.

Build:

- command classifier
- deny dangerous git/destructive operations
- require claim for mutation
- optional reservation overlap checks

Success criteria:

- Dangerous commands are blocked.
- Claim-scoped edits are allowed.
- Known bypass classes are tested.

### Phase 4: Artifact, Review, Apply Queue

Goal: make output reviewable and apply-gated.

Build:

- artifact manifest convention
- publish helpers
- reviewer workflow
- apply queue wrapper
- final evidence report

Success criteria:

- No worker pushes directly.
- Applied work has an approved artifact digest.
- The apply gate can prove what it applied.

### Phase 5: Better Droid CLI / Hermes UX

Goal: make the workflow pleasant.

Build:

- `better-droid status`
- Hermes slash commands
- dashboard or concise terminal summary
- recovery helper
- OpenSpec verify wrapper

Success criteria:

- Operator can understand mission state in one command.
- Recovery from stuck workers is explicit and safe.
- The workflow feels easier than Droid, not just more correct.

## Open Questions

1. Should OpenSpec task import live in Covey, Hermes, or a separate `better-droid` wrapper?

Recommendation: start outside Covey. Covey should stay narrow. A wrapper or Hermes plugin can translate OpenSpec into Covey API calls.

2. Should OpenSpec verification write Covey verification bundles automatically?

Recommendation: yes, but as a bridge/tooling layer. Verification bundles should be immutable artifacts linked to the relevant subtask or meta task.

3. Should apply gate be Hermes or a standalone CLI?

Recommendation: both eventually. Start as a CLI for determinism, expose through Hermes after the command contract is stable.

4. Should reservations be hard gates?

Recommendation: advisory in early phases, hard gate for autonomous swarms or high-risk paths.

5. Should Better Droid support multiple repos?

Recommendation: not initially. Start project-local. Multi-repo support should come after single-repo state, artifact, and apply semantics are boring.

## Success Metrics

Better Droid is working if:

- Work can be resumed from local state without reading chat history.
- The operator can answer "who owns what?" in one command.
- Workers cannot safely mutate without claims.
- Review verdicts bind to immutable artifacts.
- Apply is gated and auditable.
- Completion reports include test/build/git evidence.
- Stuck workers are recoverable without duplicate writers.
- The executor can be swapped without changing mission state.
- The workflow ships faster than Droid for local/high-agency work.

## Anti-Patterns

Avoid these:

- Making Covey plan work.
- Making OpenSpec track live leases.
- Letting workers push directly.
- Treating agent summaries as proof.
- Creating mutable mission state outside Covey.
- Storing project Covey state in accidental `./covey.db` files.
- Over-automating before manual lifecycle is proven.
- Hiding failures behind a friendly dashboard.
- Letting hook scripts become a full orchestrator.
- Continuing workers after claim/fence failure.

## Minimal MVP Definition

The smallest Better Droid worth building:

1. OpenSpec change exists with proposal/spec/design/tasks.
2. A wrapper imports `tasks.md` into Covey subtasks idempotently.
3. Codex/Hermes workers claim/start/renew subtasks through Covey.
4. Workers publish artifact manifests.
5. Reviewer approves or requests changes against exact artifact digest.
6. Apply gate integrates approved artifacts only.
7. Verification emits a final evidence report.
8. Status command shows OpenSpec + Covey + git state.

This MVP beats Droid if it is:

- more observable
- less wasteful
- easier to recover
- executor-agnostic
- strict about proof

## Final Position

Better Droid should be a missionized coding workflow, not another opaque autonomous engineer.

The architecture is:

```text
OpenSpec defines done.
Covey coordinates ownership and artifact lifecycle.
Codex/OpenCode/Hermes execute locally.
Skills enforce disciplined engineering behavior.
git/CI prove the result.
```

That stack preserves Droid's best idea — bounded missions with validators — while avoiding Droid's worst failure mode: a black-box runner that can burn time, stale its own state, and make root-cause analysis harder.

For this environment, the goal is not more autonomy at any cost. The goal is settled, evidence-bearing change with local control.

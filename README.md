# Covey

Covey is a small, correctness-critical coordination substrate for agent cohorts. It stores the shared state for a decomposed task and answers four questions:

1. What work exists?
2. Who is doing what right now?
3. What is the status of the artifact a session produced?
4. What happened, in order?

That is the whole job. Covey stores state, enforces invariants, and exposes changes. It does not plan work, schedule agents, apply patches, invoke models, or act as a general-purpose workflow engine.

Version: `0.1` draft.

## Scope

Covey is intentionally narrow.

- It is not an orchestrator.
- It is not a merge queue.
- It is not a message bus.
- It is not a general application database.
- It is not a distributed system.
- It is not a replacement for git, mutAI, or other higher-level coordination systems.

If a proposed feature does more than store, constrain, and notify, it probably belongs somewhere else.

## Design Principles

- Stability over features. Covey is the floor other components depend on.
- Integrity by construction. Invariants live in database constraints or transaction-scoped checks, not comments.
- Transactional atomicity. Every state change and its event-log append happen in one transaction or not at all.
- State-first design. Relational tables are authoritative; the event log is a consequence of mutations, not the source of truth.
- Loud failure. Invalid operations return typed errors at the API boundary.

## Architecture

The intended v1 shape is a single Rust coordination service over one authoritative transactional store. The first deployment mode is embedded: wrapper processes link the `covey` crate directly and share one Covey state store.

In this repository, the embedded library API is the primary interface. Daemonizing it behind HTTP or gRPC later should be a transport concern, not a data-model change.

Embedded-first keeps early development simple. Moving to a daemon later should be a transport change, not a data-model rewrite.

## Core Model

Covey tracks a fixed set of entities:

- `sessions`: who is connected, what role they have, and when they last heartbeated
- `meta_tasks`: top-level operator work items
- `subtasks`: decomposed units of work, including first-class review subtasks
- `claims`: fenced ownership of a subtask with leases
- `artifacts`: immutable outputs identified by digest
- `reviews`: verdicts attached to an exact artifact digest
- `reservations`: advisory path-scope hints for planning
- `ready_queue`: approved artifacts waiting for apply, with leased apply claims
- `event_log`: append-only change feed for subscribers
- `conflicts`: visible unresolved situations needing intervention

Schema changes are forward-only.

## Invariants That Matter

Covey exists to make invalid states hard or impossible.

- At most one active session per `agent_principal_id`
- At most one held claim per subtask
- Fence tokens increase monotonically per subtask claim lifecycle
- Ready-queue apply claims are fenced and leased before apply completion is accepted
- Artifact digests are unique and immutable
- Reviews bind to one exact artifact digest; a new artifact requires a new review
- At most one queued or in-flight ready-queue entry per subtask
- Every successful mutation appends exactly one event-log row
- Failed validations append no event-log row and leave no partial state

These guarantees are enforced through database constraints plus transactional checks in the Covey API layer.

## State Machines

The important workflow is:

`available -> claimed -> in_progress -> artifact_published -> review_pending -> approved -> ready_for_apply -> applied`

Work can also cycle through `changes_requested` after review and return to `in_progress` with a new artifact. Terminal work states are `applied` and `abandoned`.

Claims are simpler:

`held -> released | expired | revoked`

Reviews are keyed to artifact digests, so stale approvals cannot accidentally bless new outputs.

## API Shape

The crate exposes a narrow `Covey` API grouped around:

- session lifecycle: register, heartbeat, exit
- meta-task lifecycle: submit and cancel
- subtask lifecycle: create, claim, start, abandon, release claim
- lease lifecycle: renew claim and reservation leases
- artifact publication
- review request and decision
- ready-queue inspection plus atomic apply-claim operations for the apply gate
- reservation request, release, and overlap lookup
- event fetching
- conflict listing and resolution
- status queries and maintenance operations

All mutating requests carry a `session_token`. Ownership-sensitive operations also require a fence token.

## Concurrency And Liveness

Covey relies on single-writer transactional serialization in the local deployment model.

- Mutations execute as one transaction or not at all
- Read paths observe committed state
- Write contention must be handled with bounded retry or blocking semantics
- Sessions heartbeat every 10s
- Sessions older than the stale threshold are marked `stale`
- Claims, reservations, and apply claims are rejected on command paths once their leases expire; maintenance only cleans persisted rows lazily

This keeps safety in the substrate and policy in the orchestrator.

## Guarantees And Non-Goals

Covey guarantees safety, ordering, and transactional integrity inside one authoritative local store. It does not guarantee distributed consistency, recovery from manual storage edits, or anything beyond the durability model of the chosen backing store.

If something needs to plan, decide, merge, message, or repair, that is outside Covey.

## Observability

There are three intended observation surfaces:

- `event_log` for subscribers
- point-in-time status queries for sessions, subtasks, and meta-tasks
- structured process logs with operation name, session, duration, result, and affected objects

The database is authoritative. Logs are for diagnosis.

## Repository Shape

This crate should stay centered on the core coordination library. Wrapper-specific processes can embed it or expose it over RPC later, but the schema, invariants, and transactional behavior are the real product.

The current internal module map and dependency boundaries are documented in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Codex Integration

Codex lifecycle hooks for Covey live under `../integrations/codex-hooks/`.
Install them into a target repository with that template's installer script.

Covey and its CLI remain transport-thin by design. The
`integrations/codex-hooks/` template is a separate local execution wrapper that
adapts Codex lifecycle and tool events into existing Covey CLI commands plus
`mutai-rs` evidence contracts. Hooks may enforce before local Codex side
effects, but they must not become scheduler, settlement, landing, or repoops
authority. Covey remains the owner of `claim-next`, queue `claim-next`, claims,
leases, fences, sessions, reservations, artifacts, reviews, review/apply queue
state, events, conflicts, and lifecycle transitions.

## Verification

The minimum verification path for this repo is:

```bash
cargo test
```

The design is only credible if the test suite proves the invariants above: legal and illegal state transitions, uniqueness constraints, fence checks, reservation overlap behavior, atomicity, and concurrency smoke.

## Local CLI

Covey now ships a thin local CLI for direct SQLite-backed testing and exploratory work:

```bash
cargo run --bin covey -- --db ./covey.db session register \
  --agent-principal-id agent-a \
  --agent-instance-id run-1 \
  --role executor
```

The CLI is transport-thin by design:

- it maps directly onto the embedded `Covey` API
- it defaults to JSON when stdout is not a TTY
- it returns stable exit codes for success, not-found, invalid-args, permission, conflict, and internal errors

Short help:

```bash
cargo run --bin covey -- --help
```

Typical workflow:

```bash
# Register sessions
cargo run --bin covey -- session register --agent-principal-id orch --agent-instance-id orch-1 --role orchestrator
cargo run --bin covey -- session register --agent-principal-id worker --agent-instance-id worker-1 --role executor

# Create work
cargo run --bin covey -- meta submit --session-token <orch-session> --prompt-text "ship feature"
cargo run --bin covey -- subtask create --session-token <orch-session> --meta-task-id <meta-task> --title "implement" --kind work --priority 1

# Import OpenSpec planning artifacts without claiming or scheduling work
cargo run --bin covey -- import openspec \
  --change openspec-covey-importer \
  --project-root /path/to/project \
  --dry-run
cargo run --bin covey -- import openspec \
  --change openspec-covey-importer \
  --project-root /path/to/project \
  --session-token <orch-session>

# Claim and publish
cargo run --bin covey -- subtask claim-next --session-token <worker-session> --lease-duration-ms 30000
cargo run --bin covey -- artifact publish --session-token <worker-session> --claim-id <claim> --fence-seq <fence> --artifact-digest sha256:a --artifact-kind patch-bundle --base-rev base --manifest-path artifact.json --changed-paths-digest sha256:paths
```

### OpenSpec Import

`covey import openspec` maps one OpenSpec change into deterministic Covey task state. For
Better Droid changes, run `better-droid compile <change-id>` first so the change contains the
compiled mission packet set under `openspec/changes/<change-id>/mission/`.

- `openspec:<change-id>` becomes the meta task ID.
- `openspec:<change-id>:<task-id>` becomes each work subtask ID.
- `--dry-run` validates compiled mission artifacts, diffs, and emits the JSON summary without writing records or appending events.
- Write mode requires an active orchestrator `--session-token`.
- Imported subtasks remain `available`; the importer does not claim work, schedule workers, enqueue apply work, mutate OpenSpec files, or touch mutAI runtime state.
- Active claimed subtasks conflict instead of being silently rewritten when their compiled task digest changes.

The importer reads `proposal.md`, `design.md`, `tasks.md`, `specs/*/spec.md`, and the Better
Droid mission artifacts: `mission.json`, `traceability.json`, `validation.json`,
`path-policy.json`, `review-rubric.json`, `assumptions.json`, and `compile-report.json`.
`compile-report.json` must be `ready`, `import_ready: true`, and bind source, artifact, and
compiled task digests.

## Project Standard

Covey should stay boring. If it becomes interesting, it is probably taking on responsibilities that belong in the orchestrator, the apply gate, or the execution wrapper.

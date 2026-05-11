# Covey Architecture

Covey is a small, correctness-critical coordination substrate. The architecture is intentionally boring: one authoritative transactional store, one typed library API, and a thin CLI boundary for local operation.

## Design Rules

- The SQLite schema and transaction checks are the source of truth.
- Domain invariants must be encoded in schema constraints, typed request/response models, or validator functions.
- JSON is a transport format at the boundary, not the internal data model.
- The CLI must stay transport-thin and map directly onto the library API.

## Module Map

### Core library

- `src/lib.rs`
  Re-export surface for the crate. This is the public API entrypoint.
- `src/model.rs`
  Typed domain records, request DTOs, status payloads, event payloads, and persisted-state enums.
- `src/error.rs`
  Typed domain errors for invalid transitions, ownership violations, lease failures, and storage issues.
- `src/schema.rs`
  Forward-only migrations, SQLite pragmas, and event-log append helpers.
- `src/queries.rs`
  Read-side row loading and deserialization helpers.
- `src/validators.rs`
  Transaction-scoped invariant checks and transition guards.
- `src/overlap.rs`
  Reservation overlap normalization, lookup, and conflict recording logic.
- `src/store.rs`
  Connection ownership, transaction helpers, idempotency persistence, and shared mutation utilities. This file should not grow back into the full API surface.

### Operation modules

All library mutations and read workflows hang off `Covey`, but the implementation is split by domain under `src/ops/`.

- `ops/session.rs`
  Session register, heartbeat, exit, and status logic.
- `ops/meta_task.rs`
  Meta-task submission, cancellation, and meta-task status.
- `ops/workflow.rs`
  Subtask creation, claim lifecycle, artifact publication, review lifecycle, and workflow status queries.
- `ops/queue.rs`
  Ready-queue enqueue, claim, apply, supersede, and metrics logic.
- `ops/reservation.rs`
  Reservation lifecycle, overlap lookup, event listing, and conflict resolution.
- `ops/maintenance.rs`
  Lease-based cleanup and stale-session/claim/reservation maintenance.
- `ops/import/openspec.rs`
  OpenSpec-to-Covey import. Better Droid changes are imported from compiled mission JSON artifacts
  and import-owned provenance only; this module must not claim work, schedule workers, enqueue apply
  work, mutate OpenSpec source, or become a settlement authority.

### CLI boundary

- `src/bin/covey.rs`
  Process startup plus clap command and argument definitions.
- `src/bin/dispatch_support.rs`
  CLI-to-library dispatch. This translates parsed clap args into typed `Covey` requests and typed success acks.
- `src/bin/render_support.rs`
  Output-mode selection, human/json rendering, structured error reporting, and typed CLI envelope payloads.

## Dependency Direction

The intended dependency flow is:

`bin/* -> lib re-exports -> ops/* -> validators/queries/schema/store -> SQLite`

Important constraints:

- `render_support.rs` must not acquire domain logic.
- `dispatch_support.rs` must not manually encode ad hoc JSON payloads for typed operations.
- `ops/*` may share helpers through `store.rs`, but business logic should stay in the domain module that owns it.
- `model.rs` and `error.rs` define the language of the system. Other modules should depend on those types rather than invent parallel wire shapes.

## What To Avoid

- Reintroducing a monolithic `store.rs` that mixes connection plumbing with every operation.
- Adding workflow logic directly to the CLI layer.
- Using `serde_json::Value` as a substitute for request/response structs in production paths.
- Hiding state-machine rules in comments instead of validators or type shapes.

## Verification Expectations

Architectural changes in Covey should usually prove all of the following before being considered done:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

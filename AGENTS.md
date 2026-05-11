# Covey AGENTS.md

This file adds Covey-specific binding rules on top of the parent instructions in [../AGENTS.md](/data/projects/acfs-hive/AGENTS.md) and [../CLAUDE.md](/data/projects/acfs-hive/CLAUDE.md). The parent rules still apply unless this file narrows them for this Rust crate.

These rules are mandatory for agents working in this repository. Treat this file as the crate policy and dependency rulebook, not as informal guidance.

## Rust Crate Defaults

When adding new dependencies, agents must use the crate defaults below unless there is a concrete, written reason not to:

- SQLite access: `rusqlite`
- Forward-only schema migrations: `rusqlite_migration`
- Serialization: `serde` and `serde_json`
  `serde_json` is for boundary serialization/deserialization, fixtures, and genuinely dynamic payloads, not for manually encoding typed application state.
- CLI parsing: `clap`
- Typed errors: `thiserror`
- Error reporting at binary/app boundaries: `eyre` or `color-eyre`
- Structured logging: `tracing`
- Log/filter setup and formatting: `tracing-subscriber`
- Content hashing and digests: `blake3`
- Byte buffers and efficient byte handling: `bytes`
- SQLite row mapping helpers once query/result mapping boilerplate becomes material: `serde_rusqlite`
- Stable identifiers: `uuid`
- Enum display/parsing and variant helpers: `strum` and `strum_macros`
- Numeric traits and derive helpers: `num-traits` and `num-derive`
- Compile-time invariant checks: `static_assertions`
- Boilerplate-reducing derives and conversions: `derive_more`
- Lightweight constructors for request and DTO types: `derive-new`
- Builders for complex request construction: `type_state_builder` or `derive_builder`
- Delegated wrapper methods: `delegate`
- Futures and async combinators: `futures`
- Property testing: `proptest`
- Fixture-based and table-style tests: `rstest`
- Temporary files and databases in tests: `tempfile`
- Model checking for protocol/state verification: `stateright`

These are the sanctioned crate choices for Covey. Do not introduce parallel alternatives casually.

## Error Handling

- Agents must use `thiserror` for typed domain and API errors.
- Agents must use `eyre` or `color-eyre` only at executable boundaries when richer reports or operator-facing diagnostics are needed.
- Agents must keep `color-eyre` out of the core library surface and use it only for binaries and operator-facing entrypoints.
- Do not introduce multiple overlapping error stacks without a specific reason.

## Testing Conventions

- Agents must use `proptest` for invariants, transition rules, and other correctness-critical properties.
- Agents must use `rstest` for fixture-driven tests.
- If a test needs fixtures, parameterized setup, or case tables, agents must use `rstest` rather than ad hoc helpers.
- Agents must use `tempfile` for fresh per-test SQLite databases and other disposable filesystem fixtures.
- Agents must use `stateright` when the state machine or protocol model needs explicit model-check verification.
- Agents should run narrow unit/package verification first, then `cargo test` for repository-level verification when the change warrants it.

## Async And Data Types

- Agents must use `blake3` for hashing and digest computation.
- If the code needs explicit byte-oriented data structures or APIs, agents must use `bytes` rather than ad hoc `Vec<u8>` wrappers.
- Agents must use `strum` and `strum_macros` for enum display, parsing, variant inspection, and string conversion helpers.
- If the code needs numeric trait derivations or related helper macros, agents must use `num-traits` plus `num-derive`.
- If the code needs real async composition, streams, sinks, or combinators, agents must use `futures` rather than hand-rolled abstractions.

## Declarative Data Modeling

- Covey uses a declarative, strongly typed Rust style.
- JSON is a wire format, not an internal programming model.
- Agents must model request/response payloads, persisted records, and protocol states as Rust structs/enums with derived `Serialize`/`Deserialize` where applicable.
- Agents must prefer domain types, newtypes, enums, and typed builders over ad hoc `serde_json::Value` manipulation.
- Agents must not manually assemble structured payloads with `serde_json::json!`, raw `Value` maps, or string-built JSON when a concrete Rust type can represent the shape.
- Agents must encode invariants in the type system when practical: required fields, legal states, and validated conversions should live in types, not in scattered JSON assembly logic.
- Agents should use `From`/`TryFrom` or dedicated conversion functions to translate between domain types and wire DTOs at the boundary.
- `serde_json::json!` is allowed only for tests, debugging, fixtures, or truly dynamic pass-through payloads where the schema is unknown at compile time.
- If an agent uses untyped JSON in production code, the change must explain why a typed representation is not practical.

## Persistence And Invariants

- Agents must use `rusqlite_migration` for schema versioning and forward-only migrations.
- Agents should adopt `serde_rusqlite` once query/result mapping boilerplate becomes material.
- Agents must use `static_assertions` for compile-time guarantees that are worth enforcing outside runtime tests.
- Agents must use `uuid` for externally visible identifiers when the code needs generated stable IDs.

## Boilerplate Reduction

- Agents should use `derive-new` for lightweight constructors on request, status, and DTO-style types.
- If request construction becomes complex enough to need builders, agents must choose one builder crate consistently: prefer `type_state_builder` for compile-time required-field enforcement, or `derive_builder` if a simpler conventional builder is sufficient.
- Agents should use `delegate` when wrapper types would otherwise accumulate repetitive pass-through methods.
- Agents must prefer `strum`-based derives for persisted state enums, role enums, and other string-backed protocol enums instead of hand-written conversion code.

## Dependency Discipline

- Agents must treat the crate set above as the default allowed stack.
- If an agent chooses a different crate, the reason must be documented in the change summary or review context.
- Agents must not add dependencies that overlap heavily with the sanctioned set unless they remove a real limitation.

<!--
Mission-grade completion rule:
Before review, compile, import, or archive, this file must not contain unresolved placeholders such as `<...>`, `TBD`, `TODO`, `later`, `as needed`, `etc.`, or empty required sections.
Use `N/A - <specific reason>` only when a field is genuinely not applicable.
Do not satisfy sections with generic statements. Include concrete project-relative paths, exact commands, named artifacts, requirement/scenario IDs, owner/decision sources, or explicit rationale.
Importable autonomous work must fail readiness review if required fields are placeholder, vague, or prose-only.

Evolution hardening rules after 10-pass critique:
- `N/A` is valid only when it names the waived field, the concrete reason, the authority/decision source, affected requirement/task IDs, and whether the waiver blocks import, execution, review, apply, or archive.
- Evidence must use a concrete evidence form: command + cwd + expected exit/status/output, file path + line/section + finding, artifact path + digest, review finding bound to source/artifact digest, quoted issue/task/spec decision, or explicit `manual-review` with reviewer role and inspected artifacts.
- Generic evidence phrases such as `verified`, `confirmed`, `checked`, `reviewed`, `looks good`, or `manual validation` are invalid unless paired with the concrete evidence form above.
- Placeholder substitutes are invalid in importable artifacts unless followed by exact paths, commands, IDs, or measurable criteria: `various`, `relevant`, `appropriate`, `existing behavior`, `implementation details`, `normal flow`, `standard tests`, `all needed`, `misc`, `cleanup`, `refactor`, `improve`, `support`, `handle`, `update`, `fix issues`, `as applicable`, `as appropriate`, `where needed`, `if necessary`, `related files`.
- Traceability is valid only when bidirectional: every requirement maps to scenarios, every scenario maps to tasks or explicit deferral, every executable task maps to requirements/scenarios, and every validation item maps to the IDs it proves.
- Reviews and apply decisions must bind to exact source revision/digest, artifact/diff digest, validation evidence IDs, reviewer identity/role, independence status, and verdict.
- Source freshness must be explicit before import/apply: base ref/revision/digest, inspected-at time or command, and revalidation command.
- Autonomous work must fail closed on missing source digest, broad write paths, unresolved high/critical assumptions, missing independent review, stale source, missing validation evidence, or dirty unrelated path overlap.
-->

## Context

<!--
Ground the design in current repo reality.
Include source files, existing specs/changes, commands, docs, APIs, and observed failure modes inspected before writing.
Separate verified current behavior from desired future behavior.
-->

## Goals / Non-Goals

**Goals:**
<!-- What this design must achieve. -->

**Non-Goals:**
<!-- What this design explicitly will not do. -->

## Current State and Evidence

<!--
Every current-state claim must use one of these forms:
- File evidence: `<path>:<line or section>` says/does <claim>
- Command evidence: `<command>` from `<cwd>` -> <observed result>
- Spec evidence: `<spec path>#<requirement>` -> <claim>
- Assumption: <claim>, owner=<owner>, blocking=<yes/no>
Do not write architecture claims from memory when source material exists.
-->

## Design Decisions

<!--
For each decision, include decision, rationale, alternatives rejected, and consequences.
-->

### Decision 1: <name>

- **Decision:** <chosen approach>
- **Rationale:** <why this approach>
- **Alternatives considered:** <other options and why rejected>
- **Consequences:** <trade-offs and follow-up work>

## Component Responsibilities

<!--
Define ownership. Keep boundaries narrow and avoid dual writers.
-->

| Component | Owns | Must Not Own |
|---|---|---|
| OpenSpec | Planning artifacts and archived source-of-truth specs | Live claims, leases, scheduling, reviews, apply queue, runtime authority, settlement |
| Better Droid compile/lint | Mission packet projections, semantic readiness checks | Live worker state, task ownership, runtime dispatch, settlement |
| Covey | `claim-next`, queue `claim-next`, claims, leases, fences, sessions, reservations, artifacts, reviews, review/apply queue state, events, conflicts, lifecycle transitions | Product planning, model dispatch, patch application, Authority verdicts |
| `mutai-rs` Authority | Typed admission, repoops legality, apply-gate, landing/settlement, mission/runtime verdicts and evidence for one Covey-selected attempt | OpenSpec authoring, markdown parsing in settlement authority, next-work selection, Covey lifecycle transitions |
| Executors | Local implementation, debugging, tests, artifact publication under claim | Authoritative task state, apply-gate mutation unless acting as apply gate |
| git/CI | Diffs, commits, reproducible proof, remote containment | Planning or live task ownership |

## Interface Contracts and Handoffs

<!--
Define exact handoffs. Do not rely on prose-only ownership.
Every handoff should fail closed when required fields, digests, claims, approvals, or evidence are missing.
-->

| Producer | Consumer | Artifact / API | Required Fields | Evidence Binding | Failure Mode | Must Not Do |
|---|---|---|---|---|---|---|
| <component> | <component> | <artifact/API> | <fields> | <source/artifact digest, command evidence, reviewer verdict> | <fail-closed behavior> | <forbidden behavior> |

### Fail-Closed Rules
- Missing source revision/digest → block import/apply until source freshness is recorded.
- Missing artifact/diff digest → block review/apply.
- Missing exact validation evidence → block approval.
- Broad write path or dirty unrelated path overlap → block autonomous execution.
- Unresolved high/critical assumption → block import/execution/apply until approved.
- <condition> → <blocked/rejected behavior>

### Authority / Settlement Boundaries
- **Authority Owner:** <OpenSpec | Better Droid compile/lint | Covey | Authority | executor | git/CI>
- **Settlement Authority:** Authority may emit typed apply-gate, landing, reconcile, rollback, and runtime settlement evidence for one Covey-selected attempt; planning artifacts and templates may specify evidence requirements but must not settle claims.
- **No-Dual-Writer Invariant:** exactly one writer is named for task state, claim state, leases/fences, review/apply queue metadata, pending-commit state, and settlement evidence. Covey owns live lifecycle state; `mutai-rs` owns typed verdict/evidence packets.
- **Runtime Authority Restriction:** workers execute only through assigned Covey claim/dispatch context; they must not self-assign leases, mutate scheduler state, choose next work, or invent live coordination state.

## Threat Model / Abuse Cases

<!--
Required for autonomous execution, importer, scheduler, runtime, apply, settlement, security, persistence, path-policy, or generated artifact changes.
Describe how this design can fail or be misused.
-->

| Abuse / Failure Case | Impact | Prevention | Detection / Evidence | Residual Risk |
|---|---|---|---|---|
| <case> | <impact> | <control> | <observable evidence> | <risk> |

### Required Safety Properties
- <property that must always hold>

## Assumption Ledger

<!--
Track every assumption that affects implementation, validation, import, review, or apply safety.
High/critical blocking assumptions must be resolved before autonomous import/execution.
-->

| ID | Assumption | Evidence or Reason | Owner / Decision Source | Blocking? | Expires / Stale If | Linked Refs |
|---|---|---|---|---|---|---|
| A-001 | <assumption> | <evidence/reason> | <owner/source> | <yes/no> | <condition> | <REQ/SCN/task IDs> |

## Artifact and Data Shapes

<!--
Describe every artifact emitted or consumed.
For each artifact, include:
- schema identifier
- producer and consumer
- required fields
- optional fields and absence semantics
- canonicalization/digest rules
- validation rules
- consumer-impact behavior
- data-movement behavior for existing artifacts
Mission-grade OpenSpecs must cover mission, traceability, validation, path policy, review rubric, assumptions, context, and compile report when relevant.
-->

| Artifact | Schema Identifier | Producer | Consumer | Required Fields | Digest / Canonicalization | Consumer Notes |
|---|---|---|---|---|---|---|
| <artifact> | <schema-id> | <producer> | <consumer> | <fields> | <rules> | <notes> |

## Lifecycle / State Flow

<!--
Describe the lifecycle from authoring through validation, import, execution, review, apply, verification, and archive.
State what invalidates downstream state when OpenSpec source changes.
-->

## Source Freshness / Import Readiness

<!-- Required for imports, worker dispatch, review, apply, and archive. -->
| Source | Path / URI | Revision | Digest | Imported / Inspected By | Stale If | Revalidation Command |
|---|---|---|---|---|---|---|
| <source_kind> | <path> | <rev> | <digest> | <actor/tool> | <condition> | `<command>` |

### Active Claim / Review / Queue Invalidation
- If source digest, acceptance criteria, path scope, validation commands, dependency graph, or requirement text changes after dispatch, derived claims/reviews/queue entries must be marked stale or forced through explicit revalidation.
- Workers must stop mutation when their claim context is stale, expired, or fence-invalid.
- Apply gate must not apply artifacts reviewed against stale OpenSpec source unless revalidated and recorded.

## Path Policy

<!--
Define intended read/write/forbidden path semantics if autonomous workers will use this change.
Use normalized project-root-relative paths.
Write paths must be exact files unless a narrow glob is justified by generated output or an approved mechanical rename.
Include how paths are normalized and what happens on overlap or dirty unrelated files.
Protected paths must be listed as forbidden when not explicitly in scope. Examples for this repo family include:
- `authority/**`
- `contracts/imported/**`
- unrelated dirty paths
- runtime/production state
- generated artifacts not owned by this change
-->

| Path | Access | Reason | Requirement / Task IDs | Conflict Behavior |
|---|---|---|---|---|
| <project-relative path> | <read/write/generated/forbidden> | <why needed> | <REQ/SCN/task IDs> | <deny/review/approve with source> |

## Traceability Matrix

<!--
Every behavioral requirement/scenario must map to implementation, validation, and review.
Use explicit deferral rows when not implemented. Do not use broad refs like "all", "misc", or "proposal" except for final verification rows with rationale.
-->

| Requirement / Scenario | Task IDs | Paths / Artifacts | Validation Evidence | Review Evidence | Status / Deferral |
|---|---|---|---|---|---|
| <REQ/SCN ID> | <task IDs> | <paths> | <evidence> | <review> | <planned/deferred/N/A reason> |

## Validation Strategy

<!--
List exact commands/manual evidence needed at each stage.
For code changes, prefer narrow tests first and widen only when needed.
For docs/schema changes, include schema validation and whitespace checks.
-->

### Evidence Requirements

Each validation item must include:
- command or manual action
- working directory
- expected exit code or observation
- covered requirement/scenario IDs
- covered task IDs
- required stdout/stderr summary or artifact path
- revision/base context
- whether the check must fail before implementation where practical

| ID | Phase | Command / Manual Action | Working Directory | Expected Result | Covers | Evidence Capture |
|---|---|---|---|---|---|---|
| VAL-001 | <phase> | `<command>` | <cwd> | <expected result> | <REQ/SCN/task IDs> | <what to record> |

### Negative Validation
- <invalid/stale/conflicting input> → <expected rejection/blocker and evidence>

## Review Strategy

<!--
Define what reviewers must inspect and what blocks approval.
Reviews should bind to exact artifacts or diffs, not narrative summaries.
Reviewer must be distinct from implementer or record why independent review is unavailable.
Review evidence must include source revision/digest, artifact/diff digest, validation evidence IDs, reviewer identity/role, independence status, waiver reason if not independent, and verdict.
-->

### Approval Blockers
- Missing source digest or stale source.
- Missing artifact/diff digest.
- Missing exact validation evidence.
- Broad write paths or dirty unrelated path overlap.
- Unresolved high/critical assumptions.
- Missing independent review for autonomous/apply-gated work.
- Traceability gaps or generic refs.
- Evidence-only narrative from implementer.

## Concurrency, Idempotency, and Conflict Handling

<!--
Required for import, runtime, claim, review, apply, persistence, and generated artifact changes.
-->

| Operation | Idempotency Key / Digest | Concurrent Conflict | Expected Resolution | Evidence |
|---|---|---|---|---|
| <operation> | <key/digest> | <conflict> | <resolution> | <evidence> |

### Retry Safety
- <operation> can be retried safely because <reason>
- <operation> must not be retried automatically because <reason>

## Invalidation Matrix

<!--
Tie source changes to downstream stale state. Do not let stale claims, reviews, or queue entries remain silently valid.
-->

| Source Artifact Changed | Invalidates Mission Packet? | Invalidates Claims? | Invalidates Artifacts? | Invalidates Reviews? | Invalidates Apply Queue Entry? | Required Recovery Action |
|---|---|---|---|---|---|---|
| <source> | <yes/no> | <yes/no> | <yes/no> | <yes/no> | <yes/no> | <action> |

## Stale Source / Reimport / Invalidation

<!--
Explain what happens when proposal/design/tasks/specs change after import, claim, artifact publication, review, or apply queue entry.
-->

## Implementation Readiness Blockers

<!--
List conditions that make implementation/import unsafe. Blocking open questions MUST be resolved before implementation tasks are considered ready.
-->
- <blocker or N/A - reason>

## Risks / Trade-offs

<!-- Format: [Risk] → Mitigation. Include operational, security, consumer-impact, resource, and data-movement risks. -->

## Final Evidence Packet

<!-- Required before completion/archive for non-trivial or autonomous work. -->
| Field | Required Value |
|---|---|
| Mission / Change ID | <id> |
| Executor | <identity/role> |
| Reviewer | <identity/role/independence> |
| Base Revision / Source Digest | <rev/digest> |
| Result Revision / Artifact Digest | <rev/digest> |
| Commands Run | <validation IDs and summaries> |
| Changed Files | <exact paths> |
| Deviations From Plan | <none or list> |
| Residual Risks | <none or list> |
| Apply / Settlement Decision | <decision and authority> |

## Rollout / Rollback

<!--
How to roll out safely. How to undo or disable if needed.
For source-only/schema-only/tooling-only changes, state why no runtime data movement is needed and list consumers that may need reimport/regeneration.
-->

## Open Questions

<!--
For each unresolved decision, include:
- owner/decision source
- blocking: yes/no
- default if unresolved
- affected requirement/task IDs
-->

| Question | Owner / Decision Source | Blocking? | Default if Unresolved | Affected Refs |
|---|---|---|---|---|
| <question> | <owner/source> | <yes/no> | <default> | <REQ/SCN/task IDs> |

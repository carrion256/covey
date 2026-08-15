<!--
Mission-grade completion rule:
Before review, compile, import, or archive, this file must not contain unresolved placeholders such as `<...>`, `TBD`, `TODO`, `later`, `as needed`, `etc.`, or empty required sections.
Use `N/A - <specific reason>` only when a field is genuinely not applicable.
Do not satisfy requirements with generic statements. Include concrete project-relative paths, exact commands, named artifacts, requirement/scenario IDs, owner/decision sources, or explicit rationale.
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

## ADDED Requirements

### Requirement: <!-- REQ-<capability>-<slug>: stable requirement name -->
<!--
Use SHALL/MUST language. State externally observable behavior, not implementation details.
Every requirement MUST include a stable requirement ID in prose or heading, e.g. `REQ-<capability>-<slug>`.
Every scenario MUST include a stable scenario ID in prose or heading, e.g. `SCN-<capability>-<slug>`.
Explain what is required, who/what triggers it, what evidence can prove it, and what must not happen.
Do not leave placeholder headings or generic names such as "Requirement 1", "Happy path", or "works".

Each requirement must include:
- at least one positive scenario
- at least one negative/error/unauthorized/stale/conflict scenario OR `Negative cases: N/A - <specific reason and approval/decision source>`
- observable evidence for each scenario
- rejecting component and unchanged state for each negative/error/stale/unauthorized scenario
- traceability refs linking requirement → scenario → task → validation evidence

For each requirement, consider and either cover or explicitly waive:
- success path
- invalid input
- stale source/reimport
- permission/path violation
- idempotency/retry
- data movement and consumer impact
- no-partial-authoritative-write behavior
- dirty unrelated path protection
- source freshness/import invalidation behavior
-->

#### Scenario: <!-- SCN-<capability>-<slug>: positive scenario name -->
- **GIVEN** <!-- relevant precondition or existing state -->
- **AND Authority Boundary** <!-- component that owns the decision/state and components that must not mutate it -->
- **WHEN** <!-- exact trigger/action/event -->
- **THEN** <!-- exact expected result -->
- **AND Evidence** <!-- validation command/action, cwd, expected status/output, artifact/event/digest/review, or user-visible result -->
- **AND Traceability** <!-- linked task IDs and validation evidence IDs -->
- **AND Safety** <!-- no forbidden state mutation, no partial authoritative write, dirty unrelated paths untouched, or full N/A waiver -->

#### Scenario: <!-- SCN-<capability>-<slug>: required negative/error/stale/unauthorized scenario name -->
- **GIVEN** <!-- invalid, stale, conflicting, unauthorized, or missing condition -->
- **AND Authority Boundary** <!-- component that owns rejection/recovery and components that must not mutate state -->
- **WHEN** <!-- trigger/action/event -->
- **THEN** <!-- expected rejection, warning, blocker, or recovery behavior -->
- **AND Evidence** <!-- observable evidence that the failure is visible, including command/artifact/event/review binding -->
- **AND Unchanged State** <!-- exact state/path/queue/claim/artifact/review/settlement state that must remain unchanged -->
- **AND Traceability** <!-- linked task IDs and validation evidence IDs -->
- **AND Safety** <!-- no partial authoritative state was written, dirty unrelated paths untouched, or exact rollback/recovery behavior -->

<!--
Use additional delta sections as needed. Preserve heading text exactly.

## MODIFIED Requirements

### Requirement: <existing requirement name exactly>
<Copy the full existing requirement block from openspec/specs/<capability>/spec.md, then edit it. Do not include partial modified requirements.>
<Include consumer impact, data-movement expectations, and stale/imported-work behavior when behavior changes affect autonomous execution.>

## REMOVED Requirements

### Requirement: <removed requirement name exactly>
**Reason**: <why it is removed>
**Forward Path**: <how users/operators/systems move forward>
**Safety / Consumer Impact**: <what prevents stale workers, imports, artifacts, or users from relying on removed behavior>

## RENAMED Requirements

- FROM: <old requirement name>
- TO: <new requirement name>
- MIGRATION: <how traceability, tasks, artifacts, and existing references are updated>
-->

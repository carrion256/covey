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

Planning-class rule:
- Agents must not create `openspec/changes` entries for phase plans, milestone sequencing, strategy, or discovery maps. Planning belongs in `docs/plans/`, operator notes, or a human-authored architecture document outside `openspec/changes`.
- Every agent-authored OpenSpec change must be executable work: `.openspec.yaml` must use `planning_class: work_packet`, and the change must contain concrete implementation/test/review tasks with exact path scope and validation evidence.
- If the operator asks for a new `*-implementation-slice` or says "real next implementation step", create a `planning_class: work_packet` execution change with concrete implementation/test tasks even when the request is grouped under "Phase N". Do not rewrite that request into planning source or retarget older phase/roadmap notes.
-->

## Why

<!--
Explain the motivation for this change.
- What observed failure mode, user need, or strategic gap does this solve?
- Why now?
- What happens if this remains unfixed?
-->

## Evidence and Assumptions

### Verified Evidence
<!--
List concrete evidence inspected before proposing the change.
Use file paths, commands, docs, logs, issue IDs, or observed behavior.
Every factual claim that affects implementation scope should be grounded here or moved to Assumptions.
-->
- <evidence source> → <finding>

### Source Freshness / Provenance
<!--
Required before autonomous import or apply.
- **Repository / Project Root:** <path>
- **Base Ref / Revision:** <git ref and commit, or N/A - field/reason/source/refs/blocking>
- **OpenSpec Source Digest:** <digest or command to produce digest>
- **Inspected At / Command:** <timestamp or exact command evidence>
- **Revalidation Before Import/Apply:** <exact command/action>
- **Stale If:** <source/digest/requirements/tasks/path-policy/evidence changes>
-->

### Assumptions
<!--
List assumptions not directly proven by inspected evidence.
High/critical assumptions must have an approval source or block import/readiness.
Every assumption must have ID, owner/source, stale-if, linked refs, and blocking classification.
-->
- **A-001:** <assumption>
  - **Risk if wrong:** <impact>
  - **Risk Level:** <low | medium | high | critical>
  - **Approval / Revisit Source:** <owner, task ID, decision source, or N/A - reason>
  - **Blocks Import / Execution:** <yes/no and why>

## What Changes

<!--
Describe the change as concrete capability deltas.
- New behavior or artifacts
- Modified behavior or artifacts
- Removed/deprecated behavior, if any
- Data movement, consumer impact, or disabled-path handling, if any
Mark breaking changes with **BREAKING**.
Avoid verbs like improve/support/handle/update/refactor unless paired with observable acceptance criteria and validation evidence.
-->

## Capabilities

### New Capabilities
<!--
Capabilities being introduced. Replace <name> with kebab-case identifiers.
Each capability listed here needs a matching specs/<name>/spec.md delta file.
-->
- `<name>`: <brief description of the behavior or artifact contract>

### Modified Capabilities
<!--
Existing capabilities whose REQUIREMENTS are changing.
Use exact names from openspec/specs/. Leave empty only with `N/A - no existing capability requirements change`.
-->
- `<existing-name>`: <what requirement is changing and why>

## Capability / Spec Delta Map

<!--
This map prevents proposal/spec drift. Every capability above must appear here, and every delta spec file must map back to this table.
-->
| Capability | Delta File | Delta Type | Linked Requirement IDs | Reason |
|---|---|---|---|---|
| <capability> | `specs/<capability>/spec.md` | <ADDED/MODIFIED/REMOVED/RENAMED> | <REQ-* IDs or planned IDs> | <why this delta exists> |

## Scope

### In Scope
<!-- Specific outcomes, files/areas, commands, docs, or behaviors this change covers. -->

### Out of Scope / Non-Goals
<!-- Explicitly say what this change will not do. Include tempting but rejected expansions. -->

## Authority Boundaries

<!--
State relevant ownership boundaries. Keep or adapt the bullets that apply.
Do not let a proposal grant runtime authority to OpenSpec work-packet artifacts.
-->
- OpenSpec owns executable work-packet artifacts only: proposal, design, tasks, and behavioral specs.
- Better Droid may compile mission packets from OpenSpec source, but compiled packets are projections, not a second authoring authority.
- Covey owns live task coordination: `claim-next`, queue `claim-next`, subtasks, claims, leases/fences, sessions, reservations, artifacts, reviews, review/apply queue metadata, events, conflicts, and lifecycle transitions.
- Authority evaluates one Covey-selected claim, apply attempt, runtime attempt, or repoops preflight at a time and emits typed verdict/evidence packets. It does not choose next work or own lifecycle transitions.
- `mutai-rs` evaluates one Covey-selected claim, apply attempt, runtime attempt, or repoops preflight at a time.
- Executors perform local work under claims and must not self-certify completion.
- git/CI own committed proof.
- This change must not introduce OpenSpec-owned runtime leases, worker scheduling, reviews, apply queues, settlement authority, or Authority lifecycle ownership.
- This change must not introduce dual writers over task, claim, runtime, settlement, or landing state.

## Risk and Approval

- **Risk Level:** <low | medium | high | critical>
- **Risk Drivers:** <security | runtime | data movement | data loss | apply/landing | settlement | consumer impact | autonomous execution | other>
- **Human Approval Required Before Import:** <yes/no and why>
- **Human Approval Required Before Apply/Landing:** <yes/no and why>
- **Rollback / Disable Strategy Required:** <yes/no and why>

### Safety Invariants
<!-- Invariants that must remain true after this change. -->
- <invariant>

## Impact

<!--
Affected code, specs, docs, CLIs, schemas, tests, workflows, APIs, data stores, or operational runbooks.
Call out whether this is source-only, tooling-only, runtime-affecting, data-moving, or apply/landing-affecting.
If source-only/schema-only/tooling-only is claimed, state why no runtime data movement is needed and which consumers may need reimport/regeneration.
-->

## Mission Readiness Expectations

<!--
This section must be complete before autonomous execution or Covey import.
If a field is not applicable, write `N/A - <specific reason>`.
-->

### Mission Objective
- **Objective:** <one-sentence outcome workers must achieve>
- **Done Definition:** <observable end state, including required evidence>
- **Autonomy Level:** <read-only | human-assisted | autonomous-executable | apply-gated>
- **Import Readiness:** <planning_ready | planning_ready_blocked | covey_import_ready | implementation_ready>
- **Planning Class:** work_packet; agent-authored OpenSpec changes must be executable work, not phase, roadmap, discovery, or strategy source.

### Readiness Gates
- [ ] `openspec validate <change-id> --type change --strict` passes.
- [ ] Every executable task has exact allowed write paths or is explicitly read-only.
- [ ] Every executable task has exact validation/evidence obligations.
- [ ] Every requirement/scenario is linked to at least one task, validation check, or explicit deferral.
- [ ] Assumptions are listed and high/critical assumptions have an approval source.
- [ ] Security, consumer-impact, data-movement, and rollback impacts are addressed or explicitly `N/A - <specific reason>`.
- [ ] Review/apply boundaries preserve Covey, mutAI, git/CI, and OpenSpec ownership boundaries.
- [ ] No unresolved placeholders, placeholder substitutes, broad write paths, generic traceability refs, or vague executable tasks remain.
- [ ] Source freshness/provenance is recorded and stale-source behavior is defined.
- [ ] Every `N/A` waiver includes field, reason, authority source, linked refs, and blocking impact.
- [ ] Traceability coverage is complete or incomplete coverage explicitly blocks import/archive.
- [ ] Review/apply evidence binds to exact source revision/digest, artifact/diff digest, validation evidence, reviewer identity, independence status, and verdict.

### Blocking Conditions
<!-- List any condition that must prevent compile/import/execution. -->
- <blocker or N/A - reason>

## Autonomous Execution Readiness

<!-- Required before Covey import, worker dispatch, review, apply, or archive. -->
- **Readiness Verdict:** <planning_ready | planning_ready_blocked | covey_import_ready | implementation_ready | execution_ready | apply_authorized>
- **Readiness Gate:** <planning_ready | planning_ready_blocked | covey_import_ready | implementation_ready | execution_ready | apply_authorized>
- **Maximum Mutation Mode:** <read-only | propose-only | write-sandbox | write-claimed-paths | apply-gated>
- **Required Human Approvals:** <approval source or N/A - field/reason/source/refs/blocking>
- **Approval Blockers Present:** <none | list with owner and unblock condition>
- **Traceability Coverage:** <complete | incomplete-blocks-import>
- **Uncovered Requirement/Scenario IDs:** <none | list with deferral/blocker reason>

## Success Criteria

<!--
Each criterion must be observable and evidence-bound.
Do not use subjective criteria such as "works well", "improved", or "updated".
-->
| ID | Criterion | Evidence Required | Owner / Source | Blocks Archive? |
|---|---|---|---|---|
| SC-001 | <observable result> | <command, artifact, review, digest, or manual evidence> | <owner/source> | <yes/no> |

### Negative / Safety Criteria
<!-- Required for runtime, security, data movement, schema change, apply, importer, scheduler, autonomous execution, or settlement changes. -->
- <unsafe behavior that must be rejected or prevented> → <required evidence>

## Review / Apply Evidence Binding

<!-- Required when this change can feed autonomous execution or apply-gated work. -->
| Evidence Item | Required Binding | Blocks Approval If Missing? |
|---|---|---|
| OpenSpec source | source revision/digest and changed files | yes |
| Artifact/diff | artifact path/digest or git diff/stat summary | yes |
| Validation | command/action IDs, cwd, exit/status/output summary | yes |
| Reviewer | identity/role, independent yes/no, waiver reason if no | yes for autonomous/apply work |
| Verdict | approved / changes-requested / blocked with findings | yes |

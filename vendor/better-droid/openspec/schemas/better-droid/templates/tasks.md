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


Better Droid task rules:
- Preserve checkbox syntax exactly: `- [ ] X.Y Task title`.
- Keep task IDs stable across re-imports. Do not renumber casually.
- Every executable task must include the detail block below.
- Task titles must be independently executable: action verb + target component/artifact + specific behavior/result + bounded scope.
- Importable work packets must be source-scoped. Do not group executable tasks as Phase 1/2/3, milestones, or broad sequencing lanes; put those plans in `docs/plans/` or operator notes, then create smaller `planning_class: work_packet` OpenSpec changes for execution.
- If the operator gives phase labels but explicitly asks for new implementation slices, treat each named slice as its own importable `planning_class: work_packet` candidate. Preserve the phase label only as context; do not create or edit planning-only changes instead of the requested execution slice.
- Work-packet dependencies must stay shallow and blocker-scoped. Rework follow-ups should address the exact review finding/source ref, not reopen a whole roadmap or phase.
- Discovery/review/verification tasks may mark unavailable fields as `none - <specific reason>`, but must say why using the full N/A waiver rule.
- Covey import may use checkbox IDs as deterministic subtask IDs, so vague titles become vague work packets. Fix the task, not the importer.
- Do not leave placeholder tokens or semantic placeholder substitutes in importable tasks.
- If `Allowed Write Paths`, `Validation / Evidence`, `Acceptance Criteria`, or `Traceability Refs` are `none`, classify the task as read-only-discovery, blocked-needs-human, or rejected-too-vague.
- Executable implementation, data-movement, schema-change, refactor, apply, and test tasks must not use `none` for acceptance criteria, validation/evidence, traceability refs, or stale-if.
- Allowed Write Paths must be exact project-relative files or narrow bounded globs. `.` `/` `repo` `**/*` `all files` and unconstrained package roots are invalid unless this is an approved repository-wide mechanical change with explicit risk approval and forbidden-path exclusions.
- Better Droid mission artifacts under `openspec/changes/<change>/mission/**` are compiler output and runner input, not executor work. Do not list them in `Allowed Write Paths` or task `Generated Paths`; use `none - Better Droid compile output is not a task deliverable` for verification-only tasks.
- Traceability refs must be specific requirement/scenario IDs such as REQ-* and SCN-*. Broad refs like `all`, `misc`, or `proposal` are invalid except for final verification tasks with rationale and exact covered IDs listed separately.
- Reviewers must be distinct from implementers or record why independent review is unavailable. Review cannot approve solely from implementer narrative.
- Apply tasks, if present, must name the authorized apply gate and must not grant executors unilateral merge/apply/settlement authority. If no apply is in scope, state `Apply: out of scope for this OpenSpec change`.
- Protected paths must be listed as forbidden when not explicitly in scope. Examples: `authority/**`, `contracts/imported/**`, unrelated dirty paths, runtime/production state, and generated artifacts not owned by this change.
-->

## 1. Discovery and Grounding

- [ ] 1.1 Inspect current implementation and source artifacts
  - **Type:** discovery
  - **Readiness:** <execution_ready | blocked-needs-human | read-only-discovery | rejected-too-vague>
  - **Authority Owner:** <OpenSpec | Better Droid compile/lint | Covey | `mutai-rs` Authority | executor | git/CI>
  - **Purpose:** Establish current behavior before design or implementation.
  - **Scope In:** <files, docs, commands, specs to inspect>
  - **Scope Out:** <areas intentionally not inspected>
  - **Dependencies:** none
  - **Preconditions:** <required source state, clean worktree expectations, approvals, or none - reason>
  - **Source Freshness / Revalidation:** <exact command/action proving source has not changed or N/A with full waiver>
  - **Dirty Worktree Protection:** inspect dirty/untracked paths before mutation; stop if unrelated dirty paths overlap required scope.
  - **Base Revision / Source Digest Expectations:** <git rev, OpenSpec source digest, artifact digest, or none - reason>
  - **Assumptions To Verify:** <assumptions or none - reason>
  - **Allowed Read Paths:** <project-relative paths/globs; no out-of-repo paths>
  - **Allowed Write Paths:** none - read-only discovery task
  - **Generated Paths:** none - findings summary is supplied to `mutai-scheduler agent handoff --summary-file` as scheduler artifact payload
  - **Forbidden Paths:** <paths/globs that must not be touched; include unrelated dirty/protected paths where applicable>
  - **Path Conflict Policy:** <deny on overlap | approved exception with source>
  - **Path Policy Table:**
    | Path | Access | Reason | Requirement / Task IDs | Conflict Behavior |
    |---|---|---|---|---|
    | <project-relative path> | <read/write/generated/forbidden> | <why needed> | <REQ/SCN/task IDs> | <deny/review/approve with source> |
  - **Acceptance Criteria:** Findings identify current behavior, gaps, and constraints with file/line, spec, command, or observed-behavior evidence.
  - **Validation / Evidence:**
    - **Command / Action:** `<exact command or manual inspection action>`
    - **Working Directory:** <project-relative or absolute cwd>
    - **Expected Exit Code / Observation:** <expected result>
    - **Required Evidence:** <stdout/stderr summary, artifact path, digest, or sourced finding>
    - **Covers:** <proposal sections, REQ/SCN IDs, or assumptions>
  - **Expected Artifact Kind:** findings-bundle
  - **Review Checklist:** Evidence is sourced; no implementation claims are made without inspected files; reviewers inspect the scheduler manifest plus copied summary payload.
  - **Traceability Refs:** <requirement/scenario IDs or proposal sections>
  - **Stale If:** Source files, specs, architecture docs, or assumptions inspected by this task change before implementation starts.

## 2. Implementation

- [ ] 2.1 <Concrete implementation task title>
  - **Type:** implementation
  - **Readiness:** <execution_ready | blocked-needs-human | read-only-discovery | rejected-too-vague>
  - **Authority Owner:** <OpenSpec | Better Droid compile/lint | Covey | `mutai-rs` Authority | executor | git/CI>
  - **Purpose:** <what this task changes and why>
  - **Scope In:** <specific behavior or artifact>
  - **Scope Out:** <nearby behavior explicitly excluded>
  - **Dependencies:** <task IDs or none - reason>
  - **Preconditions:** <required source state, approvals, clean worktree expectations, or none - reason>
  - **Source Freshness / Revalidation:** <exact command/action proving source has not changed or N/A with full waiver>
  - **Dirty Worktree Protection:** inspect dirty/untracked paths before mutation; stop if unrelated dirty paths overlap required scope.
  - **Base Revision / Source Digest Expectations:** <git rev, OpenSpec source digest, artifact digest, or none - reason>
  - **Assumptions:** <assumption IDs or none - reason>
  - **Risk Level:** <low | medium | high | critical>
  - **Human Approval Required:** <yes/no and approval source or none - reason>
  - **Allowed Read Paths:** <project-relative paths/globs; no out-of-repo paths>
  - **Allowed Write Paths:** <exact project-relative files or narrow bounded globs>
  - **Generated Paths:** <generated files/directories or none - reason; never Better Droid mission artifacts>
  - **Forbidden Paths:** <paths/globs that must not be touched; include unrelated dirty/protected paths where applicable>
  - **Path Conflict Policy:** <deny on overlap | approved exception with source>
  - **Path Policy Table:**
    | Path | Access | Reason | Requirement / Task IDs | Conflict Behavior |
    |---|---|---|---|---|
    | <project-relative path> | <read/write/generated/forbidden> | <why needed> | <REQ/SCN/task IDs> | <deny/review/approve with source> |
  - **Acceptance Criteria:**
    - <observable behavior or artifact contract, not merely “code updated”>
    - <observable behavior or artifact contract>
  - **Failure / Negative Cases:**
    - <invalid, stale, conflicting, unauthorized, or missing-input case> → <expected rejection/recovery/evidence>
  - **Validation / Evidence:**
    - **Command / Action:** `<exact command or manual action>`
    - **Working Directory:** <project-relative or absolute cwd>
    - **Expected Exit Code / Observation:** <expected result>
    - **Required Evidence:** <stdout/stderr summary, artifact path, digest, review finding, and changed-file list outside openspec/** for product implementation>
    - **Covers:** <requirement/scenario/task IDs>
  - **Expected Artifact Kind:** patch-bundle
  - **Review Evidence Binding:** source revision/digest, artifact/diff digest, validation evidence IDs, reviewer identity/role, independence status, waiver reason if not independent, and verdict.
  - **Review Checklist:**
    - Implementation satisfies linked requirements without scope creep.
    - Tests or checks prove the acceptance criteria and negative cases.
    - No unrelated dirty paths are modified.
    - Authority boundaries are preserved.
  - **Traceability Refs:** <specific REQ/SCN IDs>
  - **Stale If:** Linked requirement, validation command, allowed write paths, assumptions, or base revision changes.

## 3. Tests and Verification

- [ ] 3.1 Add or update verification for <behavior>
  - **Type:** test
  - **Readiness:** <execution_ready | blocked-needs-human | read-only-discovery | rejected-too-vague>
  - **Authority Owner:** <OpenSpec | Better Droid compile/lint | Covey | Authority | executor | git/CI>
  - **Purpose:** Prove the implementation behavior and prevent regression.
  - **Scope In:** <test files / check scripts>
  - **Scope Out:** <tests intentionally not added>
  - **Dependencies:** <task IDs>
  - **Preconditions:** <required source state, approvals, clean worktree expectations, or none - reason>
  - **Source Freshness / Revalidation:** <exact command/action proving source has not changed or N/A with full waiver>
  - **Dirty Worktree Protection:** inspect dirty/untracked paths before mutation; stop if unrelated dirty paths overlap required scope.
  - **Base Revision / Source Digest Expectations:** <git rev, OpenSpec source digest, artifact digest, or none - reason>
  - **Assumptions:** <assumption IDs or none - reason>
  - **Risk Level:** <low | medium | high | critical>
  - **Human Approval Required:** <yes/no and approval source or none - reason>
  - **Allowed Read Paths:** <project-relative paths/globs; no out-of-repo paths>
  - **Allowed Write Paths:** <exact test/check paths or narrow bounded globs>
  - **Generated Paths:** <generated snapshots/fixtures/reports or none - reason>
  - **Forbidden Paths:** <paths/globs that must not be touched; include unrelated dirty/protected paths where applicable>
  - **Path Conflict Policy:** <deny on overlap | approved exception with source>
  - **Path Policy Table:**
    | Path | Access | Reason | Requirement / Task IDs | Conflict Behavior |
    |---|---|---|---|---|
    | <project-relative path> | <read/write/generated/forbidden> | <why needed> | <REQ/SCN/task IDs> | <deny/review/approve with source> |
  - **Acceptance Criteria:** Verification fails before implementation where practical and passes after implementation.
  - **Failure / Negative Cases:**
    - <invalid, stale, conflicting, unauthorized, or missing-input case> → <expected test/check evidence>
  - **Validation / Evidence:**
    - **Command / Action:** `<exact narrow test/check command>`
    - **Working Directory:** <project-relative or absolute cwd>
    - **Expected Exit Code / Observation:** <expected result>
    - **Required Evidence:** <stdout/stderr summary, artifact path, digest, or report>
    - **Covers:** <REQ/SCN/task IDs>
    - **Command / Action:** `<wider package/build/check command if warranted>`
    - **Working Directory:** <project-relative or absolute cwd>
    - **Expected Exit Code / Observation:** <expected result>
    - **Required Evidence:** <stdout/stderr summary, artifact path, digest, or report>
    - **Covers:** <REQ/SCN/task IDs>
  - **Expected Artifact Kind:** verification-bundle
  - **Review Checklist:** Test names describe behavior; checks cover error/negative cases where relevant.
  - **Traceability Refs:** <specific REQ/SCN IDs>
  - **Stale If:** Behavior requirements, implementation paths, base revision, or assumptions change.

## 4. Review, Apply, and Archive

- [ ] 4.1 Review artifact against mission contract
  - **Type:** review
  - **Readiness:** <execution_ready | blocked-needs-human | read-only-discovery | rejected-too-vague>
  - **Authority Owner:** <OpenSpec | Better Droid compile/lint | Covey | Authority | executor | git/CI>
  - **Purpose:** Decide whether the exact artifact satisfies the OpenSpec change.
  - **Scope In:** proposal.md, design.md, tasks.md, specs/*/spec.md, artifact manifest, changed paths, verification evidence
  - **Scope Out:** Applying, merging, pushing, or mutating integration branch
  - **Dependencies:** <artifact-producing task IDs>
  - **Preconditions:** Artifact manifest exists; source digests match reviewed OpenSpec source; reviewer is independent or waiver is recorded.
  - **Source Freshness / Revalidation:** <exact command/action proving source has not changed or N/A with full waiver>
  - **Dirty Worktree Protection:** inspect dirty/untracked paths before mutation; stop if unrelated dirty paths overlap required scope.
  - **Base Revision / Source Digest Expectations:** <git rev, OpenSpec source digest, artifact digest>
  - **Assumptions:** <assumption IDs or none - reason>
  - **Risk Level:** <low | medium | high | critical>
  - **Human Approval Required:** <yes/no and approval source or none - reason>
  - **Allowed Read Paths:** <reviewed paths>
  - **Allowed Write Paths:** none - review findings are a scheduler artifact payload
  - **Generated Paths:** none - review findings summary is supplied to `mutai-scheduler agent handoff --summary-file` as scheduler artifact payload
  - **Forbidden Paths:** production/runtime state, unrelated dirty paths, protected paths, integration branch mutation
  - **Path Conflict Policy:** deny on overlap
  - **Acceptance Criteria:** Review verdict binds to exact artifact digest and lists blockers/warnings/suggestions.
  - **Failure / Negative Cases:**
    - Missing digest, missing validation evidence, stale source, scope creep, broad write paths, or implementer-only narrative → changes-requested.
  - **Validation / Evidence:**
    - **Command / Action:** inspect exact artifact/diff, OpenSpec source, and verification evidence
    - **Working Directory:** repo root or review workspace
    - **Expected Exit Code / Observation:** approve or changes-requested verdict with findings
    - **Required Evidence:** findings bundle bound to artifact digest and source digests
    - **Covers:** all linked requirement/scenario/task IDs
  - **Expected Artifact Kind:** findings-bundle
  - **Review Checklist:** Requirements, tasks, validation, path policy, assumptions, traceability, stale-if, scope creep, artifact manifest, and copied summary payload are all checked.
  - **Traceability Refs:** all linked requirement/scenario/task IDs
  - **Stale If:** Artifact digest, base revision, OpenSpec source, validation evidence, assumptions, or changed path list changes.

- [ ] 4.2 Confirm Better Droid mission readiness before import/archive
  - **Type:** verification
  - **Readiness:** <execution_ready | blocked-needs-human | read-only-discovery | rejected-too-vague>
  - **Authority Owner:** <OpenSpec | Better Droid compile/lint | Covey | Authority | executor | git/CI>
  - **Purpose:** Prove the OpenSpec change is deep enough for Better Droid compile/import.
  - **Scope In:** proposal.md, design.md, tasks.md, specs/*/spec.md, mission readiness fields, traceability, validation, path policy, assumptions
  - **Scope Out:** Applying, merging, pushing, mutating runtime state
  - **Dependencies:** 4.1
  - **Preconditions:** Review artifact exists and source has not changed since review.
  - **Source Freshness / Revalidation:** <exact command/action proving source has not changed or N/A with full waiver>
  - **Dirty Worktree Protection:** inspect dirty/untracked paths before mutation; stop if unrelated dirty paths overlap required scope.
  - **Base Revision / Source Digest Expectations:** OpenSpec source digests match reviewed source.
  - **Assumptions:** <assumption IDs or none - reason>
  - **Risk Level:** <low | medium | high | critical>
  - **Human Approval Required:** <yes/no and approval source or none - reason>
  - **Allowed Read Paths:** OpenSpec change directory and generated mission artifacts if present
  - **Allowed Write Paths:** none - readiness evidence summary is a scheduler artifact payload
  - **Generated Paths:** none - readiness evidence summary is supplied to `mutai-scheduler agent handoff --summary-file` as scheduler artifact payload
  - **Forbidden Paths:** production/runtime state, unrelated dirty paths, `authority/**`, `contracts/imported/**`
  - **Path Conflict Policy:** deny on overlap
  - **Acceptance Criteria:**
    - No placeholder fields remain in importable task packets.
    - Every executable task has acceptance criteria, path policy, validation/evidence, traceability refs, stale-if, assumptions, and risk level.
    - Every requirement/scenario is covered by task, validation, review, or explicit deferral.
    - High/critical assumptions have approval or block import.
    - `Traceability Coverage` is complete or incomplete coverage explicitly blocks import/archive.
    - Every `N/A` waiver follows the full waiver rule.
    - Source freshness and dirty-worktree protection are recorded.
  - **Failure / Negative Cases:**
    - Missing traceability, broad write paths, unjustified `none`, or vague executable tasks block import readiness.
  - **Validation / Evidence:**
    - **Command / Action:** `openspec validate <change-id> --type change --strict`
    - **Working Directory:** repo root
    - **Expected Exit Code / Observation:** exits 0
    - **Required Evidence:** command, exit code, summary, and source revision
    - **Covers:** all requirements/tasks
  - **Expected Artifact Kind:** verification-bundle
  - **Review Checklist:** Readiness evidence binds to exact OpenSpec source revision/digests.
  - **Traceability Refs:** all requirements/scenarios/tasks
  - **Stale If:** Any OpenSpec source, mission artifact, validation evidence, or review artifact changes.

- [ ] 4.3 Run final OpenSpec and repository checks
  - **Type:** verification
  - **Readiness:** <execution_ready | blocked-needs-human | read-only-discovery | rejected-too-vague>
  - **Authority Owner:** <OpenSpec | Better Droid compile/lint | Covey | Authority | executor | git/CI>
  - **Purpose:** Prove the change is valid and ready for archive/import/apply.
  - **Scope In:** OpenSpec change directory and any intentionally touched docs/config/schema files
  - **Scope Out:** Unrelated dirty worktree paths
  - **Dependencies:** all implementation and review tasks
  - **Preconditions:** Mission readiness check passed or import/archive is explicitly deferred.
  - **Source Freshness / Revalidation:** <exact command/action proving source has not changed or N/A with full waiver>
  - **Dirty Worktree Protection:** inspect dirty/untracked paths before mutation; stop if unrelated dirty paths overlap required scope.
  - **Base Revision / Source Digest Expectations:** <git rev, OpenSpec source digest, artifact digest, or none - reason>
  - **Assumptions:** <assumption IDs or none - reason>
  - **Risk Level:** <low | medium | high | critical>
  - **Human Approval Required:** <yes/no and approval source or none - reason>
  - **Allowed Read Paths:** full repo for status/diff/check commands
  - **Allowed Write Paths:** none - final evidence summary is a scheduler artifact payload
  - **Generated Paths:** none - final evidence summary is supplied to `mutai-scheduler agent handoff --summary-file` as scheduler artifact payload
  - **Forbidden Paths:** unrelated dirty paths and live runtime state
  - **Path Conflict Policy:** deny on overlap
  - **Acceptance Criteria:** Required validation commands pass and final evidence is recorded.
  - **Failure / Negative Cases:**
    - OpenSpec validation failure, whitespace errors, failing tests, stale source, or dirty unrelated paths block completion.
  - **Validation / Evidence:**
    - **Command / Action:** `openspec validate <change-id> --type change --strict`
    - **Working Directory:** repo root
    - **Expected Exit Code / Observation:** exits 0
    - **Required Evidence:** command output and exit code
    - **Covers:** all requirements/tasks
    - **Command / Action:** `git diff --check -- <changed-paths>`
    - **Working Directory:** repo root
    - **Expected Exit Code / Observation:** exits 0
    - **Required Evidence:** command output and exit code
    - **Covers:** changed files
    - **Command / Action:** `<project-specific tests/builds if code changed>`
    - **Working Directory:** <project-relative or absolute cwd>
    - **Expected Exit Code / Observation:** exits 0
    - **Required Evidence:** command output and exit code
    - **Covers:** linked behavior
  - **Expected Artifact Kind:** verification-bundle
  - **Review Checklist:** Verification output includes command, working directory, exit code, summary, and source revision.
  - **Traceability Refs:** all requirements/tasks
  - **Stale If:** Any source artifact changes after verification.

use crate::error::Result;
use crate::model::{
    LintOptions, PROTECTED_FORBIDDEN_PATH_GROUPS, PlanningClass, SourceSnapshot, SourceTask,
};
use crate::report::{
    Blocker, ImportStatus, LintState, MissionReport, TaskClassification, Warning, report_from_lint,
};
use crate::source::{blake3_digest, load_source};
use std::collections::{BTreeMap, BTreeSet};

const REQUIRED_TASK_FIELDS: &[&str] = &[
    "Type",
    "Purpose",
    "Acceptance Criteria",
    "Validation / Evidence",
    "Traceability Refs",
    "Stale If",
];
const MAX_WORK_PACKET_EXECUTABLE_TASKS: usize = 6;
const MAX_WORK_PACKET_DEPENDENCY_DEPTH: usize = 3;
const MAX_WORK_PACKET_DEPENDENCIES_PER_TASK: usize = 2;

/// Lint a Better Droid OpenSpec change without writing mission artifacts.
///
/// # Errors
///
/// Returns an error when source files cannot be read or the change path is invalid.
pub fn lint_change(options: &LintOptions) -> Result<MissionReport> {
    let source = load_source(&options.project_root, &options.change_id)?;
    let lint = lint_source(&source);
    Ok(report_from_lint(&source, lint, Vec::new(), BTreeMap::new()))
}

pub(crate) fn lint_source(source: &SourceSnapshot) -> LintState {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    let mut classifications = Vec::with_capacity(source.tasks_parsed.len());
    let mut covered_scenarios = BTreeSet::new();

    if source
        .change_status
        .as_deref()
        .is_some_and(|status| status.starts_with("stale"))
    {
        blockers.push(Blocker {
            id: "stale_openspec_change".to_owned(),
            source_path: source.openspec_yaml.relative_path.clone(),
            task_id: None,
            scenario_id: None,
            detail: "change is marked stale; reauthor against the current Covey/mutai-rs authority split before compile or import".to_owned(),
        });
    }
    match source.planning_class {
        PlanningClass::WorkPacket => {
            blockers.extend(work_packet_granularity_blockers(source));
        }
        PlanningClass::Invalid => blockers.push(Blocker {
            id: "invalid_planning_class".to_owned(),
            source_path: source.openspec_yaml.relative_path.clone(),
            task_id: None,
            scenario_id: None,
            detail: format!(
                "planning_class must be work_packet, got {}",
                source.planning_class_raw.as_deref().unwrap_or("<missing>")
            ),
        }),
    }

    if source.tasks_parsed.is_empty() {
        blockers.push(Blocker {
            id: "missing_required_source".to_owned(),
            source_path: source.tasks.relative_path.clone(),
            task_id: None,
            scenario_id: None,
            detail: "no stable task checklist entries found".to_owned(),
        });
    }

    for spec in &source.specs {
        if !has_openspec_delta_section(&spec.text) {
            blockers.push(Blocker {
                id: "missing_openspec_delta_section".to_owned(),
                source_path: spec.relative_path.clone(),
                task_id: None,
                scenario_id: None,
                detail:
                    "spec file must use OpenSpec delta headings such as `## ADDED Requirements`"
                        .to_owned(),
            });
        }
    }

    let mut seen_task_ids = BTreeSet::new();
    for task in &source.tasks_parsed {
        let mut task_blockers = Vec::new();
        task_blockers.extend(task.field_syntax_errors.iter().cloned());
        if !is_stable_task_id(&task.id) {
            task_blockers.push("malformed task id".to_owned());
        }
        if !seen_task_ids.insert(task.id.clone()) {
            task_blockers.push("duplicate task id".to_owned());
        }
        if !has_required_task_field(task, "Type") {
            task_blockers.push("missing Type".to_owned());
        }

        let task_type = task_field(task, "Type").unwrap_or("unknown");
        let executable = is_executable_task_type(task_type);
        task_blockers.extend(disallowed_mission_artifact_task_work_blockers(task));
        if executable {
            for field in REQUIRED_TASK_FIELDS {
                if !has_required_task_field(task, field) {
                    task_blockers.push(format!("missing {field}"));
                }
            }
            if !has_required_task_field(task, "Allowed Write Paths")
                && !task_type.eq_ignore_ascii_case("verification")
            {
                task_blockers.push("missing Allowed Write Paths".to_owned());
            }
            task_blockers.extend(path_policy_blockers(task));
            task_blockers.extend(product_implementation_evidence_blockers(task));
        }

        if task.raw_block.contains("Risk Level:** high")
            || task.raw_block.contains("Risk Level:** critical")
        {
            let approval = task_field(task, "Human Approval Required").unwrap_or_default();
            if approval.contains("pending") || approval.contains("none") || approval.is_empty() {
                task_blockers.push("unresolved high or critical assumption approval".to_owned());
            }
        }
        if task
            .raw_block
            .to_ascii_lowercase()
            .contains("blocked-needs-human")
        {
            task_blockers.push(
                "blocked-needs-human gate prevents execution_ready and apply_authorized".to_owned(),
            );
        }

        for scenario in &source.scenarios {
            if task.raw_block.contains(&scenario.id) {
                covered_scenarios.insert(scenario.id.clone());
            }
        }

        let status = if task_blockers.is_empty() {
            ImportStatus::Importable
        } else if task_type.contains("rejected") {
            ImportStatus::Rejected
        } else if task_type.contains("deferred") {
            ImportStatus::Deferred
        } else {
            ImportStatus::Blocked
        };

        for detail in task_blockers {
            blockers.push(Blocker {
                id: if detail.contains("non-canonical task field syntax") {
                    "non_canonical_task_field_syntax".to_owned()
                } else if detail.contains("product implementation task") {
                    "missing_product_implementation_evidence".to_owned()
                } else if detail.contains("path")
                    || detail.contains("Allowed Write Paths")
                    || detail.contains("write scope")
                    || detail.contains("protected")
                {
                    "unsafe_path_policy".to_owned()
                } else if detail.contains("task id") {
                    "malformed_stable_task_id".to_owned()
                } else if detail.contains("blocked-needs-human")
                    || detail.contains("human approval")
                {
                    "human_gate_open".to_owned()
                } else {
                    "mission_incomplete_executable_task".to_owned()
                },
                source_path: task.source_path.clone(),
                task_id: Some(task.id.clone()),
                scenario_id: None,
                detail,
            });
        }

        let task_digest = blake3_digest(
            format!(
                "{}\n{}\n{}\n{}",
                task.id, task.title, task_type, task.raw_block
            )
            .as_bytes(),
        );
        classifications.push(TaskClassification {
            task_id: task.id.clone(),
            title: task.title.clone(),
            task_type: task_type.to_owned(),
            import_status: status,
            task_digest,
        });
    }

    for scenario in &source.scenarios {
        if !covered_scenarios.contains(&scenario.id) {
            blockers.push(Blocker {
                id: "unmapped_behavioral_scenario".to_owned(),
                source_path: source
                    .specs
                    .first()
                    .map(|spec| spec.relative_path.clone())
                    .unwrap_or_else(|| source.relative_change_path.clone()),
                task_id: None,
                scenario_id: Some(scenario.id.clone()),
                detail: format!("scenario {} has no task traceability", scenario.id),
            });
        }
    }

    if source.schema_name != "better-droid" {
        blockers.push(Blocker {
            id: "schema_mismatch".to_owned(),
            source_path: source.relative_change_path.clone(),
            task_id: None,
            scenario_id: None,
            detail: "schema must be better-droid".to_owned(),
        });
    }

    if source.tasks_parsed.len() > 30 {
        warnings.push(Warning {
            id: "large_task_set".to_owned(),
            source_path: source.tasks.relative_path.clone(),
            detail: "large task sets should be split before autonomous import".to_owned(),
        });
    }

    LintState {
        blockers,
        warnings,
        classifications,
    }
}

fn product_implementation_evidence_blockers(task: &SourceTask) -> Vec<String> {
    let task_type = task_field(task, "Type").unwrap_or_default();
    if !is_product_implementation_task_type(task_type) || !task_has_product_write_paths(task) {
        return Vec::new();
    }

    let mut blockers = Vec::new();
    let validation_command = task_field(task, "Command / Action").unwrap_or_default();
    if !command_mentions_product_validation(validation_command) {
        blockers.push(
            "product implementation task with product write paths requires validation command against product code".to_owned(),
        );
    }

    let acceptance = task_field(task, "Acceptance Criteria").unwrap_or_default();
    if !acceptance_mentions_product_behavior(acceptance) {
        blockers.push(
            "product implementation task with product write paths requires acceptance criteria tied to product behavior".to_owned(),
        );
    }

    if !task_mentions_changed_file_evidence(task) {
        blockers.push(
            "product implementation task with product write paths requires changed-file evidence outside openspec/**".to_owned(),
        );
    }

    blockers
}

fn is_product_implementation_task_type(task_type: &str) -> bool {
    matches!(
        task_type.trim().to_ascii_lowercase().as_str(),
        "implementation" | "test" | "data-movement" | "schema-change" | "refactor"
    )
}

fn task_has_product_write_paths(task: &SourceTask) -> bool {
    task_field(task, "Allowed Write Paths")
        .into_iter()
        .flat_map(split_path_list)
        .any(|path| !is_planning_only_write_path(&path))
}

fn command_mentions_product_validation(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    lower.contains("cargo test")
        || lower.contains("cargo nextest")
        || lower.contains("just test")
        || lower.contains("npm test")
        || lower.contains("pnpm test")
        || lower.contains("bun test")
        || lower.contains("pytest")
        || lower.contains("go test")
        || lower.contains("dotai-api")
        || lower.contains("dotai-webapp")
        || lower.contains("dotai-desktop")
        || lower.contains("dotai-admin")
        || lower.contains("magnitude")
        || lower.contains("covey/")
        || lower.contains("authority/")
        || lower.contains("better-droid/src")
}

fn acceptance_mentions_product_behavior(acceptance: &str) -> bool {
    let lower = acceptance.to_ascii_lowercase();
    !lower.trim().is_empty()
        && !lower.contains("todo")
        && !lower.contains("tbd")
        && (lower.contains("behavior")
            || lower.contains("reject")
            || lower.contains("accept")
            || lower.contains("import")
            || lower.contains("compile")
            || lower.contains("execute")
            || lower.contains("apply")
            || lower.contains("persist")
            || lower.contains("record")
            || lower.contains("emit")
            || lower.contains("return")
            || lower.contains("block")
            || lower.contains("fail"))
}

fn task_mentions_changed_file_evidence(task: &SourceTask) -> bool {
    let lower = task.raw_block.to_ascii_lowercase();
    let mentions_changed_files = lower.contains("changed-file")
        || lower.contains("changed file")
        || lower.contains("changed files")
        || lower.contains("file list")
        || lower.contains("git diff --name-only")
        || lower.contains("git status --short");
    let binds_outside_openspec = lower.contains("outside openspec")
        || lower.contains("outside `openspec")
        || lower.contains("non-openspec")
        || lower.contains("product file")
        || lower.contains("product files");
    mentions_changed_files && binds_outside_openspec
}

fn is_planning_only_write_path(path: &str) -> bool {
    let normalized = path.trim().trim_start_matches("./");
    normalized.is_empty()
        || normalized.eq_ignore_ascii_case("none")
        || normalized.starts_with("openspec/")
        || normalized.starts_with("docs/")
        || normalized.starts_with("better-droid/docs-site/")
        || normalized.starts_with("better-droid/openspec/")
}

pub(crate) fn work_packet_granularity_blockers(source: &SourceSnapshot) -> Vec<Blocker> {
    let mut blockers = Vec::new();
    if contains_phase_token(&source.change_id) || proposal_title_contains_phase(source) {
        blockers.push(Blocker {
            id: "phase_shaped_work_packet".to_owned(),
            source_path: source.relative_change_path.clone(),
            task_id: None,
            scenario_id: None,
            detail: "work-packet changes must be source-scoped, not phase-shaped; put phase plans outside openspec/changes and create smaller work-packet changes for execution".to_owned(),
        });
    }

    let executable_tasks = source
        .tasks_parsed
        .iter()
        .filter(|task| task_field(task, "Type").is_some_and(is_executable_task_type))
        .collect::<Vec<_>>();
    if executable_tasks.len() > MAX_WORK_PACKET_EXECUTABLE_TASKS {
        blockers.push(Blocker {
            id: "work_packet_too_large".to_owned(),
            source_path: source.tasks.relative_path.clone(),
            task_id: None,
            scenario_id: None,
            detail: format!(
                "work-packet changes may contain at most {MAX_WORK_PACKET_EXECUTABLE_TASKS} executable tasks; found {}",
                executable_tasks.len()
            ),
        });
    }

    for task in &source.tasks_parsed {
        let dependency_refs = resolved_dependency_task_ids(task, &source.tasks_parsed);
        if dependency_refs.len() > MAX_WORK_PACKET_DEPENDENCIES_PER_TASK {
            blockers.push(Blocker {
                id: "work_packet_too_many_dependencies".to_owned(),
                source_path: task.source_path.clone(),
                task_id: Some(task.id.clone()),
                scenario_id: None,
                detail: format!(
                    "work-packet tasks may depend on at most {MAX_WORK_PACKET_DEPENDENCIES_PER_TASK} sibling tasks; {} depends on {}",
                    task.id,
                    dependency_refs.len()
                ),
            });
        }
    }

    let dependency_depth = longest_dependency_chain_depth(&source.tasks_parsed);
    if dependency_depth > MAX_WORK_PACKET_DEPENDENCY_DEPTH {
        blockers.push(Blocker {
            id: "work_packet_dependency_chain_too_long".to_owned(),
            source_path: source.tasks.relative_path.clone(),
            task_id: None,
            scenario_id: None,
            detail: format!(
                "work-packet dependency chains may be at most {MAX_WORK_PACKET_DEPENDENCY_DEPTH} tasks deep; found depth {dependency_depth}"
            ),
        });
    }

    if mixes_implementation_with_review_or_apply(source) {
        blockers.push(Blocker {
            id: "mixed_review_concerns".to_owned(),
            source_path: source.tasks.relative_path.clone(),
            task_id: None,
            scenario_id: None,
            detail: "work-packet changes must not mix implementation work with review/apply concerns; keep review/apply as separate packets or Covey lifecycle steps".to_owned(),
        });
    }

    blockers
}

pub(crate) fn contains_phase_token(text: &str) -> bool {
    text.to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|token| {
            token == "phase"
                || token.strip_prefix("phase").is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
                })
        })
}

pub(crate) fn proposal_title_contains_phase(source: &SourceSnapshot) -> bool {
    source.proposal.text.lines().take(40).any(|line| {
        let trimmed = line.trim();
        let is_title_line = trimmed.starts_with('#')
            || trimmed.starts_with("- **Objective:**")
            || trimmed.starts_with("Objective:")
            || trimmed.starts_with("Title:");
        is_title_line && contains_phase_token(trimmed)
    })
}

pub(crate) fn resolved_dependency_task_ids(
    task: &SourceTask,
    all_tasks: &[SourceTask],
) -> Vec<String> {
    let Some(dependencies) = task_field(task, "Dependencies") else {
        return Vec::new();
    };
    if is_empty_or_none(dependencies) {
        return Vec::new();
    }

    all_tasks
        .iter()
        .filter(|candidate| {
            candidate.id != task.id && dependency_text_mentions_task_id(dependencies, &candidate.id)
        })
        .map(|candidate| candidate.id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn dependency_text_mentions_task_id(text: &str, task_id: &str) -> bool {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.'))
        .any(|token| token == task_id)
}

pub(crate) fn longest_dependency_chain_depth(tasks: &[SourceTask]) -> usize {
    let adjacency = tasks
        .iter()
        .map(|task| {
            (
                task.id.clone(),
                resolved_dependency_task_ids(task, tasks)
                    .into_iter()
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut memo = BTreeMap::new();
    adjacency
        .keys()
        .map(|task_id| dependency_depth(task_id, &adjacency, &mut BTreeSet::new(), &mut memo))
        .max()
        .unwrap_or(0)
}

fn dependency_depth(
    task_id: &str,
    adjacency: &BTreeMap<String, Vec<String>>,
    visiting: &mut BTreeSet<String>,
    memo: &mut BTreeMap<String, usize>,
) -> usize {
    if let Some(depth) = memo.get(task_id) {
        return *depth;
    }
    if !visiting.insert(task_id.to_owned()) {
        return MAX_WORK_PACKET_DEPENDENCY_DEPTH + 1;
    }

    let depth = adjacency
        .get(task_id)
        .map(|dependencies| {
            dependencies
                .iter()
                .map(|dependency| {
                    if adjacency.contains_key(dependency) {
                        1 + dependency_depth(dependency, adjacency, visiting, memo)
                    } else {
                        1
                    }
                })
                .max()
                .unwrap_or(1)
        })
        .unwrap_or(1);
    visiting.remove(task_id);
    memo.insert(task_id.to_owned(), depth);
    depth
}

pub(crate) fn mixes_implementation_with_review_or_apply(source: &SourceSnapshot) -> bool {
    let mut has_implementation_like = false;
    let mut has_review_or_apply = false;

    for task in &source.tasks_parsed {
        let task_type = task_field(task, "Type").unwrap_or_default();
        let normalized_type = task_type.trim().to_ascii_lowercase();
        if matches!(
            normalized_type.as_str(),
            "implementation" | "test" | "data-movement" | "schema-change" | "refactor"
        ) {
            has_implementation_like = true;
        }
        let title = task.title.to_ascii_lowercase();
        if normalized_type == "apply"
            || normalized_type.contains("review")
            || normalized_type.contains("apply")
            || title.contains("review")
            || title.contains("apply")
        {
            has_review_or_apply = true;
        }
    }

    has_implementation_like && has_review_or_apply
}

pub(crate) fn task_field<'a>(task: &'a SourceTask, field: &str) -> Option<&'a str> {
    task.fields.get(field).map(String::as_str)
}

pub(crate) fn has_openspec_delta_section(text: &str) -> bool {
    text.lines().any(|line| {
        matches!(
            line.trim(),
            "## ADDED Requirements"
                | "## MODIFIED Requirements"
                | "## REMOVED Requirements"
                | "## RENAMED Requirements"
        )
    })
}

pub(crate) fn has_required_task_field(task: &SourceTask, field: &str) -> bool {
    if field == "Validation / Evidence" {
        return task.raw_block.contains("- **Validation / Evidence:**")
            && task.raw_block.contains("- **Command / Action:**")
            && task
                .raw_block
                .contains("- **Expected Exit Code / Observation:**");
    }

    if field == "Allowed Write Paths" {
        return task
            .fields
            .get(field)
            .is_some_and(|value| !is_empty_or_none(value));
    }

    task.fields
        .get(field)
        .is_some_and(|value| !is_empty_or_none(value))
}

pub(crate) fn is_empty_or_none(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.is_empty()
        || normalized == "none"
        || normalized.starts_with("none -")
        || normalized.contains("<")
        || normalized.contains(">")
}

pub(crate) fn is_executable_task_type(task_type: &str) -> bool {
    matches!(
        task_type.trim().to_ascii_lowercase().as_str(),
        "implementation"
            | "test"
            | "documentation"
            | "data-movement"
            | "schema-change"
            | "refactor"
            | "apply"
    )
}

pub(crate) fn is_stable_task_id(task_id: &str) -> bool {
    task_id.contains('.')
        && task_id
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
}

pub(crate) fn path_policy_blockers(task: &SourceTask) -> Vec<String> {
    let mut blockers = Vec::new();
    let allowed = task_field(task, "Allowed Write Paths").unwrap_or_default();
    if [".", "/", "repo", "**/*", "all files"]
        .iter()
        .any(|bad| allowed.split(',').map(str::trim).any(|part| part == *bad))
        || allowed.trim().starts_with("/*")
        || allowed.trim() == "*"
    {
        blockers.push("repo-global write scope is forbidden".to_owned());
    }
    if allowed.contains("../") {
        blockers.push("out-of-root write scope is forbidden".to_owned());
    }

    let forbidden = task_field(task, "Forbidden Paths").unwrap_or_default();
    for protected_group in PROTECTED_FORBIDDEN_PATH_GROUPS {
        if !protected_group
            .iter()
            .any(|protected| forbidden.contains(protected) || allowed.contains(protected))
        {
            blockers.push(format!(
                "missing protected forbidden path {}",
                protected_group.join(" or ")
            ));
        }
        for protected in *protected_group {
            if allowed.contains(protected) {
                blockers.push(format!("protected path {protected} cannot be allowed"));
            }
        }
    }

    for allowed_path in split_path_list(allowed) {
        if malformed_path_policy_entry(&allowed_path) {
            blockers.push(format!(
                "malformed path policy entry is not machine-enforceable: {allowed_path}"
            ));
        }
        if split_path_list(forbidden)
            .iter()
            .any(|forbidden_path| forbidden_path == &allowed_path)
        {
            blockers.push(format!(
                "allowed and forbidden paths overlap at {allowed_path}"
            ));
        }
    }

    blockers
}

pub(crate) fn disallowed_mission_artifact_task_work_blockers(task: &SourceTask) -> Vec<String> {
    let mut blockers = Vec::new();
    let allowed = task_field(task, "Allowed Write Paths").unwrap_or_default();
    for allowed_path in split_path_list(allowed) {
        if is_better_droid_mission_artifact_path(&allowed_path) {
            blockers.push(format!(
                "Better Droid mission artifact path cannot be task writable: {allowed_path}"
            ));
        }
    }

    let generated = task_field(task, "Generated Paths").unwrap_or_default();
    for generated_path in split_path_list(generated) {
        if is_better_droid_mission_artifact_path(&generated_path) {
            blockers.push(format!(
                "Better Droid mission artifact path cannot be task-generated work: {generated_path}"
            ));
        }
    }
    blockers
}

fn is_better_droid_mission_artifact_path(path: &str) -> bool {
    let parts = path
        .replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if parts.len() < 5 {
        return false;
    }
    parts
        .windows(4)
        .any(|window| window[0] == "openspec" && window[1] == "changes" && window[3] == "mission")
}

fn malformed_path_policy_entry(path: &str) -> bool {
    let trimmed = path.trim();
    trimmed.contains('`')
        || trimmed.contains(" ")
        || trimmed.contains(" and ")
        || trimmed.contains(" only ")
        || trimmed.contains(':')
        || trimmed.starts_with("Allowed ")
        || trimmed.starts_with("Forbidden ")
        || trimmed.ends_with('.')
}

pub(crate) fn split_path_list(value: &str) -> Vec<String> {
    value
        .split([',', '\n'])
        .map(|item| {
            let trimmed = item.trim().trim_start_matches("- ").trim();
            if trimmed.starts_with('`') && trimmed.ends_with('`') && trimmed.len() >= 2 {
                trimmed.trim_matches('`').to_owned()
            } else {
                trimmed.to_owned()
            }
        })
        .filter(|item| !item.is_empty() && !item.starts_with("none -"))
        .collect()
}

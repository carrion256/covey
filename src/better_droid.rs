use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;

const REQUIRED_TASK_FIELDS: &[&str] = &[
    "Type",
    "Purpose",
    "Acceptance Criteria",
    "Validation / Evidence",
    "Traceability Refs",
    "Stale If",
];
const CANONICAL_PROTECTED_FORBIDDEN_PATHS: &[&str] = &[
    "authority/**",
    "go/controlplane/**",
    "vendored/cliproxyapiplus/**",
    ".git/**",
];
const PROTECTED_FORBIDDEN_PATH_GROUPS: &[&[&str]] = &[
    &["authority/**", "mutai-rs/**"],
    &["go/controlplane/**"],
    &["vendored/cliproxyapiplus/**"],
    &[".git/**"],
];
const ARTIFACT_NAMES: &[&str] = &[
    "mission.json",
    "mission-packet.json",
    "traceability.json",
    "validation.json",
    "path-policy.json",
    "review-rubric.json",
    "assumptions.json",
    "compile-report.json",
];

/// Options for Better Droid mission lint.
#[derive(Debug, Clone)]
pub struct LintOptions {
    pub project_root: PathBuf,
    pub change_id: String,
}

/// Options for Better Droid mission compile.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    pub project_root: PathBuf,
    pub change_id: String,
    pub output_dir: Option<PathBuf>,
}

/// Errors returned by Better Droid mission lint/compile.
#[derive(Debug, Error)]
pub enum BetterDroidError {
    #[error("invalid Better Droid source in {path}: {detail}")]
    InvalidSource { path: String, detail: String },
    #[error("output path escapes mission directory: {path}")]
    OutputPathEscape { path: String },
    #[error("failed filesystem operation for {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Result alias for Better Droid operations.
pub type Result<T> = std::result::Result<T, BetterDroidError>;

/// Machine-readable lint/compile report.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MissionReport {
    pub schema: &'static str,
    pub change_id: String,
    pub status: ReportStatus,
    pub import_ready: bool,
    pub checked_artifact_paths: Vec<String>,
    pub created_artifacts: Vec<String>,
    pub source_digests: BTreeMap<String, String>,
    pub artifact_digests: BTreeMap<String, String>,
    pub task_counts: TaskCounts,
    pub blockers: Vec<Blocker>,
    pub warnings: Vec<Warning>,
    pub task_classifications: Vec<TaskClassification>,
    pub non_canonical_run_metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Ready,
    Blocked,
}

impl std::fmt::Display for ReportStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready => formatter.write_str("ready"),
            Self::Blocked => formatter.write_str("blocked"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct TaskCounts {
    pub total: usize,
    pub importable: usize,
    pub blocked: usize,
    pub rejected: usize,
    pub deferred: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Blocker {
    pub id: String,
    pub source_path: String,
    pub task_id: Option<String>,
    pub scenario_id: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Warning {
    pub id: String,
    pub source_path: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TaskClassification {
    pub task_id: String,
    pub title: String,
    pub task_type: String,
    pub import_status: ImportStatus,
    pub task_digest: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImportStatus {
    Importable,
    Blocked,
    Rejected,
    Deferred,
}

#[derive(Debug)]
struct SourceSnapshot {
    change_id: String,
    change_path: PathBuf,
    relative_change_path: String,
    schema_name: String,
    openspec_yaml: SourceFile,
    proposal: SourceFile,
    design: SourceFile,
    tasks: SourceFile,
    specs: Vec<SourceFile>,
    tasks_parsed: Vec<SourceTask>,
    requirements: Vec<Requirement>,
    scenarios: Vec<Scenario>,
}

#[derive(Debug, Clone)]
struct SourceFile {
    relative_path: String,
    text: String,
    digest: String,
}

#[derive(Debug, Clone)]
struct SourceTask {
    id: String,
    title: String,
    source_path: String,
    fields: BTreeMap<String, String>,
    raw_block: String,
}

#[derive(Debug, Clone, Serialize)]
struct CompiledTask {
    task_id: String,
    title: String,
    task_type: String,
    import_status: ImportStatus,
    purpose: String,
    acceptance_criteria: Vec<String>,
    requirement_ids: Vec<String>,
    scenario_ids: Vec<String>,
    dependencies: Vec<String>,
    allowed_read_paths: Vec<String>,
    allowed_write_paths: Vec<String>,
    forbidden_write_paths: Vec<String>,
    validation_refs: Vec<String>,
    expected_evidence: Vec<String>,
    expected_artifact_kind: String,
    review_rubric_refs: Vec<String>,
    stale_if: Vec<String>,
    task_digest: String,
}

#[derive(Debug, Clone)]
struct Requirement {
    id: String,
    title: String,
}

#[derive(Debug, Clone)]
struct Scenario {
    id: String,
    title: String,
    requirement_id: String,
}

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

/// Compile a Better Droid OpenSpec change into canonical JSON artifacts.
///
/// # Errors
///
/// Returns an error when source files cannot be read, output confinement fails, or JSON cannot be
/// serialized.
pub fn compile_change(options: &CompileOptions) -> Result<MissionReport> {
    let source = load_source(&options.project_root, &options.change_id)?;
    let lint = lint_source(&source);

    if lint.has_blockers() {
        return Ok(report_from_lint(&source, lint, Vec::new(), BTreeMap::new()));
    }

    let output_dir =
        resolve_output_dir(&options.project_root, &source, options.output_dir.as_ref())?;
    fs::create_dir_all(&output_dir).map_err(|source_error| BetterDroidError::Io {
        path: output_dir.display().to_string(),
        source: source_error,
    })?;

    let artifacts = build_artifacts(&source, &lint);
    let mut created = Vec::with_capacity(ARTIFACT_NAMES.len());
    let mut artifact_digests = BTreeMap::new();

    for name in ARTIFACT_NAMES {
        if *name == "compile-report.json" {
            continue;
        }
        let value = artifacts
            .get(*name)
            .expect("artifact name must exist in compiled artifact map");
        let canonical_digest = canonical_json_digest(value)?;
        let artifact_path = output_dir.join(name);
        let bytes = serde_json::to_vec_pretty(value)?;
        fs::write(&artifact_path, bytes).map_err(|source_error| BetterDroidError::Io {
            path: artifact_path.display().to_string(),
            source: source_error,
        })?;
        let relative_path = artifact_relative_path(&options.project_root, &artifact_path);
        artifact_digests.insert(relative_path.clone(), canonical_digest);
        created.push(relative_path);
    }

    let report_path = output_dir.join("compile-report.json");
    let report_relative_path = artifact_relative_path(&options.project_root, &report_path);
    let report_without_self_digest = report_from_lint(
        &source,
        lint.clone(),
        {
            let mut paths = created.clone();
            paths.push(report_relative_path.clone());
            paths
        },
        artifact_digests.clone(),
    );
    let report_value_without_self_digest = serde_json::to_value(&report_without_self_digest)?;
    artifact_digests.insert(
        report_relative_path.clone(),
        canonical_json_digest(&report_value_without_self_digest)?,
    );
    let report = report_from_lint(
        &source,
        lint,
        {
            let mut paths = created.clone();
            paths.push(report_relative_path);
            paths
        },
        artifact_digests,
    );
    fs::write(&report_path, serde_json::to_vec_pretty(&report)?).map_err(|source_error| {
        BetterDroidError::Io {
            path: report_path.display().to_string(),
            source: source_error,
        }
    })?;
    Ok(report)
}

fn load_source(project_root: &Path, change_id: &str) -> Result<SourceSnapshot> {
    let change_path = project_root
        .join("openspec")
        .join("changes")
        .join(change_id);
    if !change_path.is_dir() {
        return Err(BetterDroidError::InvalidSource {
            path: normalize_relative_path(&change_path),
            detail: "missing_required_source: change directory".to_owned(),
        });
    }

    let openspec_yaml = read_required_file(project_root, &change_path.join(".openspec.yaml"))?;
    let schema_name = parse_schema_name(&openspec_yaml.text);
    if schema_name.as_deref() != Some("better-droid") {
        return Err(BetterDroidError::InvalidSource {
            path: openspec_yaml.relative_path,
            detail: "schema must be better-droid".to_owned(),
        });
    }

    let proposal = read_required_file(project_root, &change_path.join("proposal.md"))?;
    let design = read_required_file(project_root, &change_path.join("design.md"))?;
    let tasks = read_required_file(project_root, &change_path.join("tasks.md"))?;
    let specs = read_specs(project_root, &change_path.join("specs"))?;
    if specs.is_empty() {
        return Err(BetterDroidError::InvalidSource {
            path: normalize_relative_path(&change_path.join("specs")),
            detail: "missing_required_source: specs".to_owned(),
        });
    }

    let tasks_parsed = parse_tasks(&tasks);
    let (requirements, scenarios) = parse_specs(&specs);

    Ok(SourceSnapshot {
        change_id: change_id.to_owned(),
        relative_change_path: normalize_relative_path(
            change_path
                .strip_prefix(project_root)
                .unwrap_or(change_path.as_path()),
        ),
        change_path,
        schema_name: "better-droid".to_owned(),
        openspec_yaml,
        proposal,
        design,
        tasks,
        specs,
        tasks_parsed,
        requirements,
        scenarios,
    })
}

fn parse_schema_name(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("schema:").map(str::trim))
        .map(|schema| schema.trim_matches('"').to_owned())
}

fn read_required_file(project_root: &Path, path: &Path) -> Result<SourceFile> {
    if !path.is_file() {
        return Err(BetterDroidError::InvalidSource {
            path: normalize_relative_path(path),
            detail: "missing_required_source".to_owned(),
        });
    }

    read_source_file(project_root, path)
}

fn read_specs(project_root: &Path, specs_dir: &Path) -> Result<Vec<SourceFile>> {
    if !specs_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    collect_markdown_files(specs_dir, &mut paths)?;
    paths.sort();
    paths
        .iter()
        .map(|path| read_source_file(project_root, path))
        .collect()
}

fn collect_markdown_files(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(|source| BetterDroidError::Io {
        path: dir.display().to_string(),
        source,
    })? {
        let entry = entry.map_err(|source| BetterDroidError::Io {
            path: dir.display().to_string(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, paths)?;
        } else if path.extension().is_some_and(|extension| extension == "md") {
            paths.push(path);
        }
    }
    Ok(())
}

fn read_source_file(project_root: &Path, path: &Path) -> Result<SourceFile> {
    let bytes = fs::read(path).map_err(|source| BetterDroidError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(SourceFile {
        relative_path: normalize_relative_path(path.strip_prefix(project_root).unwrap_or(path)),
        text,
        digest: sha256_digest(&bytes),
    })
}

fn parse_tasks(tasks: &SourceFile) -> Vec<SourceTask> {
    let lines = tasks.text.lines().collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index].trim();
        let Some(rest) = task_line_rest(line) else {
            index += 1;
            continue;
        };

        let Some((task_id, title)) = rest.split_once(' ') else {
            index += 1;
            continue;
        };

        let mut block = Vec::new();
        index += 1;
        while index < lines.len() {
            let next = lines[index];
            if task_line_rest(next.trim()).is_some() || next.starts_with("## ") {
                break;
            }
            block.push(next);
            index += 1;
        }

        let raw_block = block.join("\n");
        result.push(SourceTask {
            id: task_id.to_owned(),
            title: title.trim().to_owned(),
            source_path: tasks.relative_path.clone(),
            fields: parse_task_fields(&raw_block),
            raw_block,
        });
    }

    result
}

fn task_line_rest(trimmed: &str) -> Option<&str> {
    trimmed
        .strip_prefix("- [ ] ")
        .or_else(|| trimmed.strip_prefix("- [x] "))
        .or_else(|| trimmed.strip_prefix("- [X] "))
}

fn parse_task_fields(raw_block: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    let mut current: Option<String> = None;

    for line in raw_block.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- **")
            && let Some((field, value)) = rest.split_once(":**")
        {
            let key = field.trim().to_owned();
            let value = value.trim().to_owned();
            current = Some(key.clone());
            fields.insert(key, value);
            continue;
        }

        if let Some(key) = current.as_ref()
            && !trimmed.is_empty()
        {
            let entry = fields.entry(key.clone()).or_default();
            if !entry.is_empty() {
                entry.push('\n');
            }
            entry.push_str(trimmed);
        }
    }

    fields
}

fn parse_specs(specs: &[SourceFile]) -> (Vec<Requirement>, Vec<Scenario>) {
    let mut requirements = Vec::new();
    let mut scenarios = Vec::new();
    let mut current_requirement_id: Option<String> = None;

    for spec in specs {
        for line in spec.text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("### Requirement:") {
                let title = rest.trim().to_owned();
                let id = extract_prefixed_id(&title, "REQ-")
                    .unwrap_or_else(|| stable_id("REQ-derived", &title));
                current_requirement_id = Some(id.clone());
                requirements.push(Requirement { id, title });
            } else if let Some(rest) = trimmed.strip_prefix("#### Scenario:") {
                let title = rest.trim().to_owned();
                let id = extract_prefixed_id(&title, "SCN-")
                    .unwrap_or_else(|| stable_id("SCN-derived", &title));
                let requirement_id = current_requirement_id
                    .clone()
                    .unwrap_or_else(|| "REQ-unknown".to_owned());
                scenarios.push(Scenario {
                    id,
                    title,
                    requirement_id,
                });
            }
        }
    }

    (requirements, scenarios)
}

fn extract_prefixed_id(text: &str, prefix: &str) -> Option<String> {
    text.split(|ch: char| ch.is_whitespace() || ch == ':' || ch == ',' || ch == ';')
        .find(|token| token.starts_with(prefix))
        .map(|token| token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-'))
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
}

fn lint_source(source: &SourceSnapshot) -> LintState {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    let mut classifications = Vec::with_capacity(source.tasks_parsed.len());
    let mut covered_scenarios = BTreeSet::new();

    if source.tasks_parsed.is_empty() {
        blockers.push(Blocker {
            id: "missing_required_source".to_owned(),
            source_path: source.tasks.relative_path.clone(),
            task_id: None,
            scenario_id: None,
            detail: "no stable task checklist entries found".to_owned(),
        });
    }

    let mut seen_task_ids = BTreeSet::new();
    for task in &source.tasks_parsed {
        let mut task_blockers = Vec::new();
        if !is_stable_task_id(&task.id) {
            task_blockers.push("malformed task id".to_owned());
        }
        if !seen_task_ids.insert(task.id.clone()) {
            task_blockers.push("duplicate task id".to_owned());
        }

        let task_type = task_field(task, "Type").unwrap_or("unknown");
        let executable = is_executable_task_type(task_type);
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
        }

        if task.raw_block.contains("Risk Level:** high")
            || task.raw_block.contains("Risk Level:** critical")
        {
            let approval = task_field(task, "Human Approval Required").unwrap_or_default();
            if approval.contains("pending") || approval.contains("none") || approval.is_empty() {
                task_blockers.push("unresolved high or critical assumption approval".to_owned());
            }
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
                id: if detail.contains("path")
                    || detail.contains("Allowed Write Paths")
                    || detail.contains("write scope")
                    || detail.contains("protected")
                {
                    "unsafe_path_policy".to_owned()
                } else if detail.contains("task id") {
                    "malformed_stable_task_id".to_owned()
                } else {
                    "mission_incomplete_executable_task".to_owned()
                },
                source_path: task.source_path.clone(),
                task_id: Some(task.id.clone()),
                scenario_id: None,
                detail,
            });
        }

        let task_digest = sha256_digest(
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

fn task_field<'a>(task: &'a SourceTask, field: &str) -> Option<&'a str> {
    task.fields.get(field).map(String::as_str)
}

fn has_required_task_field(task: &SourceTask, field: &str) -> bool {
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

fn is_empty_or_none(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.is_empty()
        || normalized == "none"
        || normalized.starts_with("none -")
        || normalized.contains("<")
        || normalized.contains(">")
}

fn is_executable_task_type(task_type: &str) -> bool {
    matches!(
        task_type.trim().to_ascii_lowercase().as_str(),
        "implementation" | "test" | "documentation" | "migration" | "refactor" | "apply"
    )
}

fn is_stable_task_id(task_id: &str) -> bool {
    task_id.contains('.')
        && task_id
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
}

fn path_policy_blockers(task: &SourceTask) -> Vec<String> {
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

fn split_path_list(value: &str) -> Vec<String> {
    value
        .split([',', '\n'])
        .map(|item| {
            item.trim()
                .trim_start_matches("- ")
                .trim_matches('`')
                .to_owned()
        })
        .filter(|item| !item.is_empty() && !item.starts_with("none -"))
        .collect()
}

#[derive(Debug, Clone)]
struct LintState {
    blockers: Vec<Blocker>,
    warnings: Vec<Warning>,
    classifications: Vec<TaskClassification>,
}

fn artifact_relative_path(project_root: &Path, artifact_path: &Path) -> String {
    normalize_relative_path(
        artifact_path
            .strip_prefix(project_root)
            .unwrap_or(artifact_path),
    )
}

impl LintState {
    fn has_blockers(&self) -> bool {
        !self.blockers.is_empty()
    }
}

fn report_from_lint(
    source: &SourceSnapshot,
    lint: LintState,
    created_artifacts: Vec<String>,
    artifact_digests: BTreeMap<String, String>,
) -> MissionReport {
    let status = if lint.blockers.is_empty() {
        ReportStatus::Ready
    } else {
        ReportStatus::Blocked
    };
    let task_counts = task_counts(&lint.classifications);
    MissionReport {
        schema: "better-droid.compile-report.v1",
        change_id: source.change_id.clone(),
        status,
        import_ready: status == ReportStatus::Ready,
        checked_artifact_paths: source_path_list(source),
        created_artifacts,
        source_digests: source_digest_map(source),
        artifact_digests,
        task_counts,
        blockers: lint.blockers,
        warnings: lint.warnings,
        task_classifications: lint.classifications,
        non_canonical_run_metadata: BTreeMap::new(),
    }
}

fn task_counts(classifications: &[TaskClassification]) -> TaskCounts {
    let mut counts = TaskCounts {
        total: classifications.len(),
        ..TaskCounts::default()
    };
    for classification in classifications {
        match classification.import_status {
            ImportStatus::Importable => counts.importable += 1,
            ImportStatus::Blocked => counts.blocked += 1,
            ImportStatus::Rejected => counts.rejected += 1,
            ImportStatus::Deferred => counts.deferred += 1,
        }
    }
    counts
}

fn source_path_list(source: &SourceSnapshot) -> Vec<String> {
    let mut paths = vec![
        source.openspec_yaml.relative_path.clone(),
        source.proposal.relative_path.clone(),
        source.design.relative_path.clone(),
        source.tasks.relative_path.clone(),
    ];
    paths.extend(source.specs.iter().map(|spec| spec.relative_path.clone()));
    paths.sort();
    paths
}

fn source_digest_map(source: &SourceSnapshot) -> BTreeMap<String, String> {
    let mut digests = BTreeMap::new();
    digests.insert(
        source.openspec_yaml.relative_path.clone(),
        source.openspec_yaml.digest.clone(),
    );
    digests.insert(
        source.proposal.relative_path.clone(),
        source.proposal.digest.clone(),
    );
    digests.insert(
        source.design.relative_path.clone(),
        source.design.digest.clone(),
    );
    digests.insert(
        source.tasks.relative_path.clone(),
        source.tasks.digest.clone(),
    );
    for spec in &source.specs {
        digests.insert(spec.relative_path.clone(), spec.digest.clone());
    }
    digests
}

fn build_artifacts(source: &SourceSnapshot, lint: &LintState) -> BTreeMap<&'static str, Value> {
    let tasks = compiled_tasks(source, lint);
    let task_ids = tasks
        .iter()
        .map(|task| task.task_id.clone())
        .collect::<Vec<_>>();
    let validation_ids = validation_ids(source);
    let source_digests = source_digest_map(source);
    let source_digest = canonical_json_digest(
        &serde_json::to_value(&source_digests).expect("source digest map is serializable"),
    )
    .expect("source digest map canonicalization succeeds");
    let objective = first_objective(&source.proposal.text);

    let mission = json!({
        "schema": "better-droid.mission.v1",
        "change_id": source.change_id,
        "change_path": source.relative_change_path,
        "source_digests": source_digests,
        "artifact_digests": {},
        "objective": objective,
        "done_definition": ["lint rejects mission-incomplete source", "compile emits canonical JSON packet set", "deterministic digest checks pass"],
        "scope_in": ["better-droid lint", "better-droid compile", "canonical JSON artifacts"],
        "scope_out": ["Covey import", "live claims", "reviews", "apply queue", "settlement"],
        "affected_capabilities": ["better-droid-lint-compile-first"],
        "risk_level": "medium",
        "operator_approval": {
            "required": true,
            "status": "pending",
            "reason": "compiler output can gate future autonomous work"
        },
        "tasks": tasks,
    });

    let trace_rows = source
        .scenarios
        .iter()
        .map(|scenario| {
            let mapped_task_ids = tasks
                .iter()
                .filter(|task| task.scenario_ids.contains(&scenario.id))
                .map(|task| task.task_id.clone())
                .collect::<Vec<_>>();
            json!({
                "requirement_id": scenario.requirement_id,
                "requirement_title": source.requirements.iter().find(|req| req.id == scenario.requirement_id).map(|req| req.title.as_str()).unwrap_or("unknown"),
                "scenario_id": scenario.id,
                "scenario_title": scenario.title,
                "task_ids": mapped_task_ids,
                "expected_paths": [],
                "validation_refs": validation_ids,
                "artifact_digest": null,
                "review_verdict": null,
                "status": if mapped_task_ids.is_empty() { "planned" } else { "covered" },
                "deferral_reason": null
            })
        })
        .collect::<Vec<_>>();

    let validation_checks = validation_ids
        .iter()
        .map(|id| {
            json!({
                "id": id,
                "phase": "worker",
                "task_ids": task_ids,
                "scenario_ids": source.scenarios.iter().map(|scenario| scenario.id.clone()).collect::<Vec<_>>(),
                "command": format!("cargo test {} --all-targets", id.trim_start_matches("VAL-BDLCF-")),
                "working_directory": "covey",
                "expected_exit_code": 0,
                "evidence_required": ["command", "working_directory", "exit_code", "stdout_stderr", "source_revision"]
            })
        })
        .collect::<Vec<_>>();

    let review_items = review_rubric_items();

    let mut artifacts = BTreeMap::new();
    artifacts.insert("mission.json", mission);
    artifacts.insert(
        "mission-packet.json",
        build_mission_packet(
            source,
            &tasks,
            &validation_checks,
            &review_items,
            &source_digest,
        ),
    );
    artifacts.insert(
        "traceability.json",
        json!({
            "schema": "better-droid.traceability.v1",
            "change_id": source.change_id,
            "rows": trace_rows
        }),
    );
    artifacts.insert(
        "validation.json",
        json!({
            "schema": "better-droid.validation.v1",
            "change_id": source.change_id,
            "checks": validation_checks
        }),
    );
    artifacts.insert(
        "path-policy.json",
        json!({
            "schema": "better-droid.path-policy.v1",
            "change_id": source.change_id,
            "default_policy": "deny-write-unless-allowed",
            "allowed_read_paths": ["openspec/changes/**", "openspec/config.yaml", "openspec/schemas/better-droid/**"],
            "allowed_write_paths": [format!("{}/mission/*.json", source.relative_change_path)],
            "forbidden_write_paths": CANONICAL_PROTECTED_FORBIDDEN_PATHS,
            "generated_paths": [format!("{}/mission/*.json", source.relative_change_path)],
            "task_overrides": [],
            "reservation_policy": {
                "required_for_autonomous_mutation": true,
                "scope_class": "generated-set"
            }
        }),
    );
    artifacts.insert(
        "review-rubric.json",
        json!({
            "schema": "better-droid.review-rubric.v1",
            "change_id": source.change_id,
            "items": review_items
        }),
    );
    artifacts.insert(
        "assumptions.json",
        json!({
            "schema": "better-droid.assumptions.v1",
            "change_id": source.change_id,
            "assumptions": [],
            "blocked_task_refs": [],
            "approval_summary": {
                "high_or_critical_pending": 0
            }
        }),
    );
    artifacts.insert(
        "compile-report.json",
        json!({
            "schema": "better-droid.compile-report.v1",
            "change_id": source.change_id,
            "status": "ready",
            "import_ready": true,
            "created_artifacts": ARTIFACT_NAMES,
            "source_digests": source_digest_map(source),
            "artifact_digests": {},
            "task_counts": task_counts(&lint.classifications),
            "blockers": [],
            "warnings": lint.warnings,
            "task_classifications": lint.classifications,
            "non_canonical_run_metadata": {}
        }),
    );
    artifacts
}

fn build_mission_packet(
    source: &SourceSnapshot,
    tasks: &[CompiledTask],
    validation_checks: &[Value],
    review_items: &[Value],
    source_digest: &str,
) -> Value {
    let change_id = source.change_id.as_str();
    let objective = first_objective(&source.proposal.text);
    let allowed_paths = mission_packet_allowed_paths(tasks);
    let prompt = mission_packet_prompt(change_id, &objective, tasks);

    json!({
        "schema_version": "mission_packet.v1",
        "mission": {
            "id": change_id,
            "title": objective,
        },
        "provenance": {
            "compiler": "better-droid",
            "source_revision": "unknown",
            "source_digest": source_digest,
        },
        "scheduler": {
            "request_id": change_id,
            "observed_at": "2026-04-26T00:00:00Z",
            "identity_refs": {
                "queue_item_ref": format!("queue/ref:{change_id}"),
                "review_ref": format!("review/ref:{change_id}"),
                "artifact_ref": format!("artifact/ref:{change_id}"),
                "apply_gate_ref": format!("apply-gate/ref:{change_id}"),
                "selected_attempt": {
                    "subtask_ref": format!("covey:subtask:{change_id}"),
                    "claim_ref": format!("covey:claim:{change_id}"),
                    "fence_seq": 1,
                    "attempt_id": format!("attempt-{change_id}"),
                    "snapshot_digest": format!("blake3:snapshot:{change_id}"),
                    "covey_event_watermark": "covey-event-seq:1",
                    "evaluator_version": "better-droid:mission-packet",
                },
            },
            "activation_result": {
                "Ready": {
                    "classification": "AllowCurrent",
                    "lineage": {
                        "artifact_digest": source_digest,
                        "apply_attempt_id": format!("attempt-{change_id}"),
                        "landing_token": format!("landing-{change_id}"),
                        "repo_identity": "repo-main",
                        "pending_commit_token": null,
                    }
                }
            }
        },
        "runtime": {
            "fleet_snapshot": {
                "observed_at": "2026-04-26T00:00:01Z",
                "agents": [{
                    "agent_id": format!("agent-{change_id}"),
                    "agent_type": "codex:smart",
                    "state": "Ready",
                    "in_flight": 0,
                    "max_in_flight": 1,
                    "capabilities": ["rust", "better-droid", "mission-runner"],
                    "retry_after_seconds": null,
                    "rate_limit": null,
                }],
                "global_rate_limit": null,
            },
            "intent_available_at": "2026-04-26T00:00:02Z",
            "executor_worker_id": format!("worker-{change_id}"),
            "executor_lease_id": format!("lease-{change_id}"),
            "executor_now": "2026-04-26T00:00:10Z",
            "executor_lease_until": "2026-04-26T00:00:20Z",
            "executor_max_attempts": 3,
            "dispatch_observed_at": "2026-04-26T00:00:12Z",
            "retry_available_at": "2026-04-26T00:01:00Z",
            "provider_mode": "FakeDeliver",
        },
        "provider": {
            "provider_id": "better-droid",
            "model_id": "compiled-mission",
            "auth_identity_id": null,
            "project_path": ".",
            "session_id": format!("session-{change_id}"),
            "cache_lane": null,
            "messages": [{
                "role": "User",
                "parts": [{
                    "Text": {
                        "text": prompt,
                    }
                }]
            }],
            "tools": [],
            "tool_choice": "None",
            "tool_results": [],
            "system": null,
            "max_tokens": null,
            "temperature": null,
            "billing_identity_id": null,
        },
        "path_policy": {
            "mutation_allowed": false,
            "allowed_paths": allowed_paths,
        },
        "repoops": null,
        "validation": validation_checks.iter().map(|check| {
            json!({
                "id": check.get("id").cloned().unwrap_or(Value::Null),
                "command": check.get("command").cloned().unwrap_or(Value::Null),
            })
        }).collect::<Vec<_>>(),
        "review_rubric": review_items.iter().map(|item| {
            json!({
                "id": item.get("id").cloned().unwrap_or(Value::Null),
                "criterion": item.get("question").cloned().unwrap_or(Value::Null),
            })
        }).collect::<Vec<_>>(),
        "assumptions": [],
    })
}

fn mission_packet_allowed_paths(tasks: &[CompiledTask]) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for task in tasks {
        paths.extend(
            task.allowed_read_paths
                .iter()
                .chain(task.allowed_write_paths.iter())
                .filter(|path| is_repo_path_pattern(path))
                .cloned(),
        );
    }
    if paths.is_empty() {
        paths.insert("openspec/changes/**".to_owned());
    }
    paths.into_iter().collect()
}

fn is_repo_path_pattern(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains("..")
        && !path.chars().any(char::is_whitespace)
        && (path.contains('/') || path.contains('.') || path.contains('*'))
}

fn mission_packet_prompt(change_id: &str, objective: &str, tasks: &[CompiledTask]) -> String {
    let mut prompt = format!("Execute Better Droid mission {change_id}: {objective}\nTasks:");
    for task in tasks {
        prompt.push_str(&format!(
            "\n- {} {}: {}",
            task.task_id, task.title, task.purpose
        ));
    }
    prompt
}

fn compiled_tasks(source: &SourceSnapshot, lint: &LintState) -> Vec<CompiledTask> {
    source
        .tasks_parsed
        .iter()
        .map(|task| {
            let classification = lint
                .classifications
                .iter()
                .find(|candidate| candidate.task_id == task.id)
                .expect("lint classification exists for parsed task");
            CompiledTask {
                task_id: task.id.clone(),
                title: task.title.clone(),
                task_type: task_field(task, "Type").unwrap_or("unknown").to_owned(),
                import_status: classification.import_status,
                purpose: task_field(task, "Purpose").unwrap_or_default().to_owned(),
                acceptance_criteria: lines_from_field(task_field(task, "Acceptance Criteria")),
                requirement_ids: ids_in_text(&task.raw_block, "REQ-"),
                scenario_ids: ids_in_text(&task.raw_block, "SCN-"),
                dependencies: lines_from_field(task_field(task, "Dependencies")),
                allowed_read_paths: split_path_list(
                    task_field(task, "Allowed Read Paths").unwrap_or_default(),
                ),
                allowed_write_paths: split_path_list(
                    task_field(task, "Allowed Write Paths").unwrap_or_default(),
                ),
                forbidden_write_paths: split_path_list(
                    task_field(task, "Forbidden Paths").unwrap_or_default(),
                ),
                validation_refs: ids_in_text(&task.raw_block, "VAL-"),
                expected_evidence: lines_from_field(task_field(task, "Validation / Evidence")),
                expected_artifact_kind: task_field(task, "Expected Artifact Kind")
                    .unwrap_or("none")
                    .to_owned(),
                review_rubric_refs: vec![
                    "RR-artifact-digest".to_owned(),
                    "RR-traceability-complete".to_owned(),
                    "RR-validation-evidence".to_owned(),
                ],
                stale_if: lines_from_field(task_field(task, "Stale If")),
                task_digest: classification.task_digest.clone(),
            }
        })
        .collect()
}

fn lines_from_field(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .lines()
        .map(|line| line.trim().trim_start_matches("- ").to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

fn ids_in_text(text: &str, prefix: &str) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for token in text.split(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                ',' | ';' | ':' | ')' | '(' | '[' | ']' | '`' | '"' | '\'' | '.'
            )
    }) {
        if token.starts_with(prefix) {
            ids.insert(
                token
                    .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
                    .to_owned(),
            );
        }
    }
    ids.into_iter().collect()
}

fn validation_ids(source: &SourceSnapshot) -> Vec<String> {
    let mut ids = ids_in_text(&source.tasks.text, "VAL-BDLCF-");
    if ids.is_empty() {
        ids.push("VAL-BDLCF-openspec".to_owned());
    }
    ids
}

fn review_rubric_items() -> Vec<Value> {
    vec![
        json!({
            "id": "RR-artifact-digest",
            "severity": "blocker",
            "question": "Does the reviewed artifact digest match the artifact under review?",
            "required_for_approval": true,
            "evidence_refs": ["artifact_digests"]
        }),
        json!({
            "id": "RR-traceability-complete",
            "severity": "blocker",
            "question": "Are all requirements and scenarios covered by task and validation evidence or explicit deferral?",
            "required_for_approval": true,
            "evidence_refs": ["traceability.json"]
        }),
        json!({
            "id": "RR-validation-evidence",
            "severity": "blocker",
            "question": "Does validation evidence include command, working directory, exit code, and output summary?",
            "required_for_approval": true,
            "evidence_refs": ["validation.json"]
        }),
    ]
}

fn first_objective(proposal: &str) -> String {
    proposal
        .lines()
        .find(|line| line.contains("Objective:"))
        .map(|line| {
            line.trim()
                .trim_start_matches("- **Objective:**")
                .trim()
                .to_owned()
        })
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| {
            "compile Better Droid OpenSpec source into canonical mission JSON".to_owned()
        })
}

fn resolve_output_dir(
    project_root: &Path,
    source: &SourceSnapshot,
    output_dir: Option<&PathBuf>,
) -> Result<PathBuf> {
    let default_output = source.change_path.join("mission");
    let requested = output_dir.map_or(default_output, |dir| {
        if dir.is_absolute() {
            dir.clone()
        } else {
            project_root.join(dir)
        }
    });
    let normalized = normalize_path_lossy(&requested);
    let allowed = normalize_path_lossy(&source.change_path.join("mission"));
    if !normalized.starts_with(&allowed) {
        return Err(BetterDroidError::OutputPathEscape {
            path: requested.display().to_string(),
        });
    }
    Ok(normalized)
}

fn normalize_path_lossy(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn canonical_json_digest(value: &Value) -> Result<String> {
    let canonical = canonicalize_value(value);
    let bytes = serde_json::to_vec(&canonical)?;
    Ok(sha256_digest(&bytes))
}

fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonicalize_value).collect()),
        Value::Object(object) => {
            let mut sorted = Map::new();
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            for key in keys {
                if key == "non_canonical_run_metadata" {
                    continue;
                }
                sorted.insert(key.clone(), canonicalize_value(&object[key]));
            }
            Value::Object(sorted)
        }
        value => value.clone(),
    }
}

fn stable_id(prefix: &str, text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    format!(
        "{prefix}-{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3]
    )
}

fn sha256_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity("sha256:".len() + digest.len() * 2);
    encoded.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn normalize_relative_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_status_display_and_report_readiness_are_stable() {
        assert_eq!(ReportStatus::Ready.to_string(), "ready");
        assert_eq!(ReportStatus::Blocked.to_string(), "blocked");

        let source = SourceSnapshot {
            change_id: "better-droid-unit".into(),
            change_path: PathBuf::from("/repo/openspec/changes/better-droid-unit"),
            relative_change_path: "openspec/changes/better-droid-unit".into(),
            schema_name: "better-droid".into(),
            openspec_yaml: source_file("openspec/changes/better-droid-unit/openspec.yaml", ""),
            proposal: source_file("openspec/changes/better-droid-unit/proposal.md", ""),
            design: source_file("openspec/changes/better-droid-unit/design.md", ""),
            tasks: source_file("openspec/changes/better-droid-unit/tasks.md", ""),
            specs: Vec::new(),
            tasks_parsed: Vec::new(),
            requirements: Vec::new(),
            scenarios: Vec::new(),
        };

        let ready = report_from_lint(
            &source,
            LintState {
                blockers: Vec::new(),
                warnings: Vec::new(),
                classifications: Vec::new(),
            },
            vec!["mission/mission.json".into()],
            BTreeMap::new(),
        );
        assert_eq!(ready.status, ReportStatus::Ready);
        assert!(ready.import_ready);

        let blocked = report_from_lint(
            &source,
            LintState {
                blockers: vec![Blocker {
                    id: "unit_blocker".into(),
                    source_path: "tasks.md".into(),
                    task_id: None,
                    scenario_id: None,
                    detail: "blocked".into(),
                }],
                warnings: Vec::new(),
                classifications: Vec::new(),
            },
            Vec::new(),
            BTreeMap::new(),
        );
        assert_eq!(blocked.status, ReportStatus::Blocked);
        assert!(!blocked.import_ready);
    }

    #[test]
    fn parsers_derive_spec_ids_and_skip_malformed_task_lines() {
        let tasks = source_file(
            "openspec/changes/unit/tasks.md",
            "\
intro
- [ ] malformed-without-title
- [X] 1.2 Implement parser coverage
  - **Type:** implementation
    continued type detail
  - **Purpose:** cover private parser branches
## Next section
- [x] 1.3 Follow up task
  - **Type:** verification
",
        );
        let parsed = parse_tasks(&tasks);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "1.2");
        assert_eq!(
            parsed[0].fields["Type"],
            "implementation\ncontinued type detail"
        );

        let specs = vec![source_file(
            "openspec/changes/unit/specs/unit/spec.md",
            "\
### Requirement: Render good output
#### Scenario: Happy path works
### Requirement: REQ-explicit Explicit id
#### Scenario: SCN-explicit Explicit scenario
",
        )];
        let (requirements, scenarios) = parse_specs(&specs);
        assert_eq!(requirements.len(), 2);
        assert!(requirements[0].id.starts_with("REQ-derived-"));
        assert_eq!(requirements[1].id, "REQ-explicit");
        assert_eq!(scenarios[0].requirement_id, requirements[0].id);
        assert_eq!(scenarios[1].id, "SCN-explicit");
    }

    #[test]
    fn path_policy_and_canonical_helpers_cover_edge_branches() {
        let task = SourceTask {
            id: "1.2".into(),
            title: "unsafe paths".into(),
            source_path: "tasks.md".into(),
            raw_block: String::new(),
            fields: BTreeMap::from([
                (
                    "Allowed Write Paths".into(),
                    "., ../escape, authority/**".into(),
                ),
                (
                    "Forbidden Paths".into(),
                    "authority/**, generated/**".into(),
                ),
            ]),
        };
        let blockers = path_policy_blockers(&task);
        assert!(
            blockers
                .iter()
                .any(|blocker| blocker == "repo-global write scope is forbidden")
        );
        assert!(
            blockers
                .iter()
                .any(|blocker| blocker == "out-of-root write scope is forbidden")
        );
        assert!(
            blockers
                .iter()
                .any(|blocker| blocker == "protected path authority/** cannot be allowed")
        );
        assert!(
            blockers
                .iter()
                .any(|blocker| blocker == "allowed and forbidden paths overlap at authority/**")
        );

        let value = json!({
            "z": [{"b": 2, "a": 1}],
            "a": true,
            "non_canonical_run_metadata": {"elapsed": "ignored"}
        });
        let canonical = canonicalize_value(&value);
        assert!(canonical.get("non_canonical_run_metadata").is_none());
        let object = canonical.as_object().expect("canonical object");
        assert_eq!(object.keys().cloned().collect::<Vec<_>>(), vec!["a", "z"]);
        assert_eq!(
            canonical_json_digest(&value).expect("digest"),
            canonical_json_digest(&canonical).expect("digest")
        );

        assert_eq!(
            normalize_path_lossy(Path::new("a/./b/../c")),
            PathBuf::from("a/c")
        );
        assert_eq!(
            stable_id("REQ-derived", "A title").len(),
            "REQ-derived-00000000".len()
        );
    }

    fn source_file(path: &str, text: &str) -> SourceFile {
        SourceFile {
            relative_path: path.into(),
            text: text.into(),
            digest: sha256_digest(text.as_bytes()),
        }
    }
}

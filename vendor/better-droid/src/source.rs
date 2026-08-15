use crate::error::{BetterDroidError, Result};
use crate::model::{PlanningClass, Requirement, Scenario, SourceFile, SourceSnapshot, SourceTask};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(crate) fn load_source(project_root: &Path, change_id: &str) -> Result<SourceSnapshot> {
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
    let planning_class_raw = parse_yaml_scalar(&openspec_yaml.text, "planning_class")
        .or_else(|| parse_yaml_scalar(&openspec_yaml.text, "planning-class"));
    let planning_class = PlanningClass::parse_yaml(planning_class_raw.as_deref());

    Ok(SourceSnapshot {
        change_id: change_id.to_owned(),
        relative_change_path: normalize_relative_path(
            change_path
                .strip_prefix(project_root)
                .unwrap_or(change_path.as_path()),
        ),
        schema_name: "better-droid".to_owned(),
        change_status: parse_yaml_scalar(&openspec_yaml.text, "status"),
        planning_class,
        planning_class_raw,
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

pub(crate) fn parse_schema_name(text: &str) -> Option<String> {
    parse_yaml_scalar(text, "schema")
}

pub(crate) fn parse_yaml_scalar(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    text.lines()
        .find_map(|line| line.trim().strip_prefix(prefix.as_str()).map(str::trim))
        .map(|value| {
            value
                .trim_matches(|character| character == '"' || character == '\'')
                .to_owned()
        })
}

pub(crate) fn read_required_file(project_root: &Path, path: &Path) -> Result<SourceFile> {
    if !path.is_file() {
        return Err(BetterDroidError::InvalidSource {
            path: normalize_relative_path(path),
            detail: "missing_required_source".to_owned(),
        });
    }

    read_source_file(project_root, path)
}

pub(crate) fn read_specs(project_root: &Path, specs_dir: &Path) -> Result<Vec<SourceFile>> {
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

pub(crate) fn collect_markdown_files(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
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

pub(crate) fn read_source_file(project_root: &Path, path: &Path) -> Result<SourceFile> {
    let bytes = fs::read(path).map_err(|source| BetterDroidError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(SourceFile {
        relative_path: normalize_relative_path(path.strip_prefix(project_root).unwrap_or(path)),
        text,
        digest: blake3_digest(&bytes),
    })
}

pub(crate) fn parse_tasks(tasks: &SourceFile) -> Vec<SourceTask> {
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
        let parsed_fields = parse_task_fields(&raw_block);
        result.push(SourceTask {
            id: task_id.to_owned(),
            title: title.trim().to_owned(),
            source_path: tasks.relative_path.clone(),
            fields: parsed_fields.fields,
            field_syntax_errors: parsed_fields.syntax_errors,
            raw_block,
        });
    }

    result
}

pub(crate) fn task_line_rest(trimmed: &str) -> Option<&str> {
    trimmed
        .strip_prefix("- [ ] ")
        .or_else(|| trimmed.strip_prefix("- [x] "))
        .or_else(|| trimmed.strip_prefix("- [X] "))
}

#[derive(Debug, Default)]
pub(crate) struct ParsedTaskFields {
    fields: BTreeMap<String, String>,
    syntax_errors: Vec<String>,
}

pub(crate) fn parse_task_fields(raw_block: &str) -> ParsedTaskFields {
    let mut fields = BTreeMap::new();
    let mut syntax_errors = Vec::new();
    let mut current: Option<String> = None;

    for line in raw_block.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- **")
            && let Some((field, value)) = rest.split_once(":**")
        {
            let key = canonical_task_field_name(field.trim());
            let value = value.trim().to_owned();
            current = Some(key.clone());
            fields.insert(key, value);
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("- ")
            && let Some((field, _value)) = rest.split_once(':')
            && recognized_task_field_name(field.trim())
        {
            syntax_errors.push(format!(
                "non-canonical task field syntax for {}; use `- **{}:** ...`",
                canonical_task_field_name(field.trim()),
                canonical_task_field_name(field.trim())
            ));
            current = None;
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

    ParsedTaskFields {
        fields,
        syntax_errors,
    }
}

pub(crate) fn recognized_task_field_name(field: &str) -> bool {
    matches!(
        canonical_task_field_name(field).as_str(),
        "Type"
            | "Readiness"
            | "Authority Owner"
            | "Purpose"
            | "Scope In"
            | "Scope Out"
            | "Dependencies"
            | "Preconditions"
            | "Source Freshness / Revalidation"
            | "Dirty Worktree Protection"
            | "Base Revision / Source Digest Expectations"
            | "Assumptions"
            | "Assumptions To Verify"
            | "Risk Level"
            | "Human Approval Required"
            | "Allowed Read Paths"
            | "Allowed Write Paths"
            | "Generated Paths"
            | "Forbidden Paths"
            | "Path Conflict Policy"
            | "Acceptance Criteria"
            | "Failure / Negative Cases"
            | "Validation / Evidence"
            | "Command / Action"
            | "Working Directory"
            | "Expected Exit Code / Observation"
            | "Required Evidence"
            | "Expected Evidence"
            | "Expected Artifact Kind"
            | "Review Evidence Binding"
            | "Review Checklist"
            | "Traceability Refs"
            | "Stale If"
            | "Covers"
    )
}

pub(crate) fn canonical_task_field_name(field: &str) -> String {
    match field.trim().to_ascii_lowercase().as_str() {
        "type" => "Type",
        "readiness" => "Readiness",
        "authority owner" => "Authority Owner",
        "purpose" => "Purpose",
        "scope in" => "Scope In",
        "scope out" => "Scope Out",
        "dependencies" => "Dependencies",
        "preconditions" => "Preconditions",
        "source freshness / revalidation" => "Source Freshness / Revalidation",
        "dirty worktree protection" => "Dirty Worktree Protection",
        "base revision / source digest expectations" => {
            "Base Revision / Source Digest Expectations"
        }
        "assumptions" => "Assumptions",
        "assumptions to verify" => "Assumptions To Verify",
        "risk level" => "Risk Level",
        "human approval required" => "Human Approval Required",
        "allowed read paths" => "Allowed Read Paths",
        "allowed write paths" => "Allowed Write Paths",
        "generated paths" => "Generated Paths",
        "forbidden paths" => "Forbidden Paths",
        "path conflict policy" => "Path Conflict Policy",
        "acceptance criteria" => "Acceptance Criteria",
        "failure / negative cases" => "Failure / Negative Cases",
        "validation" | "validation / evidence" => "Validation / Evidence",
        "command / action" => "Command / Action",
        "working directory" => "Working Directory",
        "expected exit code / observation" => "Expected Exit Code / Observation",
        "required evidence" => "Required Evidence",
        "covers" => "Covers",
        "expected evidence" => "Expected Evidence",
        "expected artifact kind" => "Expected Artifact Kind",
        "review evidence binding" => "Review Evidence Binding",
        "review checklist" => "Review Checklist",
        "traceability refs" => "Traceability Refs",
        "stale-if" | "stale if" => "Stale If",
        _ => field.trim(),
    }
    .to_owned()
}

pub(crate) fn parse_specs(specs: &[SourceFile]) -> (Vec<Requirement>, Vec<Scenario>) {
    let mut requirements = Vec::new();
    let mut scenarios = Vec::new();
    let mut current_requirement_id: Option<String> = None;

    for spec in specs {
        for line in spec.text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("### Requirement:") {
                let title = rest.trim().to_owned();
                let id = extract_prefixed_id(&title, "REQ-")
                    .or_else(|| extract_domain_requirement_id(&title))
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

pub(crate) fn extract_prefixed_id(text: &str, prefix: &str) -> Option<String> {
    text.split(|ch: char| ch.is_whitespace() || ch == ':' || ch == ',' || ch == ';')
        .find(|token| token.starts_with(prefix))
        .map(|token| token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-'))
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
}

pub(crate) fn extract_domain_requirement_id(text: &str) -> Option<String> {
    text.split(|ch: char| ch.is_whitespace() || ch == ':' || ch == ',' || ch == ';')
        .map(|token| token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-'))
        .find(|token| {
            token.strip_prefix("NZM-").is_some_and(|rest| {
                rest.split('-')
                    .next()
                    .is_some_and(|domain| !domain.is_empty())
            })
        })
        .filter(|token| {
            token
                .rsplit('-')
                .next()
                .is_some_and(|suffix| suffix.chars().all(|ch| ch.is_ascii_digit()))
        })
        .map(str::to_owned)
}

pub(crate) fn artifact_relative_path(project_root: &Path, artifact_path: &Path) -> String {
    normalize_relative_path(
        artifact_path
            .strip_prefix(project_root)
            .unwrap_or(artifact_path),
    )
}
pub(crate) fn resolve_output_dir(
    project_root: &Path,
    source: &SourceSnapshot,
    output_dir: Option<&PathBuf>,
) -> Result<PathBuf> {
    let default_output = generated_mission_dir(project_root, &source.change_id);
    let requested = output_dir.map_or(default_output, |dir| {
        if dir.is_absolute() {
            dir.clone()
        } else {
            project_root.join(dir)
        }
    });
    let normalized = normalize_path_lossy(&requested);
    let normalized_root = normalize_path_lossy(project_root);
    if !normalized.starts_with(&normalized_root)
        || is_openspec_change_mission_dir(&normalized, &normalized_root)
    {
        return Err(BetterDroidError::OutputPathEscape {
            path: requested.display().to_string(),
        });
    }
    Ok(normalized)
}

pub(crate) fn generated_mission_dir(project_root: &Path, change_id: &str) -> PathBuf {
    project_root
        .join(".codex")
        .join("state")
        .join("better-droid")
        .join(change_id)
        .join("mission")
}

fn is_openspec_change_mission_dir(path: &Path, project_root: &Path) -> bool {
    let relative = path.strip_prefix(project_root).unwrap_or(path);
    let parts = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    if parts.len() < 4 {
        return false;
    }
    parts
        .windows(4)
        .any(|window| window[0] == "openspec" && window[1] == "changes" && window[3] == "mission")
}

pub(crate) fn normalize_path_lossy(path: &Path) -> PathBuf {
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

pub(crate) fn canonical_json_digest(value: &Value) -> Result<String> {
    let canonical = canonicalize_value(value);
    let bytes = serde_json::to_vec(&canonical)?;
    Ok(blake3_digest(&bytes))
}

pub(crate) fn canonicalize_value(value: &Value) -> Value {
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

pub(crate) fn stable_id(prefix: &str, text: &str) -> String {
    let digest = blake3::hash(text.as_bytes());
    let bytes = digest.as_bytes();
    format!(
        "{prefix}-{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3]
    )
}

pub(crate) fn blake3_digest(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

pub(crate) fn normalize_relative_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use serde_json::{Map, Value};

use crate::{
    error::{CoveyError, Result},
    model::OpenSpecSourceDigest,
};

use super::{
    OpenSpecSourceTask,
    util::{normalize_relative_path, blake3_digest},
};

const REQUIRED_ARTIFACTS: &[&str] = &[
    "mission.json",
    "traceability.json",
    "validation.json",
    "path-policy.json",
    "review-rubric.json",
    "assumptions.json",
    "compile-report.json",
];

pub(super) struct MissionPacket {
    pub(super) tasks: Vec<OpenSpecSourceTask>,
    pub(super) source_digests: Vec<OpenSpecSourceDigest>,
    pub(super) artifact_digests: Vec<OpenSpecSourceDigest>,
    pub(super) artifact_paths: Vec<String>,
}

pub(super) fn load_mission_packet(
    root: &Path,
    change_id: &str,
    change_path: &Path,
) -> Result<MissionPacket> {
    let mission_dir = root.join(change_path).join("mission");
    if !mission_dir.is_dir() {
        return Err(invalid_source(
            &mission_dir,
            "missing compiled Better Droid mission directory",
        ));
    }

    let mut values = HashMap::with_capacity(REQUIRED_ARTIFACTS.len());
    for artifact in REQUIRED_ARTIFACTS {
        let path = mission_dir.join(artifact);
        if !path.is_file() {
            return Err(invalid_source(
                &path,
                "missing required Better Droid mission artifact",
            ));
        }
        let text = fs::read_to_string(&path).map_err(|err| CoveyError::InvalidSourceSchema {
            path: path.to_string_lossy().to_string(),
            detail: format!("failed to read Better Droid mission artifact: {err}"),
        })?;
        let value = serde_json::from_str::<Value>(&text).map_err(|err| {
            CoveyError::InvalidSourceSchema {
                path: path.to_string_lossy().to_string(),
                detail: format!("invalid Better Droid mission JSON: {err}"),
            }
        })?;
        values.insert((*artifact).to_owned(), value);
    }

    let report = values
        .get("compile-report.json")
        .expect("required artifact loaded");
    validate_report(change_id, root, change_path, &mission_dir, report, &values)?;
    validate_artifact_schemas(change_id, &values)?;
    let tasks = load_tasks(change_id, change_path, &values)?;

    Ok(MissionPacket {
        tasks,
        source_digests: digest_map(report, "source_digests", "compile-report.json")?,
        artifact_digests: digest_map(report, "artifact_digests", "compile-report.json")?,
        artifact_paths: string_array(report, "created_artifacts", "compile-report.json")?,
    })
}

fn validate_report(
    change_id: &str,
    root: &Path,
    change_path: &Path,
    mission_dir: &Path,
    report: &Value,
    values: &HashMap<String, Value>,
) -> Result<()> {
    require_string_eq(
        report,
        "schema",
        "better-droid.compile-report.v1",
        "compile-report.json",
    )?;
    require_string_eq(report, "change_id", change_id, "compile-report.json")?;
    require_string_eq(report, "status", "ready", "compile-report.json")?;
    if report.get("import_ready").and_then(Value::as_bool) != Some(true) {
        return Err(invalid_artifact(
            "compile-report.json",
            "compile-report import_ready must be true",
        ));
    }
    if !array_is_empty(report, "blockers") {
        return Err(invalid_artifact(
            "compile-report.json",
            "compile-report contains blockers",
        ));
    }

    let created = string_array(report, "created_artifacts", "compile-report.json")?;
    for artifact in REQUIRED_ARTIFACTS {
        let expected = normalize_relative_path(&change_path.join("mission").join(artifact));
        if !created.iter().any(|path| path == &expected) {
            return Err(invalid_artifact(
                "compile-report.json",
                &format!("created_artifacts missing {expected}"),
            ));
        }
    }

    let artifact_digests = digest_map(report, "artifact_digests", "compile-report.json")?;
    for artifact in REQUIRED_ARTIFACTS {
        let relative_path = normalize_relative_path(&change_path.join("mission").join(artifact));
        let expected = artifact_digests
            .iter()
            .find(|digest| digest.path == relative_path)
            .ok_or_else(|| {
                invalid_artifact(
                    "compile-report.json",
                    &format!("artifact_digests missing {relative_path}"),
                )
            })?;
        let value = values
            .get(*artifact)
            .expect("required artifact loaded before validation");
        let actual = if *artifact == "compile-report.json" {
            compile_report_recorded_digest(value, &relative_path)?
        } else {
            canonical_json_digest(value)?
        };
        if actual != expected.digest {
            let path = root.join(mission_dir).join(artifact);
            return Err(CoveyError::InvalidSourceSchema {
                path: path.to_string_lossy().to_string(),
                detail: format!("stale Better Droid mission artifact digest for {relative_path}"),
            });
        }
    }

    Ok(())
}

fn validate_artifact_schemas(change_id: &str, values: &HashMap<String, Value>) -> Result<()> {
    let schemas = [
        ("mission.json", "better-droid.mission.v1"),
        ("traceability.json", "better-droid.traceability.v1"),
        ("validation.json", "better-droid.validation.v1"),
        ("path-policy.json", "better-droid.path-policy.v1"),
        ("review-rubric.json", "better-droid.review-rubric.v1"),
        ("assumptions.json", "better-droid.assumptions.v1"),
    ];
    for (artifact, schema) in schemas {
        let value = values.get(artifact).expect("required artifact loaded");
        require_string_eq(value, "schema", schema, artifact)?;
        require_string_eq(value, "change_id", change_id, artifact)?;
    }

    let assumptions = values
        .get("assumptions.json")
        .expect("required artifact loaded");
    let pending = assumptions
        .pointer("/approval_summary/high_or_critical_pending")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if pending > 0 {
        return Err(invalid_artifact(
            "assumptions.json",
            "unapproved high or critical assumptions block import",
        ));
    }

    validate_path_policy(
        values
            .get("path-policy.json")
            .expect("required artifact loaded"),
    )?;
    Ok(())
}

fn validate_path_policy(path_policy: &Value) -> Result<()> {
    for field in ["allowed_write_paths", "generated_paths"] {
        for path in string_array(path_policy, field, "path-policy.json")? {
            if is_broad_path(&path) {
                return Err(invalid_artifact(
                    "path-policy.json",
                    &format!("broad path policy is not importable: {path}"),
                ));
            }
        }
    }
    Ok(())
}

fn load_tasks(
    change_id: &str,
    change_path: &Path,
    values: &HashMap<String, Value>,
) -> Result<Vec<OpenSpecSourceTask>> {
    let mission = values
        .get("mission.json")
        .expect("required artifact loaded");
    let report = values
        .get("compile-report.json")
        .expect("required artifact loaded");
    let mission_tasks = mission
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_artifact("mission.json", "missing tasks array"))?;
    let classifications = report
        .get("task_classifications")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            invalid_artifact("compile-report.json", "missing task_classifications array")
        })?;

    let mut classification_by_id = HashMap::with_capacity(classifications.len());
    for classification in classifications {
        let task_id = required_string(classification, "task_id", "compile-report.json")?;
        if classification_by_id
            .insert(task_id.to_owned(), classification)
            .is_some()
        {
            return Err(invalid_artifact(
                "compile-report.json",
                &format!("duplicate task id {task_id}"),
            ));
        }
    }

    let mut seen = HashSet::new();
    let mut tasks = Vec::with_capacity(mission_tasks.len());
    for task in mission_tasks {
        let task_id = required_string(task, "task_id", "mission.json")?;
        validate_task_id(task_id, "mission.json")?;
        if !seen.insert(task_id.to_owned()) {
            return Err(invalid_artifact(
                "mission.json",
                &format!("duplicate task id {task_id}"),
            ));
        }
        let title = required_string(task, "title", "mission.json")?;
        if title.trim().is_empty() {
            return Err(invalid_artifact(
                "mission.json",
                &format!("missing title for task {task_id}"),
            ));
        }
        let classification = classification_by_id.get(task_id).ok_or_else(|| {
            invalid_artifact(
                "compile-report.json",
                &format!("missing classification for task {task_id}"),
            )
        })?;
        require_string_eq(
            classification,
            "import_status",
            "importable",
            "compile-report.json",
        )?;
        let task_digest = required_string(classification, "task_digest", "compile-report.json")?;
        validate_digest(task_digest, "compile-report.json")?;
        if required_string(classification, "title", "compile-report.json")? != title {
            return Err(invalid_artifact(
                "compile-report.json",
                &format!("classification title mismatch for task {task_id}"),
            ));
        }
        let task_type = required_string(classification, "task_type", "compile-report.json")?;

        tasks.push(OpenSpecSourceTask {
            task_id: task_id.to_owned(),
            title: title.to_owned(),
            source_path: normalize_relative_path(&change_path.join("mission").join("mission.json")),
            task_digest: task_digest.to_owned(),
            task_type: Some(task_type.to_owned()),
        });
    }

    if tasks.is_empty() {
        return Err(invalid_artifact(
            "mission.json",
            &format!("no importable compiled tasks for {change_id}"),
        ));
    }
    if classification_by_id.len() != seen.len() {
        return Err(invalid_artifact(
            "compile-report.json",
            "task_classifications contains task ids not present in mission.json",
        ));
    }
    Ok(tasks)
}

fn digest_map(value: &Value, field: &str, artifact: &str) -> Result<Vec<OpenSpecSourceDigest>> {
    let object = value
        .get(field)
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_artifact(artifact, &format!("missing {field} object")))?;
    let mut digests = Vec::with_capacity(object.len());
    for (path, digest) in object {
        let digest = digest.as_str().ok_or_else(|| {
            invalid_artifact(
                artifact,
                &format!("{field} value for {path} must be a string"),
            )
        })?;
        validate_digest(digest, artifact)?;
        digests.push(OpenSpecSourceDigest::new(path.clone(), digest.to_owned()));
    }
    digests.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(digests)
}

fn string_array(value: &Value, field: &str, artifact: &str) -> Result<Vec<String>> {
    let array = value
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_artifact(artifact, &format!("missing {field} array")))?;
    array
        .iter()
        .map(|item| {
            item.as_str().map(str::to_owned).ok_or_else(|| {
                invalid_artifact(artifact, &format!("{field} entries must be strings"))
            })
        })
        .collect()
}

fn require_string_eq(value: &Value, field: &str, expected: &str, artifact: &str) -> Result<()> {
    let actual = required_string(value, field, artifact)?;
    if actual == expected {
        Ok(())
    } else {
        Err(invalid_artifact(
            artifact,
            &format!("{field} must be {expected}"),
        ))
    }
}

fn required_string<'a>(value: &'a Value, field: &str, artifact: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_artifact(artifact, &format!("missing {field} string")))
}

fn array_is_empty(value: &Value, field: &str) -> bool {
    value
        .get(field)
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
}

fn canonical_json_digest(value: &Value) -> Result<String> {
    let canonical = canonicalize_value(value);
    let bytes = serde_json::to_vec(&canonical).map_err(CoveyError::from)?;
    Ok(blake3_digest(&bytes))
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

fn compile_report_recorded_digest(report: &Value, self_path: &str) -> Result<String> {
    let mut report_without_self = report.clone();
    let digests = report_without_self
        .get_mut("artifact_digests")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            invalid_artifact("compile-report.json", "missing artifact_digests object")
        })?;
    digests.remove(self_path);
    canonical_json_digest(&report_without_self)
}

fn validate_digest(digest: &str, artifact: &str) -> Result<()> {
    let hex = digest
        .strip_prefix("blake3:")
        .ok_or_else(|| invalid_artifact(artifact, "digest must start with lowercase blake3:"))?;
    if hex.len() != 64
        || !hex
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
    {
        return Err(invalid_artifact(
            artifact,
            "digest must be lowercase blake3 plus 64 hex characters",
        ));
    }
    Ok(())
}

fn validate_task_id(task_id: &str, artifact: &str) -> Result<()> {
    if !task_id.contains('.')
        || task_id
            .split('.')
            .any(|segment| segment.is_empty() || !segment.chars().all(|ch| ch.is_ascii_digit()))
    {
        return Err(invalid_artifact(
            artifact,
            &format!("invalid stable task id {task_id}"),
        ));
    }
    Ok(())
}

fn is_broad_path(path: &str) -> bool {
    matches!(path.trim(), "." | "/" | "repo" | "**/*" | "all files" | "*")
        || path.trim().starts_with("/*")
}

fn invalid_artifact(artifact: &str, detail: &str) -> CoveyError {
    CoveyError::InvalidSourceSchema {
        path: artifact.to_owned(),
        detail: detail.to_owned(),
    }
}

fn invalid_source(path: &Path, detail: &str) -> CoveyError {
    CoveyError::InvalidSourceSchema {
        path: path.to_string_lossy().to_string(),
        detail: detail.to_owned(),
    }
}

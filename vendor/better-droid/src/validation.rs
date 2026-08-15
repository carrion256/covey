use crate::error::{BetterDroidError, Result};
use crate::model::{
    ARTIFACT_NAMES, AssumptionsArtifact, CompiledMission, CompiledTask, MissionArtifact,
    PathPolicyArtifact, PlanningClass,
};
use crate::report::{ImportStatus, MissionReport, PacketKind, ReportStatus};
use crate::source::{canonical_json_digest, generated_mission_dir, normalize_relative_path};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Load and validate a compiled Better Droid mission bundle for Covey import.
///
/// # Errors
///
/// Returns an error when the mission directory is missing, an artifact is malformed, or the
/// compile report digests do not match the artifact bytes on disk.
pub fn load_compiled_mission(
    root: &Path,
    change_id: &str,
    _change_path: &Path,
) -> Result<CompiledMission> {
    let mission_dir = generated_mission_dir(root, change_id);
    if !mission_dir.is_dir() {
        return Err(invalid_compiled_artifact(
            &mission_dir,
            "missing compiled Better Droid mission directory",
        ));
    }

    let mut values = HashMap::with_capacity(ARTIFACT_NAMES.len());
    for artifact in ARTIFACT_NAMES {
        let path = mission_dir.join(artifact);
        if !path.is_file() {
            return Err(invalid_compiled_artifact(
                &path,
                "missing required Better Droid mission artifact",
            ));
        }
        let text = fs::read_to_string(&path).map_err(|source| BetterDroidError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let value = serde_json::from_str::<Value>(&text).map_err(|source| {
            BetterDroidError::InvalidSource {
                path: path.to_string_lossy().to_string(),
                detail: format!("invalid Better Droid mission JSON: {source}"),
            }
        })?;
        values.insert((*artifact).to_owned(), value);
    }

    let report_value = values
        .get("compile-report.json")
        .expect("compile report artifact loaded");
    validate_raw_compile_report_gate(change_id, &mission_dir, report_value)?;
    validate_raw_task_classifications(report_value)?;
    let report = serde_json::from_value::<MissionReport>(report_value.clone())?;
    validate_compile_report(
        change_id,
        root,
        _change_path,
        &mission_dir,
        &report,
        &values,
    )?;
    validate_compiled_artifact_schemas(change_id, &values)?;
    validate_raw_mission_tasks(values.get("mission.json").expect("mission artifact loaded"))?;

    let mission = serde_json::from_value::<MissionArtifact>(
        values
            .get("mission.json")
            .expect("mission artifact loaded")
            .clone(),
    )?;
    let tasks = importable_tasks(change_id, &mission, &report)?;

    Ok(CompiledMission {
        tasks,
        planning_class: report.planning_class,
        source_digests: report.source_digests,
        artifact_digests: report.artifact_digests,
        artifact_paths: report.created_artifacts,
    })
}

pub(crate) fn validate_raw_mission_tasks(mission: &Value) -> Result<()> {
    if mission.get("tasks").and_then(Value::as_array).is_none() {
        return Err(invalid_compiled_artifact(
            Path::new("mission.json"),
            "missing tasks array",
        ));
    }
    Ok(())
}

pub(crate) fn validate_raw_compile_report_gate(
    change_id: &str,
    mission_dir: &Path,
    report: &Value,
) -> Result<()> {
    require_artifact_string_eq(
        report,
        "schema",
        "better-droid.compile-report.v1",
        "compile-report.json",
    )?;
    require_artifact_string_eq(report, "change_id", change_id, "compile-report.json")?;
    require_artifact_string_eq(
        report,
        "planning_class",
        "work_packet",
        "compile-report.json",
    )?;
    require_artifact_string_eq(
        report,
        "status",
        "covey_import_ready",
        "compile-report.json",
    )?;
    if report.get("import_ready").and_then(Value::as_bool) != Some(true) {
        return Err(invalid_compiled_artifact(
            &mission_dir.join("compile-report.json"),
            "compile-report import_ready must be true",
        ));
    }
    if report
        .get("readiness")
        .and_then(|readiness| readiness.get("covey_import_ready"))
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err(invalid_compiled_artifact(
            &mission_dir.join("compile-report.json"),
            "compile-report readiness.covey_import_ready must be true",
        ));
    }
    if report
        .get("readiness")
        .and_then(|readiness| readiness.get("implementation_ready"))
        .and_then(Value::as_bool)
        .is_none()
    {
        return Err(invalid_compiled_artifact(
            &mission_dir.join("compile-report.json"),
            "compile-report readiness.implementation_ready must be boolean",
        ));
    }
    if !report
        .get("blockers")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err(invalid_compiled_artifact(
            &mission_dir.join("compile-report.json"),
            "compile-report contains blockers",
        ));
    }
    Ok(())
}

pub(crate) fn validate_raw_task_classifications(report: &Value) -> Result<()> {
    let classifications = report
        .get("task_classifications")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            invalid_compiled_artifact(
                Path::new("compile-report.json"),
                "missing task_classifications array",
            )
        })?;
    for classification in classifications {
        for field in [
            "task_id",
            "title",
            "task_type",
            "import_status",
            "task_digest",
        ] {
            if classification.get(field).and_then(Value::as_str).is_none() {
                return Err(invalid_compiled_artifact(
                    Path::new("compile-report.json"),
                    &format!("missing {field} string"),
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_compile_report(
    change_id: &str,
    root: &Path,
    _change_path: &Path,
    mission_dir: &Path,
    report: &MissionReport,
    values: &HashMap<String, Value>,
) -> Result<()> {
    if report.schema != "better-droid.compile-report.v1" {
        return Err(invalid_compiled_artifact(
            &mission_dir.join("compile-report.json"),
            "schema must be better-droid.compile-report.v1",
        ));
    }
    if report.change_id != change_id {
        return Err(invalid_compiled_artifact(
            &mission_dir.join("compile-report.json"),
            "change_id must match requested change",
        ));
    }
    if report.planning_class != PlanningClass::WorkPacket {
        return Err(invalid_compiled_artifact(
            &mission_dir.join("compile-report.json"),
            "planning_class must be work_packet",
        ));
    }
    if report.status != ReportStatus::CoveyImportReady {
        return Err(invalid_compiled_artifact(
            &mission_dir.join("compile-report.json"),
            "status must be covey_import_ready",
        ));
    }
    if !report.import_ready {
        return Err(invalid_compiled_artifact(
            &mission_dir.join("compile-report.json"),
            "compile-report import_ready must be true",
        ));
    }
    if !report.readiness.covey_import_ready {
        return Err(invalid_compiled_artifact(
            &mission_dir.join("compile-report.json"),
            "readiness.covey_import_ready must be true",
        ));
    }
    if report.readiness.implementation_ready
        && (report.packet_kind != PacketKind::Implementation
            || report.product_impact.product_write_paths.is_empty())
    {
        return Err(invalid_compiled_artifact(
            &mission_dir.join("compile-report.json"),
            "readiness.implementation_ready requires an implementation packet with product write paths",
        ));
    }
    if report.readiness.covey_imported
        || report.readiness.execution_ready
        || report.readiness.review_approved
        || report.readiness.apply_queued
        || report.readiness.apply_authorized
        || report.readiness.landed
        || report.readiness.shipped_verified
    {
        return Err(invalid_compiled_artifact(
            &mission_dir.join("compile-report.json"),
            "Better Droid compile reports cannot claim Covey import, execution, review, apply, landed, or shipped evidence",
        ));
    }
    if report.readiness.landed
        || report.readiness.shipped_verified
        || report.product_impact.apply_receipt
    {
        return Err(invalid_compiled_artifact(
            &mission_dir.join("compile-report.json"),
            "Better Droid compile reports cannot claim landed or shipped evidence",
        ));
    }
    if !report.blockers.is_empty() {
        return Err(invalid_compiled_artifact(
            &mission_dir.join("compile-report.json"),
            "compile-report contains blockers",
        ));
    }

    for artifact in ARTIFACT_NAMES {
        let artifact_path = mission_dir.join(artifact);
        let expected_path =
            normalize_relative_path(artifact_path.strip_prefix(root).unwrap_or(&artifact_path));
        if !report
            .created_artifacts
            .iter()
            .any(|path| path == &expected_path)
        {
            return Err(invalid_compiled_artifact(
                &mission_dir.join("compile-report.json"),
                &format!("created_artifacts missing {expected_path}"),
            ));
        }

        let expected_digest = report.artifact_digests.get(&expected_path).ok_or_else(|| {
            invalid_compiled_artifact(
                &mission_dir.join("compile-report.json"),
                &format!("artifact_digests missing {expected_path}"),
            )
        })?;
        validate_blake3_digest(expected_digest, &mission_dir.join("compile-report.json"))?;
        let value = values
            .get(*artifact)
            .expect("required artifact loaded before digest validation");
        let actual_digest = if *artifact == "compile-report.json" {
            compile_report_recorded_digest(value, &expected_path)?
        } else {
            canonical_json_digest(value)?
        };
        if actual_digest != *expected_digest {
            return Err(invalid_compiled_artifact(
                &mission_dir.join(artifact),
                &format!("stale Better Droid mission artifact digest for {expected_path}"),
            ));
        }
    }

    Ok(())
}

pub(crate) fn validate_compiled_artifact_schemas(
    change_id: &str,
    values: &HashMap<String, Value>,
) -> Result<()> {
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
        require_artifact_string_eq(value, "schema", schema, artifact)?;
        require_artifact_string_eq(value, "change_id", change_id, artifact)?;
    }

    let assumptions = serde_json::from_value::<AssumptionsArtifact>(
        values
            .get("assumptions.json")
            .expect("assumptions artifact loaded")
            .clone(),
    )?;
    if assumptions.approval_summary.high_or_critical_pending > 0 {
        return Err(invalid_compiled_artifact(
            Path::new("assumptions.json"),
            "unapproved high or critical assumptions block import",
        ));
    }

    let path_policy = serde_json::from_value::<PathPolicyArtifact>(
        values
            .get("path-policy.json")
            .expect("path policy artifact loaded")
            .clone(),
    )?;
    validate_importable_path_policy(&path_policy)?;
    Ok(())
}

pub(crate) fn validate_importable_path_policy(path_policy: &PathPolicyArtifact) -> Result<()> {
    for path in path_policy
        .allowed_write_paths
        .iter()
        .chain(path_policy.generated_paths.iter())
    {
        if is_broad_path(path) {
            return Err(invalid_compiled_artifact(
                Path::new("path-policy.json"),
                &format!("broad path policy is not importable: {path}"),
            ));
        }
    }
    Ok(())
}

pub(crate) fn importable_tasks(
    change_id: &str,
    mission: &MissionArtifact,
    report: &MissionReport,
) -> Result<Vec<CompiledTask>> {
    let mut classification_by_id = HashMap::with_capacity(report.task_classifications.len());
    for classification in &report.task_classifications {
        validate_task_id(&classification.task_id, Path::new("compile-report.json"))?;
        validate_blake3_digest(
            &classification.task_digest,
            Path::new("compile-report.json"),
        )?;
        if classification_by_id
            .insert(classification.task_id.as_str(), classification)
            .is_some()
        {
            return Err(invalid_compiled_artifact(
                Path::new("compile-report.json"),
                &format!("duplicate task id {}", classification.task_id),
            ));
        }
    }

    let mut seen = HashSet::new();
    let mut tasks = Vec::with_capacity(mission.tasks.len());
    for task in &mission.tasks {
        validate_task_id(&task.task_id, Path::new("mission.json"))?;
        if !seen.insert(task.task_id.clone()) {
            return Err(invalid_compiled_artifact(
                Path::new("mission.json"),
                &format!("duplicate task id {}", task.task_id),
            ));
        }
        if task.title.trim().is_empty() {
            return Err(invalid_compiled_artifact(
                Path::new("mission.json"),
                &format!("missing title for task {}", task.task_id),
            ));
        }
        let classification = classification_by_id
            .get(task.task_id.as_str())
            .ok_or_else(|| {
                invalid_compiled_artifact(
                    Path::new("compile-report.json"),
                    &format!("missing classification for task {}", task.task_id),
                )
            })?;
        if classification.import_status != ImportStatus::Importable {
            return Err(invalid_compiled_artifact(
                Path::new("compile-report.json"),
                "import_status must be importable",
            ));
        }
        if classification.title != task.title {
            return Err(invalid_compiled_artifact(
                Path::new("compile-report.json"),
                &format!("classification title mismatch for task {}", task.task_id),
            ));
        }
        if classification.task_type != task.task_type {
            return Err(invalid_compiled_artifact(
                Path::new("compile-report.json"),
                &format!(
                    "classification task_type mismatch for task {}",
                    task.task_id
                ),
            ));
        }
        if classification.task_digest != task.task_digest {
            return Err(invalid_compiled_artifact(
                Path::new("compile-report.json"),
                &format!(
                    "classification task_digest mismatch for task {}",
                    task.task_id
                ),
            ));
        }

        tasks.push(task.clone());
    }

    if tasks.is_empty() {
        return Err(invalid_compiled_artifact(
            Path::new("mission.json"),
            &format!("no importable compiled tasks for {change_id}"),
        ));
    }
    if classification_by_id.len() != seen.len() {
        return Err(invalid_compiled_artifact(
            Path::new("compile-report.json"),
            "task_classifications contains task ids not present in mission.json",
        ));
    }

    Ok(tasks)
}

pub(crate) fn compile_report_recorded_digest(report: &Value, self_path: &str) -> Result<String> {
    let mut report_without_self = report.clone();
    let digests = report_without_self
        .get_mut("artifact_digests")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            invalid_compiled_artifact(
                Path::new("compile-report.json"),
                "missing artifact_digests object",
            )
        })?;
    digests.remove(self_path);
    canonical_json_digest(&report_without_self)
}

pub(crate) fn require_artifact_string_eq(
    value: &Value,
    field: &str,
    expected: &str,
    artifact: &str,
) -> Result<()> {
    let actual = value.get(field).and_then(Value::as_str).ok_or_else(|| {
        invalid_compiled_artifact(Path::new(artifact), &format!("missing {field} string"))
    })?;
    if actual == expected {
        Ok(())
    } else {
        Err(invalid_compiled_artifact(
            Path::new(artifact),
            &format!("{field} must be {expected}"),
        ))
    }
}

pub(crate) fn validate_blake3_digest(digest: &str, artifact: &Path) -> Result<()> {
    let hex = digest.strip_prefix("blake3:").ok_or_else(|| {
        invalid_compiled_artifact(artifact, "digest must start with lowercase blake3:")
    })?;
    if hex.len() != 64
        || !hex
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
    {
        return Err(invalid_compiled_artifact(
            artifact,
            "digest must be lowercase blake3 plus 64 hex characters",
        ));
    }
    Ok(())
}

pub(crate) fn validate_task_id(task_id: &str, artifact: &Path) -> Result<()> {
    if !task_id.contains('.')
        || task_id
            .split('.')
            .any(|segment| segment.is_empty() || !segment.chars().all(|ch| ch.is_ascii_digit()))
    {
        return Err(invalid_compiled_artifact(
            artifact,
            &format!("invalid stable task id {task_id}"),
        ));
    }
    Ok(())
}

pub(crate) fn is_broad_path(path: &str) -> bool {
    matches!(path.trim(), "." | "/" | "repo" | "**/*" | "all files" | "*")
        || path.trim().starts_with("/*")
}

pub(crate) fn invalid_compiled_artifact(path: &Path, detail: &str) -> BetterDroidError {
    BetterDroidError::InvalidSource {
        path: path.to_string_lossy().to_string(),
        detail: detail.to_owned(),
    }
}

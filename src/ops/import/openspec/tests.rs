use super::{
    mission::load_mission_packet,
    parse::parse_openspec_tasks,
    source::load_openspec_source_snapshot,
    util::{blake3_digest, normalize_relative_path},
};
use crate::error::CoveyError;
use crate::{Covey, ObjectType, RegisterSessionReq, SessionRole, model::ImportOpenSpecReq};
use better_droid::{CompileOptions, compile_change};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, fs, path::Path};
use tempfile::TempDir;

#[test]
fn openspec_task_parser_accepts_stable_checklist_ids() {
    let tasks = parse_openspec_tasks(
        "- [ ] 1.1 Add CLI\n- [x] 1.2 Parse tasks\n",
        "openspec/changes/example/tasks.md",
    )
    .expect("valid tasks");

    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].task_id.as_str(), "1.1");
    assert_eq!(tasks[0].title.as_str(), "Add CLI");
    assert!(tasks[0].task_digest.as_str().starts_with("blake3:"));
    assert_eq!(tasks[0].task_digest.as_str().len(), 71);
}

#[test]
fn openspec_import_provenance_is_queryable_for_imported_subtasks() {
    let tmp = TempDir::new().expect("tempdir");
    seed_better_droid_change(tmp.path(), "provenance-query");
    compile_better_droid_change(tmp.path(), "provenance-query");

    let db_path = tmp.path().join("covey.db");
    let covey = Covey::open(&db_path).expect("open covey");
    let orchestrator = covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                "orch",
                "orch-1",
                SessionRole::Orchestrator,
                "register-orch",
            )
            .expect("valid session registration request"),
        )
        .expect("register orchestrator");

    let result = covey
        .import_openspec(ImportOpenSpecReq::write(
            orchestrator.session_token,
            "provenance-query",
            tmp.path().to_string_lossy().to_string(),
        ))
        .expect("import openspec");
    let subtask_id = result
        .items()
        .iter()
        .find(|item| item.object_type() == ObjectType::Subtask)
        .map(|item| item.object_id().to_owned())
        .expect("imported subtask id");

    let provenance = covey
        .import_provenance(ObjectType::Subtask, &subtask_id)
        .expect("query provenance")
        .expect("subtask provenance");
    assert_eq!(provenance.openspec_change_id(), "provenance-query");
    assert_eq!(provenance.openspec_task_id(), Some("1.1"));
    assert!(
        provenance
            .mission_artifacts()
            .iter()
            .any(|artifact| artifact.ends_with("mission-packet.json"))
    );
}

#[test]
fn openspec_task_parser_rejects_duplicate_ids_and_malformed_tasks() {
    let duplicate = parse_openspec_tasks("- [ ] 1.1 Add CLI\n- [ ] 1.1 Parse tasks\n", "tasks.md");
    assert!(
        matches!(
            duplicate,
            Err(CoveyError::InvalidSourceSchema { ref detail, .. }) if detail.contains("duplicate task id")
        ),
        "duplicate task id should fail with duplicate detail: {duplicate:?}"
    );

    let malformed = parse_openspec_tasks("- [ ] Add CLI\n", "tasks.md");
    assert!(matches!(
        malformed,
        Err(CoveyError::InvalidSourceSchema { detail, .. })
            if detail.contains("task id must be hierarchical")
                || detail.contains("task id must contain only")
    ));
}

#[test]
fn openspec_source_loader_validates_required_files_and_digests() {
    let tmp = TempDir::new().expect("tempdir");
    let change_dir = tmp
        .path()
        .join("openspec")
        .join("changes")
        .join("example-change");
    fs::create_dir_all(change_dir.join("specs").join("covey-openspec-import"))
        .expect("create dirs");
    fs::write(change_dir.join("proposal.md"), "proposal").expect("proposal");
    fs::write(change_dir.join("design.md"), "design").expect("design");
    fs::write(change_dir.join("tasks.md"), "- [ ] 1.1 Add CLI\n").expect("tasks");
    fs::write(
        change_dir
            .join("specs")
            .join("covey-openspec-import")
            .join("spec.md"),
        "spec",
    )
    .expect("spec");

    let missing_mission =
        load_openspec_source_snapshot(tmp.path().to_str().expect("root path"), "example-change");
    assert!(matches!(
        missing_mission,
        Err(CoveyError::InvalidSourceSchema { detail, .. })
            if detail == "missing compiled Better Droid mission directory"
    ));

    fs::remove_file(change_dir.join("design.md")).expect("remove design");
    let missing =
        load_openspec_source_snapshot(tmp.path().to_str().expect("root path"), "example-change");
    assert!(matches!(
        missing,
        Err(CoveyError::InvalidSourceSchema { detail, .. })
            if detail == "missing required OpenSpec file"
    ));
}

#[test]
fn openspec_mission_packet_loader_accepts_compiled_better_droid_artifacts() {
    let tmp = TempDir::new().expect("tempdir");
    seed_better_droid_change(tmp.path(), "compiled-import");
    compile_better_droid_change(tmp.path(), "compiled-import");

    let source =
        load_openspec_source_snapshot(tmp.path().to_str().expect("root path"), "compiled-import")
            .expect("source snapshot");

    assert_eq!(source.change_id.as_str(), "compiled-import");
    assert_eq!(
        source.change_path.as_str(),
        "openspec/changes/compiled-import"
    );
    assert_eq!(source.tasks.len(), 1);
    assert_eq!(source.tasks[0].task_id.as_str(), "1.1");
    assert_eq!(
        source.tasks[0]
            .task_type
            .as_ref()
            .map(|value| value.as_str()),
        Some("implementation")
    );
    assert!(source.tasks[0].task_digest.as_str().starts_with("blake3:"));
    assert_eq!(source.tasks[0].task_digest.as_str().len(), 71);
    assert_eq!(source.mission_artifacts.len(), 8);
    assert_eq!(source.mission_artifact_digests.len(), 8);
    assert!(
        source
            .mission_artifacts
            .iter()
            .any(|artifact| artifact.ends_with("mission-packet.json"))
    );
    assert!(
        source
            .source_digests
            .iter()
            .any(|digest| digest.path().ends_with("tasks.md"))
    );
}

#[test]
fn openspec_mission_packet_loader_rejects_missing_blocked_and_stale_artifacts() {
    let tmp = TempDir::new().expect("tempdir");
    seed_better_droid_change(tmp.path(), "blocked-import");
    compile_better_droid_change(tmp.path(), "blocked-import");
    let mission_dir = tmp
        .path()
        .join("openspec")
        .join("changes")
        .join("blocked-import")
        .join("mission");

    fs::remove_file(mission_dir.join("path-policy.json")).expect("remove path policy");
    let missing =
        load_openspec_source_snapshot(tmp.path().to_str().expect("root path"), "blocked-import");
    assert!(matches!(
        missing,
        Err(CoveyError::InvalidSourceSchema { detail, .. })
            if detail == "missing required Better Droid mission artifact"
    ));

    compile_better_droid_change(tmp.path(), "blocked-import");
    mutate_compile_report(&mission_dir, |report| {
        report["status"] = Value::String("blocked".to_owned());
        report["import_ready"] = Value::Bool(false);
    });
    let blocked =
        load_openspec_source_snapshot(tmp.path().to_str().expect("root path"), "blocked-import");
    assert!(matches!(
        blocked,
        Err(CoveyError::InvalidSourceSchema { detail, .. })
            if detail == "compile-report import_ready must be true"
                || detail == "status must be covey_import_ready"
    ));

    compile_better_droid_change(tmp.path(), "blocked-import");
    fs::write(
        mission_dir.join("mission.json"),
        "{\"schema\":\"tampered\"}",
    )
    .expect("tamper mission");
    let stale =
        load_openspec_source_snapshot(tmp.path().to_str().expect("root path"), "blocked-import");
    assert!(matches!(
        stale,
        Err(CoveyError::InvalidSourceSchema { detail, .. })
            if detail.contains("stale Better Droid mission artifact digest")
    ));
}

#[test]
fn openspec_mission_packet_loader_rejects_duplicate_ids_and_missing_task_digest() {
    let tmp = TempDir::new().expect("tempdir");
    seed_better_droid_change(tmp.path(), "invalid-task-packet");
    compile_better_droid_change(tmp.path(), "invalid-task-packet");
    let mission_dir = tmp
        .path()
        .join("openspec")
        .join("changes")
        .join("invalid-task-packet")
        .join("mission");

    mutate_mission(&mission_dir, |mission| {
        let tasks = mission["tasks"].as_array_mut().expect("tasks array");
        let duplicate = tasks[0].clone();
        tasks.push(duplicate);
    });
    refresh_artifact_digests(&mission_dir);
    let duplicate = load_openspec_source_snapshot(
        tmp.path().to_str().expect("root path"),
        "invalid-task-packet",
    );
    assert!(
        matches!(
            duplicate,
            Err(CoveyError::InvalidSourceSchema { ref detail, .. }) if detail.contains("duplicate task id")
        ),
        "duplicate task id should fail with duplicate detail: {duplicate:?}"
    );

    compile_better_droid_change(tmp.path(), "invalid-task-packet");
    mutate_compile_report(&mission_dir, |report| {
        report["task_classifications"][0]
            .as_object_mut()
            .expect("classification object")
            .remove("task_digest");
    });
    let missing_digest = load_openspec_source_snapshot(
        tmp.path().to_str().expect("root path"),
        "invalid-task-packet",
    );
    assert!(matches!(
        missing_digest,
        Err(CoveyError::InvalidSourceSchema { detail, .. })
            if detail == "missing task_digest string"
    ));

    compile_better_droid_change(tmp.path(), "invalid-task-packet");
    mutate_compile_report_without_refresh(&mission_dir, |report| {
        report["task_classifications"][0]["task_digest"] = Value::String(
            "blake3:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        );
    });
    let tampered_report = load_openspec_source_snapshot(
        tmp.path().to_str().expect("root path"),
        "invalid-task-packet",
    );
    assert!(matches!(
        tampered_report,
        Err(CoveyError::InvalidSourceSchema { detail, .. })
            if detail.contains("stale Better Droid mission artifact digest")
    ));

    compile_better_droid_change(tmp.path(), "invalid-task-packet");
    mutate_compile_report(&mission_dir, |report| {
        let extra = serde_json::json!({
            "task_id": "9.9",
            "title": "Extra classification",
            "task_type": "implementation",
            "import_status": "importable",
            "task_digest": "blake3:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        });
        report["task_classifications"]
            .as_array_mut()
            .expect("classification array")
            .push(extra);
    });
    let extra_classification = load_openspec_source_snapshot(
        tmp.path().to_str().expect("root path"),
        "invalid-task-packet",
    );
    assert!(matches!(
        extra_classification,
        Err(CoveyError::InvalidSourceSchema { detail, .. })
            if detail == "task_classifications contains task ids not present in mission.json"
    ));
}

#[test]
fn openspec_mission_packet_loader_rejects_stale_source_digest() {
    let tmp = TempDir::new().expect("tempdir");
    seed_better_droid_change(tmp.path(), "stale-source");
    compile_better_droid_change(tmp.path(), "stale-source");
    fs::write(
        tmp.path()
            .join("openspec")
            .join("changes")
            .join("stale-source")
            .join("tasks.md"),
        "changed after compile",
    )
    .expect("tamper source");

    let stale =
        load_openspec_source_snapshot(tmp.path().to_str().expect("root path"), "stale-source");
    assert!(matches!(
        stale,
        Err(CoveyError::InvalidSourceSchema { detail, .. })
            if detail == "compiled mission source digest is stale"
    ));
}

#[test]
fn openspec_mission_packet_loader_rejects_corrupt_compiled_artifact_shapes() {
    let tmp = TempDir::new().expect("tempdir");

    let invalid_json_dir = compiled_mission_dir(tmp.path(), "invalid-json-packet");
    fs::write(invalid_json_dir.join("traceability.json"), "{not-json").expect("tamper json");
    let invalid_json = load_openspec_source_snapshot(
        tmp.path().to_str().expect("root path"),
        "invalid-json-packet",
    );
    assert!(matches!(
        invalid_json,
        Err(CoveyError::InvalidSourceSchema { detail, .. })
            if detail.contains("invalid Better Droid mission JSON")
    ));

    let blockers_dir = compiled_mission_dir(tmp.path(), "blocker-packet");
    mutate_compile_report(&blockers_dir, |report| {
        report["blockers"] = serde_json::json!([{"id": "blocked"}]);
    });
    assert_invalid_source_detail(
        tmp.path(),
        "blocker-packet",
        "compile-report contains blockers",
    );

    let roadmap_dir = compiled_mission_dir(tmp.path(), "roadmap-packet");
    mutate_compile_report(&roadmap_dir, |report| {
        report["planning_class"] = Value::String("roadmap".to_owned());
        report["status"] = Value::String("blocked".to_owned());
        report["import_ready"] = Value::Bool(false);
        report["blockers"] = serde_json::json!([{
            "id": "roadmap_not_importable",
            "source_path": "openspec/changes/roadmap-packet/.openspec.yaml",
            "task_id": null,
            "scenario_id": null,
            "detail": "planning_class=roadmap is planning-only and cannot be imported into Covey"
        }]);
    });
    assert_invalid_source_detail(
        tmp.path(),
        "roadmap-packet",
        "planning_class must be work_packet",
    );

    let missing_created_dir = compiled_mission_dir(tmp.path(), "missing-created-packet");
    mutate_compile_report(&missing_created_dir, |report| {
        report["created_artifacts"] = serde_json::json!([]);
    });
    assert_invalid_source_detail(
        tmp.path(),
        "missing-created-packet",
        "created_artifacts missing",
    );

    let missing_digest_dir = compiled_mission_dir(tmp.path(), "missing-digest-packet");
    mutate_compile_report_without_refresh(&missing_digest_dir, |report| {
        report["artifact_digests"]
            .as_object_mut()
            .expect("artifact digests object")
            .remove("openspec/changes/missing-digest-packet/mission/mission.json");
    });
    assert_invalid_source_detail(
        tmp.path(),
        "missing-digest-packet",
        "artifact_digests missing",
    );

    let pending_assumptions_dir = compiled_mission_dir(tmp.path(), "pending-assumptions-packet");
    mutate_artifact(
        &pending_assumptions_dir,
        "assumptions.json",
        |assumptions| {
            assumptions["approval_summary"]["high_or_critical_pending"] = Value::from(1);
        },
    );
    assert_invalid_source_detail(
        tmp.path(),
        "pending-assumptions-packet",
        "unapproved high or critical assumptions block import",
    );

    let broad_path_dir = compiled_mission_dir(tmp.path(), "broad-path-packet");
    mutate_artifact(&broad_path_dir, "path-policy.json", |path_policy| {
        path_policy["allowed_write_paths"] = serde_json::json!(["repo"]);
    });
    assert_invalid_source_detail(
        tmp.path(),
        "broad-path-packet",
        "broad path policy is not importable",
    );

    let missing_tasks_dir = compiled_mission_dir(tmp.path(), "missing-tasks-array-packet");
    mutate_mission(&missing_tasks_dir, |mission| {
        mission
            .as_object_mut()
            .expect("mission object")
            .remove("tasks");
    });
    refresh_artifact_digests(&missing_tasks_dir);
    assert_invalid_source_detail(
        tmp.path(),
        "missing-tasks-array-packet",
        "missing tasks array",
    );

    let missing_classifications_dir =
        compiled_mission_dir(tmp.path(), "missing-classifications-packet");
    mutate_compile_report(&missing_classifications_dir, |report| {
        report
            .as_object_mut()
            .expect("report object")
            .remove("task_classifications");
    });
    assert_invalid_source_detail(
        tmp.path(),
        "missing-classifications-packet",
        "missing task_classifications array",
    );

    let duplicate_classification_dir =
        compiled_mission_dir(tmp.path(), "duplicate-classification-packet");
    mutate_compile_report(&duplicate_classification_dir, |report| {
        let duplicate = report["task_classifications"][0].clone();
        report["task_classifications"]
            .as_array_mut()
            .expect("classification array")
            .push(duplicate);
    });
    assert_invalid_source_detail(
        tmp.path(),
        "duplicate-classification-packet",
        "duplicate task id",
    );

    let invalid_task_id_dir = compiled_mission_dir(tmp.path(), "invalid-task-id-packet");
    mutate_mission(&invalid_task_id_dir, |mission| {
        mission["tasks"][0]["task_id"] = Value::String("task-one".to_owned());
    });
    refresh_artifact_digests(&invalid_task_id_dir);
    assert_invalid_source_detail(
        tmp.path(),
        "invalid-task-id-packet",
        "invalid stable task id",
    );

    let empty_title_dir = compiled_mission_dir(tmp.path(), "empty-title-packet");
    mutate_mission(&empty_title_dir, |mission| {
        mission["tasks"][0]["title"] = Value::String("   ".to_owned());
    });
    refresh_artifact_digests(&empty_title_dir);
    assert_invalid_source_detail(tmp.path(), "empty-title-packet", "missing title for task");

    let missing_classification_dir =
        compiled_mission_dir(tmp.path(), "missing-classification-packet");
    mutate_mission(&missing_classification_dir, |mission| {
        mission["tasks"][0]["task_id"] = Value::String("1.2".to_owned());
    });
    refresh_artifact_digests(&missing_classification_dir);
    assert_invalid_source_detail(
        tmp.path(),
        "missing-classification-packet",
        "missing classification for task 1.2",
    );

    let blocked_classification_dir =
        compiled_mission_dir(tmp.path(), "blocked-classification-packet");
    mutate_compile_report(&blocked_classification_dir, |report| {
        report["task_classifications"][0]["import_status"] = Value::String("blocked".to_owned());
    });
    assert_invalid_source_detail(
        tmp.path(),
        "blocked-classification-packet",
        "import_status must be importable",
    );

    let invalid_task_digest_dir = compiled_mission_dir(tmp.path(), "invalid-task-digest-packet");
    mutate_compile_report(&invalid_task_digest_dir, |report| {
        report["task_classifications"][0]["task_digest"] = Value::String("blake3:ABC".to_owned());
    });
    assert_invalid_source_detail(
        tmp.path(),
        "invalid-task-digest-packet",
        "digest must be lowercase blake3 plus 64 hex characters",
    );

    let title_mismatch_dir = compiled_mission_dir(tmp.path(), "title-mismatch-packet");
    mutate_compile_report(&title_mismatch_dir, |report| {
        report["task_classifications"][0]["title"] = Value::String("Different title".to_owned());
    });
    assert_invalid_source_detail(
        tmp.path(),
        "title-mismatch-packet",
        "classification title mismatch",
    );

    let no_tasks_dir = compiled_mission_dir(tmp.path(), "no-importable-tasks-packet");
    mutate_mission(&no_tasks_dir, |mission| {
        mission["tasks"] = Value::Array(Vec::new());
    });
    refresh_artifact_digests(&no_tasks_dir);
    assert_invalid_source_detail(
        tmp.path(),
        "no-importable-tasks-packet",
        "no importable compiled tasks",
    );
}

#[test]
fn openspec_mission_packet_private_loader_returns_sorted_digest_vectors() {
    let tmp = TempDir::new().expect("tempdir");
    let mission_dir = compiled_mission_dir(tmp.path(), "private-loader-packet");
    let packet = load_mission_packet(
        tmp.path(),
        "private-loader-packet",
        Path::new("openspec/changes/private-loader-packet"),
    )
    .expect("load private mission packet");

    assert_eq!(packet.tasks.len(), 1);
    assert!(mission_dir.join("mission.json").is_file());
    assert!(
        packet
            .source_digests
            .windows(2)
            .all(|window| window[0].path() <= window[1].path())
    );
    assert!(
        packet
            .artifact_digests
            .windows(2)
            .all(|window| window[0].path() <= window[1].path())
    );
}

fn seed_better_droid_change(root: &Path, change_id: &str) {
    let change_dir = root.join("openspec").join("changes").join(change_id);
    fs::create_dir_all(change_dir.join("specs").join("example")).expect("create dirs");
    fs::write(change_dir.join(".openspec.yaml"), "schema: better-droid\n").expect("yaml");
    fs::write(
        change_dir.join("proposal.md"),
        "## Why\n\nCompile source for importer tests.\n",
    )
    .expect("proposal");
    fs::write(change_dir.join("design.md"), "## Context\n\nTest design.\n").expect("design");
    fs::write(
        change_dir.join("specs").join("example").join("spec.md"),
        "## MODIFIED Requirements\n\n### Requirement: REQ-TEST Mission import\nThe system SHALL import compiled packets.\n\n#### Scenario: SCN-TEST-001 Valid packet\n- **WHEN** imported\n- **THEN** it succeeds\n",
    )
    .expect("spec");
    fs::write(
        change_dir.join("tasks.md"),
        r#"## 1. Implementation

- [ ] 1.1 Import compiled packet
  - **Type:** implementation
  - **Purpose:** Exercise compiled packet import for SCN-TEST-001 and REQ-TEST.
  - **Dependencies:** none.
  - **Allowed Read Paths:** `openspec/changes/**`
  - **Allowed Write Paths:** `covey/src/ops/import/openspec/source.rs`
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Acceptance Criteria:** Compiled packet imports.
  - **Validation / Evidence:**
    - **Command / Action:** `cargo test openspec_mission_packet --lib`
    - **Expected Exit Code / Observation:** exits 0.
    - **Required Evidence:** stdout, exit code, and changed-file list outside openspec/**.
  - **Traceability Refs:** REQ-TEST, SCN-TEST-001, VAL-TEST-001.
  - **Stale If:** source changes.
"#,
    )
    .expect("tasks");
}

fn compile_better_droid_change(root: &Path, change_id: &str) {
    compile_change(&CompileOptions {
        project_root: root.to_path_buf(),
        change_id: change_id.to_owned(),
        output_dir: None,
    })
    .expect("compile better droid change");
}

fn compiled_mission_dir(root: &Path, change_id: &str) -> std::path::PathBuf {
    seed_better_droid_change(root, change_id);
    compile_better_droid_change(root, change_id);
    root.join("openspec")
        .join("changes")
        .join(change_id)
        .join("mission")
}

fn assert_invalid_source_detail(root: &Path, change_id: &str, expected_detail: &str) {
    let result = load_openspec_source_snapshot(root.to_str().expect("root path"), change_id);
    assert!(
        matches!(
            result,
            Err(CoveyError::InvalidSourceSchema { ref detail, .. })
                if detail.contains(expected_detail)
        ),
        "{change_id} should fail with detail containing {expected_detail:?}: {result:?}"
    );
}

fn mutate_mission(mission_dir: &Path, mutate: impl FnOnce(&mut Value)) {
    let path = mission_dir.join("mission.json");
    let mut mission = read_json(&path);
    mutate(&mut mission);
    write_json(&path, &mission);
}

fn mutate_artifact(mission_dir: &Path, artifact: &str, mutate: impl FnOnce(&mut Value)) {
    let path = mission_dir.join(artifact);
    let mut value = read_json(&path);
    mutate(&mut value);
    write_json(&path, &value);
    refresh_artifact_digests(mission_dir);
}

fn mutate_compile_report(mission_dir: &Path, mutate: impl FnOnce(&mut Value)) {
    let path = mission_dir.join("compile-report.json");
    let mut report = read_json(&path);
    mutate(&mut report);
    write_json(&path, &report);
    refresh_artifact_digests(mission_dir);
}

fn mutate_compile_report_without_refresh(mission_dir: &Path, mutate: impl FnOnce(&mut Value)) {
    let path = mission_dir.join("compile-report.json");
    let mut report = read_json(&path);
    mutate(&mut report);
    write_json(&path, &report);
}

fn refresh_artifact_digests(mission_dir: &Path) {
    let report_path = mission_dir.join("compile-report.json");
    let mut report = read_json(&report_path);
    let mut digests = BTreeMap::new();
    for artifact in [
        "mission.json",
        "mission-packet.json",
        "traceability.json",
        "validation.json",
        "path-policy.json",
        "review-rubric.json",
        "assumptions.json",
    ] {
        let path = mission_dir.join(artifact);
        let relative = normalize_relative_path(&relative_from_openspec_root(&path));
        digests.insert(relative, canonical_json_digest(&read_json(&path)));
    }
    report["artifact_digests"] = serde_json::to_value(&digests).expect("artifact digests json");
    let relative_report = normalize_relative_path(&relative_from_openspec_root(&report_path));
    let self_digest = canonical_json_digest(&report);
    let object = report["artifact_digests"]
        .as_object_mut()
        .expect("artifact digests object");
    object.insert(relative_report, Value::String(self_digest));
    write_json(&report_path, &report);
}

fn relative_from_openspec_root(path: &Path) -> std::path::PathBuf {
    let components = path.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        if component.as_os_str() == "openspec" {
            return components[index..].iter().collect();
        }
    }
    path.to_path_buf()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read json")).expect("parse json")
}

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).expect("json bytes")).expect("write json");
}

fn canonical_json_digest(value: &Value) -> String {
    let canonical = canonicalize_value(value);
    blake3_digest(&serde_json::to_vec(&canonical).expect("canonical json"))
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

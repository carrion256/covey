use super::{
    parse::parse_openspec_tasks,
    source::load_openspec_source_snapshot,
    util::{normalize_relative_path, sha256_digest},
};
use crate::better_droid::{CompileOptions, compile_change};
use crate::error::CoveyError;
use crate::{Covey, ObjectType, RegisterSessionReq, SessionRole, model::ImportOpenSpecReq};
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
    assert_eq!(tasks[0].task_id, "1.1");
    assert_eq!(tasks[0].title, "Add CLI");
    assert!(tasks[0].task_digest.starts_with("sha256:"));
    assert_eq!(tasks[0].task_digest.len(), 71);
}

#[test]
fn openspec_import_provenance_is_queryable_for_imported_subtasks() {
    let tmp = TempDir::new().expect("tempdir");
    seed_better_droid_change(tmp.path(), "provenance-query");
    compile_better_droid_change(tmp.path(), "provenance-query");

    let db_path = tmp.path().join("covey.db");
    let covey = Covey::open(&db_path).expect("open covey");
    let orchestrator = covey
        .register_session(RegisterSessionReq {
            agent_principal_id: "orch".to_owned(),
            agent_instance_id: "orch-1".to_owned(),
            role: SessionRole::Orchestrator,
            idempotency_key: "register-orch".to_owned(),
        })
        .expect("register orchestrator");

    let result = covey
        .import_openspec(ImportOpenSpecReq {
            session_token: Some(orchestrator.session_token),
            change_id: "provenance-query".to_owned(),
            project_root: tmp.path().to_string_lossy().to_string(),
            dry_run: false,
        })
        .expect("import openspec");
    let subtask_id = result
        .items
        .iter()
        .find(|item| item.object_type == ObjectType::Subtask)
        .map(|item| item.object_id.clone())
        .expect("imported subtask id");

    let provenance = covey
        .import_provenance(ObjectType::Subtask, &subtask_id)
        .expect("query provenance")
        .expect("subtask provenance");
    assert_eq!(provenance.openspec_change_id, "provenance-query");
    assert_eq!(provenance.openspec_task_id.as_deref(), Some("1.1"));
    assert!(
        provenance
            .mission_artifacts
            .iter()
            .any(|artifact| artifact.ends_with("mission-packet.json"))
    );
}

#[test]
fn openspec_task_parser_rejects_duplicate_ids_and_malformed_tasks() {
    let duplicate = parse_openspec_tasks("- [ ] 1.1 Add CLI\n- [ ] 1.1 Parse tasks\n", "tasks.md");
    assert!(matches!(
        duplicate,
        Err(CoveyError::InvalidSourceSchema { detail, .. }) if detail.contains("duplicate task id")
    ));

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

    assert_eq!(source.change_id, "compiled-import");
    assert_eq!(source.change_path, "openspec/changes/compiled-import");
    assert_eq!(source.tasks.len(), 1);
    assert_eq!(source.tasks[0].task_id, "1.1");
    assert_eq!(source.tasks[0].task_type.as_deref(), Some("implementation"));
    assert!(source.tasks[0].task_digest.starts_with("sha256:"));
    assert_eq!(source.tasks[0].task_digest.len(), 71);
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
            .any(|digest| digest.path.ends_with("tasks.md"))
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
                || detail == "status must be ready"
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
    assert!(matches!(
        duplicate,
        Err(CoveyError::InvalidSourceSchema { detail, .. }) if detail.contains("duplicate task id")
    ));

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
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
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
            "task_digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
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
  - **Allowed Read Paths:** `openspec/changes/**`.
  - **Allowed Write Paths:** `covey/src/ops/import/openspec/source.rs`.
  - **Forbidden Paths:** `mutai-rs/**`, `go/controlplane/**`, `vendored/cliproxyapiplus/**`, `.git/**`.
  - **Acceptance Criteria:** Compiled packet imports.
  - **Validation / Evidence:**
    - **Command / Action:** `cargo test openspec_mission_packet --lib`
    - **Expected Exit Code / Observation:** exits 0.
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

fn mutate_mission(mission_dir: &Path, mutate: impl FnOnce(&mut Value)) {
    let path = mission_dir.join("mission.json");
    let mut mission = read_json(&path);
    mutate(&mut mission);
    write_json(&path, &mission);
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
    sha256_digest(&serde_json::to_vec(&canonical).expect("canonical json"))
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

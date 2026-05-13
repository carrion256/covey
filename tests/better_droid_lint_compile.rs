use std::{fs, path::Path};

use covey::better_droid::{
    CompileOptions, ImportStatus, LintOptions, ReportStatus, compile_change, lint_change,
};
use serde_json::Value;
use tempfile::TempDir;

#[test]
fn better_droid_lint_reports_readiness_without_writes() {
    let fixture = Fixture::passing();

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
    })
    .expect("lint passing fixture");

    assert_eq!(report.status, ReportStatus::Ready);
    assert!(report.import_ready);
    assert!(report.blockers.is_empty());
    assert_eq!(report.task_counts.total, 1);
    assert_eq!(report.task_counts.importable, 1);
    assert!(!fixture.mission_dir("passing-fixture").exists());
}

#[test]
fn better_droid_source_ingest_records_required_inputs() {
    let fixture = Fixture::passing();

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
    })
    .expect("lint passing fixture");

    assert!(report.source_digests.contains_key(
        "openspec/changes/passing-fixture/specs/better-droid-lint-compile-first/spec.md"
    ));
    assert!(
        report
            .source_digests
            .contains_key("openspec/changes/passing-fixture/.openspec.yaml")
    );
    assert!(
        report.source_digests.values().all(|digest| {
            digest.starts_with("sha256:") && digest.len() == "sha256:".len() + 64
        })
    );
    assert_eq!(
        report.task_classifications[0].import_status,
        ImportStatus::Importable
    );
}

#[test]
fn better_droid_missing_source_blocks_readiness() {
    let fixture = Fixture::missing_source();

    let error = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "missing-source-fixture".to_owned(),
    })
    .expect_err("missing source should fail before lint readiness");

    assert!(error.to_string().contains("missing_required_source"));
}

#[test]
fn better_droid_vague_executable_task_is_blocked() {
    let fixture = Fixture::vague_task();

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "vague-task-fixture".to_owned(),
    })
    .expect("lint vague fixture");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(!report.import_ready);
    assert!(report.blockers.iter().any(|blocker| {
        blocker.id == "mission_incomplete_executable_task"
            && blocker.task_id.as_deref() == Some("1.1")
    }));
}

#[test]
fn better_droid_unmapped_scenario_blocks_readiness() {
    let fixture = Fixture::unmapped_scenario();

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "unmapped-scenario-fixture".to_owned(),
    })
    .expect("lint unmapped fixture");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(report.blockers.iter().any(|blocker| {
        blocker.id == "unmapped_behavioral_scenario"
            && blocker.scenario_id.as_deref() == Some("SCN-BDLCF-unmapped")
    }));
}

#[test]
fn better_droid_unsafe_path_policy_is_blocked() {
    let fixture = Fixture::unsafe_path();

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "unsafe-path-fixture".to_owned(),
    })
    .expect("lint unsafe path fixture");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(
        report
            .blockers
            .iter()
            .any(|blocker| blocker.id == "unsafe_path_policy")
    );
}

#[test]
fn better_droid_compile_rejects_blocked_source() {
    let fixture = Fixture::vague_task();

    let report = compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "vague-task-fixture".to_owned(),
        output_dir: None,
    })
    .expect("compile blocked fixture returns report");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(!report.import_ready);
    assert!(report.created_artifacts.is_empty());
    assert!(!fixture.mission_dir("vague-task-fixture").exists());
}

#[test]
fn better_droid_lint_blocks_stale_marked_change() {
    let fixture = Fixture::passing();
    let change_dir = fixture
        .root()
        .join("openspec")
        .join("changes")
        .join("passing-fixture");
    fs::write(
        change_dir.join(".openspec.yaml"),
        "schema: better-droid\nstatus: stale_reauthor_required\n",
    )
    .expect("mark fixture stale");

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_path_buf(),
        change_id: "passing-fixture".to_owned(),
    })
    .expect("lint stale fixture");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(!report.import_ready);
    assert!(
        report
            .blockers
            .iter()
            .any(|blocker| blocker.id == "stale_openspec_change")
    );
}

#[test]
fn better_droid_compile_blocks_stale_marked_change_without_writes() {
    let fixture = Fixture::passing();
    let change_dir = fixture
        .root()
        .join("openspec")
        .join("changes")
        .join("passing-fixture");
    fs::write(
        change_dir.join(".openspec.yaml"),
        "schema: better-droid\nstatus: stale_reauthor_required\nstale_reason: boundary drift\n",
    )
    .expect("mark fixture stale");

    let report = compile_change(&CompileOptions {
        project_root: fixture.root().to_path_buf(),
        change_id: "passing-fixture".to_owned(),
        output_dir: None,
    })
    .expect("compile stale fixture returns blocked report");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(!report.import_ready);
    assert!(report.created_artifacts.is_empty());
    assert!(!fixture.mission_dir("passing-fixture").exists());
    assert!(
        report
            .blockers
            .iter()
            .any(|blocker| blocker.id == "stale_openspec_change")
    );
}

#[test]
fn better_droid_valid_source_compiles_canonical_json_packet() {
    let fixture = Fixture::passing();

    let report = compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
        output_dir: None,
    })
    .expect("compile passing fixture");

    assert_eq!(report.status, ReportStatus::Ready);
    assert!(report.import_ready);
    assert_eq!(report.created_artifacts.len(), 8);
    for file_name in [
        "mission.json",
        "mission-packet.json",
        "traceability.json",
        "validation.json",
        "path-policy.json",
        "review-rubric.json",
        "assumptions.json",
        "compile-report.json",
    ] {
        assert!(
            fixture
                .mission_dir("passing-fixture")
                .join(file_name)
                .is_file()
        );
    }
}

#[test]
fn better_droid_empty_assumptions_json_is_emitted() {
    let fixture = Fixture::passing();

    compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
        output_dir: None,
    })
    .expect("compile passing fixture");

    let assumptions = read_json(
        &fixture
            .mission_dir("passing-fixture")
            .join("assumptions.json"),
    );
    assert_eq!(assumptions["schema"], "better-droid.assumptions.v1");
    assert_eq!(assumptions["assumptions"].as_array().unwrap().len(), 0);
}

#[test]
fn better_droid_compile_emits_mutai_mission_packet_v1() {
    let fixture = Fixture::passing();

    compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
        output_dir: None,
    })
    .expect("compile passing fixture");

    let packet = read_json(
        &fixture
            .mission_dir("passing-fixture")
            .join("mission-packet.json"),
    );
    assert_eq!(packet["schema_version"], "mission_packet.v1");
    assert_eq!(packet["mission"]["id"], "passing-fixture");
    assert_eq!(
        packet["mission"]["title"],
        "Compile Better Droid source into canonical mission JSON."
    );
    assert_eq!(packet["provenance"]["compiler"], "better-droid");
    assert!(
        packet["provenance"]["source_revision"]
            .as_str()
            .is_some_and(|revision| !revision.is_empty())
    );
    assert!(
        packet["provenance"]["source_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:"))
    );
    assert_eq!(
        packet["scheduler"]["activation_result"]["Ready"]["classification"],
        "AllowCurrent"
    );
    assert_eq!(
        packet["scheduler"]["identity_refs"]["selected_attempt"]["attempt_id"],
        "attempt-passing-fixture"
    );
    assert_eq!(
        packet["scheduler"]["identity_refs"]["selected_attempt"]["evaluator_version"],
        "better-droid:mission-packet"
    );
    assert_eq!(packet["runtime"]["provider_mode"], "FakeDeliver");
    assert_eq!(packet["provider"]["provider_id"], "better-droid");
    assert_eq!(packet["provider"]["model_id"], "compiled-mission");
    assert_eq!(packet["path_policy"]["mutation_allowed"], false);
    let allowed_paths = packet["path_policy"]["allowed_paths"]
        .as_array()
        .expect("allowed paths array");
    assert!(!allowed_paths.is_empty());
    assert!(allowed_paths.iter().all(|path| {
        path.as_str()
            .is_some_and(|path| !path.chars().any(char::is_whitespace))
    }));
    assert!(packet["repoops"].is_null());
    assert!(
        packet["validation"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert!(
        packet["review_rubric"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert!(packet["assumptions"].as_array().is_some());
}

#[test]
fn better_droid_compiled_json_required_fields() {
    let fixture = Fixture::passing();

    let report = compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
        output_dir: None,
    })
    .expect("compile passing fixture");

    assert_eq!(report.artifact_digests.len(), 8);
    let mission = read_json(&fixture.mission_dir("passing-fixture").join("mission.json"));
    assert_eq!(mission["schema"], "better-droid.mission.v1");
    assert!(mission.get("source_digests").is_some());
    assert!(mission.get("tasks").unwrap().as_array().unwrap().len() == 1);

    let compile_report = read_json(
        &fixture
            .mission_dir("passing-fixture")
            .join("compile-report.json"),
    );
    assert_eq!(compile_report["schema"], "better-droid.compile-report.v1");
    assert!(
        compile_report
            .get("non_canonical_run_metadata")
            .is_some_and(|metadata| metadata.as_object().is_some_and(serde_json::Map::is_empty))
    );
    let artifact_digests = compile_report["artifact_digests"].as_object().unwrap();
    assert_eq!(artifact_digests.len(), 8);
    for file_name in [
        "mission.json",
        "mission-packet.json",
        "traceability.json",
        "validation.json",
        "path-policy.json",
        "review-rubric.json",
        "assumptions.json",
        "compile-report.json",
    ] {
        assert!(artifact_digests.contains_key(&format!(
            "openspec/changes/passing-fixture/mission/{file_name}"
        )));
    }
}

#[test]
fn better_droid_repeat_compile_has_stable_digests() {
    let fixture = Fixture::passing();

    let first = compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
        output_dir: None,
    })
    .expect("first compile");
    let second = compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
        output_dir: None,
    })
    .expect("second compile");

    assert_eq!(first.artifact_digests, second.artifact_digests);
}

#[test]
fn better_droid_source_change_updates_stale_context() {
    let fixture = Fixture::passing();
    let before = compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
        output_dir: None,
    })
    .expect("compile before change");

    let tasks_path = fixture
        .root()
        .join("openspec/changes/passing-fixture/tasks.md");
    let mut tasks = fs::read_to_string(&tasks_path).expect("read tasks");
    tasks.push_str("\nAdditional acceptance evidence for digest change.\n");
    fs::write(&tasks_path, tasks).expect("write changed tasks");

    let after = compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
        output_dir: None,
    })
    .expect("compile after change");

    assert_ne!(before.source_digests, after.source_digests);
    assert_ne!(
        before.task_classifications[0].task_digest,
        after.task_classifications[0].task_digest
    );
}

#[test]
fn better_droid_lint_compile_avoid_live_state_mutation() {
    let fixture = Fixture::passing();

    lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
    })
    .expect("lint passing fixture");
    assert!(!fixture.root().join("covey.db").exists());

    compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
        output_dir: None,
    })
    .expect("compile passing fixture");
    assert!(!fixture.root().join("covey.db").exists());
}

#[test]
fn better_droid_compile_rejects_output_path_escape() {
    let fixture = Fixture::passing();

    let error = compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
        output_dir: Some(Path::new("../../outside").to_owned()),
    })
    .expect_err("output path escape should fail");

    assert!(error.to_string().contains("escapes mission directory"));
    assert!(!fixture.root().join("../../outside").exists());
}

#[test]
fn better_droid_rejects_schema_mismatch_and_missing_specs() {
    let schema_fixture = Fixture::passing();
    fs::write(
        schema_fixture
            .root()
            .join("openspec/changes/passing-fixture/.openspec.yaml"),
        "schema: other\n",
    )
    .expect("rewrite schema");
    let schema_error = lint_change(&LintOptions {
        project_root: schema_fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
    })
    .expect_err("schema mismatch should fail source loading");
    assert!(
        schema_error
            .to_string()
            .contains("schema must be better-droid")
    );

    let missing_specs = Fixture::new();
    let change_dir = missing_specs
        .root()
        .join("openspec/changes/missing-specs-fixture");
    fs::create_dir_all(&change_dir).expect("create change dir");
    fs::write(change_dir.join(".openspec.yaml"), "schema: better-droid\n").expect("write yaml");
    fs::write(change_dir.join("proposal.md"), PROPOSAL).expect("write proposal");
    fs::write(change_dir.join("design.md"), DESIGN).expect("write design");
    fs::write(change_dir.join("tasks.md"), PASSING_TASK).expect("write tasks");
    let specs_error = lint_change(&LintOptions {
        project_root: missing_specs.root().to_owned(),
        change_id: "missing-specs-fixture".to_owned(),
    })
    .expect_err("missing specs should fail source loading");
    assert!(
        specs_error
            .to_string()
            .contains("missing_required_source: specs")
    );
}

#[test]
fn better_droid_empty_task_file_blocks_readiness() {
    let fixture = Fixture::new();
    fixture.write_change("empty-tasks-fixture", "", PASSING_SPEC);

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "empty-tasks-fixture".to_owned(),
    })
    .expect("lint empty task fixture");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(report.blockers.iter().any(|blocker| {
        blocker.id == "missing_required_source"
            && blocker.detail.contains("no stable task checklist entries")
    }));
}

#[test]
fn better_droid_counts_rejected_deferred_and_high_risk_blocked_tasks() {
    let fixture = Fixture::new();
    fixture.write_change("classification-fixture", CLASSIFICATION_TASKS, PASSING_SPEC);

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "classification-fixture".to_owned(),
    })
    .expect("lint classification fixture");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert_eq!(report.task_counts.total, 3);
    assert_eq!(report.task_counts.rejected, 1);
    assert_eq!(report.task_counts.deferred, 1);
    assert_eq!(report.task_counts.blocked, 1);
    assert!(report.blockers.iter().any(|blocker| {
        blocker
            .detail
            .contains("unresolved high or critical assumption approval")
    }));
}

#[test]
fn better_droid_large_task_sets_warn_without_blocking_on_size_alone() {
    let fixture = Fixture::new();
    let mut tasks = String::new();
    for index in 1..=31 {
        tasks.push_str(&format!(
            "- [ ] 1.{index} Document large fixture task {index}\n  - **Type:** note\n  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture\n\n"
        ));
    }
    fixture.write_change("large-task-fixture", &tasks, PASSING_SPEC);

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "large-task-fixture".to_owned(),
    })
    .expect("lint large task fixture");

    assert_eq!(report.status, ReportStatus::Ready);
    assert_eq!(report.task_counts.importable, 31);
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.id == "large_task_set")
    );
}

#[test]
fn better_droid_path_policy_reports_escape_protected_and_overlap_blockers() {
    let fixture = Fixture::new();
    fixture.write_change("path-policy-fixture", PATH_POLICY_EDGE_TASK, PASSING_SPEC);

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "path-policy-fixture".to_owned(),
    })
    .expect("lint path policy fixture");

    let details = report
        .blockers
        .iter()
        .map(|blocker| blocker.detail.as_str())
        .collect::<Vec<_>>();
    assert!(
        details
            .iter()
            .any(|detail| detail.contains("repo-global write scope"))
    );
    assert!(
        details
            .iter()
            .any(|detail| detail.contains("out-of-root write scope"))
    );
    assert!(
        details
            .iter()
            .any(|detail| detail.contains("protected path authority/**"))
    );
    assert!(
        details
            .iter()
            .any(|detail| detail.contains("allowed and forbidden paths overlap"))
    );
}

#[test]
fn better_droid_compile_accepts_custom_output_inside_mission_dir() {
    let fixture = Fixture::passing();
    let output = fixture
        .root()
        .join("openspec/changes/passing-fixture/mission/custom");

    let report = compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
        output_dir: Some(output.clone()),
    })
    .expect("compile to custom mission output");

    assert_eq!(report.status, ReportStatus::Ready);
    assert!(output.join("mission.json").is_file());
    assert!(
        report
            .created_artifacts
            .iter()
            .all(|path| { path.starts_with("openspec/changes/passing-fixture/mission/custom/") })
    );
}

#[test]
fn better_droid_compile_uses_default_objective_and_default_allowed_packet_path() {
    let fixture = Fixture::new();
    fixture.write_change("default-packet-fixture", NON_EXECUTABLE_TASK, PASSING_SPEC);
    fs::write(
        fixture
            .root()
            .join("openspec/changes/default-packet-fixture/proposal.md"),
        "## Proposal\n\nNo explicit objective line here.\n",
    )
    .expect("rewrite proposal without objective");

    compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "default-packet-fixture".to_owned(),
        output_dir: None,
    })
    .expect("compile default packet fixture");

    let packet = read_json(
        &fixture
            .mission_dir("default-packet-fixture")
            .join("mission-packet.json"),
    );
    assert_eq!(
        packet["mission"]["title"],
        "compile Better Droid OpenSpec source into canonical mission JSON"
    );
    assert_eq!(
        packet["path_policy"]["allowed_paths"],
        serde_json::json!(["openspec/changes/**"])
    );
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read json")).expect("parse json")
}

struct Fixture {
    temp: TempDir,
}

impl Fixture {
    fn root(&self) -> &Path {
        self.temp.path()
    }

    fn mission_dir(&self, change_id: &str) -> std::path::PathBuf {
        self.root()
            .join("openspec")
            .join("changes")
            .join(change_id)
            .join("mission")
    }

    fn passing() -> Self {
        let fixture = Self::new();
        fixture.write_change("passing-fixture", PASSING_TASK, PASSING_SPEC);
        fixture
    }

    fn missing_source() -> Self {
        let fixture = Self::new();
        let change_dir = fixture
            .root()
            .join("openspec/changes/missing-source-fixture");
        fs::create_dir_all(&change_dir).expect("create missing source fixture");
        fs::write(change_dir.join(".openspec.yaml"), "schema: better-droid\n").expect("write yaml");
        fixture
    }

    fn vague_task() -> Self {
        let fixture = Self::new();
        fixture.write_change("vague-task-fixture", VAGUE_TASK, PASSING_SPEC);
        fixture
    }

    fn unmapped_scenario() -> Self {
        let fixture = Self::new();
        fixture.write_change("unmapped-scenario-fixture", PASSING_TASK, UNMAPPED_SPEC);
        fixture
    }

    fn unsafe_path() -> Self {
        let fixture = Self::new();
        fixture.write_change("unsafe-path-fixture", UNSAFE_PATH_TASK, PASSING_SPEC);
        fixture
    }

    fn new() -> Self {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("openspec/changes")).expect("create openspec");
        Self { temp }
    }

    fn write_change(&self, change_id: &str, tasks: &str, spec: &str) {
        let change_dir = self.root().join("openspec").join("changes").join(change_id);
        fs::create_dir_all(change_dir.join("specs/better-droid-lint-compile-first"))
            .expect("create spec dirs");
        fs::write(change_dir.join(".openspec.yaml"), "schema: better-droid\n").expect("write yaml");
        fs::write(change_dir.join("proposal.md"), PROPOSAL).expect("write proposal");
        fs::write(change_dir.join("design.md"), DESIGN).expect("write design");
        fs::write(change_dir.join("tasks.md"), tasks).expect("write tasks");
        fs::write(
            change_dir.join("specs/better-droid-lint-compile-first/spec.md"),
            spec,
        )
        .expect("write spec");
    }
}

const PROPOSAL: &str = r#"## Mission Readiness Expectations

### Mission Objective
- **Objective:** Compile Better Droid source into canonical mission JSON.
"#;

const DESIGN: &str = r#"## Context

Better Droid compile/lint is source-to-artifact tooling only.
"#;

const PASSING_SPEC: &str = r#"## ADDED Requirements

### Requirement: REQ-BDLCF-fixture Passing fixture requirement

The compiler SHALL accept complete Better Droid source.

#### Scenario: SCN-BDLCF-fixture Passing fixture scenario

- **GIVEN** complete source
- **WHEN** lint runs
- **THEN** import readiness is true
"#;

const UNMAPPED_SPEC: &str = r#"## ADDED Requirements

### Requirement: REQ-BDLCF-unmapped Unmapped fixture requirement

The compiler SHALL reject unmapped scenarios.

#### Scenario: SCN-BDLCF-unmapped Unmapped fixture scenario

- **GIVEN** no task traceability
- **WHEN** lint runs
- **THEN** import readiness is false
"#;

const PASSING_TASK: &str = r#"- [ ] 1.1 Implement passing fixture compiler behavior
  - **Type:** implementation
  - **Readiness:** ready-for-execution
  - **Authority Owner:** Better Droid compile/lint
  - **Purpose:** Exercise the passing fixture.
  - **Scope In:** `covey/src/ops/better_droid/**`
  - **Scope Out:** live state
  - **Dependencies:** none
  - **Allowed Read Paths:** `openspec/changes/**`
  - **Allowed Write Paths:** `covey/src/ops/better_droid/mod.rs`
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Acceptance Criteria:**
    - Lint reports import readiness.
  - **Validation / Evidence:**
    - **Command / Action:** `cargo test better_droid_lint_reports_readiness_without_writes --all-targets`
    - **Working Directory:** `/data/projects/mutai/covey`
    - **Expected Exit Code / Observation:** exits 0
    - **Required Evidence:** stdout and exit code
    - **Covers:** REQ-BDLCF-fixture, SCN-BDLCF-fixture, VAL-BDLCF-fixture
  - **Expected Artifact Kind:** patch-bundle
  - **Review Checklist:** no live state
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture, VAL-BDLCF-fixture
  - **Stale If:** fixture source changes
"#;

const VAGUE_TASK: &str = r#"- [ ] 1.1 Improve compiler behavior
  - **Type:** implementation
  - **Purpose:** Too vague.
  - **Allowed Write Paths:** `covey/src/ops/better_droid/mod.rs`
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
"#;

const UNSAFE_PATH_TASK: &str = r#"- [ ] 1.1 Implement unsafe path fixture
  - **Type:** implementation
  - **Readiness:** ready-for-execution
  - **Authority Owner:** Better Droid compile/lint
  - **Purpose:** Exercise unsafe path rejection.
  - **Scope In:** repository
  - **Scope Out:** live state
  - **Dependencies:** none
  - **Allowed Read Paths:** `openspec/changes/**`
  - **Allowed Write Paths:** repo
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Acceptance Criteria:**
    - Lint rejects unsafe path policy.
  - **Validation / Evidence:**
    - **Command / Action:** `cargo test better_droid_unsafe_path_policy_is_blocked --all-targets`
    - **Working Directory:** `/data/projects/mutai/covey`
    - **Expected Exit Code / Observation:** exits 0
    - **Required Evidence:** stdout and exit code
    - **Covers:** REQ-BDLCF-fixture, SCN-BDLCF-fixture, VAL-BDLCF-fixture
  - **Expected Artifact Kind:** patch-bundle
  - **Review Checklist:** no live state
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture, VAL-BDLCF-fixture
	  - **Stale If:** fixture source changes
	"#;

const CLASSIFICATION_TASKS: &str = r#"- [ ] task-rejected Rejected malformed task
  - **Type:** rejected implementation
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture

- [ ] task-deferred Deferred malformed task
  - **Type:** deferred implementation
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture

- [ ] 1.3 Implement high risk task
  - **Type:** implementation
  - **Purpose:** Exercise high-risk approval blocking.
  - **Allowed Write Paths:** `covey/src/ops/better_droid/mod.rs`
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Acceptance Criteria:**
    - High risk task is blocked.
  - **Validation / Evidence:**
    - **Command / Action:** `cargo test better_droid_counts_rejected_deferred_and_high_risk_blocked_tasks --all-targets`
    - **Expected Exit Code / Observation:** exits 0
    - **Covers:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Stale If:** fixture source changes
  - **Risk Level:** high
  - **Human Approval Required:** pending
"#;

const PATH_POLICY_EDGE_TASK: &str = r#"- [ ] 1.1 Implement path policy edge fixture
  - **Type:** implementation
  - **Purpose:** Exercise path policy edge blockers.
  - **Allowed Write Paths:** ., ../escape, authority/**, covey/src/ops/better_droid/mod.rs
  - **Forbidden Paths:** authority/**, contracts/imported/**, .git/**, covey/src/ops/better_droid/mod.rs
  - **Acceptance Criteria:**
    - Path policy is blocked.
  - **Validation / Evidence:**
    - **Command / Action:** `cargo test better_droid_path_policy_reports_escape_protected_and_overlap_blockers --all-targets`
    - **Expected Exit Code / Observation:** exits 0
    - **Covers:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Stale If:** fixture source changes
"#;

const NON_EXECUTABLE_TASK: &str = r#"- [ ] 1.1 Research default packet fixture
  - **Type:** note
  - **Purpose:** Exercise default packet path behavior.
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
"#;

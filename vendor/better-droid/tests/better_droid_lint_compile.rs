use std::{fs, path::Path};

use better_droid::{
    CompileOptions, ImportStatus, LintOptions, PacketKind, PlanningClass, ReportStatus,
    compile_change, lint_change, load_compiled_mission,
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

    assert_eq!(
        report.status,
        ReportStatus::CoveyImportReady,
        "{:?}",
        report.blockers
    );
    assert_eq!(report.planning_class, PlanningClass::WorkPacket);
    assert!(report.import_ready);
    assert!(report.readiness.implementation_ready);
    assert!(!report.readiness.execution_ready);
    assert!(report.blockers.is_empty());
    assert_eq!(report.task_counts.total, 1);
    assert_eq!(report.task_counts.importable, 1);
    assert!(!fixture.mission_dir("passing-fixture").exists());
}

#[test]
fn better_droid_explicit_work_packet_planning_class_is_valid() {
    let fixture = Fixture::passing();

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
    })
    .expect("lint explicit work packet");

    assert_eq!(report.planning_class, PlanningClass::WorkPacket);
    assert_eq!(report.status, ReportStatus::CoveyImportReady);
    assert!(report.import_ready);
    assert!(report.blockers.is_empty());
}

#[test]
fn better_droid_missing_planning_class_blocks_import_readiness() {
    let fixture = Fixture::passing();
    fs::write(
        fixture
            .root()
            .join("openspec/changes/passing-fixture/.openspec.yaml"),
        "schema: better-droid\n",
    )
    .expect("remove fixture planning class");

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
    })
    .expect("lint missing planning class");

    assert_eq!(report.planning_class, PlanningClass::Invalid);
    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(!report.import_ready);
    assert!(report.blockers.iter().any(|blocker| {
        blocker.id == "invalid_planning_class"
            && blocker.detail == "planning_class must be work_packet, got <missing>"
    }));
}

#[test]
fn better_droid_roadmap_planning_class_is_invalid() {
    let fixture = Fixture::passing();
    fs::write(
        fixture
            .root()
            .join("openspec/changes/passing-fixture/.openspec.yaml"),
        "schema: better-droid\nplanning_class: roadmap\n",
    )
    .expect("mark fixture roadmap");

    let report = compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
        output_dir: None,
    })
    .expect("compile invalid roadmap fixture returns blocked report");

    assert_eq!(report.planning_class, PlanningClass::Invalid);
    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(!report.import_ready);
    assert!(report.created_artifacts.is_empty());
    assert!(!fixture.mission_dir("passing-fixture").exists());
    assert!(report.blockers.iter().any(|blocker| {
        blocker.id == "invalid_planning_class"
            && blocker.detail == "planning_class must be work_packet, got roadmap"
    }));
}

#[test]
fn better_droid_invalid_planning_class_blocks_import_readiness() {
    let fixture = Fixture::passing();
    fs::write(
        fixture
            .root()
            .join("openspec/changes/passing-fixture/.openspec.yaml"),
        "schema: better-droid\nplanning_class: phase\n",
    )
    .expect("mark fixture invalid planning class");

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
    })
    .expect("lint invalid planning class");

    assert_eq!(report.planning_class, PlanningClass::Invalid);
    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(!report.import_ready);
    assert!(report.blockers.iter().any(|blocker| {
        blocker.id == "invalid_planning_class"
            && blocker.detail == "planning_class must be work_packet, got phase"
    }));
}

#[test]
fn better_droid_work_packet_rejects_phase_shaped_change_ids() {
    let fixture = Fixture::new();
    fixture.write_change(
        "magnitude-search-phase-3-controlled-peering",
        PASSING_TASK,
        PASSING_SPEC,
    );

    let report = compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "magnitude-search-phase-3-controlled-peering".to_owned(),
        output_dir: None,
    })
    .expect("compile phase-shaped fixture returns blocked report");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(!report.import_ready);
    assert!(report.created_artifacts.is_empty());
    assert!(
        report
            .blockers
            .iter()
            .any(|blocker| blocker.id == "phase_shaped_work_packet")
    );
}

#[test]
fn better_droid_planning_only_followup_packet_is_not_covey_import_ready() {
    let fixture = Fixture::new();
    fixture.write_change_with_proposal(
        "planning-only-followup-fixture",
        PLANNING_ONLY_PROPOSAL,
        PLANNING_ONLY_TASK,
        PASSING_SPEC,
    );

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "planning-only-followup-fixture".to_owned(),
    })
    .expect("lint planning-only fixture");

    assert_eq!(report.status, ReportStatus::PlanningReady);
    assert_eq!(report.packet_kind, PacketKind::PlanningOnly);
    assert!(report.readiness.planning_ready);
    assert!(report.readiness.not_imported);
    assert!(!report.readiness.covey_import_ready);
    assert!(!report.readiness.implementation_ready);
    assert!(!report.readiness.execution_ready);
    assert!(!report.import_ready);
    assert!(report.requires_implementation_packet);
    assert!(report.product_impact.product_write_paths.is_empty());
}

#[test]
fn better_droid_rejects_malformed_prose_path_policy_entries() {
    let fixture = Fixture::new();
    fixture.write_change(
        "malformed-path-policy-fixture",
        MALFORMED_PATH_TASK,
        PASSING_SPEC,
    );

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "malformed-path-policy-fixture".to_owned(),
    })
    .expect("lint malformed path policy fixture");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(!report.import_ready);
    assert!(report.blockers.iter().any(|blocker| {
        blocker.id == "unsafe_path_policy"
            && blocker
                .detail
                .contains("malformed path policy entry is not machine-enforceable")
    }));
}

#[test]
fn better_droid_rejects_product_implementation_without_product_validation() {
    let fixture = Fixture::new();
    fixture.write_change(
        "missing-product-validation-fixture",
        MISSING_PRODUCT_VALIDATION_TASK,
        PASSING_SPEC,
    );

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "missing-product-validation-fixture".to_owned(),
    })
    .expect("lint missing product validation fixture");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(!report.import_ready);
    assert!(report.blockers.iter().any(|blocker| {
        blocker.id == "missing_product_implementation_evidence"
            && blocker
                .detail
                .contains("requires validation command against product code")
    }));
}

#[test]
fn better_droid_rejects_product_implementation_without_changed_file_evidence() {
    let fixture = Fixture::new();
    fixture.write_change(
        "missing-changed-file-evidence-fixture",
        MISSING_CHANGED_FILE_EVIDENCE_TASK,
        PASSING_SPEC,
    );

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "missing-changed-file-evidence-fixture".to_owned(),
    })
    .expect("lint missing changed-file evidence fixture");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(!report.import_ready);
    assert!(report.blockers.iter().any(|blocker| {
        blocker.id == "missing_product_implementation_evidence"
            && blocker
                .detail
                .contains("requires changed-file evidence outside openspec/**")
    }));
}

#[test]
fn better_droid_apply_gate_mentions_do_not_make_implementation_apply_only() {
    let fixture = Fixture::new();
    fixture.write_change(
        "apply-gate-mention-implementation-fixture",
        APPLY_GATE_MENTION_IMPLEMENTATION_TASK,
        PASSING_SPEC,
    );

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "apply-gate-mention-implementation-fixture".to_owned(),
    })
    .expect("lint apply-gate mention fixture");

    assert_eq!(
        report.status,
        ReportStatus::CoveyImportReady,
        "{:?}",
        report.blockers
    );
    assert_eq!(report.packet_kind, PacketKind::Implementation);
    assert!(report.readiness.implementation_ready);
    assert!(!report.readiness.execution_ready);
}

#[test]
fn better_droid_human_gate_does_not_mask_other_blockers() {
    let fixture = Fixture::new();
    fixture.write_change(
        "human-gate-with-malformed-path-fixture",
        HUMAN_GATE_WITH_MALFORMED_PATH_TASK,
        PASSING_SPEC,
    );

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "human-gate-with-malformed-path-fixture".to_owned(),
    })
    .expect("lint human gate with malformed path fixture");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(!report.readiness.planning_ready);
    assert!(!report.readiness.planning_ready_blocked);
    assert!(!report.import_ready);
    assert!(
        report
            .blockers
            .iter()
            .any(|blocker| blocker.id == "human_gate_open")
    );
    assert!(
        report
            .blockers
            .iter()
            .any(|blocker| blocker.id == "unsafe_path_policy")
    );
}

#[test]
fn better_droid_work_packet_rejects_too_many_executable_tasks() {
    let fixture = Fixture::new();
    let tasks = (1..=7)
        .map(|index| executable_task(&format!("1.{index}"), &format!("Task {index}"), "none"))
        .collect::<String>();
    fixture.write_change("too-large-work-packet-fixture", &tasks, PASSING_SPEC);

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "too-large-work-packet-fixture".to_owned(),
    })
    .expect("lint too-large work packet");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(
        report
            .blockers
            .iter()
            .any(|blocker| blocker.id == "work_packet_too_large")
    );
}

#[test]
fn better_droid_work_packet_rejects_long_dependency_chains() {
    let fixture = Fixture::new();
    let tasks = [
        executable_task("1.1", "Root task", "none"),
        executable_task("1.2", "Second task", "1.1"),
        executable_task("1.3", "Third task", "1.2"),
        executable_task("1.4", "Fourth task", "1.3"),
    ]
    .join("");
    fixture.write_change("deep-chain-work-packet-fixture", &tasks, PASSING_SPEC);

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "deep-chain-work-packet-fixture".to_owned(),
    })
    .expect("lint deep dependency chain");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(
        report
            .blockers
            .iter()
            .any(|blocker| blocker.id == "work_packet_dependency_chain_too_long")
    );
}

#[test]
fn better_droid_work_packet_rejects_mixed_implementation_and_review_concerns() {
    let fixture = Fixture::new();
    let tasks = format!(
        "{}{}",
        executable_task("1.1", "Implementation task", "none"),
        r#"- [ ] 1.2 Review implementation artifact
  - **Type:** review
  - **Purpose:** Review the implementation artifact.
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture

"#
    );
    fixture.write_change("mixed-review-work-packet-fixture", &tasks, PASSING_SPEC);

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "mixed-review-work-packet-fixture".to_owned(),
    })
    .expect("lint mixed review work packet");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(
        report
            .blockers
            .iter()
            .any(|blocker| blocker.id == "mixed_review_concerns")
    );
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
            digest.starts_with("blake3:") && digest.len() == "blake3:".len() + 64
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
fn better_droid_noncanonical_task_fields_are_blocked() {
    let fixture = Fixture::new();
    fixture.write_change(
        "noncanonical-task-fields-fixture",
        NONCANONICAL_TASK_FIELDS,
        PASSING_SPEC,
    );

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "noncanonical-task-fields-fixture".to_owned(),
    })
    .expect("lint noncanonical task field fixture");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(
        report
            .blockers
            .iter()
            .any(|blocker| blocker.id == "non_canonical_task_field_syntax")
    );
    assert_eq!(
        report.task_classifications[0].task_type, "unknown",
        "noncanonical fields must not be silently accepted as typed executable work"
    );
}

#[test]
fn better_droid_spec_without_delta_sections_is_blocked() {
    let fixture = Fixture::new();
    fixture.write_change("non-delta-spec-fixture", PASSING_TASK, NON_DELTA_SPEC);

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "non-delta-spec-fixture".to_owned(),
    })
    .expect("lint non-delta spec fixture");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(
        report
            .blockers
            .iter()
            .any(|blocker| blocker.id == "missing_openspec_delta_section")
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
        "schema: better-droid\nplanning_class: work_packet\nstatus: stale_reauthor_required\n",
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
        "schema: better-droid\nplanning_class: work_packet\nstatus: stale_reauthor_required\nstale_reason: boundary drift\n",
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

    assert_eq!(report.status, ReportStatus::CoveyImportReady);
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
    assert!(!fixture.source_mission_dir("passing-fixture").exists());
}

#[test]
fn better_droid_loads_compiled_mission_through_typed_dtos() {
    let fixture = Fixture::passing();

    compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
        output_dir: None,
    })
    .expect("compile passing fixture");

    let compiled = load_compiled_mission(
        fixture.root(),
        "passing-fixture",
        Path::new("openspec/changes/passing-fixture"),
    )
    .expect("load typed compiled mission");

    assert_eq!(compiled.tasks.len(), 1);
    assert_eq!(compiled.planning_class, PlanningClass::WorkPacket);
    assert_eq!(compiled.tasks[0].task_id, "1.1");
    assert_eq!(compiled.tasks[0].task_type, "implementation");
    assert_eq!(compiled.tasks[0].import_status, ImportStatus::Importable);
    assert!(
        compiled
            .artifact_paths
            .iter()
            .any(|path| path.ends_with("mission-packet.json"))
    );
    assert!(
        compiled
            .source_digests
            .keys()
            .any(|path| path.ends_with("tasks.md"))
    );
    assert_eq!(
        compiled.artifact_digests.len(),
        compiled.artifact_paths.len()
    );
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
fn better_droid_compile_emits_source_derived_mission_packet_v1() {
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
            .is_some_and(|digest| digest.starts_with("blake3:"))
    );
    assert_eq!(packet["scheduler"]["request_id"], "passing-fixture");
    assert_eq!(
        packet["scheduler"]["activation_result"]["PlanningReady"]["classification"],
        "CompiledPlanningInputOnly"
    );
    assert_eq!(
        packet["scheduler"]["identity_refs"]["apply_gate_ref"],
        Value::Null
    );
    assert_eq!(packet["runtime"]["provider_mode"], "FakeDeliver");
    assert_eq!(
        packet["runtime"]["fleet_snapshot"]["agents"][0]["state"],
        "Ready"
    );
    assert_eq!(
        packet["runtime"]["executor_worker_id"],
        "worker-passing-fixture"
    );
    assert_eq!(packet["provider"]["provider_id"], "better-droid");
    assert_eq!(packet["provider"]["model_id"], "compiled-mission");
    assert_eq!(packet["path_policy"]["mutation_allowed"], true);
    let allowed_paths = packet["path_policy"]["allowed_paths"]
        .as_array()
        .expect("allowed paths array");
    assert!(
        allowed_paths
            .iter()
            .any(|path| path == "covey/src/ops/better_droid/mod.rs")
    );
    assert!(packet["repoops"].is_null());
    assert!(
        packet["validation"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert!(packet["validation"][0].get("working_directory").is_some());
    assert!(
        packet["review_rubric"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert!(packet["assumptions"].as_array().is_some());
}

#[test]
fn better_droid_packet_omits_fake_promoted_fleet_identity_contract() {
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
    let text = serde_json::to_string(&packet).expect("packet serializes");
    assert!(!text.contains("claim_fence_seq"));
    assert!(!text.contains("provider_run_id"));
    assert!(packet["runtime"]["promoted_fleet_identity_contract"].is_null());
    assert_eq!(
        packet["scheduler"]["identity_refs"]["selected_attempt"]["evaluator_version"],
        "better-droid:mission-packet"
    );
    assert_eq!(packet["runtime"]["provider_mode"], "FakeDeliver");
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
    assert_eq!(compile_report["planning_class"], "work_packet");
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
            ".codex/state/better-droid/passing-fixture/mission/{file_name}"
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

    assert!(
        error
            .to_string()
            .contains("outside openspec mission artifacts")
    );
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
    fs::write(
        change_dir.join(".openspec.yaml"),
        "schema: better-droid\nplanning_class: work_packet\n",
    )
    .expect("write yaml");
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

    assert_eq!(report.status, ReportStatus::PlanningReady);
    assert!(!report.import_ready);
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
fn better_droid_rejects_mission_artifacts_as_task_work() {
    let fixture = Fixture::new();
    fixture.write_change(
        "mission-artifact-task-work-fixture",
        MISSION_ARTIFACT_TASK_WORK,
        PASSING_SPEC,
    );

    let report = lint_change(&LintOptions {
        project_root: fixture.root().to_owned(),
        change_id: "mission-artifact-task-work-fixture".to_owned(),
    })
    .expect("lint mission artifact task work fixture");

    assert_eq!(report.status, ReportStatus::Blocked);
    assert!(!report.import_ready);
    let details = report
        .blockers
        .iter()
        .map(|blocker| blocker.detail.as_str())
        .collect::<Vec<_>>();
    assert!(details.iter().any(|detail| {
        detail.contains("Better Droid mission artifact path cannot be task writable")
    }));
    assert!(details.iter().any(|detail| {
        detail.contains("Better Droid mission artifact path cannot be task-generated work")
    }));
}

#[test]
fn better_droid_compile_accepts_custom_generated_output_inside_project() {
    let fixture = Fixture::passing();
    let output = fixture
        .root()
        .join(".codex/state/better-droid/passing-fixture/custom");

    let report = compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
        output_dir: Some(output.clone()),
    })
    .expect("compile to custom mission output");

    assert_eq!(report.status, ReportStatus::CoveyImportReady);
    assert!(output.join("mission.json").is_file());
    assert!(
        report
            .created_artifacts
            .iter()
            .all(|path| { path.starts_with(".codex/state/better-droid/passing-fixture/custom/") })
    );
}

#[test]
fn better_droid_compile_rejects_source_tree_mission_output() {
    let fixture = Fixture::passing();
    let output = fixture.source_mission_dir("passing-fixture");

    let error = compile_change(&CompileOptions {
        project_root: fixture.root().to_owned(),
        change_id: "passing-fixture".to_owned(),
        output_dir: Some(output),
    })
    .expect_err("source-tree mission output should fail");

    assert!(
        error
            .to_string()
            .contains("outside openspec mission artifacts")
    );
    assert!(!fixture.source_mission_dir("passing-fixture").exists());
}

#[test]
fn better_droid_compile_derives_packet_metadata_from_source() {
    let fixture = Fixture::new();
    fixture.write_change("default-packet-fixture", NON_EXECUTABLE_TASK, PASSING_SPEC);
    fs::write(
        fixture
            .root()
            .join("openspec/changes/default-packet-fixture/proposal.md"),
        "# Proposal: source derived packet fixture\n\nNo explicit objective line here.\n",
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
        "implement source derived packet fixture"
    );
    assert_eq!(packet["scheduler"]["request_id"], "default-packet-fixture");
    assert_eq!(
        packet["runtime"]["executor_worker_id"],
        "worker-default-packet-fixture"
    );
    assert_eq!(
        packet["path_policy"]["allowed_paths"],
        serde_json::json!([
            "openspec/changes/default-packet-fixture/.openspec.yaml",
            "openspec/changes/default-packet-fixture/design.md",
            "openspec/changes/default-packet-fixture/proposal.md",
            "openspec/changes/default-packet-fixture/specs/**",
            "openspec/changes/default-packet-fixture/tasks.md",
            "openspec/config.yaml",
            "openspec/schemas/better-droid/**"
        ])
    );
    let path_policy = read_json(
        &fixture
            .mission_dir("default-packet-fixture")
            .join("path-policy.json"),
    );
    assert_eq!(path_policy["allowed_write_paths"], serde_json::json!([]));
    assert_eq!(
        path_policy["generated_paths"],
        serde_json::json!([".codex/state/better-droid/default-packet-fixture/mission/*.json"])
    );
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read json")).expect("parse json")
}

fn executable_task(task_id: &str, title: &str, dependencies: &str) -> String {
    format!(
        r#"- [ ] {task_id} Implement {title}
  - **Type:** implementation
  - **Readiness:** execution_ready
  - **Authority Owner:** Better Droid compile/lint
  - **Purpose:** Exercise {title}.
  - **Scope In:** `covey/src/ops/better_droid/**`
  - **Scope Out:** live state
  - **Dependencies:** {dependencies}
  - **Allowed Read Paths:** `openspec/changes/**`
  - **Allowed Write Paths:** `covey/src/ops/better_droid/mod.rs`
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Acceptance Criteria:**
    - Lint reports the expected readiness result.
  - **Validation / Evidence:**
    - **Command / Action:** `cargo test better_droid_lint_compile --test better_droid_lint_compile`
    - **Working Directory:** `/data/projects/mutai/better-droid`
    - **Expected Exit Code / Observation:** exits 0
    - **Required Evidence:** stdout, exit code, and changed-file list outside openspec/**
    - **Covers:** REQ-BDLCF-fixture, SCN-BDLCF-fixture, VAL-BDLCF-fixture
  - **Expected Artifact Kind:** patch-bundle
  - **Review Checklist:** no live state
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture, VAL-BDLCF-fixture
  - **Stale If:** fixture source changes

"#
    )
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
            .join(".codex")
            .join("state")
            .join("better-droid")
            .join(change_id)
            .join("mission")
    }

    fn source_mission_dir(&self, change_id: &str) -> std::path::PathBuf {
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
        fs::write(
            change_dir.join(".openspec.yaml"),
            "schema: better-droid\nplanning_class: work_packet\n",
        )
        .expect("write yaml");
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
        self.write_change_with_proposal(change_id, PROPOSAL, tasks, spec);
    }

    fn write_change_with_proposal(&self, change_id: &str, proposal: &str, tasks: &str, spec: &str) {
        let change_dir = self.root().join("openspec").join("changes").join(change_id);
        fs::create_dir_all(change_dir.join("specs/better-droid-lint-compile-first"))
            .expect("create spec dirs");
        fs::write(
            change_dir.join(".openspec.yaml"),
            "schema: better-droid\nplanning_class: work_packet\n",
        )
        .expect("write yaml");
        fs::write(change_dir.join("proposal.md"), proposal).expect("write proposal");
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

const PLANNING_ONLY_PROPOSAL: &str = r#"## Mission Readiness Expectations

### Mission Objective
- **Objective:** Plan product implementation but do not implement code directly.

## Notes

This is a planning packet. Follow-up implementation is required before product
milestones can be satisfied.
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

const NON_DELTA_SPEC: &str = r#"## Requirements

### Requirement: REQ-BDLCF-fixture Passing fixture requirement

The compiler SHALL reject specs that do not use OpenSpec delta headings.

#### Scenario: SCN-BDLCF-fixture Passing fixture scenario

- **WHEN** lint runs
- **THEN** import readiness is false
"#;

const PASSING_TASK: &str = r#"- [ ] 1.1 Implement passing fixture compiler behavior
  - **Type:** implementation
  - **Readiness:** execution_ready
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
    - **Required Evidence:** stdout, exit code, and changed-file list outside openspec/**
    - **Covers:** REQ-BDLCF-fixture, SCN-BDLCF-fixture, VAL-BDLCF-fixture
  - **Expected Artifact Kind:** patch-bundle
  - **Review Checklist:** no live state
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture, VAL-BDLCF-fixture
  - **Stale If:** fixture source changes
"#;

const PLANNING_ONLY_TASK: &str = r#"- [ ] 1.1 Define implementation follow-up packet
  - **Type:** documentation
  - **Purpose:** Define product implementation tasks without mutating product code.
  - **Allowed Read Paths:** `openspec/changes/**`
  - **Allowed Write Paths:** `openspec/changes/planning-only-followup-fixture/**`
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Acceptance Criteria:**
    - Follow-up implementation packet requirements are documented.
  - **Validation / Evidence:**
    - **Command / Action:** `openspec validate planning-only-followup-fixture --strict`
    - **Expected Exit Code / Observation:** exits 0
    - **Covers:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Expected Artifact Kind:** planning-artifact
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Stale If:** product implementation paths are selected
"#;

const MALFORMED_PATH_TASK: &str = r#"- [ ] 1.1 Implement malformed path fixture
  - **Type:** implementation
  - **Purpose:** Exercise malformed path policy rejection.
  - **Allowed Read Paths:** `openspec/changes/**`
  - **Allowed Write Paths:** `covey/src/ops/import` and related files
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Acceptance Criteria:**
    - Malformed path policy is blocked.
  - **Validation / Evidence:**
    - **Command / Action:** `cargo test better_droid_rejects_malformed_prose_path_policy_entries --all-targets`
    - **Expected Exit Code / Observation:** exits 0
    - **Covers:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Expected Artifact Kind:** patch-bundle
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Stale If:** path policy parser changes
"#;

const MISSING_PRODUCT_VALIDATION_TASK: &str = r#"- [ ] 1.1 Implement product validation fixture
  - **Type:** implementation
  - **Readiness:** execution_ready
  - **Authority Owner:** Better Droid compile/lint
  - **Purpose:** Exercise product validation gating.
  - **Scope In:** `covey/src/ops/import/**`
  - **Scope Out:** live state
  - **Dependencies:** none
  - **Allowed Read Paths:** `openspec/changes/**`
  - **Allowed Write Paths:** `covey/src/ops/import/openspec/source.rs`
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Acceptance Criteria:**
    - Import behavior rejects missing product validation evidence.
  - **Validation / Evidence:**
    - **Command / Action:** `openspec validate missing-product-validation-fixture --strict`
    - **Expected Exit Code / Observation:** exits 0
    - **Covers:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Expected Artifact Kind:** patch-bundle
  - **Review Checklist:** product validation is required
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Stale If:** validation policy changes
"#;

const MISSING_CHANGED_FILE_EVIDENCE_TASK: &str = r#"- [ ] 1.1 Implement changed-file evidence fixture
  - **Type:** implementation
  - **Readiness:** execution_ready
  - **Authority Owner:** Better Droid compile/lint
  - **Purpose:** Exercise changed-file evidence gating.
  - **Scope In:** `covey/src/ops/import/**`
  - **Scope Out:** live state
  - **Dependencies:** none
  - **Allowed Read Paths:** `openspec/changes/**`
  - **Allowed Write Paths:** `covey/src/ops/import/openspec/source.rs`
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Acceptance Criteria:**
    - Import behavior rejects missing changed-file evidence.
  - **Validation / Evidence:**
    - **Command / Action:** `cargo test -p covey import_openspec`
    - **Expected Exit Code / Observation:** exits 0
    - **Required Evidence:** stdout and exit code
    - **Covers:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Expected Artifact Kind:** patch-bundle
  - **Review Checklist:** changed-file evidence is required
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Stale If:** fixture source changes
"#;

const APPLY_GATE_MENTION_IMPLEMENTATION_TASK: &str = r#"- [ ] 1.1 Implement settlement boundary evidence fixture
  - **Type:** implementation
  - **Readiness:** execution_ready
  - **Authority Owner:** Better Droid compile/lint
  - **Purpose:** Exercise implementation packet classification when text mentions the apply gate and says historical artifacts can be migrated by a separate change.
  - **Scope In:** `authority/src/settlement_authority/**`
  - **Scope Out:** live state
  - **Dependencies:** none
  - **Allowed Read Paths:** `openspec/changes/**`
  - **Allowed Write Paths:** `authority/src/settlement_authority/mod.rs`
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Acceptance Criteria:**
    - Implementation behavior rejects apply gate bypasses.
  - **Validation / Evidence:**
    - **Command / Action:** `cargo test -p mutai-rs settlement_authority`
    - **Expected Exit Code / Observation:** exits 0
    - **Required Evidence:** stdout, exit code, and changed-file list outside openspec/**
    - **Covers:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Expected Artifact Kind:** patch-bundle
  - **Review Checklist:** apply gate mention does not change packet kind
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Stale If:** packet classifier changes
"#;

const HUMAN_GATE_WITH_MALFORMED_PATH_TASK: &str = r#"- [ ] 1.1 Implement human gate malformed path fixture
  - **Type:** implementation
  - **Readiness:** blocked-needs-human
  - **Authority Owner:** Better Droid compile/lint
  - **Purpose:** Exercise human gate plus non-human blocker classification.
  - **Scope In:** `covey/src/ops/import/**`
  - **Scope Out:** live state
  - **Dependencies:** none
  - **Allowed Read Paths:** `openspec/changes/**`
  - **Allowed Write Paths:** `covey/src/ops/import` and related files
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Acceptance Criteria:**
    - Import behavior rejects malformed path policy even when a human gate is open.
  - **Validation / Evidence:**
    - **Command / Action:** `cargo test -p covey import_openspec`
    - **Expected Exit Code / Observation:** exits 0
    - **Required Evidence:** stdout, exit code, and changed-file list outside openspec/**
    - **Covers:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Expected Artifact Kind:** patch-bundle
  - **Review Checklist:** human approval does not mask path-policy blockers
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - **Stale If:** human gate policy changes
"#;

const VAGUE_TASK: &str = r#"- [ ] 1.1 Improve compiler behavior
  - **Type:** implementation
  - **Purpose:** Too vague.
  - **Allowed Write Paths:** `covey/src/ops/better_droid/mod.rs`
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture
"#;

const NONCANONICAL_TASK_FIELDS: &str = r#"- [ ] 1.1 Implement noncanonical task fixture
  - Type: implementation
  - Purpose: Exercise noncanonical task field rejection.
  - Allowed Read Paths: `openspec/changes/**`
  - Allowed Write Paths: `covey/src/ops/better_droid/mod.rs`
  - Forbidden Paths: `authority/**`, `contracts/imported/**`, `.git/**`
  - Acceptance Criteria: Lint rejects noncanonical field syntax.
  - Validation: `cargo test better_droid_noncanonical_task_fields_are_blocked --all-targets`
  - Expected Artifact Kind: patch-bundle
  - Review Checklist: noncanonical fields are blockers.
  - Traceability Refs: REQ-BDLCF-fixture, SCN-BDLCF-fixture
  - Stale-if: fixture source changes
"#;

const UNSAFE_PATH_TASK: &str = r#"- [ ] 1.1 Implement unsafe path fixture
  - **Type:** implementation
  - **Readiness:** execution_ready
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
    - **Required Evidence:** stdout, exit code, and changed-file list outside openspec/**
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

const MISSION_ARTIFACT_TASK_WORK: &str = r#"- [ ] 1.1 Regenerate mission packet
  - **Type:** verification
  - **Readiness:** execution_ready
  - **Authority Owner:** Better Droid compile/lint
  - **Purpose:** Exercise mission artifact path rejection.
  - **Scope In:** Better Droid generated mission packet.
  - **Scope Out:** product source mutation.
  - **Dependencies:** none.
  - **Allowed Read Paths:** `openspec/changes/mission-artifact-task-work-fixture/**`
  - **Allowed Write Paths:** `openspec/changes/mission-artifact-task-work-fixture/mission/*.json`
  - **Generated Paths:** `openspec/changes/mission-artifact-task-work-fixture/mission/*.json`
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Acceptance Criteria:**
    - Lint rejects generated mission artifacts as task work.
  - **Validation / Evidence:**
    - **Command / Action:** `cargo test better_droid_rejects_mission_artifacts_as_task_work --test better_droid_lint_compile`
    - **Working Directory:** `/data/projects/mutai/better-droid`
    - **Expected Exit Code / Observation:** exits 0
    - **Required Evidence:** stdout and blocker details
    - **Covers:** REQ-BDLCF-fixture, SCN-BDLCF-fixture, VAL-BDLCF-fixture
  - **Expected Artifact Kind:** verification-bundle
  - **Review Checklist:** generated mission artifacts are not task deliverables.
  - **Traceability Refs:** REQ-BDLCF-fixture, SCN-BDLCF-fixture, VAL-BDLCF-fixture
  - **Stale If:** Better Droid path policy changes
"#;

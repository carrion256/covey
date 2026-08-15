use crate::lint::path_policy_blockers;
use crate::model::{PlanningClass, SourceFile, SourceSnapshot, SourceTask};
use crate::report::{Blocker, LintState, ReportStatus, report_from_lint};
use crate::source::{
    blake3_digest, canonical_json_digest, canonicalize_value, normalize_path_lossy, parse_specs,
    parse_tasks, stable_id,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[test]
fn report_status_display_and_report_readiness_are_stable() {
    assert_eq!(
        ReportStatus::CoveyImportReady.to_string(),
        "covey_import_ready"
    );
    assert_eq!(ReportStatus::PlanningReady.to_string(), "planning_ready");
    assert_eq!(ReportStatus::Blocked.to_string(), "blocked");

    let source = SourceSnapshot {
        change_id: "better-droid-unit".into(),
        relative_change_path: "openspec/changes/better-droid-unit".into(),
        schema_name: "better-droid".into(),
        openspec_yaml: source_file(
            "openspec/changes/better-droid-unit/openspec.yaml",
            "planning_class: work_packet\n",
        ),
        change_status: None,
        planning_class: PlanningClass::WorkPacket,
        planning_class_raw: Some("work_packet".into()),
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
    assert_eq!(ready.status, ReportStatus::PlanningReady);
    assert!(!ready.import_ready);

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
        field_syntax_errors: Vec::new(),
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
        digest: blake3_digest(text.as_bytes()),
    }
}

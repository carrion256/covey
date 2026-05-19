use std::{fs, path::Path};

use better_droid::{CompileOptions, LintOptions, ReportStatus, compile_change, lint_change};
use covey::{Covey, ImportOpenSpecReq, ObjectType, RegisterSessionReq, SessionRole};
use tempfile::TempDir;

#[test]
fn better_droid_compiled_change_imports_through_covey() {
    let tmp = TempDir::new().expect("tempdir");
    seed_better_droid_change(tmp.path(), "covey-bd-lint-compile");

    let lint = lint_change(&LintOptions {
        project_root: tmp.path().to_path_buf(),
        change_id: "covey-bd-lint-compile".to_owned(),
    })
    .expect("lint better droid change");
    assert_eq!(lint.status, ReportStatus::Ready);
    assert!(lint.import_ready);

    let compile = compile_change(&CompileOptions {
        project_root: tmp.path().to_path_buf(),
        change_id: "covey-bd-lint-compile".to_owned(),
        output_dir: None,
    })
    .expect("compile better droid change");
    assert_eq!(compile.status, ReportStatus::Ready);
    assert!(
        tmp.path()
            .join("openspec/changes/covey-bd-lint-compile/mission/mission-packet.json")
            .is_file()
    );

    let covey = Covey::open(tmp.path().join("covey.db")).expect("open covey");
    let orchestrator = covey
        .register_session(RegisterSessionReq {
            agent_principal_id: "orch-bd-lint-compile".to_owned(),
            agent_instance_id: "orch-bd-lint-compile-1".to_owned(),
            role: SessionRole::Orchestrator,
            idempotency_key: "register-orch-bd-lint-compile".to_owned(),
        })
        .expect("register orchestrator");
    let imported = covey
        .import_openspec(ImportOpenSpecReq {
            session_token: Some(orchestrator.session_token),
            change_id: "covey-bd-lint-compile".to_owned(),
            project_root: tmp.path().to_string_lossy().to_string(),
            dry_run: false,
        })
        .expect("import compiled openspec change");

    assert!(imported.conflicts.is_empty());
    assert_eq!(imported.created, 2);
    assert!(imported.items.iter().any(|item| {
        item.object_type == ObjectType::Subtask
            && item.openspec_task_id.as_deref() == Some("1.1")
            && item.title.as_deref() == Some("Import compiled packet")
    }));
}

fn seed_better_droid_change(root: &Path, change_id: &str) {
    let change_dir = root.join("openspec").join("changes").join(change_id);
    fs::create_dir_all(change_dir.join("specs").join("covey")).expect("create dirs");
    fs::write(change_dir.join(".openspec.yaml"), "schema: better-droid\n").expect("yaml");
    fs::write(
        change_dir.join("proposal.md"),
        "## Why\n\nCompile source for Covey import tests.\n",
    )
    .expect("proposal");
    fs::write(change_dir.join("design.md"), "## Context\n\nTest design.\n").expect("design");
    fs::write(
        change_dir.join("specs").join("covey").join("spec.md"),
        "## MODIFIED Requirements\n\n### Requirement: REQ-COVEY-BD Mission import\nThe system SHALL import compiled packets.\n\n#### Scenario: SCN-COVEY-BD-001 Valid packet\n- **WHEN** imported\n- **THEN** it succeeds\n",
    )
    .expect("spec");
    fs::write(
        change_dir.join("tasks.md"),
        r#"## 1. Implementation

- [ ] 1.1 Import compiled packet
  - **Type:** implementation
  - **Purpose:** Exercise compiled packet import for SCN-COVEY-BD-001 and REQ-COVEY-BD.
  - **Dependencies:** none.
  - **Allowed Read Paths:** `openspec/changes/**`.
  - **Allowed Write Paths:** `covey/src/ops/import/openspec/source.rs`.
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`.
  - **Acceptance Criteria:** Compiled packet imports.
  - **Validation / Evidence:**
    - **Command / Action:** `cargo test -p covey --test better_droid_lint_compile`
    - **Expected Exit Code / Observation:** exits 0.
  - **Traceability Refs:** REQ-COVEY-BD, SCN-COVEY-BD-001, VAL-COVEY-BD-001.
  - **Stale If:** source changes.
"#,
    )
    .expect("tasks");
}

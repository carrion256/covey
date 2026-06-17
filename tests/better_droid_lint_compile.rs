use std::{fs, path::Path};

use better_droid::{CompileOptions, LintOptions, ReportStatus, compile_change, lint_change};
use covey::{
    ArtifactKind, ClaimNextReq, Covey, DecideReviewReq, ImportOpenSpecReq, ObjectType,
    PublishArtifactReq, RegisterSessionReq, RequestReviewReq, ReviewDecisionResult, ReviewVerdict,
    SessionRole, StartSubtaskReq,
};
use rusqlite::Connection;
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
    assert_eq!(lint.status, ReportStatus::CoveyImportReady);
    assert!(lint.import_ready);

    let compile = compile_change(&CompileOptions {
        project_root: tmp.path().to_path_buf(),
        change_id: "covey-bd-lint-compile".to_owned(),
        output_dir: None,
    })
    .expect("compile better droid change");
    assert_eq!(compile.status, ReportStatus::CoveyImportReady);
    assert!(
        tmp.path()
            .join(".codex/state/better-droid/covey-bd-lint-compile/mission/mission-packet.json")
            .is_file()
    );

    let covey = Covey::open(tmp.path().join("covey.db")).expect("open covey");
    let orchestrator = covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                "orch-bd-lint-compile",
                "orch-bd-lint-compile-1",
                SessionRole::Orchestrator,
                "register-orch-bd-lint-compile",
            )
            .expect("valid session registration request"),
        )
        .expect("register orchestrator");
    let imported = covey
        .import_openspec(ImportOpenSpecReq::write(
            orchestrator.session_token,
            "covey-bd-lint-compile",
            tmp.path().to_string_lossy().to_string(),
        ))
        .expect("import compiled openspec change");

    assert!(imported.conflicts().is_empty());
    assert_eq!(imported.created(), 2);
    assert!(imported.items().iter().any(|item| {
        item.object_type() == ObjectType::Subtask
            && item.openspec_task_id() == Some("1.1")
            && item.title() == "Import compiled packet"
    }));
}

#[test]
fn changes_requested_followup_is_scoped_to_openspec_task_and_scenarios() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("covey.db");
    seed_better_droid_change(tmp.path(), "covey-bd-followup-scope");
    compile_change(&CompileOptions {
        project_root: tmp.path().to_path_buf(),
        change_id: "covey-bd-followup-scope".to_owned(),
        output_dir: None,
    })
    .expect("compile better droid change");

    let covey = Covey::open(&db_path).expect("open covey");
    let orchestrator = covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                "orch-bd-followup",
                "orch-bd-followup-1",
                SessionRole::Orchestrator,
                "register-orch-bd-followup",
            )
            .expect("valid orchestrator session"),
        )
        .expect("register orchestrator");
    let imported = covey
        .import_openspec(ImportOpenSpecReq::write(
            orchestrator.session_token,
            "covey-bd-followup-scope",
            tmp.path().to_string_lossy().to_string(),
        ))
        .expect("import compiled openspec change");
    let subtask_id = imported
        .items()
        .iter()
        .find(|item| item.object_type() == ObjectType::Subtask)
        .map(|item| item.object_id().to_owned())
        .expect("imported subtask id");

    let executor = covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                "exec-bd-followup",
                "exec-bd-followup-1",
                SessionRole::Executor,
                "register-exec-bd-followup",
            )
            .expect("valid executor session"),
        )
        .expect("register executor");
    let reviewer = covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                "review-bd-followup",
                "review-bd-followup-1",
                SessionRole::Reviewer,
                "register-review-bd-followup",
            )
            .expect("valid reviewer session"),
        )
        .expect("register reviewer");

    let work_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                executor.session_token.clone(),
                30_000,
                "claim-work-bd-followup",
            )
            .expect("valid work claim-next"),
        )
        .expect("claim work")
        .expect("work claim available");
    assert_eq!(work_claim.subtask_id.as_str(), subtask_id);
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                executor.session_token.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                "start-work-bd-followup",
            )
            .expect("valid start work"),
        )
        .expect("start work");
    let artifact_digest =
        "blake3:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned();
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                executor.session_token.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                artifact_digest.clone(),
                ArtifactKind::PatchBundle,
                "rev-bd-followup".to_owned(),
                "artifacts/covey-bd-followup/manifest.json".to_owned(),
                "blake3:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    .to_owned(),
                "publish-work-bd-followup",
            )
            .expect("valid publish artifact"),
        )
        .expect("publish artifact");
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                executor.session_token.clone(),
                subtask_id.clone(),
                artifact_digest,
                None,
                100,
                "request-review-bd-followup",
            )
            .expect("valid request review"),
        )
        .expect("request review");

    let review_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                reviewer.session_token.clone(),
                30_000,
                "claim-review-bd-followup",
            )
            .expect("valid review claim-next"),
        )
        .expect("claim review")
        .expect("review claim available");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.session_token.clone(),
                review_claim.claim_id.clone(),
                review_claim.fence_seq,
                "start-review-bd-followup",
            )
            .expect("valid start review"),
        )
        .expect("start review");
    let decision = covey
        .decide_review(
            DecideReviewReq::try_from_raw_parts(
                reviewer.session_token,
                review_id.clone(),
                review_claim.claim_id,
                review_claim.fence_seq,
                ReviewVerdict::ChangesRequested,
                "blake3:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                    .to_owned(),
                "decide-review-bd-followup",
            )
            .expect("valid decide review"),
        )
        .expect("decide review");
    let followup_subtask_id = match decision {
        ReviewDecisionResult::Failed {
            followup_subtask_id,
            ..
        } => followup_subtask_id,
        ReviewDecisionResult::Approved { .. } => panic!("changes_requested must create follow-up"),
    };

    let conn = Connection::open(&db_path).expect("open sqlite for assertion");
    let (title, repair_source_path, repair_task_ref, repair_scenario_refs_json): (
        String,
        String,
        String,
        String,
    ) = conn
        .query_row(
            r#"
            SELECT s.title, f.repair_source_path, f.repair_task_ref, f.repair_scenario_refs_json
            FROM review_followup_subtasks f
            JOIN subtasks s ON s.subtask_id = f.followup_subtask_id
            WHERE f.review_id = ?1 AND f.followup_subtask_id = ?2
            "#,
            (review_id.as_str(), followup_subtask_id.as_str()),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("follow-up scope row");
    let scenario_refs = serde_json::from_str::<Vec<String>>(&repair_scenario_refs_json)
        .expect("scenario refs json");
    assert_eq!(
        repair_source_path,
        "openspec/changes/covey-bd-followup-scope/tasks.md"
    );
    assert_eq!(repair_task_ref, "covey-bd-followup-scope:1.1");
    assert_eq!(scenario_refs, vec!["SCN-COVEY-BD-001"]);
    assert!(title.contains("covey-bd-followup-scope:1.1"));
    assert!(title.contains("SCN-COVEY-BD-001"));
}

fn seed_better_droid_change(root: &Path, change_id: &str) {
    let change_dir = root.join("openspec").join("changes").join(change_id);
    fs::create_dir_all(change_dir.join("specs").join("covey")).expect("create dirs");
    fs::write(
        change_dir.join(".openspec.yaml"),
        "schema: better-droid\nplanning_class: work_packet\n",
    )
    .expect("yaml");
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
  - **Allowed Read Paths:** `openspec/changes/**`
  - **Allowed Write Paths:** `covey/src/ops/import/openspec/source.rs`
  - **Forbidden Paths:** `authority/**`, `contracts/imported/**`, `.git/**`
  - **Acceptance Criteria:** Compiled packet imports.
  - **Validation / Evidence:**
    - **Command / Action:** `cargo test -p covey --test better_droid_lint_compile`
    - **Expected Exit Code / Observation:** exits 0.
    - **Required Evidence:** stdout, exit code, and changed-file list outside openspec/**.
  - **Traceability Refs:** REQ-COVEY-BD, SCN-COVEY-BD-001, VAL-COVEY-BD-001.
  - **Stale If:** source changes.
"#,
    )
    .expect("tasks");
}

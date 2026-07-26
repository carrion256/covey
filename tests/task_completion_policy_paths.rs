use std::sync::Arc;

use covey::{
    ArtifactKind, ClaimNextReq, ClaimNextRoutedReq, CompletionPolicy, Covey, CreateWorkSubtaskReq,
    DecideReviewReq, ManualClock, PublishArtifactReq, RegisterSessionReq, RequestReservationReq,
    RequestReviewReq, ReviewVerdict, RoutingKey, ScopeClass, SessionRole, StartSubtaskReq,
    SubmitMetaTaskReq, SubtaskState,
};
use rusqlite::{Connection, params};
use tempfile::TempDir;

const NOW_MS: i64 = 1_700_000_000_000;

fn register(covey: &Covey, principal: &str, role: SessionRole) -> String {
    covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                principal,
                format!("{principal}-instance"),
                role,
                format!("register-{principal}"),
            )
            .expect("valid registration"),
        )
        .expect("register session")
        .session_token
        .to_string()
}

#[test]
fn reviewed_approval_completes_without_apply_and_closes_producer_claim() {
    let dir = TempDir::new().expect("temporary directory");
    let db_path = dir.path().join("covey.db");
    let clock = Arc::new(ManualClock::new(NOW_MS));
    let covey = Covey::open_with_clock(&db_path, clock.clone()).expect("open Covey");
    let orchestrator = register(&covey, "orchestrator", SessionRole::Orchestrator);
    let executor = register(&covey, "executor", SessionRole::Executor);
    let reviewer = register(&covey, "reviewer", SessionRole::Reviewer);
    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orchestrator.clone(),
                "review a non-settlement result",
                "submit-reviewed-meta",
            )
            .expect("valid meta task"),
        )
        .expect("submit meta task");
    let subtask_id = covey
        .create_work_subtask(
            CreateWorkSubtaskReq::try_from_raw_parts(
                orchestrator.clone(),
                meta_task_id.clone(),
                Some("reviewed-work".to_owned()),
                "produce findings",
                10,
                CompletionPolicy::Reviewed,
                "hermes",
                "create-reviewed-work",
            )
            .expect("valid reviewed work"),
        )
        .expect("create reviewed work");
    let work_claim = covey
        .claim_next_routed_subtask(
            ClaimNextRoutedReq::try_from_raw_parts(
                executor.clone(),
                10,
                "hermes",
                Some(meta_task_id.clone()),
                "claim-reviewed-work",
            )
            .expect("valid routed claim"),
        )
        .expect("claim reviewed work")
        .expect("reviewed work available");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                executor.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                "start-reviewed-work",
            )
            .expect("valid start"),
        )
        .expect("start reviewed work");
    let reservation_id = covey
        .request_reservation(
            RequestReservationReq::try_from_raw_parts(
                orchestrator,
                subtask_id.clone(),
                ScopeClass::RepoGlobal,
                "repo",
                Vec::new(),
                30_000,
                "reserve-reviewed-work",
            )
            .expect("valid reservation request"),
        )
        .expect("reserve reviewed work");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                executor.clone(),
                work_claim.claim_id,
                work_claim.fence_seq,
                "blake3:reviewed_findings".to_owned(),
                ArtifactKind::FindingsBundle,
                "none".to_owned(),
                "findings.json".to_owned(),
                "blake3:no_changed_paths".to_owned(),
                "publish-reviewed-findings",
            )
            .expect("valid findings artifact"),
        )
        .expect("publish findings");
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                executor.clone(),
                subtask_id.clone(),
                "blake3:reviewed_findings",
                Some("reviewed-work-review".to_owned()),
                1,
                "request-reviewed-review",
            )
            .expect("valid review request"),
        )
        .expect("request review");

    let review_status = covey
        .subtask_status("reviewed-work-review")
        .expect("read generated review status");
    assert_eq!(
        review_status.subtask().completion_policy(),
        CompletionPolicy::CanonicalApply
    );
    assert_eq!(review_status.subtask().routing_key().as_str(), "mutai");
    let hermes_route = RoutingKey::parse("hermes").expect("valid Hermes route");
    assert!(
        covey
            .subtask_candidates_routed(
                SessionRole::Reviewer,
                &hermes_route,
                10,
                Some(&meta_task_id),
            )
            .expect("read Hermes reviewer candidates")
            .is_empty()
    );
    assert_eq!(
        covey
            .subtask_candidates(SessionRole::Reviewer, 10, Some(&meta_task_id))
            .expect("read shared reviewer candidates")
            .len(),
        1
    );

    clock.advance(11);
    assert_eq!(
        covey
            .expire_old_claims()
            .expect("expire producer claim")
            .expired_count,
        1
    );

    let review_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts_scoped(
                reviewer.clone(),
                30_000,
                Some(meta_task_id.clone()),
                "claim-reviewed-review",
            )
            .expect("valid reviewer claim"),
        )
        .expect("claim review")
        .expect("review available");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                review_claim.claim_id.clone(),
                review_claim.fence_seq,
                "start-reviewed-review",
            )
            .expect("valid review start"),
        )
        .expect("start review");
    covey
        .decide_review(
            DecideReviewReq::try_from_raw_parts(
                reviewer.clone(),
                review_id,
                review_claim.claim_id,
                review_claim.fence_seq,
                ReviewVerdict::Approve,
                "blake3:review_approval".to_owned(),
                "approve-reviewed-work",
            )
            .expect("valid review decision"),
        )
        .expect("approve reviewed work");

    let status = covey
        .subtask_status(&subtask_id)
        .expect("read reviewed work status");
    assert_eq!(status.subtask().state(), SubtaskState::Completed);
    assert_eq!(
        status.subtask().completion_policy(),
        CompletionPolicy::Reviewed
    );
    assert!(status.claim().is_none());
    assert!(status.attempt_outcomes().is_empty());
    assert!(status.ready_queue().is_empty());
    let reservation_state: String = Connection::open(&db_path)
        .expect("open assertion database")
        .query_row(
            "SELECT state FROM reservations WHERE reservation_id = ?1",
            params![reservation_id],
            |row| row.get(0),
        )
        .expect("read reservation state");
    assert_eq!(reservation_state, "released");
    assert!(
        covey
            .session_status(&executor)
            .expect("read executor status")
            .session()
            .active_subtask_id()
            .is_none()
    );
    assert!(
        covey
            .session_status(&reviewer)
            .expect("read reviewer status")
            .session()
            .active_subtask_id()
            .is_none()
    );
    assert_eq!(
        covey
            .meta_task_status(&meta_task_id)
            .expect("read meta status")
            .meta_task()
            .state(),
        covey::MetaTaskState::Completed
    );
}

#[test]
fn assurance_artifact_kinds_are_policy_gated_without_partial_writes() {
    let dir = TempDir::new().expect("temporary directory");
    let covey = Covey::open(dir.path().join("covey.db")).expect("open Covey");
    let orchestrator = register(&covey, "orchestrator", SessionRole::Orchestrator);
    let executor = register(&covey, "executor", SessionRole::Executor);
    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orchestrator.clone(),
                "policy guard test",
                "submit-policy-meta",
            )
            .expect("valid meta task"),
        )
        .expect("submit meta task");
    let subtask_id = covey
        .create_work_subtask(
            CreateWorkSubtaskReq::try_from_raw_parts(
                orchestrator,
                meta_task_id,
                Some("reviewed-patch".to_owned()),
                "must not publish a mutation artifact",
                10,
                CompletionPolicy::Reviewed,
                "hermes",
                "create-reviewed-patch",
            )
            .expect("valid reviewed task"),
        )
        .expect("create reviewed task");
    let claim = covey
        .claim_next_routed_subtask(
            ClaimNextRoutedReq::try_from_raw_parts(
                executor.clone(),
                30_000,
                "hermes",
                None,
                "claim-reviewed-patch",
            )
            .expect("valid claim"),
        )
        .expect("claim task")
        .expect("task available");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                executor.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "start-reviewed-patch",
            )
            .expect("valid start"),
        )
        .expect("start task");

    let error = covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                executor,
                claim.claim_id,
                claim.fence_seq,
                "blake3:forbidden_patch".to_owned(),
                ArtifactKind::PatchBundle,
                "base".to_owned(),
                "patch.json".to_owned(),
                "blake3:changed_paths".to_owned(),
                "publish-forbidden-patch",
            )
            .expect("valid artifact request"),
        )
        .expect_err("reviewed work cannot publish mutation artifacts");
    assert!(matches!(
        error,
        covey::CoveyError::CompletionPolicyViolation { .. }
    ));
    let status = covey.subtask_status(&subtask_id).expect("read status");
    assert_eq!(status.subtask().state(), SubtaskState::InProgress);
    assert!(status.artifact().is_none());
    assert!(status.attempt_outcomes().is_empty());
    assert!(status.ready_queue().is_empty());
}

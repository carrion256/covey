use std::sync::Arc;

use covey::{
    ClaimNextReq, ClaimNextRoutedReq, CompletionPolicy, Covey, CreateSubtaskRequest,
    CreateWorkSubtaskReq, ManualClock, RegisterSessionReq, RoutingKey, SessionRole,
    SubmitMetaTaskReq,
};
use rusqlite::Connection;
use tempfile::TempDir;

fn register(covey: &Covey, principal: &str, role: SessionRole) -> String {
    covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                principal,
                format!("{principal}-instance"),
                role,
                format!("register-{principal}"),
            )
            .expect("valid session registration"),
        )
        .expect("session registration succeeds")
        .session_token
        .to_string()
}

#[test]
fn explicit_routing_isolated_from_legacy_mutai_claim_next() {
    let dir = TempDir::new().expect("temporary directory");
    let db_path = dir.path().join("covey.db");
    let covey = Covey::open_with_clock(&db_path, Arc::new(ManualClock::new(1_700_000_000_000)))
        .expect("open Covey");
    let orchestrator = register(&covey, "orchestrator", SessionRole::Orchestrator);
    let legacy_executor = register(&covey, "legacy-executor", SessionRole::Executor);
    let routed_executor = register(&covey, "routed-executor", SessionRole::Executor);
    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orchestrator.clone(),
                "mixed routing test",
                "submit-meta",
            )
            .expect("valid meta task"),
        )
        .expect("submit meta task");

    let routed_subtask_id = covey
        .create_work_subtask(
            CreateWorkSubtaskReq::try_from_raw_parts(
                orchestrator.clone(),
                meta_task_id.clone(),
                Some("hermes-work".to_owned()),
                "Hermes queue work",
                0,
                CompletionPolicy::Direct,
                "hermes",
                "create-hermes-work",
            )
            .expect("valid explicit work"),
        )
        .expect("create routed work");
    let legacy_subtask_id = covey
        .create_subtask(
            CreateSubtaskRequest::try_from_raw_parts(
                orchestrator,
                meta_task_id,
                Some("mutai-work".to_owned()),
                "mutAI work",
                100,
                "create-mutai-work",
            )
            .expect("valid legacy work"),
        )
        .expect("create legacy work");

    let legacy_candidates = covey
        .subtask_candidates(SessionRole::Executor, 10, None)
        .expect("legacy candidates");
    assert_eq!(legacy_candidates.len(), 1);
    assert_eq!(legacy_candidates[0].subtask_id, legacy_subtask_id);
    let hermes_routing_key = RoutingKey::parse("hermes").expect("valid Hermes routing key");
    let routed_candidates = covey
        .subtask_candidates_routed(SessionRole::Executor, &hermes_routing_key, 10, None)
        .expect("routed candidates");
    assert_eq!(routed_candidates.len(), 1);
    assert_eq!(routed_candidates[0].subtask_id, routed_subtask_id);
    assert_eq!(
        covey
            .claimable_subtask_availability(None)
            .expect("legacy availability")
            .executor_claimable_count(),
        1
    );
    assert_eq!(
        covey
            .claimable_subtask_availability_routed(&hermes_routing_key, None)
            .expect("routed availability")
            .executor_claimable_count(),
        1
    );

    let legacy_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(legacy_executor, 30_000, "legacy-claim-next")
                .expect("valid legacy claim-next"),
        )
        .expect("legacy claim-next succeeds")
        .expect("legacy mutAI work is claimable");
    assert_eq!(legacy_claim.subtask_id, legacy_subtask_id);

    let routed_claim = covey
        .claim_next_routed_subtask(
            ClaimNextRoutedReq::try_from_raw_parts(
                routed_executor,
                30_000,
                "hermes",
                None,
                "routed-claim-next",
            )
            .expect("valid routed claim-next"),
        )
        .expect("routed claim-next succeeds")
        .expect("Hermes work is claimable in its route");
    assert_eq!(routed_claim.subtask_id, routed_subtask_id);

    let conn = Connection::open(db_path).expect("open test database");
    let routed_facts = conn
        .query_row(
            "SELECT completion_policy, routing_key FROM subtasks WHERE subtask_id = ?1",
            [&routed_subtask_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("load routed facts");
    assert_eq!(routed_facts, ("direct".to_owned(), "hermes".to_owned()));
    let legacy_facts = conn
        .query_row(
            "SELECT completion_policy, routing_key FROM subtasks WHERE subtask_id = ?1",
            [&legacy_subtask_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("load legacy facts");
    assert_eq!(
        legacy_facts,
        ("canonical_apply".to_owned(), "mutai".to_owned())
    );
}

#[test]
fn routed_claim_next_is_executor_only() {
    let dir = TempDir::new().expect("temporary directory");
    let covey = Covey::open(dir.path().join("covey.db")).expect("open Covey");
    let reviewer = register(&covey, "reviewer", SessionRole::Reviewer);

    let error = covey
        .claim_next_routed_subtask(
            ClaimNextRoutedReq::try_from_raw_parts(
                reviewer,
                30_000,
                "hermes",
                None,
                "reviewer-routed-claim",
            )
            .expect("valid routed request"),
        )
        .expect_err("reviewers cannot claim executor routing lanes");
    assert!(matches!(error, covey::CoveyError::WrongRole { .. }));
}

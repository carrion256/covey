use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
    thread,
};

use covey::{
    ArtifactKind, AttemptOutcomeKind, ClaimNextRoutedReq, ClaimResult, ClaimSubtaskReq,
    CompletionPolicy, Covey, CoveyError, CreateWorkSubtaskReq, EnqueueForApplyReq, EventType,
    FailSubtaskReq, FinishSubtaskReq, ManualClock, MetaTaskState, PublishArtifactReq,
    RegisterSessionReq, RequestReviewReq, RetrySubtaskReq, RoutingKey, SessionRole,
    SettlementTarget, StartSubtaskReq, SubmitMetaTaskReq, SubtaskState,
};
use rusqlite::{Connection, OpenFlags, params};
use tempfile::TempDir;

const NOW_MS: i64 = 1_700_000_000_000;

struct Rig {
    _dir: TempDir,
    db_path: PathBuf,
    clock: Arc<ManualClock>,
    covey: Covey,
}

struct StartedWork {
    orchestrator: String,
    worker: String,
    meta_task_id: String,
    subtask_id: String,
    claim: ClaimResult,
}

impl Rig {
    fn new() -> Self {
        let dir = TempDir::new().expect("temporary directory");
        let db_path = dir.path().join("covey.db");
        let clock = Arc::new(ManualClock::new(NOW_MS));
        let covey = Covey::open_with_clock(&db_path, clock.clone()).expect("open Covey");
        Self {
            _dir: dir,
            db_path,
            clock,
            covey,
        }
    }

    fn tick(&self, delta_ms: i64) {
        self.clock.advance(delta_ms);
    }

    fn read_connection(&self) -> Connection {
        Connection::open_with_flags(&self.db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open read-only assertion connection")
    }
}

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
        .expect("register session")
        .session_token
        .to_string()
}

fn start_work(rig: &Rig, policy: CompletionPolicy, lease_ms: i64) -> StartedWork {
    let orchestrator = register(&rig.covey, "orchestrator", SessionRole::Orchestrator);
    let worker = register(&rig.covey, "worker", SessionRole::Executor);
    let meta_task_id = rig
        .covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orchestrator.clone(),
                "completion outcome test",
                "submit-meta",
            )
            .expect("valid meta-task request"),
        )
        .expect("submit meta task");
    let subtask_id = rig
        .covey
        .create_work_subtask(
            CreateWorkSubtaskReq::try_from_raw_parts(
                orchestrator.clone(),
                meta_task_id.clone(),
                Some("work".to_owned()),
                "execute queued work",
                1,
                policy,
                "hermes",
                "create-work",
            )
            .expect("valid work request"),
        )
        .expect("create work");
    let claim = rig
        .covey
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                lease_ms,
                "claim-work",
            )
            .expect("valid claim request"),
        )
        .expect("claim work");
    rig.covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "start-work",
            )
            .expect("valid start request"),
        )
        .expect("start work");
    StartedWork {
        orchestrator,
        worker,
        meta_task_id,
        subtask_id,
        claim,
    }
}

fn finish_request(work: &StartedWork, summary: &str, idempotency_key: &str) -> FinishSubtaskReq {
    FinishSubtaskReq::try_from_raw_parts(
        work.worker.clone(),
        work.claim.claim_id.clone(),
        work.claim.fence_seq,
        "blake3:success",
        summary,
        idempotency_key,
    )
    .expect("valid finish request")
}

fn add_direct_dependent(rig: &Rig, work: &StartedWork) -> String {
    let dependent = rig
        .covey
        .create_work_subtask(
            CreateWorkSubtaskReq::try_from_raw_parts(
                work.orchestrator.clone(),
                work.meta_task_id.clone(),
                Some("dependent".to_owned()),
                "work gated by direct outcome",
                1,
                CompletionPolicy::Direct,
                "hermes",
                "create-dependent",
            )
            .expect("valid dependent work request"),
        )
        .expect("create dependent work");
    let connection = Connection::open(&rig.db_path).expect("open assertion database for setup");
    connection
        .execute(
            r#"
            INSERT INTO subtask_dependencies (
                subtask_id, depends_on_subtask_id, source_ref, created_at
            ) VALUES (?1, ?2, ?3, ?4)
            "#,
            params![dependent, work.subtask_id, "direct-outcome", NOW_MS],
        )
        .expect("insert direct-work dependency");
    dependent
}

fn table_count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count table rows")
}

fn outcome_idempotency_count(connection: &Connection) -> i64 {
    connection
        .query_row(
            r#"
            SELECT COUNT(*) FROM mutation_idempotency
            WHERE operation IN ('finish_subtask', 'retry_subtask', 'fail_subtask')
            "#,
            [],
            |row| row.get(0),
        )
        .expect("count outcome idempotency rows")
}

fn outcome_event_count(covey: &Covey, event_type: EventType) -> usize {
    covey
        .fetch_events(0, 1_000)
        .expect("fetch events")
        .into_iter()
        .filter(|event| event.event_type() == event_type)
        .inspect(|event| {
            let _typed = event.typed().expect("outcome event must replay as typed");
        })
        .count()
}

fn assert_no_assurance_rows(rig: &Rig, subtask_id: &str) {
    let connection = rig.read_connection();
    for (table, column) in [
        ("artifacts", "produced_by_subtask_id"),
        ("reviews", "subtask_id"),
        ("ready_queue", "subtask_id"),
        ("subtask_attempt_outcomes", "subtask_id"),
    ] {
        let count: i64 = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1"),
                params![subtask_id],
                |row| row.get(0),
            )
            .expect("count subtask assurance rows");
        assert_eq!(count, 0, "unexpected {table} row");
    }
}

#[test]
fn direct_finish_records_one_receipt_and_completes_without_apply_queue() {
    let rig = Rig::new();
    let work = start_work(&rig, CompletionPolicy::Direct, 30_000);
    rig.tick(1);

    let outcome = rig
        .covey
        .finish_subtask(finish_request(&work, "research completed", "finish-work"))
        .expect("finish direct work");

    assert_eq!(outcome.claim_id, work.claim.claim_id);
    assert_eq!(outcome.subtask_id, work.subtask_id);
    assert_eq!(outcome.fence_seq, work.claim.fence_seq);
    assert_eq!(outcome.outcome_kind(), AttemptOutcomeKind::Succeeded);
    assert_eq!(outcome.evidence_digest.as_str(), "blake3:success");
    assert_eq!(outcome.summary.as_str(), "research completed");
    assert_eq!(outcome.failure_code(), None);

    let status = rig
        .covey
        .subtask_status(&work.subtask_id)
        .expect("completed subtask status");
    assert_eq!(status.subtask().state(), SubtaskState::Completed);
    assert_eq!(
        status.subtask().completion_policy(),
        CompletionPolicy::Direct
    );
    assert_eq!(status.subtask().routing_key().as_str(), "hermes");
    assert!(status.claim().is_none());
    assert!(status.artifact().is_none());
    assert!(status.reviews().is_empty());
    assert!(status.ready_queue().is_empty());
    assert_eq!(status.attempt_outcomes(), std::slice::from_ref(&outcome));
    assert!(
        rig.covey
            .ready_queue_candidates(10)
            .expect("queue")
            .is_empty()
    );
    assert_eq!(
        rig.covey
            .meta_task_status(&work.meta_task_id)
            .expect("meta-task status")
            .meta_task()
            .state(),
        MetaTaskState::Completed
    );

    let connection = rig.read_connection();
    assert_eq!(table_count(&connection, "subtask_attempt_outcomes"), 1);
    assert_eq!(table_count(&connection, "artifacts"), 0);
    assert_eq!(table_count(&connection, "reviews"), 0);
    assert_eq!(table_count(&connection, "ready_queue"), 0);
    let claim_state: String = connection
        .query_row(
            "SELECT state FROM claims WHERE claim_id = ?1",
            params![work.claim.claim_id.as_str()],
            |row| row.get(0),
        )
        .expect("read closed claim");
    assert_eq!(claim_state, "released");
    let active_subtask: Option<String> = connection
        .query_row(
            "SELECT active_subtask_id FROM sessions WHERE session_token = ?1",
            params![work.worker],
            |row| row.get(0),
        )
        .expect("read worker occupancy");
    assert_eq!(active_subtask, None);
    assert_eq!(
        outcome_event_count(&rig.covey, EventType::SubtaskFinished),
        1
    );
}

#[test]
fn direct_finish_replays_exactly_and_rejects_changed_or_new_idempotency_requests() {
    let rig = Rig::new();
    let work = start_work(&rig, CompletionPolicy::Direct, 30_000);
    let request = finish_request(&work, "stable result", "finish-stable");

    let first = rig
        .covey
        .finish_subtask(request.clone())
        .expect("first finish");
    let replay = rig
        .covey
        .finish_subtask(request)
        .expect("exact finish replay");
    assert_eq!(replay, first);

    let changed =
        rig.covey
            .finish_subtask(finish_request(&work, "changed result", "finish-stable"));
    assert!(matches!(
        changed,
        Err(CoveyError::IdempotencyConflict { .. })
    ));

    let new_key =
        rig.covey
            .finish_subtask(finish_request(&work, "stable result", "finish-new-key"));
    assert!(matches!(new_key, Err(CoveyError::ClaimNotHeld { .. })));

    let status = rig
        .covey
        .subtask_status(&work.subtask_id)
        .expect("status after replay attempts");
    assert_eq!(status.attempt_outcomes(), [first]);
    assert_eq!(
        outcome_event_count(&rig.covey, EventType::SubtaskFinished),
        1
    );
    let connection = rig.read_connection();
    assert_eq!(table_count(&connection, "subtask_attempt_outcomes"), 1);
}

#[test]
fn concurrent_competing_finishes_accept_exactly_one_receipt_and_event() {
    let rig = Rig::new();
    let work = start_work(&rig, CompletionPolicy::Direct, 30_000);
    let contender_a = Covey::open_with_clock(&rig.db_path, rig.clock.clone())
        .expect("open first independent Covey handle");
    let contender_b = Covey::open_with_clock(&rig.db_path, rig.clock.clone())
        .expect("open second independent Covey handle");
    let barrier = Arc::new(Barrier::new(2));

    let request_a = finish_request(&work, "race result a", "finish-race-a");
    let request_b = finish_request(&work, "race result b", "finish-race-b");
    let barrier_a = barrier.clone();
    let first = thread::spawn(move || {
        barrier_a.wait();
        contender_a.finish_subtask(request_a)
    });
    let second = thread::spawn(move || {
        barrier.wait();
        contender_b.finish_subtask(request_b)
    });

    let results = [
        first.join().expect("first contender did not panic"),
        second.join().expect("second contender did not panic"),
    ];
    let mut accepted = 0;
    let mut rejected = 0;
    for result in results {
        match result {
            Ok(_) => accepted += 1,
            Err(CoveyError::ClaimNotHeld { .. }) => rejected += 1,
            Err(error) => panic!("unexpected competing finish result: {error}"),
        }
    }
    assert_eq!(accepted, 1);
    assert_eq!(rejected, 1);

    let status = rig
        .covey
        .subtask_status(&work.subtask_id)
        .expect("status after competing finishes");
    assert_eq!(status.subtask().state(), SubtaskState::Completed);
    assert_eq!(status.attempt_outcomes().len(), 1);
    assert_eq!(
        outcome_event_count(&rig.covey, EventType::SubtaskFinished),
        1
    );
    let connection = rig.read_connection();
    assert_eq!(table_count(&connection, "subtask_attempt_outcomes"), 1);
    assert_eq!(outcome_idempotency_count(&connection), 1);
}

#[test]
fn retry_requeues_same_work_with_higher_fence_and_rejects_stale_attempts() {
    let rig = Rig::new();
    let work = start_work(&rig, CompletionPolicy::Direct, 30_000);

    let retry = rig
        .covey
        .retry_subtask(
            RetrySubtaskReq::try_from_raw_parts(
                work.worker.clone(),
                work.claim.claim_id.clone(),
                work.claim.fence_seq,
                "blake3:retry",
                "transient_provider_error",
                "provider unavailable",
                "retry-work",
            )
            .expect("valid retry request"),
        )
        .expect("record retryable failure");
    assert_eq!(retry.outcome_kind(), AttemptOutcomeKind::RetryableFailure);
    assert_eq!(
        retry.failure_code().map(AsRef::as_ref),
        Some("transient_provider_error")
    );
    let available = rig
        .covey
        .subtask_status(&work.subtask_id)
        .expect("status after retry");
    assert_eq!(available.subtask().state(), SubtaskState::Available);
    assert!(available.claim().is_none());
    assert_eq!(available.attempt_outcomes(), std::slice::from_ref(&retry));

    let second_claim = rig
        .covey
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                work.worker.clone(),
                work.subtask_id.clone(),
                30_000,
                "claim-work-second",
            )
            .expect("valid second claim"),
        )
        .expect("reclaim work");
    assert!(second_claim.fence_seq > work.claim.fence_seq);
    rig.covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                work.worker.clone(),
                second_claim.claim_id.clone(),
                second_claim.fence_seq,
                "start-work-second",
            )
            .expect("valid second start"),
        )
        .expect("start second attempt");

    let old_claim = rig.covey.finish_subtask(
        FinishSubtaskReq::try_from_raw_parts(
            work.worker.clone(),
            work.claim.claim_id.clone(),
            work.claim.fence_seq,
            "blake3:stale-old-claim",
            "stale old claim",
            "finish-stale-old-claim",
        )
        .expect("valid stale request"),
    );
    assert!(matches!(old_claim, Err(CoveyError::ClaimNotHeld { .. })));
    let stale_fence = rig.covey.finish_subtask(
        FinishSubtaskReq::try_from_raw_parts(
            work.worker.clone(),
            second_claim.claim_id.clone(),
            work.claim.fence_seq,
            "blake3:stale-fence",
            "stale fence",
            "finish-stale-fence",
        )
        .expect("valid stale-fence request"),
    );
    assert!(matches!(
        stale_fence,
        Err(CoveyError::StaleFenceToken { .. })
    ));

    let success = rig
        .covey
        .finish_subtask(
            FinishSubtaskReq::try_from_raw_parts(
                work.worker,
                second_claim.claim_id,
                second_claim.fence_seq,
                "blake3:second-success",
                "second attempt succeeded",
                "finish-second-attempt",
            )
            .expect("valid second finish"),
        )
        .expect("finish current attempt");
    let completed = rig
        .covey
        .subtask_status(&work.subtask_id)
        .expect("completed retry status");
    assert_eq!(completed.subtask().state(), SubtaskState::Completed);
    assert_eq!(completed.attempt_outcomes(), [retry, success]);
    assert_eq!(
        outcome_event_count(&rig.covey, EventType::SubtaskRetried),
        1
    );
    assert_eq!(
        outcome_event_count(&rig.covey, EventType::SubtaskFinished),
        1
    );
}

#[test]
fn terminal_failure_records_evidence_and_closes_work_without_queue_success() {
    let rig = Rig::new();
    let work = start_work(&rig, CompletionPolicy::Direct, 30_000);

    let failure = rig
        .covey
        .fail_subtask(
            FailSubtaskReq::try_from_raw_parts(
                work.worker.clone(),
                work.claim.claim_id.clone(),
                work.claim.fence_seq,
                "blake3:terminal-failure",
                "invalid_task_input",
                "task cannot be completed",
                "fail-work",
            )
            .expect("valid failure request"),
        )
        .expect("record terminal failure");
    assert_eq!(failure.outcome_kind(), AttemptOutcomeKind::TerminalFailure);
    assert_eq!(
        failure.failure_code().map(AsRef::as_ref),
        Some("invalid_task_input")
    );

    let status = rig
        .covey
        .subtask_status(&work.subtask_id)
        .expect("failed subtask status");
    assert_eq!(status.subtask().state(), SubtaskState::Failed);
    assert!(status.claim().is_none());
    assert!(status.ready_queue().is_empty());
    assert_eq!(status.attempt_outcomes(), [failure]);
    assert!(
        rig.covey
            .ready_queue_candidates(10)
            .expect("queue candidates")
            .is_empty()
    );
    assert_eq!(
        rig.covey
            .meta_task_status(&work.meta_task_id)
            .expect("meta status")
            .meta_task()
            .state(),
        MetaTaskState::Completed
    );
    assert_eq!(outcome_event_count(&rig.covey, EventType::SubtaskFailed), 1);
}

#[test]
fn only_successful_direct_dependencies_unblock_dependent_work() {
    let completed_rig = Rig::new();
    let completed_work = start_work(&completed_rig, CompletionPolicy::Direct, 30_000);
    let unblocked_dependent = add_direct_dependent(&completed_rig, &completed_work);
    let completed_outcome = completed_rig
        .covey
        .finish_subtask(finish_request(
            &completed_work,
            "dependency succeeded",
            "finish-dependency",
        ))
        .expect("finish direct dependency");
    assert_eq!(
        completed_outcome.outcome_kind(),
        AttemptOutcomeKind::Succeeded
    );
    let hermes_route = RoutingKey::parse("hermes").expect("valid Hermes route");
    let completed_candidates = completed_rig
        .covey
        .subtask_candidates_routed(
            SessionRole::Executor,
            &hermes_route,
            10,
            Some(&completed_work.meta_task_id),
        )
        .expect("list candidates after completed dependency");
    assert_eq!(completed_candidates.len(), 1);
    assert_eq!(completed_candidates[0].subtask_id, unblocked_dependent);
    assert_eq!(
        completed_rig
            .covey
            .claimable_subtask_availability_routed(
                &hermes_route,
                Some(&completed_work.meta_task_id),
            )
            .expect("read availability after completed dependency")
            .executor_claimable_count(),
        1
    );

    let unblocked = completed_rig
        .covey
        .claim_next_routed_subtask(
            ClaimNextRoutedReq::try_from_raw_parts(
                completed_work.worker,
                30_000,
                "hermes",
                Some(completed_work.meta_task_id),
                "claim-unblocked-dependent",
            )
            .expect("valid claim-next request"),
        )
        .expect("claim-next after completed dependency")
        .expect("completed dependency must unblock dependent work");
    assert_eq!(unblocked.subtask_id, unblocked_dependent);

    let failed_rig = Rig::new();
    let failed_work = start_work(&failed_rig, CompletionPolicy::Direct, 30_000);
    let blocked_dependent = add_direct_dependent(&failed_rig, &failed_work);
    let failed_outcome = failed_rig
        .covey
        .fail_subtask(
            FailSubtaskReq::try_from_raw_parts(
                failed_work.worker.clone(),
                failed_work.claim.claim_id.clone(),
                failed_work.claim.fence_seq,
                "blake3:failed-dependency",
                "terminal_dependency_failure",
                "dependency failed",
                "fail-dependency",
            )
            .expect("valid terminal dependency failure"),
        )
        .expect("fail direct dependency");
    assert_eq!(
        failed_outcome.outcome_kind(),
        AttemptOutcomeKind::TerminalFailure
    );
    assert!(
        failed_rig
            .covey
            .subtask_candidates_routed(
                SessionRole::Executor,
                &hermes_route,
                10,
                Some(&failed_work.meta_task_id),
            )
            .expect("list candidates after failed dependency")
            .is_empty()
    );
    assert_eq!(
        failed_rig
            .covey
            .claimable_subtask_availability_routed(&hermes_route, Some(&failed_work.meta_task_id),)
            .expect("read availability after failed dependency")
            .executor_claimable_count(),
        0
    );
    let targeted = failed_rig.covey.claim_subtask(
        ClaimSubtaskReq::try_from_raw_parts(
            failed_work.worker.clone(),
            blocked_dependent.clone(),
            30_000,
            "claim-blocked-dependent-directly",
        )
        .expect("valid targeted claim request"),
    );
    assert!(matches!(
        targeted,
        Err(CoveyError::IllegalTransition { .. })
    ));

    let blocked = failed_rig
        .covey
        .claim_next_routed_subtask(
            ClaimNextRoutedReq::try_from_raw_parts(
                failed_work.worker,
                30_000,
                "hermes",
                Some(failed_work.meta_task_id),
                "claim-blocked-dependent",
            )
            .expect("valid claim-next request"),
        )
        .expect("claim-next after failed dependency");
    assert!(blocked.is_none());
    assert_eq!(
        failed_rig
            .covey
            .subtask_status(&blocked_dependent)
            .expect("blocked dependent status")
            .subtask()
            .state(),
        SubtaskState::Available
    );
}

#[test]
fn canonical_policy_rejects_all_direct_outcomes_without_partial_writes() {
    let rig = Rig::new();
    let work = start_work(&rig, CompletionPolicy::CanonicalApply, 30_000);

    let finish = rig.covey.finish_subtask(finish_request(
        &work,
        "forbidden finish",
        "finish-canonical",
    ));
    let retry = rig.covey.retry_subtask(
        RetrySubtaskReq::try_from_raw_parts(
            work.worker.clone(),
            work.claim.claim_id.clone(),
            work.claim.fence_seq,
            "blake3:forbidden-retry",
            "transient",
            "forbidden retry",
            "retry-canonical",
        )
        .expect("valid retry request"),
    );
    let failure = rig.covey.fail_subtask(
        FailSubtaskReq::try_from_raw_parts(
            work.worker.clone(),
            work.claim.claim_id.clone(),
            work.claim.fence_seq,
            "blake3:forbidden-failure",
            "terminal",
            "forbidden failure",
            "fail-canonical",
        )
        .expect("valid failure request"),
    );
    for result in [finish.map(|_| ()), retry.map(|_| ()), failure.map(|_| ())] {
        assert!(matches!(
            result,
            Err(CoveyError::CompletionPolicyViolation {
                policy: CompletionPolicy::CanonicalApply,
                ..
            })
        ));
    }

    let status = rig
        .covey
        .subtask_status(&work.subtask_id)
        .expect("canonical status after rejected outcomes");
    assert_eq!(status.subtask().state(), SubtaskState::InProgress);
    assert_eq!(
        status.claim().map(|claim| &claim.claim_id),
        Some(&work.claim.claim_id)
    );
    assert!(status.attempt_outcomes().is_empty());
    let connection = rig.read_connection();
    assert_eq!(table_count(&connection, "subtask_attempt_outcomes"), 0);
    assert_eq!(outcome_idempotency_count(&connection), 0);
    assert_eq!(
        outcome_event_count(&rig.covey, EventType::SubtaskFinished),
        0
    );
    assert_eq!(
        outcome_event_count(&rig.covey, EventType::SubtaskRetried),
        0
    );
    assert_eq!(outcome_event_count(&rig.covey, EventType::SubtaskFailed), 0);
}

#[test]
fn direct_policy_cannot_publish_request_review_or_enter_apply_queue() {
    let rig = Rig::new();
    let work = start_work(&rig, CompletionPolicy::Direct, 30_000);

    let publish = rig.covey.publish_artifact(
        PublishArtifactReq::try_from_raw_parts(
            work.worker.clone(),
            work.claim.claim_id.clone(),
            work.claim.fence_seq,
            "blake3:forbidden-artifact".to_owned(),
            ArtifactKind::PatchBundle,
            "base".to_owned(),
            "forbidden.json".to_owned(),
            "blake3:forbidden-paths".to_owned(),
            "publish-direct-artifact",
        )
        .expect("valid artifact request"),
    );
    assert!(matches!(
        publish,
        Err(CoveyError::CompletionPolicyViolation {
            policy: CompletionPolicy::Direct,
            ..
        })
    ));

    let review = rig.covey.request_review(
        RequestReviewReq::try_from_raw_parts(
            work.worker.clone(),
            work.subtask_id.clone(),
            "blake3:forbidden-artifact",
            Some("forbidden-review".to_owned()),
            1,
            "review-direct-artifact",
        )
        .expect("valid review request"),
    );
    assert!(matches!(
        review,
        Err(CoveyError::CompletionPolicyViolation {
            policy: CompletionPolicy::Direct,
            ..
        })
    ));

    let enqueue = rig.covey.enqueue_for_apply(
        EnqueueForApplyReq::try_from_raw_parts(
            work.orchestrator.clone(),
            "blake3:forbidden-artifact".to_owned(),
            work.subtask_id.clone(),
            SettlementTarget::Canonical,
            "enqueue-direct-artifact",
        )
        .expect("valid enqueue request"),
    );
    assert!(matches!(
        enqueue,
        Err(CoveyError::CompletionPolicyViolation {
            policy: CompletionPolicy::Direct,
            ..
        })
    ));

    let status = rig
        .covey
        .subtask_status(&work.subtask_id)
        .expect("direct status after bypass attempts");
    assert_eq!(status.subtask().state(), SubtaskState::InProgress);
    assert_eq!(
        status.claim().map(|claim| &claim.claim_id),
        Some(&work.claim.claim_id)
    );
    assert_no_assurance_rows(&rig, &work.subtask_id);
    let connection = rig.read_connection();
    let policy_events: i64 = connection
        .query_row(
            r#"
            SELECT COUNT(*) FROM event_log
            WHERE event_type IN ('artifact_published', 'review_requested', 'ready_queue_enqueued')
            "#,
            [],
            |row| row.get(0),
        )
        .expect("count rejected-policy events");
    assert_eq!(policy_events, 0);
}

#[test]
fn direct_finish_rejects_wrong_owner_and_expired_lease_without_receipt() {
    let rig = Rig::new();
    let work = start_work(&rig, CompletionPolicy::Direct, 10);
    let other = register(&rig.covey, "other-worker", SessionRole::Executor);

    let wrong_owner = rig.covey.finish_subtask(
        FinishSubtaskReq::try_from_raw_parts(
            other,
            work.claim.claim_id.clone(),
            work.claim.fence_seq,
            "blake3:wrong-owner",
            "wrong owner",
            "finish-wrong-owner",
        )
        .expect("valid wrong-owner request"),
    );
    assert!(matches!(wrong_owner, Err(CoveyError::NotClaimOwner { .. })));

    rig.tick(10);
    let expired = rig
        .covey
        .finish_subtask(finish_request(&work, "late result", "finish-expired"));
    assert!(matches!(expired, Err(CoveyError::LeaseExpired { .. })));

    let status = rig
        .covey
        .subtask_status(&work.subtask_id)
        .expect("status after rejected finishes");
    assert_eq!(status.subtask().state(), SubtaskState::InProgress);
    assert_eq!(
        status.claim().map(|claim| &claim.claim_id),
        Some(&work.claim.claim_id)
    );
    assert!(status.attempt_outcomes().is_empty());
    let connection = rig.read_connection();
    assert_eq!(table_count(&connection, "subtask_attempt_outcomes"), 0);
    assert_eq!(outcome_idempotency_count(&connection), 0);
    assert_eq!(
        outcome_event_count(&rig.covey, EventType::SubtaskFinished),
        0
    );
}

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

mod support;

use covey::{
    ArtifactKind, BeginOpenSpecArchiveCleanupReq, ClaimNextReq, Covey, CreateSubtaskRequest,
    DecideReviewReq, EnqueueForApplyReq, FinishOpenSpecArchiveCleanupReq, HeartbeatReq,
    IdempotencyKey, ManualClock, MarkAppliedReq, MarkInFlightReq, OpenSpecCurrentWorkBlockerKind,
    OpenSpecCurrentWorkState, PublishArtifactReq, ReconcileApplyQueueReq,
    RecordApplyVerificationReq, RecordRuntimeAttestationReq, RegisterSessionReq, ReleaseClaimReq,
    RequestReservationReq, RequestReviewReq, ReviewVerdict, ScopeClass, SessionRole, SessionState,
    SettlementTarget, StartSubtaskReq, SubmitMetaTaskReq, SubtaskState, SubtaskTitle,
};
use rusqlite::params;
use tempfile::TempDir;

static NEXT_IDEMPOTENCY_KEY: AtomicUsize = AtomicUsize::new(1);

struct Rig {
    _dir: TempDir,
    db_path: std::path::PathBuf,
    clock: Arc<ManualClock>,
}

impl Rig {
    fn new() -> Self {
        support::enable_info_logging();
        let dir = TempDir::new().expect("tempdir");
        Self {
            db_path: dir.path().join("covey.db"),
            _dir: dir,
            clock: Arc::new(ManualClock::new(1_700_000_000_000)),
        }
    }

    fn covey(&self) -> Covey {
        Covey::open_with_clock(&self.db_path, self.clock.clone()).expect("open covey")
    }

    fn tick(&self, delta_ms: i64) {
        self.clock.advance(delta_ms);
    }
}

fn id_key(label: &str) -> IdempotencyKey {
    IdempotencyKey::parse(format!(
        "{label}-{}",
        NEXT_IDEMPOTENCY_KEY.fetch_add(1, Ordering::Relaxed)
    ))
    .expect("valid idempotency key")
}

fn register(covey: &Covey, principal: &str, role: SessionRole) -> String {
    covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                principal,
                format!("{principal}-instance"),
                role,
                id_key("register-session"),
            )
            .expect("valid session registration request"),
        )
        .expect("register session")
        .session_token
        .to_string()
}

fn approve_work_artifact(
    rig: &Rig,
    artifact_kind: ArtifactKind,
    subtask_suffix: &str,
    openspec_change_id: Option<&str>,
) -> (String, Covey) {
    let covey = rig.covey();
    let orchestrator = register(
        &covey,
        &format!("orchestrator-{subtask_suffix}"),
        SessionRole::Orchestrator,
    );
    let worker = register(
        &covey,
        &format!("worker-{subtask_suffix}"),
        SessionRole::Executor,
    );
    let reviewer = register(
        &covey,
        &format!("reviewer-{subtask_suffix}"),
        SessionRole::Reviewer,
    );
    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orchestrator.clone(),
                format!("{subtask_suffix} approval proof"),
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");
    let subtask_id = covey
        .create_subtask(
            CreateSubtaskRequest::try_from_raw_parts(
                orchestrator.clone(),
                meta_task_id,
                Some(format!("{subtask_suffix}_work")),
                format!("{subtask_suffix} work"),
                1,
                id_key("create-subtask"),
            )
            .expect("valid create-subtask request"),
        )
        .expect("create subtask");
    if let Some(change_id) = openspec_change_id {
        let conn = rusqlite::Connection::open(&rig.db_path).expect("open test db");
        conn.execute(
            "INSERT OR IGNORE INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at, updated_at)
             VALUES (?1, ?2, 'active', ?3, ?4, ?4)",
            params![
                format!("openspec:{change_id}"),
                format!("OpenSpec {change_id}"),
                orchestrator,
                1_700_000_000_000_i64,
            ],
        )
        .expect("insert openspec meta task");
        conn.execute(
            "INSERT INTO openspec_subtask_scope (subtask_id, openspec_change_id, openspec_task_id, source_path, scenario_refs_json, updated_at)
             VALUES (?1, ?2, '1.1', 'openspec/changes/test/tasks.md', '[]', ?3)",
            params![subtask_id, change_id, 1_700_000_000_001_i64],
        )
        .expect("insert openspec scope");
    }
    let work_claim = covey
        .claim_subtask(
            covey::ClaimSubtaskReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                30_000,
                id_key("claim-work"),
            )
            .expect("valid claim-subtask request"),
        )
        .expect("claim work");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                id_key("start-work"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start work");
    let artifact_digest = format!("blake3:{subtask_suffix}_artifact");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                artifact_digest.clone(),
                artifact_kind,
                "base".into(),
                format!("{subtask_suffix}.json"),
                format!("blake3:{subtask_suffix}_paths"),
                id_key("publish-artifact"),
            )
            .expect("valid publish-artifact request"),
        )
        .expect("publish artifact");
    let review_subtask_id = format!("{subtask_suffix}_review");
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                artifact_digest,
                Some(review_subtask_id.clone()),
                1,
                id_key("request-review"),
            )
            .expect("valid request-review request"),
        )
        .expect("request review");
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                worker,
                work_claim.claim_id,
                work_claim.fence_seq,
                id_key("release-work"),
            )
            .expect("valid release-claim request"),
        )
        .expect("release work");
    let review_claim = covey
        .claim_subtask(
            covey::ClaimSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                review_subtask_id.clone(),
                30_000,
                id_key("claim-review"),
            )
            .expect("valid claim-subtask request"),
        )
        .expect("claim review");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                review_claim.claim_id.clone(),
                review_claim.fence_seq,
                id_key("start-review"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start review");
    covey
        .decide_review(
            DecideReviewReq::try_from_raw_parts(
                reviewer,
                review_id,
                review_claim.claim_id,
                review_claim.fence_seq,
                ReviewVerdict::Approve,
                format!("blake3:{subtask_suffix}_findings"),
                id_key("decide-review"),
            )
            .expect("valid decide-review request"),
        )
        .expect("approve review");
    (subtask_id, covey)
}

#[test]
fn expired_review_claim_can_be_reclaimed_started_and_decided() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orchestrator = register(
        &covey,
        "orchestrator-review-expire",
        SessionRole::Orchestrator,
    );
    let worker = register(&covey, "worker-review-expire", SessionRole::Executor);
    let reviewer = register(&covey, "reviewer-review-expire", SessionRole::Reviewer);

    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orchestrator.clone(),
                "review claim expiry proof",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");
    let subtask_id = covey
        .create_subtask(
            CreateSubtaskRequest::try_from_raw_parts(
                orchestrator,
                meta_task_id,
                Some("review_expiry_work".into()),
                "review expiry work",
                1,
                id_key("create-subtask"),
            )
            .expect("valid create-subtask request"),
        )
        .expect("create subtask");

    let work_claim = covey
        .claim_subtask(
            covey::ClaimSubtaskReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                30_000,
                id_key("claim-work"),
            )
            .expect("valid claim-subtask request"),
        )
        .expect("claim work");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                id_key("start-work"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start work");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                "blake3:review_expiry_artifact".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "review-expiry-artifact.json".into(),
                "blake3:review_expiry_paths".into(),
                id_key("publish-artifact"),
            )
            .expect("valid publish-artifact request"),
        )
        .expect("publish artifact");
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                "blake3:review_expiry_artifact",
                Some("review_expiry_review".into()),
                1,
                id_key("request-review"),
            )
            .expect("valid request-review request"),
        )
        .expect("request review");
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                worker,
                work_claim.claim_id,
                work_claim.fence_seq,
                id_key("release-work"),
            )
            .expect("valid release-claim request"),
        )
        .expect("release work");

    let first_review_claim = covey
        .claim_subtask(
            covey::ClaimSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                "review_expiry_review",
                10_000,
                id_key("claim-review-first"),
            )
            .expect("valid claim-subtask request"),
        )
        .expect("claim review first");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                first_review_claim.claim_id,
                first_review_claim.fence_seq,
                id_key("start-review-first"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start first review");

    rig.tick(60_000);
    assert_eq!(
        covey
            .expire_old_claims()
            .expect("expire claims")
            .expired_count,
        1
    );

    let second_review_claim = covey
        .claim_subtask(
            covey::ClaimSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                "review_expiry_review",
                30_000,
                id_key("claim-review-second"),
            )
            .expect("valid claim-subtask request"),
        )
        .expect("claim review second");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                second_review_claim.claim_id.clone(),
                second_review_claim.fence_seq,
                id_key("start-review-second"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start second review after expiry");
    covey
        .decide_review(
            DecideReviewReq::try_from_raw_parts(
                reviewer,
                review_id,
                second_review_claim.claim_id,
                second_review_claim.fence_seq,
                covey::ReviewVerdict::Approve,
                "blake3:review_expiry_findings".into(),
                id_key("decide-review"),
            )
            .expect("valid decide-review request"),
        )
        .expect("decide review after reclaim");

    assert_eq!(
        covey
            .subtask_status("review_expiry_review")
            .expect("review status")
            .subtask()
            .state(),
        SubtaskState::Decided
    );
    assert_eq!(
        covey
            .subtask_status(&subtask_id)
            .expect("work status")
            .subtask()
            .state(),
        SubtaskState::ReadyForApply
    );
    let candidates = covey
        .ready_queue_candidates(10)
        .expect("approval enqueues apply candidate");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].subtask_id.as_str(), subtask_id);
}

#[test]
fn approving_findings_bundle_marks_work_applied_without_apply_queue() {
    let rig = Rig::new();
    let (subtask_id, covey) = approve_work_artifact(
        &rig,
        ArtifactKind::FindingsBundle,
        "findings_no_apply",
        None,
    );

    assert_eq!(
        covey
            .subtask_status(&subtask_id)
            .expect("findings work status")
            .subtask()
            .state(),
        SubtaskState::Applied
    );
    assert!(
        covey
            .ready_queue_candidates(10)
            .expect("queue candidates")
            .is_empty()
    );
}

#[test]
fn approving_verification_bundle_marks_work_applied_without_apply_queue() {
    let rig = Rig::new();
    let (subtask_id, covey) = approve_work_artifact(
        &rig,
        ArtifactKind::VerificationBundle,
        "verification_no_apply",
        None,
    );

    assert_eq!(
        covey
            .subtask_status(&subtask_id)
            .expect("verification work status")
            .subtask()
            .state(),
        SubtaskState::Applied
    );
    assert!(
        covey
            .ready_queue_candidates(10)
            .expect("queue candidates")
            .is_empty()
    );
}

#[test]
fn direct_applied_openspec_artifact_blocks_until_archive_cleanup_finishes() {
    let rig = Rig::new();
    let change_id = "direct-no-apply-change";
    let (_subtask_id, covey) = approve_work_artifact(
        &rig,
        ArtifactKind::VerificationBundle,
        "direct_no_apply_archive",
        Some(change_id),
    );

    let current = covey
        .openspec_current_work(change_id)
        .expect("current work after direct applied approval");
    assert_eq!(current.state, OpenSpecCurrentWorkState::Blocked);
    assert_eq!(current.archive_blockers.len(), 1);
    assert_eq!(
        current.blockers[0].kind,
        OpenSpecCurrentWorkBlockerKind::AppliedButUnarchived
    );
    assert!(
        current.queue_ids.is_empty(),
        "direct-applied artifact must not create ready queue rows"
    );

    let orchestrator = register(
        &covey,
        "orchestrator-direct-archive",
        SessionRole::Orchestrator,
    );
    let cleanup = covey
        .begin_openspec_archive_cleanup(
            BeginOpenSpecArchiveCleanupReq::try_from_raw_parts(
                orchestrator.clone(),
                change_id,
                vec!["openspec/changes/direct-no-apply-change".to_owned()],
                id_key("begin-archive"),
            )
            .expect("valid begin archive cleanup"),
        )
        .expect("begin archive cleanup");
    let _finish = covey
        .finish_openspec_archive_cleanup(
            FinishOpenSpecArchiveCleanupReq::try_from_raw_parts(
                orchestrator,
                change_id,
                cleanup.cleanup_claim_id.to_string(),
                cleanup.fence_seq.get(),
                "blake3:direct_no_apply_archive_proof",
                id_key("finish-archive"),
            )
            .expect("valid finish archive cleanup"),
        )
        .expect("finish archive cleanup");

    let archived = covey
        .openspec_current_work(change_id)
        .expect("current work after archive cleanup");
    assert_eq!(archived.state, OpenSpecCurrentWorkState::Archived);
    assert!(archived.blockers.is_empty());
}

#[test]
fn reconcile_apply_queue_materializes_historical_approved_work() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orchestrator = register(&covey, "orchestrator-reconcile", SessionRole::Orchestrator);
    let worker = register(&covey, "worker-reconcile", SessionRole::Executor);
    let reviewer = register(&covey, "reviewer-reconcile", SessionRole::Reviewer);

    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orchestrator.clone(),
                "historical approved reconcile proof",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");
    let subtask_id = covey
        .create_subtask(
            CreateSubtaskRequest::try_from_raw_parts(
                orchestrator.clone(),
                meta_task_id.clone(),
                Some("historical_approved_work".into()),
                "historical approved work",
                1,
                id_key("create-subtask"),
            )
            .expect("valid create-subtask request"),
        )
        .expect("create subtask");

    let conn = rusqlite::Connection::open(&rig.db_path).expect("open test db");
    conn.execute(
        "INSERT INTO artifacts (
            artifact_digest, artifact_kind, base_rev, produced_by_subtask_id,
            produced_by_session, manifest_path, changed_paths_digest, created_at
        ) VALUES (?1, 'patch_bundle', 'base', ?2, ?3, 'historical-approved.json', ?4, ?5)",
        params![
            "blake3:historical_approved_artifact",
            subtask_id,
            worker,
            "blake3:historical_approved_paths",
            1_700_000_000_001_i64,
        ],
    )
    .expect("insert historical artifact");
    conn.execute(
        "UPDATE subtasks SET state = 'approved', artifact_digest = ?2, updated_at = ?3 WHERE subtask_id = ?1",
        params![
            subtask_id,
            "blake3:historical_approved_artifact",
            1_700_000_000_002_i64,
        ],
    )
    .expect("set historical approved state");
    conn.execute(
        "INSERT INTO subtasks (
            subtask_id, meta_task_id, title, kind, review_target_subtask_id,
            review_target_artifact_digest, state, current_claim_id, artifact_digest,
            priority, created_at, updated_at
        ) VALUES ('review_historical_approved_subtask', ?1, 'historical approved review', 'review', ?2, ?3, 'decided', NULL, NULL, 1, ?4, ?4)",
        params![
            meta_task_id,
            &subtask_id,
            "blake3:historical_approved_artifact",
            1_700_000_000_003_i64,
        ],
    )
    .expect("insert historical review subtask");
    conn.execute(
        "INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq) VALUES ('review_historical_approved_subtask', 1)",
        [],
    )
    .expect("insert historical review fence counter");
    conn.execute(
        "INSERT INTO reviews (
            review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id,
            verdict, findings_digest, state, created_at, updated_at
        ) VALUES ('review_historical_approved', ?1, ?2, ?3, 'review_historical_approved_subtask', 'approve', ?4, 'decided', ?5, ?5)",
        params![
            subtask_id,
            "blake3:historical_approved_artifact",
            reviewer,
            "blake3:historical_approved_findings",
            1_700_000_000_003_i64,
        ],
    )
    .expect("insert historical approved review");
    drop(conn);

    let result = covey
        .reconcile_apply_queue(
            ReconcileApplyQueueReq::try_from_raw_parts(
                orchestrator,
                id_key("reconcile-apply-queue"),
            )
            .expect("valid reconcile request"),
        )
        .expect("reconcile apply queue");
    assert_eq!(result.approved_enqueued_count, 1);
    assert_eq!(result.ready_for_apply_enqueued_count, 0);

    let status = covey
        .subtask_status(&subtask_id)
        .expect("historical approved status");
    assert_eq!(status.subtask().state(), SubtaskState::ReadyForApply);
    let candidates = covey
        .ready_queue_candidates(10)
        .expect("reconciled apply candidate");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].subtask_id.as_str(), subtask_id);
}

#[test]
fn reconcile_apply_queue_skips_historical_approved_findings_bundle() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orchestrator = register(
        &covey,
        "orchestrator-reconcile-findings",
        SessionRole::Orchestrator,
    );
    let worker = register(&covey, "worker-reconcile-findings", SessionRole::Executor);
    let reviewer = register(&covey, "reviewer-reconcile-findings", SessionRole::Reviewer);

    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orchestrator.clone(),
                "historical findings reconcile proof",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");
    let subtask_id = covey
        .create_subtask(
            CreateSubtaskRequest::try_from_raw_parts(
                orchestrator.clone(),
                meta_task_id.clone(),
                Some("historical_findings_work".into()),
                "historical findings work",
                1,
                id_key("create-subtask"),
            )
            .expect("valid create-subtask request"),
        )
        .expect("create subtask");

    let conn = rusqlite::Connection::open(&rig.db_path).expect("open test db");
    conn.execute(
        "INSERT INTO artifacts (
            artifact_digest, artifact_kind, base_rev, produced_by_subtask_id,
            produced_by_session, manifest_path, changed_paths_digest, created_at
        ) VALUES (?1, 'findings_bundle', 'base', ?2, ?3, 'historical-findings.json', ?4, ?5)",
        params![
            "blake3:historical_findings_artifact",
            subtask_id,
            worker,
            "blake3:historical_findings_paths",
            1_700_000_000_001_i64,
        ],
    )
    .expect("insert historical findings artifact");
    conn.execute(
        "UPDATE subtasks SET state = 'approved', artifact_digest = ?2, updated_at = ?3 WHERE subtask_id = ?1",
        params![
            subtask_id,
            "blake3:historical_findings_artifact",
            1_700_000_000_002_i64,
        ],
    )
    .expect("set historical approved state");
    conn.execute(
        "INSERT INTO subtasks (
            subtask_id, meta_task_id, title, kind, review_target_subtask_id,
            review_target_artifact_digest, state, current_claim_id, artifact_digest,
            priority, created_at, updated_at
        ) VALUES ('review_historical_findings_subtask', ?1, 'historical findings review', 'review', ?2, ?3, 'decided', NULL, NULL, 1, ?4, ?4)",
        params![
            meta_task_id,
            &subtask_id,
            "blake3:historical_findings_artifact",
            1_700_000_000_003_i64,
        ],
    )
    .expect("insert historical findings review subtask");
    conn.execute(
        "INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq) VALUES ('review_historical_findings_subtask', 1)",
        [],
    )
    .expect("insert historical findings review fence counter");
    conn.execute(
        "INSERT INTO reviews (
            review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id,
            verdict, findings_digest, state, created_at, updated_at
        ) VALUES ('review_historical_findings', ?1, ?2, ?3, 'review_historical_findings_subtask', 'approve', ?4, 'decided', ?5, ?5)",
        params![
            subtask_id,
            "blake3:historical_findings_artifact",
            reviewer,
            "blake3:historical_findings_digest",
            1_700_000_000_003_i64,
        ],
    )
    .expect("insert historical findings review");
    drop(conn);

    let result = covey
        .reconcile_apply_queue(
            ReconcileApplyQueueReq::try_from_raw_parts(
                orchestrator.clone(),
                id_key("reconcile-apply-queue"),
            )
            .expect("valid reconcile request"),
        )
        .expect("reconcile apply queue");
    assert_eq!(result.enqueued_count(), 0);

    let status = covey
        .subtask_status(&subtask_id)
        .expect("historical findings status");
    assert_eq!(status.subtask().state(), SubtaskState::Approved);
    assert!(
        covey
            .ready_queue_candidates(10)
            .expect("queue candidates")
            .is_empty()
    );
    covey
        .enqueue_for_apply(
            EnqueueForApplyReq::try_from_raw_parts(
                orchestrator,
                "blake3:historical_findings_artifact".to_owned(),
                subtask_id,
                SettlementTarget::Canonical,
                id_key("enqueue-findings"),
            )
            .expect("valid enqueue request"),
        )
        .expect_err("findings bundles are not applyable artifacts");
}

#[test]
fn reconcile_apply_queue_skips_historical_approved_verification_bundle() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orchestrator = register(
        &covey,
        "orchestrator-reconcile-verification",
        SessionRole::Orchestrator,
    );
    let worker = register(
        &covey,
        "worker-reconcile-verification",
        SessionRole::Executor,
    );
    let reviewer = register(
        &covey,
        "reviewer-reconcile-verification",
        SessionRole::Reviewer,
    );
    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orchestrator.clone(),
                "historical verification reconcile proof",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");
    let subtask_id = covey
        .create_subtask(
            CreateSubtaskRequest::try_from_raw_parts(
                orchestrator.clone(),
                meta_task_id.clone(),
                Some("historical_verification_work".into()),
                "historical verification work",
                1,
                id_key("create-subtask"),
            )
            .expect("valid create-subtask request"),
        )
        .expect("create subtask");

    let conn = rusqlite::Connection::open(&rig.db_path).expect("open test db");
    conn.execute(
        "INSERT INTO artifacts (
            artifact_digest, artifact_kind, base_rev, produced_by_subtask_id,
            produced_by_session, manifest_path, changed_paths_digest, created_at
        ) VALUES (?1, 'verification_bundle', 'base', ?2, ?3, 'historical-verification.json', ?4, ?5)",
        params![
            "blake3:historical_verification_artifact",
            subtask_id,
            worker,
            "blake3:historical_verification_paths",
            1_700_000_000_001_i64,
        ],
    )
    .expect("insert historical verification artifact");
    conn.execute(
        "UPDATE subtasks SET state = 'approved', artifact_digest = ?2, updated_at = ?3 WHERE subtask_id = ?1",
        params![
            subtask_id,
            "blake3:historical_verification_artifact",
            1_700_000_000_002_i64,
        ],
    )
    .expect("set historical approved state");
    conn.execute(
        "INSERT INTO subtasks (
            subtask_id, meta_task_id, title, kind, review_target_subtask_id,
            review_target_artifact_digest, state, current_claim_id, artifact_digest,
            priority, created_at, updated_at
        ) VALUES ('review_historical_verification_subtask', ?1, 'historical verification review', 'review', ?2, ?3, 'decided', NULL, NULL, 1, ?4, ?4)",
        params![
            meta_task_id,
            &subtask_id,
            "blake3:historical_verification_artifact",
            1_700_000_000_003_i64,
        ],
    )
    .expect("insert historical verification review subtask");
    conn.execute(
        "INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq) VALUES ('review_historical_verification_subtask', 1)",
        [],
    )
    .expect("insert historical verification review fence counter");
    conn.execute(
        "INSERT INTO reviews (
            review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id,
            verdict, findings_digest, state, created_at, updated_at
        ) VALUES ('review_historical_verification', ?1, ?2, ?3, 'review_historical_verification_subtask', 'approve', ?4, 'decided', ?5, ?5)",
        params![
            subtask_id,
            "blake3:historical_verification_artifact",
            reviewer,
            "blake3:historical_verification_digest",
            1_700_000_000_003_i64,
        ],
    )
    .expect("insert historical verification review");
    drop(conn);

    let result = covey
        .reconcile_apply_queue(
            ReconcileApplyQueueReq::try_from_raw_parts(
                orchestrator.clone(),
                id_key("reconcile-apply-queue"),
            )
            .expect("valid reconcile request"),
        )
        .expect("reconcile apply queue");
    assert_eq!(result.enqueued_count(), 0);
    assert!(
        covey
            .ready_queue_candidates(10)
            .expect("queue candidates")
            .is_empty()
    );
    covey
        .enqueue_for_apply(
            EnqueueForApplyReq::try_from_raw_parts(
                orchestrator,
                "blake3:historical_verification_artifact".to_owned(),
                subtask_id,
                SettlementTarget::Canonical,
                id_key("enqueue-verification"),
            )
            .expect("valid enqueue request"),
        )
        .expect_err("verification bundles are not applyable artifacts");
}

#[test]
fn claim_next_skips_work_with_unsatisfied_dependencies() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orchestrator = register(&covey, "orchestrator-deps", SessionRole::Orchestrator);
    let worker = register(&covey, "worker-deps", SessionRole::Executor);

    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orchestrator.clone(),
                "dependency proof",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");
    let prerequisite = covey
        .create_subtask(
            CreateSubtaskRequest::try_from_raw_parts(
                orchestrator.clone(),
                meta_task_id.clone(),
                Some("dependency_prerequisite".into()),
                "dependency prerequisite",
                100,
                id_key("create-prerequisite"),
            )
            .expect("valid create-subtask request"),
        )
        .expect("create prerequisite");
    let dependent = covey
        .create_subtask(
            CreateSubtaskRequest::try_from_raw_parts(
                orchestrator,
                meta_task_id,
                Some("dependency_dependent".into()),
                "dependency dependent",
                1,
                id_key("create-dependent"),
            )
            .expect("valid create-subtask request"),
        )
        .expect("create dependent");

    let conn = rusqlite::Connection::open(&rig.db_path).expect("open test db");
    conn.execute(
        "INSERT INTO subtask_dependencies (subtask_id, depends_on_subtask_id, source_ref, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![dependent, prerequisite, "1.1", 1_700_000_000_000_i64],
    )
    .expect("insert dependency");
    drop(conn);

    let claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(worker, 30_000, id_key("claim-next"))
                .expect("valid claim-next request"),
        )
        .expect("claim next")
        .expect("claimable prerequisite");
    assert_eq!(claim.subtask_id, "dependency_prerequisite");
}

fn attest(covey: &Covey, session_token: &str) {
    covey
        .record_runtime_attestation(
            RecordRuntimeAttestationReq::try_from_parts(
                session_token.to_owned(),
                "covey-test",
                "test-model",
                format!("provider-run-{session_token}"),
                "covey-test-provider",
                Some(format!("pid-{session_token}")),
                None,
                format!("blake3:{session_token}-transcript"),
                1_700_000_000_000,
                1_700_000_000_000,
                format!("record-runtime-attestation-{session_token}"),
            )
            .expect("valid runtime attestation request"),
        )
        .expect("record runtime attestation");
}

#[test]
fn disappeared_codex_session_is_reaped_and_work_is_reclaimed() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orchestrator = register(&covey, "orchestrator-reap", SessionRole::Orchestrator);
    let disappeared_codex = register(&covey, "codex-disappeared", SessionRole::Executor);

    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orchestrator.clone(),
                "codex disappears proof",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");
    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orchestrator.clone())
                .expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id).expect("valid meta-task id"),
            subtask_id: Some(
                covey::SubtaskId::parse("codex_disappears_work").expect("valid subtask id"),
            ),
            title: SubtaskTitle::parse("codex disappears work").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create subtask");

    let stale_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                disappeared_codex.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next-disappeared-codex"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim by disappeared codex")
        .expect("claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                disappeared_codex.clone(),
                stale_claim.claim_id.clone(),
                stale_claim.fence_seq,
                id_key("start-disappeared-codex"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("disappeared codex starts");

    rig.tick(60_000);
    covey
        .heartbeat(
            HeartbeatReq::try_from_raw_parts(
                orchestrator,
                id_key("heartbeat-orchestrator-before-reap"),
            )
            .expect("valid heartbeat request"),
        )
        .expect("orchestrator remains alive");
    assert_eq!(
        covey
            .reap_stale_sessions(45_000)
            .expect("reap disappeared Codex session")
            .stale_sessions,
        1
    );
    let stale_session = covey
        .session_status(&disappeared_codex)
        .expect("stale session status");
    assert_eq!(stale_session.session().state(), SessionState::Stale);
    assert!(stale_session.session().active_subtask_id().is_none());

    let resumed_worker = register(&covey, "worker-after-disappear", SessionRole::Executor);
    let resumed_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                resumed_worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-after-disappeared-codex"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim after disappeared Codex")
        .expect("resumed claim result");
    assert_eq!(resumed_claim.subtask_id, subtask_id);
    assert!(resumed_claim.fence_seq > stale_claim.fence_seq);
    assert_eq!(
        covey
            .subtask_status(&subtask_id)
            .expect("reclaimed subtask")
            .claim()
            .expect("active resumed claim")
            .claim_id,
        resumed_claim.claim_id
    );
}

#[test]
fn unattended_claim_recovery_apply_gate_and_duplicate_completion_are_bounded() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orchestrator = register(&covey, "orchestrator", SessionRole::Orchestrator);
    let dead_worker = register(&covey, "worker-dead", SessionRole::Executor);
    let resumed_worker = register(&covey, "worker-resumed", SessionRole::Executor);
    let reviewer = register(&covey, "reviewer", SessionRole::Reviewer);
    let apply_gate = register(&covey, "apply-gate", SessionRole::ApplyGate);
    attest(&covey, &resumed_worker);
    attest(&covey, &reviewer);
    attest(&covey, &apply_gate);

    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orchestrator.clone(),
                "unattended recovery proof",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");
    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orchestrator.clone())
                .expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id).expect("valid meta-task id"),
            subtask_id: Some(covey::SubtaskId::parse("unattended_work").expect("valid subtask id")),
            title: SubtaskTitle::parse("unattended work").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create subtask");

    let dead_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                dead_worker.clone(),
                covey::LeaseDurationMs::parse(10_000).expect("valid lease duration"),
                id_key("claim-next-dead-worker"),
            )
            .expect("valid claim-next request"),
        )
        .expect("dead worker claim")
        .expect("claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                dead_worker.clone(),
                dead_claim.claim_id.clone(),
                dead_claim.fence_seq,
                id_key("start-dead-worker"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("dead worker starts");
    covey
        .request_reservation(
            RequestReservationReq::try_from_raw_parts(
                orchestrator.clone(),
                subtask_id.clone(),
                ScopeClass::RepoGlobal,
                "repo",
                Vec::new(),
                10_000,
                id_key("reservation-dead-worker"),
            )
            .expect("valid reservation request"),
        )
        .expect("dead worker reservation");

    rig.tick(10_001);
    assert_eq!(
        covey
            .expire_old_claims()
            .expect("expire stale worker claim")
            .expired_count,
        1
    );
    assert_eq!(
        covey
            .expire_old_reservations()
            .expect("expire stale worker reservation")
            .expired_count,
        1
    );
    let resumed_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                resumed_worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next-resumed-worker"),
            )
            .expect("valid claim-next request"),
        )
        .expect("resumed worker claim")
        .expect("resumed claim result");
    assert_eq!(resumed_claim.subtask_id, subtask_id);
    assert_ne!(resumed_claim.claim_id, dead_claim.claim_id);
    assert!(
        covey
            .session_status(&dead_worker)
            .expect("dead worker session status")
            .session()
            .active_subtask_id()
            .is_none()
    );
    assert_eq!(
        covey
            .subtask_status(&subtask_id)
            .expect("reclaimed subtask")
            .claim()
            .expect("active resumed claim")
            .claim_id,
        resumed_claim.claim_id
    );

    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                resumed_worker.clone(),
                resumed_claim.claim_id.clone(),
                resumed_claim.fence_seq,
                id_key("start-resumed-worker"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("resumed worker starts");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                resumed_worker.clone(),
                resumed_claim.claim_id.clone(),
                resumed_claim.fence_seq,
                "blake3:unattended_artifact".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "unattended-artifact.json".into(),
                "blake3:unattended_paths".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish artifact");
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                resumed_worker.clone(),
                subtask_id.clone(),
                "blake3:unattended_artifact",
                Some("review_unattended_work".into()),
                1,
                id_key("request-review"),
            )
            .expect("valid review request"),
        )
        .expect("request review");
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                resumed_worker.clone(),
                resumed_claim.claim_id,
                resumed_claim.fence_seq,
                id_key("release-resumed-claim"),
            )
            .expect("valid release-claim request"),
        )
        .expect("release resumed claim");

    let review_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                reviewer.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-review"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim review")
        .expect("review claim result");
    assert_eq!(review_claim.subtask_id, "review_unattended_work");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                review_claim.claim_id.clone(),
                review_claim.fence_seq,
                id_key("start-review"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start review");
    covey
        .decide_review(
            DecideReviewReq::try_from_raw_parts(
                reviewer,
                review_id.clone(),
                review_claim.claim_id,
                review_claim.fence_seq,
                covey::ReviewVerdict::Approve,
                "blake3:unattended_findings".into(),
                id_key("decide-review"),
            )
            .expect("valid review decision request"),
        )
        .expect("approve review");
    let queue_id = covey
        .enqueue_for_apply(
            EnqueueForApplyReq::try_from_raw_parts(
                orchestrator,
                "blake3:unattended_artifact".into(),
                subtask_id.clone(),
                SettlementTarget::Canonical,
                id_key("enqueue-for-apply"),
            )
            .expect("valid enqueue-for-apply request"),
        )
        .expect("enqueue for apply");
    let queue_claim = covey
        .mark_in_flight(
            MarkInFlightReq::try_from_raw_parts(
                apply_gate.clone(),
                queue_id.clone(),
                30_000,
                id_key("mark-in-flight"),
            )
            .expect("valid mark-in-flight request"),
        )
        .expect("mark in flight");
    covey
        .record_apply_verification(
            RecordApplyVerificationReq::try_from_raw_parts(
                apply_gate.clone(),
                queue_id.clone(),
                "blake3:unattended_artifact",
                review_id,
                "blake3:unattended_findings",
                queue_claim.claim_fence_seq,
                "mutai-rs",
                "blake3:unattended_verdict",
                "blake3:unattended_seal",
                id_key("record-apply-verification"),
            )
            .expect("valid apply verification request"),
        )
        .expect("record apply verification");

    let mark_applied = MarkAppliedReq::try_from_raw_parts(
        apply_gate,
        queue_id,
        queue_claim.claim_fence_seq,
        "mark-applied-stable",
    )
    .expect("valid mark-applied request");
    covey
        .mark_applied(mark_applied.clone())
        .expect("mark applied in Covey");
    let event_count_after_first_apply = covey.fetch_events(0, 1_000).expect("events").len();
    covey
        .mark_applied(mark_applied)
        .expect("duplicate completion replays idempotently");
    let event_count_after_duplicate_apply = covey.fetch_events(0, 1_000).expect("events").len();
    assert_eq!(
        event_count_after_first_apply,
        event_count_after_duplicate_apply
    );
    assert_eq!(
        covey
            .subtask_status(&subtask_id)
            .expect("final subtask status")
            .subtask()
            .state(),
        SubtaskState::Applied
    );
}

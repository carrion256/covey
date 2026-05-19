use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

mod support;

use covey::{
    ArtifactKind, ClaimNextReq, Covey, CreateSubtaskRequest, DecideReviewReq, EnqueueForApplyReq,
    HeartbeatReq, ManualClock, MarkAppliedReq, MarkInFlightReq, PublishArtifactReq,
    RecordApplyVerificationReq, RecordRuntimeAttestationReq, RegisterSessionReq, ReleaseClaimReq,
    RequestReservationReq, RequestReviewReq, ScopeClass, SessionRole, SessionState,
    SettlementTarget, StartSubtaskReq, SubmitMetaTaskReq, SubtaskState,
};
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

fn id_key(label: &str) -> String {
    format!(
        "{label}-{}",
        NEXT_IDEMPOTENCY_KEY.fetch_add(1, Ordering::Relaxed)
    )
}

fn register(covey: &Covey, principal: &str, role: SessionRole) -> String {
    covey
        .register_session(RegisterSessionReq {
            agent_principal_id: principal.into(),
            agent_instance_id: format!("{principal}-instance"),
            role,
            idempotency_key: id_key("register-session"),
        })
        .expect("register session")
        .session_token
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
                1_700_000_000_001,
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
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orchestrator.clone(),
            prompt_text: "codex disappears proof".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("submit meta task");
    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orchestrator.clone(),
            meta_task_id,
            subtask_id: Some("codex_disappears_work".into()),
            title: "codex disappears work".into(),
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create subtask");

    let stale_claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: disappeared_codex.clone(),
            lease_duration_ms: covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
            idempotency_key: id_key("claim-next-disappeared-codex"),
        })
        .expect("claim by disappeared codex")
        .expect("claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: disappeared_codex.clone(),
            claim_id: stale_claim.claim_id.clone(),
            fence_seq: stale_claim.fence_seq,
            idempotency_key: id_key("start-disappeared-codex"),
        })
        .expect("disappeared codex starts");

    rig.tick(60_000);
    covey
        .heartbeat(HeartbeatReq {
            session_token: orchestrator,
            idempotency_key: id_key("heartbeat-orchestrator-before-reap"),
        })
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
        .claim_next_subtask(ClaimNextReq {
            session_token: resumed_worker.clone(),
            lease_duration_ms: covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
            idempotency_key: id_key("claim-after-disappeared-codex"),
        })
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
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orchestrator.clone(),
            prompt_text: "unattended recovery proof".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("submit meta task");
    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orchestrator.clone(),
            meta_task_id,
            subtask_id: Some("unattended_work".into()),
            title: "unattended work".into(),
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create subtask");

    let dead_claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: dead_worker.clone(),
            lease_duration_ms: covey::LeaseDurationMs::parse(10_000).expect("valid lease duration"),
            idempotency_key: id_key("claim-next-dead-worker"),
        })
        .expect("dead worker claim")
        .expect("claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: dead_worker.clone(),
            claim_id: dead_claim.claim_id.clone(),
            fence_seq: dead_claim.fence_seq,
            idempotency_key: id_key("start-dead-worker"),
        })
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
        .claim_next_subtask(ClaimNextReq {
            session_token: resumed_worker.clone(),
            lease_duration_ms: covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
            idempotency_key: id_key("claim-next-resumed-worker"),
        })
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
        .start_subtask(StartSubtaskReq {
            session_token: resumed_worker.clone(),
            claim_id: resumed_claim.claim_id.clone(),
            fence_seq: resumed_claim.fence_seq,
            idempotency_key: id_key("start-resumed-worker"),
        })
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
        .release_claim(ReleaseClaimReq {
            session_token: resumed_worker.clone(),
            claim_id: resumed_claim.claim_id,
            fence_seq: resumed_claim.fence_seq,
            idempotency_key: id_key("release-resumed-claim"),
        })
        .expect("release resumed claim");

    let review_claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: reviewer.clone(),
            lease_duration_ms: covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
            idempotency_key: id_key("claim-review"),
        })
        .expect("claim review")
        .expect("review claim result");
    assert_eq!(review_claim.subtask_id, "review_unattended_work");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: reviewer.clone(),
            claim_id: review_claim.claim_id.clone(),
            fence_seq: review_claim.fence_seq,
            idempotency_key: id_key("start-review"),
        })
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

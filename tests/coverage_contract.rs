use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

mod support;

use covey::{
    ArtifactKind, ClaimNextReq, ClaimReadyQueueReq, ClaimSubtaskReq, ConflictResolutionState,
    Covey, CoveyError, CreateSubtaskReq, DecideReviewReq, EnqueueForApplyReq, ManualClock,
    MarkAppliedReq, MarkInFlightReq, OverlapQueryReq, PublishArtifactReq,
    RecordApplyVerificationReq, RecordRuntimeAttestationReq, RegisterSessionReq, ReleaseClaimReq,
    ReleaseReservationReq, RenewClaimReq, RenewReservationReq, RequestReservationReq,
    RequestReviewReq, ResolveConflictReq, ReviewState, ReviewVerdict, ScopeClass, SessionRole,
    SettlementTarget, StartSubtaskReq, SubmitMetaTaskReq, SubtaskKind, SubtaskState,
    SupersedeQueueItemReq, VerifyLandingAuthorizationReq,
};
use proptest::prelude::*;
use rstest::{fixture, rstest};
use rusqlite::{Connection, params};
use tempfile::TempDir;

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

struct Rig {
    _dir: TempDir,
    db_path: std::path::PathBuf,
    clock: Arc<ManualClock>,
    covey: Covey,
}

#[fixture]
fn rig() -> Rig {
    support::enable_info_logging();
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("covey.db");
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_with_clock(&db_path, clock.clone()).expect("open covey");
    Rig {
        _dir: dir,
        db_path,
        clock,
        covey,
    }
}

fn id_key(label: &str) -> String {
    format!("{label}-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
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
        .record_runtime_attestation(RecordRuntimeAttestationReq {
            session_token: session_token.to_owned(),
            provider: "covey-test".into(),
            model: "test-model".into(),
            provider_run_id: format!("provider-run-{session_token}"),
            provider_run_id_issuer: "covey-test-provider".into(),
            process_id: Some(format!("pid-{session_token}")),
            container_id: None,
            command_transcript_digest: format!("blake3:{session_token}:transcript"),
            started_at: 1_700_000_000_000,
            ended_at: 1_700_000_000_001,
            idempotency_key: format!("record-runtime-attestation-{session_token}"),
        })
        .expect("record runtime attestation");
}

fn force_runtime_ref(rig: &Rig, session_token: &str, process_id: &str, transcript_digest: &str) {
    let conn = Connection::open(&rig.db_path).expect("open db");
    let updated = conn
        .execute(
            r#"
            UPDATE runtime_attestations
            SET process_id = ?2,
                container_id = NULL,
                command_transcript_digest = ?3
            WHERE session_token = ?1
            "#,
            params![session_token, process_id, transcript_digest],
        )
        .expect("force runtime ref");
    assert_eq!(updated, 1);
}

fn force_provider_run_ref(
    rig: &Rig,
    session_token: &str,
    provider_run_id_issuer: &str,
    provider_run_id: &str,
) {
    let conn = Connection::open(&rig.db_path).expect("open db");
    let updated = conn
        .execute(
            r#"
            UPDATE runtime_attestations
            SET provider_run_id_issuer = ?2,
                provider_run_id = ?3
            WHERE session_token = ?1
            "#,
            params![session_token, provider_run_id_issuer, provider_run_id],
        )
        .expect("force provider run ref");
    assert_eq!(updated, 1);
}

fn review_session_for(rig: &Rig, subtask_id: &str) -> String {
    let conn = Connection::open(&rig.db_path).expect("open db");
    conn.query_row(
        "SELECT reviewer_session FROM reviews WHERE subtask_id = ?1",
        params![subtask_id],
        |row| row.get(0),
    )
    .expect("reviewer session")
}

fn seed_work(covey: &Covey, subtask_id: &str) -> (String, String) {
    let orch = register(
        covey,
        &format!("orch-{subtask_id}"),
        SessionRole::Orchestrator,
    );
    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: format!("meta for {subtask_id}"),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("submit meta task");
    let actual_subtask_id = covey
        .create_subtask(CreateSubtaskReq {
            session_token: orch.clone(),
            meta_task_id,
            subtask_id: Some(subtask_id.into()),
            title: format!("work {subtask_id}"),
            kind: SubtaskKind::Work,
            review_target_subtask_id: None,
            review_target_artifact_digest: None,
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create subtask");
    (orch, actual_subtask_id)
}

fn prepare_approved_artifact(rig: &Rig, subtask_id: &str, worker: &str, digest: &str) {
    let reviewer = register(
        &rig.covey,
        &format!("reviewer-{subtask_id}"),
        SessionRole::Reviewer,
    );
    attest(&rig.covey, worker);
    attest(&rig.covey, &reviewer);
    let conn = Connection::open(&rig.db_path).expect("open db");
    conn.execute(
        "INSERT INTO artifacts (
            artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session,
            manifest_path, changed_paths_digest, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            digest,
            ArtifactKind::PatchBundle.to_string(),
            "base",
            subtask_id,
            worker,
            format!("{subtask_id}.json"),
            format!("{digest}:paths"),
            1_700_000_000_000_i64,
        ],
    )
    .expect("insert artifact");
    conn.execute(
        "UPDATE subtasks SET state = ?2, artifact_digest = ?3 WHERE subtask_id = ?1",
        params![subtask_id, SubtaskState::Approved.to_string(), digest],
    )
    .expect("approve subtask");
    conn.execute(
        "INSERT INTO reviews (
            review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id,
            verdict, findings_digest, state, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?8)",
        params![
            format!("review_{subtask_id}"),
            subtask_id,
            digest,
            reviewer,
            ReviewVerdict::Approve.to_string(),
            format!("{digest}:findings"),
            ReviewState::Decided.to_string(),
            1_700_000_000_001_i64,
        ],
    )
    .expect("insert approved review evidence");
}

fn enqueue_ready_item(rig: &Rig, subtask_id: &str, digest: &str) -> (String, String) {
    let (orch, work_id) = seed_work(&rig.covey, subtask_id);
    prepare_approved_artifact(rig, &work_id, &orch, digest);
    let queue_id = rig
        .covey
        .enqueue_for_apply(EnqueueForApplyReq {
            session_token: orch.clone(),
            artifact_digest: digest.into(),
            subtask_id: work_id,
            settlement_target: SettlementTarget::Canonical,
            idempotency_key: id_key("enqueue-for-apply"),
        })
        .expect("enqueue");
    (orch, queue_id)
}

fn record_apply_verification(
    covey: &Covey,
    gate: &str,
    queue_id: &str,
    subtask_id: &str,
    digest: &str,
    findings_digest: &str,
    claim_fence_seq: i64,
) {
    attest(covey, gate);
    covey
        .record_apply_verification(RecordApplyVerificationReq {
            session_token: gate.to_owned(),
            queue_id: queue_id.to_owned(),
            artifact_digest: digest.to_owned(),
            review_id: format!("review_{subtask_id}"),
            findings_digest: findings_digest.to_owned(),
            claim_fence_seq,
            verifier: "mutai-rs".to_owned(),
            verdict_digest: format!("{digest}:verdict"),
            seal_digest: format!("{digest}:seal"),
            idempotency_key: id_key("record-apply-verification"),
        })
        .expect("record apply verification");
}

#[rstest]
fn ready_queue_alternate_paths_cover_fetch_claim_supersede_and_owner_errors(rig: Rig) {
    let (_orch, queue_id) = enqueue_ready_item(&rig, "queue_cover", "blake3:queue_cover");
    let gate = register(&rig.covey, "gate-queue-cover", SessionRole::ApplyGate);
    let other_gate = register(&rig.covey, "gate-queue-other", SessionRole::ApplyGate);

    let queued = rig.covey.fetch_ready_queue(10).expect("fetch queue");
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].queue_id, queue_id);

    let claim = rig
        .covey
        .claim_next_ready_queue_item(ClaimReadyQueueReq {
            session_token: gate.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-ready-queue"),
        })
        .expect("claim queue")
        .expect("queue claim");
    assert_eq!(claim.queue_id, queue_id);
    assert!(
        rig.covey
            .claim_next_ready_queue_item(ClaimReadyQueueReq {
                session_token: gate.clone(),
                lease_duration_ms: 30_000,
                idempotency_key: id_key("claim-ready-empty"),
            })
            .expect("empty queue claim")
            .is_none()
    );

    assert!(matches!(
        rig.covey.mark_applied(MarkAppliedReq {
            session_token: other_gate,
            queue_id: queue_id.clone(),
            claim_fence_seq: claim.claim_fence_seq,
            idempotency_key: id_key("mark-applied-wrong-owner"),
        }),
        Err(CoveyError::NotQueueClaimOwner { .. })
    ));
    assert!(matches!(
        rig.covey.mark_applied(MarkAppliedReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            claim_fence_seq: claim.claim_fence_seq + 1,
            idempotency_key: id_key("mark-applied-stale-fence"),
        }),
        Err(CoveyError::StaleFenceToken { .. })
    ));

    rig.covey
        .supersede_queue_item(SupersedeQueueItemReq {
            session_token: gate,
            queue_id: queue_id.clone(),
            idempotency_key: id_key("supersede-in-flight"),
        })
        .expect("supersede in-flight item");
    assert_eq!(rig.covey.fetch_ready_queue(10).expect("fetch after"), []);
}

#[rstest]
fn mark_in_flight_and_lease_expiry_paths_are_observable(rig: Rig) {
    let (_orch, queue_id) = enqueue_ready_item(&rig, "queue_expiry", "blake3:queue_expiry");
    let gate = register(&rig.covey, "gate-queue-expiry", SessionRole::ApplyGate);

    let claim = rig
        .covey
        .mark_in_flight(MarkInFlightReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            lease_duration_ms: 1,
            idempotency_key: id_key("mark-in-flight"),
        })
        .expect("mark in flight");
    assert_eq!(claim.claim_fence_seq, 1);

    rig.clock.advance(2);
    assert!(matches!(
        rig.covey.mark_applied(MarkAppliedReq {
            session_token: gate,
            queue_id,
            claim_fence_seq: claim.claim_fence_seq,
            idempotency_key: id_key("mark-applied-expired"),
        }),
        Err(CoveyError::LeaseExpired { .. })
    ));
}

#[rstest]
fn ready_queue_error_and_metrics_paths_are_observable(rig: Rig) {
    let (orch, work_id) = seed_work(&rig.covey, "queue_error_paths");
    prepare_approved_artifact(&rig, &work_id, &orch, "blake3:queue_error_paths");

    assert!(matches!(
        rig.covey.enqueue_for_apply(EnqueueForApplyReq {
            session_token: orch.clone(),
            artifact_digest: "blake3:different".into(),
            subtask_id: work_id.clone(),
            settlement_target: SettlementTarget::Canonical,
            idempotency_key: id_key("enqueue-digest-mismatch"),
        }),
        Err(CoveyError::IllegalTransition { .. })
    ));

    let queue_id = rig
        .covey
        .enqueue_for_apply(EnqueueForApplyReq {
            session_token: orch.clone(),
            artifact_digest: "blake3:queue_error_paths".into(),
            subtask_id: work_id,
            settlement_target: SettlementTarget::Canonical,
            idempotency_key: id_key("enqueue-for-apply"),
        })
        .expect("enqueue should succeed");
    let metrics = rig.covey.ready_queue_metrics().expect("queue metrics");
    assert_eq!(metrics.queued_count, 1);
    assert_eq!(metrics.in_flight_count, 0);
    assert!(metrics.oldest_queued_age_ms.is_some());

    assert!(matches!(
        rig.covey.claim_next_ready_queue_item(ClaimReadyQueueReq {
            session_token: orch.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-ready-wrong-role"),
        }),
        Err(CoveyError::WrongRole { .. })
    ));

    assert!(matches!(
        rig.covey.mark_applied(MarkAppliedReq {
            session_token: orch.clone(),
            queue_id: queue_id.clone(),
            claim_fence_seq: 1,
            idempotency_key: id_key("mark-applied-queued"),
        }),
        Err(CoveyError::WrongRole { .. })
    ));

    let gate = register(&rig.covey, "gate-queue-error-paths", SessionRole::ApplyGate);
    attest(&rig.covey, &gate);
    assert!(matches!(
        rig.covey.mark_applied(MarkAppliedReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            claim_fence_seq: 1,
            idempotency_key: id_key("mark-applied-not-in-flight"),
        }),
        Err(CoveyError::IllegalTransition { .. })
    ));

    let claim = rig
        .covey
        .mark_in_flight(MarkInFlightReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("mark-in-flight"),
        })
        .expect("mark in flight should claim queue item");
    let metrics = rig.covey.ready_queue_metrics().expect("queue metrics");
    assert_eq!(metrics.queued_count, 0);
    assert_eq!(metrics.in_flight_count, 1);
    assert!(metrics.oldest_in_flight_age_ms.is_some());

    assert!(matches!(
        rig.covey.mark_applied(MarkAppliedReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            claim_fence_seq: claim.claim_fence_seq,
            idempotency_key: id_key("mark-applied-missing-verification"),
        }),
        Err(CoveyError::ApplyGateEvidenceMissing { .. })
    ));
    record_apply_verification(
        &rig.covey,
        &gate,
        &queue_id,
        "queue_error_paths",
        "blake3:queue_error_paths",
        "blake3:queue_error_paths:findings",
        claim.claim_fence_seq,
    );

    rig.covey
        .mark_applied(MarkAppliedReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            claim_fence_seq: claim.claim_fence_seq,
            idempotency_key: id_key("mark-applied-success"),
        })
        .expect("mark applied should settle queue item");

    assert!(matches!(
        rig.covey.supersede_queue_item(SupersedeQueueItemReq {
            session_token: gate,
            queue_id,
            idempotency_key: id_key("supersede-applied"),
        }),
        Err(CoveyError::IllegalTransition { .. })
    ));
}

#[rstest]
fn apply_verification_requires_runtime_attestation(rig: Rig) {
    let (_orch, queue_id) = enqueue_ready_item(
        &rig,
        "queue_requires_runtime_attestation",
        "blake3:queue_requires_runtime_attestation",
    );
    let gate = register(
        &rig.covey,
        "gate-missing-runtime-attestation",
        SessionRole::ApplyGate,
    );
    let claim = rig
        .covey
        .mark_in_flight(MarkInFlightReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("mark-in-flight"),
        })
        .expect("mark in flight");

    assert!(matches!(
        rig.covey.record_apply_verification(RecordApplyVerificationReq {
            session_token: gate.clone(),
            queue_id,
            artifact_digest: "blake3:queue_requires_runtime_attestation".into(),
            review_id: "review_queue_requires_runtime_attestation".into(),
            findings_digest: "blake3:queue_requires_runtime_attestation:findings".into(),
            claim_fence_seq: claim.claim_fence_seq,
            verifier: "mutai-rs".into(),
            verdict_digest: "blake3:queue_requires_runtime_attestation:verdict".into(),
            seal_digest: "blake3:queue_requires_runtime_attestation:seal".into(),
            idempotency_key: id_key("record-apply-verification"),
        }),
        Err(CoveyError::RuntimeAttestationMissing { session_token }) if session_token == gate
    ));
}

#[rstest]
fn apply_verification_rejects_shared_actor_runtime_evidence(rig: Rig) {
    let subtask_id = "queue_shared_worker_reviewer_runtime";
    let digest = "blake3:queue_shared_worker_reviewer_runtime";
    let (worker, queue_id) = enqueue_ready_item(&rig, subtask_id, digest);
    let reviewer = review_session_for(&rig, subtask_id);
    force_runtime_ref(
        &rig,
        &worker,
        "pid-shared-review",
        "blake3:worker-transcript",
    );
    force_runtime_ref(
        &rig,
        &reviewer,
        "pid-shared-review",
        "blake3:reviewer-transcript",
    );
    let gate = register(&rig.covey, "gate-shared-runtime", SessionRole::ApplyGate);
    attest(&rig.covey, &gate);
    let claim = rig
        .covey
        .mark_in_flight(MarkInFlightReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("mark-in-flight"),
        })
        .expect("mark in flight");

    assert!(matches!(
        rig.covey.record_apply_verification(RecordApplyVerificationReq {
            session_token: gate,
            queue_id,
            artifact_digest: digest.into(),
            review_id: format!("review_{subtask_id}"),
            findings_digest: format!("{digest}:findings"),
            claim_fence_seq: claim.claim_fence_seq,
            verifier: "mutai-rs".into(),
            verdict_digest: format!("{digest}:verdict"),
            seal_digest: format!("{digest}:seal"),
            idempotency_key: id_key("record-apply-verification"),
        }),
        Err(CoveyError::ApplyGateEvidenceMissing { reason, .. })
            if reason == "producer and reviewer runtime refs are not separated"
    ));

    let subtask_id = "queue_shared_worker_gate_transcript";
    let digest = "blake3:queue_shared_worker_gate_transcript";
    let (worker, queue_id) = enqueue_ready_item(&rig, subtask_id, digest);
    let gate = register(&rig.covey, "gate-shared-transcript", SessionRole::ApplyGate);
    attest(&rig.covey, &gate);
    force_runtime_ref(&rig, &worker, "pid-worker", "blake3:shared-transcript");
    force_runtime_ref(&rig, &gate, "pid-gate", "blake3:shared-transcript");
    let claim = rig
        .covey
        .mark_in_flight(MarkInFlightReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("mark-in-flight"),
        })
        .expect("mark in flight");

    assert!(matches!(
        rig.covey.record_apply_verification(RecordApplyVerificationReq {
            session_token: gate,
            queue_id,
            artifact_digest: digest.into(),
            review_id: format!("review_{subtask_id}"),
            findings_digest: format!("{digest}:findings"),
            claim_fence_seq: claim.claim_fence_seq,
            verifier: "mutai-rs".into(),
            verdict_digest: format!("{digest}:verdict"),
            seal_digest: format!("{digest}:seal"),
            idempotency_key: id_key("record-apply-verification"),
        }),
        Err(CoveyError::ApplyGateEvidenceMissing { reason, .. })
            if reason == "producer and apply_gate transcript digests are not separated"
    ));
}

#[rstest]
fn apply_verification_rejects_shared_provider_run_identity(rig: Rig) {
    let subtask_id = "queue_shared_provider_run";
    let digest = "blake3:queue_shared_provider_run";
    let (worker, queue_id) = enqueue_ready_item(&rig, subtask_id, digest);
    let reviewer = review_session_for(&rig, subtask_id);
    force_provider_run_ref(&rig, &worker, "codex-provider", "run-shared");
    force_provider_run_ref(&rig, &reviewer, "codex-provider", "run-shared");
    let gate = register(
        &rig.covey,
        "gate-shared-provider-run",
        SessionRole::ApplyGate,
    );
    attest(&rig.covey, &gate);
    let claim = rig
        .covey
        .mark_in_flight(MarkInFlightReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("mark-in-flight"),
        })
        .expect("mark in flight");

    assert!(matches!(
        rig.covey.record_apply_verification(RecordApplyVerificationReq {
            session_token: gate,
            queue_id,
            artifact_digest: digest.into(),
            review_id: format!("review_{subtask_id}"),
            findings_digest: format!("{digest}:findings"),
            claim_fence_seq: claim.claim_fence_seq,
            verifier: "mutai-rs".into(),
            verdict_digest: format!("{digest}:verdict"),
            seal_digest: format!("{digest}:seal"),
            idempotency_key: id_key("record-apply-verification"),
        }),
        Err(CoveyError::ApplyGateEvidenceMissing { reason, .. })
            if reason == "producer and reviewer provider run ids are not separated"
    ));
}

#[rstest]
fn mark_applied_requires_live_review_evidence_and_apply_gate_separation(rig: Rig) {
    let (orch, work_id) = seed_work(&rig.covey, "queue_evidence_required");
    let gate = register(&rig.covey, "gate-evidence-required", SessionRole::ApplyGate);
    attest(&rig.covey, &gate);
    prepare_approved_artifact_without_review(
        &rig,
        &work_id,
        &orch,
        "blake3:queue_evidence_required",
    );
    let queue_id = rig
        .covey
        .enqueue_for_apply(EnqueueForApplyReq {
            session_token: orch.clone(),
            artifact_digest: "blake3:queue_evidence_required".into(),
            subtask_id: work_id,
            settlement_target: SettlementTarget::Canonical,
            idempotency_key: id_key("enqueue-for-apply"),
        })
        .expect("enqueue without review evidence");
    let claim = rig
        .covey
        .mark_in_flight(MarkInFlightReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("mark-in-flight"),
        })
        .expect("claim queue item");

    assert!(matches!(
        rig.covey.mark_applied(MarkAppliedReq {
            session_token: gate,
            queue_id,
            claim_fence_seq: claim.claim_fence_seq,
            idempotency_key: id_key("mark-applied"),
        }),
        Err(CoveyError::ApplyGateEvidenceMissing { .. })
    ));

    let (orch, work_id) = seed_work(&rig.covey, "queue_apply_gate_separation");
    let gate = register(
        &rig.covey,
        "gate-separation-producer",
        SessionRole::ApplyGate,
    );
    prepare_approved_artifact(&rig, &work_id, &gate, "blake3:queue_apply_gate_separation");
    let queue_id = rig
        .covey
        .enqueue_for_apply(EnqueueForApplyReq {
            session_token: orch,
            artifact_digest: "blake3:queue_apply_gate_separation".into(),
            subtask_id: work_id,
            settlement_target: SettlementTarget::Canonical,
            idempotency_key: id_key("enqueue-for-apply"),
        })
        .expect("enqueue with review evidence");
    let claim = rig
        .covey
        .mark_in_flight(MarkInFlightReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("mark-in-flight"),
        })
        .expect("claim queue item");

    assert!(matches!(
        rig.covey.mark_applied(MarkAppliedReq {
            session_token: gate,
            queue_id,
            claim_fence_seq: claim.claim_fence_seq,
            idempotency_key: id_key("mark-applied"),
        }),
        Err(CoveyError::ApplyGateSeparationOfDutiesViolation {
            conflicting_role,
            ..
        }) if conflicting_role == "producer"
    ));
}

#[rstest]
fn landing_authorization_verification_rechecks_live_apply_evidence(rig: Rig) {
    let subtask_id = "queue_landing_authorization";
    let digest = "blake3:queue_landing_authorization";
    let (_orch, queue_id) = enqueue_ready_item(&rig, subtask_id, digest);
    let gate = register(
        &rig.covey,
        "gate-landing-authorization",
        SessionRole::ApplyGate,
    );
    let claim = rig
        .covey
        .mark_in_flight(MarkInFlightReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("mark-in-flight"),
        })
        .expect("claim queue item");
    record_apply_verification(
        &rig.covey,
        &gate,
        &queue_id,
        subtask_id,
        digest,
        &format!("{digest}:findings"),
        claim.claim_fence_seq,
    );
    rig.covey
        .mark_applied(MarkAppliedReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            claim_fence_seq: claim.claim_fence_seq,
            idempotency_key: id_key("mark-applied"),
        })
        .expect("mark applied");

    let status = rig
        .covey
        .verify_landing_authorization(VerifyLandingAuthorizationReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            artifact_digest: digest.into(),
            review_id: format!("review_{subtask_id}"),
            findings_digest: format!("{digest}:findings"),
            claim_fence_seq: claim.claim_fence_seq,
            verifier: "mutai-rs".into(),
            verdict_digest: format!("{digest}:verdict"),
            seal_digest: format!("{digest}:seal"),
        })
        .expect("verify landing authorization");
    assert!(status.accepted);
    assert_eq!(status.recorded_by_session, gate);

    assert!(matches!(
        rig.covey.verify_landing_authorization(VerifyLandingAuthorizationReq {
            session_token: status.recorded_by_session,
            queue_id,
            artifact_digest: digest.into(),
            review_id: format!("review_{subtask_id}"),
            findings_digest: format!("{digest}:findings"),
            claim_fence_seq: claim.claim_fence_seq,
            verifier: "mutai-rs".into(),
            verdict_digest: format!("{digest}:verdict"),
            seal_digest: "blake3:wrong-apply-verification-seal".into(),
        }),
        Err(CoveyError::ApplyGateEvidenceMissing { reason, .. })
            if reason == "accepted apply verifier verdict does not match landing authorization"
    ));
}

fn prepare_approved_artifact_without_review(
    rig: &Rig,
    subtask_id: &str,
    worker: &str,
    digest: &str,
) {
    let conn = Connection::open(&rig.db_path).expect("open db");
    conn.execute(
        "INSERT INTO artifacts (
            artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session,
            manifest_path, changed_paths_digest, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            digest,
            ArtifactKind::PatchBundle.to_string(),
            "base",
            subtask_id,
            worker,
            format!("{subtask_id}.json"),
            format!("{digest}:paths"),
            1_700_000_000_000_i64,
        ],
    )
    .expect("insert artifact");
    conn.execute(
        "UPDATE subtasks SET state = ?2, artifact_digest = ?3 WHERE subtask_id = ?1",
        params![subtask_id, SubtaskState::Approved.to_string(), digest],
    )
    .expect("approve subtask");
}

#[rstest]
fn reservation_success_error_and_conflict_paths_use_public_contract(rig: Rig) {
    let (orch, work_id) = seed_work(&rig.covey, "reservation_cover");
    let first = rig
        .covey
        .request_reservation(RequestReservationReq {
            session_token: orch.clone(),
            owner_subtask_id: work_id.clone(),
            scope_class: ScopeClass::Subtree,
            scope_key: "src/reservation".into(),
            generated_members: vec!["generated/reservation.rs".into()],
            lease_duration_ms: 60_000,
            idempotency_key: id_key("request-reservation"),
        })
        .expect("request first reservation");
    let second = rig
        .covey
        .request_reservation(RequestReservationReq {
            session_token: orch.clone(),
            owner_subtask_id: work_id,
            scope_class: ScopeClass::ExactPath,
            scope_key: "src/reservation/mod.rs".into(),
            generated_members: vec![],
            lease_duration_ms: 60_000,
            idempotency_key: id_key("request-reservation"),
        })
        .expect("request overlapping reservation");

    let overlaps = rig
        .covey
        .find_overlapping_reservations(OverlapQueryReq {
            scope_class: ScopeClass::GeneratedSet,
            scope_key: "generated/query".into(),
            generated_members: vec!["src/reservation/mod.rs".into()],
        })
        .expect("find generated overlap");
    assert!(
        overlaps
            .iter()
            .any(|reservation| reservation.reservation_id == first)
    );

    let conflicts = rig.covey.list_conflicts().expect("list conflicts");
    assert!(!conflicts.is_empty());
    rig.covey
        .resolve_conflict(ResolveConflictReq {
            session_token: orch.clone(),
            conflict_id: conflicts[0].conflict_id.clone(),
            resolution_state: ConflictResolutionState::Acknowledged,
            idempotency_key: id_key("resolve-conflict"),
        })
        .expect("resolve conflict");
    assert!(matches!(
        rig.covey.resolve_conflict(ResolveConflictReq {
            session_token: orch.clone(),
            conflict_id: "missing-conflict".into(),
            resolution_state: ConflictResolutionState::Resolved,
            idempotency_key: id_key("resolve-missing-conflict"),
        }),
        Err(CoveyError::ConflictNotFound)
    ));

    let renewed = rig
        .covey
        .renew_reservation(RenewReservationReq {
            session_token: orch.clone(),
            reservation_id: second.clone(),
            extend_by_ms: 5_000,
            idempotency_key: id_key("renew-reservation"),
        })
        .expect("renew active reservation");
    assert_eq!(renewed.reservation_id, second);
    rig.covey
        .release_reservation(ReleaseReservationReq {
            session_token: orch.clone(),
            reservation_id: second.clone(),
            idempotency_key: id_key("release-reservation"),
        })
        .expect("release reservation");
    assert!(matches!(
        rig.covey.renew_reservation(RenewReservationReq {
            session_token: orch,
            reservation_id: second,
            extend_by_ms: 5_000,
            idempotency_key: id_key("renew-released-reservation"),
        }),
        Err(CoveyError::IllegalTransition { .. })
    ));
}

#[rstest]
fn lifecycle_edge_paths_cover_empty_claim_duplicate_and_wrong_role(rig: Rig) {
    let orch = register(
        &rig.covey,
        "orch-lifecycle-cover",
        SessionRole::Orchestrator,
    );
    let worker = register(&rig.covey, "worker-lifecycle-cover", SessionRole::Executor);
    assert!(
        rig.covey
            .claim_next_subtask(ClaimNextReq {
                session_token: worker.clone(),
                lease_duration_ms: 10_000,
                idempotency_key: id_key("claim-empty"),
            })
            .expect("claim on empty queue")
            .is_none()
    );
    assert!(matches!(
        rig.covey.claim_next_subtask(ClaimNextReq {
            session_token: orch.clone(),
            lease_duration_ms: 10_000,
            idempotency_key: id_key("claim-wrong-role"),
        }),
        Err(CoveyError::WrongRole { .. })
    ));

    let meta_task_id = rig
        .covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "generated subtask id".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("submit meta task");
    let generated = rig
        .covey
        .create_subtask(CreateSubtaskReq {
            session_token: orch.clone(),
            meta_task_id: meta_task_id.clone(),
            subtask_id: None,
            title: "generated".into(),
            kind: SubtaskKind::Work,
            review_target_subtask_id: None,
            review_target_artifact_digest: None,
            priority: 1,
            idempotency_key: id_key("create-generated-subtask"),
        })
        .expect("create generated subtask");
    assert!(generated.starts_with("subtask_"));
    assert!(matches!(
        rig.covey.create_subtask(CreateSubtaskReq {
            session_token: orch,
            meta_task_id,
            subtask_id: Some(generated),
            title: "duplicate".into(),
            kind: SubtaskKind::Work,
            review_target_subtask_id: None,
            review_target_artifact_digest: None,
            priority: 1,
            idempotency_key: id_key("create-duplicate-subtask"),
        }),
        Err(CoveyError::DuplicateSubtaskId { .. })
    ));
}

#[rstest]
fn artifact_review_success_supersede_and_claim_renewal_paths_are_observable(rig: Rig) {
    let (_orch, work_id) = seed_work(&rig.covey, "artifact_review_cover");
    let worker = register(&rig.covey, "worker-artifact-review", SessionRole::Executor);
    let reviewer = register(
        &rig.covey,
        "reviewer-artifact-review",
        SessionRole::Reviewer,
    );

    let work_claim = rig
        .covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-work-for-review"),
        })
        .expect("claim work")
        .expect("work claim should exist");
    assert_eq!(work_claim.subtask_id, work_id);
    rig.covey
        .start_subtask(StartSubtaskReq {
            session_token: worker.clone(),
            claim_id: work_claim.claim_id.clone(),
            fence_seq: work_claim.fence_seq,
            idempotency_key: id_key("start-work-for-review"),
        })
        .expect("start work");
    let renewed = rig
        .covey
        .renew_claim(RenewClaimReq {
            session_token: worker.clone(),
            claim_id: work_claim.claim_id.clone(),
            fence_seq: work_claim.fence_seq,
            extend_by_ms: 1_000,
            idempotency_key: id_key("renew-work-claim"),
        })
        .expect("renew work claim");
    assert!(renewed.lease_deadline > work_claim.lease_deadline);

    rig.covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: work_claim.claim_id.clone(),
            fence_seq: work_claim.fence_seq,
            artifact_digest: "blake3:artifact_review_first".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base-1".into(),
            manifest_path: "artifacts/first.json".into(),
            changed_paths_digest: "blake3:paths_first".into(),
            idempotency_key: id_key("publish-first-artifact"),
        })
        .expect("publish first artifact");
    let first_review_id = rig
        .covey
        .request_review(RequestReviewReq {
            session_token: worker.clone(),
            subtask_id: work_id.clone(),
            artifact_digest: "blake3:artifact_review_first".into(),
            review_subtask_id: Some("review_cover_first".into()),
            priority: 5,
            idempotency_key: id_key("request-first-review"),
        })
        .expect("request first review");
    assert!(!first_review_id.is_empty());

    rig.covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: work_claim.claim_id.clone(),
            fence_seq: work_claim.fence_seq,
            artifact_digest: "blake3:artifact_review_second".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base-2".into(),
            manifest_path: "artifacts/second.json".into(),
            changed_paths_digest: "blake3:paths_second".into(),
            idempotency_key: id_key("publish-second-artifact"),
        })
        .expect("publish second artifact and supersede the first review");
    let review_id = rig
        .covey
        .request_review(RequestReviewReq {
            session_token: worker.clone(),
            subtask_id: work_id.clone(),
            artifact_digest: "blake3:artifact_review_second".into(),
            review_subtask_id: Some("review_cover_second".into()),
            priority: 1,
            idempotency_key: id_key("request-second-review"),
        })
        .expect("request second review");

    let review_claim = rig
        .covey
        .claim_subtask(ClaimSubtaskReq {
            session_token: reviewer.clone(),
            subtask_id: "review_cover_second".into(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-review-subtask"),
        })
        .expect("claim review");
    rig.covey
        .start_subtask(StartSubtaskReq {
            session_token: reviewer.clone(),
            claim_id: review_claim.claim_id.clone(),
            fence_seq: review_claim.fence_seq,
            idempotency_key: id_key("start-review-subtask"),
        })
        .expect("start review");
    rig.covey
        .decide_review(DecideReviewReq {
            session_token: reviewer,
            review_id,
            claim_id: review_claim.claim_id,
            fence_seq: review_claim.fence_seq,
            verdict: ReviewVerdict::Approve,
            findings_digest: "blake3:review_findings".into(),
            idempotency_key: id_key("decide-review"),
        })
        .expect("decide review");

    rig.covey
        .release_claim(ReleaseClaimReq {
            session_token: worker,
            claim_id: work_claim.claim_id,
            fence_seq: work_claim.fence_seq,
            idempotency_key: id_key("release-approved-work-claim"),
        })
        .expect("release approved work claim");
}

proptest! {
    #[test]
    fn ready_queue_reclaim_fence_sequences_increase_after_expiry(reclaim_count in 1usize..6) {
        let rig = rig();
        let (_orch, queue_id) = enqueue_ready_item(&rig, "queue_prop", "blake3:queue_prop");
        let gate = register(&rig.covey, "gate-queue-prop", SessionRole::ApplyGate);
        let mut fences = Vec::new();

        for _ in 0..reclaim_count {
            let claim = rig
                .covey
                .claim_next_ready_queue_item(ClaimReadyQueueReq {
                    session_token: gate.clone(),
                    lease_duration_ms: 1,
                    idempotency_key: id_key("claim-ready-prop"),
                })
                .expect("claim ready")
                .expect("claim should exist");
            prop_assert_eq!(claim.queue_id.as_str(), queue_id.as_str());
            fences.push(claim.claim_fence_seq);
            rig.clock.advance(2);
        }

        let expected = (1..=reclaim_count as i64).collect::<Vec<_>>();
        prop_assert_eq!(fences, expected);
    }
}

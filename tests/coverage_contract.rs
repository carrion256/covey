use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

mod support;

use covey::{
    ArtifactKind, ClaimNextReq, ClaimReadyQueueReq, ClaimSubtaskReq, ConflictResolutionState,
    Covey, CoveyError, CreateSubtaskRequest, DecideReviewReq, EnqueueForApplyReq, EventPayload,
    FenceSeq, IdempotencyKey, LeaseDurationMs, ManualClock, MarkAppliedReq, MarkInFlightReq,
    OpenSpecArchiveStatusState, OverlapQueryReq, PublishArtifactReq, RecordApplyVerificationReq,
    RecordLandingReceiptReq, RecordOpenSpecArchiveStatusReq, RecordRuntimeAttestationReq,
    RegisterSessionReq, ReleaseClaimReq, ReleaseReservationReq, RenewClaimReq, RenewReservationReq,
    RequestReservationReq, RequestReviewReq, ResolveConflictReq, ReviewState, ReviewVerdict,
    ScopeClass, SessionRole, SettlementTarget, StartSubtaskReq, SubmitMetaTaskReq, SubtaskState,
    SubtaskTitle, SupersedeQueueItemReq, VerifyLandingAuthorizationReq,
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

fn id_key(label: &str) -> IdempotencyKey {
    IdempotencyKey::parse(format!(
        "{label}-{}",
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ))
    .expect("valid idempotency key")
}

fn claim_ready_queue_req(
    session_token: impl Into<String>,
    lease_duration_ms: i64,
    idempotency_key: impl Into<String>,
) -> ClaimReadyQueueReq {
    ClaimReadyQueueReq::try_from_raw_parts(session_token, lease_duration_ms, idempotency_key)
        .expect("valid ready-queue claim request")
}

fn mark_in_flight_req(
    session_token: impl Into<String>,
    queue_id: impl Into<String>,
    lease_duration_ms: i64,
    idempotency_key: impl Into<String>,
) -> MarkInFlightReq {
    MarkInFlightReq::try_from_raw_parts(session_token, queue_id, lease_duration_ms, idempotency_key)
        .expect("valid mark-in-flight request")
}

fn mark_applied_req(
    session_token: impl Into<String>,
    queue_id: impl Into<String>,
    claim_fence_seq: FenceSeq,
    idempotency_key: impl Into<String>,
) -> MarkAppliedReq {
    MarkAppliedReq::try_from_raw_parts(
        session_token,
        queue_id,
        claim_fence_seq.get(),
        idempotency_key,
    )
    .expect("valid mark-applied request")
}

fn supersede_queue_item_req(
    session_token: impl Into<String>,
    queue_id: impl Into<String>,
    idempotency_key: impl Into<String>,
) -> SupersedeQueueItemReq {
    SupersedeQueueItemReq::try_from_raw_parts(session_token, queue_id, idempotency_key)
        .expect("valid supersede-queue-item request")
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
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                format!("meta for {subtask_id}"),
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");
    let actual_subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id).expect("valid meta-task id"),
            subtask_id: Some(covey::SubtaskId::parse(subtask_id).expect("valid subtask id")),
            title: SubtaskTitle::parse(format!("work {subtask_id}")).expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
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
            format!("{digest}-paths"),
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
            format!("{digest}-findings"),
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
        .enqueue_for_apply(
            EnqueueForApplyReq::try_from_raw_parts(
                orch.clone(),
                digest.to_owned(),
                work_id,
                SettlementTarget::Canonical,
                id_key("enqueue-for-apply"),
            )
            .expect("valid enqueue-for-apply request"),
        )
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
    claim_fence_seq: FenceSeq,
) {
    attest(covey, gate);
    covey
        .record_apply_verification(
            RecordApplyVerificationReq::try_from_raw_parts(
                gate.to_owned(),
                queue_id.to_owned(),
                digest.to_owned(),
                format!("review_{subtask_id}"),
                findings_digest.to_owned(),
                claim_fence_seq,
                "mutai-rs",
                format!("{digest}-verdict"),
                format!("{digest}-seal"),
                id_key("record-apply-verification"),
            )
            .expect("valid apply verification request"),
        )
        .expect("record apply verification");
}

fn attach_openspec_scope(rig: &Rig, subtask_id: &str, change_id: &str) {
    let conn = Connection::open(&rig.db_path).expect("open db");
    conn.execute(
        r#"
        INSERT INTO openspec_subtask_scope (
            subtask_id, openspec_change_id, openspec_task_id, source_path,
            scenario_refs_json, updated_at
        ) VALUES (?1, ?2, ?3, ?4, '[]', ?5)
        "#,
        params![
            subtask_id,
            change_id,
            format!("{subtask_id}-task"),
            format!("openspec/changes/{change_id}/tasks.md"),
            1_700_000_000_000_i64,
        ],
    )
    .expect("insert openspec scope");
}

fn apply_queue_item(rig: &Rig, queue_id: &str, gate_principal: &str) -> FenceSeq {
    let item = rig.covey.ready_queue_item(queue_id).expect("queue item");
    let gate = register(&rig.covey, gate_principal, SessionRole::ApplyGate);
    let claim = rig
        .covey
        .claim_next_ready_queue_item(claim_ready_queue_req(
            gate.clone(),
            30_000,
            id_key("claim-ready-queue"),
        ))
        .expect("claim ready queue")
        .expect("queue claim");
    record_apply_verification(
        &rig.covey,
        &gate,
        queue_id,
        item.subtask_id(),
        item.artifact_digest(),
        &format!("{}-findings", item.artifact_digest()),
        claim.claim_fence_seq,
    );
    rig.covey
        .mark_applied(mark_applied_req(
            gate,
            queue_id.to_owned(),
            claim.claim_fence_seq,
            id_key("mark-applied"),
        ))
        .expect("mark applied");
    claim.claim_fence_seq
}

fn record_archive_status_req(
    session_token: impl Into<String>,
    queue_id: impl Into<String>,
    artifact_digest: impl Into<String>,
    openspec_change_id: impl Into<String>,
    state: OpenSpecArchiveStatusState,
    blocked_reason: Option<&str>,
    archive_proof_digest: Option<&str>,
    idempotency_label: &str,
) -> RecordOpenSpecArchiveStatusReq {
    RecordOpenSpecArchiveStatusReq::try_from_raw_parts(
        session_token,
        queue_id,
        artifact_digest,
        openspec_change_id,
        state,
        blocked_reason.map(ToOwned::to_owned),
        archive_proof_digest.map(ToOwned::to_owned),
        id_key(idempotency_label),
    )
    .expect("valid archive status request")
}

#[rstest]
fn mark_applied_creates_openspec_archive_blocker_for_imported_subtasks(rig: Rig) {
    let (_orch, queue_id) = enqueue_ready_item(
        &rig,
        "openspec_archive_auto",
        "blake3:openspec_archive_auto",
    );
    let item = rig.covey.ready_queue_item(&queue_id).expect("queue item");
    attach_openspec_scope(&rig, item.subtask_id(), "change-auto");

    apply_queue_item(&rig, &queue_id, "gate-openspec-archive-auto");

    let blockers = rig
        .covey
        .open_openspec_archive_blockers(10)
        .expect("archive blockers");
    assert_eq!(blockers.len(), 1);
    let blocker = &blockers[0];
    assert_eq!(blocker.queue_id(), queue_id);
    assert_eq!(blocker.subtask_id(), item.subtask_id());
    assert_eq!(blocker.artifact_digest(), item.artifact_digest());
    assert_eq!(blocker.openspec_change_id(), "change-auto");
    assert_eq!(blocker.state, OpenSpecArchiveStatusState::Blocked);
    assert_eq!(
        blocker.blocked_reason.as_ref().map(AsRef::as_ref),
        Some("applied_but_unarchived")
    );
    assert!(blocker.archive_proof_digest.is_none());
    assert!(
        rig.covey
            .fetch_events(0, 100)
            .expect("events")
            .into_iter()
            .filter_map(|event| event.typed().ok())
            .any(|event| matches!(
                event.payload,
                EventPayload::OpenSpecArchiveStatusRecorded(_)
            ))
    );
}

#[rstest]
fn mark_applied_does_not_create_archive_blocker_for_non_openspec_work(rig: Rig) {
    let (_orch, queue_id) =
        enqueue_ready_item(&rig, "non_openspec_archive", "blake3:non_openspec_archive");

    apply_queue_item(&rig, &queue_id, "gate-non-openspec-archive");

    assert_eq!(
        rig.covey
            .open_openspec_archive_blockers(10)
            .expect("archive blockers"),
        []
    );
}

#[rstest]
fn record_openspec_archive_status_rejects_invalid_targets_and_roles(rig: Rig) {
    let (orch, queue_id) = enqueue_ready_item(
        &rig,
        "openspec_archive_reject",
        "blake3:openspec_archive_reject",
    );
    let item = rig.covey.ready_queue_item(&queue_id).expect("queue item");
    attach_openspec_scope(&rig, item.subtask_id(), "change-reject");

    let non_applied = rig
        .covey
        .record_openspec_archive_status(record_archive_status_req(
            orch.clone(),
            queue_id.clone(),
            item.artifact_digest().to_owned(),
            "change-reject",
            OpenSpecArchiveStatusState::Blocked,
            Some("applied_but_unarchived"),
            None,
            "archive-non-applied",
        ));
    assert!(matches!(
        non_applied,
        Err(CoveyError::ApplyGateEvidenceMissing { .. })
    ));

    apply_queue_item(&rig, &queue_id, "gate-openspec-archive-reject");
    let worker = register(&rig.covey, "worker-archive-status", SessionRole::Executor);
    let wrong_role = rig
        .covey
        .record_openspec_archive_status(record_archive_status_req(
            worker,
            queue_id.clone(),
            item.artifact_digest().to_owned(),
            "change-reject",
            OpenSpecArchiveStatusState::Archived,
            None,
            Some("blake3:archive_reject_proof"),
            "archive-wrong-role",
        ));
    assert!(matches!(wrong_role, Err(CoveyError::WrongRole { .. })));

    let mismatch = rig
        .covey
        .record_openspec_archive_status(record_archive_status_req(
            orch,
            queue_id,
            "blake3:wrong_artifact",
            "change-reject",
            OpenSpecArchiveStatusState::Archived,
            None,
            Some("blake3:archive_reject_proof"),
            "archive-artifact-mismatch",
        ));
    assert!(matches!(
        mismatch,
        Err(CoveyError::ApplyGateEvidenceMissing { .. })
    ));

    assert!(
        RecordOpenSpecArchiveStatusReq::try_from_raw_parts(
            "session-token",
            "queue-id",
            "blake3:artifact",
            "change-reject",
            OpenSpecArchiveStatusState::Blocked,
            None,
            None,
            "invalid-combo",
        )
        .is_err()
    );
    assert!(
        RecordOpenSpecArchiveStatusReq::try_from_raw_parts(
            "session-token",
            "queue-id",
            "blake3:artifact",
            "change-reject",
            OpenSpecArchiveStatusState::Archived,
            Some("still blocked".to_owned()),
            Some("blake3:proof".to_owned()),
            "invalid-combo",
        )
        .is_err()
    );
}

#[rstest]
fn archive_status_receipts_are_idempotent_and_divergent_receipts_fail(rig: Rig) {
    let (orch, queue_id) = enqueue_ready_item(
        &rig,
        "openspec_archive_idem",
        "blake3:openspec_archive_idem",
    );
    let item = rig.covey.ready_queue_item(&queue_id).expect("queue item");
    attach_openspec_scope(&rig, item.subtask_id(), "change-idem");
    apply_queue_item(&rig, &queue_id, "gate-openspec-archive-idem");

    let blocked = rig
        .covey
        .record_openspec_archive_status(record_archive_status_req(
            orch.clone(),
            queue_id.clone(),
            item.artifact_digest().to_owned(),
            "change-idem",
            OpenSpecArchiveStatusState::Blocked,
            Some("applied_but_unarchived"),
            None,
            "archive-blocked-replay",
        ))
        .expect("replay blocked receipt");
    assert_eq!(blocked.state, OpenSpecArchiveStatusState::Blocked);

    let archived = rig
        .covey
        .record_openspec_archive_status(record_archive_status_req(
            orch.clone(),
            queue_id.clone(),
            item.artifact_digest().to_owned(),
            "change-idem",
            OpenSpecArchiveStatusState::Archived,
            None,
            Some("blake3:archive_idem_proof"),
            "archive-archived",
        ))
        .expect("record archive proof");
    assert_eq!(archived.state, OpenSpecArchiveStatusState::Archived);
    assert_eq!(
        rig.covey
            .open_openspec_archive_blockers(10)
            .expect("archive blockers"),
        []
    );

    let archived_replay = rig
        .covey
        .record_openspec_archive_status(record_archive_status_req(
            orch.clone(),
            queue_id.clone(),
            item.artifact_digest().to_owned(),
            "change-idem",
            OpenSpecArchiveStatusState::Archived,
            None,
            Some("blake3:archive_idem_proof"),
            "archive-archived-replay",
        ))
        .expect("replay archive proof");
    assert_eq!(archived_replay.state, OpenSpecArchiveStatusState::Archived);

    let divergent = rig
        .covey
        .record_openspec_archive_status(record_archive_status_req(
            orch,
            queue_id,
            item.artifact_digest().to_owned(),
            "change-idem",
            OpenSpecArchiveStatusState::Archived,
            None,
            Some("blake3:different_archive_proof"),
            "archive-divergent",
        ));
    assert!(matches!(
        divergent,
        Err(CoveyError::IllegalTransition { .. })
    ));
}

#[rstest]
fn ready_queue_alternate_paths_cover_fetch_claim_supersede_and_owner_errors(rig: Rig) {
    let (_orch, queue_id) = enqueue_ready_item(&rig, "queue_cover", "blake3:queue_cover");
    let gate = register(&rig.covey, "gate-queue-cover", SessionRole::ApplyGate);
    let other_gate = register(&rig.covey, "gate-queue-other", SessionRole::ApplyGate);

    let queued = rig.covey.fetch_ready_queue(10).expect("fetch queue");
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].queue_id(), queue_id);
    assert_eq!(
        rig.covey.fetch_ready_queue(0).expect("fetch zero limit"),
        []
    );

    let claim = rig
        .covey
        .claim_next_ready_queue_item(claim_ready_queue_req(
            gate.clone(),
            30_000,
            id_key("claim-ready-queue"),
        ))
        .expect("claim queue")
        .expect("queue claim");
    assert_eq!(claim.queue_id, queue_id);
    assert!(
        rig.covey
            .claim_next_ready_queue_item(claim_ready_queue_req(
                gate.clone(),
                30_000,
                id_key("claim-ready-empty"),
            ))
            .expect("empty queue claim")
            .is_none()
    );

    assert!(matches!(
        rig.covey.mark_applied(mark_applied_req(
            other_gate,
            queue_id.clone(),
            claim.claim_fence_seq,
            id_key("mark-applied-wrong-owner"),
        )),
        Err(CoveyError::NotQueueClaimOwner { .. })
    ));
    assert!(matches!(
        rig.covey.mark_applied(mark_applied_req(
            gate.clone(),
            queue_id.clone(),
            claim.claim_fence_seq + 1,
            id_key("mark-applied-stale-fence"),
        )),
        Err(CoveyError::StaleFenceToken { .. })
    ));

    rig.covey
        .supersede_queue_item(supersede_queue_item_req(
            gate,
            queue_id.clone(),
            id_key("supersede-in-flight"),
        ))
        .expect("supersede in-flight item");
    assert_eq!(rig.covey.fetch_ready_queue(10).expect("fetch after"), []);
}

#[rstest]
fn claim_next_ready_queue_skips_invalid_head_without_materializing_queue(rig: Rig) {
    let (_first_orch, first_queue_id) =
        enqueue_ready_item(&rig, "queue_invalid_head", "blake3:queue_invalid_head");
    rig.clock.advance(1);
    let (_second_orch, second_queue_id) =
        enqueue_ready_item(&rig, "queue_valid_tail", "blake3:queue_valid_tail");
    let gate = register(
        &rig.covey,
        "gate-queue-skip-invalid",
        SessionRole::ApplyGate,
    );

    let conn = Connection::open(&rig.db_path).expect("open db");
    conn.execute(
        "UPDATE subtasks SET state = ?2 WHERE subtask_id = ?1",
        params!["queue_invalid_head", SubtaskState::Approved.to_string(),],
    )
    .expect("invalidate first queued subtask");

    let claim = rig
        .covey
        .claim_next_ready_queue_item(claim_ready_queue_req(
            gate,
            30_000,
            id_key("claim-ready-skip-invalid-head"),
        ))
        .expect("claim queue")
        .expect("queue claim");
    assert_eq!(claim.queue_id, second_queue_id);

    let first_state: String = conn
        .query_row(
            "SELECT state FROM ready_queue WHERE queue_id = ?1",
            params![first_queue_id],
            |row| row.get(0),
        )
        .expect("first queue state");
    assert_eq!(first_state, covey::ReadyQueueState::Superseded.to_string());
}

#[rstest]
fn mark_in_flight_and_lease_expiry_paths_are_observable(rig: Rig) {
    let (_orch, queue_id) = enqueue_ready_item(&rig, "queue_expiry", "blake3:queue_expiry");
    let gate = register(&rig.covey, "gate-queue-expiry", SessionRole::ApplyGate);

    let claim = rig
        .covey
        .mark_in_flight(mark_in_flight_req(
            gate.clone(),
            queue_id.clone(),
            1,
            id_key("mark-in-flight"),
        ))
        .expect("mark in flight");
    assert_eq!(claim.claim_fence_seq, 1);

    rig.clock.advance(2);
    assert!(matches!(
        rig.covey.mark_applied(mark_applied_req(
            gate,
            queue_id,
            claim.claim_fence_seq,
            id_key("mark-applied-expired"),
        )),
        Err(CoveyError::LeaseExpired { .. })
    ));
}

#[rstest]
fn ready_queue_error_and_metrics_paths_are_observable(rig: Rig) {
    let (orch, work_id) = seed_work(&rig.covey, "queue_error_paths");
    prepare_approved_artifact(&rig, &work_id, &orch, "blake3:queue_error_paths");

    assert!(matches!(
        rig.covey.enqueue_for_apply(
            EnqueueForApplyReq::try_from_raw_parts(
                orch.clone(),
                "blake3:different".into(),
                work_id.clone(),
                SettlementTarget::Canonical,
                id_key("enqueue-digest-mismatch")
            )
            .expect("valid enqueue-for-apply request")
        ),
        Err(CoveyError::IllegalTransition { .. })
    ));

    let queue_id = rig
        .covey
        .enqueue_for_apply(
            EnqueueForApplyReq::try_from_raw_parts(
                orch.clone(),
                "blake3:queue_error_paths".into(),
                work_id,
                SettlementTarget::Canonical,
                id_key("enqueue-for-apply"),
            )
            .expect("valid enqueue-for-apply request"),
        )
        .expect("enqueue should succeed");
    let metrics = rig.covey.ready_queue_metrics().expect("queue metrics");
    assert_eq!(metrics.queued_count(), 1);
    assert_eq!(metrics.in_flight_count(), 0);
    assert!(metrics.oldest_queued_age_ms().is_some());

    assert!(matches!(
        rig.covey.claim_next_ready_queue_item(claim_ready_queue_req(
            orch.clone(),
            30_000,
            id_key("claim-ready-wrong-role"),
        )),
        Err(CoveyError::WrongRole { .. })
    ));

    assert!(matches!(
        rig.covey.mark_applied(mark_applied_req(
            orch.clone(),
            queue_id.clone(),
            FenceSeq::parse(1).expect("valid fence"),
            id_key("mark-applied-queued"),
        )),
        Err(CoveyError::WrongRole { .. })
    ));

    let gate = register(&rig.covey, "gate-queue-error-paths", SessionRole::ApplyGate);
    attest(&rig.covey, &gate);
    assert!(matches!(
        rig.covey.mark_applied(mark_applied_req(
            gate.clone(),
            queue_id.clone(),
            FenceSeq::parse(1).expect("valid fence"),
            id_key("mark-applied-not-in-flight"),
        )),
        Err(CoveyError::IllegalTransition { .. })
    ));

    let claim = rig
        .covey
        .mark_in_flight(mark_in_flight_req(
            gate.clone(),
            queue_id.clone(),
            30_000,
            id_key("mark-in-flight"),
        ))
        .expect("mark in flight should claim queue item");
    let metrics = rig.covey.ready_queue_metrics().expect("queue metrics");
    assert_eq!(metrics.queued_count(), 0);
    assert_eq!(metrics.in_flight_count(), 1);
    assert!(metrics.oldest_in_flight_age_ms().is_some());

    assert!(matches!(
        rig.covey.mark_applied(mark_applied_req(
            gate.clone(),
            queue_id.clone(),
            claim.claim_fence_seq,
            id_key("mark-applied-missing-verification"),
        )),
        Err(CoveyError::ApplyGateEvidenceMissing { .. })
    ));
    record_apply_verification(
        &rig.covey,
        &gate,
        &queue_id,
        "queue_error_paths",
        "blake3:queue_error_paths",
        "blake3:queue_error_paths-findings",
        claim.claim_fence_seq,
    );

    rig.covey
        .mark_applied(mark_applied_req(
            gate.clone(),
            queue_id.clone(),
            claim.claim_fence_seq,
            id_key("mark-applied-success"),
        ))
        .expect("mark applied should settle queue item");

    assert!(matches!(
        rig.covey.supersede_queue_item(supersede_queue_item_req(
            gate,
            queue_id,
            id_key("supersede-applied"),
        )),
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
        .mark_in_flight(mark_in_flight_req(
            gate.clone(),
            queue_id.clone(),
            30_000,
            id_key("mark-in-flight"),
        ))
        .expect("mark in flight");

    assert!(matches!(
        rig.covey.record_apply_verification(
            RecordApplyVerificationReq::try_from_raw_parts(
                gate.clone(),
                queue_id,
                "blake3:queue_requires_runtime_attestation",
                "review_queue_requires_runtime_attestation",
                "blake3:queue_requires_runtime_attestation-findings",
                claim.claim_fence_seq,
                "mutai-rs",
                "blake3:queue_requires_runtime_attestation-verdict",
                "blake3:queue_requires_runtime_attestation-seal",
                id_key("record-apply-verification"),
            )
            .expect("valid apply verification request"),
        ),
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
        .mark_in_flight(mark_in_flight_req(
            gate.clone(),
            queue_id.clone(),
            30_000,
            id_key("mark-in-flight"),
        ))
        .expect("mark in flight");

    assert!(matches!(
        rig.covey.record_apply_verification(
            RecordApplyVerificationReq::try_from_raw_parts(
                gate,
                queue_id,
                digest,
                format!("review_{subtask_id}"),
                format!("{digest}-findings"),
                claim.claim_fence_seq,
                "mutai-rs",
                format!("{digest}-verdict"),
                format!("{digest}-seal"),
                id_key("record-apply-verification"),
            )
            .expect("valid apply verification request"),
        ),
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
        .mark_in_flight(mark_in_flight_req(
            gate.clone(),
            queue_id.clone(),
            30_000,
            id_key("mark-in-flight"),
        ))
        .expect("mark in flight");

    assert!(matches!(
        rig.covey.record_apply_verification(
            RecordApplyVerificationReq::try_from_raw_parts(
                gate,
                queue_id,
                digest,
                format!("review_{subtask_id}"),
                format!("{digest}-findings"),
                claim.claim_fence_seq,
                "mutai-rs",
                format!("{digest}-verdict"),
                format!("{digest}-seal"),
                id_key("record-apply-verification"),
            )
            .expect("valid apply verification request"),
        ),
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
        .mark_in_flight(mark_in_flight_req(
            gate.clone(),
            queue_id.clone(),
            30_000,
            id_key("mark-in-flight"),
        ))
        .expect("mark in flight");

    assert!(matches!(
        rig.covey.record_apply_verification(
            RecordApplyVerificationReq::try_from_raw_parts(
                gate,
                queue_id,
                digest,
                format!("review_{subtask_id}"),
                format!("{digest}-findings"),
                claim.claim_fence_seq,
                "mutai-rs",
                format!("{digest}-verdict"),
                format!("{digest}-seal"),
                id_key("record-apply-verification"),
            )
            .expect("valid apply verification request"),
        ),
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
        .enqueue_for_apply(
            EnqueueForApplyReq::try_from_raw_parts(
                orch.clone(),
                "blake3:queue_evidence_required".into(),
                work_id,
                SettlementTarget::Canonical,
                id_key("enqueue-for-apply"),
            )
            .expect("valid enqueue-for-apply request"),
        )
        .expect("enqueue without review evidence");
    let claim = rig
        .covey
        .mark_in_flight(mark_in_flight_req(
            gate.clone(),
            queue_id.clone(),
            30_000,
            id_key("mark-in-flight"),
        ))
        .expect("claim queue item");

    assert!(matches!(
        rig.covey.mark_applied(mark_applied_req(
            gate,
            queue_id,
            claim.claim_fence_seq,
            id_key("mark-applied"),
        )),
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
        .enqueue_for_apply(
            EnqueueForApplyReq::try_from_raw_parts(
                orch,
                "blake3:queue_apply_gate_separation".into(),
                work_id,
                SettlementTarget::Canonical,
                id_key("enqueue-for-apply"),
            )
            .expect("valid enqueue-for-apply request"),
        )
        .expect("enqueue with review evidence");
    let claim = rig
        .covey
        .mark_in_flight(mark_in_flight_req(
            gate.clone(),
            queue_id.clone(),
            30_000,
            id_key("mark-in-flight"),
        ))
        .expect("claim queue item");

    assert!(matches!(
        rig.covey.mark_applied(mark_applied_req(
            gate,
            queue_id,
            claim.claim_fence_seq,
            id_key("mark-applied"),
        )),
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
        .mark_in_flight(mark_in_flight_req(
            gate.clone(),
            queue_id.clone(),
            30_000,
            id_key("mark-in-flight"),
        ))
        .expect("claim queue item");
    record_apply_verification(
        &rig.covey,
        &gate,
        &queue_id,
        subtask_id,
        digest,
        &format!("{digest}-findings"),
        claim.claim_fence_seq,
    );
    rig.covey
        .mark_applied(mark_applied_req(
            gate.clone(),
            queue_id.clone(),
            claim.claim_fence_seq,
            id_key("mark-applied"),
        ))
        .expect("mark applied");

    let status = rig
        .covey
        .verify_landing_authorization(
            VerifyLandingAuthorizationReq::try_from_raw_parts(
                gate.clone(),
                queue_id.clone(),
                digest,
                format!("review_{subtask_id}"),
                format!("{digest}-findings"),
                claim.claim_fence_seq,
                "mutai-rs",
                format!("{digest}-verdict"),
                format!("{digest}-seal"),
            )
            .expect("valid landing authorization request"),
        )
        .expect("verify landing authorization");
    assert!(status.accepted_flag());
    assert_eq!(status.recorded_by_session().as_str(), gate);

    assert!(matches!(
        rig.covey.verify_landing_authorization(
            VerifyLandingAuthorizationReq::try_from_raw_parts(
                status.recorded_by_session().to_string(),
                queue_id,
                digest,
                format!("review_{subtask_id}"),
                format!("{digest}-findings"),
                claim.claim_fence_seq,
                "mutai-rs",
                format!("{digest}-verdict"),
                "blake3:wrong-apply-verification-seal",
            )
            .expect("valid landing authorization request"),
        ),
        Err(CoveyError::ApplyGateEvidenceMissing { reason, .. })
            if reason == "accepted apply verifier verdict does not match landing authorization"
    ));
}

#[rstest]
fn landing_receipt_records_landed_commit_before_apply_complete(rig: Rig) {
    let subtask_id = "queue_landing_receipt_in_flight";
    let digest = "blake3:queue_landing_receipt_in_flight";
    let (_orch, queue_id) = enqueue_ready_item(&rig, subtask_id, digest);
    let gate = register(
        &rig.covey,
        "gate-landing-receipt-in-flight",
        SessionRole::ApplyGate,
    );
    let claim = rig
        .covey
        .mark_in_flight(mark_in_flight_req(
            gate.clone(),
            queue_id.clone(),
            30_000,
            id_key("mark-in-flight"),
        ))
        .expect("claim queue item");
    record_apply_verification(
        &rig.covey,
        &gate,
        &queue_id,
        subtask_id,
        digest,
        &format!("{digest}-findings"),
        claim.claim_fence_seq,
    );

    let status = rig
        .covey
        .verify_landing_authorization(
            VerifyLandingAuthorizationReq::try_from_raw_parts(
                gate.clone(),
                queue_id.clone(),
                digest,
                format!("review_{subtask_id}"),
                format!("{digest}-findings"),
                claim.claim_fence_seq,
                "mutai-rs",
                format!("{digest}-verdict"),
                format!("{digest}-seal"),
            )
            .expect("valid landing authorization request"),
        )
        .expect("in-flight landing authorization should verify after apply evidence");
    assert!(status.accepted_flag());

    rig.covey
        .record_landing_receipt(
            RecordLandingReceiptReq::try_from_raw_parts(
                gate.clone(),
                queue_id.clone(),
                digest,
                claim.claim_fence_seq,
                "origin/main",
                "0123456789abcdef0123456789abcdef01234567",
            )
            .expect("valid landing receipt request"),
        )
        .expect("record in-flight landing receipt");

    rig.covey
        .mark_applied(mark_applied_req(
            gate,
            queue_id.clone(),
            claim.claim_fence_seq,
            id_key("mark-applied"),
        ))
        .expect("mark applied");

    let conn = Connection::open(&rig.db_path).expect("open db");
    let receipt: (String, String, String) = conn
        .query_row(
            "SELECT artifact_digest, target_ref, landed_commit_oid FROM landing_receipts WHERE queue_id = ?1",
            params![queue_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("landing receipt row");
    assert_eq!(receipt.0, digest);
    assert_eq!(receipt.1, "origin/main");
    assert_eq!(receipt.2, "0123456789abcdef0123456789abcdef01234567");
}

#[rstest]
fn landing_receipt_records_landed_commit_after_apply(rig: Rig) {
    let subtask_id = "queue_landing_receipt";
    let digest = "blake3:queue_landing_receipt";
    let (_orch, queue_id) = enqueue_ready_item(&rig, subtask_id, digest);
    let gate = register(&rig.covey, "gate-landing-receipt", SessionRole::ApplyGate);
    let claim = rig
        .covey
        .mark_in_flight(mark_in_flight_req(
            gate.clone(),
            queue_id.clone(),
            30_000,
            id_key("mark-in-flight"),
        ))
        .expect("claim queue item");
    record_apply_verification(
        &rig.covey,
        &gate,
        &queue_id,
        subtask_id,
        digest,
        &format!("{digest}-findings"),
        claim.claim_fence_seq,
    );
    rig.covey
        .mark_applied(mark_applied_req(
            gate.clone(),
            queue_id.clone(),
            claim.claim_fence_seq,
            id_key("mark-applied"),
        ))
        .expect("mark applied");

    rig.covey
        .record_landing_receipt(
            RecordLandingReceiptReq::try_from_raw_parts(
                gate.clone(),
                queue_id.clone(),
                digest,
                claim.claim_fence_seq,
                "origin/main",
                "0123456789abcdef0123456789abcdef01234567",
            )
            .expect("valid landing receipt request"),
        )
        .expect("record landing receipt");
    rig.covey
        .record_landing_receipt(
            RecordLandingReceiptReq::try_from_raw_parts(
                gate.clone(),
                queue_id.clone(),
                digest,
                claim.claim_fence_seq,
                "origin/main",
                "0123456789abcdef0123456789abcdef01234567",
            )
            .expect("valid replayed landing receipt request"),
        )
        .expect("same landing receipt is idempotent");

    let divergent = rig
        .covey
        .record_landing_receipt(
            RecordLandingReceiptReq::try_from_raw_parts(
                gate.clone(),
                queue_id.clone(),
                digest,
                claim.claim_fence_seq,
                "origin/main",
                "1111111111111111111111111111111111111111",
            )
            .expect("valid divergent landing receipt request"),
        )
        .expect_err("divergent landing receipt must not rewrite settlement evidence");
    assert!(matches!(
        divergent,
        CoveyError::ApplyGateEvidenceMissing { ref reason, .. }
            if reason == "landing receipt already recorded with different target or commit"
    ));

    let conn = Connection::open(&rig.db_path).expect("open db");
    let receipt: (String, String, String) = conn
        .query_row(
            "SELECT artifact_digest, target_ref, landed_commit_oid FROM landing_receipts WHERE queue_id = ?1",
            params![queue_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("landing receipt row");
    assert_eq!(receipt.0, digest);
    assert_eq!(receipt.1, "origin/main");
    assert_eq!(receipt.2, "0123456789abcdef0123456789abcdef01234567");
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
            format!("{digest}-paths"),
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
        .request_reservation(
            RequestReservationReq::try_from_raw_parts(
                orch.clone(),
                work_id.clone(),
                ScopeClass::Subtree,
                "src/reservation",
                vec![],
                60_000,
                id_key("request-reservation"),
            )
            .expect("valid reservation request"),
        )
        .expect("request first reservation");
    let second = rig
        .covey
        .request_reservation(
            RequestReservationReq::try_from_raw_parts(
                orch.clone(),
                work_id,
                ScopeClass::ExactPath,
                "src/reservation/mod.rs",
                vec![],
                60_000,
                id_key("request-reservation"),
            )
            .expect("valid reservation request"),
        )
        .expect("request overlapping reservation");

    let overlaps = rig
        .covey
        .find_overlapping_reservations(
            OverlapQueryReq::try_from_parts(
                ScopeClass::GeneratedSet,
                "generated/query",
                vec!["src/reservation/mod.rs".into()],
            )
            .expect("valid overlap query"),
        )
        .expect("find generated overlap");
    assert!(
        overlaps
            .iter()
            .any(|reservation| reservation.reservation_id == first)
    );

    let conflicts = rig.covey.list_conflicts().expect("list conflicts");
    assert!(!conflicts.is_empty());
    let conflict_id = conflicts[0].conflict_id().to_string();
    rig.covey
        .resolve_conflict(
            ResolveConflictReq::try_from_raw_parts(
                orch.clone(),
                conflict_id.clone(),
                ConflictResolutionState::Acknowledged,
                id_key("resolve-conflict"),
            )
            .expect("valid conflict resolution request"),
        )
        .expect("resolve conflict");
    rig.covey
        .resolve_conflict(
            ResolveConflictReq::try_from_raw_parts(
                orch.clone(),
                conflict_id.clone(),
                ConflictResolutionState::Resolved,
                id_key("resolve-conflict-final"),
            )
            .expect("valid conflict resolution request"),
        )
        .expect("resolve acknowledged conflict");
    assert!(matches!(
        rig.covey.resolve_conflict(
            ResolveConflictReq::try_from_raw_parts(
                orch.clone(),
                conflict_id,
                ConflictResolutionState::Acknowledged,
                id_key("downgrade-resolved-conflict"),
            )
            .expect("valid conflict resolution request"),
        ),
        Err(CoveyError::IllegalTransition { .. })
    ));
    assert!(matches!(
        rig.covey.resolve_conflict(
            ResolveConflictReq::try_from_raw_parts(
                orch.clone(),
                "missing-conflict",
                ConflictResolutionState::Resolved,
                id_key("resolve-missing-conflict"),
            )
            .expect("valid conflict resolution request"),
        ),
        Err(CoveyError::ConflictNotFound)
    ));

    let renewed = rig
        .covey
        .renew_reservation(
            RenewReservationReq::try_from_raw_parts(
                orch.clone(),
                second.clone(),
                5_000,
                id_key("renew-reservation"),
            )
            .expect("valid renew reservation request"),
        )
        .expect("renew active reservation");
    assert_eq!(renewed.reservation_id, second);
    rig.covey
        .release_reservation(
            ReleaseReservationReq::try_from_raw_parts(
                orch.clone(),
                second.clone(),
                id_key("release-reservation"),
            )
            .expect("valid release reservation request"),
        )
        .expect("release reservation");
    assert!(matches!(
        rig.covey.renew_reservation(
            RenewReservationReq::try_from_raw_parts(
                orch,
                second,
                5_000,
                id_key("renew-released-reservation"),
            )
            .expect("valid renew reservation request"),
        ),
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
            .claim_next_subtask(
                ClaimNextReq::try_from_raw_parts(worker.clone(), 10_000, id_key("claim-empty"))
                    .expect("valid claim-next request"),
            )
            .expect("claim on empty queue")
            .is_none()
    );
    assert!(matches!(
        rig.covey.claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                orch.clone(),
                covey::LeaseDurationMs::parse(10_000).expect("valid lease duration"),
                id_key("claim-wrong-role"),
            )
            .expect("valid claim-next request")
        ),
        Err(CoveyError::WrongRole { .. })
    ));

    let meta_task_id = rig
        .covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "generated subtask id",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");
    let generated = rig
        .covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id.clone())
                .expect("valid meta-task id"),
            subtask_id: None,
            title: SubtaskTitle::parse("generated").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
            idempotency_key: id_key("create-generated-subtask"),
        })
        .expect("create generated subtask");
    assert!(generated.starts_with("subtask_"));
    assert!(matches!(
        rig.covey.create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id).expect("valid meta-task id"),
            subtask_id: Some(covey::SubtaskId::parse(generated).expect("valid subtask id")),
            title: SubtaskTitle::parse("duplicate").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
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
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-work-for-review"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim work")
        .expect("work claim should exist");
    assert_eq!(work_claim.subtask_id, work_id);
    rig.covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                id_key("start-work-for-review"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start work");
    let renewed = rig
        .covey
        .renew_claim(
            RenewClaimReq::try_from_raw_parts(
                worker.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                LeaseDurationMs::parse(1_000).expect("valid lease duration"),
                id_key("renew-work-claim"),
            )
            .expect("valid renew-claim request"),
        )
        .expect("renew work claim");
    assert!(renewed.lease_deadline > work_claim.lease_deadline);

    rig.covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                "blake3:artifact_review_first".into(),
                ArtifactKind::PatchBundle,
                "base-1".into(),
                "artifacts/first.json".into(),
                "blake3:paths_first".into(),
                id_key("publish-first-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish first artifact");
    let first_review_id = rig
        .covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                work_id.clone(),
                "blake3:artifact_review_first",
                Some("review_cover_first".into()),
                5,
                id_key("request-first-review"),
            )
            .expect("valid review request"),
        )
        .expect("request first review");
    assert!(!first_review_id.is_empty());

    rig.covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                "blake3:artifact_review_second".into(),
                ArtifactKind::PatchBundle,
                "base-2".into(),
                "artifacts/second.json".into(),
                "blake3:paths_second".into(),
                id_key("publish-second-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish second artifact and supersede the first review");
    let review_id = rig
        .covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                work_id.clone(),
                "blake3:artifact_review_second",
                Some("review_cover_second".into()),
                1,
                id_key("request-second-review"),
            )
            .expect("valid review request"),
        )
        .expect("request second review");

    let review_claim = rig
        .covey
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                covey::SubtaskId::parse("review_cover_second").expect("valid subtask id"),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-review-subtask"),
            )
            .expect("valid claim-subtask request"),
        )
        .expect("claim review");
    rig.covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                review_claim.claim_id.clone(),
                review_claim.fence_seq,
                id_key("start-review-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start review");
    rig.covey
        .decide_review(
            DecideReviewReq::try_from_raw_parts(
                reviewer,
                review_id,
                review_claim.claim_id,
                review_claim.fence_seq,
                ReviewVerdict::Approve,
                "blake3:review_findings".into(),
                id_key("decide-review"),
            )
            .expect("valid review decision request"),
        )
        .expect("decide review");

    rig.covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                worker,
                work_claim.claim_id,
                work_claim.fence_seq,
                id_key("release-approved-work-claim"),
            )
            .expect("valid release-claim request"),
        )
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
                .claim_next_ready_queue_item(claim_ready_queue_req(
                    gate.clone(),
                    1,
                    id_key("claim-ready-prop"),
                ))
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

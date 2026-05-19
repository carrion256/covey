use super::{
    AbandonSubtaskReq, ActorKind, ArtifactDigest, ArtifactKind, CancelMetaTaskReq, ClaimId,
    ClaimResult, ConflictResolutionState, CreateSubtaskRequest, DecideReviewReq,
    EnqueueForApplyReq, Event, EventPayload, EventType, ExitSessionReq, ExpiredCountPayload,
    HeartbeatReq, ImportOpenSpecAction, ImportOpenSpecEvent, LeaseDeadlineMs, MarkAppliedReq,
    MetaTaskId, ObjectType, OpenSpecImportProvenance, OpenSpecSourceDigest, PublishArtifactReq,
    ReadyQueueClaim, ReadyQueueItem, ReadyQueueState, ReleaseClaimReq, RequestReservationReq,
    RequestReviewReq, Reservation, ReservationId, ReservationState, ResolveConflictReq, Review,
    ReviewTarget, ReviewVerdict, ScopeClass, SessionHandle, SessionRole, SessionToken,
    SettlementTarget, StaleSessionsPayload, StartSubtaskReq, SubmitMetaTaskReq, Subtask, SubtaskId,
    SubtaskKind, SubtaskRow, SubtaskState, SubtaskView, SupersedeQueueItemReq, TimestampMs,
    bd_import_v1_subtask_id, make_id, parse_generated_members,
};
use crate::CoveyError;
use serde::Serialize;

#[test]
fn make_id_uses_prefix_and_uuid_suffix() {
    let id = make_id("claim");

    assert!(id.starts_with("claim_"));
    assert_eq!(id.len(), "claim_".len() + 32);
    assert!(
        id["claim_".len()..]
            .chars()
            .all(|ch| ch.is_ascii_hexdigit())
    );
}

#[test]
fn bd_import_v1_subtask_id_is_deterministic_and_traceable() {
    let first = bd_import_v1_subtask_id("ISSUE-42/fix import semantics");
    let second = bd_import_v1_subtask_id("ISSUE-42/fix import semantics");
    let different = bd_import_v1_subtask_id("ISSUE-43/fix import semantics");

    assert_eq!(first, second);
    assert_ne!(first, different);
    assert!(first.starts_with("bdwork_issue_42_fix_import_"));
}

#[test]
fn parse_generated_members_decodes_string_arrays() {
    let raw = serde_json::to_string(&vec!["src/lib.rs", "src/main.rs"])
        .expect("vector serialization must succeed");

    let parsed = parse_generated_members(&raw).expect("array payload must parse");

    assert_eq!(parsed, vec!["src/lib.rs", "src/main.rs"]);
}

#[test]
fn parse_generated_members_rejects_non_array_payloads() {
    let raw = serde_json::to_string("src/lib.rs").expect("string serialization must succeed");

    let err = parse_generated_members(&raw).expect_err("scalar payload must be rejected");

    assert!(
        err.to_string().contains("sequence"),
        "unexpected error message: {err}"
    );
}

#[test]
fn subtask_row_converts_to_domain_and_view_without_leaking_row_shape() {
    let row = SubtaskRow {
        subtask_id: SubtaskId::parse("subtask-1").expect("valid subtask id"),
        meta_task_id: MetaTaskId::parse("meta-1").expect("valid meta-task id"),
        title: "implement".to_owned(),
        kind: SubtaskKind::Work,
        review_target_subtask_id: None,
        review_target_artifact_digest: None,
        state: SubtaskState::Claimed,
        current_claim_id: Some(ClaimId::parse("claim-1").expect("valid claim id")),
        artifact_digest: None,
        priority: 10,
        created_at: TimestampMs::parse(100).expect("valid timestamp"),
        updated_at: TimestampMs::parse(200).expect("valid timestamp"),
    };

    let domain = Subtask::try_from(row.clone()).expect("valid work row");
    assert_eq!(domain.kind(), SubtaskKind::Work);
    assert!(domain.review_target().is_none());
    assert_eq!(
        domain.lifecycle().active_claim_id().map(AsRef::as_ref),
        Some("claim-1")
    );

    let view = SubtaskView::try_from(row).expect("valid work row view");
    assert_eq!(view.kind, SubtaskKind::Work);
    assert!(view.review_target.is_none());
    assert_eq!(
        view.active_claim_id.as_ref().map(AsRef::as_ref),
        Some("claim-1")
    );
}

#[test]
fn subtask_lifecycle_requires_active_claim_for_claimed_state() {
    let row = work_subtask_row(SubtaskState::Claimed, None, None);

    let err = Subtask::try_from(row).expect_err("claimed subtask without claim must be rejected");

    assert!(
        err.to_string()
            .contains("claimed subtask is missing active claim"),
        "unexpected error: {err}"
    );
}

#[test]
fn subtask_lifecycle_rejects_stale_fields_on_available_state() {
    let row = work_subtask_row(
        SubtaskState::Available,
        Some(ClaimId::parse("claim-1").expect("valid claim id")),
        Some(ArtifactDigest::parse("blake3:artifact").expect("valid digest")),
    );

    let err = Subtask::try_from(row)
        .expect_err("available subtask with claim or artifact state must be rejected");

    assert!(
        err.to_string()
            .contains("available subtask cannot carry claim or artifact state"),
        "unexpected error: {err}"
    );
}

#[test]
fn subtask_lifecycle_requires_artifact_for_artifact_published_state() {
    let row = work_subtask_row(
        SubtaskState::ArtifactPublished,
        Some(ClaimId::parse("claim-1").expect("valid claim id")),
        None,
    );

    let err = Subtask::try_from(row)
        .expect_err("artifact-published subtask without artifact must be rejected");

    assert!(
        err.to_string()
            .contains("artifact-published subtask is missing artifact digest"),
        "unexpected error: {err}"
    );
}

#[test]
fn subtask_lifecycle_accepts_artifact_published_with_artifact() {
    let artifact_digest = ArtifactDigest::parse("blake3:artifact").expect("valid digest");
    let row = work_subtask_row(
        SubtaskState::ArtifactPublished,
        Some(ClaimId::parse("claim-1").expect("valid claim id")),
        Some(artifact_digest.clone()),
    );

    let domain = Subtask::try_from(row).expect("artifact-published row must be valid");

    assert_eq!(domain.lifecycle().state(), SubtaskState::ArtifactPublished);
    assert_eq!(domain.lifecycle().artifact_digest(), Some(&artifact_digest));
}

#[test]
fn review_subtask_domain_requires_a_target_artifact() {
    let row = SubtaskRow {
        subtask_id: SubtaskId::parse("review-1").expect("valid subtask id"),
        meta_task_id: MetaTaskId::parse("meta-1").expect("valid meta-task id"),
        title: "review artifact".to_owned(),
        kind: SubtaskKind::Review,
        review_target_subtask_id: Some(SubtaskId::parse("subtask-1").expect("valid subtask id")),
        review_target_artifact_digest: Some(
            ArtifactDigest::parse("blake3:artifact").expect("valid digest"),
        ),
        state: SubtaskState::Available,
        current_claim_id: None,
        artifact_digest: None,
        priority: 10,
        created_at: TimestampMs::parse(100).expect("valid timestamp"),
        updated_at: TimestampMs::parse(200).expect("valid timestamp"),
    };

    let domain = Subtask::try_from(row).expect("valid review row");
    assert_eq!(
        domain.review_target(),
        Some(&ReviewTarget::new(
            SubtaskId::parse("subtask-1").expect("valid subtask id"),
            ArtifactDigest::parse("blake3:artifact").expect("valid digest")
        ))
    );
}

fn work_subtask_row(
    state: SubtaskState,
    current_claim_id: Option<ClaimId>,
    artifact_digest: Option<ArtifactDigest>,
) -> SubtaskRow {
    SubtaskRow {
        subtask_id: SubtaskId::parse("subtask-1").expect("valid subtask id"),
        meta_task_id: MetaTaskId::parse("meta-1").expect("valid meta-task id"),
        title: "implement".to_owned(),
        kind: SubtaskKind::Work,
        review_target_subtask_id: None,
        review_target_artifact_digest: None,
        state,
        current_claim_id,
        artifact_digest,
        priority: 10,
        created_at: TimestampMs::parse(100).expect("valid timestamp"),
        updated_at: TimestampMs::parse(200).expect("valid timestamp"),
    }
}

#[test]
fn review_domain_rejects_decided_state_without_decision_evidence() {
    let raw = serde_json::json!({
        "review_id": "review-1",
        "subtask_id": "subtask-1",
        "artifact_digest": "blake3:artifact",
        "reviewer_session": "session-1",
        "review_subtask_id": "review-subtask-1",
        "verdict": null,
        "findings_digest": null,
        "state": "decided",
        "created_at": 100,
        "updated_at": 200
    });

    let err = serde_json::from_value::<Review>(raw)
        .expect_err("decided review without verdict and findings must be rejected");

    assert!(
        err.to_string().contains("decided reviews require verdict"),
        "unexpected error: {err}"
    );
}

#[test]
fn review_domain_rejects_open_state_with_decision_evidence() {
    let raw = serde_json::json!({
        "review_id": "review-1",
        "subtask_id": "subtask-1",
        "artifact_digest": "blake3:artifact",
        "reviewer_session": "session-1",
        "review_subtask_id": "review-subtask-1",
        "verdict": "approve",
        "findings_digest": "blake3:findings",
        "state": "requested",
        "created_at": 100,
        "updated_at": 200
    });

    let err = serde_json::from_value::<Review>(raw)
        .expect_err("requested review with decision evidence must be rejected");

    assert!(
        err.to_string()
            .contains("requested reviews cannot carry decision evidence"),
        "unexpected error: {err}"
    );
}

#[test]
fn ready_queue_domain_requires_active_claim_fields_for_in_flight_items() {
    let raw = serde_json::json!({
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "subtask_id": "subtask-1",
        "settlement_target": "canonical",
        "state": "in_flight",
        "claimed_by_session_token": null,
        "claim_fence_seq": null,
        "claim_lease_deadline": null,
        "enqueued_at": 100,
        "updated_at": 200
    });

    let err = serde_json::from_value::<ReadyQueueItem>(raw)
        .expect_err("in-flight queue item without claim fields must be rejected");

    assert!(
        err.to_string()
            .contains("in-flight ready-queue items require claimed_by_session_token"),
        "unexpected error: {err}"
    );
}

#[test]
fn ready_queue_domain_rejects_active_claim_fields_on_queued_items() {
    let raw = serde_json::json!({
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "subtask_id": "subtask-1",
        "settlement_target": "canonical",
        "state": "queued",
        "claimed_by_session_token": "session-1",
        "claim_fence_seq": 7,
        "claim_lease_deadline": 300,
        "enqueued_at": 100,
        "updated_at": 200
    });

    let err = serde_json::from_value::<ReadyQueueItem>(raw)
        .expect_err("queued item with active claim fields must be rejected");

    assert!(
        err.to_string()
            .contains("queued ready-queue items cannot carry an active claim"),
        "unexpected error: {err}"
    );
}

#[test]
fn ready_queue_domain_preserves_applied_fence_without_active_claim() {
    let raw = serde_json::json!({
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "subtask_id": "subtask-1",
        "settlement_target": "canonical",
        "state": "applied",
        "claimed_by_session_token": null,
        "claim_fence_seq": 7,
        "claim_lease_deadline": null,
        "enqueued_at": 100,
        "updated_at": 200
    });

    let item = serde_json::from_value::<ReadyQueueItem>(raw)
        .expect("applied queue item keeps the accepted fence");

    assert_eq!(item.state(), ReadyQueueState::Applied);
    assert_eq!(item.claim_fence_seq(), Some(7));
    assert_eq!(item.claimed_by_session_token(), None);
}

#[test]
fn event_payload_from_json_decodes_typed_payloads() {
    let payload = SessionHandle::new(
        "session-1".to_owned(),
        "principal-1".to_owned(),
        "instance-1".to_owned(),
        SessionRole::Executor,
    );
    let payload_json =
        serde_json::to_string(&payload).expect("session handle serialization must succeed");

    let decoded = EventPayload::from_json(EventType::SessionRegistered, &payload_json)
        .expect("matching payload must decode");

    assert_eq!(decoded, EventPayload::SessionRegistered(payload));
}

#[test]
fn event_payload_from_json_rejects_mismatched_payloads() {
    let payload = SessionHandle::new(
        "session-1".to_owned(),
        "principal-1".to_owned(),
        "instance-1".to_owned(),
        SessionRole::Executor,
    );
    let payload_json =
        serde_json::to_string(&payload).expect("session handle serialization must succeed");

    let err = EventPayload::from_json(EventType::SessionHeartbeat, &payload_json)
        .expect_err("wrong event type must fail to decode");

    assert!(matches!(err, CoveyError::SerializationError(_)));
}

#[test]
fn event_typed_decodes_payload_and_preserves_event_metadata() {
    let payload = HeartbeatReq {
        session_token: "session-1".to_owned(),
        idempotency_key: "idem-1".to_owned(),
    };
    let event = Event {
        seq: 42,
        event_type: EventType::SessionHeartbeat,
        object_type: ObjectType::Session,
        object_id: "session-1".to_owned(),
        actor_kind: ActorKind::Session,
        session_token: Some(SessionToken::parse("session-1").expect("valid session token")),
        payload_json: serde_json::to_string(&payload)
            .expect("heartbeat serialization must succeed"),
        created_at: TimestampMs::parse(1_234).expect("valid timestamp"),
    };

    let typed = event.typed().expect("event payload must decode");

    assert_eq!(typed.seq, 42);
    assert_eq!(typed.object_id, "session-1");
    assert_eq!(typed.payload, EventPayload::SessionHeartbeat(payload));
}

#[test]
fn event_typed_propagates_payload_decode_failures() {
    let event = Event {
        seq: 1,
        event_type: EventType::SessionHeartbeat,
        object_type: ObjectType::Session,
        object_id: "session-1".to_owned(),
        actor_kind: ActorKind::Session,
        session_token: Some(SessionToken::parse("session-1").expect("valid session token")),
        payload_json: "{".to_owned(),
        created_at: TimestampMs::parse(99).expect("valid timestamp"),
    };

    let err = event
        .typed()
        .expect_err("malformed payload must fail to decode");

    assert!(matches!(err, CoveyError::SerializationError(_)));
}

fn payload_json<T: Serialize>(payload: &T) -> String {
    serde_json::to_string(payload).expect("payload fixture should serialize")
}

fn sample_reservation() -> Reservation {
    Reservation {
        reservation_id: ReservationId::parse("reservation-1").expect("valid reservation id"),
        owner_subtask_id: SubtaskId::parse("subtask-1").expect("valid subtask id"),
        scope_class: ScopeClass::ExactPath,
        scope_key: "src/lib.rs".to_owned(),
        generated_members: vec![],
        lease_deadline: LeaseDeadlineMs::parse(200).expect("valid lease deadline"),
        state: ReservationState::Active,
        created_at: TimestampMs::parse(10).expect("valid timestamp"),
        updated_at: TimestampMs::parse(11).expect("valid timestamp"),
    }
}

fn sample_openspec_event() -> ImportOpenSpecEvent {
    ImportOpenSpecEvent {
        change_id: "change-1".to_owned(),
        object_type: ObjectType::Subtask,
        object_id: "subtask-1".to_owned(),
        openspec_task_id: Some("1.1".to_owned()),
        action: ImportOpenSpecAction::Created,
        provenance: OpenSpecImportProvenance {
            object_type: ObjectType::Subtask,
            object_id: "subtask-1".to_owned(),
            planning_format: "openspec".to_owned(),
            openspec_change_id: "change-1".to_owned(),
            openspec_change_path: "openspec/changes/change-1".to_owned(),
            openspec_task_id: Some("1.1".to_owned()),
            proposal_digest: Some("blake3:proposal".to_owned()),
            design_digest: Some("blake3:design".to_owned()),
            tasks_digest: "blake3:tasks".to_owned(),
            spec_digests: vec![OpenSpecSourceDigest::new(
                "specs/example/spec.md".to_owned(),
                "blake3:spec".to_owned(),
            )],
            source_digests: vec![OpenSpecSourceDigest::new(
                "tasks.md".to_owned(),
                "blake3:tasks".to_owned(),
            )],
            mission_artifact_digests: vec![],
            mission_artifacts: vec![],
            task_digest: Some("blake3:task".to_owned()),
            updated_at: TimestampMs::parse(123).expect("valid timestamp"),
        },
    }
}

#[test]
fn event_payload_from_json_decodes_every_event_type_variant() {
    let session = SessionHandle::new(
        "session-1".to_owned(),
        "principal-1".to_owned(),
        "instance-1".to_owned(),
        SessionRole::Executor,
    );
    let heartbeat = HeartbeatReq {
        session_token: "session-1".to_owned(),
        idempotency_key: "idem-heartbeat".to_owned(),
    };
    let exit = ExitSessionReq {
        session_token: "session-1".to_owned(),
        idempotency_key: "idem-exit".to_owned(),
    };
    let submit = SubmitMetaTaskReq {
        session_token: "session-1".to_owned(),
        prompt_text: "do work".to_owned(),
        idempotency_key: "idem-submit".to_owned(),
    };
    let cancel = CancelMetaTaskReq {
        session_token: "session-1".to_owned(),
        meta_task_id: "meta-1".to_owned(),
        idempotency_key: "idem-cancel".to_owned(),
    };
    let create = CreateSubtaskRequest {
        session_token: "session-1".to_owned(),
        meta_task_id: "meta-1".to_owned(),
        subtask_id: Some("subtask-1".to_owned()),
        title: "implement".to_owned(),
        priority: 10,
        idempotency_key: "idem-create".to_owned(),
    };
    let claim = ClaimResult::new("claim-1".to_owned(), "subtask-1".to_owned(), 1, 500);
    let start = StartSubtaskReq {
        session_token: "session-1".to_owned(),
        claim_id: "claim-1".to_owned(),
        fence_seq: 1,
        idempotency_key: "idem-start".to_owned(),
    };
    let release = ReleaseClaimReq {
        session_token: "session-1".to_owned(),
        claim_id: "claim-1".to_owned(),
        fence_seq: 1,
        idempotency_key: "idem-release".to_owned(),
    };
    let artifact = PublishArtifactReq {
        session_token: "session-1".to_owned(),
        claim_id: "claim-1".to_owned(),
        fence_seq: 1,
        artifact_digest: "blake3:artifact".to_owned(),
        artifact_kind: ArtifactKind::PatchBundle,
        base_rev: "base".to_owned(),
        manifest_path: "manifest.json".to_owned(),
        changed_paths_digest: "blake3:paths".to_owned(),
        idempotency_key: "idem-artifact".to_owned(),
    };
    let review_request = RequestReviewReq {
        session_token: "session-1".to_owned(),
        subtask_id: "subtask-1".to_owned(),
        artifact_digest: "blake3:artifact".to_owned(),
        review_subtask_id: Some("review-subtask-1".to_owned()),
        priority: 5,
        idempotency_key: "idem-review".to_owned(),
    };
    let review_decision = DecideReviewReq {
        session_token: "session-reviewer".to_owned(),
        review_id: "review-1".to_owned(),
        claim_id: "claim-review".to_owned(),
        fence_seq: 1,
        verdict: ReviewVerdict::Approve,
        findings_digest: "blake3:findings".to_owned(),
        idempotency_key: "idem-decision".to_owned(),
    };
    let enqueue = EnqueueForApplyReq {
        session_token: "session-1".to_owned(),
        artifact_digest: "blake3:artifact".to_owned(),
        subtask_id: "subtask-1".to_owned(),
        settlement_target: SettlementTarget::Canonical,
        idempotency_key: "idem-enqueue".to_owned(),
    };
    let queue_claim = ReadyQueueClaim::new(
        "queue-1".to_owned(),
        "blake3:artifact".to_owned(),
        "subtask-1".to_owned(),
        SettlementTarget::Canonical,
        1,
        900,
    );
    let mark_applied = MarkAppliedReq {
        session_token: "session-1".to_owned(),
        queue_id: "queue-1".to_owned(),
        claim_fence_seq: 1,
        idempotency_key: "idem-applied".to_owned(),
    };
    let reservation_request = RequestReservationReq {
        session_token: "session-1".to_owned(),
        owner_subtask_id: "subtask-1".to_owned(),
        scope_class: ScopeClass::ExactPath,
        scope_key: "src/lib.rs".to_owned(),
        generated_members: vec![],
        lease_duration_ms: 60_000,
        idempotency_key: "idem-reservation".to_owned(),
    };
    let reservation = sample_reservation();
    let resolve = ResolveConflictReq {
        session_token: "session-1".to_owned(),
        conflict_id: "conflict-1".to_owned(),
        resolution_state: ConflictResolutionState::Resolved,
        idempotency_key: "idem-resolve".to_owned(),
    };
    let stale_sessions = StaleSessionsPayload::new(2);
    let expired = ExpiredCountPayload::new(3);
    let openspec = sample_openspec_event();

    let cases = vec![
        (
            EventType::SessionRegistered,
            payload_json(&session),
            EventPayload::SessionRegistered(session),
        ),
        (
            EventType::SessionHeartbeat,
            payload_json(&heartbeat),
            EventPayload::SessionHeartbeat(heartbeat),
        ),
        (
            EventType::SessionExited,
            payload_json(&exit),
            EventPayload::SessionExited(exit),
        ),
        (
            EventType::MetaTaskSubmitted,
            payload_json(&submit),
            EventPayload::MetaTaskSubmitted(submit.clone()),
        ),
        (
            EventType::MetaTaskCancelled,
            payload_json(&cancel),
            EventPayload::MetaTaskCancelled(cancel),
        ),
        (
            EventType::SubtaskCreated,
            payload_json(&create),
            EventPayload::SubtaskCreated(create),
        ),
        (
            EventType::SubtaskClaimed,
            payload_json(&claim),
            EventPayload::SubtaskClaimed(claim.clone()),
        ),
        (
            EventType::SubtaskStarted,
            payload_json(&start),
            EventPayload::SubtaskStarted(start.clone()),
        ),
        (
            EventType::SubtaskAbandoned,
            payload_json(&start),
            EventPayload::SubtaskAbandoned(AbandonSubtaskReq {
                session_token: start.session_token.clone(),
                claim_id: start.claim_id.clone(),
                fence_seq: start.fence_seq,
                idempotency_key: start.idempotency_key.clone(),
            }),
        ),
        (
            EventType::ClaimReleased,
            payload_json(&release),
            EventPayload::ClaimReleased(release),
        ),
        (
            EventType::ClaimRenewed,
            payload_json(&claim),
            EventPayload::ClaimRenewed(claim),
        ),
        (
            EventType::ArtifactPublished,
            payload_json(&artifact),
            EventPayload::ArtifactPublished(artifact),
        ),
        (
            EventType::ReviewRequested,
            payload_json(&review_request),
            EventPayload::ReviewRequested(review_request),
        ),
        (
            EventType::ReviewDecided,
            payload_json(&review_decision),
            EventPayload::ReviewDecided(review_decision),
        ),
        (
            EventType::ReadyQueueEnqueued,
            payload_json(&enqueue),
            EventPayload::ReadyQueueEnqueued(enqueue),
        ),
        (
            EventType::ReadyQueueInFlight,
            payload_json(&queue_claim),
            EventPayload::ReadyQueueInFlight(queue_claim),
        ),
        (
            EventType::ReadyQueueApplied,
            payload_json(&mark_applied),
            EventPayload::ReadyQueueApplied(mark_applied.clone()),
        ),
        (
            EventType::ReadyQueueSuperseded,
            payload_json(&mark_applied),
            EventPayload::ReadyQueueSuperseded(SupersedeQueueItemReq {
                session_token: mark_applied.session_token.clone(),
                queue_id: mark_applied.queue_id.clone(),
                idempotency_key: mark_applied.idempotency_key.clone(),
            }),
        ),
        (
            EventType::ReservationRequested,
            payload_json(&reservation_request),
            EventPayload::ReservationRequested(reservation_request),
        ),
        (
            EventType::ReservationReleased,
            payload_json(&reservation),
            EventPayload::ReservationReleased(reservation.clone()),
        ),
        (
            EventType::ReservationRenewed,
            payload_json(&reservation),
            EventPayload::ReservationRenewed(reservation),
        ),
        (
            EventType::ConflictResolved,
            payload_json(&resolve),
            EventPayload::ConflictResolved(resolve),
        ),
        (
            EventType::SessionsReaped,
            payload_json(&stale_sessions),
            EventPayload::SessionsReaped(stale_sessions),
        ),
        (
            EventType::ClaimsExpired,
            payload_json(&expired),
            EventPayload::ClaimsExpired(expired.clone()),
        ),
        (
            EventType::ReservationsExpired,
            payload_json(&expired),
            EventPayload::ReservationsExpired(expired),
        ),
        (
            EventType::OpenSpecImported,
            payload_json(&openspec),
            EventPayload::OpenSpecImported(Box::new(openspec)),
        ),
    ];

    for (event_type, payload_json, expected) in cases {
        let decoded =
            EventPayload::from_json(event_type, &payload_json).expect("payload should decode");
        assert_eq!(decoded, expected, "{event_type:?}");
    }
}

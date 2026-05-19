use super::{
    AbandonSubtaskReq, ArtifactDigest, ArtifactKind, CancelMetaTaskReq, ClaimId, ClaimResult,
    ConflictResolutionState, CreateSubtaskRequest, DecideReviewReq, EnqueueForApplyReq, Event,
    EventPayload, EventType, ExitSessionReq, ExpiredCountPayload, HeartbeatReq,
    ImportOpenSpecAction, ImportOpenSpecConflict, ImportOpenSpecEvent, ImportOpenSpecItemResult,
    LandingAuthorizationStatus, LeaseDeadlineMs, MarkAppliedReq, MetaTaskId, ObjectType,
    OpenSpecImportProvenance, OpenSpecImportProvenanceCommon, OpenSpecSourceDigest,
    PublishArtifactReq, ReadyQueueClaim, ReadyQueueItem, ReadyQueueState, ReleaseClaimReq,
    RequestReservationReq, RequestReviewReq, Reservation, ReservationId, ReservationState,
    ResolveConflictReq, Review, ReviewTarget, ReviewVerdict, ScopeClass, Session, SessionHandle,
    SessionRole, SessionState, SessionToken, SettlementTarget, StaleSessionsPayload,
    StartSubtaskReq, SubmitMetaTaskReq, Subtask, SubtaskId, SubtaskKind, SubtaskRow, SubtaskState,
    SubtaskView, SupersedeQueueItemReq, TimestampMs, bd_import_v1_subtask_id, make_id,
    parse_generated_members,
};
use crate::CoveyError;
use serde::Serialize;
use serde_json::json;

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

fn session_json(state: SessionState, active_subtask_id: Option<SubtaskId>) -> serde_json::Value {
    json!({
        "session_token": "session-1",
        "agent_principal_id": "principal-1",
        "agent_instance_id": "instance-1",
        "role": SessionRole::Executor,
        "state": state,
        "active_subtask_id": active_subtask_id,
        "last_heartbeat_at": 1,
        "last_heartbeat_tick": 1,
        "created_at": 1,
        "updated_at": 1,
    })
}

#[test]
fn session_lifecycle_rejects_terminal_active_subtask_fields() {
    let subtask_id = SubtaskId::parse("subtask-1").expect("valid subtask id");
    let active: Session =
        serde_json::from_value(session_json(SessionState::Active, Some(subtask_id.clone())))
            .expect("active session may carry an active subtask");
    assert_eq!(active.state(), SessionState::Active);
    assert_eq!(active.active_subtask_id(), Some(&subtask_id));

    for state in [SessionState::Stale, SessionState::Exited] {
        let err = serde_json::from_value::<Session>(session_json(state, Some(subtask_id.clone())))
            .expect_err("inactive session with active subtask must be rejected");
        assert!(
            err.to_string()
                .contains("session must not include active_subtask_id"),
            "unexpected error: {err}"
        );
    }
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
    assert_eq!(view.kind(), SubtaskKind::Work);
    assert!(view.review_target().is_none());
    assert_eq!(view.active_claim_id().map(AsRef::as_ref), Some("claim-1"));
}

#[test]
fn subtask_view_rejects_invalid_lifecycle_shapes() {
    let available_with_claim = json!({
        "subtask_id": "subtask-1",
        "meta_task_id": "meta-1",
        "title": "implement",
        "kind": "work",
        "review_target": null,
        "state": "available",
        "active_claim_id": "claim-1",
        "artifact_digest": null,
        "priority": 10,
        "created_at": 100,
        "updated_at": 200
    });
    let err = serde_json::from_value::<SubtaskView>(available_with_claim)
        .expect_err("available view must not carry active claim state");
    assert!(
        err.to_string()
            .contains("available subtask cannot carry claim or artifact state"),
        "unexpected error: {err}"
    );

    let claimed_without_claim = json!({
        "subtask_id": "subtask-1",
        "meta_task_id": "meta-1",
        "title": "implement",
        "kind": "work",
        "review_target": null,
        "state": "claimed",
        "active_claim_id": null,
        "artifact_digest": null,
        "priority": 10,
        "created_at": 100,
        "updated_at": 200
    });
    let err = serde_json::from_value::<SubtaskView>(claimed_without_claim)
        .expect_err("claimed view requires active claim state");
    assert!(
        err.to_string()
            .contains("claimed subtask is missing active claim"),
        "unexpected error: {err}"
    );

    let work_with_review_target = json!({
        "subtask_id": "subtask-1",
        "meta_task_id": "meta-1",
        "title": "implement",
        "kind": "work",
        "review_target": {
            "subtask_id": "subtask-2",
            "artifact_digest": "blake3:artifact"
        },
        "state": "available",
        "active_claim_id": null,
        "artifact_digest": null,
        "priority": 10,
        "created_at": 100,
        "updated_at": 200
    });
    let err = serde_json::from_value::<SubtaskView>(work_with_review_target)
        .expect_err("work view must not carry a review target");
    assert!(
        err.to_string()
            .contains("work subtask view cannot carry review target"),
        "unexpected error: {err}"
    );

    let review_without_target = json!({
        "subtask_id": "review-1",
        "meta_task_id": "meta-1",
        "title": "review",
        "kind": "review",
        "review_target": null,
        "state": "available",
        "active_claim_id": null,
        "artifact_digest": null,
        "priority": 10,
        "created_at": 100,
        "updated_at": 200
    });
    let err = serde_json::from_value::<SubtaskView>(review_without_target)
        .expect_err("review view requires a review target");
    assert!(
        err.to_string()
            .contains("review subtask view is missing review target"),
        "unexpected error: {err}"
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
fn landing_authorization_status_rejects_unaccepted_flat_json() {
    let raw = serde_json::json!({
        "accepted": false,
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "review_id": "review-1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 7,
        "verifier": "mutai-rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "blake3:seal",
        "recorded_by_session": "session-1"
    });

    let err = serde_json::from_value::<LandingAuthorizationStatus>(raw)
        .expect_err("landing authorization status cannot represent rejected checks");

    assert!(
        err.to_string()
            .contains("landing authorization status is only emitted for accepted checks"),
        "unexpected error: {err}"
    );
}

#[test]
fn landing_authorization_status_preserves_accepted_flat_json() {
    let raw = serde_json::json!({
        "accepted": true,
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "review_id": "review-1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 7,
        "verifier": "mutai-rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "blake3:seal",
        "recorded_by_session": "session-1"
    });

    let status = serde_json::from_value::<LandingAuthorizationStatus>(raw)
        .expect("accepted landing authorization status should deserialize");
    let value = serde_json::to_value(&status).expect("status should serialize");

    assert!(status.accepted_flag());
    assert_eq!(status.queue_id().as_str(), "queue-1");
    assert_eq!(status.claim_fence_seq(), 7);
    assert_eq!(status.recorded_by_session().as_str(), "session-1");
    assert_eq!(value["accepted"], true);
    assert_eq!(value["queue_id"], "queue-1");
}

#[test]
fn reservation_scope_rejects_invalid_scope_shapes() {
    let exact_with_generated_members = serde_json::json!({
        "reservation_id": "reservation-1",
        "owner_subtask_id": "subtask-1",
        "scope_class": "exact_path",
        "scope_key": "src/lib.rs",
        "generated_members": ["generated.rs"],
        "lease_deadline": 200,
        "state": "active",
        "created_at": 10,
        "updated_at": 11
    });
    let err = serde_json::from_value::<Reservation>(exact_with_generated_members)
        .expect_err("exact-path reservation with generated members must be rejected");
    assert!(
        err.to_string()
            .contains("exact_path reservations must not include generated_members"),
        "unexpected error: {err}"
    );

    let repo_global_wrong_key = serde_json::json!({
        "reservation_id": "reservation-1",
        "owner_subtask_id": "subtask-1",
        "scope_class": "repo_global",
        "scope_key": "src",
        "generated_members": [],
        "lease_deadline": 200,
        "state": "active",
        "created_at": 10,
        "updated_at": 11
    });
    let err = serde_json::from_value::<Reservation>(repo_global_wrong_key)
        .expect_err("repo-global reservation must use canonical scope key");
    assert!(
        err.to_string()
            .contains("repo-global reservations require scope_key `repo`"),
        "unexpected error: {err}"
    );

    let generated_set_without_members = serde_json::json!({
        "reservation_id": "reservation-1",
        "owner_subtask_id": "subtask-1",
        "scope_class": "generated_set",
        "scope_key": "artifact-manifest",
        "generated_members": [],
        "lease_deadline": 200,
        "state": "active",
        "created_at": 10,
        "updated_at": 11
    });
    let err = serde_json::from_value::<Reservation>(generated_set_without_members)
        .expect_err("generated-set reservation without members must be rejected");
    assert!(
        err.to_string()
            .contains("generated-set reservations require generated_members"),
        "unexpected error: {err}"
    );

    let valid_generated_set: Reservation = serde_json::from_value(serde_json::json!({
        "reservation_id": "reservation-1",
        "owner_subtask_id": "subtask-1",
        "scope_class": "generated_set",
        "scope_key": "artifact-manifest",
        "generated_members": ["src/generated.rs"],
        "lease_deadline": 200,
        "state": "active",
        "created_at": 10,
        "updated_at": 11
    }))
    .expect("valid generated-set reservation should deserialize");
    assert_eq!(valid_generated_set.scope_class(), ScopeClass::GeneratedSet);
    assert_eq!(valid_generated_set.scope_key(), "artifact-manifest");
    assert_eq!(
        valid_generated_set.generated_members(),
        ["src/generated.rs"]
    );
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
    let event = Event::session(
        42,
        EventType::SessionHeartbeat,
        ObjectType::Session,
        "session-1".to_owned(),
        SessionToken::parse("session-1").expect("valid session token"),
        serde_json::to_string(&payload).expect("heartbeat serialization must succeed"),
        TimestampMs::parse(1_234).expect("valid timestamp"),
    );

    let typed = event.typed().expect("event payload must decode");

    assert_eq!(typed.seq, 42);
    assert_eq!(typed.object_id, "session-1");
    assert_eq!(typed.payload, EventPayload::SessionHeartbeat(payload));
}

#[test]
fn event_typed_propagates_payload_decode_failures() {
    let event = Event::session(
        1,
        EventType::SessionHeartbeat,
        ObjectType::Session,
        "session-1".to_owned(),
        SessionToken::parse("session-1").expect("valid session token"),
        "{".to_owned(),
        TimestampMs::parse(99).expect("valid timestamp"),
    );

    let err = event
        .typed()
        .expect_err("malformed payload must fail to decode");

    assert!(matches!(err, CoveyError::SerializationError(_)));
}

#[test]
fn event_actor_domain_rejects_invalid_token_shapes() {
    let missing_session_token = r#"{"seq":1,"event_type":"session_heartbeat","object_type":"session","object_id":"session-1","actor_kind":"session","session_token":null,"payload_json":"{}","created_at":99}"#;
    let missing_err = serde_json::from_str::<Event>(missing_session_token)
        .expect_err("session actor without token must be rejected");
    assert!(
        missing_err
            .to_string()
            .contains("session actor events require session_token")
    );

    let system_with_session = r#"{"seq":2,"event_type":"claims_expired","object_type":"claim","object_id":"system-maintenance","actor_kind":"system","session_token":"session-1","payload_json":"{}","created_at":99}"#;
    let system_err = serde_json::from_str::<Event>(system_with_session)
        .expect_err("system actor with token must be rejected");
    assert!(
        system_err
            .to_string()
            .contains("system actor events must not include session_token")
    );
}

#[test]
fn openspec_import_provenance_rejects_object_kind_mismatches() {
    let valid_subtask = r#"{"object_type":"subtask","object_id":"subtask-1","planning_format":"openspec","openspec_change_id":"change-1","openspec_change_path":"openspec/changes/change-1","openspec_task_id":"1.1","proposal_digest":null,"design_digest":null,"tasks_digest":"blake3:tasks","spec_digests":[],"source_digests":[{"path":"tasks.md","digest":"blake3:tasks"}],"mission_artifact_digests":[],"mission_artifacts":[],"task_digest":"blake3:task","updated_at":123}"#;
    let subtask = serde_json::from_str::<OpenSpecImportProvenance>(valid_subtask)
        .expect("valid subtask provenance should decode");
    assert_eq!(subtask.object_type(), ObjectType::Subtask);
    assert_eq!(subtask.openspec_task_id(), Some("1.1"));
    assert_eq!(subtask.task_digest(), Some("blake3:task"));
    assert!(subtask.proposal_digest().is_none());

    let subtask_with_meta_fields = r#"{"object_type":"subtask","object_id":"subtask-1","planning_format":"openspec","openspec_change_id":"change-1","openspec_change_path":"openspec/changes/change-1","openspec_task_id":"1.1","proposal_digest":"blake3:proposal","design_digest":null,"tasks_digest":"blake3:tasks","spec_digests":[],"source_digests":[],"mission_artifact_digests":[],"mission_artifacts":[],"task_digest":"blake3:task","updated_at":123}"#;
    let subtask_err = serde_json::from_str::<OpenSpecImportProvenance>(subtask_with_meta_fields)
        .expect_err("subtask provenance with metatask fields must be rejected");
    assert!(
        subtask_err
            .to_string()
            .contains("subtask OpenSpec provenance must not include metatask fields")
    );

    let metatask_with_task_fields = r#"{"object_type":"meta_task","object_id":"meta-1","planning_format":"openspec","openspec_change_id":"change-1","openspec_change_path":"openspec/changes/change-1","openspec_task_id":"1.1","proposal_digest":"blake3:proposal","design_digest":"blake3:design","tasks_digest":"blake3:tasks","spec_digests":[],"source_digests":[],"mission_artifact_digests":[],"mission_artifacts":[],"task_digest":null,"updated_at":123}"#;
    let meta_err = serde_json::from_str::<OpenSpecImportProvenance>(metatask_with_task_fields)
        .expect_err("metatask provenance with task fields must be rejected");
    assert!(
        meta_err
            .to_string()
            .contains("metatask OpenSpec provenance must not include task fields")
    );

    let unsupported_object = r#"{"object_type":"claim","object_id":"claim-1","planning_format":"openspec","openspec_change_id":"change-1","openspec_change_path":"openspec/changes/change-1","openspec_task_id":null,"proposal_digest":null,"design_digest":null,"tasks_digest":"blake3:tasks","spec_digests":[],"source_digests":[],"mission_artifact_digests":[],"mission_artifacts":[],"task_digest":null,"updated_at":123}"#;
    let unsupported_err = serde_json::from_str::<OpenSpecImportProvenance>(unsupported_object)
        .expect_err("unsupported provenance object type must be rejected");
    assert!(
        unsupported_err
            .to_string()
            .contains("OpenSpec provenance only supports metatask and subtask objects")
    );
}

#[test]
fn openspec_import_item_rejects_object_kind_mismatches() {
    let valid_subtask = r#"{"object_type":"subtask","object_id":"subtask-1","openspec_task_id":"1.1","title":"Implement item","task_type":"implementation","task_digest":"blake3:task","source_path":"openspec/changes/change-1/tasks.md","action":"created"}"#;
    let subtask = serde_json::from_str::<ImportOpenSpecItemResult>(valid_subtask)
        .expect("valid subtask import item should decode");
    assert_eq!(subtask.object_type(), ObjectType::Subtask);
    assert_eq!(subtask.openspec_task_id(), Some("1.1"));
    assert_eq!(subtask.title(), "Implement item");
    assert_eq!(subtask.task_type(), Some("implementation"));
    assert_eq!(subtask.task_digest(), Some("blake3:task"));
    assert_eq!(
        subtask.source_path(),
        Some("openspec/changes/change-1/tasks.md")
    );

    let metatask_with_task_fields = r#"{"object_type":"meta_task","object_id":"meta-1","openspec_task_id":"1.1","title":"Change prompt","task_type":null,"task_digest":null,"source_path":null,"action":"created"}"#;
    let meta_err = serde_json::from_str::<ImportOpenSpecItemResult>(metatask_with_task_fields)
        .expect_err("metatask item with task fields must be rejected");
    assert!(
        meta_err
            .to_string()
            .contains("metatask OpenSpec import items must not include task fields")
    );

    let subtask_missing_digest = r#"{"object_type":"subtask","object_id":"subtask-1","openspec_task_id":"1.1","title":"Implement item","task_type":null,"task_digest":null,"source_path":"openspec/changes/change-1/tasks.md","action":"created"}"#;
    let subtask_err = serde_json::from_str::<ImportOpenSpecItemResult>(subtask_missing_digest)
        .expect_err("subtask item without task digest must be rejected");
    assert!(
        subtask_err
            .to_string()
            .contains("subtask OpenSpec import items require task_digest")
    );

    let unsupported_object = r#"{"object_type":"claim","object_id":"claim-1","openspec_task_id":null,"title":"Claim","task_type":null,"task_digest":null,"source_path":null,"action":"created"}"#;
    let unsupported_err = serde_json::from_str::<ImportOpenSpecItemResult>(unsupported_object)
        .expect_err("unsupported import item object must be rejected");
    assert!(
        unsupported_err
            .to_string()
            .contains("OpenSpec import items only support metatask and subtask objects")
    );
}

#[test]
fn openspec_import_conflict_rejects_non_subtask_shapes() {
    let valid_conflict = r#"{"object_type":"subtask","object_id":"subtask-1","openspec_task_id":"1.1","reason":"active_claim_changed_source","source_path":"openspec/changes/change-1/tasks.md","task_digest":"blake3:task"}"#;
    let conflict = serde_json::from_str::<ImportOpenSpecConflict>(valid_conflict)
        .expect("valid subtask conflict should decode");
    assert_eq!(conflict.object_type(), ObjectType::Subtask);
    assert_eq!(conflict.openspec_task_id(), "1.1");
    assert_eq!(conflict.task_digest(), "blake3:task");

    let missing_task_id = r#"{"object_type":"subtask","object_id":"subtask-1","openspec_task_id":null,"reason":"active_claim_changed_source","source_path":"openspec/changes/change-1/tasks.md","task_digest":"blake3:task"}"#;
    let missing_task_id_err = serde_json::from_str::<ImportOpenSpecConflict>(missing_task_id)
        .expect_err("subtask conflict without task id must be rejected");
    assert!(
        missing_task_id_err
            .to_string()
            .contains("subtask OpenSpec import conflicts require openspec_task_id")
    );

    let missing_task_digest = r#"{"object_type":"subtask","object_id":"subtask-1","openspec_task_id":"1.1","reason":"active_claim_changed_source","source_path":"openspec/changes/change-1/tasks.md","task_digest":null}"#;
    let missing_task_digest_err =
        serde_json::from_str::<ImportOpenSpecConflict>(missing_task_digest)
            .expect_err("subtask conflict without task digest must be rejected");
    assert!(
        missing_task_digest_err
            .to_string()
            .contains("subtask OpenSpec import conflicts require task_digest")
    );

    let unsupported_object = r#"{"object_type":"meta_task","object_id":"meta-1","openspec_task_id":null,"reason":"bad","source_path":"openspec/changes/change-1/tasks.md","task_digest":null}"#;
    let unsupported_err = serde_json::from_str::<ImportOpenSpecConflict>(unsupported_object)
        .expect_err("non-subtask conflict object must be rejected");
    assert!(
        unsupported_err
            .to_string()
            .contains("OpenSpec import conflicts only support subtask objects")
    );
}

#[test]
fn openspec_import_event_rejects_provenance_mismatches() {
    let valid_event = json!({
        "change_id": "change-1",
        "object_type": "subtask",
        "object_id": "subtask-1",
        "openspec_task_id": "1.1",
        "action": "created",
        "provenance": {
            "object_type": "subtask",
            "object_id": "subtask-1",
            "planning_format": "openspec",
            "openspec_change_id": "change-1",
            "openspec_change_path": "openspec/changes/change-1",
            "openspec_task_id": "1.1",
            "proposal_digest": null,
            "design_digest": null,
            "tasks_digest": "blake3:tasks",
            "spec_digests": [],
            "source_digests": [{"path": "tasks.md", "digest": "blake3:tasks"}],
            "mission_artifact_digests": [],
            "mission_artifacts": [],
            "task_digest": "blake3:task",
            "updated_at": 123
        }
    });
    let event = serde_json::from_value::<ImportOpenSpecEvent>(valid_event.clone())
        .expect("matching event provenance should decode");
    assert_eq!(event.change_id(), "change-1");
    assert_eq!(event.object_type(), ObjectType::Subtask);
    assert_eq!(event.object_id(), "subtask-1");
    assert_eq!(event.openspec_task_id(), Some("1.1"));
    assert_eq!(event.action(), ImportOpenSpecAction::Created);

    let mut wrong_change = valid_event.clone();
    wrong_change["change_id"] = json!("other-change");
    let wrong_change_err = serde_json::from_value::<ImportOpenSpecEvent>(wrong_change)
        .expect_err("event change id must match provenance");
    assert!(
        wrong_change_err
            .to_string()
            .contains("OpenSpec import event change_id must match provenance")
    );

    let mut wrong_object_type = valid_event.clone();
    wrong_object_type["object_type"] = json!("meta_task");
    let wrong_object_type_err = serde_json::from_value::<ImportOpenSpecEvent>(wrong_object_type)
        .expect_err("event object type must match provenance");
    assert!(
        wrong_object_type_err
            .to_string()
            .contains("OpenSpec import event object_type must match provenance")
    );

    let mut wrong_object_id = valid_event.clone();
    wrong_object_id["object_id"] = json!("subtask-2");
    let wrong_object_id_err = serde_json::from_value::<ImportOpenSpecEvent>(wrong_object_id)
        .expect_err("event object id must match provenance");
    assert!(
        wrong_object_id_err
            .to_string()
            .contains("OpenSpec import event object_id must match provenance")
    );

    let mut wrong_task_id = valid_event;
    wrong_task_id["openspec_task_id"] = json!("1.2");
    let wrong_task_id_err = serde_json::from_value::<ImportOpenSpecEvent>(wrong_task_id)
        .expect_err("event task id must match provenance");
    assert!(
        wrong_task_id_err
            .to_string()
            .contains("OpenSpec import event openspec_task_id must match provenance")
    );
}

fn payload_json<T: Serialize>(payload: &T) -> String {
    serde_json::to_string(payload).expect("payload fixture should serialize")
}

fn sample_reservation() -> Reservation {
    Reservation::try_from_parts(
        ReservationId::parse("reservation-1").expect("valid reservation id"),
        SubtaskId::parse("subtask-1").expect("valid subtask id"),
        ScopeClass::ExactPath,
        "src/lib.rs",
        vec![],
        LeaseDeadlineMs::parse(200).expect("valid lease deadline"),
        ReservationState::Active,
        TimestampMs::parse(10).expect("valid timestamp"),
        TimestampMs::parse(11).expect("valid timestamp"),
    )
    .expect("valid reservation fixture")
}

fn sample_openspec_event() -> ImportOpenSpecEvent {
    ImportOpenSpecEvent::new(
        ImportOpenSpecAction::Created,
        OpenSpecImportProvenance::subtask(
            OpenSpecImportProvenanceCommon {
                object_id: "subtask-1".to_owned(),
                planning_format: "openspec".to_owned(),
                openspec_change_id: "change-1".to_owned(),
                openspec_change_path: "openspec/changes/change-1".to_owned(),
                tasks_digest: "blake3:tasks".to_owned(),
                source_digests: vec![OpenSpecSourceDigest::new(
                    "tasks.md".to_owned(),
                    "blake3:tasks".to_owned(),
                )],
                mission_artifact_digests: vec![],
                mission_artifacts: vec![],
                updated_at: TimestampMs::parse(123).expect("valid timestamp"),
            },
            "1.1".to_owned(),
            "blake3:task".to_owned(),
        ),
    )
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

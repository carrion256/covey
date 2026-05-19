use super::{
    AbandonSubtaskReq, ArtifactDigest, ArtifactKind, CancelMetaTaskReq, ClaimId, ClaimNextReq,
    ClaimReadyQueueReq, ClaimResult, ClaimState, ClaimSubtaskReq, Conflict, ConflictKind,
    ConflictResolutionState, CreateSubtaskRequest, DecideReviewReq, EnqueueForApplyReq, Event,
    EventPayload, EventType, ExitSessionReq, ExpiredCountPayload, ExpiringClaim, FenceSeq,
    HeartbeatReq, ImportBdV1Req, ImportBdV1Result, ImportBdV1SkipReason, ImportOpenSpecAction,
    ImportOpenSpecConflict, ImportOpenSpecEvent, ImportOpenSpecItemResult, ImportOpenSpecReq,
    ImportOpenSpecResult, LandingAuthorizationStatus, LeaseDeadlineMs, MarkAppliedReq,
    MarkInFlightReq, MetaTaskId, MetaTaskStatus, ObjectType, OpenSpecImportProvenance,
    OpenSpecImportProvenanceCommon, OpenSpecSourceDigest, OverlapQueryReq, PublishArtifactReq,
    QueueId, ReadyQueueClaim, ReadyQueueItem, ReadyQueueMetrics, ReadyQueueState,
    RecordApplyVerificationReq, RecordRuntimeAttestationReq, ReleaseClaimReq,
    ReleaseReservationReq, RenewReservationReq, RequestReservationReq, RequestReviewReq,
    Reservation, ReservationId, ReservationState, ResolveConflictReq, Review, ReviewSubtask,
    ReviewTarget, ReviewVerdict, ScopeClass, Session, SessionHandle, SessionRole, SessionState,
    SessionStatus, SessionToken, SettlementTarget, StaleSessionsPayload, StartSubtaskReq,
    StuckSubtask, SubmitMetaTaskReq, Subtask, SubtaskId, SubtaskKind, SubtaskLifecycle, SubtaskRow,
    SubtaskState, SubtaskStatus, SubtaskView, SupersedeQueueItemReq, TimestampMs, TypedEvent,
    VerifyLandingAuthorizationReq, WorkSubtask, bd_import_v1_subtask_id, make_id,
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
fn session_status_rejects_missing_or_stale_active_subtask_view() {
    let active_session = session_json(
        SessionState::Active,
        Some(SubtaskId::parse("subtask-1").expect("valid subtask id")),
    );
    let missing_view = json!({
        "session": active_session,
        "active_subtask": null
    });
    let err = serde_json::from_value::<SessionStatus>(missing_view)
        .expect_err("active session status requires matching subtask view");
    assert!(
        err.to_string()
            .contains("session status requires active_subtask view"),
        "unexpected error: {err}"
    );

    let stale_view = json!({
        "session": session_json(
            SessionState::Active,
            Some(SubtaskId::parse("subtask-1").expect("valid subtask id")),
        ),
        "active_subtask": {
            "subtask_id": "subtask-2",
            "meta_task_id": "meta-1",
            "title": "implement",
            "kind": "work",
            "review_target": null,
            "state": "available",
            "active_claim_id": null,
            "artifact_digest": null,
            "priority": 10,
            "created_at": 100,
            "updated_at": 200
        }
    });
    let err = serde_json::from_value::<SessionStatus>(stale_view)
        .expect_err("active subtask view must match session active subtask");
    assert!(
        err.to_string()
            .contains("session status active_subtask must match session state"),
        "unexpected error: {err}"
    );
}

#[test]
fn meta_task_status_rejects_subtasks_from_other_meta_tasks() {
    let raw = json!({
        "meta_task": {
            "meta_task_id": "meta-1",
            "prompt_text": "ship it",
            "state": "active",
            "created_by": "session-1",
            "created_at": 100,
            "updated_at": 200
        },
        "subtasks": [{
            "subtask_id": "subtask-1",
            "meta_task_id": "meta-2",
            "title": "implement",
            "kind": "work",
            "review_target": null,
            "state": "available",
            "active_claim_id": null,
            "artifact_digest": null,
            "priority": 10,
            "created_at": 100,
            "updated_at": 200
        }]
    });

    let err = serde_json::from_value::<MetaTaskStatus>(raw)
        .expect_err("meta status subtasks must belong to its meta-task");

    assert!(
        err.to_string()
            .contains("meta-task status subtasks must belong to the meta-task"),
        "unexpected error: {err}"
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
fn work_subtask_domain_rejects_review_only_lifecycle() {
    let err = WorkSubtask::new(
        SubtaskId::parse("subtask-1").expect("valid subtask id"),
        MetaTaskId::parse("meta-1").expect("valid meta-task id"),
        "implement".to_owned(),
        SubtaskLifecycle::Decided,
        10,
        TimestampMs::parse(100).expect("valid timestamp"),
        TimestampMs::parse(200).expect("valid timestamp"),
    )
    .expect_err("work subtasks must not be constructible in decided review state");

    assert!(
        err.to_string()
            .contains("work subtasks cannot use decided review lifecycle state"),
        "unexpected error: {err}"
    );
}

#[test]
fn review_subtask_domain_rejects_work_artifact_lifecycle() {
    let err = ReviewSubtask::new(
        SubtaskId::parse("review-1").expect("valid subtask id"),
        MetaTaskId::parse("meta-1").expect("valid meta-task id"),
        "review artifact".to_owned(),
        ReviewTarget::new(
            SubtaskId::parse("subtask-1").expect("valid subtask id"),
            ArtifactDigest::parse("blake3:artifact").expect("valid digest"),
        ),
        SubtaskLifecycle::ArtifactPublished {
            active_claim_id: None,
            artifact_digest: ArtifactDigest::parse("blake3:findings").expect("valid digest"),
        },
        10,
        TimestampMs::parse(100).expect("valid timestamp"),
        TimestampMs::parse(200).expect("valid timestamp"),
    )
    .expect_err("review subtasks must not be constructible in work artifact state");

    assert!(
        err.to_string()
            .contains("review subtasks cannot use work artifact lifecycle states"),
        "unexpected error: {err}"
    );
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

#[test]
fn runtime_attestation_request_requires_runtime_identity() {
    let missing_identity = json!({
        "session_token": "session-1",
        "provider": "provider-1",
        "model": "model-1",
        "provider_run_id": "run-1",
        "provider_run_id_issuer": "issuer-1",
        "process_id": null,
        "container_id": null,
        "command_transcript_digest": "blake3:transcript",
        "started_at": 100,
        "ended_at": 101,
        "idempotency_key": "idem-1"
    });
    let missing_identity_err =
        serde_json::from_value::<RecordRuntimeAttestationReq>(missing_identity)
            .expect_err("runtime attestation request requires process or container identity");
    assert!(
        missing_identity_err
            .to_string()
            .contains("process_id or container_id is required"),
        "unexpected error: {missing_identity_err}"
    );

    let blank_process = json!({
        "session_token": "session-1",
        "provider": "provider-1",
        "model": "model-1",
        "provider_run_id": "run-1",
        "provider_run_id_issuer": "issuer-1",
        "process_id": " ",
        "container_id": null,
        "command_transcript_digest": "blake3:transcript",
        "started_at": 100,
        "ended_at": 101,
        "idempotency_key": "idem-1"
    });
    let blank_process_err = serde_json::from_value::<RecordRuntimeAttestationReq>(blank_process)
        .expect_err("blank process identity must be rejected");
    assert!(
        blank_process_err
            .to_string()
            .contains("process_id must not be empty"),
        "unexpected error: {blank_process_err}"
    );

    let req = RecordRuntimeAttestationReq::try_from_parts(
        "session-1",
        "provider-1",
        "model-1",
        "run-1",
        "issuer-1",
        Some("process-1".to_owned()),
        Some("container-1".to_owned()),
        "blake3:transcript",
        100,
        101,
        "idem-1",
    )
    .expect("process and container identities are valid together");
    assert_eq!(req.process_id(), Some("process-1"));
    assert_eq!(req.container_id(), Some("container-1"));

    let value = serde_json::to_value(&req).expect("request serializes to flat JSON");
    assert_eq!(value["process_id"], "process-1");
    assert_eq!(value["container_id"], "container-1");
}

#[test]
fn subtask_status_rejects_claim_that_disagrees_with_lifecycle() {
    let raw = json!({
        "subtask": {
            "subtask_id": "subtask-1",
            "meta_task_id": "meta-1",
            "title": "implement",
            "kind": "work",
            "review_target": null,
            "state": "claimed",
            "active_claim_id": "claim-1",
            "artifact_digest": null,
            "priority": 10,
            "created_at": 100,
            "updated_at": 200
        },
        "claim": {
            "claim_id": "claim-2",
            "subtask_id": "subtask-1",
            "owner_session_token": "session-1",
            "fence_seq": 1,
            "lease_deadline": 500,
            "state": "held",
            "created_at": 100,
            "updated_at": 100
        },
        "artifact": null,
        "reviews": [],
        "ready_queue": []
    });

    let err = serde_json::from_value::<SubtaskStatus>(raw)
        .expect_err("status claim must match active lifecycle claim");

    assert!(
        err.to_string()
            .contains("subtask status claim_id must match active claim"),
        "unexpected error: {err}"
    );
}

#[test]
fn subtask_status_rejects_artifact_without_lifecycle_artifact() {
    let raw = json!({
        "subtask": {
            "subtask_id": "subtask-1",
            "meta_task_id": "meta-1",
            "title": "implement",
            "kind": "work",
            "review_target": null,
            "state": "available",
            "active_claim_id": null,
            "artifact_digest": null,
            "priority": 10,
            "created_at": 100,
            "updated_at": 200
        },
        "claim": null,
        "artifact": {
            "artifact_digest": "blake3:artifact",
            "artifact_kind": "patch_bundle",
            "base_rev": "base-1",
            "produced_by_subtask_id": "subtask-1",
            "produced_by_session": "session-1",
            "manifest_path": "artifact.json",
            "changed_paths_digest": "blake3:paths",
            "created_at": 150
        },
        "reviews": [],
        "ready_queue": []
    });

    let err = serde_json::from_value::<SubtaskStatus>(raw)
        .expect_err("status artifact must match lifecycle artifact");

    assert!(
        err.to_string()
            .contains("subtask status must not include artifact without lifecycle artifact"),
        "unexpected error: {err}"
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
fn ready_queue_claim_payload_rejects_invalid_typed_fields() {
    let raw = serde_json::json!({
        "queue_id": "",
        "artifact_digest": "not-a-digest",
        "subtask_id": "subtask-1",
        "settlement_target": "canonical",
        "claim_fence_seq": 0,
        "lease_deadline": 900
    });

    serde_json::from_value::<ReadyQueueClaim>(raw)
        .expect_err("queue claim payload must reject invalid ids, digests, and fences");
}

#[test]
fn ready_queue_fence_request_payloads_reject_invalid_fences() {
    let mark_applied = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "claim_fence_seq": 0,
        "idempotency_key": "idem-mark-applied"
    });
    serde_json::from_value::<MarkAppliedReq>(mark_applied)
        .expect_err("mark-applied request should reject invalid fence sequence");

    let verification = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "review_id": "review-1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 0,
        "verifier": "mutai-rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "blake3:seal",
        "idempotency_key": "idem-verification"
    });
    serde_json::from_value::<RecordApplyVerificationReq>(verification)
        .expect_err("apply verification request should reject invalid fence sequence");
}

#[test]
fn ready_queue_mutation_payloads_reject_invalid_ids_and_leases() {
    let invalid_claim_lease = serde_json::json!({
        "session_token": "session-1",
        "lease_duration_ms": 0,
        "idempotency_key": "idem-claim-ready"
    });
    serde_json::from_value::<ClaimReadyQueueReq>(invalid_claim_lease)
        .expect_err("claim-ready request should reject non-positive lease durations");

    let invalid_in_flight_queue = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "",
        "lease_duration_ms": 30_000,
        "idempotency_key": "idem-mark-in-flight"
    });
    serde_json::from_value::<MarkInFlightReq>(invalid_in_flight_queue)
        .expect_err("mark-in-flight request should reject invalid queue ids");

    let invalid_in_flight_lease = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "lease_duration_ms": -1,
        "idempotency_key": "idem-mark-in-flight"
    });
    serde_json::from_value::<MarkInFlightReq>(invalid_in_flight_lease)
        .expect_err("mark-in-flight request should reject non-positive lease durations");

    let invalid_applied_queue = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "",
        "claim_fence_seq": 1,
        "idempotency_key": "idem-mark-applied"
    });
    serde_json::from_value::<MarkAppliedReq>(invalid_applied_queue)
        .expect_err("mark-applied request should reject invalid queue ids");

    let invalid_supersede_queue = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "",
        "idempotency_key": "idem-supersede"
    });
    serde_json::from_value::<SupersedeQueueItemReq>(invalid_supersede_queue)
        .expect_err("supersede request should reject invalid queue ids");
}

#[test]
fn apply_verification_payloads_reject_invalid_typed_fields() {
    let invalid_queue_id = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "",
        "artifact_digest": "blake3:artifact",
        "review_id": "review-1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 1,
        "verifier": "mutai-rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "blake3:seal",
        "idempotency_key": "idem-verification"
    });
    serde_json::from_value::<RecordApplyVerificationReq>(invalid_queue_id)
        .expect_err("apply verification should reject invalid queue ids");

    let invalid_artifact_digest = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "artifact",
        "review_id": "review-1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 1,
        "verifier": "mutai-rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "blake3:seal",
        "idempotency_key": "idem-verification"
    });
    serde_json::from_value::<RecordApplyVerificationReq>(invalid_artifact_digest)
        .expect_err("apply verification should reject invalid artifact digests");

    let invalid_review_id = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "review_id": "review 1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 1,
        "verifier": "mutai-rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "blake3:seal",
        "idempotency_key": "idem-verification"
    });
    serde_json::from_value::<RecordApplyVerificationReq>(invalid_review_id)
        .expect_err("apply verification should reject invalid review ids");

    let invalid_findings_digest = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "review_id": "review-1",
        "findings_digest": "findings",
        "claim_fence_seq": 1,
        "verifier": "mutai-rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "blake3:seal",
        "idempotency_key": "idem-verification"
    });
    serde_json::from_value::<RecordApplyVerificationReq>(invalid_findings_digest)
        .expect_err("apply verification should reject invalid findings digests");

    let invalid_verdict_digest = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "review_id": "review-1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 1,
        "verifier": "mutai-rs",
        "verdict_digest": "verdict",
        "seal_digest": "blake3:seal",
        "idempotency_key": "idem-verification"
    });
    serde_json::from_value::<RecordApplyVerificationReq>(invalid_verdict_digest)
        .expect_err("apply verification should reject invalid verdict digests");

    let invalid_seal_digest = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "review_id": "review-1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 1,
        "verifier": "mutai-rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "seal",
        "idempotency_key": "idem-verification"
    });
    serde_json::from_value::<RecordApplyVerificationReq>(invalid_seal_digest)
        .expect_err("apply verification should reject invalid seal digests");
}

#[test]
fn landing_authorization_payload_rejects_invalid_typed_fields() {
    let invalid_queue_id = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "",
        "artifact_digest": "blake3:artifact",
        "review_id": "review-1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 1,
        "verifier": "mutai-rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "blake3:seal"
    });
    serde_json::from_value::<VerifyLandingAuthorizationReq>(invalid_queue_id)
        .expect_err("landing authorization should reject invalid queue ids");

    let invalid_artifact_digest = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "artifact",
        "review_id": "review-1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 1,
        "verifier": "mutai-rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "blake3:seal"
    });
    serde_json::from_value::<VerifyLandingAuthorizationReq>(invalid_artifact_digest)
        .expect_err("landing authorization should reject invalid artifact digests");

    let invalid_review_id = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "review_id": "review 1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 1,
        "verifier": "mutai-rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "blake3:seal"
    });
    serde_json::from_value::<VerifyLandingAuthorizationReq>(invalid_review_id)
        .expect_err("landing authorization should reject invalid review ids");

    let invalid_findings_digest = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "review_id": "review-1",
        "findings_digest": "findings",
        "claim_fence_seq": 1,
        "verifier": "mutai-rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "blake3:seal"
    });
    serde_json::from_value::<VerifyLandingAuthorizationReq>(invalid_findings_digest)
        .expect_err("landing authorization should reject invalid findings digests");

    let invalid_verdict_digest = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "review_id": "review-1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 1,
        "verifier": "mutai-rs",
        "verdict_digest": "verdict",
        "seal_digest": "blake3:seal"
    });
    serde_json::from_value::<VerifyLandingAuthorizationReq>(invalid_verdict_digest)
        .expect_err("landing authorization should reject invalid verdict digests");

    let invalid_seal_digest = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "review_id": "review-1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 1,
        "verifier": "mutai-rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "seal"
    });
    serde_json::from_value::<VerifyLandingAuthorizationReq>(invalid_seal_digest)
        .expect_err("landing authorization should reject invalid seal digests");
}

#[test]
fn claim_result_payload_rejects_invalid_typed_fields() {
    let invalid_claim_id = serde_json::json!({
        "claim_id": "",
        "subtask_id": "subtask-1",
        "fence_seq": 1,
        "lease_deadline": 900
    });
    serde_json::from_value::<ClaimResult>(invalid_claim_id)
        .expect_err("claim result should reject invalid claim ids");

    let invalid_subtask_id = serde_json::json!({
        "claim_id": "claim-1",
        "subtask_id": "subtask 1",
        "fence_seq": 1,
        "lease_deadline": 900
    });
    serde_json::from_value::<ClaimResult>(invalid_subtask_id)
        .expect_err("claim result should reject invalid subtask ids");

    let invalid_fence = serde_json::json!({
        "claim_id": "claim-1",
        "subtask_id": "subtask-1",
        "fence_seq": 0,
        "lease_deadline": 900
    });
    serde_json::from_value::<ClaimResult>(invalid_fence)
        .expect_err("claim result should reject invalid fence sequences");

    let invalid_deadline = serde_json::json!({
        "claim_id": "claim-1",
        "subtask_id": "subtask-1",
        "fence_seq": 1,
        "lease_deadline": -1
    });
    serde_json::from_value::<ClaimResult>(invalid_deadline)
        .expect_err("claim result should reject invalid lease deadlines");
}

#[test]
fn claim_lifecycle_request_payloads_reject_invalid_claim_identity_and_fence() {
    let invalid_claim_id = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "",
        "fence_seq": 1,
        "idempotency_key": "idem-start"
    });
    serde_json::from_value::<StartSubtaskReq>(invalid_claim_id)
        .expect_err("start request should reject invalid claim ids");

    let invalid_fence = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 0,
        "idempotency_key": "idem-start"
    });
    serde_json::from_value::<StartSubtaskReq>(invalid_fence)
        .expect_err("start request should reject invalid fence sequences");
}

#[test]
fn claim_acquisition_payloads_reject_invalid_targets_and_leases() {
    let invalid_next_lease = serde_json::json!({
        "session_token": "session-1",
        "lease_duration_ms": 0,
        "idempotency_key": "idem-claim-next"
    });
    serde_json::from_value::<ClaimNextReq>(invalid_next_lease)
        .expect_err("claim-next request should reject non-positive leases");

    let invalid_target = serde_json::json!({
        "session_token": "session-1",
        "subtask_id": "subtask 1",
        "lease_duration_ms": 30_000,
        "idempotency_key": "idem-claim-subtask"
    });
    serde_json::from_value::<ClaimSubtaskReq>(invalid_target)
        .expect_err("claim-subtask request should reject invalid subtask ids");

    let invalid_target_lease = serde_json::json!({
        "session_token": "session-1",
        "subtask_id": "subtask-1",
        "lease_duration_ms": -1,
        "idempotency_key": "idem-claim-subtask"
    });
    serde_json::from_value::<ClaimSubtaskReq>(invalid_target_lease)
        .expect_err("claim-subtask request should reject non-positive leases");
}

#[test]
fn publish_artifact_payload_rejects_invalid_typed_fields() {
    let invalid_claim_id = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "",
        "fence_seq": 1,
        "artifact_digest": "blake3:artifact",
        "artifact_kind": "patch_bundle",
        "base_rev": "base",
        "manifest_path": "manifest.json",
        "changed_paths_digest": "blake3:paths",
        "idempotency_key": "idem-artifact"
    });
    serde_json::from_value::<PublishArtifactReq>(invalid_claim_id)
        .expect_err("publish request should reject invalid claim ids");

    let invalid_fence = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 0,
        "artifact_digest": "blake3:artifact",
        "artifact_kind": "patch_bundle",
        "base_rev": "base",
        "manifest_path": "manifest.json",
        "changed_paths_digest": "blake3:paths",
        "idempotency_key": "idem-artifact"
    });
    serde_json::from_value::<PublishArtifactReq>(invalid_fence)
        .expect_err("publish request should reject invalid fence sequences");

    let invalid_artifact_digest = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "artifact_digest": "artifact",
        "artifact_kind": "patch_bundle",
        "base_rev": "base",
        "manifest_path": "manifest.json",
        "changed_paths_digest": "blake3:paths",
        "idempotency_key": "idem-artifact"
    });
    serde_json::from_value::<PublishArtifactReq>(invalid_artifact_digest)
        .expect_err("publish request should reject invalid artifact digests");

    let invalid_base_rev = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "artifact_digest": "blake3:artifact",
        "artifact_kind": "patch_bundle",
        "base_rev": "base rev",
        "manifest_path": "manifest.json",
        "changed_paths_digest": "blake3:paths",
        "idempotency_key": "idem-artifact"
    });
    serde_json::from_value::<PublishArtifactReq>(invalid_base_rev)
        .expect_err("publish request should reject invalid base revisions");

    let invalid_changed_paths_digest = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "artifact_digest": "blake3:artifact",
        "artifact_kind": "patch_bundle",
        "base_rev": "base",
        "manifest_path": "manifest.json",
        "changed_paths_digest": "paths",
        "idempotency_key": "idem-artifact"
    });
    serde_json::from_value::<PublishArtifactReq>(invalid_changed_paths_digest)
        .expect_err("publish request should reject invalid changed paths digests");
}

#[test]
fn request_review_payload_rejects_invalid_typed_fields() {
    let invalid_subtask = serde_json::json!({
        "session_token": "session-1",
        "subtask_id": "subtask 1",
        "artifact_digest": "blake3:artifact",
        "review_subtask_id": "review-subtask-1",
        "priority": 1,
        "idempotency_key": "idem-review"
    });
    serde_json::from_value::<RequestReviewReq>(invalid_subtask)
        .expect_err("review request should reject invalid work subtask ids");

    let invalid_digest = serde_json::json!({
        "session_token": "session-1",
        "subtask_id": "subtask-1",
        "artifact_digest": "artifact",
        "review_subtask_id": "review-subtask-1",
        "priority": 1,
        "idempotency_key": "idem-review"
    });
    serde_json::from_value::<RequestReviewReq>(invalid_digest)
        .expect_err("review request should reject invalid artifact digests");

    let invalid_review_subtask = serde_json::json!({
        "session_token": "session-1",
        "subtask_id": "subtask-1",
        "artifact_digest": "blake3:artifact",
        "review_subtask_id": "review subtask",
        "priority": 1,
        "idempotency_key": "idem-review"
    });
    serde_json::from_value::<RequestReviewReq>(invalid_review_subtask)
        .expect_err("review request should reject invalid review subtask ids");
}

#[test]
fn decide_review_payload_rejects_invalid_typed_fields() {
    let invalid_review_id = serde_json::json!({
        "session_token": "session-reviewer",
        "review_id": "review 1",
        "claim_id": "claim-review",
        "fence_seq": 1,
        "verdict": "approve",
        "findings_digest": "blake3:findings",
        "idempotency_key": "idem-decision"
    });
    serde_json::from_value::<DecideReviewReq>(invalid_review_id)
        .expect_err("review decision should reject invalid review ids");

    let invalid_claim_id = serde_json::json!({
        "session_token": "session-reviewer",
        "review_id": "review-1",
        "claim_id": "",
        "fence_seq": 1,
        "verdict": "approve",
        "findings_digest": "blake3:findings",
        "idempotency_key": "idem-decision"
    });
    serde_json::from_value::<DecideReviewReq>(invalid_claim_id)
        .expect_err("review decision should reject invalid claim ids");

    let invalid_fence = serde_json::json!({
        "session_token": "session-reviewer",
        "review_id": "review-1",
        "claim_id": "claim-review",
        "fence_seq": 0,
        "verdict": "approve",
        "findings_digest": "blake3:findings",
        "idempotency_key": "idem-decision"
    });
    serde_json::from_value::<DecideReviewReq>(invalid_fence)
        .expect_err("review decision should reject invalid fence sequences");

    let invalid_findings_digest = serde_json::json!({
        "session_token": "session-reviewer",
        "review_id": "review-1",
        "claim_id": "claim-review",
        "fence_seq": 1,
        "verdict": "approve",
        "findings_digest": "findings",
        "idempotency_key": "idem-decision"
    });
    serde_json::from_value::<DecideReviewReq>(invalid_findings_digest)
        .expect_err("review decision should reject invalid findings digests");
}

#[test]
fn enqueue_for_apply_payload_rejects_invalid_typed_fields() {
    let invalid_artifact_digest = serde_json::json!({
        "session_token": "session-1",
        "artifact_digest": "artifact",
        "subtask_id": "subtask-1",
        "settlement_target": "canonical",
        "idempotency_key": "idem-enqueue"
    });
    serde_json::from_value::<EnqueueForApplyReq>(invalid_artifact_digest)
        .expect_err("enqueue request should reject invalid artifact digests");

    let invalid_subtask_id = serde_json::json!({
        "session_token": "session-1",
        "artifact_digest": "blake3:artifact",
        "subtask_id": "subtask 1",
        "settlement_target": "canonical",
        "idempotency_key": "idem-enqueue"
    });
    serde_json::from_value::<EnqueueForApplyReq>(invalid_subtask_id)
        .expect_err("enqueue request should reject invalid subtask ids");
}

#[test]
fn import_bd_v1_request_requires_exactly_one_destination_selector() {
    let existing =
        ImportBdV1Req::existing_meta_task("session-1", "beads.db", "meta-1", "idem-existing");
    assert_eq!(existing.meta_task_id(), Some("meta-1"));
    assert_eq!(existing.prompt_text(), None);
    let existing_json = serde_json::to_value(&existing).expect("request should serialize");
    assert_eq!(existing_json["meta_task_id"], "meta-1");
    assert_eq!(existing_json["prompt_text"], serde_json::Value::Null);

    let created = ImportBdV1Req::new_meta_task("session-1", "beads.db", "new work", "idem-new");
    assert_eq!(created.meta_task_id(), None);
    assert_eq!(created.prompt_text(), Some("new work"));
    let created_json = serde_json::to_value(&created).expect("request should serialize");
    assert_eq!(created_json["meta_task_id"], serde_json::Value::Null);
    assert_eq!(created_json["prompt_text"], "new work");

    let both = json!({
        "session_token": "session-1",
        "beads_db_path": "beads.db",
        "meta_task_id": "meta-1",
        "prompt_text": "new work",
        "idempotency_key": "idem-both"
    });
    let both_err = serde_json::from_value::<ImportBdV1Req>(both)
        .expect_err("bd import cannot target existing and new meta-task together");
    assert!(
        both_err
            .to_string()
            .contains("bd import request requires exactly one destination selector"),
        "unexpected error: {both_err}"
    );

    let neither = json!({
        "session_token": "session-1",
        "beads_db_path": "beads.db",
        "meta_task_id": null,
        "prompt_text": null,
        "idempotency_key": "idem-neither"
    });
    serde_json::from_value::<ImportBdV1Req>(neither)
        .expect_err("bd import requires one destination selector");
}

#[test]
fn import_bd_v1_result_derives_counts_from_items() {
    let result = ImportBdV1Result::new(
        "meta-1",
        vec![
            super::ImportBdV1ItemResult::imported("bd-1", "subtask-1"),
            super::ImportBdV1ItemResult::skipped(
                "bd-2",
                Some("subtask-2".to_owned()),
                ImportBdV1SkipReason::DeterministicDuplicate,
            ),
            super::ImportBdV1ItemResult::skipped(
                "bd-3",
                None,
                ImportBdV1SkipReason::InvalidRow {
                    detail: "missing title".to_owned(),
                },
            ),
        ],
    )
    .expect("coherent bd import result should construct");
    assert_eq!(result.imported_count(), 1);
    assert_eq!(result.skipped_count(), 2);
    assert_eq!(result.items().len(), 3);

    let value = serde_json::to_value(&result).expect("result should serialize");
    assert_eq!(value["imported_count"], 1);
    assert_eq!(value["skipped_count"], 2);
    assert_eq!(value["items"].as_array().expect("items").len(), 3);

    let mut wrong_imported = value.clone();
    wrong_imported["imported_count"] = json!(9);
    let wrong_imported_err = serde_json::from_value::<ImportBdV1Result>(wrong_imported)
        .expect_err("stored imported count must match item outcomes");
    assert!(
        wrong_imported_err
            .to_string()
            .contains("imported_count mismatch"),
        "unexpected error: {wrong_imported_err}"
    );

    let mut wrong_skipped = value;
    wrong_skipped["skipped_count"] = json!(9);
    let wrong_skipped_err = serde_json::from_value::<ImportBdV1Result>(wrong_skipped)
        .expect_err("stored skipped count must match item outcomes");
    assert!(
        wrong_skipped_err
            .to_string()
            .contains("skipped_count mismatch"),
        "unexpected error: {wrong_skipped_err}"
    );
}

#[test]
fn import_openspec_request_requires_session_for_write_mode() {
    let dry_run = ImportOpenSpecReq::dry_run("change-1", "/repo");
    assert!(dry_run.is_dry_run());
    assert_eq!(dry_run.session_token(), None);
    assert_eq!(dry_run.write_session_token(), None);
    let dry_run_json = serde_json::to_value(&dry_run).expect("dry-run request should serialize");
    assert_eq!(dry_run_json["dry_run"], true);
    assert_eq!(dry_run_json["session_token"], serde_json::Value::Null);

    let attributed = ImportOpenSpecReq::dry_run_for_session("session-1", "change-1", "/repo");
    assert!(attributed.is_dry_run());
    assert_eq!(attributed.session_token(), Some("session-1"));
    assert_eq!(attributed.write_session_token(), None);

    let write = ImportOpenSpecReq::write("session-1", "change-1", "/repo");
    assert!(!write.is_dry_run());
    assert_eq!(write.session_token(), Some("session-1"));
    assert_eq!(write.write_session_token(), Some("session-1"));
    let write_json = serde_json::to_value(&write).expect("write request should serialize");
    assert_eq!(write_json["dry_run"], false);
    assert_eq!(write_json["session_token"], "session-1");

    let missing_session = json!({
        "session_token": null,
        "change_id": "change-1",
        "project_root": "/repo",
        "dry_run": false
    });
    let err = serde_json::from_value::<ImportOpenSpecReq>(missing_session)
        .expect_err("write mode must require a session token");
    assert!(
        err.to_string()
            .contains("write mode requires --session-token"),
        "unexpected error: {err}"
    );
}

#[test]
fn import_openspec_result_derives_counts_from_items() {
    let items = vec![
        ImportOpenSpecItemResult::meta_task(
            "openspec:change-1",
            "OpenSpec change change-1",
            ImportOpenSpecAction::Created,
        )
        .expect("metatask created item should construct"),
        ImportOpenSpecItemResult::subtask(
            "openspec:change-1:1.1",
            "1.1",
            "Implement import",
            Some("implementation".to_owned()),
            "blake3:task",
            "openspec/changes/change-1/tasks.md",
            ImportOpenSpecAction::Updated,
        ),
        ImportOpenSpecItemResult::subtask(
            "openspec:change-1:1.2",
            "1.2",
            "Keep current import",
            None,
            "blake3:task-2",
            "openspec/changes/change-1/tasks.md",
            ImportOpenSpecAction::Unchanged,
        ),
    ];
    let result = ImportOpenSpecResult::new("change-1", "openspec:change-1", true, vec![], items)
        .expect("coherent result should construct");
    assert!(result.dry_run());
    assert_eq!(result.created(), 1);
    assert_eq!(result.updated(), 1);
    assert_eq!(result.unchanged(), 1);
    assert!(result.conflicts().is_empty());
    assert_eq!(result.items().len(), 3);

    let value = serde_json::to_value(&result).expect("result should serialize");
    assert_eq!(value["created"], 1);
    assert_eq!(value["updated"], 1);
    assert_eq!(value["unchanged"], 1);
    assert_eq!(value["items"].as_array().expect("items").len(), 3);

    let mut wrong_created = value.clone();
    wrong_created["created"] = json!(99);
    let wrong_created_err = serde_json::from_value::<ImportOpenSpecResult>(wrong_created)
        .expect_err("stored count must match item actions");
    assert!(
        wrong_created_err
            .to_string()
            .contains("created count mismatch"),
        "unexpected error: {wrong_created_err}"
    );

    let conflict_item = ImportOpenSpecItemResult::subtask(
        "openspec:change-1:1.3",
        "1.3",
        "Conflicting import",
        None,
        "blake3:task-3",
        "openspec/changes/change-1/tasks.md",
        ImportOpenSpecAction::Conflict,
    );
    let conflict_mismatch = ImportOpenSpecResult::new(
        "change-1",
        "openspec:change-1",
        false,
        vec![],
        vec![conflict_item],
    )
    .expect_err("conflict item requires matching conflict record");
    assert!(
        conflict_mismatch.contains("conflict item(s)"),
        "unexpected error: {conflict_mismatch}"
    );

    let conflict = ImportOpenSpecConflict::subtask(
        "openspec:change-1:1.3",
        "1.3",
        "active_claim_changed_source",
        "openspec/changes/change-1/tasks.md",
        "blake3:task-3",
    );
    let conflict_result = ImportOpenSpecResult::new(
        "change-1",
        "openspec:change-1",
        false,
        vec![conflict],
        vec![ImportOpenSpecItemResult::subtask(
            "openspec:change-1:1.3",
            "1.3",
            "Conflicting import",
            None,
            "blake3:task-3",
            "openspec/changes/change-1/tasks.md",
            ImportOpenSpecAction::Conflict,
        )],
    )
    .expect("matching conflict record should construct");
    assert_eq!(conflict_result.conflicts().len(), 1);
}

#[test]
fn ready_queue_metrics_reject_count_age_mismatches() {
    let missing_age = serde_json::json!({
        "queued_count": 1,
        "in_flight_count": 0,
        "oldest_queued_age_ms": null,
        "oldest_in_flight_age_ms": null
    });
    let missing_age_err = serde_json::from_value::<ReadyQueueMetrics>(missing_age)
        .expect_err("non-empty queued metrics require an oldest age");
    assert!(
        missing_age_err
            .to_string()
            .contains("non-empty queued ready-queue metrics require oldest age"),
        "unexpected error: {missing_age_err}"
    );

    let stale_age = serde_json::json!({
        "queued_count": 0,
        "in_flight_count": 0,
        "oldest_queued_age_ms": 12,
        "oldest_in_flight_age_ms": null
    });
    let stale_age_err = serde_json::from_value::<ReadyQueueMetrics>(stale_age)
        .expect_err("empty queued metrics must not carry an oldest age");
    assert!(
        stale_age_err
            .to_string()
            .contains("empty queued ready-queue metrics must not include oldest age"),
        "unexpected error: {stale_age_err}"
    );

    let negative_age = serde_json::json!({
        "queued_count": 0,
        "in_flight_count": 1,
        "oldest_queued_age_ms": null,
        "oldest_in_flight_age_ms": -1
    });
    let negative_age_err = serde_json::from_value::<ReadyQueueMetrics>(negative_age)
        .expect_err("oldest ages must not be negative");
    assert!(
        negative_age_err
            .to_string()
            .contains("in_flight ready-queue oldest age must not be negative"),
        "unexpected error: {negative_age_err}"
    );

    let metrics = ReadyQueueMetrics::new(1, 0, Some(12), None)
        .expect("coherent queued metrics should construct");
    assert_eq!(metrics.queued_count(), 1);
    assert_eq!(metrics.oldest_queued_age_ms(), Some(12));
    assert_eq!(metrics.in_flight_count(), 0);
    assert_eq!(metrics.oldest_in_flight_age_ms(), None);
}

#[test]
fn observability_rows_reject_negative_durations() {
    let subtask = json!({
        "subtask_id": "subtask-1",
        "meta_task_id": "meta-1",
        "title": "implement",
        "kind": "work",
        "review_target": null,
        "state": "in_progress",
        "active_claim_id": "claim-1",
        "artifact_digest": null,
        "priority": 1,
        "created_at": 100,
        "updated_at": 200
    });

    let stuck = json!({
        "subtask": subtask.clone(),
        "claim": null,
        "session": null,
        "idle_for_ms": -1
    });
    let stuck_err =
        serde_json::from_value::<StuckSubtask>(stuck).expect_err("idle duration is non-negative");
    assert!(
        stuck_err
            .to_string()
            .contains("idle_for_ms must not be negative"),
        "unexpected error: {stuck_err}"
    );

    let claim = json!({
        "claim_id": "claim-1",
        "subtask_id": "subtask-1",
        "owner_session_token": "session-1",
        "fence_seq": 1,
        "lease_deadline": 300,
        "state": ClaimState::Held,
        "created_at": 100,
        "updated_at": 200
    });
    let session = json!({
        "session_token": "session-1",
        "agent_principal_id": "agent-1",
        "agent_instance_id": "instance-1",
        "role": SessionRole::Executor,
        "state": SessionState::Active,
        "active_subtask_id": "subtask-1",
        "last_heartbeat_at": 200,
        "last_heartbeat_tick": 1,
        "created_at": 100,
        "updated_at": 200
    });
    let expiring = json!({
        "claim": claim,
        "subtask": subtask,
        "session": session,
        "expires_in_ms": -1
    });
    let expiring_err = serde_json::from_value::<ExpiringClaim>(expiring)
        .expect_err("expires-in duration is non-negative");
    assert!(
        expiring_err
            .to_string()
            .contains("expires_in_ms must not be negative"),
        "unexpected error: {expiring_err}"
    );
}

#[test]
fn observability_rows_reject_mismatched_attachments() {
    let subtask = json!({
        "subtask_id": "subtask-1",
        "meta_task_id": "meta-1",
        "title": "implement",
        "kind": "work",
        "review_target": null,
        "state": "in_progress",
        "active_claim_id": "claim-1",
        "artifact_digest": null,
        "priority": 1,
        "created_at": 100,
        "updated_at": 200
    });
    let claim = json!({
        "claim_id": "claim-1",
        "subtask_id": "subtask-1",
        "owner_session_token": "session-1",
        "fence_seq": 1,
        "lease_deadline": 300,
        "state": ClaimState::Held,
        "created_at": 100,
        "updated_at": 200
    });
    let session = json!({
        "session_token": "session-1",
        "agent_principal_id": "agent-1",
        "agent_instance_id": "instance-1",
        "role": SessionRole::Executor,
        "state": SessionState::Active,
        "active_subtask_id": "subtask-1",
        "last_heartbeat_at": 200,
        "last_heartbeat_tick": 1,
        "created_at": 100,
        "updated_at": 200
    });

    let stuck_without_claim = json!({
        "subtask": subtask.clone(),
        "claim": null,
        "session": null,
        "idle_for_ms": 10
    });
    let stuck_without_claim_err = serde_json::from_value::<StuckSubtask>(stuck_without_claim)
        .expect_err("active stuck subtask requires its claim row");
    assert!(
        stuck_without_claim_err
            .to_string()
            .contains("stuck subtask requires active claim row"),
        "unexpected error: {stuck_without_claim_err}"
    );

    let stuck_with_wrong_session = json!({
        "subtask": subtask.clone(),
        "claim": claim.clone(),
        "session": {
            "session_token": "session-2",
            "agent_principal_id": "agent-1",
            "agent_instance_id": "instance-1",
            "role": SessionRole::Executor,
            "state": SessionState::Active,
            "active_subtask_id": "subtask-1",
            "last_heartbeat_at": 200,
            "last_heartbeat_tick": 1,
            "created_at": 100,
            "updated_at": 200
        },
        "idle_for_ms": 10
    });
    let stuck_with_wrong_session_err =
        serde_json::from_value::<StuckSubtask>(stuck_with_wrong_session)
            .expect_err("stuck session must own the claim");
    assert!(
        stuck_with_wrong_session_err
            .to_string()
            .contains("stuck subtask session must own the claim"),
        "unexpected error: {stuck_with_wrong_session_err}"
    );

    let expiring_with_wrong_session_subtask = json!({
        "claim": claim.clone(),
        "subtask": subtask.clone(),
        "session": {
            "session_token": "session-1",
            "agent_principal_id": "agent-1",
            "agent_instance_id": "instance-1",
            "role": SessionRole::Executor,
            "state": SessionState::Active,
            "active_subtask_id": "subtask-2",
            "last_heartbeat_at": 200,
            "last_heartbeat_tick": 1,
            "created_at": 100,
            "updated_at": 200
        },
        "expires_in_ms": 10
    });
    let expiring_with_wrong_session_subtask_err =
        serde_json::from_value::<ExpiringClaim>(expiring_with_wrong_session_subtask)
            .expect_err("expiring session must be active on the claimed subtask");
    assert!(
        expiring_with_wrong_session_subtask_err
            .to_string()
            .contains("expiring claim session active_subtask_id must match the subtask"),
        "unexpected error: {expiring_with_wrong_session_subtask_err}"
    );

    let valid = ExpiringClaim::new(
        serde_json::from_value(claim).expect("claim deserializes"),
        serde_json::from_value(subtask).expect("subtask deserializes"),
        serde_json::from_value(session).expect("session deserializes"),
        10,
    )
    .expect("matching expiring claim should construct");
    assert_eq!(valid.expires_in_ms(), 10);
}

#[test]
fn observability_rows_preserve_flat_duration_fields() {
    let subtask = SubtaskView::try_from(work_subtask_row(
        SubtaskState::ArtifactPublished,
        None,
        Some(ArtifactDigest::parse("blake3:artifact").expect("valid digest")),
    ))
    .expect("valid subtask view");
    let row = StuckSubtask::new(subtask, None, None, 42).expect("valid stuck row");

    assert_eq!(row.idle_for_ms(), 42);
    let value = serde_json::to_value(&row).expect("stuck row serializes");
    assert_eq!(value["idle_for_ms"], 42);
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
fn conflict_domain_rejects_unknown_kind_and_mismatched_payload_shape() {
    let valid_payload = json!({
        "reservation_id": "reservation-1",
        "overlapping_reservation_id": "reservation-2",
        "owner_subtask_id": "subtask-1",
        "overlapping_owner_subtask_id": "subtask-2",
        "scope_class": "exact_path",
        "scope_key": "src/lib.rs",
        "overlapping_scope_class": "subtree",
        "overlapping_scope_key": "src"
    });
    let valid = json!({
        "conflict_id": "conflict-1",
        "object_type": "reservation",
        "object_id": "reservation-1",
        "conflict_kind": "reservation_overlap",
        "payload_json": valid_payload.to_string(),
        "detected_at": 100,
        "resolution_state": "open"
    });

    let conflict = serde_json::from_value::<Conflict>(valid).expect("valid conflict decodes");
    assert_eq!(conflict.conflict_kind(), ConflictKind::ReservationOverlap);
    assert_eq!(conflict.conflict_kind(), "reservation_overlap");

    let unknown_kind = json!({
        "conflict_id": "conflict-1",
        "object_type": "reservation",
        "object_id": "reservation-1",
        "conflict_kind": "overlap",
        "payload_json": valid_payload.to_string(),
        "detected_at": 100,
        "resolution_state": "open"
    });
    serde_json::from_value::<Conflict>(unknown_kind)
        .expect_err("unknown conflict kind must be rejected");

    let wrong_object = json!({
        "conflict_id": "conflict-1",
        "object_type": "subtask",
        "object_id": "subtask-1",
        "conflict_kind": "reservation_overlap",
        "payload_json": valid_payload.to_string(),
        "detected_at": 100,
        "resolution_state": "open"
    });
    let wrong_object_err = serde_json::from_value::<Conflict>(wrong_object)
        .expect_err("reservation overlap conflicts must target reservations");
    assert!(
        wrong_object_err
            .to_string()
            .contains("reservation_overlap conflicts must target reservation objects"),
        "unexpected error: {wrong_object_err}"
    );

    let wrong_payload = json!({
        "conflict_id": "conflict-1",
        "object_type": "reservation",
        "object_id": "reservation-1",
        "conflict_kind": "reservation_overlap",
        "payload_json": "{}",
        "detected_at": 100,
        "resolution_state": "open"
    });
    let wrong_payload_err = serde_json::from_value::<Conflict>(wrong_payload)
        .expect_err("reservation overlap conflicts require their typed payload");
    assert!(
        wrong_payload_err
            .to_string()
            .contains("reservation_overlap conflicts require typed payload"),
        "unexpected error: {wrong_payload_err}"
    );
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
fn reservation_request_and_overlap_query_reject_invalid_scope_shapes() {
    let exact_request_with_generated_members = serde_json::json!({
        "session_token": "session-1",
        "owner_subtask_id": "subtask-1",
        "scope_class": "exact_path",
        "scope_key": "src/lib.rs",
        "generated_members": ["generated.rs"],
        "lease_duration_ms": 60_000,
        "idempotency_key": "idem-reservation"
    });
    let err = serde_json::from_value::<RequestReservationReq>(exact_request_with_generated_members)
        .expect_err("exact-path reservation request with generated members must be rejected");
    assert!(
        err.to_string()
            .contains("exact_path reservations must not include generated_members"),
        "unexpected error: {err}"
    );

    let generated_set_request_without_members = serde_json::json!({
        "session_token": "session-1",
        "owner_subtask_id": "subtask-1",
        "scope_class": "generated_set",
        "scope_key": "artifact-manifest",
        "generated_members": [],
        "lease_duration_ms": 60_000,
        "idempotency_key": "idem-reservation"
    });
    let err =
        serde_json::from_value::<RequestReservationReq>(generated_set_request_without_members)
            .expect_err("generated-set reservation request without members must be rejected");
    assert!(
        err.to_string()
            .contains("generated-set reservations require generated_members"),
        "unexpected error: {err}"
    );

    let repo_global_query_wrong_key = serde_json::json!({
        "scope_class": "repo_global",
        "scope_key": "src",
        "generated_members": []
    });
    let err = serde_json::from_value::<OverlapQueryReq>(repo_global_query_wrong_key)
        .expect_err("repo-global overlap query must use canonical scope key");
    assert!(
        err.to_string()
            .contains("repo-global reservations require scope_key `repo`"),
        "unexpected error: {err}"
    );
}

#[test]
fn reservation_mutation_payloads_reject_invalid_ids_and_leases() {
    let invalid_release_id = serde_json::json!({
        "session_token": "session-1",
        "reservation_id": "",
        "idempotency_key": "idem-release-reservation"
    });
    serde_json::from_value::<ReleaseReservationReq>(invalid_release_id)
        .expect_err("release reservation should reject invalid reservation ids");

    let invalid_renew_id = serde_json::json!({
        "session_token": "session-1",
        "reservation_id": "",
        "extend_by_ms": 10_000,
        "idempotency_key": "idem-renew-reservation"
    });
    serde_json::from_value::<RenewReservationReq>(invalid_renew_id)
        .expect_err("renew reservation should reject invalid reservation ids");

    let invalid_renew_duration = serde_json::json!({
        "session_token": "session-1",
        "reservation_id": "reservation-1",
        "extend_by_ms": 0,
        "idempotency_key": "idem-renew-reservation"
    });
    serde_json::from_value::<RenewReservationReq>(invalid_renew_duration)
        .expect_err("renew reservation should reject non-positive extension durations");
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
    )
    .expect("coherent event should construct");

    let typed = event.typed().expect("event payload must decode");

    assert_eq!(typed.seq, 42);
    assert_eq!(typed.event_type(), EventType::SessionHeartbeat);
    assert_eq!(typed.object_type(), ObjectType::Session);
    assert_eq!(typed.object_id, "session-1");
    assert_eq!(typed.payload, EventPayload::SessionHeartbeat(payload));
}

#[test]
fn event_typed_rejects_object_type_that_disagrees_with_payload() {
    let payload = HeartbeatReq {
        session_token: "session-1".to_owned(),
        idempotency_key: "idem-1".to_owned(),
    };
    let err = Event::session(
        42,
        EventType::SessionHeartbeat,
        ObjectType::Claim,
        "session-1".to_owned(),
        SessionToken::parse("session-1").expect("valid session token"),
        serde_json::to_string(&payload).expect("heartbeat serialization must succeed"),
        TimestampMs::parse(1_234).expect("valid timestamp"),
    )
    .expect_err("event metadata must match decoded payload");

    assert!(
        err.to_string().contains("object_type session"),
        "error should explain the payload-implied object type: {err}"
    );
}

#[test]
fn typed_event_json_rejects_metadata_that_disagrees_with_payload() {
    let payload = EventPayload::SessionHeartbeat(HeartbeatReq {
        session_token: "session-1".to_owned(),
        idempotency_key: "idem-1".to_owned(),
    });
    let mut event_json = json!({
        "seq": 42,
        "event_type": EventType::SessionHeartbeat,
        "object_type": ObjectType::Session,
        "object_id": "session-1",
        "actor_kind": "session",
        "session_token": "session-1",
        "payload": payload,
        "created_at": 1_234
    });

    let typed: TypedEvent =
        serde_json::from_value(event_json.clone()).expect("coherent typed event should decode");
    assert_eq!(typed.event_type(), EventType::SessionHeartbeat);
    assert_eq!(typed.object_type(), ObjectType::Session);

    event_json["event_type"] = json!(EventType::SessionExited);
    let err = serde_json::from_value::<TypedEvent>(event_json.clone())
        .expect_err("event_type must match typed payload variant");
    assert!(
        err.to_string().contains("payload implies event_type"),
        "unexpected error: {err}"
    );

    event_json["event_type"] = json!(EventType::SessionHeartbeat);
    event_json["object_type"] = json!(ObjectType::Claim);
    let err = serde_json::from_value::<TypedEvent>(event_json)
        .expect_err("object_type must match typed payload variant");
    assert!(
        err.to_string().contains("payload implies object_type"),
        "unexpected error: {err}"
    );
}

#[test]
fn event_typed_propagates_payload_decode_failures() {
    let err = Event::session(
        1,
        EventType::SessionHeartbeat,
        ObjectType::Session,
        "session-1".to_owned(),
        SessionToken::parse("session-1").expect("valid session token"),
        "{".to_owned(),
        TimestampMs::parse(99).expect("valid timestamp"),
    )
    .expect_err("malformed payload must fail to construct");

    assert!(
        err.contains("event payload does not match session_heartbeat"),
        "unexpected error: {err}"
    );
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

    let metatask_conflict_action = r#"{"object_type":"meta_task","object_id":"meta-1","openspec_task_id":null,"title":"Change prompt","task_type":null,"task_digest":null,"source_path":null,"action":"conflict"}"#;
    let meta_conflict_err =
        serde_json::from_str::<ImportOpenSpecItemResult>(metatask_conflict_action)
            .expect_err("metatask import items cannot be conflicts");
    assert!(
        meta_conflict_err
            .to_string()
            .contains("metatask OpenSpec import items cannot use conflict action")
    );
    let meta_conflict_constructor_err = ImportOpenSpecItemResult::meta_task(
        "meta-1",
        "Change prompt",
        ImportOpenSpecAction::Conflict,
    )
    .expect_err("constructor must reject metatask conflicts");
    assert!(
        meta_conflict_constructor_err.contains("cannot use conflict action"),
        "unexpected error: {meta_conflict_constructor_err}"
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

#[test]
fn openspec_source_digest_rejects_invalid_path_and_digest_shapes() {
    let digest = OpenSpecSourceDigest::new("openspec/changes/change-1/tasks.md", "blake3:abc")
        .expect("valid source digest");
    assert_eq!(digest.path(), "openspec/changes/change-1/tasks.md");
    assert_eq!(digest.digest(), "blake3:abc");
    let value = serde_json::to_value(&digest).expect("source digest serializes");
    assert_eq!(value["path"], "openspec/changes/change-1/tasks.md");
    assert_eq!(value["digest"], "blake3:abc");

    let empty_path = json!({"path": "", "digest": "blake3:abc"});
    let empty_path_err = serde_json::from_value::<OpenSpecSourceDigest>(empty_path)
        .expect_err("empty source digest path must be rejected");
    assert!(
        empty_path_err
            .to_string()
            .contains("path must not be empty"),
        "unexpected error: {empty_path_err}"
    );

    let absolute_path = json!({"path": "/openspec/tasks.md", "digest": "blake3:abc"});
    let absolute_path_err = serde_json::from_value::<OpenSpecSourceDigest>(absolute_path)
        .expect_err("absolute source digest path must be rejected");
    assert!(
        absolute_path_err
            .to_string()
            .contains("path must be relative"),
        "unexpected error: {absolute_path_err}"
    );

    let escaping_path = json!({"path": "openspec/../tasks.md", "digest": "blake3:abc"});
    let escaping_path_err = serde_json::from_value::<OpenSpecSourceDigest>(escaping_path)
        .expect_err("escaping source digest path must be rejected");
    assert!(
        escaping_path_err
            .to_string()
            .contains("path must not escape upward"),
        "unexpected error: {escaping_path_err}"
    );

    let wrong_digest = json!({"path": "tasks.md", "digest": "sha256:abc"});
    let wrong_digest_err = serde_json::from_value::<OpenSpecSourceDigest>(wrong_digest)
        .expect_err("source digest must use blake3");
    assert!(
        wrong_digest_err
            .to_string()
            .contains("must use blake3: prefix"),
        "unexpected error: {wrong_digest_err}"
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
            OpenSpecImportProvenanceCommon::new(
                "subtask-1",
                "change-1",
                "openspec/changes/change-1",
                "blake3:tasks",
                vec![
                    OpenSpecSourceDigest::new("tasks.md".to_owned(), "blake3:tasks".to_owned())
                        .expect("valid source digest"),
                ],
                vec![],
                vec![],
                TimestampMs::parse(123).expect("valid timestamp"),
            ),
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
    let claim = ClaimResult::new(
        ClaimId::parse("claim-1").expect("valid claim id"),
        SubtaskId::parse("subtask-1").expect("valid subtask id"),
        FenceSeq::parse(1).expect("valid fence"),
        LeaseDeadlineMs::parse(500).expect("valid deadline"),
    );
    let start = StartSubtaskReq {
        session_token: "session-1".to_owned(),
        claim_id: ClaimId::parse("claim-1").expect("valid claim id"),
        fence_seq: FenceSeq::parse(1).expect("valid fence"),
        idempotency_key: "idem-start".to_owned(),
    };
    let release = ReleaseClaimReq {
        session_token: "session-1".to_owned(),
        claim_id: ClaimId::parse("claim-1").expect("valid claim id"),
        fence_seq: FenceSeq::parse(1).expect("valid fence"),
        idempotency_key: "idem-release".to_owned(),
    };
    let artifact = PublishArtifactReq::try_from_raw_parts(
        "session-1".to_owned(),
        ClaimId::parse("claim-1").expect("valid claim id"),
        FenceSeq::parse(1).expect("valid fence"),
        "blake3:artifact".to_owned(),
        ArtifactKind::PatchBundle,
        "base".to_owned(),
        "manifest.json".to_owned(),
        "blake3:paths".to_owned(),
        "idem-artifact".to_owned(),
    )
    .expect("valid artifact publication request");
    let review_request = RequestReviewReq::try_from_raw_parts(
        "session-1",
        "subtask-1",
        "blake3:artifact",
        Some("review-subtask-1".to_owned()),
        5,
        "idem-review",
    )
    .expect("valid review request");
    let review_decision = DecideReviewReq::try_from_raw_parts(
        "session-reviewer".to_owned(),
        "review-1".to_owned(),
        ClaimId::parse("claim-review").expect("valid claim id"),
        FenceSeq::parse(1).expect("valid fence"),
        ReviewVerdict::Approve,
        "blake3:findings".to_owned(),
        "idem-decision".to_owned(),
    )
    .expect("valid review decision request");
    let enqueue = EnqueueForApplyReq::try_from_raw_parts(
        "session-1".to_owned(),
        "blake3:artifact".to_owned(),
        "subtask-1".to_owned(),
        SettlementTarget::Canonical,
        "idem-enqueue".to_owned(),
    )
    .expect("valid enqueue-for-apply request");
    let queue_claim = ReadyQueueClaim::new(
        QueueId::parse("queue-1").expect("valid queue id"),
        ArtifactDigest::parse("blake3:artifact").expect("valid artifact digest"),
        SubtaskId::parse("subtask-1").expect("valid subtask id"),
        SettlementTarget::Canonical,
        FenceSeq::parse(1).expect("valid fence"),
        LeaseDeadlineMs::parse(900).expect("valid deadline"),
    );
    let mark_applied = MarkAppliedReq {
        session_token: "session-1".to_owned(),
        queue_id: QueueId::parse("queue-1").expect("valid queue id"),
        claim_fence_seq: FenceSeq::parse(1).expect("valid fence"),
        idempotency_key: "idem-applied".to_owned(),
    };
    let reservation_request = RequestReservationReq::try_from_raw_parts(
        "session-1",
        "subtask-1",
        ScopeClass::ExactPath,
        "src/lib.rs",
        vec![],
        60_000,
        "idem-reservation",
    )
    .expect("valid reservation request");
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

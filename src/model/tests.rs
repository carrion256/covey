use super::{
    AbandonSubtaskReq, Artifact, ArtifactDigest, ArtifactKind, CancelMetaTaskReq, Claim, ClaimId,
    ClaimNextReq, ClaimReadyQueueReq, ClaimResult, ClaimState, ClaimSubtaskReq, Conflict,
    ConflictKind, ConflictResolutionState, CreateSubtaskRequest, DecideReviewReq,
    EnqueueForApplyReq, Event, EventPayload, EventType, ExitSessionReq, ExpiredCountPayload,
    ExpiringClaim, FenceSeq, HeartbeatReq, IdempotencyKey, ImportBdV1Req, ImportBdV1Result,
    ImportBdV1SkipReason, ImportOpenSpecAction, ImportOpenSpecConflict,
    ImportOpenSpecConflictReason, ImportOpenSpecEvent, ImportOpenSpecItemResult, ImportOpenSpecReq,
    ImportOpenSpecResult, LandingAuthorizationStatus, LeaseDeadlineMs, MarkAppliedReq,
    MarkInFlightReq, MetaTask, MetaTaskId, MetaTaskState, MetaTaskStatus, ObjectType,
    OpenSpecImportProvenance, OpenSpecImportProvenanceCommon, OpenSpecMissionArtifactMetadata,
    OpenSpecSourceDigest, OpenSpecTaskId, OverlapQueryReq, PublishArtifactReq, QueueId,
    ReadyQueueClaim, ReadyQueueItem, ReadyQueueMetrics, ReadyQueueState,
    RecordApplyVerificationReq, RecordLandingReceiptReq, RecordRuntimeAttestationReq,
    RegisterSessionReq, ReleaseClaimReq, ReleaseReservationReq, RenewClaimReq, RenewReservationReq,
    RepoopsAuthorityClaimFact, RepoopsAuthorityGitContextFact, RepoopsAuthorityLockFact,
    RepoopsAuthorityScopeFact, RepoopsAuthoritySnapshotReq, RepoopsClaimRef, RequestReservationReq,
    RequestReviewReq, Reservation, ReservationId, ReservationOverlapConflictPayload,
    ReservationScope, ReservationState, ResolveConflictReq, Review, ReviewSubtask, ReviewTarget,
    ReviewVerdict, ScopeClass, Session, SessionHandle, SessionRole, SessionState, SessionStatus,
    SessionToken, SettlementTarget, StaleSessionsPayload, StartSubtaskReq, StuckSubtask,
    SubmitMetaTaskReq, Subtask, SubtaskId, SubtaskKind, SubtaskLifecycle, SubtaskPriority,
    SubtaskRow, SubtaskState, SubtaskStatus, SubtaskTitle, SubtaskView, SupersedeQueueItemReq,
    TimestampMs, TypedEvent, VerifyLandingAuthorizationReq, WorkSubtask, bd_import_v1_subtask_id,
    make_id, parse_generated_members,
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
fn session_domain_rejects_invalid_identity_and_heartbeat_tick() {
    let mut invalid_principal = session_json(SessionState::Active, None);
    invalid_principal["agent_principal_id"] = json!("principal 1");
    let err = serde_json::from_value::<Session>(invalid_principal)
        .expect_err("agent principal ids must be token-shaped");
    assert!(
        err.to_string().contains("invalid agent_principal_id"),
        "unexpected error: {err}"
    );

    let mut invalid_instance = session_json(SessionState::Active, None);
    invalid_instance["agent_instance_id"] = json!("");
    let err = serde_json::from_value::<Session>(invalid_instance)
        .expect_err("agent instance ids must be token-shaped");
    assert!(
        err.to_string().contains("invalid agent_instance_id"),
        "unexpected error: {err}"
    );

    let mut invalid_tick = session_json(SessionState::Active, None);
    invalid_tick["last_heartbeat_tick"] = json!(-1);
    let err = serde_json::from_value::<Session>(invalid_tick)
        .expect_err("session heartbeat ticks must be non-negative");
    assert!(
        err.to_string().contains("invalid last_heartbeat_tick"),
        "unexpected error: {err}"
    );
}

#[test]
fn session_domain_rejects_non_monotonic_timestamps() {
    let mut stale_heartbeat = session_json(SessionState::Active, None);
    stale_heartbeat["last_heartbeat_at"] = json!(99);
    stale_heartbeat["created_at"] = json!(100);
    let err = serde_json::from_value::<Session>(stale_heartbeat)
        .expect_err("session heartbeat before creation must be rejected");
    assert!(
        err.to_string()
            .contains("session last_heartbeat_at must be greater than or equal to created_at"),
        "unexpected error: {err}"
    );

    let mut stale_update = session_json(SessionState::Active, None);
    stale_update["created_at"] = json!(200);
    stale_update["last_heartbeat_at"] = json!(200);
    stale_update["updated_at"] = json!(100);
    let err = serde_json::from_value::<Session>(stale_update)
        .expect_err("session update before creation must be rejected");
    assert!(
        err.to_string()
            .contains("session updated_at must be greater than or equal to created_at"),
        "unexpected error: {err}"
    );

    let err = Session::try_from_parts(
        SessionToken::parse("session-1").expect("valid session token"),
        "principal-1",
        "instance-1",
        SessionRole::Executor,
        SessionState::Active,
        None,
        TimestampMs::parse(99).expect("valid heartbeat timestamp"),
        1,
        TimestampMs::parse(100).expect("valid created timestamp"),
        TimestampMs::parse(100).expect("valid updated timestamp"),
    )
    .expect_err("session constructor must reject heartbeat before creation");
    assert!(
        err.contains("session last_heartbeat_at must be greater than or equal to created_at"),
        "unexpected error: {err}"
    );
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
    let row = work_subtask_row(
        SubtaskState::Claimed,
        Some(ClaimId::parse("claim-1").expect("valid claim id")),
        None,
    );

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
fn subtask_row_preserves_flat_storage_shape() {
    let row = work_subtask_row(
        SubtaskState::Claimed,
        Some(ClaimId::parse("claim-1").expect("valid claim id")),
        None,
    );
    let value = serde_json::to_value(&row).expect("subtask row should serialize");

    assert_eq!(value["state"], "claimed");
    assert_eq!(value["current_claim_id"], "claim-1");
    assert_eq!(value["artifact_digest"], serde_json::Value::Null);
}

#[test]
fn subtask_row_preserves_flat_review_target_shape() {
    let row = SubtaskRow::try_from_parts(
        SubtaskId::parse("review-1").expect("valid subtask id"),
        MetaTaskId::parse("meta-1").expect("valid meta-task id"),
        "review artifact".to_owned(),
        SubtaskKind::Review,
        Some(SubtaskId::parse("subtask-1").expect("valid subtask id")),
        Some(ArtifactDigest::parse("blake3:artifact").expect("valid digest")),
        SubtaskState::Available,
        None,
        None,
        SubtaskPriority::parse(10).expect("valid subtask priority"),
        TimestampMs::parse(100).expect("valid timestamp"),
        TimestampMs::parse(200).expect("valid timestamp"),
    )
    .expect("valid review subtask row");
    let value = serde_json::to_value(&row).expect("subtask row should serialize");

    assert_eq!(row.kind(), SubtaskKind::Review);
    assert_eq!(
        row.review_target().map(|target| target.subtask_id.as_str()),
        Some("subtask-1")
    );
    assert_eq!(value["kind"], "review");
    assert_eq!(value["review_target_subtask_id"], "subtask-1");
    assert_eq!(value["review_target_artifact_digest"], "blake3:artifact");
}

#[test]
fn subtask_row_deserialization_rejects_invalid_lifecycle_shape() {
    let invalid = json!({
        "subtask_id": "subtask-1",
        "meta_task_id": "meta-1",
        "title": "implement",
        "kind": "work",
        "review_target_subtask_id": null,
        "review_target_artifact_digest": null,
        "state": "available",
        "current_claim_id": "claim-1",
        "artifact_digest": "blake3:artifact",
        "priority": 10,
        "created_at": 100,
        "updated_at": 200
    });

    let err = serde_json::from_value::<SubtaskRow>(invalid)
        .expect_err("available subtask rows must reject stale lifecycle fields");

    assert!(
        err.to_string()
            .contains("available subtask cannot carry claim or artifact state"),
        "unexpected error: {err}"
    );
}

#[test]
fn subtask_row_deserialization_rejects_invalid_kind_target_shape() {
    let work_with_target = json!({
        "subtask_id": "subtask-1",
        "meta_task_id": "meta-1",
        "title": "implement",
        "kind": "work",
        "review_target_subtask_id": "subtask-2",
        "review_target_artifact_digest": "blake3:artifact",
        "state": "available",
        "current_claim_id": null,
        "artifact_digest": null,
        "priority": 10,
        "created_at": 100,
        "updated_at": 200
    });
    let err = serde_json::from_value::<SubtaskRow>(work_with_target)
        .expect_err("work subtask rows must reject review targets");
    assert!(
        err.to_string().contains("work subtask has a review target"),
        "unexpected error: {err}"
    );

    let review_without_target = json!({
        "subtask_id": "review-1",
        "meta_task_id": "meta-1",
        "title": "review",
        "kind": "review",
        "review_target_subtask_id": null,
        "review_target_artifact_digest": null,
        "state": "available",
        "current_claim_id": null,
        "artifact_digest": null,
        "priority": 10,
        "created_at": 100,
        "updated_at": 200
    });
    let err = serde_json::from_value::<SubtaskRow>(review_without_target)
        .expect_err("review subtask rows must require review targets");
    assert!(
        err.to_string()
            .contains("review subtask is missing target subtask"),
        "unexpected error: {err}"
    );
}

#[test]
fn subtask_row_and_view_reject_invalid_priority() {
    let row = json!({
        "subtask_id": "subtask-1",
        "meta_task_id": "meta-1",
        "title": "implement",
        "kind": "work",
        "review_target_subtask_id": null,
        "review_target_artifact_digest": null,
        "state": "available",
        "current_claim_id": null,
        "artifact_digest": null,
        "priority": 1001,
        "created_at": 100,
        "updated_at": 200
    });
    let err = serde_json::from_value::<SubtaskRow>(row)
        .expect_err("subtask rows must reject priorities outside the schema range");
    assert!(
        err.to_string().contains("invalid priority"),
        "unexpected error: {err}"
    );

    let view = json!({
        "subtask_id": "subtask-1",
        "meta_task_id": "meta-1",
        "title": "implement",
        "kind": "work",
        "review_target": null,
        "state": "available",
        "active_claim_id": null,
        "artifact_digest": null,
        "priority": -1,
        "created_at": 100,
        "updated_at": 200
    });
    let err = serde_json::from_value::<SubtaskView>(view)
        .expect_err("subtask views must reject priorities outside the schema range");
    assert!(
        err.to_string().contains("invalid priority"),
        "unexpected error: {err}"
    );
}

#[test]
fn subtask_view_rejects_non_monotonic_timestamps() {
    let view = json!({
        "subtask_id": "subtask-1",
        "meta_task_id": "meta-1",
        "title": "implement",
        "kind": "work",
        "review_target": null,
        "state": "available",
        "active_claim_id": null,
        "artifact_digest": null,
        "priority": 10,
        "created_at": 200,
        "updated_at": 100
    });

    let err = serde_json::from_value::<SubtaskView>(view)
        .expect_err("subtask views must reject updates before creation");
    assert!(
        err.to_string()
            .contains("subtask view updated_at must be greater than or equal to created_at"),
        "unexpected error: {err}"
    );
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
    let err = try_work_subtask_row(SubtaskState::Claimed, None, None)
        .expect_err("claimed subtask without claim must be rejected at the row boundary");

    assert!(
        err.to_string()
            .contains("claimed subtask is missing active claim"),
        "unexpected error: {err}"
    );
}

#[test]
fn subtask_lifecycle_rejects_stale_fields_on_available_state() {
    let err = try_work_subtask_row(
        SubtaskState::Available,
        Some(ClaimId::parse("claim-1").expect("valid claim id")),
        Some(ArtifactDigest::parse("blake3:artifact").expect("valid digest")),
    )
    .expect_err(
        "available subtask with claim or artifact state must be rejected at the row boundary",
    );

    assert!(
        err.to_string()
            .contains("available subtask cannot carry claim or artifact state"),
        "unexpected error: {err}"
    );
}

#[test]
fn subtask_lifecycle_requires_artifact_for_artifact_published_state() {
    let err = try_work_subtask_row(
        SubtaskState::ArtifactPublished,
        Some(ClaimId::parse("claim-1").expect("valid claim id")),
        None,
    )
    .expect_err("artifact-published subtask without artifact must be rejected at the row boundary");

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
fn blocked_work_lifecycle_preserves_failed_artifact() {
    let artifact_digest = ArtifactDigest::parse("blake3:blocked-artifact").expect("valid digest");
    let row = work_subtask_row(SubtaskState::Blocked, None, Some(artifact_digest.clone()));

    let domain = Subtask::try_from(row).expect("blocked work row with evidence must be valid");

    assert_eq!(domain.lifecycle().state(), SubtaskState::Blocked);
    assert_eq!(domain.lifecycle().artifact_digest(), Some(&artifact_digest));
}

#[test]
fn blocked_work_lifecycle_requires_failed_artifact() {
    let err = try_work_subtask_row(SubtaskState::Blocked, None, None)
        .expect_err("blocked work without failed artifact evidence must be rejected");

    assert!(
        err.to_string()
            .contains("blocked subtask is missing artifact digest"),
        "unexpected error: {err}"
    );
}

#[test]
fn subtask_row_rejects_non_monotonic_timestamps() {
    let err = SubtaskRow::try_from_parts(
        SubtaskId::parse("subtask-1").expect("valid subtask id"),
        MetaTaskId::parse("meta-1").expect("valid meta-task id"),
        "implement".to_owned(),
        SubtaskKind::Work,
        None,
        None,
        SubtaskState::Available,
        None,
        None,
        SubtaskPriority::parse(10).expect("valid subtask priority"),
        TimestampMs::parse(200).expect("valid timestamp"),
        TimestampMs::parse(100).expect("valid timestamp"),
    )
    .expect_err("subtask row updated_at before created_at must be rejected");

    assert!(
        err.to_string()
            .contains("subtask updated_at must be greater than or equal to created_at"),
        "unexpected error: {err}"
    );
}

#[test]
fn work_subtask_domain_rejects_review_only_lifecycle() {
    let err = WorkSubtask::new(
        SubtaskId::parse("subtask-1").expect("valid subtask id"),
        MetaTaskId::parse("meta-1").expect("valid meta-task id"),
        "implement".to_owned(),
        SubtaskLifecycle::Decided,
        SubtaskPriority::parse(10).expect("valid subtask priority"),
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
fn work_subtask_domain_rejects_non_monotonic_timestamps() {
    let err = WorkSubtask::new(
        SubtaskId::parse("subtask-1").expect("valid subtask id"),
        MetaTaskId::parse("meta-1").expect("valid meta-task id"),
        "implement".to_owned(),
        SubtaskLifecycle::Available,
        SubtaskPriority::parse(10).expect("valid subtask priority"),
        TimestampMs::parse(200).expect("valid timestamp"),
        TimestampMs::parse(100).expect("valid timestamp"),
    )
    .expect_err("work subtask updated_at before created_at must be rejected");

    assert!(
        err.to_string()
            .contains("subtask updated_at must be greater than or equal to created_at"),
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
        SubtaskPriority::parse(10).expect("valid subtask priority"),
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
fn review_subtask_domain_rejects_non_monotonic_timestamps() {
    let err = ReviewSubtask::new(
        SubtaskId::parse("review-1").expect("valid subtask id"),
        MetaTaskId::parse("meta-1").expect("valid meta-task id"),
        "review artifact".to_owned(),
        ReviewTarget::new(
            SubtaskId::parse("subtask-1").expect("valid subtask id"),
            ArtifactDigest::parse("blake3:artifact").expect("valid digest"),
        ),
        SubtaskLifecycle::Available,
        SubtaskPriority::parse(10).expect("valid subtask priority"),
        TimestampMs::parse(200).expect("valid timestamp"),
        TimestampMs::parse(100).expect("valid timestamp"),
    )
    .expect_err("review subtask updated_at before created_at must be rejected");

    assert!(
        err.to_string()
            .contains("subtask updated_at must be greater than or equal to created_at"),
        "unexpected error: {err}"
    );
}

#[test]
fn review_subtask_domain_requires_a_target_artifact() {
    let row = SubtaskRow::try_from_parts(
        SubtaskId::parse("review-1").expect("valid subtask id"),
        MetaTaskId::parse("meta-1").expect("valid meta-task id"),
        "review artifact".to_owned(),
        SubtaskKind::Review,
        Some(SubtaskId::parse("subtask-1").expect("valid subtask id")),
        Some(ArtifactDigest::parse("blake3:artifact").expect("valid digest")),
        SubtaskState::Available,
        None,
        None,
        SubtaskPriority::parse(10).expect("valid subtask priority"),
        TimestampMs::parse(100).expect("valid timestamp"),
        TimestampMs::parse(200).expect("valid timestamp"),
    )
    .expect("valid review row");

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
    let invalid_session_token = json!({
        "session_token": "",
        "provider": "provider-1",
        "model": "model-1",
        "provider_run_id": "run-1",
        "provider_run_id_issuer": "issuer-1",
        "process_id": "process-1",
        "container_id": null,
        "command_transcript_digest": "blake3:transcript",
        "started_at": 100,
        "ended_at": 101,
        "idempotency_key": "idem-1"
    });
    serde_json::from_value::<RecordRuntimeAttestationReq>(invalid_session_token)
        .expect_err("runtime attestation request should reject invalid session tokens");

    let invalid_command_transcript_digest = json!({
        "session_token": "session-1",
        "provider": "provider-1",
        "model": "model-1",
        "provider_run_id": "run-1",
        "provider_run_id_issuer": "issuer-1",
        "process_id": "process-1",
        "container_id": null,
        "command_transcript_digest": "transcript",
        "started_at": 100,
        "ended_at": 101,
        "idempotency_key": "idem-1"
    });
    serde_json::from_value::<RecordRuntimeAttestationReq>(invalid_command_transcript_digest)
        .expect_err("runtime attestation request should reject invalid transcript digests");

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

    let padded_process = json!({
        "session_token": "session-1",
        "provider": "provider-1",
        "model": "model-1",
        "provider_run_id": "run-1",
        "provider_run_id_issuer": "issuer-1",
        "process_id": "process-1 ",
        "container_id": null,
        "command_transcript_digest": "blake3:transcript",
        "started_at": 100,
        "ended_at": 101,
        "idempotency_key": "idem-1"
    });
    let padded_process_err = serde_json::from_value::<RecordRuntimeAttestationReq>(padded_process)
        .expect_err("padded process identity must be rejected");
    assert!(
        padded_process_err
            .to_string()
            .contains("process_id must not include leading or trailing whitespace"),
        "unexpected error: {padded_process_err}"
    );

    let blank_provider_run_id = json!({
        "session_token": "session-1",
        "provider": "provider-1",
        "model": "model-1",
        "provider_run_id": " ",
        "provider_run_id_issuer": "issuer-1",
        "process_id": "process-1",
        "container_id": null,
        "command_transcript_digest": "blake3:transcript",
        "started_at": 100,
        "ended_at": 101,
        "idempotency_key": "idem-1"
    });
    let blank_provider_run_id_err =
        serde_json::from_value::<RecordRuntimeAttestationReq>(blank_provider_run_id)
            .expect_err("provider run ids must be non-empty");
    assert!(
        blank_provider_run_id_err
            .to_string()
            .contains("provider_run_id must not be empty"),
        "unexpected error: {blank_provider_run_id_err}"
    );

    let padded_provider_run_id = json!({
        "session_token": "session-1",
        "provider": "provider-1",
        "model": "model-1",
        "provider_run_id": " run-1",
        "provider_run_id_issuer": "issuer-1",
        "process_id": "process-1",
        "container_id": null,
        "command_transcript_digest": "blake3:transcript",
        "started_at": 100,
        "ended_at": 101,
        "idempotency_key": "idem-1"
    });
    let padded_provider_run_id_err =
        serde_json::from_value::<RecordRuntimeAttestationReq>(padded_provider_run_id)
            .expect_err("padded provider run ids must be rejected");
    assert!(
        padded_provider_run_id_err
            .to_string()
            .contains("provider_run_id must not include leading or trailing whitespace"),
        "unexpected error: {padded_provider_run_id_err}"
    );

    let negative_started_at = json!({
        "session_token": "session-1",
        "provider": "provider-1",
        "model": "model-1",
        "provider_run_id": "run-1",
        "provider_run_id_issuer": "issuer-1",
        "process_id": "process-1",
        "container_id": null,
        "command_transcript_digest": "blake3:transcript",
        "started_at": -1,
        "ended_at": 101,
        "idempotency_key": "idem-1"
    });
    let negative_started_at_err =
        serde_json::from_value::<RecordRuntimeAttestationReq>(negative_started_at)
            .expect_err("runtime timestamps must be non-negative");
    assert!(
        negative_started_at_err
            .to_string()
            .contains("invalid timestamp_ms"),
        "unexpected error: {negative_started_at_err}"
    );

    let inverted_timestamps = json!({
        "session_token": "session-1",
        "provider": "provider-1",
        "model": "model-1",
        "provider_run_id": "run-1",
        "provider_run_id_issuer": "issuer-1",
        "process_id": "process-1",
        "container_id": null,
        "command_transcript_digest": "blake3:transcript",
        "started_at": 102,
        "ended_at": 101,
        "idempotency_key": "idem-1"
    });
    let inverted_timestamps_err =
        serde_json::from_value::<RecordRuntimeAttestationReq>(inverted_timestamps)
            .expect_err("runtime end must not predate runtime start");
    assert!(
        inverted_timestamps_err
            .to_string()
            .contains("ended_at must be greater than or equal to started_at"),
        "unexpected error: {inverted_timestamps_err}"
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
    assert_eq!(req.provider_run_id(), "run-1");
    assert_eq!(req.provider_run_id_issuer(), "issuer-1");
    assert_eq!(req.started_at().get(), 100);
    assert_eq!(req.ended_at().get(), 101);

    let value = serde_json::to_value(&req).expect("request serializes to flat JSON");
    assert_eq!(value["process_id"], "process-1");
    assert_eq!(value["container_id"], "container-1");
    assert_eq!(value["started_at"], 100);
    assert_eq!(value["ended_at"], 101);
}

#[test]
fn register_session_request_rejects_invalid_agent_identity() {
    let req = RegisterSessionReq::try_from_raw_parts(
        "principal-1",
        "instance-1",
        SessionRole::Executor,
        "idem-register",
    )
    .expect("valid registration request");
    assert_eq!(req.agent_principal_id(), "principal-1");
    assert_eq!(req.agent_instance_id(), "instance-1");

    let value = serde_json::to_value(&req).expect("registration request serializes");
    assert_eq!(value["agent_principal_id"], "principal-1");
    assert_eq!(value["agent_instance_id"], "instance-1");

    let decoded: RegisterSessionReq =
        serde_json::from_value(value).expect("flat registration request deserializes");
    assert_eq!(decoded, req);

    let invalid_principal = json!({
        "agent_principal_id": "principal 1",
        "agent_instance_id": "instance-1",
        "role": SessionRole::Executor,
        "idempotency_key": "idem-register"
    });
    let err = serde_json::from_value::<RegisterSessionReq>(invalid_principal)
        .expect_err("agent principal ids must be token-shaped");
    assert!(
        err.to_string().contains("invalid agent_principal_id"),
        "unexpected error: {err}"
    );

    let invalid_instance = RegisterSessionReq::try_from_raw_parts(
        "principal-1",
        "",
        SessionRole::Executor,
        "idem-register",
    )
    .expect_err("agent instance ids must be token-shaped");
    assert!(
        invalid_instance
            .to_string()
            .contains("invalid agent_instance_id"),
        "unexpected error: {invalid_instance}"
    );
}

#[test]
fn session_lifecycle_requests_reject_invalid_session_tokens() {
    let heartbeat = HeartbeatReq::try_from_raw_parts("session-1", "idem-heartbeat")
        .expect("valid heartbeat request");
    assert_eq!(heartbeat.session_token, "session-1");
    let value = serde_json::to_value(&heartbeat).expect("heartbeat request serializes");
    assert_eq!(value["session_token"], "session-1");
    let decoded: HeartbeatReq =
        serde_json::from_value(value).expect("heartbeat request deserializes");
    assert_eq!(decoded, heartbeat);

    let invalid_heartbeat = json!({
        "session_token": "session 1",
        "idempotency_key": "idem-heartbeat"
    });
    let err = serde_json::from_value::<HeartbeatReq>(invalid_heartbeat)
        .expect_err("heartbeat session tokens must be token-shaped");
    assert!(
        err.to_string().contains("invalid session_token"),
        "unexpected error: {err}"
    );

    let exit =
        ExitSessionReq::try_from_raw_parts("session-1", "idem-exit").expect("valid exit request");
    assert_eq!(exit.session_token, "session-1");
    let value = serde_json::to_value(&exit).expect("exit request serializes");
    assert_eq!(value["session_token"], "session-1");
    let decoded: ExitSessionReq = serde_json::from_value(value).expect("exit request deserializes");
    assert_eq!(decoded, exit);

    let err = ExitSessionReq::try_from_raw_parts("", "idem-exit")
        .expect_err("exit session tokens must be token-shaped");
    assert!(
        err.to_string().contains("invalid session_token"),
        "unexpected error: {err}"
    );

    let err = SubmitMetaTaskReq::try_from_raw_parts("session-1", " ", "idem-submit")
        .expect_err("blank prompt text should be rejected");
    assert!(
        err.to_string().contains("invalid prompt_text"),
        "unexpected error: {err}"
    );

    let invalid_prompt = json!({
        "session_token": "session-1",
        "prompt_text": "",
        "idempotency_key": "idem-submit"
    });
    let err = serde_json::from_value::<SubmitMetaTaskReq>(invalid_prompt)
        .expect_err("submit-meta-task prompt text must be non-empty");
    assert!(
        err.to_string().contains("invalid prompt_text"),
        "unexpected error: {err}"
    );
}

#[test]
fn submit_meta_task_request_rejects_invalid_session_tokens() {
    let req = SubmitMetaTaskReq::try_from_raw_parts("session-1", "do work", "idem-submit")
        .expect("valid submit-meta-task request");
    assert_eq!(req.session_token, "session-1");
    assert_eq!(req.prompt_text, "do work");

    let value = serde_json::to_value(&req).expect("submit request serializes");
    assert_eq!(value["session_token"], "session-1");
    assert_eq!(value["prompt_text"], "do work");
    let decoded: SubmitMetaTaskReq =
        serde_json::from_value(value).expect("submit request deserializes");
    assert_eq!(decoded, req);

    let invalid = json!({
        "session_token": "session 1",
        "prompt_text": "do work",
        "idempotency_key": "idem-submit"
    });
    let err = serde_json::from_value::<SubmitMetaTaskReq>(invalid)
        .expect_err("submit-meta-task session tokens must be token-shaped");
    assert!(
        err.to_string().contains("invalid session_token"),
        "unexpected error: {err}"
    );
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

#[test]
fn subtask_status_preserves_flat_claim_and_artifact_fields() {
    let raw = claimed_artifact_subtask_status_json("subtask-1", "subtask-1");

    let status: SubtaskStatus =
        serde_json::from_value(raw.clone()).expect("valid status must deserialize");

    assert_eq!(
        status.claim().map(|claim| claim.claim_id.as_str()),
        Some("claim-1")
    );
    assert_eq!(
        status
            .artifact()
            .map(|artifact| artifact.artifact_digest.as_str()),
        Some("blake3:artifact")
    );
    let serialized = serde_json::to_value(&status).expect("status must serialize");
    let mut expected = raw;
    expected["readiness"] = json!({
        "planning_ready": false,
        "covey_imported": true,
        "execution_ready": false,
        "review_approved": false,
        "apply_queued": false,
        "apply_authorized": false,
        "landed": false,
        "shipped_verified": false
    });
    assert_eq!(serialized, expected);
}

#[test]
fn subtask_status_landed_requires_explicit_landing_receipt_projection() {
    let raw = claimed_artifact_subtask_status_json("subtask-1", "subtask-1");
    let subtask: SubtaskView =
        serde_json::from_value(raw["subtask"].clone()).expect("valid subtask view");
    let claim: Claim = serde_json::from_value(raw["claim"].clone()).expect("valid claim");
    let artifact: Artifact =
        serde_json::from_value(raw["artifact"].clone()).expect("valid artifact");

    let without_receipt = SubtaskStatus::new_with_landing_receipt(
        subtask.clone(),
        Some(claim.clone()),
        Some(artifact.clone()),
        Vec::new(),
        Vec::new(),
        false,
    )
    .expect("valid status without receipt");
    assert!(!without_receipt.readiness().landed);

    let with_receipt = SubtaskStatus::new_with_landing_receipt(
        subtask,
        Some(claim),
        Some(artifact),
        Vec::new(),
        Vec::new(),
        true,
    )
    .expect("valid status with receipt");
    assert!(with_receipt.readiness().landed);
}

#[test]
fn claim_lifecycle_preserves_flat_state_shape() {
    let raw = json!({
        "claim_id": "claim-1",
        "subtask_id": "subtask-1",
        "owner_session_token": "session-1",
        "fence_seq": 7,
        "lease_deadline": 500,
        "state": "held",
        "created_at": 100,
        "updated_at": 101
    });

    let claim: Claim = serde_json::from_value(raw.clone()).expect("claim should deserialize");

    assert_eq!(claim.state(), ClaimState::Held);
    let serialized = serde_json::to_value(&claim).expect("claim should serialize");
    assert_eq!(serialized, raw);
    assert!(
        serialized.get("lifecycle").is_none(),
        "claim JSON must remain the legacy flat storage shape"
    );
}

#[test]
fn claim_domain_rejects_non_monotonic_timestamps() {
    let raw = json!({
        "claim_id": "claim-1",
        "subtask_id": "subtask-1",
        "owner_session_token": "session-1",
        "fence_seq": 7,
        "lease_deadline": 500,
        "state": "held",
        "created_at": 200,
        "updated_at": 100
    });

    let err = serde_json::from_value::<Claim>(raw)
        .expect_err("claim updated_at before created_at must be rejected");
    assert!(
        err.to_string()
            .contains("claim updated_at must be greater than or equal to created_at"),
        "unexpected error: {err}"
    );

    let err = Claim::try_from_parts(
        ClaimId::parse("claim-1").expect("valid claim id"),
        SubtaskId::parse("subtask-1").expect("valid subtask id"),
        SessionToken::parse("session-1").expect("valid session token"),
        FenceSeq::parse(7).expect("valid fence"),
        LeaseDeadlineMs::parse(500).expect("valid lease deadline"),
        ClaimState::Held,
        TimestampMs::parse(200).expect("valid created timestamp"),
        TimestampMs::parse(100).expect("valid updated timestamp"),
    )
    .expect_err("claim constructor must reject non-monotonic timestamps");
    assert!(
        err.contains("claim updated_at must be greater than or equal to created_at"),
        "unexpected error: {err}"
    );
}

#[test]
fn meta_task_lifecycle_preserves_flat_state_shape() {
    let raw = json!({
        "meta_task_id": "meta-1",
        "prompt_text": "ship the feature",
        "state": "active",
        "created_by": "session-1",
        "created_at": 100,
        "updated_at": 101
    });

    let meta_task: MetaTask =
        serde_json::from_value(raw.clone()).expect("meta-task should deserialize");

    assert_eq!(meta_task.state(), MetaTaskState::Active);
    let serialized = serde_json::to_value(&meta_task).expect("meta-task should serialize");
    assert_eq!(serialized, raw);
    assert!(
        serialized.get("lifecycle").is_none(),
        "meta-task JSON must remain the legacy flat storage shape"
    );
}

#[test]
fn meta_task_domain_rejects_non_monotonic_timestamps() {
    let raw = json!({
        "meta_task_id": "meta-1",
        "prompt_text": "ship the feature",
        "state": "active",
        "created_by": "session-1",
        "created_at": 200,
        "updated_at": 100
    });

    let err = serde_json::from_value::<MetaTask>(raw)
        .expect_err("meta-task updated_at before created_at must be rejected");

    assert!(
        err.to_string()
            .contains("meta-task updated_at must be greater than or equal to created_at"),
        "unexpected error: {err}"
    );

    let err = MetaTask::try_from_parts(
        MetaTaskId::parse("meta-1").expect("valid meta-task id"),
        "ship the feature".to_owned(),
        MetaTaskState::Active,
        SessionToken::parse("session-1").expect("valid session token"),
        TimestampMs::parse(200).expect("valid timestamp"),
        TimestampMs::parse(100).expect("valid timestamp"),
    )
    .expect_err("meta-task constructor must reject non-monotonic timestamps");

    assert!(
        err.contains("meta-task updated_at must be greater than or equal to created_at"),
        "unexpected error: {err}"
    );
}

#[test]
fn subtask_status_rejects_claim_row_for_another_subtask() {
    let raw = claimed_artifact_subtask_status_json("subtask-2", "subtask-1");

    let err = serde_json::from_value::<SubtaskStatus>(raw)
        .expect_err("status claim must belong to its subtask");

    assert!(
        err.to_string()
            .contains("subtask status claim must belong to the subtask"),
        "unexpected error: {err}"
    );
}

#[test]
fn subtask_status_rejects_artifact_row_for_another_subtask() {
    let raw = claimed_artifact_subtask_status_json("subtask-1", "subtask-2");

    let err = serde_json::from_value::<SubtaskStatus>(raw)
        .expect_err("status artifact must belong to its subtask");

    assert!(
        err.to_string()
            .contains("subtask status artifact must belong to the subtask"),
        "unexpected error: {err}"
    );
}

fn claimed_artifact_subtask_status_json(
    claim_subtask_id: &str,
    artifact_subtask_id: &str,
) -> serde_json::Value {
    json!({
        "subtask": {
            "subtask_id": "subtask-1",
            "meta_task_id": "meta-1",
            "title": "implement",
            "kind": "work",
            "review_target": null,
            "state": "artifact_published",
            "active_claim_id": "claim-1",
            "artifact_digest": "blake3:artifact",
            "priority": 10,
            "created_at": 100,
            "updated_at": 200
        },
        "claim": {
            "claim_id": "claim-1",
            "subtask_id": claim_subtask_id,
            "owner_session_token": "session-1",
            "fence_seq": 1,
            "lease_deadline": 500,
            "state": "held",
            "created_at": 100,
            "updated_at": 100
        },
        "artifact": {
            "artifact_digest": "blake3:artifact",
            "artifact_kind": "patch_bundle",
            "base_rev": "base-1",
            "produced_by_subtask_id": artifact_subtask_id,
            "produced_by_session": "session-1",
            "manifest_path": "artifact.json",
            "changed_paths_digest": "blake3:paths",
            "created_at": 150
        },
        "reviews": [],
        "ready_queue": []
    })
}

fn work_subtask_row(
    state: SubtaskState,
    current_claim_id: Option<ClaimId>,
    artifact_digest: Option<ArtifactDigest>,
) -> SubtaskRow {
    try_work_subtask_row(state, current_claim_id, artifact_digest).expect("valid work subtask row")
}

fn try_work_subtask_row(
    state: SubtaskState,
    current_claim_id: Option<ClaimId>,
    artifact_digest: Option<ArtifactDigest>,
) -> rusqlite::Result<SubtaskRow> {
    SubtaskRow::try_from_parts(
        SubtaskId::parse("subtask-1").expect("valid subtask id"),
        MetaTaskId::parse("meta-1").expect("valid meta-task id"),
        "implement".to_owned(),
        SubtaskKind::Work,
        None,
        None,
        state,
        current_claim_id,
        artifact_digest,
        SubtaskPriority::parse(10).expect("valid subtask priority"),
        TimestampMs::parse(100).expect("valid timestamp"),
        TimestampMs::parse(200).expect("valid timestamp"),
    )
}

#[test]
fn subtask_rows_reject_blank_titles() {
    let err = SubtaskRow::try_from_parts(
        SubtaskId::parse("subtask-1").expect("valid subtask id"),
        MetaTaskId::parse("meta-1").expect("valid meta-task id"),
        " ".to_owned(),
        SubtaskKind::Work,
        None,
        None,
        SubtaskState::Available,
        None,
        None,
        SubtaskPriority::parse(10).expect("valid subtask priority"),
        TimestampMs::parse(100).expect("valid timestamp"),
        TimestampMs::parse(200).expect("valid timestamp"),
    )
    .expect_err("blank subtask titles should be rejected");

    assert!(
        err.to_string().contains("invalid title"),
        "unexpected error: {err}"
    );
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
fn review_domain_rejects_non_monotonic_timestamps() {
    let raw = serde_json::json!({
        "review_id": "review-1",
        "subtask_id": "subtask-1",
        "artifact_digest": "blake3:artifact",
        "reviewer_session": "session-1",
        "review_subtask_id": "review-subtask-1",
        "verdict": null,
        "findings_digest": null,
        "state": "requested",
        "created_at": 200,
        "updated_at": 100
    });

    let err = serde_json::from_value::<Review>(raw)
        .expect_err("review updated_at before created_at must be rejected");

    assert!(
        err.to_string()
            .contains("review updated_at must be greater than or equal to created_at"),
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
fn ready_queue_domain_rejects_invalid_claim_numbers() {
    let invalid_fence = serde_json::json!({
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "subtask_id": "subtask-1",
        "settlement_target": "canonical",
        "state": "applied",
        "claimed_by_session_token": null,
        "claim_fence_seq": 0,
        "claim_lease_deadline": null,
        "enqueued_at": 100,
        "updated_at": 200
    });

    let err = serde_json::from_value::<ReadyQueueItem>(invalid_fence)
        .expect_err("ready queue rows must reject non-positive fence values");
    assert!(
        err.to_string().contains("invalid fence_seq"),
        "unexpected error: {err}"
    );

    let invalid_deadline = serde_json::json!({
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "subtask_id": "subtask-1",
        "settlement_target": "canonical",
        "state": "in_flight",
        "claimed_by_session_token": "session-1",
        "claim_fence_seq": 1,
        "claim_lease_deadline": -1,
        "enqueued_at": 100,
        "updated_at": 200
    });

    let err = serde_json::from_value::<ReadyQueueItem>(invalid_deadline)
        .expect_err("ready queue rows must reject negative lease deadlines");
    assert!(
        err.to_string().contains("invalid lease_deadline"),
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
fn ready_queue_domain_rejects_non_monotonic_timestamps() {
    let raw = serde_json::json!({
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "subtask_id": "subtask-1",
        "settlement_target": "canonical",
        "state": "queued",
        "claimed_by_session_token": null,
        "claim_fence_seq": null,
        "claim_lease_deadline": null,
        "enqueued_at": 200,
        "updated_at": 100
    });

    let err = serde_json::from_value::<ReadyQueueItem>(raw)
        .expect_err("ready queue updated_at before enqueued_at must be rejected");

    assert!(
        err.to_string()
            .contains("ready-queue updated_at must be greater than or equal to enqueued_at"),
        "unexpected error: {err}"
    );
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
fn meta_task_mutation_request_payloads_reject_invalid_typed_ids() {
    let invalid_cancel_session = serde_json::json!({
        "session_token": "",
        "meta_task_id": "meta-1",
        "idempotency_key": "idem-cancel"
    });
    serde_json::from_value::<CancelMetaTaskReq>(invalid_cancel_session)
        .expect_err("cancel-meta-task request should reject invalid session tokens");

    let invalid_cancel_meta_task = serde_json::json!({
        "session_token": "session-1",
        "meta_task_id": "",
        "idempotency_key": "idem-cancel"
    });
    serde_json::from_value::<CancelMetaTaskReq>(invalid_cancel_meta_task)
        .expect_err("cancel-meta-task request should reject invalid meta-task ids");

    let invalid_create_meta_task = serde_json::json!({
        "session_token": "session-1",
        "meta_task_id": "meta with spaces",
        "subtask_id": "subtask-1",
        "title": "work",
        "priority": 1,
        "idempotency_key": "idem-create"
    });
    serde_json::from_value::<CreateSubtaskRequest>(invalid_create_meta_task)
        .expect_err("create-subtask request should reject invalid meta-task ids");

    let invalid_title = CreateSubtaskRequest::try_from_raw_parts(
        "session-1",
        "meta-1",
        None,
        " ",
        42,
        "idem-title",
    )
    .expect_err("blank create-subtask title should be rejected");
    assert!(
        invalid_title.to_string().contains("invalid title"),
        "unexpected error: {invalid_title}"
    );

    let create = CreateSubtaskRequest::try_from_raw_parts(
        "session-1",
        "meta-1",
        Some("subtask-1".to_owned()),
        "work",
        42,
        "idem-create",
    )
    .expect("valid create-subtask request");
    assert_eq!(create.session_token, "session-1");
    assert_eq!(create.title, "work");
    assert_eq!(create.priority, 42);
    let value = serde_json::to_value(&create).expect("create request serializes");
    assert_eq!(value["session_token"], "session-1");
    assert_eq!(value["title"], "work");
    assert_eq!(value["priority"], 42);
    let decoded: CreateSubtaskRequest =
        serde_json::from_value(value).expect("create request deserializes");
    assert_eq!(decoded, create);

    let invalid_create_session = serde_json::json!({
        "session_token": "session 1",
        "meta_task_id": "meta-1",
        "subtask_id": "subtask-1",
        "title": "work",
        "priority": 1,
        "idempotency_key": "idem-create"
    });
    let err = serde_json::from_value::<CreateSubtaskRequest>(invalid_create_session)
        .expect_err("create-subtask request should reject invalid session tokens");
    assert!(
        err.to_string().contains("invalid session_token"),
        "unexpected error: {err}"
    );

    let invalid_create_subtask = serde_json::json!({
        "session_token": "session-1",
        "meta_task_id": "meta-1",
        "subtask_id": "subtask with spaces",
        "title": "work",
        "priority": 1,
        "idempotency_key": "idem-create"
    });
    serde_json::from_value::<CreateSubtaskRequest>(invalid_create_subtask)
        .expect_err("create-subtask request should reject invalid subtask ids");

    let invalid_create_title = serde_json::json!({
        "session_token": "session-1",
        "meta_task_id": "meta-1",
        "subtask_id": "subtask-1",
        "title": " work",
        "priority": 1,
        "idempotency_key": "idem-create"
    });
    let err = serde_json::from_value::<CreateSubtaskRequest>(invalid_create_title)
        .expect_err("create-subtask request should reject invalid titles");
    assert!(
        err.to_string().contains("invalid title"),
        "unexpected error: {err}"
    );

    let invalid_create_priority = serde_json::json!({
        "session_token": "session-1",
        "meta_task_id": "meta-1",
        "subtask_id": "subtask-1",
        "title": "work",
        "priority": 1001,
        "idempotency_key": "idem-create"
    });
    serde_json::from_value::<CreateSubtaskRequest>(invalid_create_priority)
        .expect_err("create-subtask request should reject out-of-range priorities");
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
    let invalid_claim_session = serde_json::json!({
        "session_token": "",
        "lease_duration_ms": 30_000,
        "idempotency_key": "idem-claim-ready"
    });
    serde_json::from_value::<ClaimReadyQueueReq>(invalid_claim_session)
        .expect_err("claim-ready request should reject invalid session tokens");

    let invalid_claim_lease = serde_json::json!({
        "session_token": "session-1",
        "lease_duration_ms": 0,
        "idempotency_key": "idem-claim-ready"
    });
    serde_json::from_value::<ClaimReadyQueueReq>(invalid_claim_lease)
        .expect_err("claim-ready request should reject non-positive lease durations");

    let invalid_in_flight_session = serde_json::json!({
        "session_token": "session 1",
        "queue_id": "queue-1",
        "lease_duration_ms": 30_000,
        "idempotency_key": "idem-mark-in-flight"
    });
    serde_json::from_value::<MarkInFlightReq>(invalid_in_flight_session)
        .expect_err("mark-in-flight request should reject invalid session tokens");

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

    let invalid_applied_session = serde_json::json!({
        "session_token": "",
        "queue_id": "queue-1",
        "claim_fence_seq": 1,
        "idempotency_key": "idem-mark-applied"
    });
    serde_json::from_value::<MarkAppliedReq>(invalid_applied_session)
        .expect_err("mark-applied request should reject invalid session tokens");

    let invalid_applied_queue = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "",
        "claim_fence_seq": 1,
        "idempotency_key": "idem-mark-applied"
    });
    serde_json::from_value::<MarkAppliedReq>(invalid_applied_queue)
        .expect_err("mark-applied request should reject invalid queue ids");

    let invalid_supersede_session = serde_json::json!({
        "session_token": "session 1",
        "queue_id": "queue-1",
        "idempotency_key": "idem-supersede"
    });
    serde_json::from_value::<SupersedeQueueItemReq>(invalid_supersede_session)
        .expect_err("supersede request should reject invalid session tokens");

    let invalid_supersede_queue = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "",
        "idempotency_key": "idem-supersede"
    });
    serde_json::from_value::<SupersedeQueueItemReq>(invalid_supersede_queue)
        .expect_err("supersede request should reject invalid queue ids");

    let invalid_supersede_idempotency = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "idempotency_key": " "
    });
    serde_json::from_value::<SupersedeQueueItemReq>(invalid_supersede_idempotency)
        .expect_err("supersede request should reject blank idempotency keys");
}

#[test]
fn apply_verification_payloads_reject_invalid_typed_fields() {
    let invalid_session_token = serde_json::json!({
        "session_token": "",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "review_id": "review-1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 1,
        "verifier": "mutai-rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "blake3:seal",
        "idempotency_key": "idem-verification"
    });
    serde_json::from_value::<RecordApplyVerificationReq>(invalid_session_token)
        .expect_err("apply verification should reject invalid session tokens");

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

    let invalid_verifier = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "review_id": "review-1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 1,
        "verifier": "mutai rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "blake3:seal",
        "idempotency_key": "idem-verification"
    });
    serde_json::from_value::<RecordApplyVerificationReq>(invalid_verifier)
        .expect_err("apply verification should reject invalid verifier ids");

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
    let invalid_session_token = serde_json::json!({
        "session_token": "session 1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "review_id": "review-1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 1,
        "verifier": "mutai-rs",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "blake3:seal"
    });
    serde_json::from_value::<VerifyLandingAuthorizationReq>(invalid_session_token)
        .expect_err("landing authorization should reject invalid session tokens");

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

    let invalid_verifier = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "review_id": "review-1",
        "findings_digest": "blake3:findings",
        "claim_fence_seq": 1,
        "verifier": "",
        "verdict_digest": "blake3:verdict",
        "seal_digest": "blake3:seal"
    });
    serde_json::from_value::<VerifyLandingAuthorizationReq>(invalid_verifier)
        .expect_err("landing authorization should reject invalid verifier ids");

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
fn landing_receipt_payload_rejects_invalid_typed_fields() {
    let req = RecordLandingReceiptReq::try_from_raw_parts(
        "session-1",
        "queue-1",
        "blake3:artifact",
        1,
        "refs/heads/main",
        "0123456789abcdef",
    )
    .expect("valid landing receipt request");
    assert_eq!(req.session_token, "session-1");
    let value = serde_json::to_value(&req).expect("landing receipt request serializes");
    assert_eq!(value["session_token"], "session-1");
    assert_eq!(value["queue_id"], "queue-1");
    assert_eq!(value["artifact_digest"], "blake3:artifact");
    assert_eq!(value["claim_fence_seq"], 1);
    assert_eq!(value["target_ref"], "refs/heads/main");
    assert_eq!(value["landed_commit_oid"], "0123456789abcdef");
    let decoded: RecordLandingReceiptReq =
        serde_json::from_value(value).expect("landing receipt request deserializes");
    assert_eq!(decoded, req);

    let invalid_session_token = serde_json::json!({
        "session_token": "session 1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "claim_fence_seq": 1,
        "target_ref": "refs/heads/main",
        "landed_commit_oid": "0123456789abcdef"
    });
    serde_json::from_value::<RecordLandingReceiptReq>(invalid_session_token)
        .expect_err("landing receipt should reject invalid session tokens");

    let invalid_queue_id = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "",
        "artifact_digest": "blake3:artifact",
        "claim_fence_seq": 1,
        "target_ref": "refs/heads/main",
        "landed_commit_oid": "0123456789abcdef"
    });
    serde_json::from_value::<RecordLandingReceiptReq>(invalid_queue_id)
        .expect_err("landing receipt should reject invalid queue ids");

    let invalid_artifact_digest = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "artifact",
        "claim_fence_seq": 1,
        "target_ref": "refs/heads/main",
        "landed_commit_oid": "0123456789abcdef"
    });
    serde_json::from_value::<RecordLandingReceiptReq>(invalid_artifact_digest)
        .expect_err("landing receipt should reject invalid artifact digests");

    let invalid_fence = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "claim_fence_seq": 0,
        "target_ref": "refs/heads/main",
        "landed_commit_oid": "0123456789abcdef"
    });
    serde_json::from_value::<RecordLandingReceiptReq>(invalid_fence)
        .expect_err("landing receipt should reject invalid fence sequences");

    let invalid_target_ref = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "claim_fence_seq": 1,
        "target_ref": "refs heads main",
        "landed_commit_oid": "0123456789abcdef"
    });
    serde_json::from_value::<RecordLandingReceiptReq>(invalid_target_ref)
        .expect_err("landing receipt should reject invalid target refs");

    let invalid_landed_commit_oid = serde_json::json!({
        "session_token": "session-1",
        "queue_id": "queue-1",
        "artifact_digest": "blake3:artifact",
        "claim_fence_seq": 1,
        "target_ref": "refs/heads/main",
        "landed_commit_oid": "not-a-hex-oid"
    });
    serde_json::from_value::<RecordLandingReceiptReq>(invalid_landed_commit_oid)
        .expect_err("landing receipt should reject invalid commit oids");
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
fn repoops_authority_snapshot_request_rejects_invalid_typed_fields() {
    let req = RepoopsAuthoritySnapshotReq::try_from_raw_parts(
        "session-1",
        "claim-1",
        1,
        vec!["./src/lib.rs".to_owned(), "docs\\guide.md".to_owned()],
    )
    .expect("valid repoops snapshot request");
    assert_eq!(req.paths()[0], "src/lib.rs");
    assert_eq!(req.paths()[1], "docs/guide.md");
    let value = serde_json::to_value(&req).expect("repoops snapshot request serializes");
    assert_eq!(value["paths"][0], "src/lib.rs");
    assert_eq!(value["paths"][1], "docs/guide.md");
    let decoded: RepoopsAuthoritySnapshotReq =
        serde_json::from_value(value).expect("repoops snapshot request deserializes");
    assert_eq!(decoded, req);

    let invalid_session = serde_json::json!({
        "session_token": "",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "paths": ["src/lib.rs"]
    });
    serde_json::from_value::<RepoopsAuthoritySnapshotReq>(invalid_session)
        .expect_err("repoops snapshot request should reject invalid session tokens");

    let invalid_claim = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim 1",
        "fence_seq": 1,
        "paths": ["src/lib.rs"]
    });
    serde_json::from_value::<RepoopsAuthoritySnapshotReq>(invalid_claim)
        .expect_err("repoops snapshot request should reject invalid claim ids");

    let invalid_fence = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 0,
        "paths": ["src/lib.rs"]
    });
    serde_json::from_value::<RepoopsAuthoritySnapshotReq>(invalid_fence)
        .expect_err("repoops snapshot request should reject invalid fence sequences");

    let empty_paths = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "paths": []
    });
    serde_json::from_value::<RepoopsAuthoritySnapshotReq>(empty_paths)
        .expect_err("repoops snapshot request should require at least one path");

    let duplicate_paths = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "paths": ["./src/lib.rs", "src/lib.rs"]
    });
    serde_json::from_value::<RepoopsAuthoritySnapshotReq>(duplicate_paths)
        .expect_err("repoops snapshot request should reject duplicate normalized paths");

    let invalid_empty_path = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "paths": [""]
    });
    serde_json::from_value::<RepoopsAuthoritySnapshotReq>(invalid_empty_path)
        .expect_err("repoops snapshot request should reject empty paths");

    let invalid_escape_path = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "paths": ["../outside"]
    });
    serde_json::from_value::<RepoopsAuthoritySnapshotReq>(invalid_escape_path)
        .expect_err("repoops snapshot request should reject escaping paths");
}

#[test]
fn repoops_authority_claim_fact_rejects_invalid_lifecycle_shapes() {
    let claim = RepoopsAuthorityClaimFact::in_progress(
        ClaimId::parse("claim-1").expect("valid claim id"),
        "agent-1",
        vec!["src/**".to_owned()],
        Vec::new(),
        true,
        SessionToken::parse("session-1").expect("valid session token"),
    )
    .expect("valid in-progress repoops claim fact");
    let value = serde_json::to_value(&claim).expect("repoops claim fact serializes");
    assert_eq!(value["status"], "in_progress");
    assert_eq!(value["active_ownership_token"], "session-1");
    let decoded: RepoopsAuthorityClaimFact =
        serde_json::from_value(value).expect("repoops claim fact deserializes");
    assert_eq!(decoded.active_ownership_token(), Some("session-1"));

    let missing_token = json!({
        "claim_id": "claim-1",
        "status": "in_progress",
        "owner": "agent-1",
        "scope_in": ["src/**"],
        "scope_out": [],
        "has_required_contract_fields": true,
        "active_ownership_token": null
    });
    let missing_token_err = serde_json::from_value::<RepoopsAuthorityClaimFact>(missing_token)
        .expect_err("in-progress repoops claim facts require active ownership token");
    assert!(
        missing_token_err
            .to_string()
            .contains("in-progress repoops claim facts require active_ownership_token"),
        "unexpected error: {missing_token_err}"
    );

    let open_with_token = json!({
        "claim_id": "claim-1",
        "status": "open",
        "owner": "agent-1",
        "scope_in": ["src/**"],
        "scope_out": [],
        "has_required_contract_fields": true,
        "active_ownership_token": "session-1"
    });
    let open_with_token_err = serde_json::from_value::<RepoopsAuthorityClaimFact>(open_with_token)
        .expect_err("open repoops claim facts must not carry active ownership token");
    assert!(
        open_with_token_err
            .to_string()
            .contains("open repoops claim facts must not include active_ownership_token"),
        "unexpected error: {open_with_token_err}"
    );
}

#[test]
fn repoops_authority_scope_facts_reject_invalid_patterns() {
    let fact =
        RepoopsAuthorityScopeFact::new(vec!["src/**".to_owned()], vec!["target/**".to_owned()])
            .expect("valid repoops scope fact");
    assert_eq!(fact.scope_in(), ["src/**"]);
    assert_eq!(fact.scope_out(), ["target/**"]);

    RepoopsAuthorityScopeFact::new(vec!["src/\n**".to_owned()], Vec::new())
        .expect_err("repoops scope patterns should reject control characters");

    let control_scope = json!({
        "in": ["src/**"],
        "out": ["target/\n**"]
    });
    serde_json::from_value::<RepoopsAuthorityScopeFact>(control_scope)
        .expect_err("repoops scope serde should reject control characters");
}

#[test]
fn repoops_authority_lock_facts_reject_invalid_typed_fields() {
    let claim_id = RepoopsClaimRef::parse("claim-1").expect("valid repoops claim ref");
    let fact = RepoopsAuthorityLockFact::owned("src/lib.rs", "session-1", claim_id.clone())
        .expect("valid owned lock fact");
    assert_eq!(fact.path(), "src/lib.rs");
    assert_eq!(fact.owner(), "session-1");
    assert_eq!(fact.claim_id(), &claim_id);

    let value = serde_json::to_value(&fact).expect("repoops lock fact serializes");
    assert_eq!(value["path"], "src/lib.rs");
    assert_eq!(value["owner"], "session-1");
    assert_eq!(value["claim_id"], "claim-1");
    assert_eq!(value["status"], "owned");
    let decoded: RepoopsAuthorityLockFact =
        serde_json::from_value(value).expect("repoops lock fact deserializes");
    assert_eq!(decoded, fact);

    let blank_path = json!({
        "path": "",
        "owner": "session-1",
        "claim_id": "claim-1",
        "status": "owned"
    });
    serde_json::from_value::<RepoopsAuthorityLockFact>(blank_path)
        .expect_err("repoops lock fact should reject blank paths");

    let absolute_path = json!({
        "path": "/src/lib.rs",
        "owner": "session-1",
        "claim_id": "claim-1",
        "status": "owned"
    });
    serde_json::from_value::<RepoopsAuthorityLockFact>(absolute_path)
        .expect_err("repoops lock fact should reject absolute paths");

    let control_path = json!({
        "path": "src/foo\nbar.rs",
        "owner": "session-1",
        "claim_id": "claim-1",
        "status": "owned"
    });
    serde_json::from_value::<RepoopsAuthorityLockFact>(control_path)
        .expect_err("repoops lock fact should reject control characters in paths");

    let windows_separator_path = json!({
        "path": "src\\lib.rs",
        "owner": "session-1",
        "claim_id": "claim-1",
        "status": "owned"
    });
    serde_json::from_value::<RepoopsAuthorityLockFact>(windows_separator_path)
        .expect_err("repoops lock fact should reject unnormalized path separators");

    let padded_owner = json!({
        "path": "src/lib.rs",
        "owner": " session-1",
        "claim_id": "claim-1",
        "status": "foreign_owner"
    });
    serde_json::from_value::<RepoopsAuthorityLockFact>(padded_owner)
        .expect_err("repoops lock fact should reject padded owners");

    let internal_whitespace_owner = json!({
        "path": "src/lib.rs",
        "owner": "session 1",
        "claim_id": "claim-1",
        "status": "foreign_owner"
    });
    serde_json::from_value::<RepoopsAuthorityLockFact>(internal_whitespace_owner)
        .expect_err("repoops lock fact should reject whitespace inside owner refs");

    let control_owner = json!({
        "path": "src/lib.rs",
        "owner": "session\n1",
        "claim_id": "claim-1",
        "status": "foreign_owner"
    });
    serde_json::from_value::<RepoopsAuthorityLockFact>(control_owner)
        .expect_err("repoops lock fact should reject control characters in owner refs");
}

#[test]
fn repoops_authority_git_context_rejects_invalid_typed_fields() {
    let fact = RepoopsAuthorityGitContextFact::known_paths(
        "/data/projects/mutai",
        "/data/projects/mutai-worktree",
        Some("authority/src".to_owned()),
        true,
    )
    .expect("valid known git context fact");
    assert_eq!(fact.policy_project_path(), Some("/data/projects/mutai"));
    assert_eq!(
        fact.execution_project_path(),
        Some("/data/projects/mutai-worktree")
    );
    assert_eq!(fact.repo_path_prefix(), Some("authority/src"));
    assert!(fact.ownership_token_required());

    let value = serde_json::to_value(&fact).expect("git context fact serializes");
    assert_eq!(value["policy_project_path"], "/data/projects/mutai");
    assert_eq!(
        value["execution_project_path"],
        "/data/projects/mutai-worktree"
    );
    assert_eq!(value["repo_path_prefix"], "authority/src");
    let decoded: RepoopsAuthorityGitContextFact =
        serde_json::from_value(value).expect("git context fact deserializes");
    assert_eq!(decoded, fact);

    let unknown = RepoopsAuthorityGitContextFact::unknown(false);
    assert_eq!(unknown.policy_project_path(), None);
    assert!(!unknown.ownership_token_required());

    let partial_paths = json!({
        "policy_project_path": "/data/projects/mutai",
        "execution_project_path": null,
        "repo_path_prefix": null,
        "ownership_token_required": true
    });
    serde_json::from_value::<RepoopsAuthorityGitContextFact>(partial_paths)
        .expect_err("git context fact should require both project paths");

    let padded_project_path = json!({
        "policy_project_path": " /data/projects/mutai",
        "execution_project_path": "/data/projects/mutai-worktree",
        "repo_path_prefix": null,
        "ownership_token_required": true
    });
    serde_json::from_value::<RepoopsAuthorityGitContextFact>(padded_project_path)
        .expect_err("git context fact should reject padded project paths");

    let control_project_path = json!({
        "policy_project_path": "/data/projects/mutai\n",
        "execution_project_path": "/data/projects/mutai-worktree",
        "repo_path_prefix": null,
        "ownership_token_required": true
    });
    serde_json::from_value::<RepoopsAuthorityGitContextFact>(control_project_path)
        .expect_err("git context fact should reject control characters in project paths");

    let escaping_prefix = json!({
        "policy_project_path": "/data/projects/mutai",
        "execution_project_path": "/data/projects/mutai-worktree",
        "repo_path_prefix": "../outside",
        "ownership_token_required": true
    });
    serde_json::from_value::<RepoopsAuthorityGitContextFact>(escaping_prefix)
        .expect_err("git context fact should reject escaping prefixes");

    let backslash_prefix = json!({
        "policy_project_path": "/data/projects/mutai",
        "execution_project_path": "/data/projects/mutai-worktree",
        "repo_path_prefix": "authority\\src",
        "ownership_token_required": true
    });
    serde_json::from_value::<RepoopsAuthorityGitContextFact>(backslash_prefix)
        .expect_err("git context fact should reject unnormalized prefixes");

    let control_prefix = json!({
        "policy_project_path": "/data/projects/mutai",
        "execution_project_path": "/data/projects/mutai-worktree",
        "repo_path_prefix": "authority/src\n",
        "ownership_token_required": true
    });
    serde_json::from_value::<RepoopsAuthorityGitContextFact>(control_prefix)
        .expect_err("git context fact should reject control characters in prefixes");
}

#[test]
fn claim_lifecycle_request_payloads_reject_invalid_claim_identity_and_fence() {
    let start = StartSubtaskReq::try_from_raw_parts("session-1", "claim-1", 1, "idem-start")
        .expect("valid start request");
    assert_eq!(start.session_token, "session-1");
    let value = serde_json::to_value(&start).expect("start request serializes");
    assert_eq!(value["session_token"], "session-1");
    assert_eq!(value["claim_id"], "claim-1");
    assert_eq!(value["fence_seq"], 1);
    let decoded: StartSubtaskReq =
        serde_json::from_value(value).expect("start request deserializes");
    assert_eq!(decoded, start);

    let invalid_session = serde_json::json!({
        "session_token": "session 1",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "idempotency_key": "idem-start"
    });
    let err = serde_json::from_value::<StartSubtaskReq>(invalid_session)
        .expect_err("start request should reject invalid session tokens");
    assert!(
        err.to_string().contains("invalid session_token"),
        "unexpected error: {err}"
    );

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

    let abandon = AbandonSubtaskReq::try_from_raw_parts("session-1", "claim-1", 1, "idem-abandon")
        .expect("valid abandon request");
    assert_eq!(abandon.session_token, "session-1");
    let value = serde_json::to_value(&abandon).expect("abandon request serializes");
    assert_eq!(value["session_token"], "session-1");
    assert_eq!(value["claim_id"], "claim-1");
    assert_eq!(value["fence_seq"], 1);
    let decoded: AbandonSubtaskReq =
        serde_json::from_value(value).expect("abandon request deserializes");
    assert_eq!(decoded, abandon);

    let invalid_session = serde_json::json!({
        "session_token": "",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "idempotency_key": "idem-abandon"
    });
    let err = serde_json::from_value::<AbandonSubtaskReq>(invalid_session)
        .expect_err("abandon request should reject invalid session tokens");
    assert!(
        err.to_string().contains("invalid session_token"),
        "unexpected error: {err}"
    );

    let invalid_claim_id = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "",
        "fence_seq": 1,
        "idempotency_key": "idem-abandon"
    });
    serde_json::from_value::<AbandonSubtaskReq>(invalid_claim_id)
        .expect_err("abandon request should reject invalid claim ids");

    let invalid_fence = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 0,
        "idempotency_key": "idem-abandon"
    });
    serde_json::from_value::<AbandonSubtaskReq>(invalid_fence)
        .expect_err("abandon request should reject invalid fence sequences");

    let release = ReleaseClaimReq::try_from_raw_parts("session-1", "claim-1", 1, "idem-release")
        .expect("valid release request");
    assert_eq!(release.session_token, "session-1");
    let value = serde_json::to_value(&release).expect("release request serializes");
    assert_eq!(value["session_token"], "session-1");
    assert_eq!(value["claim_id"], "claim-1");
    assert_eq!(value["fence_seq"], 1);
    let decoded: ReleaseClaimReq =
        serde_json::from_value(value).expect("release request deserializes");
    assert_eq!(decoded, release);

    let invalid_session = serde_json::json!({
        "session_token": "session 1",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "idempotency_key": "idem-release"
    });
    let err = serde_json::from_value::<ReleaseClaimReq>(invalid_session)
        .expect_err("release request should reject invalid session tokens");
    assert!(
        err.to_string().contains("invalid session_token"),
        "unexpected error: {err}"
    );

    let invalid_claim_id = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "",
        "fence_seq": 1,
        "idempotency_key": "idem-release"
    });
    serde_json::from_value::<ReleaseClaimReq>(invalid_claim_id)
        .expect_err("release request should reject invalid claim ids");

    let invalid_fence = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 0,
        "idempotency_key": "idem-release"
    });
    serde_json::from_value::<ReleaseClaimReq>(invalid_fence)
        .expect_err("release request should reject invalid fence sequences");

    let renew = RenewClaimReq::try_from_raw_parts("session-1", "claim-1", 1, 30_000, "idem-renew")
        .expect("valid renew request");
    assert_eq!(renew.session_token, "session-1");
    let value = serde_json::to_value(&renew).expect("renew request serializes");
    assert_eq!(value["session_token"], "session-1");
    assert_eq!(value["claim_id"], "claim-1");
    assert_eq!(value["fence_seq"], 1);
    assert_eq!(value["extend_by_ms"], 30_000);
    let decoded: RenewClaimReq = serde_json::from_value(value).expect("renew request deserializes");
    assert_eq!(decoded, renew);

    let invalid_session = serde_json::json!({
        "session_token": "",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "extend_by_ms": 30_000,
        "idempotency_key": "idem-renew"
    });
    let err = serde_json::from_value::<RenewClaimReq>(invalid_session)
        .expect_err("renew request should reject invalid session tokens");
    assert!(
        err.to_string().contains("invalid session_token"),
        "unexpected error: {err}"
    );

    let invalid_claim_id = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "",
        "fence_seq": 1,
        "extend_by_ms": 30_000,
        "idempotency_key": "idem-renew"
    });
    serde_json::from_value::<RenewClaimReq>(invalid_claim_id)
        .expect_err("renew request should reject invalid claim ids");

    let invalid_fence = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 0,
        "extend_by_ms": 30_000,
        "idempotency_key": "idem-renew"
    });
    serde_json::from_value::<RenewClaimReq>(invalid_fence)
        .expect_err("renew request should reject invalid fence sequences");

    let invalid_extension = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "extend_by_ms": 0,
        "idempotency_key": "idem-renew"
    });
    serde_json::from_value::<RenewClaimReq>(invalid_extension)
        .expect_err("renew request should reject non-positive lease extensions");
}

#[test]
fn claim_acquisition_payloads_reject_invalid_targets_and_leases() {
    let claim_next = ClaimNextReq::try_from_raw_parts("session-1", 30_000, "idem-claim-next")
        .expect("valid claim-next request");
    assert_eq!(claim_next.session_token, "session-1");
    let value = serde_json::to_value(&claim_next).expect("claim-next request serializes");
    assert_eq!(value["session_token"], "session-1");
    assert_eq!(value["lease_duration_ms"], 30_000);
    let decoded: ClaimNextReq =
        serde_json::from_value(value).expect("claim-next request deserializes");
    assert_eq!(decoded, claim_next);

    let scoped_claim_next = ClaimNextReq::try_from_raw_parts_scoped(
        "session-1",
        30_000,
        Some("meta-1".to_owned()),
        "idem-claim-next-scoped",
    )
    .expect("valid scoped claim-next request");
    assert_eq!(
        scoped_claim_next
            .meta_task_id
            .as_ref()
            .expect("scoped claim next has meta-task id")
            .as_str(),
        "meta-1"
    );

    let invalid_next_session = serde_json::json!({
        "session_token": "session 1",
        "lease_duration_ms": 30_000,
        "idempotency_key": "idem-claim-next"
    });
    let err = serde_json::from_value::<ClaimNextReq>(invalid_next_session)
        .expect_err("claim-next request should reject invalid session tokens");
    assert!(
        err.to_string().contains("invalid session_token"),
        "unexpected error: {err}"
    );

    let invalid_next_lease = serde_json::json!({
        "session_token": "session-1",
        "lease_duration_ms": 0,
        "idempotency_key": "idem-claim-next"
    });
    serde_json::from_value::<ClaimNextReq>(invalid_next_lease)
        .expect_err("claim-next request should reject non-positive leases");

    let invalid_next_idempotency = serde_json::json!({
        "session_token": "session-1",
        "lease_duration_ms": 30_000,
        "idempotency_key": " "
    });
    serde_json::from_value::<ClaimNextReq>(invalid_next_idempotency)
        .expect_err("claim-next request should reject blank idempotency keys");

    let invalid_next_meta = ClaimNextReq::try_from_raw_parts_scoped(
        "session-1",
        30_000,
        Some("meta 1".to_owned()),
        "idem-claim-next-scoped",
    )
    .expect_err("scoped claim-next request should reject invalid meta-task ids");
    assert!(
        invalid_next_meta
            .to_string()
            .contains("invalid meta_task_id"),
        "unexpected error: {invalid_next_meta}"
    );

    let claim_subtask =
        ClaimSubtaskReq::try_from_raw_parts("session-1", "subtask-1", 30_000, "idem-claim-subtask")
            .expect("valid claim-subtask request");
    assert_eq!(claim_subtask.subtask_id, "subtask-1");

    let invalid_target_session = serde_json::json!({
        "session_token": "session 1",
        "subtask_id": "subtask-1",
        "lease_duration_ms": 30_000,
        "idempotency_key": "idem-claim-subtask"
    });
    let err = serde_json::from_value::<ClaimSubtaskReq>(invalid_target_session)
        .expect_err("claim-subtask request should reject invalid session tokens");
    assert!(
        err.to_string().contains("invalid session_token"),
        "unexpected error: {err}"
    );

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

    let invalid_target_idempotency = serde_json::json!({
        "session_token": "session-1",
        "subtask_id": "subtask-1",
        "lease_duration_ms": 30_000,
        "idempotency_key": " "
    });
    serde_json::from_value::<ClaimSubtaskReq>(invalid_target_idempotency)
        .expect_err("claim-subtask request should reject blank idempotency keys");
}

#[test]
fn publish_artifact_payload_rejects_invalid_typed_fields() {
    let invalid_session_token = serde_json::json!({
        "session_token": "",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "artifact_digest": "blake3:artifact",
        "artifact_kind": "patch_bundle",
        "base_rev": "base",
        "manifest_path": "manifest.json",
        "changed_paths_digest": "blake3:paths",
        "idempotency_key": "idem-artifact"
    });
    serde_json::from_value::<PublishArtifactReq>(invalid_session_token)
        .expect_err("publish request should reject invalid session tokens");

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

    let invalid_manifest_path = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "artifact_digest": "blake3:artifact",
        "artifact_kind": "patch_bundle",
        "base_rev": "base",
        "manifest_path": "",
        "changed_paths_digest": "blake3:paths",
        "idempotency_key": "idem-artifact"
    });
    serde_json::from_value::<PublishArtifactReq>(invalid_manifest_path)
        .expect_err("publish request should reject invalid manifest paths");

    let mission_packet_manifest_path = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "artifact_digest": "blake3:artifact",
        "artifact_kind": "patch_bundle",
        "base_rev": "base",
        "manifest_path": "openspec/changes/phase-3/mission/mission-packet.json",
        "changed_paths_digest": "blake3:paths",
        "idempotency_key": "idem-artifact"
    });
    serde_json::from_value::<PublishArtifactReq>(mission_packet_manifest_path)
        .expect_err("publish request should reject Better Droid mission packet manifest paths");

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
    let req = RequestReviewReq::try_from_raw_parts(
        "session-1",
        "subtask-1",
        "blake3:artifact",
        Some("review-subtask-1".to_owned()),
        7,
        "idem-review",
    )
    .expect("valid review request");
    assert_eq!(req.priority, 7);
    let value = serde_json::to_value(&req).expect("review request serializes");
    assert_eq!(value["priority"], 7);
    let decoded: RequestReviewReq =
        serde_json::from_value(value).expect("review request deserializes");
    assert_eq!(decoded, req);

    let invalid_session_token = serde_json::json!({
        "session_token": "",
        "subtask_id": "subtask-1",
        "artifact_digest": "blake3:artifact",
        "review_subtask_id": "review-subtask-1",
        "priority": 1,
        "idempotency_key": "idem-review"
    });
    serde_json::from_value::<RequestReviewReq>(invalid_session_token)
        .expect_err("review request should reject invalid session tokens");

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

    let invalid_priority = serde_json::json!({
        "session_token": "session-1",
        "subtask_id": "subtask-1",
        "artifact_digest": "blake3:artifact",
        "review_subtask_id": "review-subtask-1",
        "priority": -1,
        "idempotency_key": "idem-review"
    });
    serde_json::from_value::<RequestReviewReq>(invalid_priority)
        .expect_err("review request should reject out-of-range priorities");
}

#[test]
fn decide_review_payload_rejects_invalid_typed_fields() {
    let invalid_session_token = serde_json::json!({
        "session_token": "session reviewer",
        "review_id": "review-1",
        "claim_id": "claim-review",
        "fence_seq": 1,
        "verdict": "approve",
        "findings_digest": "blake3:findings",
        "idempotency_key": "idem-decision"
    });
    serde_json::from_value::<DecideReviewReq>(invalid_session_token)
        .expect_err("review decision should reject invalid session tokens");

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
    let invalid_session_token = serde_json::json!({
        "session_token": "",
        "artifact_digest": "blake3:artifact",
        "subtask_id": "subtask-1",
        "settlement_target": "canonical",
        "idempotency_key": "idem-enqueue"
    });
    serde_json::from_value::<EnqueueForApplyReq>(invalid_session_token)
        .expect_err("enqueue request should reject invalid session tokens");

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
    assert_eq!(existing_json["idempotency_key"], "idem-existing");

    let invalid_session = json!({
        "session_token": "session 1",
        "beads_db_path": "beads.db",
        "meta_task_id": "meta-1",
        "prompt_text": null,
        "idempotency_key": "idem-invalid-session"
    });
    serde_json::from_value::<ImportBdV1Req>(invalid_session)
        .expect_err("bd import request should reject invalid session tokens");

    let invalid_meta_task = json!({
        "session_token": "session-1",
        "beads_db_path": "beads.db",
        "meta_task_id": "meta 1",
        "prompt_text": null,
        "idempotency_key": "idem-invalid-meta"
    });
    serde_json::from_value::<ImportBdV1Req>(invalid_meta_task)
        .expect_err("bd import request should reject invalid meta-task ids");

    let invalid_idempotency_key = json!({
        "session_token": "session-1",
        "beads_db_path": "beads.db",
        "meta_task_id": "meta-1",
        "prompt_text": null,
        "idempotency_key": " "
    });
    serde_json::from_value::<ImportBdV1Req>(invalid_idempotency_key)
        .expect_err("bd import request should reject invalid idempotency keys");

    let invalid_beads_db_path = json!({
        "session_token": "session-1",
        "beads_db_path": " beads.db",
        "meta_task_id": "meta-1",
        "prompt_text": null,
        "idempotency_key": "idem-invalid-path"
    });
    serde_json::from_value::<ImportBdV1Req>(invalid_beads_db_path)
        .expect_err("bd import request should reject invalid source database paths");

    let created = ImportBdV1Req::new_meta_task("session-1", "beads.db", "new work", "idem-new");
    assert_eq!(created.meta_task_id(), None);
    assert_eq!(created.prompt_text(), Some("new work"));
    let created_json = serde_json::to_value(&created).expect("request should serialize");
    assert_eq!(created_json["meta_task_id"], serde_json::Value::Null);
    assert_eq!(created_json["prompt_text"], "new work");

    let blank_prompt = json!({
        "session_token": "session-1",
        "beads_db_path": "beads.db",
        "meta_task_id": null,
        "prompt_text": " ",
        "idempotency_key": "idem-blank-prompt"
    });
    serde_json::from_value::<ImportBdV1Req>(blank_prompt)
        .expect_err("bd import request should reject blank new meta-task prompts");

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
            .contains("requires exactly one destination selector"),
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
    assert_eq!(result.meta_task_id.as_str(), "meta-1");

    let value = serde_json::to_value(&result).expect("result should serialize");
    assert_eq!(value["meta_task_id"], "meta-1");
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

    let mut wrong_skipped = value.clone();
    wrong_skipped["skipped_count"] = json!(9);
    let wrong_skipped_err = serde_json::from_value::<ImportBdV1Result>(wrong_skipped)
        .expect_err("stored skipped count must match item outcomes");
    assert!(
        wrong_skipped_err
            .to_string()
            .contains("skipped_count mismatch"),
        "unexpected error: {wrong_skipped_err}"
    );

    let mut invalid_meta_task_id = value;
    invalid_meta_task_id["meta_task_id"] = json!("meta task 1");
    let invalid_meta_task_id_err = serde_json::from_value::<ImportBdV1Result>(invalid_meta_task_id)
        .expect_err("bd import result meta_task_id must be typed");
    assert!(
        invalid_meta_task_id_err
            .to_string()
            .contains("invalid meta_task_id"),
        "unexpected error: {invalid_meta_task_id_err}"
    );
}

#[test]
fn import_bd_v1_item_result_requires_typed_subtask_ids() {
    let valid_item = super::ImportBdV1ItemResult::imported("bd-1", "subtask-1");
    assert_eq!(valid_item.source_issue_id(), "bd-1");
    assert_eq!(valid_item.source_issue_id, "bd-1");

    let item_with_invalid_source = serde_json::json!({
        "source_issue_id": "bd 1",
        "subtask_id": "subtask-1",
        "skip_reason": null
    });
    let invalid_source_err =
        serde_json::from_value::<super::ImportBdV1ItemResult>(item_with_invalid_source)
            .expect_err("bd import items should reject invalid source issue ids");
    assert!(
        invalid_source_err
            .to_string()
            .contains("invalid source_issue_id"),
        "unexpected error: {invalid_source_err}"
    );

    let imported_with_invalid_subtask = serde_json::json!({
        "source_issue_id": "bd-1",
        "subtask_id": "subtask 1",
        "skip_reason": null
    });
    let imported_err =
        serde_json::from_value::<super::ImportBdV1ItemResult>(imported_with_invalid_subtask)
            .expect_err("imported item subtask_id must be typed");
    assert!(
        imported_err.to_string().contains("invalid subtask_id"),
        "unexpected error: {imported_err}"
    );

    let duplicate_without_subtask = serde_json::json!({
        "source_issue_id": "bd-2",
        "subtask_id": null,
        "skip_reason": "DeterministicDuplicate"
    });
    let duplicate_err =
        serde_json::from_value::<super::ImportBdV1ItemResult>(duplicate_without_subtask)
            .expect_err("duplicate item must identify the existing subtask");
    assert!(
        duplicate_err
            .to_string()
            .contains("deterministic duplicate bd import items require subtask_id"),
        "unexpected error: {duplicate_err}"
    );

    let duplicate_with_invalid_subtask = serde_json::json!({
        "source_issue_id": "bd-3",
        "subtask_id": "subtask 3",
        "skip_reason": "DeterministicDuplicate"
    });
    let duplicate_invalid_err =
        serde_json::from_value::<super::ImportBdV1ItemResult>(duplicate_with_invalid_subtask)
            .expect_err("duplicate item subtask_id must be typed");
    assert!(
        duplicate_invalid_err
            .to_string()
            .contains("invalid subtask_id"),
        "unexpected error: {duplicate_invalid_err}"
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
    assert_eq!(dry_run_json["change_id"], "change-1");

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

    let invalid_dry_run_session = json!({
        "session_token": "session 1",
        "change_id": "change-1",
        "project_root": "/repo",
        "dry_run": true
    });
    serde_json::from_value::<ImportOpenSpecReq>(invalid_dry_run_session)
        .expect_err("dry-run OpenSpec import should reject invalid session tokens");

    let invalid_write_session = json!({
        "session_token": "session 1",
        "change_id": "change-1",
        "project_root": "/repo",
        "dry_run": false
    });
    serde_json::from_value::<ImportOpenSpecReq>(invalid_write_session)
        .expect_err("write OpenSpec import should reject invalid session tokens");

    let invalid_change_id = json!({
        "session_token": null,
        "change_id": "Change_1",
        "project_root": "/repo",
        "dry_run": true
    });
    serde_json::from_value::<ImportOpenSpecReq>(invalid_change_id)
        .expect_err("OpenSpec import should reject non-kebab-case change ids");

    let invalid_project_root = json!({
        "session_token": null,
        "change_id": "change-1",
        "project_root": " /repo",
        "dry_run": true
    });
    serde_json::from_value::<ImportOpenSpecReq>(invalid_project_root)
        .expect_err("OpenSpec import should reject invalid project root paths");

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
        )
        .expect("subtask updated item should construct"),
        ImportOpenSpecItemResult::subtask(
            "openspec:change-1:1.2",
            "1.2",
            "Keep current import",
            None,
            "blake3:task-2",
            "openspec/changes/change-1/tasks.md",
            ImportOpenSpecAction::Unchanged,
        )
        .expect("subtask unchanged item should construct"),
    ];
    let result = ImportOpenSpecResult::new("change-1", "openspec:change-1", true, vec![], items)
        .expect("coherent result should construct");
    assert!(result.dry_run());
    assert_eq!(result.created(), 1);
    assert_eq!(result.updated(), 1);
    assert_eq!(result.unchanged(), 1);
    assert_eq!(result.change_id.as_str(), "change-1");
    assert_eq!(result.meta_task_id.as_str(), "openspec:change-1");
    assert!(result.conflicts().is_empty());
    assert_eq!(result.items().len(), 3);

    let value = serde_json::to_value(&result).expect("result should serialize");
    assert_eq!(value["change_id"], "change-1");
    assert_eq!(value["meta_task_id"], "openspec:change-1");
    assert_eq!(value["created"], 1);
    assert_eq!(value["updated"], 1);
    assert_eq!(value["unchanged"], 1);
    assert_eq!(value["items"].as_array().expect("items").len(), 3);

    let mut invalid_meta_task_id = value.clone();
    invalid_meta_task_id["meta_task_id"] = json!("openspec:change 1");
    let invalid_meta_task_id_err =
        serde_json::from_value::<ImportOpenSpecResult>(invalid_meta_task_id)
            .expect_err("OpenSpec import result meta_task_id must be typed");
    assert!(
        invalid_meta_task_id_err
            .to_string()
            .contains("invalid meta_task_id"),
        "unexpected error: {invalid_meta_task_id_err}"
    );

    let mut invalid_change_id = value.clone();
    invalid_change_id["change_id"] = json!("Change_1");
    let invalid_change_id_err = serde_json::from_value::<ImportOpenSpecResult>(invalid_change_id)
        .expect_err("OpenSpec import result change_id must be typed");
    assert!(
        invalid_change_id_err
            .to_string()
            .contains("invalid openspec_change_id"),
        "unexpected error: {invalid_change_id_err}"
    );

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
    )
    .expect("conflict item should construct");
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
        ImportOpenSpecConflictReason::ActiveClaimChangedSource,
        "openspec/changes/change-1/tasks.md",
        "blake3:task-3",
    )
    .expect("conflict record should construct");
    let conflict_result = ImportOpenSpecResult::new(
        "change-1",
        "openspec:change-1",
        false,
        vec![conflict],
        vec![
            ImportOpenSpecItemResult::subtask(
                "openspec:change-1:1.3",
                "1.3",
                "Conflicting import",
                None,
                "blake3:task-3",
                "openspec/changes/change-1/tasks.md",
                ImportOpenSpecAction::Conflict,
            )
            .expect("conflict item should construct"),
        ],
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

    let active_metrics = ReadyQueueMetrics::new(1, 2, Some(12), Some(4))
        .expect("coherent non-empty buckets should construct");
    let active_metrics_json =
        serde_json::to_value(&active_metrics).expect("metrics should serialize");
    assert_eq!(
        active_metrics_json,
        serde_json::json!({
            "queued_count": 1,
            "in_flight_count": 2,
            "oldest_queued_age_ms": 12,
            "oldest_in_flight_age_ms": 4
        })
    );
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

    let claimed_subtask = SubtaskView::try_from(work_subtask_row(
        SubtaskState::InProgress,
        Some(ClaimId::parse("claim-1").expect("valid claim id")),
        None,
    ))
    .expect("valid claimed subtask view");
    let claim: Claim = serde_json::from_value(json!({
        "claim_id": "claim-1",
        "subtask_id": "subtask-1",
        "owner_session_token": "session-1",
        "fence_seq": 1,
        "lease_deadline": 300,
        "state": ClaimState::Held,
        "created_at": 100,
        "updated_at": 200
    }))
    .expect("valid claim");
    let session: Session = serde_json::from_value(json!({
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
    }))
    .expect("valid session");
    let claimed_row = StuckSubtask::new(claimed_subtask, Some(claim), Some(session), 7)
        .expect("valid claimed stuck row");
    assert!(claimed_row.claim().is_some());
    assert!(claimed_row.session().is_some());
    let value = serde_json::to_value(&claimed_row).expect("claimed stuck row serializes");
    assert_eq!(value["claim"]["claim_id"], "claim-1");
    assert_eq!(value["session"]["session_token"], "session-1");
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
        "artifact_kind": "patch_bundle",
        "base_rev": "main",
        "manifest_path": "manifest.json",
        "changed_paths_digest": "blake3:paths",
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
    assert_eq!(status.artifact_kind(), ArtifactKind::PatchBundle);
    assert_eq!(status.base_rev().as_str(), "main");
    assert_eq!(status.manifest_path().as_str(), "manifest.json");
    assert_eq!(status.changed_paths_digest().as_str(), "blake3:paths");
    assert_eq!(status.claim_fence_seq(), 7);
    assert_eq!(status.recorded_by_session().as_str(), "session-1");
    assert_eq!(value["accepted"], true);
    assert_eq!(value["queue_id"], "queue-1");
    assert_eq!(value["artifact_kind"], "patch_bundle");
    assert_eq!(value["changed_paths_digest"], "blake3:paths");
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
    assert_eq!(
        serde_json::from_str::<ReservationOverlapConflictPayload>(&conflict.payload_json())
            .expect("conflict payload should remain typed")
            .reservation_id(),
        "reservation-1"
    );

    let invalid_conflict_id = json!({
        "conflict_id": "conflict 1",
        "object_type": "reservation",
        "object_id": "reservation-1",
        "conflict_kind": "reservation_overlap",
        "payload_json": valid_payload.to_string(),
        "detected_at": 100,
        "resolution_state": "open"
    });
    serde_json::from_value::<Conflict>(invalid_conflict_id)
        .expect_err("conflicts should reject invalid conflict ids");

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

    let wrong_object_id = json!({
        "conflict_id": "conflict-1",
        "object_type": "reservation",
        "object_id": "reservation-3",
        "conflict_kind": "reservation_overlap",
        "payload_json": valid_payload.to_string(),
        "detected_at": 100,
        "resolution_state": "open"
    });
    let wrong_object_id_err = serde_json::from_value::<Conflict>(wrong_object_id)
        .expect_err("reservation overlap conflict object id must bind one payload reservation");
    assert!(
        wrong_object_id_err
            .to_string()
            .contains("object_id must match one overlapping reservation"),
        "unexpected error: {wrong_object_id_err}"
    );
}

#[test]
fn resolve_conflict_request_rejects_invalid_typed_fields() {
    let valid = json!({
        "session_token": "session-1",
        "conflict_id": "conflict-1",
        "resolution_state": "acknowledged",
        "idempotency_key": "idem-resolve"
    });
    let decoded = serde_json::from_value::<ResolveConflictReq>(valid.clone())
        .expect("valid resolve conflict request should decode");
    assert_eq!(decoded.session_token().as_str(), "session-1");
    assert_eq!(decoded.conflict_id().as_str(), "conflict-1");
    assert_eq!(
        decoded.resolution_state(),
        ConflictResolutionState::Acknowledged
    );
    assert_eq!(
        serde_json::to_value(&decoded).expect("request serializes"),
        valid
    );

    let invalid_session_token = json!({
        "session_token": "",
        "conflict_id": "conflict-1",
        "resolution_state": "resolved",
        "idempotency_key": "idem-resolve"
    });
    serde_json::from_value::<ResolveConflictReq>(invalid_session_token)
        .expect_err("resolve conflict request should reject invalid session tokens");

    let invalid_conflict_id = json!({
        "session_token": "session-1",
        "conflict_id": "conflict 1",
        "resolution_state": "resolved",
        "idempotency_key": "idem-resolve"
    });
    serde_json::from_value::<ResolveConflictReq>(invalid_conflict_id)
        .expect_err("resolve conflict request should reject invalid conflict ids");

    let open_resolution = json!({
        "session_token": "session-1",
        "conflict_id": "conflict-1",
        "resolution_state": "open",
        "idempotency_key": "idem-resolve"
    });
    let err = serde_json::from_value::<ResolveConflictReq>(open_resolution)
        .expect_err("resolve conflict request should reject open resolution state");
    assert!(
        err.to_string()
            .contains("resolve conflict requests must acknowledge or resolve conflicts"),
        "unexpected error: {err}"
    );

    let err = ResolveConflictReq::try_from_raw_parts(
        "session-1",
        "conflict-1",
        ConflictResolutionState::Open,
        "idem-resolve",
    )
    .expect_err("constructor should reject open resolution state");
    assert_eq!(
        err.reason(),
        "resolve conflict requests must acknowledge or resolve conflicts"
    );
}

#[test]
fn reservation_overlap_conflict_payload_rejects_invalid_typed_parts() {
    let valid_payload = json!({
        "reservation_id": "reservation-1",
        "overlapping_reservation_id": "reservation-2",
        "owner_subtask_id": "subtask-1",
        "overlapping_owner_subtask_id": "subtask-2",
        "scope_class": "exact_path",
        "scope_key": "src/lib.rs",
        "overlapping_scope_class": "repo_global",
        "overlapping_scope_key": "repo"
    });

    let payload =
        serde_json::from_value::<ReservationOverlapConflictPayload>(valid_payload.clone())
            .expect("valid reservation overlap payload should decode");
    assert_eq!(payload.reservation_id(), "reservation-1");
    assert_eq!(payload.overlapping_reservation_id(), "reservation-2");
    assert_eq!(payload.owner_subtask_id(), "subtask-1");
    assert_eq!(payload.overlapping_owner_subtask_id(), "subtask-2");
    assert_eq!(payload.scope_class(), ScopeClass::ExactPath);
    assert_eq!(payload.scope_key(), "src/lib.rs");
    assert_eq!(payload.overlapping_scope_class(), ScopeClass::RepoGlobal);
    assert_eq!(payload.overlapping_scope_key(), "repo");

    let serialized = serde_json::to_value(&payload).expect("payload should serialize");
    assert_eq!(serialized, valid_payload);

    let mut blank_reservation = valid_payload.clone();
    blank_reservation["reservation_id"] = json!(" ");
    let error = serde_json::from_value::<ReservationOverlapConflictPayload>(blank_reservation)
        .expect_err("blank reservation ids must be rejected");
    assert!(
        error.to_string().contains("invalid reservation_id"),
        "unexpected error: {error}"
    );

    let mut invalid_repo_global = valid_payload.clone();
    invalid_repo_global["overlapping_scope_key"] = json!("src");
    let error = serde_json::from_value::<ReservationOverlapConflictPayload>(invalid_repo_global)
        .expect_err("repo-global overlap scopes must use canonical key");
    assert!(
        error
            .to_string()
            .contains("repo-global reservation overlap scopes require scope_key `repo`"),
        "unexpected error: {error}"
    );

    let mut empty_scope_key = valid_payload;
    empty_scope_key["scope_key"] = json!(" ");
    let error = serde_json::from_value::<ReservationOverlapConflictPayload>(empty_scope_key)
        .expect_err("non-repo overlap scopes require a key");
    assert!(
        error
            .to_string()
            .contains("exact_path reservation overlap scopes require scope_key"),
        "unexpected error: {error}"
    );

    let padded_scope_key = json!({
        "reservation_id": "reservation-1",
        "overlapping_reservation_id": "reservation-2",
        "owner_subtask_id": "subtask-1",
        "overlapping_owner_subtask_id": "subtask-2",
        "scope_class": "subtree",
        "scope_key": " src ",
        "overlapping_scope_class": "generated_set",
        "overlapping_scope_key": "artifact-manifest"
    });
    let error = serde_json::from_value::<ReservationOverlapConflictPayload>(padded_scope_key)
        .expect_err("overlap scope keys should already be normalized");
    assert!(
        error
            .to_string()
            .contains("subtree reservation overlap scope_key must be normalized"),
        "unexpected error: {error}"
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

    let generated_set_with_padded_member = serde_json::json!({
        "reservation_id": "reservation-1",
        "owner_subtask_id": "subtask-1",
        "scope_class": "generated_set",
        "scope_key": "artifact-manifest",
        "generated_members": [" src/generated.rs"],
        "lease_deadline": 200,
        "state": "active",
        "created_at": 10,
        "updated_at": 11
    });
    let err = serde_json::from_value::<Reservation>(generated_set_with_padded_member)
        .expect_err("generated-set reservation with padded member must be rejected");
    assert!(
        err.to_string()
            .contains("generated-set reservations require normalized generated_members"),
        "unexpected error: {err}"
    );

    let generated_set_with_duplicate_members = serde_json::json!({
        "reservation_id": "reservation-1",
        "owner_subtask_id": "subtask-1",
        "scope_class": "generated_set",
        "scope_key": "artifact-manifest",
        "generated_members": ["src/generated.rs", "src/generated.rs"],
        "lease_deadline": 200,
        "state": "active",
        "created_at": 10,
        "updated_at": 11
    });
    let err = serde_json::from_value::<Reservation>(generated_set_with_duplicate_members)
        .expect_err("generated-set reservation with duplicate members must be rejected");
    assert!(
        err.to_string()
            .contains("generated-set reservations require unique generated_members"),
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
fn reservation_scope_public_shape_rejects_invalid_variant_payloads() {
    let empty_generated_set = serde_json::json!({
        "GeneratedSet": {
            "scope_key": "artifact-manifest",
            "generated_members": []
        }
    });
    let err = serde_json::from_value::<ReservationScope>(empty_generated_set)
        .expect_err("generated-set scope variant should reject empty member sets");
    assert!(
        err.to_string()
            .contains("generated-set reservations require generated_members"),
        "unexpected error: {err}"
    );

    let blank_exact_scope_key = serde_json::json!({
        "ExactPath": {
            "scope_key": " "
        }
    });
    let err = serde_json::from_value::<ReservationScope>(blank_exact_scope_key)
        .expect_err("exact-path scope variant should reject blank keys");
    assert!(
        err.to_string()
            .contains("reservation scope_key must not be empty"),
        "unexpected error: {err}"
    );

    let padded_exact_scope_key = serde_json::json!({
        "ExactPath": {
            "scope_key": " src/lib.rs"
        }
    });
    let err = serde_json::from_value::<ReservationScope>(padded_exact_scope_key)
        .expect_err("exact-path scope variant should reject padded keys");
    assert!(
        err.to_string()
            .contains("reservation scope_key must be normalized"),
        "unexpected error: {err}"
    );
}

#[test]
fn reservation_lifecycle_preserves_flat_state_shape() {
    let raw = serde_json::json!({
        "reservation_id": "reservation-1",
        "owner_subtask_id": "subtask-1",
        "scope_class": "exact_path",
        "scope_key": "src/lib.rs",
        "generated_members": [],
        "lease_deadline": 200,
        "state": "active",
        "created_at": 10,
        "updated_at": 11
    });

    let reservation: Reservation =
        serde_json::from_value(raw.clone()).expect("reservation should deserialize");

    assert_eq!(reservation.state(), ReservationState::Active);
    let serialized = serde_json::to_value(&reservation).expect("reservation should serialize");
    assert_eq!(serialized, raw);
    assert!(
        serialized.get("lifecycle").is_none(),
        "reservation JSON must remain the legacy flat storage shape"
    );
}

#[test]
fn reservation_domain_rejects_non_monotonic_timestamps() {
    let raw = serde_json::json!({
        "reservation_id": "reservation-1",
        "owner_subtask_id": "subtask-1",
        "scope_class": "exact_path",
        "scope_key": "src/lib.rs",
        "generated_members": [],
        "lease_deadline": 200,
        "state": "active",
        "created_at": 20,
        "updated_at": 10
    });

    let err = serde_json::from_value::<Reservation>(raw)
        .expect_err("reservation updated_at before created_at must be rejected");

    assert!(
        err.to_string()
            .contains("reservation updated_at must be greater than or equal to created_at"),
        "unexpected error: {err}"
    );

    let err = Reservation::try_from_parts(
        ReservationId::parse("reservation-1").expect("valid reservation id"),
        SubtaskId::parse("subtask-1").expect("valid subtask id"),
        ScopeClass::ExactPath,
        "src/lib.rs",
        Vec::new(),
        LeaseDeadlineMs::parse(200).expect("valid lease deadline"),
        ReservationState::Active,
        TimestampMs::parse(20).expect("valid timestamp"),
        TimestampMs::parse(10).expect("valid timestamp"),
    )
    .expect_err("reservation constructor must reject non-monotonic timestamps");

    assert!(
        err.contains("reservation updated_at must be greater than or equal to created_at"),
        "unexpected error: {err}"
    );
}

#[test]
fn reservation_request_and_overlap_query_reject_invalid_scope_shapes() {
    let invalid_session_token = serde_json::json!({
        "session_token": "",
        "owner_subtask_id": "subtask-1",
        "scope_class": "exact_path",
        "scope_key": "src/lib.rs",
        "generated_members": [],
        "lease_duration_ms": 60_000,
        "idempotency_key": "idem-reservation"
    });
    serde_json::from_value::<RequestReservationReq>(invalid_session_token)
        .expect_err("reservation request should reject invalid session tokens");

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

    let padded_exact_query_key = serde_json::json!({
        "scope_class": "exact_path",
        "scope_key": " src/lib.rs",
        "generated_members": []
    });
    let err = serde_json::from_value::<OverlapQueryReq>(padded_exact_query_key)
        .expect_err("overlap query should reject padded exact-path keys");
    assert!(
        err.to_string()
            .contains("exact_path reservation scope_key must be normalized"),
        "unexpected error: {err}"
    );
}

#[test]
fn reservation_mutation_payloads_reject_invalid_ids_and_leases() {
    let invalid_release_session = serde_json::json!({
        "session_token": "session 1",
        "reservation_id": "reservation-1",
        "idempotency_key": "idem-release-reservation"
    });
    serde_json::from_value::<ReleaseReservationReq>(invalid_release_session)
        .expect_err("release reservation should reject invalid session tokens");

    let invalid_release_id = serde_json::json!({
        "session_token": "session-1",
        "reservation_id": "",
        "idempotency_key": "idem-release-reservation"
    });
    serde_json::from_value::<ReleaseReservationReq>(invalid_release_id)
        .expect_err("release reservation should reject invalid reservation ids");

    let invalid_release_idempotency = serde_json::json!({
        "session_token": "session-1",
        "reservation_id": "reservation-1",
        "idempotency_key": " "
    });
    serde_json::from_value::<ReleaseReservationReq>(invalid_release_idempotency)
        .expect_err("release reservation should reject blank idempotency keys");

    let invalid_renew_session = serde_json::json!({
        "session_token": "",
        "reservation_id": "reservation-1",
        "extend_by_ms": 10_000,
        "idempotency_key": "idem-renew-reservation"
    });
    serde_json::from_value::<RenewReservationReq>(invalid_renew_session)
        .expect_err("renew reservation should reject invalid session tokens");

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

    let invalid_renew_idempotency = serde_json::json!({
        "session_token": "session-1",
        "reservation_id": "reservation-1",
        "extend_by_ms": 10_000,
        "idempotency_key": " "
    });
    serde_json::from_value::<RenewReservationReq>(invalid_renew_idempotency)
        .expect_err("renew reservation should reject blank idempotency keys");
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
fn session_handle_rejects_invalid_identity_fields() {
    SessionHandle::try_from_raw_parts(
        "session 1",
        "principal-1",
        "instance-1",
        SessionRole::Executor,
    )
    .expect_err("session handles should reject invalid session tokens");

    let invalid_principal = serde_json::json!({
        "session_token": "session-1",
        "agent_principal_id": "principal 1",
        "agent_instance_id": "instance-1",
        "role": "executor"
    });
    serde_json::from_value::<SessionHandle>(invalid_principal)
        .expect_err("session handle deserialization should reject invalid principal ids");

    let invalid_instance = serde_json::json!({
        "session_token": "session-1",
        "agent_principal_id": "principal-1",
        "agent_instance_id": "",
        "role": "executor"
    });
    serde_json::from_value::<SessionHandle>(invalid_instance)
        .expect_err("session handle deserialization should reject invalid instance ids");
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
    let payload =
        HeartbeatReq::try_from_raw_parts("session-1", "idem-1").expect("valid heartbeat request");
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

    assert_eq!(typed.seq(), 42);
    assert_eq!(typed.event_type(), EventType::SessionHeartbeat);
    assert_eq!(typed.object_type(), ObjectType::Session);
    assert_eq!(typed.object_id(), "session-1");
    assert_eq!(typed.payload, EventPayload::SessionHeartbeat(payload));
}

#[test]
fn event_typed_rejects_object_type_that_disagrees_with_payload() {
    let payload =
        HeartbeatReq::try_from_raw_parts("session-1", "idem-1").expect("valid heartbeat request");
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
fn raw_event_json_preserves_flat_shape_and_rejects_payload_metadata_mismatch() {
    let payload =
        HeartbeatReq::try_from_raw_parts("session-1", "idem-1").expect("valid heartbeat request");
    let payload_json =
        serde_json::to_string(&payload).expect("heartbeat serialization must succeed");
    let mut event_json = json!({
        "seq": 42,
        "event_type": EventType::SessionHeartbeat,
        "object_type": ObjectType::Session,
        "object_id": "session-1",
        "actor_kind": "session",
        "session_token": "session-1",
        "payload_json": payload_json,
        "created_at": 1_234
    });

    let event: Event =
        serde_json::from_value(event_json.clone()).expect("coherent raw event should decode");
    assert_eq!(event.payload_json(), payload_json);
    assert_eq!(
        serde_json::to_value(&event).expect("event should serialize"),
        event_json
    );

    event_json["event_type"] = json!(EventType::SessionRegistered);
    let err = serde_json::from_value::<Event>(event_json.clone())
        .expect_err("event_type must match payload_json shape");
    assert!(
        err.to_string()
            .contains("event payload does not match session_registered"),
        "unexpected error: {err}"
    );

    event_json["event_type"] = json!(EventType::SessionHeartbeat);
    event_json["object_type"] = json!(ObjectType::Claim);
    let err = serde_json::from_value::<Event>(event_json)
        .expect_err("object_type must match payload_json shape");
    assert!(
        err.to_string()
            .contains("event payload implies object_type session"),
        "unexpected error: {err}"
    );
}

#[test]
fn event_domain_rejects_invalid_sequence_and_object_id() {
    let payload =
        HeartbeatReq::try_from_raw_parts("session-1", "idem-1").expect("valid heartbeat request");
    let payload_json =
        serde_json::to_string(&payload).expect("heartbeat serialization must succeed");

    let invalid_seq = Event::session(
        0,
        EventType::SessionHeartbeat,
        ObjectType::Session,
        "session-1".to_owned(),
        SessionToken::parse("session-1").expect("valid session token"),
        payload_json.clone(),
        TimestampMs::parse(1_234).expect("valid timestamp"),
    )
    .expect_err("event sequence must be positive");
    assert!(
        invalid_seq.contains("invalid event_seq"),
        "unexpected error: {invalid_seq}"
    );

    let invalid_object_id = Event::session(
        1,
        EventType::SessionHeartbeat,
        ObjectType::Session,
        " session-1".to_owned(),
        SessionToken::parse("session-1").expect("valid session token"),
        payload_json,
        TimestampMs::parse(1_234).expect("valid timestamp"),
    )
    .expect_err("event object id must be token-shaped");
    assert!(
        invalid_object_id.contains("invalid event_object_id"),
        "unexpected error: {invalid_object_id}"
    );
}

#[test]
fn typed_event_json_rejects_metadata_that_disagrees_with_payload() {
    let payload = EventPayload::SessionHeartbeat(
        HeartbeatReq::try_from_raw_parts("session-1", "idem-1").expect("valid heartbeat request"),
    );
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
    assert_eq!(subtask.object_id(), "subtask-1");
    assert_eq!(subtask.openspec_task_id(), Some("1.1"));
    assert_eq!(subtask.task_digest(), Some("blake3:task"));
    assert!(subtask.proposal_digest().is_none());
    let subtask_json = serde_json::to_value(&subtask).expect("subtask provenance serializes");
    assert_eq!(subtask_json["object_id"], "subtask-1");

    let generated_artifact_subtask = r#"{"object_type":"subtask","object_id":"subtask-1","planning_format":"openspec","openspec_change_id":"change-1","openspec_change_path":"openspec/changes/change-1","openspec_task_id":"1.1","proposal_digest":null,"design_digest":null,"tasks_digest":"blake3:tasks","spec_digests":[],"source_digests":[],"mission_artifact_digests":[{"path":".codex/state/better-droid/change-1/mission/mission-packet.json","digest":"blake3:packet"}],"mission_artifacts":[".codex/state/better-droid/change-1/mission/mission-packet.json"],"task_digest":"blake3:task","updated_at":123}"#;
    let generated_artifact =
        serde_json::from_str::<OpenSpecImportProvenance>(generated_artifact_subtask)
            .expect("generated artifact provenance should decode");
    let generated_artifact_json = serde_json::to_value(&generated_artifact)
        .expect("generated artifact provenance serializes");
    assert_eq!(
        generated_artifact_json["mission_artifact_metadata"][0]["storage_class"],
        "runtime_generated"
    );
    assert_eq!(
        generated_artifact_json["mission_artifact_metadata"][0]["schema"],
        "mission_packet.v1"
    );
    assert_eq!(
        generated_artifact_json["mission_artifact_metadata"][0]["digest"],
        "blake3:packet"
    );
    let explicit_metadata = r#"{"object_type":"subtask","object_id":"subtask-1","planning_format":"openspec","openspec_change_id":"change-1","openspec_change_path":"openspec/changes/change-1","openspec_task_id":"1.1","proposal_digest":null,"design_digest":null,"tasks_digest":"blake3:tasks","spec_digests":[],"source_digests":[],"mission_artifact_digests":[{"path":".codex/state/better-droid/change-1/mission/mission-packet.json","digest":"blake3:packet"}],"mission_artifact_metadata":[{"artifact_name":"mission-packet.json","artifact_kind":"better-droid-mission-artifact","schema":"mission_packet.v1","digest":"blake3:packet","storage_class":"runtime_generated","locator":".codex/state/better-droid/change-1/mission/mission-packet.json","generator":"persisted metadata sentinel"}],"mission_artifacts":[".codex/state/better-droid/change-1/mission/mission-packet.json"],"task_digest":"blake3:task","updated_at":123}"#;
    let explicit = serde_json::from_str::<OpenSpecImportProvenance>(explicit_metadata)
        .expect("explicit mission artifact metadata should decode");
    assert_eq!(
        explicit.mission_artifact_metadata()[0].generator,
        "persisted metadata sentinel"
    );
    let explicit_json =
        serde_json::to_value(&explicit).expect("explicit metadata provenance serializes");
    assert_eq!(
        explicit_json["mission_artifact_metadata"][0]["generator"],
        "persisted metadata sentinel"
    );

    let invalid_subtask_id = r#"{"object_type":"subtask","object_id":"subtask 1","planning_format":"openspec","openspec_change_id":"change-1","openspec_change_path":"openspec/changes/change-1","openspec_task_id":"1.1","proposal_digest":null,"design_digest":null,"tasks_digest":"blake3:tasks","spec_digests":[],"source_digests":[],"mission_artifact_digests":[],"mission_artifacts":[],"task_digest":"blake3:task","updated_at":123}"#;
    let invalid_subtask_id_err =
        serde_json::from_str::<OpenSpecImportProvenance>(invalid_subtask_id)
            .expect_err("subtask provenance should reject invalid Covey ids");
    assert!(
        invalid_subtask_id_err
            .to_string()
            .contains("invalid subtask_id"),
        "unexpected error: {invalid_subtask_id_err}"
    );
    let invalid_task_id = r#"{"object_type":"subtask","object_id":"subtask-1","planning_format":"openspec","openspec_change_id":"change-1","openspec_change_path":"openspec/changes/change-1","openspec_task_id":"task-one","proposal_digest":null,"design_digest":null,"tasks_digest":"blake3:tasks","spec_digests":[],"source_digests":[],"mission_artifact_digests":[],"mission_artifacts":[],"task_digest":"blake3:task","updated_at":123}"#;
    let invalid_task_id_err = serde_json::from_str::<OpenSpecImportProvenance>(invalid_task_id)
        .expect_err("subtask provenance should reject invalid OpenSpec task ids");
    assert!(
        invalid_task_id_err
            .to_string()
            .contains("OpenSpec task id must be hierarchical numeric form"),
        "unexpected error: {invalid_task_id_err}"
    );

    let invalid_change_id = r#"{"object_type":"subtask","object_id":"subtask-1","planning_format":"openspec","openspec_change_id":"Change_1","openspec_change_path":"openspec/changes/change-1","openspec_task_id":"1.1","proposal_digest":null,"design_digest":null,"tasks_digest":"blake3:tasks","spec_digests":[],"source_digests":[],"mission_artifact_digests":[],"mission_artifacts":[],"task_digest":"blake3:task","updated_at":123}"#;
    let invalid_change_id_err = serde_json::from_str::<OpenSpecImportProvenance>(invalid_change_id)
        .expect_err("OpenSpec provenance should reject invalid change ids");
    assert!(
        invalid_change_id_err
            .to_string()
            .contains("invalid openspec_change_id"),
        "unexpected error: {invalid_change_id_err}"
    );
    OpenSpecImportProvenanceCommon::new(
        "Change_1",
        "openspec/changes/change-1",
        "blake3:tasks",
        vec![],
        vec![],
        vec![],
        TimestampMs::parse(123).expect("valid timestamp"),
    )
    .expect_err("OpenSpec provenance constructor should reject invalid change ids");
    OpenSpecImportProvenanceCommon::new(
        "change-1",
        "/openspec/changes/change-1",
        "blake3:tasks",
        vec![],
        vec![],
        vec![],
        TimestampMs::parse(123).expect("valid timestamp"),
    )
    .expect_err("OpenSpec provenance constructor should reject absolute change paths");
    OpenSpecImportProvenanceCommon::new(
        "change-1",
        "openspec/changes/change-1",
        "sha256:tasks",
        vec![],
        vec![],
        vec![],
        TimestampMs::parse(123).expect("valid timestamp"),
    )
    .expect_err("OpenSpec provenance constructor should reject invalid tasks digest");
    OpenSpecImportProvenanceCommon::new(
        "change-1",
        "openspec/changes/change-1",
        "blake3:tasks",
        vec![],
        vec![],
        vec!["../mission-packet.json".to_owned()],
        TimestampMs::parse(123).expect("valid timestamp"),
    )
    .expect_err("OpenSpec provenance constructor should reject invalid mission artifact paths");
    let common_with_metadata = OpenSpecImportProvenanceCommon::with_mission_artifact_metadata(
        "change-1",
        "openspec/changes/change-1",
        "blake3:tasks",
        vec![],
        vec![
            OpenSpecSourceDigest::new(
                ".codex/state/better-droid/change-1/mission/mission-packet.json",
                "blake3:packet",
            )
            .expect("valid mission artifact digest"),
        ],
        vec![OpenSpecMissionArtifactMetadata {
            artifact_name: "mission-packet.json".to_owned(),
            artifact_kind: "better-droid-mission-artifact".to_owned(),
            schema: "mission_packet.v1".to_owned(),
            digest: "blake3:packet".to_owned(),
            storage_class: "runtime_generated".to_owned(),
            locator: ".codex/state/better-droid/change-1/mission/mission-packet.json".to_owned(),
            generator: "persisted metadata sentinel".to_owned(),
        }],
        vec![".codex/state/better-droid/change-1/mission/mission-packet.json".to_owned()],
        TimestampMs::parse(123).expect("valid timestamp"),
    )
    .expect("explicit mission artifact metadata should survive constructor");
    assert_eq!(
        common_with_metadata.mission_artifact_metadata()[0].generator,
        "persisted metadata sentinel"
    );

    let common = OpenSpecImportProvenanceCommon::new(
        "change-1",
        "openspec/changes/change-1",
        "blake3:tasks",
        vec![],
        vec![],
        vec![],
        TimestampMs::parse(123).expect("valid timestamp"),
    )
    .expect("valid OpenSpec provenance common fields");
    OpenSpecImportProvenance::subtask(
        common.clone(),
        "subtask 1",
        "1.1".to_owned(),
        "blake3:task".to_owned(),
    )
    .expect_err("subtask provenance constructor should reject invalid Covey ids");
    OpenSpecImportProvenance::subtask(
        common.clone(),
        "subtask-1",
        "task-one".to_owned(),
        "blake3:task".to_owned(),
    )
    .expect_err("subtask provenance constructor should reject invalid OpenSpec task ids");
    OpenSpecImportProvenance::subtask(
        common.clone(),
        "subtask-1",
        "1.1".to_owned(),
        "sha256:task".to_owned(),
    )
    .expect_err("subtask provenance constructor should reject invalid task digests");
    OpenSpecImportProvenance::meta_task(
        common.clone(),
        "meta 1",
        "blake3:proposal".to_owned(),
        "blake3:design".to_owned(),
        vec![],
    )
    .expect_err("metatask provenance constructor should reject invalid Covey ids");
    OpenSpecImportProvenance::meta_task(
        common,
        "meta-1",
        "sha256:proposal".to_owned(),
        "blake3:design".to_owned(),
        vec![],
    )
    .expect_err("metatask provenance constructor should reject invalid proposal digests");

    let invalid_meta_id = r#"{"object_type":"meta_task","object_id":"meta 1","planning_format":"openspec","openspec_change_id":"change-1","openspec_change_path":"openspec/changes/change-1","openspec_task_id":null,"proposal_digest":"blake3:proposal","design_digest":"blake3:design","tasks_digest":"blake3:tasks","spec_digests":[],"source_digests":[],"mission_artifact_digests":[],"mission_artifacts":[],"task_digest":null,"updated_at":123}"#;
    let invalid_meta_id_err = serde_json::from_str::<OpenSpecImportProvenance>(invalid_meta_id)
        .expect_err("metatask provenance should reject invalid Covey ids");
    assert!(
        invalid_meta_id_err
            .to_string()
            .contains("invalid meta_task_id"),
        "unexpected error: {invalid_meta_id_err}"
    );

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

    let invalid_mission_artifact = r#"{"object_type":"subtask","object_id":"subtask-1","planning_format":"openspec","openspec_change_id":"change-1","openspec_change_path":"openspec/changes/change-1","openspec_task_id":"1.1","proposal_digest":null,"design_digest":null,"tasks_digest":"blake3:tasks","spec_digests":[],"source_digests":[],"mission_artifact_digests":[],"mission_artifacts":["../mission-packet.json"],"task_digest":"blake3:task","updated_at":123}"#;
    let invalid_mission_artifact_err =
        serde_json::from_str::<OpenSpecImportProvenance>(invalid_mission_artifact)
            .expect_err("provenance should reject escaping mission artifact paths");
    assert!(
        invalid_mission_artifact_err
            .to_string()
            .contains("OpenSpec path must not escape upward"),
        "unexpected error: {invalid_mission_artifact_err}"
    );
}

#[test]
fn openspec_import_item_rejects_object_kind_mismatches() {
    let valid_subtask = r#"{"object_type":"subtask","object_id":"subtask-1","openspec_task_id":"1.1","title":"Implement item","task_type":"implementation","task_digest":"blake3:task","source_path":"openspec/changes/change-1/tasks.md","action":"created"}"#;
    let subtask = serde_json::from_str::<ImportOpenSpecItemResult>(valid_subtask)
        .expect("valid subtask import item should decode");
    assert_eq!(subtask.object_type(), ObjectType::Subtask);
    assert_eq!(subtask.object_id(), "subtask-1");
    assert_eq!(subtask.openspec_task_id(), Some("1.1"));
    assert_eq!(subtask.title(), "Implement item");
    assert_eq!(subtask.task_type(), Some("implementation"));
    assert_eq!(subtask.task_digest(), Some("blake3:task"));
    assert_eq!(
        subtask.source_path(),
        Some("openspec/changes/change-1/tasks.md")
    );
    assert!(OpenSpecTaskId::parse("1.1").is_ok());
    assert!(OpenSpecTaskId::parse("task-one").is_err());

    let invalid_subtask_id = r#"{"object_type":"subtask","object_id":"subtask 1","openspec_task_id":"1.1","title":"Implement item","task_type":"implementation","task_digest":"blake3:task","source_path":"openspec/changes/change-1/tasks.md","action":"created"}"#;
    let invalid_subtask_id_err =
        serde_json::from_str::<ImportOpenSpecItemResult>(invalid_subtask_id)
            .expect_err("subtask import items should reject invalid Covey ids");
    assert!(
        invalid_subtask_id_err
            .to_string()
            .contains("invalid subtask_id"),
        "unexpected error: {invalid_subtask_id_err}"
    );
    ImportOpenSpecItemResult::subtask(
        "subtask 1",
        "1.1",
        "Implement item",
        Some("implementation".to_owned()),
        "blake3:task",
        "openspec/changes/change-1/tasks.md",
        ImportOpenSpecAction::Created,
    )
    .expect_err("subtask constructor should reject invalid Covey ids");
    ImportOpenSpecItemResult::subtask(
        "subtask-1",
        "task-one",
        "Implement item",
        Some("implementation".to_owned()),
        "blake3:task",
        "openspec/changes/change-1/tasks.md",
        ImportOpenSpecAction::Created,
    )
    .expect_err("subtask constructor should reject invalid OpenSpec task ids");
    ImportOpenSpecItemResult::subtask(
        "subtask-1",
        "1.1",
        " Implement item",
        Some("implementation".to_owned()),
        "blake3:task",
        "openspec/changes/change-1/tasks.md",
        ImportOpenSpecAction::Created,
    )
    .expect_err("subtask constructor should reject padded titles");
    ImportOpenSpecItemResult::subtask(
        "subtask-1",
        "1.1",
        "Implement item",
        Some(" implementation".to_owned()),
        "blake3:task",
        "openspec/changes/change-1/tasks.md",
        ImportOpenSpecAction::Created,
    )
    .expect_err("subtask constructor should reject padded task types");
    ImportOpenSpecItemResult::subtask(
        "subtask-1",
        "1.1",
        "Implement item",
        Some("implementation\nreview".to_owned()),
        "blake3:task",
        "openspec/changes/change-1/tasks.md",
        ImportOpenSpecAction::Created,
    )
    .expect_err("subtask constructor should reject control characters in task types");
    ImportOpenSpecItemResult::subtask(
        "subtask-1",
        "1.1",
        "Implement item",
        Some("implementation".to_owned()),
        "sha256:task",
        "openspec/changes/change-1/tasks.md",
        ImportOpenSpecAction::Created,
    )
    .expect_err("subtask constructor should reject invalid task digests");
    ImportOpenSpecItemResult::subtask(
        "subtask-1",
        "1.1",
        "Implement item",
        Some("implementation".to_owned()),
        "blake3:task",
        "/openspec/changes/change-1/tasks.md",
        ImportOpenSpecAction::Created,
    )
    .expect_err("subtask constructor should reject absolute source paths");

    let invalid_meta_id = r#"{"object_type":"meta_task","object_id":"meta 1","openspec_task_id":null,"title":"Change prompt","task_type":null,"task_digest":null,"source_path":null,"action":"created"}"#;
    let invalid_meta_id_err = serde_json::from_str::<ImportOpenSpecItemResult>(invalid_meta_id)
        .expect_err("metatask import items should reject invalid Covey ids");
    assert!(
        invalid_meta_id_err
            .to_string()
            .contains("invalid meta_task_id"),
        "unexpected error: {invalid_meta_id_err}"
    );
    ImportOpenSpecItemResult::meta_task("meta 1", "Change prompt", ImportOpenSpecAction::Created)
        .expect_err("metatask constructor should reject invalid Covey ids");

    let blank_meta_title = r#"{"object_type":"meta_task","object_id":"meta-1","openspec_task_id":null,"title":" ","task_type":null,"task_digest":null,"source_path":null,"action":"created"}"#;
    serde_json::from_str::<ImportOpenSpecItemResult>(blank_meta_title)
        .expect_err("metatask import items should reject blank prompt titles");

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

    let subtask_invalid_digest = r#"{"object_type":"subtask","object_id":"subtask-1","openspec_task_id":"1.1","title":"Implement item","task_type":null,"task_digest":"sha256:task","source_path":"openspec/changes/change-1/tasks.md","action":"created"}"#;
    let subtask_invalid_digest_err =
        serde_json::from_str::<ImportOpenSpecItemResult>(subtask_invalid_digest)
            .expect_err("subtask item with invalid task digest must be rejected");
    assert!(
        subtask_invalid_digest_err
            .to_string()
            .contains("invalid openspec_digest"),
        "unexpected error: {subtask_invalid_digest_err}"
    );

    let subtask_invalid_source_path = r#"{"object_type":"subtask","object_id":"subtask-1","openspec_task_id":"1.1","title":"Implement item","task_type":null,"task_digest":"blake3:task","source_path":"/openspec/changes/change-1/tasks.md","action":"created"}"#;
    let subtask_invalid_source_path_err =
        serde_json::from_str::<ImportOpenSpecItemResult>(subtask_invalid_source_path)
            .expect_err("subtask item with absolute source path must be rejected");
    assert!(
        subtask_invalid_source_path_err
            .to_string()
            .contains("OpenSpec path must be relative"),
        "unexpected error: {subtask_invalid_source_path_err}"
    );

    let subtask_invalid_task_id = r#"{"object_type":"subtask","object_id":"subtask-1","openspec_task_id":"task-one","title":"Implement item","task_type":null,"task_digest":"blake3:task","source_path":"openspec/changes/change-1/tasks.md","action":"created"}"#;
    let subtask_invalid_task_id_err =
        serde_json::from_str::<ImportOpenSpecItemResult>(subtask_invalid_task_id)
            .expect_err("subtask item with invalid task id must be rejected");
    assert!(
        subtask_invalid_task_id_err
            .to_string()
            .contains("OpenSpec task id must be hierarchical numeric form"),
        "unexpected error: {subtask_invalid_task_id_err}"
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
    assert_eq!(conflict.object_id(), "subtask-1");
    assert_eq!(conflict.openspec_task_id(), "1.1");
    assert_eq!(
        conflict.reason(),
        ImportOpenSpecConflictReason::ActiveClaimChangedSource
    );
    assert_eq!(conflict.task_digest(), "blake3:task");

    let invalid_object_id = r#"{"object_type":"subtask","object_id":"subtask 1","openspec_task_id":"1.1","reason":"active_claim_changed_source","source_path":"openspec/changes/change-1/tasks.md","task_digest":"blake3:task"}"#;
    let invalid_object_id_err = serde_json::from_str::<ImportOpenSpecConflict>(invalid_object_id)
        .expect_err("subtask conflict should reject invalid Covey ids");
    assert!(
        invalid_object_id_err
            .to_string()
            .contains("invalid subtask_id"),
        "unexpected error: {invalid_object_id_err}"
    );
    ImportOpenSpecConflict::subtask(
        "subtask 1",
        "1.1",
        ImportOpenSpecConflictReason::ActiveClaimChangedSource,
        "openspec/changes/change-1/tasks.md",
        "blake3:task",
    )
    .expect_err("conflict constructor should reject invalid Covey ids");
    ImportOpenSpecConflict::subtask(
        "subtask-1",
        "task-one",
        ImportOpenSpecConflictReason::ActiveClaimChangedSource,
        "openspec/changes/change-1/tasks.md",
        "blake3:task",
    )
    .expect_err("conflict constructor should reject invalid OpenSpec task ids");
    ImportOpenSpecConflict::subtask(
        "subtask-1",
        "1.1",
        ImportOpenSpecConflictReason::ActiveClaimChangedSource,
        "openspec/changes/change-1/tasks.md",
        "sha256:task",
    )
    .expect_err("conflict constructor should reject invalid task digests");
    ImportOpenSpecConflict::subtask(
        "subtask-1",
        "1.1",
        ImportOpenSpecConflictReason::ActiveClaimChangedSource,
        "../tasks.md",
        "blake3:task",
    )
    .expect_err("conflict constructor should reject escaping source paths");

    let invalid_reason = r#"{"object_type":"subtask","object_id":"subtask-1","openspec_task_id":"1.1","reason":"unknown_conflict_reason","source_path":"openspec/changes/change-1/tasks.md","task_digest":"blake3:task"}"#;
    serde_json::from_str::<ImportOpenSpecConflict>(invalid_reason)
        .expect_err("subtask conflict with unknown reason must be rejected");

    let missing_task_id = r#"{"object_type":"subtask","object_id":"subtask-1","openspec_task_id":null,"reason":"active_claim_changed_source","source_path":"openspec/changes/change-1/tasks.md","task_digest":"blake3:task"}"#;
    let missing_task_id_err = serde_json::from_str::<ImportOpenSpecConflict>(missing_task_id)
        .expect_err("subtask conflict without task id must be rejected");
    assert!(
        missing_task_id_err
            .to_string()
            .contains("subtask OpenSpec import conflicts require openspec_task_id")
    );

    let invalid_task_id = r#"{"object_type":"subtask","object_id":"subtask-1","openspec_task_id":"task-one","reason":"active_claim_changed_source","source_path":"openspec/changes/change-1/tasks.md","task_digest":"blake3:task"}"#;
    let invalid_task_id_err = serde_json::from_str::<ImportOpenSpecConflict>(invalid_task_id)
        .expect_err("subtask conflict with invalid OpenSpec task id must be rejected");
    assert!(
        invalid_task_id_err
            .to_string()
            .contains("OpenSpec task id must be hierarchical numeric form"),
        "unexpected error: {invalid_task_id_err}"
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

    let invalid_task_digest = r#"{"object_type":"subtask","object_id":"subtask-1","openspec_task_id":"1.1","reason":"active_claim_changed_source","source_path":"openspec/changes/change-1/tasks.md","task_digest":"sha256:task"}"#;
    let invalid_task_digest_err =
        serde_json::from_str::<ImportOpenSpecConflict>(invalid_task_digest)
            .expect_err("subtask conflict with invalid task digest must be rejected");
    assert!(
        invalid_task_digest_err
            .to_string()
            .contains("invalid openspec_digest"),
        "unexpected error: {invalid_task_digest_err}"
    );

    let invalid_source_path = r#"{"object_type":"subtask","object_id":"subtask-1","openspec_task_id":"1.1","reason":"active_claim_changed_source","source_path":"../tasks.md","task_digest":"blake3:task"}"#;
    let invalid_source_path_err =
        serde_json::from_str::<ImportOpenSpecConflict>(invalid_source_path)
            .expect_err("subtask conflict with escaping source path must be rejected");
    assert!(
        invalid_source_path_err
            .to_string()
            .contains("OpenSpec path must not escape upward"),
        "unexpected error: {invalid_source_path_err}"
    );

    let unsupported_object = r#"{"object_type":"meta_task","object_id":"meta-1","openspec_task_id":null,"reason":"active_claim_changed_source","source_path":"openspec/changes/change-1/tasks.md","task_digest":null}"#;
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
            .contains("OpenSpec path must not be empty"),
        "unexpected error: {empty_path_err}"
    );

    let absolute_path = json!({"path": "/openspec/tasks.md", "digest": "blake3:abc"});
    let absolute_path_err = serde_json::from_value::<OpenSpecSourceDigest>(absolute_path)
        .expect_err("absolute source digest path must be rejected");
    assert!(
        absolute_path_err
            .to_string()
            .contains("OpenSpec path must be relative"),
        "unexpected error: {absolute_path_err}"
    );

    let escaping_path = json!({"path": "openspec/../tasks.md", "digest": "blake3:abc"});
    let escaping_path_err = serde_json::from_value::<OpenSpecSourceDigest>(escaping_path)
        .expect_err("escaping source digest path must be rejected");
    assert!(
        escaping_path_err
            .to_string()
            .contains("OpenSpec path must not escape upward"),
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
            )
            .expect("valid OpenSpec provenance common fields"),
            "subtask-1",
            "1.1".to_owned(),
            "blake3:task".to_owned(),
        )
        .expect("valid OpenSpec provenance fixture"),
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
    let heartbeat = HeartbeatReq::try_from_raw_parts("session-1", "idem-heartbeat")
        .expect("valid heartbeat request");
    let exit = ExitSessionReq::try_from_raw_parts("session-1", "idem-exit")
        .expect("valid exit-session request");
    let submit = SubmitMetaTaskReq::try_from_raw_parts("session-1", "do work", "idem-submit")
        .expect("valid submit-meta-task request");
    let cancel = CancelMetaTaskReq::try_from_raw_parts("session-1", "meta-1", "idem-cancel")
        .expect("valid cancel-meta-task request");
    let create = CreateSubtaskRequest {
        session_token: SessionToken::parse("session-1").expect("valid session token"),
        meta_task_id: MetaTaskId::parse("meta-1").expect("valid meta-task id"),
        subtask_id: Some(SubtaskId::parse("subtask-1").expect("valid subtask id")),
        title: SubtaskTitle::parse("implement").expect("valid subtask title"),
        priority: SubtaskPriority::parse(10).expect("valid subtask priority"),
        idempotency_key: IdempotencyKey::parse("idem-create").expect("valid idempotency key"),
    };
    let claim = ClaimResult::new(
        ClaimId::parse("claim-1").expect("valid claim id"),
        SubtaskId::parse("subtask-1").expect("valid subtask id"),
        FenceSeq::parse(1).expect("valid fence"),
        LeaseDeadlineMs::parse(500).expect("valid deadline"),
    );
    let start = StartSubtaskReq::try_from_raw_parts(
        "session-1".to_owned(),
        ClaimId::parse("claim-1").expect("valid claim id"),
        FenceSeq::parse(1).expect("valid fence"),
        "idem-start".to_owned(),
    )
    .expect("valid start-subtask request");
    let release = ReleaseClaimReq::try_from_raw_parts(
        "session-1".to_owned(),
        ClaimId::parse("claim-1").expect("valid claim id"),
        FenceSeq::parse(1).expect("valid fence"),
        "idem-release".to_owned(),
    )
    .expect("valid release-claim request");
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
        session_token: SessionToken::parse("session-1").expect("valid session token"),
        queue_id: QueueId::parse("queue-1").expect("valid queue id"),
        claim_fence_seq: FenceSeq::parse(1).expect("valid fence"),
        idempotency_key: IdempotencyKey::parse("idem-applied").expect("valid idempotency key"),
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
    let resolve = ResolveConflictReq::try_from_raw_parts(
        "session-1",
        "conflict-1",
        ConflictResolutionState::Resolved,
        "idem-resolve",
    )
    .expect("valid conflict resolution request");
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
            EventPayload::SubtaskAbandoned(
                AbandonSubtaskReq::try_from_raw_parts(
                    start.session_token.clone(),
                    start.claim_id.clone(),
                    start.fence_seq,
                    start.idempotency_key.clone(),
                )
                .expect("valid abandon-subtask request"),
            ),
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

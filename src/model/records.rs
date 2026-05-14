use derive_new::new;
use serde::{Deserialize, Serialize};

use super::{
    AbandonSubtaskReq, ActorKind, ArtifactKind, CancelMetaTaskReq, ClaimResult, ClaimState,
    ConflictResolutionState, CreateSubtaskReq, DecideReviewReq, EnqueueForApplyReq, EventType,
    ExitSessionReq, HeartbeatReq, ImportOpenSpecEvent, MarkAppliedReq, MetaTaskState, ObjectType,
    PublishArtifactReq, ReadyQueueClaim, ReadyQueueState, RecordApplyVerificationReq,
    RecordRuntimeAttestationReq, ReleaseClaimReq, RequestReservationReq, RequestReviewReq,
    ReservationState, ResolveConflictReq, ReviewState, ReviewVerdict, ScopeClass, SessionHandle,
    SessionRole, SessionState, SettlementTarget, StartSubtaskReq, SubmitMetaTaskReq, SubtaskKind,
    SubtaskState, SupersedeQueueItemReq,
};

/// Persisted session row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub session_token: String,
    pub agent_principal_id: String,
    pub agent_instance_id: String,
    pub role: SessionRole,
    pub state: SessionState,
    pub active_subtask_id: Option<String>,
    pub last_heartbeat_at: i64,
    pub last_heartbeat_tick: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Runtime identity evidence bound to one Covey session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAttestation {
    pub session_token: String,
    pub agent_principal_id: String,
    pub agent_instance_id: String,
    pub role: SessionRole,
    pub provider: String,
    pub model: String,
    pub provider_run_id: String,
    pub provider_run_id_issuer: String,
    pub process_id: Option<String>,
    pub container_id: Option<String>,
    pub command_transcript_digest: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub recorded_at: i64,
}

/// Persisted meta-task row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaTask {
    pub meta_task_id: String,
    pub prompt_text: String,
    pub state: MetaTaskState,
    pub created_by: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Persisted subtask row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subtask {
    pub subtask_id: String,
    pub meta_task_id: String,
    pub title: String,
    pub kind: SubtaskKind,
    pub review_target_subtask_id: Option<String>,
    pub review_target_artifact_digest: Option<String>,
    pub state: SubtaskState,
    pub current_claim_id: Option<String>,
    pub artifact_digest: Option<String>,
    pub priority: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Persisted claim row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub claim_id: String,
    pub subtask_id: String,
    pub owner_session_token: String,
    pub fence_seq: i64,
    pub lease_deadline: i64,
    pub state: ClaimState,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Persisted artifact row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub artifact_digest: String,
    pub artifact_kind: ArtifactKind,
    pub base_rev: String,
    pub produced_by_subtask_id: String,
    pub produced_by_session: String,
    pub manifest_path: String,
    pub changed_paths_digest: String,
    pub created_at: i64,
}

/// Persisted review row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub review_id: String,
    pub subtask_id: String,
    pub artifact_digest: String,
    pub reviewer_session: String,
    pub review_subtask_id: Option<String>,
    pub verdict: Option<ReviewVerdict>,
    pub findings_digest: Option<String>,
    pub state: ReviewState,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Persisted reservation row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reservation {
    pub reservation_id: String,
    pub owner_subtask_id: String,
    pub scope_class: ScopeClass,
    pub scope_key: String,
    pub generated_members: Vec<String>,
    pub lease_deadline: i64,
    pub state: ReservationState,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Persisted ready-queue row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyQueueItem {
    pub queue_id: String,
    pub artifact_digest: String,
    pub subtask_id: String,
    pub settlement_target: SettlementTarget,
    pub state: ReadyQueueState,
    pub claimed_by_session_token: Option<String>,
    pub claim_fence_seq: Option<i64>,
    pub claim_lease_deadline: Option<i64>,
    pub enqueued_at: i64,
    pub updated_at: i64,
}

/// Accepted verifier evidence bound to one apply attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyVerification {
    pub queue_id: String,
    pub artifact_digest: String,
    pub review_id: String,
    pub findings_digest: String,
    pub claim_fence_seq: i64,
    pub verifier: String,
    pub verdict_digest: String,
    pub seal_digest: String,
    pub recorded_by_session: String,
    pub created_at: i64,
}

/// Raw event-log row with JSON payload.
///
/// This log is an audit trail for subscribers, not an event-sourced state engine.
/// The relational tables remain authoritative; replaying `event_log` is not a
/// supported recovery path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub seq: i64,
    pub event_type: EventType,
    pub object_type: ObjectType,
    pub object_id: String,
    pub actor_kind: ActorKind,
    pub session_token: Option<String>,
    pub payload_json: String,
    pub created_at: i64,
}

/// Decoded event-log row with typed payload.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedEvent {
    pub seq: i64,
    pub event_type: EventType,
    pub object_type: ObjectType,
    pub object_id: String,
    pub actor_kind: ActorKind,
    pub session_token: Option<String>,
    pub payload: EventPayload,
    pub created_at: i64,
}

/// Payload emitted when stale sessions are reaped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct StaleSessionsPayload {
    pub stale_sessions: usize,
}

/// Payload emitted when leases are expired in bulk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct ExpiredCountPayload {
    pub expired_count: usize,
}

/// Typed event payload union for the append-only event log audit trail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventPayload {
    SessionRegistered(SessionHandle),
    SessionHeartbeat(HeartbeatReq),
    SessionExited(ExitSessionReq),
    RuntimeAttestationRecorded(RecordRuntimeAttestationReq),
    MetaTaskSubmitted(SubmitMetaTaskReq),
    MetaTaskCancelled(CancelMetaTaskReq),
    SubtaskCreated(CreateSubtaskReq),
    SubtaskClaimed(ClaimResult),
    SubtaskStarted(StartSubtaskReq),
    SubtaskAbandoned(AbandonSubtaskReq),
    ClaimReleased(ReleaseClaimReq),
    ClaimRenewed(ClaimResult),
    ArtifactPublished(PublishArtifactReq),
    ReviewRequested(RequestReviewReq),
    ReviewDecided(DecideReviewReq),
    ReadyQueueEnqueued(EnqueueForApplyReq),
    ReadyQueueInFlight(ReadyQueueClaim),
    ApplyVerificationRecorded(RecordApplyVerificationReq),
    ReadyQueueApplied(MarkAppliedReq),
    ReadyQueueSuperseded(SupersedeQueueItemReq),
    ReservationRequested(RequestReservationReq),
    ReservationReleased(Reservation),
    ReservationRenewed(Reservation),
    ConflictResolved(ResolveConflictReq),
    SessionsReaped(StaleSessionsPayload),
    ClaimsExpired(ExpiredCountPayload),
    ReservationsExpired(ExpiredCountPayload),
    OpenSpecImported(Box<ImportOpenSpecEvent>),
}

/// Persisted unresolved conflict row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub conflict_id: String,
    pub object_type: ObjectType,
    pub object_id: String,
    pub conflict_kind: String,
    pub payload_json: String,
    pub detected_at: i64,
    pub resolution_state: ConflictResolutionState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MutationIdempotencyRecord {
    pub actor_key: String,
    pub operation: String,
    pub idempotency_key: String,
    pub request_hash: String,
    pub response_json: String,
    pub created_at: i64,
}

/// Conflict payload describing an overlapping reservation pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReservationOverlapConflictPayload {
    pub reservation_id: String,
    pub overlapping_reservation_id: String,
    pub owner_subtask_id: String,
    pub overlapping_owner_subtask_id: String,
    pub scope_class: ScopeClass,
    pub scope_key: String,
    pub overlapping_scope_class: ScopeClass,
    pub overlapping_scope_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub(crate) struct OverlapCandidate {
    pub scope_class: ScopeClass,
    pub scope_key: String,
    pub generated_members: Vec<String>,
}

//! Wire request/response DTOs for the public Covey API.
//!
//! These structs intentionally keep CLI/API payloads in their JSON-compatible
//! primitive form. Operations parse the values into validated domain newtypes
//! before storing them or building durable domain records.

use derive_new::new;
use serde::{Deserialize, Serialize};

use super::{
    ArtifactKind, ConflictResolutionState, ReviewVerdict, ScopeClass, SessionRole, SettlementTarget,
};

/// Request to register a session with immutable identity metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterSessionReq {
    pub agent_principal_id: String,
    pub agent_instance_id: String,
    pub role: SessionRole,
    pub idempotency_key: String,
}

/// Session identity returned after successful registration.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct SessionHandle {
    pub session_token: String,
    pub agent_principal_id: String,
    pub agent_instance_id: String,
    pub role: SessionRole,
}

/// Request to create a meta-task from operator intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitMetaTaskReq {
    pub session_token: String,
    pub prompt_text: String,
    pub idempotency_key: String,
}

/// Request to cancel a meta-task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelMetaTaskReq {
    pub session_token: String,
    pub meta_task_id: String,
    pub idempotency_key: String,
}

/// Request to heartbeat an active session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatReq {
    pub session_token: String,
    pub idempotency_key: String,
}

/// Request to exit an active session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitSessionReq {
    pub session_token: String,
    pub idempotency_key: String,
}

/// Request to bind runtime identity evidence to a Covey session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordRuntimeAttestationReq {
    pub session_token: String,
    pub provider: String,
    pub model: String,
    pub provider_run_id: String,
    pub provider_run_id_issuer: String,
    pub process_id: Option<String>,
    pub container_id: Option<String>,
    pub command_transcript_digest: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub idempotency_key: String,
}

/// Request to create a work subtask from orchestrator-owned input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSubtaskRequest {
    pub session_token: String,
    pub meta_task_id: String,
    pub subtask_id: Option<String>,
    pub title: String,
    pub priority: i64,
    pub idempotency_key: String,
}

/// Request to claim the next available subtask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimNextReq {
    pub session_token: String,
    pub lease_duration_ms: i64,
    pub idempotency_key: String,
}

/// Request to claim a specific subtask by ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimSubtaskReq {
    pub session_token: String,
    pub subtask_id: String,
    pub lease_duration_ms: i64,
    pub idempotency_key: String,
}

/// Claim token and lease metadata returned after a successful claim.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct ClaimResult {
    pub claim_id: String,
    pub subtask_id: String,
    pub fence_seq: i64,
    pub lease_deadline: i64,
}

/// Request to start work on a claimed subtask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartSubtaskReq {
    pub session_token: String,
    pub claim_id: String,
    pub fence_seq: i64,
    pub idempotency_key: String,
}

/// Request to abandon a claimed subtask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbandonSubtaskReq {
    pub session_token: String,
    pub claim_id: String,
    pub fence_seq: i64,
    pub idempotency_key: String,
}

/// Request to release a held claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseClaimReq {
    pub session_token: String,
    pub claim_id: String,
    pub fence_seq: i64,
    pub idempotency_key: String,
}

/// Request to renew an active claim lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenewClaimReq {
    pub session_token: String,
    pub claim_id: String,
    pub fence_seq: i64,
    pub extend_by_ms: i64,
    pub idempotency_key: String,
}

/// Request to publish an immutable artifact for a work subtask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishArtifactReq {
    pub session_token: String,
    pub claim_id: String,
    pub fence_seq: i64,
    pub artifact_digest: String,
    pub artifact_kind: ArtifactKind,
    pub base_rev: String,
    pub manifest_path: String,
    pub changed_paths_digest: String,
    pub idempotency_key: String,
}

/// Request to create a review subtask for an exact artifact digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestReviewReq {
    pub session_token: String,
    pub subtask_id: String,
    pub artifact_digest: String,
    pub review_subtask_id: Option<String>,
    pub priority: i64,
    pub idempotency_key: String,
}

/// Request to decide a review while holding the matching review-subtask claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecideReviewReq {
    pub session_token: String,
    pub review_id: String,
    pub claim_id: String,
    pub fence_seq: i64,
    pub verdict: ReviewVerdict,
    pub findings_digest: String,
    pub idempotency_key: String,
}

/// Request to enqueue an approved artifact for apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnqueueForApplyReq {
    pub session_token: String,
    pub artifact_digest: String,
    pub subtask_id: String,
    pub settlement_target: SettlementTarget,
    pub idempotency_key: String,
}

/// Request to atomically claim the next ready-queue item for apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimReadyQueueReq {
    pub session_token: String,
    pub lease_duration_ms: i64,
    pub idempotency_key: String,
}

/// Claimed ready-queue item with apply fence and lease metadata.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct ReadyQueueClaim {
    pub queue_id: String,
    pub artifact_digest: String,
    pub subtask_id: String,
    pub settlement_target: SettlementTarget,
    pub claim_fence_seq: i64,
    pub lease_deadline: i64,
}

/// Request to mark a ready-queue item in flight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkInFlightReq {
    pub session_token: String,
    pub queue_id: String,
    pub lease_duration_ms: i64,
    pub idempotency_key: String,
}

/// Request to record an accepted verifier verdict for one apply attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordApplyVerificationReq {
    pub session_token: String,
    pub queue_id: String,
    pub artifact_digest: String,
    pub review_id: String,
    pub findings_digest: String,
    pub claim_fence_seq: i64,
    pub verifier: String,
    pub verdict_digest: String,
    pub seal_digest: String,
    pub idempotency_key: String,
}

/// Request to verify that a landing authorization is still backed by live Covey state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyLandingAuthorizationReq {
    pub session_token: String,
    pub queue_id: String,
    pub artifact_digest: String,
    pub review_id: String,
    pub findings_digest: String,
    pub claim_fence_seq: i64,
    pub verifier: String,
    pub verdict_digest: String,
    pub seal_digest: String,
}

/// Request to mark an in-flight queue item applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkAppliedReq {
    pub session_token: String,
    pub queue_id: String,
    pub claim_fence_seq: i64,
    pub idempotency_key: String,
}

/// Request to supersede a queued or in-flight queue item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupersedeQueueItemReq {
    pub session_token: String,
    pub queue_id: String,
    pub idempotency_key: String,
}

/// Request to create an advisory reservation for a subtask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestReservationReq {
    pub session_token: String,
    pub owner_subtask_id: String,
    pub scope_class: ScopeClass,
    pub scope_key: String,
    pub generated_members: Vec<String>,
    pub lease_duration_ms: i64,
    pub idempotency_key: String,
}

/// Request to release an existing reservation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseReservationReq {
    pub session_token: String,
    pub reservation_id: String,
    pub idempotency_key: String,
}

/// Request to renew an active reservation lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenewReservationReq {
    pub session_token: String,
    pub reservation_id: String,
    pub extend_by_ms: i64,
    pub idempotency_key: String,
}

/// Query for overlapping active reservations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlapQueryReq {
    pub scope_class: ScopeClass,
    pub scope_key: String,
    pub generated_members: Vec<String>,
}

/// Query for the Covey lifecycle facts needed by mutAI repoops preflight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoopsAuthoritySnapshotReq {
    pub session_token: String,
    pub claim_id: String,
    pub fence_seq: i64,
    pub paths: Vec<String>,
}

/// Request to update the resolution state of a surfaced conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveConflictReq {
    pub session_token: String,
    pub conflict_id: String,
    pub resolution_state: ConflictResolutionState,
    pub idempotency_key: String,
}

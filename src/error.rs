use crate::model::{
    ArtifactDigest, ClaimId, ClaimState, CompletionPolicy, CoveyTypeValidationError, MetaTaskState,
    ObjectType, QueueId, SessionRole, SessionState, SessionToken, StateValue, SubtaskId,
};
use thiserror::Error;

/// Errors returned by the Covey coordination substrate.
#[derive(Debug, Error)]
pub enum CoveyError {
    #[error("session not found")]
    SessionNotFound,
    #[error("subtask not found")]
    SubtaskNotFound,
    #[error("artifact not found")]
    ArtifactNotFound,
    #[error("review not found")]
    ReviewNotFound,
    #[error("meta-task not found")]
    MetaTaskNotFound,
    #[error("reservation not found")]
    ReservationNotFound,
    #[error("claim not found")]
    ClaimNotFound,
    #[error("queue item not found")]
    QueueItemNotFound,
    #[error("conflict not found")]
    ConflictNotFound,

    #[error("illegal transition on {object}: {from} -> {to}")]
    IllegalTransition {
        from: StateValue,
        to: StateValue,
        object: ObjectType,
    },
    #[error("subtask {subtask_id} is already claimed by {held_by}")]
    SubtaskAlreadyClaimed {
        subtask_id: SubtaskId,
        held_by: SessionToken,
    },
    #[error("session already active for principal {agent_principal_id}")]
    SessionAlreadyActive { agent_principal_id: String },
    #[error("session {session_token} is not active; current state is {state}")]
    SessionNotActive {
        session_token: SessionToken,
        state: SessionState,
    },
    #[error("session {session_token} already has active subtask {active_subtask_id}")]
    SessionAlreadyHasActiveSubtask {
        session_token: SessionToken,
        active_subtask_id: SubtaskId,
    },
    #[error("runtime attestation missing for session {session_token}")]
    RuntimeAttestationMissing { session_token: SessionToken },
    #[error("invalid runtime attestation for session {session_token}: {reason}")]
    InvalidRuntimeAttestation {
        session_token: SessionToken,
        reason: String,
    },
    #[error("invalid session token {session_token}")]
    InvalidSessionToken { session_token: String },
    #[error("invalid idempotency key {idempotency_key}")]
    InvalidIdempotencyKey { idempotency_key: String },
    #[error("idempotency key {idempotency_key} already used for {operation} by {actor_key}")]
    IdempotencyConflict {
        actor_key: String,
        operation: String,
        idempotency_key: String,
    },

    #[error("stale fence token: expected {expected}, provided {provided}")]
    StaleFenceToken { expected: i64, provided: i64 },
    #[error("fence token mismatch")]
    FenceTokenMismatch,
    #[error("claim {claim_id} is not held; current state is {state}")]
    ClaimNotHeld {
        claim_id: ClaimId,
        state: ClaimState,
    },

    #[error("session {session_token} does not own the claim; owner is {claim_owner}")]
    NotClaimOwner {
        session_token: SessionToken,
        claim_owner: SessionToken,
    },
    #[error("session {session_token} does not own the ready-queue claim; owner is {queue_owner}")]
    NotQueueClaimOwner {
        session_token: SessionToken,
        queue_owner: SessionToken,
    },
    #[error("wrong role: expected one of {expected:?}, actual {actual}")]
    WrongRole {
        expected: Vec<SessionRole>,
        actual: SessionRole,
    },

    #[error("artifact digest collision for {digest}")]
    ArtifactDigestCollision { digest: String },
    #[error("duplicate subtask id {subtask_id}")]
    DuplicateSubtaskId { subtask_id: SubtaskId },

    #[error("operation {operation} is not allowed for completion policy {policy}")]
    CompletionPolicyViolation {
        operation: String,
        policy: CompletionPolicy,
    },

    #[error("lease expired for {object_id}")]
    LeaseExpired { object_id: String },

    #[error("review kind mismatch")]
    ReviewKindMismatch,
    #[error("review already open for subtask {subtask_id} and artifact {artifact_digest}")]
    ReviewAlreadyOpen {
        subtask_id: SubtaskId,
        artifact_digest: ArtifactDigest,
    },
    #[error(
        "review {review_id} targets stale artifact {artifact_digest}; subtask {subtask_id} has current artifact {current_artifact_digest}"
    )]
    StaleReviewArtifact {
        review_id: String,
        subtask_id: SubtaskId,
        artifact_digest: ArtifactDigest,
        current_artifact_digest: ArtifactDigest,
    },
    #[error(
        "review separation of duties violated: reviewer principal {reviewer_principal_id} matches producer principal {producer_principal_id}"
    )]
    SeparationOfDutiesViolation {
        reviewer_principal_id: String,
        producer_principal_id: String,
    },
    #[error("apply gate evidence missing for queue item {queue_id}: {reason}")]
    ApplyGateEvidenceMissing { queue_id: QueueId, reason: String },
    #[error(
        "apply gate separation of duties violated: apply gate principal {apply_gate_principal_id} matches {conflicting_role} principal {conflicting_principal_id}"
    )]
    ApplyGateSeparationOfDutiesViolation {
        apply_gate_principal_id: String,
        conflicting_role: String,
        conflicting_principal_id: String,
    },
    #[error("meta-task {meta_task_id} is not executable in state {state}")]
    MetaTaskUnavailable {
        meta_task_id: String,
        state: MetaTaskState,
    },
    #[error("unknown artifact digest {digest}")]
    UnknownArtifactDigest { digest: String },
    #[error("invalid repository path {path}")]
    InvalidPath { path: String },
    #[error("invalid lease duration for {field}: {provided}")]
    InvalidLeaseDuration { field: String, provided: i64 },
    #[error("input {field} exceeds maximum length {max}; got {actual}")]
    InputTooLarge {
        field: String,
        actual: usize,
        max: usize,
    },

    #[error("import source database not found at {path}")]
    ImportSourceNotFound { path: String },
    #[error("invalid source schema in {path}: {detail}")]
    InvalidSourceSchema { path: String, detail: String },
    #[error("invalid import destination: {reason}")]
    InvalidImportDestination { reason: String },
    #[error("invalid import row for source issue {source_issue_id}: {reason}")]
    InvalidImportRow {
        source_issue_id: String,
        reason: String,
    },
    #[error("invalid event shape: {reason}")]
    InvalidEventShape { reason: String },
    #[error("invalid ready-queue metrics shape: {reason}")]
    InvalidReadyQueueMetrics { reason: String },
    #[error("invalid observability row shape: {reason}")]
    InvalidObservabilityRow { reason: String },
    #[error("current-work blocker not found for {blocker_id}")]
    CurrentWorkBlockerNotFound { blocker_id: String },
    #[error("current-work blocker id {blocker_id} is ambiguous")]
    AmbiguousCurrentWorkBlocker { blocker_id: String },
    #[error(
        "import duplicate for source issue {source_issue_id}: subtask {subtask_id} already exists"
    )]
    ImportDuplicate {
        source_issue_id: String,
        subtask_id: SubtaskId,
    },

    #[error(transparent)]
    DatabaseError(#[from] rusqlite::Error),
    #[error(transparent)]
    MigrationError(#[from] rusqlite_migration::Error),
    #[error(transparent)]
    SerializationError(#[from] serde_json::Error),
    #[error(transparent)]
    TypeValidationError(#[from] CoveyTypeValidationError),
}

impl PartialEq for CoveyError {
    fn eq(&self, other: &Self) -> bool {
        use CoveyError::{
            AmbiguousCurrentWorkBlocker, ApplyGateEvidenceMissing,
            ApplyGateSeparationOfDutiesViolation, ArtifactDigestCollision, ArtifactNotFound,
            ClaimNotFound, ClaimNotHeld, CompletionPolicyViolation, ConflictNotFound,
            CurrentWorkBlockerNotFound, DatabaseError, DuplicateSubtaskId, FenceTokenMismatch,
            IdempotencyConflict, IllegalTransition, ImportDuplicate, ImportSourceNotFound,
            InputTooLarge, InvalidEventShape, InvalidIdempotencyKey, InvalidImportDestination,
            InvalidImportRow, InvalidLeaseDuration, InvalidObservabilityRow, InvalidPath,
            InvalidReadyQueueMetrics, InvalidRuntimeAttestation, InvalidSessionToken,
            InvalidSourceSchema, LeaseExpired, MetaTaskNotFound, MetaTaskUnavailable,
            MigrationError, NotClaimOwner, NotQueueClaimOwner, QueueItemNotFound,
            ReservationNotFound, ReviewAlreadyOpen, ReviewKindMismatch, ReviewNotFound,
            RuntimeAttestationMissing, SeparationOfDutiesViolation, SerializationError,
            SessionAlreadyActive, SessionAlreadyHasActiveSubtask, SessionNotActive,
            SessionNotFound, StaleFenceToken, StaleReviewArtifact, SubtaskAlreadyClaimed,
            SubtaskNotFound, TypeValidationError, UnknownArtifactDigest, WrongRole,
        };

        match (self, other) {
            (SessionNotFound, SessionNotFound)
            | (SubtaskNotFound, SubtaskNotFound)
            | (ArtifactNotFound, ArtifactNotFound)
            | (ReviewNotFound, ReviewNotFound)
            | (MetaTaskNotFound, MetaTaskNotFound)
            | (ReservationNotFound, ReservationNotFound)
            | (ClaimNotFound, ClaimNotFound)
            | (QueueItemNotFound, QueueItemNotFound)
            | (ConflictNotFound, ConflictNotFound)
            | (FenceTokenMismatch, FenceTokenMismatch)
            | (ReviewKindMismatch, ReviewKindMismatch) => true,
            (
                IllegalTransition {
                    from: left_from,
                    to: left_to,
                    object: left_object,
                },
                IllegalTransition {
                    from: right_from,
                    to: right_to,
                    object: right_object,
                },
            ) => left_from == right_from && left_to == right_to && left_object == right_object,
            (
                SubtaskAlreadyClaimed {
                    subtask_id: left_subtask_id,
                    held_by: left_held_by,
                },
                SubtaskAlreadyClaimed {
                    subtask_id: right_subtask_id,
                    held_by: right_held_by,
                },
            ) => left_subtask_id == right_subtask_id && left_held_by == right_held_by,
            (
                SessionAlreadyActive {
                    agent_principal_id: left_principal_id,
                },
                SessionAlreadyActive {
                    agent_principal_id: right_principal_id,
                },
            ) => left_principal_id == right_principal_id,
            (
                SessionNotActive {
                    session_token: left_session_token,
                    state: left_state,
                },
                SessionNotActive {
                    session_token: right_session_token,
                    state: right_state,
                },
            ) => left_session_token == right_session_token && left_state == right_state,
            (
                SessionAlreadyHasActiveSubtask {
                    session_token: left_session_token,
                    active_subtask_id: left_subtask_id,
                },
                SessionAlreadyHasActiveSubtask {
                    session_token: right_session_token,
                    active_subtask_id: right_subtask_id,
                },
            ) => left_session_token == right_session_token && left_subtask_id == right_subtask_id,
            (
                RuntimeAttestationMissing {
                    session_token: left_session_token,
                },
                RuntimeAttestationMissing {
                    session_token: right_session_token,
                },
            ) => left_session_token == right_session_token,
            (
                InvalidRuntimeAttestation {
                    session_token: left_session_token,
                    reason: left_reason,
                },
                InvalidRuntimeAttestation {
                    session_token: right_session_token,
                    reason: right_reason,
                },
            ) => left_session_token == right_session_token && left_reason == right_reason,
            (
                InvalidSessionToken {
                    session_token: left_token,
                },
                InvalidSessionToken {
                    session_token: right_token,
                },
            ) => left_token == right_token,
            (
                InvalidIdempotencyKey {
                    idempotency_key: left_key,
                },
                InvalidIdempotencyKey {
                    idempotency_key: right_key,
                },
            ) => left_key == right_key,
            (
                IdempotencyConflict {
                    actor_key: left_actor_key,
                    operation: left_operation,
                    idempotency_key: left_key,
                },
                IdempotencyConflict {
                    actor_key: right_actor_key,
                    operation: right_operation,
                    idempotency_key: right_key,
                },
            ) => {
                left_actor_key == right_actor_key
                    && left_operation == right_operation
                    && left_key == right_key
            }
            (
                StaleFenceToken {
                    expected: left_expected,
                    provided: left_provided,
                },
                StaleFenceToken {
                    expected: right_expected,
                    provided: right_provided,
                },
            ) => left_expected == right_expected && left_provided == right_provided,
            (
                ClaimNotHeld {
                    claim_id: left_claim_id,
                    state: left_state,
                },
                ClaimNotHeld {
                    claim_id: right_claim_id,
                    state: right_state,
                },
            ) => left_claim_id == right_claim_id && left_state == right_state,
            (
                NotClaimOwner {
                    session_token: left_session_token,
                    claim_owner: left_claim_owner,
                },
                NotClaimOwner {
                    session_token: right_session_token,
                    claim_owner: right_claim_owner,
                },
            ) => left_session_token == right_session_token && left_claim_owner == right_claim_owner,
            (
                NotQueueClaimOwner {
                    session_token: left_session_token,
                    queue_owner: left_queue_owner,
                },
                NotQueueClaimOwner {
                    session_token: right_session_token,
                    queue_owner: right_queue_owner,
                },
            ) => left_session_token == right_session_token && left_queue_owner == right_queue_owner,
            (
                WrongRole {
                    expected: left_expected,
                    actual: left_actual,
                },
                WrongRole {
                    expected: right_expected,
                    actual: right_actual,
                },
            ) => left_expected == right_expected && left_actual == right_actual,
            (
                ArtifactDigestCollision {
                    digest: left_digest,
                },
                ArtifactDigestCollision {
                    digest: right_digest,
                },
            )
            | (
                UnknownArtifactDigest {
                    digest: left_digest,
                },
                UnknownArtifactDigest {
                    digest: right_digest,
                },
            ) => left_digest == right_digest,
            (
                DuplicateSubtaskId {
                    subtask_id: left_subtask_id,
                },
                DuplicateSubtaskId {
                    subtask_id: right_subtask_id,
                },
            ) => left_subtask_id == right_subtask_id,
            (
                CompletionPolicyViolation {
                    operation: left_operation,
                    policy: left_policy,
                },
                CompletionPolicyViolation {
                    operation: right_operation,
                    policy: right_policy,
                },
            ) => left_operation == right_operation && left_policy == right_policy,
            (
                ReviewAlreadyOpen {
                    subtask_id: left_subtask_id,
                    artifact_digest: left_artifact_digest,
                },
                ReviewAlreadyOpen {
                    subtask_id: right_subtask_id,
                    artifact_digest: right_artifact_digest,
                },
            ) => {
                left_subtask_id == right_subtask_id && left_artifact_digest == right_artifact_digest
            }
            (
                StaleReviewArtifact {
                    review_id: left_review_id,
                    subtask_id: left_subtask_id,
                    artifact_digest: left_artifact_digest,
                    current_artifact_digest: left_current_artifact_digest,
                },
                StaleReviewArtifact {
                    review_id: right_review_id,
                    subtask_id: right_subtask_id,
                    artifact_digest: right_artifact_digest,
                    current_artifact_digest: right_current_artifact_digest,
                },
            ) => {
                left_review_id == right_review_id
                    && left_subtask_id == right_subtask_id
                    && left_artifact_digest == right_artifact_digest
                    && left_current_artifact_digest == right_current_artifact_digest
            }
            (
                SeparationOfDutiesViolation {
                    reviewer_principal_id: left_reviewer,
                    producer_principal_id: left_producer,
                },
                SeparationOfDutiesViolation {
                    reviewer_principal_id: right_reviewer,
                    producer_principal_id: right_producer,
                },
            ) => left_reviewer == right_reviewer && left_producer == right_producer,
            (
                ApplyGateEvidenceMissing {
                    queue_id: left_queue_id,
                    reason: left_reason,
                },
                ApplyGateEvidenceMissing {
                    queue_id: right_queue_id,
                    reason: right_reason,
                },
            ) => left_queue_id == right_queue_id && left_reason == right_reason,
            (
                ApplyGateSeparationOfDutiesViolation {
                    apply_gate_principal_id: left_apply_gate,
                    conflicting_role: left_role,
                    conflicting_principal_id: left_conflicting,
                },
                ApplyGateSeparationOfDutiesViolation {
                    apply_gate_principal_id: right_apply_gate,
                    conflicting_role: right_role,
                    conflicting_principal_id: right_conflicting,
                },
            ) => {
                left_apply_gate == right_apply_gate
                    && left_role == right_role
                    && left_conflicting == right_conflicting
            }
            (
                MetaTaskUnavailable {
                    meta_task_id: left_meta_task_id,
                    state: left_state,
                },
                MetaTaskUnavailable {
                    meta_task_id: right_meta_task_id,
                    state: right_state,
                },
            ) => left_meta_task_id == right_meta_task_id && left_state == right_state,
            (
                LeaseExpired {
                    object_id: left_object_id,
                },
                LeaseExpired {
                    object_id: right_object_id,
                },
            ) => left_object_id == right_object_id,
            (
                InvalidLeaseDuration {
                    field: left_field,
                    provided: left_provided,
                },
                InvalidLeaseDuration {
                    field: right_field,
                    provided: right_provided,
                },
            ) => left_field == right_field && left_provided == right_provided,
            (InvalidPath { path: left_path }, InvalidPath { path: right_path }) => {
                left_path == right_path
            }
            (
                InputTooLarge {
                    field: left_field,
                    actual: left_actual,
                    max: left_max,
                },
                InputTooLarge {
                    field: right_field,
                    actual: right_actual,
                    max: right_max,
                },
            ) => left_field == right_field && left_actual == right_actual && left_max == right_max,
            (
                ImportSourceNotFound { path: left_path },
                ImportSourceNotFound { path: right_path },
            ) => left_path == right_path,
            (
                InvalidSourceSchema {
                    path: left_path,
                    detail: left_detail,
                },
                InvalidSourceSchema {
                    path: right_path,
                    detail: right_detail,
                },
            ) => left_path == right_path && left_detail == right_detail,
            (
                InvalidImportDestination {
                    reason: left_reason,
                },
                InvalidImportDestination {
                    reason: right_reason,
                },
            ) => left_reason == right_reason,
            (
                InvalidImportRow {
                    source_issue_id: left_id,
                    reason: left_reason,
                },
                InvalidImportRow {
                    source_issue_id: right_id,
                    reason: right_reason,
                },
            ) => left_id == right_id && left_reason == right_reason,
            (
                InvalidEventShape {
                    reason: left_reason,
                },
                InvalidEventShape {
                    reason: right_reason,
                },
            )
            | (
                InvalidReadyQueueMetrics {
                    reason: left_reason,
                },
                InvalidReadyQueueMetrics {
                    reason: right_reason,
                },
            )
            | (
                InvalidObservabilityRow {
                    reason: left_reason,
                },
                InvalidObservabilityRow {
                    reason: right_reason,
                },
            ) => left_reason == right_reason,
            (
                CurrentWorkBlockerNotFound {
                    blocker_id: left_id,
                },
                CurrentWorkBlockerNotFound {
                    blocker_id: right_id,
                },
            )
            | (
                AmbiguousCurrentWorkBlocker {
                    blocker_id: left_id,
                },
                AmbiguousCurrentWorkBlocker {
                    blocker_id: right_id,
                },
            ) => left_id == right_id,
            (
                ImportDuplicate {
                    source_issue_id: left_id,
                    subtask_id: left_subtask_id,
                },
                ImportDuplicate {
                    source_issue_id: right_id,
                    subtask_id: right_subtask_id,
                },
            ) => left_id == right_id && left_subtask_id == right_subtask_id,
            (DatabaseError(left), DatabaseError(right)) => left.to_string() == right.to_string(),
            (MigrationError(left), MigrationError(right)) => left.to_string() == right.to_string(),
            (SerializationError(left), SerializationError(right)) => {
                left.to_string() == right.to_string()
            }
            (TypeValidationError(left), TypeValidationError(right)) => left == right,
            _ => false,
        }
    }
}

impl Eq for CoveyError {}

/// Convenience result alias for Covey operations.
pub type Result<T> = std::result::Result<T, CoveyError>;

#[cfg(test)]
mod tests {
    use super::CoveyError;
    use crate::model::{
        ArtifactDigest, ClaimId, ClaimState, MetaTaskState, ObjectType, QueueId, SessionRole,
        SessionState, SessionToken, StateValue, SubtaskId, SubtaskState,
    };
    use rstest::rstest;

    #[test]
    fn partial_eq_matches_structured_error_payloads() {
        let left = CoveyError::IllegalTransition {
            from: StateValue::Subtask(SubtaskState::Claimed),
            to: StateValue::Subtask(SubtaskState::InProgress),
            object: ObjectType::Subtask,
        };
        let right = CoveyError::IllegalTransition {
            from: StateValue::Subtask(SubtaskState::Claimed),
            to: StateValue::Subtask(SubtaskState::InProgress),
            object: ObjectType::Subtask,
        };

        assert_eq!(left, right);
    }

    #[test]
    fn partial_eq_distinguishes_structured_error_differences() {
        let left = CoveyError::SessionNotActive {
            session_token: session_token("session-1"),
            state: SessionState::Exited,
        };
        let right = CoveyError::SessionNotActive {
            session_token: session_token("session-1"),
            state: SessionState::Stale,
        };

        assert_ne!(left, right);
    }

    #[test]
    fn partial_eq_compares_serialization_errors_by_message() {
        let left = CoveyError::SerializationError(
            serde_json::from_str::<u32>("not-json").expect_err("input must fail to parse"),
        );
        let right = CoveyError::SerializationError(
            serde_json::from_str::<u32>("not-json").expect_err("input must fail to parse"),
        );
        let different = CoveyError::SerializationError(
            serde_json::from_str::<u32>("[]").expect_err("shape mismatch must fail to parse"),
        );

        assert_eq!(left, right);
        assert_ne!(left, different);
    }

    #[rstest]
    #[case::session_not_found(
        CoveyError::SessionNotFound,
        CoveyError::SessionNotFound,
        CoveyError::SubtaskNotFound
    )]
    #[case::artifact_not_found(
        CoveyError::ArtifactNotFound,
        CoveyError::ArtifactNotFound,
        CoveyError::ReviewNotFound
    )]
    #[case::meta_task_not_found(
        CoveyError::MetaTaskNotFound,
        CoveyError::MetaTaskNotFound,
        CoveyError::ReservationNotFound
    )]
    #[case::claim_not_found(
        CoveyError::ClaimNotFound,
        CoveyError::ClaimNotFound,
        CoveyError::QueueItemNotFound
    )]
    #[case::conflict_not_found(
        CoveyError::ConflictNotFound,
        CoveyError::ConflictNotFound,
        CoveyError::FenceTokenMismatch
    )]
    #[case::review_kind_mismatch(
        CoveyError::ReviewKindMismatch,
        CoveyError::ReviewKindMismatch,
        CoveyError::SessionNotFound
    )]
    #[case::subtask_claimed(
        CoveyError::SubtaskAlreadyClaimed { subtask_id: subtask_id("subtask-1"), held_by: session_token("session-1") },
        CoveyError::SubtaskAlreadyClaimed { subtask_id: subtask_id("subtask-1"), held_by: session_token("session-1") },
        CoveyError::SubtaskAlreadyClaimed { subtask_id: subtask_id("subtask-2"), held_by: session_token("session-1") }
    )]
    #[case::session_already_active(
        CoveyError::SessionAlreadyActive { agent_principal_id: "agent-1".into() },
        CoveyError::SessionAlreadyActive { agent_principal_id: "agent-1".into() },
        CoveyError::SessionAlreadyActive { agent_principal_id: "agent-2".into() }
    )]
    #[case::session_has_active_subtask(
        CoveyError::SessionAlreadyHasActiveSubtask { session_token: session_token("session-1"), active_subtask_id: subtask_id("subtask-1") },
        CoveyError::SessionAlreadyHasActiveSubtask { session_token: session_token("session-1"), active_subtask_id: subtask_id("subtask-1") },
        CoveyError::SessionAlreadyHasActiveSubtask { session_token: session_token("session-1"), active_subtask_id: subtask_id("subtask-2") }
    )]
    #[case::invalid_session_token(
        CoveyError::InvalidSessionToken { session_token: "session-1".into() },
        CoveyError::InvalidSessionToken { session_token: "session-1".into() },
        CoveyError::InvalidSessionToken { session_token: "session-2".into() }
    )]
    #[case::invalid_idempotency_key(
        CoveyError::InvalidIdempotencyKey { idempotency_key: "idem-1".into() },
        CoveyError::InvalidIdempotencyKey { idempotency_key: "idem-1".into() },
        CoveyError::InvalidIdempotencyKey { idempotency_key: "idem-2".into() }
    )]
    #[case::idempotency_conflict(
        CoveyError::IdempotencyConflict { actor_key: "actor-1".into(), operation: "claim".into(), idempotency_key: "idem-1".into() },
        CoveyError::IdempotencyConflict { actor_key: "actor-1".into(), operation: "claim".into(), idempotency_key: "idem-1".into() },
        CoveyError::IdempotencyConflict { actor_key: "actor-2".into(), operation: "claim".into(), idempotency_key: "idem-1".into() }
    )]
    #[case::stale_fence_token(
        CoveyError::StaleFenceToken { expected: 2, provided: 1 },
        CoveyError::StaleFenceToken { expected: 2, provided: 1 },
        CoveyError::StaleFenceToken { expected: 3, provided: 1 }
    )]
    #[case::claim_not_held(
        CoveyError::ClaimNotHeld { claim_id: claim_id("claim-1"), state: ClaimState::Released },
        CoveyError::ClaimNotHeld { claim_id: claim_id("claim-1"), state: ClaimState::Released },
        CoveyError::ClaimNotHeld { claim_id: claim_id("claim-1"), state: ClaimState::Expired }
    )]
    #[case::not_claim_owner(
        CoveyError::NotClaimOwner { session_token: session_token("session-1"), claim_owner: session_token("session-2") },
        CoveyError::NotClaimOwner { session_token: session_token("session-1"), claim_owner: session_token("session-2") },
        CoveyError::NotClaimOwner { session_token: session_token("session-1"), claim_owner: session_token("session-3") }
    )]
    #[case::not_queue_claim_owner(
        CoveyError::NotQueueClaimOwner { session_token: session_token("session-1"), queue_owner: session_token("session-2") },
        CoveyError::NotQueueClaimOwner { session_token: session_token("session-1"), queue_owner: session_token("session-2") },
        CoveyError::NotQueueClaimOwner { session_token: session_token("session-1"), queue_owner: session_token("session-3") }
    )]
    #[case::wrong_role(
        CoveyError::WrongRole { expected: vec![SessionRole::Executor, SessionRole::Reviewer], actual: SessionRole::Orchestrator },
        CoveyError::WrongRole { expected: vec![SessionRole::Executor, SessionRole::Reviewer], actual: SessionRole::Orchestrator },
        CoveyError::WrongRole { expected: vec![SessionRole::Executor], actual: SessionRole::Orchestrator }
    )]
    #[case::artifact_digest_collision(
        CoveyError::ArtifactDigestCollision { digest: "digest-1".into() },
        CoveyError::ArtifactDigestCollision { digest: "digest-1".into() },
        CoveyError::ArtifactDigestCollision { digest: "digest-2".into() }
    )]
    #[case::unknown_artifact_digest(
        CoveyError::UnknownArtifactDigest { digest: "digest-1".into() },
        CoveyError::UnknownArtifactDigest { digest: "digest-1".into() },
        CoveyError::UnknownArtifactDigest { digest: "digest-2".into() }
    )]
    #[case::duplicate_subtask_id(
        CoveyError::DuplicateSubtaskId { subtask_id: subtask_id("subtask-1") },
        CoveyError::DuplicateSubtaskId { subtask_id: subtask_id("subtask-1") },
        CoveyError::DuplicateSubtaskId { subtask_id: subtask_id("subtask-2") }
    )]
    #[case::review_already_open(
        CoveyError::ReviewAlreadyOpen { subtask_id: subtask_id("subtask-1"), artifact_digest: artifact_digest("blake3:digest1") },
        CoveyError::ReviewAlreadyOpen { subtask_id: subtask_id("subtask-1"), artifact_digest: artifact_digest("blake3:digest1") },
        CoveyError::ReviewAlreadyOpen { subtask_id: subtask_id("subtask-1"), artifact_digest: artifact_digest("blake3:digest2") }
    )]
    #[case::separation_of_duties(
        CoveyError::SeparationOfDutiesViolation { reviewer_principal_id: "principal-1".into(), producer_principal_id: "principal-1".into() },
        CoveyError::SeparationOfDutiesViolation { reviewer_principal_id: "principal-1".into(), producer_principal_id: "principal-1".into() },
        CoveyError::SeparationOfDutiesViolation { reviewer_principal_id: "principal-1".into(), producer_principal_id: "principal-2".into() }
    )]
    #[case::apply_gate_evidence_missing(
        CoveyError::ApplyGateEvidenceMissing { queue_id: queue_id("queue-1"), reason: "missing verification".into() },
        CoveyError::ApplyGateEvidenceMissing { queue_id: queue_id("queue-1"), reason: "missing verification".into() },
        CoveyError::ApplyGateEvidenceMissing { queue_id: queue_id("queue-2"), reason: "missing verification".into() }
    )]
    #[case::meta_task_unavailable(
        CoveyError::MetaTaskUnavailable { meta_task_id: "meta-1".into(), state: MetaTaskState::Cancelled },
        CoveyError::MetaTaskUnavailable { meta_task_id: "meta-1".into(), state: MetaTaskState::Cancelled },
        CoveyError::MetaTaskUnavailable { meta_task_id: "meta-1".into(), state: MetaTaskState::Completed }
    )]
    #[case::lease_expired(
        CoveyError::LeaseExpired { object_id: "claim-1".into() },
        CoveyError::LeaseExpired { object_id: "claim-1".into() },
        CoveyError::LeaseExpired { object_id: "claim-2".into() }
    )]
    #[case::invalid_lease_duration(
        CoveyError::InvalidLeaseDuration { field: "lease_duration_ms".into(), provided: 0 },
        CoveyError::InvalidLeaseDuration { field: "lease_duration_ms".into(), provided: 0 },
        CoveyError::InvalidLeaseDuration { field: "lease_duration_ms".into(), provided: -1 }
    )]
    #[case::invalid_path(
        CoveyError::InvalidPath { path: "../escape".into() },
        CoveyError::InvalidPath { path: "../escape".into() },
        CoveyError::InvalidPath { path: "safe".into() }
    )]
    #[case::input_too_large(
        CoveyError::InputTooLarge { field: "title".into(), actual: 257, max: 256 },
        CoveyError::InputTooLarge { field: "title".into(), actual: 257, max: 256 },
        CoveyError::InputTooLarge { field: "title".into(), actual: 258, max: 256 }
    )]
    #[case::import_source_not_found(
        CoveyError::ImportSourceNotFound { path: "missing.db".into() },
        CoveyError::ImportSourceNotFound { path: "missing.db".into() },
        CoveyError::ImportSourceNotFound { path: "other.db".into() }
    )]
    #[case::invalid_source_schema(
        CoveyError::InvalidSourceSchema { path: "source.db".into(), detail: "missing table".into() },
        CoveyError::InvalidSourceSchema { path: "source.db".into(), detail: "missing table".into() },
        CoveyError::InvalidSourceSchema { path: "source.db".into(), detail: "bad column".into() }
    )]
    #[case::invalid_import_destination(
        CoveyError::InvalidImportDestination { reason: "ambiguous".into() },
        CoveyError::InvalidImportDestination { reason: "ambiguous".into() },
        CoveyError::InvalidImportDestination { reason: "missing".into() }
    )]
    #[case::invalid_import_row(
        CoveyError::InvalidImportRow { source_issue_id: "42".into(), reason: "empty title".into() },
        CoveyError::InvalidImportRow { source_issue_id: "42".into(), reason: "empty title".into() },
        CoveyError::InvalidImportRow { source_issue_id: "43".into(), reason: "empty title".into() }
    )]
    #[case::import_duplicate(
        CoveyError::ImportDuplicate { source_issue_id: "42".into(), subtask_id: subtask_id("subtask-1") },
        CoveyError::ImportDuplicate { source_issue_id: "42".into(), subtask_id: subtask_id("subtask-1") },
        CoveyError::ImportDuplicate { source_issue_id: "42".into(), subtask_id: subtask_id("subtask-2") }
    )]
    fn partial_eq_covers_structured_error_variants(
        #[case] left: CoveyError,
        #[case] same: CoveyError,
        #[case] different: CoveyError,
    ) {
        assert_eq!(left, same);
        assert_ne!(left, different);
    }

    fn claim_id(value: &str) -> ClaimId {
        ClaimId::parse(value).expect("test claim id must be valid")
    }

    fn session_token(value: &str) -> SessionToken {
        SessionToken::parse(value).expect("test session token must be valid")
    }

    fn queue_id(value: &str) -> QueueId {
        QueueId::parse(value).expect("test queue id must be valid")
    }

    fn subtask_id(value: &str) -> SubtaskId {
        SubtaskId::parse(value).expect("test subtask id must be valid")
    }

    fn artifact_digest(value: &str) -> ArtifactDigest {
        ArtifactDigest::parse(value).expect("test artifact digest must be valid")
    }
}

use crate::model::{ClaimState, MetaTaskState, ObjectType, SessionRole, SessionState, StateValue};
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
    SubtaskAlreadyClaimed { subtask_id: String, held_by: String },
    #[error("session already active for principal {agent_principal_id}")]
    SessionAlreadyActive { agent_principal_id: String },
    #[error("session {session_token} is not active; current state is {state}")]
    SessionNotActive {
        session_token: String,
        state: SessionState,
    },
    #[error("session {session_token} already has active subtask {active_subtask_id}")]
    SessionAlreadyHasActiveSubtask {
        session_token: String,
        active_subtask_id: String,
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
    ClaimNotHeld { claim_id: String, state: ClaimState },

    #[error("session {session_token} does not own the claim; owner is {claim_owner}")]
    NotClaimOwner {
        session_token: String,
        claim_owner: String,
    },
    #[error("session {session_token} does not own the ready-queue claim; owner is {queue_owner}")]
    NotQueueClaimOwner {
        session_token: String,
        queue_owner: String,
    },
    #[error("wrong role: expected one of {expected:?}, actual {actual}")]
    WrongRole {
        expected: Vec<SessionRole>,
        actual: SessionRole,
    },

    #[error("artifact digest collision for {digest}")]
    ArtifactDigestCollision { digest: String },
    #[error("duplicate subtask id {subtask_id}")]
    DuplicateSubtaskId { subtask_id: String },

    #[error("lease expired for {object_id}")]
    LeaseExpired { object_id: String },

    #[error("review kind mismatch")]
    ReviewKindMismatch,
    #[error("review already open for subtask {subtask_id} and artifact {artifact_digest}")]
    ReviewAlreadyOpen {
        subtask_id: String,
        artifact_digest: String,
    },
    #[error(
        "review separation of duties violated: reviewer principal {reviewer_principal_id} matches producer principal {producer_principal_id}"
    )]
    SeparationOfDutiesViolation {
        reviewer_principal_id: String,
        producer_principal_id: String,
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
    #[error(
        "import duplicate for source issue {source_issue_id}: subtask {subtask_id} already exists"
    )]
    ImportDuplicate {
        source_issue_id: String,
        subtask_id: String,
    },

    #[error(transparent)]
    DatabaseError(#[from] rusqlite::Error),
    #[error(transparent)]
    MigrationError(#[from] rusqlite_migration::Error),
    #[error(transparent)]
    SerializationError(#[from] serde_json::Error),
}

impl PartialEq for CoveyError {
    fn eq(&self, other: &Self) -> bool {
        use CoveyError::{
            ArtifactDigestCollision, ArtifactNotFound, ClaimNotFound, ClaimNotHeld,
            ConflictNotFound, DatabaseError, DuplicateSubtaskId, FenceTokenMismatch,
            IdempotencyConflict, IllegalTransition, ImportDuplicate, ImportSourceNotFound,
            InputTooLarge, InvalidIdempotencyKey, InvalidImportDestination, InvalidImportRow,
            InvalidLeaseDuration, InvalidPath, InvalidSessionToken, InvalidSourceSchema,
            LeaseExpired, MetaTaskNotFound, MetaTaskUnavailable, MigrationError, NotClaimOwner,
            NotQueueClaimOwner, QueueItemNotFound, ReservationNotFound, ReviewAlreadyOpen,
            ReviewKindMismatch, ReviewNotFound, SeparationOfDutiesViolation, SerializationError,
            SessionAlreadyActive, SessionAlreadyHasActiveSubtask, SessionNotActive,
            SessionNotFound, StaleFenceToken, SubtaskAlreadyClaimed, SubtaskNotFound,
            UnknownArtifactDigest, WrongRole,
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
    use crate::model::{ObjectType, SessionState, StateValue, SubtaskState};

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
            session_token: "session-1".to_owned(),
            state: SessionState::Exited,
        };
        let right = CoveyError::SessionNotActive {
            session_token: "session-1".to_owned(),
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
}

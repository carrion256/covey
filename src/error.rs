use crate::model::{
    ArtifactDigest, ClaimId, ClaimState, CompletionPolicy, CoveyTypeValidationError, MetaTaskState,
    ObjectType, QueueId, SessionRole, SessionState, SessionToken, StateValue, SubtaskId,
    SubtaskState,
};
use thiserror::Error;

/// Errors returned by the Covey coordination substrate.
///
/// `Into<&'static str>` yields the snake_case variant name: the stable reason
/// a caller can branch on.
#[derive(Debug, Error, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
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

    #[error("covey store not found at {path}")]
    StoreNotFound { path: String },
    #[error("covey store is open read-only; the operation writes")]
    StoreReadOnly,
    #[error("covey store schema version {found} is behind; version {expected} is pending")]
    StoreMigrationRequired { found: usize, expected: usize },
    /// Another writer held the store for the whole wait. Nothing was written,
    /// so the same call is safe to retry.
    #[error("covey store is busy: another writer holds it")]
    StoreBusy,

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
    #[error("session already active for principal {agent_principal_id} in role {role}")]
    SessionAlreadyActive {
        agent_principal_id: String,
        role: SessionRole,
    },
    #[error(
        "principal {agent_principal_id} has no active session{}",
        .role.map(|role| format!(" in role {role}")).unwrap_or_default()
    )]
    NoActiveSession {
        agent_principal_id: String,
        role: Option<SessionRole>,
    },
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
    #[error("dependency subtask {depends_on_subtask_id} not found for subtask {subtask_id}")]
    SubtaskDependencyNotFound {
        subtask_id: SubtaskId,
        depends_on_subtask_id: SubtaskId,
    },
    /// The dependency ended without succeeding and nothing can stand in for
    /// it, so a subtask that waits on it would never become claimable.
    #[error("dependency subtask {depends_on_subtask_id} is {state} and can never be satisfied")]
    SubtaskDependencyUnsatisfiable {
        depends_on_subtask_id: SubtaskId,
        state: SubtaskState,
    },
    #[error("invalid subtask dependency: {reason}")]
    InvalidSubtaskDependency { reason: String },

    #[error("operation {operation} is not allowed for completion policy {policy}")]
    CompletionPolicyViolation {
        operation: String,
        policy: CompletionPolicy,
    },
    #[error(
        "operation {operation} requires attempt evidence for subtask {subtask_id} \
         (evidence policy is required)"
    )]
    AttemptEvidenceRequired {
        operation: String,
        subtask_id: String,
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
    #[error("meta-task {meta_task_id} already has a source link")]
    SourceLinkExists { meta_task_id: String },
    #[error("meta-task handle #{handle} not found")]
    MetaTaskHandleNotFound { handle: u64 },
    #[error("meta-task {meta_task_id} still has {open_subtasks} open subtasks")]
    MetaTaskHasOpenSubtasks {
        meta_task_id: String,
        open_subtasks: usize,
    },
    #[error("meta-task {meta_task_id} is already superseded by {superseded_by}")]
    MetaTaskAlreadySuperseded {
        meta_task_id: String,
        superseded_by: String,
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
    DatabaseError(rusqlite::Error),
    #[error(transparent)]
    MigrationError(#[from] rusqlite_migration::Error),
    #[error(transparent)]
    SerializationError(#[from] serde_json::Error),
    #[error(transparent)]
    TypeValidationError(#[from] CoveyTypeValidationError),
}

impl From<rusqlite::Error> for CoveyError {
    /// SQLite reports lock contention as `SQLITE_BUSY` or `SQLITE_LOCKED`.
    /// Both mean "retry later", so they become [`CoveyError::StoreBusy`].
    fn from(error: rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
                Self::StoreBusy
            }
            _ => Self::DatabaseError(error),
        }
    }
}

/// Convenience result alias for Covey operations.
pub type Result<T> = std::result::Result<T, CoveyError>;

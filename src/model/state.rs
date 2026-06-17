use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Top-level database object kinds addressed by events and conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ObjectType {
    MetaTask,
    Subtask,
    Claim,
    Artifact,
    Review,
    ReadyQueue,
    Session,
    RuntimeAttestation,
    Reservation,
    Conflict,
}

pub(crate) const fn object_type_name(object_type: ObjectType) -> &'static str {
    match object_type {
        ObjectType::MetaTask => "meta_task",
        ObjectType::Subtask => "subtask",
        ObjectType::Claim => "claim",
        ObjectType::Artifact => "artifact",
        ObjectType::Review => "review",
        ObjectType::ReadyQueue => "ready_queue",
        ObjectType::Session => "session",
        ObjectType::RuntimeAttestation => "runtime_attestation",
        ObjectType::Reservation => "reservation",
        ObjectType::Conflict => "conflict",
    }
}

pub(crate) const fn claim_state_name(state: ClaimState) -> &'static str {
    match state {
        ClaimState::Held => "held",
        ClaimState::Released => "released",
        ClaimState::Expired => "expired",
        ClaimState::Revoked => "revoked",
    }
}

pub(crate) const fn session_state_name(state: SessionState) -> &'static str {
    match state {
        SessionState::Active => "active",
        SessionState::Stale => "stale",
        SessionState::Exited => "exited",
    }
}

pub(crate) const fn review_state_name(state: ReviewState) -> &'static str {
    match state {
        ReviewState::Requested => "requested",
        ReviewState::InProgress => "in_progress",
        ReviewState::Decided => "decided",
        ReviewState::Superseded => "superseded",
    }
}

pub(crate) const fn subtask_state_name(state: SubtaskState) -> &'static str {
    match state {
        SubtaskState::Available => "available",
        SubtaskState::Blocked => "blocked",
        SubtaskState::Claimed => "claimed",
        SubtaskState::InProgress => "in_progress",
        SubtaskState::ArtifactPublished => "artifact_published",
        SubtaskState::ReviewPending => "review_pending",
        SubtaskState::ChangesRequested => "changes_requested",
        SubtaskState::Approved => "approved",
        SubtaskState::Decided => "decided",
        SubtaskState::ReadyForApply => "ready_for_apply",
        SubtaskState::Applied => "applied",
        SubtaskState::Abandoned => "abandoned",
    }
}

pub(crate) const fn ready_queue_state_name(state: ReadyQueueState) -> &'static str {
    match state {
        ReadyQueueState::Queued => "queued",
        ReadyQueueState::InFlight => "in_flight",
        ReadyQueueState::Applied => "applied",
        ReadyQueueState::Superseded => "superseded",
        ReadyQueueState::Cancelled => "cancelled",
    }
}

pub(crate) const fn meta_task_state_name(state: MetaTaskState) -> &'static str {
    match state {
        MetaTaskState::Planning => "planning",
        MetaTaskState::Active => "active",
        MetaTaskState::Completed => "completed",
        MetaTaskState::Cancelled => "cancelled",
    }
}

pub(crate) const fn subtask_kind_name(kind: SubtaskKind) -> &'static str {
    match kind {
        SubtaskKind::Work => "work",
        SubtaskKind::Review => "review",
    }
}

pub(crate) const fn reservation_state_name(state: ReservationState) -> &'static str {
    match state {
        ReservationState::Active => "active",
        ReservationState::Released => "released",
        ReservationState::Expired => "expired",
    }
}

pub(crate) const fn review_verdict_name(verdict: ReviewVerdict) -> &'static str {
    match verdict {
        ReviewVerdict::Approve => "approve",
        ReviewVerdict::ChangesRequested => "changes_requested",
        ReviewVerdict::Blocked => "blocked",
    }
}

pub(crate) const fn conflict_resolution_state_name(state: ConflictResolutionState) -> &'static str {
    match state {
        ConflictResolutionState::Open => "open",
        ConflictResolutionState::Acknowledged => "acknowledged",
        ConflictResolutionState::Resolved => "resolved",
    }
}

pub(crate) const fn scope_class_name(scope_class: ScopeClass) -> &'static str {
    match scope_class {
        ScopeClass::ExactPath => "exact_path",
        ScopeClass::Subtree => "subtree",
        ScopeClass::RepoGlobal => "repo_global",
        ScopeClass::GeneratedSet => "generated_set",
    }
}

pub(crate) const fn conflict_kind_name(kind: ConflictKind) -> &'static str {
    match kind {
        ConflictKind::ReservationOverlap => "reservation_overlap",
    }
}

/// Mutating event kinds emitted by the append-only event log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum EventType {
    SessionRegistered,
    SessionHeartbeat,
    SessionExited,
    RuntimeAttestationRecorded,
    MetaTaskSubmitted,
    MetaTaskCancelled,
    SubtaskCreated,
    SubtaskClaimed,
    SubtaskStarted,
    SubtaskAbandoned,
    ClaimReleased,
    ClaimRenewed,
    ArtifactPublished,
    ReviewRequested,
    ReviewDecided,
    PermissiveLandingRecorded,
    ReadyQueueEnqueued,
    ReadyQueueInFlight,
    ApplyVerificationRecorded,
    ReadyQueueApplied,
    ReadyQueueSuperseded,
    ReservationRequested,
    ReservationReleased,
    ReservationRenewed,
    ConflictResolved,
    SessionsReaped,
    ClaimsExpired,
    ReservationsExpired,
    OpenSpecImported,
}

/// Actor classes that can append to the event log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ActorKind {
    Session,
    System,
}

/// Session roles recognized by Covey authorization checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SessionRole {
    Executor,
    Orchestrator,
    ApplyGate,
    Reviewer,
}

/// Session lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SessionState {
    Active,
    Stale,
    Exited,
}

/// Meta-task lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum MetaTaskState {
    Planning,
    Active,
    Completed,
    Cancelled,
}

/// Subtask variants supported by Covey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SubtaskKind {
    Work,
    Review,
}

/// Unified subtask states across work and review subtasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SubtaskState {
    Available,
    Blocked,
    Claimed,
    InProgress,
    ArtifactPublished,
    ReviewPending,
    ChangesRequested,
    Approved,
    Decided,
    ReadyForApply,
    Applied,
    Abandoned,
}

/// Claim lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ClaimState {
    Held,
    Released,
    Expired,
    Revoked,
}

/// Immutable artifact bundle kinds tracked by Covey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ArtifactKind {
    PatchBundle,
    IsolatedCommitRef,
    TreeBundle,
    FindingsBundle,
    VerificationBundle,
}

/// Review decisions attached to an exact artifact digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ReviewVerdict {
    Approve,
    ChangesRequested,
    Blocked,
}

/// Failed review decisions that always require follow-up work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum FailedReviewVerdict {
    ChangesRequested,
    Blocked,
}

impl TryFrom<ReviewVerdict> for FailedReviewVerdict {
    type Error = ReviewVerdict;

    fn try_from(value: ReviewVerdict) -> Result<Self, Self::Error> {
        match value {
            ReviewVerdict::Approve => Err(value),
            ReviewVerdict::ChangesRequested => Ok(Self::ChangesRequested),
            ReviewVerdict::Blocked => Ok(Self::Blocked),
        }
    }
}

impl From<FailedReviewVerdict> for ReviewVerdict {
    fn from(value: FailedReviewVerdict) -> Self {
        match value {
            FailedReviewVerdict::ChangesRequested => Self::ChangesRequested,
            FailedReviewVerdict::Blocked => Self::Blocked,
        }
    }
}

/// Review row lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ReviewState {
    Requested,
    InProgress,
    Decided,
    Superseded,
}

/// Reservation scope classes used for overlap detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ScopeClass {
    ExactPath,
    Subtree,
    RepoGlobal,
    GeneratedSet,
}

/// Reservation lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ReservationState {
    Active,
    Released,
    Expired,
}

/// Settlement destinations supported by the ready queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SettlementTarget {
    Canonical,
}

/// Ready-queue lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ReadyQueueState {
    Queued,
    InFlight,
    Applied,
    Superseded,
    Cancelled,
}

/// Conflict resolution states for surfaced operator/orchestrator follow-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ConflictResolutionState {
    Open,
    Acknowledged,
    Resolved,
}

/// Conflict payload kinds surfaced for operator/orchestrator follow-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ConflictKind {
    ReservationOverlap,
}

impl PartialEq<&str> for ConflictKind {
    fn eq(&self, other: &&str) -> bool {
        matches!(
            (self, *other),
            (Self::ReservationOverlap, "reservation_overlap")
        )
    }
}

impl PartialEq<ConflictKind> for &str {
    fn eq(&self, other: &ConflictKind) -> bool {
        matches!(
            (self, other),
            (&"reservation_overlap", ConflictKind::ReservationOverlap)
        )
    }
}

/// Typed view of state-machine labels used in transition errors.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    derive_more::Display,
    derive_more::From,
    Serialize,
    Deserialize,
)]
pub enum StateValue {
    #[display("{_0}")]
    Session(SessionState),
    #[display("{_0}")]
    MetaTask(MetaTaskState),
    #[display("{_0}")]
    Subtask(SubtaskState),
    #[display("{_0}")]
    Claim(ClaimState),
    #[display("{_0}")]
    Review(ReviewState),
    #[display("{_0}")]
    Reservation(ReservationState),
    #[display("{_0}")]
    ReadyQueue(ReadyQueueState),
    #[display("{_0}")]
    ConflictResolution(ConflictResolutionState),
}

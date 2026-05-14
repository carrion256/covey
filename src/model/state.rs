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
}

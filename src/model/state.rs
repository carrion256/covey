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
    OperatorBlocker,
    ApplyGateBlocker,
    SettlementReconcileBlocker,
    ApplyWorktree,
    VcsWorkspace,
    VcsPacketStackEntry,
    VcsPrPublication,
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
        ObjectType::OperatorBlocker => "operator_blocker",
        ObjectType::ApplyGateBlocker => "apply_gate_blocker",
        ObjectType::SettlementReconcileBlocker => "settlement_reconcile_blocker",
        ObjectType::ApplyWorktree => "apply_worktree",
        ObjectType::VcsWorkspace => "vcs_workspace",
        ObjectType::VcsPacketStackEntry => "vcs_packet_stack_entry",
        ObjectType::VcsPrPublication => "vcs_pr_publication",
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
        SubtaskState::Completed => "completed",
        SubtaskState::Failed => "failed",
        SubtaskState::Abandoned => "abandoned",
    }
}

pub(crate) const fn completion_policy_name(policy: CompletionPolicy) -> &'static str {
    match policy {
        CompletionPolicy::Direct => "direct",
        CompletionPolicy::Reviewed => "reviewed",
        CompletionPolicy::CanonicalApply => "canonical_apply",
    }
}

pub(crate) const fn attempt_outcome_kind_name(kind: AttemptOutcomeKind) -> &'static str {
    match kind {
        AttemptOutcomeKind::Succeeded => "succeeded",
        AttemptOutcomeKind::RetryableFailure => "retryable_failure",
        AttemptOutcomeKind::TerminalFailure => "terminal_failure",
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

pub(crate) const fn openspec_archive_status_state_name(
    state: OpenSpecArchiveStatusState,
) -> &'static str {
    match state {
        OpenSpecArchiveStatusState::Blocked => "blocked",
        OpenSpecArchiveStatusState::Archived => "archived",
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
        SubtaskKind::Cleanup => "cleanup",
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
    WorkSubtaskCreated,
    SubtaskClaimed,
    SubtaskStarted,
    SubtaskFinished,
    SubtaskRetried,
    SubtaskFailed,
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
    ApplyGateBlockerRecorded,
    SettlementReconcileBlockerRecorded,
    ReadyQueueApplied,
    OpenSpecArchiveStatusRecorded,
    ReadyQueueSuperseded,
    ReservationRequested,
    ReservationReleased,
    ReservationRenewed,
    ConflictResolved,
    SessionsReaped,
    ClaimsExpired,
    ReservationsExpired,
    OpenSpecImported,
    OperatorBlockerRecorded,
    OperatorBlockerResolved,
    ApplyWorktreeRecorded,
    ApplyWorktreeStateRecorded,
    VcsWorkspaceRecorded,
    VcsWorkspaceObserved,
    VcsPacketStackEntryRecorded,
    VcsPrPublicationRecorded,
    ProseApplyBlockerRecorded,
}

/// Covey-owned lifecycle state for apply worktrees retained as evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ApplyWorktreeState {
    Active,
    Applied,
    Archived,
    RetainedEvidence,
    CleanupAllowed,
}

pub(crate) const fn apply_worktree_state_name(state: ApplyWorktreeState) -> &'static str {
    match state {
        ApplyWorktreeState::Active => "active",
        ApplyWorktreeState::Applied => "applied",
        ApplyWorktreeState::Archived => "archived",
        ApplyWorktreeState::RetainedEvidence => "retained_evidence",
        ApplyWorktreeState::CleanupAllowed => "cleanup_allowed",
    }
}

/// Kind of scheduler-created VCS execution cache recorded by Covey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum VcsWorkspaceKind {
    Packet,
    Claim,
    Apply,
    Execution,
}

pub(crate) const fn vcs_workspace_kind_name(kind: VcsWorkspaceKind) -> &'static str {
    match kind {
        VcsWorkspaceKind::Packet => "packet",
        VcsWorkspaceKind::Claim => "claim",
        VcsWorkspaceKind::Apply => "apply",
        VcsWorkspaceKind::Execution => "execution",
    }
}

/// Lifecycle state for a registered disposable VCS workspace/cache path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum VcsWorkspaceState {
    Active,
    Retained,
    CleanupAllowed,
    Archived,
}

pub(crate) const fn vcs_workspace_state_name(state: VcsWorkspaceState) -> &'static str {
    match state {
        VcsWorkspaceState::Active => "active",
        VcsWorkspaceState::Retained => "retained",
        VcsWorkspaceState::CleanupAllowed => "cleanup_allowed",
        VcsWorkspaceState::Archived => "archived",
    }
}

/// Last observed filesystem/VCS cleanliness for a registered workspace path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum VcsWorkspaceCleanliness {
    Unknown,
    Clean,
    Dirty,
    Missing,
    Stale,
    Unusable,
}

pub(crate) const fn vcs_workspace_cleanliness_name(
    cleanliness: VcsWorkspaceCleanliness,
) -> &'static str {
    match cleanliness {
        VcsWorkspaceCleanliness::Unknown => "unknown",
        VcsWorkspaceCleanliness::Clean => "clean",
        VcsWorkspaceCleanliness::Dirty => "dirty",
        VcsWorkspaceCleanliness::Missing => "missing",
        VcsWorkspaceCleanliness::Stale => "stale",
        VcsWorkspaceCleanliness::Unusable => "unusable",
    }
}

/// Review/projection state for one claim included in an OpenSpec packet stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum VcsPacketStackEntryState {
    Candidate,
    TreeEquivalent,
    Published,
    Superseded,
}

pub(crate) const fn vcs_packet_stack_entry_state_name(
    state: VcsPacketStackEntryState,
) -> &'static str {
    match state {
        VcsPacketStackEntryState::Candidate => "candidate",
        VcsPacketStackEntryState::TreeEquivalent => "tree_equivalent",
        VcsPacketStackEntryState::Published => "published",
        VcsPacketStackEntryState::Superseded => "superseded",
    }
}

/// Unit published to GitHub from scheduler-owned VCS projection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum VcsPrPublicationKind {
    Packet,
    Claim,
}

pub(crate) const fn vcs_pr_publication_kind_name(kind: VcsPrPublicationKind) -> &'static str {
    match kind {
        VcsPrPublicationKind::Packet => "packet",
        VcsPrPublicationKind::Claim => "claim",
    }
}

/// Publication status for a scheduler-created PR projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum VcsPrPublicationStatus {
    Prepared,
    Published,
    Blocked,
    Superseded,
}

pub(crate) const fn vcs_pr_publication_status_name(status: VcsPrPublicationStatus) -> &'static str {
    match status {
        VcsPrPublicationStatus::Prepared => "prepared",
        VcsPrPublicationStatus::Published => "published",
        VcsPrPublicationStatus::Blocked => "blocked",
        VcsPrPublicationStatus::Superseded => "superseded",
    }
}

/// Current-work target kind for explicit operator blockers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum OperatorBlockerTargetKind {
    Subtask,
    ReadyQueue,
}

/// Native apply-gate blocker evidence class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ApplyGateBlockerKind {
    AuthorityHold,
    GitApplyUncertainty,
}

pub(crate) const fn apply_gate_blocker_kind_name(kind: ApplyGateBlockerKind) -> &'static str {
    match kind {
        ApplyGateBlockerKind::AuthorityHold => "authority_hold",
        ApplyGateBlockerKind::GitApplyUncertainty => "git_apply_uncertainty",
    }
}

/// Authority settlement reconcile reason recorded against one queue item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SettlementReconcileReason {
    CommitUnknown,
    AuthorityLost,
    StaleFence,
    PartialPrepare,
    PartialFinalize,
    FailedCanonicalApply,
    DuplicateCompletion,
}

pub(crate) const fn settlement_reconcile_reason_name(
    reason: SettlementReconcileReason,
) -> &'static str {
    match reason {
        SettlementReconcileReason::CommitUnknown => "commit_unknown",
        SettlementReconcileReason::AuthorityLost => "authority_lost",
        SettlementReconcileReason::StaleFence => "stale_fence",
        SettlementReconcileReason::PartialPrepare => "partial_prepare",
        SettlementReconcileReason::PartialFinalize => "partial_finalize",
        SettlementReconcileReason::FailedCanonicalApply => "failed_canonical_apply",
        SettlementReconcileReason::DuplicateCompletion => "duplicate_completion",
    }
}

/// Durable operator blocker lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum OperatorBlockerState {
    Open,
    Resolved,
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
    Cleanup,
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
    Completed,
    Failed,
    Abandoned,
}

/// Immutable assurance required before a work subtask is successful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CompletionPolicy {
    Direct,
    Reviewed,
    CanonicalApply,
}

/// Immutable outcome recorded for one fenced execution attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AttemptOutcomeKind {
    Succeeded,
    RetryableFailure,
    TerminalFailure,
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

/// Cleanup status for applied OpenSpec-imported queue items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum OpenSpecArchiveStatusState {
    Blocked,
    Archived,
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
    OpenSpecArchiveStatus(OpenSpecArchiveStatusState),
    #[display("{_0}")]
    ConflictResolution(ConflictResolutionState),
    #[display("{_0}")]
    ApplyWorktree(ApplyWorktreeState),
    #[display("{_0}")]
    VcsWorkspace(VcsWorkspaceState),
}

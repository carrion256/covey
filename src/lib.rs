#![allow(unexpected_cfgs)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Covey is a correctness-critical coordination substrate for agent cohorts.
//!
//! Covey intentionally targets a single-node embedded deployment over one
//! authoritative transactional store.
//! The database is authoritative; `event_log` is an audit trail for subscribers,
//! not an event-sourced recovery surface.

mod clock;
mod error;
mod model;
mod ops;
mod overlap;
pub mod proof_apply;
mod queries;
mod schema;
mod store;
#[cfg(test)]
mod tests;
mod validators;

pub use clock::{Clock, ManualClock, SystemClock};
pub use error::{CoveyError, Result};
pub use model::{
    AbandonSubtaskReq, ActorKind, AgentInstanceId, AgentPrincipalId, AppliedReadyQueueItem,
    ApplyGateBlocker, ApplyGateBlockerEvidenceId, ApplyGateBlockerKind, ApplyGateBlockerReason,
    ApplyQueueReconcileResult, ApplyVerification, ApplyWorktree, ApplyWorktreePath,
    ApplyWorktreeState, Artifact, ArtifactDigest, ArtifactKind, ArtifactManifestPath, BaseRev,
    BeadsDbPath, BeginOpenSpecArchiveCleanupReq, CancelMetaTaskReq, CancelledReadyQueueItem,
    ChangedPathsDigest, ChangesRequestedFollowupReconcileResult, Claim, ClaimId, ClaimNextReq,
    ClaimReadyQueueReq, ClaimResult, ClaimState, ClaimSubtaskReq, ClaimableSubtaskAvailability,
    CleanupSubtask, CommandTranscriptDigest, Conflict, ConflictId, ConflictResolutionState,
    CoveyTypeValidationError, CreateSubtaskRequest, DecideReviewReq, DecidedReview,
    EnqueueForApplyReq, Event, EventObjectId, EventPayload, EventSeq, EventType, ExitSessionReq,
    ExpireResult, ExpiringClaim, FailedReviewVerdict, FenceSeq, FindingsDigest,
    FinishOpenSpecArchiveCleanupReq, HeartbeatReq, IdempotencyKey, ImportBdV1ItemResult,
    ImportBdV1Req, ImportBdV1Result, ImportBdV1SkipReason, ImportOpenSpecAction,
    ImportOpenSpecConflict, ImportOpenSpecConflictReason, ImportOpenSpecEvent,
    ImportOpenSpecItemResult, ImportOpenSpecReq, ImportOpenSpecResult, ImportProseReq,
    ImportProseResult, ImportProseTaskReq, InFlightReadyQueueItem, InProgressReview,
    LandedCommitOid, LandingAuthorizationStatus, LeaseDeadlineMs, LeaseDurationMs, MarkAppliedReq,
    MarkApplyWorktreeStateReq, MarkInFlightReq, MetaTask, MetaTaskId, MetaTaskState,
    MetaTaskStatus, ModelId, ObjectType, OpenSpecArchiveBlockedReason, OpenSpecArchiveCleanupClaim,
    OpenSpecArchiveCleanupFinish, OpenSpecArchiveEligibility, OpenSpecArchiveStatus,
    OpenSpecArchiveStatusState, OpenSpecChangeId, OpenSpecCurrentWork, OpenSpecCurrentWorkBlocker,
    OpenSpecCurrentWorkBlockerKind, OpenSpecCurrentWorkBlockerResolution, OpenSpecCurrentWorkOwner,
    OpenSpecCurrentWorkRepairAction, OpenSpecCurrentWorkRepairPlaybook,
    OpenSpecCurrentWorkRepairSafety, OpenSpecCurrentWorkState, OpenSpecDigest,
    OpenSpecImportProvenance, OpenSpecPath, OpenSpecSourceDigest, OpenSpecTaskId, OperatorBlocker,
    OperatorBlockerEvidenceId, OperatorBlockerId, OperatorBlockerReason, OperatorBlockerState,
    OperatorBlockerTargetKind, OverlapQueryReq, PermissiveLandingReceiptDigest, ProjectRootPath,
    PromptText, ProseApplyBlockerId, ProseCurrentWork, ProseCurrentWorkBlocker,
    ProseCurrentWorkOwner, ProseCurrentWorkState, ProseTasksetId, ProviderId, PublishArtifactReq,
    QueueId, QueuedReadyQueueItem, ReadyQueueActiveClaim, ReadyQueueCandidate, ReadyQueueClaim,
    ReadyQueueCommon, ReadyQueueItem, ReadyQueueMetrics, ReadyQueueState, ReapResult,
    ReconcileApplyQueueReq, ReconcileChangesRequestedFollowupsReq, RecordApplyGateBlockerReq,
    RecordApplyVerificationReq, RecordApplyWorktreeReq, RecordLandingReceiptReq,
    RecordOpenSpecArchiveStatusReq, RecordOperatorBlockerReq, RecordPermissiveLandingReceiptReq,
    RecordProseApplyBlockerReq, RecordRuntimeAttestationReq, RecordSettlementReconcileBlockerReq,
    RegisterSessionReq, ReleaseClaimReq, ReleaseReservationReq, RenewClaimReq, RenewReservationReq,
    RepoopsAuthorityClaimFact, RepoopsAuthorityClaimStatus, RepoopsAuthorityGitContextFact,
    RepoopsAuthorityLockFact, RepoopsAuthorityLockStatus, RepoopsAuthorityPolicyFact,
    RepoopsAuthorityPolicyMode, RepoopsAuthorityScopeFact, RepoopsAuthoritySnapshot,
    RepoopsAuthoritySnapshotReq, RepoopsPath, RequestReservationReq, RequestReviewReq,
    RequestedReview, Reservation, ReservationId, ReservationOverlapConflictPayload,
    ReservationState, ResolveConflictReq, ResolveOperatorBlockerReq, Review, ReviewCommon,
    ReviewDecisionResult, ReviewId, ReviewState, ReviewSubtask, ReviewTarget, ReviewVerdict,
    RuntimeAttestation, ScopeClass, Session, SessionHandle, SessionHeartbeatTick, SessionRole,
    SessionState, SessionStatus, SessionToken, SettlementReconcileBlocker,
    SettlementReconcileEvidenceId, SettlementReconcileReason, SettlementTarget, SourceIssueId,
    StartSubtaskReq, StateValue, StuckSubtask, SubmitMetaTaskReq, Subtask, SubtaskCandidate,
    SubtaskId, SubtaskKind, SubtaskLifecycle, SubtaskPriority, SubtaskState, SubtaskStatus,
    SubtaskTitle, SubtaskView, SupersedeQueueItemReq, SupersededReadyQueueItem, SupersededReview,
    TimestampMs, TypedEvent, VerifierId, VerifyLandingAuthorizationReq, WorkSubtask,
};
pub use store::Covey;

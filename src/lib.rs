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
    AbandonSubtaskReq, ActorKind, AppliedReadyQueueItem, ApplyVerification, Artifact,
    ArtifactDigest, ArtifactKind, BaseRev, CancelMetaTaskReq, CancelledReadyQueueItem,
    ChangedPathsDigest, Claim, ClaimId, ClaimNextReq, ClaimReadyQueueReq, ClaimResult, ClaimState,
    ClaimSubtaskReq, Conflict, ConflictResolutionState, CoveyTypeValidationError,
    CreateSubtaskRequest, DecideReviewReq, DecidedReview, EnqueueForApplyReq, Event, EventPayload,
    EventType, ExitSessionReq, ExpireResult, ExpiringClaim, FenceSeq, FindingsDigest, HeartbeatReq,
    ImportBdV1ItemResult, ImportBdV1Req, ImportBdV1Result, ImportBdV1SkipReason,
    ImportOpenSpecAction, ImportOpenSpecConflict, ImportOpenSpecEvent, ImportOpenSpecItemResult,
    ImportOpenSpecReq, ImportOpenSpecResult, InFlightReadyQueueItem, InProgressReview,
    LandingAuthorizationStatus, LeaseDeadlineMs, LeaseDurationMs, MarkAppliedReq, MarkInFlightReq,
    MetaTask, MetaTaskId, MetaTaskState, MetaTaskStatus, ModelId, ObjectType,
    OpenSpecImportProvenance, OpenSpecSourceDigest, OverlapQueryReq, ProviderId,
    PublishArtifactReq, QueueId, QueuedReadyQueueItem, ReadyQueueActiveClaim, ReadyQueueClaim,
    ReadyQueueCommon, ReadyQueueItem, ReadyQueueMetrics, ReadyQueueState, ReapResult,
    RecordApplyVerificationReq, RecordRuntimeAttestationReq, RegisterSessionReq, ReleaseClaimReq,
    ReleaseReservationReq, RenewClaimReq, RenewReservationReq, RepoopsAuthorityClaimFact,
    RepoopsAuthorityClaimStatus, RepoopsAuthorityGitContextFact, RepoopsAuthorityLockFact,
    RepoopsAuthorityLockStatus, RepoopsAuthorityPolicyFact, RepoopsAuthorityPolicyMode,
    RepoopsAuthorityScopeFact, RepoopsAuthoritySnapshot, RepoopsAuthoritySnapshotReq,
    RequestReservationReq, RequestReviewReq, RequestedReview, Reservation, ReservationId,
    ReservationOverlapConflictPayload, ReservationState, ResolveConflictReq, Review, ReviewCommon,
    ReviewId, ReviewState, ReviewSubtask, ReviewTarget, ReviewVerdict, RuntimeAttestation,
    ScopeClass, Session, SessionHandle, SessionRole, SessionState, SessionStatus, SessionToken,
    SettlementTarget, StartSubtaskReq, StateValue, StuckSubtask, SubmitMetaTaskReq, Subtask,
    SubtaskId, SubtaskKind, SubtaskLifecycle, SubtaskState, SubtaskStatus, SubtaskView,
    SupersedeQueueItemReq, SupersededReadyQueueItem, SupersededReview, TimestampMs, TypedEvent,
    VerifyLandingAuthorizationReq, WorkSubtask,
};
pub use store::Covey;

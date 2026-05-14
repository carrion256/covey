#![allow(unexpected_cfgs)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Covey is a correctness-critical coordination substrate for agent cohorts.
//!
//! Covey intentionally targets a single-node embedded deployment over one
//! authoritative transactional store.
//! The database is authoritative; `event_log` is an audit trail for subscribers,
//! not an event-sourced recovery surface.

pub mod better_droid;
mod clock;
mod error;
mod model;
mod ops;
mod overlap;
mod queries;
mod schema;
mod store;
#[cfg(test)]
mod tests;
mod validators;

pub use clock::{Clock, ManualClock, SystemClock};
pub use error::{CoveyError, Result};
pub use model::{
    AbandonSubtaskReq, ActorKind, ApplyVerification, Artifact, ArtifactKind, CancelMetaTaskReq,
    Claim, ClaimNextReq, ClaimReadyQueueReq, ClaimResult, ClaimState, ClaimSubtaskReq, Conflict,
    ConflictResolutionState, CreateSubtaskReq, DecideReviewReq, EnqueueForApplyReq, Event,
    EventPayload, EventType, ExitSessionReq, ExpireResult, ExpiringClaim, HeartbeatReq,
    ImportBdV1ItemResult, ImportBdV1Req, ImportBdV1Result, ImportBdV1SkipReason,
    ImportOpenSpecAction, ImportOpenSpecConflict, ImportOpenSpecEvent, ImportOpenSpecItemResult,
    ImportOpenSpecReq, ImportOpenSpecResult, MarkAppliedReq, MarkInFlightReq, MetaTask,
    MetaTaskState, MetaTaskStatus, ObjectType, OpenSpecImportProvenance, OpenSpecSourceDigest,
    OverlapQueryReq, PublishArtifactReq, ReadyQueueClaim, ReadyQueueItem, ReadyQueueMetrics,
    ReadyQueueState, ReapResult, RecordApplyVerificationReq, RegisterSessionReq, ReleaseClaimReq,
    ReleaseReservationReq, RenewClaimReq, RenewReservationReq, RepoopsAuthorityClaimFact,
    RepoopsAuthorityGitContextFact, RepoopsAuthorityLockFact, RepoopsAuthorityPolicyFact,
    RepoopsAuthorityScopeFact, RepoopsAuthoritySnapshot, RepoopsAuthoritySnapshotReq,
    RequestReservationReq, RequestReviewReq, Reservation, ReservationOverlapConflictPayload,
    ReservationState, ResolveConflictReq, Review, ReviewState, ReviewVerdict, ScopeClass, Session,
    SessionHandle, SessionRole, SessionState, SessionStatus, SettlementTarget, StartSubtaskReq,
    StateValue, StuckSubtask, SubmitMetaTaskReq, Subtask, SubtaskKind, SubtaskState, SubtaskStatus,
    SupersedeQueueItemReq, TypedEvent,
};
pub use store::Covey;

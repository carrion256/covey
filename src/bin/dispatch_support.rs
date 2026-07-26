mod import;
mod meta;
mod queue;
mod repoops;
mod reservation;
mod session;
mod system;
mod workflow;

use clap::ValueEnum;
use covey::{
    AbandonSubtaskReq, ClaimNextReq, ClaimNextRoutedReq, ClaimReadyQueueReq, ClaimSubtaskReq,
    Covey, CreateSubtaskRequest, CreateWorkSubtaskReq, DecideReviewReq, EnqueueForApplyReq,
    ExitSessionReq, FailSubtaskReq, FinishSubtaskReq, HeartbeatReq, ImportBdV1Req,
    ImportOpenSpecReq, MarkAppliedReq, MarkInFlightReq, OpenSpecArchiveStatusState,
    OverlapQueryReq, PublishArtifactReq, ReconcileApplyQueueReq, RecordApplyGateBlockerReq,
    RecordApplyVerificationReq, RecordLandingReceiptReq, RecordOpenSpecArchiveStatusReq,
    RecordPermissiveLandingReceiptReq, RecordRuntimeAttestationReq,
    RecordSettlementReconcileBlockerReq, RegisterSessionReq, ReleaseClaimReq,
    ReleaseReservationReq, RenewClaimReq, RenewReservationReq, RequestReservationReq,
    RequestReviewReq, ResolveConflictReq, RetrySubtaskReq, SettlementTarget, StartSubtaskReq,
    SubmitMetaTaskReq, SubtaskKind, SupersedeQueueItemReq, VerifyLandingAuthorizationReq,
};
use uuid::Uuid;

use crate::{
    cli::{
        ArtifactCommand, ClaimCommand, Commands, ConflictCommand, EventsCommand, ImportCommand,
        MaintCommand, MetaCommand, QueueCommand, ReservationCommand, ReviewCommand,
        ReviewVerdictArg, SessionCommand, SubtaskCommand,
    },
    render_support::{
        ArtifactPublishAck, ClaimFenceAck, ConflictResolutionAck, CoveyImportProductImpactAck,
        CoveyImportReadinessAck, ImportBdV1Ack, ImportOpenSpecAck, MetaTaskAck, MetaTaskRef,
        QueueClaimAck, QueueOpAck, QueueRef, Rendered, ReservationAck, ReservationRef,
        ReviewDecisionAck, ReviewDecisionAckVerdict, ReviewRef, SessionTokenAck, SubtaskRef,
    },
};

pub(crate) fn dispatch(store: &Covey, command: Commands) -> covey::Result<Rendered> {
    match command {
        Commands::Session { command } => session::dispatch_session(store, command),
        Commands::Meta { command } => meta::dispatch_meta(store, command),
        Commands::Subtask { command } => workflow::dispatch_subtask(store, command),
        Commands::Claim { command } => workflow::dispatch_claim(store, command),
        Commands::Artifact { command } => workflow::dispatch_artifact(store, command),
        Commands::Review { command } => workflow::dispatch_review(store, command),
        Commands::Queue { command } => queue::dispatch_queue(store, command),
        Commands::Reservation { command } => reservation::dispatch_reservation(store, command),
        Commands::Repoops { command } => repoops::dispatch_repoops(store, command),
        Commands::Events { command } => system::dispatch_events(store, command),
        Commands::Conflict { command } => system::dispatch_conflict(store, command),
        Commands::Maint { command } => system::dispatch_maint(store, command),
        Commands::Import { command } => import::dispatch_import(store, command),
        Commands::Digest { .. } => unreachable!("digest commands bypass Covey store dispatch"),
        Commands::Proof { .. } => unreachable!("proof commands bypass Covey store dispatch"),
    }
}

fn new_idempotency_key(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

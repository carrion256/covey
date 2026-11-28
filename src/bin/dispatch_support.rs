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
    Covey, CoveyError, CoveyTypeValidationError, CreateWorkSubtaskReq, DecideReviewReq,
    EnqueueForApplyReq, ExitSessionReq, FailSubtaskReq, FinishSubtaskReq, HeartbeatReq,
    ImportBdV1Req, ImportOpenSpecReq, LinkMetaTaskSourceReq, MarkAppliedReq, MarkInFlightReq,
    OpenSpecArchiveStatusState, OverlapQueryReq, PublishArtifactReq, ReconcileApplyQueueReq,
    RecordApplyGateBlockerReq, RecordApplyVerificationReq, RecordLandingReceiptReq,
    RecordOpenSpecArchiveStatusReq, RecordPermissiveLandingReceiptReq, RecordRuntimeAttestationReq,
    RecordSettlementReconcileBlockerReq, RegisterSessionReq, ReleaseClaimReq,
    ReleaseReservationReq, RenewClaimReq, RenewReservationReq, RequestReservationReq,
    RequestReviewReq, ResolveConflictReq, RetireSubtaskReq, RetrySubtaskReq, SessionRole,
    SettlementTarget, StartSubtaskReq, SubmitMetaTaskReq, SubtaskKind, SupersedeQueueItemReq,
    VerifyLandingAuthorizationReq,
};
use uuid::Uuid;

use crate::{
    cli::{
        ArtifactCommand, ClaimCommand, Commands, ConflictCommand, EventsCommand, ImportCommand,
        MaintCommand, MetaCommand, QueueCommand, ReservationCommand, ReviewCommand,
        ReviewVerdictArg, SessionArg, SessionCommand, SubtaskCommand,
    },
    render_support::{
        ArtifactPublishAck, ClaimFenceAck, ConflictResolutionAck, CoveyImportProductImpactAck,
        CoveyImportReadinessAck, ImportBdV1Ack, ImportOpenSpecAck, MetaTaskAck, MetaTaskRef,
        QueueClaimAck, QueueOpAck, QueueRef, Rendered, ReservationAck, ReservationRef,
        ReviewDecisionAck, ReviewDecisionAckVerdict, ReviewRef, SessionTokenAck, SubtaskRef,
    },
};

pub(crate) fn dispatch(store: &Covey, actor: &Actor, command: Commands) -> covey::Result<Rendered> {
    match command {
        Commands::Session { command } => session::dispatch_session(store, actor, command),
        Commands::Meta { command } => meta::dispatch_meta(store, actor, command),
        Commands::Subtask { command } => workflow::dispatch_subtask(store, actor, command),
        Commands::Claim { command } => workflow::dispatch_claim(store, actor, command),
        Commands::Artifact { command } => workflow::dispatch_artifact(store, actor, command),
        Commands::Review { command } => workflow::dispatch_review(store, actor, command),
        Commands::Queue { command } => queue::dispatch_queue(store, actor, command),
        Commands::Reservation { command } => {
            reservation::dispatch_reservation(store, actor, command)
        }
        Commands::Repoops { command } => repoops::dispatch_repoops(store, actor, command),
        Commands::Events { command } => system::dispatch_events(store, command),
        Commands::Conflict { command } => system::dispatch_conflict(store, actor, command),
        Commands::Maint { command } => system::dispatch_maint(store, command),
        Commands::Import { command } => import::dispatch_import(store, actor, command),
        Commands::Digest { .. } => unreachable!("digest commands bypass Covey store dispatch"),
        Commands::Proof { .. } => unreachable!("proof commands bypass Covey store dispatch"),
    }
}

/// Who a command acts as when it names no session token: the principal and
/// role of the global `--as` flags, and whether the store is read-only.
#[derive(Debug, Default)]
pub(crate) struct Actor {
    pub(crate) principal: Option<String>,
    pub(crate) role: Option<SessionRole>,
    pub(crate) read_only: bool,
}

/// What a command needs of its session when the caller names a principal
/// with `--as` instead of a session token.
#[derive(Clone, Copy, Debug)]
enum SessionNeed {
    /// The command starts work: the principal's session in this role is used,
    /// and registered when there is none.
    Ensure(SessionRole),
    /// The command continues work a session already holds: the principal's
    /// session in this role has to exist.
    Existing(SessionRole),
    /// A `session` command: the principal's one active session.
    Own,
}

/// Planning and filing work is done as the orchestrator.
const PLANNING: SessionNeed = SessionNeed::Ensure(SessionRole::Orchestrator);
/// Taking the next piece of work is done as the executor.
const TAKING_WORK: SessionNeed = SessionNeed::Ensure(SessionRole::Executor);
/// A claim on work is held by an executor session.
const HOLDING_WORK: SessionNeed = SessionNeed::Existing(SessionRole::Executor);
/// A claim on a review is held by a reviewer session.
const HOLDING_REVIEW: SessionNeed = SessionNeed::Existing(SessionRole::Reviewer);
/// Taking an apply-queue item is done as the apply gate.
const TAKING_APPLY: SessionNeed = SessionNeed::Ensure(SessionRole::ApplyGate);
/// A claimed apply-queue item is held by an apply-gate session.
const HOLDING_APPLY: SessionNeed = SessionNeed::Existing(SessionRole::ApplyGate);

impl Actor {
    /// True when `--as` names a principal and leaves the role to the command.
    fn uses_command_role(&self) -> bool {
        self.principal.is_some() && self.role.is_none()
    }

    /// The token of the session a command runs as: the one it names, or the
    /// session of the `--as` principal that `need` describes. `--as-role`
    /// replaces the role the command would use. A read-only store is never
    /// written to, so there a session is only looked up.
    fn session_token(
        &self,
        store: &Covey,
        session: SessionArg,
        need: SessionNeed,
    ) -> covey::Result<String> {
        let principal = match (session.session_token, self.principal.as_deref()) {
            (Some(session_token), None) => return Ok(session_token),
            (None, Some(principal)) => principal,
            (Some(_), Some(_)) | (None, None) => {
                return Err(CoveyTypeValidationError::new(
                    "session",
                    "give exactly one of --session-token and --as",
                )
                .into());
            }
        };
        let role = match (self.role, need) {
            (Some(role), _) => role,
            (None, SessionNeed::Ensure(role) | SessionNeed::Existing(role)) => role,
            (None, SessionNeed::Own) => return only_active_session(store, principal),
        };
        if matches!(need, SessionNeed::Ensure(_)) && !self.read_only {
            let ensured = store.ensure_session(RegisterSessionReq::try_from_raw_parts(
                principal,
                new_agent_instance_id(),
                role,
                new_idempotency_key("ensure-session"),
            )?)?;
            return Ok(ensured.session.session_token.to_string());
        }
        store
            .active_session_for_principal(principal, role)?
            .map(|session| session.session_token.to_string())
            .ok_or_else(|| CoveyError::NoActiveSession {
                agent_principal_id: principal.to_owned(),
                role: Some(role),
            })
    }
}

/// The token of the one active session a principal holds, whatever its role.
fn only_active_session(store: &Covey, principal: &str) -> covey::Result<String> {
    let mut sessions = Vec::new();
    for role in [
        SessionRole::Executor,
        SessionRole::Orchestrator,
        SessionRole::ApplyGate,
        SessionRole::Reviewer,
    ] {
        sessions.extend(store.active_session_for_principal(principal, role)?);
    }
    match sessions.as_slice() {
        [session] => Ok(session.session_token.to_string()),
        [] => Err(CoveyError::NoActiveSession {
            agent_principal_id: principal.to_owned(),
            role: None,
        }),
        _ => Err(CoveyTypeValidationError::new(
            "--as",
            "the principal has active sessions in several roles; name one with --as-role",
        )
        .into()),
    }
}

fn new_idempotency_key(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

/// The instance id of a session whose caller named none.
fn new_agent_instance_id() -> String {
    format!("instance-{}", Uuid::new_v4().simple())
}

/// The idempotency key of a call fenced by one claim, used when the caller
/// named none. Running the same command again for the same claim and fence
/// replays the first result instead of running the operation a second time.
fn fenced_idempotency_key(
    operation: &str,
    claim_id: &str,
    fence_seq: impl std::fmt::Display,
) -> String {
    format!("{operation}:{claim_id}:{fence_seq}")
}

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use covey::proof_apply::{ApplyProofBatchArgs, ApplyProofVerifyArgs};

mod import;
mod queue;
mod repoops;
mod reservation;
mod session_meta;
mod system;
mod workflow;

pub(crate) use import::*;
pub(crate) use queue::*;
pub(crate) use repoops::*;
pub(crate) use reservation::*;
pub(crate) use session_meta::*;
pub(crate) use system::*;
pub(crate) use workflow::*;

#[derive(Parser, Debug)]
#[command(
    name = "covey",
    about = "Covey local coordination CLI",
    long_about = None,
    disable_help_subcommand = true
)]
pub(crate) struct Cli {
    /// Store path. Without this flag or `COVEY_DB_PATH`, `./covey.db` is used
    /// only when it already exists; a store is created only at a named path.
    #[arg(long, global = true, env = "COVEY_DB_PATH")]
    pub(crate) db: Option<PathBuf>,

    /// Workflow profiles file (JSON). Profiles select creation completion
    /// policy, routing key, and evidence policy as defaults for new tasks;
    /// omitted profile fields mean the simple flow (direct completion,
    /// default route, optional evidence applies to direct receipts only).
    #[arg(long, global = true)]
    pub(crate) config: Option<PathBuf>,

    #[arg(long, global = true)]
    pub(crate) profile: Option<String>,

    #[arg(long, global = true)]
    pub(crate) json: bool,

    /// Open an existing store without creating or migrating it and refuse every
    /// write.
    #[arg(long, global = true)]
    pub(crate) read_only: bool,

    /// Act as this principal instead of naming a session token. The command
    /// runs as the principal's active session in the role it needs: a command
    /// that starts work registers that session when there is none, and a
    /// command that continues work a session holds only looks it up.
    #[arg(long = "as", global = true, value_name = "PRINCIPAL")]
    pub(crate) as_principal: Option<String>,

    /// Role to act in with `--as`, in place of the command's usual role.
    #[arg(long, global = true, value_enum, value_name = "ROLE")]
    pub(crate) as_role: Option<SessionRoleArg>,

    #[command(subcommand)]
    pub(crate) command: Commands,
}

// Names the session a command acts as. Deliberately not a doc comment: clap
// reports the doc comment of a flattened struct as the description of every
// command that flattens it.
#[derive(Args, Debug)]
pub(crate) struct SessionArg {
    /// Token of the session to act as. Give either this or the global `--as`.
    #[arg(long)]
    pub(crate) session_token: Option<String>,
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Commands {
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    Meta {
        #[command(subcommand)]
        command: MetaCommand,
    },
    Subtask {
        #[command(subcommand)]
        command: SubtaskCommand,
    },
    Claim {
        #[command(subcommand)]
        command: ClaimCommand,
    },
    Artifact {
        #[command(subcommand)]
        command: ArtifactCommand,
    },
    Review {
        #[command(subcommand)]
        command: ReviewCommand,
    },
    Queue {
        #[command(subcommand)]
        command: QueueCommand,
    },
    Reservation {
        #[command(subcommand)]
        command: ReservationCommand,
    },
    Repoops {
        #[command(subcommand)]
        command: RepoopsCommand,
    },
    Events {
        #[command(subcommand)]
        command: EventsCommand,
    },
    Conflict {
        #[command(subcommand)]
        command: ConflictCommand,
    },
    Maint {
        #[command(subcommand)]
        command: MaintCommand,
    },
    Import {
        #[command(subcommand)]
        command: ImportCommand,
    },
    Digest {
        #[command(subcommand)]
        command: DigestCommand,
    },
    Proof {
        #[command(subcommand)]
        command: ProofCommand,
    },
}

impl Commands {
    /// True when dispatching this command reaches only the store's read
    /// helpers, so the command never writes the store.
    pub(crate) fn is_read_only_command(&self) -> bool {
        match self {
            Self::Session { command } => match command {
                SessionCommand::ActiveForPrincipal(_) | SessionCommand::Status(_) => true,
                SessionCommand::Register(_)
                | SessionCommand::Ensure(_)
                | SessionCommand::Attest(_)
                | SessionCommand::Heartbeat(_)
                | SessionCommand::Exit(_) => false,
            },
            Self::Meta { command } => match command {
                MetaCommand::Status(_) | MetaCommand::List(_) => true,
                MetaCommand::Submit(_)
                | MetaCommand::Cancel(_)
                | MetaCommand::Complete(_)
                | MetaCommand::Annotate(_)
                | MetaCommand::Update(_)
                | MetaCommand::LinkSource(_) => false,
            },
            Self::Subtask { command } => match command {
                SubtaskCommand::Status(_)
                | SubtaskCommand::Candidates(_)
                | SubtaskCommand::Availability(_)
                | SubtaskCommand::Stuck(_) => true,
                SubtaskCommand::Create(_)
                | SubtaskCommand::ClaimNext(_)
                | SubtaskCommand::Claim(_)
                | SubtaskCommand::Start(_)
                | SubtaskCommand::Finish(_)
                | SubtaskCommand::Retry(_)
                | SubtaskCommand::Fail(_)
                | SubtaskCommand::Abandon(_)
                | SubtaskCommand::Retire(_) => false,
            },
            Self::Claim { command } => match command {
                ClaimCommand::Expiring(_) => true,
                ClaimCommand::Renew(_) | ClaimCommand::Release(_) => false,
            },
            Self::Artifact { command } => match command {
                ArtifactCommand::Publish(_) => false,
            },
            Self::Review { command } => match command {
                ReviewCommand::Request(_)
                | ReviewCommand::Decide(_)
                | ReviewCommand::PermissiveLand(_) => false,
            },
            Self::Queue { command } => match command {
                QueueCommand::List(_)
                | QueueCommand::Candidates(_)
                | QueueCommand::VerifyLandingAuthorization(_)
                | QueueCommand::ListOpenSpecArchiveBlockers(_)
                | QueueCommand::Metrics => true,
                QueueCommand::Enqueue(_)
                | QueueCommand::ReconcileApply(_)
                | QueueCommand::ClaimNext(_)
                | QueueCommand::MarkInFlight(_)
                | QueueCommand::RecordApplyVerification(_)
                | QueueCommand::RecordApplyGateBlocker(_)
                | QueueCommand::RecordSettlementReconcileBlocker(_)
                | QueueCommand::RecordLandingReceipt(_)
                | QueueCommand::MarkApplied(_)
                | QueueCommand::RecordOpenSpecArchiveStatus(_)
                | QueueCommand::Supersede(_) => false,
            },
            Self::Reservation { command } => match command {
                ReservationCommand::Overlaps(_) => true,
                ReservationCommand::Request(_)
                | ReservationCommand::Release(_)
                | ReservationCommand::Renew(_) => false,
            },
            Self::Repoops { command } => match command {
                RepoopsCommand::AuthoritySnapshot(_) => true,
            },
            Self::Events { command } => match command {
                EventsCommand::List(_) => true,
            },
            Self::Conflict { command } => match command {
                ConflictCommand::List => true,
                ConflictCommand::Resolve(_) => false,
            },
            Self::Maint { command } => match command {
                MaintCommand::Backup(_) => true,
                MaintCommand::ReapStale(_)
                | MaintCommand::ExpireClaims
                | MaintCommand::ExpireReservations
                | MaintCommand::Prune(_) => false,
            },
            Self::Import { command } => match command {
                ImportCommand::Provenance(_) => true,
                ImportCommand::Bd(_) | ImportCommand::Openspec(_) => false,
            },
            Self::Digest { .. } | Self::Proof { .. } => true,
        }
    }
}

#[derive(Subcommand, Debug)]
pub(crate) enum DigestCommand {
    /// Compute a BLAKE3 digest with a blake3: prefix.
    Blake3(DigestBlake3Args),
}

#[derive(Args, Debug)]
pub(crate) struct DigestBlake3Args {
    /// Read bytes from this file.
    #[arg(long, conflicts_with_all = ["text", "stdin"])]
    pub(crate) file: Option<PathBuf>,

    /// Hash this UTF-8 string.
    #[arg(long, conflicts_with_all = ["file", "stdin"])]
    pub(crate) text: Option<String>,

    /// Read bytes from stdin.
    #[arg(long, conflicts_with_all = ["file", "text"])]
    pub(crate) stdin: bool,
}

#[derive(Subcommand, Debug)]
pub(crate) enum ProofCommand {
    Apply {
        #[command(subcommand)]
        command: ProofApplyCommand,
    },
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ProofApplyCommand {
    /// Verify and seal one Covey-bound apply proof.
    Verify(ApplyProofVerifyArgs),
    /// Verify and aggregate a batch of Covey-bound apply proofs.
    #[command(name = "verify-batch")]
    VerifyBatch(ApplyProofBatchArgs),
    /// Print the reusable verifier request/output contract.
    #[command(name = "print-contract")]
    PrintContract,
}

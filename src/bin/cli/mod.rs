use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use covey::proof_apply::{ApplyProofBatchArgs, ApplyProofVerifyArgs};

use crate::DEFAULT_DB_PATH;

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
    #[arg(long, global = true, default_value = DEFAULT_DB_PATH)]
    pub(crate) db: PathBuf,

    #[arg(long, global = true)]
    pub(crate) json: bool,

    #[command(subcommand)]
    pub(crate) command: Commands,
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

use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
}

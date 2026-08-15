use clap::{Args, Subcommand, ValueEnum};
use covey::ConflictResolutionState;

#[derive(Subcommand, Debug)]
pub(crate) enum EventsCommand {
    List(ListEventsArgs),
}

#[derive(Subcommand, Debug)]
pub(crate) enum ConflictCommand {
    List,
    Resolve(ResolveConflictArgs),
}

#[derive(Subcommand, Debug)]
pub(crate) enum MaintCommand {
    #[command(name = "reap-stale")]
    ReapStale(ReapStaleArgs),
    #[command(name = "expire-claims")]
    ExpireClaims,
    #[command(name = "expire-reservations")]
    ExpireReservations,
    #[command(name = "backup")]
    /// Write a consistent snapshot of the database to a fresh SQLite file.
    Backup(BackupArgs),
}
#[derive(Args, Debug)]
pub(crate) struct BackupArgs {
    #[arg(long)]
    pub(crate) output: std::path::PathBuf,
}
#[derive(Args, Debug)]
pub(crate) struct ListEventsArgs {
    #[arg(long, default_value_t = 0)]
    pub(crate) after_seq: i64,
    #[arg(long, default_value_t = 100)]
    pub(crate) limit: usize,
    #[arg(long)]
    pub(crate) typed: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ResolveConflictArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) conflict_id: String,
    #[arg(long)]
    pub(crate) resolution_state: ConflictResolutionStateArg,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ReapStaleArgs {
    #[arg(long)]
    pub(crate) stale_threshold_ms: i64,
}
#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum ConflictResolutionStateArg {
    Open,
    Acknowledged,
    Resolved,
}

impl From<ConflictResolutionStateArg> for ConflictResolutionState {
    fn from(value: ConflictResolutionStateArg) -> Self {
        match value {
            ConflictResolutionStateArg::Open => ConflictResolutionState::Open,
            ConflictResolutionStateArg::Acknowledged => ConflictResolutionState::Acknowledged,
            ConflictResolutionStateArg::Resolved => ConflictResolutionState::Resolved,
        }
    }
}

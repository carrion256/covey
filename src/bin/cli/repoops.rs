use clap::{Args, Subcommand};

use super::SessionArg;

#[derive(Subcommand, Debug)]
pub(crate) enum RepoopsCommand {
    AuthoritySnapshot(RepoopsAuthoritySnapshotArgs),
}

#[derive(Args, Debug)]
pub(crate) struct RepoopsAuthoritySnapshotArgs {
    #[command(flatten)]
    pub(crate) session: SessionArg,

    #[arg(long)]
    pub(crate) claim_id: String,

    #[arg(long)]
    pub(crate) fence_seq: i64,

    #[arg(long = "paths", required = true, num_args = 1..)]
    pub(crate) paths: Vec<String>,
}

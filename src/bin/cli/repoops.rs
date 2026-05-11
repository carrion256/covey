use clap::{Args, Subcommand};

#[derive(Subcommand, Debug)]
pub(crate) enum RepoopsCommand {
    AuthoritySnapshot(RepoopsAuthoritySnapshotArgs),
}

#[derive(Args, Debug)]
pub(crate) struct RepoopsAuthoritySnapshotArgs {
    #[arg(long)]
    pub(crate) session_token: String,

    #[arg(long)]
    pub(crate) claim_id: String,

    #[arg(long)]
    pub(crate) fence_seq: i64,

    #[arg(long = "paths", required = true, num_args = 1..)]
    pub(crate) paths: Vec<String>,
}

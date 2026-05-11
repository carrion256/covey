use covey::{Covey, RepoopsAuthoritySnapshotReq};

use crate::{cli::RepoopsCommand, render_support::Rendered};

pub(crate) fn dispatch_repoops(store: &Covey, command: RepoopsCommand) -> covey::Result<Rendered> {
    match command {
        RepoopsCommand::AuthoritySnapshot(args) => {
            let snapshot = store.repoops_authority_snapshot(RepoopsAuthoritySnapshotReq {
                session_token: args.session_token,
                claim_id: args.claim_id,
                fence_seq: args.fence_seq,
                paths: args.paths,
            })?;
            Ok(Rendered::pretty(snapshot))
        }
    }
}

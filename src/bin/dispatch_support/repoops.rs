use covey::{Covey, RepoopsAuthoritySnapshotReq};

use super::{Actor, HOLDING_WORK};
use crate::{cli::RepoopsCommand, render_support::Rendered};

pub(crate) fn dispatch_repoops(
    store: &Covey,
    actor: &Actor,
    command: RepoopsCommand,
) -> covey::Result<Rendered> {
    match command {
        RepoopsCommand::AuthoritySnapshot(args) => {
            let snapshot = store.repoops_authority_snapshot(
                RepoopsAuthoritySnapshotReq::try_from_raw_parts(
                    actor.session_token(store, args.session, HOLDING_WORK)?,
                    args.claim_id,
                    args.fence_seq,
                    args.paths,
                )
                .map_err(covey::CoveyError::from)?,
            )?;
            Ok(Rendered::pretty(snapshot))
        }
    }
}

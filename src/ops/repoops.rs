#![cfg_attr(coverage_nightly, coverage(off))]

use std::time::Instant;

use crate::{
    Covey,
    error::Result,
    model::{
        Claim, RepoopsAuthorityClaimFact, RepoopsAuthorityGitContextFact, RepoopsAuthorityLockFact,
        RepoopsAuthorityPolicyFact, RepoopsAuthorityScopeFact, RepoopsAuthoritySnapshot,
        RepoopsAuthoritySnapshotReq, Reservation, ScopeClass, Session,
    },
    queries::{load_active_reservations_tx, load_subtask_tx},
    validators::require_current_claim,
};

const REPOOPS_AUTHORITY_SNAPSHOT_VERSION: &str = "covey_repoops_authority_snapshot.v1";

impl Covey {
    /// Returns Covey lifecycle facts for mutAI repoops preflight.
    ///
    /// This method does not decide repo mutation legality. It validates the
    /// current claim/fence and packages coordination facts so `mutai-rs`
    /// can make the repoops decision.
    pub fn repoops_authority_snapshot(
        &self,
        req: RepoopsAuthoritySnapshotReq,
    ) -> Result<RepoopsAuthoritySnapshot> {
        let started_at = Instant::now();
        let now = self.clock.wall_now_ms();
        let result = self.with_read_tx(|tx| {
            let claim =
                require_current_claim(tx, &req.session_token, &req.claim_id, req.fence_seq, now)?;
            let subtask = load_subtask_tx(tx, &claim.subtask_id)?;
            let reservations = load_active_reservations_tx(tx, now)?;
            let session = crate::queries::load_session_tx(tx, &req.session_token)?;
            let paths = normalize_requested_paths(&req.paths);
            let scope_in = scope_patterns_for_subtask(&reservations, &claim.subtask_id);
            let locks = lock_facts_for_paths(&paths, &reservations, &claim, &session);
            let caller_ownership_token = ownership_token_ref(&req.session_token);
            let active_ownership_token = ownership_token_ref(&claim.owner_session_token);
            let fact_sources = vec![
                format!("session_token_ref:{caller_ownership_token}"),
                format!("claim:{}", req.claim_id),
                format!("subtask:{}", claim.subtask_id),
                "reservations:active".to_owned(),
                "claims.owner_session_token:token_ref".to_owned(),
            ];
            let claim_fact = RepoopsAuthorityClaimFact::new(
                claim.claim_id.clone(),
                subtask.state.to_string(),
                session.agent_principal_id.clone(),
                scope_in.clone(),
                Vec::new(),
                !scope_in.is_empty(),
                Some(active_ownership_token),
            );
            Ok(RepoopsAuthoritySnapshot::new(
                REPOOPS_AUTHORITY_SNAPSHOT_VERSION.to_owned(),
                session.agent_principal_id,
                Some(claim.claim_id),
                Some(caller_ownership_token),
                None,
                RepoopsAuthorityPolicyFact::new("enforce".to_owned(), 2, None),
                Some(claim_fact),
                RepoopsAuthorityScopeFact::new(scope_in, Vec::new()),
                locks,
                Some(RepoopsAuthorityGitContextFact::new(None, None, None, true)),
                None,
                fact_sources,
            ))
        });
        self.log_operation(
            "repoops_authority_snapshot",
            &req.session_token,
            started_at,
            &result,
            |snapshot| {
                let mut objects = vec![format!("claim:{}", req.claim_id)];
                objects.extend(
                    snapshot
                        .locks
                        .iter()
                        .map(|lock| format!("repoops-lock:{}", lock.path)),
                );
                objects
            },
        );
        result
    }
}

fn ownership_token_ref(session_token: &str) -> String {
    format!(
        "covey-session-token-blake3:{}",
        blake3::hash(session_token.as_bytes())
    )
}

fn normalize_requested_paths(paths: &[String]) -> Vec<String> {
    let mut normalized = paths
        .iter()
        .filter_map(|path| normalize_repo_relative_path(path))
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn normalize_repo_relative_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut normalized = trimmed.replace('\\', "/");
    while let Some(rest) = normalized.strip_prefix("./") {
        normalized = rest.to_owned();
    }
    while let Some(rest) = normalized.strip_prefix('/') {
        normalized = rest.to_owned();
    }
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn scope_patterns_for_subtask(reservations: &[Reservation], subtask_id: &str) -> Vec<String> {
    let mut patterns = reservations
        .iter()
        .filter(|reservation| reservation.owner_subtask_id == subtask_id)
        .flat_map(scope_patterns_for_reservation)
        .collect::<Vec<_>>();
    patterns.sort();
    patterns.dedup();
    patterns
}

fn scope_patterns_for_reservation(reservation: &Reservation) -> Vec<String> {
    match reservation.scope_class {
        ScopeClass::RepoGlobal => vec!["**".to_owned()],
        ScopeClass::ExactPath => normalize_repo_relative_path(&reservation.scope_key)
            .into_iter()
            .collect(),
        ScopeClass::Subtree => normalize_repo_relative_path(&reservation.scope_key)
            .map(|path| format!("{path}/**"))
            .into_iter()
            .collect(),
        ScopeClass::GeneratedSet => reservation
            .generated_members
            .iter()
            .filter_map(|member| normalize_repo_relative_path(member))
            .collect(),
    }
}

fn lock_facts_for_paths(
    paths: &[String],
    reservations: &[Reservation],
    claim: &Claim,
    session: &Session,
) -> Vec<RepoopsAuthorityLockFact> {
    let mut locks = Vec::new();
    for path in paths {
        for reservation in reservations {
            if !reservation_covers_path(reservation, path) {
                continue;
            }
            if reservation.owner_subtask_id == claim.subtask_id {
                locks.push(RepoopsAuthorityLockFact::new(
                    path.clone(),
                    session.agent_principal_id.clone(),
                    claim.claim_id.clone(),
                    "owned".to_owned(),
                ));
            } else {
                locks.push(RepoopsAuthorityLockFact::new(
                    path.clone(),
                    format!("subtask:{}", reservation.owner_subtask_id),
                    format!("unknown:{}", reservation.owner_subtask_id),
                    "foreign_owner".to_owned(),
                ));
            }
        }
    }
    locks.sort_by(|left, right| {
        (&left.path, &left.owner, &left.claim_id).cmp(&(&right.path, &right.owner, &right.claim_id))
    });
    locks.dedup();
    locks
}

fn reservation_covers_path(reservation: &Reservation, path: &str) -> bool {
    match reservation.scope_class {
        ScopeClass::RepoGlobal => true,
        ScopeClass::ExactPath => {
            normalize_repo_relative_path(&reservation.scope_key).as_deref() == Some(path)
        }
        ScopeClass::Subtree => normalize_repo_relative_path(&reservation.scope_key)
            .is_some_and(|base| path == base || path.starts_with(&format!("{base}/"))),
        ScopeClass::GeneratedSet => reservation
            .generated_members
            .iter()
            .any(|member| normalize_repo_relative_path(member).as_deref() == Some(path)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClaimState, ReservationState, SubtaskState};

    fn reservation(
        scope_class: ScopeClass,
        scope_key: &str,
        owner_subtask_id: &str,
    ) -> Reservation {
        Reservation {
            reservation_id: "reservation-1".to_owned(),
            owner_subtask_id: owner_subtask_id.to_owned(),
            scope_class,
            scope_key: scope_key.to_owned(),
            generated_members: Vec::new(),
            lease_deadline: 1_000,
            state: ReservationState::Active,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn scope_patterns_map_reservation_shapes() {
        assert_eq!(
            scope_patterns_for_reservation(&reservation(ScopeClass::Subtree, "src", "task-1")),
            vec!["src/**"]
        );
        assert_eq!(
            scope_patterns_for_reservation(&reservation(ScopeClass::RepoGlobal, "repo", "task-1")),
            vec!["**"]
        );
    }

    #[test]
    fn lock_facts_distinguish_owned_and_foreign_reservations() {
        let claim = Claim {
            claim_id: "claim-1".to_owned(),
            subtask_id: "task-1".to_owned(),
            owner_session_token: "session-1".to_owned(),
            fence_seq: 7,
            lease_deadline: 1_000,
            state: ClaimState::Held,
            created_at: 1,
            updated_at: 1,
        };
        let session = Session {
            session_token: "session-1".to_owned(),
            agent_principal_id: "worker-1".to_owned(),
            agent_instance_id: "worker-1-instance".to_owned(),
            role: crate::model::SessionRole::Executor,
            state: crate::model::SessionState::Active,
            active_subtask_id: Some("task-1".to_owned()),
            last_heartbeat_at: 1,
            last_heartbeat_tick: 1,
            created_at: 1,
            updated_at: 1,
        };
        let reservations = vec![
            reservation(ScopeClass::ExactPath, "src/lib.rs", "task-1"),
            reservation(ScopeClass::ExactPath, "src/lib.rs", "task-2"),
        ];
        let locks =
            lock_facts_for_paths(&["src/lib.rs".to_owned()], &reservations, &claim, &session);
        assert!(locks.iter().any(|lock| lock.status == "owned"));
        assert!(locks.iter().any(|lock| lock.status == "foreign_owner"));
    }

    #[test]
    fn subtask_state_string_matches_repoops_claim_status() {
        assert_eq!(SubtaskState::InProgress.to_string(), "in_progress");
    }
}

#![cfg_attr(coverage_nightly, coverage(off))]

use std::time::Instant;

use crate::{
    Covey,
    error::Result,
    model::{
        Claim, RepoopsAuthorityClaimFact, RepoopsAuthorityClaimStatus,
        RepoopsAuthorityGitContextFact, RepoopsAuthorityLockFact, RepoopsAuthorityPolicyFact,
        RepoopsAuthorityScopeFact, RepoopsAuthoritySnapshot, RepoopsAuthoritySnapshotCommon,
        RepoopsAuthoritySnapshotReq, RepoopsClaimRef, Reservation, ScopeClass, Session,
        SubtaskState,
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
            let paths = normalize_requested_paths(req.paths());
            let scope_in = scope_patterns_for_subtask(&reservations, &claim.subtask_id);
            let scope = RepoopsAuthorityScopeFact::new(scope_in.clone(), Vec::new())
                .map_err(|reason| crate::CoveyError::InvalidObservabilityRow { reason })?;
            let locks = lock_facts_for_paths(&paths, &reservations, &claim, &session)
                .map_err(|reason| crate::CoveyError::InvalidObservabilityRow { reason })?;
            let caller_ownership_token = ownership_token_ref(&req.session_token);
            let active_ownership_token = ownership_token_ref(&claim.owner_session_token);
            let fact_sources = vec![
                format!("session_token_ref:{caller_ownership_token}"),
                format!("claim:{}", req.claim_id),
                format!("subtask:{}", claim.subtask_id),
                "reservations:active".to_owned(),
                "claims.owner_session_token:token_ref".to_owned(),
            ];
            let claim_fact = match repoops_claim_status_for_subtask(subtask.state()) {
                RepoopsAuthorityClaimStatus::InProgress => RepoopsAuthorityClaimFact::in_progress(
                    claim.claim_id.clone(),
                    session.agent_principal_id().to_owned(),
                    scope_in.clone(),
                    Vec::new(),
                    !scope_in.is_empty(),
                    active_ownership_token,
                ),
                RepoopsAuthorityClaimStatus::Open => RepoopsAuthorityClaimFact::open(
                    claim.claim_id.clone(),
                    session.agent_principal_id().to_owned(),
                    scope_in.clone(),
                    Vec::new(),
                    !scope_in.is_empty(),
                ),
            }
            .map_err(|reason| crate::CoveyError::InvalidObservabilityRow { reason })?;
            let common = RepoopsAuthoritySnapshotCommon::new(
                REPOOPS_AUTHORITY_SNAPSHOT_VERSION.to_owned(),
                session.agent_principal_id().to_owned(),
                RepoopsAuthorityPolicyFact::enforce(),
                scope,
                locks,
                Some(RepoopsAuthorityGitContextFact::unknown(true)),
                fact_sources,
            )
            .map_err(|reason| crate::CoveyError::InvalidObservabilityRow { reason })?;
            RepoopsAuthoritySnapshot::claim_bound(common, caller_ownership_token, claim_fact)
                .map_err(|reason| crate::CoveyError::InvalidObservabilityRow { reason })
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
                        .locks()
                        .iter()
                        .map(|lock| format!("repoops-lock:{}", lock.path())),
                );
                objects
            },
        );
        result
    }
}

fn repoops_claim_status_for_subtask(state: SubtaskState) -> RepoopsAuthorityClaimStatus {
    match state {
        SubtaskState::InProgress => RepoopsAuthorityClaimStatus::InProgress,
        SubtaskState::Available
        | SubtaskState::Blocked
        | SubtaskState::Claimed
        | SubtaskState::ArtifactPublished
        | SubtaskState::ReviewPending
        | SubtaskState::ChangesRequested
        | SubtaskState::Approved
        | SubtaskState::Decided
        | SubtaskState::ReadyForApply
        | SubtaskState::Applied
        | SubtaskState::Abandoned => RepoopsAuthorityClaimStatus::Open,
    }
}

fn ownership_token_ref(session_token: &str) -> crate::model::SessionToken {
    crate::model::SessionToken::parse(format!(
        "covey-session-token-blake3:{}",
        blake3::hash(session_token.as_bytes())
    ))
    .expect("blake3-derived ownership token ref should be token-safe")
}

fn normalize_requested_paths(paths: &[crate::model::RepoopsPath]) -> Vec<String> {
    let mut normalized = paths
        .iter()
        .map(|path| path.as_str().to_owned())
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
    match reservation.scope_class() {
        ScopeClass::RepoGlobal => vec!["**".to_owned()],
        ScopeClass::ExactPath => normalize_repo_relative_path(reservation.scope_key())
            .into_iter()
            .collect(),
        ScopeClass::Subtree => normalize_repo_relative_path(reservation.scope_key())
            .map(|path| format!("{path}/**"))
            .into_iter()
            .collect(),
        ScopeClass::GeneratedSet => reservation
            .generated_members()
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
) -> std::result::Result<Vec<RepoopsAuthorityLockFact>, String> {
    let mut locks = Vec::new();
    for path in paths {
        for reservation in reservations {
            if !reservation_covers_path(reservation, path) {
                continue;
            }
            if reservation.owner_subtask_id == claim.subtask_id {
                locks.push(RepoopsAuthorityLockFact::owned(
                    path.clone(),
                    session.agent_principal_id().to_owned(),
                    repoops_claim_ref(claim.claim_id.as_str()),
                )?);
            } else {
                locks.push(RepoopsAuthorityLockFact::foreign_owner(
                    path.clone(),
                    format!("subtask:{}", reservation.owner_subtask_id),
                    repoops_claim_ref(format!("unknown:{}", reservation.owner_subtask_id)),
                )?);
            }
        }
    }
    locks.sort_by(|left, right| {
        (left.path(), left.owner(), left.claim_id()).cmp(&(
            right.path(),
            right.owner(),
            right.claim_id(),
        ))
    });
    locks.dedup();
    Ok(locks)
}

fn repoops_claim_ref(value: impl Into<String>) -> RepoopsClaimRef {
    RepoopsClaimRef::parse(value).expect("repoops claim refs are derived from validated ids")
}

fn reservation_covers_path(reservation: &Reservation, path: &str) -> bool {
    match reservation.scope_class() {
        ScopeClass::RepoGlobal => true,
        ScopeClass::ExactPath => {
            normalize_repo_relative_path(reservation.scope_key()).as_deref() == Some(path)
        }
        ScopeClass::Subtree => normalize_repo_relative_path(reservation.scope_key())
            .is_some_and(|base| path == base || path.starts_with(&format!("{base}/"))),
        ScopeClass::GeneratedSet => reservation
            .generated_members()
            .iter()
            .any(|member| normalize_repo_relative_path(member).as_deref() == Some(path)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ClaimId, ClaimState, FenceSeq, LeaseDeadlineMs, RepoopsAuthorityLockStatus, ReservationId,
        ReservationState, SessionToken, SubtaskId, SubtaskState, TimestampMs,
    };

    fn reservation(
        scope_class: ScopeClass,
        scope_key: &str,
        owner_subtask_id: &str,
    ) -> Reservation {
        Reservation::try_from_parts(
            ReservationId::parse("reservation-1").expect("valid reservation id"),
            SubtaskId::parse(owner_subtask_id).expect("valid subtask id"),
            scope_class,
            scope_key,
            Vec::new(),
            LeaseDeadlineMs::parse(1_000).expect("valid lease deadline"),
            ReservationState::Active,
            TimestampMs::parse(1).expect("valid timestamp"),
            TimestampMs::parse(1).expect("valid timestamp"),
        )
        .expect("valid reservation fixture")
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
        let claim = Claim::try_from_parts(
            ClaimId::parse("claim-1").expect("valid claim id"),
            SubtaskId::parse("task-1").expect("valid subtask id"),
            SessionToken::parse("session-1").expect("valid session token"),
            FenceSeq::parse(7).expect("valid fence"),
            LeaseDeadlineMs::parse(1_000).expect("valid lease deadline"),
            ClaimState::Held,
            TimestampMs::parse(1).expect("valid timestamp"),
            TimestampMs::parse(1).expect("valid timestamp"),
        )
        .expect("valid claim");
        let session = Session::try_from_parts(
            SessionToken::parse("session-1").expect("valid session token"),
            "worker-1",
            "worker-1-instance",
            crate::model::SessionRole::Executor,
            crate::model::SessionState::Active,
            Some(SubtaskId::parse("task-1").expect("valid subtask id")),
            TimestampMs::parse(1).expect("valid timestamp"),
            1,
            TimestampMs::parse(1).expect("valid timestamp"),
            TimestampMs::parse(1).expect("valid timestamp"),
        )
        .expect("valid active session fixture");
        let reservations = vec![
            reservation(ScopeClass::ExactPath, "src/lib.rs", "task-1"),
            reservation(ScopeClass::ExactPath, "src/lib.rs", "task-2"),
        ];
        let locks =
            lock_facts_for_paths(&["src/lib.rs".to_owned()], &reservations, &claim, &session)
                .expect("valid lock facts");
        assert!(
            locks
                .iter()
                .any(|lock| lock.status() == RepoopsAuthorityLockStatus::Owned)
        );
        assert!(
            locks
                .iter()
                .any(|lock| lock.status() == RepoopsAuthorityLockStatus::ForeignOwner)
        );
    }

    #[test]
    fn subtask_state_maps_to_repoops_claim_status() {
        assert_eq!(
            repoops_claim_status_for_subtask(SubtaskState::InProgress),
            RepoopsAuthorityClaimStatus::InProgress
        );
        assert_eq!(
            repoops_claim_status_for_subtask(SubtaskState::Claimed),
            RepoopsAuthorityClaimStatus::Open
        );
    }

    #[test]
    fn repoops_authority_snapshot_statuses_reject_unknown_strings() {
        let unknown_claim_status = serde_json::json!({
            "claim_id": "claim-1",
            "status": "mystery",
            "owner": "worker-1",
            "scope_in": ["src/**"],
            "scope_out": [],
            "has_required_contract_fields": true,
            "active_ownership_token": "token-ref"
        });
        serde_json::from_value::<RepoopsAuthorityClaimFact>(unknown_claim_status)
            .expect_err("unknown claim status must be rejected");

        let missing_in_progress_token = serde_json::json!({
            "claim_id": "claim-1",
            "status": "in_progress",
            "owner": "worker-1",
            "scope_in": ["src/**"],
            "scope_out": [],
            "has_required_contract_fields": true,
            "active_ownership_token": null
        });
        serde_json::from_value::<RepoopsAuthorityClaimFact>(missing_in_progress_token)
            .expect_err("in-progress claim facts must carry an ownership token");

        let blank_in_progress_token = serde_json::json!({
            "claim_id": "claim-1",
            "status": "in_progress",
            "owner": "worker-1",
            "scope_in": ["src/**"],
            "scope_out": [],
            "has_required_contract_fields": true,
            "active_ownership_token": " "
        });
        serde_json::from_value::<RepoopsAuthorityClaimFact>(blank_in_progress_token)
            .expect_err("in-progress claim facts must carry a valid ownership token");

        let stale_open_token = serde_json::json!({
            "claim_id": "claim-1",
            "status": "open",
            "owner": "worker-1",
            "scope_in": ["src/**"],
            "scope_out": [],
            "has_required_contract_fields": true,
            "active_ownership_token": "token-ref"
        });
        serde_json::from_value::<RepoopsAuthorityClaimFact>(stale_open_token)
            .expect_err("open claim facts must not carry stale ownership tokens");

        let unknown_lock_status = serde_json::json!({
            "path": "src/lib.rs",
            "owner": "worker-1",
            "claim_id": "claim-1",
            "status": "maybe_owned"
        });
        serde_json::from_value::<RepoopsAuthorityLockFact>(unknown_lock_status)
            .expect_err("unknown lock status must be rejected");

        let blank_lock_path = serde_json::json!({
            "path": " ",
            "owner": "worker-1",
            "claim_id": "claim-1",
            "status": "owned"
        });
        serde_json::from_value::<RepoopsAuthorityLockFact>(blank_lock_path)
            .expect_err("lock path must not be blank");

        let padded_lock_owner = serde_json::json!({
            "path": "src/lib.rs",
            "owner": "worker-1 ",
            "claim_id": "claim-1",
            "status": "owned"
        });
        serde_json::from_value::<RepoopsAuthorityLockFact>(padded_lock_owner)
            .expect_err("lock owner must be normalized");

        let absolute_lock_path = serde_json::json!({
            "path": "/src/lib.rs",
            "owner": "worker-1",
            "claim_id": "claim-1",
            "status": "owned"
        });
        serde_json::from_value::<RepoopsAuthorityLockFact>(absolute_lock_path)
            .expect_err("lock path must be repo-relative");

        RepoopsAuthorityLockFact::foreign_owner(
            "src/../lib.rs",
            "worker-2",
            repoops_claim_ref("claim-2"),
        )
        .expect_err("constructor must reject traversing lock paths");

        let unknown_policy_mode = serde_json::json!({
            "mode": "observe",
            "phase": 2,
            "denied_rule_id": null
        });
        serde_json::from_value::<RepoopsAuthorityPolicyFact>(unknown_policy_mode)
            .expect_err("unknown policy mode must be rejected");

        let unsupported_policy_phase = serde_json::json!({
            "mode": "enforce",
            "phase": 1,
            "denied_rule_id": null
        });
        serde_json::from_value::<RepoopsAuthorityPolicyFact>(unsupported_policy_phase)
            .expect_err("unsupported enforce policy phases must be rejected");

        let denied_rule_on_enforce_policy = serde_json::json!({
            "mode": "enforce",
            "phase": 2,
            "denied_rule_id": "rule-should-not-be-here"
        });
        serde_json::from_value::<RepoopsAuthorityPolicyFact>(denied_rule_on_enforce_policy)
            .expect_err("enforce policy facts must not carry denied rule ids");

        let valid_claim_bound_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": "claim-1",
            "ownership_token": "token-ref",
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": {
                "claim_id": "claim-1",
                "status": "in_progress",
                "owner": "worker-1",
                "scope_in": ["src/**"],
                "scope_out": [],
                "has_required_contract_fields": true,
                "active_ownership_token": "token-ref"
            },
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": null,
            "fact_sources": []
        });
        let snapshot =
            serde_json::from_value::<RepoopsAuthoritySnapshot>(valid_claim_bound_snapshot)
                .expect("valid claim-bound repoops snapshot");
        assert_eq!(snapshot.claim_id().map(ClaimId::as_str), Some("claim-1"));
        assert_eq!(snapshot.ownership_token(), Some("token-ref"));
        assert!(snapshot.claim().is_some());
        assert!(snapshot.constraint_reason().is_none());

        let missing_claim_fact_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": "claim-1",
            "ownership_token": "token-ref",
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": null,
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": null,
            "fact_sources": []
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(missing_claim_fact_snapshot)
            .expect_err("claim-bound snapshots must include the claim fact");

        let mixed_constraint_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": "claim-1",
            "ownership_token": "token-ref",
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": {
                "claim_id": "claim-1",
                "status": "in_progress",
                "owner": "worker-1",
                "scope_in": ["src/**"],
                "scope_out": [],
                "has_required_contract_fields": true,
                "active_ownership_token": "token-ref"
            },
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": "claim unavailable",
            "fact_sources": []
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(mixed_constraint_snapshot)
            .expect_err("repoops snapshots must not mix claim authority and constraints");

        let valid_constraint_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": null,
            "ownership_token": null,
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": null,
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": "claim unavailable",
            "fact_sources": []
        });
        let snapshot =
            serde_json::from_value::<RepoopsAuthoritySnapshot>(valid_constraint_snapshot)
                .expect("valid constrained repoops snapshot");
        assert_eq!(snapshot.constraint_reason(), Some("claim unavailable"));

        let blank_constraint_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": null,
            "ownership_token": null,
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": null,
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": " ",
            "fact_sources": []
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(blank_constraint_snapshot)
            .expect_err("constrained repoops snapshots must carry a non-empty reason");

        let padded_constraint_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": null,
            "ownership_token": null,
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": null,
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": " claim unavailable",
            "fact_sources": []
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(padded_constraint_snapshot)
            .expect_err("constrained repoops snapshots must carry normalized reasons");

        let control_constraint_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": null,
            "ownership_token": null,
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": null,
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": "claim\nunavailable",
            "fact_sources": []
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(control_constraint_snapshot)
            .expect_err("constrained repoops snapshots must reject control characters in reasons");

        let mismatched_claim_id_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": "claim-1",
            "ownership_token": "token-ref",
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": {
                "claim_id": "claim-other",
                "status": "in_progress",
                "owner": "worker-1",
                "scope_in": ["src/**"],
                "scope_out": [],
                "has_required_contract_fields": true,
                "active_ownership_token": "token-ref"
            },
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": null,
            "fact_sources": []
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(mismatched_claim_id_snapshot)
            .expect_err("repoops snapshots must bind the same top-level and claim-fact ids");

        let blank_ownership_token_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": "claim-1",
            "ownership_token": "",
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": {
                "claim_id": "claim-1",
                "status": "in_progress",
                "owner": "worker-1",
                "scope_in": ["src/**"],
                "scope_out": [],
                "has_required_contract_fields": true,
                "active_ownership_token": "token-ref"
            },
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": null,
            "fact_sources": []
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(blank_ownership_token_snapshot)
            .expect_err("claim-bound snapshots must carry a valid ownership token");

        let override_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": "claim-1",
            "ownership_token": "token-ref",
            "override_token": "override-ref",
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": {
                "claim_id": "claim-1",
                "status": "in_progress",
                "owner": "worker-1",
                "scope_in": ["src/**"],
                "scope_out": [],
                "has_required_contract_fields": true,
                "active_ownership_token": "token-ref"
            },
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": null,
            "fact_sources": []
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(override_snapshot)
            .expect_err("override tokens are not supported in repoops snapshots yet");
    }

    #[test]
    fn repoops_authority_snapshot_rejects_invalid_common_identity_fields() {
        let valid_claim_bound_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": "claim-1",
            "ownership_token": "token-ref",
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": {
                "claim_id": "claim-1",
                "status": "in_progress",
                "owner": "worker-1",
                "scope_in": ["src/**"],
                "scope_out": [],
                "has_required_contract_fields": true,
                "active_ownership_token": "token-ref"
            },
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": null,
            "fact_sources": ["session_token_ref:token-ref"]
        });
        let snapshot =
            serde_json::from_value::<RepoopsAuthoritySnapshot>(valid_claim_bound_snapshot.clone())
                .expect("valid claim-bound snapshot with fact sources");
        assert_eq!(snapshot.fact_sources(), ["session_token_ref:token-ref"]);
        assert_eq!(
            serde_json::to_value(&snapshot).expect("serialize snapshot"),
            valid_claim_bound_snapshot
        );

        let common_with_blank_schema = RepoopsAuthoritySnapshotCommon::new(
            String::new(),
            "worker-1".to_owned(),
            RepoopsAuthorityPolicyFact::enforce(),
            RepoopsAuthorityScopeFact::new(vec!["src/**".to_owned()], Vec::new())
                .expect("valid scope"),
            Vec::new(),
            None,
            Vec::new(),
        );
        common_with_blank_schema.expect_err("snapshot common must reject blank schema_version");

        let common_with_padded_fact_source = RepoopsAuthoritySnapshotCommon::new(
            "covey_repoops_authority_snapshot.v1".to_owned(),
            "worker-1".to_owned(),
            RepoopsAuthorityPolicyFact::enforce(),
            RepoopsAuthorityScopeFact::new(vec!["src/**".to_owned()], Vec::new())
                .expect("valid scope"),
            Vec::new(),
            None,
            vec!["session_token_ref:token-ref ".to_owned()],
        );
        common_with_padded_fact_source
            .expect_err("snapshot common must reject padded fact sources");

        let padded_schema_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1 ",
            "agent_id": "worker-1",
            "claim_id": null,
            "ownership_token": null,
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": null,
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": "claim unavailable",
            "fact_sources": []
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(padded_schema_snapshot)
            .expect_err("snapshot serde must reject padded schema versions");

        let unknown_schema_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v2",
            "agent_id": "worker-1",
            "claim_id": null,
            "ownership_token": null,
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": null,
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": "claim unavailable",
            "fact_sources": []
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(unknown_schema_snapshot)
            .expect_err("snapshot serde must reject unknown schema versions");

        let padded_agent_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": " worker-1",
            "claim_id": null,
            "ownership_token": null,
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": null,
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": "claim unavailable",
            "fact_sources": []
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(padded_agent_snapshot)
            .expect_err("snapshot serde must reject padded agent ids");

        let invalid_agent_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker 1",
            "claim_id": null,
            "ownership_token": null,
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": null,
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": "claim unavailable",
            "fact_sources": []
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(invalid_agent_snapshot)
            .expect_err("snapshot serde must reject non-token agent ids");

        let blank_fact_source_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": null,
            "ownership_token": null,
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": null,
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": "claim unavailable",
            "fact_sources": [""]
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(blank_fact_source_snapshot)
            .expect_err("snapshot serde must reject blank fact sources");

        let padded_fact_source_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": null,
            "ownership_token": null,
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": null,
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": "claim unavailable",
            "fact_sources": [" session_token_ref:token-ref"]
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(padded_fact_source_snapshot)
            .expect_err("snapshot serde must reject padded fact sources");

        let control_fact_source_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": null,
            "ownership_token": null,
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": null,
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": "claim unavailable",
            "fact_sources": ["session_token_ref:token\nref"]
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(control_fact_source_snapshot)
            .expect_err("snapshot serde must reject control characters in fact sources");

        let duplicate_fact_source_snapshot = serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": null,
            "ownership_token": null,
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": null,
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": [],
            "git_context": null,
            "constraint_reason": "claim unavailable",
            "fact_sources": ["session_token_ref:token-ref", "session_token_ref:token-ref"]
        });
        serde_json::from_value::<RepoopsAuthoritySnapshot>(duplicate_fact_source_snapshot)
            .expect_err("snapshot serde must reject duplicate fact sources");
    }

    #[test]
    fn repoops_authority_git_context_rejects_partial_or_invalid_path_facts() {
        let unknown = RepoopsAuthorityGitContextFact::unknown(true);
        assert_eq!(unknown.policy_project_path(), None);
        assert_eq!(unknown.execution_project_path(), None);
        assert_eq!(unknown.repo_path_prefix(), None);
        assert!(unknown.ownership_token_required());
        let unknown_json = serde_json::to_value(&unknown).expect("serialize unknown context");
        assert_eq!(unknown_json["policy_project_path"], serde_json::Value::Null);
        assert_eq!(
            unknown_json["ownership_token_required"],
            serde_json::Value::Bool(true)
        );

        let known = RepoopsAuthorityGitContextFact::known_paths(
            "/repo",
            "/repo/nested",
            Some("nested".to_owned()),
            true,
        )
        .expect("valid concrete git context");
        assert_eq!(known.policy_project_path(), Some("/repo"));
        assert_eq!(known.execution_project_path(), Some("/repo/nested"));
        assert_eq!(known.repo_path_prefix(), Some("nested"));

        let partial_paths = serde_json::json!({
            "policy_project_path": "/repo",
            "execution_project_path": null,
            "repo_path_prefix": null,
            "ownership_token_required": true
        });
        serde_json::from_value::<RepoopsAuthorityGitContextFact>(partial_paths)
            .expect_err("git context must not carry only one project path");

        let prefix_without_paths = serde_json::json!({
            "policy_project_path": null,
            "execution_project_path": null,
            "repo_path_prefix": "nested",
            "ownership_token_required": true
        });
        serde_json::from_value::<RepoopsAuthorityGitContextFact>(prefix_without_paths)
            .expect_err("git context prefix requires concrete project paths");

        let padded_project_path = serde_json::json!({
            "policy_project_path": "/repo ",
            "execution_project_path": "/repo/nested",
            "repo_path_prefix": null,
            "ownership_token_required": true
        });
        serde_json::from_value::<RepoopsAuthorityGitContextFact>(padded_project_path)
            .expect_err("git context project paths must be normalized");

        let traversing_prefix = serde_json::json!({
            "policy_project_path": "/repo",
            "execution_project_path": "/repo/nested",
            "repo_path_prefix": "nested/../other",
            "ownership_token_required": true
        });
        serde_json::from_value::<RepoopsAuthorityGitContextFact>(traversing_prefix)
            .expect_err("git context prefix must be normalized");
    }

    #[test]
    fn repoops_authority_snapshot_rejects_lock_subject_mismatches() {
        let owned_by_other_claim = claim_bound_snapshot_with_locks(serde_json::json!([{
            "path": "src/lib.rs",
            "owner": "worker-1",
            "claim_id": "claim-other",
            "status": "owned"
        }]));
        serde_json::from_value::<RepoopsAuthoritySnapshot>(owned_by_other_claim)
            .expect_err("owned lock facts must match the snapshot claim id");

        let foreign_with_current_claim = claim_bound_snapshot_with_locks(serde_json::json!([{
            "path": "src/lib.rs",
            "owner": "worker-2",
            "claim_id": "claim-1",
            "status": "foreign_owner"
        }]));
        serde_json::from_value::<RepoopsAuthoritySnapshot>(foreign_with_current_claim)
            .expect_err("foreign lock facts must not reuse the snapshot claim id");

        let foreign_with_current_owner = claim_bound_snapshot_with_locks(serde_json::json!([{
            "path": "src/lib.rs",
            "owner": "worker-1",
            "claim_id": "claim-other",
            "status": "foreign_owner"
        }]));
        serde_json::from_value::<RepoopsAuthoritySnapshot>(foreign_with_current_owner)
            .expect_err("foreign lock facts must not reuse the snapshot owner");

        let constrained_owned_lock = constrained_snapshot_with_locks(serde_json::json!([{
            "path": "src/lib.rs",
            "owner": "worker-1",
            "claim_id": "claim-1",
            "status": "owned"
        }]));
        serde_json::from_value::<RepoopsAuthoritySnapshot>(constrained_owned_lock)
            .expect_err("constrained snapshots must not include owned locks");

        let constrained_agent_owned_foreign_lock =
            constrained_snapshot_with_locks(serde_json::json!([{
                "path": "src/lib.rs",
                "owner": "worker-1",
                "claim_id": "claim-other",
                "status": "foreign_owner"
            }]));
        serde_json::from_value::<RepoopsAuthoritySnapshot>(constrained_agent_owned_foreign_lock)
            .expect_err("constrained snapshots must not include locks owned by the agent");
    }

    #[test]
    fn repoops_authority_scope_fact_rejects_invalid_patterns() {
        let valid_scope = serde_json::json!({
            "in": ["src/**"],
            "out": ["target/**"]
        });
        let scope = serde_json::from_value::<RepoopsAuthorityScopeFact>(valid_scope)
            .expect("valid repoops scope fact");
        assert_eq!(scope.scope_in(), ["src/**"]);
        assert_eq!(scope.scope_out(), ["target/**"]);
        let scope_json = serde_json::to_value(&scope).expect("serialize repoops scope fact");
        assert_eq!(
            scope_json,
            serde_json::json!({
                "in": ["src/**"],
                "out": ["target/**"]
            })
        );

        let blank_scope_in = serde_json::json!({
            "in": [" "],
            "out": []
        });
        serde_json::from_value::<RepoopsAuthorityScopeFact>(blank_scope_in)
            .expect_err("repoops scope inclusion patterns must not be blank");

        let padded_scope_out = serde_json::json!({
            "in": [],
            "out": [" target/**"]
        });
        serde_json::from_value::<RepoopsAuthorityScopeFact>(padded_scope_out)
            .expect_err("repoops scope exclusion patterns must be normalized");

        let duplicate_scope_in = serde_json::json!({
            "in": ["src/**", "src/**"],
            "out": []
        });
        serde_json::from_value::<RepoopsAuthorityScopeFact>(duplicate_scope_in)
            .expect_err("repoops scope inclusion patterns must be unique");
    }

    #[test]
    fn repoops_authority_claim_fact_rejects_invalid_contract_fields() {
        let blank_owner = serde_json::json!({
            "claim_id": "claim-1",
            "status": "open",
            "owner": " ",
            "scope_in": [],
            "scope_out": [],
            "has_required_contract_fields": false,
            "active_ownership_token": null
        });
        serde_json::from_value::<RepoopsAuthorityClaimFact>(blank_owner)
            .expect_err("repoops claim facts must carry a non-blank owner");

        let invalid_owner_principal = serde_json::json!({
            "claim_id": "claim-1",
            "status": "open",
            "owner": "worker 1",
            "scope_in": [],
            "scope_out": [],
            "has_required_contract_fields": false,
            "active_ownership_token": null
        });
        serde_json::from_value::<RepoopsAuthorityClaimFact>(invalid_owner_principal)
            .expect_err("repoops claim owner must be a valid agent principal id");

        let duplicate_scope = serde_json::json!({
            "claim_id": "claim-1",
            "status": "open",
            "owner": "worker-1",
            "scope_in": ["src/**", "src/**"],
            "scope_out": [],
            "has_required_contract_fields": true,
            "active_ownership_token": null
        });
        serde_json::from_value::<RepoopsAuthorityClaimFact>(duplicate_scope)
            .expect_err("repoops claim scope patterns must be unique");

        let missing_required_scope = serde_json::json!({
            "claim_id": "claim-1",
            "status": "open",
            "owner": "worker-1",
            "scope_in": [],
            "scope_out": [],
            "has_required_contract_fields": true,
            "active_ownership_token": null
        });
        serde_json::from_value::<RepoopsAuthorityClaimFact>(missing_required_scope)
            .expect_err("repoops claim facts cannot mark missing scope as contract-complete");

        let missing_scope = serde_json::json!({
            "claim_id": "claim-1",
            "status": "open",
            "owner": "worker-1",
            "scope_in": [],
            "scope_out": [],
            "has_required_contract_fields": false,
            "active_ownership_token": null
        });
        let claim = serde_json::from_value::<RepoopsAuthorityClaimFact>(missing_scope)
            .expect("missing scope is represented explicitly for authority denial");
        assert!(!claim.has_required_contract_fields());

        let valid_claim = serde_json::json!({
            "claim_id": "claim-1",
            "status": "in_progress",
            "owner": "worker-1",
            "scope_in": ["src/**"],
            "scope_out": ["target/**"],
            "has_required_contract_fields": true,
            "active_ownership_token": "token-ref"
        });
        let claim = serde_json::from_value::<RepoopsAuthorityClaimFact>(valid_claim)
            .expect("valid repoops claim fact");
        assert_eq!(claim.scope_in(), ["src/**"]);
        assert_eq!(claim.scope_out(), ["target/**"]);
        let claim_json = serde_json::to_value(&claim).expect("serialize repoops claim fact");
        assert_eq!(
            claim_json,
            serde_json::json!({
                "claim_id": "claim-1",
                "status": "in_progress",
                "owner": "worker-1",
                "scope_in": ["src/**"],
                "scope_out": ["target/**"],
                "has_required_contract_fields": true,
                "active_ownership_token": "token-ref"
            })
        );
    }

    fn claim_bound_snapshot_with_locks(locks: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": "claim-1",
            "ownership_token": "token-ref",
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": {
                "claim_id": "claim-1",
                "status": "in_progress",
                "owner": "worker-1",
                "scope_in": ["src/**"],
                "scope_out": [],
                "has_required_contract_fields": true,
                "active_ownership_token": "token-ref"
            },
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": locks,
            "git_context": null,
            "constraint_reason": null,
            "fact_sources": []
        })
    }

    fn constrained_snapshot_with_locks(locks: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "schema_version": "covey_repoops_authority_snapshot.v1",
            "agent_id": "worker-1",
            "claim_id": null,
            "ownership_token": null,
            "override_token": null,
            "policy": {
                "mode": "enforce",
                "phase": 2,
                "denied_rule_id": null
            },
            "claim": null,
            "scope": {
                "in": ["src/**"],
                "out": []
            },
            "locks": locks,
            "git_context": null,
            "constraint_reason": "claim unavailable",
            "fact_sources": []
        })
    }
}

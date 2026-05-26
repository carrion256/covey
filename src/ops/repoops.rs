#![cfg_attr(coverage_nightly, coverage(off))]

use std::{borrow::Cow, collections::HashMap, time::Instant};

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
    queries::{load_repoops_relevant_reservations_tx, load_subtask_tx},
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
            let session = crate::queries::load_session_tx(tx, &req.session_token)?;
            let paths = normalize_requested_paths(req.paths());
            let reservations =
                load_repoops_relevant_reservations_tx(tx, now, &claim.subtask_id, &paths)?;
            let prepared_reservations = prepare_reservations(&reservations);
            let scope_in = scope_patterns_for_subtask(&prepared_reservations, &claim.subtask_id);
            let scope = RepoopsAuthorityScopeFact::new(scope_in.clone(), Vec::new())
                .map_err(|reason| crate::CoveyError::InvalidObservabilityRow { reason })?;
            let locks =
                lock_facts_for_prepared_paths(&paths, &prepared_reservations, &claim, &session)
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
    const PREFIX: &str = "covey-session-token-blake3:";
    let digest = blake3::hash(session_token.as_bytes()).to_hex();
    let mut token_ref = String::with_capacity(PREFIX.len() + digest.len());
    token_ref.push_str(PREFIX);
    token_ref.push_str(digest.as_str());
    crate::model::SessionToken::parse(token_ref)
        .expect("blake3-derived ownership token ref should be token-safe")
}

fn normalize_requested_paths(paths: &[crate::model::RepoopsPath]) -> Vec<String> {
    if let [path] = paths {
        return vec![path.as_str().to_owned()];
    }
    let mut normalized = paths
        .iter()
        .map(|path| path.as_str().to_owned())
        .collect::<Vec<_>>();
    sort_dedup_strings(&mut normalized);
    normalized
}

fn sort_dedup_strings(values: &mut Vec<String>) {
    if values.len() > 1 {
        values.sort_unstable();
        values.dedup();
    }
}

fn normalize_repo_relative_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if repo_relative_path_is_normalized(trimmed) {
        return Some(trimmed.to_owned());
    }
    let mut normalized = if trimmed.as_bytes().contains(&b'\\') {
        Cow::Owned(trimmed.replace('\\', "/"))
    } else {
        Cow::Borrowed(trimmed)
    };
    while let Some(rest) = normalized.strip_prefix("./") {
        normalized = Cow::Owned(rest.to_owned());
    }
    while let Some(rest) = normalized.strip_prefix('/') {
        normalized = Cow::Owned(rest.to_owned());
    }
    let mut parts = Vec::with_capacity(normalized.bytes().filter(|byte| *byte == b'/').count() + 1);
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

fn repo_relative_path_is_normalized(path: &str) -> bool {
    !path.as_bytes().contains(&b'\\')
        && !path.starts_with('/')
        && !path.starts_with("./")
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn prepare_reservations(reservations: &[Reservation]) -> Vec<PreparedReservation<'_>> {
    reservations
        .iter()
        .map(|reservation| PreparedReservation {
            reservation,
            scope: PreparedScope::from_reservation(reservation),
        })
        .collect()
}

struct PreparedReservation<'a> {
    reservation: &'a Reservation,
    scope: PreparedScope,
}

enum PreparedScope {
    RepoGlobal,
    ExactPath(Option<String>),
    Subtree(Option<PreparedSubtreeScope>),
    GeneratedSet(Vec<String>),
}

struct PreparedSubtreeScope {
    base: String,
    child_prefix: String,
}

struct PreparedReservationIndex<'p, 'r> {
    repo_global: Vec<&'p PreparedReservation<'r>>,
    exact_paths: HashMap<&'p str, Vec<&'p PreparedReservation<'r>>>,
    subtrees: Vec<&'p PreparedReservation<'r>>,
}

impl PreparedScope {
    fn from_reservation(reservation: &Reservation) -> Self {
        match reservation.scope_class() {
            ScopeClass::RepoGlobal => Self::RepoGlobal,
            ScopeClass::ExactPath => {
                Self::ExactPath(normalize_repo_relative_path(reservation.scope_key()))
            }
            ScopeClass::Subtree => Self::Subtree(
                normalize_repo_relative_path(reservation.scope_key()).map(|base| {
                    PreparedSubtreeScope {
                        child_prefix: format!("{base}/"),
                        base,
                    }
                }),
            ),
            ScopeClass::GeneratedSet => {
                let mut members = reservation
                    .generated_member_strs()
                    .filter_map(normalize_repo_relative_path)
                    .collect::<Vec<_>>();
                sort_dedup_strings(&mut members);
                Self::GeneratedSet(members)
            }
        }
    }

    fn append_scope_patterns(&self, patterns: &mut Vec<String>) {
        match self {
            Self::RepoGlobal => patterns.push("**".to_owned()),
            Self::ExactPath(Some(path)) => patterns.push(path.clone()),
            Self::ExactPath(None) => {}
            Self::Subtree(Some(scope)) => patterns.push(format!("{}/**", scope.base)),
            Self::Subtree(None) => {}
            Self::GeneratedSet(members) => patterns.extend(members.iter().cloned()),
        }
    }

    #[cfg(test)]
    fn covers_path(&self, path: &str) -> bool {
        match self {
            Self::RepoGlobal => true,
            Self::ExactPath(candidate) => candidate.as_deref() == Some(path),
            Self::Subtree(scope) => scope.as_ref().is_some_and(|scope| scope.covers_path(path)),
            Self::GeneratedSet(members) => members
                .binary_search_by(|member| member.as_str().cmp(path))
                .is_ok(),
        }
    }

    fn subtree_scope(&self) -> Option<&PreparedSubtreeScope> {
        match self {
            Self::Subtree(Some(scope)) => Some(scope),
            _ => None,
        }
    }
}

impl PreparedSubtreeScope {
    fn covers_path(&self, path: &str) -> bool {
        path == self.base || path.starts_with(&self.child_prefix)
    }
}

impl<'p, 'r> PreparedReservationIndex<'p, 'r> {
    fn from_prepared(reservations: &'p [PreparedReservation<'r>]) -> Self {
        let exact_path_capacity = reservations
            .iter()
            .map(|prepared| match &prepared.scope {
                PreparedScope::ExactPath(Some(_)) => 1,
                PreparedScope::GeneratedSet(members) => members.len(),
                _ => 0,
            })
            .sum();
        let mut index = Self {
            repo_global: Vec::new(),
            exact_paths: HashMap::with_capacity(exact_path_capacity),
            subtrees: Vec::new(),
        };
        for prepared in reservations {
            match &prepared.scope {
                PreparedScope::RepoGlobal => index.repo_global.push(prepared),
                PreparedScope::ExactPath(Some(path)) => {
                    index.exact_paths.entry(path).or_default().push(prepared);
                }
                PreparedScope::ExactPath(None) => {}
                PreparedScope::Subtree(Some(_)) => index.subtrees.push(prepared),
                PreparedScope::Subtree(None) => {}
                PreparedScope::GeneratedSet(members) => {
                    for path in members {
                        index
                            .exact_paths
                            .entry(path.as_str())
                            .or_default()
                            .push(prepared);
                    }
                }
            }
        }
        index
    }

    fn matching_reservations(
        &'p self,
        path: &'p str,
    ) -> impl Iterator<Item = &'p PreparedReservation<'r>> + 'p {
        let exact = self
            .exact_paths
            .get(path)
            .into_iter()
            .flat_map(|reservations| reservations.iter().copied());
        let subtrees = self.subtrees.iter().copied().filter(move |prepared| {
            prepared
                .scope
                .subtree_scope()
                .is_some_and(|scope| scope.covers_path(path))
        });
        self.repo_global
            .iter()
            .copied()
            .chain(exact)
            .chain(subtrees)
    }
}

fn scope_patterns_for_subtask(
    reservations: &[PreparedReservation<'_>],
    subtask_id: &str,
) -> Vec<String> {
    let mut patterns = Vec::new();
    for prepared in reservations {
        if prepared.reservation.owner_subtask_id == subtask_id {
            prepared.scope.append_scope_patterns(&mut patterns);
        }
    }
    sort_dedup_strings(&mut patterns);
    patterns
}

#[cfg(test)]
fn scope_patterns_for_reservation(reservation: &Reservation) -> Vec<String> {
    let mut patterns = Vec::new();
    PreparedScope::from_reservation(reservation).append_scope_patterns(&mut patterns);
    patterns
}

#[cfg(test)]
fn lock_facts_for_paths(
    paths: &[String],
    reservations: &[Reservation],
    claim: &Claim,
    session: &Session,
) -> std::result::Result<Vec<RepoopsAuthorityLockFact>, String> {
    let prepared_reservations = prepare_reservations(reservations);
    lock_facts_for_prepared_paths(paths, &prepared_reservations, claim, session)
}

fn lock_facts_for_prepared_paths(
    paths: &[String],
    reservations: &[PreparedReservation<'_>],
    claim: &Claim,
    session: &Session,
) -> std::result::Result<Vec<RepoopsAuthorityLockFact>, String> {
    let mut locks = Vec::new();
    let reservation_index = PreparedReservationIndex::from_prepared(reservations);
    for path in paths {
        for prepared in reservation_index.matching_reservations(path) {
            let reservation = prepared.reservation;
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
    if locks.len() > 1 {
        locks.sort_unstable_by(|left, right| {
            (left.path(), left.owner(), left.claim_id()).cmp(&(
                right.path(),
                right.owner(),
                right.claim_id(),
            ))
        });
        locks.dedup();
    }
    Ok(locks)
}

#[cfg(test)]
fn lock_facts_for_prepared_paths_nested(
    paths: &[String],
    reservations: &[PreparedReservation<'_>],
    claim: &Claim,
    session: &Session,
) -> std::result::Result<Vec<RepoopsAuthorityLockFact>, String> {
    let mut locks = Vec::new();
    for path in paths {
        for prepared in reservations {
            if !prepared.scope.covers_path(path) {
                continue;
            }
            let reservation = prepared.reservation;
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
    if locks.len() > 1 {
        locks.sort_unstable_by(|left, right| {
            (left.path(), left.owner(), left.claim_id()).cmp(&(
                right.path(),
                right.owner(),
                right.claim_id(),
            ))
        });
        locks.dedup();
    }
    Ok(locks)
}

fn repoops_claim_ref(value: impl Into<String>) -> RepoopsClaimRef {
    RepoopsClaimRef::parse(value).expect("repoops claim refs are derived from validated ids")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ClaimId, ClaimState, FenceSeq, LeaseDeadlineMs, RepoopsAuthorityLockStatus, RepoopsPath,
        ReservationId, ReservationState, SessionToken, SubtaskId, SubtaskState, TimestampMs,
    };

    fn reservation(
        scope_class: ScopeClass,
        scope_key: &str,
        owner_subtask_id: &str,
    ) -> Reservation {
        reservation_with_members(scope_class, scope_key, owner_subtask_id, Vec::new())
    }

    fn reservation_with_members(
        scope_class: ScopeClass,
        scope_key: &str,
        owner_subtask_id: &str,
        generated_members: Vec<String>,
    ) -> Reservation {
        Reservation::try_from_parts(
            ReservationId::parse("reservation-1").expect("valid reservation id"),
            SubtaskId::parse(owner_subtask_id).expect("valid subtask id"),
            scope_class,
            scope_key,
            generated_members,
            LeaseDeadlineMs::parse(1_000).expect("valid lease deadline"),
            ReservationState::Active,
            TimestampMs::parse(1).expect("valid timestamp"),
            TimestampMs::parse(1).expect("valid timestamp"),
        )
        .expect("valid reservation fixture")
    }

    fn claim_fixture() -> Claim {
        Claim::try_from_parts(
            ClaimId::parse("claim-1").expect("valid claim id"),
            SubtaskId::parse("task-1").expect("valid subtask id"),
            SessionToken::parse("session-1").expect("valid session token"),
            FenceSeq::parse(7).expect("valid fence"),
            LeaseDeadlineMs::parse(1_000).expect("valid lease deadline"),
            ClaimState::Held,
            TimestampMs::parse(1).expect("valid timestamp"),
            TimestampMs::parse(1).expect("valid timestamp"),
        )
        .expect("valid claim")
    }

    fn session_fixture() -> Session {
        Session::try_from_parts(
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
        .expect("valid active session fixture")
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
        assert_eq!(
            scope_patterns_for_reservation(&reservation_with_members(
                ScopeClass::GeneratedSet,
                "generated",
                "task-1",
                vec!["./src/generated.rs".to_owned(), "src/other.rs".to_owned()]
            )),
            vec!["src/generated.rs".to_owned(), "src/other.rs".to_owned()]
        );
    }

    #[test]
    fn repo_relative_path_fast_path_matches_canonicalization_shape() {
        assert!(repo_relative_path_is_normalized("src/lib.rs"));
        assert_eq!(
            normalize_repo_relative_path("src/lib.rs").as_deref(),
            Some("src/lib.rs")
        );

        for raw in ["./src/lib.rs", "/src/lib.rs", "src//lib.rs", "src/./lib.rs"] {
            assert!(!repo_relative_path_is_normalized(raw));
            assert_eq!(
                normalize_repo_relative_path(raw).as_deref(),
                Some("src/lib.rs")
            );
        }

        assert!(!repo_relative_path_is_normalized("src/../lib.rs"));
        assert_eq!(normalize_repo_relative_path("src/../lib.rs"), None);
    }

    #[test]
    fn ownership_token_ref_preserves_blake3_token_shape() {
        let expected = format!("covey-session-token-blake3:{}", blake3::hash(b"session-1"));
        assert_eq!(ownership_token_ref("session-1").as_str(), expected);
    }

    #[test]
    fn requested_path_single_fast_path_preserves_multi_path_dedup() {
        let single = vec![RepoopsPath::parse("src/lib.rs").expect("valid repoops path")];
        assert_eq!(
            normalize_requested_paths(&single),
            vec!["src/lib.rs".to_owned()]
        );

        let duplicate = vec![
            RepoopsPath::parse("src/main.rs").expect("valid repoops path"),
            RepoopsPath::parse("src/lib.rs").expect("valid repoops path"),
            RepoopsPath::parse("src/main.rs").expect("valid repoops path"),
        ];
        assert_eq!(
            normalize_requested_paths(&duplicate),
            vec!["src/lib.rs".to_owned(), "src/main.rs".to_owned()]
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
    fn lock_facts_cover_prepared_subtree_and_generated_reservations() {
        let claim = claim_fixture();
        let session = session_fixture();
        let reservations = vec![
            reservation(ScopeClass::Subtree, "src", "task-1"),
            reservation_with_members(
                ScopeClass::GeneratedSet,
                "generated",
                "task-2",
                vec!["./generated/out.rs".to_owned()],
            ),
        ];

        let locks = lock_facts_for_paths(
            &["src/lib.rs".to_owned(), "generated/out.rs".to_owned()],
            &reservations,
            &claim,
            &session,
        )
        .expect("valid lock facts");

        assert!(locks.iter().any(|lock| {
            lock.path() == "src/lib.rs" && lock.status() == RepoopsAuthorityLockStatus::Owned
        }));
        assert!(locks.iter().any(|lock| {
            lock.path() == "generated/out.rs"
                && lock.status() == RepoopsAuthorityLockStatus::ForeignOwner
        }));
    }

    #[test]
    fn prepared_reservation_index_matches_nested_lock_facts() {
        let claim = claim_fixture();
        let session = session_fixture();
        let reservations = vec![
            reservation(ScopeClass::RepoGlobal, "repo", "task-3"),
            reservation(ScopeClass::ExactPath, "./src/lib.rs", "task-1"),
            reservation(ScopeClass::ExactPath, "src/lib.rs", "task-4"),
            reservation(ScopeClass::Subtree, "src", "task-5"),
            reservation_with_members(
                ScopeClass::GeneratedSet,
                "generated",
                "task-1",
                vec![
                    "./generated/out.rs".to_owned(),
                    "generated/other.rs".to_owned(),
                ],
            ),
            reservation_with_members(
                ScopeClass::GeneratedSet,
                "generated",
                "task-6",
                vec!["generated/out.rs".to_owned(), "src/lib.rs".to_owned()],
            ),
        ];
        let paths = vec![
            "src/lib.rs".to_owned(),
            "src/nested/mod.rs".to_owned(),
            "generated/out.rs".to_owned(),
            "unmatched/file.rs".to_owned(),
        ];
        let prepared = prepare_reservations(&reservations);

        let indexed = lock_facts_for_prepared_paths(&paths, &prepared, &claim, &session)
            .expect("indexed lock facts");
        let nested = lock_facts_for_prepared_paths_nested(&paths, &prepared, &claim, &session)
            .expect("nested lock facts");

        assert_eq!(indexed, nested);
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

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    error::{CoveyError, Result},
    model::{
        ArtifactDigest, Claim, ClaimState, ConflictResolutionState, CoveyTypeValidationError,
        FenceSeq, MetaTaskState, ObjectType, ReadyQueueState, ReservationState, ReviewState,
        RuntimeAttestation, Session, SessionRole, SessionState, SessionToken, StateValue,
        SubtaskId, SubtaskKind, SubtaskState, claim_state_name, review_state_name,
    },
    queries::{
        load_artifact_tx, load_claim_tx, load_meta_task_tx, load_runtime_attestation_tx,
        load_session_tx, load_subtask_tx,
    },
};

pub(crate) const MAX_PROMPT_LEN: usize = 32 * 1024;
pub(crate) const MAX_TITLE_LEN: usize = 512;
pub(crate) const MAX_IDEMPOTENCY_KEY_LEN: usize = 256;
pub(crate) const MAX_AGENT_ID_LEN: usize = 512;
pub(crate) const MAX_OBJECT_ID_LEN: usize = 256;
pub(crate) const MAX_DIGEST_LEN: usize = 512;
pub(crate) const MAX_BASE_REV_LEN: usize = 512;
pub(crate) const MAX_PATH_LEN: usize = 4 * 1024;
pub(crate) const MAX_GENERATED_MEMBERS: usize = 1_024;
pub(crate) const MAX_RUNTIME_FIELD_LEN: usize = 512;
/// Upper bound on the intake thread identifier of a meta-task source backlink.
pub(crate) const MAX_SOURCE_THREAD_ID_LEN: usize = 512;
/// Upper bound on the intake receipt stamp of a meta-task source backlink.
pub(crate) const MAX_SOURCE_RECEIVED_AT_LEN: usize = 128;
/// Upper bound on the optional source URL of a meta-task source backlink.
pub(crate) const MAX_SOURCE_URL_LEN: usize = 2 * 1024;
/// Upper bound on the recovery method named by a backfilled source link.
pub(crate) const MAX_SOURCE_METHOD_LEN: usize = 256;

/// Rejects a blank or oversized source-backlink field with a typed error.
pub(crate) fn ensure_source_field(
    field: &'static str,
    value: &str,
    max: usize,
) -> std::result::Result<(), CoveyTypeValidationError> {
    if value.trim().is_empty() {
        return Err(CoveyTypeValidationError::new(field, "must not be empty"));
    }
    if value.len() > max {
        return Err(CoveyTypeValidationError::new(
            field,
            "exceeds the maximum length for a source backlink field",
        ));
    }
    Ok(())
}

/// Rejects a source path that is not absolute.
pub(crate) fn ensure_source_path(path: &str) -> std::result::Result<(), CoveyTypeValidationError> {
    ensure_source_field("path", path, MAX_PATH_LEN)?;
    if !path.starts_with('/') {
        return Err(CoveyTypeValidationError::new(
            "path",
            "must be an absolute path",
        ));
    }
    Ok(())
}

pub(crate) fn require_session(tx: &Transaction<'_>, session_token: &str) -> Result<Session> {
    load_session_tx(tx, session_token)
}

pub(crate) fn require_active_session(tx: &Transaction<'_>, session_token: &str) -> Result<Session> {
    let session = require_session(tx, session_token)?;
    if session.state() == SessionState::Active {
        Ok(session)
    } else {
        let state = session.state();
        Err(CoveyError::SessionNotActive {
            session_token: session.session_token,
            state,
        })
    }
}

pub(crate) fn require_role(
    tx: &Transaction<'_>,
    session_token: &str,
    expected: &[SessionRole],
) -> Result<Session> {
    let session = require_active_session(tx, session_token)?;
    if expected.contains(&session.role) {
        Ok(session)
    } else {
        Err(CoveyError::WrongRole {
            expected: expected.to_vec(),
            actual: session.role,
        })
    }
}

pub(crate) fn require_runtime_attestation(
    tx: &Transaction<'_>,
    session: &Session,
) -> Result<RuntimeAttestation> {
    let attestation = load_runtime_attestation_tx(tx, &session.session_token)?;
    if attestation.agent_principal_id != session.agent_principal_id
        || attestation.agent_instance_id != session.agent_instance_id
        || attestation.role != session.role
    {
        return Err(CoveyError::InvalidRuntimeAttestation {
            session_token: session.session_token.clone(),
            reason: "attestation identity does not match the session identity".to_owned(),
        });
    }
    if attestation.provider_run_identity_missing() {
        return Err(CoveyError::InvalidRuntimeAttestation {
            session_token: session.session_token.clone(),
            reason: "provider run identity is required".to_owned(),
        });
    }
    Ok(attestation)
}

pub(crate) fn ensure_meta_task_exists(tx: &Transaction<'_>, meta_task_id: &str) -> Result<()> {
    load_meta_task_tx(tx, meta_task_id).map(|_| ())
}

pub(crate) fn ensure_meta_task_is_schedulable(
    tx: &Transaction<'_>,
    meta_task_id: &str,
) -> Result<()> {
    let meta_task = load_meta_task_tx(tx, meta_task_id)?;
    if matches!(
        meta_task.state(),
        MetaTaskState::Completed | MetaTaskState::Cancelled
    ) {
        Err(CoveyError::MetaTaskUnavailable {
            meta_task_id: meta_task_id.to_owned(),
            state: meta_task.state(),
        })
    } else {
        Ok(())
    }
}

pub(crate) fn ensure_subtask_exists(tx: &Transaction<'_>, subtask_id: &str) -> Result<()> {
    load_subtask_tx(tx, subtask_id).map(|_| ())
}

pub(crate) fn ensure_artifact_exists(tx: &Transaction<'_>, artifact_digest: &str) -> Result<()> {
    match load_artifact_tx(tx, artifact_digest) {
        Ok(_) => Ok(()),
        Err(CoveyError::ArtifactNotFound) => Err(CoveyError::UnknownArtifactDigest {
            digest: artifact_digest.to_owned(),
        }),
        Err(err) => Err(err),
    }
}

pub(crate) fn ensure_artifact_digest_unused(
    tx: &Transaction<'_>,
    artifact_digest: &str,
) -> Result<()> {
    let exists = tx
        .query_row(
            "SELECT 1 FROM artifacts WHERE artifact_digest = ?1",
            params![artifact_digest],
            |_| Ok(()),
        )
        .optional()?;
    if exists.is_some() {
        return Err(CoveyError::ArtifactDigestCollision {
            digest: artifact_digest.to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn subtask_exists(tx: &Transaction<'_>, subtask_id: &str) -> Result<bool> {
    let exists = tx
        .query_row(
            "SELECT 1 FROM subtasks WHERE subtask_id = ?1",
            params![subtask_id],
            |_| Ok(()),
        )
        .optional()?;
    Ok(exists.is_some())
}

/// Upper bound on dependency edges one created subtask may declare.
pub(crate) const MAX_SUBTASK_DEPENDENCIES: usize = 64;

/// Collapses repeat dependency ids while keeping first-seen order, so the same
/// dependency set always produces the same request bytes and one row per edge.
pub(crate) fn dedupe_subtask_dependencies(dependencies: &[SubtaskId]) -> Vec<SubtaskId> {
    let mut deduped: Vec<SubtaskId> = Vec::with_capacity(dependencies.len());
    for dependency in dependencies {
        if !deduped.contains(dependency) {
            deduped.push(dependency.clone());
        }
    }
    deduped
}

/// Normalizes and validates the dependency list of a subtask being created.
///
/// A subtask created here has no dependents yet, so a dependency cycle cannot
/// appear at create time; the existing claim-time satisfaction gate is the only
/// place ordering is enforced. Dependencies in any state and on any other
/// meta-task are accepted: no extra governance is applied here.
pub(crate) fn normalize_subtask_dependencies(
    subtask_id: &SubtaskId,
    dependencies: &[SubtaskId],
) -> Result<Vec<SubtaskId>> {
    let deduped = dedupe_subtask_dependencies(dependencies);
    if deduped.len() > MAX_SUBTASK_DEPENDENCIES {
        return Err(CoveyError::InvalidSubtaskDependency {
            reason: format!(
                "subtask {subtask_id} declares {} dependencies; the maximum is \
                 {MAX_SUBTASK_DEPENDENCIES}",
                deduped.len()
            ),
        });
    }
    if deduped.iter().any(|dependency| dependency == subtask_id) {
        return Err(CoveyError::InvalidSubtaskDependency {
            reason: format!("subtask {subtask_id} cannot depend on itself"),
        });
    }
    Ok(deduped)
}

pub(crate) fn held_claim_owner(
    tx: &Transaction<'_>,
    subtask_id: &str,
) -> Result<Option<SessionToken>> {
    tx.query_row(
        "SELECT owner_session_token FROM claims WHERE subtask_id = ?1 AND state = ?2",
        params![subtask_id, claim_state_name(ClaimState::Held)],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn require_session_can_claim_kind(session: &Session, kind: SubtaskKind) -> Result<()> {
    let allowed = match kind {
        SubtaskKind::Work => session.role == SessionRole::Executor,
        SubtaskKind::Review => session.role == SessionRole::Reviewer,
        SubtaskKind::Cleanup => session.role == SessionRole::Orchestrator,
    };
    if allowed {
        Ok(())
    } else {
        let expected = match kind {
            SubtaskKind::Work => vec![SessionRole::Executor],
            SubtaskKind::Review => vec![SessionRole::Reviewer],
            SubtaskKind::Cleanup => vec![SessionRole::Orchestrator],
        };
        Err(CoveyError::WrongRole {
            expected,
            actual: session.role,
        })
    }
}

pub(crate) fn require_session_can_request_review(session: &Session) -> Result<()> {
    if matches!(
        session.role,
        SessionRole::Executor | SessionRole::Orchestrator
    ) {
        Ok(())
    } else {
        Err(CoveyError::WrongRole {
            expected: vec![SessionRole::Executor, SessionRole::Orchestrator],
            actual: session.role,
        })
    }
}

pub(crate) fn require_session_can_enqueue(session: &Session) -> Result<()> {
    if matches!(
        session.role,
        SessionRole::Orchestrator | SessionRole::ApplyGate
    ) {
        Ok(())
    } else {
        Err(CoveyError::WrongRole {
            expected: vec![SessionRole::Orchestrator, SessionRole::ApplyGate],
            actual: session.role,
        })
    }
}

pub(crate) fn issue_fence_seq(tx: &Transaction<'_>, subtask_id: &str) -> Result<FenceSeq> {
    tx.query_row(
        r#"
        INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq)
        VALUES (?1, 2)
        ON CONFLICT(subtask_id) DO UPDATE SET next_fence_seq = next_fence_seq + 1
        RETURNING next_fence_seq - 1
        "#,
        params![subtask_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

/// Loads the claim a fenced call presents and checks that the caller still
/// owns it: the claim is `held`, by this session, under this fence.
///
/// A lapsed lease does not invalidate a held claim. The lease only decides
/// when the claim may be taken away (`expire_old_claims`, stale-session
/// reaping, the lazy expiry inside a claim). Until one of those has expired
/// it, the owner keeps every claim-fenced call. Afterwards the owner gets
/// [`CoveyError::ClaimNotHeld`], or [`CoveyError::SessionNotActive`] when the
/// reaping that expired the claim also retired the session: the session is
/// checked first.
pub(crate) fn require_current_claim(
    tx: &Transaction<'_>,
    session_token: &str,
    claim_id: &str,
    fence_seq: FenceSeq,
) -> Result<Claim> {
    let session = require_active_session(tx, session_token)?;
    let claim = load_claim_tx(tx, claim_id)?;
    if claim.owner_session_token != session.session_token {
        return Err(CoveyError::NotClaimOwner {
            session_token: session.session_token,
            claim_owner: claim.owner_session_token,
        });
    }
    if claim.fence_seq != fence_seq {
        return Err(CoveyError::StaleFenceToken {
            expected: claim.fence_seq.get(),
            provided: fence_seq.get(),
        });
    }
    if claim.state() != ClaimState::Held {
        let state = claim.state();
        return Err(CoveyError::ClaimNotHeld {
            claim_id: claim.claim_id,
            state,
        });
    }
    Ok(claim)
}

/// Refuses a held claim whose lease has run out. Only a call whose claim
/// stands for something that ends with the lease asks this; every other
/// fenced call accepts a held claim whatever its lease says (see
/// [`require_current_claim`]).
pub(crate) fn require_live_lease(claim: &Claim, now: i64) -> Result<()> {
    if claim.lease_deadline.get() <= now {
        return Err(CoveyError::LeaseExpired {
            object_id: claim.claim_id.to_string(),
        });
    }
    Ok(())
}

pub(crate) fn close_claim_and_detach(
    tx: &Transaction<'_>,
    claim: &Claim,
    state: ClaimState,
    now: i64,
) -> Result<()> {
    let rows = tx.execute(
        "UPDATE claims SET state = ?2, updated_at = ?3 WHERE claim_id = ?1 AND state = ?4",
        params![
            claim.claim_id,
            claim_state_name(state),
            now,
            claim_state_name(ClaimState::Held)
        ],
    )?;
    if rows != 1 {
        return Err(CoveyError::IllegalTransition {
            from: ClaimState::Held.into(),
            to: state.into(),
            object: ObjectType::Claim,
        });
    }
    Ok(())
}

pub(crate) fn clear_session_active_subtask(
    tx: &Transaction<'_>,
    session_token: &str,
    now: i64,
) -> Result<()> {
    tx.execute(
        "UPDATE sessions SET active_subtask_id = NULL, updated_at = ?2 WHERE session_token = ?1",
        params![session_token, now],
    )?;
    Ok(())
}

pub(crate) fn validate_idempotency_key(idempotency_key: &str) -> Result<()> {
    if idempotency_key.trim().is_empty() {
        return Err(CoveyError::InvalidIdempotencyKey {
            idempotency_key: idempotency_key.to_owned(),
        });
    }
    ensure_length("idempotency_key", idempotency_key, MAX_IDEMPOTENCY_KEY_LEN)?;
    Ok(())
}

pub(crate) fn ensure_length(field: &str, value: &str, max: usize) -> Result<()> {
    let actual = value.len();
    if actual > max {
        return Err(CoveyError::InputTooLarge {
            field: field.to_owned(),
            actual,
            max,
        });
    }
    Ok(())
}

pub(crate) fn ensure_non_empty(
    field: &str,
    value: &str,
    session_token: &SessionToken,
) -> Result<()> {
    if value.trim().is_empty() {
        return Err(CoveyError::InvalidRuntimeAttestation {
            session_token: session_token.clone(),
            reason: format!("{field} must not be empty"),
        });
    }
    Ok(())
}

pub(crate) fn ensure_generated_member_count(count: usize) -> Result<()> {
    if count > MAX_GENERATED_MEMBERS {
        return Err(CoveyError::InputTooLarge {
            field: "generated_members".to_owned(),
            actual: count,
            max: MAX_GENERATED_MEMBERS,
        });
    }
    Ok(())
}

pub(crate) fn ensure_positive_lease_duration(field: &str, value: i64) -> Result<()> {
    if value <= 0 {
        return Err(CoveyError::InvalidLeaseDuration {
            field: field.to_owned(),
            provided: value,
        });
    }
    Ok(())
}

pub(crate) fn ensure_no_open_review_round(
    tx: &Transaction<'_>,
    subtask_id: &str,
    artifact_digest: &str,
) -> Result<()> {
    let existing = tx
        .query_row(
            r#"
            SELECT 1
            FROM reviews
            WHERE subtask_id = ?1
              AND artifact_digest = ?2
              AND state IN (?3, ?4)
            "#,
            params![
                subtask_id,
                artifact_digest,
                review_state_name(ReviewState::Requested),
                review_state_name(ReviewState::InProgress)
            ],
            |_| Ok(()),
        )
        .optional()?;
    if existing.is_some() {
        Err(CoveyError::ReviewAlreadyOpen {
            subtask_id: SubtaskId::parse(subtask_id)?,
            artifact_digest: ArtifactDigest::parse(artifact_digest)?,
        })
    } else {
        Ok(())
    }
}

pub(crate) fn ensure_subtask_transition(
    kind: SubtaskKind,
    from: SubtaskState,
    to: SubtaskState,
) -> Result<()> {
    let allowed = match kind {
        SubtaskKind::Work => matches!(
            (from, to),
            (SubtaskState::Available, SubtaskState::Claimed)
                | (SubtaskState::Claimed, SubtaskState::InProgress)
                | (SubtaskState::InProgress, SubtaskState::Available)
                | (SubtaskState::InProgress, SubtaskState::Completed)
                | (SubtaskState::InProgress, SubtaskState::Failed)
                | (SubtaskState::InProgress, SubtaskState::ArtifactPublished)
                | (
                    SubtaskState::ArtifactPublished,
                    SubtaskState::ArtifactPublished
                )
                | (SubtaskState::ReviewPending, SubtaskState::ArtifactPublished)
                | (SubtaskState::ArtifactPublished, SubtaskState::ReviewPending)
                | (SubtaskState::ReviewPending, SubtaskState::ChangesRequested)
                | (SubtaskState::ReviewPending, SubtaskState::Blocked)
                | (SubtaskState::ReviewPending, SubtaskState::Approved)
                | (SubtaskState::ReviewPending, SubtaskState::Completed)
                | (SubtaskState::ReviewPending, SubtaskState::Applied)
                | (SubtaskState::Approved, SubtaskState::ReadyForApply)
                | (SubtaskState::ReadyForApply, SubtaskState::Applied)
        ),
        SubtaskKind::Review => matches!(
            (from, to),
            (SubtaskState::Available, SubtaskState::Claimed)
                | (SubtaskState::Claimed, SubtaskState::InProgress)
                | (SubtaskState::InProgress, SubtaskState::Decided)
        ),
        SubtaskKind::Cleanup => matches!(
            (from, to),
            (SubtaskState::Available, SubtaskState::Claimed)
                | (SubtaskState::Claimed, SubtaskState::InProgress)
                | (SubtaskState::InProgress, SubtaskState::Applied)
                | (SubtaskState::Available, SubtaskState::Applied)
        ),
    };
    ensure_transition(from, to, ObjectType::Subtask, allowed)
}

pub(crate) fn ensure_review_transition(from: ReviewState, to: ReviewState) -> Result<()> {
    let allowed = matches!(
        (from, to),
        (ReviewState::Requested, ReviewState::InProgress)
            | (ReviewState::Requested, ReviewState::Superseded)
            | (ReviewState::InProgress, ReviewState::Decided)
            | (ReviewState::InProgress, ReviewState::Superseded)
    );
    ensure_transition(from, to, ObjectType::Review, allowed)
}

pub(crate) fn ensure_ready_queue_transition(
    from: ReadyQueueState,
    to: ReadyQueueState,
) -> Result<()> {
    let allowed = matches!(
        (from, to),
        (ReadyQueueState::Queued, ReadyQueueState::InFlight)
            | (ReadyQueueState::InFlight, ReadyQueueState::Applied)
            | (ReadyQueueState::Queued, ReadyQueueState::Superseded)
            | (ReadyQueueState::InFlight, ReadyQueueState::Superseded)
            | (ReadyQueueState::Queued, ReadyQueueState::Cancelled)
            | (ReadyQueueState::InFlight, ReadyQueueState::Cancelled)
    );
    ensure_transition(from, to, ObjectType::ReadyQueue, allowed)
}

pub(crate) fn ensure_conflict_resolution_transition(
    from: ConflictResolutionState,
    to: ConflictResolutionState,
) -> Result<()> {
    let allowed = matches!(
        (from, to),
        (
            ConflictResolutionState::Open,
            ConflictResolutionState::Acknowledged
        ) | (
            ConflictResolutionState::Open,
            ConflictResolutionState::Resolved
        ) | (
            ConflictResolutionState::Acknowledged,
            ConflictResolutionState::Acknowledged
        ) | (
            ConflictResolutionState::Acknowledged,
            ConflictResolutionState::Resolved
        ) | (
            ConflictResolutionState::Resolved,
            ConflictResolutionState::Resolved
        )
    );
    ensure_transition(from, to, ObjectType::Conflict, allowed)
}

pub(crate) fn ensure_reservation_transition(
    from: ReservationState,
    to: ReservationState,
) -> Result<()> {
    let allowed = matches!(
        (from, to),
        (ReservationState::Active, ReservationState::Released)
            | (ReservationState::Active, ReservationState::Expired)
    );
    ensure_transition(from, to, ObjectType::Reservation, allowed)
}

pub(crate) fn ensure_transition(
    from: impl Into<StateValue>,
    to: impl Into<StateValue>,
    object: ObjectType,
    allowed: bool,
) -> Result<()> {
    if allowed {
        Ok(())
    } else {
        Err(CoveyError::IllegalTransition {
            from: from.into(),
            to: to.into(),
            object,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_GENERATED_MEMBERS, MAX_IDEMPOTENCY_KEY_LEN, ensure_conflict_resolution_transition,
        ensure_generated_member_count, ensure_length, ensure_positive_lease_duration,
        ensure_ready_queue_transition, ensure_reservation_transition, ensure_review_transition,
        ensure_subtask_transition, ensure_transition, require_session_can_claim_kind,
        require_session_can_enqueue, require_session_can_request_review, validate_idempotency_key,
    };
    use crate::{
        CoveyError,
        model::{
            ClaimState, ConflictResolutionState, ObjectType, ReadyQueueState, ReservationState,
            Session, SessionRole, SessionState, SessionToken, StateValue, SubtaskKind,
            SubtaskState, TimestampMs,
        },
    };

    fn session(role: SessionRole) -> Session {
        Session::try_from_parts(
            SessionToken::parse(format!("session-{role}")).expect("valid session token"),
            "principal-1",
            "instance-1",
            role,
            SessionState::Active,
            None,
            TimestampMs::parse(0).expect("valid timestamp"),
            0,
            TimestampMs::parse(0).expect("valid timestamp"),
            TimestampMs::parse(0).expect("valid timestamp"),
        )
        .expect("valid session fixture")
    }

    fn assert_too_large(error: &CoveyError, field: &str, actual: usize, max: usize) {
        assert!(
            matches!(
                error,
                CoveyError::InputTooLarge { field: f, actual: a, max: m }
                    if f == field && *a == actual && *m == max
            ),
            "{error:?}"
        );
    }

    fn assert_wrong_role(error: &CoveyError, expected: &[SessionRole], actual: SessionRole) {
        assert!(
            matches!(
                error,
                CoveyError::WrongRole { expected: e, actual: a }
                    if e.as_slice() == expected && *a == actual
            ),
            "{error:?}"
        );
    }

    fn assert_illegal_transition(
        error: &CoveyError,
        from: impl Into<StateValue>,
        to: impl Into<StateValue>,
        object: ObjectType,
    ) {
        let (from, to) = (from.into(), to.into());
        assert!(
            matches!(
                error,
                CoveyError::IllegalTransition { from: f, to: t, object: o }
                    if *f == from && *t == to && *o == object
            ),
            "{error:?}"
        );
    }

    #[test]
    fn validate_idempotency_key_accepts_non_empty_keys() {
        assert!(validate_idempotency_key("idem-123").is_ok());
    }

    #[test]
    fn validate_idempotency_key_rejects_blank_and_oversized_values() {
        let blank =
            validate_idempotency_key("   ").expect_err("blank idempotency key must be rejected");
        let oversized = validate_idempotency_key(&"x".repeat(MAX_IDEMPOTENCY_KEY_LEN + 1))
            .expect_err("oversized idempotency key must be rejected");

        assert!(
            matches!(
                &blank,
                CoveyError::InvalidIdempotencyKey { idempotency_key } if idempotency_key == "   "
            ),
            "{blank:?}"
        );
        assert_too_large(
            &oversized,
            "idempotency_key",
            MAX_IDEMPOTENCY_KEY_LEN + 1,
            MAX_IDEMPOTENCY_KEY_LEN,
        );
    }

    #[test]
    fn ensure_length_enforces_upper_bounds() {
        assert!(ensure_length("title", "abc", 3).is_ok());

        let err = ensure_length("title", "abcd", 3).expect_err("overflow must fail");

        assert_too_large(&err, "title", 4, 3);
    }

    #[test]
    fn ensure_generated_member_count_enforces_the_limit() {
        assert!(ensure_generated_member_count(MAX_GENERATED_MEMBERS).is_ok());

        let err = ensure_generated_member_count(MAX_GENERATED_MEMBERS + 1)
            .expect_err("overflow must fail");

        assert_too_large(
            &err,
            "generated_members",
            MAX_GENERATED_MEMBERS + 1,
            MAX_GENERATED_MEMBERS,
        );
    }

    #[test]
    fn ensure_positive_lease_duration_rejects_zero_and_negative_values() {
        assert!(ensure_positive_lease_duration("lease_duration_ms", 1).is_ok());

        let zero = ensure_positive_lease_duration("lease_duration_ms", 0)
            .expect_err("zero lease duration must fail");
        let negative = ensure_positive_lease_duration("lease_duration_ms", -1)
            .expect_err("negative lease duration must fail");

        for (error, refused) in [(&zero, 0), (&negative, -1)] {
            assert!(
                matches!(
                    error,
                    CoveyError::InvalidLeaseDuration { field, provided }
                        if field == "lease_duration_ms" && *provided == refused
                ),
                "{error:?}"
            );
        }
    }

    #[test]
    fn require_session_can_claim_kind_enforces_executor_and_reviewer_roles() {
        assert!(
            require_session_can_claim_kind(&session(SessionRole::Executor), SubtaskKind::Work)
                .is_ok()
        );
        assert!(
            require_session_can_claim_kind(&session(SessionRole::Reviewer), SubtaskKind::Review,)
                .is_ok()
        );

        let err =
            require_session_can_claim_kind(&session(SessionRole::Reviewer), SubtaskKind::Work)
                .expect_err("reviewers must not claim work subtasks");

        assert_wrong_role(&err, &[SessionRole::Executor], SessionRole::Reviewer);
    }

    #[test]
    fn require_session_can_request_review_allows_only_executor_and_orchestrator() {
        assert!(require_session_can_request_review(&session(SessionRole::Executor)).is_ok());
        assert!(require_session_can_request_review(&session(SessionRole::Orchestrator)).is_ok());

        let err = require_session_can_request_review(&session(SessionRole::Reviewer))
            .expect_err("reviewers must not request review");

        assert_wrong_role(
            &err,
            &[SessionRole::Executor, SessionRole::Orchestrator],
            SessionRole::Reviewer,
        );
    }

    #[test]
    fn require_session_can_enqueue_allows_only_orchestrator_and_apply_gate() {
        assert!(require_session_can_enqueue(&session(SessionRole::Orchestrator)).is_ok());
        assert!(require_session_can_enqueue(&session(SessionRole::ApplyGate)).is_ok());

        let err = require_session_can_enqueue(&session(SessionRole::Executor))
            .expect_err("executors must not enqueue apply work");

        assert_wrong_role(
            &err,
            &[SessionRole::Orchestrator, SessionRole::ApplyGate],
            SessionRole::Executor,
        );
    }

    #[test]
    fn ensure_subtask_transition_accepts_supported_work_and_review_paths() {
        assert!(
            ensure_subtask_transition(
                SubtaskKind::Work,
                SubtaskState::Available,
                SubtaskState::Claimed,
            )
            .is_ok()
        );
        assert!(
            ensure_subtask_transition(
                SubtaskKind::Work,
                SubtaskState::Approved,
                SubtaskState::ReadyForApply,
            )
            .is_ok()
        );
        assert!(
            ensure_subtask_transition(
                SubtaskKind::Work,
                SubtaskState::ReviewPending,
                SubtaskState::ChangesRequested,
            )
            .is_ok()
        );
        assert!(
            ensure_subtask_transition(
                SubtaskKind::Work,
                SubtaskState::ReviewPending,
                SubtaskState::Blocked,
            )
            .is_ok()
        );
        assert!(
            ensure_subtask_transition(
                SubtaskKind::Work,
                SubtaskState::ReviewPending,
                SubtaskState::Applied,
            )
            .is_ok()
        );
        assert!(
            ensure_subtask_transition(
                SubtaskKind::Review,
                SubtaskState::InProgress,
                SubtaskState::Decided,
            )
            .is_ok()
        );
    }

    #[test]
    fn ensure_subtask_transition_rejects_illegal_state_changes() {
        let work_err = ensure_subtask_transition(
            SubtaskKind::Work,
            SubtaskState::Approved,
            SubtaskState::Applied,
        )
        .expect_err("work subtasks must go through ready_for_apply");
        let review_err = ensure_subtask_transition(
            SubtaskKind::Review,
            SubtaskState::Available,
            SubtaskState::InProgress,
        )
        .expect_err("review subtasks must be claimed before starting");
        let retry_err = ensure_subtask_transition(
            SubtaskKind::Work,
            SubtaskState::ChangesRequested,
            SubtaskState::Claimed,
        )
        .expect_err("changes-requested artifacts require a new evidence item");
        let blocked_retry_err = ensure_subtask_transition(
            SubtaskKind::Work,
            SubtaskState::Blocked,
            SubtaskState::Claimed,
        )
        .expect_err("blocked artifacts require a new evidence item");

        assert_illegal_transition(
            &work_err,
            SubtaskState::Approved,
            SubtaskState::Applied,
            ObjectType::Subtask,
        );
        assert_illegal_transition(
            &review_err,
            SubtaskState::Available,
            SubtaskState::InProgress,
            ObjectType::Subtask,
        );
        assert_illegal_transition(
            &retry_err,
            SubtaskState::ChangesRequested,
            SubtaskState::Claimed,
            ObjectType::Subtask,
        );
        assert_illegal_transition(
            &blocked_retry_err,
            SubtaskState::Blocked,
            SubtaskState::Claimed,
            ObjectType::Subtask,
        );
    }

    #[test]
    fn state_specific_transition_guards_enforce_allowed_edges() {
        assert!(
            ensure_review_transition(
                crate::model::ReviewState::Requested,
                crate::model::ReviewState::InProgress,
            )
            .is_ok()
        );
        assert!(
            ensure_ready_queue_transition(ReadyQueueState::Queued, ReadyQueueState::InFlight)
                .is_ok()
        );
        assert!(
            ensure_ready_queue_transition(ReadyQueueState::Queued, ReadyQueueState::Cancelled)
                .is_ok()
        );
        assert!(
            ensure_reservation_transition(ReservationState::Active, ReservationState::Released)
                .is_ok()
        );
        assert!(
            ensure_conflict_resolution_transition(
                ConflictResolutionState::Open,
                ConflictResolutionState::Resolved,
            )
            .is_ok()
        );

        let review_err = ensure_review_transition(
            crate::model::ReviewState::Decided,
            crate::model::ReviewState::InProgress,
        )
        .expect_err("decided reviews are terminal");
        let queue_err =
            ensure_ready_queue_transition(ReadyQueueState::Applied, ReadyQueueState::Queued)
                .expect_err("applied queue items are terminal");
        let reservation_err =
            ensure_reservation_transition(ReservationState::Released, ReservationState::Expired)
                .expect_err("released reservations are terminal");
        let conflict_err = ensure_conflict_resolution_transition(
            ConflictResolutionState::Resolved,
            ConflictResolutionState::Acknowledged,
        )
        .expect_err("resolved conflicts cannot be downgraded");

        assert_illegal_transition(
            &review_err,
            crate::model::ReviewState::Decided,
            crate::model::ReviewState::InProgress,
            ObjectType::Review,
        );
        assert_illegal_transition(
            &queue_err,
            ReadyQueueState::Applied,
            ReadyQueueState::Queued,
            ObjectType::ReadyQueue,
        );
        assert_illegal_transition(
            &reservation_err,
            ReservationState::Released,
            ReservationState::Expired,
            ObjectType::Reservation,
        );
        assert_illegal_transition(
            &conflict_err,
            ConflictResolutionState::Resolved,
            ConflictResolutionState::Acknowledged,
            ObjectType::Conflict,
        );
    }

    #[test]
    fn ensure_transition_returns_typed_illegal_transition_errors() {
        let err = ensure_transition(
            ClaimState::Held,
            ClaimState::Revoked,
            ObjectType::Claim,
            false,
        )
        .expect_err("disallowed edge must fail");

        assert_illegal_transition(
            &err,
            ClaimState::Held,
            ClaimState::Revoked,
            ObjectType::Claim,
        );
    }
}

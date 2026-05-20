use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    error::{CoveyError, Result},
    model::{
        ArtifactDigest, Claim, ClaimState, FenceSeq, MetaTaskState, ObjectType, ReadyQueueState,
        ReservationState, ReviewState, RuntimeAttestation, Session, SessionRole, SessionState,
        SessionToken, StateValue, SubtaskId, SubtaskKind, SubtaskState,
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

pub(crate) fn ensure_no_other_active_session(
    tx: &Transaction<'_>,
    principal_id: &str,
) -> Result<()> {
    let existing = tx
        .query_row(
            "SELECT session_token FROM sessions WHERE agent_principal_id = ?1 AND state = ?2",
            params![principal_id, SessionState::Active.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if existing.is_some() {
        return Err(CoveyError::SessionAlreadyActive {
            agent_principal_id: principal_id.to_owned(),
        });
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

pub(crate) fn held_claim_owner(
    tx: &Transaction<'_>,
    subtask_id: &str,
) -> Result<Option<SessionToken>> {
    tx.query_row(
        "SELECT owner_session_token FROM claims WHERE subtask_id = ?1 AND state = ?2",
        params![subtask_id, ClaimState::Held.to_string()],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn require_session_can_claim_kind(session: &Session, kind: SubtaskKind) -> Result<()> {
    let expected = match kind {
        SubtaskKind::Work => vec![SessionRole::Executor],
        SubtaskKind::Review => vec![SessionRole::Reviewer],
    };
    if expected.contains(&session.role) {
        Ok(())
    } else {
        Err(CoveyError::WrongRole {
            expected,
            actual: session.role,
        })
    }
}

pub(crate) fn require_session_can_request_review(session: &Session) -> Result<()> {
    let expected = vec![SessionRole::Executor, SessionRole::Orchestrator];
    if expected.contains(&session.role) {
        Ok(())
    } else {
        Err(CoveyError::WrongRole {
            expected,
            actual: session.role,
        })
    }
}

pub(crate) fn require_session_can_enqueue(session: &Session) -> Result<()> {
    let expected = vec![SessionRole::Orchestrator, SessionRole::ApplyGate];
    if expected.contains(&session.role) {
        Ok(())
    } else {
        Err(CoveyError::WrongRole {
            expected,
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

pub(crate) fn require_current_claim(
    tx: &Transaction<'_>,
    session_token: &str,
    claim_id: &str,
    fence_seq: FenceSeq,
    now: i64,
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
    if claim.lease_deadline <= now {
        return Err(CoveyError::LeaseExpired {
            object_id: claim_id.to_owned(),
        });
    }
    Ok(claim)
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
            state.to_string(),
            now,
            ClaimState::Held.to_string()
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
                ReviewState::Requested.to_string(),
                ReviewState::InProgress.to_string()
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
                | (SubtaskState::ChangesRequested, SubtaskState::Claimed)
                | (SubtaskState::Claimed, SubtaskState::InProgress)
                | (SubtaskState::InProgress, SubtaskState::ArtifactPublished)
                | (
                    SubtaskState::ArtifactPublished,
                    SubtaskState::ArtifactPublished
                )
                | (SubtaskState::ReviewPending, SubtaskState::ArtifactPublished)
                | (SubtaskState::ArtifactPublished, SubtaskState::ReviewPending)
                | (SubtaskState::ReviewPending, SubtaskState::ChangesRequested)
                | (SubtaskState::ReviewPending, SubtaskState::Approved)
                | (SubtaskState::Approved, SubtaskState::ReadyForApply)
                | (SubtaskState::ReadyForApply, SubtaskState::Applied)
        ),
        SubtaskKind::Review => matches!(
            (from, to),
            (SubtaskState::Available, SubtaskState::Claimed)
                | (SubtaskState::Claimed, SubtaskState::InProgress)
                | (SubtaskState::InProgress, SubtaskState::Decided)
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
    );
    ensure_transition(from, to, ObjectType::ReadyQueue, allowed)
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
        MAX_GENERATED_MEMBERS, MAX_IDEMPOTENCY_KEY_LEN, ensure_generated_member_count,
        ensure_length, ensure_positive_lease_duration, ensure_ready_queue_transition,
        ensure_reservation_transition, ensure_review_transition, ensure_subtask_transition,
        ensure_transition, require_session_can_claim_kind, require_session_can_enqueue,
        require_session_can_request_review, validate_idempotency_key,
    };
    use crate::{
        CoveyError,
        model::{
            ClaimState, ObjectType, ReadyQueueState, ReservationState, Session, SessionRole,
            SessionState, SessionToken, StateValue, SubtaskKind, SubtaskState, TimestampMs,
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

        assert_eq!(
            blank,
            CoveyError::InvalidIdempotencyKey {
                idempotency_key: "   ".to_owned(),
            }
        );
        assert_eq!(
            oversized,
            CoveyError::InputTooLarge {
                field: "idempotency_key".to_owned(),
                actual: MAX_IDEMPOTENCY_KEY_LEN + 1,
                max: MAX_IDEMPOTENCY_KEY_LEN,
            }
        );
    }

    #[test]
    fn ensure_length_enforces_upper_bounds() {
        assert!(ensure_length("title", "abc", 3).is_ok());

        let err = ensure_length("title", "abcd", 3).expect_err("overflow must fail");

        assert_eq!(
            err,
            CoveyError::InputTooLarge {
                field: "title".to_owned(),
                actual: 4,
                max: 3,
            }
        );
    }

    #[test]
    fn ensure_generated_member_count_enforces_the_limit() {
        assert!(ensure_generated_member_count(MAX_GENERATED_MEMBERS).is_ok());

        let err = ensure_generated_member_count(MAX_GENERATED_MEMBERS + 1)
            .expect_err("overflow must fail");

        assert_eq!(
            err,
            CoveyError::InputTooLarge {
                field: "generated_members".to_owned(),
                actual: MAX_GENERATED_MEMBERS + 1,
                max: MAX_GENERATED_MEMBERS,
            }
        );
    }

    #[test]
    fn ensure_positive_lease_duration_rejects_zero_and_negative_values() {
        assert!(ensure_positive_lease_duration("lease_duration_ms", 1).is_ok());

        let zero = ensure_positive_lease_duration("lease_duration_ms", 0)
            .expect_err("zero lease duration must fail");
        let negative = ensure_positive_lease_duration("lease_duration_ms", -1)
            .expect_err("negative lease duration must fail");

        assert_eq!(
            zero,
            CoveyError::InvalidLeaseDuration {
                field: "lease_duration_ms".to_owned(),
                provided: 0,
            }
        );
        assert_eq!(
            negative,
            CoveyError::InvalidLeaseDuration {
                field: "lease_duration_ms".to_owned(),
                provided: -1,
            }
        );
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

        assert_eq!(
            err,
            CoveyError::WrongRole {
                expected: vec![SessionRole::Executor],
                actual: SessionRole::Reviewer,
            }
        );
    }

    #[test]
    fn require_session_can_request_review_allows_only_executor_and_orchestrator() {
        assert!(require_session_can_request_review(&session(SessionRole::Executor)).is_ok());
        assert!(require_session_can_request_review(&session(SessionRole::Orchestrator)).is_ok());

        let err = require_session_can_request_review(&session(SessionRole::Reviewer))
            .expect_err("reviewers must not request review");

        assert_eq!(
            err,
            CoveyError::WrongRole {
                expected: vec![SessionRole::Executor, SessionRole::Orchestrator],
                actual: SessionRole::Reviewer,
            }
        );
    }

    #[test]
    fn require_session_can_enqueue_allows_only_orchestrator_and_apply_gate() {
        assert!(require_session_can_enqueue(&session(SessionRole::Orchestrator)).is_ok());
        assert!(require_session_can_enqueue(&session(SessionRole::ApplyGate)).is_ok());

        let err = require_session_can_enqueue(&session(SessionRole::Executor))
            .expect_err("executors must not enqueue apply work");

        assert_eq!(
            err,
            CoveyError::WrongRole {
                expected: vec![SessionRole::Orchestrator, SessionRole::ApplyGate],
                actual: SessionRole::Executor,
            }
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

        assert_eq!(
            work_err,
            CoveyError::IllegalTransition {
                from: StateValue::Subtask(SubtaskState::Approved),
                to: StateValue::Subtask(SubtaskState::Applied),
                object: ObjectType::Subtask,
            }
        );
        assert_eq!(
            review_err,
            CoveyError::IllegalTransition {
                from: StateValue::Subtask(SubtaskState::Available),
                to: StateValue::Subtask(SubtaskState::InProgress),
                object: ObjectType::Subtask,
            }
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
            ensure_reservation_transition(ReservationState::Active, ReservationState::Released)
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

        assert_eq!(
            review_err,
            CoveyError::IllegalTransition {
                from: crate::model::ReviewState::Decided.into(),
                to: crate::model::ReviewState::InProgress.into(),
                object: ObjectType::Review,
            }
        );
        assert_eq!(
            queue_err,
            CoveyError::IllegalTransition {
                from: ReadyQueueState::Applied.into(),
                to: ReadyQueueState::Queued.into(),
                object: ObjectType::ReadyQueue,
            }
        );
        assert_eq!(
            reservation_err,
            CoveyError::IllegalTransition {
                from: ReservationState::Released.into(),
                to: ReservationState::Expired.into(),
                object: ObjectType::Reservation,
            }
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

        assert_eq!(
            err,
            CoveyError::IllegalTransition {
                from: StateValue::Claim(ClaimState::Held),
                to: StateValue::Claim(ClaimState::Revoked),
                object: ObjectType::Claim,
            }
        );
    }
}

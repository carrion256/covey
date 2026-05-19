#![cfg_attr(coverage_nightly, coverage(off))]

use std::time::Instant;

use rusqlite::{Transaction, params};

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        AbandonSubtaskReq, ClaimNextReq, ClaimResult, ClaimState, ClaimSubtaskReq,
        CreateSubtaskRequest, EventType, ObjectType, ReviewState, Session, SessionRole,
        SessionState, StartSubtaskReq, SubtaskId, SubtaskKind, SubtaskState,
    },
    queries::load_subtask_tx,
    schema::advance_lease_clock,
    store::{
        append_session_event, expire_claim_if_needed_for_subtask, ordered_claim_candidates,
        refresh_meta_task_state,
    },
    validators::{
        MAX_OBJECT_ID_LEN, MAX_TITLE_LEN, clear_session_active_subtask, close_claim_and_detach,
        ensure_length, ensure_meta_task_exists, ensure_meta_task_is_schedulable,
        ensure_positive_lease_duration, ensure_subtask_transition, ensure_transition,
        held_claim_owner, issue_fence_seq, require_active_session, require_current_claim,
        require_session_can_claim_kind, subtask_exists,
    },
};

impl Covey {
    /// Creates a new work or review subtask under an existing meta-task.
    pub fn create_subtask(&self, req: CreateSubtaskRequest) -> Result<String> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "create_subtask",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || create_subtask_tx(tx, &req, now),
            )
        });
        self.log_operation(
            "create_subtask",
            &req.session_token,
            started_at,
            &result,
            |subtask_id| vec![format!("subtask:{subtask_id}")],
        );
        result
    }

    /// Claims the next available subtask according to priority and creation order.
    pub fn claim_next_subtask(&self, req: ClaimNextReq) -> Result<Option<ClaimResult>> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "claim_next_subtask",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    let session = require_active_session(tx, &req.session_token)?;
                    ensure_positive_lease_duration("lease_duration_ms", req.lease_duration_ms)?;
                    if let Some(active_subtask_id) = session.active_subtask_id().cloned() {
                        return Err(CoveyError::SessionAlreadyHasActiveSubtask {
                            session_token: session.session_token,
                            active_subtask_id,
                        });
                    }
                    let (kind, candidate_states) = match session.role {
                        SessionRole::Executor => (
                            SubtaskKind::Work,
                            vec![SubtaskState::Available, SubtaskState::ChangesRequested],
                        ),
                        SessionRole::Reviewer => {
                            (SubtaskKind::Review, vec![SubtaskState::Available])
                        }
                        other => {
                            return Err(CoveyError::WrongRole {
                                expected: vec![SessionRole::Executor, SessionRole::Reviewer],
                                actual: other,
                            });
                        }
                    };

                    let candidates = ordered_claim_candidates(tx, kind, &candidate_states, now)?;
                    for subtask_id in candidates {
                        match claim_selected_subtask_tx(
                            tx,
                            &session,
                            &req.session_token,
                            &subtask_id,
                            crate::model::LeaseDurationMs::parse(req.lease_duration_ms)?,
                            lease_now,
                            now,
                        ) {
                            Ok(result) => return Ok(Some(result)),
                            Err(CoveyError::SubtaskAlreadyClaimed { .. }) => continue,
                            Err(CoveyError::IllegalTransition { to, object, .. })
                                if to == SubtaskState::Claimed.into()
                                    && object == ObjectType::Subtask =>
                            {
                                continue;
                            }
                            Err(err) => return Err(err),
                        }
                    }

                    Ok(None)
                },
            )
        });
        self.log_operation(
            "claim_next_subtask",
            &req.session_token,
            started_at,
            &result,
            |claim| {
                claim
                    .as_ref()
                    .map(|claim| {
                        vec![
                            format!("claim:{}", claim.claim_id),
                            format!("subtask:{}", claim.subtask_id),
                        ]
                    })
                    .unwrap_or_default()
            },
        );
        result
    }

    /// Claims a specific known subtask.
    pub fn claim_subtask(&self, req: ClaimSubtaskReq) -> Result<ClaimResult> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "claim_subtask",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    let session = require_active_session(tx, &req.session_token)?;
                    ensure_positive_lease_duration("lease_duration_ms", req.lease_duration_ms)?;
                    if let Some(active_subtask_id) = session.active_subtask_id().cloned() {
                        return Err(CoveyError::SessionAlreadyHasActiveSubtask {
                            session_token: session.session_token,
                            active_subtask_id,
                        });
                    }
                    if !subtask_exists(tx, &req.subtask_id)? {
                        return Err(CoveyError::SubtaskNotFound);
                    }

                    claim_selected_subtask_tx(
                        tx,
                        &session,
                        &req.session_token,
                        &req.subtask_id,
                        crate::model::LeaseDurationMs::parse(req.lease_duration_ms)?,
                        lease_now,
                        now,
                    )
                },
            )
        });
        self.log_operation(
            "claim_subtask",
            &req.session_token,
            started_at,
            &result,
            |claim| {
                vec![
                    format!("claim:{}", claim.claim_id),
                    format!("subtask:{}", claim.subtask_id),
                ]
            },
        );
        result
    }

    /// Transitions a claimed subtask into active execution.
    pub fn start_subtask(&self, req: StartSubtaskReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "start_subtask",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    ensure_length("claim_id", &req.claim_id, MAX_OBJECT_ID_LEN)?;
                    let claim = require_current_claim(
                        tx,
                        &req.session_token,
                        &req.claim_id,
                        crate::model::FenceSeq::parse(req.fence_seq)?,
                        lease_now,
                    )?;
                    let session = require_active_session(tx, &req.session_token)?;
                    let subtask = load_subtask_tx(tx, &claim.subtask_id)?;
                    ensure_meta_task_is_schedulable(tx, &subtask.meta_task_id)?;
                    require_session_can_claim_kind(&session, subtask.kind)?;
                    ensure_subtask_transition(
                        subtask.kind,
                        subtask.state,
                        SubtaskState::InProgress,
                    )?;
                    let updated = tx.execute(
                        "UPDATE subtasks SET state = ?2, updated_at = ?3 WHERE subtask_id = ?1 AND state = ?4",
                        params![
                            subtask.subtask_id,
                            SubtaskState::InProgress.to_string(),
                            now,
                            subtask.state.to_string()
                        ],
                    )?;
                    if updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: subtask.state.into(),
                            to: SubtaskState::InProgress.into(),
                            object: ObjectType::Subtask,
                        });
                    }
                    if subtask.kind == SubtaskKind::Review {
                        let review_state_raw = tx.query_row(
                            "SELECT state FROM reviews WHERE review_subtask_id = ?1",
                            params![subtask.subtask_id],
                            |row| row.get::<_, String>(0),
                        )?;
                        let review_state = match review_state_raw.as_str() {
                            "requested" => ReviewState::Requested,
                            "in_progress" => ReviewState::InProgress,
                            "decided" => ReviewState::Decided,
                            "superseded" => ReviewState::Superseded,
                            _ => return Err(CoveyError::DatabaseError(rusqlite::Error::InvalidQuery)),
                        };
                        crate::validators::ensure_review_transition(
                            review_state,
                            ReviewState::InProgress,
                        )?;
                        tx.execute(
                            "UPDATE reviews SET state = ?2, reviewer_session = ?3, updated_at = ?4 WHERE review_subtask_id = ?1 AND state = ?5",
                            params![
                                subtask.subtask_id,
                                ReviewState::InProgress.to_string(),
                                req.session_token,
                                now,
                                review_state.to_string()
                            ],
                        )?;
                    }
                    append_session_event(
                        tx,
                        EventType::SubtaskStarted,
                        ObjectType::Subtask,
                        &subtask.subtask_id,
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(())
                },
            )
        });
        self.log_operation(
            "start_subtask",
            &req.session_token,
            started_at,
            &result,
            |_| vec![format!("claim:{}", req.claim_id)],
        );
        result
    }

    /// Abandons a subtask and releases the current claim.
    pub fn abandon_subtask(&self, req: AbandonSubtaskReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "abandon_subtask",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    ensure_length("claim_id", &req.claim_id, MAX_OBJECT_ID_LEN)?;
                    let claim = require_current_claim(
                        tx,
                        &req.session_token,
                        &req.claim_id,
                        crate::model::FenceSeq::parse(req.fence_seq)?,
                        lease_now,
                    )?;
                    let session = require_active_session(tx, &req.session_token)?;
                    let subtask = load_subtask_tx(tx, &claim.subtask_id)?;
                    require_session_can_claim_kind(&session, subtask.kind)?;
                    ensure_transition(
                        subtask.state,
                        SubtaskState::Abandoned,
                        ObjectType::Subtask,
                        !matches!(subtask.state, SubtaskState::Applied | SubtaskState::Abandoned),
                    )?;
                    close_claim_and_detach(tx, &claim, ClaimState::Released, now)?;
                    let updated = tx.execute(
                        "UPDATE subtasks SET state = ?2, current_claim_id = NULL, updated_at = ?3 WHERE subtask_id = ?1 AND current_claim_id = ?4",
                        params![
                            subtask.subtask_id,
                            SubtaskState::Abandoned.to_string(),
                            now,
                            claim.claim_id
                        ],
                    )?;
                    if updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: subtask.state.into(),
                            to: SubtaskState::Abandoned.into(),
                            object: ObjectType::Subtask,
                        });
                    }
                    clear_session_active_subtask(tx, &req.session_token, now)?;
                    refresh_meta_task_state(tx, &subtask.meta_task_id, now)?;
                    append_session_event(
                        tx,
                        EventType::SubtaskAbandoned,
                        ObjectType::Subtask,
                        &subtask.subtask_id,
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(())
                },
            )
        });
        self.log_operation(
            "abandon_subtask",
            &req.session_token,
            started_at,
            &result,
            |_| vec![format!("claim:{}", req.claim_id)],
        );
        result
    }

    /// Releases a held claim and makes the subtask claimable again when appropriate.
    pub fn release_claim(&self, req: crate::model::ReleaseClaimReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "release_claim",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    ensure_length("claim_id", &req.claim_id, MAX_OBJECT_ID_LEN)?;
                    let claim = require_current_claim(
                        tx,
                        &req.session_token,
                        &req.claim_id,
                        crate::model::FenceSeq::parse(req.fence_seq)?,
                        lease_now,
                    )?;
                    let session = require_active_session(tx, &req.session_token)?;
                    let subtask = load_subtask_tx(tx, &claim.subtask_id)?;
                    require_session_can_claim_kind(&session, subtask.kind)?;
                    close_claim_and_detach(tx, &claim, ClaimState::Released, now)?;
                    let next_state = match subtask.state {
                        SubtaskState::Claimed | SubtaskState::InProgress => SubtaskState::Available,
                        other => other,
                    };
                    let updated = tx.execute(
                        "UPDATE subtasks SET state = ?2, current_claim_id = NULL, updated_at = ?3 WHERE subtask_id = ?1 AND current_claim_id = ?4",
                        params![
                            subtask.subtask_id,
                            next_state.to_string(),
                            now,
                            claim.claim_id
                        ],
                    )?;
                    if updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: subtask.state.into(),
                            to: next_state.into(),
                            object: ObjectType::Subtask,
                        });
                    }
                    clear_session_active_subtask(tx, &req.session_token, now)?;
                    append_session_event(
                        tx,
                        EventType::ClaimReleased,
                        ObjectType::Claim,
                        &claim.claim_id,
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(())
                },
            )
        });
        self.log_operation(
            "release_claim",
            &req.session_token,
            started_at,
            &result,
            |_| vec![format!("claim:{}", req.claim_id)],
        );
        result
    }

    /// Renews the lease on a held claim without changing its fence token.
    pub fn renew_claim(&self, req: crate::model::RenewClaimReq) -> Result<ClaimResult> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "renew_claim",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    ensure_positive_lease_duration("extend_by_ms", req.extend_by_ms)?;
                    ensure_length("claim_id", &req.claim_id, MAX_OBJECT_ID_LEN)?;
                    let claim = require_current_claim(
                        tx,
                        &req.session_token,
                        &req.claim_id,
                        crate::model::FenceSeq::parse(req.fence_seq)?,
                        lease_now,
                    )?;
                    let renewed_deadline = claim.lease_deadline.get().max(lease_now) + req.extend_by_ms;
                    let updated = tx.execute(
                        "UPDATE claims SET lease_deadline = ?2, updated_at = ?3 WHERE claim_id = ?1 AND state = ?4 AND owner_session_token = ?5 AND fence_seq = ?6",
                        params![
                            req.claim_id,
                            renewed_deadline,
                            now,
                            ClaimState::Held.to_string(),
                            req.session_token,
                            req.fence_seq
                        ],
                    )?;
                    if updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: claim.state.into(),
                            to: claim.state.into(),
                            object: ObjectType::Claim,
                        });
                    }
                    let result = ClaimResult::new(
                        claim.claim_id.to_string(),
                        claim.subtask_id.to_string(),
                        claim.fence_seq.get(),
                        renewed_deadline,
                    );
                    append_session_event(
                        tx,
                        EventType::ClaimRenewed,
                        ObjectType::Claim,
                        &result.claim_id,
                        &req.session_token,
                        &result,
                        now,
                    )?;
                    Ok(result)
                },
            )
        });
        self.log_operation(
            "renew_claim",
            &req.session_token,
            started_at,
            &result,
            |claim| vec![format!("claim:{}", claim.claim_id)],
        );
        result
    }
}

fn claim_selected_subtask_tx(
    tx: &Transaction<'_>,
    session: &Session,
    session_token: &str,
    subtask_id: &str,
    lease_duration_ms: crate::model::LeaseDurationMs,
    lease_now: i64,
    now: i64,
) -> Result<ClaimResult> {
    expire_claim_if_needed_for_subtask(tx, subtask_id, lease_now, now)?;

    let subtask = load_subtask_tx(tx, subtask_id)?;
    ensure_meta_task_is_schedulable(tx, &subtask.meta_task_id)?;
    require_session_can_claim_kind(session, subtask.kind)?;
    if let Some(held_by) = held_claim_owner(tx, subtask_id)? {
        return Err(CoveyError::SubtaskAlreadyClaimed {
            subtask_id: subtask.subtask_id.clone(),
            held_by,
        });
    }
    ensure_subtask_transition(subtask.kind, subtask.state, SubtaskState::Claimed)?;

    let fence_seq = issue_fence_seq(tx, subtask_id)?;
    let claim_id = crate::model::make_id("claim");
    let lease_deadline = crate::model::LeaseDeadlineMs::parse(lease_now + lease_duration_ms.get())?;
    tx.execute(
        r#"
        INSERT INTO claims (
            claim_id, subtask_id, owner_session_token, fence_seq, lease_deadline,
            state, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
        "#,
        params![
            claim_id,
            subtask_id,
            session_token,
            fence_seq,
            lease_deadline,
            ClaimState::Held.to_string(),
            now
        ],
    )?;

    let subtask_updated = tx.execute(
        "UPDATE subtasks SET state = ?2, current_claim_id = ?3, artifact_digest = NULL, updated_at = ?4 WHERE subtask_id = ?1 AND state = ?5 AND current_claim_id IS NULL",
        params![
            subtask_id,
            SubtaskState::Claimed.to_string(),
            claim_id,
            now,
            subtask.state.to_string()
        ],
    )?;
    if subtask_updated != 1 {
        tx.execute("DELETE FROM claims WHERE claim_id = ?1", params![claim_id])?;
        return Err(CoveyError::IllegalTransition {
            from: load_subtask_tx(tx, subtask_id)?.state.into(),
            to: SubtaskState::Claimed.into(),
            object: ObjectType::Subtask,
        });
    }

    let session_updated = tx.execute(
        "UPDATE sessions SET active_subtask_id = ?2, updated_at = ?3 WHERE session_token = ?1 AND active_subtask_id IS NULL AND state = ?4",
        params![
            session_token,
            subtask_id,
            now,
            SessionState::Active.to_string()
        ],
    )?;
    if session_updated != 1 {
        let current_session = crate::validators::require_session(tx, session_token)?;
        let active_subtask_id = current_session
            .active_subtask_id()
            .cloned()
            .unwrap_or_else(|| subtask.subtask_id.clone());
        return Err(CoveyError::SessionAlreadyHasActiveSubtask {
            session_token: current_session.session_token,
            active_subtask_id,
        });
    }

    let result = ClaimResult::new(
        claim_id,
        subtask_id.to_owned(),
        fence_seq.get(),
        lease_deadline.get(),
    );
    append_session_event(
        tx,
        EventType::SubtaskClaimed,
        ObjectType::Claim,
        &result.claim_id,
        session_token,
        &result,
        now,
    )?;
    Ok(result)
}

pub(crate) fn create_subtask_tx(
    tx: &rusqlite::Transaction<'_>,
    req: &CreateSubtaskRequest,
    now: i64,
) -> Result<String> {
    crate::validators::require_role(tx, &req.session_token, &[SessionRole::Orchestrator])?;
    ensure_length("title", &req.title, MAX_TITLE_LEN)?;
    ensure_length("meta_task_id", &req.meta_task_id, MAX_OBJECT_ID_LEN)?;
    ensure_meta_task_exists(tx, &req.meta_task_id)?;
    ensure_meta_task_is_schedulable(tx, &req.meta_task_id)?;

    let subtask_id = req
        .subtask_id
        .clone()
        .unwrap_or_else(|| crate::model::make_id("subtask"));
    ensure_length("subtask_id", &subtask_id, MAX_OBJECT_ID_LEN)?;
    let subtask_id = SubtaskId::parse(subtask_id)?;
    if subtask_exists(tx, &subtask_id)? {
        return Err(CoveyError::DuplicateSubtaskId {
            subtask_id: subtask_id.clone(),
        });
    }

    tx.execute(
        r#"
        INSERT INTO subtasks (
            subtask_id, meta_task_id, title, kind, review_target_subtask_id,
            review_target_artifact_digest, state, current_claim_id, artifact_digest,
            priority, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, ?8, ?9, ?9)
        "#,
        params![
            subtask_id.as_str(),
            req.meta_task_id,
            req.title,
            SubtaskKind::Work.to_string(),
            Option::<String>::None,
            Option::<String>::None,
            SubtaskState::Available.to_string(),
            req.priority,
            now,
        ],
    )?;
    tx.execute(
        "INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq) VALUES (?1, 1)",
        params![subtask_id.as_str()],
    )?;
    append_session_event(
        tx,
        EventType::SubtaskCreated,
        ObjectType::Subtask,
        subtask_id.as_str(),
        &req.session_token,
        req,
        now,
    )?;
    refresh_meta_task_state(tx, &req.meta_task_id, now)?;
    Ok(subtask_id.to_string())
}

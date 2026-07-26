#![cfg_attr(coverage_nightly, coverage(off))]

use std::time::Instant;

use rusqlite::{Transaction, params};

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        AttemptEvidenceDigest, AttemptFailureCode, AttemptOutcome, AttemptOutcomeKind,
        AttemptSummary, ClaimId, ClaimState, CompletionPolicy, EventType, FailSubtaskReq, FenceSeq,
        FinishSubtaskReq, ObjectType, ReservationState, RetrySubtaskReq, SessionToken,
        SubtaskState, TimestampMs, attempt_outcome_kind_name, reservation_state_name,
        subtask_state_name,
    },
    overlap::resolve_reservation_overlap_conflicts,
    queries::load_subtask_tx,
    schema::advance_lease_clock,
    store::{append_session_event, refresh_meta_task_state},
    validators::{
        clear_session_active_subtask, close_claim_and_detach, ensure_meta_task_is_schedulable,
        ensure_subtask_transition, require_active_session, require_current_claim,
        require_session_can_claim_kind,
    },
};

struct OutcomeInput<'a> {
    session_token: &'a SessionToken,
    claim_id: &'a ClaimId,
    fence_seq: FenceSeq,
    outcome_kind: AttemptOutcomeKind,
    evidence_digest: &'a AttemptEvidenceDigest,
    failure_code: Option<&'a AttemptFailureCode>,
    summary: &'a AttemptSummary,
}

impl Covey {
    /// Completes direct work with an immutable fenced success receipt.
    pub fn finish_subtask(&self, req: FinishSubtaskReq) -> Result<AttemptOutcome> {
        self.record_direct_outcome(
            "finish_subtask",
            EventType::SubtaskFinished,
            &req,
            &req.session_token,
            &req.idempotency_key,
            OutcomeInput {
                session_token: &req.session_token,
                claim_id: &req.claim_id,
                fence_seq: req.fence_seq,
                outcome_kind: AttemptOutcomeKind::Succeeded,
                evidence_digest: &req.evidence_digest,
                failure_code: None,
                summary: &req.summary,
            },
        )
    }

    /// Records a retryable direct-work failure and returns the work to its lane.
    pub fn retry_subtask(&self, req: RetrySubtaskReq) -> Result<AttemptOutcome> {
        self.record_direct_outcome(
            "retry_subtask",
            EventType::SubtaskRetried,
            &req,
            &req.session_token,
            &req.idempotency_key,
            OutcomeInput {
                session_token: &req.session_token,
                claim_id: &req.claim_id,
                fence_seq: req.fence_seq,
                outcome_kind: AttemptOutcomeKind::RetryableFailure,
                evidence_digest: &req.evidence_digest,
                failure_code: Some(&req.failure_code),
                summary: &req.summary,
            },
        )
    }

    /// Records a terminal direct-work failure.
    pub fn fail_subtask(&self, req: FailSubtaskReq) -> Result<AttemptOutcome> {
        self.record_direct_outcome(
            "fail_subtask",
            EventType::SubtaskFailed,
            &req,
            &req.session_token,
            &req.idempotency_key,
            OutcomeInput {
                session_token: &req.session_token,
                claim_id: &req.claim_id,
                fence_seq: req.fence_seq,
                outcome_kind: AttemptOutcomeKind::TerminalFailure,
                evidence_digest: &req.evidence_digest,
                failure_code: Some(&req.failure_code),
                summary: &req.summary,
            },
        )
    }

    fn record_direct_outcome<Req: serde::Serialize>(
        &self,
        operation: &'static str,
        event_type: EventType,
        req: &Req,
        session_token: &SessionToken,
        idempotency_key: &crate::model::IdempotencyKey,
        input: OutcomeInput<'_>,
    ) -> Result<AttemptOutcome> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                session_token.as_str(),
                operation,
                idempotency_key.as_str(),
                req,
                TimestampMs::parse(now)?,
                || record_direct_outcome_tx(tx, event_type, &input, lease_now, now),
            )
        });
        self.log_operation(
            operation,
            session_token.as_str(),
            started_at,
            &result,
            |outcome| {
                vec![
                    format!("claim:{}", outcome.claim_id),
                    format!("subtask:{}", outcome.subtask_id),
                ]
            },
        );
        result
    }
}

fn record_direct_outcome_tx(
    tx: &Transaction<'_>,
    event_type: EventType,
    input: &OutcomeInput<'_>,
    lease_now: i64,
    now: i64,
) -> Result<AttemptOutcome> {
    let claim = require_current_claim(
        tx,
        input.session_token.as_str(),
        input.claim_id.as_str(),
        input.fence_seq,
        lease_now,
    )?;
    let session = require_active_session(tx, input.session_token.as_str())?;
    let subtask = load_subtask_tx(tx, claim.subtask_id.as_str())?;
    ensure_meta_task_is_schedulable(tx, subtask.meta_task_id.as_str())?;
    require_session_can_claim_kind(&session, subtask.kind())?;
    if subtask.completion_policy() != CompletionPolicy::Direct {
        return Err(CoveyError::CompletionPolicyViolation {
            operation: attempt_outcome_kind_name(input.outcome_kind).to_owned(),
            policy: subtask.completion_policy(),
        });
    }

    let next_state = match input.outcome_kind {
        AttemptOutcomeKind::Succeeded => SubtaskState::Completed,
        AttemptOutcomeKind::RetryableFailure => SubtaskState::Available,
        AttemptOutcomeKind::TerminalFailure => SubtaskState::Failed,
    };
    ensure_subtask_transition(subtask.kind(), subtask.state(), next_state)?;

    let outcome = AttemptOutcome::new(
        claim.claim_id.clone(),
        subtask.subtask_id.clone(),
        claim.fence_seq,
        input.outcome_kind,
        input.evidence_digest.clone(),
        input.failure_code.cloned(),
        input.summary.clone(),
        TimestampMs::parse(now)?,
    )
    .map_err(|reason| CoveyError::InvalidObservabilityRow { reason })?;

    tx.execute(
        r#"
        INSERT INTO subtask_attempt_outcomes (
            claim_id, subtask_id, fence_seq, outcome_kind, evidence_digest,
            failure_code, summary, recorded_by_session, recorded_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
        params![
            outcome.claim_id.as_str(),
            outcome.subtask_id.as_str(),
            outcome.fence_seq.get(),
            attempt_outcome_kind_name(outcome.outcome_kind()),
            outcome.evidence_digest.as_str(),
            outcome.failure_code().map(AsRef::as_ref),
            outcome.summary.as_str(),
            input.session_token.as_str(),
            now,
        ],
    )?;

    close_claim_and_detach(tx, &claim, ClaimState::Released, now)?;
    release_active_reservations(tx, subtask.subtask_id.as_str(), now)?;
    let updated = tx.execute(
        r#"
        UPDATE subtasks
        SET state = ?2, current_claim_id = NULL, updated_at = ?3
        WHERE subtask_id = ?1 AND state = ?4 AND current_claim_id = ?5
        "#,
        params![
            subtask.subtask_id.as_str(),
            subtask_state_name(next_state),
            now,
            subtask_state_name(SubtaskState::InProgress),
            claim.claim_id.as_str(),
        ],
    )?;
    if updated != 1 {
        return Err(CoveyError::IllegalTransition {
            from: subtask.state().into(),
            to: next_state.into(),
            object: ObjectType::Subtask,
        });
    }
    clear_session_active_subtask(tx, input.session_token.as_str(), now)?;
    refresh_meta_task_state(tx, subtask.meta_task_id.as_str(), now)?;
    append_session_event(
        tx,
        event_type,
        ObjectType::Subtask,
        subtask.subtask_id.as_str(),
        input.session_token.as_str(),
        &outcome,
        now,
    )?;
    Ok(outcome)
}

pub(super) fn release_active_reservations(
    tx: &Transaction<'_>,
    subtask_id: &str,
    now: i64,
) -> Result<()> {
    let mut statement = tx.prepare(
        "SELECT reservation_id FROM reservations WHERE owner_subtask_id = ?1 AND state = ?2",
    )?;
    let reservation_ids = statement
        .query_map(
            params![subtask_id, reservation_state_name(ReservationState::Active)],
            |row| row.get::<_, String>(0),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(statement);

    tx.execute(
        "UPDATE reservations SET state = ?2, updated_at = ?3 WHERE owner_subtask_id = ?1 AND state = ?4",
        params![
            subtask_id,
            reservation_state_name(ReservationState::Released),
            now,
            reservation_state_name(ReservationState::Active),
        ],
    )?;
    for reservation_id in reservation_ids {
        resolve_reservation_overlap_conflicts(tx, &reservation_id, now)?;
    }
    Ok(())
}

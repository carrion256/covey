#![cfg_attr(coverage_nightly, coverage(off))]

use std::time::Instant;

use rusqlite::Transaction;
use rusqlite::params;

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        ClaimState, EventType, MetaTaskState, MetaTaskStatus, ObjectType, ReadyQueueState,
        SessionRole, SubtaskState,
    },
    queries::{load_meta_task_tx, load_subtasks_for_meta_task_tx},
    store::{append_session_event, load_claims_for_meta_task},
    validators::{
        MAX_OBJECT_ID_LEN, MAX_PROMPT_LEN, ensure_length, ensure_transition, require_role,
    },
};

impl Covey {
    /// Creates a new planning meta-task.
    pub fn submit_meta_task(&self, req: crate::model::SubmitMetaTaskReq) -> Result<String> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                req.session_token.as_str(),
                "submit_meta_task",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || submit_meta_task_tx(tx, req.session_token.as_str(), &req.prompt_text, &req, now),
            )
        });
        self.log_operation(
            "submit_meta_task",
            req.session_token.as_str(),
            started_at,
            &result,
            |meta_task_id| vec![format!("meta_task:{meta_task_id}")],
        );
        result
    }

    /// Cancels a meta-task that has not already reached a terminal state.
    pub fn cancel_meta_task(&self, req: crate::model::CancelMetaTaskReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "cancel_meta_task",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::Orchestrator])?;
                    ensure_length("meta_task_id", &req.meta_task_id, MAX_OBJECT_ID_LEN)?;
                    let meta = load_meta_task_tx(tx, req.meta_task_id.as_str())?;
                    ensure_transition(
                        meta.state(),
                        MetaTaskState::Cancelled,
                        ObjectType::MetaTask,
                        !matches!(
                            meta.state(),
                            MetaTaskState::Completed | MetaTaskState::Cancelled
                        ),
                    )?;
                    let updated = tx.execute(
                        "UPDATE meta_tasks SET state = ?2, updated_at = ?3 WHERE meta_task_id = ?1 AND state NOT IN (?4, ?5)",
                        params![
                            req.meta_task_id.as_str(),
                            MetaTaskState::Cancelled.to_string(),
                            now,
                            MetaTaskState::Completed.to_string(),
                            MetaTaskState::Cancelled.to_string()
                        ],
                    )?;
                    if updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: meta.state().into(),
                            to: MetaTaskState::Cancelled.into(),
                            object: ObjectType::MetaTask,
                        });
                    }
                    let held_claims = load_claims_for_meta_task(tx, req.meta_task_id.as_str())?;
                    for claim in &held_claims {
                        tx.execute(
                            "UPDATE claims SET state = ?2, updated_at = ?3 WHERE claim_id = ?1 AND state = ?4",
                            params![
                                claim.claim_id,
                                ClaimState::Revoked.to_string(),
                                now,
                                ClaimState::Held.to_string()
                            ],
                        )?;
                        tx.execute(
                            "UPDATE sessions SET active_subtask_id = NULL, updated_at = ?3 WHERE session_token = ?1 AND active_subtask_id = ?2",
                            params![claim.owner_session_token, claim.subtask_id, now],
                        )?;
                    }
                    tx.execute(
                        r#"
                        UPDATE ready_queue
                        SET state = ?2,
                            claimed_by_session_token = NULL,
                            claim_lease_deadline = NULL,
                            updated_at = ?3
                        WHERE subtask_id IN (
                            SELECT subtask_id FROM subtasks WHERE meta_task_id = ?1
                        )
                          AND state IN (?4, ?5)
                        "#,
                        params![
                            req.meta_task_id.as_str(),
                            ReadyQueueState::Cancelled.to_string(),
                            now,
                            ReadyQueueState::Queued.to_string(),
                            ReadyQueueState::InFlight.to_string()
                        ],
                    )?;
                    tx.execute(
                        r#"
                        UPDATE subtasks
                        SET state = CASE
                                WHEN state IN (?2, ?3) THEN state
                                ELSE ?4
                            END,
                            current_claim_id = NULL,
                            updated_at = ?5
                        WHERE meta_task_id = ?1
                        "#,
                        params![
                            req.meta_task_id.as_str(),
                            SubtaskState::Applied.to_string(),
                            SubtaskState::Abandoned.to_string(),
                            SubtaskState::Abandoned.to_string(),
                            now
                        ],
                    )?;
                    append_session_event(
                        tx,
                        EventType::MetaTaskCancelled,
                        ObjectType::MetaTask,
                        &req.meta_task_id,
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(())
                },
            )
        });
        self.log_operation(
            "cancel_meta_task",
            &req.session_token,
            started_at,
            &result,
            |_| vec![format!("meta_task:{}", req.meta_task_id)],
        );
        result
    }

    /// Returns the current persisted status for a meta-task and its subtasks.
    pub fn meta_task_status(&self, meta_task_id: &str) -> Result<MetaTaskStatus> {
        let started_at = Instant::now();
        let result = self.with_read_tx(|tx| {
            let meta_task = load_meta_task_tx(tx, meta_task_id)?;
            let subtasks = load_subtasks_for_meta_task_tx(tx, meta_task_id)?
                .into_iter()
                .map(crate::model::SubtaskView::try_from)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            MetaTaskStatus::new(meta_task, subtasks)
                .map_err(|reason| CoveyError::InvalidObservabilityRow { reason })
        });
        self.log_operation(
            "meta_task_status",
            "system",
            started_at,
            &result,
            |status| vec![format!("meta_task:{}", status.meta_task().meta_task_id)],
        );
        result
    }
}

pub(crate) fn submit_meta_task_tx<T: serde::Serialize>(
    tx: &Transaction<'_>,
    session_token: &str,
    prompt_text: &str,
    event_payload: &T,
    now: i64,
) -> Result<String> {
    require_role(tx, session_token, &[SessionRole::Orchestrator])?;
    ensure_length("prompt_text", prompt_text, MAX_PROMPT_LEN)?;
    let meta_task_id = crate::model::make_id("meta");
    tx.execute(
        r#"
        INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?5)
        "#,
        params![
            meta_task_id,
            prompt_text,
            MetaTaskState::Planning.to_string(),
            session_token,
            now
        ],
    )?;
    append_session_event(
        tx,
        EventType::MetaTaskSubmitted,
        ObjectType::MetaTask,
        &meta_task_id,
        session_token,
        event_payload,
        now,
    )?;
    Ok(meta_task_id)
}

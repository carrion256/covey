use std::time::Instant;

use rusqlite::params;

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        ClaimReadyQueueReq, EnqueueForApplyReq, EventType, MarkAppliedReq, MarkInFlightReq,
        ObjectType, ReadyQueueClaim, ReadyQueueItem, ReadyQueueMetrics, ReadyQueueState,
        SessionRole, SubtaskState, SupersedeQueueItemReq,
    },
    queries::{collect_rows, deserialize_row, load_queue_item_tx, load_subtask_tx},
    schema::advance_lease_clock,
    store::{
        append_session_event, claim_ready_queue_item, ordered_ready_queue_candidates,
        refresh_meta_task_state, requeue_stale_ready_queue_claims,
    },
    validators::{
        MAX_DIGEST_LEN, MAX_OBJECT_ID_LEN, ensure_length, ensure_meta_task_is_schedulable,
        ensure_positive_lease_duration, ensure_ready_queue_transition, ensure_subtask_transition,
        require_active_session, require_role, require_session_can_enqueue,
    },
};

impl Covey {
    /// Enqueues an approved artifact for apply.
    pub fn enqueue_for_apply(&self, req: EnqueueForApplyReq) -> Result<String> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "enqueue_for_apply",
                &req.idempotency_key,
                &req,
                now,
                || {
                    let session = require_active_session(tx, &req.session_token)?;
                    require_session_can_enqueue(&session)?;
                    ensure_length("artifact_digest", &req.artifact_digest, MAX_DIGEST_LEN)?;
                    ensure_length("subtask_id", &req.subtask_id, MAX_OBJECT_ID_LEN)?;
                    let subtask = load_subtask_tx(tx, &req.subtask_id)?;
                    ensure_meta_task_is_schedulable(tx, &subtask.meta_task_id)?;
                    ensure_subtask_transition(
                        subtask.kind,
                        subtask.state,
                        SubtaskState::ReadyForApply,
                    )?;
                    if subtask.artifact_digest.as_deref() != Some(req.artifact_digest.as_str()) {
                        return Err(CoveyError::IllegalTransition {
                            from: subtask.state.into(),
                            to: SubtaskState::ReadyForApply.into(),
                            object: ObjectType::Subtask,
                        });
                    }

                    tx.execute(
                        r#"
                        UPDATE ready_queue
                        SET state = ?2,
                            claimed_by_session_token = NULL,
                            claim_lease_deadline = NULL,
                            updated_at = ?3
                        WHERE subtask_id = ?1 AND state IN (?4, ?5)
                        "#,
                        params![
                            req.subtask_id,
                            ReadyQueueState::Superseded.to_string(),
                            now,
                            ReadyQueueState::Queued.to_string(),
                            ReadyQueueState::InFlight.to_string()
                        ],
                    )?;

                    let queue_id = crate::model::make_id("queue");
                    tx.execute(
                        r#"
                        INSERT INTO ready_queue (
                            queue_id, artifact_digest, subtask_id, settlement_target, state,
                            claimed_by_session_token, claim_fence_seq, claim_lease_deadline,
                            enqueued_at, updated_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, ?6, ?6)
                        "#,
                        params![
                            queue_id,
                            req.artifact_digest,
                            req.subtask_id,
                            req.settlement_target.to_string(),
                            ReadyQueueState::Queued.to_string(),
                            now
                        ],
                    )?;
                    let updated = tx.execute(
                        "UPDATE subtasks SET state = ?2, updated_at = ?3 WHERE subtask_id = ?1 AND state = ?4 AND artifact_digest = ?5",
                        params![
                            req.subtask_id,
                            SubtaskState::ReadyForApply.to_string(),
                            now,
                            subtask.state.to_string(),
                            req.artifact_digest
                        ],
                    )?;
                    if updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: subtask.state.into(),
                            to: SubtaskState::ReadyForApply.into(),
                            object: ObjectType::Subtask,
                        });
                    }
                    append_session_event(
                        tx,
                        EventType::ReadyQueueEnqueued,
                        ObjectType::ReadyQueue,
                        &queue_id,
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(queue_id)
                },
            )
        });
        self.log_operation(
            "enqueue_for_apply",
            &req.session_token,
            started_at,
            &result,
            |queue_id| {
                vec![
                    format!("queue:{queue_id}"),
                    format!("subtask:{}", req.subtask_id),
                ]
            },
        );
        result
    }

    /// Returns queued apply items in enqueue order.
    pub fn fetch_ready_queue(&self, limit: usize) -> Result<Vec<ReadyQueueItem>> {
        let started_at = Instant::now();
        let result = self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT queue_id, artifact_digest, subtask_id, settlement_target, state,
                       claimed_by_session_token, claim_fence_seq, claim_lease_deadline,
                       enqueued_at, updated_at
                FROM ready_queue
                WHERE state = ?1
                ORDER BY enqueued_at ASC
                LIMIT ?2
                "#,
            )?;
            let rows = stmt.query_map(
                params![ReadyQueueState::Queued.to_string(), limit as i64],
                deserialize_row::<ReadyQueueItem>,
            )?;
            collect_rows(rows)
        });
        self.log_operation(
            "fetch_ready_queue",
            "system",
            started_at,
            &result,
            |items| {
                items
                    .iter()
                    .map(|item| format!("queue:{}", item.queue_id))
                    .collect()
            },
        );
        result
    }

    /// Atomically claims the next queued apply item for an apply gate session.
    pub fn claim_next_ready_queue_item(
        &self,
        req: ClaimReadyQueueReq,
    ) -> Result<Option<ReadyQueueClaim>> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "claim_next_ready_queue_item",
                &req.idempotency_key,
                &req,
                now,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::ApplyGate])?;
                    ensure_positive_lease_duration("lease_duration_ms", req.lease_duration_ms)?;
                    requeue_stale_ready_queue_claims(tx, lease_now, now)?;
                    for queue_id in ordered_ready_queue_candidates(tx)? {
                        if let Some(claim) = claim_ready_queue_item(
                            tx,
                            &queue_id,
                            &req.session_token,
                            req.lease_duration_ms,
                            lease_now,
                            now,
                        )? {
                            append_session_event(
                                tx,
                                EventType::ReadyQueueInFlight,
                                ObjectType::ReadyQueue,
                                &claim.queue_id,
                                &req.session_token,
                                &claim,
                                now,
                            )?;
                            return Ok(Some(claim));
                        }
                    }
                    Ok(None)
                },
            )
        });
        self.log_operation(
            "claim_next_ready_queue_item",
            &req.session_token,
            started_at,
            &result,
            |claim| {
                claim
                    .iter()
                    .map(|claim| format!("queue:{}", claim.queue_id))
                    .collect()
            },
        );
        result
    }

    /// Marks a specific queued apply item as in flight and returns its apply claim token.
    pub fn mark_in_flight(&self, req: MarkInFlightReq) -> Result<ReadyQueueClaim> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "mark_in_flight",
                &req.idempotency_key,
                &req,
                now,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::ApplyGate])?;
                    ensure_positive_lease_duration("lease_duration_ms", req.lease_duration_ms)?;
                    ensure_length("queue_id", &req.queue_id, MAX_OBJECT_ID_LEN)?;
                    requeue_stale_ready_queue_claims(tx, lease_now, now)?;
                    let item = load_queue_item_tx(tx, &req.queue_id)?;
                    let claim = claim_ready_queue_item(
                        tx,
                        &req.queue_id,
                        &req.session_token,
                        req.lease_duration_ms,
                        lease_now,
                        now,
                    )?
                    .ok_or(CoveyError::IllegalTransition {
                        from: item.state.into(),
                        to: ReadyQueueState::InFlight.into(),
                        object: ObjectType::ReadyQueue,
                    })?;
                    append_session_event(
                        tx,
                        EventType::ReadyQueueInFlight,
                        ObjectType::ReadyQueue,
                        &claim.queue_id,
                        &req.session_token,
                        &claim,
                        now,
                    )?;
                    Ok(claim)
                },
            )
        });
        self.log_operation(
            "mark_in_flight",
            &req.session_token,
            started_at,
            &result,
            |claim| vec![format!("queue:{}", claim.queue_id)],
        );
        result
    }

    /// Marks an in-flight apply item as applied and settles the underlying subtask.
    pub fn mark_applied(&self, req: MarkAppliedReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "mark_applied",
                &req.idempotency_key,
                &req,
                now,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::ApplyGate])?;
                    ensure_length("queue_id", &req.queue_id, MAX_OBJECT_ID_LEN)?;
                    let item = load_queue_item_tx(tx, &req.queue_id)?;
                    ensure_ready_queue_transition(item.state, ReadyQueueState::Applied)?;
                    let queue_owner = item
                        .claimed_by_session_token
                        .clone()
                        .ok_or(CoveyError::IllegalTransition {
                            from: item.state.into(),
                            to: ReadyQueueState::Applied.into(),
                            object: ObjectType::ReadyQueue,
                        })?;
                    if queue_owner != req.session_token {
                        return Err(CoveyError::NotQueueClaimOwner {
                            session_token: req.session_token.clone(),
                            queue_owner,
                        });
                    }
                    let expected_fence = item.claim_fence_seq.ok_or(
                        CoveyError::IllegalTransition {
                            from: item.state.into(),
                            to: ReadyQueueState::Applied.into(),
                            object: ObjectType::ReadyQueue,
                        },
                    )?;
                    if expected_fence != req.claim_fence_seq {
                        return Err(CoveyError::StaleFenceToken {
                            expected: expected_fence,
                            provided: req.claim_fence_seq,
                        });
                    }
                    let claim_deadline = item.claim_lease_deadline.ok_or(
                        CoveyError::IllegalTransition {
                            from: item.state.into(),
                            to: ReadyQueueState::Applied.into(),
                            object: ObjectType::ReadyQueue,
                        },
                    )?;
                    if claim_deadline <= lease_now {
                        return Err(CoveyError::LeaseExpired {
                            object_id: req.queue_id.clone(),
                        });
                    }
                    let subtask = load_subtask_tx(tx, &item.subtask_id)?;
                    ensure_subtask_transition(subtask.kind, subtask.state, SubtaskState::Applied)?;
                    if subtask.artifact_digest.as_deref() != Some(item.artifact_digest.as_str()) {
                        return Err(CoveyError::IllegalTransition {
                            from: subtask.state.into(),
                            to: SubtaskState::Applied.into(),
                            object: ObjectType::Subtask,
                        });
                    }
                    let queue_updated = tx.execute(
                        "UPDATE ready_queue SET state = ?2, claimed_by_session_token = NULL, claim_lease_deadline = NULL, updated_at = ?3 WHERE queue_id = ?1 AND state = ?4 AND claimed_by_session_token = ?5 AND claim_fence_seq = ?6",
                        params![
                            req.queue_id,
                            ReadyQueueState::Applied.to_string(),
                            now,
                            ReadyQueueState::InFlight.to_string(),
                            req.session_token,
                            req.claim_fence_seq
                        ],
                    )?;
                    if queue_updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: item.state.into(),
                            to: ReadyQueueState::Applied.into(),
                            object: ObjectType::ReadyQueue,
                        });
                    }
                    let subtask_updated = tx.execute(
                        "UPDATE subtasks SET state = ?2, updated_at = ?3 WHERE subtask_id = ?1 AND state = ?4 AND artifact_digest = ?5",
                        params![
                            item.subtask_id,
                            SubtaskState::Applied.to_string(),
                            now,
                            SubtaskState::ReadyForApply.to_string(),
                            item.artifact_digest
                        ],
                    )?;
                    if subtask_updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: subtask.state.into(),
                            to: SubtaskState::Applied.into(),
                            object: ObjectType::Subtask,
                        });
                    }
                    refresh_meta_task_state(tx, &subtask.meta_task_id, now)?;
                    append_session_event(
                        tx,
                        EventType::ReadyQueueApplied,
                        ObjectType::ReadyQueue,
                        &req.queue_id,
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(())
                },
            )
        });
        self.log_operation(
            "mark_applied",
            &req.session_token,
            started_at,
            &result,
            |_| vec![format!("queue:{}", req.queue_id)],
        );
        result
    }

    /// Supersedes a queued or in-flight apply item.
    pub fn supersede_queue_item(&self, req: SupersedeQueueItemReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "supersede_queue_item",
                &req.idempotency_key,
                &req,
                now,
                || {
                    require_role(
                        tx,
                        &req.session_token,
                        &[SessionRole::ApplyGate, SessionRole::Orchestrator],
                    )?;
                    ensure_length("queue_id", &req.queue_id, MAX_OBJECT_ID_LEN)?;
                    let item = load_queue_item_tx(tx, &req.queue_id)?;
                    ensure_ready_queue_transition(item.state, ReadyQueueState::Superseded)?;
                    let updated = tx.execute(
                        "UPDATE ready_queue SET state = ?2, claimed_by_session_token = NULL, claim_lease_deadline = NULL, updated_at = ?3 WHERE queue_id = ?1 AND state IN (?4, ?5)",
                        params![
                            req.queue_id,
                            ReadyQueueState::Superseded.to_string(),
                            now,
                            ReadyQueueState::Queued.to_string(),
                            ReadyQueueState::InFlight.to_string()
                        ],
                    )?;
                    if updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: item.state.into(),
                            to: ReadyQueueState::Superseded.into(),
                            object: ObjectType::ReadyQueue,
                        });
                    }
                    append_session_event(
                        tx,
                        EventType::ReadyQueueSuperseded,
                        ObjectType::ReadyQueue,
                        &req.queue_id,
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(())
                },
            )
        });
        self.log_operation(
            "supersede_queue_item",
            &req.session_token,
            started_at,
            &result,
            |_| vec![format!("queue:{}", req.queue_id)],
        );
        result
    }

    /// Returns queue depth and oldest-item ages for the ready queue.
    pub fn ready_queue_metrics(&self) -> Result<ReadyQueueMetrics> {
        let started_at = Instant::now();
        let now = self.clock.wall_now_ms();
        let result = self.with_read_conn(|conn| {
            let (queued_count, oldest_queued): (i64, Option<i64>) = conn.query_row(
                "SELECT COUNT(*), MIN(enqueued_at) FROM ready_queue WHERE state = ?1",
                params![ReadyQueueState::Queued.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let (in_flight_count, oldest_in_flight): (i64, Option<i64>) = conn.query_row(
                "SELECT COUNT(*), MIN(enqueued_at) FROM ready_queue WHERE state = ?1",
                params![ReadyQueueState::InFlight.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            Ok(ReadyQueueMetrics::new(
                queued_count.max(0) as usize,
                in_flight_count.max(0) as usize,
                oldest_queued.map(|created_at| (now - created_at).max(0)),
                oldest_in_flight.map(|created_at| (now - created_at).max(0)),
            ))
        });
        self.log_operation(
            "ready_queue_metrics",
            "system",
            started_at,
            &result,
            |_metrics| Vec::<String>::new(),
        );
        result
    }
}

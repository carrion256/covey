#![cfg_attr(coverage_nightly, coverage(off))]

use std::time::Instant;

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        ClaimReadyQueueReq, EnqueueForApplyReq, EventType, FenceSeq, FindingsDigest,
        LandingAuthorizationStatus, MarkAppliedReq, MarkInFlightReq, ObjectType, QueueId,
        ReadyQueueClaim, ReadyQueueItem, ReadyQueueMetrics, ReadyQueueState,
        RecordApplyVerificationReq, ReviewId, ReviewState, ReviewVerdict, RuntimeAttestation,
        Session, SessionRole, SessionToken, SettlementTarget, SubtaskState, SupersedeQueueItemReq,
        VerifyLandingAuthorizationReq, ready_queue_state_name, review_state_name,
        review_verdict_name, subtask_state_name,
    },
    queries::{
        collect_rows, deserialize_row, load_queue_item_tx, load_session_tx, load_subtask_tx,
    },
    schema::advance_lease_clock,
    store::{
        append_session_event, claim_ready_queue_item, ordered_ready_queue_candidate,
        refresh_meta_task_state, requeue_stale_ready_queue_claims,
    },
    validators::{
        MAX_DIGEST_LEN, MAX_OBJECT_ID_LEN, ensure_length, ensure_meta_task_is_schedulable,
        ensure_positive_lease_duration, ensure_ready_queue_transition, ensure_subtask_transition,
        require_active_session, require_role, require_runtime_attestation,
        require_session_can_enqueue,
    },
};

const fn settlement_target_name(target: SettlementTarget) -> &'static str {
    match target {
        SettlementTarget::Canonical => "canonical",
    }
}

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
                crate::model::TimestampMs::parse(now)?,
                || {
                    let session = require_active_session(tx, &req.session_token)?;
                    require_session_can_enqueue(&session)?;
                    ensure_length("artifact_digest", &req.artifact_digest, MAX_DIGEST_LEN)?;
                    ensure_length("subtask_id", &req.subtask_id, MAX_OBJECT_ID_LEN)?;
                    let subtask = load_subtask_tx(tx, &req.subtask_id)?;
                    ensure_meta_task_is_schedulable(tx, &subtask.meta_task_id)?;
                    ensure_subtask_transition(
                        subtask.kind(),
                        subtask.state(),
                        SubtaskState::ReadyForApply,
                    )?;
                    if subtask.artifact_digest().map(AsRef::as_ref)
                        != Some(req.artifact_digest.as_str())
                    {
                        return Err(CoveyError::IllegalTransition {
                            from: subtask.state().into(),
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
                            ready_queue_state_name(ReadyQueueState::Superseded),
                            now,
                            ready_queue_state_name(ReadyQueueState::Queued),
                            ready_queue_state_name(ReadyQueueState::InFlight)
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
                            settlement_target_name(req.settlement_target),
                            ready_queue_state_name(ReadyQueueState::Queued),
                            now
                        ],
                    )?;
                    let updated = tx.execute(
                        "UPDATE subtasks SET state = ?2, updated_at = ?3 WHERE subtask_id = ?1 AND state = ?4 AND artifact_digest = ?5",
                        params![
                            req.subtask_id,
                            subtask_state_name(SubtaskState::ReadyForApply),
                            now,
                            subtask_state_name(subtask.state()),
                            req.artifact_digest
                        ],
                    )?;
                    if updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: subtask.state().into(),
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
        let result = if limit == 0 {
            Ok(Vec::new())
        } else {
            self.with_read_conn(|conn| {
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
                    params![
                        ready_queue_state_name(ReadyQueueState::Queued),
                        limit as i64
                    ],
                    deserialize_row::<ReadyQueueItem>,
                )?;
                collect_rows(rows)
            })
        };
        self.log_operation(
            "fetch_ready_queue",
            "system",
            started_at,
            &result,
            |items| {
                items
                    .iter()
                    .map(|item| format!("queue:{}", item.queue_id()))
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
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::ApplyGate])?;
                    ensure_positive_lease_duration(
                        "lease_duration_ms",
                        req.lease_duration_ms.get(),
                    )?;
                    requeue_stale_ready_queue_claims(tx, lease_now, now)?;
                    while let Some(queue_id) = ordered_ready_queue_candidate(tx)? {
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
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::ApplyGate])?;
                    ensure_positive_lease_duration(
                        "lease_duration_ms",
                        req.lease_duration_ms.get(),
                    )?;
                    ensure_length("queue_id", &req.queue_id, MAX_OBJECT_ID_LEN)?;
                    requeue_stale_ready_queue_claims(tx, lease_now, now)?;
                    let item = load_queue_item_tx(tx, req.queue_id.as_str())?;
                    let claim = claim_ready_queue_item(
                        tx,
                        req.queue_id.as_str(),
                        &req.session_token,
                        req.lease_duration_ms,
                        lease_now,
                        now,
                    )?
                    .ok_or(CoveyError::IllegalTransition {
                        from: item.state().into(),
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

    /// Records an accepted verifier verdict for the current apply attempt.
    pub fn record_apply_verification(&self, req: RecordApplyVerificationReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "record_apply_verification",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    let session = require_role(tx, &req.session_token, &[SessionRole::ApplyGate])?;
                    require_runtime_attestation(tx, &session)?;
                    ensure_length("queue_id", &req.queue_id, MAX_OBJECT_ID_LEN)?;
                    ensure_length("artifact_digest", &req.artifact_digest, MAX_DIGEST_LEN)?;
                    ensure_length("review_id", &req.review_id, MAX_OBJECT_ID_LEN)?;
                    ensure_length("findings_digest", &req.findings_digest, MAX_DIGEST_LEN)?;
                    ensure_length("verifier", &req.verifier, MAX_OBJECT_ID_LEN)?;
                    ensure_length("verdict_digest", &req.verdict_digest, MAX_DIGEST_LEN)?;
                    ensure_length("seal_digest", &req.seal_digest, MAX_DIGEST_LEN)?;

                    let item = load_queue_item_tx(tx, &req.queue_id)?;
                    let queue_id = QueueId::parse(item.queue_id())?;
                    if item.state() != ReadyQueueState::InFlight
                        || item.artifact_digest() != req.artifact_digest
                        || item.claim_fence_seq() != Some(req.claim_fence_seq.get())
                    {
                        return Err(CoveyError::ApplyGateEvidenceMissing {
                            queue_id,
                            reason:
                                "apply verification must target the current in-flight queue fence"
                                    .to_owned(),
                        });
                    }
                    let live_evidence = require_live_apply_gate_evidence(tx, &item, &session)?;
                    if live_evidence.review_id != req.review_id
                        || live_evidence.findings_digest != req.findings_digest
                    {
                        return Err(CoveyError::ApplyGateEvidenceMissing {
                            queue_id,
                            reason: "apply verification does not match the live approved review"
                                .to_owned(),
                        });
                    }

                    tx.execute(
                        r#"
                        INSERT INTO apply_verifications (
                            queue_id, artifact_digest, review_id, findings_digest,
                            claim_fence_seq, verifier, verdict_digest, seal_digest,
                            recorded_by_session, created_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                        "#,
                        params![
                            req.queue_id.as_str(),
                            req.artifact_digest.as_str(),
                            req.review_id.as_str(),
                            req.findings_digest.as_str(),
                            req.claim_fence_seq,
                            req.verifier.as_str(),
                            req.verdict_digest.as_str(),
                            req.seal_digest.as_str(),
                            req.session_token.as_str(),
                            now,
                        ],
                    )?;
                    append_session_event(
                        tx,
                        EventType::ApplyVerificationRecorded,
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
            "record_apply_verification",
            &req.session_token,
            started_at,
            &result,
            |_| vec![format!("queue:{}", req.queue_id)],
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
                crate::model::TimestampMs::parse(now)?,
                || {
                    let session = require_role(tx, &req.session_token, &[SessionRole::ApplyGate])?;
                    ensure_length("queue_id", &req.queue_id, MAX_OBJECT_ID_LEN)?;
                    let item = load_queue_item_tx(tx, req.queue_id.as_str())?;
                    ensure_ready_queue_transition(item.state(), ReadyQueueState::Applied)?;
                    let queue_owner = item
                        .claimed_by_session_token()
                        .map(crate::model::SessionToken::parse)
                        .transpose()?
                        .ok_or(CoveyError::IllegalTransition {
                            from: item.state().into(),
                            to: ReadyQueueState::Applied.into(),
                            object: ObjectType::ReadyQueue,
                        })?;
                    let session_token = session.session_token;
                    if queue_owner != session_token {
                        return Err(CoveyError::NotQueueClaimOwner {
                            session_token,
                            queue_owner,
                        });
                    }
                    let expected_fence = item.claim_fence_seq().ok_or(
                        CoveyError::IllegalTransition {
                            from: item.state().into(),
                            to: ReadyQueueState::Applied.into(),
                            object: ObjectType::ReadyQueue,
                        },
                    )?;
                    if expected_fence != req.claim_fence_seq.get() {
                        return Err(CoveyError::StaleFenceToken {
                            expected: expected_fence,
                            provided: req.claim_fence_seq.get(),
                        });
                    }
                    let claim_deadline = item.claim_lease_deadline().ok_or(
                        CoveyError::IllegalTransition {
                            from: item.state().into(),
                            to: ReadyQueueState::Applied.into(),
                            object: ObjectType::ReadyQueue,
                        },
                    )?;
                    if claim_deadline <= lease_now {
                        return Err(CoveyError::LeaseExpired {
                            object_id: req.queue_id.to_string(),
                        });
                    }
                    let subtask = load_subtask_tx(tx, item.subtask_id())?;
                    ensure_subtask_transition(
                        subtask.kind(),
                        subtask.state(),
                        SubtaskState::Applied,
                    )?;
                    if subtask.artifact_digest().map(AsRef::as_ref) != Some(item.artifact_digest())
                    {
                        return Err(CoveyError::IllegalTransition {
                            from: subtask.state().into(),
                            to: SubtaskState::Applied.into(),
                            object: ObjectType::Subtask,
                        });
                    }
                    let apply_gate_session = load_session_tx(tx, &req.session_token)?;
                    let live_evidence =
                        require_live_apply_gate_evidence(tx, &item, &apply_gate_session)?;
                    let queue_id = QueueId::parse(item.queue_id())?;
                    let claim_fence_seq = parse_landing_value(&queue_id, req.claim_fence_seq)?;
                    require_recorded_apply_verification(
                        tx,
                        &item,
                        &live_evidence,
                        claim_fence_seq,
                    )?;
                    let queue_updated = tx.execute(
                        "UPDATE ready_queue SET state = ?2, claimed_by_session_token = NULL, claim_lease_deadline = NULL, updated_at = ?3 WHERE queue_id = ?1 AND state = ?4 AND claimed_by_session_token = ?5 AND claim_fence_seq = ?6",
                        params![
                            req.queue_id.as_str(),
                            ready_queue_state_name(ReadyQueueState::Applied),
                            now,
                            ready_queue_state_name(ReadyQueueState::InFlight),
                            req.session_token.as_str(),
                            req.claim_fence_seq
                        ],
                    )?;
                    if queue_updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: item.state().into(),
                            to: ReadyQueueState::Applied.into(),
                            object: ObjectType::ReadyQueue,
                        });
                    }
                    let subtask_updated = tx.execute(
                        "UPDATE subtasks SET state = ?2, updated_at = ?3 WHERE subtask_id = ?1 AND state = ?4 AND artifact_digest = ?5",
                        params![
                            item.subtask_id(),
                            subtask_state_name(SubtaskState::Applied),
                            now,
                            subtask_state_name(SubtaskState::ReadyForApply),
                            item.artifact_digest()
                        ],
                    )?;
                    if subtask_updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: subtask.state().into(),
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
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(
                        tx,
                        &req.session_token,
                        &[SessionRole::ApplyGate, SessionRole::Orchestrator],
                    )?;
                    ensure_length("queue_id", &req.queue_id, MAX_OBJECT_ID_LEN)?;
                    let item = load_queue_item_tx(tx, req.queue_id.as_str())?;
                    ensure_ready_queue_transition(item.state(), ReadyQueueState::Superseded)?;
                    let updated = tx.execute(
                        "UPDATE ready_queue SET state = ?2, claimed_by_session_token = NULL, claim_lease_deadline = NULL, updated_at = ?3 WHERE queue_id = ?1 AND state IN (?4, ?5)",
                        params![
                            req.queue_id.as_str(),
                            ready_queue_state_name(ReadyQueueState::Superseded),
                            now,
                            ready_queue_state_name(ReadyQueueState::Queued),
                            ready_queue_state_name(ReadyQueueState::InFlight)
                        ],
                    )?;
                    if updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: item.state().into(),
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
            let queued_state = ready_queue_state_name(ReadyQueueState::Queued);
            let in_flight_state = ready_queue_state_name(ReadyQueueState::InFlight);
            let (queued_count, oldest_queued, in_flight_count, oldest_in_flight): (
                i64,
                Option<i64>,
                i64,
                Option<i64>,
            ) = conn.query_row(
                r#"
                SELECT COALESCE(SUM(CASE WHEN state = ?1 THEN 1 ELSE 0 END), 0),
                       MIN(CASE WHEN state = ?1 THEN enqueued_at END),
                       COALESCE(SUM(CASE WHEN state = ?2 THEN 1 ELSE 0 END), 0),
                       MIN(CASE WHEN state = ?2 THEN enqueued_at END)
                FROM ready_queue
                WHERE state IN (?1, ?2)
                "#,
                params![&queued_state, &in_flight_state],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
            ReadyQueueMetrics::new(
                queued_count.max(0) as usize,
                in_flight_count.max(0) as usize,
                oldest_queued.map(|created_at| (now - created_at).max(0)),
                oldest_in_flight.map(|created_at| (now - created_at).max(0)),
            )
            .map_err(|reason| CoveyError::InvalidReadyQueueMetrics { reason })
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

    /// Verifies that a landing authorization still matches live Covey apply evidence.
    pub fn verify_landing_authorization(
        &self,
        req: VerifyLandingAuthorizationReq,
    ) -> Result<LandingAuthorizationStatus> {
        let started_at = Instant::now();
        let session_token_for_log = req.session_token.clone();
        let result = self.with_read_tx(|tx| {
            require_role(
                tx,
                &req.session_token,
                &[SessionRole::ApplyGate, SessionRole::Orchestrator],
            )?;
            ensure_length("queue_id", &req.queue_id, MAX_OBJECT_ID_LEN)?;
            ensure_length("artifact_digest", &req.artifact_digest, MAX_DIGEST_LEN)?;
            ensure_length("review_id", &req.review_id, MAX_OBJECT_ID_LEN)?;
            ensure_length("findings_digest", &req.findings_digest, MAX_DIGEST_LEN)?;
            ensure_length("verifier", &req.verifier, MAX_OBJECT_ID_LEN)?;
            ensure_length("verdict_digest", &req.verdict_digest, MAX_DIGEST_LEN)?;
            ensure_length("seal_digest", &req.seal_digest, MAX_DIGEST_LEN)?;

            let item = load_queue_item_tx(tx, &req.queue_id)?;
            let queue_id = QueueId::parse(item.queue_id())?;
            if item.state() != ReadyQueueState::Applied {
                return Err(CoveyError::ApplyGateEvidenceMissing {
                    queue_id,
                    reason: "landing authorization queue item is not applied".to_owned(),
                });
            }
            if item.artifact_digest() != req.artifact_digest {
                return Err(CoveyError::ApplyGateEvidenceMissing {
                    queue_id,
                    reason: "landing authorization artifact digest does not match queue item"
                        .to_owned(),
                });
            }
            if item.claim_fence_seq() != Some(req.claim_fence_seq.get()) {
                return Err(CoveyError::ApplyGateEvidenceMissing {
                    queue_id,
                    reason: "landing authorization claim fence does not match queue item"
                        .to_owned(),
                });
            }

            let recorded_by_session: SessionToken = tx
                .query_row(
                    r#"
                    SELECT recorded_by_session
                    FROM apply_verifications
                    WHERE queue_id = ?1
                      AND artifact_digest = ?2
                      AND review_id = ?3
                      AND findings_digest = ?4
                      AND claim_fence_seq = ?5
                      AND verifier = ?6
                      AND verdict_digest = ?7
                      AND seal_digest = ?8
                    LIMIT 1
                    "#,
                    params![
                        req.queue_id.as_str(),
                        req.artifact_digest.as_str(),
                        req.review_id.as_str(),
                        req.findings_digest.as_str(),
                        req.claim_fence_seq,
                        req.verifier.as_str(),
                        req.verdict_digest.as_str(),
                        req.seal_digest.as_str(),
                    ],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| CoveyError::ApplyGateEvidenceMissing {
                    queue_id: queue_id.clone(),
                    reason: "accepted apply verifier verdict does not match landing authorization"
                        .to_owned(),
                })?;

            Ok(LandingAuthorizationStatus::accepted(
                req.queue_id,
                req.artifact_digest,
                req.review_id,
                req.findings_digest,
                req.claim_fence_seq,
                req.verifier,
                req.verdict_digest,
                req.seal_digest,
                recorded_by_session,
            ))
        });
        self.log_operation(
            "verify_landing_authorization",
            &session_token_for_log,
            started_at,
            &result,
            |status| vec![format!("queue:{}", status.queue_id())],
        );
        result
    }
}

fn parse_landing_value<T, V>(queue_id: &QueueId, value: V) -> Result<T>
where
    T: TryFrom<V>,
    T::Error: std::fmt::Display,
{
    T::try_from(value).map_err(|err| CoveyError::ApplyGateEvidenceMissing {
        queue_id: queue_id.clone(),
        reason: format!("invalid landing authorization value: {err}"),
    })
}

struct LiveApplyGateEvidence {
    review_id: ReviewId,
    findings_digest: FindingsDigest,
    producer_session_token: SessionToken,
    reviewer_session_token: SessionToken,
    producer_principal_id: String,
    reviewer_principal_id: String,
}

fn require_live_apply_gate_evidence(
    tx: &Transaction<'_>,
    item: &ReadyQueueItem,
    apply_gate_session: &Session,
) -> Result<LiveApplyGateEvidence> {
    let queue_id = QueueId::parse(item.queue_id())?;
    let (
        review_id,
        findings_digest,
        producer_session_token,
        reviewer_session_token,
        producer_principal_id,
        reviewer_principal_id,
    ) = tx
        .query_row(
            r#"
            SELECT review.review_id,
                   review.findings_digest,
                   producer.session_token,
                   reviewer.session_token,
                   producer.agent_principal_id,
                   reviewer.agent_principal_id
            FROM reviews review
            JOIN artifacts artifact
              ON artifact.artifact_digest = review.artifact_digest
             AND artifact.produced_by_subtask_id = review.subtask_id
            JOIN sessions producer
              ON producer.session_token = artifact.produced_by_session
            JOIN sessions reviewer
              ON reviewer.session_token = review.reviewer_session
            WHERE review.subtask_id = ?1
              AND review.artifact_digest = ?2
              AND review.state = ?3
              AND review.verdict = ?4
              AND review.findings_digest IS NOT NULL
              AND TRIM(review.findings_digest) != ''
            ORDER BY review.updated_at DESC
            LIMIT 1
            "#,
            params![
                item.subtask_id(),
                item.artifact_digest(),
                review_state_name(ReviewState::Decided),
                review_verdict_name(ReviewVerdict::Approve)
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| CoveyError::ApplyGateEvidenceMissing {
            queue_id: queue_id.clone(),
            reason: "approved review with findings digest not found for queued artifact".to_owned(),
        })?;
    let evidence = LiveApplyGateEvidence {
        review_id: parse_landing_value(&queue_id, review_id)?,
        findings_digest: parse_landing_value(&queue_id, findings_digest)?,
        producer_session_token: parse_landing_value(&queue_id, producer_session_token)?,
        reviewer_session_token: parse_landing_value(&queue_id, reviewer_session_token)?,
        producer_principal_id,
        reviewer_principal_id,
    };

    if evidence.producer_principal_id == evidence.reviewer_principal_id {
        return Err(CoveyError::ApplyGateEvidenceMissing {
            queue_id,
            reason: "producer and reviewer principals are not separated".to_owned(),
        });
    }
    if apply_gate_session.agent_principal_id == evidence.producer_principal_id {
        return Err(CoveyError::ApplyGateSeparationOfDutiesViolation {
            apply_gate_principal_id: apply_gate_session.agent_principal_id().to_owned(),
            conflicting_role: "producer".to_owned(),
            conflicting_principal_id: evidence.producer_principal_id,
        });
    }
    if apply_gate_session.agent_principal_id == evidence.reviewer_principal_id {
        return Err(CoveyError::ApplyGateSeparationOfDutiesViolation {
            apply_gate_principal_id: apply_gate_session.agent_principal_id().to_owned(),
            conflicting_role: "reviewer".to_owned(),
            conflicting_principal_id: evidence.reviewer_principal_id,
        });
    }
    let producer_session = load_session_tx(tx, &evidence.producer_session_token)?;
    let reviewer_session = load_session_tx(tx, &evidence.reviewer_session_token)?;
    let producer_attestation = require_runtime_attestation(tx, &producer_session)?;
    let reviewer_attestation = require_runtime_attestation(tx, &reviewer_session)?;
    let apply_gate_attestation = require_runtime_attestation(tx, apply_gate_session)?;
    require_runtime_actor_separation(
        &queue_id,
        "producer",
        &producer_attestation,
        "reviewer",
        &reviewer_attestation,
    )?;
    require_runtime_actor_separation(
        &queue_id,
        "producer",
        &producer_attestation,
        "apply_gate",
        &apply_gate_attestation,
    )?;
    require_runtime_actor_separation(
        &queue_id,
        "reviewer",
        &reviewer_attestation,
        "apply_gate",
        &apply_gate_attestation,
    )?;
    Ok(evidence)
}

fn require_runtime_actor_separation(
    queue_id: &QueueId,
    left_role: &str,
    left: &RuntimeAttestation,
    right_role: &str,
    right: &RuntimeAttestation,
) -> Result<()> {
    if runtime_ref(left) == runtime_ref(right) {
        return Err(CoveyError::ApplyGateEvidenceMissing {
            queue_id: queue_id.clone(),
            reason: format!("{left_role} and {right_role} runtime refs are not separated"),
        });
    }
    if provider_run_ref(left) == provider_run_ref(right) {
        return Err(CoveyError::ApplyGateEvidenceMissing {
            queue_id: queue_id.clone(),
            reason: format!("{left_role} and {right_role} provider run ids are not separated"),
        });
    }
    if left.command_transcript_digest == right.command_transcript_digest {
        return Err(CoveyError::ApplyGateEvidenceMissing {
            queue_id: queue_id.clone(),
            reason: format!("{left_role} and {right_role} transcript digests are not separated"),
        });
    }
    Ok(())
}

fn runtime_ref(attestation: &RuntimeAttestation) -> (Option<&str>, Option<&str>) {
    attestation.runtime_ref()
}

fn provider_run_ref(attestation: &RuntimeAttestation) -> Option<(&str, &str)> {
    attestation.provider_run_ref()
}

fn require_recorded_apply_verification(
    tx: &Transaction<'_>,
    item: &ReadyQueueItem,
    evidence: &LiveApplyGateEvidence,
    claim_fence_seq: FenceSeq,
) -> Result<()> {
    let queue_id = QueueId::parse(item.queue_id())?;
    let exists = tx
        .query_row(
            r#"
            SELECT 1
            FROM apply_verifications
            WHERE queue_id = ?1
              AND artifact_digest = ?2
              AND review_id = ?3
              AND findings_digest = ?4
              AND claim_fence_seq = ?5
            LIMIT 1
            "#,
            params![
                item.queue_id(),
                item.artifact_digest(),
                evidence.review_id.as_str(),
                evidence.findings_digest.as_str(),
                claim_fence_seq.get()
            ],
            |_| Ok(()),
        )
        .optional()?;
    if exists.is_none() {
        return Err(CoveyError::ApplyGateEvidenceMissing {
            queue_id,
            reason:
                "accepted apply verifier verdict not recorded for queue artifact review and fence"
                    .to_owned(),
        });
    }
    Ok(())
}

#![cfg_attr(coverage_nightly, coverage(off))]

use std::time::Instant;

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        ApplyQueueReconcileResult, ArtifactKind, ClaimReadyQueueReq, EnqueueForApplyReq, EventType,
        FenceSeq, FindingsDigest, LandingAuthorizationStatus, MarkAppliedReq, MarkInFlightReq,
        MetaTaskState, ObjectType, QueueId, ReadyQueueCandidate, ReadyQueueClaim, ReadyQueueItem,
        ReadyQueueMetrics, ReadyQueueState, ReconcileApplyQueueReq, RecordApplyVerificationReq,
        RecordLandingReceiptReq, ReviewId, ReviewState, ReviewVerdict, RuntimeAttestation, Session,
        SessionRole, SessionToken, SettlementTarget, SubtaskKind, SubtaskState,
        SupersedeQueueItemReq, VerifyLandingAuthorizationReq, meta_task_state_name,
        ready_queue_state_name, review_state_name, review_verdict_name, subtask_kind_name,
        subtask_state_name,
    },
    queries::{
        collect_rows, deserialize_row, load_artifact_tx, load_queue_item_tx, load_session_tx,
        load_subtask_tx,
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
                    if subtask.artifact_digest().map(AsRef::as_ref)
                        != Some(req.artifact_digest.as_str())
                    {
                        return Err(CoveyError::IllegalTransition {
                            from: subtask.state().into(),
                            to: SubtaskState::ReadyForApply.into(),
                            object: ObjectType::Subtask,
                        });
                    }
                    if subtask.state() == SubtaskState::ReadyForApply {
                        if let Some(queue_id) = active_queue_id_for_artifact_tx(
                            tx,
                            req.subtask_id.as_str(),
                            req.artifact_digest.as_str(),
                        )? {
                            return Ok(queue_id);
                        }
                    }
                    ensure_subtask_transition(
                        subtask.kind(),
                        subtask.state(),
                        SubtaskState::ReadyForApply,
                    )?;
                    let queue_id = enqueue_approved_subtask_for_apply_tx(
                        tx,
                        &req.session_token,
                        req.subtask_id.as_str(),
                        req.artifact_digest.as_str(),
                        req.settlement_target,
                        now,
                        req.idempotency_key.as_str(),
                    )?
                    .ok_or(CoveyError::IllegalTransition {
                        from: subtask.state().into(),
                        to: SubtaskState::ReadyForApply.into(),
                        object: ObjectType::Subtask,
                    })?;
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

    /// Reconciles approved or ready-for-apply artifacts into queued apply items.
    pub fn reconcile_apply_queue(
        &self,
        req: ReconcileApplyQueueReq,
    ) -> Result<ApplyQueueReconcileResult> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "reconcile_apply_queue",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(
                        tx,
                        &req.session_token,
                        &[SessionRole::Orchestrator, SessionRole::ApplyGate],
                    )?;
                    let approved_queue_ids =
                        enqueue_approved_apply_candidates_tx(tx, &req.session_token, now)?;
                    let ready_queue_ids =
                        enqueue_orphaned_ready_for_apply_items_tx(tx, &req.session_token, now)?;
                    let approved_enqueued_count = approved_queue_ids.len();
                    let ready_for_apply_enqueued_count = ready_queue_ids.len();
                    let queue_ids = approved_queue_ids
                        .into_iter()
                        .chain(ready_queue_ids)
                        .collect::<Vec<_>>();
                    ApplyQueueReconcileResult::new(
                        approved_enqueued_count,
                        ready_for_apply_enqueued_count,
                        queue_ids,
                    )
                    .map_err(|reason| CoveyError::InvalidObservabilityRow { reason })
                },
            )
        });
        self.log_operation(
            "reconcile_apply_queue",
            &req.session_token,
            started_at,
            &result,
            |result| {
                result
                    .queue_ids
                    .iter()
                    .map(|queue_id| format!("queue:{queue_id}"))
                    .collect()
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

    /// Returns one ready-queue item by id.
    pub fn ready_queue_item(&self, queue_id: &str) -> Result<ReadyQueueItem> {
        let started_at = Instant::now();
        let result = self.with_read_tx(|tx| {
            ensure_length("queue_id", queue_id, MAX_OBJECT_ID_LEN)?;
            load_queue_item_tx(tx, queue_id)
        });
        self.log_operation("ready_queue_item", "system", started_at, &result, |item| {
            vec![format!("queue:{}", item.queue_id())]
        });
        result
    }

    /// Returns read-only, ordered apply-queue candidates for deterministic scheduling.
    pub fn ready_queue_candidates(&self, limit: usize) -> Result<Vec<ReadyQueueCandidate>> {
        let started_at = Instant::now();
        let result = self.fetch_ready_queue(limit).and_then(|items| {
            items
                .iter()
                .map(ReadyQueueCandidate::from_item)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|reason| CoveyError::InvalidObservabilityRow { reason })
        });
        self.log_operation(
            "ready_queue_candidates",
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
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::ApplyGate])?;
                    ensure_positive_lease_duration(
                        "lease_duration_ms",
                        req.lease_duration_ms.get(),
                    )?;
                    requeue_stale_ready_queue_claims(tx, lease_now, now)?;
                    enqueue_orphaned_ready_for_apply_items_tx(tx, &req.session_token, now)?;
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

            let artifact = load_artifact_tx(tx, req.artifact_digest.as_str())?;

            Ok(LandingAuthorizationStatus::accepted(
                req.queue_id,
                req.artifact_digest,
                artifact.artifact_kind,
                artifact.base_rev,
                artifact.manifest_path,
                artifact.changed_paths_digest,
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

    /// Records the final commit oid as a settlement receipt after landing.
    pub fn record_landing_receipt(&self, req: RecordLandingReceiptReq) -> Result<()> {
        let started_at = Instant::now();
        let session_token_for_log = req.session_token.clone();
        let result = self.with_write_tx(|tx, now| {
            require_role(
                tx,
                &req.session_token,
                &[SessionRole::ApplyGate, SessionRole::Orchestrator],
            )?;
            let item = load_queue_item_tx(tx, req.queue_id.as_str())?;
            let queue_id = QueueId::parse(item.queue_id())?;
            if item.state() != ReadyQueueState::Applied {
                return Err(CoveyError::ApplyGateEvidenceMissing {
                    queue_id,
                    reason: "landing receipt requires an applied queue item".to_owned(),
                });
            }
            if item.artifact_digest() != req.artifact_digest {
                return Err(CoveyError::ApplyGateEvidenceMissing {
                    queue_id,
                    reason: "landing receipt artifact digest does not match queue item".to_owned(),
                });
            }
            if item.claim_fence_seq() != Some(req.claim_fence_seq.get()) {
                return Err(CoveyError::ApplyGateEvidenceMissing {
                    queue_id,
                    reason: "landing receipt claim fence does not match queue item".to_owned(),
                });
            }
            let existing_receipt = tx
                .query_row(
                    r#"
                    SELECT target_ref, landed_commit_oid
                    FROM landing_receipts
                    WHERE queue_id = ?1
                      AND artifact_digest = ?2
                    LIMIT 1
                    "#,
                    params![req.queue_id.as_str(), req.artifact_digest.as_str()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            if let Some((target_ref, landed_commit_oid)) = existing_receipt {
                if target_ref == req.target_ref.as_str()
                    && landed_commit_oid == req.landed_commit_oid.as_str()
                {
                    return Ok(());
                }
                return Err(CoveyError::ApplyGateEvidenceMissing {
                    queue_id,
                    reason: "landing receipt already recorded with different target or commit"
                        .to_owned(),
                });
            }
            tx.execute(
                r#"
                INSERT INTO landing_receipts (
                    queue_id, artifact_digest, claim_fence_seq, target_ref, landed_commit_oid,
                    recorded_by_session, created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
                params![
                    req.queue_id.as_str(),
                    req.artifact_digest.as_str(),
                    req.claim_fence_seq,
                    req.target_ref.as_str(),
                    req.landed_commit_oid.as_str(),
                    req.session_token.as_str(),
                    now,
                ],
            )?;
            Ok(())
        });
        self.log_operation(
            "record_landing_receipt",
            &session_token_for_log,
            started_at,
            &result,
            |_| vec![format!("queue:{}", req.queue_id)],
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

fn enqueue_orphaned_ready_for_apply_items_tx(
    tx: &Transaction<'_>,
    session_token: &SessionToken,
    now: i64,
) -> Result<Vec<String>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT s.subtask_id, s.artifact_digest
        FROM subtasks s
        JOIN artifacts a ON a.artifact_digest = s.artifact_digest
        JOIN meta_tasks m ON m.meta_task_id = s.meta_task_id
        WHERE s.kind = ?1
          AND s.state = ?2
          AND s.artifact_digest IS NOT NULL
          AND a.artifact_kind != ?10
          AND m.state NOT IN (?3, ?4)
          AND EXISTS (
              SELECT 1
              FROM reviews r
              WHERE r.subtask_id = s.subtask_id
                AND r.artifact_digest = s.artifact_digest
                AND r.state = ?5
                AND r.verdict = ?6
          )
          AND NOT EXISTS (
              SELECT 1
              FROM ready_queue q
              WHERE q.subtask_id = s.subtask_id
                AND q.artifact_digest = s.artifact_digest
                AND q.state IN (?7, ?8, ?9)
          )
        ORDER BY s.updated_at ASC, s.created_at ASC
        "#,
    )?;
    let rows = stmt.query_map(
        params![
            subtask_kind_name(SubtaskKind::Work),
            subtask_state_name(SubtaskState::ReadyForApply),
            meta_task_state_name(MetaTaskState::Completed),
            meta_task_state_name(MetaTaskState::Cancelled),
            review_state_name(ReviewState::Decided),
            review_verdict_name(ReviewVerdict::Approve),
            ready_queue_state_name(ReadyQueueState::Queued),
            ready_queue_state_name(ReadyQueueState::InFlight),
            ready_queue_state_name(ReadyQueueState::Applied),
            artifact_kind_name(ArtifactKind::FindingsBundle),
        ],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )?;
    let orphaned = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut queue_ids = Vec::with_capacity(orphaned.len());
    for (subtask_id, artifact_digest) in orphaned {
        if let Some(queue_id) = insert_ready_queue_item_tx(
            tx,
            session_token,
            &subtask_id,
            &artifact_digest,
            SettlementTarget::Canonical,
            now,
            format!("auto-requeue-ready-for-apply:{subtask_id}:{artifact_digest}"),
        )? {
            queue_ids.push(queue_id);
        }
    }
    Ok(queue_ids)
}

fn enqueue_approved_apply_candidates_tx(
    tx: &Transaction<'_>,
    session_token: &SessionToken,
    now: i64,
) -> Result<Vec<String>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT s.subtask_id, s.artifact_digest
        FROM subtasks s
        JOIN artifacts a ON a.artifact_digest = s.artifact_digest
        JOIN meta_tasks m ON m.meta_task_id = s.meta_task_id
        WHERE s.kind = ?1
          AND s.state = ?2
          AND s.artifact_digest IS NOT NULL
          AND a.artifact_kind != ?10
          AND m.state NOT IN (?3, ?4)
          AND EXISTS (
              SELECT 1
              FROM reviews r
              WHERE r.subtask_id = s.subtask_id
                AND r.artifact_digest = s.artifact_digest
                AND r.state = ?5
                AND r.verdict = ?6
          )
          AND NOT EXISTS (
              SELECT 1
              FROM ready_queue q
              WHERE q.subtask_id = s.subtask_id
                AND q.artifact_digest = s.artifact_digest
                AND q.state IN (?7, ?8, ?9)
          )
        ORDER BY s.updated_at ASC, s.created_at ASC
        "#,
    )?;
    let rows = stmt.query_map(
        params![
            subtask_kind_name(SubtaskKind::Work),
            subtask_state_name(SubtaskState::Approved),
            meta_task_state_name(MetaTaskState::Completed),
            meta_task_state_name(MetaTaskState::Cancelled),
            review_state_name(ReviewState::Decided),
            review_verdict_name(ReviewVerdict::Approve),
            ready_queue_state_name(ReadyQueueState::Queued),
            ready_queue_state_name(ReadyQueueState::InFlight),
            ready_queue_state_name(ReadyQueueState::Applied),
            artifact_kind_name(ArtifactKind::FindingsBundle),
        ],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )?;
    let approved = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut queue_ids = Vec::with_capacity(approved.len());
    for (subtask_id, artifact_digest) in approved {
        if let Some(queue_id) = enqueue_approved_subtask_for_apply_tx(
            tx,
            session_token,
            &subtask_id,
            &artifact_digest,
            SettlementTarget::Canonical,
            now,
            format!("auto-enqueue-approved:{subtask_id}:{artifact_digest}"),
        )? {
            queue_ids.push(queue_id);
        }
    }
    Ok(queue_ids)
}

pub(crate) fn enqueue_approved_subtask_for_apply_tx(
    tx: &Transaction<'_>,
    session_token: &SessionToken,
    subtask_id: &str,
    artifact_digest: &str,
    settlement_target: SettlementTarget,
    now: i64,
    idempotency_key: impl Into<String>,
) -> Result<Option<String>> {
    ensure_applyable_artifact_tx(tx, artifact_digest)?;

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
            subtask_id,
            ready_queue_state_name(ReadyQueueState::Superseded),
            now,
            ready_queue_state_name(ReadyQueueState::Queued),
            ready_queue_state_name(ReadyQueueState::InFlight)
        ],
    )?;

    let updated = tx.execute(
        "UPDATE subtasks SET state = ?2, updated_at = ?3 WHERE subtask_id = ?1 AND state = ?4 AND artifact_digest = ?5",
        params![
            subtask_id,
            subtask_state_name(SubtaskState::ReadyForApply),
            now,
            subtask_state_name(SubtaskState::Approved),
            artifact_digest
        ],
    )?;
    if updated == 0 {
        let already_queued = tx
            .query_row(
                r#"
            SELECT 1
            FROM subtasks s
            JOIN ready_queue q ON q.subtask_id = s.subtask_id
            WHERE s.subtask_id = ?1
              AND s.artifact_digest = ?2
              AND s.state = ?3
              AND q.artifact_digest = ?2
              AND q.state IN (?4, ?5, ?6)
            LIMIT 1
            "#,
                params![
                    subtask_id,
                    artifact_digest,
                    subtask_state_name(SubtaskState::ReadyForApply),
                    ready_queue_state_name(ReadyQueueState::Queued),
                    ready_queue_state_name(ReadyQueueState::InFlight),
                    ready_queue_state_name(ReadyQueueState::Applied),
                ],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if already_queued {
            return Ok(None);
        }
        return Err(CoveyError::IllegalTransition {
            from: SubtaskState::Approved.into(),
            to: SubtaskState::ReadyForApply.into(),
            object: ObjectType::Subtask,
        });
    }

    insert_ready_queue_item_tx(
        tx,
        session_token,
        subtask_id,
        artifact_digest,
        settlement_target,
        now,
        idempotency_key,
    )
}

fn insert_ready_queue_item_tx(
    tx: &Transaction<'_>,
    session_token: &SessionToken,
    subtask_id: &str,
    artifact_digest: &str,
    settlement_target: SettlementTarget,
    now: i64,
    idempotency_key: impl Into<String>,
) -> Result<Option<String>> {
    let existing_queue_id = active_queue_id_for_artifact_tx(tx, subtask_id, artifact_digest)?;
    if existing_queue_id.is_some() {
        return Ok(None);
    }

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
            &queue_id,
            artifact_digest,
            subtask_id,
            settlement_target_name(settlement_target),
            ready_queue_state_name(ReadyQueueState::Queued),
            now,
        ],
    )?;
    let enqueue_event = EnqueueForApplyReq::try_from_raw_parts(
        session_token.as_str(),
        artifact_digest.to_owned(),
        subtask_id.to_owned(),
        settlement_target,
        idempotency_key,
    )?;
    append_session_event(
        tx,
        EventType::ReadyQueueEnqueued,
        ObjectType::ReadyQueue,
        &queue_id,
        session_token.as_str(),
        &enqueue_event,
        now,
    )?;
    Ok(Some(queue_id))
}

const fn artifact_kind_name(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::PatchBundle => "patch_bundle",
        ArtifactKind::IsolatedCommitRef => "isolated_commit_ref",
        ArtifactKind::TreeBundle => "tree_bundle",
        ArtifactKind::FindingsBundle => "findings_bundle",
        ArtifactKind::VerificationBundle => "verification_bundle",
    }
}

const fn is_applyable_artifact_kind(kind: ArtifactKind) -> bool {
    matches!(
        kind,
        ArtifactKind::PatchBundle
            | ArtifactKind::IsolatedCommitRef
            | ArtifactKind::TreeBundle
            | ArtifactKind::VerificationBundle
    )
}

fn ensure_applyable_artifact_tx(tx: &Transaction<'_>, artifact_digest: &str) -> Result<()> {
    let artifact = load_artifact_tx(tx, artifact_digest)?;
    if is_applyable_artifact_kind(artifact.artifact_kind) {
        return Ok(());
    }
    Err(CoveyError::IllegalTransition {
        from: SubtaskState::Approved.into(),
        to: SubtaskState::ReadyForApply.into(),
        object: ObjectType::Subtask,
    })
}

fn active_queue_id_for_artifact_tx(
    tx: &Transaction<'_>,
    subtask_id: &str,
    artifact_digest: &str,
) -> Result<Option<String>> {
    tx.query_row(
        r#"
        SELECT queue_id
        FROM ready_queue
        WHERE subtask_id = ?1
          AND artifact_digest = ?2
          AND state IN (?3, ?4, ?5)
        ORDER BY enqueued_at ASC
        LIMIT 1
        "#,
        params![
            subtask_id,
            artifact_digest,
            ready_queue_state_name(ReadyQueueState::Queued),
            ready_queue_state_name(ReadyQueueState::InFlight),
            ready_queue_state_name(ReadyQueueState::Applied),
        ],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(Into::into)
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
    let left_provider_run = provider_run_ref(left);
    let right_provider_run = provider_run_ref(right);
    if left_provider_run
        .zip(right_provider_run)
        .is_some_and(|(left, right)| left == right)
    {
        return Err(CoveyError::ApplyGateEvidenceMissing {
            queue_id: queue_id.clone(),
            reason: format!("{left_role} and {right_role} provider run ids are not separated"),
        });
    }
    if runtime_ref(left) == runtime_ref(right) {
        return Err(CoveyError::ApplyGateEvidenceMissing {
            queue_id: queue_id.clone(),
            reason: format!("{left_role} and {right_role} runtime refs are not separated"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        CommandTranscriptDigest, ModelId, ProviderId, ProviderRunId, ProviderRunIdIssuer,
        RuntimeProcessId, TimestampMs,
    };

    fn attestation(provider_run_id: &str) -> RuntimeAttestation {
        RuntimeAttestation::try_from_parts(
            SessionToken::parse(format!("session-{provider_run_id}")).expect("valid session token"),
            format!("principal-{provider_run_id}"),
            format!("instance-{provider_run_id}"),
            SessionRole::Executor,
            ProviderId::parse("codex").expect("valid provider"),
            ModelId::parse("gpt-5").expect("valid model"),
            ProviderRunId::parse(provider_run_id)
                .expect("valid provider run id")
                .to_string(),
            ProviderRunIdIssuer::parse("codex-cli")
                .expect("valid provider run issuer")
                .to_string(),
            Some(
                RuntimeProcessId::parse(format!("codex-shell-{provider_run_id}"))
                    .expect("valid process id")
                    .to_string(),
            ),
            None,
            CommandTranscriptDigest::parse(format!("blake3:{provider_run_id}-transcript"))
                .expect("valid transcript digest"),
            TimestampMs::parse(1).expect("valid started_at"),
            TimestampMs::parse(2).expect("valid ended_at"),
            TimestampMs::parse(3).expect("valid recorded_at"),
        )
        .expect("valid runtime attestation")
    }

    #[test]
    fn runtime_actor_separation_accepts_distinct_provider_runs_with_shared_local_placeholders() {
        let queue_id = QueueId::parse("queue-test").expect("valid queue id");
        let left = attestation("provider-run-left");
        let right = attestation("provider-run-right");

        require_runtime_actor_separation(&queue_id, "producer", &left, "reviewer", &right)
            .expect("distinct provider runs prove runtime actor separation");
    }

    #[test]
    fn runtime_actor_separation_rejects_matching_provider_runs() {
        let queue_id = QueueId::parse("queue-test").expect("valid queue id");
        let left = attestation("provider-run-shared");
        let right = attestation("provider-run-shared");

        let error =
            require_runtime_actor_separation(&queue_id, "producer", &left, "reviewer", &right)
                .expect_err("matching provider runs must fail separation");
        assert!(
            error
                .to_string()
                .contains("provider run ids are not separated")
        );
    }
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

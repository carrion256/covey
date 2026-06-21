#![cfg_attr(coverage_nightly, coverage(off))]

use std::time::Instant;

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        ApplyQueueReconcileResult, ArtifactKind, BeginOpenSpecArchiveCleanupReq, ClaimId,
        ClaimReadyQueueReq, ClaimResult, ClaimState, EnqueueForApplyReq, EventType, FenceSeq,
        FindingsDigest, FinishOpenSpecArchiveCleanupReq, LandingAuthorizationStatus,
        LeaseDeadlineMs, MarkAppliedReq, MarkInFlightReq, MetaTaskState, ObjectType,
        OpenSpecArchiveCleanupClaim, OpenSpecArchiveCleanupFinish, OpenSpecArchiveEligibility,
        OpenSpecArchiveStatus, OpenSpecArchiveStatusState, OpenSpecChangeId, QueueId,
        ReadyQueueCandidate, ReadyQueueClaim, ReadyQueueItem, ReadyQueueMetrics, ReadyQueueState,
        ReconcileApplyQueueReq, RecordApplyGateBlockerReq, RecordApplyVerificationReq,
        RecordLandingReceiptReq, RecordOpenSpecArchiveStatusReq,
        RecordSettlementReconcileBlockerReq, ReleaseClaimReq, ReviewId, ReviewState, ReviewVerdict,
        RuntimeAttestation, ScopeClass, Session, SessionRole, SessionToken, SettlementTarget,
        SubtaskKind, SubtaskState, SubtaskTitle, SubtaskView, SupersedeQueueItemReq,
        VerifyLandingAuthorizationReq, apply_gate_blocker_kind_name, claim_state_name,
        meta_task_state_name, openspec_archive_status_state_name, ready_queue_state_name,
        reservation_state_name, review_state_name, review_verdict_name, scope_class_name,
        settlement_reconcile_reason_name, subtask_kind_name, subtask_state_name,
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
        issue_fence_seq, require_active_session, require_current_claim, require_role,
        require_runtime_attestation, require_session_can_enqueue,
    },
};

const fn settlement_target_name(target: SettlementTarget) -> &'static str {
    match target {
        SettlementTarget::Canonical => "canonical",
    }
}

const OPENSPEC_SCOPE_WITH_FOLLOWUPS_CTE: &str = r#"
        WITH RECURSIVE openspec_scope(subtask_id) AS (
            SELECT subtask_id
            FROM openspec_subtask_scope
            WHERE openspec_change_id = ?1
            UNION
            SELECT followup.followup_subtask_id
            FROM review_followup_subtasks followup
            JOIN openspec_scope scope
              ON followup.source_subtask_id = scope.subtask_id
        )
"#;

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
            let lease_now = advance_lease_clock(tx, now)?;
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
                    requeue_stale_ready_queue_claims(tx, lease_now, now)?;
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

    /// Records native apply-gate blocker evidence for the current apply attempt.
    pub fn record_apply_gate_blocker(&self, req: RecordApplyGateBlockerReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "record_apply_gate_blocker",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    let session = require_role(tx, &req.session_token, &[SessionRole::ApplyGate])?;
                    require_runtime_attestation(tx, &session)?;
                    ensure_length("queue_id", &req.queue_id, MAX_OBJECT_ID_LEN)?;
                    ensure_length("artifact_digest", &req.artifact_digest, MAX_DIGEST_LEN)?;
                    ensure_length("verifier", &req.verifier, MAX_OBJECT_ID_LEN)?;
                    ensure_length("reason", &req.reason, MAX_OBJECT_ID_LEN)?;
                    ensure_length("evidence_id", &req.evidence_id, MAX_OBJECT_ID_LEN)?;

                    let item = load_queue_item_tx(tx, req.queue_id.as_str())?;
                    let queue_id = QueueId::parse(item.queue_id())?;
                    if item.state() != ReadyQueueState::InFlight
                        || item.artifact_digest() != req.artifact_digest.as_str()
                        || item.claim_fence_seq() != Some(req.claim_fence_seq.get())
                    {
                        return Err(CoveyError::ApplyGateEvidenceMissing {
                            queue_id,
                            reason:
                                "apply-gate blocker must target the current in-flight queue fence"
                                    .to_owned(),
                        });
                    }
                    let live_evidence = require_live_apply_gate_evidence(tx, &item, &session)?;

                    tx.execute(
                        r#"
                        INSERT INTO apply_gate_blockers (
                            queue_id, artifact_digest, review_id, findings_digest,
                            claim_fence_seq, verifier, blocker_kind, reason, evidence_id,
                            recorded_by_session, created_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                        "#,
                        params![
                            req.queue_id.as_str(),
                            req.artifact_digest.as_str(),
                            live_evidence.review_id.as_str(),
                            live_evidence.findings_digest.as_str(),
                            req.claim_fence_seq,
                            req.verifier.as_str(),
                            apply_gate_blocker_kind_name(req.blocker_kind),
                            req.reason.as_str(),
                            req.evidence_id.as_str(),
                            req.session_token.as_str(),
                            now,
                        ],
                    )?;
                    append_session_event(
                        tx,
                        EventType::ApplyGateBlockerRecorded,
                        ObjectType::ApplyGateBlocker,
                        req.evidence_id.as_str(),
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(())
                },
            )
        });
        self.log_operation(
            "record_apply_gate_blocker",
            &req.session_token,
            started_at,
            &result,
            |_| vec![format!("queue:{}", req.queue_id)],
        );
        result
    }

    /// Records Authority settlement reconcile evidence without changing queue state.
    pub fn record_settlement_reconcile_blocker(
        &self,
        req: RecordSettlementReconcileBlockerReq,
    ) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "record_settlement_reconcile_blocker",
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
                    ensure_length("artifact_digest", &req.artifact_digest, MAX_DIGEST_LEN)?;
                    ensure_length(
                        "authority_evidence_id",
                        &req.authority_evidence_id,
                        MAX_OBJECT_ID_LEN,
                    )?;

                    let item = load_queue_item_tx(tx, req.queue_id.as_str())?;
                    let queue_id = QueueId::parse(item.queue_id())?;
                    if !matches!(
                        item.state(),
                        ReadyQueueState::InFlight | ReadyQueueState::Applied
                    ) {
                        return Err(CoveyError::ApplyGateEvidenceMissing {
                            queue_id,
                            reason: "settlement reconcile evidence must target an in-flight or applied queue item".to_owned(),
                        });
                    }
                    if item.artifact_digest() != req.artifact_digest.as_str()
                        || item.claim_fence_seq() != Some(req.claim_fence_seq.get())
                    {
                        return Err(CoveyError::ApplyGateEvidenceMissing {
                            queue_id,
                            reason:
                                "settlement reconcile evidence does not match queue artifact or fence"
                                    .to_owned(),
                        });
                    }
                    let review_evidence = load_approved_review_evidence_tx(tx, &item)?;

                    tx.execute(
                        r#"
                        INSERT INTO settlement_reconcile_blockers (
                            queue_id, artifact_digest, review_id, findings_digest,
                            claim_fence_seq, reconcile_reason, authority_evidence_id,
                            recorded_by_session, created_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                        "#,
                        params![
                            req.queue_id.as_str(),
                            req.artifact_digest.as_str(),
                            review_evidence.review_id.as_str(),
                            review_evidence.findings_digest.as_str(),
                            req.claim_fence_seq,
                            settlement_reconcile_reason_name(req.reconcile_reason),
                            req.authority_evidence_id.as_str(),
                            req.session_token.as_str(),
                            now,
                        ],
                    )?;
                    append_session_event(
                        tx,
                        EventType::SettlementReconcileBlockerRecorded,
                        ObjectType::SettlementReconcileBlocker,
                        req.authority_evidence_id.as_str(),
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(())
                },
            )
        });
        self.log_operation(
            "record_settlement_reconcile_blocker",
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
                    record_applied_openspec_archive_blocker_tx(
                        tx,
                        &req.session_token,
                        &item,
                        now,
                        req.idempotency_key.as_str(),
                    )?;
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

    /// Records or resolves the OpenSpec archive cleanup status for an applied queue item.
    pub fn record_openspec_archive_status(
        &self,
        req: RecordOpenSpecArchiveStatusReq,
    ) -> Result<OpenSpecArchiveStatus> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "record_openspec_archive_status",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::Orchestrator])?;
                    ensure_length("queue_id", &req.queue_id, MAX_OBJECT_ID_LEN)?;
                    ensure_length("artifact_digest", &req.artifact_digest, MAX_DIGEST_LEN)?;
                    ensure_length(
                        "openspec_change_id",
                        &req.openspec_change_id,
                        MAX_OBJECT_ID_LEN,
                    )?;
                    if let Some(reason) = &req.blocked_reason {
                        ensure_length("blocked_reason", reason, MAX_OBJECT_ID_LEN)?;
                    }
                    if let Some(digest) = &req.archive_proof_digest {
                        ensure_length("archive_proof_digest", digest, MAX_DIGEST_LEN)?;
                    }

                    let item = load_queue_item_tx(tx, req.queue_id.as_str())?;
                    let queue_id = QueueId::parse(item.queue_id())?;
                    if item.state() != ReadyQueueState::Applied {
                        return Err(CoveyError::ApplyGateEvidenceMissing {
                            queue_id: queue_id.clone(),
                            reason: "OpenSpec archive status requires an applied queue item"
                                .to_owned(),
                        });
                    }
                    if item.artifact_digest() != req.artifact_digest {
                        return Err(CoveyError::ApplyGateEvidenceMissing {
                            queue_id: queue_id.clone(),
                            reason:
                                "OpenSpec archive status artifact digest does not match queue item"
                                    .to_owned(),
                        });
                    }
                    let scope_change_id = openspec_change_id_for_subtask_tx(tx, item.subtask_id())?
                        .ok_or_else(|| CoveyError::ApplyGateEvidenceMissing {
                            queue_id: queue_id.clone(),
                            reason: "OpenSpec archive status requires imported subtask scope"
                                .to_owned(),
                        })?;
                    if scope_change_id != req.openspec_change_id {
                        return Err(CoveyError::ApplyGateEvidenceMissing {
                            queue_id,
                            reason:
                                "OpenSpec archive status change id does not match imported scope"
                                    .to_owned(),
                        });
                    }

                    let existing =
                        load_openspec_archive_status_optional_tx(tx, req.queue_id.as_str())?;
                    if let Some(existing) = existing {
                        if archive_status_matches_req(&existing, &req) {
                            return Ok(existing);
                        }
                        if existing.state == OpenSpecArchiveStatusState::Blocked
                            && req.state == OpenSpecArchiveStatusState::Archived
                        {
                            update_openspec_archive_status_tx(tx, &req, item.subtask_id(), now)?;
                            append_session_event(
                                tx,
                                EventType::OpenSpecArchiveStatusRecorded,
                                ObjectType::ReadyQueue,
                                &req.queue_id,
                                &req.session_token,
                                &req,
                                now,
                            )?;
                            return load_openspec_archive_status_tx(tx, req.queue_id.as_str());
                        }
                        return Err(CoveyError::IllegalTransition {
                            from: existing.state.into(),
                            to: req.state.into(),
                            object: ObjectType::ReadyQueue,
                        });
                    }

                    insert_openspec_archive_status_tx(tx, &req, item.subtask_id(), now, now)?;
                    append_session_event(
                        tx,
                        EventType::OpenSpecArchiveStatusRecorded,
                        ObjectType::ReadyQueue,
                        &req.queue_id,
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    load_openspec_archive_status_tx(tx, req.queue_id.as_str())
                },
            )
        });
        self.log_operation(
            "record_openspec_archive_status",
            &req.session_token,
            started_at,
            &result,
            |status| vec![format!("queue:{}", status.queue_id())],
        );
        result
    }

    /// Returns applied OpenSpec queue items still blocked on archive cleanup.
    pub fn open_openspec_archive_blockers(
        &self,
        limit: usize,
    ) -> Result<Vec<OpenSpecArchiveStatus>> {
        let started_at = Instant::now();
        let result = if limit == 0 {
            Ok(Vec::new())
        } else {
            self.with_read_conn(|conn| {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT queue_id, subtask_id, artifact_digest, openspec_change_id, state,
                           blocked_reason, archive_proof_digest, recorded_by_session,
                           created_at, updated_at
                    FROM openspec_archive_status
                    WHERE state = ?1
                    ORDER BY updated_at ASC, queue_id ASC
                    LIMIT ?2
                    "#,
                )?;
                let rows = stmt.query_map(
                    params![
                        openspec_archive_status_state_name(OpenSpecArchiveStatusState::Blocked),
                        limit as i64
                    ],
                    deserialize_row::<OpenSpecArchiveStatus>,
                )?;
                collect_rows(rows)
            })
        };
        self.log_operation(
            "open_openspec_archive_blockers",
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

    /// Returns Covey-owned archive eligibility facts for one OpenSpec change.
    pub fn openspec_archive_eligibility(
        &self,
        openspec_change_id: &str,
    ) -> Result<OpenSpecArchiveEligibility> {
        let started_at = Instant::now();
        let result = self.with_read_tx(|tx| {
            let change_id = OpenSpecChangeId::parse(openspec_change_id.to_owned())?;
            archive_eligibility_for_change_tx(tx, &change_id)
        });
        self.log_operation(
            "openspec_archive_eligibility",
            "system",
            started_at,
            &result,
            |eligibility| {
                vec![format!(
                    "openspec-change:{}",
                    eligibility.openspec_change_id
                )]
            },
        );
        result
    }

    /// Creates or reuses an orchestrator-owned cleanup claim for one OpenSpec archive.
    pub fn begin_openspec_archive_cleanup(
        &self,
        req: BeginOpenSpecArchiveCleanupReq,
    ) -> Result<OpenSpecArchiveCleanupClaim> {
        const CLEANUP_LEASE_MS: i64 = 600_000;

        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "begin_openspec_archive_cleanup",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::Orchestrator])?;
                    let eligibility =
                        archive_eligibility_for_change_tx(tx, &req.openspec_change_id)?;
                    if !eligibility.safe_to_archive {
                        return Err(CoveyError::ApplyGateEvidenceMissing {
                            queue_id: QueueId::parse("queue-openspec-archive-cleanup")?,
                            reason: "OpenSpec archive cleanup requires all scoped subtasks applied and at least one open archive blocker".to_owned(),
                        });
                    }
                    if let Some(existing) = load_cleanup_claim_for_change_tx(
                        tx,
                        &req.openspec_change_id,
                        eligibility.open_archive_blockers.clone(),
                    )? {
                        return Ok(existing);
                    }

                    let meta_task_id = format!("openspec:{}", req.openspec_change_id);
                    ensure_meta_task_is_schedulable(tx, &meta_task_id)?;
                    let cleanup_subtask_id =
                        format!("openspec:{}:cleanup:archive", req.openspec_change_id);
                    let title = SubtaskTitle::parse(format!(
                        "Archive OpenSpec change {}",
                        req.openspec_change_id
                    ))?;
                    tx.execute(
                        r#"
                        INSERT INTO subtasks (
                            subtask_id, meta_task_id, title, kind,
                            review_target_subtask_id, review_target_artifact_digest,
                            state, current_claim_id, artifact_digest, priority,
                            created_at, updated_at
                        ) VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, NULL, NULL, 1000, ?6, ?6)
                        ON CONFLICT(subtask_id) DO NOTHING
                        "#,
                        params![
                            cleanup_subtask_id,
                            meta_task_id,
                            title.as_str(),
                            subtask_kind_name(SubtaskKind::Cleanup),
                            subtask_state_name(SubtaskState::Available),
                            now,
                        ],
                    )?;
                    let fence_seq = issue_fence_seq(tx, &cleanup_subtask_id)?;
                    let claim_id = crate::model::make_id("claim");
                    let lease_deadline =
                        LeaseDeadlineMs::parse(lease_now + CLEANUP_LEASE_MS)?;
                    tx.execute(
                        r#"
                        INSERT INTO claims (
                            claim_id, subtask_id, owner_session_token, fence_seq,
                            lease_deadline, state, created_at, updated_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                        "#,
                        params![
                            claim_id,
                            cleanup_subtask_id,
                            req.session_token.as_str(),
                            fence_seq,
                            lease_deadline,
                            claim_state_name(ClaimState::Held),
                            now,
                        ],
                    )?;
                    tx.execute(
                        r#"
                        UPDATE subtasks
                        SET state = ?2, current_claim_id = ?3, updated_at = ?4
                        WHERE subtask_id = ?1
                          AND state = ?5
                          AND current_claim_id IS NULL
                        "#,
                        params![
                            cleanup_subtask_id,
                            subtask_state_name(SubtaskState::InProgress),
                            claim_id,
                            now,
                            subtask_state_name(SubtaskState::Available),
                        ],
                    )?;
                    let archive_paths = req
                        .paths()
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>();
                    for archive_path in &archive_paths {
                        let reservation_id = crate::model::make_id("reservation");
                        tx.execute(
                            r#"
                            INSERT INTO reservations (
                                reservation_id, owner_subtask_id, scope_class, scope_key,
                                lease_deadline, state, created_at, updated_at
                            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                            "#,
                            params![
                                reservation_id,
                                cleanup_subtask_id,
                                scope_class_name(ScopeClass::Subtree),
                                archive_path,
                                lease_deadline,
                                reservation_state_name(crate::model::ReservationState::Active),
                                now,
                            ],
                        )?;
                        let cleanup_queue_id = QueueId::parse("queue-openspec-archive-cleanup")?;
                        let reservation_req =
                            crate::model::RequestReservationReq::try_from_raw_parts(
                                req.session_token.as_str(),
                                cleanup_subtask_id.clone(),
                                ScopeClass::Subtree,
                                archive_path.clone(),
                                Vec::new(),
                                CLEANUP_LEASE_MS,
                                format!(
                                    "begin-openspec-archive-cleanup-reservation:{}:{}",
                                    req.openspec_change_id, archive_path
                                ),
                            )
                            .map_err(|reason| CoveyError::ApplyGateEvidenceMissing {
                                queue_id: cleanup_queue_id,
                                reason,
                            })?;
                        append_session_event(
                            tx,
                            EventType::ReservationRequested,
                            ObjectType::Reservation,
                            &reservation_id,
                            req.session_token.as_str(),
                            &reservation_req,
                            now,
                        )?;
                    }
                    tx.execute(
                        r#"
                        INSERT INTO openspec_archive_cleanup_claims (
                            openspec_change_id, cleanup_subtask_id, cleanup_claim_id,
                            archive_paths_json, archive_proof_digest, recorded_by_session,
                            created_at, updated_at
                        ) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?6)
                        "#,
                        params![
                            req.openspec_change_id.as_str(),
                            cleanup_subtask_id,
                            claim_id,
                            serde_json::to_string(&archive_paths)?,
                            req.session_token.as_str(),
                            now,
                        ],
                    )?;
                    let claim_result = ClaimResult::new(
                        ClaimId::parse(claim_id.clone())?,
                        crate::model::SubtaskId::parse(cleanup_subtask_id.clone())?,
                        fence_seq,
                        lease_deadline,
                    );
                    append_session_event(
                        tx,
                        EventType::SubtaskClaimed,
                        ObjectType::Claim,
                        &claim_result.claim_id,
                        req.session_token.as_str(),
                        &claim_result,
                        now,
                    )?;
                    Ok(OpenSpecArchiveCleanupClaim::new(
                        req.openspec_change_id.clone(),
                        crate::model::SubtaskId::parse(cleanup_subtask_id)?,
                        ClaimId::parse(claim_id)?,
                        fence_seq,
                        req.paths().to_vec(),
                        eligibility.open_archive_blockers,
                    ))
                },
            )
        });
        self.log_operation(
            "begin_openspec_archive_cleanup",
            &req.session_token,
            started_at,
            &result,
            |claim| vec![format!("claim:{}", claim.cleanup_claim_id)],
        );
        result
    }

    /// Resolves all open OpenSpec archive blockers for one change with one proof digest.
    pub fn finish_openspec_archive_cleanup(
        &self,
        req: FinishOpenSpecArchiveCleanupReq,
    ) -> Result<OpenSpecArchiveCleanupFinish> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "finish_openspec_archive_cleanup",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::Orchestrator])?;
                    let cleanup_subtask_id = cleanup_subtask_id_for_change_tx(
                        tx,
                        &req.openspec_change_id,
                        &req.cleanup_claim_id,
                    )?;
                    let _claim = require_current_claim(
                        tx,
                        &req.session_token,
                        &req.cleanup_claim_id,
                        req.fence_seq,
                        now,
                    )?;
                    let open_blockers = load_open_openspec_archive_blockers_for_change_tx(
                        tx,
                        &req.openspec_change_id,
                    )?;
                    if open_blockers.is_empty() {
                        return archived_cleanup_finish_tx(tx, &req);
                    }
                    for blocker in &open_blockers {
                        let status_req = RecordOpenSpecArchiveStatusReq::try_from_raw_parts(
                            req.session_token.as_str(),
                            blocker.queue_id().to_owned(),
                            blocker.artifact_digest().to_owned(),
                            req.openspec_change_id.as_str().to_owned(),
                            OpenSpecArchiveStatusState::Archived,
                            None,
                            Some(req.archive_proof_digest.as_str().to_owned()),
                            format!(
                                "finish-openspec-archive-cleanup:{}:{}",
                                req.openspec_change_id,
                                blocker.queue_id()
                            ),
                        )?;
                        update_openspec_archive_status_tx(
                            tx,
                            &status_req,
                            blocker.subtask_id(),
                            now,
                        )?;
                        append_session_event(
                            tx,
                            EventType::OpenSpecArchiveStatusRecorded,
                            ObjectType::ReadyQueue,
                            blocker.queue_id(),
                            req.session_token.as_str(),
                            &status_req,
                            now,
                        )?;
                    }
                    tx.execute(
                        r#"
                        UPDATE openspec_archive_cleanup_claims
                        SET archive_proof_digest = ?2, recorded_by_session = ?3, updated_at = ?4
                        WHERE openspec_change_id = ?1
                          AND cleanup_claim_id = ?5
                          AND (archive_proof_digest IS NULL OR archive_proof_digest = ?2)
                        "#,
                        params![
                            req.openspec_change_id.as_str(),
                            req.archive_proof_digest.as_str(),
                            req.session_token.as_str(),
                            now,
                            req.cleanup_claim_id.as_str(),
                        ],
                    )?;
                    tx.execute(
                        r#"
                        UPDATE claims
                        SET state = ?2, updated_at = ?3
                        WHERE claim_id = ?1 AND state = ?4
                        "#,
                        params![
                            req.cleanup_claim_id.as_str(),
                            claim_state_name(ClaimState::Released),
                            now,
                            claim_state_name(ClaimState::Held),
                        ],
                    )?;
                    tx.execute(
                        r#"
                        UPDATE subtasks
                        SET state = ?2, current_claim_id = NULL, updated_at = ?3
                        WHERE subtask_id = ?1 AND current_claim_id = ?4
                        "#,
                        params![
                            cleanup_subtask_id.as_str(),
                            subtask_state_name(SubtaskState::Abandoned),
                            now,
                            req.cleanup_claim_id.as_str(),
                        ],
                    )?;
                    let release_req = ReleaseClaimReq::try_from_raw_parts(
                        req.session_token.as_str(),
                        req.cleanup_claim_id.as_str(),
                        req.fence_seq.get(),
                        format!(
                            "finish-openspec-archive-cleanup-release:{}",
                            req.openspec_change_id
                        ),
                    )?;
                    append_session_event(
                        tx,
                        EventType::ClaimReleased,
                        ObjectType::Claim,
                        req.cleanup_claim_id.as_str(),
                        req.session_token.as_str(),
                        &release_req,
                        now,
                    )?;
                    Ok(OpenSpecArchiveCleanupFinish {
                        openspec_change_id: req.openspec_change_id.clone(),
                        archive_proof_digest: req.archive_proof_digest.clone(),
                        archived_queue_ids: open_blockers
                            .iter()
                            .map(|blocker| blocker.queue_id.clone())
                            .collect(),
                        cleanup_subtask_id,
                        cleanup_claim_id: req.cleanup_claim_id.clone(),
                    })
                },
            )
        });
        self.log_operation(
            "finish_openspec_archive_cleanup",
            &req.session_token,
            started_at,
            &result,
            |finish| {
                finish
                    .archived_queue_ids
                    .iter()
                    .map(|queue_id| format!("queue:{queue_id}"))
                    .collect()
            },
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
            let session = require_role(
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
            if !matches!(
                item.state(),
                ReadyQueueState::InFlight | ReadyQueueState::Applied
            ) {
                return Err(CoveyError::ApplyGateEvidenceMissing {
                    queue_id,
                    reason: "landing authorization queue item is not in flight or applied"
                        .to_owned(),
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
            if item.state() == ReadyQueueState::InFlight {
                let queue_owner = item
                    .claimed_by_session_token()
                    .map(crate::model::SessionToken::parse)
                    .transpose()?
                    .ok_or_else(|| CoveyError::ApplyGateEvidenceMissing {
                        queue_id: queue_id.clone(),
                        reason: "landing authorization in-flight queue item has no owner"
                            .to_owned(),
                    })?;
                if queue_owner != session.session_token {
                    return Err(CoveyError::NotQueueClaimOwner {
                        session_token: session.session_token,
                        queue_owner,
                    });
                }
                let live_evidence = require_live_apply_gate_evidence(tx, &item, &session)?;
                require_recorded_apply_verification(
                    tx,
                    &item,
                    &live_evidence,
                    req.claim_fence_seq,
                )?;
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

    /// Records the final commit oid as a settlement receipt for the fenced landing attempt.
    pub fn record_landing_receipt(&self, req: RecordLandingReceiptReq) -> Result<()> {
        let started_at = Instant::now();
        let session_token_for_log = req.session_token.clone();
        let result = self.with_write_tx(|tx, now| {
            let session = require_role(
                tx,
                &req.session_token,
                &[SessionRole::ApplyGate, SessionRole::Orchestrator],
            )?;
            let item = load_queue_item_tx(tx, req.queue_id.as_str())?;
            let queue_id = QueueId::parse(item.queue_id())?;
            if !matches!(
                item.state(),
                ReadyQueueState::InFlight | ReadyQueueState::Applied
            ) {
                return Err(CoveyError::ApplyGateEvidenceMissing {
                    queue_id,
                    reason: "landing receipt requires an in-flight or applied queue item"
                        .to_owned(),
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
            if item.state() == ReadyQueueState::InFlight {
                let queue_owner = item
                    .claimed_by_session_token()
                    .map(crate::model::SessionToken::parse)
                    .transpose()?
                    .ok_or_else(|| CoveyError::ApplyGateEvidenceMissing {
                        queue_id: queue_id.clone(),
                        reason: "landing receipt in-flight queue item has no owner".to_owned(),
                    })?;
                if queue_owner != session.session_token {
                    return Err(CoveyError::NotQueueClaimOwner {
                        session_token: session.session_token,
                        queue_owner,
                    });
                }
                let live_evidence = require_live_apply_gate_evidence(tx, &item, &session)?;
                require_recorded_apply_verification(
                    tx,
                    &item,
                    &live_evidence,
                    req.claim_fence_seq,
                )?;
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

fn archive_eligibility_for_change_tx(
    tx: &Transaction<'_>,
    openspec_change_id: &OpenSpecChangeId,
) -> Result<OpenSpecArchiveEligibility> {
    let scoped_subtasks = load_openspec_scoped_subtasks_tx(tx, openspec_change_id)?;
    let open_archive_blockers =
        load_open_openspec_archive_blockers_for_change_tx(tx, openspec_change_id)?;
    let repaired_source_subtask_ids =
        load_openspec_repaired_source_subtask_ids_tx(tx, openspec_change_id)?;
    let pending_subtasks = scoped_subtasks
        .iter()
        .filter(|subtask| {
            !matches!(
                subtask.state(),
                SubtaskState::Applied | SubtaskState::Abandoned
            ) && !repaired_source_subtask_ids
                .iter()
                .any(|repaired| repaired == &subtask.subtask_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    Ok(OpenSpecArchiveEligibility::new(
        openspec_change_id.clone(),
        scoped_subtasks,
        open_archive_blockers,
        pending_subtasks,
    ))
}

fn load_openspec_scoped_subtasks_tx(
    tx: &Transaction<'_>,
    openspec_change_id: &OpenSpecChangeId,
) -> Result<Vec<SubtaskView>> {
    let mut stmt = tx.prepare(
        format!(
            r#"
        {OPENSPEC_SCOPE_WITH_FOLLOWUPS_CTE}
        SELECT s.subtask_id, s.meta_task_id, s.title, s.kind,
               s.review_target_subtask_id, s.review_target_artifact_digest,
               s.state, s.current_claim_id, s.artifact_digest, s.priority,
               s.created_at, s.updated_at
        FROM openspec_scope scope
        JOIN subtasks s ON s.subtask_id = scope.subtask_id
        ORDER BY s.created_at ASC, s.subtask_id ASC
        "#
        )
        .as_str(),
    )?;
    let rows = stmt.query_map(params![openspec_change_id.as_str()], |row| {
        let subtask = deserialize_row::<crate::model::SubtaskRow>(row)?;
        SubtaskView::try_from(subtask)
    })?;
    collect_rows(rows)
}

fn load_openspec_repaired_source_subtask_ids_tx(
    tx: &Transaction<'_>,
    openspec_change_id: &OpenSpecChangeId,
) -> Result<Vec<crate::model::SubtaskId>> {
    let mut stmt = tx.prepare(
        format!(
            r#"
        {OPENSPEC_SCOPE_WITH_FOLLOWUPS_CTE}
        SELECT DISTINCT followup.source_subtask_id
        FROM openspec_scope scope
        JOIN review_followup_subtasks followup
          ON followup.source_subtask_id = scope.subtask_id
        ORDER BY followup.source_subtask_id ASC
        "#
        )
        .as_str(),
    )?;
    let rows = stmt.query_map(params![openspec_change_id.as_str()], |row| {
        row.get::<_, String>(0).and_then(|raw| {
            crate::model::SubtaskId::parse(raw).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    err.into(),
                )
            })
        })
    })?;
    collect_rows(rows)
}

fn load_open_openspec_archive_blockers_for_change_tx(
    tx: &Transaction<'_>,
    openspec_change_id: &OpenSpecChangeId,
) -> Result<Vec<OpenSpecArchiveStatus>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT queue_id, subtask_id, artifact_digest, openspec_change_id, state,
               blocked_reason, archive_proof_digest, recorded_by_session,
               created_at, updated_at
        FROM openspec_archive_status
        WHERE openspec_change_id = ?1
          AND state = ?2
        ORDER BY updated_at ASC, queue_id ASC
        "#,
    )?;
    let rows = stmt.query_map(
        params![
            openspec_change_id.as_str(),
            openspec_archive_status_state_name(OpenSpecArchiveStatusState::Blocked),
        ],
        deserialize_row::<OpenSpecArchiveStatus>,
    )?;
    collect_rows(rows)
}

fn load_cleanup_claim_for_change_tx(
    tx: &Transaction<'_>,
    openspec_change_id: &OpenSpecChangeId,
    open_archive_blockers: Vec<OpenSpecArchiveStatus>,
) -> Result<Option<OpenSpecArchiveCleanupClaim>> {
    let row = tx
        .query_row(
            r#"
            SELECT cleanup_subtask_id, cleanup_claim_id, archive_paths_json
            FROM openspec_archive_cleanup_claims
            WHERE openspec_change_id = ?1
              AND archive_proof_digest IS NULL
            "#,
            params![openspec_change_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((cleanup_subtask_id, cleanup_claim_id, archive_paths_json)) = row else {
        return Ok(None);
    };
    let claim = tx.query_row(
        "SELECT fence_seq FROM claims WHERE claim_id = ?1 AND state = ?2",
        params![cleanup_claim_id, claim_state_name(ClaimState::Held)],
        |row| row.get::<_, i64>(0),
    )?;
    let archive_paths = serde_json::from_str::<Vec<String>>(&archive_paths_json)?
        .into_iter()
        .map(crate::model::RepoopsPath::parse)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(Some(OpenSpecArchiveCleanupClaim::new(
        openspec_change_id.clone(),
        crate::model::SubtaskId::parse(cleanup_subtask_id)?,
        ClaimId::parse(cleanup_claim_id)?,
        FenceSeq::parse(claim)?,
        archive_paths,
        open_archive_blockers,
    )))
}

fn cleanup_subtask_id_for_change_tx(
    tx: &Transaction<'_>,
    openspec_change_id: &OpenSpecChangeId,
    cleanup_claim_id: &ClaimId,
) -> Result<crate::model::SubtaskId> {
    let raw = tx.query_row(
        r#"
        SELECT cleanup_subtask_id
        FROM openspec_archive_cleanup_claims
        WHERE openspec_change_id = ?1
          AND cleanup_claim_id = ?2
        "#,
        params![openspec_change_id.as_str(), cleanup_claim_id.as_str()],
        |row| row.get::<_, String>(0),
    )?;
    crate::model::SubtaskId::parse(raw).map_err(Into::into)
}

fn archived_cleanup_finish_tx(
    tx: &Transaction<'_>,
    req: &FinishOpenSpecArchiveCleanupReq,
) -> Result<OpenSpecArchiveCleanupFinish> {
    let (cleanup_subtask_id, proof): (String, Option<String>) = tx.query_row(
        r#"
        SELECT cleanup_subtask_id, archive_proof_digest
        FROM openspec_archive_cleanup_claims
        WHERE openspec_change_id = ?1
          AND cleanup_claim_id = ?2
        "#,
        params![
            req.openspec_change_id.as_str(),
            req.cleanup_claim_id.as_str()
        ],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if proof.as_deref() != Some(req.archive_proof_digest.as_str()) {
        return Err(CoveyError::ApplyGateEvidenceMissing {
            queue_id: QueueId::parse("queue-openspec-archive-cleanup")?,
            reason: "OpenSpec archive cleanup proof digest does not match recorded proof"
                .to_owned(),
        });
    }
    let mut stmt = tx.prepare(
        r#"
        SELECT queue_id
        FROM openspec_archive_status
        WHERE openspec_change_id = ?1
          AND state = ?2
          AND archive_proof_digest = ?3
        ORDER BY queue_id ASC
        "#,
    )?;
    let rows = stmt.query_map(
        params![
            req.openspec_change_id.as_str(),
            openspec_archive_status_state_name(OpenSpecArchiveStatusState::Archived),
            req.archive_proof_digest.as_str(),
        ],
        |row| row.get::<_, String>(0),
    )?;
    let queue_ids = collect_rows(rows)?
        .into_iter()
        .map(QueueId::parse)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(OpenSpecArchiveCleanupFinish {
        openspec_change_id: req.openspec_change_id.clone(),
        archive_proof_digest: req.archive_proof_digest.clone(),
        archived_queue_ids: queue_ids,
        cleanup_subtask_id: crate::model::SubtaskId::parse(cleanup_subtask_id)?,
        cleanup_claim_id: req.cleanup_claim_id.clone(),
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

fn record_applied_openspec_archive_blocker_tx(
    tx: &Transaction<'_>,
    session_token: &SessionToken,
    item: &ReadyQueueItem,
    now: i64,
    idempotency_key: &str,
) -> Result<()> {
    let Some(openspec_change_id) = openspec_change_id_for_subtask_tx(tx, item.subtask_id())? else {
        return Ok(());
    };
    let req = RecordOpenSpecArchiveStatusReq::try_from_raw_parts(
        session_token.as_str(),
        item.queue_id().to_owned(),
        item.artifact_digest().to_owned(),
        openspec_change_id.as_str().to_owned(),
        OpenSpecArchiveStatusState::Blocked,
        Some("applied_but_unarchived".to_owned()),
        None,
        format!("auto-openspec-archive-blocker:{idempotency_key}"),
    )?;
    let existing = load_openspec_archive_status_optional_tx(tx, item.queue_id())?;
    if let Some(existing) = existing {
        if archive_status_matches_req(&existing, &req) {
            return Ok(());
        }
        return Err(CoveyError::IllegalTransition {
            from: existing.state.into(),
            to: req.state.into(),
            object: ObjectType::ReadyQueue,
        });
    }
    insert_openspec_archive_status_tx(tx, &req, item.subtask_id(), now, now)?;
    append_session_event(
        tx,
        EventType::OpenSpecArchiveStatusRecorded,
        ObjectType::ReadyQueue,
        item.queue_id(),
        session_token.as_str(),
        &req,
        now,
    )
}

fn openspec_change_id_for_subtask_tx(
    tx: &Transaction<'_>,
    subtask_id: &str,
) -> Result<Option<crate::model::OpenSpecChangeId>> {
    tx.query_row(
        r#"
        WITH RECURSIVE source_chain(subtask_id, depth) AS (
            SELECT ?1, 0
            UNION
            SELECT followup.source_subtask_id, chain.depth + 1
            FROM review_followup_subtasks followup
            JOIN source_chain chain
              ON followup.followup_subtask_id = chain.subtask_id
        )
        SELECT scope.openspec_change_id
        FROM source_chain chain
        JOIN openspec_subtask_scope scope
          ON scope.subtask_id = chain.subtask_id
        ORDER BY chain.depth ASC
        LIMIT 1
        "#,
        params![subtask_id],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(crate::model::OpenSpecChangeId::parse)
    .transpose()
    .map_err(Into::into)
}

fn load_openspec_archive_status_tx(
    tx: &Transaction<'_>,
    queue_id: &str,
) -> Result<OpenSpecArchiveStatus> {
    tx.query_row(
        r#"
        SELECT queue_id, subtask_id, artifact_digest, openspec_change_id, state,
               blocked_reason, archive_proof_digest, recorded_by_session,
               created_at, updated_at
        FROM openspec_archive_status
        WHERE queue_id = ?1
        "#,
        params![queue_id],
        deserialize_row::<OpenSpecArchiveStatus>,
    )
    .map_err(Into::into)
}

fn load_openspec_archive_status_optional_tx(
    tx: &Transaction<'_>,
    queue_id: &str,
) -> Result<Option<OpenSpecArchiveStatus>> {
    tx.query_row(
        r#"
        SELECT queue_id, subtask_id, artifact_digest, openspec_change_id, state,
               blocked_reason, archive_proof_digest, recorded_by_session,
               created_at, updated_at
        FROM openspec_archive_status
        WHERE queue_id = ?1
        "#,
        params![queue_id],
        deserialize_row::<OpenSpecArchiveStatus>,
    )
    .optional()
    .map_err(Into::into)
}

fn archive_status_matches_req(
    status: &OpenSpecArchiveStatus,
    req: &RecordOpenSpecArchiveStatusReq,
) -> bool {
    status.queue_id == req.queue_id
        && status.artifact_digest == req.artifact_digest
        && status.openspec_change_id == req.openspec_change_id
        && status.state == req.state
        && status.blocked_reason == req.blocked_reason
        && status.archive_proof_digest == req.archive_proof_digest
}

fn insert_openspec_archive_status_tx(
    tx: &Transaction<'_>,
    req: &RecordOpenSpecArchiveStatusReq,
    subtask_id: &str,
    created_at: i64,
    updated_at: i64,
) -> Result<()> {
    tx.execute(
        r#"
        INSERT INTO openspec_archive_status (
            queue_id, subtask_id, artifact_digest, openspec_change_id, state,
            blocked_reason, archive_proof_digest, recorded_by_session,
            created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        "#,
        params![
            req.queue_id.as_str(),
            subtask_id,
            req.artifact_digest.as_str(),
            req.openspec_change_id.as_str(),
            openspec_archive_status_state_name(req.state),
            req.blocked_reason.as_ref().map(AsRef::as_ref),
            req.archive_proof_digest.as_ref().map(AsRef::as_ref),
            req.session_token.as_str(),
            created_at,
            updated_at,
        ],
    )?;
    Ok(())
}

fn update_openspec_archive_status_tx(
    tx: &Transaction<'_>,
    req: &RecordOpenSpecArchiveStatusReq,
    subtask_id: &str,
    now: i64,
) -> Result<()> {
    let updated = tx.execute(
        r#"
        UPDATE openspec_archive_status
        SET state = ?2,
            blocked_reason = ?3,
            archive_proof_digest = ?4,
            recorded_by_session = ?5,
            updated_at = ?6
        WHERE queue_id = ?1
          AND subtask_id = ?7
          AND artifact_digest = ?8
          AND openspec_change_id = ?9
          AND state = ?10
        "#,
        params![
            req.queue_id.as_str(),
            openspec_archive_status_state_name(req.state),
            req.blocked_reason.as_ref().map(AsRef::as_ref),
            req.archive_proof_digest.as_ref().map(AsRef::as_ref),
            req.session_token.as_str(),
            now,
            subtask_id,
            req.artifact_digest.as_str(),
            req.openspec_change_id.as_str(),
            openspec_archive_status_state_name(OpenSpecArchiveStatusState::Blocked),
        ],
    )?;
    if updated == 1 {
        Ok(())
    } else {
        Err(CoveyError::IllegalTransition {
            from: OpenSpecArchiveStatusState::Blocked.into(),
            to: req.state.into(),
            object: ObjectType::ReadyQueue,
        })
    }
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

struct ApprovedReviewEvidence {
    review_id: ReviewId,
    findings_digest: FindingsDigest,
}

fn load_approved_review_evidence_tx(
    tx: &Transaction<'_>,
    item: &ReadyQueueItem,
) -> Result<ApprovedReviewEvidence> {
    let queue_id = QueueId::parse(item.queue_id())?;
    let (review_id, findings_digest) = tx
        .query_row(
            r#"
            SELECT review.review_id,
                   review.findings_digest
            FROM reviews review
            JOIN artifacts artifact
              ON artifact.artifact_digest = review.artifact_digest
             AND artifact.produced_by_subtask_id = review.subtask_id
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
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| CoveyError::ApplyGateEvidenceMissing {
            queue_id: queue_id.clone(),
            reason: "approved review with findings digest not found for queued artifact".to_owned(),
        })?;
    Ok(ApprovedReviewEvidence {
        review_id: parse_landing_value(&queue_id, review_id)?,
        findings_digest: parse_landing_value(&queue_id, findings_digest)?,
    })
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

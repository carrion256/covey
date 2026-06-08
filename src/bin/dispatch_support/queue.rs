use super::*;

pub(super) fn dispatch_queue(store: &Covey, command: QueueCommand) -> covey::Result<Rendered> {
    match command {
        QueueCommand::List(args) => {
            let items = store.fetch_ready_queue(args.limit)?;
            Ok(Rendered::pretty(&items))
        }
        QueueCommand::Candidates(args) => {
            let items = store.ready_queue_candidates(args.limit)?;
            Ok(Rendered::pretty(&items))
        }
        QueueCommand::Enqueue(args) => {
            let req = EnqueueForApplyReq::try_from_raw_parts(
                args.session_token,
                args.artifact_digest,
                args.subtask_id,
                SettlementTarget::Canonical,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("enqueue-for-apply")),
            )?;
            let queue_id = store.enqueue_for_apply(req)?;
            Ok(Rendered::summary(
                QueueRef {
                    queue_id: queue_id.clone(),
                },
                format!("queue {}", queue_id),
            ))
        }
        QueueCommand::ReconcileApply(args) => {
            let result =
                store.reconcile_apply_queue(ReconcileApplyQueueReq::try_from_raw_parts(
                    args.session_token,
                    args.idempotency_key
                        .unwrap_or_else(|| new_idempotency_key("reconcile-apply-queue")),
                )?)?;
            Ok(Rendered::summary(
                &result,
                format!("apply queue reconciled {} item(s)", result.enqueued_count()),
            ))
        }
        QueueCommand::ClaimNext(args) => {
            let claim =
                store.claim_next_ready_queue_item(ClaimReadyQueueReq::try_from_raw_parts(
                    args.session_token,
                    args.lease_duration_ms,
                    args.idempotency_key
                        .unwrap_or_else(|| new_idempotency_key("claim-next-ready-queue")),
                )?)?;
            Ok(Rendered::summary(
                &claim,
                match &claim {
                    Some(claim) => format!(
                        "queue_claim {} subtask={} fence={}",
                        claim.queue_id, claim.subtask_id, claim.claim_fence_seq
                    ),
                    None => "no queue item available".into(),
                },
            ))
        }
        QueueCommand::MarkInFlight(args) => {
            let claim = store.mark_in_flight(MarkInFlightReq::try_from_raw_parts(
                args.session_token,
                args.queue_id,
                args.lease_duration_ms,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("mark-in-flight")),
            )?)?;
            Ok(Rendered::summary(
                &claim,
                format!(
                    "queue {} in_flight fence={}",
                    claim.queue_id, claim.claim_fence_seq
                ),
            ))
        }
        QueueCommand::RecordApplyVerification(args) => {
            let req = RecordApplyVerificationReq::try_from_raw_parts(
                args.session_token,
                args.queue_id.clone(),
                args.artifact_digest,
                args.review_id,
                args.findings_digest,
                args.claim_fence_seq,
                args.verifier,
                args.verdict_digest,
                args.seal_digest,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("record-apply-verification")),
            )?;
            store.record_apply_verification(req)?;
            Ok(Rendered::summary(
                QueueClaimAck {
                    operation: "record_apply_verification",
                    queue_id: args.queue_id.clone(),
                    claim_fence_seq: args.claim_fence_seq,
                },
                format!("queue apply verification recorded {}", args.queue_id),
            ))
        }
        QueueCommand::VerifyLandingAuthorization(args) => {
            let status = store.verify_landing_authorization(
                VerifyLandingAuthorizationReq::try_from_raw_parts(
                    args.session_token,
                    args.queue_id,
                    args.artifact_digest,
                    args.review_id,
                    args.findings_digest,
                    args.claim_fence_seq,
                    args.verifier,
                    args.verdict_digest,
                    args.seal_digest,
                )?,
            )?;
            Ok(Rendered::summary(
                &status,
                format!("landing authorization verified {}", status.queue_id()),
            ))
        }
        QueueCommand::RecordLandingReceipt(args) => {
            let queue_id = args.queue_id.clone();
            let artifact_digest = args.artifact_digest.clone();
            let landed_commit_oid = args.landed_commit_oid.clone();
            let req = RecordLandingReceiptReq::try_from_raw_parts(
                args.session_token,
                args.queue_id,
                args.artifact_digest,
                args.claim_fence_seq,
                args.target_ref,
                args.landed_commit_oid,
            )?;
            store.record_landing_receipt(req)?;
            Ok(Rendered::summary(
                serde_json::json!({
                    "operation": "record_landing_receipt",
                    "queue_id": queue_id,
                    "artifact_digest": artifact_digest,
                    "landed_commit_oid": landed_commit_oid,
                }),
                format!("landing receipt recorded {queue_id}"),
            ))
        }
        QueueCommand::MarkApplied(args) => {
            let req = MarkAppliedReq::try_from_raw_parts(
                args.session_token,
                args.queue_id.clone(),
                args.claim_fence_seq,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("mark-applied")),
            )?;
            store.mark_applied(req)?;
            Ok(Rendered::summary(
                QueueClaimAck {
                    operation: "mark_applied",
                    queue_id: args.queue_id.clone(),
                    claim_fence_seq: args.claim_fence_seq,
                },
                format!("queue applied {}", args.queue_id),
            ))
        }
        QueueCommand::Supersede(args) => {
            store.supersede_queue_item(SupersedeQueueItemReq::try_from_raw_parts(
                args.session_token,
                args.queue_id.clone(),
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("supersede-queue-item")),
            )?)?;
            Ok(Rendered::summary(
                QueueOpAck {
                    operation: "supersede",
                    queue_id: args.queue_id.clone(),
                },
                format!("queue superseded {}", args.queue_id),
            ))
        }
        QueueCommand::Metrics => {
            let metrics = store.ready_queue_metrics()?;
            Ok(Rendered::pretty(&metrics))
        }
    }
}

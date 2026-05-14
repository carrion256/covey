use super::*;

pub(super) fn dispatch_queue(store: &Covey, command: QueueCommand) -> covey::Result<Rendered> {
    match command {
        QueueCommand::List(args) => {
            let items = store.fetch_ready_queue(args.limit)?;
            Ok(Rendered::pretty(&items))
        }
        QueueCommand::Enqueue(args) => {
            let queue_id = store.enqueue_for_apply(EnqueueForApplyReq {
                session_token: args.session_token,
                artifact_digest: args.artifact_digest,
                subtask_id: args.subtask_id,
                settlement_target: SettlementTarget::Canonical,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("enqueue-for-apply")),
            })?;
            Ok(Rendered::summary(
                QueueRef {
                    queue_id: queue_id.clone(),
                },
                format!("queue {}", queue_id),
            ))
        }
        QueueCommand::ClaimNext(args) => {
            let claim = store.claim_next_ready_queue_item(ClaimReadyQueueReq {
                session_token: args.session_token,
                lease_duration_ms: args.lease_duration_ms,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("claim-next-ready-queue")),
            })?;
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
            let claim = store.mark_in_flight(MarkInFlightReq {
                session_token: args.session_token,
                queue_id: args.queue_id,
                lease_duration_ms: args.lease_duration_ms,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("mark-in-flight")),
            })?;
            Ok(Rendered::summary(
                &claim,
                format!(
                    "queue {} in_flight fence={}",
                    claim.queue_id, claim.claim_fence_seq
                ),
            ))
        }
        QueueCommand::RecordApplyVerification(args) => {
            store.record_apply_verification(RecordApplyVerificationReq {
                session_token: args.session_token,
                queue_id: args.queue_id.clone(),
                artifact_digest: args.artifact_digest,
                review_id: args.review_id,
                findings_digest: args.findings_digest,
                claim_fence_seq: args.claim_fence_seq,
                verifier: args.verifier,
                verdict_digest: args.verdict_digest,
                seal_digest: args.seal_digest,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("record-apply-verification")),
            })?;
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
            let status = store.verify_landing_authorization(VerifyLandingAuthorizationReq {
                session_token: args.session_token,
                queue_id: args.queue_id,
                artifact_digest: args.artifact_digest,
                review_id: args.review_id,
                findings_digest: args.findings_digest,
                claim_fence_seq: args.claim_fence_seq,
                verifier: args.verifier,
                verdict_digest: args.verdict_digest,
                seal_digest: args.seal_digest,
            })?;
            Ok(Rendered::summary(
                &status,
                format!("landing authorization verified {}", status.queue_id),
            ))
        }
        QueueCommand::MarkApplied(args) => {
            store.mark_applied(MarkAppliedReq {
                session_token: args.session_token,
                queue_id: args.queue_id.clone(),
                claim_fence_seq: args.claim_fence_seq,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("mark-applied")),
            })?;
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
            store.supersede_queue_item(SupersedeQueueItemReq {
                session_token: args.session_token,
                queue_id: args.queue_id.clone(),
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("supersede-queue-item")),
            })?;
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

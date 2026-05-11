use super::*;

pub(super) fn dispatch_subtask(store: &Covey, command: SubtaskCommand) -> covey::Result<Rendered> {
    match command {
        SubtaskCommand::Create(args) => {
            let subtask_id = store.create_subtask(CreateSubtaskReq {
                session_token: args.session_token,
                meta_task_id: args.meta_task_id,
                subtask_id: args.subtask_id,
                title: args.title,
                kind: args.kind.into(),
                review_target_subtask_id: args.review_target_subtask_id,
                review_target_artifact_digest: args.review_target_artifact_digest,
                priority: args.priority,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("create-subtask")),
            })?;
            Ok(Rendered::summary(
                SubtaskRef {
                    subtask_id: subtask_id.clone(),
                },
                format!("subtask {}", subtask_id),
            ))
        }
        SubtaskCommand::ClaimNext(args) => {
            let claim = store.claim_next_subtask(ClaimNextReq {
                session_token: args.session_token.clone(),
                lease_duration_ms: args.lease_duration_ms,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("claim-next-subtask")),
            })?;
            Ok(Rendered::summary(
                &claim,
                match &claim {
                    Some(claim) => format!(
                        "claim {} subtask={} fence={}",
                        claim.claim_id, claim.subtask_id, claim.fence_seq
                    ),
                    None => "no subtask available".into(),
                },
            ))
        }
        SubtaskCommand::Claim(args) => {
            let claim = store.claim_subtask(ClaimSubtaskReq {
                session_token: args.session_token.clone(),
                subtask_id: args.subtask_id.clone(),
                lease_duration_ms: args.lease_duration_ms,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("claim-subtask")),
            })?;
            Ok(Rendered::summary(
                &claim,
                format!(
                    "claim {} subtask={} fence={}",
                    claim.claim_id, claim.subtask_id, claim.fence_seq
                ),
            ))
        }
        SubtaskCommand::Start(args) => {
            store.start_subtask(StartSubtaskReq {
                session_token: args.session_token.clone(),
                claim_id: args.claim_id.clone(),
                fence_seq: args.fence_seq,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("start-subtask")),
            })?;
            Ok(Rendered::summary(
                ClaimFenceAck {
                    operation: "start",
                    claim_id: args.claim_id.clone(),
                    fence_seq: args.fence_seq,
                },
                format!("subtask started claim={}", args.claim_id),
            ))
        }
        SubtaskCommand::Abandon(args) => {
            store.abandon_subtask(AbandonSubtaskReq {
                session_token: args.session_token.clone(),
                claim_id: args.claim_id.clone(),
                fence_seq: args.fence_seq,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("abandon-subtask")),
            })?;
            Ok(Rendered::summary(
                ClaimFenceAck {
                    operation: "abandon",
                    claim_id: args.claim_id.clone(),
                    fence_seq: args.fence_seq,
                },
                format!("subtask abandoned claim={}", args.claim_id),
            ))
        }
        SubtaskCommand::Status(args) => {
            let status = store.subtask_status(&args.subtask_id)?;
            Ok(Rendered::pretty(&status))
        }
        SubtaskCommand::Stuck(args) => {
            let subtasks = store.list_stuck_subtasks(args.older_than_ms, args.limit)?;
            Ok(Rendered::pretty(&subtasks))
        }
    }
}

pub(super) fn dispatch_claim(store: &Covey, command: ClaimCommand) -> covey::Result<Rendered> {
    match command {
        ClaimCommand::Renew(args) => {
            let claim = store.renew_claim(RenewClaimReq {
                session_token: args.session_token,
                claim_id: args.claim_id,
                fence_seq: args.fence_seq,
                extend_by_ms: args.extend_by_ms,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("renew-claim")),
            })?;
            Ok(Rendered::summary(
                &claim,
                format!(
                    "claim {} fence={} lease_deadline={}",
                    claim.claim_id, claim.fence_seq, claim.lease_deadline
                ),
            ))
        }
        ClaimCommand::Release(args) => {
            store.release_claim(ReleaseClaimReq {
                session_token: args.session_token.clone(),
                claim_id: args.claim_id.clone(),
                fence_seq: args.fence_seq,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("release-claim")),
            })?;
            Ok(Rendered::summary(
                ClaimFenceAck {
                    operation: "release",
                    claim_id: args.claim_id.clone(),
                    fence_seq: args.fence_seq,
                },
                format!("claim released {}", args.claim_id),
            ))
        }
        ClaimCommand::Expiring(args) => {
            let claims = store.list_expiring_claims(args.within_ms, args.limit)?;
            Ok(Rendered::pretty(&claims))
        }
    }
}

pub(super) fn dispatch_artifact(
    store: &Covey,
    command: ArtifactCommand,
) -> covey::Result<Rendered> {
    match command {
        ArtifactCommand::Publish(args) => {
            let claim_id = args.claim_id.clone();
            let artifact_digest = args.artifact_digest.clone();
            let artifact_kind = args
                .artifact_kind
                .to_possible_value()
                .expect("label")
                .get_name()
                .to_owned();
            store.publish_artifact(PublishArtifactReq {
                session_token: args.session_token,
                claim_id: args.claim_id,
                fence_seq: args.fence_seq,
                artifact_digest: artifact_digest.clone(),
                artifact_kind: args.artifact_kind.into(),
                base_rev: args.base_rev,
                manifest_path: args.manifest_path,
                changed_paths_digest: args.changed_paths_digest,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("publish-artifact")),
            })?;
            Ok(Rendered::summary(
                ArtifactPublishAck {
                    operation: "publish",
                    artifact_digest: artifact_digest.clone(),
                    artifact_kind,
                    claim_id,
                    fence_seq: args.fence_seq,
                },
                format!("artifact {}", artifact_digest),
            ))
        }
    }
}

pub(super) fn dispatch_review(store: &Covey, command: ReviewCommand) -> covey::Result<Rendered> {
    match command {
        ReviewCommand::Request(args) => {
            let review_id = store.request_review(RequestReviewReq {
                session_token: args.session_token,
                subtask_id: args.subtask_id,
                artifact_digest: args.artifact_digest,
                review_subtask_id: args.review_subtask_id,
                priority: args.priority,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("request-review")),
            })?;
            Ok(Rendered::summary(
                ReviewRef {
                    review_id: review_id.clone(),
                },
                format!("review {}", review_id),
            ))
        }
        ReviewCommand::Decide(args) => {
            let review_id = args.review_id.clone();
            let claim_id = args.claim_id.clone();
            let verdict = args
                .verdict
                .to_possible_value()
                .expect("label")
                .get_name()
                .to_owned();
            store.decide_review(DecideReviewReq {
                session_token: args.session_token,
                review_id: review_id.clone(),
                claim_id: args.claim_id,
                fence_seq: args.fence_seq,
                verdict: args.verdict.into(),
                findings_digest: args.findings_digest,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("decide-review")),
            })?;
            Ok(Rendered::summary(
                ReviewDecisionAck {
                    operation: "decide",
                    review_id: review_id.clone(),
                    claim_id,
                    fence_seq: args.fence_seq,
                    verdict,
                },
                format!("review decided {}", review_id),
            ))
        }
    }
}

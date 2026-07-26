use super::*;

pub(super) fn dispatch_subtask(store: &Covey, command: SubtaskCommand) -> covey::Result<Rendered> {
    match command {
        SubtaskCommand::Create(args) => {
            if SubtaskKind::from(args.kind) != SubtaskKind::Work
                || args.review_target_subtask_id.is_some()
                || args.review_target_artifact_digest.is_some()
            {
                return Err(covey::CoveyError::ReviewKindMismatch);
            }
            let idempotency_key = args
                .idempotency_key
                .unwrap_or_else(|| new_idempotency_key("create-subtask"));
            let subtask_id = match (args.completion_policy, args.routing_key) {
                (None, None) => store.create_subtask(CreateSubtaskRequest::try_from_raw_parts(
                    args.session_token,
                    args.meta_task_id,
                    args.subtask_id,
                    args.title,
                    args.priority,
                    idempotency_key,
                )?)?,
                (completion_policy, routing_key) => {
                    store.create_work_subtask(CreateWorkSubtaskReq::try_from_raw_parts(
                        args.session_token,
                        args.meta_task_id,
                        args.subtask_id,
                        args.title,
                        args.priority,
                        completion_policy
                            .map(Into::into)
                            .unwrap_or(covey::CompletionPolicy::CanonicalApply),
                        routing_key.unwrap_or_else(|| "mutai".to_owned()),
                        idempotency_key,
                    )?)?
                }
            };
            Ok(Rendered::summary(
                SubtaskRef {
                    subtask_id: subtask_id.clone(),
                },
                format!("subtask {}", subtask_id),
            ))
        }
        SubtaskCommand::ClaimNext(args) => {
            let idempotency_key = args
                .idempotency_key
                .unwrap_or_else(|| new_idempotency_key("claim-next-subtask"));
            let claim = if let Some(routing_key) = args.routing_key {
                store.claim_next_routed_subtask(ClaimNextRoutedReq::try_from_raw_parts(
                    args.session_token.clone(),
                    args.lease_duration_ms,
                    routing_key,
                    args.meta_task_id.clone(),
                    idempotency_key,
                )?)?
            } else {
                store.claim_next_subtask(ClaimNextReq::try_from_raw_parts_scoped(
                    args.session_token.clone(),
                    args.lease_duration_ms,
                    args.meta_task_id.clone(),
                    idempotency_key,
                )?)?
            };
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
            let claim = store.claim_subtask(ClaimSubtaskReq::try_from_raw_parts(
                args.session_token.clone(),
                args.subtask_id.clone(),
                args.lease_duration_ms,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("claim-subtask")),
            )?)?;
            Ok(Rendered::summary(
                &claim,
                format!(
                    "claim {} subtask={} fence={}",
                    claim.claim_id, claim.subtask_id, claim.fence_seq
                ),
            ))
        }
        SubtaskCommand::Start(args) => {
            store.start_subtask(StartSubtaskReq::try_from_raw_parts(
                args.session_token.clone(),
                args.claim_id.clone(),
                args.fence_seq,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("start-subtask")),
            )?)?;
            Ok(Rendered::summary(
                ClaimFenceAck {
                    operation: "start",
                    claim_id: args.claim_id.clone(),
                    fence_seq: args.fence_seq,
                },
                format!("subtask started claim={}", args.claim_id),
            ))
        }
        SubtaskCommand::Finish(args) => {
            let outcome = store.finish_subtask(FinishSubtaskReq::try_from_raw_parts(
                args.session_token,
                args.claim_id,
                args.fence_seq,
                args.evidence_digest,
                args.summary,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("finish-subtask")),
            )?)?;
            Ok(Rendered::summary(
                &outcome,
                format!("subtask completed {}", outcome.subtask_id),
            ))
        }
        SubtaskCommand::Retry(args) => {
            let outcome = store.retry_subtask(RetrySubtaskReq::try_from_raw_parts(
                args.session_token,
                args.claim_id,
                args.fence_seq,
                args.evidence_digest,
                args.failure_code,
                args.summary,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("retry-subtask")),
            )?)?;
            Ok(Rendered::summary(
                &outcome,
                format!("subtask retry recorded {}", outcome.subtask_id),
            ))
        }
        SubtaskCommand::Fail(args) => {
            let outcome = store.fail_subtask(FailSubtaskReq::try_from_raw_parts(
                args.session_token,
                args.claim_id,
                args.fence_seq,
                args.evidence_digest,
                args.failure_code,
                args.summary,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("fail-subtask")),
            )?)?;
            Ok(Rendered::summary(
                &outcome,
                format!("subtask failed {}", outcome.subtask_id),
            ))
        }
        SubtaskCommand::Abandon(args) => {
            store.abandon_subtask(AbandonSubtaskReq::try_from_raw_parts(
                args.session_token.clone(),
                args.claim_id.clone(),
                args.fence_seq,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("abandon-subtask")),
            )?)?;
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
        SubtaskCommand::Candidates(args) => {
            let role = args.role.into();
            let candidates = if let Some(routing_key) = args.routing_key {
                store.subtask_candidates_routed(
                    role,
                    &covey::RoutingKey::parse(routing_key)?,
                    args.limit,
                    args.meta_task_id.as_deref(),
                )?
            } else {
                store.subtask_candidates(role, args.limit, args.meta_task_id.as_deref())?
            };
            Ok(Rendered::pretty(&candidates))
        }
        SubtaskCommand::Availability(args) => {
            let availability = if let Some(routing_key) = args.routing_key {
                store.claimable_subtask_availability_routed(
                    &covey::RoutingKey::parse(routing_key)?,
                    args.meta_task_id.as_deref(),
                )?
            } else {
                store.claimable_subtask_availability(args.meta_task_id.as_deref())?
            };
            Ok(Rendered::pretty(&availability))
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
            let claim = store.renew_claim(RenewClaimReq::try_from_raw_parts(
                args.session_token,
                args.claim_id,
                args.fence_seq,
                args.extend_by_ms,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("renew-claim")),
            )?)?;
            Ok(Rendered::summary(
                &claim,
                format!(
                    "claim {} fence={} lease_deadline={}",
                    claim.claim_id, claim.fence_seq, claim.lease_deadline
                ),
            ))
        }
        ClaimCommand::Release(args) => {
            store.release_claim(ReleaseClaimReq::try_from_raw_parts(
                args.session_token.clone(),
                args.claim_id.clone(),
                args.fence_seq,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("release-claim")),
            )?)?;
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
            let req = PublishArtifactReq::try_from_raw_parts(
                args.session_token,
                args.claim_id,
                args.fence_seq,
                artifact_digest.clone(),
                args.artifact_kind.into(),
                args.base_rev,
                args.manifest_path,
                args.changed_paths_digest,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("publish-artifact")),
            )?;
            store.publish_artifact(req)?;
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
            let req = RequestReviewReq::try_from_raw_parts(
                args.session_token,
                args.subtask_id,
                args.artifact_digest,
                args.review_subtask_id,
                args.priority,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("request-review")),
            )
            .map_err(|path| covey::CoveyError::InvalidPath { path })?;
            let review_id = store.request_review(req)?;
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
            let verdict = match args.verdict {
                ReviewVerdictArg::Approve => ReviewDecisionAckVerdict::Approve,
                ReviewVerdictArg::ChangesRequested => ReviewDecisionAckVerdict::ChangesRequested,
                ReviewVerdictArg::Blocked => ReviewDecisionAckVerdict::Blocked,
            };
            let req = DecideReviewReq::try_from_raw_parts(
                args.session_token,
                args.review_id,
                args.claim_id,
                args.fence_seq,
                args.verdict.into(),
                args.findings_digest,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("decide-review")),
            )?;
            let result = store.decide_review(req)?;
            Ok(Rendered::summary(
                ReviewDecisionAck {
                    operation: "decide",
                    review_id: review_id.clone(),
                    claim_id,
                    fence_seq: args.fence_seq,
                    verdict,
                    followup_subtask_id: result.followup_subtask_id().map(ToString::to_string),
                    receipt_digest: None,
                },
                format!("review decided {}", review_id),
            ))
        }
        ReviewCommand::PermissiveLand(args) => {
            let review_id = args.review_id.clone();
            let claim_id = args.claim_id.clone();
            let receipt_digest = args.receipt_digest.clone();
            let req = RecordPermissiveLandingReceiptReq::try_from_raw_parts(
                args.session_token,
                args.review_id,
                args.claim_id,
                args.fence_seq,
                args.artifact_digest,
                args.findings_digest,
                args.target_ref,
                args.landed_commit_oid,
                args.receipt_digest,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("permissive-land-review")),
            )?;
            let result = store.record_permissive_landing_receipt(req)?;
            Ok(Rendered::summary(
                ReviewDecisionAck {
                    operation: "permissive-land",
                    review_id: review_id.clone(),
                    claim_id,
                    fence_seq: args.fence_seq,
                    verdict: ReviewDecisionAckVerdict::Approve,
                    followup_subtask_id: result.followup_subtask_id().map(ToString::to_string),
                    receipt_digest: Some(receipt_digest),
                },
                format!("permissive landing recorded for review {}", review_id),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{
        AbandonSubtaskArgs, ArtifactKindArg, ClaimNextArgs, ClaimSubtaskArgs, CreateSubtaskArgs,
        DecideReviewArgs, ExpiringClaimsArgs, PublishArtifactArgs, ReleaseClaimArgs,
        RenewClaimArgs, RequestReviewArgs, ReviewVerdictArg, StartSubtaskArgs, StuckSubtasksArgs,
        SubtaskAvailabilityArgs, SubtaskKindArg, SubtaskStatusArgs,
    };
    use covey::SessionRole;

    #[test]
    fn workflow_dispatchers_cover_subtask_claim_artifact_and_review_paths() {
        let tempdir = tempfile::tempdir().expect("temp db dir");
        let store = Covey::open(tempdir.path().join("covey.sqlite")).expect("open covey store");
        let orchestrator = register_session(&store, "orchestrator", SessionRole::Orchestrator);
        let executor_a = register_session(&store, "executor-a", SessionRole::Executor);
        let executor_b = register_session(&store, "executor-b", SessionRole::Executor);
        let executor_c = register_session(&store, "executor-c", SessionRole::Executor);
        let reviewer = register_session(&store, "reviewer", SessionRole::Reviewer);
        let meta_task_id = store
            .submit_meta_task(
                SubmitMetaTaskReq::try_from_raw_parts(
                    orchestrator.clone(),
                    "dispatch workflow coverage",
                    "submit-workflow-dispatch",
                )
                .expect("valid submit-meta-task request"),
            )
            .expect("submit meta task");

        for (subtask_id, title, priority) in [
            ("work-a", "first work", 10),
            ("work-b", "second work", 20),
            ("work-review", "reviewed work", 30),
        ] {
            let rendered = dispatch_subtask(
                &store,
                SubtaskCommand::Create(CreateSubtaskArgs {
                    session_token: orchestrator.clone(),
                    meta_task_id: meta_task_id.clone(),
                    title: title.to_owned(),
                    kind: SubtaskKindArg::Work,
                    priority,
                    subtask_id: Some(subtask_id.to_owned()),
                    review_target_subtask_id: None,
                    review_target_artifact_digest: None,
                    idempotency_key: None,
                    completion_policy: None,
                    routing_key: None,
                }),
            )
            .expect("create subtask through dispatcher");
            assert!(rendered.human.contains("subtask"));
        }

        let status = dispatch_subtask(
            &store,
            SubtaskCommand::Status(SubtaskStatusArgs {
                subtask_id: "work-a".to_owned(),
            }),
        )
        .expect("status should render");
        assert_eq!(status.data["subtask"]["subtask_id"], "work-a");
        let availability = dispatch_subtask(
            &store,
            SubtaskCommand::Availability(SubtaskAvailabilityArgs {
                meta_task_id: None,
                routing_key: None,
            }),
        )
        .expect("availability should render");
        assert_eq!(availability.data["executor_claimable_count"], 3);
        assert_eq!(availability.data["reviewer_claimable_count"], 0);
        let stuck = dispatch_subtask(
            &store,
            SubtaskCommand::Stuck(StuckSubtasksArgs {
                older_than_ms: 0,
                limit: 10,
            }),
        )
        .expect("stuck query should render");
        assert!(stuck.data.is_array());

        let claim_a = dispatch_subtask(
            &store,
            SubtaskCommand::ClaimNext(ClaimNextArgs {
                session_token: executor_a.clone(),
                lease_duration_ms: 60_000,
                meta_task_id: None,
                idempotency_key: None,
                routing_key: None,
            }),
        )
        .expect("claim next should render");
        assert_eq!(claim_a.data["subtask_id"], "work-a");
        let claim_a_id = claim_a.data["claim_id"].as_str().unwrap().to_owned();
        let claim_a_fence = claim_a.data["fence_seq"].as_i64().unwrap();
        dispatch_subtask(
            &store,
            SubtaskCommand::Start(StartSubtaskArgs {
                session_token: executor_a.clone(),
                claim_id: claim_a_id.clone(),
                fence_seq: claim_a_fence,
                idempotency_key: None,
            }),
        )
        .expect("start should render");
        dispatch_subtask(
            &store,
            SubtaskCommand::Abandon(AbandonSubtaskArgs {
                session_token: executor_a,
                claim_id: claim_a_id,
                fence_seq: claim_a_fence,
                idempotency_key: None,
            }),
        )
        .expect("abandon should render");

        let claim_b = dispatch_subtask(
            &store,
            SubtaskCommand::Claim(ClaimSubtaskArgs {
                session_token: executor_b.clone(),
                subtask_id: "work-b".to_owned(),
                lease_duration_ms: 60_000,
                idempotency_key: None,
            }),
        )
        .expect("claim exact should render");
        let claim_b_id = claim_b.data["claim_id"].as_str().unwrap().to_owned();
        let claim_b_fence = claim_b.data["fence_seq"].as_i64().unwrap();
        let renewed = dispatch_claim(
            &store,
            ClaimCommand::Renew(RenewClaimArgs {
                session_token: executor_b.clone(),
                claim_id: claim_b_id.clone(),
                fence_seq: claim_b_fence,
                extend_by_ms: 60_000,
                idempotency_key: None,
            }),
        )
        .expect("renew should render");
        assert_eq!(renewed.data["claim_id"], claim_b_id);
        let expiring = dispatch_claim(
            &store,
            ClaimCommand::Expiring(ExpiringClaimsArgs {
                within_ms: 120_000,
                limit: 10,
            }),
        )
        .expect("expiring should render");
        assert!(expiring.data.is_array());
        dispatch_claim(
            &store,
            ClaimCommand::Release(ReleaseClaimArgs {
                session_token: executor_b,
                claim_id: claim_b_id,
                fence_seq: claim_b_fence,
                idempotency_key: None,
            }),
        )
        .expect("release should render");

        let claim_c = dispatch_subtask(
            &store,
            SubtaskCommand::Claim(ClaimSubtaskArgs {
                session_token: executor_c.clone(),
                subtask_id: "work-review".to_owned(),
                lease_duration_ms: 60_000,
                idempotency_key: None,
            }),
        )
        .expect("claim work for artifact");
        let claim_c_id = claim_c.data["claim_id"].as_str().unwrap().to_owned();
        let claim_c_fence = claim_c.data["fence_seq"].as_i64().unwrap();
        dispatch_subtask(
            &store,
            SubtaskCommand::Start(StartSubtaskArgs {
                session_token: executor_c.clone(),
                claim_id: claim_c_id.clone(),
                fence_seq: claim_c_fence,
                idempotency_key: None,
            }),
        )
        .expect("start artifact work");
        let artifact = dispatch_artifact(
            &store,
            ArtifactCommand::Publish(PublishArtifactArgs {
                session_token: executor_c,
                claim_id: claim_c_id,
                fence_seq: claim_c_fence,
                artifact_digest: "blake3:artifact-digest-work-review".to_owned(),
                artifact_kind: ArtifactKindArg::PatchBundle,
                base_rev: "base-rev".to_owned(),
                manifest_path: "manifest.json".to_owned(),
                changed_paths_digest: "blake3:changed-paths-digest".to_owned(),
                idempotency_key: None,
            }),
        )
        .expect("publish artifact");
        assert_eq!(
            artifact.data["artifact_digest"],
            "blake3:artifact-digest-work-review"
        );

        let review = dispatch_review(
            &store,
            ReviewCommand::Request(RequestReviewArgs {
                session_token: orchestrator,
                subtask_id: "work-review".to_owned(),
                artifact_digest: "blake3:artifact-digest-work-review".to_owned(),
                priority: 5,
                review_subtask_id: Some("review-work-review".to_owned()),
                idempotency_key: None,
            }),
        )
        .expect("request review");
        let review_id = review.data["review_id"].as_str().unwrap().to_owned();
        let review_claim = dispatch_subtask(
            &store,
            SubtaskCommand::Claim(ClaimSubtaskArgs {
                session_token: reviewer.clone(),
                subtask_id: "review-work-review".to_owned(),
                lease_duration_ms: 60_000,
                idempotency_key: None,
            }),
        )
        .expect("claim review subtask");
        let review_claim_id = review_claim.data["claim_id"].as_str().unwrap().to_owned();
        let review_fence = review_claim.data["fence_seq"].as_i64().unwrap();
        dispatch_subtask(
            &store,
            SubtaskCommand::Start(StartSubtaskArgs {
                session_token: reviewer.clone(),
                claim_id: review_claim_id.clone(),
                fence_seq: review_fence,
                idempotency_key: None,
            }),
        )
        .expect("start review subtask");
        let decision = dispatch_review(
            &store,
            ReviewCommand::Decide(DecideReviewArgs {
                session_token: reviewer,
                review_id: review_id.clone(),
                claim_id: review_claim_id,
                fence_seq: review_fence,
                verdict: ReviewVerdictArg::Approve,
                findings_digest: "blake3:findings-digest".to_owned(),
                idempotency_key: None,
            }),
        )
        .expect("decide review");
        assert_eq!(decision.data["review_id"], review_id);
        assert_eq!(decision.data["verdict"], "approve");
    }

    fn register_session(store: &Covey, label: &str, role: SessionRole) -> String {
        store
            .register_session(
                RegisterSessionReq::try_from_raw_parts(
                    format!("principal-{label}"),
                    format!("instance-{label}"),
                    role,
                    format!("register-{label}"),
                )
                .expect("valid generated session registration request"),
            )
            .expect("register session")
            .session_token
            .to_string()
    }
}

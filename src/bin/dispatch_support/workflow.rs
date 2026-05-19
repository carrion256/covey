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
            let subtask_id = store.create_subtask(CreateSubtaskRequest {
                session_token: args.session_token,
                meta_task_id: args.meta_task_id,
                subtask_id: args.subtask_id,
                title: args.title,
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
            let claim = store.claim_next_subtask(ClaimNextReq::try_from_raw_parts(
                args.session_token.clone(),
                args.lease_duration_ms,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("claim-next-subtask")),
            )?)?;
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
            let claim_id = covey::ClaimId::parse(args.claim_id.clone())?;
            let fence_seq = covey::FenceSeq::parse(args.fence_seq)?;
            store.start_subtask(StartSubtaskReq {
                session_token: args.session_token.clone(),
                claim_id,
                fence_seq,
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
            let claim_id = covey::ClaimId::parse(args.claim_id.clone())?;
            let fence_seq = covey::FenceSeq::parse(args.fence_seq)?;
            store.abandon_subtask(AbandonSubtaskReq {
                session_token: args.session_token.clone(),
                claim_id,
                fence_seq,
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
            let claim_id = covey::ClaimId::parse(args.claim_id)?;
            let fence_seq = covey::FenceSeq::parse(args.fence_seq)?;
            let extend_by_ms = covey::LeaseDurationMs::parse(args.extend_by_ms)?;
            let claim = store.renew_claim(RenewClaimReq {
                session_token: args.session_token,
                claim_id,
                fence_seq,
                extend_by_ms,
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
            let claim_id = covey::ClaimId::parse(args.claim_id.clone())?;
            let fence_seq = covey::FenceSeq::parse(args.fence_seq)?;
            store.release_claim(ReleaseClaimReq {
                session_token: args.session_token.clone(),
                claim_id,
                fence_seq,
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
            let verdict = args
                .verdict
                .to_possible_value()
                .expect("label")
                .get_name()
                .to_owned();
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
            store.decide_review(req)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{
        AbandonSubtaskArgs, ArtifactKindArg, ClaimNextArgs, ClaimSubtaskArgs, CreateSubtaskArgs,
        DecideReviewArgs, ExpiringClaimsArgs, PublishArtifactArgs, ReleaseClaimArgs,
        RenewClaimArgs, RequestReviewArgs, ReviewVerdictArg, StartSubtaskArgs, StuckSubtasksArgs,
        SubtaskKindArg, SubtaskStatusArgs,
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
            .submit_meta_task(SubmitMetaTaskReq {
                session_token: orchestrator.clone(),
                prompt_text: "dispatch workflow coverage".to_owned(),
                idempotency_key: "submit-workflow-dispatch".to_owned(),
            })
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
                idempotency_key: None,
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
            .register_session(RegisterSessionReq {
                agent_principal_id: format!("principal-{label}"),
                agent_instance_id: format!("instance-{label}"),
                role,
                idempotency_key: format!("register-{label}"),
            })
            .expect("register session")
            .session_token
    }
}

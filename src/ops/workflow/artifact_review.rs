#![cfg_attr(coverage_nightly, coverage(off))]

use std::time::Instant;

use rusqlite::params;

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        ClaimState, DecideReviewReq, EventType, FailedReviewVerdict, ObjectType,
        PublishArtifactReq, ReviewDecisionResult, ReviewState, ReviewVerdict, SessionRole,
        SubtaskId, SubtaskKind, SubtaskState, SubtaskTitle,
    },
    queries::{load_artifact_tx, load_review_tx, load_session_tx, load_subtask_tx},
    schema::advance_lease_clock,
    store::append_session_event,
    validators::{
        MAX_BASE_REV_LEN, MAX_DIGEST_LEN, MAX_OBJECT_ID_LEN, MAX_PATH_LEN,
        clear_session_active_subtask, close_claim_and_detach, ensure_artifact_digest_unused,
        ensure_artifact_exists, ensure_length, ensure_meta_task_is_schedulable,
        ensure_no_open_review_round, ensure_subtask_transition, require_active_session,
        require_current_claim, require_session_can_claim_kind, require_session_can_request_review,
        subtask_exists,
    },
};

impl Covey {
    /// Publishes an immutable artifact for a work subtask.
    pub fn publish_artifact(&self, req: PublishArtifactReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "publish_artifact",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    ensure_length("claim_id", &req.claim_id, MAX_OBJECT_ID_LEN)?;
                    ensure_length("artifact_digest", &req.artifact_digest, MAX_DIGEST_LEN)?;
                    ensure_length("base_rev", &req.base_rev, MAX_BASE_REV_LEN)?;
                    ensure_length("manifest_path", &req.manifest_path, MAX_PATH_LEN)?;
                    ensure_length(
                        "changed_paths_digest",
                        &req.changed_paths_digest,
                        MAX_DIGEST_LEN,
                    )?;
                    let claim = require_current_claim(
                        tx,
                        &req.session_token,
                        &req.claim_id,
                        req.fence_seq,
                        lease_now,
                    )?;
                    let session = require_active_session(tx, &req.session_token)?;
                    let subtask = load_subtask_tx(tx, &claim.subtask_id)?;
                    ensure_meta_task_is_schedulable(tx, &subtask.meta_task_id)?;
                    require_session_can_claim_kind(&session, subtask.kind())?;
                    if subtask.kind() != SubtaskKind::Work {
                        return Err(CoveyError::ReviewKindMismatch);
                    }
                    ensure_subtask_transition(
                        subtask.kind(),
                        subtask.state(),
                        SubtaskState::ArtifactPublished,
                    )?;
                    ensure_artifact_digest_unused(tx, &req.artifact_digest)?;

                    tx.execute(
                        r#"
                        INSERT INTO artifacts (
                            artifact_digest, artifact_kind, base_rev, produced_by_subtask_id,
                            produced_by_session, manifest_path, changed_paths_digest, created_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                        "#,
                        params![
                            req.artifact_digest,
                            req.artifact_kind.to_string(),
                            req.base_rev,
                            subtask.subtask_id,
                            req.session_token,
                            req.manifest_path,
                            req.changed_paths_digest,
                            now
                        ],
                    )?;
                    tx.execute(
                        r#"
                        UPDATE reviews
                        SET state = ?2, updated_at = ?3
                        WHERE subtask_id = ?1
                          AND artifact_digest != ?4
                          AND state IN (?5, ?6)
                        "#,
                        params![
                            subtask.subtask_id,
                            ReviewState::Superseded.to_string(),
                            now,
                            req.artifact_digest,
                            ReviewState::Requested.to_string(),
                            ReviewState::InProgress.to_string()
                        ],
                    )?;
                    let updated = tx.execute(
                        r#"
                        UPDATE subtasks
                        SET artifact_digest = ?2,
                            state = ?3,
                            updated_at = ?4
                        WHERE subtask_id = ?1 AND state = ?5
                        "#,
                        params![
                            subtask.subtask_id,
                            req.artifact_digest,
                            SubtaskState::ArtifactPublished.to_string(),
                            now,
                            subtask.state().to_string()
                        ],
                    )?;
                    if updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: subtask.state().into(),
                            to: SubtaskState::ArtifactPublished.into(),
                            object: ObjectType::Subtask,
                        });
                    }
                    append_session_event(
                        tx,
                        EventType::ArtifactPublished,
                        ObjectType::Artifact,
                        &req.artifact_digest,
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(())
                },
            )
        });
        self.log_operation(
            "publish_artifact",
            &req.session_token,
            started_at,
            &result,
            |_| {
                vec![
                    format!("claim:{}", req.claim_id),
                    format!("artifact:{}", req.artifact_digest),
                ]
            },
        );
        result
    }

    /// Creates a review request for the current artifact digest of a work subtask.
    pub fn request_review(&self, req: crate::model::RequestReviewReq) -> Result<String> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                req.session_token.as_str(),
                "request_review",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    let session = require_active_session(tx, req.session_token.as_str())?;
                    require_session_can_request_review(&session)?;
                    ensure_length("subtask_id", &req.subtask_id, MAX_OBJECT_ID_LEN)?;
                    ensure_length("artifact_digest", &req.artifact_digest, MAX_DIGEST_LEN)?;
                    let subtask = load_subtask_tx(tx, &req.subtask_id)?;
                    if subtask.kind() != SubtaskKind::Work {
                        return Err(CoveyError::ReviewKindMismatch);
                    }
                    ensure_meta_task_is_schedulable(tx, &subtask.meta_task_id)?;
                    if subtask.artifact_digest() != Some(&req.artifact_digest) {
                        return Err(CoveyError::UnknownArtifactDigest {
                            digest: req.artifact_digest.to_string(),
                        });
                    }
                    ensure_artifact_exists(tx, &req.artifact_digest)?;
                    ensure_no_open_review_round(tx, &req.subtask_id, &req.artifact_digest)?;
                    ensure_subtask_transition(
                        subtask.kind(),
                        subtask.state(),
                        SubtaskState::ReviewPending,
                    )?;

                    let review_subtask_id = req
                        .review_subtask_id
                        .clone()
                        .unwrap_or_else(|| {
                            SubtaskId::parse(crate::model::make_id("subtask"))
                                .expect("generated subtask id must be valid")
                        });
                    ensure_length("review_subtask_id", &review_subtask_id, MAX_OBJECT_ID_LEN)?;
                    if subtask_exists(tx, &review_subtask_id)? {
                        return Err(CoveyError::DuplicateSubtaskId {
                            subtask_id: review_subtask_id.clone(),
                        });
                    }
                    tx.execute(
                        r#"
                        INSERT INTO subtasks (
                            subtask_id, meta_task_id, title, kind, review_target_subtask_id,
                            review_target_artifact_digest, state, current_claim_id, artifact_digest,
                            priority, created_at, updated_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, ?8, ?9, ?9)
                        "#,
                        params![
                            review_subtask_id.as_str(),
                            subtask.meta_task_id,
                            format!("review {}", req.artifact_digest),
                            SubtaskKind::Review.to_string(),
                            req.subtask_id.as_str(),
                            req.artifact_digest.as_str(),
                            SubtaskState::Available.to_string(),
                            req.priority.get(),
                            now
                        ],
                    )?;
                    tx.execute(
                        "INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq) VALUES (?1, 1)",
                        params![review_subtask_id],
                    )?;
                    let review_id = crate::model::make_id("review");
                    tx.execute(
                        r#"
                        INSERT INTO reviews (
                            review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id,
                            verdict, findings_digest, state, created_at, updated_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6, ?7, ?7)
                        "#,
                        params![
                            review_id,
                            req.subtask_id.as_str(),
                            req.artifact_digest.as_str(),
                            req.session_token.as_str(),
                            review_subtask_id,
                            ReviewState::Requested.to_string(),
                            now
                        ],
                    )?;
                    let updated = tx.execute(
                        "UPDATE subtasks SET state = ?2, updated_at = ?3 WHERE subtask_id = ?1 AND state = ?4 AND artifact_digest = ?5",
                        params![
                            req.subtask_id,
                            SubtaskState::ReviewPending.to_string(),
                            now,
                            subtask.state().to_string(),
                            req.artifact_digest.as_str()
                        ],
                    )?;
                    if updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: subtask.state().into(),
                            to: SubtaskState::ReviewPending.into(),
                            object: ObjectType::Subtask,
                        });
                    }
                    append_session_event(
                        tx,
                        EventType::ReviewRequested,
                        ObjectType::Review,
                        &review_id,
                        req.session_token.as_str(),
                        &req,
                        now,
                    )?;
                    Ok(review_id)
                },
            )
        });
        self.log_operation(
            "request_review",
            req.session_token.as_str(),
            started_at,
            &result,
            |review_id| {
                vec![
                    format!("review:{review_id}"),
                    format!("subtask:{}", req.subtask_id),
                ]
            },
        );
        result
    }

    /// Decides a review and updates the reviewed subtask if the artifact is still current.
    pub fn decide_review(&self, req: DecideReviewReq) -> Result<ReviewDecisionResult> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "decide_review",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    let session = crate::validators::require_role(
                        tx,
                        &req.session_token,
                        &[SessionRole::Reviewer],
                    )?;
                    ensure_length("review_id", &req.review_id, MAX_OBJECT_ID_LEN)?;
                    ensure_length("claim_id", &req.claim_id, MAX_OBJECT_ID_LEN)?;
                    ensure_length("findings_digest", &req.findings_digest, MAX_DIGEST_LEN)?;
                    let claim = require_current_claim(
                        tx,
                        &req.session_token,
                        &req.claim_id,
                        req.fence_seq,
                        lease_now,
                    )?;
                    let review = load_review_tx(tx, &req.review_id)?;
                    let review_subtask_id = review.review_subtask_id().to_owned();
                    if claim.subtask_id != review_subtask_id {
                        return Err(CoveyError::FenceTokenMismatch);
                    }
                    let review_subtask = load_subtask_tx(tx, &review_subtask_id)?;
                    ensure_meta_task_is_schedulable(tx, &review_subtask.meta_task_id)?;
                    require_session_can_claim_kind(&session, review_subtask.kind())?;
                    let artifact = load_artifact_tx(tx, review.artifact_digest())?;
                    let producer_session = load_session_tx(tx, &artifact.produced_by_session)?;
                    if producer_session.agent_principal_id == session.agent_principal_id {
                        return Err(CoveyError::SeparationOfDutiesViolation {
                            reviewer_principal_id: session.agent_principal_id().to_owned(),
                            producer_principal_id: producer_session.agent_principal_id().to_owned(),
                        });
                    }
                    let work_subtask = load_subtask_tx(tx, review.subtask_id())?;
                    if work_subtask.artifact_digest().map(AsRef::as_ref)
                        != Some(review.artifact_digest())
                    {
                        let Some(current_artifact_digest) = work_subtask.artifact_digest().cloned()
                        else {
                            return Err(CoveyError::UnknownArtifactDigest {
                                digest: review.artifact_digest().to_owned(),
                            });
                        };
                        return Err(CoveyError::StaleReviewArtifact {
                            review_id: req.review_id.to_string(),
                            subtask_id: crate::model::SubtaskId::parse(review.subtask_id())?,
                            artifact_digest: crate::model::ArtifactDigest::parse(
                                review.artifact_digest(),
                            )?,
                            current_artifact_digest,
                        });
                    }
                    crate::validators::ensure_review_transition(review.state(), ReviewState::Decided)?;

                    let review_updated = tx.execute(
                        r#"
                        UPDATE reviews
                        SET reviewer_session = ?2,
                            verdict = ?3,
                            findings_digest = ?4,
                            state = ?5,
                            updated_at = ?6
                        WHERE review_id = ?1 AND state = ?7
                        "#,
                        params![
                            req.review_id,
                            req.session_token,
                            req.verdict.to_string(),
                            req.findings_digest.as_str(),
                            ReviewState::Decided.to_string(),
                            now,
                            review.state().to_string()
                        ],
                    )?;
                    if review_updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: review.state().into(),
                            to: ReviewState::Decided.into(),
                            object: ObjectType::Review,
                        });
                    }
                    ensure_subtask_transition(
                        review_subtask.kind(),
                        review_subtask.state(),
                        SubtaskState::Decided,
                    )?;
                    let subtask_updated = tx.execute(
                        "UPDATE subtasks SET state = ?2, current_claim_id = NULL, updated_at = ?3 WHERE subtask_id = ?1 AND current_claim_id = ?4 AND state = ?5",
                        params![
                            review_subtask_id,
                            SubtaskState::Decided.to_string(),
                            now,
                            claim.claim_id,
                            review_subtask.state().to_string()
                        ],
                    )?;
                    if subtask_updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: review_subtask.state().into(),
                            to: SubtaskState::Decided.into(),
                            object: ObjectType::Subtask,
                        });
                    }
                    close_claim_and_detach(tx, &claim, ClaimState::Released, now)?;
                    clear_session_active_subtask(tx, &req.session_token, now)?;

                    let work_state = match req.verdict {
                        ReviewVerdict::Approve => SubtaskState::Approved,
                        ReviewVerdict::ChangesRequested => SubtaskState::ChangesRequested,
                        ReviewVerdict::Blocked => SubtaskState::Blocked,
                    };
                    ensure_subtask_transition(work_subtask.kind(), work_subtask.state(), work_state)?;
                    let work_updated = tx.execute(
                        "UPDATE subtasks SET state = ?2, updated_at = ?3 WHERE subtask_id = ?1 AND artifact_digest = ?4 AND state = ?5",
                        params![
                            review.subtask_id(),
                            work_state.to_string(),
                            now,
                            review.artifact_digest(),
                            work_subtask.state().to_string()
                        ],
                    )?;
                    if work_updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: work_subtask.state().into(),
                            to: work_state.into(),
                            object: ObjectType::Subtask,
                        });
                    }

                    let decision = match FailedReviewVerdict::try_from(req.verdict) {
                        Err(ReviewVerdict::Approve) => ReviewDecisionResult::Approved {
                            review_id: req.review_id.clone(),
                        },
                        Err(failed_as_error) => {
                            unreachable!("failed verdict conversion returned {failed_as_error}")
                        }
                        Ok(failed_verdict) => {
                            let followup_subtask_id = create_review_followup_subtask_tx(
                                tx,
                                &work_subtask,
                                review.artifact_digest(),
                                req.findings_digest.as_str(),
                                req.session_token.as_str(),
                                req.review_id.as_str(),
                                now,
                            )?;
                            ReviewDecisionResult::Failed {
                                review_id: req.review_id.clone(),
                                verdict: failed_verdict,
                                followup_subtask_id,
                            }
                        }
                    };

                    append_session_event(
                        tx,
                        EventType::ReviewDecided,
                        ObjectType::Review,
                        &req.review_id,
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(decision)
                },
            )
        });
        self.log_operation(
            "decide_review",
            &req.session_token,
            started_at,
            &result,
            |decision| {
                vec![
                    format!("review:{}", decision.review_id()),
                    format!("claim:{}", req.claim_id),
                ]
            },
        );
        result
    }
}

fn create_review_followup_subtask_tx(
    tx: &rusqlite::Transaction<'_>,
    source_subtask: &crate::model::SubtaskRow,
    source_artifact_digest: &str,
    findings_digest: &str,
    created_by_session: &str,
    review_id: &str,
    now: i64,
) -> Result<SubtaskId> {
    let followup_subtask_id = SubtaskId::parse(crate::model::make_id("subtask"))
        .expect("generated subtask ids are valid");
    let title = SubtaskTitle::parse(format!(
        "address review findings for {}",
        source_subtask.subtask_id
    ))?;
    tx.execute(
        r#"
        INSERT INTO subtasks (
            subtask_id, meta_task_id, title, kind, review_target_subtask_id,
            review_target_artifact_digest, state, current_claim_id, artifact_digest,
            priority, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, NULL, NULL, ?6, ?7, ?7)
        "#,
        params![
            followup_subtask_id.as_str(),
            source_subtask.meta_task_id.as_str(),
            title.as_str(),
            SubtaskKind::Work.to_string(),
            SubtaskState::Available.to_string(),
            source_subtask.priority.get(),
            now,
        ],
    )?;
    tx.execute(
        "INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq) VALUES (?1, 1)",
        params![followup_subtask_id.as_str()],
    )?;
    tx.execute(
        r#"
        INSERT INTO review_followup_subtasks (
            review_id, source_subtask_id, source_artifact_digest, findings_digest,
            followup_subtask_id, created_by_session, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            review_id,
            source_subtask.subtask_id.as_str(),
            source_artifact_digest,
            findings_digest,
            followup_subtask_id.as_str(),
            created_by_session,
            now
        ],
    )?;
    Ok(followup_subtask_id)
}

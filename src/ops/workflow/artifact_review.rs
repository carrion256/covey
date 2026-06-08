#![cfg_attr(coverage_nightly, coverage(off))]

use std::time::Instant;

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        ArtifactKind, ClaimState, DecideReviewReq, EventType, FailedReviewVerdict, ObjectType,
        PublishArtifactReq, ReviewDecisionResult, ReviewState, ReviewVerdict, SessionRole,
        SubtaskId, SubtaskKind, SubtaskState, SubtaskTitle, review_state_name, review_verdict_name,
        subtask_kind_name, subtask_state_name,
    },
    ops::queue::enqueue_approved_subtask_for_apply_tx,
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

const fn artifact_kind_name(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::PatchBundle => "patch_bundle",
        ArtifactKind::IsolatedCommitRef => "isolated_commit_ref",
        ArtifactKind::TreeBundle => "tree_bundle",
        ArtifactKind::FindingsBundle => "findings_bundle",
        ArtifactKind::VerificationBundle => "verification_bundle",
    }
}

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
                            artifact_kind_name(req.artifact_kind),
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
                            review_state_name(ReviewState::Superseded),
                            now,
                            req.artifact_digest,
                            review_state_name(ReviewState::Requested),
                            review_state_name(ReviewState::InProgress)
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
                            subtask_state_name(SubtaskState::ArtifactPublished),
                            now,
                            subtask_state_name(subtask.state())
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
                            subtask_kind_name(SubtaskKind::Review),
                            req.subtask_id.as_str(),
                            req.artifact_digest.as_str(),
                            subtask_state_name(SubtaskState::Available),
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
                            review_state_name(ReviewState::Requested),
                            now
                        ],
                    )?;
                    let updated = tx.execute(
                        "UPDATE subtasks SET state = ?2, updated_at = ?3 WHERE subtask_id = ?1 AND state = ?4 AND artifact_digest = ?5",
                        params![
                            req.subtask_id,
                            subtask_state_name(SubtaskState::ReviewPending),
                            now,
                            subtask_state_name(subtask.state()),
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
                    let review_subtask_id = review.review_subtask_id();
                    if claim.subtask_id.as_str() != review_subtask_id {
                        return Err(CoveyError::FenceTokenMismatch);
                    }
                    let review_subtask = load_subtask_tx(tx, review_subtask_id)?;
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
                            review_verdict_name(req.verdict),
                            req.findings_digest.as_str(),
                            review_state_name(ReviewState::Decided),
                            now,
                            review_state_name(review.state())
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
                            subtask_state_name(SubtaskState::Decided),
                            now,
                            claim.claim_id,
                            subtask_state_name(review_subtask.state())
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
                            subtask_state_name(work_state),
                            now,
                            review.artifact_digest(),
                            subtask_state_name(work_subtask.state())
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

                    if matches!(req.verdict, ReviewVerdict::Approve) {
                        enqueue_approved_subtask_for_apply_tx(
                            tx,
                            &req.session_token,
                            review.subtask_id(),
                            review.artifact_digest(),
                            crate::model::SettlementTarget::Canonical,
                            now,
                            format!("auto-enqueue-approved-review:{}", req.review_id),
                        )?;
                    }

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
    tx: &Transaction<'_>,
    source_subtask: &crate::model::SubtaskRow,
    source_artifact_digest: &str,
    findings_digest: &str,
    created_by_session: &str,
    review_id: &str,
    now: i64,
) -> Result<SubtaskId> {
    let followup_subtask_id = SubtaskId::parse(crate::model::make_id("subtask"))
        .expect("generated subtask ids are valid");
    let repair_scope = load_openspec_repair_scope_tx(tx, source_subtask.subtask_id.as_str())?;
    let title = SubtaskTitle::parse(repair_followup_title(
        source_subtask.subtask_id.as_str(),
        repair_scope.as_ref(),
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
            subtask_kind_name(SubtaskKind::Work),
            subtask_state_name(SubtaskState::Available),
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
            followup_subtask_id, created_by_session, created_at,
            repair_source_path, repair_task_ref, repair_scenario_refs_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        "#,
        params![
            review_id,
            source_subtask.subtask_id.as_str(),
            source_artifact_digest,
            findings_digest,
            followup_subtask_id.as_str(),
            created_by_session,
            now,
            repair_scope
                .as_ref()
                .map(|scope| scope.source_path.as_str()),
            repair_scope.as_ref().map(|scope| scope.task_ref.as_str()),
            repair_scope
                .as_ref()
                .map(|scope| scope.scenario_refs_json.as_str())
                .unwrap_or("[]"),
        ],
    )?;
    Ok(followup_subtask_id)
}

struct OpenSpecRepairScope {
    source_path: String,
    task_ref: String,
    scenario_refs: Vec<String>,
    scenario_refs_json: String,
}

fn load_openspec_repair_scope_tx(
    tx: &Transaction<'_>,
    source_subtask_id: &str,
) -> Result<Option<OpenSpecRepairScope>> {
    tx.query_row(
        r#"
        SELECT openspec_change_id, openspec_task_id, source_path, scenario_refs_json
        FROM openspec_subtask_scope
        WHERE subtask_id = ?1
        "#,
        params![source_subtask_id],
        |row| {
            let change_id: String = row.get(0)?;
            let task_id: String = row.get(1)?;
            let source_path: String = row.get(2)?;
            let scenario_refs_json: String = row.get(3)?;
            let scenario_refs = serde_json::from_str::<Vec<String>>(&scenario_refs_json)
                .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
            Ok(OpenSpecRepairScope {
                source_path,
                task_ref: format!("{change_id}:{task_id}"),
                scenario_refs,
                scenario_refs_json,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn repair_followup_title(source_subtask_id: &str, scope: Option<&OpenSpecRepairScope>) -> String {
    let Some(scope) = scope else {
        return format!("address review findings for {source_subtask_id}");
    };
    let scenario_suffix = if scope.scenario_refs.is_empty() {
        "no scenario refs".to_owned()
    } else {
        scope
            .scenario_refs
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "repair review findings for {} scenarios {}",
        scope.task_ref, scenario_suffix
    )
}

pub(crate) fn ensure_changes_requested_followup_blocks_tx(
    tx: &Transaction<'_>,
    created_by_session: &str,
    now: i64,
) -> Result<usize> {
    let mut stmt = tx.prepare(
        r#"
        SELECT r.review_id
        FROM reviews r
        JOIN subtasks s ON s.subtask_id = r.subtask_id
        WHERE r.state = ?1
          AND r.verdict = ?2
          AND s.kind = ?3
          AND s.state = ?4
          AND NOT EXISTS (
              SELECT 1
              FROM review_followup_subtasks f
              WHERE f.review_id = r.review_id
          )
        ORDER BY r.updated_at ASC, r.created_at ASC
        "#,
    )?;
    let rows = stmt.query_map(
        params![
            review_state_name(ReviewState::Decided),
            review_verdict_name(ReviewVerdict::ChangesRequested),
            subtask_kind_name(SubtaskKind::Work),
            subtask_state_name(SubtaskState::ChangesRequested),
        ],
        |row| row.get::<_, String>(0),
    )?;
    let review_ids = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut created = 0;
    for review_id in review_ids {
        let review = load_review_tx(tx, &review_id)?;
        let source_subtask = load_subtask_tx(tx, review.subtask_id())?;
        let findings_digest = review
            .findings_digest()
            .ok_or(CoveyError::IllegalTransition {
                from: review.state().into(),
                to: SubtaskState::Available.into(),
                object: ObjectType::Subtask,
            })?;
        create_review_followup_subtask_tx(
            tx,
            &source_subtask,
            review.artifact_digest(),
            findings_digest,
            created_by_session,
            review.review_id(),
            now,
        )?;
        created += 1;
    }
    Ok(created)
}

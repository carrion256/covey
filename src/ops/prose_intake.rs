#![cfg_attr(coverage_nightly, coverage(off))]

use std::{collections::BTreeSet, str::FromStr, time::Instant};

use rusqlite::{OptionalExtension, Transaction, params, types::Type};

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        ArtifactDigest, CreateSubtaskRequest, EventType, ImportProseReq, ImportProseResult,
        MetaTaskId, ObjectType, ProseCurrentWork, ProseCurrentWorkBlocker, ProseCurrentWorkOwner,
        ProseCurrentWorkState, ProseTasksetId, QueueId, ReadyQueueState,
        RecordProseApplyBlockerReq, ReviewId, ReviewState, SubtaskId, SubtaskState,
    },
    ops::{meta_task::submit_meta_task_tx, workflow::create_subtask_tx},
    queries::collect_rows,
    schema::append_event,
    validators::require_role,
};

const PROSE_PROVENANCE_TIER: &str = "lightweight_prose_intake";

impl Covey {
    /// Imports a confirmed lightweight prose preview as a Covey meta-task with available work.
    pub fn import_prose(&self, req: ImportProseReq) -> Result<ImportProseResult> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                req.session_token.as_str(),
                "import_prose",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || import_prose_tx(tx, &req, now),
            )
        });
        self.log_operation(
            "import_prose",
            req.session_token.as_str(),
            started_at,
            &result,
            |result| {
                let mut affected = vec![format!("meta_task:{}", result.meta_task_id)];
                affected.extend(
                    result
                        .subtask_ids
                        .iter()
                        .map(|subtask_id| format!("subtask:{subtask_id}")),
                );
                affected
            },
        );
        result
    }

    /// Returns the batch-level current-work projection for one prose taskset.
    pub fn prose_current_work(&self, taskset_id: &str) -> Result<ProseCurrentWork> {
        let started_at = Instant::now();
        let taskset_id = ProseTasksetId::parse(taskset_id)?;
        let result = self.with_read_tx(|tx| load_prose_current_work_tx(tx, &taskset_id));
        self.log_operation(
            "prose_current_work",
            "system",
            started_at,
            &result,
            |work| {
                work.subtask_ids
                    .iter()
                    .map(|subtask_id| format!("subtask:{subtask_id}"))
                    .collect()
            },
        );
        result
    }

    /// Records a named blocker for prose-lane automatic apply.
    pub fn record_prose_apply_blocker(&self, req: RecordProseApplyBlockerReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                req.session_token.as_str(),
                "record_prose_apply_blocker",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(
                        tx,
                        req.session_token.as_str(),
                        &[crate::model::SessionRole::ApplyGate],
                    )?;
                    let exists = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM prose_tasksets WHERE taskset_id = ?1)",
                        params![req.taskset_id.as_str()],
                        |row| row.get::<_, i64>(0),
                    )?;
                    if exists == 0 {
                        return Err(CoveyError::MetaTaskNotFound);
                    }
                    tx.execute(
                        r#"
                        INSERT INTO prose_apply_blockers (
                            blocker_id, taskset_id, queue_id, artifact_digest, review_id,
                            reason, detail, created_at, updated_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
                        ON CONFLICT(blocker_id) DO UPDATE SET
                            detail = excluded.detail,
                            updated_at = excluded.updated_at
                        "#,
                        params![
                            req.blocker_id.as_str(),
                            req.taskset_id.as_str(),
                            req.queue_id.as_ref().map(QueueId::as_str),
                            req.artifact_digest.as_ref().map(ArtifactDigest::as_str),
                            req.review_id.as_ref().map(ReviewId::as_str),
                            req.reason.as_str(),
                            req.detail.as_str(),
                            now,
                        ],
                    )?;
                    append_event(
                        tx,
                        EventType::ProseApplyBlockerRecorded,
                        ObjectType::ReadyQueue,
                        req.queue_id
                            .as_ref()
                            .map(QueueId::as_str)
                            .unwrap_or_else(|| req.taskset_id.as_str()),
                        req.session_token.as_str(),
                        &req,
                        now,
                    )?;
                    Ok(())
                },
            )
        });
        self.log_operation(
            "record_prose_apply_blocker",
            req.session_token.as_str(),
            started_at,
            &result,
            |_| vec![format!("prose_taskset:{}", req.taskset_id)],
        );
        result
    }

    /// Returns the prose taskset id that owns one scoped subtask, when present.
    pub fn prose_taskset_id_for_subtask(&self, subtask_id: &str) -> Result<Option<ProseTasksetId>> {
        let subtask_id = SubtaskId::parse(subtask_id.to_owned())?;
        self.with_read_tx(|tx| prose_taskset_id_for_subtask_tx(tx, &subtask_id))
    }
}

fn import_prose_tx(
    tx: &Transaction<'_>,
    req: &ImportProseReq,
    now: i64,
) -> Result<ImportProseResult> {
    if req.tasks.is_empty() {
        return Err(CoveyError::InvalidImportDestination {
            reason: "prose import requires at least one task".to_owned(),
        });
    }
    require_role(
        tx,
        req.session_token.as_str(),
        &[crate::model::SessionRole::Orchestrator],
    )?;
    let meta_task_id = MetaTaskId::parse(submit_meta_task_tx(
        tx,
        req.session_token.as_str(),
        req.prompt_text.as_str(),
        req,
        now,
    )?)?;
    tx.execute(
        r#"
        INSERT INTO prose_tasksets (
            taskset_id, meta_task_id, provenance_tier, source_excerpt, preview_digest,
            created_by, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
        "#,
        params![
            req.taskset_id.as_str(),
            meta_task_id.as_str(),
            PROSE_PROVENANCE_TIER,
            req.source_excerpt.as_str(),
            req.preview_digest.as_str(),
            req.session_token.as_str(),
            now,
        ],
    )?;

    let mut subtask_ids = Vec::with_capacity(req.tasks.len());
    for (index, task) in req.tasks.iter().enumerate() {
        let create_req = CreateSubtaskRequest {
            session_token: req.session_token.clone(),
            meta_task_id: meta_task_id.clone(),
            subtask_id: None,
            title: task.title.clone(),
            priority: crate::model::SubtaskPriority::parse(100)?,
            idempotency_key: crate::model::IdempotencyKey::parse(format!(
                "{}:create-subtask:{index}",
                req.idempotency_key.as_str()
            ))?,
        };
        let subtask_id = SubtaskId::parse(create_subtask_tx(tx, &create_req, now)?)?;
        tx.execute(
            r#"
            INSERT INTO prose_subtask_scope (subtask_id, taskset_id, item_index, updated_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            params![
                subtask_id.as_str(),
                req.taskset_id.as_str(),
                i64::try_from(index).map_err(|_| CoveyError::InvalidImportDestination {
                    reason: "prose task index overflowed i64".to_owned(),
                })?,
                now,
            ],
        )?;
        subtask_ids.push(subtask_id);
    }

    Ok(ImportProseResult {
        taskset_id: req.taskset_id.clone(),
        meta_task_id,
        preview_digest: req.preview_digest.clone(),
        provenance_tier: PROSE_PROVENANCE_TIER.to_owned(),
        subtask_ids,
    })
}

fn prose_taskset_id_for_subtask_tx(
    tx: &Transaction<'_>,
    subtask_id: &SubtaskId,
) -> Result<Option<ProseTasksetId>> {
    tx.query_row(
        r#"
        WITH RECURSIVE prose_scope(subtask_id, taskset_id) AS (
            SELECT subtask_id, taskset_id
            FROM prose_subtask_scope
            UNION
            SELECT followup.followup_subtask_id, scope.taskset_id
            FROM review_followup_subtasks followup
            JOIN prose_scope scope
              ON followup.source_subtask_id = scope.subtask_id
        )
        SELECT taskset_id
        FROM prose_scope
        WHERE subtask_id = ?1
        LIMIT 1
        "#,
        params![subtask_id.as_str()],
        |row| {
            row.get::<_, String>(0).and_then(|raw| {
                ProseTasksetId::parse(raw).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(0, Type::Text, err.into())
                })
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn load_prose_current_work_tx(
    tx: &Transaction<'_>,
    taskset_id: &ProseTasksetId,
) -> Result<ProseCurrentWork> {
    let (meta_task_id, provenance_tier, preview_digest) = tx.query_row(
        r#"
        SELECT meta_task_id, provenance_tier, preview_digest
        FROM prose_tasksets
        WHERE taskset_id = ?1
        "#,
        params![taskset_id.as_str()],
        |row| {
            Ok((
                row.get::<_, MetaTaskId>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, ArtifactDigest>(2)?,
            ))
        },
    )?;
    let subtasks = load_prose_subtask_states_tx(tx, taskset_id)?;
    let queue_items = load_prose_queue_states_tx(tx, taskset_id)?;
    let reviews = load_prose_review_states_tx(tx, taskset_id)?;
    let blockers = load_prose_blockers_tx(tx, taskset_id)?;

    let subtask_ids = subtasks.iter().map(|row| row.subtask_id.clone()).collect();
    let queue_ids = queue_items.iter().map(|row| row.queue_id.clone()).collect();
    let artifact_digests = queue_items
        .iter()
        .map(|row| row.artifact_digest.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let review_ids = reviews.iter().map(|row| row.review_id.clone()).collect();
    let (state, next_owner) =
        classify_prose_current_work(&subtasks, &queue_items, &reviews, &blockers);

    Ok(ProseCurrentWork {
        taskset_id: taskset_id.clone(),
        meta_task_id,
        provenance_tier,
        preview_digest,
        state,
        next_owner,
        subtask_ids,
        queue_ids,
        artifact_digests,
        review_ids,
        blockers,
    })
}

#[derive(Debug)]
struct ProseSubtaskStateRow {
    subtask_id: SubtaskId,
    state: SubtaskState,
}

#[derive(Debug)]
struct ProseQueueStateRow {
    queue_id: QueueId,
    artifact_digest: ArtifactDigest,
    state: ReadyQueueState,
}

#[derive(Debug)]
struct ProseReviewStateRow {
    review_id: ReviewId,
    state: ReviewState,
}

fn load_prose_subtask_states_tx(
    tx: &Transaction<'_>,
    taskset_id: &ProseTasksetId,
) -> Result<Vec<ProseSubtaskStateRow>> {
    let mut stmt = tx.prepare(
        r#"
        WITH RECURSIVE prose_scope(subtask_id) AS (
            SELECT subtask_id
            FROM prose_subtask_scope
            WHERE taskset_id = ?1
            UNION
            SELECT followup.followup_subtask_id
            FROM review_followup_subtasks followup
            JOIN prose_scope scope
              ON followup.source_subtask_id = scope.subtask_id
        )
        SELECT subtasks.subtask_id, subtasks.state
        FROM prose_scope
        JOIN subtasks ON subtasks.subtask_id = prose_scope.subtask_id
        ORDER BY subtasks.created_at ASC, subtasks.subtask_id ASC
        "#,
    )?;
    let rows = stmt.query_map(params![taskset_id.as_str()], |row| {
        let raw_state: String = row.get(1)?;
        Ok(ProseSubtaskStateRow {
            subtask_id: row.get(0)?,
            state: SubtaskState::from_str(raw_state.as_str()).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(1, Type::Text, err.into())
            })?,
        })
    })?;
    collect_rows(rows)
}

fn load_prose_queue_states_tx(
    tx: &Transaction<'_>,
    taskset_id: &ProseTasksetId,
) -> Result<Vec<ProseQueueStateRow>> {
    let mut stmt = tx.prepare(
        r#"
        WITH RECURSIVE prose_scope(subtask_id) AS (
            SELECT subtask_id
            FROM prose_subtask_scope
            WHERE taskset_id = ?1
            UNION
            SELECT followup.followup_subtask_id
            FROM review_followup_subtasks followup
            JOIN prose_scope scope
              ON followup.source_subtask_id = scope.subtask_id
        )
        SELECT ready_queue.queue_id, ready_queue.artifact_digest, ready_queue.state
        FROM ready_queue
        JOIN prose_scope ON prose_scope.subtask_id = ready_queue.subtask_id
        ORDER BY ready_queue.enqueued_at ASC, ready_queue.queue_id ASC
        "#,
    )?;
    let rows = stmt.query_map(params![taskset_id.as_str()], |row| {
        let raw_state: String = row.get(2)?;
        Ok(ProseQueueStateRow {
            queue_id: row.get(0)?,
            artifact_digest: row.get(1)?,
            state: ReadyQueueState::from_str(raw_state.as_str()).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(2, Type::Text, err.into())
            })?,
        })
    })?;
    collect_rows(rows)
}

fn load_prose_review_states_tx(
    tx: &Transaction<'_>,
    taskset_id: &ProseTasksetId,
) -> Result<Vec<ProseReviewStateRow>> {
    let mut stmt = tx.prepare(
        r#"
        WITH RECURSIVE prose_scope(subtask_id) AS (
            SELECT subtask_id
            FROM prose_subtask_scope
            WHERE taskset_id = ?1
            UNION
            SELECT followup.followup_subtask_id
            FROM review_followup_subtasks followup
            JOIN prose_scope scope
              ON followup.source_subtask_id = scope.subtask_id
        )
        SELECT reviews.review_id, reviews.state
        FROM reviews
        JOIN prose_scope ON prose_scope.subtask_id = reviews.subtask_id
        ORDER BY reviews.created_at ASC, reviews.review_id ASC
        "#,
    )?;
    let rows = stmt.query_map(params![taskset_id.as_str()], |row| {
        let raw_state: String = row.get(1)?;
        Ok(ProseReviewStateRow {
            review_id: row.get(0)?,
            state: ReviewState::from_str(raw_state.as_str()).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(1, Type::Text, err.into())
            })?,
        })
    })?;
    collect_rows(rows)
}

fn load_prose_blockers_tx(
    tx: &Transaction<'_>,
    taskset_id: &ProseTasksetId,
) -> Result<Vec<ProseCurrentWorkBlocker>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT blocker_id, queue_id, artifact_digest, review_id, reason, detail
        FROM prose_apply_blockers
        WHERE taskset_id = ?1
        ORDER BY created_at ASC, blocker_id ASC
        "#,
    )?;
    let rows = stmt.query_map(params![taskset_id.as_str()], |row| {
        let blocker_id: String = row.get(0)?;
        let queue_id = row.get::<_, Option<QueueId>>(1)?;
        let artifact_digest = row.get::<_, Option<ArtifactDigest>>(2)?;
        let review_id = row.get::<_, Option<ReviewId>>(3)?;
        let reason: String = row.get(4)?;
        let detail: String = row.get(5)?;
        Ok(ProseCurrentWorkBlocker {
            evidence_id: format!("prose_current_work:{reason}:{blocker_id}"),
            blocker_id,
            owner: ProseCurrentWorkOwner::Operator,
            queue_id,
            artifact_digest,
            review_id,
            reason: detail,
        })
    })?;
    collect_rows(rows)
}

fn classify_prose_current_work(
    subtasks: &[ProseSubtaskStateRow],
    queue_items: &[ProseQueueStateRow],
    reviews: &[ProseReviewStateRow],
    blockers: &[ProseCurrentWorkBlocker],
) -> (ProseCurrentWorkState, ProseCurrentWorkOwner) {
    if !blockers.is_empty()
        || subtasks.iter().any(|row| {
            matches!(
                row.state,
                SubtaskState::Blocked | SubtaskState::Abandoned | SubtaskState::ChangesRequested
            )
        })
    {
        return (
            ProseCurrentWorkState::Blocked,
            ProseCurrentWorkOwner::Operator,
        );
    }
    if !queue_items.is_empty()
        && queue_items
            .iter()
            .all(|row| row.state == ReadyQueueState::Applied)
        && subtasks
            .iter()
            .all(|row| row.state == SubtaskState::Applied)
    {
        return (ProseCurrentWorkState::Applied, ProseCurrentWorkOwner::Covey);
    }
    if queue_items.iter().any(|row| {
        matches!(
            row.state,
            ReadyQueueState::Queued | ReadyQueueState::InFlight
        )
    }) || subtasks
        .iter()
        .any(|row| row.state == SubtaskState::ReadyForApply)
    {
        return (
            ProseCurrentWorkState::Applying,
            ProseCurrentWorkOwner::SchedulerApply,
        );
    }
    if reviews.iter().any(|row| row.state != ReviewState::Decided)
        || subtasks.iter().any(|row| {
            matches!(
                row.state,
                SubtaskState::ArtifactPublished | SubtaskState::ReviewPending
            )
        })
    {
        return (
            ProseCurrentWorkState::Reviewing,
            ProseCurrentWorkOwner::Reviewer,
        );
    }
    if subtasks
        .iter()
        .any(|row| matches!(row.state, SubtaskState::Claimed | SubtaskState::InProgress))
    {
        return (
            ProseCurrentWorkState::Claimed,
            ProseCurrentWorkOwner::Executor,
        );
    }
    (
        ProseCurrentWorkState::Imported,
        ProseCurrentWorkOwner::Covey,
    )
}

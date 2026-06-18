use std::time::Instant;

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    Covey,
    error::Result,
    model::{
        OpenSpecArchiveStatus, OpenSpecChangeId, OpenSpecCurrentWork, ReadyQueueItem, Review,
        SubtaskId, SubtaskRow, SubtaskView,
    },
    queries::{collect_rows, deserialize_row},
};

impl Covey {
    /// Returns the Covey-derived current-work projection for one OpenSpec change.
    pub fn openspec_current_work(&self, change_id: &str) -> Result<OpenSpecCurrentWork> {
        let started_at = Instant::now();
        let change_id = OpenSpecChangeId::parse(change_id)?;
        let result = self.with_read_tx(|tx| {
            let subtasks = load_current_work_subtasks_tx(tx, &change_id)?;
            let reviews = load_current_work_reviews_tx(tx, &change_id)?;
            let queue_items = load_current_work_queue_items_tx(tx, &change_id)?;
            let archive_statuses = load_current_work_archive_statuses_tx(tx, &change_id)?;
            Ok(OpenSpecCurrentWork::from_parts(
                change_id.clone(),
                subtasks,
                reviews,
                queue_items,
                archive_statuses,
            ))
        });
        self.log_operation(
            "openspec_current_work",
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

    /// Returns the OpenSpec change id that owns one scoped Covey subtask.
    pub fn openspec_change_id_for_subtask(
        &self,
        subtask_id: &str,
    ) -> Result<Option<OpenSpecChangeId>> {
        let started_at = Instant::now();
        let subtask_id = SubtaskId::parse(subtask_id)?;
        let result = self.with_read_tx(|tx| openspec_change_id_for_subtask_tx(tx, &subtask_id));
        self.log_operation(
            "openspec_change_id_for_subtask",
            "system",
            started_at,
            &result,
            |change_id| {
                change_id
                    .as_ref()
                    .map(|change_id| vec![format!("openspec:{}", change_id.as_str())])
                    .unwrap_or_default()
            },
        );
        result
    }
}

fn openspec_change_id_for_subtask_tx(
    tx: &Transaction<'_>,
    subtask_id: &SubtaskId,
) -> Result<Option<OpenSpecChangeId>> {
    tx.query_row(
        "SELECT openspec_change_id FROM openspec_subtask_scope WHERE subtask_id = ?1",
        params![subtask_id.as_str()],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(OpenSpecChangeId::parse)
    .transpose()
    .map_err(Into::into)
}

fn load_current_work_subtasks_tx(
    tx: &Transaction<'_>,
    change_id: &OpenSpecChangeId,
) -> Result<Vec<SubtaskView>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT s.subtask_id, s.meta_task_id, s.title, s.kind,
               s.review_target_subtask_id, s.review_target_artifact_digest,
               s.state, s.current_claim_id, s.artifact_digest, s.priority,
               s.created_at, s.updated_at
        FROM openspec_subtask_scope scope
        JOIN subtasks s ON s.subtask_id = scope.subtask_id
        WHERE scope.openspec_change_id = ?1
        ORDER BY s.created_at ASC, s.subtask_id ASC
        "#,
    )?;
    let rows = stmt.query_map(params![change_id.as_str()], |row| {
        let subtask = deserialize_row::<SubtaskRow>(row)?;
        SubtaskView::try_from(subtask)
    })?;
    collect_rows(rows)
}

fn load_current_work_reviews_tx(
    tx: &Transaction<'_>,
    change_id: &OpenSpecChangeId,
) -> Result<Vec<Review>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT r.review_id, r.subtask_id, r.artifact_digest, r.reviewer_session,
               r.review_subtask_id, r.verdict, r.findings_digest, r.state,
               r.created_at, r.updated_at
        FROM openspec_subtask_scope scope
        JOIN reviews r ON r.subtask_id = scope.subtask_id
        WHERE scope.openspec_change_id = ?1
        ORDER BY r.created_at ASC, r.review_id ASC
        "#,
    )?;
    let rows = stmt.query_map(params![change_id.as_str()], deserialize_row::<Review>)?;
    collect_rows(rows)
}

fn load_current_work_queue_items_tx(
    tx: &Transaction<'_>,
    change_id: &OpenSpecChangeId,
) -> Result<Vec<ReadyQueueItem>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT q.queue_id, q.artifact_digest, q.subtask_id, q.settlement_target,
               q.state, q.claimed_by_session_token, q.claim_fence_seq,
               q.claim_lease_deadline, q.enqueued_at, q.updated_at
        FROM openspec_subtask_scope scope
        JOIN ready_queue q ON q.subtask_id = scope.subtask_id
        WHERE scope.openspec_change_id = ?1
        ORDER BY q.enqueued_at ASC, q.queue_id ASC
        "#,
    )?;
    let rows = stmt.query_map(
        params![change_id.as_str()],
        deserialize_row::<ReadyQueueItem>,
    )?;
    collect_rows(rows)
}

fn load_current_work_archive_statuses_tx(
    tx: &Transaction<'_>,
    change_id: &OpenSpecChangeId,
) -> Result<Vec<OpenSpecArchiveStatus>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT queue_id, subtask_id, artifact_digest, openspec_change_id, state,
               blocked_reason, archive_proof_digest, recorded_by_session,
               created_at, updated_at
        FROM openspec_archive_status
        WHERE openspec_change_id = ?1
        ORDER BY updated_at ASC, queue_id ASC
        "#,
    )?;
    let rows = stmt.query_map(
        params![change_id.as_str()],
        deserialize_row::<OpenSpecArchiveStatus>,
    )?;
    collect_rows(rows)
}

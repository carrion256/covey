#![cfg_attr(coverage_nightly, coverage(off))]

use std::time::Instant;

use rusqlite::Transaction;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        AnnotateMetaTaskReq, CancelMetaTaskReq, ClaimState, CompleteMetaTaskReq,
        CoveyTypeValidationError, CreateSubtaskRequest, DueDate, EventType, FieldChange,
        LinkMetaTaskSourceReq, MetaTask, MetaTaskAnnotationChange, MetaTaskAnnotationValue,
        MetaTaskClosure, MetaTaskHandle, MetaTaskId, MetaTaskListQuery, MetaTaskSourceLinkDraft,
        MetaTaskSourceLinkProvenance, MetaTaskState, MetaTaskStatus, MetaTaskSubmission,
        MetaTaskSummary, MetaTaskTitle, MetaTaskUrgency, ObjectType, ReadyQueueState, SessionRole,
        SubmitMetaTaskReq, SubmittedSubtask, SubtaskId, SubtaskPriority, SubtaskState, TimestampMs,
        UpdateMetaTaskReq, claim_state_name, meta_task_annotation_key_name, meta_task_closure_name,
        meta_task_state_name, meta_task_urgency_name, ready_queue_state_name, subtask_state_name,
    },
    ops::workflow::create_work_subtask_tx,
    queries::{
        count_open_subtasks_for_meta_task_tx, load_meta_task_annotations_tx,
        load_meta_task_dedupe_tx, load_meta_task_id_for_handle_tx, load_meta_task_source_link_tx,
        load_meta_task_summary_page_tx, load_meta_task_summary_tx, load_meta_task_superseded_by_tx,
        load_meta_task_supersedes_tx, load_meta_task_tx, load_subtasks_for_meta_task_tx,
    },
    store::{append_session_event, load_claims_for_meta_task},
    validators::{
        MAX_OBJECT_ID_LEN, MAX_PROMPT_LEN, ensure_length, ensure_transition, require_role,
    },
};

/// Largest page a discovery listing returns in one call.
const MAX_LIST_LIMIT: usize = 500;

/// Priority of a subtask filed without one; `subtask create` defaults to the
/// same value.
const DEFAULT_SUBTASK_PRIORITY: i64 = 100;

/// What a submission stores for an idempotent replay.
///
/// A submission without subtasks stores the bare meta-task id, which is also
/// the shape of every record written before subtasks could be filed with the
/// task, so those records still replay.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum StoredSubmission {
    MetaTaskId(String),
    WithSubtasks {
        meta_task_id: String,
        subtasks: Vec<SubmittedSubtask>,
    },
}

impl StoredSubmission {
    fn new(meta_task_id: String, subtasks: Vec<SubmittedSubtask>) -> Self {
        if subtasks.is_empty() {
            Self::MetaTaskId(meta_task_id)
        } else {
            Self::WithSubtasks {
                meta_task_id,
                subtasks,
            }
        }
    }

    fn into_parts(self) -> (String, Vec<SubmittedSubtask>) {
        match self {
            Self::MetaTaskId(meta_task_id) => (meta_task_id, Vec::new()),
            Self::WithSubtasks {
                meta_task_id,
                subtasks,
            } => (meta_task_id, subtasks),
        }
    }
}

impl Covey {
    /// Creates a new planning meta-task and returns its id.
    pub fn submit_meta_task(&self, req: SubmitMetaTaskReq) -> Result<String> {
        self.submit_meta_task_reporting(req)
            .map(|submission| submission.meta_task_id)
    }

    /// Creates a new planning meta-task, reporting whether a dedupe key
    /// resolved the submission to a task the principal already filed.
    ///
    /// A request that names subtask titles files one work subtask per title in
    /// the same transaction, after the `meta_task_submitted` event, and the
    /// task is `active` when the call returns. A dedupe hit files nothing.
    pub fn submit_meta_task_reporting(&self, req: SubmitMetaTaskReq) -> Result<MetaTaskSubmission> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let session =
                require_role(tx, req.session_token.as_str(), &[SessionRole::Orchestrator])?;
            // A dedupe key resolves to the principal's open task under that key
            // first and writes nothing at all: no task, no event, and no
            // idempotency row.
            if let Some(dedupe_key) = req.dedupe_key.as_ref()
                && let Some((existing, state)) = load_meta_task_dedupe_tx(
                    tx,
                    session.agent_principal_id.as_str(),
                    dedupe_key.as_str(),
                )?
            {
                return Ok(MetaTaskSubmission {
                    meta_task_id: existing.to_string(),
                    deduplicated: true,
                    state,
                    subtasks: Vec::new(),
                });
            }
            let (meta_task_id, subtasks) = crate::store::with_idempotent_mutation(
                tx,
                req.session_token.as_str(),
                "submit_meta_task",
                &req.idempotency_key,
                &req,
                TimestampMs::parse(now)?,
                || {
                    let meta_task_id = submit_meta_task_with_tracking_tx(tx, &req, now)?;
                    let subtasks = self.submit_subtasks_tx(tx, &req, &meta_task_id, now)?;
                    Ok(StoredSubmission::new(meta_task_id, subtasks))
                },
            )?
            .into_parts();
            let state = load_meta_task_tx(tx, meta_task_id.as_str())?.state();
            Ok(MetaTaskSubmission {
                meta_task_id,
                deduplicated: false,
                state,
                subtasks,
            })
        });
        self.log_operation(
            "submit_meta_task",
            req.session_token.as_str(),
            started_at,
            &result,
            |submission| vec![format!("meta_task:{}", submission.meta_task_id)],
        );
        result
    }

    /// Files the work subtasks a submission asked for under its new meta task,
    /// one per title in request order, each resolved through the opened
    /// workflow profile as [`Covey::create_subtask`] resolves it.
    fn submit_subtasks_tx(
        &self,
        tx: &Transaction<'_>,
        req: &SubmitMetaTaskReq,
        meta_task_id: &str,
        now: i64,
    ) -> Result<Vec<SubmittedSubtask>> {
        let meta_task_id = MetaTaskId::parse(meta_task_id.to_owned())?;
        req.subtask_titles
            .iter()
            .map(|title| {
                let create = self.resolve_subtask_request(CreateSubtaskRequest {
                    session_token: req.session_token.clone(),
                    meta_task_id: meta_task_id.clone(),
                    subtask_id: None,
                    title: title.clone(),
                    priority: SubtaskPriority::parse(DEFAULT_SUBTASK_PRIORITY)?,
                    depends_on: Vec::new(),
                    idempotency_key: req.idempotency_key.clone(),
                });
                let subtask_id = create_work_subtask_tx(tx, &create, now)?;
                Ok(SubmittedSubtask {
                    subtask_id: SubtaskId::parse(subtask_id)?,
                    title: title.clone(),
                })
            })
            .collect()
    }

    /// Cancels a meta-task that has not already reached a terminal state.
    pub fn cancel_meta_task(&self, req: CancelMetaTaskReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                req.session_token.as_str(),
                "cancel_meta_task",
                &req.idempotency_key,
                &req,
                TimestampMs::parse(now)?,
                || cancel_meta_task_tx(tx, &req, now),
            )
        });
        self.log_operation(
            "cancel_meta_task",
            req.session_token.as_str(),
            started_at,
            &result,
            |_| vec![format!("meta_task:{}", req.meta_task_id)],
        );
        result
    }

    /// Completes a meta-task that has no open subtasks.
    ///
    /// The task moves to `completed`, stores the typed closure (`done` when the
    /// request names none), and records one `meta_task_completed` event. It
    /// tears nothing down: no claim is revoked and no subtask is abandoned.
    pub fn complete_meta_task(&self, req: CompleteMetaTaskReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                req.session_token.as_str(),
                "complete_meta_task",
                &req.idempotency_key,
                &req,
                TimestampMs::parse(now)?,
                || complete_meta_task_tx(tx, &req, now),
            )
        });
        self.log_operation(
            "complete_meta_task",
            req.session_token.as_str(),
            started_at,
            &result,
            |_| vec![format!("meta_task:{}", req.meta_task_id)],
        );
        result
    }

    /// Sets or clears one annotation of a meta task.
    pub fn annotate_meta_task(&self, req: AnnotateMetaTaskReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                req.session_token.as_str(),
                "annotate_meta_task",
                &req.idempotency_key,
                &req,
                TimestampMs::parse(now)?,
                || annotate_meta_task_tx(tx, &req, now),
            )
        });
        self.log_operation(
            "annotate_meta_task",
            req.session_token.as_str(),
            started_at,
            &result,
            |_| vec![format!("meta_task:{}", req.meta_task_id)],
        );
        result
    }

    /// Sets or clears the typed tracking fields of a meta task: title, urgency,
    /// and due date.
    ///
    /// The task may be in any state. One `meta_task_updated` event records the
    /// request. Returns the task's summary row as it reads after the update.
    pub fn update_meta_task(&self, req: UpdateMetaTaskReq) -> Result<MetaTaskSummary> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                req.session_token.as_str(),
                "update_meta_task",
                &req.idempotency_key,
                &req,
                TimestampMs::parse(now)?,
                || update_meta_task_tx(tx, &req, now),
            )?;
            load_meta_task_summary_tx(tx, req.meta_task_id.as_str())
        });
        self.log_operation(
            "update_meta_task",
            req.session_token.as_str(),
            started_at,
            &result,
            |_| vec![format!("meta_task:{}", req.meta_task_id)],
        );
        result
    }

    /// Attaches an intake source backlink to an existing meta task.
    ///
    /// Provenance is set once: a meta task that already carries a link is
    /// refused with [`CoveyError::SourceLinkExists`]. Replaying the same
    /// idempotency key returns the original success without a second row.
    pub fn link_meta_task_source(&self, req: LinkMetaTaskSourceReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                req.session_token.as_str(),
                "link_meta_task_source",
                &req.idempotency_key,
                &req,
                TimestampMs::parse(now)?,
                || link_meta_task_source_tx(tx, &req, now),
            )
        });
        self.log_operation(
            "link_meta_task_source",
            req.session_token.as_str(),
            started_at,
            &result,
            |_| vec![format!("meta_task:{}", req.meta_task_id)],
        );
        result
    }

    /// Returns the current persisted status for a meta-task and its subtasks,
    /// with its annotations and its replacement links.
    pub fn meta_task_status(&self, meta_task_id: &str) -> Result<MetaTaskStatus> {
        let started_at = Instant::now();
        let result = self.with_read_tx(|tx| {
            let meta_task = load_meta_task_tx(tx, meta_task_id)?;
            let source_link = load_meta_task_source_link_tx(tx, meta_task_id)?;
            let meta_task = meta_task.with_source_link(source_link);
            let annotations =
                load_meta_task_annotations_tx(tx, std::slice::from_ref(&meta_task.meta_task_id))?
                    .remove(&meta_task.meta_task_id)
                    .unwrap_or_default();
            let supersedes = load_meta_task_supersedes_tx(tx, meta_task_id)?;
            let superseded_by = load_meta_task_superseded_by_tx(tx, meta_task_id)?;
            let subtasks = load_subtasks_for_meta_task_tx(tx, meta_task_id)?
                .into_iter()
                .map(crate::model::SubtaskView::try_from)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            MetaTaskStatus::new(meta_task, subtasks)
                .map(|status| {
                    status
                        .with_annotations(annotations)
                        .with_supersedes(supersedes)
                        .with_superseded_by(superseded_by)
                })
                .map_err(|reason| CoveyError::InvalidObservabilityRow { reason })
        });
        self.log_operation(
            "meta_task_status",
            "system",
            started_at,
            &result,
            |status| vec![format!("meta_task:{}", status.meta_task().meta_task_id)],
        );
        result
    }

    /// Resolves the meta task a store-issued handle numbers.
    pub fn meta_task_id_for_handle(&self, handle: MetaTaskHandle) -> Result<MetaTaskId> {
        let started_at = Instant::now();
        let result = self.with_read_tx(|tx| load_meta_task_id_for_handle_tx(tx, handle));
        self.log_operation(
            "meta_task_id_for_handle",
            "system",
            started_at,
            &result,
            |meta_task_id| vec![format!("meta_task:{meta_task_id}")],
        );
        result
    }

    /// Returns a bounded page of meta-tasks ordered newest-created first.
    ///
    /// The returned summaries never contain creator session tokens.
    ///
    /// # Errors
    ///
    /// Returns an error when `limit` is outside `1..=500` or `offset`
    /// exceeds the SQLite signed 64-bit range.
    pub fn list_meta_tasks(&self, limit: usize, offset: u64) -> Result<Vec<MetaTaskSummary>> {
        self.list_meta_tasks_with(&MetaTaskListQuery::new(limit, offset))
    }

    /// Returns one filtered, ordered page of meta-task summaries.
    ///
    /// # Errors
    ///
    /// Returns an error when the query's limit is outside `1..=500` or its
    /// offset exceeds the SQLite signed 64-bit range.
    pub fn list_meta_tasks_with(&self, query: &MetaTaskListQuery) -> Result<Vec<MetaTaskSummary>> {
        validate_meta_task_page(query.limit(), query.offset())?;
        let started_at = Instant::now();
        let result = self.with_read_tx(|tx| load_meta_task_summary_page_tx(tx, query));
        self.log_operation(
            "list_meta_tasks",
            "system",
            started_at,
            &result,
            |summaries| {
                summaries
                    .iter()
                    .map(|summary| format!("meta_task:{}", summary.meta_task_id()))
                    .collect()
            },
        );
        result
    }
}

fn validate_meta_task_page(limit: usize, offset: u64) -> Result<()> {
    if !(1..=MAX_LIST_LIMIT).contains(&limit) {
        return Err(CoveyTypeValidationError::new("limit", "must be between 1 and 500").into());
    }
    i64::try_from(offset)
        .map_err(|_| CoveyTypeValidationError::new("offset", "exceeds the maximum page offset"))?;
    Ok(())
}

/// Cancels one meta task, storing the resolved closure.
///
/// `closure` is the value written on the row; the event payload carries the
/// same value, so a reader never has to re-derive a verb's default.
fn cancel_meta_task_tx(tx: &Transaction<'_>, req: &CancelMetaTaskReq, now: i64) -> Result<()> {
    require_role(tx, req.session_token.as_str(), &[SessionRole::Orchestrator])?;
    ensure_length("meta_task_id", req.meta_task_id.as_str(), MAX_OBJECT_ID_LEN)?;
    let meta_task = load_meta_task_tx(tx, req.meta_task_id.as_str())?;
    let closure = req.closure.unwrap_or(MetaTaskClosure::Dropped);
    let event_payload = CancelMetaTaskReq {
        session_token: req.session_token.clone(),
        meta_task_id: req.meta_task_id.clone(),
        closure: Some(closure),
        idempotency_key: req.idempotency_key.clone(),
    };
    retire_meta_task_tx(
        tx,
        &meta_task,
        req.session_token.as_str(),
        closure,
        &event_payload,
        now,
    )
}

/// Completes one meta task without tearing anything down.
fn complete_meta_task_tx(tx: &Transaction<'_>, req: &CompleteMetaTaskReq, now: i64) -> Result<()> {
    require_role(tx, req.session_token.as_str(), &[SessionRole::Orchestrator])?;
    ensure_length("meta_task_id", req.meta_task_id.as_str(), MAX_OBJECT_ID_LEN)?;
    let meta_task = load_meta_task_tx(tx, req.meta_task_id.as_str())?;
    ensure_transition(
        meta_task.state(),
        MetaTaskState::Completed,
        ObjectType::MetaTask,
        !matches!(meta_task.state(), MetaTaskState::Cancelled),
    )?;
    let open_subtasks = count_open_subtasks_for_meta_task_tx(tx, req.meta_task_id.as_str())?;
    if open_subtasks > 0 {
        return Err(CoveyError::MetaTaskHasOpenSubtasks {
            meta_task_id: req.meta_task_id.to_string(),
            open_subtasks,
        });
    }
    let closure = req.closure.unwrap_or(MetaTaskClosure::Done);
    let updated = tx.execute(
        r#"
        UPDATE meta_tasks
        SET state = ?2, closure = ?3, updated_at = ?4
        WHERE meta_task_id = ?1 AND state != ?5
        "#,
        params![
            req.meta_task_id.as_str(),
            meta_task_state_name(MetaTaskState::Completed),
            meta_task_closure_name(closure),
            now,
            meta_task_state_name(MetaTaskState::Cancelled),
        ],
    )?;
    if updated != 1 {
        return Err(CoveyError::IllegalTransition {
            from: meta_task.state().into(),
            to: MetaTaskState::Completed.into(),
            object: ObjectType::MetaTask,
        });
    }
    let event_payload = CompleteMetaTaskReq {
        session_token: req.session_token.clone(),
        meta_task_id: req.meta_task_id.clone(),
        closure: Some(closure),
        idempotency_key: req.idempotency_key.clone(),
    };
    append_session_event(
        tx,
        EventType::MetaTaskCompleted,
        ObjectType::MetaTask,
        req.meta_task_id.as_str(),
        req.session_token.as_str(),
        &event_payload,
        now,
    )?;
    Ok(())
}

/// Writes one annotation and records the request as one event.
fn annotate_meta_task_tx(tx: &Transaction<'_>, req: &AnnotateMetaTaskReq, now: i64) -> Result<()> {
    require_role(tx, req.session_token.as_str(), &[SessionRole::Orchestrator])?;
    ensure_length("meta_task_id", req.meta_task_id.as_str(), MAX_OBJECT_ID_LEN)?;
    load_meta_task_tx(tx, req.meta_task_id.as_str())?;
    match &req.change {
        MetaTaskAnnotationChange::Set(value) => {
            let key = value.key();
            tx.execute(
                r#"
                INSERT INTO meta_task_annotations (meta_task_id, key, value, updated_by, updated_at)
                VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT(meta_task_id, key) DO UPDATE SET
                    value = excluded.value,
                    updated_by = excluded.updated_by,
                    updated_at = excluded.updated_at
                "#,
                params![
                    req.meta_task_id.as_str(),
                    meta_task_annotation_key_name(key),
                    stored_annotation_value(value),
                    req.session_token.as_str(),
                    now,
                ],
            )?;
        }
        MetaTaskAnnotationChange::Clear(key) => {
            // Clearing a key that was never set is the same successful no-op as
            // clearing one whose value is already gone.
            tx.execute(
                "DELETE FROM meta_task_annotations WHERE meta_task_id = ?1 AND key = ?2",
                params![
                    req.meta_task_id.as_str(),
                    meta_task_annotation_key_name(*key)
                ],
            )?;
        }
    }
    append_session_event(
        tx,
        EventType::MetaTaskAnnotated,
        ObjectType::MetaTask,
        req.meta_task_id.as_str(),
        req.session_token.as_str(),
        req,
        now,
    )?;
    Ok(())
}

/// The text one annotation value is stored as.
///
/// `acked` holds the `true`/`false` literals the table constrains; every other
/// key holds its already-trimmed text.
fn stored_annotation_value(value: &MetaTaskAnnotationValue) -> String {
    match value {
        MetaTaskAnnotationValue::Verdict(text)
        | MetaTaskAnnotationValue::Tier(text)
        | MetaTaskAnnotationValue::Entity(text) => text.clone(),
        MetaTaskAnnotationValue::Acked(flag) => flag.to_string(),
    }
}

/// Applies the field changes of one update request and records the request as
/// one event.
///
/// The task may be in any state: tracking fields describe a task, they do not
/// move it.
fn update_meta_task_tx(tx: &Transaction<'_>, req: &UpdateMetaTaskReq, now: i64) -> Result<()> {
    require_role(tx, req.session_token.as_str(), &[SessionRole::Orchestrator])?;
    ensure_length("meta_task_id", req.meta_task_id.as_str(), MAX_OBJECT_ID_LEN)?;
    if !req.has_changes() {
        return Err(CoveyTypeValidationError::new(
            "changes",
            "at least one of title, urgency, or due_on must change",
        )
        .into());
    }
    load_meta_task_tx(tx, req.meta_task_id.as_str())?;
    let (writes_title, title) =
        column_change(req.title.as_ref(), |title| title.as_str().to_owned());
    let (writes_urgency, urgency) = column_change(req.urgency.as_ref(), |urgency| {
        meta_task_urgency_name(*urgency).to_owned()
    });
    let (writes_due_on, due_on) = column_change(req.due_on.as_ref(), ToString::to_string);
    tx.execute(
        r#"
        UPDATE meta_tasks
        SET title = CASE WHEN ?2 THEN ?3 ELSE title END,
            urgency = CASE WHEN ?4 THEN ?5 ELSE urgency END,
            due_on = CASE WHEN ?6 THEN ?7 ELSE due_on END,
            updated_at = ?8
        WHERE meta_task_id = ?1
        "#,
        params![
            req.meta_task_id.as_str(),
            writes_title,
            title,
            writes_urgency,
            urgency,
            writes_due_on,
            due_on,
            now,
        ],
    )?;
    append_session_event(
        tx,
        EventType::MetaTaskUpdated,
        ObjectType::MetaTask,
        req.meta_task_id.as_str(),
        req.session_token.as_str(),
        req,
        now,
    )?;
    Ok(())
}

/// The SQL parameters of one optional field change: whether the column is
/// rewritten, and the text it is rewritten to, where `None` writes NULL.
fn column_change<T>(
    change: Option<&FieldChange<T>>,
    text: impl Fn(&T) -> String,
) -> (bool, Option<String>) {
    match change {
        Some(FieldChange::Set(value)) => (true, Some(text(value))),
        Some(FieldChange::Clear) => (true, None),
        None => (false, None),
    }
}

/// Retires one meta task and everything it holds.
///
/// Refuses a task that already reached a terminal state, stores `closure` on
/// the row, revokes held claims, cancels queued work, abandons open subtasks,
/// and records the cancellation event with `event_payload`.
fn retire_meta_task_tx<T: Serialize>(
    tx: &Transaction<'_>,
    meta_task: &MetaTask,
    session_token: &str,
    closure: MetaTaskClosure,
    event_payload: &T,
    now: i64,
) -> Result<()> {
    let meta_task_id = meta_task.meta_task_id.as_str();
    ensure_transition(
        meta_task.state(),
        MetaTaskState::Cancelled,
        ObjectType::MetaTask,
        !matches!(
            meta_task.state(),
            MetaTaskState::Completed | MetaTaskState::Cancelled
        ),
    )?;
    let updated = tx.execute(
        r#"
        UPDATE meta_tasks
        SET state = ?2, closure = ?3, updated_at = ?4
        WHERE meta_task_id = ?1 AND state NOT IN (?5, ?6)
        "#,
        params![
            meta_task_id,
            meta_task_state_name(MetaTaskState::Cancelled),
            meta_task_closure_name(closure),
            now,
            meta_task_state_name(MetaTaskState::Completed),
            meta_task_state_name(MetaTaskState::Cancelled),
        ],
    )?;
    if updated != 1 {
        return Err(CoveyError::IllegalTransition {
            from: meta_task.state().into(),
            to: MetaTaskState::Cancelled.into(),
            object: ObjectType::MetaTask,
        });
    }
    let held_claims = load_claims_for_meta_task(tx, meta_task_id)?;
    for claim in &held_claims {
        tx.execute(
            "UPDATE claims SET state = ?2, updated_at = ?3 WHERE claim_id = ?1 AND state = ?4",
            params![
                claim.claim_id,
                claim_state_name(ClaimState::Revoked),
                now,
                claim_state_name(ClaimState::Held),
            ],
        )?;
        tx.execute(
            "UPDATE sessions SET active_subtask_id = NULL, updated_at = ?3 WHERE session_token = ?1 AND active_subtask_id = ?2",
            params![claim.owner_session_token, claim.subtask_id, now],
        )?;
    }
    tx.execute(
        r#"
        UPDATE ready_queue
        SET state = ?2,
            claimed_by_session_token = NULL,
            claim_lease_deadline = NULL,
            updated_at = ?3
        WHERE subtask_id IN (
            SELECT subtask_id FROM subtasks WHERE meta_task_id = ?1
        )
          AND state IN (?4, ?5)
        "#,
        params![
            meta_task_id,
            ready_queue_state_name(ReadyQueueState::Cancelled),
            now,
            ready_queue_state_name(ReadyQueueState::Queued),
            ready_queue_state_name(ReadyQueueState::InFlight),
        ],
    )?;
    tx.execute(
        r#"
        UPDATE subtasks
        SET state = CASE
                WHEN state IN (?2, ?3, ?4, ?5, ?6) THEN state
                ELSE ?7
            END,
            current_claim_id = NULL,
            updated_at = ?8
        WHERE meta_task_id = ?1
        "#,
        params![
            meta_task_id,
            subtask_state_name(SubtaskState::Applied),
            subtask_state_name(SubtaskState::Completed),
            subtask_state_name(SubtaskState::Failed),
            subtask_state_name(SubtaskState::Abandoned),
            subtask_state_name(SubtaskState::Decided),
            subtask_state_name(SubtaskState::Abandoned),
            now,
        ],
    )?;
    append_session_event(
        tx,
        EventType::MetaTaskCancelled,
        ObjectType::MetaTask,
        meta_task_id,
        session_token,
        event_payload,
        now,
    )?;
    Ok(())
}

/// Creates a meta task with no typed tracking fields.
///
/// This is the plain submission path used by importers: it writes a planning
/// row, an optional source link, and one submission event.
pub(crate) fn submit_meta_task_tx<T: serde::Serialize>(
    tx: &Transaction<'_>,
    session_token: &str,
    prompt_text: &str,
    source_link: Option<&MetaTaskSourceLinkDraft>,
    event_payload: &T,
    now: i64,
) -> Result<String> {
    require_role(tx, session_token, &[SessionRole::Orchestrator])?;
    ensure_length("prompt_text", prompt_text, MAX_PROMPT_LEN)?;
    let meta_task_id = crate::model::make_id("meta");
    insert_meta_task_row_tx(
        tx,
        &meta_task_id,
        session_token,
        prompt_text,
        source_link,
        None,
        None,
        None,
        now,
    )?;
    append_session_event(
        tx,
        EventType::MetaTaskSubmitted,
        ObjectType::MetaTask,
        &meta_task_id,
        session_token,
        event_payload,
        now,
    )?;
    Ok(meta_task_id)
}

/// Creates one meta task from a submission request that carries every tracking
/// field: typed columns, a dedupe row, and the tasks it replaces.
fn submit_meta_task_with_tracking_tx(
    tx: &Transaction<'_>,
    req: &SubmitMetaTaskReq,
    now: i64,
) -> Result<String> {
    let session = require_role(tx, req.session_token.as_str(), &[SessionRole::Orchestrator])?;
    ensure_length("prompt_text", req.prompt_text.as_str(), MAX_PROMPT_LEN)?;
    // Duplicates are folded while first-seen order is kept: a repeated id would
    // otherwise retry a replacement that the first occurrence already wrote.
    let mut supersedes: Vec<MetaTaskId> = Vec::with_capacity(req.supersedes.len());
    for meta_task_id in &req.supersedes {
        if !supersedes.contains(meta_task_id) {
            supersedes.push(meta_task_id.clone());
        }
    }
    let meta_task_id = crate::model::make_id("meta");
    // Every replacement is checked before the new row exists, so a refused
    // submission leaves the store exactly as it was.
    for replaced_id in &supersedes {
        if replaced_id.as_str() == meta_task_id.as_str() {
            return Err(CoveyTypeValidationError::new(
                "supersedes",
                "must not name the meta task being submitted",
            )
            .into());
        }
        let replaced = load_meta_task_tx(tx, replaced_id.as_str())?;
        if let Some(superseding) = load_meta_task_superseded_by_tx(tx, replaced_id.as_str())? {
            return Err(CoveyError::MetaTaskAlreadySuperseded {
                meta_task_id: replaced.meta_task_id.to_string(),
                superseded_by: superseding.to_string(),
            });
        }
    }
    insert_meta_task_row_tx(
        tx,
        &meta_task_id,
        req.session_token.as_str(),
        req.prompt_text.as_str(),
        req.source_link.as_ref(),
        req.title.as_ref(),
        req.urgency,
        req.due_on.as_ref(),
        now,
    )?;
    if let Some(dedupe_key) = req.dedupe_key.as_ref() {
        tx.execute(
            r#"
            INSERT INTO meta_task_dedupe_keys (
                agent_principal_id, dedupe_key, meta_task_id, created_at
            ) VALUES (?1, ?2, ?3, ?4)
            "#,
            params![
                session.agent_principal_id.as_str(),
                dedupe_key.as_str(),
                meta_task_id,
                now,
            ],
        )?;
    }
    for replaced_id in &supersedes {
        let replaced = load_meta_task_tx(tx, replaced_id.as_str())?;
        if matches!(
            replaced.state(),
            MetaTaskState::Planning | MetaTaskState::Active
        ) {
            // A replaced task that is still open is retired exactly as a cancel
            // retires it, with `superseded` as the stored closure. Its payload
            // reuses the cancel request shape so subscribers read one payload
            // type for every retirement.
            let event_payload = CancelMetaTaskReq {
                session_token: req.session_token.clone(),
                meta_task_id: replaced.meta_task_id.clone(),
                closure: Some(MetaTaskClosure::Superseded),
                idempotency_key: req.idempotency_key.clone(),
            };
            retire_meta_task_tx(
                tx,
                &replaced,
                req.session_token.as_str(),
                MetaTaskClosure::Superseded,
                &event_payload,
                now,
            )?;
        }
        // A replaced task that already finished keeps its state; the link row
        // is written either way.
        tx.execute(
            r#"
            INSERT INTO meta_task_supersedes (
                superseding_meta_task_id, superseded_meta_task_id, superseded_at
            ) VALUES (?1, ?2, ?3)
            "#,
            params![meta_task_id, replaced_id.as_str(), now],
        )?;
    }
    append_session_event(
        tx,
        EventType::MetaTaskSubmitted,
        ObjectType::MetaTask,
        &meta_task_id,
        req.session_token.as_str(),
        req,
        now,
    )?;
    Ok(meta_task_id)
}

/// Writes one meta task row.
///
/// `handle` is never written here: the store's trigger numbers the row, so a
/// row inserted through SQL is numbered exactly like one inserted through the
/// API.
#[allow(clippy::too_many_arguments)]
fn insert_meta_task_row_tx(
    tx: &Transaction<'_>,
    meta_task_id: &str,
    session_token: &str,
    prompt_text: &str,
    source_link: Option<&MetaTaskSourceLinkDraft>,
    title: Option<&MetaTaskTitle>,
    urgency: Option<MetaTaskUrgency>,
    due_on: Option<&DueDate>,
    now: i64,
) -> Result<()> {
    let due_on_text = due_on.map(ToString::to_string);
    tx.execute(
        r#"
        INSERT INTO meta_tasks (
            meta_task_id, prompt_text, state, created_by, created_at, updated_at,
            title, urgency, due_on
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, ?8)
        "#,
        params![
            meta_task_id,
            prompt_text,
            meta_task_state_name(MetaTaskState::Planning),
            session_token,
            now,
            title.map(MetaTaskTitle::as_str),
            urgency.map(meta_task_urgency_name),
            due_on_text.as_deref(),
        ],
    )?;
    if let Some(source_link) = source_link {
        insert_meta_task_source_link_tx(tx, meta_task_id, source_link, session_token, now)?;
    }
    Ok(())
}

/// Writes the one source-backlink row of a meta task.
///
/// The origin and the presence of a method come from the typed draft, so the
/// persisted pair always satisfies the table constraint.
fn insert_meta_task_source_link_tx(
    tx: &Transaction<'_>,
    meta_task_id: &str,
    draft: &MetaTaskSourceLinkDraft,
    linked_by: &str,
    now: i64,
) -> Result<()> {
    tx.execute(
        r#"
        INSERT INTO meta_task_source_links (
            meta_task_id, thread_id, url, received_at, path, origin, method, linked_by, linked_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
        params![
            meta_task_id,
            draft.thread_id(),
            draft.url(),
            draft.received_at(),
            draft.path(),
            crate::model::meta_task_source_link_origin_name(draft.origin()),
            draft.method(),
            linked_by,
            now,
        ],
    )?;
    Ok(())
}

/// Attaches a backfilled source backlink to an existing, unlinked meta task.
pub(crate) fn link_meta_task_source_tx(
    tx: &Transaction<'_>,
    req: &LinkMetaTaskSourceReq,
    now: i64,
) -> Result<()> {
    // The same authority `meta cancel` demands: an active orchestrator session.
    require_role(tx, &req.session_token, &[SessionRole::Orchestrator])?;
    let meta_task = load_meta_task_tx(tx, req.meta_task_id.as_str())?;
    if load_meta_task_source_link_tx(tx, meta_task.meta_task_id.as_str())?.is_some() {
        return Err(CoveyError::SourceLinkExists {
            meta_task_id: meta_task.meta_task_id.to_string(),
        });
    }
    match req.source_link.provenance() {
        MetaTaskSourceLinkProvenance::Backfilled { .. } => {}
        MetaTaskSourceLinkProvenance::Submitted => {
            return Err(CoveyError::TypeValidationError(
                CoveyTypeValidationError::new(
                    "source_link",
                    "a backfilled source link must name its method",
                ),
            ));
        }
    }
    insert_meta_task_source_link_tx(
        tx,
        meta_task.meta_task_id.as_str(),
        &req.source_link,
        req.session_token.as_str(),
        now,
    )?;
    append_session_event(
        tx,
        EventType::MetaTaskSourceLinked,
        ObjectType::MetaTask,
        meta_task.meta_task_id.as_str(),
        &req.session_token,
        req,
        now,
    )?;
    Ok(())
}

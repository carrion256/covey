#![cfg_attr(coverage_nightly, coverage(off))]

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use serde::{Serialize, de::DeserializeOwned};
use tracing::{Level, enabled, info, warn};

use crate::{
    clock::{Clock, SystemClock},
    error::{CoveyError, Result},
    model::{
        Claim, ClaimState, EventType, LeaseDeadlineMs, LeaseDurationMs, MetaTaskState, ObjectType,
        ReadyQueueClaim, ReadyQueueState, ReviewState, SessionState, SubtaskKind, SubtaskState,
        TimestampMs, claim_state_name, meta_task_state_name, ready_queue_state_name,
        review_state_name, session_state_name, subtask_kind_name, subtask_state_name,
    },
    queries::{
        collect_rows, deserialize_row, load_meta_task_tx, load_mutation_idempotency_record_tx,
        load_queue_item_tx, load_subtask_tx,
    },
    schema::{
        EventActor, SYSTEM_EVENT_SESSION_TOKEN, append_event, apply_migrations, apply_pragmas,
    },
    validators::{
        close_claim_and_detach, ensure_meta_task_is_schedulable, ensure_ready_queue_transition,
        validate_idempotency_key,
    },
};

/// SQLite-backed Covey store and transactional API surface.
pub struct Covey {
    pub(crate) db_path: Option<PathBuf>,
    pub(crate) conn: Mutex<Connection>,
    pub(crate) clock: Arc<dyn Clock>,
}

static_assertions::assert_impl_all!(Covey: Send, Sync);

pub(crate) const LIST_CONFLICTS_LIMIT: usize = 1_000;

impl Covey {
    /// Opens or creates a Covey database at the provided path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_clock(path, Arc::new(SystemClock))
    }

    /// Opens or creates a Covey database with an injected clock.
    pub fn open_with_clock(path: impl AsRef<Path>, clock: Arc<dyn Clock>) -> Result<Self> {
        let db_path = path.as_ref().to_path_buf();
        let mut conn = Connection::open(&db_path)?;
        apply_pragmas(&conn)?;
        apply_migrations(&mut conn)?;
        Ok(Self {
            db_path: Some(db_path),
            conn: Mutex::new(conn),
            clock,
        })
    }

    pub(crate) fn with_write_tx<T, F>(&self, f: F) -> Result<T>
    where
        F: FnMut(&Transaction<'_>, i64) -> Result<T>,
    {
        let mut f = f;
        let mut conn = self
            .conn
            .lock()
            .expect("covey writer connection mutex poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = self.clock.wall_now_ms();
        let result = f(&tx, now)?;
        tx.commit()?;
        Ok(result)
    }

    pub(crate) fn with_read_conn<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        if let Some(path) = &self.db_path {
            let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            return f(&conn);
        }

        let conn = self
            .conn
            .lock()
            .expect("covey writer connection mutex poisoned");
        f(&conn)
    }

    pub(crate) fn with_read_tx<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Transaction<'_>) -> Result<T>,
    {
        if let Some(path) = &self.db_path {
            let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
            let result = f(&tx)?;
            tx.commit()?;
            return Ok(result);
        }

        let mut conn = self
            .conn
            .lock()
            .expect("covey writer connection mutex poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }

    pub(crate) fn log_operation<T, F>(
        &self,
        operation: &'static str,
        session_token: &str,
        started_at: Instant,
        result: &Result<T>,
        affected_object_ids: F,
    ) where
        F: FnOnce(&T) -> Vec<String>,
    {
        let duration_ms = started_at.elapsed().as_millis() as u64;
        let actor = redact_session_token(session_token);
        match result {
            Ok(value) => {
                if enabled!(Level::INFO) {
                    info!(
                        operation,
                        actor,
                        duration_ms,
                        result = "ok",
                        affected_object_ids = ?affected_object_ids(value),
                    );
                }
            }
            Err(error) => warn!(
                operation,
                actor,
                duration_ms,
                result = "error",
                error = %error,
                affected_object_ids = ?Vec::<String>::new(),
            ),
        }
    }
}

pub(crate) fn with_idempotent_mutation<T, Req, F>(
    tx: &Transaction<'_>,
    actor_key: &str,
    operation: &str,
    idempotency_key: &str,
    request: &Req,
    created_at: TimestampMs,
    f: F,
) -> Result<T>
where
    T: Serialize + DeserializeOwned,
    Req: Serialize,
    F: FnOnce() -> Result<T>,
{
    validate_idempotency_key(idempotency_key)?;
    let request_hash = hash_request(request)?;
    if let Some(existing) =
        load_mutation_idempotency_record_tx(tx, actor_key, operation, idempotency_key)?
    {
        if existing.request_hash() != request_hash {
            return Err(CoveyError::IdempotencyConflict {
                actor_key: actor_key.to_owned(),
                operation: operation.to_owned(),
                idempotency_key: idempotency_key.to_owned(),
            });
        }
        return serde_json::from_str(existing.response_json()).map_err(Into::into);
    }

    let value = f()?;
    tx.execute(
        r#"
        INSERT INTO mutation_idempotency (
            actor_key, operation, idempotency_key, request_hash, response_json, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        "#,
        params![
            actor_key,
            operation,
            idempotency_key,
            request_hash,
            serde_json::to_string(&value)?,
            created_at
        ],
    )?;
    Ok(value)
}

pub(crate) fn append_session_event<T: Serialize>(
    tx: &Transaction<'_>,
    event_type: EventType,
    object_type: ObjectType,
    object_id: &str,
    session_token: &str,
    payload: &T,
    now: i64,
) -> Result<()> {
    append_event(
        tx,
        event_type,
        object_type,
        object_id,
        EventActor::Session(session_token),
        payload,
        now,
    )
}

pub(crate) fn append_system_event<T: Serialize>(
    tx: &Transaction<'_>,
    event_type: EventType,
    object_type: ObjectType,
    object_id: &str,
    payload: &T,
    now: i64,
) -> Result<()> {
    append_event(
        tx,
        event_type,
        object_type,
        object_id,
        EventActor::System,
        payload,
        now,
    )
}

fn redact_session_token(session_token: &str) -> String {
    if session_token == "system" || session_token == SYSTEM_EVENT_SESSION_TOKEN {
        return "system".to_owned();
    }
    let suffix_start = session_token
        .char_indices()
        .rev()
        .nth(7)
        .map_or(0, |(index, _)| index);
    let suffix = &session_token[suffix_start..];
    let mut redacted = String::with_capacity("session:".len() + suffix.len());
    redacted.push_str("session:");
    redacted.push_str(suffix);
    redacted
}

fn hash_request<T: Serialize>(request: &T) -> Result<String> {
    let mut hasher = blake3::Hasher::new();
    serde_json::to_writer(&mut hasher, request)?;
    Ok(hasher.finalize().to_hex().to_string())
}

pub(crate) fn ordered_claim_candidate(
    tx: &Transaction<'_>,
    kind: SubtaskKind,
    candidate_state: SubtaskState,
    meta_task_id: Option<&str>,
    now: i64,
) -> Result<Option<String>> {
    match kind {
        SubtaskKind::Work => {
            if let Some(meta_task_id) = meta_task_id {
                if !meta_task_is_claimable(tx, meta_task_id)? {
                    return Ok(None);
                }
                let mut stmt = tx.prepare(
                    r#"
                    SELECT s.subtask_id
                    FROM subtasks s
                    WHERE s.kind = ?1
                      AND s.state = ?2
                      AND s.meta_task_id = ?4
                      AND NOT EXISTS (
                          SELECT 1
                          FROM subtask_dependencies d
                          JOIN subtasks dep ON dep.subtask_id = d.depends_on_subtask_id
                          WHERE d.subtask_id = s.subtask_id
                            AND dep.state NOT IN (?5, ?6, ?7, ?8)
                            AND NOT EXISTS (
                                WITH RECURSIVE followup_chain(followup_subtask_id) AS (
                                    SELECT f.followup_subtask_id
                                    FROM review_followup_subtasks f
                                    WHERE f.source_subtask_id = dep.subtask_id
                                    UNION
                                    SELECT f.followup_subtask_id
                                    FROM review_followup_subtasks f
                                    JOIN followup_chain chain
                                      ON f.source_subtask_id = chain.followup_subtask_id
                                )
                                SELECT 1
                                FROM followup_chain chain
                                JOIN subtasks followup
                                  ON followup.subtask_id = chain.followup_subtask_id
                                WHERE followup.state IN (?5, ?6, ?7, ?8)
                            )
                      )
                    ORDER BY
                        MAX(
                            s.priority - MIN(MAX(?3 - s.created_at, 0) / 30000, s.priority),
                            0
                        ) ASC,
                        s.priority ASC,
                        s.created_at ASC
                    LIMIT 1
                    "#,
                )?;
                stmt.query_row(
                    params![
                        subtask_kind_name(kind),
                        subtask_state_name(candidate_state),
                        now,
                        meta_task_id,
                        subtask_state_name(SubtaskState::Approved),
                        subtask_state_name(SubtaskState::ReadyForApply),
                        subtask_state_name(SubtaskState::Applied),
                        subtask_state_name(SubtaskState::Decided)
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(Into::into)
            } else {
                let mut stmt = tx.prepare(
                    r#"
                    SELECT s.subtask_id
                    FROM subtasks s
                    JOIN meta_tasks m ON m.meta_task_id = s.meta_task_id
                    WHERE s.kind = ?1
                      AND s.state = ?2
                      AND m.state NOT IN (?3, ?4)
                      AND NOT EXISTS (
                          SELECT 1
                          FROM subtask_dependencies d
                          JOIN subtasks dep ON dep.subtask_id = d.depends_on_subtask_id
                          WHERE d.subtask_id = s.subtask_id
                            AND dep.state NOT IN (?6, ?7, ?8, ?9)
                            AND NOT EXISTS (
                                WITH RECURSIVE followup_chain(followup_subtask_id) AS (
                                    SELECT f.followup_subtask_id
                                    FROM review_followup_subtasks f
                                    WHERE f.source_subtask_id = dep.subtask_id
                                    UNION
                                    SELECT f.followup_subtask_id
                                    FROM review_followup_subtasks f
                                    JOIN followup_chain chain
                                      ON f.source_subtask_id = chain.followup_subtask_id
                                )
                                SELECT 1
                                FROM followup_chain chain
                                JOIN subtasks followup
                                  ON followup.subtask_id = chain.followup_subtask_id
                                WHERE followup.state IN (?6, ?7, ?8, ?9)
                            )
                      )
                    ORDER BY
                        MAX(
                            s.priority - MIN(MAX(?5 - s.created_at, 0) / 30000, s.priority),
                            0
                        ) ASC,
                        s.priority ASC,
                        s.created_at ASC
                    LIMIT 1
                    "#,
                )?;
                stmt.query_row(
                    params![
                        subtask_kind_name(kind),
                        subtask_state_name(candidate_state),
                        meta_task_state_name(MetaTaskState::Completed),
                        meta_task_state_name(MetaTaskState::Cancelled),
                        now,
                        subtask_state_name(SubtaskState::Approved),
                        subtask_state_name(SubtaskState::ReadyForApply),
                        subtask_state_name(SubtaskState::Applied),
                        subtask_state_name(SubtaskState::Decided)
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(Into::into)
            }
        }
        SubtaskKind::Review => {
            if let Some(meta_task_id) = meta_task_id {
                if !meta_task_is_claimable(tx, meta_task_id)? {
                    return Ok(None);
                }
                let mut stmt = tx.prepare(
                    r#"
                    SELECT s.subtask_id
                    FROM subtasks s
                    WHERE s.kind = ?1
                      AND s.state = ?2
                      AND s.meta_task_id = ?3
                    ORDER BY s.priority ASC, s.created_at ASC
                    LIMIT 1
                    "#,
                )?;
                stmt.query_row(
                    params![
                        subtask_kind_name(kind),
                        subtask_state_name(candidate_state),
                        meta_task_id
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(Into::into)
            } else {
                let mut stmt = tx.prepare(
                    r#"
                    SELECT s.subtask_id
                    FROM subtasks s
                    JOIN meta_tasks m ON m.meta_task_id = s.meta_task_id
                    WHERE s.kind = ?1
                      AND s.state = ?2
                      AND m.state NOT IN (?3, ?4)
                    ORDER BY s.priority ASC, s.created_at ASC
                    LIMIT 1
                    "#,
                )?;
                stmt.query_row(
                    params![
                        subtask_kind_name(kind),
                        subtask_state_name(candidate_state),
                        meta_task_state_name(MetaTaskState::Completed),
                        meta_task_state_name(MetaTaskState::Cancelled)
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(Into::into)
            }
        }
        SubtaskKind::Cleanup => Ok(None),
    }
}

fn meta_task_is_claimable(tx: &Transaction<'_>, meta_task_id: &str) -> Result<bool> {
    let state = tx
        .query_row(
            "SELECT state FROM meta_tasks WHERE meta_task_id = ?1",
            params![meta_task_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(!matches!(
        state.as_deref(),
        None | Some("completed") | Some("cancelled")
    ))
}

pub(crate) fn subtask_dependencies_satisfied(
    tx: &Transaction<'_>,
    subtask_id: &str,
) -> Result<bool> {
    let has_unsatisfied = tx.query_row(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM subtask_dependencies d
            JOIN subtasks dep ON dep.subtask_id = d.depends_on_subtask_id
            WHERE d.subtask_id = ?1
              AND dep.state NOT IN (?2, ?3, ?4, ?5)
              AND NOT EXISTS (
                  WITH RECURSIVE followup_chain(followup_subtask_id) AS (
                      SELECT f.followup_subtask_id
                      FROM review_followup_subtasks f
                      WHERE f.source_subtask_id = dep.subtask_id
                      UNION
                      SELECT f.followup_subtask_id
                      FROM review_followup_subtasks f
                      JOIN followup_chain chain
                        ON f.source_subtask_id = chain.followup_subtask_id
                  )
                  SELECT 1
                  FROM followup_chain chain
                  JOIN subtasks followup
                    ON followup.subtask_id = chain.followup_subtask_id
                  WHERE followup.state IN (?2, ?3, ?4, ?5)
              )
            LIMIT 1
        )
        "#,
        params![
            subtask_id,
            subtask_state_name(SubtaskState::Approved),
            subtask_state_name(SubtaskState::ReadyForApply),
            subtask_state_name(SubtaskState::Applied),
            subtask_state_name(SubtaskState::Decided)
        ],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(has_unsatisfied == 0)
}

pub(crate) fn ordered_ready_queue_candidate(tx: &Transaction<'_>) -> Result<Option<String>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT queue_id
        FROM ready_queue
        WHERE state = ?1
        ORDER BY enqueued_at ASC
        LIMIT 1
        "#,
    )?;
    stmt.query_row(
        params![ready_queue_state_name(ReadyQueueState::Queued)],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn refresh_meta_task_state(
    tx: &Transaction<'_>,
    meta_task_id: &str,
    now: i64,
) -> Result<()> {
    let meta_task = load_meta_task_tx(tx, meta_task_id)?;
    if meta_task.state() == MetaTaskState::Cancelled {
        return Ok(());
    }

    let has_subtasks = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM subtasks WHERE meta_task_id = ?1 LIMIT 1)",
        params![meta_task_id],
        |row| row.get::<_, i64>(0),
    )?;
    let desired_state = if has_subtasks == 0 {
        MetaTaskState::Planning
    } else {
        let has_open_subtasks = tx.query_row(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM subtasks
                WHERE meta_task_id = ?1
                  AND state NOT IN ('applied', 'abandoned', 'decided')
                LIMIT 1
            )
            "#,
            params![meta_task_id],
            |row| row.get::<_, i64>(0),
        )?;
        if has_open_subtasks == 0 {
            MetaTaskState::Completed
        } else {
            MetaTaskState::Active
        }
    };

    if meta_task.state() != desired_state {
        tx.execute(
            "UPDATE meta_tasks SET state = ?2, updated_at = ?3 WHERE meta_task_id = ?1 AND state != ?4",
            params![
                meta_task_id,
                meta_task_state_name(desired_state),
                now,
                meta_task_state_name(MetaTaskState::Cancelled)
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn requeue_stale_ready_queue_claims(
    tx: &Transaction<'_>,
    lease_now: i64,
    now: i64,
) -> Result<()> {
    let mut candidates = Vec::new();
    let mut stmt = tx.prepare(
        r#"
        SELECT queue_id, enqueued_at
        FROM ready_queue
        WHERE state = ?1 AND claim_lease_deadline <= ?2
        "#,
    )?;
    let rows = stmt.query_map(
        params![ready_queue_state_name(ReadyQueueState::InFlight), lease_now],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    )?;
    candidates.extend(collect_rows(rows)?);

    let mut stmt = tx.prepare(
        r#"
        SELECT q.queue_id, q.enqueued_at
        FROM sessions s
        JOIN ready_queue q INDEXED BY idx_ready_queue_inflight_claimant_enqueued
          ON q.claimed_by_session_token = s.session_token
         AND q.state = 'in_flight'
        WHERE s.state IN (?1, ?2)
        "#,
    )?;
    let rows = stmt.query_map(
        params![
            session_state_name(SessionState::Stale),
            session_state_name(SessionState::Exited)
        ],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    )?;
    candidates.extend(collect_rows(rows)?);

    let mut stmt = tx.prepare(
        r#"
        SELECT q.queue_id, q.enqueued_at
        FROM ready_queue q INDEXED BY idx_ready_queue_inflight_claimant_enqueued
        LEFT JOIN sessions s ON s.session_token = q.claimed_by_session_token
        WHERE q.state = 'in_flight'
          AND s.session_token IS NULL
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    candidates.extend(collect_rows(rows)?);

    if candidates.len() > 1 {
        candidates.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        candidates.dedup_by(|left, right| left.0 == right.0);
        candidates.sort_unstable_by(|left, right| {
            left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0))
        });
    }
    for (queue_id, _) in candidates {
        tx.execute(
            "UPDATE ready_queue SET state = ?2, claimed_by_session_token = NULL, claim_lease_deadline = NULL, updated_at = ?3 WHERE queue_id = ?1 AND state = ?4",
            params![
                queue_id,
                ready_queue_state_name(ReadyQueueState::Queued),
                now,
                ready_queue_state_name(ReadyQueueState::InFlight)
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn claim_ready_queue_item(
    tx: &Transaction<'_>,
    queue_id: &str,
    session_token: &str,
    lease_duration_ms: LeaseDurationMs,
    lease_now: i64,
    now: i64,
) -> Result<Option<ReadyQueueClaim>> {
    let item = load_queue_item_tx(tx, queue_id)?;
    ensure_ready_queue_transition(item.state(), ReadyQueueState::InFlight)?;
    let subtask = load_subtask_tx(tx, item.subtask_id())?;
    match ensure_meta_task_is_schedulable(tx, &subtask.meta_task_id) {
        Ok(()) => {}
        Err(CoveyError::MetaTaskUnavailable { .. }) => {
            tx.execute(
                "UPDATE ready_queue SET state = ?2, claimed_by_session_token = NULL, claim_lease_deadline = NULL, updated_at = ?3 WHERE queue_id = ?1 AND state = ?4",
                params![
                    queue_id,
                    ready_queue_state_name(ReadyQueueState::Cancelled),
                    now,
                    ready_queue_state_name(ReadyQueueState::Queued)
                ],
            )?;
            return Ok(None);
        }
        Err(err) => return Err(err),
    }

    if subtask.state() != SubtaskState::ReadyForApply
        || subtask.artifact_digest().map(AsRef::as_ref) != Some(item.artifact_digest())
    {
        tx.execute(
            "UPDATE ready_queue SET state = ?2, claimed_by_session_token = NULL, claim_lease_deadline = NULL, updated_at = ?3 WHERE queue_id = ?1 AND state = ?4",
            params![
                queue_id,
                ready_queue_state_name(ReadyQueueState::Superseded),
                now,
                ready_queue_state_name(ReadyQueueState::Queued)
            ],
        )?;
        return Ok(None);
    }

    let lease_deadline = LeaseDeadlineMs::parse(lease_now + lease_duration_ms.get())?;
    let claim_fence_seq = tx
        .query_row(
            r#"
            UPDATE ready_queue
            SET state = ?2,
                claimed_by_session_token = ?3,
                claim_fence_seq = COALESCE(claim_fence_seq, 0) + 1,
                claim_lease_deadline = ?4,
                updated_at = ?5
            WHERE queue_id = ?1 AND state = ?6
            RETURNING claim_fence_seq
            "#,
            params![
                queue_id,
                ready_queue_state_name(ReadyQueueState::InFlight),
                session_token,
                lease_deadline,
                now,
                ready_queue_state_name(ReadyQueueState::Queued)
            ],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(claim_fence_seq) = claim_fence_seq else {
        return Ok(None);
    };

    Ok(Some(ReadyQueueClaim::new(
        crate::model::QueueId::parse(item.queue_id())?,
        crate::model::ArtifactDigest::parse(item.artifact_digest())?,
        crate::model::SubtaskId::parse(item.subtask_id())?,
        item.settlement_target(),
        crate::model::FenceSeq::parse(claim_fence_seq)?,
        lease_deadline,
    )))
}

pub(crate) fn load_claims_for_meta_task(
    tx: &Transaction<'_>,
    meta_task_id: &str,
) -> Result<Vec<Claim>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT c.claim_id, c.subtask_id, c.owner_session_token, c.fence_seq, c.lease_deadline, c.state, c.created_at, c.updated_at
        FROM subtasks s
        JOIN claims c INDEXED BY idx_claims_one_held_per_subtask
          ON c.subtask_id = s.subtask_id
         AND c.state = 'held'
        WHERE s.meta_task_id = ?1
        ORDER BY c.created_at ASC
        "#,
    )?;
    let rows = stmt.query_map(params![meta_task_id], deserialize_row::<Claim>)?;
    collect_rows(rows)
}

pub(crate) fn expire_claim_if_needed_for_subtask(
    tx: &Transaction<'_>,
    subtask_id: &str,
    lease_now: i64,
    now: i64,
) -> Result<()> {
    let stale_claim = tx
        .query_row(
            r#"
            SELECT c.claim_id, c.subtask_id, c.owner_session_token, c.fence_seq, c.lease_deadline, c.created_at, c.updated_at
            FROM claims c
            JOIN sessions s ON s.session_token = c.owner_session_token
            WHERE c.subtask_id = ?1 AND c.state = ?2
              AND (c.lease_deadline <= ?3 OR s.state <> ?4)
            LIMIT 1
            "#,
            params![
                subtask_id,
                claim_state_name(ClaimState::Held),
                lease_now,
                session_state_name(SessionState::Active)
            ],
            |row| {
                let claim = Claim::try_from_parts(
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    ClaimState::Held,
                    row.get(5)?,
                    row.get(6)?,
                )
                .map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
                    )
                })?;
                Ok(claim)
            },
        )
        .optional()?;

    let Some(claim) = stale_claim else {
        return Ok(());
    };

    close_claim_and_detach(tx, &claim, ClaimState::Expired, now)?;
    reset_in_progress_review_for_expired_claim(tx, &claim.subtask_id, now)?;
    tx.execute(
        "UPDATE subtasks SET current_claim_id = NULL, state = CASE WHEN state IN (?3, ?4) THEN ?5 ELSE state END, updated_at = ?6 WHERE subtask_id = ?1 AND current_claim_id = ?2",
        params![
            claim.subtask_id,
            claim.claim_id,
            subtask_state_name(SubtaskState::Claimed),
            subtask_state_name(SubtaskState::InProgress),
            subtask_state_name(SubtaskState::Available),
            now
        ],
    )?;
    tx.execute(
        "UPDATE sessions SET active_subtask_id = NULL, updated_at = ?3 WHERE session_token = ?1 AND active_subtask_id = ?2",
        params![claim.owner_session_token, claim.subtask_id, now],
    )?;
    Ok(())
}

pub(crate) fn reset_in_progress_review_for_expired_claim(
    tx: &Transaction<'_>,
    review_subtask_id: &str,
    now: i64,
) -> Result<()> {
    tx.execute(
        "UPDATE reviews SET state = ?2, updated_at = ?3 WHERE review_subtask_id = ?1 AND state = ?4",
        params![
            review_subtask_id,
            review_state_name(ReviewState::Requested),
            now,
            review_state_name(ReviewState::InProgress)
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_session_token_to_last_eight_chars() {
        assert_eq!(
            redact_session_token("session_1234567890abcdef"),
            "session:90abcdef"
        );
        assert_eq!(redact_session_token("short"), "session:short");
        assert_eq!(
            redact_session_token("session_αβγδεζηθικ"),
            "session:γδεζηθικ"
        );
        assert_eq!(redact_session_token("system"), "system");
        assert_eq!(redact_session_token(SYSTEM_EVENT_SESSION_TOKEN), "system");
    }

    #[test]
    fn hash_request_matches_buffered_json_blake3() {
        let request = serde_json::json!({
            "session_token": "session_123",
            "metadata": {
                "priority": 3,
                "paths": ["covey/src/store.rs", "covey/src/schema.rs"]
            },
            "idempotency_key": "idem-123"
        });
        let serialized = serde_json::to_vec(&request).expect("serialize request");
        let expected = blake3::hash(&serialized).to_hex().to_string();

        assert_eq!(hash_request(&request).expect("hash request"), expected);
    }
}

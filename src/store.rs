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
        Claim, ClaimState, EventType, MetaTaskState, ObjectType, ReadyQueueClaim, ReadyQueueState,
        SessionState, SubtaskKind, SubtaskState,
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
    created_at: i64,
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
        if existing.request_hash != request_hash {
            return Err(CoveyError::IdempotencyConflict {
                actor_key: actor_key.to_owned(),
                operation: operation.to_owned(),
                idempotency_key: idempotency_key.to_owned(),
            });
        }
        return serde_json::from_str(&existing.response_json).map_err(Into::into);
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
    let suffix_len = session_token.chars().count().min(8);
    let suffix: String = session_token
        .chars()
        .rev()
        .take(suffix_len)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("session:{suffix}")
}

fn hash_request<T: Serialize>(request: &T) -> Result<String> {
    let serialized = serde_json::to_vec(request)?;
    Ok(blake3::hash(&serialized).to_hex().to_string())
}

pub(crate) fn ordered_claim_candidates(
    tx: &Transaction<'_>,
    kind: SubtaskKind,
    candidate_states: &[SubtaskState],
    now: i64,
) -> Result<Vec<String>> {
    match kind {
        SubtaskKind::Work => {
            let mut stmt = tx.prepare(
                r#"
                SELECT s.subtask_id
                FROM subtasks s
                JOIN meta_tasks m ON m.meta_task_id = s.meta_task_id
                WHERE s.kind = ?1
                  AND s.state IN (?2, ?3)
                  AND m.state NOT IN (?4, ?5)
                ORDER BY
                    MAX(
                        s.priority - MIN(MAX(?6 - s.created_at, 0) / 30000, s.priority),
                        0
                    ) ASC,
                    s.priority ASC,
                    s.created_at ASC
                "#,
            )?;
            let rows = stmt.query_map(
                params![
                    kind.to_string(),
                    candidate_states[0].to_string(),
                    candidate_states[1].to_string(),
                    MetaTaskState::Completed.to_string(),
                    MetaTaskState::Cancelled.to_string(),
                    now
                ],
                |row| row.get::<_, String>(0),
            )?;
            collect_rows(rows)
        }
        SubtaskKind::Review => {
            let mut stmt = tx.prepare(
                r#"
                SELECT s.subtask_id
                FROM subtasks s
                JOIN meta_tasks m ON m.meta_task_id = s.meta_task_id
                WHERE s.kind = ?1
                  AND s.state = ?2
                  AND m.state NOT IN (?3, ?4)
                ORDER BY s.priority ASC, s.created_at ASC
                "#,
            )?;
            let rows = stmt.query_map(
                params![
                    kind.to_string(),
                    candidate_states[0].to_string(),
                    MetaTaskState::Completed.to_string(),
                    MetaTaskState::Cancelled.to_string()
                ],
                |row| row.get::<_, String>(0),
            )?;
            collect_rows(rows)
        }
    }
}

pub(crate) fn ordered_ready_queue_candidates(tx: &Transaction<'_>) -> Result<Vec<String>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT queue_id
        FROM ready_queue
        WHERE state = ?1
        ORDER BY enqueued_at ASC
        "#,
    )?;
    let rows = stmt.query_map(params![ReadyQueueState::Queued.to_string()], |row| {
        row.get::<_, String>(0)
    })?;
    collect_rows(rows)
}

pub(crate) fn refresh_meta_task_state(
    tx: &Transaction<'_>,
    meta_task_id: &str,
    now: i64,
) -> Result<()> {
    let meta_task = load_meta_task_tx(tx, meta_task_id)?;
    if meta_task.state == MetaTaskState::Cancelled {
        return Ok(());
    }

    let subtask_count = tx.query_row(
        "SELECT COUNT(*) FROM subtasks WHERE meta_task_id = ?1",
        params![meta_task_id],
        |row| row.get::<_, i64>(0),
    )?;
    let desired_state = if subtask_count == 0 {
        MetaTaskState::Planning
    } else {
        let open_subtask_count = tx.query_row(
            r#"
            SELECT COUNT(*)
            FROM subtasks
            WHERE meta_task_id = ?1
              AND state NOT IN (?2, ?3, ?4)
            "#,
            params![
                meta_task_id,
                SubtaskState::Applied.to_string(),
                SubtaskState::Abandoned.to_string(),
                SubtaskState::Decided.to_string()
            ],
            |row| row.get::<_, i64>(0),
        )?;
        if open_subtask_count == 0 {
            MetaTaskState::Completed
        } else {
            MetaTaskState::Active
        }
    };

    if meta_task.state != desired_state {
        tx.execute(
            "UPDATE meta_tasks SET state = ?2, updated_at = ?3 WHERE meta_task_id = ?1 AND state != ?4",
            params![
                meta_task_id,
                desired_state.to_string(),
                now,
                MetaTaskState::Cancelled.to_string()
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
    let mut stmt = tx.prepare(
        r#"
        SELECT q.queue_id
        FROM ready_queue q
        LEFT JOIN sessions s ON s.session_token = q.claimed_by_session_token
        WHERE q.state = ?1
          AND (
                q.claim_lease_deadline <= ?2
                OR s.state != ?3
                OR s.session_token IS NULL
              )
        ORDER BY q.enqueued_at ASC
        "#,
    )?;
    let rows = stmt.query_map(
        params![
            ReadyQueueState::InFlight.to_string(),
            lease_now,
            SessionState::Active.to_string()
        ],
        |row| row.get::<_, String>(0),
    )?;
    for queue_id in collect_rows(rows)? {
        tx.execute(
            "UPDATE ready_queue SET state = ?2, claimed_by_session_token = NULL, claim_lease_deadline = NULL, updated_at = ?3 WHERE queue_id = ?1 AND state = ?4",
            params![
                queue_id,
                ReadyQueueState::Queued.to_string(),
                now,
                ReadyQueueState::InFlight.to_string()
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn claim_ready_queue_item(
    tx: &Transaction<'_>,
    queue_id: &str,
    session_token: &str,
    lease_duration_ms: i64,
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
                    ReadyQueueState::Cancelled.to_string(),
                    now,
                    ReadyQueueState::Queued.to_string()
                ],
            )?;
            return Ok(None);
        }
        Err(err) => return Err(err),
    }

    if subtask.state != SubtaskState::ReadyForApply
        || subtask.artifact_digest.as_deref() != Some(item.artifact_digest())
    {
        tx.execute(
            "UPDATE ready_queue SET state = ?2, claimed_by_session_token = NULL, claim_lease_deadline = NULL, updated_at = ?3 WHERE queue_id = ?1 AND state = ?4",
            params![
                queue_id,
                ReadyQueueState::Superseded.to_string(),
                now,
                ReadyQueueState::Queued.to_string()
            ],
        )?;
        return Ok(None);
    }

    let lease_deadline = lease_now + lease_duration_ms;
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
                ReadyQueueState::InFlight.to_string(),
                session_token,
                lease_deadline,
                now,
                ReadyQueueState::Queued.to_string()
            ],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(claim_fence_seq) = claim_fence_seq else {
        return Ok(None);
    };

    Ok(Some(ReadyQueueClaim::new(
        item.queue_id().to_owned(),
        item.artifact_digest().to_owned(),
        item.subtask_id().to_owned(),
        item.settlement_target(),
        claim_fence_seq,
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
        FROM claims c
        JOIN subtasks s ON s.subtask_id = c.subtask_id
        WHERE s.meta_task_id = ?1
          AND c.state = ?2
        ORDER BY c.created_at ASC
        "#,
    )?;
    let rows = stmt.query_map(
        params![meta_task_id, ClaimState::Held.to_string()],
        deserialize_row::<Claim>,
    )?;
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
            SELECT c.claim_id, c.subtask_id, c.owner_session_token, c.fence_seq, c.lease_deadline, c.state, c.created_at, c.updated_at,
                   s.state
            FROM claims c
            JOIN sessions s ON s.session_token = c.owner_session_token
            WHERE c.subtask_id = ?1 AND c.state = ?2
            ORDER BY c.created_at ASC
            LIMIT 1
            "#,
            params![subtask_id, ClaimState::Held.to_string()],
            |row| {
                let claim = Claim {
                    claim_id: row.get(0)?,
                    subtask_id: row.get(1)?,
                    owner_session_token: row.get(2)?,
                    fence_seq: row.get(3)?,
                    lease_deadline: row.get(4)?,
                    state: ClaimState::Held,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                };
                let session_state = row.get::<_, String>(8)?;
                Ok((claim, session_state))
            },
        )
        .optional()?;

    let Some((claim, session_state)) = stale_claim else {
        return Ok(());
    };

    if claim.lease_deadline > lease_now && session_state == SessionState::Active.to_string() {
        return Ok(());
    }

    close_claim_and_detach(tx, &claim, ClaimState::Expired, now)?;
    tx.execute(
        "UPDATE subtasks SET current_claim_id = NULL, state = CASE WHEN state IN (?3, ?4) THEN ?5 ELSE state END, updated_at = ?6 WHERE subtask_id = ?1 AND current_claim_id = ?2",
        params![
            claim.subtask_id,
            claim.claim_id,
            SubtaskState::Claimed.to_string(),
            SubtaskState::InProgress.to_string(),
            SubtaskState::Available.to_string(),
            now
        ],
    )?;
    tx.execute(
        "UPDATE sessions SET active_subtask_id = NULL, updated_at = ?3 WHERE session_token = ?1 AND active_subtask_id = ?2",
        params![claim.owner_session_token, claim.subtask_id, now],
    )?;
    Ok(())
}

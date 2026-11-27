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
    config::WorkflowProfile,
    error::{CoveyError, Result},
    model::{
        Claim, ClaimState, EventType, LeaseDeadlineMs, LeaseDurationMs,
        MetaTaskAutoCompletedPayload, MetaTaskClosure, MetaTaskState, ObjectType, ReadyQueueClaim,
        ReadyQueueState, ReviewState, SessionState, SubtaskKind, SubtaskState, TimestampMs,
        claim_state_name, meta_task_closure_name, meta_task_state_name, ready_queue_state_name,
        review_state_name, session_state_name, subtask_kind_name, subtask_state_name,
    },
    queries::{
        collect_rows, deserialize_row, load_meta_task_tx, load_mutation_idempotency_record_tx,
        load_queue_item_tx, load_subtask_tx,
    },
    schema::{
        EventActor, SYSTEM_EVENT_SESSION_TOKEN, advance_lease_clock, append_event,
        apply_migrations, apply_pragmas, schema_version,
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
    pub(crate) workflow: WorkflowProfile,
    pub(crate) read_only: bool,
}

static_assertions::assert_impl_all!(Covey: Send, Sync);

/// Attempts at taking the write lock before a mutation reports
/// [`CoveyError::StoreBusy`]. Each attempt already waits the connection's
/// `busy_timeout`.
const WRITE_LOCK_ATTEMPTS: usize = 3;

/// The lease of a claim whose row names no duration: one written before
/// claims recorded the lease their owner asked for.
const DEFAULT_CLAIM_LEASE_MS: i64 = 30_000;

pub(crate) const LIST_CONFLICTS_LIMIT: usize = 1_000;

impl Covey {
    /// Opens or creates a Covey database at the provided path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_clock(path, Arc::new(SystemClock))
    }

    /// Opens or creates a Covey database using the given workflow profile.
    ///
    /// The profile selects the completion policy, routing lane, and
    /// direct-receipt evidence policy for newly created work. It never
    /// weakens reviewed or canonical-apply governance on existing rows.
    pub fn open_with_workflow(path: impl AsRef<Path>, workflow: WorkflowProfile) -> Result<Self> {
        Self::open_with_clock_and_workflow(path, Arc::new(SystemClock), workflow)
    }

    /// Opens or creates a Covey database with an injected clock.
    pub fn open_with_clock(path: impl AsRef<Path>, clock: Arc<dyn Clock>) -> Result<Self> {
        Self::open_with_clock_and_workflow(path, clock, WorkflowProfile::default())
    }

    /// Opens or creates a Covey database with an injected clock and the given
    /// workflow profile.
    ///
    /// The profile selects the completion policy, routing lane, and
    /// direct-receipt evidence policy for newly created work. It never
    /// weakens reviewed or canonical-apply governance on existing rows.
    pub fn open_with_clock_and_workflow(
        path: impl AsRef<Path>,
        clock: Arc<dyn Clock>,
        workflow: WorkflowProfile,
    ) -> Result<Self> {
        let db_path = path.as_ref().to_path_buf();
        let mut conn = Connection::open(&db_path)?;
        apply_pragmas(&conn)?;
        apply_migrations(&mut conn, &db_path)?;
        Ok(Self {
            db_path: Some(db_path),
            conn: Mutex::new(conn),
            clock,
            workflow,
            read_only: false,
        })
    }

    /// Opens an existing Covey database without creating or migrating it and
    /// refuses every write.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_read_only_with_clock_and_workflow(
            path,
            Arc::new(SystemClock),
            WorkflowProfile::default(),
        )
    }

    /// Opens an existing Covey database read-only using the given workflow
    /// profile.
    pub fn open_read_only_with_workflow(
        path: impl AsRef<Path>,
        workflow: WorkflowProfile,
    ) -> Result<Self> {
        Self::open_read_only_with_clock_and_workflow(path, Arc::new(SystemClock), workflow)
    }

    fn open_read_only_with_clock_and_workflow(
        path: impl AsRef<Path>,
        clock: Arc<dyn Clock>,
        workflow: WorkflowProfile,
    ) -> Result<Self> {
        let db_path = path.as_ref().to_path_buf();
        if !db_path.exists() {
            return Err(CoveyError::StoreNotFound {
                path: db_path.display().to_string(),
            });
        }
        let conn = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        apply_read_pragmas(&conn)?;
        let found = schema_version(&conn)?;
        let expected = crate::schema::expected_schema_version();
        if found != expected {
            return Err(CoveyError::StoreMigrationRequired { found, expected });
        }
        Ok(Self {
            db_path: Some(db_path),
            conn: Mutex::new(conn),
            clock,
            workflow,
            read_only: true,
        })
    }

    /// Returns the workflow profile this store resolves new work with.
    #[must_use]
    pub fn workflow(&self) -> &WorkflowProfile {
        &self.workflow
    }

    /// Runs one mutation in an immediate transaction and commits it.
    ///
    /// `now` is the store's own time: the wall clock ratcheted through the
    /// lease clock inside the transaction. It never runs backwards from one
    /// mutation to the next, whatever the wall clock does, so every timestamp
    /// a mutation writes comes from one non-decreasing source.
    ///
    /// Taking the write lock is retried, because nothing has run before
    /// `BEGIN`. A store that stays locked is [`CoveyError::StoreBusy`].
    pub(crate) fn with_write_tx<T, F>(&self, f: F) -> Result<T>
    where
        F: FnMut(&Transaction<'_>, i64) -> Result<T>,
    {
        if self.read_only {
            return Err(CoveyError::StoreReadOnly);
        }

        let mut f = f;
        let mut conn = self
            .conn
            .lock()
            .expect("covey writer connection mutex poisoned");
        let mut attempts = 0;
        let tx = loop {
            attempts += 1;
            match conn.transaction_with_behavior(TransactionBehavior::Immediate) {
                Ok(tx) => break tx,
                Err(error) => match CoveyError::from(error) {
                    CoveyError::StoreBusy if attempts < WRITE_LOCK_ATTEMPTS => {}
                    error => return Err(error),
                },
            }
        };
        let now = advance_lease_clock(&tx, self.clock.wall_now_ms())?;
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
            apply_read_pragmas(&conn)?;
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
            apply_read_pragmas(&conn)?;
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

fn apply_read_pragmas(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA busy_timeout = 5000;
        PRAGMA mmap_size = 268435456;
        "#,
    )?;
    Ok(())
}

/// Counts a successful mutation by `actor_key` as a liveness signal.
///
/// When `actor_key` is the token of an active session, that session's
/// heartbeat moves to the store's current time and every subtask claim the
/// session holds has its lease extended to at least the lease clock plus the
/// claim's own lease duration. An actor key that names no active session
/// changes nothing. No event is appended.
fn record_session_liveness(tx: &Transaction<'_>, actor_key: &str, now: i64) -> Result<()> {
    let lease_now = advance_lease_clock(tx, now)?;
    let heartbeats = tx.execute(
        "UPDATE sessions SET last_heartbeat_at = ?2, last_heartbeat_tick = ?3, updated_at = ?2
         WHERE session_token = ?1 AND state = ?4",
        params![
            actor_key,
            now,
            lease_now,
            session_state_name(SessionState::Active)
        ],
    )?;
    if heartbeats == 0 {
        return Ok(());
    }
    tx.execute(
        "UPDATE claims
         SET lease_deadline = ?2 + COALESCE(lease_duration_ms, ?3), updated_at = ?4
         WHERE owner_session_token = ?1 AND state = ?5
           AND lease_deadline < ?2 + COALESCE(lease_duration_ms, ?3)",
        params![
            actor_key,
            lease_now,
            DEFAULT_CLAIM_LEASE_MS,
            now,
            claim_state_name(ClaimState::Held)
        ],
    )?;
    Ok(())
}

/// Runs `f` once per `(actor_key, operation, idempotency_key)`.
///
/// A repeated key with the same request returns the stored response without
/// running `f`; with a different request it is refused. A first, successful
/// run also counts as a liveness signal for the acting session (see
/// [`record_session_liveness`]); a replay and a refusal do not.
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
    record_session_liveness(tx, actor_key, created_at.get())?;
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
    routing_key: &str,
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
                      AND s.routing_key = ?4
                      AND s.meta_task_id = ?5
                      AND NOT EXISTS (
                          SELECT 1
                          FROM subtask_dependencies d
                          JOIN subtasks dep ON dep.subtask_id = d.depends_on_subtask_id
                          WHERE d.subtask_id = s.subtask_id
                            AND NOT (
                                (dep.kind = 'review' AND dep.state = 'decided')
                                OR (dep.completion_policy = 'canonical_apply' AND dep.state = 'applied')
                                OR (
                                    dep.completion_policy IN ('direct', 'reviewed')
                                    AND dep.state = 'completed'
                                )
                            )
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
                                WHERE (followup.kind = 'review' AND followup.state = 'decided')
                                   OR (
                                       followup.completion_policy = 'canonical_apply'
                                       AND followup.state = 'applied'
                                   )
                                   OR (
                                       followup.completion_policy IN ('direct', 'reviewed')
                                       AND followup.state = 'completed'
                                   )
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
                        routing_key,
                        meta_task_id,
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
                      AND s.routing_key = ?3
                      AND m.state NOT IN (?4, ?5)
                      AND NOT EXISTS (
                          SELECT 1
                          FROM subtask_dependencies d
                          JOIN subtasks dep ON dep.subtask_id = d.depends_on_subtask_id
                          WHERE d.subtask_id = s.subtask_id
                            AND NOT (
                                (dep.kind = 'review' AND dep.state = 'decided')
                                OR (dep.completion_policy = 'canonical_apply' AND dep.state = 'applied')
                                OR (
                                    dep.completion_policy IN ('direct', 'reviewed')
                                    AND dep.state = 'completed'
                                )
                            )
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
                                WHERE (followup.kind = 'review' AND followup.state = 'decided')
                                   OR (
                                       followup.completion_policy = 'canonical_apply'
                                       AND followup.state = 'applied'
                                   )
                                   OR (
                                       followup.completion_policy IN ('direct', 'reviewed')
                                       AND followup.state = 'completed'
                                   )
                            )
                      )
                    ORDER BY
                        MAX(
                            s.priority - MIN(MAX(?6 - s.created_at, 0) / 30000, s.priority),
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
                        routing_key,
                        meta_task_state_name(MetaTaskState::Completed),
                        meta_task_state_name(MetaTaskState::Cancelled),
                        now,
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
                      AND s.routing_key = ?3
                      AND s.meta_task_id = ?4
                    ORDER BY s.priority ASC, s.created_at ASC
                    LIMIT 1
                    "#,
                )?;
                stmt.query_row(
                    params![
                        subtask_kind_name(kind),
                        subtask_state_name(candidate_state),
                        routing_key,
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
                      AND s.routing_key = ?3
                      AND m.state NOT IN (?4, ?5)
                    ORDER BY s.priority ASC, s.created_at ASC
                    LIMIT 1
                    "#,
                )?;
                stmt.query_row(
                    params![
                        subtask_kind_name(kind),
                        subtask_state_name(candidate_state),
                        routing_key,
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

/// SQL fragment over the alias `dep`: the claim-gate verdict for one
/// dependency edge. A dependency counts once it has reached the terminal state
/// its own completion policy requires, or once one of its repair follow-ups
/// (directly or through a follow-up chain) reached its own.
///
/// This is the single statement of the dependency satisfaction law. Both the
/// claim gate below and the `subtask status` dependency view are built from
/// it, so the two can never disagree.
pub(crate) const DEPENDENCY_SATISFIED_SQL: &str = r#"
    (
        (dep.kind = 'review' AND dep.state = 'decided')
        OR (dep.completion_policy = 'canonical_apply' AND dep.state = 'applied')
        OR (
            dep.completion_policy IN ('direct', 'reviewed')
            AND dep.state = 'completed'
        )
    )
    OR EXISTS (
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
        WHERE (followup.kind = 'review' AND followup.state = 'decided')
           OR (
               followup.completion_policy = 'canonical_apply'
               AND followup.state = 'applied'
           )
           OR (
               followup.completion_policy IN ('direct', 'reviewed')
               AND followup.state = 'completed'
           )
    )
"#;

pub(crate) fn subtask_dependencies_satisfied(
    tx: &Transaction<'_>,
    subtask_id: &str,
) -> Result<bool> {
    let sql = format!(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM subtask_dependencies d
            JOIN subtasks dep ON dep.subtask_id = d.depends_on_subtask_id
            WHERE d.subtask_id = ?1
              AND NOT ({})
            LIMIT 1
        )
        "#,
        DEPENDENCY_SATISFIED_SQL
    );
    let has_unsatisfied = tx.query_row(&sql, params![subtask_id], |row| row.get::<_, i64>(0))?;
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
        // `meta complete` closes a task that has no subtasks. A refresh must
        // not send that explicit completion back to planning.
        if meta_task.state() == MetaTaskState::Completed {
            MetaTaskState::Completed
        } else {
            MetaTaskState::Planning
        }
    } else {
        let has_open_subtasks = tx.query_row(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM subtasks
                WHERE meta_task_id = ?1
                  AND state NOT IN ('applied', 'completed', 'failed', 'abandoned', 'decided')
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
        // A task that completes here was closed by its last subtask settling,
        // not by `meta complete`: the closure is decided from its work, and the
        // change gets its own event. Any other move leaves a terminal state or
        // starts work, and carries no closure.
        let closure = if desired_state == MetaTaskState::Completed {
            Some(auto_completion_closure(tx, meta_task_id)?)
        } else {
            None
        };
        let updated = tx.execute(
            r#"
            UPDATE meta_tasks
            SET state = ?2, closure = ?5, updated_at = ?3
            WHERE meta_task_id = ?1 AND state != ?4
            "#,
            params![
                meta_task_id,
                meta_task_state_name(desired_state),
                now,
                meta_task_state_name(MetaTaskState::Cancelled),
                closure.map(meta_task_closure_name),
            ],
        )?;
        if updated == 1
            && let Some(closure) = closure
        {
            append_system_event(
                tx,
                EventType::MetaTaskAutoCompleted,
                ObjectType::MetaTask,
                meta_task_id,
                &MetaTaskAutoCompletedPayload::new(meta_task.meta_task_id.clone(), closure),
                now,
            )?;
        }
    }
    Ok(())
}

/// The closure of a meta task that its last settled subtask completed: `done`
/// when at least one work subtask finished (`completed` or `applied`),
/// `dropped` when all of its work failed or was abandoned.
fn auto_completion_closure(tx: &Transaction<'_>, meta_task_id: &str) -> Result<MetaTaskClosure> {
    let finished_work: bool = tx.query_row(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM subtasks
            WHERE meta_task_id = ?1 AND kind = ?2 AND state IN (?3, ?4)
        )
        "#,
        params![
            meta_task_id,
            subtask_kind_name(SubtaskKind::Work),
            subtask_state_name(SubtaskState::Completed),
            subtask_state_name(SubtaskState::Applied),
        ],
        |row| row.get(0),
    )?;
    Ok(if finished_work {
        MetaTaskClosure::Done
    } else {
        MetaTaskClosure::Dropped
    })
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
    use std::{
        fs,
        sync::{
            Barrier,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
    };

    use rusqlite_migration::{M, Migrations};
    use tempfile::TempDir;

    use crate::{RegisterSessionReq, SessionRole, SubmitMetaTaskReq};

    use super::*;

    /// Migration sources in application order, mirroring the build script.
    fn migration_sql() -> Vec<String> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/migrations");
        let mut files = fs::read_dir(&dir)
            .expect("read migrations directory")
            .map(|entry| entry.expect("migration entry").path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("sql"))
            .collect::<Vec<_>>();
        files.sort();
        files
            .iter()
            .map(|path| fs::read_to_string(path).expect("read migration source"))
            .collect()
    }

    /// Writes a store at the schema version before the newest migration and
    /// returns the version it records together with the expected version.
    fn earlier_store(path: &Path) -> (usize, usize) {
        let expected = crate::schema::expected_schema_version();
        let applied = expected.saturating_sub(1);
        let sources = migration_sql();
        assert_eq!(
            sources.len(),
            expected,
            "the migration directory must hold one source per expected version"
        );
        let mut conn = Connection::open(path).expect("create the earlier store file");
        let migrations = sources
            .iter()
            .take(applied)
            .map(|sql| M::up(sql.as_str()))
            .collect::<Vec<_>>();
        Migrations::new(migrations)
            .to_latest(&mut conn)
            .expect("apply the earlier migrations");
        (applied, expected)
    }

    #[test]
    fn open_read_only_reports_a_missing_store() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("absent.db");
        let error = Covey::open_read_only(&path)
            .err()
            .expect("a missing store must be refused");
        match error {
            CoveyError::StoreNotFound { path: reported } => {
                assert_eq!(reported, path.display().to_string());
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert!(
            !path.exists(),
            "a read-only open must not create the store file"
        );
    }

    #[test]
    fn open_read_only_refuses_a_store_behind_the_expected_schema() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("earlier.db");
        let (applied, expected) = earlier_store(&path);
        let error = Covey::open_read_only(&path)
            .err()
            .expect("an earlier schema must be refused");
        match error {
            CoveyError::StoreMigrationRequired {
                found,
                expected: wanted,
            } => {
                assert_eq!(found, applied);
                assert_eq!(wanted, expected);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn two_openers_racing_on_an_earlier_store_both_succeed() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("earlier.db");
        let (_applied, expected) = earlier_store(&path);

        // Both openers leave the barrier together. Every wait after that is
        // bounded by the store itself: 15 s for the migration lock and the 5 s
        // busy timeout per statement. An opener that reaches a bound fails by
        // name below instead of hanging.
        let barrier = Arc::new(Barrier::new(2));
        let openers = [1, 2].map(|opener| {
            let barrier = Arc::clone(&barrier);
            let path = path.clone();
            thread::spawn(move || {
                barrier.wait();
                Covey::open(&path)
                    .map(|_store| ())
                    .map_err(|error| format!("opener {opener} failed: {error:?}"))
            })
        });
        for opener in openers {
            opener
                .join()
                .expect("an opener thread panicked")
                .expect("both racing openers must open the store");
        }

        let conn = Connection::open(&path).expect("reopen the store");
        assert_eq!(
            schema_version(&conn).expect("read the schema version"),
            expected
        );
    }

    #[test]
    fn a_foreign_key_violation_rolls_the_migration_back() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("earlier.db");
        let (applied, _expected) = earlier_store(&path);
        {
            let conn = Connection::open(&path).expect("open the earlier store");
            conn.pragma_update(None, "foreign_keys", "OFF")
                .expect("stop enforcing foreign keys for the planted row");
            conn.execute(
                r#"
                INSERT INTO meta_task_dedupe_keys (
                    agent_principal_id, dedupe_key, meta_task_id, created_at
                ) VALUES ('principal', 'key', 'no-such-meta-task', 1)
                "#,
                [],
            )
            .expect("plant a dangling reference");
        }

        let error = Covey::open(&path)
            .err()
            .expect("a dangling reference must stop the migration");
        assert!(
            matches!(error, CoveyError::MigrationError(_)),
            "unexpected error: {error:?}"
        );

        let conn = Connection::open(&path).expect("reopen the store");
        assert_eq!(
            schema_version(&conn).expect("read the schema version"),
            applied,
            "the refused migration must leave the store at its earlier version"
        );
    }

    static REFUSED_WAITS: AtomicUsize = AtomicUsize::new(0);

    /// Busy handler that never waits and counts how often SQLite asked.
    fn refuse_to_wait(_retries_so_far: i32) -> bool {
        REFUSED_WAITS.fetch_add(1, Ordering::SeqCst);
        false
    }

    #[test]
    fn a_store_that_stays_locked_is_store_busy_after_every_attempt() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("covey.db");
        let covey = Covey::open(&path).expect("create the store");
        // SQLite asks the busy handler once per refused `BEGIN IMMEDIATE`, so
        // the handler counts the attempts without sitting through a timeout.
        covey
            .conn
            .lock()
            .expect("writer connection")
            .busy_handler(Some(refuse_to_wait))
            .expect("install the busy handler");

        let blocker = Connection::open(&path).expect("open the blocking connection");
        blocker
            .execute_batch("BEGIN IMMEDIATE")
            .expect("hold the write lock");

        let refused = covey.with_write_tx(|_tx, _now| Ok(()));
        assert!(
            matches!(refused, Err(CoveyError::StoreBusy)),
            "unexpected result: {refused:?}"
        );
        assert_eq!(REFUSED_WAITS.load(Ordering::SeqCst), WRITE_LOCK_ATTEMPTS);

        blocker
            .execute_batch("COMMIT")
            .expect("release the write lock");
        covey
            .with_write_tx(|_tx, _now| Ok(()))
            .expect("the freed store takes the write");
    }

    #[test]
    fn read_only_store_lists_meta_tasks_and_refuses_writes() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("covey.db");
        let session_token = {
            let writable = Covey::open(&path).expect("create the store");
            let session = writable
                .register_session(
                    RegisterSessionReq::try_from_raw_parts(
                        "read-only-principal",
                        "read-only-instance",
                        SessionRole::Orchestrator,
                        "register-read-only",
                    )
                    .expect("valid registration request"),
                )
                .expect("register session");
            writable
                .submit_meta_task(
                    SubmitMetaTaskReq::try_from_raw_parts(
                        session.session_token.clone(),
                        "read-only listing",
                        "submit-read-only",
                    )
                    .expect("valid submit request"),
                )
                .expect("submit meta task");
            session.session_token
        };

        let store = Covey::open_read_only(&path).expect("open the store read-only");
        let summaries = store.list_meta_tasks(10, 0).expect("list meta tasks");
        assert_eq!(summaries.len(), 1, "the stored meta task must be readable");

        let error = store
            .submit_meta_task(
                SubmitMetaTaskReq::try_from_raw_parts(
                    session_token,
                    "a read-only store must refuse this",
                    "submit-read-only-second",
                )
                .expect("valid submit request"),
            )
            .expect_err("a read-only store must refuse writes");
        assert!(
            matches!(error, CoveyError::StoreReadOnly),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn refresh_keeps_a_completed_task_without_subtasks_completed() {
        let covey = crate::Covey::open_in_memory_with_clock(std::sync::Arc::new(
            crate::ManualClock::new(1_700_000_000_000),
        ))
        .expect("open in-memory covey");
        let orchestrator = covey
            .register_session(
                RegisterSessionReq::try_from_raw_parts(
                    "refresh-orchestrator",
                    "refresh-orchestrator-1",
                    SessionRole::Orchestrator,
                    "register-refresh",
                )
                .expect("valid registration request"),
            )
            .expect("register session");
        let meta_task_id = covey
            .submit_meta_task(
                SubmitMetaTaskReq::try_from_raw_parts(
                    orchestrator.session_token.clone(),
                    "closed without subtasks",
                    "submit-refresh",
                )
                .expect("valid submit request"),
            )
            .expect("submit meta task");
        covey
            .complete_meta_task(
                crate::CompleteMetaTaskReq::try_from_raw_parts(
                    orchestrator.session_token.clone(),
                    meta_task_id.clone(),
                    "complete-refresh",
                )
                .expect("valid complete request"),
            )
            .expect("complete the childless task");

        covey
            .with_write_tx(|tx, now| super::refresh_meta_task_state(tx, &meta_task_id, now))
            .expect("refresh the completed task");

        let meta_task = covey
            .meta_task_status(&meta_task_id)
            .expect("meta task status")
            .meta_task()
            .clone();
        assert_eq!(meta_task.state(), crate::MetaTaskState::Completed);
        assert_eq!(meta_task.closure, Some(crate::MetaTaskClosure::Done));
    }

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

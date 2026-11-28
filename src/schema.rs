use std::{
    fs::{File, OpenOptions, TryLockError},
    path::Path,
    thread,
    time::{Duration, Instant},
};

use rusqlite::{Connection, Transaction, params};
use rusqlite_migration::{M, Migrations};
use serde::Serialize;

use crate::{
    error::{CoveyError, Result},
    model::{ActorKind, Event, EventType, ObjectType, SessionToken, TimestampMs, object_type_name},
};

pub(crate) const SYSTEM_EVENT_SESSION_TOKEN: &str = "__covey_system__";

pub(crate) enum EventActor<'a> {
    Session(&'a str),
    System,
}

const fn actor_kind_name(kind: ActorKind) -> &'static str {
    match kind {
        ActorKind::Session => "session",
        ActorKind::System => "system",
    }
}

const fn event_type_name(event_type: EventType) -> &'static str {
    match event_type {
        EventType::SessionRegistered => "session_registered",
        EventType::SessionHeartbeat => "session_heartbeat",
        EventType::SessionExited => "session_exited",
        EventType::RuntimeAttestationRecorded => "runtime_attestation_recorded",
        EventType::MetaTaskSubmitted => "meta_task_submitted",
        EventType::MetaTaskCancelled => "meta_task_cancelled",
        EventType::SubtaskCreated => "subtask_created",
        EventType::WorkSubtaskCreated => "work_subtask_created",
        EventType::SubtaskClaimed => "subtask_claimed",
        EventType::SubtaskStarted => "subtask_started",
        EventType::SubtaskFinished => "subtask_finished",
        EventType::SubtaskRetried => "subtask_retried",
        EventType::SubtaskFailed => "subtask_failed",
        EventType::SubtaskAbandoned => "subtask_abandoned",
        EventType::SubtaskRetired => "subtask_retired",
        EventType::ClaimReleased => "claim_released",
        EventType::ClaimRenewed => "claim_renewed",
        EventType::ArtifactPublished => "artifact_published",
        EventType::ReviewRequested => "review_requested",
        EventType::ReviewDecided => "review_decided",
        EventType::PermissiveLandingRecorded => "permissive_landing_recorded",
        EventType::ReadyQueueEnqueued => "ready_queue_enqueued",
        EventType::ReadyQueueInFlight => "ready_queue_in_flight",
        EventType::ApplyVerificationRecorded => "apply_verification_recorded",
        EventType::ApplyGateBlockerRecorded => "apply_gate_blocker_recorded",
        EventType::SettlementReconcileBlockerRecorded => "settlement_reconcile_blocker_recorded",
        EventType::ReadyQueueApplied => "ready_queue_applied",
        EventType::OpenSpecArchiveStatusRecorded => "open_spec_archive_status_recorded",
        EventType::ReadyQueueSuperseded => "ready_queue_superseded",
        EventType::ReservationRequested => "reservation_requested",
        EventType::ReservationReleased => "reservation_released",
        EventType::ReservationRenewed => "reservation_renewed",
        EventType::ConflictResolved => "conflict_resolved",
        EventType::SessionsReaped => "sessions_reaped",
        EventType::ClaimsExpired => "claims_expired",
        EventType::ReservationsExpired => "reservations_expired",
        EventType::OpenSpecImported => "open_spec_imported",
        EventType::OperatorBlockerRecorded => "operator_blocker_recorded",
        EventType::OperatorBlockerResolved => "operator_blocker_resolved",
        EventType::ApplyWorktreeRecorded => "apply_worktree_recorded",
        EventType::ApplyWorktreeStateRecorded => "apply_worktree_state_recorded",
        EventType::VcsWorkspaceRecorded => "vcs_workspace_recorded",
        EventType::VcsWorkspaceObserved => "vcs_workspace_observed",
        EventType::VcsPacketStackEntryRecorded => "vcs_packet_stack_entry_recorded",
        EventType::VcsPrPublicationRecorded => "vcs_pr_publication_recorded",
        EventType::ProseApplyBlockerRecorded => "prose_apply_blocker_recorded",
        EventType::MetaTaskSourceLinked => "meta_task_source_linked",
        EventType::MetaTaskCompleted => "meta_task_completed",
        EventType::MetaTaskAnnotated => "meta_task_annotated",
        EventType::MetaTaskUpdated => "meta_task_updated",
        EventType::MetaTaskAutoCompleted => "meta_task_auto_completed",
        EventType::StorePruned => "store_pruned",
    }
}

impl<'a> From<&'a str> for EventActor<'a> {
    fn from(value: &'a str) -> Self {
        Self::Session(value)
    }
}

/// How long a call waits for a lock another process holds, whether the
/// store's write lock or the migration lock, before it answers
/// [`CoveyError::StoreBusy`]. Callers put a deadline of their own on each
/// call, so a longer wait would replace this definite, retryable answer with
/// their timeout.
pub(crate) const LOCK_WAIT: Duration = Duration::from_secs(5);

/// The busy timeout comes first so that every later statement, including the
/// switch to WAL on a store another process is opening, waits for a lock
/// instead of failing at once.
pub(crate) fn apply_pragmas(conn: &Connection) -> Result<()> {
    conn.busy_timeout(LOCK_WAIT)?;
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = FULL;
        PRAGMA wal_autocheckpoint = 1000;
        PRAGMA mmap_size = 268435456;
        "#,
    )?;
    Ok(())
}

mod generated_migrations {
    include!(concat!(env!("OUT_DIR"), "/covey_migrations.rs"));
}

pub(crate) fn expected_schema_version() -> usize {
    generated_migrations::MIGRATIONS.len()
}

/// The schema version the store records: SQLite's `user_version`.
pub(crate) fn schema_version(conn: &Connection) -> Result<usize> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    Ok(usize::try_from(version).unwrap_or(0))
}

const MIGRATION_LOCK_POLL: Duration = Duration::from_millis(50);

/// Brings the store to this binary's schema version.
///
/// A store already at that version costs one `user_version` read. Otherwise
/// the migrations run under an exclusive lock on the sidecar file
/// `<db path>.migrate.lock`. The runner reads the version again under the
/// lock, so an opener that waited for another process's migration finds
/// nothing left to apply instead of applying it a second time.
pub(crate) fn apply_migrations(conn: &mut Connection, db_path: &Path) -> Result<()> {
    if schema_version(conn)? == expected_schema_version() {
        return Ok(());
    }
    let _lock = lock_for_migration(db_path, LOCK_WAIT)?;
    run_migrations(conn)
}

/// Takes the lock that serialises migration across processes. It is held for
/// as long as the returned handle lives, and the operating system releases it
/// with the handle, so a migrator that crashed cannot leave it held. An
/// in-memory store is private to its connection and takes no lock.
///
/// Waiting longer than `wait` for another holder is [`CoveyError::StoreBusy`].
fn lock_for_migration(db_path: &Path, wait: Duration) -> Result<Option<File>> {
    if db_path == Path::new(":memory:") {
        return Ok(None);
    }
    let mut lock_path = db_path.as_os_str().to_owned();
    lock_path.push(".migrate.lock");
    let lock_error = |error: std::io::Error| {
        CoveyError::from(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
            Some(format!(
                "cannot lock {} for migration: {error}",
                Path::new(&lock_path).display()
            )),
        ))
    };
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(lock_error)?;
    let deadline = Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Some(file)),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                thread::sleep(MIGRATION_LOCK_POLL);
            }
            Err(TryLockError::WouldBlock) => return Err(CoveyError::StoreBusy),
            Err(TryLockError::Error(error)) => return Err(lock_error(error)),
        }
    }
}

/// Applies the pending migrations in one transaction. The final migration
/// checks every foreign key before that transaction commits, so a violation
/// rolls the whole migration back.
fn run_migrations(conn: &mut Connection) -> Result<()> {
    let last = generated_migrations::MIGRATIONS.len() - 1;
    let migrations = generated_migrations::MIGRATIONS
        .iter()
        .copied()
        .enumerate()
        .map(|(index, sql)| {
            let migration = M::up(sql);
            if index == last {
                migration.foreign_key_check()
            } else {
                migration
            }
        })
        .collect();
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let migration_result = Migrations::new(migrations)
        .to_latest(conn)
        .map_err(CoveyError::from);
    let restore_result = conn
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(CoveyError::from);
    migration_result?;
    restore_result
}

pub(crate) fn advance_lease_clock(tx: &Transaction<'_>, wall_now_ms: i64) -> Result<i64> {
    tx.query_row(
        r#"
        INSERT INTO lease_clock (clock_id, last_tick_ms)
        VALUES (1, ?1)
        ON CONFLICT(clock_id) DO UPDATE
        SET last_tick_ms = MAX(lease_clock.last_tick_ms, excluded.last_tick_ms)
        RETURNING last_tick_ms
        "#,
        params![wall_now_ms.max(0)],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

pub(crate) fn append_event<'a, T: Serialize>(
    tx: &Transaction<'_>,
    event_type: EventType,
    object_type: ObjectType,
    object_id: &str,
    actor: impl Into<EventActor<'a>>,
    payload: &T,
    now: i64,
) -> Result<()> {
    let payload_json = serde_json::to_string(payload)?;
    let created_at = TimestampMs::parse(now)?;
    let (actor_kind, session_token) = match actor.into() {
        EventActor::Session(session_token) => {
            let parsed_session_token = SessionToken::parse(session_token)?;
            Event::session(
                1,
                event_type,
                object_type,
                object_id.to_owned(),
                parsed_session_token,
                payload_json.clone(),
                created_at,
            )
            .map_err(|reason| CoveyError::InvalidEventShape { reason })?;
            (ActorKind::Session, session_token)
        }
        EventActor::System => {
            Event::system(
                1,
                event_type,
                object_type,
                object_id.to_owned(),
                payload_json.clone(),
                created_at,
            )
            .map_err(|reason| CoveyError::InvalidEventShape { reason })?;
            (ActorKind::System, SYSTEM_EVENT_SESSION_TOKEN)
        }
    };
    tx.execute(
        r#"
        INSERT INTO event_log (event_type, object_type, object_id, actor_kind, session_token, payload_json, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            event_type_name(event_type),
            object_type_name(object_type),
            object_id,
            actor_kind_name(actor_kind),
            session_token,
            payload_json,
            now
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn migrated_connection() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory database");
        apply_pragmas(&conn).expect("apply pragmas");
        apply_migrations(&mut conn, Path::new(":memory:")).expect("apply migrations");
        conn
    }

    #[test]
    fn append_event_rejects_payload_type_mismatch_without_writing() {
        let mut conn = migrated_connection();
        let tx = conn.transaction().expect("open write transaction");

        let err = append_event(
            &tx,
            EventType::SessionHeartbeat,
            ObjectType::Session,
            "session-1",
            "session-1",
            &json!({"stale_sessions": 1}),
            123,
        )
        .expect_err("mismatched event payload must be rejected before insert");

        assert!(
            matches!(err, CoveyError::InvalidEventShape { .. }),
            "unexpected error: {err}"
        );
        let event_count: i64 = tx
            .query_row("SELECT COUNT(*) FROM event_log", [], |row| row.get(0))
            .expect("count event rows");
        assert_eq!(event_count, 0);
    }

    #[test]
    fn append_event_rejects_object_type_mismatch_without_writing() {
        let mut conn = migrated_connection();
        let tx = conn.transaction().expect("open write transaction");
        let payload = json!({
            "session_token": "session-1",
            "idempotency_key": "idem-1"
        });

        let err = append_event(
            &tx,
            EventType::SessionHeartbeat,
            ObjectType::Claim,
            "session-1",
            "session-1",
            &payload,
            123,
        )
        .expect_err("mismatched event object type must be rejected before insert");

        assert!(
            matches!(err, CoveyError::InvalidEventShape { .. }),
            "unexpected error: {err}"
        );
        let event_count: i64 = tx
            .query_row("SELECT COUNT(*) FROM event_log", [], |row| row.get(0))
            .expect("count event rows");
        assert_eq!(event_count, 0);
    }

    #[test]
    fn waiting_for_the_migration_lock_is_bounded_and_reports_store_busy() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let db_path = dir.path().join("covey.db");
        let holder = lock_for_migration(&db_path, Duration::ZERO)
            .expect("the first opener takes the lock")
            .expect("a file-backed store has a lock file");

        let wait = Duration::from_millis(120);
        let started = Instant::now();
        let refused = lock_for_migration(&db_path, wait);
        let waited = started.elapsed();

        assert!(
            matches!(refused, Err(CoveyError::StoreBusy)),
            "unexpected result: {refused:?}"
        );
        assert!(
            waited >= wait,
            "the wait must last its whole bound, waited {waited:?}"
        );
        assert!(
            waited < Duration::from_secs(5),
            "the wait must end at its bound, waited {waited:?}"
        );

        drop(holder);
        lock_for_migration(&db_path, Duration::ZERO)
            .expect("the lock is free once its holder is gone");
    }
}

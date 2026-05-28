use rusqlite::{Connection, OptionalExtension, Transaction, params};
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
        EventType::SubtaskClaimed => "subtask_claimed",
        EventType::SubtaskStarted => "subtask_started",
        EventType::SubtaskAbandoned => "subtask_abandoned",
        EventType::ClaimReleased => "claim_released",
        EventType::ClaimRenewed => "claim_renewed",
        EventType::ArtifactPublished => "artifact_published",
        EventType::ReviewRequested => "review_requested",
        EventType::ReviewDecided => "review_decided",
        EventType::ReadyQueueEnqueued => "ready_queue_enqueued",
        EventType::ReadyQueueInFlight => "ready_queue_in_flight",
        EventType::ApplyVerificationRecorded => "apply_verification_recorded",
        EventType::ReadyQueueApplied => "ready_queue_applied",
        EventType::ReadyQueueSuperseded => "ready_queue_superseded",
        EventType::ReservationRequested => "reservation_requested",
        EventType::ReservationReleased => "reservation_released",
        EventType::ReservationRenewed => "reservation_renewed",
        EventType::ConflictResolved => "conflict_resolved",
        EventType::SessionsReaped => "sessions_reaped",
        EventType::ClaimsExpired => "claims_expired",
        EventType::ReservationsExpired => "reservations_expired",
        EventType::OpenSpecImported => "open_spec_imported",
    }
}

impl<'a> From<&'a str> for EventActor<'a> {
    fn from(value: &'a str) -> Self {
        Self::Session(value)
    }
}

pub(crate) fn apply_pragmas(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = FULL;
        PRAGMA busy_timeout = 5000;
        PRAGMA wal_autocheckpoint = 1000;
        PRAGMA mmap_size = 268435456;
        "#,
    )?;
    Ok(())
}

mod generated_migrations {
    include!(concat!(env!("OUT_DIR"), "/covey_migrations.rs"));
}

pub(crate) fn apply_migrations(conn: &mut Connection) -> Result<()> {
    let migrations = generated_migrations::MIGRATIONS
        .iter()
        .copied()
        .map(M::up)
        .collect();
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let migration_result = Migrations::new(migrations)
        .to_latest(conn)
        .map_err(CoveyError::from);
    let restore_result = conn
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(CoveyError::from);
    migration_result?;
    restore_result?;
    ensure_foreign_key_integrity(conn)
}

fn ensure_foreign_key_integrity(conn: &Connection) -> Result<()> {
    let violation = conn
        .query_row(
            "SELECT \"table\", rowid, parent, fkid FROM pragma_foreign_key_check LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?;
    if let Some((table, rowid, parent, fkid)) = violation {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY),
            Some(format!(
                "foreign key violation after migrations: table {table} rowid {rowid} references missing parent {parent} via foreign key {fkid}"
            )),
        )
        .into());
    }
    Ok(())
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
        apply_migrations(&mut conn).expect("apply migrations");
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
}

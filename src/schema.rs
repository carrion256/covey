use rusqlite::{Connection, Transaction, params};
use rusqlite_migration::{M, Migrations};
use serde::Serialize;

use crate::{
    error::{CoveyError, Result},
    model::{ActorKind, EventType, ObjectType},
};

pub(crate) const SYSTEM_EVENT_SESSION_TOKEN: &str = "__covey_system__";

pub(crate) enum EventActor<'a> {
    Session(&'a str),
    System,
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
    Migrations::new(migrations)
        .to_latest(conn)
        .map_err(CoveyError::from)
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
    let (actor_kind, session_token) = match actor.into() {
        EventActor::Session(session_token) => (ActorKind::Session, session_token),
        EventActor::System => (ActorKind::System, SYSTEM_EVENT_SESSION_TOKEN),
    };
    tx.execute(
        r#"
        INSERT INTO event_log (event_type, object_type, object_id, actor_kind, session_token, payload_json, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            event_type.to_string(),
            object_type.to_string(),
            object_id,
            actor_kind.to_string(),
            session_token,
            serde_json::to_string(payload)?,
            now
        ],
    )?;
    Ok(())
}

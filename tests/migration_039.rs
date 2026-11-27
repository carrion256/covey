//! Upgrading a pre-039 database keeps every dedupe row and lets one key name
//! more than one task.
//!
//! Migration 039 rebuilds `meta_task_dedupe_keys` with the task id in its
//! primary key. Rows written before the migration survive it unchanged, and
//! the constraints the table had still hold for rows written straight to SQL.

use std::{fs, path::PathBuf};

use covey::{Covey, ManualClock};
use rstest::rstest;
use rusqlite::{Connection, params};
use rusqlite_migration::{M, Migrations};
use tempfile::TempDir;

const PRE_HISTORY_VERSION: usize = 38;
const HISTORY_VERSION: usize = 39;
const NOW_MS: i64 = 1_700_000_000_000;

struct TestDb {
    _dir: TempDir,
    path: PathBuf,
    connection: Connection,
}

/// Every migration source, in application order.
fn migration_sources() -> Vec<String> {
    let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/migrations");
    let mut paths = fs::read_dir(migrations_dir)
        .expect("read migrations")
        .map(|entry| entry.expect("migration entry").path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("sql"))
        .collect::<Vec<_>>();
    assert!(
        paths.len() >= HISTORY_VERSION,
        "expected at least {HISTORY_VERSION} migration files, found {}",
        paths.len()
    );
    paths.sort();
    paths
        .into_iter()
        .map(|path| fs::read_to_string(path).expect("read migration SQL"))
        .collect()
}

fn database_at(version: usize) -> TestDb {
    let dir = TempDir::new().expect("temporary database directory");
    let path = dir.path().join("covey.db");
    let mut connection = Connection::open(&path).expect("open database");
    let sources = migration_sources();
    let migrations = Migrations::new(
        sources
            .iter()
            .take(HISTORY_VERSION)
            .map(|source| M::up(source.as_str()))
            .collect(),
    );
    connection
        .pragma_update(None, "foreign_keys", "OFF")
        .expect("disable foreign keys around migrations");
    let migration_result = migrations.to_version(&mut connection, version);
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .expect("restore foreign keys after migrations");
    migration_result.expect("apply migrations");
    TestDb {
        _dir: dir,
        path,
        connection,
    }
}

fn assert_integrity(connection: &Connection) {
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("integrity check");
    assert_eq!(integrity, "ok");
    let foreign_key_violations: i64 = connection
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .expect("foreign key check");
    assert_eq!(foreign_key_violations, 0);
}

/// Seeds one session and three meta tasks for the dedupe rows to reference.
fn seed_meta_tasks(connection: &Connection) {
    connection
        .execute_batch(
            r#"
            INSERT INTO sessions (
                session_token, agent_principal_id, agent_instance_id, role, state,
                active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at
            ) VALUES ('session-orchestrator', 'orchestrator', 'orchestrator-1',
                      'orchestrator', 'active', NULL, 1, 1, 1, 1);
            INSERT INTO meta_tasks (
                meta_task_id, prompt_text, state, created_by, created_at, updated_at
            ) VALUES
                ('meta-a', 'first task', 'cancelled', 'session-orchestrator', 10, 10),
                ('meta-b', 'second task', 'planning', 'session-orchestrator', 20, 20),
                ('meta-c', 'third task', 'planning', 'session-orchestrator', 30, 30);
            "#,
        )
        .expect("seed session and meta tasks");
}

fn insert_dedupe_key(
    connection: &Connection,
    agent_principal_id: &str,
    dedupe_key: &str,
    meta_task_id: &str,
    created_at: i64,
) -> rusqlite::Result<usize> {
    connection.execute(
        "INSERT INTO meta_task_dedupe_keys (agent_principal_id, dedupe_key, meta_task_id,
             created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![agent_principal_id, dedupe_key, meta_task_id, created_at],
    )
}

fn dedupe_rows(connection: &Connection) -> Vec<(String, String, String, i64)> {
    connection
        .prepare(
            "SELECT agent_principal_id, dedupe_key, meta_task_id, created_at
             FROM meta_task_dedupe_keys
             ORDER BY agent_principal_id, dedupe_key, meta_task_id",
        )
        .expect("prepare dedupe query")
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .expect("query dedupe rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("read dedupe rows")
}

#[test]
fn pre_039_database_upgrades_and_keeps_its_dedupe_rows() {
    let db = database_at(PRE_HISTORY_VERSION);
    seed_meta_tasks(&db.connection);
    for (principal, key, meta_task_id, created_at) in [
        ("orchestrator", "intake-1", "meta-a", 11),
        ("orchestrator", "intake-2", "meta-b", 21),
        ("other-principal", "intake-1", "meta-c", 31),
    ] {
        insert_dedupe_key(&db.connection, principal, key, meta_task_id, created_at)
            .expect("insert a pre-039 dedupe row");
    }
    let before = dedupe_rows(&db.connection);

    // Opening through Covey applies the current migration suite to the old file.
    let _covey = Covey::open_with_clock(&db.path, std::sync::Arc::new(ManualClock::new(NOW_MS)))
        .expect("open upgraded covey");

    let user_version: i64 = db
        .connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read schema version");
    assert_eq!(user_version, migration_sources().len() as i64);
    assert_eq!(dedupe_rows(&db.connection), before);

    let task_index: i64 = db
        .connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'index' AND name = 'idx_meta_task_dedupe_keys_task'",
            [],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    assert_eq!(task_index, 1, "the task lookup index is created");
    assert_integrity(&db.connection);
}

#[test]
fn one_key_names_several_tasks_and_a_task_only_once() {
    let db = database_at(HISTORY_VERSION);
    seed_meta_tasks(&db.connection);

    insert_dedupe_key(&db.connection, "orchestrator", "intake-1", "meta-a", 11)
        .expect("file the first task under the key");
    insert_dedupe_key(&db.connection, "orchestrator", "intake-1", "meta-b", 21)
        .expect("file a second task under the same key");

    let repeated = insert_dedupe_key(&db.connection, "orchestrator", "intake-1", "meta-b", 22)
        .expect_err("the same task cannot be filed twice under one key");
    assert!(
        repeated.to_string().contains("UNIQUE constraint failed"),
        "the refusal must come from the primary key, got {repeated}"
    );
    assert_eq!(dedupe_rows(&db.connection).len(), 2);
    assert_integrity(&db.connection);
}

#[rstest]
#[case::blank_key("orchestrator", "   ", "meta-a", "CHECK")]
#[case::unknown_meta_task("orchestrator", "intake-1", "meta-missing", "FOREIGN KEY")]
fn the_rebuilt_table_keeps_its_constraints(
    #[case] principal: &str,
    #[case] dedupe_key: &str,
    #[case] meta_task_id: &str,
    #[case] constraint: &str,
) {
    let db = database_at(HISTORY_VERSION);
    seed_meta_tasks(&db.connection);

    let refused = insert_dedupe_key(&db.connection, principal, dedupe_key, meta_task_id, 11)
        .expect_err("a row that breaks a table constraint is refused");
    assert!(
        refused.to_string().contains(constraint),
        "the refusal must come from the {constraint} constraint, got {refused}"
    );
    assert!(dedupe_rows(&db.connection).is_empty());
}

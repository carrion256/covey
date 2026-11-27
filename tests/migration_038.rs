//! Upgrading a pre-038 database numbers existing meta tasks and constrains the
//! typed tracking fields.
//!
//! Migration 038 adds typed columns to `meta_tasks`, backfills a store-issued
//! `handle` in creation order, and adds three side tables. Anything written
//! straight to SQL has to obey the same constraints as API writes.

use std::{fs, path::PathBuf};

use covey::{Covey, ManualClock};
use rstest::rstest;
use rusqlite::{Connection, params};
use rusqlite_migration::{M, Migrations};
use tempfile::TempDir;

const PRE_TRACKING_VERSION: usize = 37;
const TRACKING_VERSION: usize = 38;
const NOW_MS: i64 = 1_700_000_000_000;

struct TestDb {
    _dir: TempDir,
    path: PathBuf,
    connection: Connection,
}

fn migration_sources() -> Vec<String> {
    let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/migrations");
    let mut paths = fs::read_dir(migrations_dir)
        .expect("read migrations")
        .map(|entry| entry.expect("migration entry").path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("sql"))
        .collect::<Vec<_>>();
    assert!(
        paths.len() >= TRACKING_VERSION,
        "expected at least {TRACKING_VERSION} migration files, found {}",
        paths.len()
    );
    paths.sort();
    paths
        .into_iter()
        .take(TRACKING_VERSION)
        .map(|path| fs::read_to_string(path).expect("read migration SQL"))
        .collect::<Vec<_>>()
}

fn migrate_to(connection: &mut Connection, version: usize) {
    let sources = migration_sources();
    let migrations = Migrations::new(
        sources
            .iter()
            .map(|source| M::up(source.as_str()))
            .collect(),
    );
    connection
        .pragma_update(None, "foreign_keys", "OFF")
        .expect("disable foreign keys around migrations");
    let migration_result = migrations.to_version(connection, version);
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .expect("restore foreign keys after migrations");
    migration_result.expect("apply migrations");
}

fn database_at(version: usize) -> TestDb {
    let dir = TempDir::new().expect("temporary database directory");
    let path = dir.path().join("covey.db");
    let mut connection = Connection::open(&path).expect("open database");
    migrate_to(&mut connection, version);
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

/// Seeds a session and four meta tasks exactly as a pre-038 store would hold
/// them. Two tasks share a `created_at`, and the ids of that pair sort against
/// their insertion order.
fn seed_legacy_meta_tasks(connection: &Connection) {
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
                ('meta-b', 'second of the shared timestamp', 'planning',
                 'session-orchestrator', 10, 10),
                ('meta-a', 'first of the shared timestamp', 'planning',
                 'session-orchestrator', 10, 10),
                ('meta-d', 'oldest task', 'planning', 'session-orchestrator', 5, 5),
                ('meta-c', 'newest task', 'planning', 'session-orchestrator', 20, 20);
            "#,
        )
        .expect("seed legacy session and meta tasks");
}

fn table_exists(connection: &Connection, name: &str) -> bool {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![name],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    count == 1
}

fn insert_meta_task(connection: &Connection, meta_task_id: &str, created_at: i64) {
    connection
        .execute(
            "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at,
                 updated_at)
             VALUES (?1, 'prompt', 'planning', 'session-orchestrator', ?2, ?2)",
            params![meta_task_id, created_at],
        )
        .expect("insert a meta task without a handle");
}

fn insert_meta_task_with_handle(
    connection: &Connection,
    meta_task_id: &str,
    created_at: i64,
    handle: i64,
) {
    connection
        .execute(
            "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at,
                 updated_at, handle)
             VALUES (?1, 'prompt', 'planning', 'session-orchestrator', ?2, ?2, ?3)",
            params![meta_task_id, created_at, handle],
        )
        .expect("insert a meta task with an explicit handle");
}

fn handle_of(connection: &Connection, meta_task_id: &str) -> Option<i64> {
    connection
        .query_row(
            "SELECT handle FROM meta_tasks WHERE meta_task_id = ?1",
            params![meta_task_id],
            |row| row.get(0),
        )
        .expect("read the meta task handle")
}

fn insert_dedupe_key(
    connection: &Connection,
    agent_principal_id: &str,
    dedupe_key: &str,
    meta_task_id: &str,
) {
    connection
        .execute(
            "INSERT INTO meta_task_dedupe_keys (agent_principal_id, dedupe_key, meta_task_id,
                 created_at)
             VALUES (?1, ?2, ?3, 1)",
            params![agent_principal_id, dedupe_key, meta_task_id],
        )
        .expect("insert a dedupe key");
}

#[test]
fn pre_038_database_upgrades_and_numbers_existing_meta_tasks() {
    let db = database_at(PRE_TRACKING_VERSION);
    assert!(!table_exists(&db.connection, "meta_task_supersedes"));
    assert!(!table_exists(&db.connection, "meta_task_dedupe_keys"));
    assert!(!table_exists(&db.connection, "meta_task_annotations"));
    seed_legacy_meta_tasks(&db.connection);

    // Opening through Covey applies the current migration suite to the old file.
    let _covey = Covey::open_with_clock(&db.path, std::sync::Arc::new(ManualClock::new(NOW_MS)))
        .expect("open upgraded covey");

    let user_version: i64 = db
        .connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read schema version");
    let migration_files =
        fs::read_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/migrations"))
            .expect("read migrations")
            .count();
    assert_eq!(user_version, migration_files as i64);

    let handles = db
        .connection
        .prepare("SELECT meta_task_id, handle FROM meta_tasks ORDER BY created_at, meta_task_id")
        .expect("prepare handle query")
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .expect("query handles")
        .collect::<Result<Vec<_>, _>>()
        .expect("read handles");
    assert_eq!(
        handles,
        vec![
            ("meta-d".to_owned(), 1),
            ("meta-a".to_owned(), 2),
            ("meta-b".to_owned(), 3),
            ("meta-c".to_owned(), 4),
        ]
    );

    let null_counts: (i64, i64, i64, i64) = db
        .connection
        .query_row(
            "SELECT SUM(title IS NULL), SUM(urgency IS NULL), SUM(due_on IS NULL),
                    SUM(closure IS NULL) FROM meta_tasks",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("count null tracking fields");
    assert_eq!(null_counts, (4, 4, 4, 4));

    for table in [
        "meta_task_supersedes",
        "meta_task_dedupe_keys",
        "meta_task_annotations",
    ] {
        assert!(table_exists(&db.connection, table), "{table} is created");
    }
    assert_integrity(&db.connection);
}

#[test]
fn rows_written_straight_to_sql_get_the_next_handle() {
    let db = database_at(TRACKING_VERSION);
    seed_legacy_meta_tasks(&db.connection);

    insert_meta_task(&db.connection, "meta-implicit-1", 30);
    assert_eq!(handle_of(&db.connection, "meta-implicit-1"), Some(5));
    insert_meta_task(&db.connection, "meta-implicit-2", 31);
    assert_eq!(handle_of(&db.connection, "meta-implicit-2"), Some(6));
    insert_meta_task_with_handle(&db.connection, "meta-explicit", 32, 42);
    assert_eq!(handle_of(&db.connection, "meta-explicit"), Some(42));

    let duplicate = db
        .connection
        .execute(
            "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at,
                 updated_at, handle)
             VALUES ('meta-duplicate', 'prompt', 'planning', 'session-orchestrator', 33, 33, 42)",
            [],
        )
        .expect_err("an explicit duplicate handle is refused");
    assert!(
        duplicate
            .to_string()
            .contains("UNIQUE constraint failed: meta_tasks.handle"),
        "the refusal must come from the handle unique index, got {duplicate}"
    );

    let zero = db
        .connection
        .execute(
            "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at,
                 updated_at, handle)
             VALUES ('meta-zero', 'prompt', 'planning', 'session-orchestrator', 34, 34, 0)",
            [],
        )
        .expect_err("handle 0 is refused");
    assert!(
        zero.to_string().contains("CHECK"),
        "the refusal must come from the handle CHECK, got {zero}"
    );
    assert_integrity(&db.connection);
}

#[rstest]
#[case::blank_title(
    "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at,
         updated_at, title)
     VALUES ('meta-refused', 'prompt', 'planning', 'session-orchestrator', 40, 40, '')"
)]
#[case::whitespace_title(
    "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at,
         updated_at, title)
     VALUES ('meta-refused', 'prompt', 'planning', 'session-orchestrator', 40, 40, '   ')"
)]
#[case::unknown_urgency(
    "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at,
         updated_at, urgency)
     VALUES ('meta-refused', 'prompt', 'planning', 'session-orchestrator', 40, 40, 'urgent')"
)]
#[case::short_month_due_date(
    "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at,
         updated_at, due_on)
     VALUES ('meta-refused', 'prompt', 'planning', 'session-orchestrator', 40, 40, '2026-1-01')"
)]
#[case::compact_due_date(
    "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at,
         updated_at, due_on)
     VALUES ('meta-refused', 'prompt', 'planning', 'session-orchestrator', 40, 40, '20260101')"
)]
#[case::slash_due_date(
    "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at,
         updated_at, due_on)
     VALUES ('meta-refused', 'prompt', 'planning', 'session-orchestrator', 40, 40, '2026/01/01')"
)]
#[case::unknown_closure(
    "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at,
         updated_at, closure)
     VALUES ('meta-refused', 'prompt', 'planning', 'session-orchestrator', 40, 40, 'finished')"
)]
#[case::unknown_annotation_key(
    "INSERT INTO meta_task_annotations (meta_task_id, key, value, updated_by, updated_at)
     VALUES ('meta-a', 'note', 'recorded', 'session-orchestrator', 1)"
)]
#[case::non_boolean_acked(
    "INSERT INTO meta_task_annotations (meta_task_id, key, value, updated_by, updated_at)
     VALUES ('meta-a', 'acked', 'yes', 'session-orchestrator', 1)"
)]
#[case::blank_annotation_value(
    "INSERT INTO meta_task_annotations (meta_task_id, key, value, updated_by, updated_at)
     VALUES ('meta-a', 'verdict', '', 'session-orchestrator', 1)"
)]
#[case::blank_dedupe_key(
    "INSERT INTO meta_task_dedupe_keys (agent_principal_id, dedupe_key, meta_task_id, created_at)
     VALUES ('principal-1', '', 'meta-a', 1)"
)]
#[case::self_supersede(
    "INSERT INTO meta_task_supersedes (
         superseding_meta_task_id, superseded_meta_task_id, superseded_at
     ) VALUES ('meta-a', 'meta-a', 1)"
)]
fn upgraded_schema_rejects_invalid_tracking_rows(#[case] sql: &str) {
    let db = database_at(TRACKING_VERSION);
    seed_legacy_meta_tasks(&db.connection);

    let error = db
        .connection
        .execute(sql, [])
        .expect_err("the schema constraints must reject this row");
    assert!(
        error.to_string().contains("CHECK"),
        "the refusal must come from a schema CHECK constraint, got {error}"
    );
    assert_integrity(&db.connection);
}

#[test]
fn supersede_targets_and_dedupe_keys_are_unique() {
    let db = database_at(TRACKING_VERSION);
    seed_legacy_meta_tasks(&db.connection);

    db.connection
        .execute(
            "INSERT INTO meta_task_supersedes (
                 superseding_meta_task_id, superseded_meta_task_id, superseded_at
             ) VALUES ('meta-b', 'meta-a', 1)",
            [],
        )
        .expect("first supersede link");

    let replaced_twice = db
        .connection
        .execute(
            "INSERT INTO meta_task_supersedes (
                 superseding_meta_task_id, superseded_meta_task_id, superseded_at
             ) VALUES ('meta-c', 'meta-a', 2)",
            [],
        )
        .expect_err("a replaced task is replaced once");
    assert!(
        replaced_twice
            .to_string()
            .contains("UNIQUE constraint failed: meta_task_supersedes.superseded_meta_task_id"),
        "the refusal must come from the replaced-task unique index, got {replaced_twice}"
    );

    insert_dedupe_key(&db.connection, "principal-1", "shared-key", "meta-a");
    insert_dedupe_key(&db.connection, "principal-2", "shared-key", "meta-b");

    let duplicate_key = db
        .connection
        .execute(
            "INSERT INTO meta_task_dedupe_keys (agent_principal_id, dedupe_key, meta_task_id,
                 created_at)
             VALUES ('principal-1', 'shared-key', 'meta-c', 3)",
            [],
        )
        .expect_err("one principal holds a dedupe key once");
    assert!(
        duplicate_key
            .to_string()
            .contains("UNIQUE constraint failed: meta_task_dedupe_keys"),
        "the refusal must come from the dedupe key primary key, got {duplicate_key}"
    );
    assert_integrity(&db.connection);
}

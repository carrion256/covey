//! Upgrading a pre-037 database keeps existing meta tasks unlinked.
//!
//! Migration 037 only adds the `meta_task_source_links` side table: rows that
//! predate it must stay without a source link, and the table constraints must
//! hold for anything written straight to SQL.

use std::{fs, path::PathBuf};

use covey::{Covey, ManualClock};
use rstest::rstest;
use rusqlite::{Connection, params};
use rusqlite_migration::{M, Migrations};
use tempfile::TempDir;

const PRE_SOURCE_LINK_VERSION: usize = 36;
const SOURCE_LINK_VERSION: usize = 37;
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
        paths.len() >= SOURCE_LINK_VERSION,
        "expected at least {SOURCE_LINK_VERSION} migration files, found {}",
        paths.len()
    );
    paths.sort();
    paths
        .into_iter()
        .take(SOURCE_LINK_VERSION)
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

/// Seeds a session and two meta tasks exactly as a pre-037 store would hold them.
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
                ('meta-legacy-a', 'Backlink: /data/intake/legacy.md', 'planning',
                 'session-orchestrator', 1, 1),
                ('meta-legacy-b', 'plain legacy prompt', 'active',
                 'session-orchestrator', 1, 1);
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

#[test]
fn pre_037_database_upgrades_and_existing_meta_tasks_stay_unlinked() {
    let db = database_at(PRE_SOURCE_LINK_VERSION);
    assert!(!table_exists(&db.connection, "meta_task_source_links"));
    seed_legacy_meta_tasks(&db.connection);

    // Opening through Covey applies the current migration suite to the old file.
    let covey = Covey::open_with_clock(&db.path, std::sync::Arc::new(ManualClock::new(NOW_MS)))
        .expect("open upgraded covey");

    for meta_task_id in ["meta-legacy-a", "meta-legacy-b"] {
        let status = covey
            .meta_task_status(meta_task_id)
            .expect("status of an upgraded meta task");
        assert_eq!(status.meta_task().meta_task_id.as_str(), meta_task_id);
        assert!(
            status.meta_task().source_link().is_none(),
            "a meta task predating migration 037 has no source link"
        );
        let rendered = serde_json::to_value(&status).expect("status serializes");
        assert_eq!(
            rendered["meta_task"]["source_link"],
            serde_json::Value::Null
        );
    }

    assert!(table_exists(&db.connection, "meta_task_source_links"));
    let rows: i64 = db
        .connection
        .query_row("SELECT COUNT(*) FROM meta_task_source_links", [], |row| {
            row.get(0)
        })
        .expect("count source link rows");
    assert_eq!(rows, 0, "the migration performs no backfill");
    assert_integrity(&db.connection);
}

#[rstest]
#[case::relative_path(
    "INSERT INTO meta_task_source_links (
         meta_task_id, thread_id, url, received_at, path, origin, method, linked_by, linked_at
     ) VALUES ('meta-legacy-a', 'thread-1', NULL, '2026-11-06T00:00:00Z',
               'relative/orders.md', 'submitted', NULL, 'session-orchestrator', 1)"
)]
#[case::blank_thread_id(
    "INSERT INTO meta_task_source_links (
         meta_task_id, thread_id, url, received_at, path, origin, method, linked_by, linked_at
     ) VALUES ('meta-legacy-a', '', NULL, '2026-11-06T00:00:00Z',
               '/data/intake/orders.md', 'submitted', NULL, 'session-orchestrator', 1)"
)]
#[case::backfilled_without_method(
    "INSERT INTO meta_task_source_links (
         meta_task_id, thread_id, url, received_at, path, origin, method, linked_by, linked_at
     ) VALUES ('meta-legacy-a', 'thread-1', NULL, '2026-11-06T00:00:00Z',
               '/data/intake/orders.md', 'backfilled', NULL, 'session-orchestrator', 1)"
)]
#[case::submitted_with_method(
    "INSERT INTO meta_task_source_links (
         meta_task_id, thread_id, url, received_at, path, origin, method, linked_by, linked_at
     ) VALUES ('meta-legacy-a', 'thread-1', NULL, '2026-11-06T00:00:00Z',
               '/data/intake/orders.md', 'submitted', 'guessed', 'session-orchestrator', 1)"
)]
#[case::unknown_origin(
    "INSERT INTO meta_task_source_links (
         meta_task_id, thread_id, url, received_at, path, origin, method, linked_by, linked_at
     ) VALUES ('meta-legacy-a', 'thread-1', NULL, '2026-11-06T00:00:00Z',
               '/data/intake/orders.md', 'inferred', NULL, 'session-orchestrator', 1)"
)]
#[case::blank_url(
    "INSERT INTO meta_task_source_links (
         meta_task_id, thread_id, url, received_at, path, origin, method, linked_by, linked_at
     ) VALUES ('meta-legacy-a', 'thread-1', '', '2026-11-06T00:00:00Z',
               '/data/intake/orders.md', 'submitted', NULL, 'session-orchestrator', 1)"
)]
fn upgraded_schema_rejects_invalid_source_link_rows(#[case] sql: &str) {
    let db = database_at(SOURCE_LINK_VERSION);
    seed_legacy_meta_tasks(&db.connection);

    let error = db
        .connection
        .execute(sql, [])
        .expect_err("the table constraints must reject this row");
    assert!(
        error.to_string().contains("CHECK"),
        "the refusal must come from a schema CHECK constraint, got {error}"
    );
    assert_integrity(&db.connection);
}

#[test]
fn upgraded_schema_holds_at_most_one_link_per_meta_task() {
    let db = database_at(SOURCE_LINK_VERSION);
    seed_legacy_meta_tasks(&db.connection);
    db.connection
        .execute(
            "INSERT INTO meta_task_source_links (
                 meta_task_id, thread_id, url, received_at, path, origin, method,
                 linked_by, linked_at
             ) VALUES ('meta-legacy-a', 'thread-1', NULL, '2026-11-06T00:00:00Z',
                       '/data/intake/orders.md', 'submitted', NULL,
                       'session-orchestrator', 1)",
            [],
        )
        .expect("first source link row");

    let error = db
        .connection
        .execute(
            "INSERT INTO meta_task_source_links (
                 meta_task_id, thread_id, url, received_at, path, origin, method,
                 linked_by, linked_at
             ) VALUES ('meta-legacy-a', 'thread-2', NULL, '2026-11-06T00:00:00Z',
                       '/data/intake/other.md', 'submitted', NULL,
                       'session-orchestrator', 2)",
            [],
        )
        .expect_err("a meta task holds exactly one source link");
    assert!(
        error
            .to_string()
            .contains("UNIQUE constraint failed: meta_task_source_links.meta_task_id"),
        "the refusal must come from the primary key, got {error}"
    );
}

#[test]
fn upgraded_schema_accepts_both_legal_origin_pairings() {
    let db = database_at(SOURCE_LINK_VERSION);
    seed_legacy_meta_tasks(&db.connection);

    db.connection
        .execute(
            "INSERT INTO meta_task_source_links (
                 meta_task_id, thread_id, url, received_at, path, origin, method,
                 linked_by, linked_at
             ) VALUES ('meta-legacy-a', 'thread-1', 'https://example.test/1',
                       '2026-11-06T00:00:00Z', '/data/intake/a.md', 'submitted', NULL,
                       'session-orchestrator', 1)",
            [],
        )
        .expect("submitted link row");
    db.connection
        .execute(
            "INSERT INTO meta_task_source_links (
                 meta_task_id, thread_id, url, received_at, path, origin, method,
                 linked_by, linked_at
             ) VALUES ('meta-legacy-b', 'thread-2', NULL, '2026-11-06T00:00:00Z',
                       '/data/intake/b.md', 'backfilled', 'recovered from logs',
                       'session-orchestrator', 2)",
            [],
        )
        .expect("backfilled link row");

    let covey = Covey::open_with_clock(&db.path, std::sync::Arc::new(ManualClock::new(NOW_MS)))
        .expect("open covey");
    let status = covey
        .meta_task_status("meta-legacy-b")
        .expect("status after upgrade");
    let link = status
        .meta_task()
        .source_link()
        .expect("seeded backfilled link is read back");
    assert_eq!(link.method(), Some("recovered from logs"));
    assert_eq!(link.origin().to_string(), "backfilled");
    assert!(matches!(
        covey
            .meta_task_status("meta-legacy-a")
            .expect("status after upgrade")
            .meta_task()
            .source_link()
            .expect("seeded submitted link")
            .origin(),
        covey::MetaTaskSourceLinkOrigin::Submitted
    ));
}

//! Upgrading a pre-040 database keeps every session and lets one principal
//! hold an active session in each role.
//!
//! Migration 040 replaces the unique index over active sessions per principal
//! with one over principal and role. Sessions written before the migration
//! survive it, and rows written straight to SQL obey the rule the index states.

use std::{fs, path::PathBuf, sync::Arc};

use covey::{Covey, ManualClock, RegisterSessionReq, SessionRole};
use rstest::rstest;
use rusqlite::{Connection, params};
use rusqlite_migration::{M, Migrations};
use tempfile::TempDir;

const PRE_ROLE_INDEX_VERSION: usize = 39;
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
        paths.len() > PRE_ROLE_INDEX_VERSION,
        "expected more than {PRE_ROLE_INDEX_VERSION} migration files, found {}",
        paths.len()
    );
    paths.sort();
    paths
        .into_iter()
        .map(|path| fs::read_to_string(path).expect("read migration SQL"))
        .collect()
}

/// A store with the migrations before 040 applied and nothing after them.
fn pre_040_database() -> TestDb {
    let dir = TempDir::new().expect("temporary database directory");
    let path = dir.path().join("covey.db");
    let mut connection = Connection::open(&path).expect("open database");
    let sources = migration_sources();
    let migrations = Migrations::new(
        sources
            .iter()
            .take(PRE_ROLE_INDEX_VERSION)
            .map(|source| M::up(source.as_str()))
            .collect(),
    );
    connection
        .pragma_update(None, "foreign_keys", "OFF")
        .expect("disable foreign keys around migrations");
    let migration_result = migrations.to_latest(&mut connection);
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

/// Opening through Covey applies the current migration suite to the old file.
fn upgrade(db: &TestDb) -> Covey {
    Covey::open_with_clock(&db.path, Arc::new(ManualClock::new(NOW_MS)))
        .expect("open upgraded covey")
}

fn insert_session(
    connection: &Connection,
    session_token: &str,
    role: &str,
    state: &str,
) -> rusqlite::Result<usize> {
    connection.execute(
        "INSERT INTO sessions (
             session_token, agent_principal_id, agent_instance_id, role, state,
             active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at
         ) VALUES (?1, 'agent', 'instance-1', ?2, ?3, NULL, 1, 1, 1, 1)",
        params![session_token, role, state],
    )
}

fn index_exists(connection: &Connection, name: &str) -> bool {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
            params![name],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    count == 1
}

fn session_tokens(connection: &Connection) -> Vec<String> {
    connection
        .prepare("SELECT session_token FROM sessions ORDER BY session_token")
        .expect("prepare session query")
        .query_map([], |row| row.get(0))
        .expect("query sessions")
        .collect::<Result<Vec<_>, _>>()
        .expect("read sessions")
}

#[test]
fn pre_040_database_upgrades_and_keeps_its_sessions() {
    let db = pre_040_database();
    insert_session(&db.connection, "session-planner", "orchestrator", "active")
        .expect("insert the active planning session");
    insert_session(&db.connection, "session-ended", "executor", "exited")
        .expect("insert an ended executor session");
    let refused = insert_session(&db.connection, "session-worker", "executor", "active")
        .expect_err("before 040 a principal holds one active session in any role");
    assert!(refused.to_string().contains("UNIQUE"), "{refused}");

    let covey = upgrade(&db);

    let user_version: i64 = db
        .connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read schema version");
    assert_eq!(user_version, migration_sources().len() as i64);
    assert!(!index_exists(
        &db.connection,
        "idx_sessions_one_active_per_principal"
    ));
    assert!(index_exists(
        &db.connection,
        "idx_sessions_one_active_per_principal_role"
    ));
    assert_eq!(
        session_tokens(&db.connection),
        ["session-ended", "session-planner"]
    );

    let worker = covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                "agent",
                "instance-2",
                SessionRole::Executor,
                "register-worker",
            )
            .expect("valid session registration request"),
        )
        .expect("the planning principal registers an executor session after the upgrade");
    assert_eq!(worker.role, SessionRole::Executor);

    let foreign_key_violations: i64 = db
        .connection
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .expect("foreign key check");
    assert_eq!(foreign_key_violations, 0);
}

#[rstest]
#[case::another_active_session_in_the_same_role("orchestrator", "active", false)]
#[case::an_active_session_in_another_role("executor", "active", true)]
#[case::an_ended_session_in_the_same_role("orchestrator", "exited", true)]
fn the_index_allows_one_active_session_per_principal_and_role(
    #[case] role: &str,
    #[case] state: &str,
    #[case] accepted: bool,
) {
    let db = pre_040_database();
    let _covey = upgrade(&db);
    insert_session(&db.connection, "session-first", "orchestrator", "active")
        .expect("insert the first active session");

    match insert_session(&db.connection, "session-second", role, state) {
        Ok(_) => assert!(
            accepted,
            "a second {state} session in role {role} must be refused"
        ),
        Err(error) => {
            assert!(
                !accepted,
                "a second {state} session in role {role} must be accepted: {error}"
            );
            assert!(error.to_string().contains("UNIQUE"), "{error}");
        }
    }
}

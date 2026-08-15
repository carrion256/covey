use std::{fs, path::PathBuf};

use rusqlite::{Connection, params};
use rusqlite_migration::{M, Migrations};
use tempfile::TempDir;

const PRE_GENERIC_VERSION: usize = 34;
const GENERIC_VERSION: usize = 35;

struct TestDb {
    _dir: TempDir,
    connection: Connection,
}

fn migration_sources() -> Vec<String> {
    let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/migrations");
    let mut paths = fs::read_dir(migrations_dir)
        .expect("read migrations")
        .map(|entry| entry.expect("migration entry").path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("sql"))
        .collect::<Vec<_>>();
    paths.sort();
    assert!(
        paths.len() >= GENERIC_VERSION,
        "migration 035 is missing from the migration directory"
    );
    assert_eq!(
        paths[GENERIC_VERSION - 1]
            .file_name()
            .and_then(|value| value.to_str()),
        Some("035_generic_subtask_defaults.sql")
    );
    paths
        .into_iter()
        .take(GENERIC_VERSION)
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
        .expect("disable foreign keys around table-rebuild migrations");
    let migration_result = migrations.to_version(connection, version);
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .expect("restore foreign keys after migrations");
    migration_result.expect("apply migrations");
}

fn database_at(version: usize) -> TestDb {
    let dir = TempDir::new().expect("temporary database directory");
    let path = dir.path().join("covey.db");
    let mut connection = Connection::open(path).expect("open database");
    migrate_to(&mut connection, version);
    TestDb {
        _dir: dir,
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

fn seed_meta_task(connection: &Connection, meta_task_id: &str) {
    connection
        .execute(
            r#"
            INSERT INTO sessions (
                session_token, agent_principal_id, agent_instance_id, role, state,
                active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at
            ) VALUES ('session-orch', 'orch', 'orch-1', 'orchestrator', 'active', NULL, 1, 1, 1, 1)
            "#,
            [],
        )
        .expect("seed orchestrator session");
    connection
        .execute(
            r#"
            INSERT INTO meta_tasks (
                meta_task_id, prompt_text, state, created_by, created_at, updated_at
            ) VALUES (?1, 'meta', 'active', 'session-orch', 1, 1)
            "#,
            params![meta_task_id],
        )
        .expect("seed meta task");
}

#[test]
fn v34_database_with_persisted_mutai_rows_migrates_to_generic_defaults_without_data_loss() {
    let mut db = database_at(PRE_GENERIC_VERSION);
    seed_meta_task(&db.connection, "meta-mutai");

    db.connection
        .execute_batch(
            r#"
            INSERT INTO subtasks (
                subtask_id, meta_task_id, title, kind, review_target_subtask_id,
                review_target_artifact_digest, state, current_claim_id, artifact_digest,
                priority, created_at, updated_at
            ) VALUES
                ('mutai-open', 'meta-mutai', 'open', 'work', NULL, NULL,
                 'available', NULL, NULL, 1, 2, 2),
                ('mutai-applied', 'meta-mutai', 'applied', 'work', NULL, NULL,
                 'available', NULL, NULL, 2, 3, 3),
                ('mutai-review', 'meta-mutai', 'review applied', 'review',
                 'mutai-applied', 'blake3:mutai', 'decided', NULL, NULL, 1, 4, 4);

            INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq)
            VALUES ('mutai-open', 1), ('mutai-applied', 2), ('mutai-review', 1);
            "#,
        )
        .expect("seed version 34 mutai graph via legacy defaults");
    migrate_to(&mut db.connection, GENERIC_VERSION);
    let (open_policy, open_route, applied_policy, review_policy, review_route): (
        String,
        String,
        String,
        String,
        String,
    ) = db
        .connection
        .query_row(
            "SELECT
                (SELECT completion_policy FROM subtasks WHERE subtask_id = 'mutai-open'),
                (SELECT routing_key FROM subtasks WHERE subtask_id = 'mutai-open'),
                (SELECT completion_policy FROM subtasks WHERE subtask_id = 'mutai-applied'),
                (SELECT completion_policy FROM subtasks WHERE subtask_id = 'mutai-review'),
                (SELECT routing_key FROM subtasks WHERE subtask_id = 'mutai-review')",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("read persisted policies after migration");
    assert_eq!(
        open_policy, "canonical_apply",
        "explicit mutAI row preserved"
    );
    assert_eq!(open_route, "mutai", "explicit mutAI route preserved");
    assert_eq!(applied_policy, "canonical_apply");
    assert_eq!(review_policy, "canonical_apply");
    assert_eq!(review_route, "mutai");

    assert_integrity(&db.connection);
    let count: i64 = db
        .connection
        .query_row(
            "SELECT COUNT(*) FROM subtasks WHERE meta_task_id = 'meta-mutai'",
            [],
            |row| row.get(0),
        )
        .expect("count migrated rows");
    assert_eq!(count, 3, "no subtask rows lost by migration 035");
}

#[test]
fn fresh_v35_database_defaults_to_generic_policy_and_route() {
    let db = database_at(GENERIC_VERSION);
    seed_meta_task(&db.connection, "meta-generic");

    db.connection
        .execute(
            r#"
            INSERT INTO subtasks (
                subtask_id, meta_task_id, title, kind, review_target_subtask_id,
                review_target_artifact_digest, state, current_claim_id, artifact_digest,
                priority, created_at, updated_at
            ) VALUES ('fresh-open', 'meta-generic', 'open', 'work', NULL, NULL,
                      'available', NULL, NULL, 1, 2, 2)
            "#,
            [],
        )
        .expect("insert subtask without policy columns");

    let (policy, route): (String, String) = db
        .connection
        .query_row(
            "SELECT completion_policy, routing_key FROM subtasks WHERE subtask_id = 'fresh-open'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read fresh generic row");
    assert_eq!(policy, "direct", "fresh databases default to direct policy");
    assert_eq!(
        route, "default",
        "fresh databases default to the default lane"
    );

    assert_integrity(&db.connection);
}

#[test]
fn v35_review_subtasks_accept_reviewed_and_canonical_policies_on_any_lane() {
    let db = database_at(GENERIC_VERSION);
    seed_meta_task(&db.connection, "meta-generic");

    db.connection
        .execute(
            r#"
            INSERT INTO subtasks (
                subtask_id, meta_task_id, title, kind, review_target_subtask_id,
                review_target_artifact_digest, state, current_claim_id, artifact_digest,
                priority, completion_policy, routing_key, created_at, updated_at
            ) VALUES
                ('review-generic', 'meta-generic', 'review generic', 'review',
                 'work-a', 'blake3:generic-a', 'decided', NULL, NULL, 1,
                 'reviewed', 'default', 3, 3),
                ('review-canon', 'meta-generic', 'review canonical', 'review',
                 'work-b', 'blake3:generic-b', 'decided', NULL, NULL, 1,
                 'canonical_apply', 'default', 4, 4)
            "#,
            [],
        )
        .expect("review subtasks on the default lane under allowed policies");

    assert_integrity(&db.connection);
    let direct_review = db.connection.execute(
        r#"
        INSERT INTO subtasks (
            subtask_id, meta_task_id, title, kind, review_target_subtask_id,
            review_target_artifact_digest, state, current_claim_id, artifact_digest,
            priority, completion_policy, routing_key, created_at, updated_at
        ) VALUES ('review-direct', 'meta-generic', 'review direct', 'review',
                  'work-c', 'blake3:generic-c', 'decided', NULL, NULL, 1,
                  'direct', 'default', 5, 5)
        "#,
        [],
    );
    let error = direct_review.expect_err("a direct-policy review subtask is rejected");
    assert!(
        error
            .to_string()
            .contains("completion_policy IN ('canonical_apply', 'reviewed')"),
        "unexpected rejection: {error}"
    );
}

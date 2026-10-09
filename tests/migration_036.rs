use std::{fs, path::PathBuf};

use rstest::rstest;
use rusqlite::{Connection, params};
use rusqlite_migration::{M, Migrations};
use tempfile::TempDir;

const PRE_EVIDENCE_VERSION: usize = 35;
const EVIDENCE_VERSION: usize = 36;

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
    assert!(
        paths.len() >= EVIDENCE_VERSION,
        "expected at least {EVIDENCE_VERSION} migration files, found {}",
        paths.len()
    );
    paths.sort();
    paths
        .into_iter()
        .take(EVIDENCE_VERSION)
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
    let mut connection = Connection::open(dir.path().join("covey.db")).expect("open database");
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

fn seed_legacy_evidence(connection: &Connection) {
    connection
        .execute_batch(
            r#"
            INSERT INTO sessions (
                session_token, agent_principal_id, agent_instance_id, role, state,
                active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at
            ) VALUES ('session-worker', 'worker', 'worker-1', 'executor', 'active',
                      NULL, 1, 1, 1, 1);
            INSERT INTO meta_tasks (
                meta_task_id, prompt_text, state, created_by, created_at, updated_at
            ) VALUES ('meta-legacy', 'legacy', 'active', 'session-worker', 1, 1);
            INSERT INTO subtasks (
                subtask_id, meta_task_id, title, kind, review_target_subtask_id,
                review_target_artifact_digest, state, current_claim_id, artifact_digest,
                priority, completion_policy, routing_key, created_at, updated_at
            ) VALUES
                ('attempt-work', 'meta-legacy', 'legacy attempt', 'work', NULL, NULL,
                 'available', NULL, NULL, 1, 'direct', 'hermes', 2, 3),
                ('idle-work', 'meta-legacy', 'legacy idle', 'work', NULL, NULL,
                 'available', NULL, NULL, 1, 'direct', 'hermes', 2, 2);
            INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq)
            VALUES ('attempt-work', 4), ('idle-work', 4);
            INSERT INTO claims (
                claim_id, subtask_id, owner_session_token, fence_seq, lease_deadline,
                state, created_at, updated_at
            ) VALUES ('claim-attempt', 'attempt-work', 'session-worker', 1, 100,
                      'released', 3, 3);
            "#,
        )
        .expect("seed legacy session and tasks");
    connection
        .execute(
            r#"
            INSERT INTO subtask_attempt_outcomes (
                claim_id, subtask_id, fence_seq, outcome_kind, evidence_digest,
                failure_code, summary, recorded_by_session, recorded_at
            ) VALUES ('claim-attempt', 'attempt-work', 1, 'retryable_failure',
                      'blake3:legacy', 'transient_provider_error',
                      'legacy evidence digest', 'session-worker', 4)
            "#,
            [],
        )
        .expect("seed legacy evidence-backed outcome");
}

/// Register a fresh claim for `subtask_id` owned by `session-worker` with the
/// next fence from `subtask_fence_counter`, advancing the counter.
fn mint_claim(connection: &Connection, claim_id: &str, subtask_id: &str) {
    let fence: i64 = connection
        .query_row(
            "SELECT next_fence_seq FROM subtask_fence_counter WHERE subtask_id = ?1",
            params![subtask_id],
            |row| row.get(0),
        )
        .expect("fence counter");
    connection
        .execute(
            r#"
            INSERT INTO claims (
                claim_id, subtask_id, owner_session_token, fence_seq, lease_deadline,
                state, created_at, updated_at
            ) VALUES (?1, ?2, 'session-worker', ?3, 100, 'released', 10, 10)
            "#,
            params![claim_id, subtask_id, fence],
        )
        .expect("mint claim");
    connection
        .execute(
            "UPDATE subtask_fence_counter SET next_fence_seq = ?2 WHERE subtask_id = ?1",
            params![subtask_id, fence + 1],
        )
        .expect("advance fence counter");
}

#[test]
fn legacy_task_and_outcome_upgrade_keeps_required_policy_and_row_identity() {
    let mut db = database_at(PRE_EVIDENCE_VERSION);
    seed_legacy_evidence(&db.connection);
    migrate_to(&mut db.connection, EVIDENCE_VERSION);

    let policy: String = db
        .connection
        .query_row(
            "SELECT evidence_policy FROM subtasks WHERE subtask_id = 'attempt-work'",
            [],
            |row| row.get(0),
        )
        .expect("read legacy evidence policy");
    assert_eq!(
        policy, "required",
        "tasks predating migration 036 keep requiring evidence"
    );

    let outcome: (Option<String>, String, i64) = db
        .connection
        .query_row(
            "SELECT evidence_digest, summary, recorded_at
             FROM subtask_attempt_outcomes WHERE claim_id = 'claim-attempt'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("legacy outcome row survives the upgrade");
    assert_eq!(
        outcome,
        (
            Some("blake3:legacy".to_owned()),
            "legacy evidence digest".to_owned(),
            4
        ),
        "migration 036 must not alter outcome ledger rows"
    );
    assert_integrity(&db.connection);
}

#[rstest]
#[case::append_only_update(
    "UPDATE subtask_attempt_outcomes SET summary = 'rewritten' WHERE claim_id = 'claim-attempt'",
    "append-only"
)]
#[case::append_only_delete(
    "DELETE FROM subtask_attempt_outcomes WHERE claim_id = 'claim-attempt'",
    "append-only"
)]
#[case::policy_immutability(
    "UPDATE subtasks SET evidence_policy = 'optional' WHERE subtask_id = 'attempt-work'",
    "immutable"
)]
fn upgraded_invariants_reject_mutation(#[case] sql: &str, #[case] expected_reason: &str) {
    let mut db = database_at(PRE_EVIDENCE_VERSION);
    seed_legacy_evidence(&db.connection);
    migrate_to(&mut db.connection, EVIDENCE_VERSION);

    let error = db
        .connection
        .execute(sql, [])
        .expect_err("upgraded invariants must reject this mutation");
    assert!(
        error.to_string().contains(expected_reason),
        "rejection must name the violated invariant ({expected_reason}): {error}"
    );
}

#[test]
fn required_policy_legacy_task_rejects_evidence_free_outcome_but_accepts_digest() {
    let mut db = database_at(PRE_EVIDENCE_VERSION);
    seed_legacy_evidence(&db.connection);
    migrate_to(&mut db.connection, EVIDENCE_VERSION);

    // Fresh valid claim so any rejection is about evidence, not FK shape.
    mint_claim(&db.connection, "claim-req-missing", "attempt-work");
    let error = db
        .connection
        .execute(
            r#"
            INSERT INTO subtask_attempt_outcomes (
                claim_id, subtask_id, fence_seq, outcome_kind, evidence_digest,
                failure_code, summary, recorded_by_session, recorded_at
            ) VALUES ('claim-req-missing', 'attempt-work', 4, 'succeeded', NULL,
                      NULL, 'evidence-free finish on legacy task', 'session-worker', 11)
            "#,
            [],
        )
        .expect_err("required-policy legacy task must reject evidence-free outcomes");
    assert!(
        error.to_string().contains("attempt evidence"),
        "rejection must come from the required-evidence trigger: {error}"
    );

    // Positive control: the same claim shape with a valid digest succeeds.
    db.connection
        .execute(
            r#"
            INSERT INTO subtask_attempt_outcomes (
                claim_id, subtask_id, fence_seq, outcome_kind, evidence_digest,
                failure_code, summary, recorded_by_session, recorded_at
            ) VALUES ('claim-req-missing', 'attempt-work', 4, 'succeeded', 'blake3:ok',
                      NULL, 'governed finish', 'session-worker', 12)
            "#,
            [],
        )
        .expect("required-policy legacy task accepts a valid digest");
    assert_integrity(&db.connection);
}

#[test]
fn upgraded_binding_and_digest_checks_still_reject_invalid_outcomes() {
    let mut db = database_at(PRE_EVIDENCE_VERSION);
    seed_legacy_evidence(&db.connection);
    migrate_to(&mut db.connection, EVIDENCE_VERSION);

    // Claim references another claim's identity tuple (FK on the claim binding).
    mint_claim(&db.connection, "claim-binding", "attempt-work");
    let error = db
        .connection
        .execute(
            r#"
            INSERT INTO subtask_attempt_outcomes (
                claim_id, subtask_id, fence_seq, outcome_kind, evidence_digest,
                failure_code, summary, recorded_by_session, recorded_at
            ) VALUES ('claim-binding', 'idle-work', 4, 'succeeded', 'blake3:ok',
                      NULL, 'wrong binding', 'session-worker', 11)
            "#,
            [],
        )
        .expect_err("outcome must bind a claim with the same subtask/fence/owner");
    assert!(
        error.to_string().contains("FOREIGN KEY"),
        "binding rejection must be the claim foreign key: {error}"
    );

    // Malformed digest on otherwise valid rows remains rejected.
    mint_claim(&db.connection, "claim-malformed", "attempt-work");
    let error = db
        .connection
        .execute(
            r#"
            INSERT INTO subtask_attempt_outcomes (
                claim_id, subtask_id, fence_seq, outcome_kind, evidence_digest,
                failure_code, summary, recorded_by_session, recorded_at
            ) VALUES ('claim-malformed', 'attempt-work', 5, 'succeeded', 'bad digest',
                      NULL, 'malformed evidence', 'session-worker', 11)
            "#,
            [],
        )
        .expect_err("malformed evidence digests must stay rejected");
    assert!(
        error.to_string().contains("CHECK"),
        "digest rejection must be the evidence CHECK constraint: {error}"
    );
    assert_integrity(&db.connection);
}

#[test]
fn post_migration_optional_task_accepts_evidence_free_outcomes() {
    let mut db = database_at(PRE_EVIDENCE_VERSION);
    seed_legacy_evidence(&db.connection);
    migrate_to(&mut db.connection, EVIDENCE_VERSION);

    db.connection
        .execute(
            r#"
            INSERT INTO subtasks (
                subtask_id, meta_task_id, title, kind, review_target_subtask_id,
                review_target_artifact_digest, state, current_claim_id, artifact_digest,
                priority, completion_policy, routing_key, evidence_policy,
                created_at, updated_at
            ) VALUES ('optional-work', 'meta-legacy', 'optional task', 'work', NULL, NULL,
                      'available', NULL, NULL, 1, 'direct', 'hermes', 'optional', 10, 10)
            "#,
            [],
        )
        .expect("seed post-migration optional-policy task");
    db.connection
        .execute(
            "INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq) VALUES ('optional-work', 1)",
            [],
        )
        .expect("seed fence counter");
    mint_claim(&db.connection, "claim-optional", "optional-work");

    db.connection
        .execute(
            r#"
            INSERT INTO subtask_attempt_outcomes (
                claim_id, subtask_id, fence_seq, outcome_kind, evidence_digest,
                failure_code, summary, recorded_by_session, recorded_at
            ) VALUES ('claim-optional', 'optional-work', 1, 'succeeded', NULL,
                      NULL, 'evidence-free finish', 'session-worker', 11)
            "#,
            [],
        )
        .expect("optional-policy task records an evidence-free outcome");
    assert_integrity(&db.connection);
}

#[test]
fn omitted_policy_column_defaults_to_required_on_v36_schema() {
    let db = database_at(EVIDENCE_VERSION);
    db.connection
        .execute_batch(
            r#"
            INSERT INTO sessions (
                session_token, agent_principal_id, agent_instance_id, role, state,
                active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at
            ) VALUES ('session-orch', 'orch', 'orch-1', 'orchestrator', 'active', NULL, 1, 1, 1, 1);
            INSERT INTO meta_tasks (
                meta_task_id, prompt_text, state, created_by, created_at, updated_at
            ) VALUES ('meta-default', 'default', 'active', 'session-orch', 1, 1);
            INSERT INTO subtasks (
                subtask_id, meta_task_id, title, kind, review_target_subtask_id,
                review_target_artifact_digest, state, current_claim_id, artifact_digest,
                priority, completion_policy, routing_key, created_at, updated_at
            ) VALUES ('shaped-legacy', 'meta-default', 'shaped', 'work', NULL, NULL,
                      'available', NULL, NULL, 1, 'direct', 'hermes', 2, 2);
            "#,
        )
        .expect("insert subtask without evidence_policy");
    let policy: String = db
        .connection
        .query_row(
            "SELECT evidence_policy FROM subtasks WHERE subtask_id = 'shaped-legacy'",
            [],
            |row| row.get(0),
        )
        .expect("read column default");
    assert_eq!(
        policy, "required",
        "evidence_policy defaults to required when omitted"
    );
    assert_integrity(&db.connection);
}

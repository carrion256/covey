use std::{collections::BTreeSet, fs, path::PathBuf};

use rusqlite::{Connection, params};
use rusqlite_migration::{M, Migrations};
use tempfile::TempDir;

const PRE_POLICY_VERSION: usize = 33;
const POLICY_VERSION: usize = 34;

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
        paths.len() >= POLICY_VERSION,
        "migration 034 is missing from the migration directory"
    );
    assert_eq!(
        paths[POLICY_VERSION - 1]
            .file_name()
            .and_then(|value| value.to_str()),
        Some("034_task_completion_policies.sql")
    );
    paths
        .into_iter()
        .take(POLICY_VERSION)
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

fn seed_v33_graph(connection: &Connection) {
    connection
        .execute_batch(
            r#"
            INSERT INTO sessions (
                session_token, agent_principal_id, agent_instance_id, role, state,
                active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at
            ) VALUES
                ('session-orch', 'orch', 'orch-1', 'orchestrator', 'active', NULL, 1, 1, 1, 1),
                ('session-worker', 'worker', 'worker-1', 'executor', 'active', NULL, 1, 1, 1, 1),
                ('session-reviewer', 'reviewer', 'reviewer-1', 'reviewer', 'active', NULL, 1, 1, 1, 1);

            INSERT INTO meta_tasks (
                meta_task_id, prompt_text, state, created_by, created_at, updated_at
            ) VALUES ('meta-legacy', 'legacy graph', 'active', 'session-orch', 1, 1);

            INSERT INTO subtasks (
                subtask_id, meta_task_id, title, kind, review_target_subtask_id,
                review_target_artifact_digest, state, current_claim_id, artifact_digest,
                priority, created_at, updated_at
            ) VALUES
                ('legacy-open', 'meta-legacy', 'open', 'work', NULL, NULL,
                 'available', NULL, NULL, 1, 2, 2),
                ('legacy-applied', 'meta-legacy', 'applied', 'work', NULL, NULL,
                 'available', NULL, NULL, 2, 3, 3);

            INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq)
            VALUES ('legacy-open', 1), ('legacy-applied', 2);

            INSERT INTO claims (
                claim_id, subtask_id, owner_session_token, fence_seq, lease_deadline,
                state, created_at, updated_at
            ) VALUES (
                'claim-applied', 'legacy-applied', 'session-worker', 1, 100,
                'released', 4, 5
            );

            INSERT INTO artifacts (
                artifact_digest, artifact_kind, base_rev, produced_by_subtask_id,
                produced_by_session, manifest_path, changed_paths_digest, created_at
            ) VALUES (
                'blake3:legacy', 'patch_bundle', 'base', 'legacy-applied',
                'session-worker', 'legacy.json', 'blake3:paths', 5
            );

            UPDATE subtasks
            SET state = 'applied', artifact_digest = 'blake3:legacy', updated_at = 8
            WHERE subtask_id = 'legacy-applied';

            INSERT INTO subtasks (
                subtask_id, meta_task_id, title, kind, review_target_subtask_id,
                review_target_artifact_digest, state, current_claim_id, artifact_digest,
                priority, created_at, updated_at
            ) VALUES (
                'legacy-review', 'meta-legacy', 'review', 'review',
                'legacy-applied', 'blake3:legacy', 'decided', NULL, NULL, 1, 6, 7
            );
            INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq)
            VALUES ('legacy-review', 1);

            INSERT INTO reviews (
                review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id,
                verdict, findings_digest, state, created_at, updated_at
            ) VALUES (
                'review-legacy', 'legacy-applied', 'blake3:legacy', 'session-reviewer',
                'legacy-review', 'approve', 'blake3:findings', 'decided', 6, 7
            );

            INSERT INTO ready_queue (
                queue_id, artifact_digest, subtask_id, settlement_target, state,
                claimed_by_session_token, claim_fence_seq, claim_lease_deadline,
                enqueued_at, updated_at
            ) VALUES (
                'queue-legacy', 'blake3:legacy', 'legacy-applied', 'canonical', 'applied',
                NULL, 1, NULL, 7, 8
            );

            INSERT INTO subtask_dependencies (
                subtask_id, depends_on_subtask_id, source_ref, created_at
            ) VALUES ('legacy-open', 'legacy-applied', 'legacy:test', 8);

            INSERT INTO openspec_subtask_scope (
                subtask_id, openspec_change_id, openspec_task_id, source_path,
                scenario_refs_json, updated_at
            ) VALUES (
                'legacy-applied', 'legacy-change', '1.1',
                'openspec/changes/legacy/tasks.md', '[]', 8
            );

            INSERT INTO reservations (
                reservation_id, owner_subtask_id, scope_class, scope_key,
                lease_deadline, state, created_at, updated_at
            ) VALUES (
                'reservation-legacy', 'legacy-open', 'exact_path', 'src/legacy.rs',
                100, 'active', 8, 8
            );

            INSERT INTO mutation_idempotency (
                actor_key, operation, idempotency_key, request_hash, response_json, created_at
            ) VALUES ('session-worker', 'legacy-op', 'legacy-key', 'hash', '{}', 8);

            INSERT INTO event_log (
                event_type, object_type, object_id, actor_kind, session_token,
                payload_json, created_at
            ) VALUES (
                'artifact_published', 'artifact', 'blake3:legacy', 'session',
                'session-worker', '{}', 8
            );
            "#,
        )
        .expect("seed version 33 graph");
    assert_integrity(connection);
}

fn seed_policy_parents(connection: &Connection) {
    connection
        .execute_batch(
            r#"
            INSERT INTO sessions (
                session_token, agent_principal_id, agent_instance_id, role, state,
                active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at
            ) VALUES
                ('session-orch', 'orch', 'orch-1', 'orchestrator', 'active', NULL, 1, 1, 1, 1),
                ('session-worker', 'worker', 'worker-1', 'executor', 'active', NULL, 1, 1, 1, 1),
                ('session-other', 'other', 'other-1', 'executor', 'active', NULL, 1, 1, 1, 1);
            INSERT INTO meta_tasks (
                meta_task_id, prompt_text, state, created_by, created_at, updated_at
            ) VALUES ('meta-policy', 'policy checks', 'active', 'session-orch', 1, 1);
            "#,
        )
        .expect("seed policy parents");
}

fn insert_work(connection: &Connection, subtask_id: &str, policy: &str, routing_key: &str) {
    connection
        .execute(
            r#"
            INSERT INTO subtasks (
                subtask_id, meta_task_id, title, kind, review_target_subtask_id,
                review_target_artifact_digest, state, current_claim_id, artifact_digest,
                priority, completion_policy, routing_key, created_at, updated_at
            ) VALUES (?1, 'meta-policy', ?1, 'work', NULL, NULL, 'available', NULL, NULL,
                      1, ?2, ?3, 2, 2)
            "#,
            params![subtask_id, policy, routing_key],
        )
        .expect("insert work subtask");
    connection
        .execute(
            "INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq) VALUES (?1, 1)",
            params![subtask_id],
        )
        .expect("insert fence counter");
}

#[test]
fn migration_034_upgrades_v33_fail_closed_without_losing_lifecycle_graph() {
    let mut db = database_at(PRE_POLICY_VERSION);
    seed_v33_graph(&db.connection);

    migrate_to(&mut db.connection, POLICY_VERSION);

    let policies = db
        .connection
        .prepare(
            "SELECT subtask_id, completion_policy, routing_key, state FROM subtasks ORDER BY subtask_id",
        )
        .expect("prepare migrated subtask query")
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .expect("query migrated subtasks")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect migrated subtasks");
    assert_eq!(policies.len(), 3);
    assert!(
        policies
            .iter()
            .all(|(_, policy, routing, _)| policy == "canonical_apply" && routing == "mutai")
    );
    assert!(
        policies
            .iter()
            .any(|(id, _, _, state)| id == "legacy-applied" && state == "applied")
    );
    let claim_binding: (String, i64, String) = db
        .connection
        .query_row(
            "SELECT subtask_id, fence_seq, state FROM claims WHERE claim_id = 'claim-applied'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read preserved claim binding");
    assert_eq!(
        claim_binding,
        ("legacy-applied".to_owned(), 1, "released".to_owned())
    );
    let next_fence: i64 = db
        .connection
        .query_row(
            "SELECT next_fence_seq FROM subtask_fence_counter WHERE subtask_id = 'legacy-applied'",
            [],
            |row| row.get(0),
        )
        .expect("read preserved fence counter");
    assert_eq!(next_fence, 2);
    let queue_binding: (String, String, String) = db
        .connection
        .query_row(
            "SELECT subtask_id, artifact_digest, state FROM ready_queue WHERE queue_id = 'queue-legacy'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read preserved queue binding");
    assert_eq!(
        queue_binding,
        (
            "legacy-applied".to_owned(),
            "blake3:legacy".to_owned(),
            "applied".to_owned(),
        )
    );

    for (table, expected) in [
        ("claims", 1_i64),
        ("artifacts", 1),
        ("reviews", 1),
        ("ready_queue", 1),
        ("subtask_dependencies", 1),
        ("openspec_subtask_scope", 1),
        ("reservations", 1),
        ("mutation_idempotency", 1),
        ("event_log", 1),
    ] {
        let actual: i64 = db
            .connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count preserved rows");
        assert_eq!(actual, expected, "unexpected preserved count for {table}");
    }

    let expected_indexes = BTreeSet::from([
        "idx_subtasks_available_review_meta_priority".to_owned(),
        "idx_subtasks_available_review_priority".to_owned(),
        "idx_subtasks_available_work_meta_priority".to_owned(),
        "idx_subtasks_available_work_priority".to_owned(),
        "idx_subtasks_available_work_routing_meta_priority".to_owned(),
        "idx_subtasks_available_work_routing_priority".to_owned(),
        "idx_subtasks_meta_task_priority".to_owned(),
        "idx_subtasks_nonterminal_updated".to_owned(),
        "idx_subtasks_open_meta".to_owned(),
    ]);
    let actual_indexes = db
        .connection
        .prepare("SELECT name FROM pragma_index_list('subtasks') WHERE origin = 'c'")
        .expect("prepare index query")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query subtask indexes")
        .collect::<rusqlite::Result<BTreeSet<_>>>()
        .expect("collect subtask indexes");
    assert_eq!(actual_indexes, expected_indexes);

    assert_integrity(&db.connection);
    migrate_to(&mut db.connection, POLICY_VERSION);
    assert_integrity(&db.connection);
}

#[test]
fn migration_034_enforces_policy_routing_and_terminal_state_shapes() {
    let db = database_at(POLICY_VERSION);
    seed_policy_parents(&db.connection);

    db.connection
        .execute(
            r#"
            INSERT INTO subtasks (
                subtask_id, meta_task_id, title, kind, review_target_subtask_id,
                review_target_artifact_digest, state, current_claim_id, artifact_digest,
                priority, created_at, updated_at
            ) VALUES (
                'legacy-default', 'meta-policy', 'legacy default', 'work', NULL, NULL,
                'available', NULL, NULL, 1, 2, 2
            )
            "#,
            [],
        )
        .expect("legacy-shaped insert uses fail-closed defaults");
    let defaults: (String, String) = db
        .connection
        .query_row(
            "SELECT completion_policy, routing_key FROM subtasks WHERE subtask_id = 'legacy-default'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read defaults");
    assert_eq!(defaults, ("canonical_apply".to_owned(), "mutai".to_owned()));

    for (index, invalid) in [
        "".to_owned(),
        "has space".to_owned(),
        "has\ttab".to_owned(),
        "has\nnewline".to_owned(),
        "has\u{a0}nbsp".to_owned(),
        "has\u{2003}emspace".to_owned(),
        "x".repeat(257),
        "é".repeat(129),
    ]
    .into_iter()
    .enumerate()
    {
        let result = db.connection.execute(
            r#"
            INSERT INTO subtasks (
                subtask_id, meta_task_id, title, kind, review_target_subtask_id,
                review_target_artifact_digest, state, current_claim_id, artifact_digest,
                priority, completion_policy, routing_key, created_at, updated_at
            ) VALUES (?1, 'meta-policy', ?1, 'work', NULL, NULL, 'available', NULL, NULL,
                      1, 'direct', ?2, 2, 2)
            "#,
            params![format!("invalid-routing-{index}"), invalid],
        );
        assert!(result.is_err(), "invalid routing key was persisted");
    }

    insert_work(&db.connection, "unicode-routing", "direct", "hermés");
    assert!(
        db.connection
            .execute(
                "UPDATE subtasks SET routing_key = 'other' WHERE subtask_id = 'unicode-routing'",
                [],
            )
            .is_err(),
        "routing keys must be immutable"
    );
    assert!(
        db.connection
            .execute(
                "UPDATE subtasks SET completion_policy = 'reviewed' WHERE subtask_id = 'unicode-routing'",
                [],
            )
            .is_err(),
        "completion policies must be immutable"
    );

    assert!(
        db.connection
            .execute(
                r#"
                INSERT INTO subtasks (
                    subtask_id, meta_task_id, title, kind, state, priority,
                    completion_policy, routing_key, created_at, updated_at
                ) VALUES (
                    'unknown-policy', 'meta-policy', 'unknown', 'work', 'available', 1,
                    'unknown', 'hermes', 2, 2
                )
                "#,
                [],
            )
            .is_err()
    );
    assert!(
        db.connection
            .execute(
                r#"
                INSERT INTO subtasks (
                    subtask_id, meta_task_id, title, kind, state, priority,
                    completion_policy, routing_key, created_at, updated_at
                ) VALUES (
                    'canonical-completed', 'meta-policy', 'canonical completed', 'work',
                    'completed', 1, 'canonical_apply', 'mutai', 2, 2
                )
                "#,
                [],
            )
            .is_err(),
        "canonical work must not use non-settlement success"
    );
    assert!(
        db.connection
            .execute(
                r#"
                INSERT INTO subtasks (
                    subtask_id, meta_task_id, title, kind, state, priority,
                    completion_policy, routing_key, created_at, updated_at
                ) VALUES (
                    'reviewed-completed-no-artifact', 'meta-policy', 'reviewed completed', 'work',
                    'completed', 1, 'reviewed', 'hermes', 2, 2
                )
                "#,
                [],
            )
            .is_err(),
        "reviewed completion must retain its reviewed artifact"
    );
    assert!(
        db.connection
            .execute(
                r#"
                INSERT INTO subtasks (
                    subtask_id, meta_task_id, title, kind, state, priority,
                    completion_policy, routing_key, created_at, updated_at
                ) VALUES (
                    'direct-applied', 'meta-policy', 'direct applied', 'work',
                    'applied', 1, 'direct', 'hermes', 2, 2
                )
                "#,
                [],
            )
            .is_err(),
        "direct work must not acquire settlement success"
    );

    insert_work(&db.connection, "direct-completed", "direct", "hermes");
    db.connection
        .execute(
            "UPDATE subtasks SET state = 'completed' WHERE subtask_id = 'direct-completed'",
            [],
        )
        .expect("direct work may use non-settlement completion");
    insert_work(&db.connection, "direct-failed", "direct", "hermes");
    db.connection
        .execute(
            "UPDATE subtasks SET state = 'failed' WHERE subtask_id = 'direct-failed'",
            [],
        )
        .expect("work may record terminal failure without settlement evidence");
    insert_work(&db.connection, "reviewed-completed", "reviewed", "hermes");
    db.connection
        .execute(
            r#"
            INSERT INTO artifacts (
                artifact_digest, artifact_kind, base_rev, produced_by_subtask_id,
                produced_by_session, manifest_path, changed_paths_digest, created_at
            ) VALUES (
                'blake3:reviewed', 'findings_bundle', 'base', 'reviewed-completed',
                'session-worker', 'reviewed.json', 'blake3:paths', 3
            )
            "#,
            [],
        )
        .expect("insert reviewed artifact");
    db.connection
        .execute(
            r#"
            UPDATE subtasks
            SET state = 'completed', artifact_digest = 'blake3:reviewed'
            WHERE subtask_id = 'reviewed-completed'
            "#,
            [],
        )
        .expect("reviewed work may complete with retained artifact binding");

    assert_integrity(&db.connection);
}

#[test]
fn migration_034_binds_attempt_outcomes_to_claims_and_makes_them_append_only() {
    let db = database_at(POLICY_VERSION);
    seed_policy_parents(&db.connection);
    insert_work(&db.connection, "attempt-work", "direct", "hermes");
    insert_work(&db.connection, "other-work", "direct", "hermes");
    db.connection
        .execute_batch(
            r#"
            INSERT INTO claims (
                claim_id, subtask_id, owner_session_token, fence_seq, lease_deadline,
                state, created_at, updated_at
            ) VALUES (
                'claim-attempt', 'attempt-work', 'session-worker', 1, 100,
                'held', 3, 3
            );
            UPDATE subtasks
            SET state = 'in_progress', current_claim_id = 'claim-attempt', updated_at = 3
            WHERE subtask_id = 'attempt-work';
            "#,
        )
        .expect("seed live attempt");
    assert!(
        db.connection
            .execute(
                "UPDATE subtasks SET state = 'completed' WHERE subtask_id = 'attempt-work'",
                [],
            )
            .is_err(),
        "terminal work must not retain its active claim"
    );

    for invalid_insert in [
        "INSERT INTO subtask_attempt_outcomes VALUES ('claim-attempt', 'other-work', 1, 'succeeded', 'blake3:ok', NULL, 'done', 'session-worker', 4)",
        "INSERT INTO subtask_attempt_outcomes VALUES ('claim-attempt', 'attempt-work', 2, 'succeeded', 'blake3:ok', NULL, 'done', 'session-worker', 4)",
        "INSERT INTO subtask_attempt_outcomes VALUES ('claim-attempt', 'attempt-work', 1, 'succeeded', 'blake3:ok', NULL, 'done', 'session-other', 4)",
        "INSERT INTO subtask_attempt_outcomes VALUES ('claim-attempt', 'attempt-work', 1, 'succeeded', 'missing-prefix', NULL, 'done', 'session-worker', 4)",
        "INSERT INTO subtask_attempt_outcomes VALUES ('claim-attempt', 'attempt-work', 1, 'succeeded', 'blake3:bad value', NULL, 'done', 'session-worker', 4)",
        "INSERT INTO subtask_attempt_outcomes VALUES ('claim-attempt', 'attempt-work', 1, 'succeeded', 'blake3:ok', 'unexpected', 'done', 'session-worker', 4)",
        "INSERT INTO subtask_attempt_outcomes VALUES ('claim-attempt', 'attempt-work', 1, 'terminal_failure', 'blake3:ok', NULL, 'failed', 'session-worker', 4)",
        "INSERT INTO subtask_attempt_outcomes VALUES ('claim-attempt', 'attempt-work', 1, 'terminal_failure', 'blake3:ok', 'bad code', 'failed', 'session-worker', 4)",
        "INSERT INTO subtask_attempt_outcomes VALUES ('claim-attempt', 'attempt-work', 1, 'succeeded', 'blake3:ok', NULL, ' leading', 'session-worker', 4)",
        "INSERT INTO subtask_attempt_outcomes VALUES ('claim-attempt', 'attempt-work', 1, 'succeeded', 'blake3:ok', NULL, 'line' || char(10) || 'break', 'session-worker', 4)",
    ] {
        assert!(
            db.connection.execute(invalid_insert, []).is_err(),
            "invalid attempt outcome was persisted: {invalid_insert}"
        );
    }

    db.connection
        .execute(
            r#"
            INSERT INTO subtask_attempt_outcomes (
                claim_id, subtask_id, fence_seq, outcome_kind, evidence_digest,
                failure_code, summary, recorded_by_session, recorded_at
            ) VALUES (
                'claim-attempt', 'attempt-work', 1, 'succeeded', 'blake3:ok',
                NULL, 'completed directly', 'session-worker', 4
            )
            "#,
            [],
        )
        .expect("insert bound attempt outcome");
    db.connection
        .execute_batch(
            r#"
            INSERT INTO claims (
                claim_id, subtask_id, owner_session_token, fence_seq, lease_deadline,
                state, created_at, updated_at
            ) VALUES (
                'claim-failure', 'other-work', 'session-other', 1, 100,
                'held', 4, 4
            );
            UPDATE subtasks
            SET state = 'in_progress', current_claim_id = 'claim-failure', updated_at = 4
            WHERE subtask_id = 'other-work';
            INSERT INTO subtask_attempt_outcomes (
                claim_id, subtask_id, fence_seq, outcome_kind, evidence_digest,
                failure_code, summary, recorded_by_session, recorded_at
            ) VALUES (
                'claim-failure', 'other-work', 1, 'terminal_failure', 'blake3:failure',
                'provider_error', 'provider returned an error', 'session-other', 5
            );
            "#,
        )
        .expect("insert bound failure outcome");

    assert!(
        db.connection
            .execute(
                "UPDATE subtask_attempt_outcomes SET summary = 'rewritten' WHERE claim_id = 'claim-attempt'",
                [],
            )
            .is_err(),
        "attempt outcomes must reject updates"
    );
    assert!(
        db.connection
            .execute(
                "DELETE FROM subtask_attempt_outcomes WHERE claim_id = 'claim-attempt'",
                [],
            )
            .is_err(),
        "attempt outcomes must reject deletion"
    );
    assert!(
        db.connection
            .execute(
                r#"
                INSERT INTO subtask_attempt_outcomes (
                    claim_id, subtask_id, fence_seq, outcome_kind, evidence_digest,
                    failure_code, summary, recorded_by_session, recorded_at
                ) VALUES (
                    'claim-attempt', 'attempt-work', 1, 'terminal_failure', 'blake3:second',
                    'failed', 'second outcome', 'session-worker', 5
                )
                "#,
                [],
            )
            .is_err(),
        "one claim/fence must not acquire two outcomes"
    );

    let outcome: (String, String, i64, String) = db
        .connection
        .query_row(
            r#"
            SELECT subtask_id, outcome_kind, fence_seq, summary
            FROM subtask_attempt_outcomes
            WHERE claim_id = 'claim-attempt'
            "#,
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("read immutable outcome");
    assert_eq!(
        outcome,
        (
            "attempt-work".to_owned(),
            "succeeded".to_owned(),
            1,
            "completed directly".to_owned(),
        )
    );
    let outcome_count: i64 = db
        .connection
        .query_row("SELECT COUNT(*) FROM subtask_attempt_outcomes", [], |row| {
            row.get(0)
        })
        .expect("count accepted outcomes");
    assert_eq!(outcome_count, 2);
    assert_integrity(&db.connection);
}

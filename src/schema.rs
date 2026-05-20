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

pub(crate) fn apply_migrations(conn: &mut Connection) -> Result<()> {
    Migrations::new(vec![
        M::up(
            r#"
        CREATE TABLE IF NOT EXISTS sessions (
            session_token TEXT PRIMARY KEY,
            agent_principal_id TEXT NOT NULL,
            agent_instance_id TEXT NOT NULL,
            role TEXT NOT NULL,
            state TEXT NOT NULL,
            active_subtask_id TEXT,
            last_heartbeat_at INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            CHECK (role IN ('executor', 'orchestrator', 'apply_gate', 'reviewer')),
            CHECK (state IN ('active', 'stale', 'exited'))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_one_active_per_principal
        ON sessions(agent_principal_id)
        WHERE state = 'active';

        CREATE TABLE IF NOT EXISTS meta_tasks (
            meta_task_id TEXT PRIMARY KEY,
            prompt_text TEXT NOT NULL,
            state TEXT NOT NULL,
            created_by TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            CHECK (state IN ('planning', 'active', 'completed', 'cancelled')),
            FOREIGN KEY(created_by) REFERENCES sessions(session_token)
        );

        CREATE TABLE IF NOT EXISTS subtasks (
            subtask_id TEXT PRIMARY KEY,
            meta_task_id TEXT NOT NULL REFERENCES meta_tasks(meta_task_id),
            title TEXT NOT NULL,
            kind TEXT NOT NULL,
            review_target_subtask_id TEXT,
            review_target_artifact_digest TEXT,
            state TEXT NOT NULL,
            current_claim_id TEXT,
            artifact_digest TEXT,
            priority INTEGER NOT NULL DEFAULT 100,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            CHECK (kind IN ('work', 'review')),
            CHECK (priority BETWEEN 0 AND 1000),
            CHECK (state IN ('available', 'claimed', 'in_progress', 'artifact_published',
                             'review_pending', 'changes_requested', 'approved', 'decided',
                             'ready_for_apply', 'applied', 'abandoned')),
            CHECK ((kind = 'review') = (review_target_subtask_id IS NOT NULL))
        );

        CREATE TABLE IF NOT EXISTS subtask_fence_counter (
            subtask_id TEXT PRIMARY KEY REFERENCES subtasks(subtask_id),
            next_fence_seq INTEGER NOT NULL DEFAULT 1
        );

        CREATE TABLE IF NOT EXISTS claims (
            claim_id TEXT PRIMARY KEY,
            subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
            owner_session_token TEXT NOT NULL REFERENCES sessions(session_token),
            fence_seq INTEGER NOT NULL,
            lease_deadline INTEGER NOT NULL,
            state TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            CHECK (state IN ('held', 'released', 'expired', 'revoked'))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_claims_one_held_per_subtask
        ON claims(subtask_id)
        WHERE state = 'held';

        CREATE TABLE IF NOT EXISTS artifacts (
            artifact_digest TEXT PRIMARY KEY,
            artifact_kind TEXT NOT NULL,
            base_rev TEXT NOT NULL,
            produced_by_subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
            produced_by_session TEXT NOT NULL REFERENCES sessions(session_token),
            manifest_path TEXT NOT NULL,
            changed_paths_digest TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            CHECK (artifact_kind IN ('patch_bundle', 'isolated_commit_ref', 'tree_bundle',
                                     'findings_bundle', 'verification_bundle'))
        );

        CREATE TABLE IF NOT EXISTS reviews (
            review_id TEXT PRIMARY KEY,
            subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
            artifact_digest TEXT NOT NULL REFERENCES artifacts(artifact_digest),
            reviewer_session TEXT NOT NULL REFERENCES sessions(session_token),
            review_subtask_id TEXT REFERENCES subtasks(subtask_id),
            verdict TEXT,
            findings_digest TEXT,
            state TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            CHECK (state IN ('requested', 'in_progress', 'decided', 'superseded')),
            CHECK ((state = 'decided') = (verdict IS NOT NULL))
        );

        CREATE TABLE IF NOT EXISTS reservations (
            reservation_id TEXT PRIMARY KEY,
            owner_subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
            scope_class TEXT NOT NULL,
            scope_key TEXT NOT NULL,
            lease_deadline INTEGER NOT NULL,
            state TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            CHECK (scope_class IN ('exact_path', 'subtree', 'repo_global', 'generated_set')),
            CHECK (state IN ('active', 'released', 'expired'))
        );

        CREATE TABLE IF NOT EXISTS reservation_generated_members (
            reservation_id TEXT PRIMARY KEY REFERENCES reservations(reservation_id),
            members_json TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ready_queue (
            queue_id TEXT PRIMARY KEY,
            artifact_digest TEXT NOT NULL REFERENCES artifacts(artifact_digest),
            subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
            settlement_target TEXT NOT NULL,
            state TEXT NOT NULL,
            claimed_by_session_token TEXT REFERENCES sessions(session_token),
            claim_fence_seq INTEGER,
            claim_lease_deadline INTEGER,
            enqueued_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            CHECK (settlement_target IN ('canonical')),
            CHECK (state IN ('queued', 'in_flight', 'applied', 'superseded', 'cancelled')),
            CHECK (
                (state = 'in_flight'
                    AND claimed_by_session_token IS NOT NULL
                    AND claim_fence_seq IS NOT NULL
                    AND claim_lease_deadline IS NOT NULL)
                OR
                (state != 'in_flight'
                    AND claimed_by_session_token IS NULL
                    AND claim_lease_deadline IS NULL)
            )
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_ready_queue_one_active_per_subtask
        ON ready_queue(subtask_id)
        WHERE state IN ('queued', 'in_flight');

        CREATE TABLE IF NOT EXISTS event_log (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            event_type TEXT NOT NULL,
            object_type TEXT NOT NULL,
            object_id TEXT NOT NULL,
            session_token TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS conflicts (
            conflict_id TEXT PRIMARY KEY,
            object_type TEXT NOT NULL,
            object_id TEXT NOT NULL,
            conflict_kind TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            detected_at INTEGER NOT NULL,
            resolution_state TEXT NOT NULL,
            CHECK (resolution_state IN ('open', 'acknowledged', 'resolved'))
        );
        "#,
        ),
        M::up(
            r#"
        ALTER TABLE event_log ADD COLUMN actor_kind TEXT NOT NULL DEFAULT 'session';
        CREATE INDEX IF NOT EXISTS idx_reservations_state_scope
        ON reservations(state, scope_class, scope_key);
        CREATE INDEX IF NOT EXISTS idx_subtasks_meta_task_priority
        ON subtasks(meta_task_id, priority, created_at);
        CREATE INDEX IF NOT EXISTS idx_ready_queue_state_enqueued
        ON ready_queue(state, enqueued_at);
        "#,
        ),
        M::up(
            r#"
        CREATE TABLE IF NOT EXISTS lease_clock (
            clock_id INTEGER PRIMARY KEY CHECK (clock_id = 1),
            last_tick_ms INTEGER NOT NULL
        );
        INSERT INTO lease_clock (clock_id, last_tick_ms)
        VALUES (1, 0)
        ON CONFLICT(clock_id) DO NOTHING;

        ALTER TABLE sessions ADD COLUMN last_heartbeat_tick INTEGER NOT NULL DEFAULT 0;
        UPDATE sessions
        SET last_heartbeat_tick = CASE
            WHEN last_heartbeat_at > 0 THEN last_heartbeat_at
            ELSE 0
        END
        WHERE last_heartbeat_tick = 0;

        ALTER TABLE reservation_generated_members RENAME TO reservation_generated_members_previous;
        CREATE TABLE reservation_generated_members (
            reservation_id TEXT NOT NULL REFERENCES reservations(reservation_id),
            member_path TEXT NOT NULL,
            PRIMARY KEY (reservation_id, member_path)
        );
        INSERT INTO reservation_generated_members (reservation_id, member_path)
        SELECT reservation_generated_members_previous.reservation_id, json_each.value
        FROM reservation_generated_members_previous
        JOIN json_each(reservation_generated_members_previous.members_json);
        DROP TABLE reservation_generated_members_previous;

        CREATE INDEX IF NOT EXISTS idx_reservation_generated_members_member_path
        ON reservation_generated_members(member_path, reservation_id);
        CREATE INDEX IF NOT EXISTS idx_sessions_state_heartbeat_tick
        ON sessions(state, last_heartbeat_tick);
        CREATE TABLE IF NOT EXISTS mutation_idempotency (
            actor_key TEXT NOT NULL,
            operation TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            request_hash TEXT NOT NULL,
            response_json TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (actor_key, operation, idempotency_key)
        );
        "#,
        ),
        M::up(
            r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_reviews_one_open_round_per_artifact
        ON reviews(subtask_id, artifact_digest)
        WHERE state IN ('requested', 'in_progress');
        CREATE UNIQUE INDEX IF NOT EXISTS idx_reviews_one_row_per_review_subtask
        ON reviews(review_subtask_id)
        WHERE review_subtask_id IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_claims_state_deadline
        ON claims(state, lease_deadline);
        CREATE INDEX IF NOT EXISTS idx_reservations_state_deadline
        ON reservations(state, lease_deadline);
        CREATE INDEX IF NOT EXISTS idx_conflicts_resolution_detected
        ON conflicts(resolution_state, detected_at DESC);
        "#,
        ),
        M::up(
            r#"
        PRAGMA foreign_keys = OFF;

        CREATE TABLE sessions_new (
            session_token TEXT PRIMARY KEY,
            agent_principal_id TEXT NOT NULL,
            agent_instance_id TEXT NOT NULL,
            role TEXT NOT NULL,
            state TEXT NOT NULL,
            active_subtask_id TEXT REFERENCES subtasks(subtask_id),
            last_heartbeat_at INTEGER NOT NULL,
            last_heartbeat_tick INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            CHECK (role IN ('executor', 'orchestrator', 'apply_gate', 'reviewer')),
            CHECK (state IN ('active', 'stale', 'exited'))
        );
        INSERT INTO sessions_new (
            session_token, agent_principal_id, agent_instance_id, role, state,
            active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at
        )
        SELECT
            session_token, agent_principal_id, agent_instance_id, role, state,
            active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at
        FROM sessions;
        DROP TABLE sessions;
        ALTER TABLE sessions_new RENAME TO sessions;
        CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_one_active_per_principal
        ON sessions(agent_principal_id)
        WHERE state = 'active';
        CREATE INDEX IF NOT EXISTS idx_sessions_state_heartbeat_tick
        ON sessions(state, last_heartbeat_tick);

        CREATE TABLE subtasks_new (
            subtask_id TEXT PRIMARY KEY,
            meta_task_id TEXT NOT NULL REFERENCES meta_tasks(meta_task_id),
            title TEXT NOT NULL,
            kind TEXT NOT NULL,
            review_target_subtask_id TEXT,
            review_target_artifact_digest TEXT,
            state TEXT NOT NULL,
            current_claim_id TEXT REFERENCES claims(claim_id),
            artifact_digest TEXT REFERENCES artifacts(artifact_digest),
            priority INTEGER NOT NULL DEFAULT 100,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            CHECK (kind IN ('work', 'review')),
            CHECK (priority BETWEEN 0 AND 1000),
            CHECK (state IN ('available', 'claimed', 'in_progress', 'artifact_published',
                             'review_pending', 'changes_requested', 'approved', 'decided',
                             'ready_for_apply', 'applied', 'abandoned')),
            CHECK (
                (kind = 'review' AND review_target_subtask_id IS NOT NULL AND review_target_artifact_digest IS NOT NULL)
                OR
                (kind = 'work' AND review_target_subtask_id IS NULL AND review_target_artifact_digest IS NULL)
            )
        );
        INSERT INTO subtasks_new (
            subtask_id, meta_task_id, title, kind, review_target_subtask_id,
            review_target_artifact_digest, state, current_claim_id, artifact_digest,
            priority, created_at, updated_at
        )
        SELECT
            subtask_id, meta_task_id, title, kind, review_target_subtask_id,
            review_target_artifact_digest, state, current_claim_id, artifact_digest,
            priority, created_at, updated_at
        FROM subtasks;
        DROP TABLE subtasks;
        ALTER TABLE subtasks_new RENAME TO subtasks;
        CREATE INDEX IF NOT EXISTS idx_subtasks_meta_task_priority
        ON subtasks(meta_task_id, priority, created_at);

        PRAGMA foreign_keys = ON;
        "#,
        ),
        M::up(
            r#"
        PRAGMA foreign_keys = OFF;

        CREATE TABLE meta_tasks_new (
            meta_task_id TEXT PRIMARY KEY,
            prompt_text TEXT NOT NULL,
            state TEXT NOT NULL,
            created_by TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            CHECK (state IN ('planning', 'active', 'completed', 'cancelled')),
            FOREIGN KEY(created_by) REFERENCES sessions(session_token)
        );
        INSERT INTO meta_tasks_new (
            meta_task_id, prompt_text, state, created_by, created_at, updated_at
        )
        SELECT
            meta_task_id,
            prompt_text,
            CASE WHEN state = 'draining' THEN 'active' ELSE state END,
            created_by,
            created_at,
            updated_at
        FROM meta_tasks;
        DROP TABLE meta_tasks;
        ALTER TABLE meta_tasks_new RENAME TO meta_tasks;

        PRAGMA foreign_keys = ON;
        "#,
        ),
        M::up(
            r#"
        PRAGMA foreign_keys = OFF;

        CREATE TABLE ready_queue_new (
            queue_id TEXT PRIMARY KEY,
            artifact_digest TEXT NOT NULL REFERENCES artifacts(artifact_digest),
            subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
            settlement_target TEXT NOT NULL,
            state TEXT NOT NULL,
            claimed_by_session_token TEXT REFERENCES sessions(session_token),
            claim_fence_seq INTEGER,
            claim_lease_deadline INTEGER,
            enqueued_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            CHECK (settlement_target IN ('canonical')),
            CHECK (state IN ('queued', 'in_flight', 'applied', 'superseded', 'cancelled')),
            CHECK (
                (state = 'in_flight'
                    AND claimed_by_session_token IS NOT NULL
                    AND claim_fence_seq IS NOT NULL
                    AND claim_lease_deadline IS NOT NULL)
                OR
                (state != 'in_flight'
                    AND claimed_by_session_token IS NULL
                    AND claim_lease_deadline IS NULL)
            )
        );
        INSERT INTO ready_queue_new (
            queue_id, artifact_digest, subtask_id, settlement_target, state,
            claimed_by_session_token, claim_fence_seq, claim_lease_deadline,
            enqueued_at, updated_at
        )
        SELECT
            queue_id,
            artifact_digest,
            subtask_id,
            settlement_target,
            CASE WHEN state = 'in_flight' THEN 'queued' ELSE state END,
            NULL,
            NULL,
            NULL,
            enqueued_at,
            updated_at
        FROM ready_queue;
        DROP TABLE ready_queue;
        ALTER TABLE ready_queue_new RENAME TO ready_queue;
        CREATE UNIQUE INDEX IF NOT EXISTS idx_ready_queue_one_active_per_subtask
        ON ready_queue(subtask_id)
        WHERE state IN ('queued', 'in_flight');
        CREATE INDEX IF NOT EXISTS idx_ready_queue_state_enqueued
        ON ready_queue(state, enqueued_at);
        CREATE INDEX IF NOT EXISTS idx_ready_queue_state_claim_deadline
        ON ready_queue(state, claim_lease_deadline, enqueued_at);

        PRAGMA foreign_keys = ON;
        "#,
        ),
        M::up(
            r#"
        CREATE TABLE IF NOT EXISTS import_provenance (
            object_type TEXT NOT NULL,
            object_id TEXT NOT NULL,
            planning_format TEXT NOT NULL,
            openspec_change_id TEXT NOT NULL,
            openspec_change_path TEXT NOT NULL,
            openspec_task_id TEXT,
            proposal_digest TEXT,
            design_digest TEXT,
            tasks_digest TEXT NOT NULL,
            spec_digests_json TEXT NOT NULL,
            task_digest TEXT,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (object_type, object_id),
            CHECK (object_type IN ('meta_task', 'subtask')),
            CHECK (planning_format = 'openspec')
        );
        CREATE INDEX IF NOT EXISTS idx_import_provenance_openspec_change
        ON import_provenance(planning_format, openspec_change_id, openspec_task_id);
        "#,
        ),
        M::up(
            r#"
        ALTER TABLE import_provenance
        ADD COLUMN source_digests_json TEXT NOT NULL DEFAULT '[]';
        ALTER TABLE import_provenance
        ADD COLUMN mission_artifact_digests_json TEXT NOT NULL DEFAULT '[]';
        ALTER TABLE import_provenance
        ADD COLUMN mission_artifacts_json TEXT NOT NULL DEFAULT '[]';
        "#,
        ),
        M::up(
            r#"
        CREATE TABLE IF NOT EXISTS apply_verifications (
            queue_id TEXT NOT NULL REFERENCES ready_queue(queue_id),
            artifact_digest TEXT NOT NULL REFERENCES artifacts(artifact_digest),
            review_id TEXT NOT NULL REFERENCES reviews(review_id),
            findings_digest TEXT NOT NULL,
            claim_fence_seq INTEGER NOT NULL,
            verifier TEXT NOT NULL,
            verdict_digest TEXT NOT NULL,
            seal_digest TEXT NOT NULL,
            recorded_by_session TEXT NOT NULL REFERENCES sessions(session_token),
            created_at INTEGER NOT NULL,
            PRIMARY KEY (queue_id, claim_fence_seq, seal_digest)
        );
        CREATE INDEX IF NOT EXISTS idx_apply_verifications_lookup
        ON apply_verifications(queue_id, artifact_digest, review_id, findings_digest, claim_fence_seq);
        "#,
        ),
        M::up(
            r#"
        CREATE TABLE IF NOT EXISTS runtime_attestations (
            session_token TEXT PRIMARY KEY REFERENCES sessions(session_token),
            agent_principal_id TEXT NOT NULL,
            agent_instance_id TEXT NOT NULL,
            role TEXT NOT NULL,
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            process_id TEXT,
            container_id TEXT,
            command_transcript_digest TEXT NOT NULL,
            started_at INTEGER NOT NULL,
            ended_at INTEGER NOT NULL,
            recorded_at INTEGER NOT NULL,
            CHECK (role IN ('executor', 'orchestrator', 'apply_gate', 'reviewer')),
            CHECK (TRIM(agent_principal_id) != ''),
            CHECK (TRIM(agent_instance_id) != ''),
            CHECK (TRIM(provider) != ''),
            CHECK (TRIM(model) != ''),
            CHECK (TRIM(command_transcript_digest) != ''),
            CHECK (
                (process_id IS NOT NULL AND TRIM(process_id) != '')
                OR
                (container_id IS NOT NULL AND TRIM(container_id) != '')
            ),
            CHECK (ended_at >= started_at)
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_attestations_principal_role
        ON runtime_attestations(agent_principal_id, role);
        "#,
        ),
        M::up(
            r#"
        ALTER TABLE runtime_attestations
        ADD COLUMN provider_run_id TEXT NOT NULL DEFAULT '__covey_missing_provider_run_id__';
        ALTER TABLE runtime_attestations
        ADD COLUMN provider_run_id_issuer TEXT NOT NULL DEFAULT '__covey_missing_provider_run_id_issuer__';
        CREATE INDEX IF NOT EXISTS idx_runtime_attestations_provider_run
        ON runtime_attestations(provider_run_id_issuer, provider_run_id);
        "#,
        ),
        M::up(
            r#"
        CREATE TABLE IF NOT EXISTS subtask_dependencies (
            subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id) ON DELETE CASCADE,
            depends_on_subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id) ON DELETE CASCADE,
            source_ref TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (subtask_id, depends_on_subtask_id)
        );
        CREATE INDEX IF NOT EXISTS idx_subtask_dependencies_depends_on
        ON subtask_dependencies(depends_on_subtask_id, subtask_id);
        "#,
        ),
    ])
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

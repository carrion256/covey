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

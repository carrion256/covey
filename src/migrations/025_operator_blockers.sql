CREATE TABLE IF NOT EXISTS operator_blockers (
    blocker_id TEXT PRIMARY KEY,
    openspec_change_id TEXT NOT NULL,
    target_kind TEXT NOT NULL,
    subtask_id TEXT REFERENCES subtasks(subtask_id),
    queue_id TEXT REFERENCES ready_queue(queue_id),
    artifact_digest TEXT REFERENCES artifacts(artifact_digest),
    reason TEXT NOT NULL,
    source_evidence_id TEXT,
    recorded_by_session TEXT NOT NULL REFERENCES sessions(session_token),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (target_kind IN ('subtask', 'ready_queue')),
    CHECK (
        (target_kind = 'subtask'
            AND subtask_id IS NOT NULL
            AND queue_id IS NULL
            AND artifact_digest IS NULL)
        OR
        (target_kind = 'ready_queue'
            AND subtask_id IS NOT NULL
            AND queue_id IS NOT NULL
            AND artifact_digest IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_operator_blockers_openspec_updated
ON operator_blockers(openspec_change_id, updated_at);

CREATE INDEX IF NOT EXISTS idx_operator_blockers_subtask
ON operator_blockers(subtask_id);

CREATE INDEX IF NOT EXISTS idx_operator_blockers_queue
ON operator_blockers(queue_id);

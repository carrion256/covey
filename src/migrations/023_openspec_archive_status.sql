CREATE TABLE IF NOT EXISTS openspec_archive_status (
    queue_id TEXT PRIMARY KEY REFERENCES ready_queue(queue_id),
    subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
    artifact_digest TEXT NOT NULL REFERENCES artifacts(artifact_digest),
    openspec_change_id TEXT NOT NULL,
    state TEXT NOT NULL,
    blocked_reason TEXT,
    archive_proof_digest TEXT,
    recorded_by_session TEXT NOT NULL REFERENCES sessions(session_token),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (state IN ('blocked', 'archived')),
    CHECK (
        (state = 'blocked'
            AND blocked_reason IS NOT NULL
            AND archive_proof_digest IS NULL)
        OR
        (state = 'archived'
            AND blocked_reason IS NULL
            AND archive_proof_digest IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_openspec_archive_status_state_updated
ON openspec_archive_status(state, updated_at);

CREATE INDEX IF NOT EXISTS idx_openspec_archive_status_subtask
ON openspec_archive_status(subtask_id);

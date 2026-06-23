CREATE TABLE IF NOT EXISTS apply_worktrees (
    path TEXT PRIMARY KEY NOT NULL,
    queue_id TEXT NOT NULL REFERENCES ready_queue(queue_id) ON DELETE RESTRICT,
    artifact_digest TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN (
        'active',
        'applied',
        'archived',
        'retained_evidence',
        'cleanup_allowed'
    )),
    recorded_by_session TEXT NOT NULL REFERENCES sessions(session_token) ON DELETE RESTRICT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_apply_worktrees_queue_state
ON apply_worktrees(queue_id, state, updated_at);

CREATE INDEX IF NOT EXISTS idx_apply_worktrees_state_created
ON apply_worktrees(state, created_at);

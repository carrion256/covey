CREATE TABLE IF NOT EXISTS vcs_workspaces (
    workspace_id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN (
        'packet',
        'claim',
        'apply',
        'execution'
    )),
    path TEXT NOT NULL UNIQUE,
    jj_workspace_name TEXT,
    claim_id TEXT REFERENCES claims(claim_id) ON DELETE RESTRICT,
    subtask_id TEXT REFERENCES subtasks(subtask_id) ON DELETE RESTRICT,
    openspec_change_id TEXT,
    queue_id TEXT REFERENCES ready_queue(queue_id) ON DELETE RESTRICT,
    artifact_digest TEXT,
    current_bookmark TEXT,
    current_change_id TEXT,
    current_commit_id TEXT,
    state TEXT NOT NULL CHECK (state IN (
        'active',
        'retained',
        'cleanup_allowed',
        'archived'
    )),
    last_cleanliness TEXT NOT NULL CHECK (last_cleanliness IN (
        'unknown',
        'clean',
        'dirty',
        'missing',
        'stale',
        'unusable'
    )),
    last_observed_reason TEXT,
    recorded_by_session TEXT NOT NULL REFERENCES sessions(session_token) ON DELETE RESTRICT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_observed_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_vcs_workspaces_kind_state
ON vcs_workspaces(kind, state, updated_at);

CREATE INDEX IF NOT EXISTS idx_vcs_workspaces_claim
ON vcs_workspaces(claim_id, kind, state);

CREATE INDEX IF NOT EXISTS idx_vcs_workspaces_openspec
ON vcs_workspaces(openspec_change_id, kind, state);

CREATE INDEX IF NOT EXISTS idx_vcs_workspaces_queue
ON vcs_workspaces(queue_id, kind, state);

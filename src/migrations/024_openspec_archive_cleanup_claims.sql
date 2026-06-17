PRAGMA foreign_keys = OFF;

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
    CHECK (kind IN ('work', 'review', 'cleanup')),
    CHECK (priority BETWEEN 0 AND 1000),
    CHECK (state IN ('available', 'blocked', 'claimed', 'in_progress', 'artifact_published',
                     'review_pending', 'changes_requested', 'approved', 'decided',
                     'ready_for_apply', 'applied', 'abandoned')),
    CHECK (state NOT IN ('blocked', 'changes_requested') OR (kind = 'work' AND artifact_digest IS NOT NULL)),
    CHECK (
        (kind = 'review' AND review_target_subtask_id IS NOT NULL AND review_target_artifact_digest IS NOT NULL)
        OR
        (kind IN ('work', 'cleanup') AND review_target_subtask_id IS NULL AND review_target_artifact_digest IS NULL)
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

CREATE INDEX IF NOT EXISTS idx_subtasks_available_review_priority
ON subtasks(priority, created_at)
WHERE kind = 'review' AND state = 'available';

CREATE INDEX IF NOT EXISTS idx_subtasks_available_review_meta_priority
ON subtasks(meta_task_id, priority, created_at)
WHERE kind = 'review' AND state = 'available';

CREATE INDEX IF NOT EXISTS idx_subtasks_available_work_priority
ON subtasks(priority, created_at)
WHERE kind = 'work' AND state = 'available';

CREATE INDEX IF NOT EXISTS idx_subtasks_available_work_meta_priority
ON subtasks(meta_task_id, priority, created_at)
WHERE kind = 'work' AND state = 'available';

CREATE INDEX IF NOT EXISTS idx_subtasks_nonterminal_updated
ON subtasks(updated_at)
WHERE state NOT IN ('available', 'applied', 'abandoned', 'decided');

CREATE INDEX IF NOT EXISTS idx_subtasks_open_meta
ON subtasks(meta_task_id)
WHERE state NOT IN ('applied', 'abandoned', 'decided');

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS openspec_archive_cleanup_claims (
    openspec_change_id TEXT PRIMARY KEY,
    cleanup_subtask_id TEXT NOT NULL UNIQUE REFERENCES subtasks(subtask_id),
    cleanup_claim_id TEXT NOT NULL UNIQUE REFERENCES claims(claim_id),
    archive_paths_json TEXT NOT NULL,
    archive_proof_digest TEXT,
    recorded_by_session TEXT NOT NULL REFERENCES sessions(session_token),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_openspec_archive_status_change_state
ON openspec_archive_status(openspec_change_id, state, updated_at);

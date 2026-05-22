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
    CHECK (kind IN ('work', 'review')),
    CHECK (priority BETWEEN 0 AND 1000),
    CHECK (state IN ('available', 'blocked', 'claimed', 'in_progress', 'artifact_published',
                     'review_pending', 'changes_requested', 'approved', 'decided',
                     'ready_for_apply', 'applied', 'abandoned')),
    CHECK (state NOT IN ('blocked', 'changes_requested') OR (kind = 'work' AND artifact_digest IS NOT NULL)),
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

CREATE UNIQUE INDEX IF NOT EXISTS idx_reviews_review_source_artifact
ON reviews(review_id, subtask_id, artifact_digest);

CREATE TABLE IF NOT EXISTS review_followup_subtasks (
    review_id TEXT NOT NULL,
    source_subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
    source_artifact_digest TEXT NOT NULL REFERENCES artifacts(artifact_digest),
    findings_digest TEXT NOT NULL,
    followup_subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
    created_by_session TEXT NOT NULL REFERENCES sessions(session_token),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (review_id),
    UNIQUE (followup_subtask_id),
    FOREIGN KEY (review_id, source_subtask_id, source_artifact_digest)
        REFERENCES reviews(review_id, subtask_id, artifact_digest)
);
CREATE INDEX IF NOT EXISTS idx_review_followup_subtasks_review
ON review_followup_subtasks(review_id, created_at);

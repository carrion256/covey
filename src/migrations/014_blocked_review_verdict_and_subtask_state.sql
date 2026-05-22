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

CREATE TABLE reviews_new (
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
    CHECK (verdict IS NULL OR verdict IN ('approve', 'changes_requested', 'blocked')),
    CHECK ((state = 'decided') = (verdict IS NOT NULL))
);
INSERT INTO reviews_new (
    review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id,
    verdict, findings_digest, state, created_at, updated_at
)
SELECT
    review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id,
    verdict, findings_digest, state, created_at, updated_at
FROM reviews;
DROP TABLE reviews;
ALTER TABLE reviews_new RENAME TO reviews;
CREATE UNIQUE INDEX IF NOT EXISTS idx_reviews_one_open_round_per_artifact
ON reviews(subtask_id, artifact_digest)
WHERE state IN ('requested', 'in_progress');
CREATE UNIQUE INDEX IF NOT EXISTS idx_reviews_one_row_per_review_subtask
ON reviews(review_subtask_id)
WHERE review_subtask_id IS NOT NULL;

PRAGMA foreign_keys = ON;

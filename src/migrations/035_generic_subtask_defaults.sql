PRAGMA foreign_keys = OFF;

-- Covey is a standalone task tracker: brand-new databases must not carry
-- implicit mutAI values. The subtask table now defaults to the generic
-- direct completion policy on the "default" routing lane. Persisted mutAI
-- rows keep their explicit canonical_apply/'mutai' values; only the column
-- defaults and the review-subtask constraint change.
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
    completion_policy TEXT NOT NULL DEFAULT 'direct',
    routing_key TEXT NOT NULL DEFAULT 'default',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (kind IN ('work', 'review', 'cleanup')),
    CHECK (priority BETWEEN 0 AND 1000),
    CHECK (completion_policy IN ('direct', 'reviewed', 'canonical_apply')),
    CHECK (length(CAST(routing_key AS BLOB)) BETWEEN 1 AND 256),
    CHECK (instr(routing_key, char(0)) = 0),
    CHECK (routing_key NOT GLOB (
        '*['
        || char(1) || '-' || char(32)
        || char(127) || '-' || char(160)
        || char(5760)
        || char(8192) || '-' || char(8202)
        || char(8232) || char(8233) || char(8239)
        || char(8287) || char(12288)
        || ']*'
    )),
    CHECK (kind = 'work' OR completion_policy IN ('canonical_apply', 'reviewed')),
    CHECK (state IN ('available', 'blocked', 'claimed', 'in_progress', 'artifact_published',
                     'review_pending', 'changes_requested', 'approved', 'decided',
                     'ready_for_apply', 'applied', 'completed', 'failed', 'abandoned')),
    CHECK (state NOT IN ('blocked', 'changes_requested') OR (kind = 'work' AND artifact_digest IS NOT NULL)),
    CHECK (state NOT IN ('completed', 'failed') OR kind = 'work'),
    CHECK (state NOT IN ('completed', 'failed') OR current_claim_id IS NULL),
    CHECK (
        state != 'completed'
        OR (completion_policy = 'direct' AND artifact_digest IS NULL)
        OR (completion_policy = 'reviewed' AND artifact_digest IS NOT NULL)
    ),
    CHECK (state != 'failed' OR (completion_policy = 'direct' AND artifact_digest IS NULL)),
    CHECK (state NOT IN ('approved', 'ready_for_apply', 'applied') OR completion_policy = 'canonical_apply'),
    CHECK (completion_policy != 'direct' OR artifact_digest IS NULL),
    CHECK (
        (kind = 'review' AND review_target_subtask_id IS NOT NULL AND review_target_artifact_digest IS NOT NULL)
        OR
        (kind IN ('work', 'cleanup') AND review_target_subtask_id IS NULL AND review_target_artifact_digest IS NULL)
    )
);

INSERT INTO subtasks_new (
    subtask_id, meta_task_id, title, kind, review_target_subtask_id,
    review_target_artifact_digest, state, current_claim_id, artifact_digest,
    priority, completion_policy, routing_key, created_at, updated_at
)
SELECT
    subtask_id, meta_task_id, title, kind, review_target_subtask_id,
    review_target_artifact_digest, state, current_claim_id, artifact_digest,
    priority, completion_policy, routing_key, created_at, updated_at
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

CREATE INDEX IF NOT EXISTS idx_subtasks_available_work_routing_priority
ON subtasks(routing_key, priority, created_at)
WHERE kind = 'work' AND state = 'available';

CREATE INDEX IF NOT EXISTS idx_subtasks_available_work_routing_meta_priority
ON subtasks(routing_key, meta_task_id, priority, created_at)
WHERE kind = 'work' AND state = 'available';

CREATE INDEX IF NOT EXISTS idx_subtasks_nonterminal_updated
ON subtasks(updated_at)
WHERE state NOT IN ('available', 'applied', 'completed', 'failed', 'abandoned', 'decided');

CREATE INDEX IF NOT EXISTS idx_subtasks_open_meta
ON subtasks(meta_task_id)
WHERE state NOT IN ('applied', 'completed', 'failed', 'abandoned', 'decided');

CREATE TRIGGER subtask_completion_policy_immutable
BEFORE UPDATE OF completion_policy, routing_key ON subtasks
WHEN OLD.completion_policy IS NOT NEW.completion_policy
  OR OLD.routing_key IS NOT NEW.routing_key
BEGIN
    SELECT RAISE(ABORT, 'subtask completion policy and routing key are immutable');
END;

PRAGMA foreign_keys = ON;

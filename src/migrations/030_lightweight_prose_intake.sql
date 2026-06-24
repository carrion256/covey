CREATE TABLE IF NOT EXISTS prose_tasksets (
    taskset_id TEXT PRIMARY KEY,
    meta_task_id TEXT NOT NULL REFERENCES meta_tasks(meta_task_id),
    provenance_tier TEXT NOT NULL,
    source_excerpt TEXT NOT NULL,
    preview_digest TEXT NOT NULL,
    created_by TEXT NOT NULL REFERENCES sessions(session_token),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (provenance_tier = 'lightweight_prose_intake')
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_prose_tasksets_preview_digest
ON prose_tasksets(preview_digest);

CREATE TABLE IF NOT EXISTS prose_subtask_scope (
    subtask_id TEXT PRIMARY KEY REFERENCES subtasks(subtask_id),
    taskset_id TEXT NOT NULL REFERENCES prose_tasksets(taskset_id),
    item_index INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(taskset_id, item_index)
);

CREATE INDEX IF NOT EXISTS idx_prose_subtask_scope_taskset
ON prose_subtask_scope(taskset_id, item_index);

CREATE TABLE IF NOT EXISTS prose_apply_blockers (
    blocker_id TEXT PRIMARY KEY,
    taskset_id TEXT NOT NULL REFERENCES prose_tasksets(taskset_id),
    queue_id TEXT REFERENCES ready_queue(queue_id),
    artifact_digest TEXT,
    review_id TEXT,
    reason TEXT NOT NULL,
    detail TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_prose_apply_blockers_taskset
ON prose_apply_blockers(taskset_id, updated_at);

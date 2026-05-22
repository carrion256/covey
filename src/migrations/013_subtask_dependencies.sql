CREATE TABLE IF NOT EXISTS subtask_dependencies (
    subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id) ON DELETE CASCADE,
    depends_on_subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id) ON DELETE CASCADE,
    source_ref TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (subtask_id, depends_on_subtask_id)
);
CREATE INDEX IF NOT EXISTS idx_subtask_dependencies_depends_on
ON subtask_dependencies(depends_on_subtask_id, subtask_id);

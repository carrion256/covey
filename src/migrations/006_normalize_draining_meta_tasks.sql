PRAGMA foreign_keys = OFF;

CREATE TABLE meta_tasks_new (
    meta_task_id TEXT PRIMARY KEY,
    prompt_text TEXT NOT NULL,
    state TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (state IN ('planning', 'active', 'completed', 'cancelled')),
    FOREIGN KEY(created_by) REFERENCES sessions(session_token)
);
INSERT INTO meta_tasks_new (
    meta_task_id, prompt_text, state, created_by, created_at, updated_at
)
SELECT
    meta_task_id,
    prompt_text,
    CASE WHEN state = 'draining' THEN 'active' ELSE state END,
    created_by,
    created_at,
    updated_at
FROM meta_tasks;
DROP TABLE meta_tasks;
ALTER TABLE meta_tasks_new RENAME TO meta_tasks;

PRAGMA foreign_keys = ON;

-- One dedupe row per (principal, key, task): a key keeps its history.
--
-- A key used to name at most one task per agent principal, so a task that
-- ended kept its row and also blocked the same principal from filing anything
-- else under that key. The table now records every task filed under a key, and
-- the open-task lookup `meta submit` performs is a query over the task state.
-- A closed task keeps its row for audit and never matches.
--
-- The primary key widens to include the task id. Existing rows are copied
-- unchanged and the foreign key and the non-blank CHECK are restated.

CREATE TABLE meta_task_dedupe_keys_new (
    agent_principal_id TEXT NOT NULL,
    dedupe_key TEXT NOT NULL,
    meta_task_id TEXT NOT NULL REFERENCES meta_tasks(meta_task_id),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (agent_principal_id, dedupe_key, meta_task_id),
    CHECK (length(trim(dedupe_key)) > 0)
);
INSERT INTO meta_task_dedupe_keys_new (
    agent_principal_id, dedupe_key, meta_task_id, created_at
)
SELECT
    agent_principal_id, dedupe_key, meta_task_id, created_at
FROM meta_task_dedupe_keys
ORDER BY created_at, meta_task_id;
DROP TABLE meta_task_dedupe_keys;
ALTER TABLE meta_task_dedupe_keys_new RENAME TO meta_task_dedupe_keys;
CREATE INDEX IF NOT EXISTS idx_meta_task_dedupe_keys_task
ON meta_task_dedupe_keys(meta_task_id);

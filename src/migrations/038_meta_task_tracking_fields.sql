-- Typed tracking fields for meta tasks.
--
-- Meta tasks gain optional typed columns next to prompt_text: a one-line
-- title, an urgency, a calendar due date, and the typed reason a finished
-- task closed. Nothing parses or rewrites prompt_text: rows that predate
-- this migration keep NULL in every new column.
--
-- `handle` is a store-issued positive number, unique per store. The backfill
-- numbers existing rows in creation order (created_at, then meta_task_id),
-- and the trigger below numbers every later row the same way whatever wrote
-- it, so raw SQL and imports are numbered exactly like API writes.

ALTER TABLE meta_tasks ADD COLUMN title TEXT
    CHECK (title IS NULL OR length(trim(title)) > 0);
ALTER TABLE meta_tasks ADD COLUMN urgency TEXT
    CHECK (urgency IS NULL OR urgency IN ('low', 'medium', 'high'));
ALTER TABLE meta_tasks ADD COLUMN due_on TEXT
    CHECK (due_on IS NULL OR due_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]');
ALTER TABLE meta_tasks ADD COLUMN closure TEXT
    CHECK (closure IS NULL OR closure IN ('done', 'upstream_closed', 'superseded', 'dropped'));
ALTER TABLE meta_tasks ADD COLUMN handle INTEGER
    CHECK (handle >= 1);

WITH ordered AS (
    SELECT meta_task_id,
           ROW_NUMBER() OVER (ORDER BY created_at, meta_task_id) AS position
    FROM meta_tasks
)
UPDATE meta_tasks
SET handle = (
    SELECT position FROM ordered WHERE ordered.meta_task_id = meta_tasks.meta_task_id
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_meta_tasks_handle ON meta_tasks(handle);

CREATE TRIGGER IF NOT EXISTS meta_tasks_assign_handle
AFTER INSERT ON meta_tasks
WHEN NEW.handle IS NULL
BEGIN
    UPDATE meta_tasks
    SET handle = (SELECT COALESCE(MAX(handle), 0) + 1 FROM meta_tasks)
    WHERE meta_task_id = NEW.meta_task_id;
END;

-- One replacement per replaced task. The replaced task is retired in the
-- same transaction that writes the link.
CREATE TABLE IF NOT EXISTS meta_task_supersedes (
    superseding_meta_task_id TEXT NOT NULL REFERENCES meta_tasks(meta_task_id),
    superseded_meta_task_id TEXT NOT NULL REFERENCES meta_tasks(meta_task_id),
    superseded_at INTEGER NOT NULL,
    PRIMARY KEY (superseding_meta_task_id, superseded_meta_task_id),
    CHECK (superseding_meta_task_id <> superseded_meta_task_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_meta_task_supersedes_superseded
ON meta_task_supersedes(superseded_meta_task_id);

-- At most one meta task per dedupe key per agent principal.
CREATE TABLE IF NOT EXISTS meta_task_dedupe_keys (
    agent_principal_id TEXT NOT NULL,
    dedupe_key TEXT NOT NULL,
    meta_task_id TEXT NOT NULL REFERENCES meta_tasks(meta_task_id),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (agent_principal_id, dedupe_key),
    CHECK (length(trim(dedupe_key)) > 0)
);

-- Annotations: a closed key set, one value per key per meta task.
CREATE TABLE IF NOT EXISTS meta_task_annotations (
    meta_task_id TEXT NOT NULL REFERENCES meta_tasks(meta_task_id),
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    updated_by TEXT NOT NULL REFERENCES sessions(session_token),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (meta_task_id, key),
    CHECK (key IN ('verdict', 'acked', 'tier', 'entity')),
    CHECK (
        (key = 'acked' AND value IN ('true', 'false'))
        OR (key <> 'acked' AND length(trim(value)) > 0)
    )
);

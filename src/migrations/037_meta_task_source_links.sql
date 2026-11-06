-- Typed provenance for a meta task's intake source.
--
-- One optional side row per meta task: a meta task created from an intake
-- submission records where it came from in the same transaction as the task
-- itself (origin 'submitted'), and an older unlinked task may be given its
-- source exactly once afterwards (origin 'backfilled' with a named method).
-- Rows that predate this table stay unlinked: no backfill is performed here.
CREATE TABLE IF NOT EXISTS meta_task_source_links (
    meta_task_id TEXT PRIMARY KEY REFERENCES meta_tasks(meta_task_id),
    thread_id TEXT NOT NULL,
    url TEXT,
    received_at TEXT NOT NULL,
    path TEXT NOT NULL,
    origin TEXT NOT NULL,
    method TEXT,
    linked_by TEXT NOT NULL REFERENCES sessions(session_token),
    linked_at INTEGER NOT NULL,
    CHECK (origin IN ('submitted', 'backfilled')),
    CHECK (
        (origin = 'submitted' AND method IS NULL)
        OR (origin = 'backfilled' AND method IS NOT NULL AND length(method) > 0)
    ),
    CHECK (length(thread_id) > 0),
    CHECK (length(received_at) > 0),
    CHECK (length(path) > 0 AND substr(path, 1, 1) = '/'),
    CHECK (url IS NULL OR length(url) > 0)
);

CREATE TABLE IF NOT EXISTS import_provenance (
    object_type TEXT NOT NULL,
    object_id TEXT NOT NULL,
    planning_format TEXT NOT NULL,
    openspec_change_id TEXT NOT NULL,
    openspec_change_path TEXT NOT NULL,
    openspec_task_id TEXT,
    proposal_digest TEXT,
    design_digest TEXT,
    tasks_digest TEXT NOT NULL,
    spec_digests_json TEXT NOT NULL,
    task_digest TEXT,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (object_type, object_id),
    CHECK (object_type IN ('meta_task', 'subtask')),
    CHECK (planning_format = 'openspec')
);
CREATE INDEX IF NOT EXISTS idx_import_provenance_openspec_change
ON import_provenance(planning_format, openspec_change_id, openspec_task_id);

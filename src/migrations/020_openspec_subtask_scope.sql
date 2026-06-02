CREATE TABLE IF NOT EXISTS openspec_subtask_scope (
    subtask_id TEXT PRIMARY KEY REFERENCES subtasks(subtask_id),
    openspec_change_id TEXT NOT NULL,
    openspec_task_id TEXT NOT NULL,
    source_path TEXT NOT NULL,
    scenario_refs_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

ALTER TABLE review_followup_subtasks
ADD COLUMN repair_source_path TEXT;

ALTER TABLE review_followup_subtasks
ADD COLUMN repair_task_ref TEXT;

ALTER TABLE review_followup_subtasks
ADD COLUMN repair_scenario_refs_json TEXT NOT NULL DEFAULT '[]';

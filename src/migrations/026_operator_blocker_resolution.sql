ALTER TABLE operator_blockers
ADD COLUMN state TEXT NOT NULL DEFAULT 'open';

ALTER TABLE operator_blockers
ADD COLUMN resolved_reason TEXT;

ALTER TABLE operator_blockers
ADD COLUMN resolved_by_session TEXT REFERENCES sessions(session_token);

ALTER TABLE operator_blockers
ADD COLUMN resolved_at INTEGER;

CREATE INDEX IF NOT EXISTS idx_operator_blockers_openspec_state_updated
ON operator_blockers(openspec_change_id, state, updated_at);

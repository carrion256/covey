ALTER TABLE event_log ADD COLUMN actor_kind TEXT NOT NULL DEFAULT 'session';
CREATE INDEX IF NOT EXISTS idx_reservations_state_scope
ON reservations(state, scope_class, scope_key);
CREATE INDEX IF NOT EXISTS idx_subtasks_meta_task_priority
ON subtasks(meta_task_id, priority, created_at);
CREATE INDEX IF NOT EXISTS idx_ready_queue_state_enqueued
ON ready_queue(state, enqueued_at);

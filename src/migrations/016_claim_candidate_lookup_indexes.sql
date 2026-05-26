CREATE INDEX IF NOT EXISTS idx_subtasks_available_review_priority
ON subtasks(priority, created_at)
WHERE kind = 'review' AND state = 'available';

CREATE INDEX IF NOT EXISTS idx_subtasks_available_review_meta_priority
ON subtasks(meta_task_id, priority, created_at)
WHERE kind = 'review' AND state = 'available';

CREATE INDEX IF NOT EXISTS idx_subtasks_available_work_priority
ON subtasks(priority, created_at)
WHERE kind = 'work' AND state = 'available';

CREATE INDEX IF NOT EXISTS idx_subtasks_available_work_meta_priority
ON subtasks(meta_task_id, priority, created_at)
WHERE kind = 'work' AND state = 'available';

CREATE INDEX IF NOT EXISTS idx_reviews_subtask_created
ON reviews(subtask_id, created_at);

CREATE INDEX IF NOT EXISTS idx_ready_queue_subtask_enqueued
ON ready_queue(subtask_id, enqueued_at);

CREATE INDEX IF NOT EXISTS idx_subtasks_nonterminal_updated
ON subtasks(updated_at)
WHERE state NOT IN ('available', 'applied', 'abandoned', 'decided');

CREATE INDEX IF NOT EXISTS idx_subtasks_open_meta
ON subtasks(meta_task_id)
WHERE state NOT IN ('applied', 'abandoned', 'decided');

CREATE INDEX IF NOT EXISTS idx_conflicts_detected_desc
ON conflicts(detected_at DESC);

CREATE INDEX IF NOT EXISTS idx_conflicts_reservation_overlap_subject
ON conflicts(json_extract(payload_json, '$.reservation_id'))
WHERE conflict_kind = 'reservation_overlap' AND resolution_state != 'resolved';

CREATE INDEX IF NOT EXISTS idx_conflicts_reservation_overlap_overlapping
ON conflicts(json_extract(payload_json, '$.overlapping_reservation_id'))
WHERE conflict_kind = 'reservation_overlap' AND resolution_state != 'resolved';

CREATE INDEX IF NOT EXISTS idx_claims_held_owner_created
ON claims(owner_session_token, created_at)
WHERE state = 'held';

CREATE INDEX IF NOT EXISTS idx_sessions_state_token
ON sessions(state, session_token);

CREATE INDEX IF NOT EXISTS idx_ready_queue_inflight_claimant_enqueued
ON ready_queue(claimed_by_session_token, enqueued_at)
WHERE state = 'in_flight';

CREATE INDEX IF NOT EXISTS idx_reservations_active_scope_key_deadline
ON reservations(scope_class, scope_key, lease_deadline)
WHERE state = 'active';

CREATE INDEX IF NOT EXISTS idx_reservations_active_owner_deadline
ON reservations(owner_subtask_id, lease_deadline)
WHERE state = 'active';

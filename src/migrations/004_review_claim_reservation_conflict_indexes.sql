CREATE UNIQUE INDEX IF NOT EXISTS idx_reviews_one_open_round_per_artifact
ON reviews(subtask_id, artifact_digest)
WHERE state IN ('requested', 'in_progress');
CREATE UNIQUE INDEX IF NOT EXISTS idx_reviews_one_row_per_review_subtask
ON reviews(review_subtask_id)
WHERE review_subtask_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_claims_state_deadline
ON claims(state, lease_deadline);
CREATE INDEX IF NOT EXISTS idx_reservations_state_deadline
ON reservations(state, lease_deadline);
CREATE INDEX IF NOT EXISTS idx_conflicts_resolution_detected
ON conflicts(resolution_state, detected_at DESC);

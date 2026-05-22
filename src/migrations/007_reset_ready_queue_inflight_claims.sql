PRAGMA foreign_keys = OFF;

CREATE TABLE ready_queue_new (
    queue_id TEXT PRIMARY KEY,
    artifact_digest TEXT NOT NULL REFERENCES artifacts(artifact_digest),
    subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
    settlement_target TEXT NOT NULL,
    state TEXT NOT NULL,
    claimed_by_session_token TEXT REFERENCES sessions(session_token),
    claim_fence_seq INTEGER,
    claim_lease_deadline INTEGER,
    enqueued_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (settlement_target IN ('canonical')),
    CHECK (state IN ('queued', 'in_flight', 'applied', 'superseded', 'cancelled')),
    CHECK (
        (state = 'in_flight'
            AND claimed_by_session_token IS NOT NULL
            AND claim_fence_seq IS NOT NULL
            AND claim_lease_deadline IS NOT NULL)
        OR
        (state != 'in_flight'
            AND claimed_by_session_token IS NULL
            AND claim_lease_deadline IS NULL)
    )
);
INSERT INTO ready_queue_new (
    queue_id, artifact_digest, subtask_id, settlement_target, state,
    claimed_by_session_token, claim_fence_seq, claim_lease_deadline,
    enqueued_at, updated_at
)
SELECT
    queue_id,
    artifact_digest,
    subtask_id,
    settlement_target,
    CASE WHEN state = 'in_flight' THEN 'queued' ELSE state END,
    NULL,
    NULL,
    NULL,
    enqueued_at,
    updated_at
FROM ready_queue;
DROP TABLE ready_queue;
ALTER TABLE ready_queue_new RENAME TO ready_queue;
CREATE UNIQUE INDEX IF NOT EXISTS idx_ready_queue_one_active_per_subtask
ON ready_queue(subtask_id)
WHERE state IN ('queued', 'in_flight');
CREATE INDEX IF NOT EXISTS idx_ready_queue_state_enqueued
ON ready_queue(state, enqueued_at);
CREATE INDEX IF NOT EXISTS idx_ready_queue_state_claim_deadline
ON ready_queue(state, claim_lease_deadline, enqueued_at);

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS landing_receipts (
    queue_id TEXT NOT NULL REFERENCES ready_queue(queue_id),
    artifact_digest TEXT NOT NULL REFERENCES artifacts(artifact_digest),
    claim_fence_seq INTEGER NOT NULL,
    target_ref TEXT NOT NULL,
    landed_commit_oid TEXT NOT NULL,
    recorded_by_session TEXT NOT NULL REFERENCES sessions(session_token),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (queue_id, artifact_digest)
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_landing_receipts_artifact
ON landing_receipts(artifact_digest);

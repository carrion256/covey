CREATE TABLE IF NOT EXISTS apply_verifications (
    queue_id TEXT NOT NULL REFERENCES ready_queue(queue_id),
    artifact_digest TEXT NOT NULL REFERENCES artifacts(artifact_digest),
    review_id TEXT NOT NULL REFERENCES reviews(review_id),
    findings_digest TEXT NOT NULL,
    claim_fence_seq INTEGER NOT NULL,
    verifier TEXT NOT NULL,
    verdict_digest TEXT NOT NULL,
    seal_digest TEXT NOT NULL,
    recorded_by_session TEXT NOT NULL REFERENCES sessions(session_token),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (queue_id, claim_fence_seq, seal_digest)
);
CREATE INDEX IF NOT EXISTS idx_apply_verifications_lookup
ON apply_verifications(queue_id, artifact_digest, review_id, findings_digest, claim_fence_seq);

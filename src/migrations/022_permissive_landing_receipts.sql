CREATE TABLE IF NOT EXISTS permissive_landing_receipts (
    review_id TEXT PRIMARY KEY REFERENCES reviews(review_id),
    artifact_digest TEXT NOT NULL REFERENCES artifacts(artifact_digest),
    findings_digest TEXT NOT NULL,
    target_ref TEXT NOT NULL,
    landed_commit_oid TEXT,
    receipt_digest TEXT NOT NULL,
    recorded_by_session TEXT NOT NULL REFERENCES sessions(session_token),
    claim_id TEXT NOT NULL REFERENCES claims(claim_id),
    fence_seq INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_permissive_landing_receipts_artifact
ON permissive_landing_receipts(artifact_digest);

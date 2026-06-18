CREATE TABLE IF NOT EXISTS settlement_reconcile_blockers (
    queue_id TEXT NOT NULL REFERENCES ready_queue(queue_id),
    artifact_digest TEXT NOT NULL REFERENCES artifacts(artifact_digest),
    review_id TEXT NOT NULL REFERENCES reviews(review_id),
    findings_digest TEXT NOT NULL,
    claim_fence_seq INTEGER NOT NULL,
    reconcile_reason TEXT NOT NULL,
    authority_evidence_id TEXT NOT NULL,
    recorded_by_session TEXT NOT NULL REFERENCES sessions(session_token),
    created_at INTEGER NOT NULL,
    CHECK (
        reconcile_reason IN (
            'commit_unknown',
            'authority_lost',
            'stale_fence',
            'partial_prepare',
            'partial_finalize',
            'failed_canonical_apply',
            'duplicate_completion'
        )
    ),
    PRIMARY KEY (queue_id, claim_fence_seq, authority_evidence_id)
);

CREATE INDEX IF NOT EXISTS idx_settlement_reconcile_blockers_current_work
ON settlement_reconcile_blockers(queue_id, artifact_digest, claim_fence_seq);

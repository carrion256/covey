CREATE TABLE IF NOT EXISTS apply_gate_blockers (
    queue_id TEXT NOT NULL REFERENCES ready_queue(queue_id),
    artifact_digest TEXT NOT NULL REFERENCES artifacts(artifact_digest),
    review_id TEXT NOT NULL REFERENCES reviews(review_id),
    findings_digest TEXT NOT NULL,
    claim_fence_seq INTEGER NOT NULL,
    verifier TEXT NOT NULL,
    blocker_kind TEXT NOT NULL,
    reason TEXT NOT NULL,
    evidence_id TEXT NOT NULL,
    recorded_by_session TEXT NOT NULL REFERENCES sessions(session_token),
    created_at INTEGER NOT NULL,
    CHECK (blocker_kind IN ('authority_hold', 'git_apply_uncertainty')),
    PRIMARY KEY (queue_id, claim_fence_seq, evidence_id)
);

CREATE INDEX IF NOT EXISTS idx_apply_gate_blockers_current_work
ON apply_gate_blockers(queue_id, artifact_digest, claim_fence_seq);

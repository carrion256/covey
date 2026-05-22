CREATE TABLE IF NOT EXISTS review_followup_subtasks (
    review_id TEXT NOT NULL REFERENCES reviews(review_id),
    findings_digest TEXT NOT NULL,
    followup_subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
    created_by_session TEXT NOT NULL REFERENCES sessions(session_token),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (review_id, followup_subtask_id),
    UNIQUE (followup_subtask_id)
);
CREATE INDEX IF NOT EXISTS idx_review_followup_subtasks_review
ON review_followup_subtasks(review_id, created_at);

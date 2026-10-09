PRAGMA foreign_keys = OFF;

-- Evidence policy is a per-task, immutable property chosen when the task is
-- created. Every task that exists before this migration keeps requiring
-- evidence (the column default), so historical governance is unchanged. New
-- INSERTs in application code always set the column explicitly.
ALTER TABLE subtasks ADD COLUMN evidence_policy TEXT NOT NULL DEFAULT 'required'
    CHECK (evidence_policy IN ('optional', 'required'));

CREATE TRIGGER subtask_evidence_policy_immutable
BEFORE UPDATE OF evidence_policy ON subtasks
WHEN OLD.evidence_policy IS NOT NEW.evidence_policy
BEGIN
    SELECT RAISE(ABORT, 'subtask evidence policy is immutable');
END;

-- Rebuild the append-only outcome ledger so evidence_digest may be absent.
-- Every other column, CHECK, index, trigger and the claim binding foreign key
-- is carried over unchanged; a supplied digest is still fully validated.
CREATE TABLE subtask_attempt_outcomes_new (
    claim_id TEXT PRIMARY KEY,
    subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
    fence_seq INTEGER NOT NULL CHECK (fence_seq > 0),
    outcome_kind TEXT NOT NULL,
    evidence_digest TEXT,
    failure_code TEXT,
    summary TEXT NOT NULL,
    recorded_by_session TEXT NOT NULL REFERENCES sessions(session_token),
    recorded_at INTEGER NOT NULL CHECK (recorded_at >= 0),
    CHECK (outcome_kind IN ('succeeded', 'retryable_failure', 'terminal_failure')),
    CHECK (
        evidence_digest IS NULL
        OR (
            length(CAST(evidence_digest AS BLOB)) BETWEEN 3 AND 256
            AND instr(evidence_digest, ':') > 1
            AND instr(evidence_digest, ':') < length(evidence_digest)
            AND substr(evidence_digest, 1, instr(evidence_digest, ':') - 1)
                NOT GLOB '*[^a-z0-9_-]*'
            AND substr(evidence_digest, instr(evidence_digest, ':') + 1)
                NOT GLOB '*[^A-Za-z0-9_.-]*'
        )
    ),
    CHECK (length(CAST(summary AS BLOB)) BETWEEN 1 AND 4096),
    CHECK (instr(summary, char(0)) = 0),
    CHECK (summary NOT GLOB (
        '*['
        || char(1) || '-' || char(31)
        || char(127) || '-' || char(159)
        || ']*'
    )),
    CHECK (summary = trim(
        summary,
        char(32) || char(160) || char(5760)
        || char(8192) || char(8193) || char(8194) || char(8195)
        || char(8196) || char(8197) || char(8198) || char(8199)
        || char(8200) || char(8201) || char(8202)
        || char(8232) || char(8233) || char(8239)
        || char(8287) || char(12288)
    )),
    CHECK (failure_code IS NULL OR length(CAST(failure_code AS BLOB)) BETWEEN 1 AND 256),
    CHECK (failure_code IS NULL OR instr(failure_code, char(0)) = 0),
    CHECK (failure_code IS NULL OR failure_code NOT GLOB (
        '*['
        || char(1) || '-' || char(32)
        || char(127) || '-' || char(160)
        || char(5760)
        || char(8192) || '-' || char(8202)
        || char(8232) || char(8233) || char(8239)
        || char(8287) || char(12288)
        || ']*'
    )),
    CHECK (
        (outcome_kind = 'succeeded' AND failure_code IS NULL)
        OR
        (outcome_kind IN ('retryable_failure', 'terminal_failure')
            AND failure_code IS NOT NULL)
    ),
    UNIQUE (subtask_id, fence_seq),
    FOREIGN KEY (claim_id, subtask_id, fence_seq, recorded_by_session)
        REFERENCES claims(claim_id, subtask_id, fence_seq, owner_session_token)
);

INSERT INTO subtask_attempt_outcomes_new (
    claim_id, subtask_id, fence_seq, outcome_kind, evidence_digest,
    failure_code, summary, recorded_by_session, recorded_at
)
SELECT
    claim_id, subtask_id, fence_seq, outcome_kind, evidence_digest,
    failure_code, summary, recorded_by_session, recorded_at
FROM subtask_attempt_outcomes;

DROP TABLE subtask_attempt_outcomes;
ALTER TABLE subtask_attempt_outcomes_new RENAME TO subtask_attempt_outcomes;

CREATE INDEX IF NOT EXISTS idx_subtask_attempt_outcomes_subtask_recorded
ON subtask_attempt_outcomes(subtask_id, recorded_at);

CREATE TRIGGER subtask_attempt_outcomes_append_only_update
BEFORE UPDATE ON subtask_attempt_outcomes
BEGIN
    SELECT RAISE(ABORT, 'subtask attempt outcomes are append-only');
END;

CREATE TRIGGER subtask_attempt_outcomes_append_only_delete
BEFORE DELETE ON subtask_attempt_outcomes
BEGIN
    SELECT RAISE(ABORT, 'subtask attempt outcomes are append-only');
END;

-- SQL-level backstop for the application check: a task whose persisted policy
-- requires evidence can never record an outcome without it.
CREATE TRIGGER subtask_attempt_outcomes_required_evidence
BEFORE INSERT ON subtask_attempt_outcomes
WHEN NEW.evidence_digest IS NULL
 AND (SELECT evidence_policy FROM subtasks WHERE subtask_id = NEW.subtask_id) = 'required'
BEGIN
    SELECT RAISE(ABORT, 'subtask requires attempt evidence');
END;

PRAGMA foreign_keys = ON;

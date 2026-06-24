CREATE TABLE IF NOT EXISTS vcs_packet_stack_entries (
    stack_entry_id TEXT PRIMARY KEY NOT NULL,
    openspec_change_id TEXT NOT NULL,
    packet_bookmark TEXT NOT NULL,
    claim_id TEXT NOT NULL REFERENCES claims(claim_id) ON DELETE RESTRICT,
    subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id) ON DELETE RESTRICT,
    artifact_digest TEXT NOT NULL REFERENCES artifacts(artifact_digest) ON DELETE RESTRICT,
    review_id TEXT NOT NULL REFERENCES reviews(review_id) ON DELETE RESTRICT,
    findings_digest TEXT NOT NULL,
    claim_bookmark TEXT NOT NULL,
    claim_change_id TEXT NOT NULL,
    claim_commit_id TEXT NOT NULL,
    stack_position INTEGER NOT NULL CHECK (stack_position >= 0),
    tree_equivalence_digest TEXT,
    state TEXT NOT NULL CHECK (state IN (
        'candidate',
        'tree_equivalent',
        'published',
        'superseded'
    )),
    recorded_by_session TEXT NOT NULL REFERENCES sessions(session_token) ON DELETE RESTRICT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(openspec_change_id, claim_id),
    UNIQUE(openspec_change_id, stack_position)
);

CREATE INDEX IF NOT EXISTS idx_vcs_packet_stack_entries_openspec
ON vcs_packet_stack_entries(openspec_change_id, stack_position);

CREATE INDEX IF NOT EXISTS idx_vcs_packet_stack_entries_claim
ON vcs_packet_stack_entries(claim_id, state);

CREATE INDEX IF NOT EXISTS idx_vcs_packet_stack_entries_review
ON vcs_packet_stack_entries(review_id);

CREATE TABLE IF NOT EXISTS vcs_pr_publications (
    publication_id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN (
        'packet',
        'claim'
    )),
    openspec_change_id TEXT,
    claim_id TEXT REFERENCES claims(claim_id) ON DELETE RESTRICT,
    bookmark TEXT NOT NULL,
    head_commit_id TEXT NOT NULL,
    base_ref TEXT NOT NULL,
    pr_url TEXT,
    status TEXT NOT NULL CHECK (status IN (
        'prepared',
        'published',
        'blocked',
        'superseded'
    )),
    blocker_reason TEXT,
    recorded_by_session TEXT NOT NULL REFERENCES sessions(session_token) ON DELETE RESTRICT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (
        (kind = 'packet' AND openspec_change_id IS NOT NULL AND claim_id IS NULL)
        OR (kind = 'claim' AND claim_id IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_vcs_pr_publications_packet
ON vcs_pr_publications(openspec_change_id, status);

CREATE INDEX IF NOT EXISTS idx_vcs_pr_publications_claim
ON vcs_pr_publications(claim_id, status);

CREATE TABLE IF NOT EXISTS sessions (
    session_token TEXT PRIMARY KEY,
    agent_principal_id TEXT NOT NULL,
    agent_instance_id TEXT NOT NULL,
    role TEXT NOT NULL,
    state TEXT NOT NULL,
    active_subtask_id TEXT,
    last_heartbeat_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (role IN ('executor', 'orchestrator', 'apply_gate', 'reviewer')),
    CHECK (state IN ('active', 'stale', 'exited'))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_one_active_per_principal
ON sessions(agent_principal_id)
WHERE state = 'active';

CREATE TABLE IF NOT EXISTS meta_tasks (
    meta_task_id TEXT PRIMARY KEY,
    prompt_text TEXT NOT NULL,
    state TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (state IN ('planning', 'active', 'completed', 'cancelled')),
    FOREIGN KEY(created_by) REFERENCES sessions(session_token)
);

CREATE TABLE IF NOT EXISTS subtasks (
    subtask_id TEXT PRIMARY KEY,
    meta_task_id TEXT NOT NULL REFERENCES meta_tasks(meta_task_id),
    title TEXT NOT NULL,
    kind TEXT NOT NULL,
    review_target_subtask_id TEXT,
    review_target_artifact_digest TEXT,
    state TEXT NOT NULL,
    current_claim_id TEXT,
    artifact_digest TEXT,
    priority INTEGER NOT NULL DEFAULT 100,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (kind IN ('work', 'review')),
    CHECK (priority BETWEEN 0 AND 1000),
    CHECK (state IN ('available', 'blocked', 'claimed', 'in_progress', 'artifact_published',
                     'review_pending', 'changes_requested', 'approved', 'decided',
                     'ready_for_apply', 'applied', 'abandoned')),
    CHECK ((kind = 'review') = (review_target_subtask_id IS NOT NULL))
);

CREATE TABLE IF NOT EXISTS subtask_fence_counter (
    subtask_id TEXT PRIMARY KEY REFERENCES subtasks(subtask_id),
    next_fence_seq INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS claims (
    claim_id TEXT PRIMARY KEY,
    subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
    owner_session_token TEXT NOT NULL REFERENCES sessions(session_token),
    fence_seq INTEGER NOT NULL,
    lease_deadline INTEGER NOT NULL,
    state TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (state IN ('held', 'released', 'expired', 'revoked'))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_claims_one_held_per_subtask
ON claims(subtask_id)
WHERE state = 'held';

CREATE TABLE IF NOT EXISTS artifacts (
    artifact_digest TEXT PRIMARY KEY,
    artifact_kind TEXT NOT NULL,
    base_rev TEXT NOT NULL,
    produced_by_subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
    produced_by_session TEXT NOT NULL REFERENCES sessions(session_token),
    manifest_path TEXT NOT NULL,
    changed_paths_digest TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    CHECK (artifact_kind IN ('patch_bundle', 'isolated_commit_ref', 'tree_bundle',
                             'findings_bundle', 'verification_bundle'))
);

CREATE TABLE IF NOT EXISTS reviews (
    review_id TEXT PRIMARY KEY,
    subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
    artifact_digest TEXT NOT NULL REFERENCES artifacts(artifact_digest),
    reviewer_session TEXT NOT NULL REFERENCES sessions(session_token),
    review_subtask_id TEXT REFERENCES subtasks(subtask_id),
    verdict TEXT,
    findings_digest TEXT,
    state TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (state IN ('requested', 'in_progress', 'decided', 'superseded')),
    CHECK (verdict IS NULL OR verdict IN ('approve', 'changes_requested', 'blocked')),
    CHECK ((state = 'decided') = (verdict IS NOT NULL))
);

CREATE TABLE IF NOT EXISTS reservations (
    reservation_id TEXT PRIMARY KEY,
    owner_subtask_id TEXT NOT NULL REFERENCES subtasks(subtask_id),
    scope_class TEXT NOT NULL,
    scope_key TEXT NOT NULL,
    lease_deadline INTEGER NOT NULL,
    state TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (scope_class IN ('exact_path', 'subtree', 'repo_global', 'generated_set')),
    CHECK (state IN ('active', 'released', 'expired'))
);

CREATE TABLE IF NOT EXISTS reservation_generated_members (
    reservation_id TEXT PRIMARY KEY REFERENCES reservations(reservation_id),
    members_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ready_queue (
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
CREATE UNIQUE INDEX IF NOT EXISTS idx_ready_queue_one_active_per_subtask
ON ready_queue(subtask_id)
WHERE state IN ('queued', 'in_flight');

CREATE TABLE IF NOT EXISTS event_log (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type TEXT NOT NULL,
    object_type TEXT NOT NULL,
    object_id TEXT NOT NULL,
    session_token TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS conflicts (
    conflict_id TEXT PRIMARY KEY,
    object_type TEXT NOT NULL,
    object_id TEXT NOT NULL,
    conflict_kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    detected_at INTEGER NOT NULL,
    resolution_state TEXT NOT NULL,
    CHECK (resolution_state IN ('open', 'acknowledged', 'resolved'))
);

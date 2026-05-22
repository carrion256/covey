CREATE TABLE IF NOT EXISTS runtime_attestations (
    session_token TEXT PRIMARY KEY REFERENCES sessions(session_token),
    agent_principal_id TEXT NOT NULL,
    agent_instance_id TEXT NOT NULL,
    role TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    process_id TEXT,
    container_id TEXT,
    command_transcript_digest TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    CHECK (role IN ('executor', 'orchestrator', 'apply_gate', 'reviewer')),
    CHECK (TRIM(agent_principal_id) != ''),
    CHECK (TRIM(agent_instance_id) != ''),
    CHECK (TRIM(provider) != ''),
    CHECK (TRIM(model) != ''),
    CHECK (TRIM(command_transcript_digest) != ''),
    CHECK (
        (process_id IS NOT NULL AND TRIM(process_id) != '')
        OR
        (container_id IS NOT NULL AND TRIM(container_id) != '')
    ),
    CHECK (ended_at >= started_at)
);
CREATE INDEX IF NOT EXISTS idx_runtime_attestations_principal_role
ON runtime_attestations(agent_principal_id, role);

-- One active session per (principal, role).
--
-- A principal used to hold a single active session, whatever its role, so an
-- agent that both plans and executes needed two principals. Uniqueness now
-- covers the role as well: a principal may hold one active session in each
-- role at the same time. Existing rows already satisfy the wider rule.

DROP INDEX IF EXISTS idx_sessions_one_active_per_principal;
CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_one_active_per_principal_role
ON sessions(agent_principal_id, role)
WHERE state = 'active';

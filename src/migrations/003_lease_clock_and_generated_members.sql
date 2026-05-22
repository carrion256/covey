CREATE TABLE IF NOT EXISTS lease_clock (
    clock_id INTEGER PRIMARY KEY CHECK (clock_id = 1),
    last_tick_ms INTEGER NOT NULL
);
INSERT INTO lease_clock (clock_id, last_tick_ms)
VALUES (1, 0)
ON CONFLICT(clock_id) DO NOTHING;

ALTER TABLE sessions ADD COLUMN last_heartbeat_tick INTEGER NOT NULL DEFAULT 0;
UPDATE sessions
SET last_heartbeat_tick = CASE
    WHEN last_heartbeat_at > 0 THEN last_heartbeat_at
    ELSE 0
END
WHERE last_heartbeat_tick = 0;

ALTER TABLE reservation_generated_members RENAME TO reservation_generated_members_previous;
CREATE TABLE reservation_generated_members (
    reservation_id TEXT NOT NULL REFERENCES reservations(reservation_id),
    member_path TEXT NOT NULL,
    PRIMARY KEY (reservation_id, member_path)
);
INSERT INTO reservation_generated_members (reservation_id, member_path)
SELECT reservation_generated_members_previous.reservation_id, json_each.value
FROM reservation_generated_members_previous
JOIN json_each(reservation_generated_members_previous.members_json);
DROP TABLE reservation_generated_members_previous;

CREATE INDEX IF NOT EXISTS idx_reservation_generated_members_member_path
ON reservation_generated_members(member_path, reservation_id);
CREATE INDEX IF NOT EXISTS idx_sessions_state_heartbeat_tick
ON sessions(state, last_heartbeat_tick);
CREATE TABLE IF NOT EXISTS mutation_idempotency (
    actor_key TEXT NOT NULL,
    operation TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    response_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (actor_key, operation, idempotency_key)
);

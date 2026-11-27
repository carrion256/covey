-- A claim records the lease its owner asked for.
--
-- Every successful mutation a session makes extends the leases of the claims
-- that session holds, and the extension needs each claim's own duration.
-- Claiming and renewing store it. A claim written before this migration holds
-- NULL, which reads as the 30000 ms default lease.

ALTER TABLE claims ADD COLUMN lease_duration_ms INTEGER
    CHECK (lease_duration_ms IS NULL OR lease_duration_ms > 0);

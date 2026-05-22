ALTER TABLE runtime_attestations
ADD COLUMN provider_run_id TEXT NOT NULL DEFAULT '__covey_missing_provider_run_id__';
ALTER TABLE runtime_attestations
ADD COLUMN provider_run_id_issuer TEXT NOT NULL DEFAULT '__covey_missing_provider_run_id_issuer__';
CREATE INDEX IF NOT EXISTS idx_runtime_attestations_provider_run
ON runtime_attestations(provider_run_id_issuer, provider_run_id);

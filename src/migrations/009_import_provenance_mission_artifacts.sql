ALTER TABLE import_provenance
ADD COLUMN source_digests_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE import_provenance
ADD COLUMN mission_artifact_digests_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE import_provenance
ADD COLUMN mission_artifacts_json TEXT NOT NULL DEFAULT '[]';

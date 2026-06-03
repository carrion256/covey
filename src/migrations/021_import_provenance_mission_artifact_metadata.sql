ALTER TABLE import_provenance
ADD COLUMN mission_artifact_metadata_json TEXT NOT NULL DEFAULT '[]';

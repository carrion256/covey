use rusqlite::{Transaction, params};

use crate::{
    error::Result,
    model::{EventType, ImportOpenSpecEvent, ObjectType, OpenSpecImportProvenance, TimestampMs},
    store::append_session_event,
};

use super::{
    OPENSPEC_PLANNING_FORMAT, OpenSpecImportRecord, OpenSpecSourceSnapshot, OpenSpecSourceTask,
};

pub(super) fn meta_provenance(
    source: &OpenSpecSourceSnapshot,
    object_id: &str,
    now: i64,
) -> OpenSpecImportProvenance {
    OpenSpecImportProvenance {
        object_type: ObjectType::MetaTask,
        object_id: object_id.to_owned(),
        planning_format: OPENSPEC_PLANNING_FORMAT.to_owned(),
        openspec_change_id: source.change_id.clone(),
        openspec_change_path: source.change_path.clone(),
        openspec_task_id: None,
        proposal_digest: Some(source.proposal_digest.clone()),
        design_digest: Some(source.design_digest.clone()),
        tasks_digest: source.tasks_digest.clone(),
        spec_digests: source.spec_digests.clone(),
        source_digests: source.source_digests.clone(),
        mission_artifact_digests: source.mission_artifact_digests.clone(),
        mission_artifacts: source.mission_artifacts.clone(),
        task_digest: None,
        updated_at: timestamp_ms(now),
    }
}

pub(super) fn task_provenance(
    source: &OpenSpecSourceSnapshot,
    task: &OpenSpecSourceTask,
    object_id: &str,
    now: i64,
) -> OpenSpecImportProvenance {
    OpenSpecImportProvenance {
        object_type: ObjectType::Subtask,
        object_id: object_id.to_owned(),
        planning_format: OPENSPEC_PLANNING_FORMAT.to_owned(),
        openspec_change_id: source.change_id.clone(),
        openspec_change_path: source.change_path.clone(),
        openspec_task_id: Some(task.task_id.clone()),
        proposal_digest: None,
        design_digest: None,
        tasks_digest: source.tasks_digest.clone(),
        spec_digests: Vec::new(),
        source_digests: source.source_digests.clone(),
        mission_artifact_digests: source.mission_artifact_digests.clone(),
        mission_artifacts: source.mission_artifacts.clone(),
        task_digest: Some(task.task_digest.clone()),
        updated_at: timestamp_ms(now),
    }
}
pub(super) fn provenance_equivalent(
    existing: Option<&OpenSpecImportProvenance>,
    expected: &OpenSpecImportProvenance,
) -> bool {
    let Some(existing) = existing else {
        return false;
    };
    existing.object_type == expected.object_type
        && existing.object_id == expected.object_id
        && existing.planning_format == expected.planning_format
        && existing.openspec_change_id == expected.openspec_change_id
        && existing.openspec_change_path == expected.openspec_change_path
        && existing.openspec_task_id == expected.openspec_task_id
        && existing.proposal_digest == expected.proposal_digest
        && existing.design_digest == expected.design_digest
        && existing.tasks_digest == expected.tasks_digest
        && existing.spec_digests == expected.spec_digests
        && existing.source_digests == expected.source_digests
        && existing.mission_artifact_digests == expected.mission_artifact_digests
        && existing.mission_artifacts == expected.mission_artifacts
        && existing.task_digest == expected.task_digest
}
pub(super) fn upsert_openspec_provenance_tx(
    tx: &Transaction<'_>,
    provenance: &OpenSpecImportProvenance,
    now: i64,
) -> Result<()> {
    tx.execute(
        r#"
        INSERT INTO import_provenance (
            object_type, object_id, planning_format, openspec_change_id,
            openspec_change_path, openspec_task_id, proposal_digest, design_digest,
            tasks_digest, spec_digests_json, source_digests_json,
            mission_artifact_digests_json, mission_artifacts_json, task_digest, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ON CONFLICT(object_type, object_id) DO UPDATE SET
            planning_format = excluded.planning_format,
            openspec_change_id = excluded.openspec_change_id,
            openspec_change_path = excluded.openspec_change_path,
            openspec_task_id = excluded.openspec_task_id,
            proposal_digest = excluded.proposal_digest,
            design_digest = excluded.design_digest,
            tasks_digest = excluded.tasks_digest,
            spec_digests_json = excluded.spec_digests_json,
            source_digests_json = excluded.source_digests_json,
            mission_artifact_digests_json = excluded.mission_artifact_digests_json,
            mission_artifacts_json = excluded.mission_artifacts_json,
            task_digest = excluded.task_digest,
            updated_at = excluded.updated_at
        "#,
        params![
            provenance.object_type.to_string(),
            provenance.object_id,
            provenance.planning_format,
            provenance.openspec_change_id,
            provenance.openspec_change_path,
            provenance.openspec_task_id,
            provenance.proposal_digest,
            provenance.design_digest,
            provenance.tasks_digest,
            serde_json::to_string(&provenance.spec_digests)?,
            serde_json::to_string(&provenance.source_digests)?,
            serde_json::to_string(&provenance.mission_artifact_digests)?,
            serde_json::to_string(&provenance.mission_artifacts)?,
            provenance.task_digest,
            now
        ],
    )?;
    Ok(())
}

pub(super) fn append_openspec_import_event_tx(
    tx: &Transaction<'_>,
    session_token: &str,
    record: &OpenSpecImportRecord,
    now: i64,
) -> Result<()> {
    let mut provenance = record.provenance.clone();
    provenance.updated_at = timestamp_ms(now);
    let payload = ImportOpenSpecEvent {
        change_id: provenance.openspec_change_id.clone(),
        object_type: record.object_type,
        object_id: record.object_id.clone(),
        openspec_task_id: record.openspec_task_id.clone(),
        action: record.action,
        provenance,
    };
    append_session_event(
        tx,
        EventType::OpenSpecImported,
        record.object_type,
        &record.object_id,
        session_token,
        &payload,
        now,
    )
}

fn timestamp_ms(value: i64) -> TimestampMs {
    TimestampMs::parse(value).expect("wall clock timestamps are non-negative")
}

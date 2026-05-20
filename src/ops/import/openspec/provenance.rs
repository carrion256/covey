use rusqlite::{Transaction, params};

use crate::{
    error::Result,
    model::{
        EventType, ImportOpenSpecEvent, OpenSpecImportProvenance, OpenSpecImportProvenanceCommon,
        TimestampMs,
    },
    store::append_session_event,
};

use super::{OpenSpecImportRecord, OpenSpecSourceSnapshot, OpenSpecSourceTask};

pub(super) fn meta_provenance(
    source: &OpenSpecSourceSnapshot,
    object_id: &str,
    now: i64,
) -> OpenSpecImportProvenance {
    OpenSpecImportProvenance::meta_task(
        OpenSpecImportProvenanceCommon::new(
            source.change_id.to_string(),
            source.change_path.to_string(),
            source.tasks_digest.to_string(),
            source.source_digests.clone(),
            source.mission_artifact_digests.clone(),
            mission_artifact_strings(source),
            timestamp_ms(now),
        )
        .expect("validated OpenSpec source change id must be valid provenance"),
        object_id.to_owned(),
        source.proposal_digest.to_string(),
        source.design_digest.to_string(),
        source.spec_digests.clone(),
    )
    .expect("deterministic OpenSpec meta-task id must be a valid Covey id")
}

pub(super) fn task_provenance(
    source: &OpenSpecSourceSnapshot,
    task: &OpenSpecSourceTask,
    object_id: &str,
    now: i64,
) -> OpenSpecImportProvenance {
    OpenSpecImportProvenance::subtask(
        OpenSpecImportProvenanceCommon::new(
            source.change_id.to_string(),
            source.change_path.to_string(),
            source.tasks_digest.to_string(),
            source.source_digests.clone(),
            source.mission_artifact_digests.clone(),
            mission_artifact_strings(source),
            timestamp_ms(now),
        )
        .expect("validated OpenSpec source change id must be valid provenance"),
        object_id.to_owned(),
        task.task_id.to_string(),
        task.task_digest.to_string(),
    )
    .expect("deterministic OpenSpec subtask id must be a valid Covey id")
}

pub(super) fn provenance_equivalent(
    existing: Option<&OpenSpecImportProvenance>,
    expected: &OpenSpecImportProvenance,
) -> bool {
    let Some(existing) = existing else {
        return false;
    };
    existing.object_type() == expected.object_type()
        && existing.object_id() == expected.object_id()
        && existing.planning_format() == expected.planning_format()
        && existing.openspec_change_id() == expected.openspec_change_id()
        && existing.openspec_change_path() == expected.openspec_change_path()
        && existing.openspec_task_id() == expected.openspec_task_id()
        && existing.proposal_digest() == expected.proposal_digest()
        && existing.design_digest() == expected.design_digest()
        && existing.tasks_digest() == expected.tasks_digest()
        && existing.spec_digests() == expected.spec_digests()
        && existing.source_digests() == expected.source_digests()
        && existing.mission_artifact_digests() == expected.mission_artifact_digests()
        && existing.mission_artifacts() == expected.mission_artifacts()
        && existing.task_digest() == expected.task_digest()
}

fn mission_artifact_strings(source: &OpenSpecSourceSnapshot) -> Vec<String> {
    source
        .mission_artifacts
        .iter()
        .map(ToString::to_string)
        .collect()
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
            provenance.object_type().to_string(),
            provenance.object_id(),
            provenance.planning_format(),
            provenance.openspec_change_id(),
            provenance.openspec_change_path(),
            provenance.openspec_task_id(),
            provenance.proposal_digest(),
            provenance.design_digest(),
            provenance.tasks_digest(),
            serde_json::to_string(provenance.spec_digests())?,
            serde_json::to_string(provenance.source_digests())?,
            serde_json::to_string(provenance.mission_artifact_digests())?,
            serde_json::to_string(provenance.mission_artifacts())?,
            provenance.task_digest(),
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
    provenance.set_updated_at(timestamp_ms(now));
    let payload = ImportOpenSpecEvent::new(record.action, provenance);
    append_session_event(
        tx,
        EventType::OpenSpecImported,
        record.object_type(),
        record.object_id(),
        session_token,
        &payload,
        now,
    )
}

fn timestamp_ms(value: i64) -> TimestampMs {
    TimestampMs::parse(value).expect("wall clock timestamps are non-negative")
}

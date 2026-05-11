use rusqlite::{Transaction, params};

use crate::{
    error::{CoveyError, Result},
    model::{CreateSubtaskReq, ImportOpenSpecAction, ObjectType, SubtaskKind, SubtaskState},
    ops::workflow::create_subtask_tx,
};

use super::{
    OpenSpecImportRecord,
    provenance::{append_openspec_import_event_tx, upsert_openspec_provenance_tx},
};

pub(super) fn apply_openspec_import_diff_tx(
    tx: &Transaction<'_>,
    session_token: &str,
    records: &[OpenSpecImportRecord],
    now: i64,
) -> Result<()> {
    let meta_task_id = records
        .iter()
        .find(|record| record.object_type == ObjectType::MetaTask)
        .map(|record| record.object_id.clone())
        .ok_or_else(|| CoveyError::InvalidImportDestination {
            reason: "missing OpenSpec meta task record".to_owned(),
        })?;

    for record in records {
        match (record.object_type, record.action) {
            (ObjectType::MetaTask, ImportOpenSpecAction::Created) => {
                let prompt_text = record.title.as_deref().unwrap_or("");
                tx.execute(
                    r#"
                    INSERT INTO meta_tasks (
                        meta_task_id, prompt_text, state, created_by, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                    "#,
                    params![
                        record.object_id,
                        prompt_text,
                        crate::model::MetaTaskState::Planning.to_string(),
                        session_token,
                        now
                    ],
                )?;
                upsert_openspec_provenance_tx(tx, &record.provenance, now)?;
                append_openspec_import_event_tx(tx, session_token, record, now)?;
            }
            (ObjectType::MetaTask, ImportOpenSpecAction::Updated) => {
                let prompt_text = record.title.as_deref().unwrap_or("");
                tx.execute(
                    "UPDATE meta_tasks SET prompt_text = ?2, updated_at = ?3 WHERE meta_task_id = ?1",
                    params![record.object_id, prompt_text, now],
                )?;
                upsert_openspec_provenance_tx(tx, &record.provenance, now)?;
                append_openspec_import_event_tx(tx, session_token, record, now)?;
            }
            (ObjectType::Subtask, ImportOpenSpecAction::Created) => {
                let title = record
                    .title
                    .clone()
                    .ok_or_else(|| CoveyError::InvalidImportRow {
                        source_issue_id: record.object_id.clone(),
                        reason: "missing OpenSpec task title".to_owned(),
                    })?;
                create_subtask_tx(
                    tx,
                    &CreateSubtaskReq {
                        session_token: session_token.to_owned(),
                        meta_task_id: meta_task_id.clone(),
                        subtask_id: Some(record.object_id.clone()),
                        title,
                        kind: SubtaskKind::Work,
                        review_target_subtask_id: None,
                        review_target_artifact_digest: None,
                        priority: 100,
                        idempotency_key: format!("import-openspec:{}", record.object_id),
                    },
                    now,
                )?;
                upsert_openspec_provenance_tx(tx, &record.provenance, now)?;
                append_openspec_import_event_tx(tx, session_token, record, now)?;
            }
            (ObjectType::Subtask, ImportOpenSpecAction::Updated) => {
                let title =
                    record
                        .title
                        .as_deref()
                        .ok_or_else(|| CoveyError::InvalidImportRow {
                            source_issue_id: record.object_id.clone(),
                            reason: "missing OpenSpec task title".to_owned(),
                        })?;
                let updated = tx.execute(
                    r#"
                    UPDATE subtasks
                    SET title = ?2, updated_at = ?3
                    WHERE subtask_id = ?1
                      AND current_claim_id IS NULL
                      AND state IN (?4, ?5)
                    "#,
                    params![
                        record.object_id,
                        title,
                        now,
                        SubtaskState::Available.to_string(),
                        SubtaskState::ChangesRequested.to_string()
                    ],
                )?;
                if updated != 1 {
                    return Err(CoveyError::ImportDuplicate {
                        source_issue_id: record.openspec_task_id.clone().unwrap_or_default(),
                        subtask_id: record.object_id.clone(),
                    });
                }
                upsert_openspec_provenance_tx(tx, &record.provenance, now)?;
                append_openspec_import_event_tx(tx, session_token, record, now)?;
            }
            (_, ImportOpenSpecAction::Unchanged | ImportOpenSpecAction::Conflict) => {}
            _ => {
                return Err(CoveyError::InvalidImportDestination {
                    reason: "OpenSpec import only supports meta_task and subtask records"
                        .to_owned(),
                });
            }
        }
    }
    Ok(())
}

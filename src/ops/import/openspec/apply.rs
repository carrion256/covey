#![cfg_attr(coverage_nightly, coverage(off))]

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    error::{CoveyError, Result},
    model::{
        CreateSubtaskRequest, ImportOpenSpecAction, MetaTaskState, ObjectType, SubtaskId,
        SubtaskState, meta_task_state_name, subtask_state_name,
    },
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
        .find(|record| record.object_type() == ObjectType::MetaTask)
        .map(|record| record.object_id().to_owned())
        .ok_or_else(|| CoveyError::InvalidImportDestination {
            reason: "missing OpenSpec meta task record".to_owned(),
        })?;

    for record in records {
        match (record.object_type(), record.action) {
            (ObjectType::MetaTask, ImportOpenSpecAction::Created) => {
                let prompt_text = record.title();
                tx.execute(
                    r#"
                    INSERT INTO meta_tasks (
                        meta_task_id, prompt_text, state, created_by, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                    "#,
                    params![
                        record.object_id(),
                        prompt_text,
                        meta_task_state_name(MetaTaskState::Planning),
                        session_token,
                        now
                    ],
                )?;
                upsert_openspec_provenance_tx(tx, &record.provenance, now)?;
                append_openspec_import_event_tx(tx, session_token, record, now)?;
            }
            (ObjectType::MetaTask, ImportOpenSpecAction::Updated) => {
                let prompt_text = record.title();
                tx.execute(
                    "UPDATE meta_tasks SET prompt_text = ?2, updated_at = ?3 WHERE meta_task_id = ?1",
                    params![record.object_id(), prompt_text, now],
                )?;
                upsert_openspec_provenance_tx(tx, &record.provenance, now)?;
                append_openspec_import_event_tx(tx, session_token, record, now)?;
            }
            (ObjectType::Subtask, ImportOpenSpecAction::Created) => {
                let title = record.title().to_owned();
                create_subtask_tx(
                    tx,
                    &CreateSubtaskRequest::try_from_raw_parts(
                        session_token.to_owned(),
                        meta_task_id.clone(),
                        Some(record.object_id().to_owned()),
                        title,
                        100,
                        format!("import-openspec:{}", record.object_id()),
                    )?,
                    now,
                )?;
                upsert_openspec_provenance_tx(tx, &record.provenance, now)?;
                append_openspec_import_event_tx(tx, session_token, record, now)?;
            }
            (ObjectType::Subtask, ImportOpenSpecAction::Updated) => {
                let title = record.title();
                let updated = tx.execute(
                    r#"
                    UPDATE subtasks
                    SET title = ?2, updated_at = ?3
                    WHERE subtask_id = ?1
                      AND current_claim_id IS NULL
                      AND state IN (?4, ?5)
                    "#,
                    params![
                        record.object_id(),
                        title,
                        now,
                        subtask_state_name(SubtaskState::Available),
                        subtask_state_name(SubtaskState::ChangesRequested)
                    ],
                )?;
                if updated != 1 {
                    if settled_subtask_with_same_title_tx(tx, record.object_id(), title)? {
                        continue;
                    }
                    return Err(CoveyError::ImportDuplicate {
                        source_issue_id: record
                            .openspec_task_id()
                            .map(ToString::to_string)
                            .unwrap_or_default(),
                        subtask_id: SubtaskId::parse(record.object_id().to_owned())?,
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
    upsert_subtask_dependencies_tx(tx, records, now)?;
    Ok(())
}

fn settled_subtask_with_same_title_tx(
    tx: &Transaction<'_>,
    subtask_id: &str,
    title: &str,
) -> Result<bool> {
    let existing = tx
        .query_row(
            r#"
            SELECT title
            FROM subtasks
            WHERE subtask_id = ?1
              AND current_claim_id IS NULL
              AND state NOT IN (?2, ?3)
            "#,
            params![
                subtask_id,
                subtask_state_name(SubtaskState::Available),
                subtask_state_name(SubtaskState::ChangesRequested)
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(existing.as_deref() == Some(title))
}

fn upsert_subtask_dependencies_tx(
    tx: &Transaction<'_>,
    records: &[OpenSpecImportRecord],
    now: i64,
) -> Result<()> {
    let task_to_subtask: BTreeMap<&str, &str> = records
        .iter()
        .filter(|record| record.object_type() == ObjectType::Subtask)
        .filter_map(|record| Some((record.openspec_task_id()?.as_str(), record.object_id())))
        .collect();

    for record in records
        .iter()
        .filter(|record| record.object_type() == ObjectType::Subtask)
        .filter(|record| record.action != ImportOpenSpecAction::Conflict)
    {
        tx.execute(
            "DELETE FROM subtask_dependencies WHERE subtask_id = ?1",
            params![record.object_id()],
        )?;
        for dependency in resolved_dependency_ids(record, &task_to_subtask) {
            if dependency == record.object_id() {
                continue;
            }
            tx.execute(
                r#"
                INSERT INTO subtask_dependencies (
                    subtask_id, depends_on_subtask_id, source_ref, created_at
                ) VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT(subtask_id, depends_on_subtask_id) DO UPDATE SET
                    source_ref = excluded.source_ref
                "#,
                params![record.object_id(), dependency, dependency, now],
            )?;
        }
    }
    Ok(())
}

fn resolved_dependency_ids<'a>(
    record: &'a OpenSpecImportRecord,
    task_to_subtask: &'a BTreeMap<&str, &str>,
) -> Vec<&'a str> {
    record
        .dependencies()
        .iter()
        .filter(|raw| raw.as_str() != "none")
        .flat_map(|raw| {
            task_to_subtask
                .iter()
                .filter_map(move |(task_id, subtask_id)| {
                    raw.as_str()
                        .split(|ch: char| {
                            !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
                        })
                        .any(|token| dependency_token_matches(token, task_id))
                        .then_some(*subtask_id)
                })
        })
        .collect()
}

fn dependency_token_matches(token: &str, task_id: &str) -> bool {
    token == task_id
        || token
            .strip_suffix(task_id)
            .is_some_and(|prefix| prefix.ends_with('-'))
}

#[cfg(test)]
mod tests {
    use super::dependency_token_matches;

    #[test]
    fn dependency_tokens_match_numeric_and_better_droid_task_ids() {
        assert!(dependency_token_matches("3.1", "3.1"));
        assert!(dependency_token_matches("TASK-MAG0-3.1", "3.1"));
        assert!(dependency_token_matches("TASK-MAG1-2.4", "2.4"));
        assert!(!dependency_token_matches("13.1", "3.1"));
        assert!(!dependency_token_matches("TASK-MAG0-13.1", "3.1"));
        assert!(!dependency_token_matches("TASK-MAG0-3.10", "3.1"));
    }
}

use rusqlite::Transaction;

use crate::{
    error::{CoveyError, Result},
    model::{
        ImportOpenSpecAction, ImportOpenSpecConflict, ImportOpenSpecConflictReason,
        ImportOpenSpecItemResult, ImportOpenSpecResult, ObjectType,
    },
    queries::{load_import_provenance_tx, load_meta_task_tx, load_subtask_tx},
    validators::{MAX_PROMPT_LEN, MAX_TITLE_LEN, ensure_length},
};

use super::{
    OpenSpecImportDiff, OpenSpecImportRecord, OpenSpecSourceSnapshot,
    ids::{
        openspec_meta_prompt, openspec_meta_task_id, openspec_subtask_id,
        validate_deterministic_covey_id,
    },
    provenance::{meta_provenance, provenance_equivalent, task_provenance},
};

pub(super) fn build_openspec_import_diff_tx(
    tx: &Transaction<'_>,
    source: &OpenSpecSourceSnapshot,
    dry_run: bool,
) -> Result<OpenSpecImportDiff> {
    let meta_task_id = openspec_meta_task_id(&source.change_id);
    validate_deterministic_covey_id("meta_task_id", &meta_task_id)?;

    let mut records = Vec::with_capacity(source.tasks.len() + 1);
    let mut items = Vec::with_capacity(source.tasks.len() + 1);
    let mut conflicts = Vec::new();
    let mut created = 0usize;
    let mut updated = 0usize;
    let mut unchanged = 0usize;

    let meta_prompt = openspec_meta_prompt(source);
    ensure_length("prompt_text", &meta_prompt, MAX_PROMPT_LEN)?;
    let meta_provenance = meta_provenance(source, &meta_task_id, 0);
    let meta_action = match load_meta_task_tx(tx, &meta_task_id) {
        Ok(existing) => {
            let existing_provenance =
                load_import_provenance_tx(tx, ObjectType::MetaTask, &meta_task_id)?;
            if existing.prompt_text == meta_prompt
                && provenance_equivalent(existing_provenance.as_ref(), &meta_provenance)
            {
                ImportOpenSpecAction::Unchanged
            } else {
                ImportOpenSpecAction::Updated
            }
        }
        Err(CoveyError::MetaTaskNotFound) => ImportOpenSpecAction::Created,
        Err(err) => return Err(err),
    };
    count_action(meta_action, &mut created, &mut updated, &mut unchanged);
    records.push(OpenSpecImportRecord {
        object_type: ObjectType::MetaTask,
        object_id: meta_task_id.clone(),
        openspec_task_id: None,
        title: Some(meta_prompt.clone()),
        dependencies: Vec::new(),
        action: meta_action,
        provenance: meta_provenance,
    });
    items.push(
        ImportOpenSpecItemResult::meta_task(meta_task_id.clone(), meta_prompt, meta_action)
            .map_err(|reason| CoveyError::InvalidImportDestination { reason })?,
    );

    for task in &source.tasks {
        ensure_length("title", &task.title, MAX_TITLE_LEN)?;
        let subtask_id = openspec_subtask_id(&source.change_id, &task.task_id);
        validate_deterministic_covey_id("subtask_id", &subtask_id)?;
        let provenance = task_provenance(source, task, &subtask_id, 0);
        let mut action = ImportOpenSpecAction::Created;
        let mut conflict = None;

        match load_subtask_tx(tx, &subtask_id) {
            Ok(existing) => {
                if existing.meta_task_id != meta_task_id {
                    action = ImportOpenSpecAction::Conflict;
                    conflict = Some(
                        ImportOpenSpecConflict::subtask(
                            subtask_id.clone(),
                            task.task_id.clone(),
                            ImportOpenSpecConflictReason::ExistingSubtaskDifferentMetaTask,
                            task.source_path.clone(),
                            task.task_digest.clone(),
                        )
                        .map_err(|reason| CoveyError::InvalidImportDestination { reason })?,
                    );
                } else {
                    let existing_provenance =
                        load_import_provenance_tx(tx, ObjectType::Subtask, &subtask_id)?;
                    let changed = existing.title != task.title
                        || !provenance_equivalent(existing_provenance.as_ref(), &provenance);
                    if changed && existing.current_claim_id().is_some() {
                        action = ImportOpenSpecAction::Conflict;
                        conflict = Some(
                            ImportOpenSpecConflict::subtask(
                                subtask_id.clone(),
                                task.task_id.clone(),
                                ImportOpenSpecConflictReason::ActiveClaimChangedSource,
                                task.source_path.clone(),
                                task.task_digest.clone(),
                            )
                            .map_err(|reason| CoveyError::InvalidImportDestination { reason })?,
                        );
                    } else if changed {
                        action = ImportOpenSpecAction::Updated;
                    } else {
                        action = ImportOpenSpecAction::Unchanged;
                    }
                }
            }
            Err(CoveyError::SubtaskNotFound) => {}
            Err(err) => return Err(err),
        }

        if let Some(conflict) = conflict {
            conflicts.push(conflict);
        } else {
            count_action(action, &mut created, &mut updated, &mut unchanged);
        }
        records.push(OpenSpecImportRecord {
            object_type: ObjectType::Subtask,
            object_id: subtask_id.clone(),
            openspec_task_id: Some(task.task_id.clone()),
            title: Some(task.title.clone()),
            dependencies: task.dependencies.clone(),
            action,
            provenance,
        });
        items.push(
            ImportOpenSpecItemResult::subtask(
                subtask_id,
                task.task_id.clone(),
                task.title.clone(),
                task.task_type.clone(),
                task.task_digest.clone(),
                task.source_path.clone(),
                action,
            )
            .map_err(|reason| CoveyError::InvalidImportDestination { reason })?,
        );
    }

    let result = ImportOpenSpecResult::new(
        source.change_id.clone(),
        meta_task_id,
        dry_run,
        conflicts,
        items,
    )
    .map_err(|reason| CoveyError::InvalidImportDestination { reason })?;
    debug_assert_eq!(result.created(), created);
    debug_assert_eq!(result.updated(), updated);
    debug_assert_eq!(result.unchanged(), unchanged);

    Ok(OpenSpecImportDiff { result, records })
}

fn count_action(
    action: ImportOpenSpecAction,
    created: &mut usize,
    updated: &mut usize,
    unchanged: &mut usize,
) {
    match action {
        ImportOpenSpecAction::Created => *created += 1,
        ImportOpenSpecAction::Updated => *updated += 1,
        ImportOpenSpecAction::Unchanged => *unchanged += 1,
        ImportOpenSpecAction::Conflict => {}
    }
}

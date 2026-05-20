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
    let raw_meta_task_id = openspec_meta_task_id(source.change_id.as_str());
    validate_deterministic_covey_id("meta_task_id", &raw_meta_task_id)?;
    let meta_task_id =
        crate::model::MetaTaskId::parse(raw_meta_task_id.clone()).map_err(|err| {
            CoveyError::InvalidImportDestination {
                reason: err.to_string(),
            }
        })?;

    let mut records = Vec::with_capacity(source.tasks.len() + 1);
    let mut items = Vec::with_capacity(source.tasks.len() + 1);
    let mut conflicts = Vec::new();
    let mut created = 0usize;
    let mut updated = 0usize;
    let mut unchanged = 0usize;

    let meta_prompt = openspec_meta_prompt(source);
    ensure_length("prompt_text", &meta_prompt, MAX_PROMPT_LEN)?;
    let meta_prompt = crate::model::PromptText::parse(meta_prompt).map_err(|err| {
        CoveyError::InvalidImportDestination {
            reason: err.to_string(),
        }
    })?;
    let meta_provenance = meta_provenance(source, meta_task_id.as_str(), 0);
    let meta_action = match load_meta_task_tx(tx, meta_task_id.as_str()) {
        Ok(existing) => {
            let existing_provenance =
                load_import_provenance_tx(tx, ObjectType::MetaTask, meta_task_id.as_str())?;
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
    records.push(OpenSpecImportRecord::meta_task(
        meta_task_id.clone(),
        meta_prompt.clone(),
        meta_action,
        meta_provenance,
    ));
    items.push(
        ImportOpenSpecItemResult::meta_task(
            meta_task_id.to_string(),
            meta_prompt.to_string(),
            meta_action,
        )
        .map_err(|reason| CoveyError::InvalidImportDestination { reason })?,
    );

    for task in &source.tasks {
        ensure_length("title", task.title.as_str(), MAX_TITLE_LEN)?;
        let raw_subtask_id = openspec_subtask_id(source.change_id.as_str(), task.task_id.as_str());
        validate_deterministic_covey_id("subtask_id", &raw_subtask_id)?;
        let subtask_id = crate::model::SubtaskId::parse(raw_subtask_id.clone()).map_err(|err| {
            CoveyError::InvalidImportDestination {
                reason: err.to_string(),
            }
        })?;
        let provenance = task_provenance(source, task, subtask_id.as_str(), 0);
        let mut action = ImportOpenSpecAction::Created;
        let mut conflict = None;

        match load_subtask_tx(tx, subtask_id.as_str()) {
            Ok(existing) => {
                if existing.meta_task_id != meta_task_id {
                    action = ImportOpenSpecAction::Conflict;
                    conflict = Some(
                        ImportOpenSpecConflict::subtask(
                            subtask_id.to_string(),
                            task.task_id.to_string(),
                            ImportOpenSpecConflictReason::ExistingSubtaskDifferentMetaTask,
                            task.source_path.to_string(),
                            task.task_digest.to_string(),
                        )
                        .map_err(|reason| CoveyError::InvalidImportDestination { reason })?,
                    );
                } else {
                    let existing_provenance =
                        load_import_provenance_tx(tx, ObjectType::Subtask, subtask_id.as_str())?;
                    let changed = existing.title != task.title
                        || !provenance_equivalent(existing_provenance.as_ref(), &provenance);
                    if changed && existing.current_claim_id().is_some() {
                        action = ImportOpenSpecAction::Conflict;
                        conflict = Some(
                            ImportOpenSpecConflict::subtask(
                                subtask_id.to_string(),
                                task.task_id.to_string(),
                                ImportOpenSpecConflictReason::ActiveClaimChangedSource,
                                task.source_path.to_string(),
                                task.task_digest.to_string(),
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
        records.push(OpenSpecImportRecord::subtask(
            subtask_id.clone(),
            task.task_id.clone(),
            task.title.clone(),
            task.dependencies.clone(),
            action,
            provenance,
        ));
        items.push(
            ImportOpenSpecItemResult::subtask(
                subtask_id.to_string(),
                task.task_id.to_string(),
                task.title.to_string(),
                task.task_type
                    .as_ref()
                    .map(|task_type| task_type.as_str().to_owned()),
                task.task_digest.to_string(),
                task.source_path.to_string(),
                action,
            )
            .map_err(|reason| CoveyError::InvalidImportDestination { reason })?,
        );
    }

    let result = ImportOpenSpecResult::new(
        source.change_id.to_string(),
        meta_task_id.to_string(),
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

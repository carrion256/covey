use super::*;

pub(super) fn dispatch_import(store: &Covey, command: ImportCommand) -> covey::Result<Rendered> {
    match command {
        ImportCommand::Bd(args) => {
            let result = store.import_bd_v1(
                ImportBdV1Req::from_flat_selectors(
                    args.session_token.clone(),
                    args.beads_db,
                    args.meta_task_id,
                    args.prompt_text,
                    args.idempotency_key
                        .unwrap_or_else(|| new_idempotency_key("import-bd")),
                )
                .map_err(|error| covey::CoveyError::InvalidImportDestination {
                    reason: error.to_string(),
                })?,
            )?;
            let human = result.human_summary();
            let (meta_task_id, imported_count, skipped_count, items) = result.into_flat_parts();
            Ok(Rendered::summary(
                ImportBdV1Ack {
                    operation: "import_bd",
                    meta_task_id,
                    imported_count,
                    skipped_count,
                    items,
                },
                human,
            ))
        }
        ImportCommand::Openspec(args) => {
            let result = store.import_openspec(
                ImportOpenSpecReq::from_flat_mode(
                    args.session_token,
                    args.change,
                    args.project_root.to_string_lossy().to_string(),
                    args.dry_run,
                )
                .map_err(|error| covey::CoveyError::InvalidImportDestination {
                    reason: error.to_string(),
                })?,
            )?;
            let human = result.human_summary();
            let (change_id, meta_task_id, dry_run, created, updated, unchanged, conflicts, items) =
                result.into_flat_parts();
            Ok(Rendered::summary(
                ImportOpenSpecAck {
                    operation: "import_openspec",
                    change_id,
                    meta_task_id,
                    dry_run,
                    created,
                    updated,
                    unchanged,
                    conflicts,
                    items,
                },
                human,
            ))
        }
        ImportCommand::Provenance(args) => {
            let provenance = store.import_provenance(args.object_type.into(), &args.object_id)?;
            Ok(Rendered::pretty(&provenance))
        }
    }
}

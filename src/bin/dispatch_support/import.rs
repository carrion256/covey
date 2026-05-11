use super::*;

pub(super) fn dispatch_import(store: &Covey, command: ImportCommand) -> covey::Result<Rendered> {
    match command {
        ImportCommand::Bd(args) => {
            let result = store.import_bd_v1(ImportBdV1Req {
                session_token: args.session_token.clone(),
                beads_db_path: args.beads_db,
                meta_task_id: args.meta_task_id,
                prompt_text: args.prompt_text,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("import-bd")),
            })?;
            let human = result.human_summary();
            Ok(Rendered::summary(
                ImportBdV1Ack {
                    operation: "import_bd",
                    meta_task_id: result.meta_task_id.clone(),
                    imported_count: result.imported_count,
                    skipped_count: result.skipped_count,
                    items: result.items,
                },
                human,
            ))
        }
        ImportCommand::Openspec(args) => {
            let result = store.import_openspec(ImportOpenSpecReq {
                session_token: args.session_token,
                change_id: args.change,
                project_root: args.project_root.to_string_lossy().to_string(),
                dry_run: args.dry_run,
            })?;
            let human = result.human_summary();
            Ok(Rendered::summary(
                ImportOpenSpecAck {
                    operation: "import_openspec",
                    change_id: result.change_id,
                    meta_task_id: result.meta_task_id,
                    dry_run: result.dry_run,
                    created: result.created,
                    updated: result.updated,
                    unchanged: result.unchanged,
                    conflicts: result.conflicts,
                    items: result.items,
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

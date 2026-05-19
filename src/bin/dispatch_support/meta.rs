use super::*;

pub(super) fn dispatch_meta(store: &Covey, command: MetaCommand) -> covey::Result<Rendered> {
    match command {
        MetaCommand::Submit(args) => {
            let meta_task_id = store.submit_meta_task(SubmitMetaTaskReq {
                session_token: args.session_token,
                prompt_text: args.prompt_text,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("submit-meta-task")),
            })?;
            Ok(Rendered::summary(
                MetaTaskRef {
                    meta_task_id: meta_task_id.clone(),
                },
                format!("meta_task {}", meta_task_id),
            ))
        }
        MetaCommand::Cancel(args) => {
            store.cancel_meta_task(covey::CancelMetaTaskReq::try_from_raw_parts(
                args.session_token.clone(),
                args.meta_task_id.clone(),
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("cancel-meta-task")),
            )?)?;
            Ok(Rendered::summary(
                MetaTaskAck {
                    operation: "cancel",
                    meta_task_id: args.meta_task_id.clone(),
                },
                format!("meta_task cancelled {}", args.meta_task_id),
            ))
        }
        MetaCommand::Status(args) => {
            let status = store.meta_task_status(&args.meta_task_id)?;
            Ok(Rendered::pretty(&status))
        }
    }
}

use super::*;

pub(super) fn dispatch_meta(store: &Covey, command: MetaCommand) -> covey::Result<Rendered> {
    match command {
        MetaCommand::Submit(args) => {
            let mut request = SubmitMetaTaskReq::try_from_raw_parts(
                args.session_token,
                args.prompt_text,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("submit-meta-task")),
            )?;
            if let (Some(thread_id), Some(received_at), Some(path)) = (
                args.source_thread_id,
                args.source_received_at,
                args.source_path,
            ) {
                request = request.with_source_link(covey::MetaTaskSourceLinkDraft::submitted(
                    thread_id,
                    received_at,
                    path,
                    args.source_url,
                )?);
            }
            let meta_task_id = store.submit_meta_task(request)?;
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
        MetaCommand::LinkSource(args) => {
            store.link_meta_task_source(LinkMetaTaskSourceReq::try_from_raw_parts(
                args.session_token.clone(),
                args.meta_task_id.clone(),
                args.thread_id,
                args.received_at,
                args.path,
                args.url,
                args.method,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("link-meta-task-source")),
            )?)?;
            Ok(Rendered::summary(
                MetaTaskAck {
                    operation: "link_source",
                    meta_task_id: args.meta_task_id.clone(),
                },
                format!("meta_task source linked {}", args.meta_task_id),
            ))
        }
        MetaCommand::Status(args) => {
            let status = store.meta_task_status(&args.meta_task_id)?;
            Ok(Rendered::pretty(&status))
        }
        MetaCommand::List(args) => {
            let summaries = store.list_meta_tasks(args.limit as usize, args.offset)?;
            Ok(Rendered::pretty(&summaries))
        }
    }
}

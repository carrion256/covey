use super::*;

use crate::cli::MetaTaskAnnotationKeyArg;
use crate::render_support::MetaTaskAnnotationAck;

use covey::{
    AnnotateMetaTaskReq, CompleteMetaTaskReq, DueDate, MetaTaskAnnotationChange,
    MetaTaskAnnotationKey, MetaTaskAnnotationValue, MetaTaskClosure, MetaTaskDedupeKey,
    MetaTaskHandle, MetaTaskId, MetaTaskListQuery, MetaTaskTitle, MetaTaskUrgency,
};

pub(super) fn dispatch_meta(store: &Covey, command: MetaCommand) -> covey::Result<Rendered> {
    match command {
        MetaCommand::Submit(args) => {
            let prompt_text = args.prompt_text.or(args.prompt_file).ok_or_else(|| {
                covey::CoveyError::InvalidPath {
                    path: "prompt source selector".to_owned(),
                }
            })?;
            let mut request = SubmitMetaTaskReq::try_from_raw_parts(
                args.session_token,
                prompt_text,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("submit-meta-task")),
            )?;
            let dedupe_key = args.dedupe_key;
            if let Some(title) = args.title {
                request = request.with_title(MetaTaskTitle::parse(title)?);
            }
            if let Some(urgency) = args.urgency {
                request = request.with_urgency(MetaTaskUrgency::from(urgency));
            }
            if let Some(due) = args.due {
                request = request.with_due_on(DueDate::parse(due)?);
            }
            if let Some(dedupe_key) = dedupe_key.clone() {
                request = request.with_dedupe_key(MetaTaskDedupeKey::parse(dedupe_key)?);
            }
            if !args.supersedes.is_empty() {
                let supersedes = args
                    .supersedes
                    .iter()
                    .map(|meta_task_id| MetaTaskId::parse(meta_task_id.as_str()))
                    .collect::<Result<Vec<_>, _>>()?;
                request = request.with_supersedes(supersedes);
            }
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
            let submission = store.submit_meta_task_reporting(request)?;
            let human = if submission.deduplicated {
                match dedupe_key {
                    Some(dedupe_key) => format!(
                        "meta_task {} already filed (dedupe key {dedupe_key})",
                        submission.meta_task_id
                    ),
                    None => format!("meta_task {}", submission.meta_task_id),
                }
            } else {
                format!("meta_task {}", submission.meta_task_id)
            };
            Ok(Rendered::summary(
                MetaTaskRef {
                    meta_task_id: submission.meta_task_id,
                    deduplicated: submission.deduplicated,
                },
                human,
            ))
        }
        MetaCommand::Cancel(args) => {
            let meta_task_id = resolve_meta_task_id(store, args.meta_task_id, args.handle)?;
            let closure = MetaTaskClosure::from(args.closure);
            store.cancel_meta_task(
                covey::CancelMetaTaskReq::try_from_raw_parts(
                    args.session_token.clone(),
                    meta_task_id.clone(),
                    args.idempotency_key
                        .unwrap_or_else(|| new_idempotency_key("cancel-meta-task")),
                )?
                .with_closure(closure),
            )?;
            Ok(Rendered::summary(
                MetaTaskAck {
                    operation: "cancel",
                    meta_task_id: meta_task_id.clone(),
                    closure: Some(closure),
                },
                format!("meta_task cancelled {meta_task_id} ({closure})"),
            ))
        }
        MetaCommand::Complete(args) => {
            let meta_task_id = resolve_meta_task_id(store, args.meta_task_id, args.handle)?;
            let closure = MetaTaskClosure::from(args.closure);
            store.complete_meta_task(
                CompleteMetaTaskReq::try_from_raw_parts(
                    args.session_token.clone(),
                    meta_task_id.clone(),
                    args.idempotency_key
                        .unwrap_or_else(|| new_idempotency_key("complete-meta-task")),
                )?
                .with_closure(closure),
            )?;
            Ok(Rendered::summary(
                MetaTaskAck {
                    operation: "complete",
                    meta_task_id: meta_task_id.clone(),
                    closure: Some(closure),
                },
                format!("meta_task completed {meta_task_id} ({closure})"),
            ))
        }
        MetaCommand::Annotate(args) => {
            let meta_task_id = resolve_meta_task_id(store, args.meta_task_id, args.handle)?;
            let change =
                annotation_change(args.verdict, args.acked, args.tier, args.entity, args.clear)?;
            store.annotate_meta_task(AnnotateMetaTaskReq::try_from_raw_parts(
                args.session_token,
                meta_task_id.clone(),
                change.clone(),
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("annotate-meta-task")),
            )?)?;
            let (key, value) = match &change {
                MetaTaskAnnotationChange::Set(value) => {
                    (value.key(), Some(annotation_value_text(value)))
                }
                MetaTaskAnnotationChange::Clear(key) => (*key, None),
            };
            let key = key.to_string();
            let human = match &value {
                Some(value) => format!("meta_task {meta_task_id} annotation {key} set to {value}"),
                None => format!("meta_task {meta_task_id} annotation {key} cleared"),
            };
            Ok(Rendered::summary(
                MetaTaskAnnotationAck {
                    operation: "annotate",
                    meta_task_id,
                    key,
                    value,
                },
                human,
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
                    closure: None,
                },
                format!("meta_task source linked {}", args.meta_task_id),
            ))
        }
        MetaCommand::Status(args) => {
            let meta_task_id = resolve_meta_task_id(store, args.meta_task_id, args.handle)?;
            let status = store.meta_task_status(&meta_task_id)?;
            Ok(Rendered::pretty(&status))
        }
        MetaCommand::List(args) => {
            let mut query = MetaTaskListQuery::new(args.limit as usize, args.offset);
            if let Some(state) = args.state {
                query = query.with_state(state.into());
            }
            if let Some(due_before) = args.due_before {
                query = query.with_due_before(DueDate::parse(due_before)?);
            }
            if let Some(source_thread_id) = args.source_thread_id {
                query = query.with_source_thread_id(source_thread_id);
            }
            if let Some(dedupe_key) = args.dedupe_key {
                query = query.with_dedupe_key(MetaTaskDedupeKey::parse(dedupe_key)?);
            }
            query = query.with_sort(args.sort.into());
            let summaries = store.list_meta_tasks_with(&query)?;
            Ok(Rendered::pretty(&summaries))
        }
    }
}

/// Resolves the meta-task selector carried by one command.
///
/// Exactly one of the two selectors is accepted by the argument parser, so the
/// empty pair only reports a selector the parser let through.
fn resolve_meta_task_id(
    store: &Covey,
    meta_task_id: Option<String>,
    handle: Option<String>,
) -> covey::Result<String> {
    match (meta_task_id, handle) {
        (Some(meta_task_id), None) => Ok(meta_task_id),
        (None, Some(handle)) => {
            let handle = MetaTaskHandle::parse(handle)?;
            Ok(store.meta_task_id_for_handle(handle)?.to_string())
        }
        _ => Err(covey::CoveyError::InvalidPath {
            path: "meta-task selector".to_owned(),
        }),
    }
}

/// Builds the annotation change described by the mutually exclusive flags.
fn annotation_change(
    verdict: Option<String>,
    acked: Option<bool>,
    tier: Option<String>,
    entity: Option<String>,
    clear: Option<MetaTaskAnnotationKeyArg>,
) -> covey::Result<MetaTaskAnnotationChange> {
    if let Some(key) = clear {
        return Ok(MetaTaskAnnotationChange::Clear(
            MetaTaskAnnotationKey::from(key),
        ));
    }
    if let Some(verdict) = verdict {
        return Ok(MetaTaskAnnotationChange::Set(
            MetaTaskAnnotationValue::verdict(verdict)?,
        ));
    }
    if let Some(acked) = acked {
        return Ok(MetaTaskAnnotationChange::Set(
            MetaTaskAnnotationValue::acked(acked),
        ));
    }
    if let Some(tier) = tier {
        return Ok(MetaTaskAnnotationChange::Set(
            MetaTaskAnnotationValue::tier(tier)?,
        ));
    }
    if let Some(entity) = entity {
        return Ok(MetaTaskAnnotationChange::Set(
            MetaTaskAnnotationValue::entity(entity)?,
        ));
    }
    Err(covey::CoveyError::InvalidPath {
        path: "annotation selector".to_owned(),
    })
}

fn annotation_value_text(value: &MetaTaskAnnotationValue) -> String {
    match value {
        MetaTaskAnnotationValue::Verdict(text)
        | MetaTaskAnnotationValue::Tier(text)
        | MetaTaskAnnotationValue::Entity(text) => text.clone(),
        MetaTaskAnnotationValue::Acked(acked) => acked.to_string(),
    }
}

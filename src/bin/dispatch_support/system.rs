use super::*;
use serde::Serialize;

pub(super) fn dispatch_events(store: &Covey, command: EventsCommand) -> covey::Result<Rendered> {
    match command {
        EventsCommand::List(args) => {
            let events = store.fetch_events(args.after_seq, args.limit)?;
            if args.typed {
                Ok(render_typed_events(&events))
            } else {
                Ok(Rendered::pretty(&events))
            }
        }
    }
}

fn render_typed_events(events: &[covey::Event]) -> Rendered {
    let items: Vec<TypedEventListItem> = events
        .iter()
        .map(|event| match event.typed() {
            Ok(event) => TypedEventListItem::Typed { event },
            Err(error) => TypedEventListItem::DecodeError {
                event: event.clone(),
                error: error.to_string(),
            },
        })
        .collect();
    let decode_error_count = items
        .iter()
        .filter(|item| matches!(item, TypedEventListItem::DecodeError { .. }))
        .count();
    Rendered::pretty(TypedEventList {
        typed_event_count: items.len() - decode_error_count,
        decode_error_count,
        items,
    })
}

#[derive(Serialize)]
struct TypedEventList {
    typed_event_count: usize,
    decode_error_count: usize,
    items: Vec<TypedEventListItem>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
enum TypedEventListItem {
    Typed { event: covey::TypedEvent },
    DecodeError { event: covey::Event, error: String },
}

pub(super) fn dispatch_conflict(
    store: &Covey,
    command: ConflictCommand,
) -> covey::Result<Rendered> {
    match command {
        ConflictCommand::List => {
            let conflicts = store.list_conflicts()?;
            Ok(Rendered::pretty(&conflicts))
        }
        ConflictCommand::Resolve(args) => {
            store.resolve_conflict(ResolveConflictReq::try_from_raw_parts(
                args.session_token,
                args.conflict_id.clone(),
                args.resolution_state.into(),
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("resolve-conflict")),
            )?)?;
            Ok(Rendered::summary(
                ConflictResolutionAck {
                    operation: "resolve",
                    conflict_id: args.conflict_id.clone(),
                    resolution_state: args
                        .resolution_state
                        .to_possible_value()
                        .expect("label")
                        .get_name()
                        .to_owned(),
                },
                format!(
                    "conflict {} {}",
                    args.conflict_id,
                    args.resolution_state
                        .to_possible_value()
                        .expect("label")
                        .get_name()
                ),
            ))
        }
    }
}

pub(super) fn dispatch_maint(store: &Covey, command: MaintCommand) -> covey::Result<Rendered> {
    match command {
        MaintCommand::ReapStale(args) => {
            let result = store.reap_stale_sessions(args.stale_threshold_ms)?;
            Ok(Rendered::summary(
                &result,
                format!("stale_sessions={}", result.stale_sessions),
            ))
        }
        MaintCommand::ExpireClaims => {
            let result = store.expire_old_claims()?;
            Ok(Rendered::summary(
                &result,
                format!("expired_claims={}", result.expired_count),
            ))
        }
        MaintCommand::ExpireReservations => {
            let result = store.expire_old_reservations()?;
            Ok(Rendered::summary(
                &result,
                format!("expired_reservations={}", result.expired_count),
            ))
        }
        MaintCommand::Backup(args) => {
            store.backup_database(&args.output)?;
            Ok(Rendered::summary(
                (),
                format!("backup_written={}", args.output.display()),
            ))
        }
    }
}

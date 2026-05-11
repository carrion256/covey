use super::*;

pub(super) fn dispatch_events(store: &Covey, command: EventsCommand) -> covey::Result<Rendered> {
    match command {
        EventsCommand::List(args) => {
            let events = store.fetch_events(args.after_seq, args.limit)?;
            if args.typed {
                let typed = events
                    .iter()
                    .map(covey::Event::typed)
                    .collect::<covey::Result<Vec<_>>>()?;
                Ok(Rendered::pretty(&typed))
            } else {
                Ok(Rendered::pretty(&events))
            }
        }
    }
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
            store.resolve_conflict(ResolveConflictReq {
                session_token: args.session_token,
                conflict_id: args.conflict_id.clone(),
                resolution_state: args.resolution_state.into(),
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("resolve-conflict")),
            })?;
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
    }
}

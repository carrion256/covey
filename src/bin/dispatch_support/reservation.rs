use super::*;

pub(super) fn dispatch_reservation(
    store: &Covey,
    command: ReservationCommand,
) -> covey::Result<Rendered> {
    match command {
        ReservationCommand::Request(args) => {
            let reservation_id = store.request_reservation(RequestReservationReq {
                session_token: args.session_token,
                owner_subtask_id: args.owner_subtask_id,
                scope_class: args.scope_class.into(),
                scope_key: args.scope_key,
                generated_members: args.generated_members,
                lease_duration_ms: args.lease_duration_ms,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("request-reservation")),
            })?;
            Ok(Rendered::summary(
                ReservationRef {
                    reservation_id: reservation_id.clone(),
                },
                format!("reservation {}", reservation_id),
            ))
        }
        ReservationCommand::Release(args) => {
            store.release_reservation(ReleaseReservationReq {
                session_token: args.session_token,
                reservation_id: args.reservation_id.clone(),
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("release-reservation")),
            })?;
            Ok(Rendered::summary(
                ReservationAck {
                    operation: "release",
                    reservation_id: args.reservation_id.clone(),
                },
                format!("reservation released {}", args.reservation_id),
            ))
        }
        ReservationCommand::Renew(args) => {
            let reservation = store.renew_reservation(RenewReservationReq {
                session_token: args.session_token,
                reservation_id: args.reservation_id,
                extend_by_ms: args.extend_by_ms,
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("renew-reservation")),
            })?;
            Ok(Rendered::summary(
                &reservation,
                format!(
                    "reservation {} lease_deadline={}",
                    reservation.reservation_id, reservation.lease_deadline
                ),
            ))
        }
        ReservationCommand::Overlaps(args) => {
            let overlaps = store.find_overlapping_reservations(OverlapQueryReq {
                scope_class: args.scope_class.into(),
                scope_key: args.scope_key,
                generated_members: args.generated_members,
            })?;
            Ok(Rendered::pretty(&overlaps))
        }
    }
}

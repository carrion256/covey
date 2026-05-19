use super::*;

pub(super) fn dispatch_reservation(
    store: &Covey,
    command: ReservationCommand,
) -> covey::Result<Rendered> {
    match command {
        ReservationCommand::Request(args) => {
            let owner_subtask_id = covey::SubtaskId::parse(args.owner_subtask_id)?;
            let lease_duration_ms = covey::LeaseDurationMs::parse(args.lease_duration_ms)?;
            let req = RequestReservationReq::try_from_parts(
                args.session_token,
                owner_subtask_id,
                args.scope_class.into(),
                args.scope_key,
                args.generated_members,
                lease_duration_ms,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("request-reservation")),
            )
            .map_err(|path| covey::CoveyError::InvalidPath { path })?;
            let reservation_id = store.request_reservation(req)?;
            Ok(Rendered::summary(
                ReservationRef {
                    reservation_id: reservation_id.clone(),
                },
                format!("reservation {}", reservation_id),
            ))
        }
        ReservationCommand::Release(args) => {
            store.release_reservation(ReleaseReservationReq::try_from_raw_parts(
                args.session_token,
                args.reservation_id.clone(),
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("release-reservation")),
            )?)?;
            Ok(Rendered::summary(
                ReservationAck {
                    operation: "release",
                    reservation_id: args.reservation_id.clone(),
                },
                format!("reservation released {}", args.reservation_id),
            ))
        }
        ReservationCommand::Renew(args) => {
            let reservation = store.renew_reservation(RenewReservationReq::try_from_raw_parts(
                args.session_token,
                args.reservation_id,
                args.extend_by_ms,
                args.idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("renew-reservation")),
            )?)?;
            Ok(Rendered::summary(
                &reservation,
                format!(
                    "reservation {} lease_deadline={}",
                    reservation.reservation_id, reservation.lease_deadline
                ),
            ))
        }
        ReservationCommand::Overlaps(args) => {
            let req = OverlapQueryReq::try_from_parts(
                args.scope_class.into(),
                args.scope_key,
                args.generated_members,
            )
            .map_err(|path| covey::CoveyError::InvalidPath { path })?;
            let overlaps = store.find_overlapping_reservations(req)?;
            Ok(Rendered::pretty(&overlaps))
        }
    }
}

use std::time::Instant;

use rusqlite::params;

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        Conflict, Event, EventType, ObjectType, OverlapCandidate, OverlapQueryReq, Reservation,
        ReservationState, ResolveConflictReq, ScopeClass, SessionRole,
    },
    overlap::{
        find_overlapping_reservations_conn, find_overlapping_reservations_tx,
        normalize_generated_members, normalize_scope_key, record_reservation_overlap_conflicts,
        resolve_reservation_overlap_conflicts,
    },
    queries::{collect_rows, deserialize_row, load_reservation_tx, map_event},
    schema::advance_lease_clock,
    store::append_session_event,
    validators::{
        MAX_OBJECT_ID_LEN, MAX_PATH_LEN, ensure_generated_member_count, ensure_length,
        ensure_positive_lease_duration, ensure_reservation_transition, ensure_subtask_exists,
        require_role,
    },
};

impl Covey {
    /// Creates an advisory reservation and records any overlap conflicts it surfaces.
    pub fn request_reservation(&self, req: crate::model::RequestReservationReq) -> Result<String> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "request_reservation",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::Orchestrator])?;
                    ensure_positive_lease_duration("lease_duration_ms", req.lease_duration_ms)?;
                    ensure_length("owner_subtask_id", &req.owner_subtask_id, MAX_OBJECT_ID_LEN)?;
                    ensure_subtask_exists(tx, &req.owner_subtask_id)?;
                    ensure_generated_member_count(req.generated_members.len())?;
                    let normalized_scope_key = normalize_scope_key(req.scope_class, &req.scope_key)?;
                    ensure_length("reservation_scope_key", &normalized_scope_key, MAX_PATH_LEN)?;
                    let normalized_members = normalize_generated_members(&req.generated_members)?;
                    ensure_reservation_scope_shape(
                        req.scope_class,
                        &normalized_scope_key,
                        &normalized_members,
                    )?;
                    for member in &normalized_members {
                        ensure_length("generated_member", member, MAX_PATH_LEN)?;
                    }
                    let overlaps = find_overlapping_reservations_tx(
                        tx,
                        &OverlapCandidate::new(
                            req.scope_class,
                            normalized_scope_key.clone(),
                            normalized_members.clone(),
                        ),
                    )?;
                    let reservation_id = crate::model::make_id("reservation");
                    tx.execute(
                        r#"
                        INSERT INTO reservations (
                            reservation_id, owner_subtask_id, scope_class, scope_key, lease_deadline, state, created_at, updated_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                        "#,
                        params![
                            reservation_id,
                            req.owner_subtask_id,
                            req.scope_class.to_string(),
                            normalized_scope_key,
                            lease_now + req.lease_duration_ms,
                            ReservationState::Active.to_string(),
                            now
                        ],
                    )?;
                    for member in &normalized_members {
                        tx.execute(
                            "INSERT INTO reservation_generated_members (reservation_id, member_path) VALUES (?1, ?2)",
                            params![reservation_id, member],
                        )?;
                    }
                    record_reservation_overlap_conflicts(tx, &reservation_id, &req, &overlaps, now)?;
                    append_session_event(
                        tx,
                        EventType::ReservationRequested,
                        ObjectType::Reservation,
                        &reservation_id,
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(reservation_id)
                },
            )
        });
        self.log_operation(
            "request_reservation",
            &req.session_token,
            started_at,
            &result,
            |reservation_id| {
                vec![
                    format!("reservation:{reservation_id}"),
                    format!("subtask:{}", req.owner_subtask_id),
                ]
            },
        );
        result
    }

    /// Releases an active reservation.
    pub fn release_reservation(&self, req: crate::model::ReleaseReservationReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "release_reservation",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::Orchestrator])?;
                    ensure_length("reservation_id", &req.reservation_id, MAX_OBJECT_ID_LEN)?;
                    let reservation = load_reservation_tx(tx, &req.reservation_id)?;
                    ensure_reservation_transition(reservation.state, ReservationState::Released)?;
                    let updated = tx.execute(
                        "UPDATE reservations SET state = ?2, updated_at = ?3 WHERE reservation_id = ?1 AND state = ?4",
                        params![
                            req.reservation_id,
                            ReservationState::Released.to_string(),
                            now,
                            ReservationState::Active.to_string()
                        ],
                    )?;
                    if updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: reservation.state.into(),
                            to: ReservationState::Released.into(),
                            object: ObjectType::Reservation,
                        });
                    }
                    resolve_reservation_overlap_conflicts(tx, &req.reservation_id, now)?;
                    let scope_class = reservation.scope_class();
                    let scope_key = reservation.scope_key().to_owned();
                    let generated_members = reservation.generated_members().to_vec();
                    let released = Reservation::try_from_parts(
                        reservation.reservation_id,
                        reservation.owner_subtask_id,
                        scope_class,
                        scope_key,
                        generated_members,
                        reservation.lease_deadline,
                        ReservationState::Released,
                        reservation.created_at,
                        crate::model::TimestampMs::parse(now)
                            .expect("wall clock timestamps are non-negative"),
                    )
                    .expect("existing reservation scope remains valid after release");
                    append_session_event(
                        tx,
                        EventType::ReservationReleased,
                        ObjectType::Reservation,
                        &req.reservation_id,
                        &req.session_token,
                        &released,
                        now,
                    )?;
                    Ok(())
                },
            )
        });
        self.log_operation(
            "release_reservation",
            &req.session_token,
            started_at,
            &result,
            |_| vec![format!("reservation:{}", req.reservation_id)],
        );
        result
    }

    /// Renews an active reservation lease.
    pub fn renew_reservation(&self, req: crate::model::RenewReservationReq) -> Result<Reservation> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "renew_reservation",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::Orchestrator])?;
                    ensure_positive_lease_duration("extend_by_ms", req.extend_by_ms)?;
                    ensure_length("reservation_id", &req.reservation_id, MAX_OBJECT_ID_LEN)?;
                    let reservation = load_reservation_tx(tx, &req.reservation_id)?;
                    if reservation.state != ReservationState::Active {
                        return Err(CoveyError::IllegalTransition {
                            from: reservation.state.into(),
                            to: ReservationState::Active.into(),
                            object: ObjectType::Reservation,
                        });
                    }
                    if reservation.lease_deadline <= lease_now {
                        return Err(CoveyError::LeaseExpired {
                            object_id: reservation.reservation_id.to_string(),
                        });
                    }
                    let renewed_deadline =
                        reservation.lease_deadline.get().max(lease_now) + req.extend_by_ms;
                    let updated = tx.execute(
                        "UPDATE reservations SET lease_deadline = ?2, updated_at = ?3 WHERE reservation_id = ?1 AND state = ?4",
                        params![
                            req.reservation_id,
                            renewed_deadline,
                            now,
                            ReservationState::Active.to_string()
                        ],
                    )?;
                    if updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: reservation.state.into(),
                            to: reservation.state.into(),
                            object: ObjectType::Reservation,
                        });
                    }
                    let scope_class = reservation.scope_class();
                    let scope_key = reservation.scope_key().to_owned();
                    let generated_members = reservation.generated_members().to_vec();
                    let renewed = Reservation::try_from_parts(
                        reservation.reservation_id,
                        reservation.owner_subtask_id,
                        scope_class,
                        scope_key,
                        generated_members,
                        crate::model::LeaseDeadlineMs::parse(renewed_deadline)
                            .expect("renewed lease deadlines are non-negative"),
                        ReservationState::Active,
                        reservation.created_at,
                        crate::model::TimestampMs::parse(now)
                            .expect("wall clock timestamps are non-negative"),
                    )
                    .expect("existing reservation scope remains valid after renewal");
                    append_session_event(
                        tx,
                        EventType::ReservationRenewed,
                        ObjectType::Reservation,
                        &renewed.reservation_id,
                        &req.session_token,
                        &renewed,
                        now,
                    )?;
                    Ok(renewed)
                },
            )
        });
        self.log_operation(
            "renew_reservation",
            &req.session_token,
            started_at,
            &result,
            |reservation| vec![format!("reservation:{}", reservation.reservation_id)],
        );
        result
    }

    /// Returns active reservations that overlap a proposed reservation candidate.
    pub fn find_overlapping_reservations(&self, req: OverlapQueryReq) -> Result<Vec<Reservation>> {
        let started_at = Instant::now();
        let normalized_scope_key = normalize_scope_key(req.scope_class, &req.scope_key)?;
        ensure_length("reservation_scope_key", &normalized_scope_key, MAX_PATH_LEN)?;
        ensure_generated_member_count(req.generated_members.len())?;
        let normalized_members = normalize_generated_members(&req.generated_members)?;
        ensure_reservation_scope_shape(
            req.scope_class,
            &normalized_scope_key,
            &normalized_members,
        )?;
        for member in &normalized_members {
            ensure_length("generated_member", member, MAX_PATH_LEN)?;
        }
        let candidate =
            OverlapCandidate::new(req.scope_class, normalized_scope_key, normalized_members);
        let result =
            self.with_read_conn(|conn| find_overlapping_reservations_conn(conn, &candidate));
        self.log_operation(
            "find_overlapping_reservations",
            "system",
            started_at,
            &result,
            |reservations: &Vec<Reservation>| {
                reservations
                    .iter()
                    .map(|reservation| format!("reservation:{}", reservation.reservation_id))
                    .collect()
            },
        );
        result
    }

    /// Fetches event-log rows with sequence numbers greater than `after_seq`.
    pub fn fetch_events(&self, after_seq: i64, limit: usize) -> Result<Vec<Event>> {
        let started_at = Instant::now();
        let result = self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT seq, event_type, object_type, object_id, actor_kind, session_token, payload_json, created_at
                FROM event_log
                WHERE seq > ?1
                ORDER BY seq
                LIMIT ?2
                "#,
            )?;
            let rows = stmt.query_map(params![after_seq, limit as i64], map_event)?;
            collect_rows(rows)
        });
        self.log_operation("fetch_events", "system", started_at, &result, |events| {
            events
                .iter()
                .map(|event| format!("event_seq:{}", event.seq))
                .collect()
        });
        result
    }

    /// Lists surfaced conflicts, bounded to avoid report-style unbounded reads.
    pub fn list_conflicts(&self) -> Result<Vec<Conflict>> {
        let started_at = Instant::now();
        let result = self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT conflict_id, object_type, object_id, conflict_kind, payload_json, detected_at, resolution_state
                FROM conflicts
                ORDER BY detected_at DESC
                LIMIT ?1
                "#,
            )?;
            let rows = stmt.query_map(
                params![crate::store::LIST_CONFLICTS_LIMIT as i64],
                deserialize_row::<Conflict>,
            )?;
            collect_rows(rows)
        });
        self.log_operation(
            "list_conflicts",
            "system",
            started_at,
            &result,
            |conflicts| {
                conflicts
                    .iter()
                    .map(|conflict| format!("conflict:{}", conflict.conflict_id))
                    .collect()
            },
        );
        result
    }

    /// Updates the resolution state of a surfaced conflict.
    pub fn resolve_conflict(&self, req: ResolveConflictReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "resolve_conflict",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::Orchestrator])?;
                    ensure_length("conflict_id", &req.conflict_id, MAX_OBJECT_ID_LEN)?;
                    let updated = tx.execute(
                        "UPDATE conflicts SET resolution_state = ?2 WHERE conflict_id = ?1",
                        params![req.conflict_id, req.resolution_state.to_string()],
                    )?;
                    if updated == 0 {
                        return Err(CoveyError::ConflictNotFound);
                    }
                    append_session_event(
                        tx,
                        EventType::ConflictResolved,
                        ObjectType::Conflict,
                        &req.conflict_id,
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(())
                },
            )
        });
        self.log_operation(
            "resolve_conflict",
            &req.session_token,
            started_at,
            &result,
            |_| vec![format!("conflict:{}", req.conflict_id)],
        );
        result
    }
}

fn ensure_reservation_scope_shape(
    scope_class: ScopeClass,
    scope_key: &str,
    generated_members: &[String],
) -> Result<()> {
    if scope_key.trim().is_empty() {
        return Err(CoveyError::InvalidPath {
            path: scope_key.to_owned(),
        });
    }
    match scope_class {
        ScopeClass::ExactPath | ScopeClass::Subtree | ScopeClass::RepoGlobal => {
            if generated_members.is_empty() {
                Ok(())
            } else {
                Err(CoveyError::InvalidPath {
                    path: format!("{scope_class} reservation must not include generated_members"),
                })
            }
        }
        ScopeClass::GeneratedSet => {
            if generated_members.is_empty() {
                Err(CoveyError::InvalidPath {
                    path: "generated_set reservation requires generated_members".to_owned(),
                })
            } else {
                Ok(())
            }
        }
    }
}

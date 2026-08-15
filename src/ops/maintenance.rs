use std::path::Path;
use std::time::Instant;

use rusqlite::params;

use crate::{
    Covey,
    error::Result,
    model::{
        ClaimState, EventType, ExpireResult, ExpiredCountPayload, ObjectType, ReapResult,
        ReservationState, SessionState, StaleSessionsPayload, SubtaskState,
    },
    overlap::resolve_reservation_overlap_conflicts,
    queries::{load_expirable_claims, load_expired_reservation_ids},
    schema::advance_lease_clock,
    store::{append_system_event, reset_in_progress_review_for_expired_claim},
    validators::close_claim_and_detach,
};

#[inline]
fn session_state_name(state: SessionState) -> &'static str {
    match state {
        SessionState::Active => "active",
        SessionState::Stale => "stale",
        SessionState::Exited => "exited",
    }
}

#[inline]
fn subtask_state_name(state: SubtaskState) -> &'static str {
    match state {
        SubtaskState::Available => "available",
        SubtaskState::Claimed => "claimed",
        SubtaskState::InProgress => "in_progress",
        SubtaskState::ArtifactPublished => "artifact_published",
        SubtaskState::ReviewPending => "review_pending",
        SubtaskState::Approved => "approved",
        SubtaskState::ChangesRequested => "changes_requested",
        SubtaskState::Blocked => "blocked",
        SubtaskState::Decided => "decided",
        SubtaskState::ReadyForApply => "ready_for_apply",
        SubtaskState::Applied => "applied",
        SubtaskState::Completed => "completed",
        SubtaskState::Failed => "failed",
        SubtaskState::Abandoned => "abandoned",
    }
}

#[inline]
fn reservation_state_name(state: ReservationState) -> &'static str {
    match state {
        ReservationState::Active => "active",
        ReservationState::Released => "released",
        ReservationState::Expired => "expired",
    }
}

impl Covey {
    /// Marks sessions stale when their heartbeat age exceeds the provided threshold.
    pub fn reap_stale_sessions(&self, stale_threshold_ms: i64) -> Result<ReapResult> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            let stale_before = lease_now - stale_threshold_ms;
            let stale_sessions = tx.execute(
                "UPDATE sessions SET state = ?1, updated_at = ?2 WHERE state = ?3 AND last_heartbeat_tick <= ?4",
                params![
                    session_state_name(SessionState::Stale),
                    now,
                    session_state_name(SessionState::Active),
                    stale_before
                ],
            )?;
            let stale_claims = load_expirable_claims(tx, lease_now)?;
            for claim in &stale_claims {
                close_claim_and_detach(tx, claim, ClaimState::Expired, now)?;
                reset_in_progress_review_for_expired_claim(tx, &claim.subtask_id, now)?;
                tx.execute(
                    "UPDATE subtasks SET current_claim_id = NULL, state = CASE WHEN state IN (?3, ?4) THEN ?5 ELSE state END, updated_at = ?6 WHERE subtask_id = ?1 AND current_claim_id = ?2",
                    params![
                        claim.subtask_id,
                        claim.claim_id,
                        subtask_state_name(SubtaskState::Claimed),
                        subtask_state_name(SubtaskState::InProgress),
                        subtask_state_name(SubtaskState::Available),
                        now
                    ],
                )?;
                tx.execute(
                    "UPDATE sessions SET active_subtask_id = NULL, updated_at = ?3 WHERE session_token = ?1 AND active_subtask_id = ?2",
                    params![claim.owner_session_token, claim.subtask_id, now],
                )?;
            }
            if stale_sessions > 0 {
                append_system_event(
                    tx,
                    EventType::SessionsReaped,
                    ObjectType::Session,
                    "stale",
                    &StaleSessionsPayload::new(stale_sessions),
                    now,
                )?;
            }
            Ok(ReapResult::new(stale_sessions))
        });
        self.log_operation("reap_stale_sessions", "system", started_at, &result, |_| {
            Vec::<String>::new()
        });
        result
    }

    /// Expires held claims whose leases elapsed or whose owning sessions are no longer active.
    pub fn expire_old_claims(&self) -> Result<ExpireResult> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            let stale_claims = load_expirable_claims(tx, lease_now)?;
            for claim in &stale_claims {
                close_claim_and_detach(tx, claim, ClaimState::Expired, now)?;
                reset_in_progress_review_for_expired_claim(tx, &claim.subtask_id, now)?;
                tx.execute(
                    "UPDATE subtasks SET current_claim_id = NULL, state = CASE WHEN state IN (?3, ?4) THEN ?5 ELSE state END, updated_at = ?6 WHERE subtask_id = ?1 AND current_claim_id = ?2",
                    params![
                        claim.subtask_id,
                        claim.claim_id,
                        subtask_state_name(SubtaskState::Claimed),
                        subtask_state_name(SubtaskState::InProgress),
                        subtask_state_name(SubtaskState::Available),
                        now
                    ],
                )?;
                tx.execute(
                    "UPDATE sessions SET active_subtask_id = NULL, updated_at = ?3 WHERE session_token = ?1 AND active_subtask_id = ?2",
                    params![claim.owner_session_token, claim.subtask_id, now],
                )?;
            }
            if !stale_claims.is_empty() {
                append_system_event(
                    tx,
                    EventType::ClaimsExpired,
                    ObjectType::Claim,
                    "expired",
                    &ExpiredCountPayload::new(stale_claims.len()),
                    now,
                )?;
            }
            Ok(ExpireResult::new(stale_claims.len()))
        });
        self.log_operation("expire_old_claims", "system", started_at, &result, |_| {
            Vec::<String>::new()
        });
        result
    }

    /// Expires active reservations whose leases have elapsed.
    pub fn expire_old_reservations(&self) -> Result<ExpireResult> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            let expired_reservation_ids = load_expired_reservation_ids(tx, lease_now)?;
            let expired = tx.execute(
                "UPDATE reservations SET state = ?1, updated_at = ?2 WHERE state = ?3 AND lease_deadline <= ?4",
                params![
                    reservation_state_name(ReservationState::Expired),
                    now,
                    reservation_state_name(ReservationState::Active),
                    lease_now
                ],
            )?;
            for reservation_id in &expired_reservation_ids {
                resolve_reservation_overlap_conflicts(tx, reservation_id, now)?;
            }
            if expired > 0 {
                append_system_event(
                    tx,
                    EventType::ReservationsExpired,
                    ObjectType::Reservation,
                    "expired",
                    &ExpiredCountPayload::new(expired),
                    now,
                )?;
            }
            Ok(ExpireResult::new(expired))
        });
        self.log_operation(
            "expire_old_reservations",
            "system",
            started_at,
            &result,
            |_| Vec::<String>::new(),
        );
        result
    }
    /// Backs up the database to a fresh SQLite file via VACUUM INTO.
    ///
    /// The backup is a consistent snapshot produced without writing to the
    /// source database; the destination must not exist yet.
    pub fn backup_database(&self, output_path: &Path) -> Result<()> {
        let escaped = output_path.display().to_string().replace('\'', "''");
        self.with_read_conn(|conn| {
            conn.execute_batch(&format!("VACUUM INTO '{escaped}'"))?;
            Ok(())
        })
    }
}

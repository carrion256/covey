use std::{str::FromStr, time::Instant};

use rusqlite::{OptionalExtension, Row, params};

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        Claim, ClaimState, ExpiringClaim, Session, SessionRole, SessionState, StuckSubtask,
        SubtaskKind, SubtaskRow, SubtaskState, SubtaskStatus, SubtaskView, claim_state_name,
    },
    queries::{
        collect_rows, load_artifact_tx, load_claim_tx, load_queue_items_for_subtask_tx,
        load_reviews_for_subtask_tx, load_subtask_tx,
    },
};

impl Covey {
    /// Returns the current persisted status for a subtask and its related rows.
    pub fn subtask_status(&self, subtask_id: &str) -> Result<SubtaskStatus> {
        let started_at = Instant::now();
        let result = self.with_read_tx(|tx| {
            let subtask = load_subtask_tx(tx, subtask_id)?;
            let claim = subtask
                .current_claim_id()
                .map(AsRef::as_ref)
                .map(|claim_id| load_claim_tx(tx, claim_id))
                .transpose()?;
            let artifact = subtask
                .artifact_digest()
                .map(AsRef::as_ref)
                .map(|artifact_digest| load_artifact_tx(tx, artifact_digest))
                .transpose()?;
            let reviews = load_reviews_for_subtask_tx(tx, subtask_id)?;
            let ready_queue = load_queue_items_for_subtask_tx(tx, subtask_id)?;
            SubtaskStatus::new(
                SubtaskView::try_from(subtask)?,
                claim,
                artifact,
                reviews,
                ready_queue,
            )
            .map_err(|reason| CoveyError::InvalidObservabilityRow { reason })
        });
        self.log_operation("subtask_status", "system", started_at, &result, |status| {
            vec![format!("subtask:{}", status.subtask().subtask_id)]
        });
        result
    }

    /// Lists non-terminal subtasks that have not advanced within the provided age bound.
    pub fn list_stuck_subtasks(
        &self,
        older_than_ms: i64,
        limit: usize,
    ) -> Result<Vec<StuckSubtask>> {
        let started_at = Instant::now();
        let now = self.clock.wall_now_ms();
        let cutoff = now - older_than_ms.max(0);
        let result = self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT st.subtask_id, st.meta_task_id, st.title, st.kind,
                       st.review_target_subtask_id, st.review_target_artifact_digest,
                       st.state, st.current_claim_id, st.artifact_digest, st.priority,
                       st.created_at, st.updated_at,
                       c.claim_id, c.subtask_id, c.owner_session_token, c.fence_seq,
                       c.lease_deadline, c.state, c.created_at, c.updated_at,
                       s.session_token, s.agent_principal_id, s.agent_instance_id,
                       s.role, s.state, s.active_subtask_id, s.last_heartbeat_at,
                       s.last_heartbeat_tick, s.created_at, s.updated_at
                FROM subtasks st
                LEFT JOIN claims c ON c.claim_id = st.current_claim_id
                LEFT JOIN sessions s ON s.session_token = c.owner_session_token
                WHERE st.state NOT IN ('available', 'applied', 'abandoned', 'decided')
                  AND st.updated_at <= ?1
                ORDER BY st.updated_at ASC
                LIMIT ?2
                "#,
            )?;
            let mut rows = stmt.query(params![cutoff, limit as i64])?;
            let mut stuck = Vec::new();
            while let Some(row) = rows.next()? {
                stuck.push(stuck_subtask_from_joined_row(row, now)?);
            }
            Ok(stuck)
        });
        self.log_operation(
            "list_stuck_subtasks",
            "system",
            started_at,
            &result,
            |stuck: &Vec<StuckSubtask>| {
                stuck
                    .iter()
                    .map(|row| format!("subtask:{}", row.subtask().subtask_id))
                    .collect()
            },
        );
        result
    }

    /// Lists held claims whose leases will expire within the provided horizon.
    pub fn list_expiring_claims(&self, within_ms: i64, limit: usize) -> Result<Vec<ExpiringClaim>> {
        let started_at = Instant::now();
        let wall_now = self.clock.wall_now_ms().max(0);
        let result = self.with_read_conn(|conn| {
            let lease_now = conn
                .query_row(
                    "SELECT last_tick_ms FROM lease_clock WHERE clock_id = 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .unwrap_or(0)
                .max(wall_now);
            let lease_cutoff = lease_now + within_ms.max(0);
            let mut stmt = conn.prepare(
                r#"
                SELECT c.claim_id, c.subtask_id, c.owner_session_token, c.fence_seq,
                       c.lease_deadline, c.state, c.created_at, c.updated_at,
                       st.subtask_id, st.meta_task_id, st.title, st.kind,
                       st.review_target_subtask_id, st.review_target_artifact_digest,
                       st.state, st.current_claim_id, st.artifact_digest, st.priority,
                       st.created_at, st.updated_at,
                       s.session_token, s.agent_principal_id, s.agent_instance_id,
                       s.role, s.state, s.active_subtask_id, s.last_heartbeat_at,
                       s.last_heartbeat_tick, s.created_at, s.updated_at
                FROM claims c
                JOIN subtasks st ON st.subtask_id = c.subtask_id
                JOIN sessions s ON s.session_token = c.owner_session_token
                WHERE c.state = ?1 AND c.lease_deadline <= ?2
                ORDER BY c.lease_deadline ASC
                LIMIT ?3
                "#,
            )?;
            let rows = stmt.query_map(
                params![
                    claim_state_name(ClaimState::Held),
                    lease_cutoff,
                    limit as i64
                ],
                |row| expiring_claim_from_joined_row(row, lease_now),
            )?;
            collect_rows(rows)
        });
        self.log_operation(
            "list_expiring_claims",
            "system",
            started_at,
            &result,
            |claims: &Vec<ExpiringClaim>| {
                claims
                    .iter()
                    .map(|row| format!("claim:{}", row.claim().claim_id))
                    .collect()
            },
        );
        result
    }
}

fn stuck_subtask_from_joined_row(row: &Row<'_>, now: i64) -> Result<StuckSubtask> {
    let subtask = SubtaskRow::try_from_parts(
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        parse_enum::<SubtaskKind>(row.get::<_, String>(3)?)?,
        row.get(4)?,
        row.get(5)?,
        parse_enum::<SubtaskState>(row.get::<_, String>(6)?)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
    )?;
    let expects_claim = subtask.current_claim_id().is_some();
    let idle_for_ms = (now - subtask.updated_at().get()).max(0);
    let claim_id = row.get::<_, Option<crate::model::ClaimId>>(12)?;
    let claim = claim_id
        .map(|claim_id| {
            Claim::try_from_parts(
                claim_id,
                row.get(13)?,
                row.get(14)?,
                row.get(15)?,
                row.get(16)?,
                parse_enum(row.get::<_, String>(17)?)?,
                row.get(18)?,
                row.get(19)?,
            )
            .map_err(to_sql_err)
        })
        .transpose()?;
    if expects_claim && claim.is_none() {
        return Err(CoveyError::ClaimNotFound);
    }

    let session_token = row.get::<_, Option<crate::model::SessionToken>>(20)?;
    let session = session_token
        .map(|session_token| {
            Session::try_from_parts(
                session_token,
                row.get::<_, String>(21)?,
                row.get::<_, String>(22)?,
                parse_enum::<SessionRole>(row.get::<_, String>(23)?)?,
                parse_enum::<SessionState>(row.get::<_, String>(24)?)?,
                row.get(25)?,
                row.get(26)?,
                row.get(27)?,
                row.get(28)?,
                row.get(29)?,
            )
            .map_err(to_sql_err)
        })
        .transpose()?;
    if claim.is_some() && session.is_none() {
        return Err(CoveyError::SessionNotFound);
    }

    StuckSubtask::new(SubtaskView::try_from(subtask)?, claim, session, idle_for_ms)
        .map_err(|reason| CoveyError::InvalidObservabilityRow { reason })
}

fn expiring_claim_from_joined_row(
    row: &Row<'_>,
    lease_now: i64,
) -> rusqlite::Result<ExpiringClaim> {
    let claim = Claim::try_from_parts(
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        parse_enum(row.get::<_, String>(5)?)?,
        row.get(6)?,
        row.get(7)?,
    )
    .map_err(to_sql_err)?;
    let subtask = SubtaskRow::try_from_parts(
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        parse_enum::<SubtaskKind>(row.get::<_, String>(11)?)?,
        row.get(12)?,
        row.get(13)?,
        parse_enum::<SubtaskState>(row.get::<_, String>(14)?)?,
        row.get(15)?,
        row.get(16)?,
        row.get(17)?,
        row.get(18)?,
        row.get(19)?,
    )?;
    let session = Session::try_from_parts(
        row.get(20)?,
        row.get::<_, String>(21)?,
        row.get::<_, String>(22)?,
        parse_enum::<SessionRole>(row.get::<_, String>(23)?)?,
        parse_enum::<SessionState>(row.get::<_, String>(24)?)?,
        row.get(25)?,
        row.get(26)?,
        row.get(27)?,
        row.get(28)?,
        row.get(29)?,
    )
    .map_err(to_sql_err)?;
    let expires_in_ms = (claim.lease_deadline.get() - lease_now).max(0);
    ExpiringClaim::new(
        claim,
        SubtaskView::try_from(subtask)?,
        session,
        expires_in_ms,
    )
    .map_err(to_sql_err)
}

fn parse_enum<T>(raw: String) -> rusqlite::Result<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    T::from_str(&raw).map_err(to_sql_err)
}

fn to_sql_err(error: impl ToString) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.to_string(),
        )),
    )
}

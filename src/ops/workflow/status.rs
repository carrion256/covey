use std::time::Instant;

use rusqlite::{OptionalExtension, params};

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        Claim, ClaimState, ExpiringClaim, StuckSubtask, SubtaskRow, SubtaskState, SubtaskStatus,
        SubtaskView,
    },
    queries::{
        collect_rows, deserialize_row, load_artifact_tx, load_claim_conn, load_claim_tx,
        load_queue_items_for_subtask_tx, load_reviews_for_subtask_tx, load_session_conn,
        load_subtask_conn, load_subtask_tx,
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
                SELECT subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest,
                       state, current_claim_id, artifact_digest, priority, created_at, updated_at
                FROM subtasks
                WHERE state NOT IN (?1, ?2, ?3, ?4)
                  AND updated_at <= ?5
                ORDER BY updated_at ASC
                LIMIT ?6
                "#,
            )?;
            let rows = stmt.query_map(
                params![
                    SubtaskState::Available.to_string(),
                    SubtaskState::Applied.to_string(),
                    SubtaskState::Abandoned.to_string(),
                    SubtaskState::Decided.to_string(),
                    cutoff,
                    limit as i64,
                ],
                deserialize_row::<SubtaskRow>,
            )?;
            let subtasks = collect_rows(rows)?;
            subtasks
                .into_iter()
                .map(|subtask| {
                    let claim = subtask
                        .current_claim_id()
                        .map(AsRef::as_ref)
                        .map(|claim_id| load_claim_conn(conn, claim_id))
                        .transpose()?;
                    let session = claim
                        .as_ref()
                        .map(|held| load_session_conn(conn, &held.owner_session_token))
                        .transpose()?;
                    let idle_for_ms = (now - subtask.updated_at().get()).max(0);
                    StuckSubtask::new(
                        SubtaskView::try_from(subtask)?,
                        claim,
                        session,
                        idle_for_ms,
                    )
                    .map_err(|reason| CoveyError::InvalidObservabilityRow { reason })
                })
                .collect()
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
                SELECT claim_id, subtask_id, owner_session_token, fence_seq, lease_deadline, state, created_at, updated_at
                FROM claims
                WHERE state = ?1 AND lease_deadline <= ?2
                ORDER BY lease_deadline ASC
                LIMIT ?3
                "#,
            )?;
            let rows = stmt.query_map(
                params![ClaimState::Held.to_string(), lease_cutoff, limit as i64],
                deserialize_row::<Claim>,
            )?;
            let claims = collect_rows(rows)?;
            claims
                .into_iter()
                .map(|claim| {
                    let subtask = SubtaskView::try_from(load_subtask_conn(conn, &claim.subtask_id)?)?;
                    let session = load_session_conn(conn, &claim.owner_session_token)?;
                    ExpiringClaim::new(
                        claim.clone(),
                        subtask,
                        session,
                        (claim.lease_deadline.get() - lease_now).max(0),
                    )
                    .map_err(|reason| CoveyError::InvalidObservabilityRow { reason })
                })
                .collect()
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

use std::{str::FromStr, time::Instant};

use rusqlite::{OptionalExtension, Row, params};

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        Claim, ClaimState, ClaimableSubtaskAvailability, ExpiringClaim, Session, SessionRole,
        SessionState, StuckSubtask, SubtaskCandidate, SubtaskKind, SubtaskRow, SubtaskState,
        SubtaskStatus, SubtaskView, claim_state_name, meta_task_state_name, subtask_kind_name,
        subtask_state_name,
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
            let landing_receipt_recorded = landing_receipt_exists_for_subtask_tx(tx, subtask_id)?;
            SubtaskStatus::new_with_landing_receipt(
                SubtaskView::try_from(subtask)?,
                claim,
                artifact,
                reviews,
                ready_queue,
                landing_receipt_recorded,
            )
            .map_err(|reason| CoveyError::InvalidObservabilityRow { reason })
        });
        self.log_operation("subtask_status", "system", started_at, &result, |status| {
            vec![format!("subtask:{}", status.subtask().subtask_id)]
        });
        result
    }

    /// Returns read-only counts for currently claimable executor and reviewer subtasks.
    pub fn claimable_subtask_availability(
        &self,
        meta_task_id: Option<&str>,
    ) -> Result<ClaimableSubtaskAvailability> {
        let started_at = Instant::now();
        let result = self.with_read_tx(|tx| {
            let executor_claimable_count =
                claimable_subtask_count_tx(tx, SubtaskKind::Work, meta_task_id)?;
            let reviewer_claimable_count =
                claimable_subtask_count_tx(tx, SubtaskKind::Review, meta_task_id)?;
            Ok(ClaimableSubtaskAvailability::new(
                executor_claimable_count,
                reviewer_claimable_count,
            ))
        });
        self.log_operation(
            "claimable_subtask_availability",
            "system",
            started_at,
            &result,
            |_| Vec::new(),
        );
        result
    }

    /// Returns read-only, ordered subtask candidates for deterministic scheduling.
    pub fn subtask_candidates(
        &self,
        role: SessionRole,
        limit: usize,
        meta_task_id: Option<&str>,
    ) -> Result<Vec<SubtaskCandidate>> {
        let started_at = Instant::now();
        let now = self.clock.wall_now_ms();
        let result = if limit == 0 {
            Ok(Vec::new())
        } else {
            self.with_read_tx(|tx| match role {
                SessionRole::Executor => {
                    subtask_candidates_tx(tx, SubtaskKind::Work, meta_task_id, limit, now)
                }
                SessionRole::Reviewer => {
                    subtask_candidates_tx(tx, SubtaskKind::Review, meta_task_id, limit, now)
                }
                other => Err(CoveyError::WrongRole {
                    expected: vec![SessionRole::Executor, SessionRole::Reviewer],
                    actual: other,
                }),
            })
        };
        self.log_operation(
            "subtask_candidates",
            "system",
            started_at,
            &result,
            |items| {
                items
                    .iter()
                    .map(|item| format!("subtask:{}", item.subtask_id))
                    .collect()
            },
        );
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
                  AND NOT (
                      st.state IN ('changes_requested', 'blocked')
                      AND EXISTS (
                          WITH RECURSIVE followup_chain(followup_subtask_id) AS (
                              SELECT f.followup_subtask_id
                              FROM review_followup_subtasks f
                              WHERE f.source_subtask_id = st.subtask_id
                              UNION
                              SELECT f.followup_subtask_id
                              FROM review_followup_subtasks f
                              JOIN followup_chain chain
                                ON f.source_subtask_id = chain.followup_subtask_id
                          )
                          SELECT 1
                          FROM followup_chain
                      )
                  )
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

fn subtask_candidates_tx(
    tx: &rusqlite::Transaction<'_>,
    kind: SubtaskKind,
    meta_task_id: Option<&str>,
    limit: usize,
    now: i64,
) -> Result<Vec<SubtaskCandidate>> {
    match kind {
        SubtaskKind::Work => subtask_work_candidates_tx(tx, meta_task_id, limit, now),
        SubtaskKind::Review => subtask_review_candidates_tx(tx, meta_task_id, limit),
    }
}

fn subtask_work_candidates_tx(
    tx: &rusqlite::Transaction<'_>,
    meta_task_id: Option<&str>,
    limit: usize,
    now: i64,
) -> Result<Vec<SubtaskCandidate>> {
    let terminal_dependency_states = [
        subtask_state_name(SubtaskState::Approved),
        subtask_state_name(SubtaskState::ReadyForApply),
        subtask_state_name(SubtaskState::Applied),
        subtask_state_name(SubtaskState::Decided),
    ];
    let base_select = r#"
        SELECT s.subtask_id, s.meta_task_id, s.title, s.kind,
               s.review_target_subtask_id, s.review_target_artifact_digest,
               s.state, s.artifact_digest, s.priority,
               MAX(s.priority - MIN(MAX(?1 - s.created_at, 0) / 30000, s.priority), 0) AS effective_priority,
               EXISTS (
                   SELECT 1
                   FROM review_followup_subtasks followup
                   WHERE followup.followup_subtask_id = s.subtask_id
               ) AS is_repair_followup,
               (
                   SELECT COUNT(*)
                   FROM subtask_dependencies dep_edge
                   JOIN subtasks dependent ON dependent.subtask_id = dep_edge.subtask_id
                   WHERE dep_edge.depends_on_subtask_id = s.subtask_id
                     AND dependent.state = ?2
               ) AS blocked_dependents_count,
               s.created_at, s.updated_at
        FROM subtasks s
        JOIN meta_tasks m ON m.meta_task_id = s.meta_task_id
        WHERE s.kind = ?3
          AND s.state = ?2
          AND m.state NOT IN (?4, ?5)
    "#;
    let dependency_clause = r#"
          AND NOT EXISTS (
              SELECT 1
              FROM subtask_dependencies d
              JOIN subtasks dep ON dep.subtask_id = d.depends_on_subtask_id
              WHERE d.subtask_id = s.subtask_id
                AND dep.state NOT IN (?6, ?7, ?8, ?9)
                AND NOT EXISTS (
                    WITH RECURSIVE followup_chain(followup_subtask_id) AS (
                        SELECT f.followup_subtask_id
                        FROM review_followup_subtasks f
                        WHERE f.source_subtask_id = dep.subtask_id
                        UNION
                        SELECT f.followup_subtask_id
                        FROM review_followup_subtasks f
                        JOIN followup_chain chain
                          ON f.source_subtask_id = chain.followup_subtask_id
                    )
                    SELECT 1
                    FROM followup_chain chain
                    JOIN subtasks followup
                      ON followup.subtask_id = chain.followup_subtask_id
                    WHERE followup.state IN (?6, ?7, ?8, ?9)
                )
          )
        ORDER BY effective_priority ASC, s.priority ASC, s.created_at ASC
        LIMIT ?10
    "#;
    let scoped_clause = " AND s.meta_task_id = ?11\n";
    let sql = if meta_task_id.is_some() {
        format!("{base_select}{scoped_clause}{dependency_clause}")
    } else {
        format!("{base_select}{dependency_clause}")
    };
    let mut stmt = tx.prepare(&sql)?;
    let mut rows = if let Some(meta_task_id) = meta_task_id {
        stmt.query(params![
            now,
            subtask_state_name(SubtaskState::Available),
            subtask_kind_name(SubtaskKind::Work),
            meta_task_state_name(crate::model::MetaTaskState::Completed),
            meta_task_state_name(crate::model::MetaTaskState::Cancelled),
            terminal_dependency_states[0],
            terminal_dependency_states[1],
            terminal_dependency_states[2],
            terminal_dependency_states[3],
            limit as i64,
            meta_task_id,
        ])?
    } else {
        stmt.query(params![
            now,
            subtask_state_name(SubtaskState::Available),
            subtask_kind_name(SubtaskKind::Work),
            meta_task_state_name(crate::model::MetaTaskState::Completed),
            meta_task_state_name(crate::model::MetaTaskState::Cancelled),
            terminal_dependency_states[0],
            terminal_dependency_states[1],
            terminal_dependency_states[2],
            terminal_dependency_states[3],
            limit as i64,
        ])?
    };
    let mut candidates = Vec::new();
    while let Some(row) = rows.next()? {
        candidates.push(candidate_from_row(row)?);
    }
    Ok(candidates)
}

fn subtask_review_candidates_tx(
    tx: &rusqlite::Transaction<'_>,
    meta_task_id: Option<&str>,
    limit: usize,
) -> Result<Vec<SubtaskCandidate>> {
    let base_select = r#"
        SELECT s.subtask_id, s.meta_task_id, s.title, s.kind,
               s.review_target_subtask_id, s.review_target_artifact_digest,
               s.state, s.artifact_digest, s.priority,
               s.priority AS effective_priority,
               0 AS is_repair_followup,
               0 AS blocked_dependents_count,
               s.created_at, s.updated_at
        FROM subtasks s
        JOIN meta_tasks m ON m.meta_task_id = s.meta_task_id
        WHERE s.kind = ?1
          AND s.state = ?2
          AND m.state NOT IN (?3, ?4)
    "#;
    let scoped_clause = " AND s.meta_task_id = ?6\n";
    let order_clause = " ORDER BY s.priority ASC, s.created_at ASC LIMIT ?5";
    let sql = if meta_task_id.is_some() {
        format!("{base_select}{scoped_clause}{order_clause}")
    } else {
        format!("{base_select}{order_clause}")
    };
    let mut stmt = tx.prepare(&sql)?;
    let mut rows = if let Some(meta_task_id) = meta_task_id {
        stmt.query(params![
            subtask_kind_name(SubtaskKind::Review),
            subtask_state_name(SubtaskState::Available),
            meta_task_state_name(crate::model::MetaTaskState::Completed),
            meta_task_state_name(crate::model::MetaTaskState::Cancelled),
            limit as i64,
            meta_task_id,
        ])?
    } else {
        stmt.query(params![
            subtask_kind_name(SubtaskKind::Review),
            subtask_state_name(SubtaskState::Available),
            meta_task_state_name(crate::model::MetaTaskState::Completed),
            meta_task_state_name(crate::model::MetaTaskState::Cancelled),
            limit as i64,
        ])?
    };
    let mut candidates = Vec::new();
    while let Some(row) = rows.next()? {
        candidates.push(candidate_from_row(row)?);
    }
    Ok(candidates)
}

fn candidate_from_row(row: &Row<'_>) -> Result<SubtaskCandidate> {
    let blocked_dependents_count = row.get::<_, i64>(11)?;
    SubtaskCandidate::new(
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
        usize::try_from(blocked_dependents_count).map_err(to_sql_err)?,
        row.get(12)?,
        row.get(13)?,
    )
    .map_err(|reason| CoveyError::InvalidObservabilityRow { reason })
}

fn landing_receipt_exists_for_subtask_tx(
    tx: &rusqlite::Transaction<'_>,
    subtask_id: &str,
) -> Result<bool> {
    tx.query_row(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM landing_receipts receipt
            JOIN ready_queue queue ON queue.queue_id = receipt.queue_id
            WHERE queue.subtask_id = ?1
        )
        "#,
        params![subtask_id],
        |row| row.get::<_, bool>(0),
    )
    .map_err(Into::into)
}

fn claimable_subtask_count_tx(
    tx: &rusqlite::Transaction<'_>,
    kind: SubtaskKind,
    meta_task_id: Option<&str>,
) -> Result<usize> {
    let count = match kind {
        SubtaskKind::Work => {
            if let Some(meta_task_id) = meta_task_id {
                tx.query_row(
                    r#"
                    SELECT COUNT(*)
                    FROM subtasks s
                    JOIN meta_tasks m ON m.meta_task_id = s.meta_task_id
                    WHERE s.kind = ?1
                      AND s.state = ?2
                      AND s.meta_task_id = ?3
                      AND m.state NOT IN (?4, ?5)
                      AND NOT EXISTS (
                          SELECT 1
                          FROM subtask_dependencies d
                          JOIN subtasks dep ON dep.subtask_id = d.depends_on_subtask_id
                          WHERE d.subtask_id = s.subtask_id
                            AND dep.state NOT IN (?6, ?7, ?8, ?9)
                            AND NOT EXISTS (
                                WITH RECURSIVE followup_chain(followup_subtask_id) AS (
                                    SELECT f.followup_subtask_id
                                    FROM review_followup_subtasks f
                                    WHERE f.source_subtask_id = dep.subtask_id
                                    UNION
                                    SELECT f.followup_subtask_id
                                    FROM review_followup_subtasks f
                                    JOIN followup_chain chain
                                      ON f.source_subtask_id = chain.followup_subtask_id
                                )
                                SELECT 1
                                FROM followup_chain chain
                                JOIN subtasks followup
                                  ON followup.subtask_id = chain.followup_subtask_id
                                WHERE followup.state IN (?6, ?7, ?8, ?9)
                            )
                      )
                    "#,
                    params![
                        subtask_kind_name(kind),
                        subtask_state_name(SubtaskState::Available),
                        meta_task_id,
                        meta_task_state_name(crate::model::MetaTaskState::Completed),
                        meta_task_state_name(crate::model::MetaTaskState::Cancelled),
                        subtask_state_name(SubtaskState::Approved),
                        subtask_state_name(SubtaskState::ReadyForApply),
                        subtask_state_name(SubtaskState::Applied),
                        subtask_state_name(SubtaskState::Decided),
                    ],
                    |row| row.get::<_, i64>(0),
                )?
            } else {
                tx.query_row(
                    r#"
                    SELECT COUNT(*)
                    FROM subtasks s
                    JOIN meta_tasks m ON m.meta_task_id = s.meta_task_id
                    WHERE s.kind = ?1
                      AND s.state = ?2
                      AND m.state NOT IN (?3, ?4)
                      AND NOT EXISTS (
                          SELECT 1
                          FROM subtask_dependencies d
                          JOIN subtasks dep ON dep.subtask_id = d.depends_on_subtask_id
                          WHERE d.subtask_id = s.subtask_id
                            AND dep.state NOT IN (?5, ?6, ?7, ?8)
                            AND NOT EXISTS (
                                WITH RECURSIVE followup_chain(followup_subtask_id) AS (
                                    SELECT f.followup_subtask_id
                                    FROM review_followup_subtasks f
                                    WHERE f.source_subtask_id = dep.subtask_id
                                    UNION
                                    SELECT f.followup_subtask_id
                                    FROM review_followup_subtasks f
                                    JOIN followup_chain chain
                                      ON f.source_subtask_id = chain.followup_subtask_id
                                )
                                SELECT 1
                                FROM followup_chain chain
                                JOIN subtasks followup
                                  ON followup.subtask_id = chain.followup_subtask_id
                                WHERE followup.state IN (?5, ?6, ?7, ?8)
                            )
                      )
                    "#,
                    params![
                        subtask_kind_name(kind),
                        subtask_state_name(SubtaskState::Available),
                        meta_task_state_name(crate::model::MetaTaskState::Completed),
                        meta_task_state_name(crate::model::MetaTaskState::Cancelled),
                        subtask_state_name(SubtaskState::Approved),
                        subtask_state_name(SubtaskState::ReadyForApply),
                        subtask_state_name(SubtaskState::Applied),
                        subtask_state_name(SubtaskState::Decided),
                    ],
                    |row| row.get::<_, i64>(0),
                )?
            }
        }
        SubtaskKind::Review => {
            if let Some(meta_task_id) = meta_task_id {
                tx.query_row(
                    r#"
                    SELECT COUNT(*)
                    FROM subtasks s
                    JOIN meta_tasks m ON m.meta_task_id = s.meta_task_id
                    WHERE s.kind = ?1
                      AND s.state = ?2
                      AND s.meta_task_id = ?3
                      AND m.state NOT IN (?4, ?5)
                    "#,
                    params![
                        subtask_kind_name(kind),
                        subtask_state_name(SubtaskState::Available),
                        meta_task_id,
                        meta_task_state_name(crate::model::MetaTaskState::Completed),
                        meta_task_state_name(crate::model::MetaTaskState::Cancelled),
                    ],
                    |row| row.get::<_, i64>(0),
                )?
            } else {
                tx.query_row(
                    r#"
                    SELECT COUNT(*)
                    FROM subtasks s
                    JOIN meta_tasks m ON m.meta_task_id = s.meta_task_id
                    WHERE s.kind = ?1
                      AND s.state = ?2
                      AND m.state NOT IN (?3, ?4)
                    "#,
                    params![
                        subtask_kind_name(kind),
                        subtask_state_name(SubtaskState::Available),
                        meta_task_state_name(crate::model::MetaTaskState::Completed),
                        meta_task_state_name(crate::model::MetaTaskState::Cancelled),
                    ],
                    |row| row.get::<_, i64>(0),
                )?
            }
        }
    };
    usize::try_from(count).map_err(|_| CoveyError::InvalidObservabilityRow {
        reason: "claimable subtask count must not be negative".to_owned(),
    })
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

use std::{str::FromStr, time::Instant};

use rusqlite::{OptionalExtension, Transaction, params, types::Type};

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        ApplyGateBlocker, ApplyGateBlockerKind, Claim, ClaimState, OpenSpecArchiveStatus,
        OpenSpecChangeId, OpenSpecCurrentWork, OpenSpecCurrentWorkBlockerResolution,
        OpenSpecCurrentWorkStaleClaim, OperatorBlocker, ReadyQueueItem, Review,
        SettlementReconcileBlocker, SettlementReconcileReason, SubtaskId, SubtaskRow, SubtaskView,
    },
    ops::operator_blocker::operator_blocker_select_sql,
    queries::{collect_rows, deserialize_row},
};

const CURRENT_WORK_SCOPE_CTE: &str = r#"
        WITH RECURSIVE current_scope(subtask_id) AS (
            SELECT subtask_id
            FROM openspec_subtask_scope
            WHERE openspec_change_id = ?1
            UNION
            SELECT followup.followup_subtask_id
            FROM review_followup_subtasks followup
            JOIN current_scope scope
              ON followup.source_subtask_id = scope.subtask_id
        )
"#;

impl Covey {
    /// Returns the Covey-derived current-work projection for one OpenSpec change.
    pub fn openspec_current_work(&self, change_id: &str) -> Result<OpenSpecCurrentWork> {
        self.openspec_current_work_with_stale_claim_threshold(change_id, None)
    }

    /// Returns the Covey-derived current-work projection for one OpenSpec change.
    ///
    /// When `stale_claim_older_than_ms` is present, held claims scoped to the
    /// OpenSpec change whose subtasks have not moved for at least that duration
    /// are emitted as Covey current-work blockers. The threshold is explicit so
    /// the projection does not hide scheduler-local stuck policy.
    pub fn openspec_current_work_with_stale_claim_threshold(
        &self,
        change_id: &str,
        stale_claim_older_than_ms: Option<i64>,
    ) -> Result<OpenSpecCurrentWork> {
        let started_at = Instant::now();
        let change_id = OpenSpecChangeId::parse(change_id)?;
        let result = self.with_read_tx(|tx| {
            let subtasks = load_current_work_subtasks_tx(tx, &change_id)?;
            let reviews = load_current_work_reviews_tx(tx, &change_id)?;
            let queue_items = load_current_work_queue_items_tx(tx, &change_id)?;
            let archive_statuses = load_current_work_archive_statuses_tx(tx, &change_id)?;
            let landing_receipt_queue_ids =
                load_current_work_landing_receipt_queue_ids_tx(tx, &change_id)?;
            let apply_gate_blockers = load_current_work_apply_gate_blockers_tx(tx, &change_id)?;
            let settlement_reconcile_blockers =
                load_current_work_settlement_reconcile_blockers_tx(tx, &change_id)?;
            let operator_blockers = load_current_work_operator_blockers_tx(tx, &change_id)?;
            let active_claims = load_current_work_active_claims_tx(tx, &change_id)?;
            let repaired_source_subtask_ids =
                load_current_work_repaired_source_subtask_ids_tx(tx, &change_id)?;
            let lease_now_ms = current_lease_now_ms_tx(tx, self.clock.wall_now_ms())?;
            let stale_claims = stale_claim_older_than_ms
                .map(|threshold| {
                    load_current_work_stale_claims_tx(tx, &change_id, threshold, lease_now_ms)
                })
                .transpose()?
                .unwrap_or_default();
            Ok(OpenSpecCurrentWork::from_parts(
                change_id.clone(),
                subtasks,
                reviews,
                queue_items,
                archive_statuses,
                landing_receipt_queue_ids,
                apply_gate_blockers,
                settlement_reconcile_blockers,
                operator_blockers,
                active_claims,
                repaired_source_subtask_ids,
                stale_claims,
                lease_now_ms,
            ))
        });
        self.log_operation(
            "openspec_current_work",
            "system",
            started_at,
            &result,
            |work| {
                work.subtask_ids
                    .iter()
                    .map(|subtask_id| format!("subtask:{subtask_id}"))
                    .collect()
            },
        );
        result
    }

    /// Returns the OpenSpec change id that owns one scoped Covey subtask.
    pub fn openspec_change_id_for_subtask(
        &self,
        subtask_id: &str,
    ) -> Result<Option<OpenSpecChangeId>> {
        let started_at = Instant::now();
        let subtask_id = SubtaskId::parse(subtask_id)?;
        let result = self.with_read_tx(|tx| openspec_change_id_for_subtask_tx(tx, &subtask_id));
        self.log_operation(
            "openspec_change_id_for_subtask",
            "system",
            started_at,
            &result,
            |change_id| {
                change_id
                    .as_ref()
                    .map(|change_id| vec![format!("openspec:{}", change_id.as_str())])
                    .unwrap_or_default()
            },
        );
        result
    }

    /// Resolves one live current-work blocker by stable blocker id.
    ///
    /// Synthetic missing-import blocker ids are resolved from their embedded
    /// OpenSpec change id. Durable operator blockers are resolved through the
    /// authoritative operator-blocker row. Other ids are matched against known
    /// OpenSpec current-work projections. Unknown, stale, or ambiguous ids fail
    /// without mutation.
    pub fn resolve_openspec_current_work_blocker(
        &self,
        blocker_id: &str,
        stale_claim_older_than_ms: Option<i64>,
    ) -> Result<OpenSpecCurrentWorkBlockerResolution> {
        if !blocker_id.starts_with("blocker_") {
            return Err(CoveyError::CurrentWorkBlockerNotFound {
                blocker_id: blocker_id.to_owned(),
            });
        }
        if let Some(change_id) = blocker_id
            .strip_prefix("blocker_openspec_current_work_missing_import_")
            .filter(|value| !value.is_empty())
        {
            return self.resolve_current_work_blocker_for_change(
                change_id,
                blocker_id,
                stale_claim_older_than_ms,
            );
        }
        if let Some(operator_blocker_id) = blocker_id
            .strip_prefix("blocker_openspec_current_work_operator_blocked_")
            .filter(|value| !value.is_empty())
        {
            let operator_blocker = self.operator_blocker(operator_blocker_id)?;
            return self.resolve_current_work_blocker_for_change(
                operator_blocker.openspec_change_id.as_str(),
                blocker_id,
                stale_claim_older_than_ms,
            );
        }

        let change_ids = self.with_read_tx(load_current_work_change_ids_tx)?;
        let mut matches = Vec::new();
        for change_id in change_ids {
            if let Ok(resolution) = self.resolve_current_work_blocker_for_change(
                change_id.as_str(),
                blocker_id,
                stale_claim_older_than_ms,
            ) {
                matches.push(resolution);
            }
        }
        match matches.len() {
            0 => Err(CoveyError::CurrentWorkBlockerNotFound {
                blocker_id: blocker_id.to_owned(),
            }),
            1 => Ok(matches.remove(0)),
            _ => Err(CoveyError::AmbiguousCurrentWorkBlocker {
                blocker_id: blocker_id.to_owned(),
            }),
        }
    }

    fn resolve_current_work_blocker_for_change(
        &self,
        change_id: &str,
        blocker_id: &str,
        stale_claim_older_than_ms: Option<i64>,
    ) -> Result<OpenSpecCurrentWorkBlockerResolution> {
        let current_work = self.openspec_current_work_with_stale_claim_threshold(
            change_id,
            stale_claim_older_than_ms,
        )?;
        let blocker = current_work
            .blockers
            .iter()
            .find(|blocker| blocker.blocker_id == blocker_id)
            .cloned()
            .ok_or_else(|| CoveyError::CurrentWorkBlockerNotFound {
                blocker_id: blocker_id.to_owned(),
            })?;
        Ok(OpenSpecCurrentWorkBlockerResolution {
            openspec_change_id: current_work.openspec_change_id.clone(),
            current_work,
            blocker,
        })
    }
}

fn load_current_work_change_ids_tx(tx: &Transaction<'_>) -> Result<Vec<OpenSpecChangeId>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT openspec_change_id
        FROM openspec_subtask_scope
        UNION
        SELECT openspec_change_id
        FROM openspec_archive_status
        UNION
        SELECT openspec_change_id
        FROM operator_blockers
        ORDER BY openspec_change_id
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        row.get::<_, String>(0).and_then(|raw| {
            OpenSpecChangeId::parse(raw)
                .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, err.into()))
        })
    })?;
    collect_rows(rows)
}

fn load_current_work_stale_claims_tx(
    tx: &Transaction<'_>,
    change_id: &OpenSpecChangeId,
    older_than_ms: i64,
    lease_now_ms: i64,
) -> Result<Vec<OpenSpecCurrentWorkStaleClaim>> {
    let threshold_ms = older_than_ms.max(0);
    let cutoff = lease_now_ms.saturating_sub(threshold_ms);
    let mut stmt = tx.prepare(
        format!(
            r#"
        {CURRENT_WORK_SCOPE_CTE}
        SELECT c.claim_id, c.subtask_id, c.owner_session_token, c.fence_seq,
               c.lease_deadline, c.state, c.created_at, c.updated_at,
               s.updated_at
        FROM current_scope scope
        JOIN subtasks s ON s.subtask_id = scope.subtask_id
        JOIN claims c ON c.claim_id = s.current_claim_id
        WHERE c.state = ?2
          AND c.lease_deadline > ?3
          AND s.state IN ('claimed', 'in_progress')
          AND s.artifact_digest IS NULL
          AND s.updated_at <= ?4
        ORDER BY s.updated_at ASC, c.claim_id ASC
        "#
        )
        .as_str(),
    )?;
    let rows = stmt.query_map(
        params![
            change_id.as_str(),
            ClaimState::Held.to_string(),
            lease_now_ms,
            cutoff
        ],
        |row| {
            let raw_state = row.get::<_, String>(5)?;
            let claim_state = ClaimState::from_str(&raw_state).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(5, Type::Text, err.into())
            })?;
            let claim = Claim::try_from_parts(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                claim_state,
                row.get(6)?,
                row.get(7)?,
            )
            .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, err.into()))?;
            let updated_at = row.get::<_, i64>(8)?;
            Ok(OpenSpecCurrentWorkStaleClaim {
                claim,
                idle_for_ms: lease_now_ms.saturating_sub(updated_at).max(0),
                threshold_ms,
            })
        },
    )?;
    collect_rows(rows)
}

pub(crate) fn openspec_change_id_for_subtask_tx(
    tx: &Transaction<'_>,
    subtask_id: &SubtaskId,
) -> Result<Option<OpenSpecChangeId>> {
    tx.query_row(
        r#"
        WITH RECURSIVE source_chain(subtask_id, depth) AS (
            SELECT ?1, 0
            UNION
            SELECT followup.source_subtask_id, chain.depth + 1
            FROM review_followup_subtasks followup
            JOIN source_chain chain
              ON followup.followup_subtask_id = chain.subtask_id
        )
        SELECT scope.openspec_change_id
        FROM source_chain chain
        JOIN openspec_subtask_scope scope
          ON scope.subtask_id = chain.subtask_id
        ORDER BY chain.depth ASC
        LIMIT 1
        "#,
        params![subtask_id.as_str()],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(OpenSpecChangeId::parse)
    .transpose()
    .map_err(Into::into)
}

fn load_current_work_subtasks_tx(
    tx: &Transaction<'_>,
    change_id: &OpenSpecChangeId,
) -> Result<Vec<SubtaskView>> {
    let mut stmt = tx.prepare(
        format!(
            r#"
        {CURRENT_WORK_SCOPE_CTE}
        SELECT s.subtask_id, s.meta_task_id, s.title, s.kind,
               s.review_target_subtask_id, s.review_target_artifact_digest,
               s.state, s.current_claim_id, s.artifact_digest, s.priority,
               s.created_at, s.updated_at
        FROM current_scope scope
        JOIN subtasks s ON s.subtask_id = scope.subtask_id
        ORDER BY s.created_at ASC, s.subtask_id ASC
        "#
        )
        .as_str(),
    )?;
    let rows = stmt.query_map(params![change_id.as_str()], |row| {
        let subtask = deserialize_row::<SubtaskRow>(row)?;
        SubtaskView::try_from(subtask)
    })?;
    collect_rows(rows)
}

fn load_current_work_reviews_tx(
    tx: &Transaction<'_>,
    change_id: &OpenSpecChangeId,
) -> Result<Vec<Review>> {
    let mut stmt = tx.prepare(
        format!(
            r#"
        {CURRENT_WORK_SCOPE_CTE}
        SELECT r.review_id, r.subtask_id, r.artifact_digest, r.reviewer_session,
               r.review_subtask_id, r.verdict, r.findings_digest, r.state,
               r.created_at, r.updated_at
        FROM current_scope scope
        JOIN reviews r ON r.subtask_id = scope.subtask_id
        ORDER BY r.created_at ASC, r.review_id ASC
        "#
        )
        .as_str(),
    )?;
    let rows = stmt.query_map(params![change_id.as_str()], deserialize_row::<Review>)?;
    collect_rows(rows)
}

fn load_current_work_queue_items_tx(
    tx: &Transaction<'_>,
    change_id: &OpenSpecChangeId,
) -> Result<Vec<ReadyQueueItem>> {
    let mut stmt = tx.prepare(
        format!(
            r#"
        {CURRENT_WORK_SCOPE_CTE}
        SELECT q.queue_id, q.artifact_digest, q.subtask_id, q.settlement_target,
               q.state, q.claimed_by_session_token, q.claim_fence_seq,
               q.claim_lease_deadline, q.enqueued_at, q.updated_at
        FROM current_scope scope
        JOIN ready_queue q ON q.subtask_id = scope.subtask_id
        ORDER BY q.enqueued_at ASC, q.queue_id ASC
        "#
        )
        .as_str(),
    )?;
    let rows = stmt.query_map(
        params![change_id.as_str()],
        deserialize_row::<ReadyQueueItem>,
    )?;
    collect_rows(rows)
}

fn load_current_work_archive_statuses_tx(
    tx: &Transaction<'_>,
    change_id: &OpenSpecChangeId,
) -> Result<Vec<OpenSpecArchiveStatus>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT queue_id, subtask_id, artifact_digest, openspec_change_id, state,
               blocked_reason, archive_proof_digest, recorded_by_session,
               created_at, updated_at
        FROM openspec_archive_status
        WHERE openspec_change_id = ?1
        ORDER BY updated_at ASC, queue_id ASC
        "#,
    )?;
    let rows = stmt.query_map(
        params![change_id.as_str()],
        deserialize_row::<OpenSpecArchiveStatus>,
    )?;
    collect_rows(rows)
}

fn load_current_work_landing_receipt_queue_ids_tx(
    tx: &Transaction<'_>,
    change_id: &OpenSpecChangeId,
) -> Result<Vec<crate::model::QueueId>> {
    let mut stmt = tx.prepare(
        format!(
            r#"
        {CURRENT_WORK_SCOPE_CTE}
        SELECT q.queue_id
        FROM current_scope scope
        JOIN ready_queue q ON q.subtask_id = scope.subtask_id
        JOIN landing_receipts receipt
          ON receipt.artifact_digest = q.artifact_digest
        ORDER BY receipt.created_at ASC, receipt.queue_id ASC
        "#
        )
        .as_str(),
    )?;
    let rows = stmt.query_map(params![change_id.as_str()], |row| {
        row.get::<_, String>(0).and_then(|raw| {
            crate::model::QueueId::parse(raw)
                .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, err.into()))
        })
    })?;
    collect_rows(rows)
}

fn load_current_work_apply_gate_blockers_tx(
    tx: &Transaction<'_>,
    change_id: &OpenSpecChangeId,
) -> Result<Vec<ApplyGateBlocker>> {
    let mut stmt = tx.prepare(
        format!(
            r#"
        {CURRENT_WORK_SCOPE_CTE}
        SELECT blocker.queue_id, blocker.artifact_digest, blocker.review_id,
               blocker.findings_digest, blocker.claim_fence_seq, blocker.verifier,
               blocker.blocker_kind, blocker.reason, blocker.evidence_id,
               blocker.recorded_by_session, blocker.created_at
        FROM current_scope scope
        JOIN ready_queue queue ON queue.subtask_id = scope.subtask_id
        JOIN apply_gate_blockers blocker
          ON blocker.queue_id = queue.queue_id
         AND blocker.artifact_digest = queue.artifact_digest
         AND blocker.claim_fence_seq = queue.claim_fence_seq
        WHERE queue.state IN ('queued', 'in_flight')
        ORDER BY blocker.created_at ASC, blocker.evidence_id ASC
        "#
        )
        .as_str(),
    )?;
    let rows = stmt.query_map(params![change_id.as_str()], |row| {
        let raw_kind = row.get::<_, String>(6)?;
        let blocker_kind = raw_kind.parse::<ApplyGateBlockerKind>().map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(6, Type::Text, Box::new(err))
        })?;
        Ok(ApplyGateBlocker {
            queue_id: row.get(0)?,
            artifact_digest: row.get(1)?,
            review_id: row.get(2)?,
            findings_digest: row.get(3)?,
            claim_fence_seq: row.get(4)?,
            verifier: row.get(5)?,
            blocker_kind,
            reason: row.get(7)?,
            evidence_id: row.get(8)?,
            recorded_by_session: row.get(9)?,
            created_at: row.get(10)?,
        })
    })?;
    collect_rows(rows)
}

fn load_current_work_settlement_reconcile_blockers_tx(
    tx: &Transaction<'_>,
    change_id: &OpenSpecChangeId,
) -> Result<Vec<SettlementReconcileBlocker>> {
    let mut stmt = tx.prepare(
        format!(
            r#"
        {CURRENT_WORK_SCOPE_CTE}
        SELECT blocker.queue_id, blocker.artifact_digest, blocker.review_id,
               blocker.findings_digest, blocker.claim_fence_seq,
               blocker.reconcile_reason, blocker.authority_evidence_id,
               blocker.recorded_by_session, blocker.created_at
        FROM current_scope scope
        JOIN ready_queue queue ON queue.subtask_id = scope.subtask_id
        JOIN settlement_reconcile_blockers blocker
          ON blocker.queue_id = queue.queue_id
         AND blocker.artifact_digest = queue.artifact_digest
         AND blocker.claim_fence_seq = queue.claim_fence_seq
        WHERE queue.state IN ('queued', 'in_flight', 'applied')
        ORDER BY blocker.created_at ASC, blocker.authority_evidence_id ASC
        "#
        )
        .as_str(),
    )?;
    let rows = stmt.query_map(params![change_id.as_str()], |row| {
        let raw_reason = row.get::<_, String>(5)?;
        let reconcile_reason = raw_reason
            .parse::<SettlementReconcileReason>()
            .map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(5, Type::Text, Box::new(err))
            })?;
        Ok(SettlementReconcileBlocker {
            queue_id: row.get(0)?,
            artifact_digest: row.get(1)?,
            review_id: row.get(2)?,
            findings_digest: row.get(3)?,
            claim_fence_seq: row.get(4)?,
            reconcile_reason,
            authority_evidence_id: row.get(6)?,
            recorded_by_session: row.get(7)?,
            created_at: row.get(8)?,
        })
    })?;
    collect_rows(rows)
}

fn load_current_work_operator_blockers_tx(
    tx: &Transaction<'_>,
    change_id: &OpenSpecChangeId,
) -> Result<Vec<OperatorBlocker>> {
    let sql = operator_blocker_select_sql(
        "WHERE openspec_change_id = ?1 AND state = 'open' ORDER BY updated_at ASC, blocker_id ASC",
    );
    let mut stmt = tx.prepare(&sql)?;
    let rows = stmt.query_map(
        params![change_id.as_str()],
        deserialize_row::<OperatorBlocker>,
    )?;
    collect_rows(rows)
}

fn load_current_work_active_claims_tx(
    tx: &Transaction<'_>,
    change_id: &OpenSpecChangeId,
) -> Result<Vec<Claim>> {
    let mut stmt = tx.prepare(
        format!(
            r#"
        {CURRENT_WORK_SCOPE_CTE}
        SELECT c.claim_id, c.subtask_id, c.owner_session_token, c.fence_seq,
               c.lease_deadline, c.state, c.created_at, c.updated_at
        FROM current_scope scope
        JOIN subtasks s ON s.subtask_id = scope.subtask_id
        JOIN claims c ON c.claim_id = s.current_claim_id
        WHERE c.state = ?2
        ORDER BY c.lease_deadline ASC, c.claim_id ASC
        "#
        )
        .as_str(),
    )?;
    let rows = stmt.query_map(
        params![change_id.as_str(), ClaimState::Held.to_string()],
        deserialize_row::<Claim>,
    )?;
    collect_rows(rows)
}

fn load_current_work_repaired_source_subtask_ids_tx(
    tx: &Transaction<'_>,
    change_id: &OpenSpecChangeId,
) -> Result<Vec<SubtaskId>> {
    let mut stmt = tx.prepare(
        format!(
            r#"
        {CURRENT_WORK_SCOPE_CTE}
        SELECT DISTINCT followup.source_subtask_id
        FROM current_scope scope
        JOIN review_followup_subtasks followup
          ON followup.source_subtask_id = scope.subtask_id
        ORDER BY followup.source_subtask_id ASC
        "#
        )
        .as_str(),
    )?;
    let rows = stmt.query_map(params![change_id.as_str()], |row| {
        row.get::<_, String>(0).and_then(|raw| {
            SubtaskId::parse(raw)
                .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, err.into()))
        })
    })?;
    collect_rows(rows)
}

fn current_lease_now_ms_tx(tx: &Transaction<'_>, wall_now_ms: i64) -> Result<i64> {
    let last_tick_ms = tx
        .query_row(
            "SELECT last_tick_ms FROM lease_clock WHERE clock_id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0);
    Ok(last_tick_ms.max(wall_now_ms.max(0)))
}

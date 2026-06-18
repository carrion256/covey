#![cfg_attr(coverage_nightly, coverage(off))]

use std::time::Instant;

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        EventType, ObjectType, OperatorBlocker, OperatorBlockerState, OperatorBlockerTargetKind,
        QueueId, RecordOperatorBlockerReq, ResolveOperatorBlockerReq, SessionRole, TimestampMs,
    },
    ops::current_work::openspec_change_id_for_subtask_tx,
    queries::{deserialize_row, load_queue_item_tx, load_subtask_tx},
    store::append_session_event,
    validators::{MAX_OBJECT_ID_LEN, ensure_length, require_role},
};

impl Covey {
    /// Loads a durable operator blocker by id.
    pub fn operator_blocker(&self, blocker_id: &str) -> Result<OperatorBlocker> {
        let started_at = Instant::now();
        let result = self.with_read_tx(|tx| load_operator_blocker_tx(tx, blocker_id));
        self.log_operation(
            "operator_blocker",
            "system",
            started_at,
            &result,
            |blocker| vec![format!("operator_blocker:{}", blocker.blocker_id.as_str())],
        );
        result
    }

    /// Records an explicit operator blocker for one OpenSpec current-work target.
    pub fn record_operator_blocker(
        &self,
        req: RecordOperatorBlockerReq,
    ) -> Result<OperatorBlocker> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "record_operator_blocker",
                &req.idempotency_key,
                &req,
                TimestampMs::parse(now)?,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::Orchestrator])?;
                    ensure_length("blocker_id", &req.blocker_id, MAX_OBJECT_ID_LEN)?;
                    ensure_length(
                        "openspec_change_id",
                        &req.openspec_change_id,
                        MAX_OBJECT_ID_LEN,
                    )?;
                    ensure_length("subtask_id", &req.subtask_id, MAX_OBJECT_ID_LEN)?;
                    ensure_length("reason", &req.reason, MAX_OBJECT_ID_LEN)?;
                    if let Some(evidence_id) = &req.source_evidence_id {
                        ensure_length("source_evidence_id", evidence_id, MAX_OBJECT_ID_LEN)?;
                    }

                    match req.target_kind {
                        OperatorBlockerTargetKind::Subtask => {
                            load_subtask_tx(tx, &req.subtask_id)?;
                            require_openspec_scope_tx(tx, &req.subtask_id, &req)?;
                        }
                        OperatorBlockerTargetKind::ReadyQueue => {
                            let queue_id = req.queue_id.as_ref().ok_or_else(|| {
                                CoveyError::InvalidImportDestination {
                                    reason: "operator blocker ready queue target requires queue_id"
                                        .to_owned(),
                                }
                            })?;
                            ensure_length("queue_id", queue_id, MAX_OBJECT_ID_LEN)?;
                            let item = load_queue_item_tx(tx, queue_id.as_str())?;
                            if item.subtask_id() != req.subtask_id.as_str()
                                || Some(item.artifact_digest()) != req.artifact_digest.as_ref().map(AsRef::as_ref)
                            {
                                return Err(CoveyError::ApplyGateEvidenceMissing {
                                    queue_id: QueueId::parse(item.queue_id().to_owned())?,
                                    reason: "operator blocker ready queue target does not match queue item"
                                        .to_owned(),
                                });
                            }
                            require_openspec_scope_tx(tx, &req.subtask_id, &req)?;
                        }
                    }

                    if let Some(existing) =
                        load_operator_blocker_optional_tx(tx, req.blocker_id.as_str())?
                    {
                        if !operator_blocker_matches_req(&existing, &req) {
                            return Err(CoveyError::InvalidImportDestination {
                                reason: "operator blocker id already exists with different target or evidence"
                                    .to_owned(),
                            });
                        }
                        return Ok(existing);
                    }
                    insert_operator_blocker_tx(tx, &req, now)?;
                    append_session_event(
                        tx,
                        EventType::OperatorBlockerRecorded,
                        ObjectType::OperatorBlocker,
                        req.blocker_id.as_str(),
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    load_operator_blocker_tx(tx, req.blocker_id.as_str())
                },
            )
        });
        self.log_operation(
            "record_operator_blocker",
            &req.session_token,
            started_at,
            &result,
            |blocker| {
                vec![
                    format!("operator_blocker:{}", blocker.blocker_id.as_str()),
                    format!("subtask:{}", blocker.subtask_id.as_str()),
                ]
            },
        );
        result
    }

    /// Resolves an explicit operator blocker without changing target lifecycle state.
    pub fn resolve_operator_blocker(
        &self,
        req: ResolveOperatorBlockerReq,
    ) -> Result<OperatorBlocker> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "resolve_operator_blocker",
                &req.idempotency_key,
                &req,
                TimestampMs::parse(now)?,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::Orchestrator])?;
                    ensure_length("blocker_id", &req.blocker_id, MAX_OBJECT_ID_LEN)?;
                    ensure_length("resolved_reason", &req.resolved_reason, MAX_OBJECT_ID_LEN)?;

                    let existing = load_operator_blocker_tx(tx, req.blocker_id.as_str())?;
                    if existing.state == OperatorBlockerState::Resolved {
                        if existing.resolved_reason.as_ref() == Some(&req.resolved_reason)
                            && existing.resolved_by_session.as_ref() == Some(&req.session_token)
                        {
                            return Ok(existing);
                        }
                        return Err(CoveyError::InvalidImportDestination {
                            reason: "operator blocker is already resolved with different evidence"
                                .to_owned(),
                        });
                    }

                    tx.execute(
                        r#"
                        UPDATE operator_blockers
                        SET state = ?2,
                            resolved_reason = ?3,
                            resolved_by_session = ?4,
                            resolved_at = ?5,
                            updated_at = ?5
                        WHERE blocker_id = ?1
                        "#,
                        params![
                            req.blocker_id.as_str(),
                            OperatorBlockerState::Resolved.to_string(),
                            req.resolved_reason.as_str(),
                            req.session_token.as_str(),
                            now
                        ],
                    )?;
                    append_session_event(
                        tx,
                        EventType::OperatorBlockerResolved,
                        ObjectType::OperatorBlocker,
                        req.blocker_id.as_str(),
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    load_operator_blocker_tx(tx, req.blocker_id.as_str())
                },
            )
        });
        self.log_operation(
            "resolve_operator_blocker",
            &req.session_token,
            started_at,
            &result,
            |blocker| vec![format!("operator_blocker:{}", blocker.blocker_id.as_str())],
        );
        result
    }
}

fn require_openspec_scope_tx(
    tx: &Transaction<'_>,
    subtask_id: &crate::model::SubtaskId,
    req: &RecordOperatorBlockerReq,
) -> Result<()> {
    let scope = openspec_change_id_for_subtask_tx(tx, subtask_id)?.ok_or_else(|| {
        CoveyError::InvalidImportDestination {
            reason: "operator blocker target requires imported OpenSpec subtask scope".to_owned(),
        }
    })?;
    if scope != req.openspec_change_id {
        return Err(CoveyError::InvalidImportDestination {
            reason: "operator blocker change id does not match target scope".to_owned(),
        });
    }
    Ok(())
}

fn load_operator_blocker_tx(tx: &Transaction<'_>, blocker_id: &str) -> Result<OperatorBlocker> {
    tx.query_row(
        operator_blocker_select_sql("WHERE blocker_id = ?1").as_str(),
        params![blocker_id],
        deserialize_row::<OperatorBlocker>,
    )
    .map_err(Into::into)
}

fn load_operator_blocker_optional_tx(
    tx: &Transaction<'_>,
    blocker_id: &str,
) -> Result<Option<OperatorBlocker>> {
    tx.query_row(
        operator_blocker_select_sql("WHERE blocker_id = ?1").as_str(),
        params![blocker_id],
        deserialize_row::<OperatorBlocker>,
    )
    .optional()
    .map_err(Into::into)
}

fn insert_operator_blocker_tx(
    tx: &Transaction<'_>,
    req: &RecordOperatorBlockerReq,
    now: i64,
) -> Result<()> {
    tx.execute(
        r#"
        INSERT INTO operator_blockers (
            blocker_id, openspec_change_id, target_kind, subtask_id,
            queue_id, artifact_digest, reason, source_evidence_id,
            recorded_by_session, created_at, updated_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
        "#,
        params![
            req.blocker_id.as_str(),
            req.openspec_change_id.as_str(),
            req.target_kind.to_string(),
            req.subtask_id.as_str(),
            req.queue_id.as_ref().map(AsRef::as_ref),
            req.artifact_digest.as_ref().map(AsRef::as_ref),
            req.reason.as_str(),
            req.source_evidence_id.as_ref().map(AsRef::as_ref),
            req.session_token.as_str(),
            now
        ],
    )?;
    Ok(())
}

fn operator_blocker_matches_req(blocker: &OperatorBlocker, req: &RecordOperatorBlockerReq) -> bool {
    blocker.blocker_id == req.blocker_id
        && blocker.openspec_change_id == req.openspec_change_id
        && blocker.target_kind == req.target_kind
        && blocker.subtask_id == req.subtask_id
        && blocker.queue_id == req.queue_id
        && blocker.artifact_digest == req.artifact_digest
        && blocker.reason == req.reason
        && blocker.source_evidence_id == req.source_evidence_id
        && blocker.recorded_by_session == req.session_token
}

pub(crate) fn operator_blocker_select_sql(where_clause: &str) -> String {
    format!(
        r#"
        SELECT blocker_id, openspec_change_id, target_kind, subtask_id,
               queue_id, artifact_digest, reason, source_evidence_id,
               state, recorded_by_session, resolved_reason, resolved_by_session,
               resolved_at, created_at, updated_at
        FROM operator_blockers
        {where_clause}
        "#
    )
}

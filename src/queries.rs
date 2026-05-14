use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Deserialize;
use serde_rusqlite::from_row;

use crate::{
    error::{CoveyError, Result},
    model::{
        ActorKind, Artifact, Claim, ClaimState, Event, MetaTask, MutationIdempotencyRecord,
        ObjectType, OpenSpecImportProvenance, OpenSpecSourceDigest, ReadyQueueItem, Reservation,
        ReservationState, Review, RuntimeAttestation, Session, SessionState, Subtask,
        parse_generated_members,
    },
    schema::SYSTEM_EVENT_SESSION_TOKEN,
};

fn map_missing_row<T>(result: rusqlite::Result<T>, not_found: CoveyError) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(not_found),
        Err(err) => Err(err.into()),
    }
}

pub(crate) fn load_expirable_claims(tx: &Transaction<'_>, now: i64) -> Result<Vec<Claim>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT c.claim_id, c.subtask_id, c.owner_session_token, c.fence_seq, c.lease_deadline, c.state, c.created_at, c.updated_at
        FROM claims c
        JOIN sessions s ON s.session_token = c.owner_session_token
        WHERE c.state = ?1 AND (c.lease_deadline <= ?2 OR s.state != ?3)
        ORDER BY c.created_at ASC
        "#,
    )?;
    let rows = stmt.query_map(
        params![
            ClaimState::Held.to_string(),
            now,
            SessionState::Active.to_string()
        ],
        deserialize_row::<Claim>,
    )?;
    collect_rows(rows)
}

pub(crate) fn load_expired_reservation_ids(tx: &Transaction<'_>, now: i64) -> Result<Vec<String>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT reservation_id
        FROM reservations
        WHERE state = ?1 AND lease_deadline <= ?2
        ORDER BY created_at ASC
        "#,
    )?;
    let rows = stmt.query_map(params![ReservationState::Active.to_string(), now], |row| {
        row.get::<_, String>(0)
    })?;
    collect_rows(rows)
}

pub(crate) fn load_session_conn(conn: &Connection, session_token: &str) -> Result<Session> {
    map_missing_row(
        conn.query_row(
            r#"
            SELECT session_token, agent_principal_id, agent_instance_id, role, state,
                   active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at
            FROM sessions WHERE session_token = ?1
            "#,
            params![session_token],
            deserialize_row::<Session>,
        ),
        CoveyError::SessionNotFound,
    )
}

pub(crate) fn load_session_tx(tx: &Transaction<'_>, session_token: &str) -> Result<Session> {
    map_missing_row(
        tx.query_row(
            r#"
            SELECT session_token, agent_principal_id, agent_instance_id, role, state,
                   active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at
            FROM sessions WHERE session_token = ?1
            "#,
            params![session_token],
            deserialize_row::<Session>,
        ),
        CoveyError::SessionNotFound,
    )
}

pub(crate) fn load_runtime_attestation_tx(
    tx: &Transaction<'_>,
    session_token: &str,
) -> Result<RuntimeAttestation> {
    map_missing_row(
        tx.query_row(
            r#"
            SELECT session_token, agent_principal_id, agent_instance_id, role,
                   provider, model, provider_run_id, provider_run_id_issuer,
                   process_id, container_id, command_transcript_digest,
                   started_at, ended_at, recorded_at
            FROM runtime_attestations
            WHERE session_token = ?1
            "#,
            params![session_token],
            deserialize_row::<RuntimeAttestation>,
        ),
        CoveyError::RuntimeAttestationMissing {
            session_token: session_token.to_owned(),
        },
    )
}

pub(crate) fn load_meta_task_tx(tx: &Transaction<'_>, meta_task_id: &str) -> Result<MetaTask> {
    map_missing_row(
        tx.query_row(
            "SELECT meta_task_id, prompt_text, state, created_by, created_at, updated_at FROM meta_tasks WHERE meta_task_id = ?1",
            params![meta_task_id],
            deserialize_row::<MetaTask>,
        ),
        CoveyError::MetaTaskNotFound,
    )
}

pub(crate) fn load_subtask_conn(conn: &Connection, subtask_id: &str) -> Result<Subtask> {
    map_missing_row(
        conn.query_row(
            "SELECT subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at FROM subtasks WHERE subtask_id = ?1",
            params![subtask_id],
            deserialize_row::<Subtask>,
        ),
        CoveyError::SubtaskNotFound,
    )
}

pub(crate) fn load_subtask_tx(tx: &Transaction<'_>, subtask_id: &str) -> Result<Subtask> {
    map_missing_row(
        tx.query_row(
            "SELECT subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at FROM subtasks WHERE subtask_id = ?1",
            params![subtask_id],
            deserialize_row::<Subtask>,
        ),
        CoveyError::SubtaskNotFound,
    )
}

pub(crate) fn load_claim_conn(conn: &Connection, claim_id: &str) -> Result<Claim> {
    map_missing_row(
        conn.query_row(
            "SELECT claim_id, subtask_id, owner_session_token, fence_seq, lease_deadline, state, created_at, updated_at FROM claims WHERE claim_id = ?1",
            params![claim_id],
            deserialize_row::<Claim>,
        ),
        CoveyError::ClaimNotFound,
    )
}

pub(crate) fn load_claim_tx(tx: &Transaction<'_>, claim_id: &str) -> Result<Claim> {
    map_missing_row(
        tx.query_row(
            "SELECT claim_id, subtask_id, owner_session_token, fence_seq, lease_deadline, state, created_at, updated_at FROM claims WHERE claim_id = ?1",
            params![claim_id],
            deserialize_row::<Claim>,
        ),
        CoveyError::ClaimNotFound,
    )
}

pub(crate) fn load_artifact_tx(tx: &Transaction<'_>, artifact_digest: &str) -> Result<Artifact> {
    map_missing_row(
        tx.query_row(
            "SELECT artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session, manifest_path, changed_paths_digest, created_at FROM artifacts WHERE artifact_digest = ?1",
            params![artifact_digest],
            deserialize_row::<Artifact>,
        ),
        CoveyError::ArtifactNotFound,
    )
}

pub(crate) fn load_review_tx(tx: &Transaction<'_>, review_id: &str) -> Result<Review> {
    map_missing_row(
        tx.query_row(
            "SELECT review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id, verdict, findings_digest, state, created_at, updated_at FROM reviews WHERE review_id = ?1",
            params![review_id],
            deserialize_row::<Review>,
        ),
        CoveyError::ReviewNotFound,
    )
}

pub(crate) fn load_reservation_tx(
    tx: &Transaction<'_>,
    reservation_id: &str,
) -> Result<Reservation> {
    map_missing_row(
        tx.query_row(
            r#"
            SELECT r.reservation_id, r.owner_subtask_id, r.scope_class, r.scope_key,
                   COALESCE((
                       SELECT json_group_array(member_path)
                       FROM reservation_generated_members gm
                       WHERE gm.reservation_id = r.reservation_id
                   ), '[]'),
                   r.lease_deadline, r.state, r.created_at, r.updated_at
            FROM reservations r
            WHERE r.reservation_id = ?1
            "#,
            params![reservation_id],
            map_reservation,
        ),
        CoveyError::ReservationNotFound,
    )
}

pub(crate) fn load_active_reservations_tx(
    tx: &Transaction<'_>,
    now: i64,
) -> Result<Vec<Reservation>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT r.reservation_id, r.owner_subtask_id, r.scope_class, r.scope_key,
               COALESCE((
                   SELECT json_group_array(member_path)
                   FROM reservation_generated_members gm
                   WHERE gm.reservation_id = r.reservation_id
               ), '[]'),
               r.lease_deadline, r.state, r.created_at, r.updated_at
        FROM reservations r
        WHERE r.state = ?1 AND r.lease_deadline > ?2
        ORDER BY r.created_at ASC
        "#,
    )?;
    let rows = stmt.query_map(
        params![ReservationState::Active.to_string(), now],
        map_reservation,
    )?;
    collect_rows(rows)
}

pub(crate) fn load_mutation_idempotency_record_tx(
    tx: &Transaction<'_>,
    actor_key: &str,
    operation: &str,
    idempotency_key: &str,
) -> Result<Option<MutationIdempotencyRecord>> {
    tx.query_row(
        r#"
        SELECT actor_key, operation, idempotency_key, request_hash, response_json, created_at
        FROM mutation_idempotency
        WHERE actor_key = ?1 AND operation = ?2 AND idempotency_key = ?3
        "#,
        params![actor_key, operation, idempotency_key],
        deserialize_row::<MutationIdempotencyRecord>,
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn load_queue_item_tx(tx: &Transaction<'_>, queue_id: &str) -> Result<ReadyQueueItem> {
    map_missing_row(
        tx.query_row(
            "SELECT queue_id, artifact_digest, subtask_id, settlement_target, state, claimed_by_session_token, claim_fence_seq, claim_lease_deadline, enqueued_at, updated_at FROM ready_queue WHERE queue_id = ?1",
            params![queue_id],
            deserialize_row::<ReadyQueueItem>,
        ),
        CoveyError::QueueItemNotFound,
    )
}

pub(crate) fn load_import_provenance_tx(
    tx: &Transaction<'_>,
    object_type: ObjectType,
    object_id: &str,
) -> Result<Option<OpenSpecImportProvenance>> {
    tx.query_row(
        r#"
        SELECT object_type, object_id, planning_format, openspec_change_id,
               openspec_change_path, openspec_task_id, proposal_digest, design_digest,
               tasks_digest, spec_digests_json, source_digests_json,
               mission_artifact_digests_json, mission_artifacts_json, task_digest, updated_at
        FROM import_provenance
        WHERE object_type = ?1 AND object_id = ?2
        "#,
        params![object_type.to_string(), object_id],
        map_import_provenance,
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn load_reviews_for_subtask_tx(
    tx: &Transaction<'_>,
    subtask_id: &str,
) -> Result<Vec<Review>> {
    let mut stmt = tx.prepare(
        "SELECT review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id, verdict, findings_digest, state, created_at, updated_at FROM reviews WHERE subtask_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![subtask_id], deserialize_row::<Review>)?;
    collect_rows(rows)
}

pub(crate) fn load_queue_items_for_subtask_tx(
    tx: &Transaction<'_>,
    subtask_id: &str,
) -> Result<Vec<ReadyQueueItem>> {
    let mut stmt = tx.prepare(
        "SELECT queue_id, artifact_digest, subtask_id, settlement_target, state, claimed_by_session_token, claim_fence_seq, claim_lease_deadline, enqueued_at, updated_at FROM ready_queue WHERE subtask_id = ?1 ORDER BY enqueued_at ASC",
    )?;
    let rows = stmt.query_map(params![subtask_id], deserialize_row::<ReadyQueueItem>)?;
    collect_rows(rows)
}

pub(crate) fn load_subtasks_for_meta_task_tx(
    tx: &Transaction<'_>,
    meta_task_id: &str,
) -> Result<Vec<Subtask>> {
    let mut stmt = tx.prepare(
        "SELECT subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at FROM subtasks WHERE meta_task_id = ?1 ORDER BY priority ASC, created_at ASC",
    )?;
    let rows = stmt.query_map(params![meta_task_id], deserialize_row::<Subtask>)?;
    collect_rows(rows)
}

fn parse_enum<T>(raw: String) -> rusqlite::Result<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    T::from_str(&raw).map_err(to_sql_err)
}

pub(crate) fn deserialize_row<T>(row: &rusqlite::Row<'_>) -> rusqlite::Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    from_row(row).map_err(to_sql_err)
}

fn map_reservation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Reservation> {
    let generated = row
        .get::<_, Option<String>>(4)?
        .unwrap_or_else(|| "[]".to_owned());
    Ok(Reservation {
        reservation_id: row.get(0)?,
        owner_subtask_id: row.get(1)?,
        scope_class: parse_enum(row.get::<_, String>(2)?)?,
        scope_key: row.get(3)?,
        generated_members: parse_generated_members(&generated).map_err(to_sql_err)?,
        lease_deadline: row.get(5)?,
        state: parse_enum(row.get::<_, String>(6)?)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn map_import_provenance(row: &rusqlite::Row<'_>) -> rusqlite::Result<OpenSpecImportProvenance> {
    let object_type: ObjectType = parse_enum(row.get::<_, String>(0)?)?;
    let spec_digests_json: String = row.get(9)?;
    let spec_digests = serde_json::from_str::<Vec<OpenSpecSourceDigest>>(&spec_digests_json)
        .map_err(to_sql_err)?;
    let source_digests_json: String = row.get(10)?;
    let source_digests = serde_json::from_str::<Vec<OpenSpecSourceDigest>>(&source_digests_json)
        .map_err(to_sql_err)?;
    let mission_artifact_digests_json: String = row.get(11)?;
    let mission_artifact_digests =
        serde_json::from_str::<Vec<OpenSpecSourceDigest>>(&mission_artifact_digests_json)
            .map_err(to_sql_err)?;
    let mission_artifacts_json: String = row.get(12)?;
    let mission_artifacts =
        serde_json::from_str::<Vec<String>>(&mission_artifacts_json).map_err(to_sql_err)?;

    Ok(OpenSpecImportProvenance {
        object_type,
        object_id: row.get(1)?,
        planning_format: row.get(2)?,
        openspec_change_id: row.get(3)?,
        openspec_change_path: row.get(4)?,
        openspec_task_id: row.get(5)?,
        proposal_digest: row.get(6)?,
        design_digest: row.get(7)?,
        tasks_digest: row.get(8)?,
        spec_digests,
        source_digests,
        mission_artifact_digests,
        mission_artifacts,
        task_digest: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

pub(crate) fn map_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    let actor_kind: ActorKind = parse_enum(row.get::<_, String>(4)?)?;
    let raw_session_token = row.get::<_, String>(5)?;
    let session_token =
        if actor_kind == ActorKind::System && raw_session_token == SYSTEM_EVENT_SESSION_TOKEN {
            None
        } else {
            Some(raw_session_token)
        };
    Ok(Event {
        seq: row.get(0)?,
        event_type: parse_enum(row.get::<_, String>(1)?)?,
        object_type: parse_enum(row.get::<_, String>(2)?)?,
        object_id: row.get(3)?,
        actor_kind,
        session_token,
        payload_json: row.get(6)?,
        created_at: row.get(7)?,
    })
}

pub(crate) fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>> {
    let mut collected = Vec::new();
    for row in rows {
        collected.push(row?);
    }
    Ok(collected)
}

fn to_sql_err<E: std::fmt::Display>(err: E) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            err.to_string(),
        )),
    )
}

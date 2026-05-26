use std::str::FromStr;

use rusqlite::{OptionalExtension, ToSql, Transaction, params, params_from_iter};
use serde::Deserialize;
use serde_rusqlite::from_row;

use crate::{
    error::{CoveyError, Result},
    model::{
        ActorKind, Artifact, Claim, ClaimState, Event, MetaTask, MutationIdempotencyRecord,
        ObjectType, OpenSpecImportProvenance, OpenSpecSourceDigest, ReadyQueueItem, Reservation,
        ReservationState, Review, RuntimeAttestation, Session, SessionState, SessionToken,
        SubtaskRow, TimestampMs, claim_state_name, object_type_name, parse_generated_members,
        reservation_state_name, session_state_name,
    },
    schema::SYSTEM_EVENT_SESSION_TOKEN,
};

fn sql_placeholders(count: usize) -> String {
    let mut placeholders = String::with_capacity(count.saturating_mul(3).saturating_sub(2));
    for index in 0..count {
        if index > 0 {
            placeholders.push_str(", ");
        }
        placeholders.push('?');
    }
    placeholders
}

fn map_missing_row<T>(result: rusqlite::Result<T>, not_found: CoveyError) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(not_found),
        Err(err) => Err(err.into()),
    }
}

pub(crate) fn load_expirable_claims(tx: &Transaction<'_>, now: i64) -> Result<Vec<Claim>> {
    let mut claims = Vec::new();
    let mut stmt = tx.prepare(
        r#"
        SELECT c.claim_id, c.subtask_id, c.owner_session_token, c.fence_seq, c.lease_deadline, c.state, c.created_at, c.updated_at
        FROM claims c
        WHERE c.state = ?1 AND c.lease_deadline <= ?2
        "#,
    )?;
    let rows = stmt.query_map(
        params![claim_state_name(ClaimState::Held), now],
        deserialize_row::<Claim>,
    )?;
    claims.extend(collect_rows(rows)?);

    let mut stmt = tx.prepare(
        r#"
        SELECT c.claim_id, c.subtask_id, c.owner_session_token, c.fence_seq, c.lease_deadline, c.state, c.created_at, c.updated_at
        FROM sessions s
        JOIN claims c INDEXED BY idx_claims_held_owner_created
          ON c.owner_session_token = s.session_token
         AND c.state = 'held'
        WHERE s.state IN (?1, ?2)
        "#,
    )?;
    let rows = stmt.query_map(
        params![
            session_state_name(SessionState::Stale),
            session_state_name(SessionState::Exited)
        ],
        deserialize_row::<Claim>,
    )?;
    claims.extend(collect_rows(rows)?);

    claims.sort_unstable_by(|left, right| left.claim_id.cmp(&right.claim_id));
    claims.dedup_by(|left, right| left.claim_id == right.claim_id);
    claims.sort_unstable_by(|left, right| {
        left.created_at()
            .cmp(&right.created_at())
            .then_with(|| left.claim_id.cmp(&right.claim_id))
    });
    Ok(claims)
}

pub(crate) fn load_expired_reservation_ids(tx: &Transaction<'_>, now: i64) -> Result<Vec<String>> {
    let mut stmt = tx.prepare(
        r#"
        SELECT reservation_id
        FROM reservations
        WHERE state = ?1 AND lease_deadline <= ?2
        "#,
    )?;
    let rows = stmt.query_map(
        params![reservation_state_name(ReservationState::Active), now],
        |row| row.get::<_, String>(0),
    )?;
    collect_rows(rows)
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
            session_token: SessionToken::parse(session_token)?,
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

pub(crate) fn load_subtask_tx(tx: &Transaction<'_>, subtask_id: &str) -> Result<SubtaskRow> {
    map_missing_row(
        tx.query_row(
            "SELECT subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at FROM subtasks WHERE subtask_id = ?1",
            params![subtask_id],
            deserialize_row::<SubtaskRow>,
        ),
        CoveyError::SubtaskNotFound,
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
                   CASE
                       WHEN r.scope_class = 'generated_set' THEN COALESCE((
                           SELECT json_group_array(member_path)
                           FROM reservation_generated_members gm
                           WHERE gm.reservation_id = r.reservation_id
                       ), '[]')
                       ELSE '[]'
                   END,
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

pub(crate) fn load_repoops_relevant_reservations_tx(
    tx: &Transaction<'_>,
    now: i64,
    owner_subtask_id: &str,
    paths: &[String],
) -> Result<Vec<Reservation>> {
    let mut reservation_ids = Vec::new();
    collect_active_owner_reservation_ids(tx, now, owner_subtask_id, &mut reservation_ids)?;

    if !paths.is_empty() {
        collect_active_scope_key_reservation_ids(
            tx,
            now,
            "repo_global",
            &["repo"],
            &mut reservation_ids,
        )?;
        collect_active_scope_key_reservation_ids(
            tx,
            now,
            "exact_path",
            paths,
            &mut reservation_ids,
        )?;
        let ancestors = repoops_path_ancestor_scope_keys(paths);
        collect_active_scope_key_reservation_ids(
            tx,
            now,
            "subtree",
            &ancestors,
            &mut reservation_ids,
        )?;
        collect_active_generated_member_reservation_ids(tx, now, paths, &mut reservation_ids)?;
    }

    reservation_ids.sort_unstable();
    reservation_ids.dedup();
    load_reservations_by_ids_tx(tx, reservation_ids)
}

fn collect_active_owner_reservation_ids(
    tx: &Transaction<'_>,
    now: i64,
    owner_subtask_id: &str,
    reservation_ids: &mut Vec<String>,
) -> Result<()> {
    let mut stmt = tx.prepare(
        r#"
        SELECT reservation_id
        FROM reservations
        WHERE state = ?1 AND lease_deadline > ?2 AND owner_subtask_id = ?3
        "#,
    )?;
    let rows = stmt.query_map(
        params![
            reservation_state_name(ReservationState::Active),
            now,
            owner_subtask_id
        ],
        |row| row.get::<_, String>(0),
    )?;
    for row in rows {
        reservation_ids.push(row?);
    }
    Ok(())
}

fn collect_active_scope_key_reservation_ids(
    tx: &Transaction<'_>,
    now: i64,
    scope_class: &str,
    scope_keys: &[impl AsRef<str>],
    reservation_ids: &mut Vec<String>,
) -> Result<()> {
    let active_state = reservation_state_name(ReservationState::Active);
    for chunk in scope_keys.chunks(256) {
        if chunk.is_empty() {
            continue;
        }
        if let [scope_key] = chunk {
            let mut stmt = tx.prepare(
                r#"
                SELECT reservation_id
                FROM reservations
                WHERE state = ?1 AND lease_deadline > ?2 AND scope_class = ?3 AND scope_key = ?4
                "#,
            )?;
            let rows = stmt.query_map(
                params![active_state, now, scope_class, scope_key.as_ref()],
                |row| row.get::<_, String>(0),
            )?;
            for row in rows {
                reservation_ids.push(row?);
            }
            continue;
        }
        let sql = format!(
            r#"
            SELECT reservation_id
            FROM reservations
            WHERE state = ? AND lease_deadline > ? AND scope_class = ? AND scope_key IN ({})
            "#,
            sql_placeholders(chunk.len())
        );
        let scope_key_refs = chunk.iter().map(AsRef::as_ref).collect::<Vec<&str>>();
        let mut values = Vec::with_capacity(chunk.len() + 3);
        values.push(&active_state as &dyn ToSql);
        values.push(&now as &dyn ToSql);
        values.push(&scope_class as &dyn ToSql);
        values.extend(
            scope_key_refs
                .iter()
                .map(|scope_key| scope_key as &dyn ToSql),
        );
        let mut stmt = tx.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values), |row| row.get::<_, String>(0))?;
        for row in rows {
            reservation_ids.push(row?);
        }
    }
    Ok(())
}

fn collect_active_generated_member_reservation_ids(
    tx: &Transaction<'_>,
    now: i64,
    member_paths: &[String],
    reservation_ids: &mut Vec<String>,
) -> Result<()> {
    let active_state = reservation_state_name(ReservationState::Active);
    for chunk in member_paths.chunks(256) {
        if chunk.is_empty() {
            continue;
        }
        if let [member_path] = chunk {
            let mut stmt = tx.prepare(
                r#"
                SELECT r.reservation_id
                FROM reservation_generated_members gm INDEXED BY idx_reservation_generated_members_member_path
                JOIN reservations r ON r.reservation_id = gm.reservation_id
                WHERE r.state = ?1 AND r.lease_deadline > ?2 AND gm.member_path = ?3
                "#,
            )?;
            let rows = stmt.query_map(params![active_state, now, member_path], |row| {
                row.get::<_, String>(0)
            })?;
            for row in rows {
                reservation_ids.push(row?);
            }
            continue;
        }
        let sql = format!(
            r#"
            SELECT r.reservation_id
            FROM reservation_generated_members gm INDEXED BY idx_reservation_generated_members_member_path
            JOIN reservations r ON r.reservation_id = gm.reservation_id
            WHERE r.state = ? AND r.lease_deadline > ? AND gm.member_path IN ({})
            "#,
            sql_placeholders(chunk.len())
        );
        let mut values = Vec::with_capacity(chunk.len() + 2);
        values.push(&active_state as &dyn ToSql);
        values.push(&now as &dyn ToSql);
        values.extend(chunk.iter().map(|member_path| member_path as &dyn ToSql));
        let mut stmt = tx.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values), |row| row.get::<_, String>(0))?;
        for row in rows {
            reservation_ids.push(row?);
        }
    }
    Ok(())
}

fn load_reservations_by_ids_tx(
    tx: &Transaction<'_>,
    reservation_ids: Vec<String>,
) -> Result<Vec<Reservation>> {
    if reservation_ids.is_empty() {
        return Ok(Vec::new());
    }
    if let [reservation_id] = reservation_ids.as_slice() {
        let mut stmt = tx.prepare(
            r#"
            SELECT r.reservation_id, r.owner_subtask_id, r.scope_class, r.scope_key,
                   CASE
                       WHEN r.scope_class = 'generated_set' THEN COALESCE((
                           SELECT json_group_array(member_path)
                           FROM reservation_generated_members gm
                           WHERE gm.reservation_id = r.reservation_id
                       ), '[]')
                       ELSE '[]'
                   END,
                   r.lease_deadline, r.state, r.created_at, r.updated_at
            FROM reservations r
            WHERE r.reservation_id = ?1
            ORDER BY r.created_at ASC
            "#,
        )?;
        let rows = stmt.query_map(params![reservation_id], map_reservation)?;
        return collect_rows(rows);
    }
    let sql = format!(
        r#"
        SELECT r.reservation_id, r.owner_subtask_id, r.scope_class, r.scope_key,
               CASE
                   WHEN r.scope_class = 'generated_set' THEN COALESCE((
                       SELECT json_group_array(member_path)
                       FROM reservation_generated_members gm
                       WHERE gm.reservation_id = r.reservation_id
                   ), '[]')
                   ELSE '[]'
               END,
               r.lease_deadline, r.state, r.created_at, r.updated_at
        FROM reservations r
        WHERE r.reservation_id IN ({})
        ORDER BY r.created_at ASC
        "#,
        sql_placeholders(reservation_ids.len())
    );
    let mut stmt = tx.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(reservation_ids.iter()), map_reservation)?;
    collect_rows(rows)
}

fn repoops_path_ancestor_scope_keys(paths: &[String]) -> Vec<String> {
    if let [path] = paths {
        return repoops_single_path_ancestor_scope_keys(path);
    }
    let ancestor_count = paths
        .iter()
        .map(|path| path.bytes().filter(|byte| *byte == b'/').count() + 1)
        .sum();
    let mut ancestors = Vec::with_capacity(ancestor_count);
    for path in paths {
        let mut current = String::with_capacity(path.len());
        for component in path.split('/') {
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(component);
            ancestors.push(current.clone());
        }
    }
    if paths.len() > 1 && ancestors.len() > 1 {
        ancestors.sort_unstable();
        ancestors.dedup();
    }
    ancestors
}

fn repoops_single_path_ancestor_scope_keys(path: &str) -> Vec<String> {
    if !path.as_bytes().contains(&b'/') {
        return vec![path.to_owned()];
    }
    let mut ancestors = Vec::with_capacity(path.bytes().filter(|byte| *byte == b'/').count() + 1);
    let mut current = String::with_capacity(path.len());
    for component in path.split('/') {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(component);
        ancestors.push(current.clone());
    }
    ancestors
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
        params![object_type_name(object_type), object_id],
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
) -> Result<Vec<SubtaskRow>> {
    let mut stmt = tx.prepare(
        "SELECT subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at FROM subtasks WHERE meta_task_id = ?1 ORDER BY priority ASC, created_at ASC",
    )?;
    let rows = stmt.query_map(params![meta_task_id], deserialize_row::<SubtaskRow>)?;
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
    Reservation::try_from_parts(
        row.get(0)?,
        row.get(1)?,
        parse_enum(row.get::<_, String>(2)?)?,
        row.get::<_, String>(3)?,
        parse_generated_members(&row.get::<_, String>(4)?).map_err(to_sql_err)?,
        row.get(5)?,
        parse_enum(row.get::<_, String>(6)?)?,
        row.get(7)?,
        row.get(8)?,
    )
    .map_err(to_sql_err)
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

    OpenSpecImportProvenance::from_storage_parts(
        object_type,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        spec_digests,
        source_digests,
        mission_artifact_digests,
        mission_artifacts,
        row.get(13)?,
        row.get(14)?,
    )
    .map_err(to_sql_err)
}

pub(crate) fn map_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    let actor_kind: ActorKind = parse_enum(row.get::<_, String>(4)?)?;
    let raw_session_token = row.get::<_, String>(5)?;
    let session_token =
        if actor_kind == ActorKind::System && raw_session_token == SYSTEM_EVENT_SESSION_TOKEN {
            None
        } else {
            Some(SessionToken::parse(raw_session_token).map_err(to_sql_err)?)
        };
    let seq = row.get(0)?;
    let event_type = parse_enum(row.get::<_, String>(1)?)?;
    let object_type = parse_enum(row.get::<_, String>(2)?)?;
    let object_id = row.get(3)?;
    let payload_json = row.get(6)?;
    let created_at = row.get::<_, TimestampMs>(7)?;
    match (actor_kind, session_token) {
        (ActorKind::Session, Some(session_token)) => Ok(Event::session(
            seq,
            event_type,
            object_type,
            object_id,
            session_token,
            payload_json,
            created_at,
        )
        .map_err(to_sql_err)?),
        (ActorKind::Session, None) => Err(to_sql_err(
            "session actor events require session_token".to_owned(),
        )),
        (ActorKind::System, None) => Ok(Event::system(
            seq,
            event_type,
            object_type,
            object_id,
            payload_json,
            created_at,
        )
        .map_err(to_sql_err)?),
        (ActorKind::System, Some(_)) => Err(to_sql_err(
            "system actor events must not include session_token".to_owned(),
        )),
    }
}

pub(crate) fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>> {
    let (lower_bound, _) = rows.size_hint();
    let mut collected = Vec::with_capacity(lower_bound);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repoops_single_path_ancestor_fast_path_matches_generic_shape() {
        assert_eq!(
            repoops_path_ancestor_scope_keys(&["src/lib.rs".to_owned()]),
            vec!["src".to_owned(), "src/lib.rs".to_owned()]
        );
        assert_eq!(
            repoops_path_ancestor_scope_keys(&["README.md".to_owned()]),
            vec!["README.md".to_owned()]
        );
        assert_eq!(
            repoops_path_ancestor_scope_keys(&["src/lib.rs".to_owned(), "src/main.rs".to_owned()]),
            vec![
                "src".to_owned(),
                "src/lib.rs".to_owned(),
                "src/main.rs".to_owned()
            ]
        );
    }
}

use std::{borrow::Cow, ops::Deref};

use rusqlite::{
    Connection, OptionalExtension, Statement, ToSql, Transaction, params, params_from_iter,
};

use crate::{
    error::{CoveyError, Result},
    model::{
        ConflictKind, ConflictResolutionState, ObjectType, OverlapCandidate, RequestReservationReq,
        Reservation, ReservationOverlapConflictPayload, ScopeClass, conflict_kind_name,
        conflict_resolution_state_name, object_type_name, parse_generated_members,
    },
};

const SQL_IN_CHUNK_SIZE: usize = 500;

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

trait ReservationExecutor {
    fn prepare<'stmt>(&'stmt self, sql: &str) -> rusqlite::Result<Statement<'stmt>>;
}

impl ReservationExecutor for Connection {
    fn prepare<'stmt>(&'stmt self, sql: &str) -> rusqlite::Result<Statement<'stmt>> {
        Connection::prepare(self, sql)
    }
}

impl ReservationExecutor for Transaction<'_> {
    fn prepare<'stmt>(&'stmt self, sql: &str) -> rusqlite::Result<Statement<'stmt>> {
        self.deref().prepare(sql)
    }
}

pub(crate) fn find_overlapping_reservations_conn(
    conn: &Connection,
    candidate: &OverlapCandidate,
) -> Result<Vec<Reservation>> {
    find_overlapping_reservations(conn, candidate)
}

pub(crate) fn normalize_scope_key(scope_class: ScopeClass, raw: &str) -> Result<String> {
    match scope_class {
        ScopeClass::RepoGlobal => Ok("repo".to_owned()),
        ScopeClass::GeneratedSet => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                Err(CoveyError::InvalidPath {
                    path: raw.to_owned(),
                })
            } else {
                Ok(trimmed.to_owned())
            }
        }
        ScopeClass::ExactPath | ScopeClass::Subtree => normalize_relative_repo_path(raw),
    }
}

pub(crate) fn normalize_generated_members<I, S>(members: I) -> Result<Vec<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let members = members.into_iter();
    let (lower_bound, upper_bound) = members.size_hint();
    let mut normalized = Vec::with_capacity(upper_bound.unwrap_or(lower_bound));
    for member in members {
        normalized.push(normalize_relative_repo_path(member.as_ref())?);
    }
    Ok(sorted_unique_ids(normalized))
}

pub(crate) fn find_overlapping_reservations_tx(
    tx: &Transaction<'_>,
    candidate: &OverlapCandidate,
) -> Result<Vec<Reservation>> {
    find_overlapping_reservations(tx, candidate)
}

fn find_overlapping_reservations<E: ReservationExecutor>(
    exec: &E,
    candidate: &OverlapCandidate,
) -> Result<Vec<Reservation>> {
    if candidate.scope_class() == ScopeClass::RepoGlobal {
        return load_active_reservations(exec);
    }
    let reservation_ids = match candidate.scope_class() {
        ScopeClass::RepoGlobal => unreachable!("repo-global overlap candidates return above"),
        ScopeClass::ExactPath => query_overlap_ids_for_exact(exec, candidate.scope_key())?,
        ScopeClass::Subtree => query_overlap_ids_for_subtree(exec, candidate.scope_key())?,
        ScopeClass::GeneratedSet => {
            let generated_members = candidate.generated_member_strs().collect::<Vec<_>>();
            query_overlap_ids_for_generated(exec, &generated_members)?
        }
    };
    load_reservations_by_ids(exec, &reservation_ids)
}

fn normalize_relative_repo_path(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CoveyError::InvalidPath {
            path: raw.to_owned(),
        });
    }
    let path = if trimmed.as_bytes().contains(&b'\\') {
        Cow::Owned(trimmed.replace('\\', "/"))
    } else {
        Cow::Borrowed(trimmed)
    };
    if path.starts_with('/') || path.starts_with("./../") || path == ".." {
        return Err(CoveyError::InvalidPath {
            path: raw.to_owned(),
        });
    }
    if path.len() >= 2 && path.as_bytes()[1] == b':' {
        return Err(CoveyError::InvalidPath {
            path: raw.to_owned(),
        });
    }

    let mut components = Vec::with_capacity(path.bytes().filter(|byte| *byte == b'/').count() + 1);
    for component in path.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            if components.pop().is_none() {
                return Err(CoveyError::InvalidPath {
                    path: raw.to_owned(),
                });
            }
            continue;
        }
        components.push(component);
    }

    if components.is_empty() {
        return Err(CoveyError::InvalidPath {
            path: raw.to_owned(),
        });
    }

    Ok(components.join("/"))
}

pub(crate) fn record_reservation_overlap_conflicts(
    tx: &Transaction<'_>,
    reservation_id: &str,
    req: &RequestReservationReq,
    overlaps: &[Reservation],
    now: i64,
) -> Result<()> {
    if overlaps.is_empty() {
        return Ok(());
    }
    let requested_scope_key = normalize_scope_key(req.scope_class(), req.scope_key())?;
    for overlap in overlaps {
        let payload = ReservationOverlapConflictPayload::try_from_raw_parts(
            reservation_id,
            overlap.reservation_id.to_string(),
            req.owner_subtask_id.to_string(),
            overlap.owner_subtask_id.to_string(),
            req.scope_class(),
            requested_scope_key.as_str(),
            overlap.scope_class(),
            overlap.scope_key().to_owned(),
        )
        .map_err(|reason| CoveyError::InvalidEventShape { reason })?;
        let (left, right) = ordered_pair(reservation_id, &overlap.reservation_id);
        let conflict_id = format!("conflict_reservation_overlap_{left}_{right}");
        tx.execute(
            r#"
            INSERT INTO conflicts (
                conflict_id, object_type, object_id, conflict_kind, payload_json, detected_at, resolution_state
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(conflict_id) DO UPDATE SET
                payload_json = excluded.payload_json,
                detected_at = excluded.detected_at,
                resolution_state = 'open'
            "#,
            params![
                conflict_id,
                object_type_name(ObjectType::Reservation),
                left,
                conflict_kind_name(ConflictKind::ReservationOverlap),
                serde_json::to_string(&payload)?,
                now,
                conflict_resolution_state_name(ConflictResolutionState::Open),
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn resolve_reservation_overlap_conflicts(
    tx: &Transaction<'_>,
    reservation_id: &str,
    now: i64,
) -> Result<()> {
    let resolved = conflict_resolution_state_name(ConflictResolutionState::Resolved);
    tx.execute(
        r#"
        UPDATE conflicts
        SET resolution_state = ?1,
            detected_at = ?2
        WHERE conflict_kind = 'reservation_overlap'
          AND resolution_state != 'resolved'
          AND json_extract(payload_json, '$.reservation_id') = ?3
        "#,
        params![resolved, now, reservation_id],
    )?;
    tx.execute(
        r#"
        UPDATE conflicts
        SET resolution_state = ?1,
            detected_at = ?2
        WHERE conflict_kind = 'reservation_overlap'
          AND resolution_state != 'resolved'
          AND json_extract(payload_json, '$.overlapping_reservation_id') = ?3
        "#,
        params![
            conflict_resolution_state_name(ConflictResolutionState::Resolved),
            now,
            reservation_id
        ],
    )?;
    Ok(())
}

fn ordered_pair<'a>(left: &'a str, right: &'a str) -> (&'a str, &'a str) {
    if left <= right {
        (left, right)
    } else {
        (right, left)
    }
}

fn query_overlap_ids_for_exact<E: ReservationExecutor>(
    exec: &E,
    path: &str,
) -> Result<Vec<String>> {
    let lease_now = current_lease_tick(exec)?;
    let mut ids = Vec::new();
    ids.extend(query_repo_global_ids_at(exec, lease_now)?);
    ids.extend(query_exact_path_ids_at(exec, lease_now, path)?);
    ids.extend(query_containing_subtree_ids_at(exec, lease_now, path)?);
    ids.extend(query_generated_member_ids_at(exec, lease_now, path)?);
    Ok(sorted_unique_ids(ids))
}

fn query_repo_global_ids_at<E: ReservationExecutor>(
    exec: &E,
    lease_now: i64,
) -> Result<Vec<String>> {
    let mut stmt = exec.prepare(
        "SELECT reservation_id FROM reservations INDEXED BY idx_reservations_active_scope_key_deadline WHERE state = 'active' AND scope_class = 'repo_global' AND lease_deadline > ?1",
    )?;
    collect_id_rows(stmt.query_map(params![lease_now], |row| row.get::<_, String>(0))?)
}

fn query_exact_path_ids_at<E: ReservationExecutor>(
    exec: &E,
    lease_now: i64,
    path: &str,
) -> Result<Vec<String>> {
    let mut stmt = exec.prepare(
        "SELECT reservation_id FROM reservations INDEXED BY idx_reservations_active_scope_key_deadline WHERE state = 'active' AND scope_class = 'exact_path' AND scope_key = ?2 AND lease_deadline > ?1",
    )?;
    collect_id_rows(stmt.query_map(params![lease_now, path], |row| row.get::<_, String>(0))?)
}

fn query_containing_subtree_ids_at<E: ReservationExecutor>(
    exec: &E,
    lease_now: i64,
    path: &str,
) -> Result<Vec<String>> {
    let ancestors = path_ancestor_scope_keys(path);
    let sql = format!(
        r#"
        SELECT reservation_id
        FROM reservations INDEXED BY idx_reservations_active_scope_key_deadline
        WHERE state = 'active'
          AND scope_class = 'subtree'
          AND lease_deadline > ?
          AND scope_key IN ({})
        "#,
        sql_placeholders(ancestors.len())
    );
    let mut values = Vec::with_capacity(1 + ancestors.len());
    values.push(&lease_now as &dyn ToSql);
    values.extend(ancestors.iter().map(|ancestor| ancestor as &dyn ToSql));
    let mut stmt = exec.prepare(&sql)?;
    collect_id_rows(stmt.query_map(params_from_iter(values), |row| row.get::<_, String>(0))?)
}

fn query_generated_member_ids_at<E: ReservationExecutor>(
    exec: &E,
    lease_now: i64,
    member: &str,
) -> Result<Vec<String>> {
    let mut stmt = exec.prepare(
        r#"
        SELECT r.reservation_id
        FROM reservation_generated_members gm INDEXED BY idx_reservation_generated_members_member_path
        CROSS JOIN reservations r
        WHERE r.reservation_id = gm.reservation_id
          AND gm.member_path = ?2
          AND r.state = 'active'
          AND r.scope_class = 'generated_set'
          AND r.lease_deadline > ?1
        "#,
    )?;
    collect_id_rows(stmt.query_map(params![lease_now, member], |row| row.get::<_, String>(0))?)
}

fn query_overlap_ids_for_subtree<E: ReservationExecutor>(
    exec: &E,
    subtree: &str,
) -> Result<Vec<String>> {
    let lease_now = current_lease_tick(exec)?;
    let mut ids = Vec::new();
    ids.extend(query_repo_global_ids_at(exec, lease_now)?);
    ids.extend(query_exact_path_ids_under_subtree_at(
        exec, lease_now, subtree,
    )?);
    ids.extend(query_subtree_ids_under_subtree_at(
        exec, lease_now, subtree,
    )?);
    ids.extend(query_generated_member_ids_under_subtree_at(
        exec, lease_now, subtree,
    )?);
    Ok(sorted_unique_ids(ids))
}

fn query_exact_path_ids_under_subtree_at<E: ReservationExecutor>(
    exec: &E,
    lease_now: i64,
    subtree: &str,
) -> Result<Vec<String>> {
    let mut ids = query_exact_path_ids_at(exec, lease_now, subtree)?;
    ids.extend(query_reservation_scope_ids_under_path_at(
        exec,
        lease_now,
        "exact_path",
        subtree,
    )?);
    Ok(ids)
}

fn query_subtree_ids_under_subtree_at<E: ReservationExecutor>(
    exec: &E,
    lease_now: i64,
    subtree: &str,
) -> Result<Vec<String>> {
    let mut ids = query_containing_subtree_ids_at(exec, lease_now, subtree)?;
    ids.extend(query_reservation_scope_ids_under_path_at(
        exec, lease_now, "subtree", subtree,
    )?);
    Ok(ids)
}

fn query_generated_member_ids_under_subtree_at<E: ReservationExecutor>(
    exec: &E,
    lease_now: i64,
    subtree: &str,
) -> Result<Vec<String>> {
    let mut ids = query_generated_member_ids_at(exec, lease_now, subtree)?;
    ids.extend(query_generated_member_ids_in_descendant_range_at(
        exec, lease_now, subtree,
    )?);
    Ok(ids)
}

fn query_reservation_scope_ids_under_path_at<E: ReservationExecutor>(
    exec: &E,
    lease_now: i64,
    scope_class: &str,
    path: &str,
) -> Result<Vec<String>> {
    let descendant_prefix = descendant_path_prefix(path);
    let Some(upper_bound) = lexicographic_prefix_upper_bound(&descendant_prefix) else {
        return Ok(Vec::new());
    };
    let mut stmt = exec.prepare(
        r#"
        SELECT reservation_id
        FROM reservations INDEXED BY idx_reservations_active_scope_key_deadline
        WHERE state = 'active'
          AND scope_class = ?2
          AND scope_key >= ?3
          AND scope_key < ?4
          AND lease_deadline > ?1
        "#,
    )?;
    collect_id_rows(stmt.query_map(
        params![lease_now, scope_class, descendant_prefix, upper_bound],
        |row| row.get::<_, String>(0),
    )?)
}

fn query_generated_member_ids_in_descendant_range_at<E: ReservationExecutor>(
    exec: &E,
    lease_now: i64,
    path: &str,
) -> Result<Vec<String>> {
    let descendant_prefix = descendant_path_prefix(path);
    let Some(upper_bound) = lexicographic_prefix_upper_bound(&descendant_prefix) else {
        return Ok(Vec::new());
    };
    let mut stmt = exec.prepare(
        r#"
        SELECT r.reservation_id
        FROM reservation_generated_members gm INDEXED BY idx_reservation_generated_members_member_path
        CROSS JOIN reservations r
        WHERE r.reservation_id = gm.reservation_id
          AND gm.member_path >= ?2
          AND gm.member_path < ?3
          AND r.state = 'active'
          AND r.scope_class = 'generated_set'
          AND r.lease_deadline > ?1
        "#,
    )?;
    collect_id_rows(
        stmt.query_map(params![lease_now, descendant_prefix, upper_bound], |row| {
            row.get::<_, String>(0)
        })?,
    )
}

fn query_overlap_ids_for_generated<E: ReservationExecutor>(
    exec: &E,
    members: &[&str],
) -> Result<Vec<String>> {
    let lease_now = current_lease_tick(exec)?;
    let mut ids = Vec::new();
    ids.extend(query_repo_global_ids_at(exec, lease_now)?);
    ids.extend(query_exact_path_ids_for_members_at(
        exec, lease_now, members,
    )?);
    ids.extend(query_containing_subtree_ids_for_members_at(
        exec, lease_now, members,
    )?);
    if !members.is_empty() {
        ids.extend(query_generated_member_ids_for_members(
            exec, lease_now, members,
        )?);
    }
    Ok(sorted_unique_ids(ids))
}

fn query_exact_path_ids_for_members_at<E: ReservationExecutor>(
    exec: &E,
    lease_now: i64,
    members: &[&str],
) -> Result<Vec<String>> {
    query_reservation_scope_ids_for_keys_at(exec, lease_now, "exact_path", members)
}

fn query_containing_subtree_ids_for_members_at<E: ReservationExecutor>(
    exec: &E,
    lease_now: i64,
    members: &[&str],
) -> Result<Vec<String>> {
    let ancestor_key_count = members
        .iter()
        .map(|member| member.bytes().filter(|byte| *byte == b'/').count() + 1)
        .sum();
    let mut ancestor_keys = Vec::with_capacity(ancestor_key_count);
    for member in members {
        ancestor_keys.extend(path_ancestor_scope_keys(member));
    }
    ancestor_keys.sort_unstable();
    ancestor_keys.dedup();
    query_reservation_scope_ids_for_keys_at(exec, lease_now, "subtree", &ancestor_keys)
}

fn query_reservation_scope_ids_for_keys_at<E, K>(
    exec: &E,
    lease_now: i64,
    scope_class: &str,
    keys: &[K],
) -> Result<Vec<String>>
where
    E: ReservationExecutor,
    K: ToSql,
{
    let mut ids = Vec::new();
    for chunk in keys.chunks(SQL_IN_CHUNK_SIZE) {
        if chunk.is_empty() {
            continue;
        }
        let sql = format!(
            r#"
            SELECT reservation_id
            FROM reservations INDEXED BY idx_reservations_active_scope_key_deadline
            WHERE state = 'active'
              AND scope_class = ?
              AND lease_deadline > ?
              AND scope_key IN ({})
            "#,
            sql_placeholders(chunk.len())
        );
        let mut values = Vec::with_capacity(chunk.len() + 2);
        values.push(&scope_class as &dyn ToSql);
        values.push(&lease_now as &dyn ToSql);
        values.extend(chunk.iter().map(|key| key as &dyn ToSql));
        let mut stmt = exec.prepare(&sql)?;
        ids.extend(collect_id_rows(
            stmt.query_map(params_from_iter(values), |row| row.get::<_, String>(0))?,
        )?);
    }
    Ok(ids)
}

fn query_generated_member_ids_for_members<E: ReservationExecutor>(
    exec: &E,
    lease_now: i64,
    members: &[&str],
) -> Result<Vec<String>> {
    if members.is_empty() {
        return Ok(Vec::new());
    }
    let mut ids = Vec::new();
    for chunk in members.chunks(SQL_IN_CHUNK_SIZE) {
        let sql = format!(
            r#"
            SELECT r.reservation_id
            FROM reservation_generated_members gm INDEXED BY idx_reservation_generated_members_member_path
            CROSS JOIN reservations r
            WHERE r.reservation_id = gm.reservation_id
              AND gm.member_path IN ({})
              AND r.state = 'active'
              AND r.scope_class = 'generated_set'
              AND r.lease_deadline > ?
            "#,
            sql_placeholders(chunk.len())
        );
        let mut values = Vec::with_capacity(chunk.len() + 1);
        values.extend(chunk.iter().map(|member| member as &dyn ToSql));
        values.push(&lease_now as &dyn ToSql);
        let mut stmt = exec.prepare(&sql)?;
        ids.extend(collect_id_rows(
            stmt.query_map(params_from_iter(values), |row| row.get::<_, String>(0))?,
        )?);
    }
    Ok(ids)
}

fn descendant_path_prefix(path: &str) -> String {
    format!("{path}/")
}

fn lexicographic_prefix_upper_bound(prefix: &str) -> Option<String> {
    for (idx, ch) in prefix.char_indices().rev() {
        if let Some(next) = char::from_u32(u32::from(ch) + 1) {
            let mut upper_bound = String::with_capacity(idx + next.len_utf8());
            upper_bound.push_str(&prefix[..idx]);
            upper_bound.push(next);
            return Some(upper_bound);
        }
    }
    None
}

fn path_ancestor_scope_keys(path: &str) -> Vec<String> {
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

fn current_lease_tick<E: ReservationExecutor>(exec: &E) -> Result<i64> {
    let mut stmt = exec.prepare("SELECT last_tick_ms FROM lease_clock WHERE clock_id = 1")?;
    let value = stmt
        .query_row([], |row| row.get::<_, i64>(0))
        .optional()?
        .unwrap_or(0);
    Ok(value)
}

fn load_active_reservations<E: ReservationExecutor>(exec: &E) -> Result<Vec<Reservation>> {
    let lease_now = current_lease_tick(exec)?;
    let mut stmt = exec.prepare(
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
        WHERE r.state = 'active' AND r.lease_deadline > ?1
        ORDER BY r.created_at ASC
        "#,
    )?;
    let rows = stmt.query_map(params![lease_now], map_reservation)?;
    let mut reservations = Vec::new();
    for row in rows {
        reservations.push(row?);
    }
    Ok(reservations)
}

fn load_reservations_by_ids<E: ReservationExecutor>(
    exec: &E,
    reservation_ids: &[String],
) -> Result<Vec<Reservation>> {
    if reservation_ids.is_empty() {
        return Ok(Vec::new());
    }
    if let [reservation_id] = reservation_ids {
        let mut stmt = exec.prepare(
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
        )?;
        let rows = stmt.query_map(params![reservation_id], map_reservation)?;
        let mut reservations = Vec::new();
        for row in rows {
            reservations.push(row?);
        }
        return Ok(reservations);
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
    let mut stmt = exec.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(reservation_ids.iter()), map_reservation)?;
    let mut reservations = Vec::new();
    for row in rows {
        reservations.push(row?);
    }
    Ok(reservations)
}

fn map_reservation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Reservation> {
    Reservation::try_from_parts(
        row.get(0)?,
        row.get(1)?,
        row.get::<_, String>(2)?.parse().map_err(to_sql_err)?,
        row.get::<_, String>(3)?,
        parse_generated_members(&row.get::<_, String>(4)?).map_err(to_sql_err)?,
        row.get(5)?,
        row.get::<_, String>(6)?.parse().map_err(to_sql_err)?,
        row.get(7)?,
        row.get(8)?,
    )
    .map_err(to_sql_err)
}

fn collect_id_rows(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<String>>,
) -> Result<Vec<String>> {
    let mut values = Vec::new();
    for row in rows {
        values.push(row?);
    }
    Ok(values)
}

fn sorted_unique_ids(mut ids: Vec<String>) -> Vec<String> {
    if ids.len() <= 1 {
        return ids;
    }
    ids.sort_unstable();
    ids.dedup();
    ids
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
    use super::{
        SQL_IN_CHUNK_SIZE, descendant_path_prefix, find_overlapping_reservations_conn,
        lexicographic_prefix_upper_bound, normalize_generated_members, normalize_scope_key,
        path_ancestor_scope_keys, record_reservation_overlap_conflicts,
        resolve_reservation_overlap_conflicts,
    };
    use crate::{
        CoveyError,
        model::{
            LeaseDeadlineMs, OverlapCandidate, RequestReservationReq, Reservation,
            ReservationState, ScopeClass, TimestampMs,
        },
        schema::{apply_migrations, apply_pragmas},
    };
    use rusqlite::{Connection, params};

    #[test]
    fn normalize_scope_key_canonicalizes_repo_paths() {
        assert_eq!(
            normalize_scope_key(ScopeClass::ExactPath, " ./src//lib.rs ")
                .expect("relative path must normalize"),
            "src/lib.rs"
        );
        assert_eq!(
            normalize_scope_key(ScopeClass::Subtree, "src/./ops/../tests")
                .expect("subtree path must normalize"),
            "src/tests"
        );
        assert_eq!(
            normalize_scope_key(ScopeClass::RepoGlobal, "ignored")
                .expect("repo-global key is synthetic"),
            "repo"
        );
    }

    #[test]
    fn normalize_scope_key_rejects_invalid_inputs() {
        let empty_generated = normalize_scope_key(ScopeClass::GeneratedSet, "   ")
            .expect_err("empty generated set key must fail");
        let absolute_path = normalize_scope_key(ScopeClass::ExactPath, "/tmp/file")
            .expect_err("absolute paths must be rejected");

        assert_eq!(
            empty_generated,
            CoveyError::InvalidPath {
                path: "   ".to_owned(),
            }
        );
        assert_eq!(
            absolute_path,
            CoveyError::InvalidPath {
                path: "/tmp/file".to_owned(),
            }
        );
    }

    #[test]
    fn normalize_generated_members_sorts_dedups_and_normalizes_paths() {
        let members = vec![
            "src//main.rs".to_owned(),
            "./src/lib.rs".to_owned(),
            "src/main.rs".to_owned(),
        ];

        let normalized =
            normalize_generated_members(&members).expect("member set must normalize successfully");

        assert_eq!(normalized, vec!["src/lib.rs", "src/main.rs"]);
    }

    #[test]
    fn normalize_generated_members_rejects_escape_paths() {
        let members = vec!["../secret.txt".to_owned()];

        let err = normalize_generated_members(&members).expect_err("escape paths must be rejected");

        assert_eq!(
            err,
            CoveyError::InvalidPath {
                path: "../secret.txt".to_owned(),
            }
        );
    }

    #[test]
    fn prefix_range_helpers_preserve_path_boundary_semantics() {
        assert_eq!(descendant_path_prefix("src"), "src/");
        assert_eq!(
            lexicographic_prefix_upper_bound("src/").as_deref(),
            Some("src0")
        );
        assert_eq!(
            path_ancestor_scope_keys("src/runtime/lib.rs"),
            vec!["src", "src/runtime", "src/runtime/lib.rs"]
        );
    }

    #[test]
    fn overlap_queries_cover_scope_classes_and_active_lease_filtering() {
        let conn = overlap_conn();
        insert_reservation(
            &conn,
            "repo",
            ScopeClass::RepoGlobal,
            "repo",
            &[],
            lease_deadline(10),
            timestamp(1),
        );
        insert_reservation(
            &conn,
            "exact",
            ScopeClass::ExactPath,
            "src/lib.rs",
            &[],
            lease_deadline(10),
            timestamp(2),
        );
        insert_reservation(
            &conn,
            "tree",
            ScopeClass::Subtree,
            "src",
            &[],
            lease_deadline(10),
            timestamp(3),
        );
        insert_reservation(
            &conn,
            "nested-tree",
            ScopeClass::Subtree,
            "src/runtime",
            &[],
            lease_deadline(10),
            timestamp(4),
        );
        insert_reservation(
            &conn,
            "generated",
            ScopeClass::GeneratedSet,
            "generated-key",
            &["src/generated.rs", "docs/generated.md"],
            lease_deadline(10),
            timestamp(5),
        );
        insert_reservation(
            &conn,
            "expired",
            ScopeClass::ExactPath,
            "src/lib.rs",
            &[],
            lease_deadline(0),
            timestamp(6),
        );
        insert_reservation(
            &conn,
            "expired-generated",
            ScopeClass::GeneratedSet,
            "expired-generated-key",
            &["docs/generated.md"],
            lease_deadline(0),
            timestamp(7),
        );

        let exact = find_overlapping_reservations_conn(
            &conn,
            &OverlapCandidate::new(
                ScopeClass::ExactPath,
                "src/lib.rs".to_owned(),
                Vec::<String>::new(),
            ),
        )
        .expect("exact overlap query should succeed");
        assert_eq!(reservation_ids(&exact), vec!["repo", "exact", "tree"]);

        let subtree = find_overlapping_reservations_conn(
            &conn,
            &OverlapCandidate::new(ScopeClass::Subtree, "src".to_owned(), Vec::<String>::new()),
        )
        .expect("subtree overlap query should succeed");
        assert_eq!(
            reservation_ids(&subtree),
            vec!["repo", "exact", "tree", "nested-tree", "generated"]
        );

        let nested_subtree = find_overlapping_reservations_conn(
            &conn,
            &OverlapCandidate::new(
                ScopeClass::Subtree,
                "src/runtime".to_owned(),
                Vec::<String>::new(),
            ),
        )
        .expect("nested subtree overlap query should succeed");
        assert_eq!(
            reservation_ids(&nested_subtree),
            vec!["repo", "tree", "nested-tree"]
        );

        let generated = find_overlapping_reservations_conn(
            &conn,
            &OverlapCandidate::new(
                ScopeClass::GeneratedSet,
                "generated-key".to_owned(),
                vec!["docs/generated.md".to_owned()],
            ),
        )
        .expect("generated overlap query should succeed");
        assert_eq!(reservation_ids(&generated), vec!["repo", "generated"]);

        let generated_with_path_overlap = find_overlapping_reservations_conn(
            &conn,
            &OverlapCandidate::new(
                ScopeClass::GeneratedSet,
                "generated-key".to_owned(),
                vec!["docs/generated.md".to_owned(), "src/lib.rs".to_owned()],
            ),
        )
        .expect("generated overlap query should include exact and containing subtree overlaps");
        assert_eq!(
            reservation_ids(&generated_with_path_overlap),
            vec!["repo", "exact", "tree", "generated"]
        );

        let repo_global = find_overlapping_reservations_conn(
            &conn,
            &OverlapCandidate::new(
                ScopeClass::RepoGlobal,
                "repo".to_owned(),
                Vec::<String>::new(),
            ),
        )
        .expect("repo-global overlap query should succeed");
        assert_eq!(
            reservation_ids(&repo_global),
            vec!["repo", "exact", "tree", "nested-tree", "generated"]
        );
        let exact_reservation = repo_global
            .iter()
            .find(|reservation| reservation.reservation_id == "exact")
            .expect("exact reservation should be hydrated");
        assert_eq!(exact_reservation.generated_members(), Vec::<String>::new());
        let repo_reservation = repo_global
            .iter()
            .find(|reservation| reservation.reservation_id == "repo")
            .expect("repo-global reservation should be hydrated");
        assert_eq!(repo_reservation.generated_members(), Vec::<String>::new());
        let generated_reservation = repo_global
            .iter()
            .find(|reservation| reservation.reservation_id == "generated")
            .expect("generated-set reservation should be hydrated");
        assert_eq!(
            generated_reservation.generated_members(),
            vec![
                "docs/generated.md".to_owned(),
                "src/generated.rs".to_owned()
            ]
        );

        let empty_generated =
            OverlapCandidate::try_new(ScopeClass::GeneratedSet, "generated-key", Vec::new())
                .expect_err("generated-set candidates require members");
        assert_eq!(
            empty_generated,
            "generated-set reservations require generated_members"
        );
    }

    #[test]
    fn generated_member_overlap_batches_large_member_sets() {
        let conn = overlap_conn();
        let target_member = format!("bulk/{SQL_IN_CHUNK_SIZE}.rs");
        insert_reservation(
            &conn,
            "generated-bulk",
            ScopeClass::GeneratedSet,
            "generated-bulk-key",
            &[target_member.as_str()],
            lease_deadline(10),
            timestamp(1),
        );
        let members = (0..=SQL_IN_CHUNK_SIZE)
            .map(|index| format!("bulk/{index}.rs"))
            .collect::<Vec<_>>();

        let generated = find_overlapping_reservations_conn(
            &conn,
            &OverlapCandidate::new(
                ScopeClass::GeneratedSet,
                "generated-key".to_owned(),
                members,
            ),
        )
        .expect("generated overlap query should succeed across chunks");

        assert_eq!(reservation_ids(&generated), vec!["generated-bulk"]);
    }

    #[test]
    fn overlap_conflict_records_are_upserted_and_resolved_for_either_side() {
        let mut conn = overlap_conn();
        insert_reservation(
            &conn,
            "left",
            ScopeClass::ExactPath,
            "src/lib.rs",
            &[],
            lease_deadline(10),
            timestamp(1),
        );
        insert_reservation(
            &conn,
            "right",
            ScopeClass::Subtree,
            "src",
            &[],
            lease_deadline(10),
            timestamp(2),
        );
        let overlaps = find_overlapping_reservations_conn(
            &conn,
            &OverlapCandidate::new(
                ScopeClass::ExactPath,
                "src/lib.rs".to_owned(),
                Vec::<String>::new(),
            ),
        )
        .expect("overlaps should load");
        let right = overlaps
            .into_iter()
            .find(|reservation| reservation.reservation_id == "right")
            .expect("right reservation should overlap");

        let req = RequestReservationReq::try_from_raw_parts(
            "session-orchestrator",
            "subtask-owner",
            ScopeClass::ExactPath,
            "src/lib.rs",
            Vec::new(),
            10,
            "reservation-idempotency",
        )
        .expect("valid reservation request");

        let tx = conn.transaction().expect("begin write transaction");
        record_reservation_overlap_conflicts(&tx, "left", &req, &[right], 100)
            .expect("record conflict");
        record_reservation_overlap_conflicts(&tx, "left", &req, &[], 101)
            .expect("empty conflict list is accepted");
        resolve_reservation_overlap_conflicts(&tx, "right", 102).expect("resolve conflict");
        tx.commit().expect("commit conflict writes");

        let (object_id, state): (String, String) = conn
            .query_row(
                "SELECT object_id, resolution_state FROM conflicts WHERE conflict_kind = 'reservation_overlap'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("conflict row should exist");
        assert_eq!(object_id, "left");
        assert_eq!(state, "resolved");
    }

    fn overlap_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        apply_pragmas(&conn).expect("apply pragmas");
        apply_migrations(&mut conn).expect("apply schema");
        conn.execute(
            "INSERT INTO sessions (
                session_token, agent_principal_id, agent_instance_id, role, state,
                active_subtask_id, last_heartbeat_at, created_at, updated_at
            ) VALUES ('session-orchestrator', 'principal', 'instance', 'orchestrator', 'active',
                NULL, 1, 1, 1)",
            [],
        )
        .expect("insert session");
        conn.execute(
            "INSERT INTO meta_tasks (
                meta_task_id, prompt_text, state, created_by, created_at, updated_at
            ) VALUES ('meta-task', 'prompt', 'active', 'session-orchestrator', 1, 1)",
            [],
        )
        .expect("insert meta task");
        conn.execute(
            "INSERT INTO subtasks (
                subtask_id, meta_task_id, title, kind, review_target_subtask_id,
                review_target_artifact_digest, state, current_claim_id, artifact_digest,
                priority, created_at, updated_at
            ) VALUES ('subtask-owner', 'meta-task', 'work', 'work', NULL, NULL, 'available',
                NULL, NULL, 100, 1, 1)",
            [],
        )
        .expect("insert subtask");
        conn
    }

    fn insert_reservation(
        conn: &Connection,
        reservation_id: &str,
        scope_class: ScopeClass,
        scope_key: &str,
        members: &[&str],
        lease_deadline: LeaseDeadlineMs,
        created_at: TimestampMs,
    ) {
        conn.execute(
            "INSERT INTO reservations (
                reservation_id, owner_subtask_id, scope_class, scope_key, lease_deadline,
                state, created_at, updated_at
            ) VALUES (?1, 'subtask-owner', ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                reservation_id,
                crate::model::scope_class_name(scope_class),
                scope_key,
                lease_deadline.get(),
                crate::model::reservation_state_name(ReservationState::Active),
                created_at.get()
            ],
        )
        .expect("insert reservation");
        for member in members {
            conn.execute(
                "INSERT INTO reservation_generated_members (reservation_id, member_path) VALUES (?1, ?2)",
                params![reservation_id, member],
            )
            .expect("insert generated member");
        }
    }

    fn reservation_ids(reservations: &[Reservation]) -> Vec<&str> {
        reservations
            .iter()
            .map(|reservation| reservation.reservation_id.as_str())
            .collect()
    }

    fn lease_deadline(value: i64) -> LeaseDeadlineMs {
        LeaseDeadlineMs::parse(value).expect("test lease deadline must be valid")
    }

    fn timestamp(value: i64) -> TimestampMs {
        TimestampMs::parse(value).expect("test timestamp must be valid")
    }
}

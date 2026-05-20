use std::collections::BTreeSet;
use std::ops::Deref;

use rusqlite::{Connection, OptionalExtension, Statement, Transaction, params, params_from_iter};

use crate::{
    error::{CoveyError, Result},
    model::{
        ConflictKind, ConflictResolutionState, ObjectType, OverlapCandidate, RequestReservationReq,
        Reservation, ReservationOverlapConflictPayload, ScopeClass, parse_generated_members,
    },
};

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

pub(crate) fn normalize_generated_members(members: &[String]) -> Result<Vec<String>> {
    let mut normalized = BTreeSet::new();
    for member in members {
        normalized.insert(normalize_relative_repo_path(member)?);
    }
    Ok(normalized.into_iter().collect())
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
    let reservation_ids = match candidate.scope_class() {
        ScopeClass::RepoGlobal => load_active_reservation_ids(exec)?,
        ScopeClass::ExactPath => query_overlap_ids_for_exact(exec, candidate.scope_key())?,
        ScopeClass::Subtree => query_overlap_ids_for_subtree(exec, candidate.scope_key())?,
        ScopeClass::GeneratedSet => {
            let generated_members = candidate.generated_members();
            query_overlap_ids_for_generated(exec, &generated_members)?
        }
    };
    load_reservations_by_ids(exec, &reservation_ids.into_iter().collect::<Vec<_>>())
}

fn normalize_relative_repo_path(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CoveyError::InvalidPath {
            path: raw.to_owned(),
        });
    }
    let path = trimmed.replace('\\', "/");
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

    let mut components = Vec::new();
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
    for overlap in overlaps {
        let payload = ReservationOverlapConflictPayload::try_from_raw_parts(
            reservation_id,
            overlap.reservation_id.to_string(),
            req.owner_subtask_id.to_string(),
            overlap.owner_subtask_id.to_string(),
            req.scope_class(),
            normalize_scope_key(req.scope_class(), req.scope_key())?,
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
                ObjectType::Reservation.to_string(),
                left,
                ConflictKind::ReservationOverlap.to_string(),
                serde_json::to_string(&payload)?,
                now,
                ConflictResolutionState::Open.to_string(),
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
    tx.execute(
        r#"
        UPDATE conflicts
        SET resolution_state = ?1,
            detected_at = ?2
        WHERE conflict_kind = 'reservation_overlap'
          AND resolution_state != ?1
          AND (
                json_extract(payload_json, '$.reservation_id') = ?3
                OR json_extract(payload_json, '$.overlapping_reservation_id') = ?3
              )
        "#,
        params![
            ConflictResolutionState::Resolved.to_string(),
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

fn load_active_reservation_ids<E: ReservationExecutor>(exec: &E) -> Result<BTreeSet<String>> {
    let lease_now = current_lease_tick(exec)?;
    let mut stmt = exec.prepare(
        "SELECT reservation_id FROM reservations WHERE state = 'active' AND lease_deadline > ?1 ORDER BY created_at ASC",
    )?;
    collect_id_rows(stmt.query_map(params![lease_now], |row| row.get::<_, String>(0))?)
}

fn query_overlap_ids_for_exact<E: ReservationExecutor>(
    exec: &E,
    path: &str,
) -> Result<BTreeSet<String>> {
    let lease_now = current_lease_tick(exec)?;
    let mut stmt = exec.prepare(
        r#"
        SELECT DISTINCT r.reservation_id
        FROM reservations r
        LEFT JOIN reservation_generated_members gm ON gm.reservation_id = r.reservation_id
        WHERE r.state = 'active'
          AND r.lease_deadline > ?1
          AND (
                r.scope_class = 'repo_global'
                OR (r.scope_class = 'exact_path' AND r.scope_key = ?2)
                OR (r.scope_class = 'subtree' AND (?2 = r.scope_key OR ?2 LIKE r.scope_key || '/%'))
                OR (r.scope_class = 'generated_set' AND gm.member_path = ?2)
              )
        ORDER BY r.created_at ASC
        "#,
    )?;
    collect_id_rows(stmt.query_map(params![lease_now, path], |row| row.get::<_, String>(0))?)
}

fn query_overlap_ids_for_subtree<E: ReservationExecutor>(
    exec: &E,
    subtree: &str,
) -> Result<BTreeSet<String>> {
    let lease_now = current_lease_tick(exec)?;
    let mut stmt = exec.prepare(
        r#"
        SELECT DISTINCT r.reservation_id
        FROM reservations r
        LEFT JOIN reservation_generated_members gm ON gm.reservation_id = r.reservation_id
        WHERE r.state = 'active'
          AND r.lease_deadline > ?1
          AND (
                r.scope_class = 'repo_global'
                OR (r.scope_class = 'exact_path' AND (r.scope_key = ?2 OR r.scope_key LIKE ?2 || '/%'))
                OR (r.scope_class = 'subtree' AND (
                        r.scope_key = ?2
                        OR r.scope_key LIKE ?2 || '/%'
                        OR ?2 LIKE r.scope_key || '/%'
                    ))
                OR (r.scope_class = 'generated_set' AND (
                        gm.member_path = ?2
                        OR gm.member_path LIKE ?2 || '/%'
                    ))
              )
        ORDER BY r.created_at ASC
        "#,
    )?;
    collect_id_rows(stmt.query_map(params![lease_now, subtree], |row| row.get::<_, String>(0))?)
}

fn query_overlap_ids_for_generated<E: ReservationExecutor>(
    exec: &E,
    members: &[String],
) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    ids.extend(load_repo_global_reservation_ids(exec)?);
    for member in members {
        ids.extend(query_overlap_ids_for_exact(exec, member)?);
    }
    if !members.is_empty() {
        let mut placeholders = Vec::with_capacity(members.len());
        placeholders.resize(members.len(), "?");
        let sql = format!(
            r#"
            SELECT DISTINCT r.reservation_id
            FROM reservations r
            JOIN reservation_generated_members gm ON gm.reservation_id = r.reservation_id
            WHERE r.state = 'active'
              AND r.scope_class = 'generated_set'
              AND gm.member_path IN ({})
            ORDER BY r.created_at ASC
            "#,
            placeholders.join(", ")
        );
        let mut stmt = exec.prepare(&sql)?;
        ids.extend(collect_id_rows(
            stmt.query_map(params_from_iter(members.iter()), |row| {
                row.get::<_, String>(0)
            })?,
        )?);
    }
    Ok(ids)
}

fn load_repo_global_reservation_ids<E: ReservationExecutor>(exec: &E) -> Result<BTreeSet<String>> {
    let lease_now = current_lease_tick(exec)?;
    let mut stmt = exec.prepare(
        "SELECT reservation_id FROM reservations WHERE state = 'active' AND scope_class = 'repo_global' AND lease_deadline > ?1 ORDER BY created_at ASC",
    )?;
    collect_id_rows(stmt.query_map(params![lease_now], |row| row.get::<_, String>(0))?)
}

fn current_lease_tick<E: ReservationExecutor>(exec: &E) -> Result<i64> {
    let mut stmt = exec.prepare("SELECT last_tick_ms FROM lease_clock WHERE clock_id = 1")?;
    let value = stmt
        .query_row([], |row| row.get::<_, i64>(0))
        .optional()?
        .unwrap_or(0);
    Ok(value)
}

fn load_reservations_by_ids<E: ReservationExecutor>(
    exec: &E,
    reservation_ids: &[String],
) -> Result<Vec<Reservation>> {
    if reservation_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut placeholders = Vec::with_capacity(reservation_ids.len());
    placeholders.resize(reservation_ids.len(), "?");
    let sql = format!(
        r#"
        SELECT r.reservation_id, r.owner_subtask_id, r.scope_class, r.scope_key,
               COALESCE((
                   SELECT json_group_array(member_path)
                   FROM reservation_generated_members gm
                   WHERE gm.reservation_id = r.reservation_id
               ), '[]'),
               r.lease_deadline, r.state, r.created_at, r.updated_at
        FROM reservations r
        WHERE r.reservation_id IN ({})
        ORDER BY r.created_at ASC
        "#,
        placeholders.join(", ")
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
) -> Result<BTreeSet<String>> {
    let mut values = BTreeSet::new();
    for row in rows {
        values.insert(row?);
    }
    Ok(values)
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
        find_overlapping_reservations_conn, normalize_generated_members, normalize_scope_key,
        record_reservation_overlap_conflicts, resolve_reservation_overlap_conflicts,
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
            "generated",
            ScopeClass::GeneratedSet,
            "generated-key",
            &["src/generated.rs", "docs/generated.md"],
            lease_deadline(10),
            timestamp(4),
        );
        insert_reservation(
            &conn,
            "expired",
            ScopeClass::ExactPath,
            "src/lib.rs",
            &[],
            lease_deadline(0),
            timestamp(5),
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
            vec!["repo", "exact", "tree", "generated"]
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

        let empty_generated =
            OverlapCandidate::try_new(ScopeClass::GeneratedSet, "generated-key", Vec::new())
                .expect_err("generated-set candidates require members");
        assert_eq!(
            empty_generated,
            "generated-set reservations require generated_members"
        );
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
                scope_class.to_string(),
                scope_key,
                lease_deadline.get(),
                ReservationState::Active.to_string(),
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

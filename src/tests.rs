use std::sync::{Arc, Mutex};

use rusqlite::{Connection, params};

use crate::{
    ClaimNextReq, ClaimReadyQueueReq, Clock, Covey, ManualClock, RegisterSessionReq, Result,
    SessionRole, SubmitMetaTaskReq,
    schema::{apply_migrations, apply_pragmas},
};

impl Covey {
    /// Opens an in-memory Covey database with an injected clock for tests.
    pub fn open_in_memory_with_clock(clock: Arc<dyn Clock>) -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        apply_pragmas(&conn)?;
        apply_migrations(&mut conn)?;
        Ok(Self {
            db_path: None,
            conn: Mutex::new(conn),
            clock,
        })
    }
}

#[test]
fn in_memory_covey_exercises_shared_connection_read_paths() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    let session = covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                "in-memory-orchestrator",
                "in-memory-orchestrator-1",
                SessionRole::Orchestrator,
                "register-in-memory",
            )
            .expect("valid session registration request"),
        )
        .expect("register session");
    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                session.session_token.clone(),
                "exercise in-memory read paths",
                "submit-in-memory",
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");

    assert_eq!(
        covey
            .meta_task_status(&meta_task_id)
            .expect("meta status")
            .meta_task()
            .meta_task_id,
        meta_task_id
    );
    assert!(!covey.fetch_events(0, 10).expect("fetch events").is_empty());
}

#[test]
fn fetch_events_preserves_historical_payloads_that_fail_current_typed_validation() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    let legacy_payload = serde_json::json!({
        "session_token": "session-1",
        "claim_id": "claim-1",
        "fence_seq": 1,
        "artifact_digest": "blake3:artifact",
        "artifact_kind": "patch_bundle",
        "base_rev": "base",
        "manifest_path": "openspec/changes/phase-3/mission/mission-packet.json",
        "changed_paths_digest": "blake3:paths",
        "idempotency_key": "idem-artifact"
    })
    .to_string();

    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "INSERT INTO event_log (event_type, object_type, object_id, actor_kind, session_token, payload_json, created_at)
             VALUES ('artifact_published', 'artifact', 'blake3:artifact', 'session', 'session-1', ?1, 1)",
            params![legacy_payload],
        )
        .expect("insert historical event");
    }

    let events = covey.fetch_events(0, 10).expect("fetch historical events");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload_json(), legacy_payload);
    let typed_err = events[0]
        .typed()
        .expect_err("current typed validation should still reject historical payload");
    assert!(
        typed_err
            .to_string()
            .contains("mission-packet compiler output"),
        "unexpected typed decode error: {typed_err}"
    );
}

#[test]
fn claimable_subtask_availability_reports_reviewer_lane_when_executor_work_is_blocked() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "INSERT INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
             VALUES ('session-orch', 'orch', 'orch-1', 'orchestrator', 'active', NULL, 1, 1, 1, 1)",
            [],
        )
        .expect("insert orchestrator session");
        conn.execute(
            "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at, updated_at)
             VALUES ('meta-availability', 'availability', 'active', 'session-orch', 1, 1)",
            [],
        )
        .expect("insert meta task");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
             VALUES ('dep-work', 'meta-availability', 'dependency', 'work', NULL, NULL, 'available', NULL, NULL, 1, 1, 1)",
            [],
        )
        .expect("insert dependency work");
        conn.execute(
            "INSERT INTO artifacts (artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session, manifest_path, changed_paths_digest, created_at)
             VALUES ('blake3:dep', 'patch_bundle', 'base', 'dep-work', 'session-orch', 'artifact.json', 'blake3:paths', 1)",
            [],
        )
        .expect("insert dependency artifact");
        conn.execute(
            "UPDATE subtasks SET state = 'changes_requested', artifact_digest = 'blake3:dep', updated_at = 2 WHERE subtask_id = 'dep-work'",
            [],
        )
        .expect("mark dependency changes_requested");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
             VALUES ('blocked-work', 'meta-availability', 'blocked work', 'work', NULL, NULL, 'available', NULL, NULL, 2, 2, 2)",
            [],
        )
        .expect("insert blocked available work");
        conn.execute(
            "INSERT INTO subtask_dependencies (subtask_id, depends_on_subtask_id, source_ref, created_at)
             VALUES ('blocked-work', 'dep-work', 'test', 2)",
            [],
        )
        .expect("insert dependency edge");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
             VALUES ('review-work', 'meta-availability', 'review blake3:dep', 'review', 'dep-work', 'blake3:dep', 'available', NULL, NULL, 1, 3, 3)",
            [],
        )
        .expect("insert available review");
    }

    let availability = covey
        .claimable_subtask_availability(None)
        .expect("read availability");
    assert_eq!(availability.executor_claimable_count(), 0);
    assert_eq!(availability.reviewer_claimable_count(), 1);

    let scoped = covey
        .claimable_subtask_availability(Some("meta-availability"))
        .expect("read scoped availability");
    assert_eq!(scoped, availability);
}

#[test]
fn recursive_review_followup_chain_satisfies_work_dependencies() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    let source_digest = "blake3:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let first_repair_digest =
        "blake3:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let second_repair_digest =
        "blake3:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "INSERT INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
             VALUES ('session-exec', 'exec', 'exec-1', 'executor', 'active', NULL, 1, 1, 1, 1)",
            [],
        )
        .expect("insert executor session");
        conn.execute(
            "INSERT INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
             VALUES ('session-orch', 'orch', 'orch-1', 'orchestrator', 'active', NULL, 1, 1, 1, 1)",
            [],
        )
        .expect("insert orchestrator session");
        conn.execute(
            "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at, updated_at)
             VALUES ('meta-chain', 'chain', 'active', 'session-orch', 1, 1)",
            [],
        )
        .expect("insert meta task");
        for (subtask_id, state, priority, updated_at) in [
            ("source-work", "available", 1, 1),
            ("first-repair", "available", 1, 2),
            ("second-repair", "available", 1, 3),
            ("dependent-work", "available", 2, 4),
        ] {
            conn.execute(
                "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
                 VALUES (?1, 'meta-chain', ?1, 'work', NULL, NULL, ?2, NULL, NULL, ?3, ?4, ?4)",
                params![subtask_id, state, priority, updated_at],
            )
            .expect("insert work subtask");
            conn.execute(
                "INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq) VALUES (?1, 1)",
                params![subtask_id],
            )
            .expect("insert fence counter");
        }
        for (subtask_id, artifact_digest) in [
            ("source-work", source_digest),
            ("first-repair", first_repair_digest),
            ("second-repair", second_repair_digest),
        ] {
            conn.execute(
                "INSERT INTO artifacts (artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session, manifest_path, changed_paths_digest, created_at)
                 VALUES (?1, 'patch_bundle', 'base', ?2, 'session-orch', ?2 || '.json', ?1, 1)",
                params![artifact_digest, subtask_id],
            )
            .expect("insert artifact");
            conn.execute(
                "UPDATE subtasks SET state = CASE WHEN subtask_id = 'second-repair' THEN 'applied' ELSE 'changes_requested' END, artifact_digest = ?1 WHERE subtask_id = ?2",
                params![artifact_digest, subtask_id],
            )
            .expect("publish work artifact state");
        }
        for (review_id, review_subtask_id, source_subtask_id, artifact_digest) in [
            (
                "review-source",
                "review-source-subtask",
                "source-work",
                source_digest,
            ),
            (
                "review-first-repair",
                "review-first-repair-subtask",
                "first-repair",
                first_repair_digest,
            ),
        ] {
            conn.execute(
                "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
                 VALUES (?1, 'meta-chain', ?1, 'review', ?2, ?3, 'decided', NULL, NULL, 1, 1, 1)",
                params![review_subtask_id, source_subtask_id, artifact_digest],
            )
            .expect("insert review subtask");
            conn.execute(
                "INSERT INTO reviews (review_id, subtask_id, artifact_digest, review_subtask_id, reviewer_session, state, verdict, findings_digest, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 'session-orch', 'decided', 'changes_requested', ?3, 1, 1)",
                params![review_id, source_subtask_id, artifact_digest, review_subtask_id],
            )
            .expect("insert decided review");
        }
        conn.execute(
            "INSERT INTO subtask_dependencies (subtask_id, depends_on_subtask_id, source_ref, created_at)
             VALUES ('dependent-work', 'source-work', 'test', 4)",
            [],
        )
        .expect("insert dependency edge");
        conn.execute(
            "INSERT INTO review_followup_subtasks (review_id, source_subtask_id, source_artifact_digest, findings_digest, followup_subtask_id, created_by_session, created_at)
             VALUES ('review-source', 'source-work', ?1, ?1, 'first-repair', 'session-orch', 2)",
            params![source_digest],
        )
        .expect("insert first follow-up edge");
        conn.execute(
            "INSERT INTO review_followup_subtasks (review_id, source_subtask_id, source_artifact_digest, findings_digest, followup_subtask_id, created_by_session, created_at)
             VALUES ('review-first-repair', 'first-repair', ?1, ?1, 'second-repair', 'session-orch', 3)",
            params![first_repair_digest],
        )
        .expect("insert second follow-up edge");
    }

    let availability = covey
        .claimable_subtask_availability(Some("meta-chain"))
        .expect("read scoped availability");
    assert_eq!(availability.executor_claimable_count(), 1);

    let claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts_scoped(
                "session-exec",
                30_000,
                Some("meta-chain".to_owned()),
                "claim-dependent-work",
            )
            .expect("valid claim-next request"),
        )
        .expect("claim-next succeeds")
        .expect("dependent work should be claimable");
    assert_eq!(claim.subtask_id.as_str(), "dependent-work");
}

#[test]
fn apply_gate_requeues_orphaned_ready_for_apply_item_before_claiming() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    let digest = "blake3:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "INSERT INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
             VALUES ('session-apply', 'apply', 'apply-1', 'apply_gate', 'active', NULL, 1, 1, 1, 1)",
            [],
        )
        .expect("insert apply-gate session");
        conn.execute(
            "INSERT INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
             VALUES ('session-orch', 'orch', 'orch-1', 'orchestrator', 'active', NULL, 1, 1, 1, 1)",
            [],
        )
        .expect("insert orchestrator session");
        conn.execute(
            "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at, updated_at)
             VALUES ('meta-apply', 'apply', 'active', 'session-orch', 1, 1)",
            [],
        )
        .expect("insert meta task");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
             VALUES ('ready-work', 'meta-apply', 'ready work', 'work', NULL, NULL, 'available', NULL, NULL, 1, 1, 1)",
            [],
        )
        .expect("insert ready work");
        conn.execute(
            "INSERT INTO artifacts (artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session, manifest_path, changed_paths_digest, created_at)
             VALUES (?1, 'patch_bundle', 'base', 'ready-work', 'session-orch', 'artifact.json', ?1, 1)",
            params![digest],
        )
        .expect("insert artifact");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
             VALUES ('review-subtask', 'meta-apply', 'review ready work', 'review', 'ready-work', ?1, 'decided', NULL, NULL, 1, 1, 1)",
            params![digest],
        )
        .expect("insert review subtask");
        conn.execute(
            "UPDATE subtasks SET state = 'ready_for_apply', artifact_digest = ?1 WHERE subtask_id = 'ready-work'",
            params![digest],
        )
        .expect("mark ready work ready for apply");
        conn.execute(
            "INSERT INTO reviews (review_id, subtask_id, artifact_digest, review_subtask_id, reviewer_session, state, verdict, findings_digest, created_at, updated_at)
             VALUES ('review-ready', 'ready-work', ?1, 'review-subtask', 'session-orch', 'decided', 'approve', ?1, 1, 1)",
            params![digest],
        )
        .expect("insert approved review");
        conn.execute(
            "INSERT INTO ready_queue (queue_id, artifact_digest, subtask_id, settlement_target, state, claimed_by_session_token, claim_fence_seq, claim_lease_deadline, enqueued_at, updated_at)
             VALUES ('queue-old', ?1, 'ready-work', 'canonical', 'superseded', NULL, NULL, NULL, 1, 2)",
            params![digest],
        )
        .expect("insert superseded queue row");
    }

    let before = covey.ready_queue_metrics().expect("read queue metrics");
    assert_eq!(before.queued_count(), 0);

    let claim = covey
        .claim_next_ready_queue_item(
            ClaimReadyQueueReq::try_from_raw_parts(
                "session-apply",
                30_000,
                "claim-orphaned-ready-for-apply",
            )
            .expect("valid ready queue claim request"),
        )
        .expect("claim ready queue succeeds")
        .expect("orphaned ready-for-apply item should be requeued and claimed");
    assert_eq!(claim.subtask_id.as_str(), "ready-work");
    assert_ne!(claim.queue_id.as_str(), "queue-old");
}

#[test]
fn claim_candidate_lookup_queries_use_candidate_indexes() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    let conn = covey.conn.lock().expect("covey connection mutex");

    let subtask_indexes = index_names(&conn, "subtasks");
    assert!(
        subtask_indexes
            .iter()
            .any(|name| name == "idx_subtasks_available_review_priority"),
        "global review candidate lookup index is missing: {subtask_indexes:?}"
    );
    assert!(
        subtask_indexes
            .iter()
            .any(|name| name == "idx_subtasks_available_review_meta_priority"),
        "scoped review candidate lookup index is missing: {subtask_indexes:?}"
    );
    assert!(
        subtask_indexes
            .iter()
            .any(|name| name == "idx_subtasks_available_work_priority"),
        "global work candidate lookup index is missing: {subtask_indexes:?}"
    );
    assert!(
        subtask_indexes
            .iter()
            .any(|name| name == "idx_subtasks_available_work_meta_priority"),
        "scoped work candidate lookup index is missing: {subtask_indexes:?}"
    );
    assert!(
        subtask_indexes
            .iter()
            .any(|name| name == "idx_subtasks_nonterminal_updated"),
        "stuck-subtask lookup index is missing: {subtask_indexes:?}"
    );
    assert!(
        subtask_indexes
            .iter()
            .any(|name| name == "idx_subtasks_open_meta"),
        "open-subtask meta refresh index is missing: {subtask_indexes:?}"
    );
    let review_indexes = index_names(&conn, "reviews");
    assert!(
        review_indexes
            .iter()
            .any(|name| name == "idx_reviews_subtask_created"),
        "subtask review status lookup index is missing: {review_indexes:?}"
    );
    let review_followup_indexes = index_names(&conn, "review_followup_subtasks");
    assert!(
        review_followup_indexes
            .iter()
            .any(|name| name == "idx_review_followup_subtasks_source"),
        "review follow-up source lookup index is missing: {review_followup_indexes:?}"
    );
    let ready_queue_indexes = index_names(&conn, "ready_queue");
    assert!(
        ready_queue_indexes
            .iter()
            .any(|name| name == "idx_ready_queue_subtask_enqueued"),
        "subtask ready-queue status lookup index is missing: {ready_queue_indexes:?}"
    );
    assert!(
        ready_queue_indexes
            .iter()
            .any(|name| name == "idx_ready_queue_inflight_claimant_enqueued"),
        "in-flight ready-queue claimant lookup index is missing: {ready_queue_indexes:?}"
    );
    let reservation_indexes = index_names(&conn, "reservations");
    assert!(
        reservation_indexes
            .iter()
            .any(|name| name == "idx_reservations_state_deadline"),
        "reservation expiry lookup index is missing: {reservation_indexes:?}"
    );
    assert!(
        reservation_indexes
            .iter()
            .any(|name| name == "idx_reservations_active_scope_key_deadline"),
        "active reservation overlap scope lookup index is missing: {reservation_indexes:?}"
    );
    let conflict_indexes = index_names(&conn, "conflicts");
    assert!(
        conflict_indexes
            .iter()
            .any(|name| name == "idx_conflicts_detected_desc"),
        "conflict listing index is missing: {conflict_indexes:?}"
    );
    assert!(
        conflict_indexes
            .iter()
            .any(|name| name == "idx_conflicts_reservation_overlap_subject"),
        "reservation-overlap subject conflict index is missing: {conflict_indexes:?}"
    );
    assert!(
        conflict_indexes
            .iter()
            .any(|name| name == "idx_conflicts_reservation_overlap_overlapping"),
        "reservation-overlap overlapping conflict index is missing: {conflict_indexes:?}"
    );
    let claim_indexes = index_names(&conn, "claims");
    assert!(
        claim_indexes
            .iter()
            .any(|name| name == "idx_claims_held_owner_created"),
        "held-claim owner lookup index is missing: {claim_indexes:?}"
    );
    let session_indexes = index_names(&conn, "sessions");
    assert!(
        session_indexes
            .iter()
            .any(|name| name == "idx_sessions_state_token"),
        "session state lookup index is missing: {session_indexes:?}"
    );

    let review_global_plan = query_plan(
        &conn,
        r#"
        SELECT s.subtask_id
        FROM subtasks s
        JOIN meta_tasks m ON m.meta_task_id = s.meta_task_id
        WHERE s.kind = ?1
          AND s.state = ?2
          AND m.state NOT IN (?3, ?4)
        ORDER BY s.priority ASC, s.created_at ASC
        LIMIT 1
        "#,
        params!["review", "available", "completed", "cancelled"],
    );
    assert_plan_uses_index(
        "global review claim candidate lookup",
        &review_global_plan,
        "idx_subtasks_available_review_priority",
    );
    assert!(
        !plan_mentions(&review_global_plan, "USE TEMP B-TREE"),
        "global review claim candidate lookup should stream priority order from the index: {review_global_plan:?}"
    );

    let scoped_review_plan = query_plan(
        &conn,
        r#"
        SELECT s.subtask_id
        FROM subtasks s
        WHERE s.kind = ?1
          AND s.state = ?2
          AND s.meta_task_id = ?3
        ORDER BY s.priority ASC, s.created_at ASC
        LIMIT 1
        "#,
        params!["review", "available", "meta-1"],
    );
    assert_plan_uses_index(
        "scoped review claim candidate lookup",
        &scoped_review_plan,
        "idx_subtasks_available_review_meta_priority",
    );

    let work_global_plan = query_plan(
        &conn,
        r#"
        SELECT s.subtask_id
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
                AND dep.state NOT IN (?6, ?7, ?8, ?9)
          )
        ORDER BY
            MAX(
                s.priority - MIN(MAX(?5 - s.created_at, 0) / 30000, s.priority),
                0
            ) ASC,
            s.priority ASC,
            s.created_at ASC
        LIMIT 1
        "#,
        params![
            "work",
            "available",
            "completed",
            "cancelled",
            1_700_000_000_000_i64,
            "approved",
            "ready_for_apply",
            "applied",
            "decided"
        ],
    );
    assert_plan_uses_index(
        "global work claim candidate lookup",
        &work_global_plan,
        "idx_subtasks_available_work_priority",
    );

    let scoped_work_plan = query_plan(
        &conn,
        r#"
        SELECT s.subtask_id
        FROM subtasks s
        WHERE s.kind = ?1
          AND s.state = ?2
          AND s.meta_task_id = ?4
          AND NOT EXISTS (
              SELECT 1
              FROM subtask_dependencies d
              JOIN subtasks dep ON dep.subtask_id = d.depends_on_subtask_id
              WHERE d.subtask_id = s.subtask_id
                AND dep.state NOT IN (?5, ?6, ?7, ?8)
          )
        ORDER BY
            MAX(
                s.priority - MIN(MAX(?3 - s.created_at, 0) / 30000, s.priority),
                0
            ) ASC,
            s.priority ASC,
            s.created_at ASC
        LIMIT 1
        "#,
        params![
            "work",
            "available",
            1_700_000_000_000_i64,
            "meta-1",
            "approved",
            "ready_for_apply",
            "applied",
            "decided"
        ],
    );
    assert_plan_uses_index(
        "scoped work claim candidate lookup",
        &scoped_work_plan,
        "idx_subtasks_available_work_meta_priority",
    );

    let subtask_reviews_plan = query_plan(
        &conn,
        r#"
        SELECT review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id, verdict, findings_digest, state, created_at, updated_at
        FROM reviews
        WHERE subtask_id = ?1
        ORDER BY created_at ASC
        "#,
        params!["work-1"],
    );
    assert_plan_uses_index(
        "subtask status review lookup",
        &subtask_reviews_plan,
        "idx_reviews_subtask_created",
    );
    assert!(
        !plan_mentions(&subtask_reviews_plan, "USE TEMP B-TREE"),
        "subtask status review lookup should stream created_at order from the index: {subtask_reviews_plan:?}"
    );

    let subtask_ready_queue_plan = query_plan(
        &conn,
        r#"
        SELECT queue_id, artifact_digest, subtask_id, settlement_target, state, claimed_by_session_token, claim_fence_seq, claim_lease_deadline, enqueued_at, updated_at
        FROM ready_queue
        WHERE subtask_id = ?1
        ORDER BY enqueued_at ASC
        "#,
        params!["work-1"],
    );
    assert_plan_uses_index(
        "subtask status ready-queue lookup",
        &subtask_ready_queue_plan,
        "idx_ready_queue_subtask_enqueued",
    );
    assert!(
        !plan_mentions(&subtask_ready_queue_plan, "USE TEMP B-TREE"),
        "subtask status ready-queue lookup should stream enqueued_at order from the index: {subtask_ready_queue_plan:?}"
    );

    let expired_ready_queue_claims_plan = query_plan(
        &conn,
        r#"
        SELECT queue_id, enqueued_at
        FROM ready_queue
        WHERE state = ?1 AND claim_lease_deadline <= ?2
        "#,
        params!["in_flight", 1_700_000_000_000_i64],
    );
    assert_plan_uses_index(
        "expired ready-queue claim lookup",
        &expired_ready_queue_claims_plan,
        "idx_ready_queue_state_claim_deadline",
    );
    assert!(
        !plan_mentions(&expired_ready_queue_claims_plan, "USE TEMP B-TREE"),
        "expired ready-queue claim lookup should not sort before the enqueued_at merge: {expired_ready_queue_claims_plan:?}"
    );

    let inactive_ready_queue_claims_plan = query_plan(
        &conn,
        r#"
        SELECT q.queue_id, q.enqueued_at
        FROM sessions s
        JOIN ready_queue q INDEXED BY idx_ready_queue_inflight_claimant_enqueued
          ON q.claimed_by_session_token = s.session_token
         AND q.state = 'in_flight'
        WHERE s.state IN (?1, ?2)
        "#,
        params!["stale", "exited"],
    );
    assert_plan_uses_index(
        "inactive-session ready-queue claim lookup",
        &inactive_ready_queue_claims_plan,
        "idx_sessions_state_token",
    );
    assert_plan_uses_index(
        "inactive-session ready-queue claim lookup",
        &inactive_ready_queue_claims_plan,
        "idx_ready_queue_inflight_claimant_enqueued",
    );
    assert!(
        !plan_mentions(&inactive_ready_queue_claims_plan, "USE TEMP B-TREE"),
        "inactive-session ready-queue claim lookup should not sort before the enqueued_at merge: {inactive_ready_queue_claims_plan:?}"
    );

    let missing_session_ready_queue_claims_plan = query_plan(
        &conn,
        r#"
        SELECT q.queue_id, q.enqueued_at
        FROM ready_queue q INDEXED BY idx_ready_queue_inflight_claimant_enqueued
        LEFT JOIN sessions s ON s.session_token = q.claimed_by_session_token
        WHERE q.state = 'in_flight'
          AND s.session_token IS NULL
        "#,
        [],
    );
    assert_plan_uses_index(
        "missing-session ready-queue claim lookup",
        &missing_session_ready_queue_claims_plan,
        "idx_ready_queue_inflight_claimant_enqueued",
    );
    assert!(
        !plan_mentions(&missing_session_ready_queue_claims_plan, "USE TEMP B-TREE"),
        "missing-session ready-queue claim lookup should not sort before the enqueued_at merge: {missing_session_ready_queue_claims_plan:?}"
    );

    let stuck_subtasks_plan = query_plan(
        &conn,
        r#"
        SELECT subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest,
               state, current_claim_id, artifact_digest, priority, created_at, updated_at
        FROM subtasks
        WHERE state NOT IN ('available', 'applied', 'abandoned', 'decided')
          AND updated_at <= ?1
        ORDER BY updated_at ASC
        LIMIT ?2
        "#,
        params![1_700_000_000_000_i64, 100_i64],
    );
    assert_plan_uses_index(
        "stuck subtask lookup",
        &stuck_subtasks_plan,
        "idx_subtasks_nonterminal_updated",
    );
    assert!(
        !plan_mentions(&stuck_subtasks_plan, "USE TEMP B-TREE"),
        "stuck subtask lookup should stream updated_at order from the index: {stuck_subtasks_plan:?}"
    );

    let stuck_subtasks_observability_plan = query_plan(
        &conn,
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
        params![1_700_000_000_000_i64, 100_i64],
    );
    assert_plan_uses_index(
        "stuck-subtask observability lookup",
        &stuck_subtasks_observability_plan,
        "idx_subtasks_nonterminal_updated",
    );
    assert!(
        !plan_mentions(&stuck_subtasks_observability_plan, "USE TEMP B-TREE"),
        "stuck-subtask observability lookup should stream updated_at order from the subtask index: {stuck_subtasks_observability_plan:?}"
    );

    let dependency_satisfaction_plan = query_plan(
        &conn,
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM subtask_dependencies d
            JOIN subtasks dep ON dep.subtask_id = d.depends_on_subtask_id
            WHERE d.subtask_id = ?1
              AND dep.state NOT IN (?2, ?3, ?4, ?5)
            LIMIT 1
        )
        "#,
        params![
            "work-1",
            "approved",
            "ready_for_apply",
            "applied",
            "decided"
        ],
    );
    assert_plan_uses_index(
        "dependency satisfaction lookup",
        &dependency_satisfaction_plan,
        "sqlite_autoindex_subtask_dependencies_1",
    );

    let any_subtask_exists_plan = query_plan(
        &conn,
        r#"
        SELECT EXISTS(SELECT 1 FROM subtasks WHERE meta_task_id = ?1 LIMIT 1)
        "#,
        params!["meta-1"],
    );
    assert_plan_uses_index(
        "any-subtask meta refresh existence check",
        &any_subtask_exists_plan,
        "idx_subtasks_meta_task_priority",
    );

    let open_subtask_exists_plan = query_plan(
        &conn,
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM subtasks
            WHERE meta_task_id = ?1
              AND state NOT IN ('applied', 'abandoned', 'decided')
            LIMIT 1
        )
        "#,
        params!["meta-1"],
    );
    assert_plan_uses_index(
        "open-subtask meta refresh existence check",
        &open_subtask_exists_plan,
        "idx_subtasks_open_meta",
    );

    let held_claims_for_meta_plan = query_plan(
        &conn,
        r#"
        SELECT c.claim_id, c.subtask_id, c.owner_session_token, c.fence_seq, c.lease_deadline, c.state, c.created_at, c.updated_at
        FROM subtasks s
        JOIN claims c INDEXED BY idx_claims_one_held_per_subtask
          ON c.subtask_id = s.subtask_id
         AND c.state = 'held'
        WHERE s.meta_task_id = ?1
        ORDER BY c.created_at ASC
        "#,
        params!["meta-1"],
    );
    assert_plan_uses_index(
        "held claims for meta-task lookup",
        &held_claims_for_meta_plan,
        "idx_subtasks_meta_task_priority",
    );
    assert_plan_uses_index(
        "held claims for meta-task lookup",
        &held_claims_for_meta_plan,
        "idx_claims_one_held_per_subtask",
    );

    let held_claim_for_subtask_plan = query_plan(
        &conn,
        r#"
        SELECT c.claim_id, c.subtask_id, c.owner_session_token, c.fence_seq, c.lease_deadline, c.created_at, c.updated_at
        FROM claims c
        JOIN sessions s ON s.session_token = c.owner_session_token
        WHERE c.subtask_id = ?1 AND c.state = ?2
          AND (c.lease_deadline <= ?3 OR s.state <> ?4)
        LIMIT 1
        "#,
        params!["work-1", "held", 1_700_000_000_000_i64, "active"],
    );
    assert_plan_uses_index(
        "held claim for subtask lookup",
        &held_claim_for_subtask_plan,
        "idx_claims_one_held_per_subtask",
    );
    assert!(
        !plan_mentions(&held_claim_for_subtask_plan, "USE TEMP B-TREE"),
        "held claim for subtask lookup should not sort because held claims are unique per subtask: {held_claim_for_subtask_plan:?}"
    );

    let lease_expired_claims_plan = query_plan(
        &conn,
        r#"
        SELECT c.claim_id, c.subtask_id, c.owner_session_token, c.fence_seq, c.lease_deadline, c.state, c.created_at, c.updated_at
        FROM claims c
        WHERE c.state = ?1 AND c.lease_deadline <= ?2
        "#,
        params!["held", 1_700_000_000_000_i64],
    );
    assert_plan_uses_index(
        "lease-expired claim lookup",
        &lease_expired_claims_plan,
        "idx_claims_state_deadline",
    );
    assert!(
        !plan_mentions(&lease_expired_claims_plan, "USE TEMP B-TREE"),
        "lease-expired claim lookup should not sort before the hook-side created_at merge: {lease_expired_claims_plan:?}"
    );

    let expiring_claims_observability_plan = query_plan(
        &conn,
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
        params!["held", 1_700_000_000_000_i64, 100_i64],
    );
    assert_plan_uses_index(
        "expiring-claims observability lookup",
        &expiring_claims_observability_plan,
        "idx_claims_state_deadline",
    );
    assert!(
        !plan_mentions(&expiring_claims_observability_plan, "USE TEMP B-TREE"),
        "expiring-claims observability lookup should stream lease_deadline order from the claim index: {expiring_claims_observability_plan:?}"
    );

    let inactive_session_claims_plan = query_plan(
        &conn,
        r#"
        SELECT c.claim_id, c.subtask_id, c.owner_session_token, c.fence_seq, c.lease_deadline, c.state, c.created_at, c.updated_at
        FROM sessions s
        JOIN claims c INDEXED BY idx_claims_held_owner_created
          ON c.owner_session_token = s.session_token
         AND c.state = 'held'
        WHERE s.state IN (?1, ?2)
        "#,
        params!["stale", "exited"],
    );
    assert_plan_uses_index(
        "inactive-session claim lookup",
        &inactive_session_claims_plan,
        "idx_sessions_state_token",
    );
    assert_plan_uses_index(
        "inactive-session claim lookup",
        &inactive_session_claims_plan,
        "idx_claims_held_owner_created",
    );
    assert!(
        !plan_mentions(&inactive_session_claims_plan, "USE TEMP B-TREE"),
        "inactive-session claim lookup should not sort before the hook-side created_at merge: {inactive_session_claims_plan:?}"
    );

    let expired_reservations_plan = query_plan(
        &conn,
        r#"
        SELECT reservation_id
        FROM reservations
        WHERE state = ?1 AND lease_deadline <= ?2
        "#,
        params!["active", 1_700_000_000_000_i64],
    );
    assert_plan_uses_index(
        "expired reservation lookup",
        &expired_reservations_plan,
        "idx_reservations_state_deadline",
    );
    assert!(
        !plan_mentions(&expired_reservations_plan, "USE TEMP B-TREE"),
        "expired reservation lookup should not sort because expiry resolution is order-independent: {expired_reservations_plan:?}"
    );

    let active_reservations_plan = query_plan(
        &conn,
        r#"
        SELECT reservation_id
        FROM reservations
        WHERE state = 'active' AND lease_deadline > ?1
        "#,
        params![1_700_000_000_000_i64],
    );
    assert_plan_uses_index(
        "active reservation overlap lookup",
        &active_reservations_plan,
        "idx_reservations_state_deadline",
    );
    assert!(
        !plan_mentions(&active_reservations_plan, "USE TEMP B-TREE"),
        "active reservation overlap lookup should not sort because candidate IDs are collected into a set: {active_reservations_plan:?}"
    );

    let repo_global_reservations_plan = query_plan(
        &conn,
        r#"
        SELECT reservation_id
        FROM reservations
        WHERE state = 'active' AND scope_class = 'repo_global' AND lease_deadline > ?1
        "#,
        params![1_700_000_000_000_i64],
    );
    assert_plan_uses_index(
        "repo-global reservation overlap lookup",
        &repo_global_reservations_plan,
        "idx_reservations_state_deadline",
    );
    assert!(
        !plan_mentions(&repo_global_reservations_plan, "USE TEMP B-TREE"),
        "repo-global reservation overlap lookup should not sort because candidate IDs are collected into a set: {repo_global_reservations_plan:?}"
    );

    let indexed_repo_global_reservations_plan = query_plan(
        &conn,
        r#"
        SELECT reservation_id
        FROM reservations INDEXED BY idx_reservations_active_scope_key_deadline
        WHERE state = 'active' AND scope_class = 'repo_global' AND lease_deadline > ?1
        "#,
        params![1_700_000_000_000_i64],
    );
    assert_plan_uses_index(
        "repo-global reservation overlap scope lookup",
        &indexed_repo_global_reservations_plan,
        "idx_reservations_active_scope_key_deadline",
    );
    assert!(
        !plan_mentions(&indexed_repo_global_reservations_plan, "USE TEMP B-TREE"),
        "repo-global reservation overlap scope lookup should not sort before set collection: {indexed_repo_global_reservations_plan:?}"
    );

    let exact_path_reservations_plan = query_plan(
        &conn,
        r#"
        SELECT reservation_id
        FROM reservations INDEXED BY idx_reservations_active_scope_key_deadline
        WHERE state = 'active' AND scope_class = 'exact_path' AND scope_key = ?2 AND lease_deadline > ?1
        "#,
        params![1_700_000_000_000_i64, "src/lib.rs"],
    );
    assert_plan_uses_index(
        "exact-path reservation overlap scope lookup",
        &exact_path_reservations_plan,
        "idx_reservations_active_scope_key_deadline",
    );
    assert!(
        !plan_mentions(&exact_path_reservations_plan, "USE TEMP B-TREE"),
        "exact-path reservation overlap scope lookup should not sort before set collection: {exact_path_reservations_plan:?}"
    );

    let exact_paths_under_subtree_plan = query_plan(
        &conn,
        r#"
        SELECT reservation_id
        FROM reservations INDEXED BY idx_reservations_active_scope_key_deadline
        WHERE state = 'active' AND scope_class = 'exact_path' AND scope_key >= ?2 AND scope_key < ?3 AND lease_deadline > ?1
        "#,
        params![1_700_000_000_000_i64, "src/", "src0"],
    );
    assert_plan_uses_index(
        "exact-paths-under-subtree reservation overlap scope lookup",
        &exact_paths_under_subtree_plan,
        "idx_reservations_active_scope_key_deadline",
    );
    assert!(
        !plan_mentions(&exact_paths_under_subtree_plan, "USE TEMP B-TREE"),
        "exact-paths-under-subtree reservation overlap scope lookup should not sort before set collection: {exact_paths_under_subtree_plan:?}"
    );

    let subtrees_under_subtree_plan = query_plan(
        &conn,
        r#"
        SELECT reservation_id
        FROM reservations INDEXED BY idx_reservations_active_scope_key_deadline
        WHERE state = 'active' AND scope_class = 'subtree' AND scope_key >= ?2 AND scope_key < ?3 AND lease_deadline > ?1
        "#,
        params![1_700_000_000_000_i64, "src/", "src0"],
    );
    assert_plan_uses_index(
        "subtrees-under-subtree reservation overlap scope lookup",
        &subtrees_under_subtree_plan,
        "idx_reservations_active_scope_key_deadline",
    );
    assert!(
        !plan_mentions(&subtrees_under_subtree_plan, "USE TEMP B-TREE"),
        "subtrees-under-subtree reservation overlap scope lookup should not sort before set collection: {subtrees_under_subtree_plan:?}"
    );

    let containing_subtree_reservations_plan = query_plan(
        &conn,
        r#"
        SELECT reservation_id
        FROM reservations INDEXED BY idx_reservations_active_scope_key_deadline
        WHERE state = 'active' AND scope_class = 'subtree' AND lease_deadline > ?1 AND scope_key IN (?2, ?3)
        "#,
        params![1_700_000_000_000_i64, "src", "src/lib.rs"],
    );
    assert_plan_uses_index(
        "containing-subtree reservation overlap scope lookup",
        &containing_subtree_reservations_plan,
        "idx_reservations_active_scope_key_deadline",
    );
    assert!(
        !plan_mentions(&containing_subtree_reservations_plan, "USE TEMP B-TREE"),
        "containing-subtree reservation overlap scope lookup should not sort before set collection: {containing_subtree_reservations_plan:?}"
    );

    let generated_member_reservations_plan = query_plan(
        &conn,
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
        params![1_700_000_000_000_i64, "generated/out.rs"],
    );
    assert_plan_uses_index(
        "generated-member reservation overlap lookup",
        &generated_member_reservations_plan,
        "idx_reservation_generated_members_member_path",
    );
    assert_plan_uses_index(
        "generated-member reservation overlap lookup",
        &generated_member_reservations_plan,
        "sqlite_autoindex_reservations_1",
    );
    assert!(
        !plan_mentions(&generated_member_reservations_plan, "USE TEMP B-TREE"),
        "generated-member reservation overlap lookup should not sort or distinct before set collection: {generated_member_reservations_plan:?}"
    );

    let generated_members_under_subtree_plan = query_plan(
        &conn,
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
        params![1_700_000_000_000_i64, "generated/", "generated0"],
    );
    assert_plan_uses_index(
        "generated-members-under-subtree reservation overlap lookup",
        &generated_members_under_subtree_plan,
        "idx_reservation_generated_members_member_path",
    );
    assert_plan_uses_index(
        "generated-members-under-subtree reservation overlap lookup",
        &generated_members_under_subtree_plan,
        "sqlite_autoindex_reservations_1",
    );
    assert!(
        !plan_mentions(&generated_members_under_subtree_plan, "USE TEMP B-TREE"),
        "generated-members-under-subtree reservation overlap lookup should not sort or distinct before set collection: {generated_members_under_subtree_plan:?}"
    );

    let generated_members_reservations_plan = query_plan(
        &conn,
        r#"
        SELECT r.reservation_id
        FROM reservation_generated_members gm INDEXED BY idx_reservation_generated_members_member_path
        CROSS JOIN reservations r
        WHERE r.reservation_id = gm.reservation_id
          AND gm.member_path IN (?2, ?3)
          AND r.state = 'active'
          AND r.scope_class = 'generated_set'
          AND r.lease_deadline > ?1
        "#,
        params![
            1_700_000_000_000_i64,
            "generated/out.rs",
            "generated/other.rs"
        ],
    );
    assert_plan_uses_index(
        "generated-members reservation overlap lookup",
        &generated_members_reservations_plan,
        "idx_reservation_generated_members_member_path",
    );
    assert_plan_uses_index(
        "generated-members reservation overlap lookup",
        &generated_members_reservations_plan,
        "sqlite_autoindex_reservations_1",
    );
    assert!(
        !plan_mentions(&generated_members_reservations_plan, "USE TEMP B-TREE"),
        "generated-members reservation overlap lookup should not sort or distinct before set collection: {generated_members_reservations_plan:?}"
    );

    let generated_exact_members_plan = query_plan(
        &conn,
        r#"
        SELECT reservation_id
        FROM reservations INDEXED BY idx_reservations_active_scope_key_deadline
        WHERE state = 'active'
          AND scope_class = ?1
          AND lease_deadline > ?2
          AND scope_key IN (?3, ?4)
        "#,
        params![
            "exact_path",
            1_700_000_000_000_i64,
            "generated/out.rs",
            "generated/other.rs"
        ],
    );
    assert_plan_uses_index(
        "generated-candidate exact-path batch overlap lookup",
        &generated_exact_members_plan,
        "idx_reservations_active_scope_key_deadline",
    );
    assert!(
        !plan_mentions(&generated_exact_members_plan, "USE TEMP B-TREE"),
        "generated-candidate exact-path batch overlap lookup should not sort before set collection: {generated_exact_members_plan:?}"
    );

    let generated_ancestor_subtree_plan = query_plan(
        &conn,
        r#"
        SELECT reservation_id
        FROM reservations INDEXED BY idx_reservations_active_scope_key_deadline
        WHERE state = 'active'
          AND scope_class = ?1
          AND lease_deadline > ?2
          AND scope_key IN (?3, ?4, ?5)
        "#,
        params![
            "subtree",
            1_700_000_000_000_i64,
            "generated",
            "generated/pkg",
            "generated/pkg/out.rs"
        ],
    );
    assert_plan_uses_index(
        "generated-candidate ancestor-subtree batch overlap lookup",
        &generated_ancestor_subtree_plan,
        "idx_reservations_active_scope_key_deadline",
    );
    assert!(
        !plan_mentions(&generated_ancestor_subtree_plan, "USE TEMP B-TREE"),
        "generated-candidate ancestor-subtree batch overlap lookup should not sort before set collection: {generated_ancestor_subtree_plan:?}"
    );

    let conflicts_by_detected_plan = query_plan(
        &conn,
        r#"
        SELECT conflict_id, object_type, object_id, conflict_kind, payload_json, detected_at, resolution_state
        FROM conflicts
        ORDER BY detected_at DESC
        LIMIT ?1
        "#,
        params![1_000_i64],
    );
    assert_plan_uses_index(
        "conflict listing",
        &conflicts_by_detected_plan,
        "idx_conflicts_detected_desc",
    );
    assert!(
        !plan_mentions(&conflicts_by_detected_plan, "USE TEMP B-TREE"),
        "conflict listing should stream detected_at order from the index: {conflicts_by_detected_plan:?}"
    );

    let reservation_subject_conflict_plan = query_plan(
        &conn,
        r#"
        UPDATE conflicts
        SET resolution_state = ?1,
            detected_at = ?2
        WHERE conflict_kind = 'reservation_overlap'
          AND resolution_state != 'resolved'
          AND json_extract(payload_json, '$.reservation_id') = ?3
        "#,
        params!["resolved", 1_700_000_000_000_i64, "reservation-1"],
    );
    assert_plan_uses_index(
        "reservation-overlap subject conflict resolution",
        &reservation_subject_conflict_plan,
        "idx_conflicts_reservation_overlap_subject",
    );

    let reservation_overlapping_conflict_plan = query_plan(
        &conn,
        r#"
        UPDATE conflicts
        SET resolution_state = ?1,
            detected_at = ?2
        WHERE conflict_kind = 'reservation_overlap'
          AND resolution_state != 'resolved'
          AND json_extract(payload_json, '$.overlapping_reservation_id') = ?3
        "#,
        params!["resolved", 1_700_000_000_000_i64, "reservation-1"],
    );
    assert_plan_uses_index(
        "reservation-overlap overlapping conflict resolution",
        &reservation_overlapping_conflict_plan,
        "idx_conflicts_reservation_overlap_overlapping",
    );
}

fn index_names(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_index_list(?1)")
        .expect("prepare index list");
    stmt.query_map(params![table], |row| row.get::<_, String>(0))
        .expect("query index list")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect index list")
}

fn query_plan<P>(conn: &Connection, sql: &str, params: P) -> Vec<String>
where
    P: rusqlite::Params,
{
    let mut stmt = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("prepare query plan");
    stmt.query_map(params, |row| row.get::<_, String>(3))
        .expect("query plan")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect query plan")
}

fn assert_plan_uses_index(label: &str, plan: &[String], index_name: &str) {
    assert!(
        plan.iter().any(|step| step.contains(index_name)),
        "{label} should use {index_name}; plan was {plan:?}"
    );
    assert!(
        !plan
            .iter()
            .any(|step| step.contains("SCAN s") && !step.contains(index_name)),
        "{label} should not scan subtasks without the candidate index; plan was {plan:?}"
    );
}

fn plan_mentions(plan: &[String], needle: &str) -> bool {
    plan.iter().any(|step| step.contains(needle))
}

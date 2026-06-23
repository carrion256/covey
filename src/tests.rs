use std::sync::{Arc, Mutex};

use rusqlite::{Connection, params};

use crate::{
    ApplyWorktreeState, BeginOpenSpecArchiveCleanupReq, ClaimNextReq, ClaimReadyQueueReq, Clock,
    Covey, CoveyError, FinishOpenSpecArchiveCleanupReq, ManualClock, OpenSpecArchiveStatusState,
    OpenSpecCurrentWorkBlockerKind, OpenSpecCurrentWorkOwner, OpenSpecCurrentWorkRepairAction,
    OpenSpecCurrentWorkRepairSafety, OpenSpecCurrentWorkState, OperatorBlockerState,
    OperatorBlockerTargetKind, ReadyQueueState, ReconcileApplyQueueReq,
    ReconcileChangesRequestedFollowupsReq, RecordApplyWorktreeReq, RecordOpenSpecArchiveStatusReq,
    RecordOperatorBlockerReq, RecordRuntimeAttestationReq, RegisterSessionReq, ReleaseClaimReq,
    ResolveOperatorBlockerReq, Result, SessionRole, SubmitMetaTaskReq,
    schema::{apply_migrations, apply_pragmas},
};

const TEST_WALL_NOW_MS: i64 = 1_700_000_000_000;

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
fn scheduler_candidate_apis_are_read_only_and_return_exact_ids() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "INSERT INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
             VALUES ('session-orch', 'orch-candidates', 'orch-candidates-1', 'orchestrator', 'active', NULL, 1, 1, 1, 1)",
            [],
        )
        .expect("insert orchestrator session");
        conn.execute(
            "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at, updated_at)
             VALUES ('meta-candidates', 'candidates', 'active', 'session-orch', 1, 1)",
            [],
        )
        .expect("insert meta task");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
             VALUES ('work-candidate', 'meta-candidates', 'work candidate', 'work', NULL, NULL, 'available', NULL, NULL, 5, 2, 2)",
            [],
        )
        .expect("insert work candidate");
        conn.execute(
            "INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq) VALUES ('work-candidate', 1)",
            [],
        )
        .expect("insert work fence counter");
        conn.execute(
            "INSERT INTO artifacts (artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session, manifest_path, changed_paths_digest, created_at)
             VALUES ('blake3:reviewartifact', 'patch_bundle', 'base', 'work-candidate', 'session-orch', 'artifact.json', 'blake3:paths', 3)",
            [],
        )
        .expect("insert artifact");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
             VALUES ('review-candidate', 'meta-candidates', 'review candidate', 'review', 'work-candidate', 'blake3:reviewartifact', 'available', NULL, NULL, 1, 3, 3)",
            [],
        )
        .expect("insert review candidate");
        conn.execute(
            "INSERT INTO ready_queue (queue_id, artifact_digest, subtask_id, settlement_target, state, claimed_by_session_token, claim_fence_seq, claim_lease_deadline, enqueued_at, updated_at)
             VALUES ('queue-candidate', 'blake3:reviewartifact', 'work-candidate', 'canonical', 'queued', NULL, NULL, NULL, 4, 4)",
            [],
        )
        .expect("insert ready queue candidate");
    }

    let before_events = covey.fetch_events(0, 100).expect("events before").len();
    let executor_candidates = covey
        .subtask_candidates(SessionRole::Executor, 10, None)
        .expect("executor candidates");
    let reviewer_candidates = covey
        .subtask_candidates(SessionRole::Reviewer, 10, None)
        .expect("reviewer candidates");
    let queue_candidates = covey.ready_queue_candidates(10).expect("queue candidates");
    let after_events = covey.fetch_events(0, 100).expect("events after").len();

    assert_eq!(before_events, after_events);
    assert_eq!(executor_candidates.len(), 1);
    assert_eq!(executor_candidates[0].subtask_id.as_str(), "work-candidate");
    assert_eq!(reviewer_candidates.len(), 1);
    assert_eq!(
        reviewer_candidates[0].subtask_id.as_str(),
        "review-candidate"
    );
    assert_eq!(queue_candidates.len(), 1);
    assert_eq!(queue_candidates[0].queue_id.as_str(), "queue-candidate");
}

#[test]
fn reconcile_apply_queue_requeues_expired_in_flight_claims() {
    let clock = Arc::new(ManualClock::new(TEST_WALL_NOW_MS));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open covey");
    let orchestrator = covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                "reconcile-queue-orchestrator",
                "reconcile-queue-orchestrator-1",
                SessionRole::Orchestrator,
                "register-reconcile-queue-orchestrator",
            )
            .expect("valid orchestrator registration"),
        )
        .expect("register orchestrator");
    let apply_gate = covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                "reconcile-queue-apply",
                "reconcile-queue-apply-1",
                SessionRole::ApplyGate,
                "register-reconcile-queue-apply",
            )
            .expect("valid apply registration"),
        )
        .expect("register apply gate");
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at, updated_at)
             VALUES ('meta-reconcile-queue', 'reconcile queue', 'active', ?1, 1, 1)",
            params![orchestrator.session_token],
        )
        .expect("insert meta task");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
             VALUES ('work-reconcile-queue', 'meta-reconcile-queue', 'work reconcile queue', 'work', NULL, NULL, 'available', NULL, NULL, 1, 2, 2)",
            [],
        )
        .expect("insert subtask");
        conn.execute(
            "INSERT INTO artifacts (artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session, manifest_path, changed_paths_digest, created_at)
             VALUES ('blake3:reconcile_queue_artifact', 'patch_bundle', 'base', 'work-reconcile-queue', ?1, 'artifact.json', 'blake3:reconcile_queue_paths', 3)",
            params![apply_gate.session_token],
        )
        .expect("insert artifact");
        conn.execute(
            "UPDATE subtasks SET state = 'approved', artifact_digest = 'blake3:reconcile_queue_artifact', updated_at = 3 WHERE subtask_id = 'work-reconcile-queue'",
            [],
        )
        .expect("approve subtask");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
             VALUES ('review-reconcile-queue-subtask', 'meta-reconcile-queue', 'review reconcile queue', 'review', 'work-reconcile-queue', 'blake3:reconcile_queue_artifact', 'decided', NULL, NULL, 1, 4, 4)",
            [],
        )
        .expect("insert review subtask");
        conn.execute(
            "INSERT INTO reviews (review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id, verdict, findings_digest, state, created_at, updated_at)
             VALUES ('review-reconcile-queue', 'work-reconcile-queue', 'blake3:reconcile_queue_artifact', ?1, 'review-reconcile-queue-subtask', 'approve', 'blake3:reconcile_queue_findings', 'decided', 4, 4)",
            params![orchestrator.session_token],
        )
        .expect("insert approved review");
        conn.execute(
            "INSERT INTO ready_queue (queue_id, artifact_digest, subtask_id, settlement_target, state, claimed_by_session_token, claim_fence_seq, claim_lease_deadline, enqueued_at, updated_at)
             VALUES ('queue-reconcile-expired', 'blake3:reconcile_queue_artifact', 'work-reconcile-queue', 'canonical', 'in_flight', ?1, 1, ?2, 5, 5)",
            params![apply_gate.session_token, TEST_WALL_NOW_MS - 1],
        )
        .expect("insert expired in-flight queue item");
    }

    let _reconcile = covey
        .reconcile_apply_queue(
            ReconcileApplyQueueReq::try_from_raw_parts(
                orchestrator.session_token,
                "reconcile-expired-ready-queue",
            )
            .expect("valid reconcile request"),
        )
        .expect("reconcile apply queue");

    let item = covey
        .ready_queue_item("queue-reconcile-expired")
        .expect("queue item");
    assert_eq!(item.state(), ReadyQueueState::Queued);
    assert_eq!(item.claimed_by_session_token(), None);
    assert_eq!(item.claim_lease_deadline(), None);
}

fn seed_archive_session_and_meta(covey: &Covey, change_id: &str) {
    let conn = covey.conn.lock().expect("covey connection mutex");
    conn.execute(
        "INSERT OR IGNORE INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
         VALUES ('session-orch-archive', 'orch-archive', 'orch-archive-1', 'orchestrator', 'active', NULL, 1, 1, 1, 1)",
        [],
    )
    .expect("insert orchestrator session");
    conn.execute(
        "INSERT OR IGNORE INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
         VALUES ('session-exec-archive', 'exec-archive', 'exec-archive-1', 'executor', 'active', NULL, 1, 1, 1, 1)",
        [],
    )
    .expect("insert executor session");
    conn.execute(
        "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at, updated_at)
         VALUES (?1, 'archive fixture', 'active', 'session-orch-archive', 1, 1)",
        params![format!("openspec:{change_id}")],
    )
    .expect("insert OpenSpec meta task");
}

fn seed_archive_scoped_subtask(
    covey: &Covey,
    change_id: &str,
    subtask_id: &str,
    queue_id: Option<&str>,
    artifact_digest: &str,
    state: &str,
    created_at: i64,
) {
    let conn = covey.conn.lock().expect("covey connection mutex");
    let initial_state = if state == "applied" {
        "available"
    } else {
        state
    };
    conn.execute(
        "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'work', NULL, NULL, ?4, NULL, ?5, 1, ?6, ?6)",
        params![
            subtask_id,
            format!("openspec:{change_id}"),
            format!("work {subtask_id}"),
            initial_state,
            Option::<&str>::None,
            created_at,
        ],
    )
    .expect("insert scoped subtask");
    conn.execute(
        "INSERT INTO openspec_subtask_scope (subtask_id, openspec_change_id, openspec_task_id, source_path, scenario_refs_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, '[]', ?5)",
        params![
            subtask_id,
            change_id,
            format!("task-{subtask_id}"),
            format!("openspec/changes/{change_id}/tasks.md"),
            created_at,
        ],
    )
    .expect("insert OpenSpec scope");
    if let Some(queue_id) = queue_id {
        conn.execute(
            "INSERT INTO artifacts (artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session, manifest_path, changed_paths_digest, created_at)
             VALUES (?1, 'patch_bundle', 'base', ?2, 'session-orch-archive', ?3, ?4, ?5)",
            params![
                artifact_digest,
                subtask_id,
                format!("{subtask_id}.json"),
                format!("blake3:paths-{subtask_id}"),
                created_at,
            ],
        )
        .expect("insert artifact");
        conn.execute(
            "UPDATE subtasks SET state = 'applied', artifact_digest = ?2, updated_at = ?3 WHERE subtask_id = ?1",
            params![subtask_id, artifact_digest, created_at],
        )
        .expect("mark scoped subtask applied");
        conn.execute(
            "INSERT INTO ready_queue (queue_id, artifact_digest, subtask_id, settlement_target, state, claimed_by_session_token, claim_fence_seq, claim_lease_deadline, enqueued_at, updated_at)
             VALUES (?1, ?2, ?3, 'canonical', 'applied', NULL, 1, NULL, ?4, ?4)",
            params![queue_id, artifact_digest, subtask_id, created_at],
        )
        .expect("insert applied queue item");
        conn.execute(
            "INSERT INTO landing_receipts (queue_id, artifact_digest, claim_fence_seq, target_ref, landed_commit_oid, recorded_by_session, created_at)
             VALUES (?1, ?2, 1, 'refs/heads/main', ?3, 'session-orch-archive', ?4)",
            params![
                queue_id,
                artifact_digest,
                "0123456789abcdef0123456789abcdef01234567",
                created_at,
            ],
        )
        .expect("insert landing receipt");
        conn.execute(
            "INSERT INTO openspec_archive_status (queue_id, subtask_id, artifact_digest, openspec_change_id, state, blocked_reason, archive_proof_digest, recorded_by_session, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'blocked', 'applied_but_unarchived', NULL, 'session-orch-archive', ?5, ?5)",
            params![queue_id, subtask_id, artifact_digest, change_id, created_at],
        )
        .expect("insert archive blocker");
    }
}

#[test]
fn apply_worktree_registry_marks_cleanup_allowed_after_archive_receipt() {
    let clock = Arc::new(ManualClock::new(TEST_WALL_NOW_MS));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "change-worktree-cleanup");
    let apply_gate = covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                "apply-worktree-cleanup",
                "apply-worktree-cleanup-1",
                SessionRole::ApplyGate,
                "register-apply-worktree-cleanup",
            )
            .expect("valid apply session"),
        )
        .expect("register apply gate");
    covey
        .record_runtime_attestation(
            RecordRuntimeAttestationReq::try_from_parts(
                apply_gate.session_token.to_string(),
                "codex",
                "gpt-5",
                "provider-run-apply-worktree-cleanup",
                "codex-cli",
                Some("process-apply-worktree-cleanup".to_owned()),
                None,
                "blake3:apply-worktree-cleanup-transcript",
                TEST_WALL_NOW_MS,
                TEST_WALL_NOW_MS,
                "runtime-apply-worktree-cleanup",
            )
            .expect("valid runtime attestation"),
        )
        .expect("record runtime attestation");
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
             VALUES ('worktree-cleanup-work', 'openspec:change-worktree-cleanup', 'worktree cleanup work', 'work', NULL, NULL, 'available', NULL, NULL, 1, 2, 2)",
            [],
        )
        .expect("insert worktree cleanup subtask");
        conn.execute(
            "INSERT INTO openspec_subtask_scope (subtask_id, openspec_change_id, openspec_task_id, source_path, scenario_refs_json, updated_at)
             VALUES ('worktree-cleanup-work', 'change-worktree-cleanup', 'task-worktree-cleanup', 'openspec/changes/change-worktree-cleanup/tasks.md', '[]', 2)",
            [],
        )
        .expect("insert OpenSpec scope");
        conn.execute(
            "INSERT INTO artifacts (artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session, manifest_path, changed_paths_digest, created_at)
             VALUES ('blake3:worktree-cleanup-artifact', 'patch_bundle', 'base', 'worktree-cleanup-work', 'session-exec-archive', 'worktree-cleanup-work.json', 'blake3:paths-worktree-cleanup', 2)",
            [],
        )
        .expect("insert artifact");
        conn.execute(
            "UPDATE subtasks SET state = 'ready_for_apply', artifact_digest = 'blake3:worktree-cleanup-artifact', updated_at = 2 WHERE subtask_id = 'worktree-cleanup-work'",
            [],
        )
        .expect("mark worktree cleanup subtask ready");
        conn.execute(
            "INSERT INTO ready_queue (queue_id, artifact_digest, subtask_id, settlement_target, state, claimed_by_session_token, claim_fence_seq, claim_lease_deadline, enqueued_at, updated_at)
             VALUES ('queue-worktree-cleanup', 'blake3:worktree-cleanup-artifact', 'worktree-cleanup-work', 'canonical', 'in_flight', ?1, 1, ?2, 2, 2)",
            params![apply_gate.session_token.as_str(), TEST_WALL_NOW_MS + 60_000],
        )
        .expect("insert in-flight queue");
    }

    let registered = covey
        .record_apply_worktree(
            RecordApplyWorktreeReq::try_from_raw_parts(
                apply_gate.session_token.to_string(),
                "queue-worktree-cleanup",
                "blake3:worktree-cleanup-artifact",
                "/data/tmp/mutai-apply-worktree-cleanup",
                "record-apply-worktree-cleanup",
            )
            .expect("valid worktree registry request"),
        )
        .expect("record apply worktree");
    assert_eq!(registered.state, ApplyWorktreeState::Active);

    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "UPDATE ready_queue SET state = 'applied', claimed_by_session_token = NULL, claim_lease_deadline = NULL, updated_at = 3 WHERE queue_id = 'queue-worktree-cleanup'",
            [],
        )
        .expect("mark queue applied");
        conn.execute(
            "UPDATE subtasks SET state = 'applied', updated_at = 3 WHERE subtask_id = 'worktree-cleanup-work'",
            [],
        )
        .expect("mark subtask applied");
        conn.execute(
            "INSERT INTO openspec_archive_status (queue_id, subtask_id, artifact_digest, openspec_change_id, state, blocked_reason, archive_proof_digest, recorded_by_session, created_at, updated_at)
             VALUES ('queue-worktree-cleanup', 'worktree-cleanup-work', 'blake3:worktree-cleanup-artifact', 'change-worktree-cleanup', 'blocked', 'applied_but_unarchived', NULL, 'session-orch-archive', 3, 3)",
            [],
        )
        .expect("insert archive blocker");
    }
    covey
        .record_openspec_archive_status(
            RecordOpenSpecArchiveStatusReq::try_from_raw_parts(
                "session-orch-archive",
                "queue-worktree-cleanup",
                "blake3:worktree-cleanup-artifact",
                "change-worktree-cleanup",
                OpenSpecArchiveStatusState::Archived,
                None,
                Some("blake3:archive-proof-worktree-cleanup".to_owned()),
                "archive-worktree-cleanup",
            )
            .expect("valid archive status"),
        )
        .expect("record archive receipt");

    let candidates = covey
        .apply_worktree_cleanup_candidates(TEST_WALL_NOW_MS, 10)
        .expect("cleanup candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0].path.as_str(),
        "/data/tmp/mutai-apply-worktree-cleanup"
    );
    assert_eq!(candidates[0].state, ApplyWorktreeState::CleanupAllowed);
}

fn seed_current_work_scoped_subtask(
    covey: &Covey,
    change_id: &str,
    subtask_id: &str,
    state: &str,
    artifact_digest: Option<&str>,
    created_at: i64,
) {
    let conn = covey.conn.lock().expect("covey connection mutex");
    conn.execute(
        "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'work', NULL, NULL, 'available', NULL, NULL, 1, ?4, ?4)",
        params![
            subtask_id,
            format!("openspec:{change_id}"),
            format!("work {subtask_id}"),
            created_at,
        ],
    )
    .expect("insert current-work scoped subtask");
    if let Some(artifact_digest) = artifact_digest {
        conn.execute(
            "INSERT INTO artifacts (artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session, manifest_path, changed_paths_digest, created_at)
             VALUES (?1, 'patch_bundle', 'base', ?2, 'session-orch-archive', ?3, ?4, ?5)",
            params![
                artifact_digest,
                subtask_id,
                format!("{subtask_id}.json"),
                format!("blake3:paths-{subtask_id}"),
                created_at,
            ],
        )
        .expect("insert current-work artifact");
    }
    conn.execute(
        "UPDATE subtasks SET state = ?1, artifact_digest = ?2, updated_at = ?3 WHERE subtask_id = ?4",
        params![state, artifact_digest, created_at, subtask_id],
    )
    .expect("update current-work scoped subtask lifecycle");
    conn.execute(
        "INSERT INTO openspec_subtask_scope (subtask_id, openspec_change_id, openspec_task_id, source_path, scenario_refs_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, '[]', ?5)",
        params![
            subtask_id,
            change_id,
            format!("task-{subtask_id}"),
            format!("openspec/changes/{change_id}/tasks.md"),
            created_at,
        ],
    )
    .expect("insert current-work OpenSpec scope");
}

fn seed_current_work_claimed_subtask(
    covey: &Covey,
    change_id: &str,
    subtask_id: &str,
    claim_id: &str,
    created_at: i64,
) {
    seed_current_work_claimed_subtask_with_deadline(
        covey,
        change_id,
        subtask_id,
        claim_id,
        TEST_WALL_NOW_MS + 60_000,
        created_at,
    );
}

fn seed_current_work_claimed_subtask_with_deadline(
    covey: &Covey,
    change_id: &str,
    subtask_id: &str,
    claim_id: &str,
    lease_deadline_ms: i64,
    created_at: i64,
) {
    seed_current_work_scoped_subtask(covey, change_id, subtask_id, "available", None, created_at);
    let conn = covey.conn.lock().expect("covey connection mutex");
    conn.execute(
        "INSERT INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'executor', 'active', ?4, ?5, ?5, ?5, ?5)",
        params![
            format!("session-{claim_id}"),
            format!("agent-{claim_id}"),
            format!("instance-{claim_id}"),
            subtask_id,
            created_at,
        ],
    )
    .expect("insert executor session");
    conn.execute(
        "INSERT INTO claims (claim_id, subtask_id, owner_session_token, fence_seq, lease_deadline, state, created_at, updated_at)
         VALUES (?1, ?2, ?3, 1, ?4, 'held', ?5, ?5)",
        params![
            claim_id,
            subtask_id,
            format!("session-{claim_id}"),
            lease_deadline_ms,
            created_at,
        ],
    )
    .expect("insert current-work claim");
    conn.execute(
        "UPDATE subtasks SET state = 'claimed', current_claim_id = ?1, updated_at = ?2 WHERE subtask_id = ?3",
        params![claim_id, created_at, subtask_id],
    )
    .expect("mark current-work subtask claimed");
}

fn seed_current_work_queue(
    covey: &Covey,
    subtask_id: &str,
    queue_id: &str,
    artifact_digest: &str,
    state: &str,
    created_at: i64,
) {
    let conn = covey.conn.lock().expect("covey connection mutex");
    let claim_fence_seq = if matches!(state, "applied" | "in_flight") {
        Some(1_i64)
    } else {
        None
    };
    let claimed_by_session = if state == "in_flight" {
        Some("session-orch-archive")
    } else {
        None
    };
    let claim_lease_deadline = if state == "in_flight" {
        Some(TEST_WALL_NOW_MS + 60_000)
    } else {
        None
    };
    conn.execute(
        "INSERT INTO ready_queue (queue_id, artifact_digest, subtask_id, settlement_target, state, claimed_by_session_token, claim_fence_seq, claim_lease_deadline, enqueued_at, updated_at)
         VALUES (?1, ?2, ?3, 'canonical', ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            queue_id,
            artifact_digest,
            subtask_id,
            state,
            claimed_by_session,
            claim_fence_seq,
            claim_lease_deadline,
            created_at,
        ],
    )
    .expect("insert current-work queue item");
}

fn seed_current_work_repair_followup(
    covey: &Covey,
    change_id: &str,
    source_subtask_id: &str,
    source_artifact_digest: &str,
    followup_subtask_id: &str,
    followup_artifact_digest: &str,
    created_at: i64,
) {
    let conn = covey.conn.lock().expect("covey connection mutex");
    let review_subtask_id = format!("review-{source_subtask_id}");
    let review_id = format!("review-{source_subtask_id}");
    let findings_digest = format!("blake3:findings-{source_subtask_id}");
    conn.execute(
        "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'review', ?4, ?5, 'decided', NULL, NULL, 1, ?6, ?6)",
        params![
            review_subtask_id,
            format!("openspec:{change_id}"),
            format!("review {source_subtask_id}"),
            source_subtask_id,
            source_artifact_digest,
            created_at,
        ],
    )
    .expect("insert current-work repair review subtask");
    conn.execute(
        "INSERT INTO reviews (review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id, verdict, findings_digest, state, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'session-orch-archive', ?4, 'changes_requested', ?5, 'decided', ?6, ?6)",
        params![
            review_id,
            source_subtask_id,
            source_artifact_digest,
            review_subtask_id,
            findings_digest,
            created_at,
        ],
    )
    .expect("insert current-work repair review");
    conn.execute(
        "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'work', NULL, NULL, 'available', NULL, NULL, 1, ?4, ?4)",
        params![
            followup_subtask_id,
            format!("openspec:{change_id}"),
            format!("repair {source_subtask_id}"),
            created_at + 1,
        ],
    )
    .expect("insert current-work repair followup");
    conn.execute(
        "INSERT INTO artifacts (artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session, manifest_path, changed_paths_digest, created_at)
         VALUES (?1, 'patch_bundle', 'base', ?2, 'session-exec-archive', ?3, ?4, ?5)",
        params![
            followup_artifact_digest,
            followup_subtask_id,
            format!("{followup_subtask_id}.json"),
            format!("blake3:paths-{followup_subtask_id}"),
            created_at + 1,
        ],
    )
    .expect("insert current-work repair artifact");
    conn.execute(
        "UPDATE subtasks SET state = 'ready_for_apply', artifact_digest = ?2, updated_at = ?3 WHERE subtask_id = ?1",
        params![followup_subtask_id, followup_artifact_digest, created_at + 1],
    )
    .expect("mark current-work repair ready for apply");
    conn.execute(
        "INSERT INTO review_followup_subtasks (
            review_id, source_subtask_id, source_artifact_digest, findings_digest,
            followup_subtask_id, created_by_session, created_at,
            repair_source_path, repair_task_ref, repair_scenario_refs_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'session-orch-archive', ?6, ?7, ?8, '[]')",
        params![
            review_id,
            source_subtask_id,
            source_artifact_digest,
            findings_digest,
            followup_subtask_id,
            created_at + 1,
            format!("openspec/changes/{change_id}/tasks.md"),
            format!("{change_id}:task-{source_subtask_id}"),
        ],
    )
    .expect("insert current-work repair followup link");
}

fn seed_landing_receipt(covey: &Covey, queue_id: &str, artifact_digest: &str, created_at: i64) {
    let conn = covey.conn.lock().expect("covey connection mutex");
    conn.execute(
        "INSERT INTO landing_receipts (queue_id, artifact_digest, claim_fence_seq, target_ref, landed_commit_oid, recorded_by_session, created_at)
         VALUES (?1, ?2, 1, 'refs/heads/main', ?3, 'session-orch-archive', ?4)",
        params![
            queue_id,
            artifact_digest,
            "0123456789abcdef0123456789abcdef01234567",
            created_at,
        ],
    )
    .expect("insert landing receipt");
}

fn seed_apply_gate_blocker(
    covey: &Covey,
    queue_id: &str,
    artifact_digest: &str,
    blocker_kind: &str,
    reason: &str,
    evidence_id: &str,
    created_at: i64,
) {
    let conn = covey.conn.lock().expect("covey connection mutex");
    conn.execute(
        "INSERT OR IGNORE INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
         SELECT ?1, subtask.meta_task_id, ?2, 'review', queue.subtask_id, queue.artifact_digest, 'decided', NULL, NULL, 1, ?3, ?3
         FROM ready_queue queue
         JOIN subtasks subtask ON subtask.subtask_id = queue.subtask_id
         WHERE queue.queue_id = ?4",
        params![
            format!("review-subtask-{queue_id}"),
            format!("review {queue_id}"),
            created_at,
            queue_id,
        ],
    )
    .expect("insert review subtask for apply-gate blocker");
    conn.execute(
        "INSERT OR IGNORE INTO reviews (review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id, verdict, findings_digest, state, created_at, updated_at)
         SELECT ?1, subtask_id, artifact_digest, 'session-orch-archive', ?2, 'approve', ?3, 'decided', ?4, ?4
         FROM ready_queue WHERE queue_id = ?5",
        params![
            format!("review-{queue_id}"),
            format!("review-subtask-{queue_id}"),
            format!("blake3:findings-{queue_id}"),
            created_at,
            queue_id,
        ],
    )
    .expect("insert review for apply-gate blocker");
    conn.execute(
        "INSERT INTO apply_gate_blockers (
            queue_id, artifact_digest, review_id, findings_digest, claim_fence_seq,
            verifier, blocker_kind, reason, evidence_id, recorded_by_session, created_at
         ) VALUES (?1, ?2, ?3, ?4, 1, 'mutai-rs:settlement-apply-gate', ?5, ?6, ?7, 'session-orch-archive', ?8)",
        params![
            queue_id,
            artifact_digest,
            format!("review-{queue_id}"),
            format!("blake3:findings-{queue_id}"),
            blocker_kind,
            reason,
            evidence_id,
            created_at,
        ],
    )
    .expect("insert apply-gate blocker");
}

fn seed_settlement_reconcile_blocker(
    covey: &Covey,
    queue_id: &str,
    artifact_digest: &str,
    reconcile_reason: &str,
    authority_evidence_id: &str,
    created_at: i64,
) {
    let conn = covey.conn.lock().expect("covey connection mutex");
    conn.execute(
        "INSERT OR IGNORE INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
         SELECT ?1, subtask.meta_task_id, ?2, 'review', queue.subtask_id, queue.artifact_digest, 'decided', NULL, NULL, 1, ?3, ?3
         FROM ready_queue queue
         JOIN subtasks subtask ON subtask.subtask_id = queue.subtask_id
         WHERE queue.queue_id = ?4",
        params![
            format!("review-subtask-reconcile-{queue_id}"),
            format!("review reconcile {queue_id}"),
            created_at,
            queue_id,
        ],
    )
    .expect("insert review subtask for settlement reconcile blocker");
    conn.execute(
        "INSERT OR IGNORE INTO reviews (review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id, verdict, findings_digest, state, created_at, updated_at)
         SELECT ?1, subtask_id, artifact_digest, 'session-orch-archive', ?2, 'approve', ?3, 'decided', ?4, ?4
         FROM ready_queue WHERE queue_id = ?5",
        params![
            format!("review-reconcile-{queue_id}"),
            format!("review-subtask-reconcile-{queue_id}"),
            format!("blake3:findings-reconcile-{queue_id}"),
            created_at,
            queue_id,
        ],
    )
    .expect("insert review for settlement reconcile blocker");
    conn.execute(
        "INSERT INTO settlement_reconcile_blockers (
            queue_id, artifact_digest, review_id, findings_digest, claim_fence_seq,
            reconcile_reason, authority_evidence_id, recorded_by_session, created_at
         ) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, 'session-orch-archive', ?7)",
        params![
            queue_id,
            artifact_digest,
            format!("review-reconcile-{queue_id}"),
            format!("blake3:findings-reconcile-{queue_id}"),
            reconcile_reason,
            authority_evidence_id,
            created_at,
        ],
    )
    .expect("insert settlement reconcile blocker");
}

#[test]
fn openspec_current_work_reports_each_covey_derived_state() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");

    seed_archive_session_and_meta(&covey, "current-imported");
    seed_current_work_scoped_subtask(
        &covey,
        "current-imported",
        "current-imported-work",
        "available",
        None,
        2,
    );

    seed_archive_session_and_meta(&covey, "current-claimed");
    seed_current_work_claimed_subtask(
        &covey,
        "current-claimed",
        "current-claimed-work",
        "claim-current-work",
        3,
    );

    seed_archive_session_and_meta(&covey, "current-reviewing");
    seed_current_work_scoped_subtask(
        &covey,
        "current-reviewing",
        "current-reviewing-work",
        "review_pending",
        Some("blake3:current-reviewing"),
        4,
    );

    seed_archive_session_and_meta(&covey, "current-applying");
    seed_current_work_scoped_subtask(
        &covey,
        "current-applying",
        "current-applying-work",
        "ready_for_apply",
        Some("blake3:current-applying"),
        5,
    );
    seed_current_work_queue(
        &covey,
        "current-applying-work",
        "queue-current-applying",
        "blake3:current-applying",
        "queued",
        5,
    );

    seed_archive_session_and_meta(&covey, "current-archived");
    seed_current_work_scoped_subtask(
        &covey,
        "current-archived",
        "current-archived-work",
        "applied",
        Some("blake3:current-archived"),
        6,
    );
    seed_current_work_queue(
        &covey,
        "current-archived-work",
        "queue-current-archived",
        "blake3:current-archived",
        "applied",
        6,
    );
    seed_landing_receipt(
        &covey,
        "queue-current-archived",
        "blake3:current-archived",
        6,
    );
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "INSERT INTO openspec_archive_status (queue_id, subtask_id, artifact_digest, openspec_change_id, state, blocked_reason, archive_proof_digest, recorded_by_session, created_at, updated_at)
             VALUES ('queue-current-archived', 'current-archived-work', 'blake3:current-archived', 'current-archived', 'archived', NULL, 'blake3:archive-current-archived', 'session-orch-archive', 6, 6)",
            [],
        )
        .expect("insert archived status");
    }

    seed_archive_session_and_meta(&covey, "current-blocked");
    seed_current_work_scoped_subtask(
        &covey,
        "current-blocked",
        "current-blocked-work",
        "changes_requested",
        Some("blake3:current-blocked"),
        7,
    );

    let cases = [
        (
            "current-imported",
            OpenSpecCurrentWorkState::Imported,
            OpenSpecCurrentWorkOwner::Executor,
        ),
        (
            "current-claimed",
            OpenSpecCurrentWorkState::Claimed,
            OpenSpecCurrentWorkOwner::Executor,
        ),
        (
            "current-reviewing",
            OpenSpecCurrentWorkState::Reviewing,
            OpenSpecCurrentWorkOwner::Reviewer,
        ),
        (
            "current-applying",
            OpenSpecCurrentWorkState::Applying,
            OpenSpecCurrentWorkOwner::ApplyGate,
        ),
        (
            "current-archived",
            OpenSpecCurrentWorkState::Archived,
            OpenSpecCurrentWorkOwner::Operator,
        ),
        (
            "current-blocked",
            OpenSpecCurrentWorkState::Blocked,
            OpenSpecCurrentWorkOwner::Executor,
        ),
    ];

    for (change_id, expected_state, expected_owner) in cases {
        let current = covey
            .openspec_current_work(change_id)
            .expect("current work");
        assert_eq!(current.state, expected_state, "state for {change_id}");
        assert_eq!(
            current.next_owner, expected_owner,
            "next owner for {change_id}"
        );
    }
}

#[test]
fn openspec_current_work_reports_missing_import_as_blocked() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");

    let current = covey
        .openspec_current_work("missing-current-work")
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Blocked);
    assert_eq!(current.next_owner, OpenSpecCurrentWorkOwner::Covey);
    assert_eq!(current.blockers.len(), 1);
    assert_eq!(
        current.blockers[0].kind,
        OpenSpecCurrentWorkBlockerKind::MissingImport
    );
    assert_eq!(
        current.blockers[0].evidence_id,
        "openspec_current_work:missing_import:missing-current-work"
    );
    assert_eq!(
        current.blockers[0].allowed_repairs,
        vec!["mutai-scheduler orchestrator run-openspec"]
    );
}

#[test]
fn resolves_synthetic_missing_import_current_work_blocker_by_id() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");

    let resolved = covey
        .resolve_openspec_current_work_blocker(
            "blocker_openspec_current_work_missing_import_missing-current-work",
            None,
        )
        .expect("resolve missing import blocker");

    assert_eq!(resolved.openspec_change_id.as_str(), "missing-current-work");
    assert_eq!(
        resolved.blocker.kind,
        OpenSpecCurrentWorkBlockerKind::MissingImport
    );
    assert_eq!(
        resolved.blocker.repair_playbook.repair_action,
        OpenSpecCurrentWorkRepairAction::RunOpenSpec
    );
}

#[test]
fn resolving_unknown_current_work_blocker_fails_without_projection() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");

    let error = covey
        .resolve_openspec_current_work_blocker(
            "blocker_openspec_current_work_subtask_blocked_missing",
            None,
        )
        .expect_err("unknown blocker must fail");

    assert!(matches!(
        error,
        CoveyError::CurrentWorkBlockerNotFound { .. }
    ));
}

#[test]
fn openspec_current_work_reports_expired_claim_as_named_blocker() {
    let clock = Arc::new(ManualClock::new(TEST_WALL_NOW_MS));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-expired-claim");
    seed_current_work_claimed_subtask_with_deadline(
        &covey,
        "current-expired-claim",
        "current-expired-claim-work",
        "claim-current-expired",
        TEST_WALL_NOW_MS,
        2,
    );

    let current = covey
        .openspec_current_work("current-expired-claim")
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Blocked);
    assert_eq!(current.next_owner, OpenSpecCurrentWorkOwner::Covey);
    assert_eq!(
        current
            .claim_ids
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
        vec!["claim-current-expired"]
    );
    assert_eq!(current.blockers.len(), 1);
    assert_eq!(
        current.blockers[0].kind,
        OpenSpecCurrentWorkBlockerKind::ExpiredClaim
    );
    assert_eq!(
        current.blockers[0].blocker_id,
        "blocker_openspec_current_work_expired_claim_claim-current-expired"
    );
    assert_eq!(
        current.blockers[0].evidence_id,
        "openspec_current_work:expired_claim:current-expired-claim-work:claim-current-expired"
    );
    assert_eq!(
        current.blockers[0].claim_id.as_ref().map(|id| id.as_str()),
        Some("claim-current-expired")
    );
    assert_eq!(
        current.blockers[0].allowed_repairs,
        vec![
            "mutai-scheduler orchestrator recover expired-claim",
            "mutai-scheduler orchestrator recover redispatch"
        ]
    );
}

#[test]
fn openspec_current_work_reports_stale_claim_when_threshold_is_explicit() {
    let clock = Arc::new(ManualClock::new(TEST_WALL_NOW_MS));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-stale-claim");
    seed_current_work_claimed_subtask(
        &covey,
        "current-stale-claim",
        "current-stale-claim-work",
        "claim-current-stale",
        2,
    );

    let default_current = covey
        .openspec_current_work("current-stale-claim")
        .expect("default current work");
    assert_eq!(default_current.state, OpenSpecCurrentWorkState::Claimed);
    assert!(default_current.blockers.is_empty());

    let current = covey
        .openspec_current_work_with_stale_claim_threshold("current-stale-claim", Some(60_000))
        .expect("current work with stale threshold");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Blocked);
    assert_eq!(current.next_owner, OpenSpecCurrentWorkOwner::Covey);
    assert_eq!(current.blockers.len(), 1);
    assert_eq!(
        current.blockers[0].kind,
        OpenSpecCurrentWorkBlockerKind::StaleClaim
    );
    assert_eq!(
        current.blockers[0].blocker_id,
        "blocker_openspec_current_work_stale_claim_claim-current-stale"
    );
    assert_eq!(
        current.blockers[0].evidence_id,
        "openspec_current_work:stale_claim:current-stale-claim-work:claim-current-stale:60000"
    );
    assert_eq!(
        current.blockers[0].allowed_repairs,
        vec![
            "mutai-scheduler orchestrator recover dead-claim",
            "mutai-scheduler orchestrator recover operator-blocked"
        ]
    );
    assert!(matches!(
        covey.resolve_openspec_current_work_blocker(&current.blockers[0].blocker_id, None),
        Err(CoveyError::CurrentWorkBlockerNotFound { .. })
    ));
    let resolved = covey
        .resolve_openspec_current_work_blocker(&current.blockers[0].blocker_id, Some(60_000))
        .expect("resolve stale blocker with threshold");
    assert_eq!(resolved.openspec_change_id.as_str(), "current-stale-claim");
    assert_eq!(
        resolved.blocker.repair_playbook.repair_action,
        OpenSpecCurrentWorkRepairAction::RecoverDeadClaim
    );
}

#[test]
fn openspec_current_work_reports_operator_blocker() {
    let clock = Arc::new(ManualClock::new(TEST_WALL_NOW_MS));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-operator-blocked");
    seed_current_work_scoped_subtask(
        &covey,
        "current-operator-blocked",
        "current-operator-blocked-work",
        "available",
        None,
        2,
    );

    let blocker = covey
        .record_operator_blocker(
            RecordOperatorBlockerReq::try_from_raw_parts(
                "session-orch-archive",
                "operator-blocker-current",
                "current-operator-blocked",
                OperatorBlockerTargetKind::Subtask,
                "current-operator-blocked-work",
                None,
                None,
                "hook_state_stale_claim",
                Some("evidence_mutai_scheduler_run:hook_stale_claim:work:claim".to_owned()),
                "record-current-operator-blocker",
            )
            .expect("operator blocker request"),
        )
        .expect("record operator blocker");

    assert_eq!(blocker.reason.as_str(), "hook_state_stale_claim");

    let current = covey
        .openspec_current_work("current-operator-blocked")
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Blocked);
    assert_eq!(current.next_owner, OpenSpecCurrentWorkOwner::Operator);
    assert_eq!(current.blockers.len(), 1);
    assert_eq!(
        current.blockers[0].kind,
        OpenSpecCurrentWorkBlockerKind::HookStateStaleClaim
    );
    assert_eq!(
        current.blockers[0].blocker_id,
        "blocker_openspec_current_work_operator_blocked_operator-blocker-current"
    );
    assert_eq!(
        current.blockers[0].evidence_id,
        "evidence_mutai_scheduler_run:hook_stale_claim:work:claim"
    );
    assert_eq!(
        current.blockers[0].allowed_repairs,
        vec![
            "mutai-scheduler orchestrator current-work",
            "mutai-scheduler orchestrator recover operator-blocked",
            "mutai-scheduler orchestrator recover resolve-operator-blocker"
        ]
    );
    let resolved = covey
        .resolve_openspec_current_work_blocker(&current.blockers[0].blocker_id, None)
        .expect("resolve durable operator blocker");
    assert_eq!(
        resolved.openspec_change_id.as_str(),
        "current-operator-blocked"
    );
    assert_eq!(resolved.blocker, current.blockers[0]);
}

#[test]
fn resolving_operator_blocker_removes_it_from_current_work_but_keeps_audit_row() {
    let clock = Arc::new(ManualClock::new(TEST_WALL_NOW_MS));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-operator-resolved");
    seed_current_work_scoped_subtask(
        &covey,
        "current-operator-resolved",
        "current-operator-resolved-work",
        "available",
        None,
        2,
    );

    covey
        .record_operator_blocker(
            RecordOperatorBlockerReq::try_from_raw_parts(
                "session-orch-archive",
                "operator-blocker-resolved",
                "current-operator-resolved",
                OperatorBlockerTargetKind::Subtask,
                "current-operator-resolved-work",
                None,
                None,
                "scheduler_state_loss",
                Some("evidence_mutai_scheduler_run:scheduler_state_loss:work".to_owned()),
                "record-current-operator-resolved",
            )
            .expect("operator blocker request"),
        )
        .expect("record operator blocker");

    let blocked = covey
        .openspec_current_work("current-operator-resolved")
        .expect("current work before resolve");
    assert_eq!(blocked.state, OpenSpecCurrentWorkState::Blocked);
    assert_eq!(blocked.blockers.len(), 1);

    let resolved = covey
        .resolve_operator_blocker(
            ResolveOperatorBlockerReq::try_from_raw_parts(
                "session-orch-archive",
                "operator-blocker-resolved",
                "repaired",
                "resolve-current-operator-resolved",
            )
            .expect("resolve operator blocker request"),
        )
        .expect("resolve operator blocker");
    assert_eq!(resolved.state, OperatorBlockerState::Resolved);
    assert_eq!(
        resolved
            .resolved_reason
            .as_ref()
            .expect("resolved reason")
            .as_str(),
        "repaired"
    );

    let loaded = covey
        .operator_blocker("operator-blocker-resolved")
        .expect("load resolved operator blocker");
    assert_eq!(loaded.state, OperatorBlockerState::Resolved);

    let current = covey
        .openspec_current_work("current-operator-resolved")
        .expect("current work after resolve");
    assert_eq!(current.state, OpenSpecCurrentWorkState::Imported);
    assert!(current.blockers.is_empty());
}

#[test]
fn resolve_operator_blocker_rejects_different_resolution_after_resolved() {
    let clock = Arc::new(ManualClock::new(TEST_WALL_NOW_MS));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-operator-resolve-collision");
    seed_current_work_scoped_subtask(
        &covey,
        "current-operator-resolve-collision",
        "current-operator-resolve-collision-work",
        "available",
        None,
        2,
    );

    covey
        .record_operator_blocker(
            RecordOperatorBlockerReq::try_from_raw_parts(
                "session-orch-archive",
                "operator-blocker-resolve-collision",
                "current-operator-resolve-collision",
                OperatorBlockerTargetKind::Subtask,
                "current-operator-resolve-collision-work",
                None,
                None,
                "scheduler_state_loss",
                None,
                "record-current-operator-resolve-collision",
            )
            .expect("operator blocker request"),
        )
        .expect("record operator blocker");

    covey
        .resolve_operator_blocker(
            ResolveOperatorBlockerReq::try_from_raw_parts(
                "session-orch-archive",
                "operator-blocker-resolve-collision",
                "repaired",
                "resolve-current-operator-collision-1",
            )
            .expect("first resolve request"),
        )
        .expect("resolve operator blocker");

    let err = covey
        .resolve_operator_blocker(
            ResolveOperatorBlockerReq::try_from_raw_parts(
                "session-orch-archive",
                "operator-blocker-resolve-collision",
                "different_repair",
                "resolve-current-operator-collision-2",
            )
            .expect("second resolve request"),
        )
        .expect_err("different resolve evidence is rejected");
    assert!(
        err.to_string()
            .contains("operator blocker is already resolved with different evidence")
    );
}

#[test]
fn openspec_current_work_classifies_escalated_operator_blocker_reasons() {
    let clock = Arc::new(ManualClock::new(TEST_WALL_NOW_MS));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-typed-operator-blockers");
    seed_current_work_scoped_subtask(
        &covey,
        "current-typed-operator-blockers",
        "current-typed-scheduler-loss-work",
        "available",
        None,
        2,
    );
    seed_current_work_scoped_subtask(
        &covey,
        "current-typed-operator-blockers",
        "current-typed-authority-hold-work",
        "available",
        None,
        3,
    );
    seed_current_work_scoped_subtask(
        &covey,
        "current-typed-operator-blockers",
        "current-typed-git-apply-work",
        "available",
        None,
        4,
    );

    for (blocker_id, subtask_id, reason) in [
        (
            "operator-blocker-scheduler-state-loss",
            "current-typed-scheduler-loss-work",
            "scheduler_state_loss",
        ),
        (
            "operator-blocker-authority-hold",
            "current-typed-authority-hold-work",
            "authority_hold",
        ),
        (
            "operator-blocker-git-apply",
            "current-typed-git-apply-work",
            "git_apply_uncertainty",
        ),
    ] {
        covey
            .record_operator_blocker(
                RecordOperatorBlockerReq::try_from_raw_parts(
                    "session-orch-archive",
                    blocker_id,
                    "current-typed-operator-blockers",
                    OperatorBlockerTargetKind::Subtask,
                    subtask_id,
                    None,
                    None,
                    reason,
                    Some(format!(
                        "evidence_mutai_scheduler_run:{reason}:{subtask_id}"
                    )),
                    format!("record-{blocker_id}"),
                )
                .expect("typed operator blocker request"),
            )
            .expect("record typed operator blocker");
    }

    let current = covey
        .openspec_current_work("current-typed-operator-blockers")
        .expect("current work");
    let kinds = current
        .blockers
        .iter()
        .map(|blocker| blocker.kind)
        .collect::<Vec<_>>();

    assert!(kinds.contains(&OpenSpecCurrentWorkBlockerKind::SchedulerStateLoss));
    assert!(kinds.contains(&OpenSpecCurrentWorkBlockerKind::AuthorityHold));
    assert!(kinds.contains(&OpenSpecCurrentWorkBlockerKind::GitApplyUncertainty));
    assert!(current.blockers.iter().all(|blocker| {
        blocker
            .allowed_repairs
            .contains(&"mutai-scheduler orchestrator current-work".to_owned())
    }));
}

#[test]
fn record_operator_blocker_rejects_id_reuse_with_different_shape() {
    let clock = Arc::new(ManualClock::new(TEST_WALL_NOW_MS));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-operator-collision");
    seed_current_work_scoped_subtask(
        &covey,
        "current-operator-collision",
        "current-operator-collision-work",
        "available",
        None,
        2,
    );

    covey
        .record_operator_blocker(
            RecordOperatorBlockerReq::try_from_raw_parts(
                "session-orch-archive",
                "operator-blocker-collision",
                "current-operator-collision",
                OperatorBlockerTargetKind::Subtask,
                "current-operator-collision-work",
                None,
                None,
                "first_reason",
                None,
                "record-current-operator-collision-1",
            )
            .expect("first operator blocker request"),
        )
        .expect("record first operator blocker");

    let err = covey
        .record_operator_blocker(
            RecordOperatorBlockerReq::try_from_raw_parts(
                "session-orch-archive",
                "operator-blocker-collision",
                "current-operator-collision",
                OperatorBlockerTargetKind::Subtask,
                "current-operator-collision-work",
                None,
                None,
                "second_reason",
                None,
                "record-current-operator-collision-2",
            )
            .expect("second operator blocker request"),
        )
        .expect_err("blocker id collision must reject different shape");

    assert!(
        err.to_string()
            .contains("operator blocker id already exists")
    );
}

#[test]
fn openspec_current_work_is_scoped_to_one_change() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");

    seed_archive_session_and_meta(&covey, "scope-claimed");
    seed_current_work_claimed_subtask(
        &covey,
        "scope-claimed",
        "scope-claimed-work",
        "claim-scope-claimed",
        2,
    );
    seed_archive_session_and_meta(&covey, "scope-applying");
    seed_current_work_scoped_subtask(
        &covey,
        "scope-applying",
        "scope-applying-work",
        "ready_for_apply",
        Some("blake3:scope-applying"),
        3,
    );
    seed_current_work_queue(
        &covey,
        "scope-applying-work",
        "queue-scope-applying",
        "blake3:scope-applying",
        "queued",
        3,
    );

    let current = covey
        .openspec_current_work("scope-claimed")
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Claimed);
    assert_eq!(current.subtask_ids.len(), 1);
    assert_eq!(current.subtask_ids[0].as_str(), "scope-claimed-work");
    assert!(current.queue_ids.is_empty());
}

#[test]
fn openspec_current_work_tracks_repair_followup_ready_queue() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    let change_id = "current-repair-followup";
    let source_subtask_id = "current-repair-source";
    let followup_subtask_id = "current-repair-followup-work";

    seed_archive_session_and_meta(&covey, change_id);
    seed_current_work_scoped_subtask(
        &covey,
        change_id,
        source_subtask_id,
        "changes_requested",
        Some("blake3:current-repair-source"),
        2,
    );
    seed_current_work_repair_followup(
        &covey,
        change_id,
        source_subtask_id,
        "blake3:current-repair-source",
        followup_subtask_id,
        "blake3:current-repair-followup",
        3,
    );
    seed_current_work_queue(
        &covey,
        followup_subtask_id,
        "queue-current-repair-followup",
        "blake3:current-repair-followup",
        "queued",
        4,
    );

    let current = covey
        .openspec_current_work(change_id)
        .expect("current work includes repair followup");
    let followup_scope = covey
        .openspec_change_id_for_subtask(followup_subtask_id)
        .expect("followup scope lookup")
        .expect("followup maps to source OpenSpec change");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Applying);
    assert_eq!(current.next_owner, OpenSpecCurrentWorkOwner::ApplyGate);
    assert_eq!(followup_scope.as_str(), change_id);
    assert!(
        current
            .subtask_ids
            .iter()
            .any(|subtask_id| subtask_id.as_str() == source_subtask_id)
    );
    assert!(
        current
            .subtask_ids
            .iter()
            .any(|subtask_id| subtask_id.as_str() == followup_subtask_id)
    );
    assert!(
        current
            .queue_ids
            .iter()
            .any(|queue_id| queue_id.as_str() == "queue-current-repair-followup")
    );
    assert!(current.blockers.is_empty());
}

#[test]
fn reconcile_changes_requested_revives_abandoned_artifactless_followup() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    let change_id = "current-abandoned-repair";
    let source_subtask_id = "current-abandoned-repair-source";
    let followup_subtask_id = "current-abandoned-repair-followup";

    seed_archive_session_and_meta(&covey, change_id);
    seed_current_work_scoped_subtask(
        &covey,
        change_id,
        source_subtask_id,
        "changes_requested",
        Some("blake3:current-abandoned-repair-source"),
        2,
    );
    seed_current_work_repair_followup(
        &covey,
        change_id,
        source_subtask_id,
        "blake3:current-abandoned-repair-source",
        followup_subtask_id,
        "blake3:current-abandoned-repair-followup",
        3,
    );
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "UPDATE subtasks SET state = 'abandoned', artifact_digest = NULL, updated_at = 4 WHERE subtask_id = ?1",
            params![followup_subtask_id],
        )
        .expect("mark repair followup abandoned without artifact");
    }

    let reconcile = covey
        .reconcile_changes_requested_followups(
            ReconcileChangesRequestedFollowupsReq::try_from_raw_parts(
                "session-orch-archive",
                "revive-abandoned-followup",
            )
            .expect("valid reconcile request"),
        )
        .expect("reconcile followups");

    assert_eq!(reconcile.created_count, 1);
    assert_eq!(
        reconcile.followup_subtask_ids,
        vec![followup_subtask_id.to_owned()]
    );
    let status = covey
        .subtask_status(followup_subtask_id)
        .expect("subtask status");
    assert_eq!(status.subtask().state().to_string(), "available");
    assert!(status.subtask().artifact_digest().is_none());
}

#[test]
fn openspec_current_work_does_not_archive_when_any_scoped_subtask_is_non_terminal() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-nonterminal");
    seed_current_work_scoped_subtask(
        &covey,
        "current-nonterminal",
        "current-terminal-work",
        "applied",
        Some("blake3:current-terminal"),
        2,
    );
    seed_current_work_queue(
        &covey,
        "current-terminal-work",
        "queue-current-terminal",
        "blake3:current-terminal",
        "applied",
        2,
    );
    seed_landing_receipt(
        &covey,
        "queue-current-terminal",
        "blake3:current-terminal",
        2,
    );
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "INSERT INTO openspec_archive_status (queue_id, subtask_id, artifact_digest, openspec_change_id, state, blocked_reason, archive_proof_digest, recorded_by_session, created_at, updated_at)
             VALUES ('queue-current-terminal', 'current-terminal-work', 'blake3:current-terminal', 'current-nonterminal', 'archived', NULL, 'blake3:archive-current-terminal', 'session-orch-archive', 2, 2)",
            [],
        )
        .expect("insert archived status");
    }
    seed_current_work_scoped_subtask(
        &covey,
        "current-nonterminal",
        "current-open-work",
        "available",
        None,
        3,
    );

    let current = covey
        .openspec_current_work("current-nonterminal")
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Imported);
}

#[test]
fn openspec_current_work_defers_archive_blocker_until_all_scoped_subtasks_terminal() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-unarchived-pending-sibling");
    seed_archive_scoped_subtask(
        &covey,
        "current-unarchived-pending-sibling",
        "current-unarchived-pending-applied",
        Some("queue-current-unarchived-pending"),
        "blake3:current-unarchived-pending",
        "applied",
        2,
    );
    seed_current_work_scoped_subtask(
        &covey,
        "current-unarchived-pending-sibling",
        "current-unarchived-pending-open",
        "available",
        None,
        3,
    );

    let current = covey
        .openspec_current_work("current-unarchived-pending-sibling")
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Imported);
    assert_eq!(current.next_owner, OpenSpecCurrentWorkOwner::Executor);
    assert_eq!(current.archive_blockers.len(), 1);
    assert!(current.blockers.is_empty());
}

#[test]
fn openspec_current_work_defers_missing_landing_receipt_until_all_scoped_subtasks_terminal() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-receipt-pending-sibling");
    seed_current_work_scoped_subtask(
        &covey,
        "current-receipt-pending-sibling",
        "current-receipt-pending-applied",
        "applied",
        Some("blake3:current-receipt-pending"),
        2,
    );
    seed_current_work_queue(
        &covey,
        "current-receipt-pending-applied",
        "queue-current-receipt-pending",
        "blake3:current-receipt-pending",
        "applied",
        2,
    );
    seed_current_work_scoped_subtask(
        &covey,
        "current-receipt-pending-sibling",
        "current-receipt-pending-open",
        "available",
        None,
        3,
    );

    let current = covey
        .openspec_current_work("current-receipt-pending-sibling")
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Imported);
    assert_eq!(current.next_owner, OpenSpecCurrentWorkOwner::Executor);
    assert!(
        current
            .blockers
            .iter()
            .all(|blocker| blocker.reason != "landing_receipt_missing"),
        "landing receipt cleanup must not hide open sibling work: {:?}",
        current.blockers
    );
}

#[test]
fn openspec_current_work_applied_but_unarchived_is_named_blocker() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-unarchived");
    seed_archive_scoped_subtask(
        &covey,
        "current-unarchived",
        "current-unarchived-work",
        Some("queue-current-unarchived"),
        "blake3:current-unarchived",
        "applied",
        2,
    );

    let current = covey
        .openspec_current_work("current-unarchived")
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Blocked);
    assert_eq!(
        current.next_owner,
        OpenSpecCurrentWorkOwner::OpenSpecArchive
    );
    assert_eq!(current.blockers.len(), 1);
    assert_eq!(
        current.blockers[0].kind,
        OpenSpecCurrentWorkBlockerKind::AppliedButUnarchived
    );
    assert_eq!(
        current.blockers[0].blocker_id,
        "blocker_openspec_current_work_applied_but_unarchived_queue-current-unarchived"
    );
    assert_eq!(
        current.blockers[0].evidence_id,
        "openspec_current_work:applied_but_unarchived:queue-current-unarchived:blake3:current-unarchived"
    );
    assert_eq!(
        current.blockers[0].queue_id.as_ref().map(|id| id.as_str()),
        Some("queue-current-unarchived")
    );
    assert_eq!(
        current.blockers[0].allowed_repairs,
        vec![
            "mutai-scheduler orchestrator archive-openspec",
            "mutai-scheduler orchestrator recover open-spec-archive-status"
        ]
    );
    assert_eq!(
        current.blockers[0].repair_playbook.repair_action,
        OpenSpecCurrentWorkRepairAction::ArchiveOpenSpec
    );
    assert_eq!(
        current.blockers[0].repair_playbook.repair_safety,
        OpenSpecCurrentWorkRepairSafety::Mutating
    );
    assert_eq!(
        current.blockers[0].repair_playbook.required_evidence_id,
        current.blockers[0].evidence_id
    );
    assert!(
        current.blockers[0]
            .repair_playbook
            .expected_postcondition
            .contains("archived")
    );
    assert!(
        current.blockers[0]
            .repair_playbook
            .rollback_retry_note
            .contains("retry")
    );
    let resolved = covey
        .resolve_openspec_current_work_blocker(&current.blockers[0].blocker_id, None)
        .expect("resolve current-work blocker by id");
    assert_eq!(resolved.openspec_change_id.as_str(), "current-unarchived");
    assert_eq!(resolved.blocker, current.blockers[0]);
}

#[test]
fn openspec_current_work_synthesizes_unarchived_blocker_from_applied_queue() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-unarchived-no-status");
    seed_current_work_scoped_subtask(
        &covey,
        "current-unarchived-no-status",
        "current-unarchived-no-status-work",
        "applied",
        Some("blake3:current-unarchived-no-status"),
        2,
    );
    seed_current_work_queue(
        &covey,
        "current-unarchived-no-status-work",
        "queue-current-unarchived-no-status",
        "blake3:current-unarchived-no-status",
        "applied",
        2,
    );
    seed_landing_receipt(
        &covey,
        "queue-current-unarchived-no-status",
        "blake3:current-unarchived-no-status",
        2,
    );

    let current = covey
        .openspec_current_work("current-unarchived-no-status")
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Blocked);
    assert_eq!(
        current.next_owner,
        OpenSpecCurrentWorkOwner::OpenSpecArchive
    );
    assert!(current.archive_blockers.is_empty());
    assert_eq!(current.blockers.len(), 1);
    assert_eq!(
        current.blockers[0].kind,
        OpenSpecCurrentWorkBlockerKind::AppliedButUnarchived
    );
    assert_eq!(
        current.blockers[0].evidence_id,
        "openspec_current_work:applied_but_unarchived:queue-current-unarchived-no-status:blake3:current-unarchived-no-status"
    );
    assert_eq!(
        current.blockers[0].queue_id.as_ref().map(|id| id.as_str()),
        Some("queue-current-unarchived-no-status")
    );
    assert_eq!(
        current.blockers[0].allowed_repairs,
        vec![
            "mutai-scheduler orchestrator archive-openspec",
            "mutai-scheduler orchestrator recover open-spec-archive-status"
        ]
    );
}

#[test]
fn openspec_current_work_blocks_applied_queue_without_landing_receipt() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-missing-landing-receipt");
    seed_current_work_scoped_subtask(
        &covey,
        "current-missing-landing-receipt",
        "current-missing-landing-receipt-work",
        "applied",
        Some("blake3:current-missing-landing-receipt"),
        2,
    );
    seed_current_work_queue(
        &covey,
        "current-missing-landing-receipt-work",
        "queue-current-missing-landing-receipt",
        "blake3:current-missing-landing-receipt",
        "applied",
        2,
    );

    let current = covey
        .openspec_current_work("current-missing-landing-receipt")
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Blocked);
    assert_eq!(current.next_owner, OpenSpecCurrentWorkOwner::ApplyGate);
    assert!(current.archive_blockers.is_empty());
    assert_eq!(current.blockers.len(), 2);
    assert_eq!(
        current.blockers[0].kind,
        OpenSpecCurrentWorkBlockerKind::GitApplyUncertainty
    );
    assert_eq!(
        current.blockers[0].blocker_id,
        "blocker_openspec_current_work_landing_receipt_missing_queue-current-missing-landing-receipt"
    );
    assert_eq!(
        current.blockers[0].evidence_id,
        "openspec_current_work:landing_receipt_missing:queue-current-missing-landing-receipt:blake3:current-missing-landing-receipt"
    );
    assert_eq!(current.blockers[0].reason, "landing_receipt_missing");
    assert_eq!(
        current.blockers[0].allowed_repairs,
        vec![
            "mutai-scheduler orchestrator current-work",
            "mutai-scheduler orchestrator recover landing-receipt"
        ]
    );
    assert_eq!(
        current.blockers[1].kind,
        OpenSpecCurrentWorkBlockerKind::AppliedButUnarchived
    );
}

#[test]
fn openspec_current_work_treats_duplicate_applied_queue_as_receipted_by_artifact() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-duplicate-receipt");
    seed_current_work_scoped_subtask(
        &covey,
        "current-duplicate-receipt",
        "current-duplicate-receipt-work",
        "applied",
        Some("blake3:current-duplicate-receipt"),
        2,
    );
    seed_current_work_queue(
        &covey,
        "current-duplicate-receipt-work",
        "queue-current-duplicate-receipt-original",
        "blake3:current-duplicate-receipt",
        "applied",
        2,
    );
    seed_current_work_queue(
        &covey,
        "current-duplicate-receipt-work",
        "queue-current-duplicate-receipt-requeue",
        "blake3:current-duplicate-receipt",
        "applied",
        3,
    );
    seed_landing_receipt(
        &covey,
        "queue-current-duplicate-receipt-original",
        "blake3:current-duplicate-receipt",
        2,
    );

    let current = covey
        .openspec_current_work("current-duplicate-receipt")
        .expect("current work");

    assert!(
        current
            .blockers
            .iter()
            .all(|blocker| blocker.reason != "landing_receipt_missing"),
        "duplicate applied queues for an already receipted artifact must not require an impossible second receipt: {:?}",
        current.blockers
    );
    assert_eq!(current.state, OpenSpecCurrentWorkState::Blocked);
    assert_eq!(
        current.next_owner,
        OpenSpecCurrentWorkOwner::OpenSpecArchive
    );
    assert_eq!(
        current
            .blockers
            .iter()
            .filter(|blocker| {
                blocker.kind == OpenSpecCurrentWorkBlockerKind::AppliedButUnarchived
            })
            .count(),
        2
    );
}

#[test]
fn openspec_current_work_reports_native_apply_gate_authority_hold() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-authority-hold");
    seed_current_work_scoped_subtask(
        &covey,
        "current-authority-hold",
        "current-authority-hold-work",
        "approved",
        Some("blake3:current-authority-hold"),
        2,
    );
    seed_current_work_queue(
        &covey,
        "current-authority-hold-work",
        "queue-current-authority-hold",
        "blake3:current-authority-hold",
        "in_flight",
        2,
    );
    seed_apply_gate_blocker(
        &covey,
        "queue-current-authority-hold",
        "blake3:current-authority-hold",
        "authority_hold",
        "authority_lost",
        "evidence_authority_lost_queue_current_authority_hold",
        3,
    );

    let current = covey
        .openspec_current_work("current-authority-hold")
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Blocked);
    assert_eq!(current.next_owner, OpenSpecCurrentWorkOwner::Authority);
    assert_eq!(current.blockers.len(), 1);
    assert_eq!(
        current.blockers[0].kind,
        OpenSpecCurrentWorkBlockerKind::AuthorityHold
    );
    assert_eq!(
        current.blockers[0].owner,
        OpenSpecCurrentWorkOwner::Authority
    );
    assert_eq!(
        current.blockers[0].queue_id.as_ref().map(|id| id.as_str()),
        Some("queue-current-authority-hold")
    );
    assert_eq!(
        current.blockers[0]
            .subtask_id
            .as_ref()
            .map(|id| id.as_str()),
        Some("current-authority-hold-work")
    );
    assert_eq!(
        current.blockers[0].evidence_id,
        "evidence_authority_lost_queue_current_authority_hold"
    );
    assert_eq!(current.blockers[0].reason, "authority_lost");
}

#[test]
fn openspec_current_work_reports_native_apply_gate_commit_unknown() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-commit-unknown");
    seed_current_work_scoped_subtask(
        &covey,
        "current-commit-unknown",
        "current-commit-unknown-work",
        "approved",
        Some("blake3:current-commit-unknown"),
        2,
    );
    seed_current_work_queue(
        &covey,
        "current-commit-unknown-work",
        "queue-current-commit-unknown",
        "blake3:current-commit-unknown",
        "in_flight",
        2,
    );
    seed_apply_gate_blocker(
        &covey,
        "queue-current-commit-unknown",
        "blake3:current-commit-unknown",
        "git_apply_uncertainty",
        "commit_unknown",
        "evidence_commit_unknown_queue_current_commit_unknown",
        3,
    );

    let current = covey
        .openspec_current_work("current-commit-unknown")
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Blocked);
    assert_eq!(current.next_owner, OpenSpecCurrentWorkOwner::ApplyGate);
    assert_eq!(current.blockers.len(), 1);
    assert_eq!(
        current.blockers[0].kind,
        OpenSpecCurrentWorkBlockerKind::GitApplyUncertainty
    );
    assert_eq!(
        current.blockers[0].owner,
        OpenSpecCurrentWorkOwner::ApplyGate
    );
    assert_eq!(
        current.blockers[0].queue_id.as_ref().map(|id| id.as_str()),
        Some("queue-current-commit-unknown")
    );
    assert_eq!(
        current.blockers[0]
            .subtask_id
            .as_ref()
            .map(|id| id.as_str()),
        Some("current-commit-unknown-work")
    );
    assert_eq!(
        current.blockers[0].evidence_id,
        "evidence_commit_unknown_queue_current_commit_unknown"
    );
    assert_eq!(current.blockers[0].reason, "commit_unknown");
}

#[test]
fn openspec_current_work_reports_native_settlement_reconcile_authority_lost() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-reconcile-authority-lost");
    seed_current_work_scoped_subtask(
        &covey,
        "current-reconcile-authority-lost",
        "current-reconcile-authority-lost-work",
        "approved",
        Some("blake3:current-reconcile-authority-lost"),
        2,
    );
    seed_current_work_queue(
        &covey,
        "current-reconcile-authority-lost-work",
        "queue-current-reconcile-authority-lost",
        "blake3:current-reconcile-authority-lost",
        "in_flight",
        2,
    );
    seed_settlement_reconcile_blocker(
        &covey,
        "queue-current-reconcile-authority-lost",
        "blake3:current-reconcile-authority-lost",
        "authority_lost",
        "evidence_reconcile_authority_lost_queue_current",
        3,
    );

    let current = covey
        .openspec_current_work("current-reconcile-authority-lost")
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Blocked);
    assert_eq!(current.next_owner, OpenSpecCurrentWorkOwner::Authority);
    assert_eq!(current.blockers.len(), 1);
    assert_eq!(
        current.blockers[0].kind,
        OpenSpecCurrentWorkBlockerKind::AuthorityHold
    );
    assert_eq!(
        current.blockers[0].owner,
        OpenSpecCurrentWorkOwner::Authority
    );
    assert_eq!(
        current.blockers[0].evidence_id,
        "evidence_reconcile_authority_lost_queue_current"
    );
    assert_eq!(
        current.blockers[0]
            .subtask_id
            .as_ref()
            .map(|id| id.as_str()),
        Some("current-reconcile-authority-lost-work")
    );
    assert_eq!(current.blockers[0].reason, "authority_lost");
}

#[test]
fn openspec_current_work_reports_native_settlement_reconcile_failed_apply() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-reconcile-failed-apply");
    seed_current_work_scoped_subtask(
        &covey,
        "current-reconcile-failed-apply",
        "current-reconcile-failed-apply-work",
        "approved",
        Some("blake3:current-reconcile-failed-apply"),
        2,
    );
    seed_current_work_queue(
        &covey,
        "current-reconcile-failed-apply-work",
        "queue-current-reconcile-failed-apply",
        "blake3:current-reconcile-failed-apply",
        "in_flight",
        2,
    );
    seed_settlement_reconcile_blocker(
        &covey,
        "queue-current-reconcile-failed-apply",
        "blake3:current-reconcile-failed-apply",
        "failed_canonical_apply",
        "evidence_reconcile_failed_apply_queue_current",
        3,
    );

    let current = covey
        .openspec_current_work("current-reconcile-failed-apply")
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Blocked);
    assert_eq!(current.next_owner, OpenSpecCurrentWorkOwner::ApplyGate);
    assert_eq!(current.blockers.len(), 1);
    assert_eq!(
        current.blockers[0].kind,
        OpenSpecCurrentWorkBlockerKind::GitApplyUncertainty
    );
    assert_eq!(
        current.blockers[0].owner,
        OpenSpecCurrentWorkOwner::ApplyGate
    );
    assert_eq!(
        current.blockers[0].evidence_id,
        "evidence_reconcile_failed_apply_queue_current"
    );
    assert_eq!(
        current.blockers[0]
            .subtask_id
            .as_ref()
            .map(|id| id.as_str()),
        Some("current-reconcile-failed-apply-work")
    );
    assert_eq!(current.blockers[0].reason, "failed_canonical_apply");
}

#[test]
fn openspec_current_work_subtask_blockers_precede_archive_until_all_scoped_subtasks_terminal() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "current-precedence");
    seed_archive_scoped_subtask(
        &covey,
        "current-precedence",
        "current-precedence-applied",
        Some("queue-current-precedence"),
        "blake3:current-precedence-applied",
        "applied",
        2,
    );
    seed_current_work_scoped_subtask(
        &covey,
        "current-precedence",
        "current-precedence-blocked",
        "changes_requested",
        Some("blake3:current-precedence-blocked"),
        3,
    );

    let current = covey
        .openspec_current_work("current-precedence")
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Blocked);
    assert_eq!(current.next_owner, OpenSpecCurrentWorkOwner::Executor);
    assert_eq!(current.blockers.len(), 1);
    assert_eq!(
        current.blockers[0].kind,
        OpenSpecCurrentWorkBlockerKind::SubtaskBlocked
    );
    assert_eq!(
        current.blockers[0].evidence_id,
        "openspec_current_work:subtask_blocked:current-precedence-blocked:changes_requested"
    );
    assert_eq!(
        current.blockers[0].allowed_repairs,
        vec![
            "mutai-scheduler orchestrator recover subtask",
            "mutai-scheduler orchestrator recover redispatch"
        ]
    );
    assert_eq!(current.archive_blockers.len(), 1);
}

#[test]
fn openspec_current_work_archived_ignores_repaired_source_subtasks() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    let change_id = "current-archived-repair";
    let source_subtask_id = "current-archived-repair-source";
    let followup_subtask_id = "current-archived-repair-followup";
    let queue_id = "queue-current-archived-repair";
    let artifact_digest = "blake3:current-archived-repair";

    seed_archive_session_and_meta(&covey, change_id);
    seed_current_work_scoped_subtask(
        &covey,
        change_id,
        source_subtask_id,
        "changes_requested",
        Some("blake3:current-archived-repair-source"),
        2,
    );
    seed_current_work_repair_followup(
        &covey,
        change_id,
        source_subtask_id,
        "blake3:current-archived-repair-source",
        followup_subtask_id,
        artifact_digest,
        3,
    );
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "UPDATE subtasks SET state = 'applied', updated_at = 4 WHERE subtask_id = ?1",
            params![followup_subtask_id],
        )
        .expect("mark repair followup applied");
    }
    seed_current_work_queue(
        &covey,
        followup_subtask_id,
        queue_id,
        artifact_digest,
        "applied",
        4,
    );
    seed_landing_receipt(&covey, queue_id, artifact_digest, 4);
    covey
        .record_openspec_archive_status(
            RecordOpenSpecArchiveStatusReq::try_from_raw_parts(
                "session-orch-archive",
                queue_id,
                artifact_digest,
                change_id,
                OpenSpecArchiveStatusState::Archived,
                None,
                Some("blake3:archive-current-archived-repair".to_owned()),
                "record-archived-repair-followup",
            )
            .expect("valid archived status"),
        )
        .expect("record archived repair followup status");

    let current = covey
        .openspec_current_work(change_id)
        .expect("current work");

    assert_eq!(current.state, OpenSpecCurrentWorkState::Archived);
    assert_eq!(current.next_owner, OpenSpecCurrentWorkOwner::Operator);
    assert!(current.blockers.is_empty());
}

#[test]
fn openspec_archive_eligibility_blocks_until_all_scoped_subtasks_are_terminal() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "change-pending");
    seed_archive_scoped_subtask(
        &covey,
        "change-pending",
        "work-applied",
        Some("queue-applied"),
        "blake3:archive-applied",
        "applied",
        2,
    );
    seed_archive_scoped_subtask(
        &covey,
        "change-pending",
        "work-pending",
        None,
        "blake3:archive-pending",
        "available",
        3,
    );

    let eligibility = covey
        .openspec_archive_eligibility("change-pending")
        .expect("archive eligibility");

    assert!(!eligibility.safe_to_archive);
    assert_eq!(eligibility.scoped_subtasks.len(), 2);
    assert_eq!(eligibility.pending_subtasks.len(), 1);
    assert_eq!(
        eligibility.pending_subtasks[0].subtask_id.as_str(),
        "work-pending"
    );
    assert_eq!(eligibility.open_archive_blockers.len(), 1);
}

#[test]
fn openspec_archive_eligibility_blocks_abandoned_scoped_subtasks() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "change-abandoned");
    seed_archive_scoped_subtask(
        &covey,
        "change-abandoned",
        "work-applied-before-abandoned",
        Some("queue-applied-before-abandoned"),
        "blake3:archive-applied-before-abandoned",
        "applied",
        2,
    );
    seed_archive_scoped_subtask(
        &covey,
        "change-abandoned",
        "work-abandoned",
        None,
        "blake3:archive-abandoned",
        "abandoned",
        3,
    );

    let eligibility = covey
        .openspec_archive_eligibility("change-abandoned")
        .expect("archive eligibility");

    assert!(!eligibility.safe_to_archive);
    assert_eq!(eligibility.pending_subtasks.len(), 1);
    assert_eq!(
        eligibility.pending_subtasks[0].subtask_id.as_str(),
        "work-abandoned"
    );
    assert_eq!(eligibility.open_archive_blockers.len(), 1);
}

#[test]
fn openspec_archive_eligibility_includes_applied_repair_followups() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    let change_id = "change-repair-archive";
    let source_subtask_id = "work-repair-archive-source";
    let followup_subtask_id = "work-repair-archive-followup";
    let queue_id = "queue-repair-archive-followup";
    let artifact_digest = "blake3:repair-archive-followup";

    seed_archive_session_and_meta(&covey, change_id);
    seed_current_work_scoped_subtask(
        &covey,
        change_id,
        source_subtask_id,
        "changes_requested",
        Some("blake3:repair-archive-source"),
        2,
    );
    seed_current_work_repair_followup(
        &covey,
        change_id,
        source_subtask_id,
        "blake3:repair-archive-source",
        followup_subtask_id,
        artifact_digest,
        3,
    );
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "UPDATE subtasks SET state = 'applied', updated_at = 4 WHERE subtask_id = ?1",
            params![followup_subtask_id],
        )
        .expect("mark repair followup applied");
    }
    seed_current_work_queue(
        &covey,
        followup_subtask_id,
        queue_id,
        artifact_digest,
        "applied",
        4,
    );
    covey
        .record_openspec_archive_status(
            RecordOpenSpecArchiveStatusReq::try_from_raw_parts(
                "session-orch-archive",
                queue_id,
                artifact_digest,
                change_id,
                OpenSpecArchiveStatusState::Blocked,
                Some("applied_but_unarchived".to_owned()),
                None,
                "record-repair-followup-archive-blocker",
            )
            .expect("valid archive blocker"),
        )
        .expect("record repair followup archive blocker");

    let eligibility = covey
        .openspec_archive_eligibility(change_id)
        .expect("archive eligibility");

    assert!(eligibility.safe_to_archive);
    assert!(
        eligibility
            .scoped_subtasks
            .iter()
            .any(|subtask| subtask.subtask_id.as_str() == followup_subtask_id)
    );
    assert_eq!(eligibility.pending_subtasks.len(), 0);
    assert_eq!(eligibility.open_archive_blockers.len(), 1);
    assert_eq!(
        eligibility.open_archive_blockers[0].queue_id.as_str(),
        queue_id
    );
}

#[test]
fn openspec_archive_cleanup_claim_is_orchestrator_only_idempotent_and_non_dispatchable() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "change-cleanup");
    seed_archive_scoped_subtask(
        &covey,
        "change-cleanup",
        "work-cleanup",
        Some("queue-cleanup"),
        "blake3:archive-cleanup",
        "applied",
        2,
    );
    let paths = vec![
        "openspec/changes/change-cleanup".to_owned(),
        "openspec/archive".to_owned(),
        "openspec/specs".to_owned(),
    ];

    let executor_err = covey
        .begin_openspec_archive_cleanup(
            BeginOpenSpecArchiveCleanupReq::try_from_raw_parts(
                "session-exec-archive",
                "change-cleanup",
                paths.clone(),
                "begin-cleanup-executor",
            )
            .expect("valid executor cleanup request"),
        )
        .expect_err("executor must not begin cleanup");
    assert!(
        executor_err.to_string().contains("wrong role"),
        "unexpected error: {executor_err}"
    );

    let first = covey
        .begin_openspec_archive_cleanup(
            BeginOpenSpecArchiveCleanupReq::try_from_raw_parts(
                "session-orch-archive",
                "change-cleanup",
                paths.clone(),
                "begin-cleanup-once",
            )
            .expect("valid cleanup begin"),
        )
        .expect("begin cleanup");
    let second = covey
        .begin_openspec_archive_cleanup(
            BeginOpenSpecArchiveCleanupReq::try_from_raw_parts(
                "session-orch-archive",
                "change-cleanup",
                paths,
                "begin-cleanup-twice",
            )
            .expect("valid cleanup begin"),
        )
        .expect("reuse cleanup");

    assert_eq!(first.cleanup_subtask_id, second.cleanup_subtask_id);
    assert_eq!(first.cleanup_claim_id, second.cleanup_claim_id);
    assert_eq!(first.open_archive_blockers.len(), 1);
    let active_reservations: i64 = {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.query_row(
            "SELECT COUNT(*) FROM reservations WHERE owner_subtask_id = ?1 AND state = 'active'",
            params![first.cleanup_subtask_id.as_str()],
            |row| row.get(0),
        )
        .expect("count cleanup reservations")
    };
    assert_eq!(active_reservations, 3);
    assert!(
        covey
            .subtask_candidates(SessionRole::Executor, 10, None)
            .expect("executor candidates")
            .iter()
            .all(|candidate| candidate.subtask_id != first.cleanup_subtask_id)
    );
    assert!(
        covey
            .subtask_candidates(SessionRole::Reviewer, 10, None)
            .expect("reviewer candidates")
            .iter()
            .all(|candidate| candidate.subtask_id != first.cleanup_subtask_id)
    );
}

#[test]
fn openspec_archive_cleanup_can_retry_after_released_failed_claim() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "change-cleanup-retry");
    seed_archive_scoped_subtask(
        &covey,
        "change-cleanup-retry",
        "work-cleanup-retry",
        Some("queue-cleanup-retry"),
        "blake3:archive-cleanup-retry",
        "applied",
        2,
    );
    let paths = vec![
        "openspec/changes/change-cleanup-retry".to_owned(),
        "openspec/archive".to_owned(),
        "openspec/specs".to_owned(),
    ];
    let first = covey
        .begin_openspec_archive_cleanup(
            BeginOpenSpecArchiveCleanupReq::try_from_raw_parts(
                "session-orch-archive",
                "change-cleanup-retry",
                paths.clone(),
                "begin-cleanup-retry-first",
            )
            .expect("valid cleanup begin"),
        )
        .expect("begin first cleanup");

    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                "session-orch-archive",
                first.cleanup_claim_id.to_string(),
                first.fence_seq.get(),
                "release-cleanup-retry-first",
            )
            .expect("valid release"),
        )
        .expect("release failed cleanup claim");

    let second = covey
        .begin_openspec_archive_cleanup(
            BeginOpenSpecArchiveCleanupReq::try_from_raw_parts(
                "session-orch-archive",
                "change-cleanup-retry",
                paths,
                "begin-cleanup-retry-second",
            )
            .expect("valid cleanup begin"),
        )
        .expect("begin retry cleanup");

    assert_eq!(first.cleanup_subtask_id, second.cleanup_subtask_id);
    assert_eq!(first.cleanup_claim_id, second.cleanup_claim_id);
    assert_ne!(first.fence_seq, second.fence_seq);
    assert_eq!(second.open_archive_blockers.len(), 1);
}

#[test]
fn openspec_archive_cleanup_can_retry_after_expired_cleanup_claim() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock.clone()).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "change-cleanup-expired");
    seed_archive_scoped_subtask(
        &covey,
        "change-cleanup-expired",
        "work-cleanup-expired",
        Some("queue-cleanup-expired"),
        "blake3:archive-cleanup-expired",
        "applied",
        2,
    );
    let paths = vec![
        "openspec/changes/change-cleanup-expired".to_owned(),
        "openspec/archive".to_owned(),
        "openspec/specs".to_owned(),
    ];
    let first = covey
        .begin_openspec_archive_cleanup(
            BeginOpenSpecArchiveCleanupReq::try_from_raw_parts(
                "session-orch-archive",
                "change-cleanup-expired",
                paths.clone(),
                "begin-cleanup-expired-first",
            )
            .expect("valid cleanup begin"),
        )
        .expect("begin first cleanup");

    clock.advance(700_000);

    let second = covey
        .begin_openspec_archive_cleanup(
            BeginOpenSpecArchiveCleanupReq::try_from_raw_parts(
                "session-orch-archive",
                "change-cleanup-expired",
                paths,
                "begin-cleanup-expired-second",
            )
            .expect("valid cleanup begin"),
        )
        .expect("begin retry cleanup after expiry");

    assert_eq!(first.cleanup_subtask_id, second.cleanup_subtask_id);
    assert_eq!(first.cleanup_claim_id, second.cleanup_claim_id);
    assert_ne!(first.fence_seq, second.fence_seq);
    assert_eq!(second.open_archive_blockers.len(), 1);
}

#[test]
fn openspec_archive_cleanup_can_start_after_meta_task_completed() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "change-cleanup-completed");
    seed_archive_scoped_subtask(
        &covey,
        "change-cleanup-completed",
        "work-cleanup-completed",
        Some("queue-cleanup-completed"),
        "blake3:archive-cleanup-completed",
        "applied",
        2,
    );
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "UPDATE meta_tasks SET state = 'completed' WHERE meta_task_id = 'openspec:change-cleanup-completed'",
            [],
        )
        .expect("complete OpenSpec meta task");
    }

    let cleanup = covey
        .begin_openspec_archive_cleanup(
            BeginOpenSpecArchiveCleanupReq::try_from_raw_parts(
                "session-orch-archive",
                "change-cleanup-completed",
                vec![
                    "openspec/changes/change-cleanup-completed".to_owned(),
                    "openspec/archive".to_owned(),
                    "openspec/specs".to_owned(),
                ],
                "begin-cleanup-completed",
            )
            .expect("valid cleanup begin"),
        )
        .expect("begin cleanup for completed OpenSpec meta task");

    assert_eq!(
        cleanup.cleanup_subtask_id.as_str(),
        "openspec:change-cleanup-completed:cleanup:archive"
    );
    assert_eq!(cleanup.open_archive_blockers.len(), 1);
}

#[test]
fn finish_openspec_archive_cleanup_resolves_all_open_blockers_for_one_change() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    seed_archive_session_and_meta(&covey, "change-finish");
    seed_archive_scoped_subtask(
        &covey,
        "change-finish",
        "work-finish-a",
        Some("queue-finish-a"),
        "blake3:archive-finish-a",
        "applied",
        2,
    );
    seed_archive_scoped_subtask(
        &covey,
        "change-finish",
        "work-finish-b",
        Some("queue-finish-b"),
        "blake3:archive-finish-b",
        "applied",
        3,
    );
    let cleanup = covey
        .begin_openspec_archive_cleanup(
            BeginOpenSpecArchiveCleanupReq::try_from_raw_parts(
                "session-orch-archive",
                "change-finish",
                vec![
                    "openspec/changes/change-finish".to_owned(),
                    "openspec/archive".to_owned(),
                    "openspec/specs".to_owned(),
                ],
                "begin-finish-cleanup",
            )
            .expect("valid cleanup begin"),
        )
        .expect("begin cleanup");

    let finish = covey
        .finish_openspec_archive_cleanup(
            FinishOpenSpecArchiveCleanupReq::try_from_raw_parts(
                "session-orch-archive",
                "change-finish",
                cleanup.cleanup_claim_id.to_string(),
                cleanup.fence_seq.get(),
                "blake3:archive-proof-finish",
                "finish-cleanup-once",
            )
            .expect("valid cleanup finish"),
        )
        .expect("finish cleanup");
    let finish_again = covey
        .finish_openspec_archive_cleanup(
            FinishOpenSpecArchiveCleanupReq::try_from_raw_parts(
                "session-orch-archive",
                "change-finish",
                cleanup.cleanup_claim_id.to_string(),
                cleanup.fence_seq.get(),
                "blake3:archive-proof-finish",
                "finish-cleanup-once",
            )
            .expect("valid cleanup finish"),
        )
        .expect("finish cleanup idempotently");

    assert_eq!(finish.archived_queue_ids.len(), 2);
    assert_eq!(finish.archived_queue_ids, finish_again.archived_queue_ids);
    assert!(
        covey
            .open_openspec_archive_blockers(10)
            .expect("open blockers")
            .is_empty()
    );
    let archived_rows: i64 = {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.query_row(
            "SELECT COUNT(*) FROM openspec_archive_status WHERE openspec_change_id = 'change-finish' AND state = 'archived' AND archive_proof_digest = 'blake3:archive-proof-finish'",
            [],
            |row| row.get(0),
        )
        .expect("count archived rows")
    };
    assert_eq!(archived_rows, 2);
    let active_reservations: i64 = {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.query_row(
            "SELECT COUNT(*) FROM reservations WHERE owner_subtask_id = ?1 AND state = 'active'",
            params![finish.cleanup_subtask_id.as_str()],
            |row| row.get(0),
        )
        .expect("count active cleanup reservations")
    };
    assert_eq!(active_reservations, 0);
}

#[test]
fn changes_requested_reconciliation_creates_claimable_repair_followup() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    let artifact_digest = "blake3:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    let findings_digest = "blake3:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "INSERT INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
             VALUES ('session-orch', 'orch-reconcile-followup', 'orch-reconcile-followup-1', 'orchestrator', 'active', NULL, 1, 1, 1, 1)",
            [],
        )
        .expect("insert orchestrator session");
        conn.execute(
            "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at, updated_at)
             VALUES ('meta-followup', 'followup', 'active', 'session-orch', 1, 1)",
            [],
        )
        .expect("insert meta task");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
             VALUES ('source-work', 'meta-followup', 'source work', 'work', NULL, NULL, 'available', NULL, NULL, 5, 2, 2)",
            [],
        )
        .expect("insert source work");
        conn.execute(
            "INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq) VALUES ('source-work', 1)",
            [],
        )
        .expect("insert source fence counter");
        conn.execute(
            "INSERT INTO artifacts (artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session, manifest_path, changed_paths_digest, created_at)
             VALUES (?1, 'patch_bundle', 'HEAD', 'source-work', 'session-orch', 'source-work.json', ?1, 3)",
            params![artifact_digest],
        )
        .expect("insert artifact");
        conn.execute(
            "UPDATE subtasks SET state = 'changes_requested', artifact_digest = ?1, updated_at = 3 WHERE subtask_id = 'source-work'",
            params![artifact_digest],
        )
        .expect("mark source work changes requested");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
             VALUES ('review-source-work', 'meta-followup', 'review source work', 'review', 'source-work', ?1, 'decided', NULL, NULL, 1, 3, 3)",
            params![artifact_digest],
        )
        .expect("insert decided review subtask");
        conn.execute(
            "INSERT INTO reviews (review_id, subtask_id, artifact_digest, review_subtask_id, reviewer_session, state, verdict, findings_digest, created_at, updated_at)
             VALUES ('review-source-work-id', 'source-work', ?1, 'review-source-work', 'session-orch', 'decided', 'changes_requested', ?2, 3, 3)",
            params![artifact_digest, findings_digest],
        )
        .expect("insert changes-requested review");
    }

    let before = covey
        .claimable_subtask_availability(Some("meta-followup"))
        .expect("availability before reconciliation");
    assert_eq!(before.executor_claimable_count(), 0);

    let result = covey
        .reconcile_changes_requested_followups(
            ReconcileChangesRequestedFollowupsReq::try_from_raw_parts(
                "session-orch",
                "reconcile-followups",
            )
            .expect("valid reconcile followups request"),
        )
        .expect("reconcile followups");
    assert_eq!(result.created_count, 1);

    let after = covey
        .claimable_subtask_availability(Some("meta-followup"))
        .expect("availability after reconciliation");
    assert_eq!(after.executor_claimable_count(), 1);
    let candidates = covey
        .subtask_candidates(SessionRole::Executor, 10, Some("meta-followup"))
        .expect("executor candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0].subtask_id, result.followup_subtask_ids[0],
        "repair candidate should be the generated follow-up"
    );
    assert!(candidates[0].is_repair_followup);

    let replay = covey
        .reconcile_changes_requested_followups(
            ReconcileChangesRequestedFollowupsReq::try_from_raw_parts(
                "session-orch",
                "reconcile-followups-again",
            )
            .expect("valid second reconcile followups request"),
        )
        .expect("second reconcile followups");
    assert_eq!(replay.created_count, 0);
}

#[test]
fn failed_review_reconciliation_creates_claimable_blocked_repair_followup() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    let artifact_digest = "blake3:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    let findings_digest = "blake3:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "INSERT INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
             VALUES ('session-orch', 'orch-reconcile-blocked-followup', 'orch-reconcile-blocked-followup-1', 'orchestrator', 'active', NULL, 1, 1, 1, 1)",
            [],
        )
        .expect("insert orchestrator session");
        conn.execute(
            "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at, updated_at)
             VALUES ('meta-blocked-followup', 'blocked followup', 'active', 'session-orch', 1, 1)",
            [],
        )
        .expect("insert meta task");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
             VALUES ('blocked-work', 'meta-blocked-followup', 'blocked work', 'work', NULL, NULL, 'available', NULL, NULL, 5, 2, 2)",
            [],
        )
        .expect("insert blocked work");
        conn.execute(
            "INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq) VALUES ('blocked-work', 1)",
            [],
        )
        .expect("insert blocked work fence counter");
        conn.execute(
            "INSERT INTO artifacts (artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session, manifest_path, changed_paths_digest, created_at)
             VALUES (?1, 'patch_bundle', 'HEAD', 'blocked-work', 'session-orch', 'blocked-work.json', ?1, 3)",
            params![artifact_digest],
        )
        .expect("insert artifact");
        conn.execute(
            "UPDATE subtasks SET state = 'blocked', artifact_digest = ?1, updated_at = 3 WHERE subtask_id = 'blocked-work'",
            params![artifact_digest],
        )
        .expect("mark blocked work blocked");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
             VALUES ('review-blocked-work', 'meta-blocked-followup', 'review blocked work', 'review', 'blocked-work', ?1, 'decided', NULL, NULL, 1, 3, 3)",
            params![artifact_digest],
        )
        .expect("insert decided review subtask");
        conn.execute(
            "INSERT INTO reviews (review_id, subtask_id, artifact_digest, review_subtask_id, reviewer_session, state, verdict, findings_digest, created_at, updated_at)
             VALUES ('review-blocked-work-id', 'blocked-work', ?1, 'review-blocked-work', 'session-orch', 'decided', 'blocked', ?2, 3, 3)",
            params![artifact_digest, findings_digest],
        )
        .expect("insert blocked review");
    }

    let result = covey
        .reconcile_changes_requested_followups(
            ReconcileChangesRequestedFollowupsReq::try_from_raw_parts(
                "session-orch",
                "reconcile-blocked-followups",
            )
            .expect("valid reconcile followups request"),
        )
        .expect("reconcile blocked followups");
    assert_eq!(result.created_count, 1);

    let candidates = covey
        .subtask_candidates(SessionRole::Executor, 10, Some("meta-blocked-followup"))
        .expect("executor candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].subtask_id, result.followup_subtask_ids[0]);
    assert!(candidates[0].is_repair_followup);
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
fn stuck_subtasks_exclude_failed_work_superseded_by_followup_chain() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    let source_digest = "blake3:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let repair_digest = "blake3:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let leaf_digest = "blake3:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "INSERT INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
             VALUES ('session-orch', 'orch-stuck', 'orch-stuck-1', 'orchestrator', 'active', NULL, 1, 1, 1, 1)",
            [],
        )
        .expect("insert orchestrator session");
        conn.execute(
            "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at, updated_at)
             VALUES ('meta-stuck-chain', 'stuck chain', 'active', 'session-orch', 1, 1)",
            [],
        )
        .expect("insert meta task");
        for (subtask_id, state, artifact_digest, created_at) in [
            ("source-work", "changes_requested", source_digest, 2),
            ("applied-repair", "applied", repair_digest, 3),
            ("unresolved-leaf", "changes_requested", leaf_digest, 4),
        ] {
            conn.execute(
                "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
                 VALUES (?1, 'meta-stuck-chain', ?1, 'work', NULL, NULL, 'available', NULL, NULL, 1, ?2, ?2)",
                params![subtask_id, created_at],
            )
            .expect("insert work subtask");
            conn.execute(
                "INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq) VALUES (?1, 1)",
                params![subtask_id],
            )
            .expect("insert fence counter");
            conn.execute(
                "INSERT INTO artifacts (artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session, manifest_path, changed_paths_digest, created_at)
                 VALUES (?1, 'patch_bundle', 'base', ?2, 'session-orch', ?2 || '.json', ?1, ?3)",
                params![artifact_digest, subtask_id, created_at],
            )
            .expect("insert artifact");
            conn.execute(
                "UPDATE subtasks SET state = ?2, artifact_digest = ?3 WHERE subtask_id = ?1",
                params![subtask_id, state, artifact_digest],
            )
            .expect("publish work artifact state");
        }
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, created_at, updated_at)
             VALUES ('review-source', 'meta-stuck-chain', 'review source', 'review', 'source-work', ?1, 'decided', NULL, NULL, 1, 3, 3)",
            params![source_digest],
        )
        .expect("insert review subtask");
        conn.execute(
            "INSERT INTO reviews (review_id, subtask_id, artifact_digest, review_subtask_id, reviewer_session, state, verdict, findings_digest, created_at, updated_at)
             VALUES ('review-source-id', 'source-work', ?1, 'review-source', 'session-orch', 'decided', 'changes_requested', ?1, 3, 3)",
            params![source_digest],
        )
        .expect("insert review");
        conn.execute(
            "INSERT INTO review_followup_subtasks (review_id, source_subtask_id, source_artifact_digest, findings_digest, followup_subtask_id, created_by_session, created_at)
             VALUES ('review-source-id', 'source-work', ?1, ?1, 'applied-repair', 'session-orch', 3)",
            params![source_digest],
        )
        .expect("insert follow-up edge");
    }

    let stuck = covey
        .list_stuck_subtasks(0, 10)
        .expect("list stuck subtasks");
    let stuck_ids = stuck
        .iter()
        .map(|row| row.subtask().subtask_id.as_str())
        .collect::<Vec<_>>();
    assert!(!stuck_ids.contains(&"source-work"));
    assert!(stuck_ids.contains(&"unresolved-leaf"));
}

#[test]
fn observability_queries_skip_invalid_claim_session_attachments() {
    let now = 1_700_000_000_000;
    let clock = Arc::new(ManualClock::new(now));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    {
        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "INSERT INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
             VALUES ('session-orch', 'orch-observe', 'orch-observe-1', 'orchestrator', 'active', NULL, ?1, ?1, ?1, ?1)",
            params![now],
        )
        .expect("insert orchestrator session");
        conn.execute(
            "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at, updated_at)
             VALUES ('meta-observe', 'observe invalid rows', 'active', 'session-orch', ?1, ?1)",
            params![now],
        )
        .expect("insert meta task");
        conn.execute(
            "INSERT INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
             VALUES ('session-bad', 'worker-bad', 'worker-bad-1', 'executor', 'active', NULL, ?1, ?1, ?1, ?1)",
            params![now],
        )
        .expect("insert invalid worker session");
        conn.execute(
            "INSERT INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
             VALUES ('session-good', 'worker-good', 'worker-good-1', 'executor', 'active', NULL, ?1, ?1, ?1, ?1)",
            params![now],
        )
        .expect("insert valid worker session");
        for (subtask_id, claim_id, session_token) in [
            ("work-bad", "claim-bad", "session-bad"),
            ("work-good", "claim-good", "session-good"),
        ] {
            conn.execute(
                "INSERT INTO subtasks (
                    subtask_id, meta_task_id, title, kind, review_target_subtask_id,
                    review_target_artifact_digest, state, current_claim_id, artifact_digest,
                    priority, created_at, updated_at
                ) VALUES (?1, 'meta-observe', ?1, 'work', NULL, NULL, 'in_progress', NULL, NULL, 1, ?2, ?2)",
                params![subtask_id, now - 10_000],
            )
            .expect("insert subtask");
            conn.execute(
                "INSERT INTO claims (
                    claim_id, subtask_id, owner_session_token, fence_seq, lease_deadline,
                    state, created_at, updated_at
                ) VALUES (?1, ?2, ?3, 1, ?4, 'held', ?5, ?5)",
                params![
                    claim_id,
                    subtask_id,
                    session_token,
                    now + 5_000,
                    now - 10_000
                ],
            )
            .expect("insert claim");
            conn.execute(
                "UPDATE subtasks SET current_claim_id = ?2 WHERE subtask_id = ?1",
                params![subtask_id, claim_id],
            )
            .expect("attach current claim");
        }
        conn.execute(
            "UPDATE sessions SET active_subtask_id = 'work-good' WHERE session_token = 'session-good'",
            [],
        )
        .expect("attach valid worker session to subtask");
    }

    let stuck = covey
        .list_stuck_subtasks(1_000, 10)
        .expect("stuck query skips invalid attachment");
    assert_eq!(stuck.len(), 1);
    assert_eq!(stuck[0].subtask().subtask_id, "work-good");

    let expiring = covey
        .list_expiring_claims(30_000, 10)
        .expect("expiring query skips invalid attachment");
    assert_eq!(expiring.len(), 1);
    assert_eq!(expiring[0].claim().claim_id, "claim-good");
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

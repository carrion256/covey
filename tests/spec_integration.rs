use std::{
    collections::HashSet,
    env,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering},
    },
    thread,
};

mod support;

use covey::{
    AbandonSubtaskReq, ActorKind, ArtifactKind, CancelMetaTaskReq, ClaimNextReq,
    ClaimReadyQueueReq, ClaimResult, ClaimSubtaskReq, Covey, CoveyError, CreateSubtaskRequest,
    DecideReviewReq, EnqueueForApplyReq, EventPayload, EventType, ExitSessionReq, HeartbeatReq,
    ImportBdV1ItemResult, ImportBdV1Req, ImportBdV1Result, ImportBdV1SkipReason, ManualClock,
    MarkAppliedReq, MarkInFlightReq, MetaTaskState, ObjectType, OverlapQueryReq,
    PublishArtifactReq, ReadyQueueState, RecordApplyVerificationReq, RecordRuntimeAttestationReq,
    RegisterSessionReq, ReleaseClaimReq, ReleaseReservationReq, RenewClaimReq, RenewReservationReq,
    RequestReservationReq, RequestReviewReq, ReservationOverlapConflictPayload, ResolveConflictReq,
    ScopeClass, SessionRole, SessionState, SessionToken, SettlementTarget, StartSubtaskReq,
    StateValue, SubmitMetaTaskReq, SubtaskId, SubtaskKind, SubtaskState,
};
use rusqlite::ffi::{SQLITE_TESTCTRL_FAULT_INSTALL, sqlite3_test_control};
use rusqlite::{Connection, TransactionBehavior, params};
use tempfile::TempDir;

const CRASH_HELPER_ENV: &str = "COVEY_CRASH_HELPER";
const CRASH_DB_PATH_ENV: &str = "COVEY_CRASH_DB_PATH";
const SQLITE_FAULT_HELPER_ENV: &str = "COVEY_SQLITE_FAULT_HELPER";

const SQLITE_SYNC_FAULT_CODE: i32 = 400;

static SQLITE_FAULT_TARGET_CODE: AtomicI32 = AtomicI32::new(-1);
static SQLITE_FAULT_TRIGGERED: AtomicBool = AtomicBool::new(false);
static SQLITE_FAULT_SEEN_TARGET: AtomicBool = AtomicBool::new(false);
static NEXT_IDEMPOTENCY_KEY: AtomicUsize = AtomicUsize::new(1);

fn session_token(value: &str) -> SessionToken {
    SessionToken::parse(value).expect("test session token must be valid")
}

fn subtask_id(value: &str) -> SubtaskId {
    SubtaskId::parse(value).expect("test subtask id must be valid")
}

struct Rig {
    _dir: TempDir,
    db_path: PathBuf,
    clock: Arc<ManualClock>,
}

impl Rig {
    fn new() -> Self {
        support::enable_info_logging();
        let dir = TempDir::new().expect("tempdir");
        let db_path = dir.path().join("covey.db");
        Self {
            _dir: dir,
            db_path,
            clock: Arc::new(ManualClock::new(1_700_000_000_000)),
        }
    }

    fn covey(&self) -> Covey {
        Covey::open_with_clock(&self.db_path, self.clock.clone()).expect("open covey")
    }

    fn tick(&self, delta_ms: i64) {
        self.clock.advance(delta_ms);
    }
}

fn register(covey: &Covey, token: &str, principal: &str, role: SessionRole) -> String {
    let _ = token;
    covey
        .register_session(RegisterSessionReq {
            agent_principal_id: principal.into(),
            agent_instance_id: format!("{principal}-instance"),
            role,
            idempotency_key: id_key("register-session"),
        })
        .expect("register session")
        .session_token
}

fn attest(covey: &Covey, session_token: &str) {
    covey
        .record_runtime_attestation(RecordRuntimeAttestationReq {
            session_token: session_token.to_owned(),
            provider: "covey-test".into(),
            model: "test-model".into(),
            provider_run_id: format!("provider-run-{session_token}"),
            provider_run_id_issuer: "covey-test-provider".into(),
            process_id: Some(format!("pid-{session_token}")),
            container_id: None,
            command_transcript_digest: format!("blake3:{session_token}-transcript"),
            started_at: 1_700_000_000_000,
            ended_at: 1_700_000_000_001,
            idempotency_key: format!("record-runtime-attestation-{session_token}"),
        })
        .expect("record runtime attestation");
}

fn seed_work_subtask(rig: &Rig) -> (String, String) {
    let covey = rig.covey();
    let orch = register(&covey, "orch", "orch", SessionRole::Orchestrator);
    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "operator quest".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("submit meta task");
    rig.tick(1);
    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch,
            meta_task_id: meta_task_id.clone(),
            subtask_id: Some("work_1".into()),
            title: "implement covey".into(),
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create subtask");
    (meta_task_id, subtask_id)
}

fn subtask_claim_events(covey: &Covey) -> Vec<ClaimResult> {
    covey
        .fetch_events(0, 1_000)
        .expect("events")
        .into_iter()
        .filter_map(|event| event.typed().ok())
        .filter_map(|event| match event.payload {
            EventPayload::SubtaskClaimed(payload) => Some(payload),
            _ => None,
        })
        .collect()
}

fn count_subtask_claim_events(covey: &Covey) -> usize {
    subtask_claim_events(covey).len()
}

fn record_apply_verification(
    covey: &Covey,
    gate: &str,
    queue_id: &str,
    artifact_digest: &str,
    review_id: &str,
    findings_digest: &str,
    claim_fence_seq: i64,
) {
    attest(covey, gate);
    covey
        .record_apply_verification(RecordApplyVerificationReq {
            session_token: gate.to_owned(),
            queue_id: queue_id.to_owned(),
            artifact_digest: artifact_digest.to_owned(),
            review_id: review_id.to_owned(),
            findings_digest: findings_digest.to_owned(),
            claim_fence_seq,
            verifier: "mutai-rs".to_owned(),
            verdict_digest: format!("{artifact_digest}:verdict"),
            seal_digest: format!("{artifact_digest}:seal"),
            idempotency_key: id_key("record-apply-verification"),
        })
        .expect("record apply verification");
}

fn seed_changes_requested_work_subtask(rig: &Rig) -> String {
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(rig);
    let worker = register(
        &covey,
        "worker-changes-requested-source",
        "worker-changes-requested-source",
        SessionRole::Executor,
    );
    let reviewer = register(
        &covey,
        "reviewer-changes-requested-source",
        "reviewer-changes-requested-source",
        SessionRole::Reviewer,
    );

    let claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim work subtask")
        .expect("claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start work subtask");
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:changes_requested_seed".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "changes_requested_seed.json".into(),
            changed_paths_digest: "blake3:changes_requested_seed_paths".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish artifact");
    let review_id = covey
        .request_review(RequestReviewReq {
            session_token: worker.clone(),
            subtask_id: subtask_id.clone(),
            artifact_digest: "blake3:changes_requested_seed".into(),
            review_subtask_id: Some("review_changes_requested_seed".into()),
            priority: 1,
            idempotency_key: id_key("request-review"),
        })
        .expect("request review");
    covey
        .release_claim(ReleaseClaimReq {
            session_token: worker,
            claim_id: claim.claim_id,
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("release-claim"),
        })
        .expect("release work claim");

    let review_subtask_id = covey
        .subtask_status(&subtask_id)
        .expect("work status")
        .reviews
        .into_iter()
        .find(|review| review.review_id() == review_id)
        .map(|review| review.review_subtask_id().to_owned())
        .expect("review subtask id");
    let review_claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: reviewer.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim review subtask")
        .expect("review claim result");
    assert_eq!(review_claim.subtask_id, review_subtask_id);
    covey
        .start_subtask(StartSubtaskReq {
            session_token: reviewer.clone(),
            claim_id: review_claim.claim_id.clone(),
            fence_seq: review_claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start review subtask");
    covey
        .decide_review(DecideReviewReq {
            session_token: reviewer,
            review_id: review_id.clone(),
            claim_id: review_claim.claim_id,
            fence_seq: review_claim.fence_seq,
            verdict: covey::ReviewVerdict::ChangesRequested,
            findings_digest: "blake3:changes_requested_findings".into(),
            idempotency_key: id_key("decide-review"),
        })
        .expect("request changes");

    assert_eq!(
        covey
            .subtask_status(&subtask_id)
            .expect("updated status")
            .subtask
            .state,
        SubtaskState::ChangesRequested
    );
    subtask_id
}

fn seed_bd_import_source(db_path: &Path) {
    let conn = Connection::open(db_path).expect("open beads db");
    conn.execute(
        "CREATE TABLE issues (id TEXT PRIMARY KEY, title TEXT NOT NULL, description TEXT, acceptance_criteria TEXT, status TEXT NOT NULL, priority INTEGER NOT NULL, issue_type TEXT NOT NULL, owner TEXT, assignee TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, closed_at TEXT, close_reason TEXT, deleted_at TEXT)",
        [],
    )
    .expect("create issues table");
    conn.execute(
        "CREATE TABLE dependencies (issue_id TEXT NOT NULL, depends_on_id TEXT NOT NULL, type TEXT NOT NULL, created_at TEXT NOT NULL)",
        [],
    )
    .expect("create dependencies table");
    conn.execute(
        "CREATE TABLE labels (issue_id TEXT NOT NULL, label TEXT NOT NULL)",
        [],
    )
    .expect("create labels table");

    let rows = [
        ("BD-1", "first imported work item", "open", 1_i64, "task"),
        ("BD-2", "second imported work item", "open", 2_i64, "task"),
        ("BD-3", "closed work item", "closed", 3_i64, "task"),
        ("BD-4", "epic container", "open", 4_i64, "epic"),
        ("BD-5", "feature work item", "open", 5_i64, "feature"),
        ("BD-6", "review labeled feature", "open", 6_i64, "feature"),
        ("BD-7", "unsupported bug", "open", 7_i64, "bug"),
    ];
    for (id, title, status, priority, issue_type) in rows {
        conn.execute(
            "INSERT INTO issues (id, title, description, acceptance_criteria, status, priority, issue_type, owner, assignee, created_at, updated_at, closed_at, close_reason, deleted_at) VALUES (?1, ?2, '', '', ?3, ?4, ?5, '', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL, '', NULL)",
            params![id, title, status, priority, issue_type],
        )
        .expect("insert issue");
    }

    conn.execute(
        "INSERT INTO dependencies (issue_id, depends_on_id, type, created_at) VALUES (?1, ?2, 'blocks', '2026-01-01T00:00:00Z')",
        params!["BD-2", "BD-1"],
    )
    .expect("insert dependency");
    conn.execute(
        "INSERT INTO labels (issue_id, label) VALUES (?1, ?2)",
        params!["BD-6", "review"],
    )
    .expect("insert label");
}

fn seed_empty_bd_import_source(db_path: &Path) {
    let conn = Connection::open(db_path).expect("open beads db");
    conn.execute(
        "CREATE TABLE issues (id TEXT PRIMARY KEY, title TEXT NOT NULL, description TEXT, acceptance_criteria TEXT, status TEXT NOT NULL, priority INTEGER NOT NULL, issue_type TEXT NOT NULL, owner TEXT, assignee TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, closed_at TEXT, close_reason TEXT, deleted_at TEXT)",
        [],
    )
    .expect("create issues table");
    conn.execute(
        "CREATE TABLE dependencies (issue_id TEXT NOT NULL, depends_on_id TEXT NOT NULL, type TEXT NOT NULL, created_at TEXT NOT NULL)",
        [],
    )
    .expect("create dependencies table");
    conn.execute(
        "CREATE TABLE labels (issue_id TEXT NOT NULL, label TEXT NOT NULL)",
        [],
    )
    .expect("create labels table");
}

fn seed_invalid_bd_import_source(db_path: &Path) {
    let conn = Connection::open(db_path).expect("open beads db");
    conn.execute(
        "CREATE TABLE issues (id TEXT PRIMARY KEY, title TEXT NOT NULL, description TEXT, acceptance_criteria TEXT, status TEXT NOT NULL, priority INTEGER NOT NULL, issue_type TEXT NOT NULL, owner TEXT, assignee TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, closed_at TEXT, close_reason TEXT, deleted_at TEXT)",
        [],
    )
    .expect("create issues table");
    conn.execute(
        "CREATE TABLE dependencies (issue_id TEXT NOT NULL, depends_on_id TEXT NOT NULL, type TEXT NOT NULL, created_at TEXT NOT NULL)",
        [],
    )
    .expect("create dependencies table");
    conn.execute(
        "CREATE TABLE labels (issue_id TEXT NOT NULL, label TEXT NOT NULL)",
        [],
    )
    .expect("create labels table");

    let long_id = "X".repeat(257);
    let long_title = "Y".repeat(513);
    let rows = [
        ("", "missing id", "open", 1_i64, "task"),
        ("BD-BLANK-TITLE", "   ", "open", 2_i64, "task"),
        (long_id.as_str(), "overlong id", "open", 3_i64, "task"),
        ("BD-LONG-TITLE", long_title.as_str(), "open", 4_i64, "task"),
    ];
    for (id, title, status, priority, issue_type) in rows {
        conn.execute(
            "INSERT INTO issues (id, title, description, acceptance_criteria, status, priority, issue_type, owner, assignee, created_at, updated_at, closed_at, close_reason, deleted_at) VALUES (?1, ?2, '', '', ?3, ?4, ?5, '', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL, '', NULL)",
            params![id, title, status, priority, issue_type],
        )
        .expect("insert invalid issue");
    }
}

fn id_key(label: &str) -> String {
    format!(
        "{label}-{}",
        NEXT_IDEMPOTENCY_KEY.fetch_add(1, Ordering::Relaxed)
    )
}

fn send_heartbeat(covey: &Covey, session_token: &str) -> covey::Result<()> {
    covey.heartbeat(HeartbeatReq {
        session_token: session_token.to_owned(),
        idempotency_key: id_key("heartbeat"),
    })
}

fn close_session(covey: &Covey, session_token: &str) -> covey::Result<()> {
    covey.exit_session(ExitSessionReq {
        session_token: session_token.to_owned(),
        idempotency_key: id_key("exit-session"),
    })
}

fn pragma_integrity_check(db_path: &Path) -> String {
    let conn = Connection::open(db_path).expect("open integrity-check db");
    conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("integrity check")
}

type SqliteFaultCallback = unsafe extern "C" fn(i32) -> i32;

unsafe extern "C" fn sqlite_fault_callback(test_code: i32) -> i32 {
    if test_code == SQLITE_FAULT_TARGET_CODE.load(Ordering::Relaxed) {
        SQLITE_FAULT_SEEN_TARGET.store(true, Ordering::Relaxed);
        if !SQLITE_FAULT_TRIGGERED.swap(true, Ordering::Relaxed) {
            return 1;
        }
    }
    0
}

fn install_sqlite_fault_callback(target_code: i32) {
    SQLITE_FAULT_TARGET_CODE.store(target_code, Ordering::Relaxed);
    SQLITE_FAULT_TRIGGERED.store(false, Ordering::Relaxed);
    SQLITE_FAULT_SEEN_TARGET.store(false, Ordering::Relaxed);
    unsafe {
        sqlite3_test_control(
            SQLITE_TESTCTRL_FAULT_INSTALL,
            Some(sqlite_fault_callback as SqliteFaultCallback),
        );
    }
}

fn uninstall_sqlite_fault_callback() {
    unsafe {
        sqlite3_test_control(SQLITE_TESTCTRL_FAULT_INSTALL, None::<SqliteFaultCallback>);
    }
}

#[test]
fn session_lifecycle_and_stale_reap_follow_spec() {
    let rig = Rig::new();
    let covey = rig.covey();

    let sess_a = register(&covey, "sess_a", "principal_a", SessionRole::Executor);
    let duplicate = covey.register_session(RegisterSessionReq {
        agent_principal_id: "principal_a".into(),
        agent_instance_id: "principal_a-next".into(),
        role: SessionRole::Executor,
        idempotency_key: id_key("register-session"),
    });
    assert!(matches!(
        duplicate,
        Err(CoveyError::SessionAlreadyActive { agent_principal_id }) if agent_principal_id == "principal_a"
    ));

    let missing = send_heartbeat(&covey, "missing");
    assert!(matches!(missing, Err(CoveyError::SessionNotFound)));

    rig.tick(60_000);
    let first = covey.reap_stale_sessions(45_000).expect("reap stale");
    assert_eq!(first.stale_sessions, 1);
    let second = covey.reap_stale_sessions(45_000).expect("reap stale twice");
    assert_eq!(second.stale_sessions, 0);

    close_session(&covey, &sess_a).expect("exit old session");
    assert!(matches!(
        send_heartbeat(&covey, &sess_a),
        Err(CoveyError::IllegalTransition { from, object, .. })
            if from == SessionState::Exited.into() && object == ObjectType::Session
    ));
    assert!(matches!(
        covey.register_session(RegisterSessionReq {
            agent_principal_id: "principal_b".into(),
            agent_instance_id: "principal_b-instance".into(),
            role: SessionRole::Reviewer,
            idempotency_key: id_key("register-session"),
        }),
        Ok(handle) if handle.session_token != sess_a
    ));
    rig.tick(1);
    register(&covey, "sess_c", "principal_a", SessionRole::Executor);
}

#[test]
fn idempotent_mutations_replay_and_reject_payload_drift() {
    let rig = Rig::new();
    let covey = rig.covey();

    let request = RegisterSessionReq {
        agent_principal_id: "principal".into(),
        agent_instance_id: "instance".into(),
        role: SessionRole::Executor,
        idempotency_key: "register-stable-key".into(),
    };
    let first = covey
        .register_session(request.clone())
        .expect("first register");
    let replay = covey.register_session(request).expect("idempotent replay");
    assert_eq!(first, replay);

    assert!(matches!(
        covey.register_session(RegisterSessionReq {
            agent_principal_id: "principal".into(),
            agent_instance_id: "different-instance".into(),
            role: SessionRole::Executor,
            idempotency_key: "register-stable-key".into(),
        }),
        Err(CoveyError::IdempotencyConflict { actor_key, operation, idempotency_key })
            if actor_key == "principal"
                && operation == "register_session"
                && idempotency_key == "register-stable-key"
    ));
}

#[test]
fn failed_mutations_do_not_append_event_rows_and_artifact_digests_are_unique() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);

    let claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim next")
        .expect("claim exists");
    rig.tick(1);
    covey
        .start_subtask(StartSubtaskReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start work");
    rig.tick(1);
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:artifact_a".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "deadbeef".into(),
            manifest_path: "artifacts/a.json".into(),
            changed_paths_digest: "blake3:paths_a".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish artifact");

    let before_events = covey.fetch_events(0, 100).expect("events").len();
    let collision = covey.publish_artifact(PublishArtifactReq {
        session_token: worker,
        claim_id: claim.claim_id,
        fence_seq: claim.fence_seq,
        artifact_digest: "blake3:artifact_a".into(),
        artifact_kind: ArtifactKind::PatchBundle,
        base_rev: "deadbeef".into(),
        manifest_path: "artifacts/b.json".into(),
        changed_paths_digest: "blake3:paths_b".into(),
        idempotency_key: id_key("publish-artifact"),
    });
    assert!(matches!(
        collision,
        Err(CoveyError::ArtifactDigestCollision { digest }) if digest == "blake3:artifact_a"
    ));

    let after_events = covey.fetch_events(0, 100).expect("events").len();
    assert_eq!(before_events, after_events);
    assert_eq!(
        covey
            .subtask_status(&subtask_id)
            .expect("status")
            .subtask
            .state,
        SubtaskState::ArtifactPublished
    );
}

#[test]
fn stale_fence_tokens_are_rejected_after_reclaim() {
    let rig = Rig::new();
    let covey = rig.covey();
    seed_work_subtask(&rig);
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);

    let first = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 10_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("first claim")
        .expect("claim result");
    rig.tick(1);
    covey
        .release_claim(ReleaseClaimReq {
            session_token: worker.clone(),
            claim_id: first.claim_id.clone(),
            fence_seq: first.fence_seq,
            idempotency_key: id_key("release-claim"),
        })
        .expect("release first");

    rig.tick(1);
    let second = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 10_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("second claim")
        .expect("claim result");
    let stale = covey.release_claim(ReleaseClaimReq {
        session_token: worker.clone(),
        claim_id: first.claim_id,
        fence_seq: first.fence_seq,
        idempotency_key: id_key("release-claim"),
    });
    assert!(matches!(
        stale,
        Err(CoveyError::ClaimNotFound)
            | Err(CoveyError::StaleFenceToken { .. })
            | Err(CoveyError::LeaseExpired { .. })
            | Err(CoveyError::ClaimNotHeld { .. })
    ));
    assert!(second.fence_seq > 0);
}

#[test]
fn claim_renewal_extends_the_active_lease() {
    let rig = Rig::new();
    let covey = rig.covey();
    seed_work_subtask(&rig);
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);

    let claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 10_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim")
        .expect("claim result");
    rig.tick(5_000);

    let renewed = covey
        .renew_claim(RenewClaimReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            extend_by_ms: 20_000,
            idempotency_key: id_key("renew-claim"),
        })
        .expect("renew claim");
    assert_eq!(renewed.claim_id, claim.claim_id);
    assert_eq!(renewed.fence_seq, claim.fence_seq);
    assert_eq!(renewed.lease_deadline, claim.lease_deadline + 20_000);

    rig.tick(10_001);
    covey
        .start_subtask(StartSubtaskReq {
            session_token: worker,
            claim_id: claim.claim_id,
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start after renewal");
}

#[test]
fn import_repeat_is_deterministic() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(
        &covey,
        "orch-import",
        "orch-import",
        SessionRole::Orchestrator,
    );

    let first_subtask_id = covey
        .import_bd_v1_work_subtask(
            &orch,
            None,
            Some("import bd issue set"),
            "bd-issue-1",
            "imported issue one",
            3,
            "import-repeat-first",
        )
        .expect("first import");
    let status_after_first = covey
        .subtask_status(&first_subtask_id)
        .expect("subtask status");
    let destination_meta_task_id = status_after_first.subtask.meta_task_id.clone();

    let second_subtask_id = covey
        .import_bd_v1_work_subtask(
            &orch,
            Some(&destination_meta_task_id),
            None,
            "bd-issue-1",
            "imported issue one",
            3,
            "import-repeat-second",
        )
        .expect("repeat import");

    assert_eq!(first_subtask_id, second_subtask_id);
    assert!(first_subtask_id.starts_with("bdwork_bd_issue_1_"));

    let meta_status = covey
        .meta_task_status(&destination_meta_task_id)
        .expect("meta-task status");
    assert_eq!(meta_status.subtasks.len(), 1);
    let imported = &meta_status.subtasks[0];
    assert_eq!(imported.subtask_id, first_subtask_id);
    assert_eq!(imported.kind, SubtaskKind::Work);
    assert_eq!(imported.state, SubtaskState::Available);
    assert!(imported.active_claim_id.is_none());
    assert!(imported.review_target.is_none());
    assert!(imported.review_target.is_none());

    let event_log = covey.fetch_events(0, 100).expect("event log");
    let subtask_created_events = event_log
        .iter()
        .filter(|event| event.event_type == EventType::SubtaskCreated)
        .count();
    let meta_submitted_events = event_log
        .iter()
        .filter(|event| event.event_type == EventType::MetaTaskSubmitted)
        .count();
    assert_eq!(subtask_created_events, 1);
    assert_eq!(meta_submitted_events, 1);

    let conn = Connection::open(&rig.db_path).expect("open db");
    let claim_count = conn
        .query_row("SELECT COUNT(*) FROM claims", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("claim count");
    assert_eq!(claim_count, 0);
}

#[test]
fn import_bd_reimport_allowed_non_task_types_is_deterministic() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(
        &covey,
        "orch-reimport-feature",
        "orch-reimport-feature",
        SessionRole::Orchestrator,
    );

    let beads_dir = TempDir::new().expect("tempdir");
    let beads_db = beads_dir.path().join("beads.db");
    {
        let conn = Connection::open(&beads_db).expect("open beads db");
        conn.execute(
            "CREATE TABLE issues (id TEXT PRIMARY KEY, title TEXT NOT NULL, description TEXT, acceptance_criteria TEXT, status TEXT NOT NULL, priority INTEGER NOT NULL, issue_type TEXT NOT NULL, owner TEXT, assignee TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, closed_at TEXT, close_reason TEXT, deleted_at TEXT)",
            [],
        )
        .expect("create issues table");
        conn.execute(
            "INSERT INTO issues (id, title, description, acceptance_criteria, status, priority, issue_type, owner, assignee, created_at, updated_at, closed_at, close_reason, deleted_at) VALUES (?1, ?2, '', '', ?3, ?4, ?5, '', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL, '', NULL)",
            params!["BD-FEATURE-1", "feature work item", "open", 5_i64, "feature"],
        )
        .expect("insert issue");
    }

    let first_result = covey
        .import_bd_v1(ImportBdV1Req {
            session_token: orch.clone(),
            beads_db_path: beads_db.to_string_lossy().to_string(),
            meta_task_id: None,
            prompt_text: Some("feature import destination".into()),
            idempotency_key: id_key("import-feature-first"),
        })
        .expect("first import");

    assert!(first_result.meta_task_id.starts_with("meta_"));
    assert_eq!(first_result.imported_count, 1);
    assert_eq!(first_result.skipped_count, 0);
    assert!(first_result.items.iter().any(|item| {
        item.source_issue_id == "BD-FEATURE-1"
            && item.skip_reason.is_none()
            && item
                .subtask_id
                .as_deref()
                .is_some_and(|subtask_id| subtask_id.starts_with("bdwork_bd_feature_1_"))
    }));
    let destination_meta_task_id = first_result.meta_task_id;

    let second_result = covey
        .import_bd_v1(ImportBdV1Req {
            session_token: orch.clone(),
            beads_db_path: beads_db.to_string_lossy().to_string(),
            meta_task_id: Some(destination_meta_task_id.clone()),
            prompt_text: None,
            idempotency_key: id_key("import-feature-second"),
        })
        .expect("repeat import");

    assert_eq!(second_result.meta_task_id, destination_meta_task_id);
    assert_eq!(second_result.imported_count, 0);
    assert_eq!(second_result.skipped_count, 1);
    assert!(second_result.items.iter().any(|item| {
        item.source_issue_id == "BD-FEATURE-1"
            && item.skip_reason == Some(ImportBdV1SkipReason::DeterministicDuplicate)
    }));

    let meta_status = covey
        .meta_task_status(&destination_meta_task_id)
        .expect("meta-task status");
    assert_eq!(meta_status.meta_task.state, MetaTaskState::Active);
    assert_eq!(meta_status.subtasks.len(), 1);
    let imported = &meta_status.subtasks[0];
    assert!(imported.subtask_id.starts_with("bdwork_bd_feature_1_"));
    assert_eq!(imported.kind, SubtaskKind::Work);
    assert_eq!(imported.state, SubtaskState::Available);
    assert!(imported.active_claim_id.is_none());

    let event_log = covey.fetch_events(0, 100).expect("event log");
    let subtask_created_events = event_log
        .iter()
        .filter(|event| event.event_type == EventType::SubtaskCreated)
        .count();
    let meta_submitted_events = event_log
        .iter()
        .filter(|event| event.event_type == EventType::MetaTaskSubmitted)
        .count();
    assert_eq!(subtask_created_events, 1);
    assert_eq!(meta_submitted_events, 1);

    let conn = Connection::open(&rig.db_path).expect("open db");
    let claim_count = conn
        .query_row("SELECT COUNT(*) FROM claims", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("claim count");
    assert_eq!(claim_count, 0);
}

#[test]
fn import_bd_reimport_allowed_non_task_type_into_different_meta_task_conflicts() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(
        &covey,
        "orch-reimport-epic-conflict",
        "orch-reimport-epic-conflict",
        SessionRole::Orchestrator,
    );

    let beads_dir = TempDir::new().expect("tempdir");
    let beads_db = beads_dir.path().join("beads.db");
    {
        let conn = Connection::open(&beads_db).expect("open beads db");
        conn.execute(
            "CREATE TABLE issues (id TEXT PRIMARY KEY, title TEXT NOT NULL, description TEXT, acceptance_criteria TEXT, status TEXT NOT NULL, priority INTEGER NOT NULL, issue_type TEXT NOT NULL, owner TEXT, assignee TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, closed_at TEXT, close_reason TEXT, deleted_at TEXT)",
            [],
        )
        .expect("create issues table");
        conn.execute(
            "INSERT INTO issues (id, title, description, acceptance_criteria, status, priority, issue_type, owner, assignee, created_at, updated_at, closed_at, close_reason, deleted_at) VALUES (?1, ?2, '', '', ?3, ?4, ?5, '', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL, '', NULL)",
            params!["BD-EPIC-1", "epic container", "open", 4_i64, "epic"],
        )
        .expect("insert issue");
    }

    let first_result = covey
        .import_bd_v1(ImportBdV1Req {
            session_token: orch.clone(),
            beads_db_path: beads_db.to_string_lossy().to_string(),
            meta_task_id: None,
            prompt_text: Some("epic import first destination".into()),
            idempotency_key: id_key("import-epic-first"),
        })
        .expect("first import");

    assert!(first_result.meta_task_id.starts_with("meta_"));
    assert_eq!(first_result.imported_count, 1);
    assert_eq!(first_result.skipped_count, 0);

    let conflict = covey.import_bd_v1(ImportBdV1Req {
        session_token: orch.clone(),
        beads_db_path: beads_db.to_string_lossy().to_string(),
        meta_task_id: None,
        prompt_text: Some("epic import second destination".into()),
        idempotency_key: id_key("import-epic-second"),
    });

    assert!(matches!(
        conflict,
        Err(CoveyError::ImportDuplicate { source_issue_id, subtask_id })
            if source_issue_id == "BD-EPIC-1" && subtask_id.starts_with("bdwork_bd_epic_1_")
    ));

    let event_log = covey.fetch_events(0, 100).expect("event log");
    let subtask_created_events = event_log
        .iter()
        .filter(|event| event.event_type == EventType::SubtaskCreated)
        .count();
    let meta_submitted_events = event_log
        .iter()
        .filter(|event| event.event_type == EventType::MetaTaskSubmitted)
        .count();
    assert_eq!(subtask_created_events, 1);
    assert_eq!(meta_submitted_events, 1);

    let conn = Connection::open(&rig.db_path).expect("open db");
    let claim_count = conn
        .query_row("SELECT COUNT(*) FROM claims", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("claim count");
    assert_eq!(claim_count, 0);
}

#[test]
fn import_type_surface_smoke() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(
        &covey,
        "orch-import",
        "orch-import",
        SessionRole::Orchestrator,
    );

    let req = ImportBdV1Req {
        session_token: orch.clone(),
        beads_db_path: "/nonexistent/path/beads.db".to_owned(),
        meta_task_id: None,
        prompt_text: Some("test import".to_owned()),
        idempotency_key: "import-smoke".to_owned(),
    };
    let req_json = serde_json::to_string(&req).expect("serialize request");
    let req_back: ImportBdV1Req = serde_json::from_str(&req_json).expect("deserialize request");
    assert_eq!(req, req_back);

    let result = ImportBdV1Result::new(
        "meta-1".to_owned(),
        2,
        1,
        vec![
            ImportBdV1ItemResult {
                source_issue_id: "bd-1".to_owned(),
                subtask_id: Some("subtask-1".to_owned()),
                skip_reason: None,
            },
            ImportBdV1ItemResult {
                source_issue_id: "bd-2".to_owned(),
                subtask_id: None,
                skip_reason: Some(ImportBdV1SkipReason::DeterministicDuplicate),
            },
        ],
    );
    let result_json = serde_json::to_string(&result).expect("serialize result");
    let result_back: ImportBdV1Result =
        serde_json::from_str(&result_json).expect("deserialize result");
    assert_eq!(result, result_back);

    let tmp = TempDir::new().expect("tempdir");
    let beads_db = tmp.path().join("beads.db");
    {
        let conn = Connection::open(&beads_db).expect("open beads db");
        conn.execute(
            "CREATE TABLE issues (id TEXT PRIMARY KEY, title TEXT NOT NULL, description TEXT, status TEXT, priority INTEGER, issue_type TEXT, owner TEXT, assignee TEXT, created_at TEXT, updated_at TEXT, closed_at TEXT, close_reason TEXT, deleted_at TEXT)",
            [],
        )
        .expect("create issues table");
    }

    let valid_req = ImportBdV1Req {
        session_token: orch.clone(),
        beads_db_path: beads_db.to_string_lossy().to_string(),
        meta_task_id: None,
        prompt_text: Some("test import valid".to_owned()),
        idempotency_key: "import-smoke-valid".to_owned(),
    };
    let import_result = covey
        .import_bd_v1(valid_req)
        .expect("import should succeed");
    assert!(import_result.meta_task_id.starts_with("meta_"));
    assert_eq!(import_result.imported_count, 0);
    assert_eq!(import_result.skipped_count, 0);
    assert!(import_result.items.is_empty());
}

#[test]
fn import_bd_creates_available_work_subtasks() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(
        &covey,
        "orch-import-core",
        "orch-import-core",
        SessionRole::Orchestrator,
    );

    let tmp = TempDir::new().expect("tempdir");
    let beads_db = tmp.path().join("beads.db");
    seed_bd_import_source(&beads_db);

    let result = covey
        .import_bd_v1(ImportBdV1Req {
            session_token: orch.clone(),
            beads_db_path: beads_db.to_string_lossy().to_string(),
            meta_task_id: None,
            prompt_text: Some("import bd issue set".to_owned()),
            idempotency_key: "import-bd-core".to_owned(),
        })
        .expect("import should succeed");

    assert!(result.meta_task_id.starts_with("meta_"));
    assert_eq!(result.imported_count, 4);
    assert_eq!(result.skipped_count, 3);
    assert_eq!(result.items.len(), 7);

    assert!(result.items.iter().any(|item| {
        item.source_issue_id == "BD-1"
            && item.skip_reason.is_none()
            && item
                .subtask_id
                .as_deref()
                .is_some_and(|subtask_id| subtask_id.starts_with("bdwork_bd_1_"))
    }));
    assert!(result.items.iter().any(|item| {
        item.source_issue_id == "BD-2"
            && item.skip_reason.is_none()
            && item
                .subtask_id
                .as_deref()
                .is_some_and(|subtask_id| subtask_id.starts_with("bdwork_bd_2_"))
    }));
    assert!(result.items.iter().any(|item| {
        item.source_issue_id == "BD-4"
            && item.skip_reason.is_none()
            && item
                .subtask_id
                .as_deref()
                .is_some_and(|subtask_id| subtask_id.starts_with("bdwork_bd_4_"))
    }));
    assert!(result.items.iter().any(|item| {
        item.source_issue_id == "BD-5"
            && item.skip_reason.is_none()
            && item
                .subtask_id
                .as_deref()
                .is_some_and(|subtask_id| subtask_id.starts_with("bdwork_bd_5_"))
    }));

    assert!(result.items.iter().any(|item| {
        item.source_issue_id == "BD-3"
            && item.skip_reason
                == Some(ImportBdV1SkipReason::InvalidRow {
                    detail: "unsupported status closed".to_owned(),
                })
    }));
    assert!(result.items.iter().any(|item| {
        item.source_issue_id == "BD-6"
            && item.skip_reason
                == Some(ImportBdV1SkipReason::InvalidRow {
                    detail: "unsupported labeled issue".to_owned(),
                })
    }));
    assert!(result.items.iter().any(|item| {
        item.source_issue_id == "BD-7"
            && item.skip_reason
                == Some(ImportBdV1SkipReason::InvalidRow {
                    detail: "unsupported issue_type bug".to_owned(),
                })
    }));

    let meta_status = covey
        .meta_task_status(&result.meta_task_id)
        .expect("meta-task status");
    assert_eq!(meta_status.meta_task.state, MetaTaskState::Active);
    assert_eq!(meta_status.subtasks.len(), 4);
    for subtask in &meta_status.subtasks {
        assert_eq!(subtask.kind, SubtaskKind::Work);
        assert_eq!(subtask.state, SubtaskState::Available);
        assert!(subtask.active_claim_id.is_none());
        assert!(subtask.review_target.is_none());
        assert!(subtask.review_target.is_none());
    }

    let conn = Connection::open(&rig.db_path).expect("open db");
    let claim_count = conn
        .query_row("SELECT COUNT(*) FROM claims", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("claim count");
    let session_active_subtask: Option<String> = conn
        .query_row(
            "SELECT active_subtask_id FROM sessions WHERE session_token = ?1",
            params![orch],
            |row| row.get(0),
        )
        .expect("session active subtask");
    assert_eq!(claim_count, 0);
    assert!(session_active_subtask.is_none());
}

#[test]
fn import_bd_into_existing_non_terminal_meta_task_preserves_existing_work_and_no_claim_side_effects()
 {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(
        &covey,
        "orch-import-existing-core",
        "orch-import-existing-core",
        SessionRole::Orchestrator,
    );

    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "existing destination".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("submit meta task");
    let manual_subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id: meta_task_id.clone(),
            subtask_id: Some("manual_existing_work".into()),
            title: "manual existing work".into(),
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create manual subtask");

    let beads_dir = TempDir::new().expect("tempdir");
    let beads_db = beads_dir.path().join("beads.db");
    seed_bd_import_source(&beads_db);

    let result = covey
        .import_bd_v1(ImportBdV1Req {
            session_token: orch.clone(),
            beads_db_path: beads_db.to_string_lossy().to_string(),
            meta_task_id: Some(meta_task_id.clone()),
            prompt_text: None,
            idempotency_key: id_key("import-bd-existing"),
        })
        .expect("import should succeed");

    assert_eq!(result.meta_task_id, meta_task_id);
    assert_eq!(result.imported_count, 4);
    assert_eq!(result.skipped_count, 3);

    let meta_status = covey
        .meta_task_status(&result.meta_task_id)
        .expect("meta-task status");
    assert_eq!(meta_status.meta_task.state, MetaTaskState::Active);
    assert_eq!(meta_status.subtasks.len(), 5);
    assert!(
        meta_status
            .subtasks
            .iter()
            .any(|subtask| subtask.subtask_id == manual_subtask_id)
    );

    let imported_subtasks: Vec<_> = meta_status
        .subtasks
        .iter()
        .filter(|subtask| subtask.subtask_id != manual_subtask_id)
        .collect();
    assert_eq!(imported_subtasks.len(), 4);
    for subtask in imported_subtasks {
        assert_eq!(subtask.kind, SubtaskKind::Work);
        assert_eq!(subtask.state, SubtaskState::Available);
        assert!(subtask.active_claim_id.is_none());
    }

    let session_status = covey.session_status(&orch).expect("session status");
    assert!(session_status.session.active_subtask_id().is_none());
    assert!(session_status.active_subtask.is_none());

    let conn = Connection::open(&rig.db_path).expect("open db");
    let claim_count = conn
        .query_row("SELECT COUNT(*) FROM claims", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("claim count");
    assert_eq!(claim_count, 0);
}

#[test]
fn import_bd_empty_source_creates_destination_without_subtasks_or_side_effects() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(
        &covey,
        "orch-import-empty-core",
        "orch-import-empty-core",
        SessionRole::Orchestrator,
    );

    let beads_dir = TempDir::new().expect("tempdir");
    let beads_db = beads_dir.path().join("beads.db");
    seed_empty_bd_import_source(&beads_db);

    let result = covey
        .import_bd_v1(ImportBdV1Req {
            session_token: orch.clone(),
            beads_db_path: beads_db.to_string_lossy().to_string(),
            meta_task_id: None,
            prompt_text: Some("empty import destination".into()),
            idempotency_key: id_key("import-bd-empty"),
        })
        .expect("import should succeed");

    assert_eq!(result.imported_count, 0);
    assert_eq!(result.skipped_count, 0);
    assert!(result.items.is_empty());

    let meta_status = covey
        .meta_task_status(&result.meta_task_id)
        .expect("meta-task status");
    assert_eq!(meta_status.meta_task.state, MetaTaskState::Planning);
    assert!(meta_status.subtasks.is_empty());

    let session_status = covey.session_status(&orch).expect("session status");
    assert!(session_status.session.active_subtask_id().is_none());
    assert!(session_status.active_subtask.is_none());

    let conn = Connection::open(&rig.db_path).expect("open db");
    let claim_count = conn
        .query_row("SELECT COUNT(*) FROM claims", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("claim count");
    assert_eq!(claim_count, 0);
}

#[test]
fn import_bd_invalid_rows_are_reported_without_creating_subtasks_or_claims() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(
        &covey,
        "orch-import-invalid-core",
        "orch-import-invalid-core",
        SessionRole::Orchestrator,
    );

    let beads_dir = TempDir::new().expect("tempdir");
    let beads_db = beads_dir.path().join("beads.db");
    seed_invalid_bd_import_source(&beads_db);

    let result = covey
        .import_bd_v1(ImportBdV1Req {
            session_token: orch.clone(),
            beads_db_path: beads_db.to_string_lossy().to_string(),
            meta_task_id: None,
            prompt_text: Some("invalid import destination".into()),
            idempotency_key: id_key("import-bd-invalid-rows"),
        })
        .expect("import should succeed");

    assert_eq!(result.imported_count, 0);
    assert_eq!(result.skipped_count, 4);
    assert_eq!(result.items.len(), 4);
    assert!(result.items.iter().all(|item| item.subtask_id.is_none()));
    assert!(result.items.iter().any(|item| {
        item.skip_reason
            == Some(ImportBdV1SkipReason::InvalidRow {
                detail: "missing issue id".to_owned(),
            })
    }));
    assert!(result.items.iter().any(|item| {
        item.skip_reason
            == Some(ImportBdV1SkipReason::InvalidRow {
                detail: "missing title".to_owned(),
            })
    }));
    assert!(result.items.iter().any(|item| {
        item.skip_reason
            == Some(ImportBdV1SkipReason::InvalidRow {
                detail: "issue id exceeds max length 256".to_owned(),
            })
    }));
    assert!(result.items.iter().any(|item| {
        item.skip_reason
            == Some(ImportBdV1SkipReason::InvalidRow {
                detail: "title exceeds max length 512".to_owned(),
            })
    }));

    let meta_status = covey
        .meta_task_status(&result.meta_task_id)
        .expect("meta-task status");
    assert!(meta_status.subtasks.is_empty());

    let session_status = covey.session_status(&orch).expect("session status");
    assert!(session_status.session.active_subtask_id().is_none());
    assert!(session_status.active_subtask.is_none());

    let conn = Connection::open(&rig.db_path).expect("open db");
    let claim_count = conn
        .query_row("SELECT COUNT(*) FROM claims", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("claim count");
    assert_eq!(claim_count, 0);
}

#[test]
fn import_bd_allowed_type_labels_and_casefolding_behave_correctly() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(
        &covey,
        "orch-import-casefold",
        "orch-import-casefold",
        SessionRole::Orchestrator,
    );

    let beads_dir = TempDir::new().expect("tempdir");
    let beads_db = beads_dir.path().join("beads.db");
    {
        let conn = Connection::open(&beads_db).expect("open beads db");
        conn.execute(
            "CREATE TABLE issues (id TEXT PRIMARY KEY, title TEXT NOT NULL, description TEXT, acceptance_criteria TEXT, status TEXT NOT NULL, priority INTEGER NOT NULL, issue_type TEXT NOT NULL, owner TEXT, assignee TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, closed_at TEXT, close_reason TEXT, deleted_at TEXT)",
            [],
        )
        .expect("create issues table");
        conn.execute(
            "CREATE TABLE labels (issue_id TEXT NOT NULL, label TEXT NOT NULL)",
            [],
        )
        .expect("create labels table");

        let rows = [
            (
                "BD-FEATURE-MIXED",
                "mixed case feature",
                "open",
                1_i64,
                "Feature",
            ),
            ("BD-EPIC-MIXED", "mixed case epic", "open", 2_i64, "EPIC"),
            (
                "BD-REVIEW-TASK",
                "review labeled task",
                "open",
                3_i64,
                "task",
            ),
            (
                "BD-SKIP-TASK",
                "import skip labeled task",
                "open",
                4_i64,
                "task",
            ),
            ("BD-BUG-LOWER", "unsupported bug", "open", 5_i64, "bug"),
        ];
        for (id, title, status, priority, issue_type) in rows {
            conn.execute(
                "INSERT INTO issues (id, title, description, acceptance_criteria, status, priority, issue_type, owner, assignee, created_at, updated_at, closed_at, close_reason, deleted_at) VALUES (?1, ?2, '', '', ?3, ?4, ?5, '', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL, '', NULL)",
                params![id, title, status, priority, issue_type],
            )
            .expect("insert issue");
        }

        conn.execute(
            "INSERT INTO labels (issue_id, label) VALUES (?1, ?2)",
            params!["BD-REVIEW-TASK", "review"],
        )
        .expect("insert review label");
        conn.execute(
            "INSERT INTO labels (issue_id, label) VALUES (?1, ?2)",
            params!["BD-SKIP-TASK", "import:skip"],
        )
        .expect("insert import:skip label");
    }

    let result = covey
        .import_bd_v1(ImportBdV1Req {
            session_token: orch.clone(),
            beads_db_path: beads_db.to_string_lossy().to_string(),
            meta_task_id: None,
            prompt_text: Some("casefold and label edge cases".into()),
            idempotency_key: id_key("import-bd-casefold"),
        })
        .expect("import should succeed");

    assert!(result.meta_task_id.starts_with("meta_"));
    assert_eq!(result.imported_count, 2);
    assert_eq!(result.skipped_count, 3);
    assert_eq!(result.items.len(), 5);

    assert!(result.items.iter().any(|item| {
        item.source_issue_id == "BD-FEATURE-MIXED"
            && item.skip_reason.is_none()
            && item
                .subtask_id
                .as_deref()
                .is_some_and(|subtask_id| subtask_id.starts_with("bdwork_bd_feature_mixed_"))
    }));
    assert!(result.items.iter().any(|item| {
        item.source_issue_id == "BD-EPIC-MIXED"
            && item.skip_reason.is_none()
            && item
                .subtask_id
                .as_deref()
                .is_some_and(|subtask_id| subtask_id.starts_with("bdwork_bd_epic_mixed_"))
    }));

    assert!(result.items.iter().any(|item| {
        item.source_issue_id == "BD-REVIEW-TASK"
            && item.skip_reason
                == Some(ImportBdV1SkipReason::InvalidRow {
                    detail: "unsupported labeled issue".to_owned(),
                })
    }));
    assert!(result.items.iter().any(|item| {
        item.source_issue_id == "BD-SKIP-TASK"
            && item.skip_reason
                == Some(ImportBdV1SkipReason::InvalidRow {
                    detail: "unsupported labeled issue".to_owned(),
                })
    }));
    assert!(result.items.iter().any(|item| {
        item.source_issue_id == "BD-BUG-LOWER"
            && item.skip_reason
                == Some(ImportBdV1SkipReason::InvalidRow {
                    detail: "unsupported issue_type bug".to_owned(),
                })
    }));

    let meta_status = covey
        .meta_task_status(&result.meta_task_id)
        .expect("meta-task status");
    assert_eq!(meta_status.meta_task.state, MetaTaskState::Active);
    assert_eq!(meta_status.subtasks.len(), 2);
    for subtask in &meta_status.subtasks {
        assert_eq!(subtask.kind, SubtaskKind::Work);
        assert_eq!(subtask.state, SubtaskState::Available);
        assert!(subtask.active_claim_id.is_none());
    }

    let conn = Connection::open(&rig.db_path).expect("open db");
    let claim_count = conn
        .query_row("SELECT COUNT(*) FROM claims", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("claim count");
    assert_eq!(claim_count, 0);
}

#[test]
fn import_malformed_source_fails_safely() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(
        &covey,
        "orch-import-malformed",
        "orch-import-malformed",
        SessionRole::Orchestrator,
    );

    let tmp = TempDir::new().expect("tempdir");
    let beads_db = tmp.path().join("bad_beads.db");
    {
        let conn = Connection::open(&beads_db).expect("open malformed beads db");
        conn.execute(
            "CREATE TABLE issues (id TEXT PRIMARY KEY, title TEXT NOT NULL, priority INTEGER NOT NULL, issue_type TEXT NOT NULL)",
            [],
        )
        .expect("create malformed issues table");
    }

    let err = covey
        .import_bd_v1(ImportBdV1Req {
            session_token: orch.clone(),
            beads_db_path: beads_db.to_string_lossy().to_string(),
            meta_task_id: None,
            prompt_text: Some("should not import".to_owned()),
            idempotency_key: "import-malformed-source".to_owned(),
        })
        .expect_err("malformed import should fail");

    assert!(matches!(
        err,
        CoveyError::InvalidSourceSchema { detail, .. }
            if detail == "issues table missing required column status"
    ));

    let conn = Connection::open(&rig.db_path).expect("open covey db");
    let meta_count = conn
        .query_row("SELECT COUNT(*) FROM meta_tasks", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("meta count");
    let subtask_count = conn
        .query_row("SELECT COUNT(*) FROM subtasks", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("subtask count");
    let claim_count = conn
        .query_row("SELECT COUNT(*) FROM claims", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("claim count");
    let session_active_subtask: Option<String> = conn
        .query_row(
            "SELECT active_subtask_id FROM sessions WHERE session_token = ?1",
            params![orch],
            |row| row.get(0),
        )
        .expect("session active subtask");
    assert_eq!(meta_count, 0);
    assert_eq!(subtask_count, 0);
    assert_eq!(claim_count, 0);
    assert!(session_active_subtask.is_none());
}

#[test]
fn import_claim_composition_remains_deferred() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(
        &covey,
        "orch-import-targeted-claim-boundary",
        "orch-import-targeted-claim-boundary",
        SessionRole::Orchestrator,
    );

    let tmp = TempDir::new().expect("tempdir");
    let beads_db = tmp.path().join("beads.db");
    seed_bd_import_source(&beads_db);

    let result = covey
        .import_bd_v1(ImportBdV1Req {
            session_token: orch.clone(),
            beads_db_path: beads_db.to_string_lossy().to_string(),
            meta_task_id: None,
            prompt_text: Some("targeted claim boundary test".to_owned()),
            idempotency_key: id_key("import-targeted-claim-boundary"),
        })
        .expect("import should succeed");

    // All imported subtasks remain available; no claims are created.
    let meta_status = covey
        .meta_task_status(&result.meta_task_id)
        .expect("meta-task status");
    for subtask in &meta_status.subtasks {
        assert_eq!(subtask.state, SubtaskState::Available);
        assert!(subtask.active_claim_id.is_none());
    }

    let conn = Connection::open(&rig.db_path).expect("open db");
    let claim_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM claims", [], |row| row.get(0))
        .expect("claim count");
    assert_eq!(claim_count, 0);

    // `claim_subtask` now exists as a separate primitive, but the importer does NOT
    // automatically create held claims. Any future import-and-claim mode must compose
    // through `claim_subtask` after import; the importer must not become a second claim
    // engine. This boundary test documents that deferred composition seam.
}

fn assert_claim_subtask_contract_shape(_: covey::Result<ClaimResult>) {}

const CLAIM_SUBTASK_RESULT_SHAPE: &str = "Result<ClaimResult>";
const CLAIM_SUBTASK_SELECTION_CONTRACT: &str =
    "claim_subtask is a target-selection variant of claim_next_subtask, not a new claim model";
const CLAIM_SUBTASK_ERROR_PRECEDENCE: [&str; 9] = [
    "1.active_session_validation",
    "2.lease_duration_validation",
    "3.session_active_subtask_occupancy",
    "4.subtask_existence",
    "5.lazy_expiry_of_stale_claim_on_target",
    "6.meta_task_schedulability",
    "7.role_kind_compatibility",
    "8.live_held_claim_conflict",
    "9.legal_transition_to_claimed",
];
const CLAIM_SUBTASK_STAGE_OUTCOMES: [&str; 9] = [
    "SessionNotFound|SessionNotActive",
    "InvalidLeaseDuration",
    "SessionAlreadyHasActiveSubtask",
    "SubtaskNotFound",
    "lazy_expiry_or_clear_stale_claim_before_conflict_check",
    "MetaTaskUnavailable",
    "WrongRole",
    "SubtaskAlreadyClaimed",
    "IllegalTransition",
];
const CLAIM_SUBTASK_SAME_SESSION_BEHAVIOR: &str = "if a session already has any active_subtask_id, return SessionAlreadyHasActiveSubtask first, even when the target subtask is the same active subtask";
const CLAIM_SUBTASK_NON_GOALS: [&str; 4] = [
    "import_and_claim_composition",
    "force_claim",
    "owner_impersonation",
    "fairness_or_policy_changes",
];

#[test]
fn claim_subtask_error_precedence_contract() {
    assert_eq!(CLAIM_SUBTASK_RESULT_SHAPE, "Result<ClaimResult>");
    assert_eq!(
        CLAIM_SUBTASK_SELECTION_CONTRACT,
        "claim_subtask is a target-selection variant of claim_next_subtask, not a new claim model"
    );

    assert_claim_subtask_contract_shape(Err(CoveyError::SubtaskNotFound));

    assert_eq!(
        CLAIM_SUBTASK_ERROR_PRECEDENCE[0],
        "1.active_session_validation"
    );
    assert_eq!(
        CLAIM_SUBTASK_ERROR_PRECEDENCE[1],
        "2.lease_duration_validation"
    );
    assert_eq!(
        CLAIM_SUBTASK_ERROR_PRECEDENCE[2],
        "3.session_active_subtask_occupancy"
    );
    assert_eq!(CLAIM_SUBTASK_ERROR_PRECEDENCE[3], "4.subtask_existence");
    assert_eq!(
        CLAIM_SUBTASK_ERROR_PRECEDENCE[4],
        "5.lazy_expiry_of_stale_claim_on_target"
    );
    assert_eq!(
        CLAIM_SUBTASK_ERROR_PRECEDENCE[5],
        "6.meta_task_schedulability"
    );
    assert_eq!(
        CLAIM_SUBTASK_ERROR_PRECEDENCE[6],
        "7.role_kind_compatibility"
    );
    assert_eq!(
        CLAIM_SUBTASK_ERROR_PRECEDENCE[7],
        "8.live_held_claim_conflict"
    );
    assert_eq!(
        CLAIM_SUBTASK_ERROR_PRECEDENCE[8],
        "9.legal_transition_to_claimed"
    );
    assert_eq!(
        CLAIM_SUBTASK_STAGE_OUTCOMES,
        [
            "SessionNotFound|SessionNotActive",
            "InvalidLeaseDuration",
            "SessionAlreadyHasActiveSubtask",
            "SubtaskNotFound",
            "lazy_expiry_or_clear_stale_claim_before_conflict_check",
            "MetaTaskUnavailable",
            "WrongRole",
            "SubtaskAlreadyClaimed",
            "IllegalTransition",
        ]
    );
    assert_eq!(
        CLAIM_SUBTASK_SAME_SESSION_BEHAVIOR,
        "if a session already has any active_subtask_id, return SessionAlreadyHasActiveSubtask first, even when the target subtask is the same active subtask"
    );
    assert_eq!(
        CLAIM_SUBTASK_NON_GOALS,
        [
            "import_and_claim_composition",
            "force_claim",
            "owner_impersonation",
            "fairness_or_policy_changes",
        ]
    );
}

#[test]
fn claim_subtask_type_surface_smoke() {
    let req = ClaimSubtaskReq {
        session_token: "session_abc123".to_owned(),
        subtask_id: "subtask_xyz789".to_owned(),
        lease_duration_ms: 30_000,
        idempotency_key: "idem_key_001".to_owned(),
    };

    let json = serde_json::to_string(&req).expect("serializes to JSON");
    let decoded: ClaimSubtaskReq = serde_json::from_str(&json).expect("deserializes from JSON");

    assert_eq!(req, decoded);
    assert_eq!(req.session_token, "session_abc123");
    assert_eq!(req.subtask_id, "subtask_xyz789");
    assert_eq!(req.lease_duration_ms, 30_000);
    assert_eq!(req.idempotency_key, "idem_key_001");

    let json_representation = serde_json::json!({
        "session_token": "session_abc123",
        "subtask_id": "subtask_xyz789",
        "lease_duration_ms": 30_000,
        "idempotency_key": "idem_key_001"
    });

    let from_json: ClaimSubtaskReq =
        serde_json::from_value(json_representation).expect("parses from JSON value");
    assert_eq!(from_json.session_token, "session_abc123");
    assert_eq!(from_json.subtask_id, "subtask_xyz789");
}

#[test]
fn claim_subtask_error_surface_smoke() {
    let subtask_not_found = CoveyError::SubtaskNotFound;
    assert!(matches!(subtask_not_found, CoveyError::SubtaskNotFound));

    let session_occupied = CoveyError::SessionAlreadyHasActiveSubtask {
        session_token: session_token("session_1"),
        active_subtask_id: subtask_id("subtask_1"),
    };
    assert!(matches!(
        session_occupied,
        CoveyError::SessionAlreadyHasActiveSubtask { .. }
    ));

    let wrong_role = CoveyError::WrongRole {
        expected: vec![SessionRole::Executor],
        actual: SessionRole::Orchestrator,
    };
    assert!(matches!(wrong_role, CoveyError::WrongRole { .. }));

    let meta_unavailable = CoveyError::MetaTaskUnavailable {
        meta_task_id: "meta_1".to_owned(),
        state: MetaTaskState::Cancelled,
    };
    assert!(matches!(
        meta_unavailable,
        CoveyError::MetaTaskUnavailable { .. }
    ));

    let already_claimed = CoveyError::SubtaskAlreadyClaimed {
        subtask_id: subtask_id("subtask_1"),
        held_by: session_token("session_other"),
    };
    assert!(matches!(
        already_claimed,
        CoveyError::SubtaskAlreadyClaimed { .. }
    ));

    let illegal_transition = CoveyError::IllegalTransition {
        from: StateValue::Subtask(SubtaskState::Claimed),
        to: StateValue::Subtask(SubtaskState::Claimed),
        object: ObjectType::Subtask,
    };
    assert!(matches!(
        meta_unavailable,
        CoveyError::MetaTaskUnavailable { .. }
    ));

    let already_claimed = CoveyError::SubtaskAlreadyClaimed {
        subtask_id: subtask_id("subtask_1"),
        held_by: session_token("session_other"),
    };
    assert!(matches!(
        already_claimed,
        CoveyError::SubtaskAlreadyClaimed { .. }
    ));

    let _illegal_transition = CoveyError::IllegalTransition {
        from: StateValue::Subtask(SubtaskState::Claimed),
        to: StateValue::Subtask(SubtaskState::Claimed),
        object: ObjectType::Subtask,
    };
    assert!(matches!(
        _illegal_transition,
        CoveyError::IllegalTransition { .. }
    ));

    let invalid_lease = CoveyError::InvalidLeaseDuration {
        field: "lease_duration_ms".to_owned(),
        provided: -1,
    };
    assert!(matches!(
        invalid_lease,
        CoveyError::InvalidLeaseDuration { .. }
    ));

    let idempotency_conflict = CoveyError::IdempotencyConflict {
        actor_key: "actor_1".to_owned(),
        operation: "claim_subtask".to_owned(),
        idempotency_key: "idem_1".to_owned(),
    };
    assert!(matches!(
        idempotency_conflict,
        CoveyError::IdempotencyConflict { .. }
    ));

    let errors: Vec<CoveyError> = vec![
        subtask_not_found,
        session_occupied,
        wrong_role,
        meta_unavailable,
        already_claimed,
        illegal_transition,
        invalid_lease,
        idempotency_conflict,
    ];

    assert_eq!(errors.len(), 8);
}

#[test]
fn claim_subtask_claims_known_available_work_subtask() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let worker = register(
        &covey,
        "worker-targeted-claim",
        "worker-targeted-claim",
        SessionRole::Executor,
    );

    let claim = covey
        .claim_subtask(ClaimSubtaskReq {
            session_token: worker.clone(),
            subtask_id: subtask_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-subtask"),
        })
        .expect("claim known subtask");

    assert_eq!(claim.subtask_id, subtask_id);
    assert!(claim.fence_seq > 0);

    let status = covey.subtask_status(&subtask_id).expect("subtask status");
    assert_eq!(status.subtask.state, SubtaskState::Claimed);
    assert_eq!(
        status.subtask.active_claim_id.as_deref(),
        Some(claim.claim_id.as_str())
    );
    let held_claim = status.claim.expect("held claim status");
    assert_eq!(held_claim.claim_id, claim.claim_id);
    assert_eq!(held_claim.owner_session_token, worker);
    assert_eq!(held_claim.fence_seq, claim.fence_seq);
    assert_eq!(held_claim.lease_deadline, claim.lease_deadline);

    let session_status = covey
        .session_status(&held_claim.owner_session_token)
        .expect("session status");
    assert_eq!(
        session_status
            .session
            .active_subtask_id()
            .map(|subtask_id| subtask_id.as_str()),
        Some(subtask_id.as_str())
    );

    let conn = Connection::open(&rig.db_path).expect("open db");
    let claim_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM claims", [], |row| row.get(0))
        .expect("claim count");
    assert_eq!(claim_count, 1);

    let claim_events = subtask_claim_events(&covey);
    assert_eq!(claim_events.len(), 1);
    assert_eq!(claim_events[0], claim);
}

#[test]
fn claim_subtask_claims_changes_requested_work_subtask() {
    let rig = Rig::new();
    let covey = rig.covey();
    let subtask_id = seed_changes_requested_work_subtask(&rig);
    let worker = register(
        &covey,
        "worker-changes-requested-claim",
        "worker-changes-requested-claim",
        SessionRole::Executor,
    );
    let before_events = count_subtask_claim_events(&covey);

    let claim = covey
        .claim_subtask(ClaimSubtaskReq {
            session_token: worker.clone(),
            subtask_id: subtask_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-subtask-changes-requested"),
        })
        .expect("claim changes requested subtask");

    assert_eq!(claim.subtask_id, subtask_id);
    assert_eq!(count_subtask_claim_events(&covey), before_events + 1);
    assert_eq!(
        covey
            .subtask_status(&claim.subtask_id)
            .expect("subtask status")
            .subtask
            .state,
        SubtaskState::Claimed
    );
    assert_eq!(
        covey
            .session_status(&worker)
            .expect("session status")
            .session
            .active_subtask_id()
            .map(|subtask_id| subtask_id.as_str()),
        Some(claim.subtask_id.as_str())
    );
}

#[test]
fn claim_subtask_rejects_wrong_role_for_work_and_review_targets() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, work_subtask_id) = seed_work_subtask(&rig);
    let reviewer = register(
        &covey,
        "reviewer-wrong-role",
        "reviewer-wrong-role",
        SessionRole::Reviewer,
    );
    let before_work_events = count_subtask_claim_events(&covey);

    assert!(matches!(
        covey.claim_subtask(ClaimSubtaskReq {
            session_token: reviewer.clone(),
            subtask_id: work_subtask_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-subtask-wrong-role-work"),
        }),
        Err(CoveyError::WrongRole { actual, .. }) if actual == SessionRole::Reviewer
    ));
    assert_eq!(count_subtask_claim_events(&covey), before_work_events);

    let executor = register(
        &covey,
        "executor-wrong-role",
        "executor-wrong-role",
        SessionRole::Executor,
    );
    let work_claim = covey
        .claim_subtask(ClaimSubtaskReq {
            session_token: executor.clone(),
            subtask_id: work_subtask_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-subtask-review-seed"),
        })
        .expect("claim work target");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: executor.clone(),
            claim_id: work_claim.claim_id.clone(),
            fence_seq: work_claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start work target");
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: executor.clone(),
            claim_id: work_claim.claim_id.clone(),
            fence_seq: work_claim.fence_seq,
            artifact_digest: "blake3:wrong_role_review_target".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "wrong_role_review_target.json".into(),
            changed_paths_digest: "blake3:wrong_role_review_target_paths".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish work target artifact");
    let review_id = covey
        .request_review(RequestReviewReq {
            session_token: executor.clone(),
            subtask_id: work_subtask_id.clone(),
            artifact_digest: "blake3:wrong_role_review_target".into(),
            review_subtask_id: Some("wrong_role_review_target_review".into()),
            priority: 1,
            idempotency_key: id_key("request-review"),
        })
        .expect("request review target");
    covey
        .release_claim(ReleaseClaimReq {
            session_token: executor.clone(),
            claim_id: work_claim.claim_id,
            fence_seq: work_claim.fence_seq,
            idempotency_key: id_key("release-claim"),
        })
        .expect("release work target claim");

    let review_subtask_id = covey
        .subtask_status(&work_subtask_id)
        .expect("work status")
        .reviews
        .into_iter()
        .find(|review| review.review_id() == review_id)
        .map(|review| review.review_subtask_id().to_owned())
        .expect("review subtask id");
    let before_review_events = count_subtask_claim_events(&covey);

    assert!(matches!(
        covey.claim_subtask(ClaimSubtaskReq {
            session_token: executor,
            subtask_id: review_subtask_id,
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-subtask-wrong-role-review"),
        }),
        Err(CoveyError::WrongRole { actual, .. }) if actual == SessionRole::Executor
    ));
    assert_eq!(count_subtask_claim_events(&covey), before_review_events);
}

#[test]
fn claim_subtask_rejects_terminal_meta_task() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (meta_task_id, subtask_id) = seed_work_subtask(&rig);
    let orch = register(
        &covey,
        "orch-terminal-meta",
        "orch-terminal-meta",
        SessionRole::Orchestrator,
    );
    let worker = register(
        &covey,
        "worker-terminal-meta",
        "worker-terminal-meta",
        SessionRole::Executor,
    );
    let before_events = count_subtask_claim_events(&covey);

    covey
        .cancel_meta_task(CancelMetaTaskReq {
            session_token: orch,
            meta_task_id: meta_task_id.clone(),
            idempotency_key: id_key("cancel-meta-task"),
        })
        .expect("cancel meta task");

    assert!(matches!(
        covey.claim_subtask(ClaimSubtaskReq {
            session_token: worker,
            subtask_id,
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-subtask-terminal-meta"),
        }),
        Err(CoveyError::MetaTaskUnavailable { meta_task_id: found_meta_task_id, state })
            if found_meta_task_id == meta_task_id && state == MetaTaskState::Cancelled
    ));
    assert_eq!(count_subtask_claim_events(&covey), before_events);
}

#[test]
fn claim_subtask_rejects_session_occupancy() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (meta_task_id, first_subtask_id) = seed_work_subtask(&rig);
    let orch = register(
        &covey,
        "orch-occupancy",
        "orch-occupancy",
        SessionRole::Orchestrator,
    );
    let second_subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch,
            meta_task_id,
            subtask_id: Some("work_2".into()),
            title: "second work".into(),
            priority: 2,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create second subtask");
    let worker = register(
        &covey,
        "worker-occupancy",
        "worker-occupancy",
        SessionRole::Executor,
    );
    let _first_claim = covey
        .claim_subtask(ClaimSubtaskReq {
            session_token: worker.clone(),
            subtask_id: first_subtask_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-subtask-first"),
        })
        .expect("claim first subtask");
    let before_events = count_subtask_claim_events(&covey);

    assert!(matches!(
        covey.claim_subtask(ClaimSubtaskReq {
            session_token: worker.clone(),
            subtask_id: second_subtask_id,
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-subtask-second"),
        }),
        Err(CoveyError::SessionAlreadyHasActiveSubtask { session_token, active_subtask_id })
            if session_token == worker && active_subtask_id == first_subtask_id
    ));
    assert_eq!(count_subtask_claim_events(&covey), before_events);
}

#[test]
fn claim_subtask_rejects_live_claim_conflict() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let first_worker = register(
        &covey,
        "worker-live-claim-a",
        "worker-live-claim-a",
        SessionRole::Executor,
    );
    let second_worker = register(
        &covey,
        "worker-live-claim-b",
        "worker-live-claim-b",
        SessionRole::Executor,
    );

    let _first_claim = covey
        .claim_subtask(ClaimSubtaskReq {
            session_token: first_worker.clone(),
            subtask_id: subtask_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-subtask-live-claim-a"),
        })
        .expect("first claim succeeds");
    let before_events = count_subtask_claim_events(&covey);

    assert!(matches!(
        covey.claim_subtask(ClaimSubtaskReq {
            session_token: second_worker,
            subtask_id: subtask_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-subtask-live-claim-b"),
        }),
        Err(CoveyError::SubtaskAlreadyClaimed { subtask_id: found_subtask_id, held_by })
            if found_subtask_id == subtask_id && held_by == first_worker
    ));
    assert_eq!(count_subtask_claim_events(&covey), before_events);
}

#[test]
fn claim_subtask_expires_stale_held_claim_on_target() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let first_worker = register(
        &covey,
        "worker-stale-target-a",
        "worker-stale-target-a",
        SessionRole::Executor,
    );
    let second_worker = register(
        &covey,
        "worker-stale-target-b",
        "worker-stale-target-b",
        SessionRole::Executor,
    );

    let first_claim = covey
        .claim_subtask(ClaimSubtaskReq {
            session_token: first_worker.clone(),
            subtask_id: subtask_id.clone(),
            lease_duration_ms: 5,
            idempotency_key: id_key("claim-subtask-stale-a"),
        })
        .expect("first targeted claim");
    let before_reclaim_events = count_subtask_claim_events(&covey);

    rig.tick(6);
    let second_claim = covey
        .claim_subtask(ClaimSubtaskReq {
            session_token: second_worker.clone(),
            subtask_id: subtask_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-subtask-stale-b"),
        })
        .expect("reclaim after stale expiry");

    assert_eq!(
        count_subtask_claim_events(&covey),
        before_reclaim_events + 1
    );
    assert_eq!(second_claim.subtask_id, subtask_id);
    assert!(second_claim.fence_seq > first_claim.fence_seq);
    let stale_session = covey
        .session_status(&first_worker)
        .expect("stale owner session");
    assert!(stale_session.session.active_subtask_id().is_none());
    let fresh_session = covey
        .session_status(&second_worker)
        .expect("fresh owner session");
    assert_eq!(
        fresh_session
            .session
            .active_subtask_id()
            .map(|subtask_id| subtask_id.as_str()),
        Some(second_claim.subtask_id.as_str())
    );

    let conn = Connection::open(&rig.db_path).expect("open db");
    let stale_claim_state: String = conn
        .query_row(
            "SELECT state FROM claims WHERE claim_id = ?1",
            params![first_claim.claim_id],
            |row| row.get(0),
        )
        .expect("stale claim state");
    assert_eq!(stale_claim_state, "expired");
}

#[test]
fn claim_subtask_is_idempotent() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let worker = register(
        &covey,
        "worker-idempotent-claim",
        "worker-idempotent-claim",
        SessionRole::Executor,
    );
    let idempotency_key = id_key("claim-subtask-idempotent");

    let first = covey
        .claim_subtask(ClaimSubtaskReq {
            session_token: worker.clone(),
            subtask_id: subtask_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: idempotency_key.clone(),
        })
        .expect("initial targeted claim");
    let events_after_first = count_subtask_claim_events(&covey);

    let replay = covey
        .claim_subtask(ClaimSubtaskReq {
            session_token: worker.clone(),
            subtask_id,
            lease_duration_ms: 30_000,
            idempotency_key,
        })
        .expect("idempotent replay");

    assert_eq!(replay, first);
    assert_eq!(count_subtask_claim_events(&covey), events_after_first);

    let conn = Connection::open(&rig.db_path).expect("open db");
    let claim_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM claims", [], |row| row.get(0))
        .expect("claim count");
    assert_eq!(claim_count, 1);
}

#[test]
fn claim_subtask_rejects_idempotency_key_payload_drift() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let worker = register(
        &covey,
        "worker-idempotency-drift",
        "worker-idempotency-drift",
        SessionRole::Executor,
    );
    let idempotency_key = id_key("claim-subtask-stable-key");

    let _initial_claim = covey
        .claim_subtask(ClaimSubtaskReq {
            session_token: worker.clone(),
            subtask_id,
            lease_duration_ms: 30_000,
            idempotency_key: idempotency_key.clone(),
        })
        .expect("initial targeted claim");
    let before_events = count_subtask_claim_events(&covey);

    assert!(matches!(
        covey.claim_subtask(ClaimSubtaskReq {
            session_token: worker.clone(),
            subtask_id: "work_1".into(),
            lease_duration_ms: 60_000,
            idempotency_key: idempotency_key.clone(),
        }),
        Err(CoveyError::IdempotencyConflict { actor_key, operation, idempotency_key: conflict_key })
            if actor_key == worker && operation == "claim_subtask" && conflict_key == idempotency_key
    ));
    assert_eq!(count_subtask_claim_events(&covey), before_events);
}

#[test]
fn claim_subtask_rejects_illegal_state_without_appending_event() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let first_worker = register(
        &covey,
        "worker-illegal-state-a",
        "worker-illegal-state-a",
        SessionRole::Executor,
    );
    let second_worker = register(
        &covey,
        "worker-illegal-state-b",
        "worker-illegal-state-b",
        SessionRole::Executor,
    );

    let claim = covey
        .claim_subtask(ClaimSubtaskReq {
            session_token: first_worker.clone(),
            subtask_id: subtask_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-subtask-illegal-state-seed"),
        })
        .expect("seed claim");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: first_worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start seeded subtask");
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: first_worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:illegal_state_target".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "illegal_state_target.json".into(),
            changed_paths_digest: "blake3:illegal_state_target_paths".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish seeded artifact");
    covey
        .release_claim(ReleaseClaimReq {
            session_token: first_worker,
            claim_id: claim.claim_id,
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("release-claim"),
        })
        .expect("release seeded claim");
    let before_events = count_subtask_claim_events(&covey);

    assert!(matches!(
        covey.claim_subtask(ClaimSubtaskReq {
            session_token: second_worker,
            subtask_id,
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-subtask-illegal-state"),
        }),
        Err(CoveyError::IllegalTransition { from, to, object })
            if from == SubtaskState::ArtifactPublished.into()
                && to == SubtaskState::Claimed.into()
                && object == ObjectType::Subtask
    ));
    assert_eq!(count_subtask_claim_events(&covey), before_events);
}

#[test]
fn claim_subtask_matches_claim_next_for_changes_requested_work() {
    let targeted_rig = Rig::new();
    let targeted_covey = targeted_rig.covey();
    let targeted_subtask_id = seed_changes_requested_work_subtask(&targeted_rig);
    let targeted_worker = register(
        &targeted_covey,
        "worker-targeted-equivalence",
        "worker-targeted-equivalence",
        SessionRole::Executor,
    );
    let targeted_before_events = count_subtask_claim_events(&targeted_covey);

    let next_rig = Rig::new();
    let next_covey = next_rig.covey();
    let next_subtask_id = seed_changes_requested_work_subtask(&next_rig);
    let next_worker = register(
        &next_covey,
        "worker-next-equivalence",
        "worker-next-equivalence",
        SessionRole::Executor,
    );
    let next_before_events = count_subtask_claim_events(&next_covey);

    let targeted = targeted_covey
        .claim_subtask(ClaimSubtaskReq {
            session_token: targeted_worker.clone(),
            subtask_id: targeted_subtask_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-subtask-equivalence"),
        })
        .expect("targeted changes requested claim");
    let next = next_covey
        .claim_next_subtask(ClaimNextReq {
            session_token: next_worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next-equivalence"),
        })
        .expect("next claim result")
        .expect("next claim payload");

    assert_eq!(targeted.subtask_id, targeted_subtask_id);
    assert_eq!(next.subtask_id, next_subtask_id);
    assert_eq!(targeted.fence_seq, next.fence_seq);
    assert_eq!(targeted.lease_deadline, next.lease_deadline);
    assert_eq!(
        count_subtask_claim_events(&targeted_covey),
        targeted_before_events + 1
    );
    assert_eq!(
        count_subtask_claim_events(&next_covey),
        next_before_events + 1
    );
    assert_eq!(
        targeted_covey
            .subtask_status(&targeted_subtask_id)
            .expect("targeted status")
            .subtask
            .state,
        next_covey
            .subtask_status(&next_subtask_id)
            .expect("next status")
            .subtask
            .state
    );
    assert_eq!(
        targeted_covey
            .session_status(&targeted_worker)
            .expect("targeted worker status")
            .session
            .active_subtask_id()
            .cloned(),
        next_covey
            .session_status(&next_worker)
            .expect("next worker status")
            .session
            .active_subtask_id()
            .cloned()
    );
}

#[test]
fn claim_subtask_mixed_path_race_single_winner() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let targeted_worker = register(
        &covey,
        "worker-targeted-race",
        "worker-targeted-race",
        SessionRole::Executor,
    );
    let next_worker = register(
        &covey,
        "worker-next-race",
        "worker-next-race",
        SessionRole::Executor,
    );

    let barrier = Arc::new(Barrier::new(3));
    let targeted_db = rig.db_path.clone();
    let targeted_clock = rig.clock.clone();
    let targeted_barrier = barrier.clone();
    let targeted_subtask_id = subtask_id.clone();
    let targeted_session = targeted_worker.clone();
    let targeted_handle = thread::spawn(move || {
        let covey =
            Covey::open_with_clock(targeted_db, targeted_clock).expect("open targeted covey");
        targeted_barrier.wait();
        covey.claim_subtask(ClaimSubtaskReq {
            session_token: targeted_session,
            subtask_id: targeted_subtask_id,
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-subtask-race"),
        })
    });

    let next_db = rig.db_path.clone();
    let next_clock = rig.clock.clone();
    let next_barrier = barrier.clone();
    let next_session = next_worker.clone();
    let next_handle = thread::spawn(move || {
        let covey = Covey::open_with_clock(next_db, next_clock).expect("open next covey");
        next_barrier.wait();
        covey.claim_next_subtask(ClaimNextReq {
            session_token: next_session,
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next-race"),
        })
    });

    barrier.wait();
    let targeted_result = targeted_handle.join().expect("join targeted thread");
    let next_result = next_handle.join().expect("join next thread");

    let targeted_claim = targeted_result.ok();
    let next_claim = match next_result {
        Ok(claim) => claim,
        Err(err) => {
            assert!(matches!(err, CoveyError::SubtaskAlreadyClaimed { .. }));
            None
        }
    };

    let winner = match (targeted_claim, next_claim) {
        (Some(targeted), None) => targeted,
        (None, Some(next)) => {
            assert_eq!(next.subtask_id, subtask_id);
            next
        }
        (Some(_), Some(_)) => panic!("only one claim path may win"),
        (None, None) => panic!("one claim path must win"),
    };

    let status = covey
        .subtask_status(&subtask_id)
        .expect("subtask status after race");
    assert_eq!(status.subtask.state, SubtaskState::Claimed);
    assert_eq!(
        status.subtask.active_claim_id.as_deref(),
        Some(winner.claim_id.as_str())
    );
    assert_eq!(
        status.claim.as_ref().map(|claim| claim.claim_id.as_str()),
        Some(winner.claim_id.as_str())
    );

    let conn = Connection::open(&rig.db_path).expect("open db");
    let claim_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM claims", [], |row| row.get(0))
        .expect("claim count after race");
    assert_eq!(claim_count, 1);

    let claim_events = subtask_claim_events(&covey);
    assert_eq!(claim_events.len(), 1);
    assert_eq!(claim_events[0], winner);
}

#[test]
fn pending_reviews_are_superseded_when_new_artifact_is_published() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);

    let claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start");
    rig.tick(1);
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:a".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "a.json".into(),
            changed_paths_digest: "blake3:paths_a".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish a");
    rig.tick(1);
    let review_id = covey
        .request_review(RequestReviewReq {
            session_token: worker.clone(),
            subtask_id: subtask_id.clone(),
            artifact_digest: "blake3:a".into(),
            review_subtask_id: Some("review_a".into()),
            priority: 2,
            idempotency_key: id_key("request-review"),
        })
        .expect("request review");

    rig.tick(1);
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id,
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:b".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "b.json".into(),
            changed_paths_digest: "blake3:paths_b".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish b");

    let status = covey.subtask_status(&subtask_id).expect("status");
    assert_eq!(status.subtask.artifact_digest.as_deref(), Some("blake3:b"));
    let review = status
        .reviews
        .into_iter()
        .find(|review| review.review_id() == review_id)
        .expect("review exists");
    assert_eq!(review.state(), covey::ReviewState::Superseded);
}

#[test]
fn deciding_review_for_old_artifact_does_not_bless_new_artifact() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);
    let reviewer = register(&covey, "reviewer", "reviewer", SessionRole::Reviewer);

    let claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start");
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:a".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "a.json".into(),
            changed_paths_digest: "blake3:paths_a".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish a");
    let review_id = covey
        .request_review(RequestReviewReq {
            session_token: worker.clone(),
            subtask_id: subtask_id.clone(),
            artifact_digest: "blake3:a".into(),
            review_subtask_id: Some("review_stale".into()),
            priority: 2,
            idempotency_key: id_key("request-review"),
        })
        .expect("request review");
    rig.tick(1);
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:b".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "b.json".into(),
            changed_paths_digest: "blake3:paths_b".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish b");
    covey
        .release_claim(ReleaseClaimReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id,
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("release-claim"),
        })
        .expect("release worker");

    let review_subtask_id = covey
        .subtask_status(&subtask_id)
        .expect("status")
        .reviews
        .into_iter()
        .find(|review| review.review_id() == review_id)
        .map(|review| review.review_subtask_id().to_owned())
        .expect("review subtask");

    let review_claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: reviewer.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim review")
        .expect("review claim");
    assert_eq!(review_claim.subtask_id, review_subtask_id);
    assert!(matches!(
        covey.start_subtask(StartSubtaskReq {
            session_token: reviewer.clone(),
            claim_id: review_claim.claim_id.clone(),
            fence_seq: review_claim.fence_seq,
        idempotency_key: id_key("start-subtask"),
        }),
        Err(CoveyError::IllegalTransition { object, .. }) if object == ObjectType::Review
    ));

    let status = covey.subtask_status(&subtask_id).expect("status");
    assert_eq!(status.subtask.artifact_digest.as_deref(), Some("blake3:b"));
    assert_eq!(status.subtask.state, SubtaskState::ArtifactPublished);
    assert_eq!(
        covey
            .subtask_status(&review_subtask_id)
            .expect("review subtask status")
            .subtask
            .state,
        SubtaskState::Claimed
    );
}

#[test]
fn ready_queue_orders_items_and_applies_in_order() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(&covey, "orch", "orch", SessionRole::Orchestrator);
    let gate = register(&covey, "gate", "gate", SessionRole::ApplyGate);

    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "queue".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("meta task");
    let first = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id: meta_task_id.clone(),
            subtask_id: Some("ready_1".into()),
            title: "first".into(),
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask1");
    let second = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id,
            subtask_id: Some("ready_2".into()),
            title: "second".into(),
            priority: 2,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask2");

    let mut review_records = Vec::new();
    for (subtask_id, digest, created_at) in
        [(&first, "blake3:q1", 1_i64), (&second, "blake3:q2", 2_i64)]
    {
        let worker_alias = format!("worker_{digest}");
        let worker = register(&covey, &worker_alias, &worker_alias, SessionRole::Executor);
        attest(&covey, &worker);
        let claim = covey
            .claim_next_subtask(ClaimNextReq {
                session_token: worker.clone(),
                lease_duration_ms: 30_000,
                idempotency_key: id_key("claim-next"),
            })
            .expect("claim")
            .expect("claim result");
        covey
            .start_subtask(StartSubtaskReq {
                session_token: worker.clone(),
                claim_id: claim.claim_id.clone(),
                fence_seq: claim.fence_seq,
                idempotency_key: id_key("start-subtask"),
            })
            .expect("start");
        covey
            .publish_artifact(PublishArtifactReq {
                session_token: worker.clone(),
                claim_id: claim.claim_id.clone(),
                fence_seq: claim.fence_seq,
                artifact_digest: digest.into(),
                artifact_kind: ArtifactKind::PatchBundle,
                base_rev: "base".into(),
                manifest_path: format!("{digest}.json"),
                changed_paths_digest: format!("blake3:paths_{created_at}"),
                idempotency_key: id_key("publish-artifact"),
            })
            .expect("publish");
        let review_id = covey
            .request_review(RequestReviewReq {
                session_token: worker.clone(),
                subtask_id: subtask_id.to_string(),
                artifact_digest: digest.into(),
                review_subtask_id: Some(format!("review_{digest}")),
                priority: 1,
                idempotency_key: id_key("request-review"),
            })
            .expect("review req");
        covey
            .release_claim(ReleaseClaimReq {
                session_token: worker.clone(),
                claim_id: claim.claim_id,
                fence_seq: claim.fence_seq,
                idempotency_key: id_key("release-claim"),
            })
            .expect("release");
        let reviewer_alias = format!("reviewer_{digest}");
        let reviewer = register(
            &covey,
            &reviewer_alias,
            &reviewer_alias,
            SessionRole::Reviewer,
        );
        attest(&covey, &reviewer);
        let review_claim = covey
            .claim_next_subtask(ClaimNextReq {
                session_token: reviewer.clone(),
                lease_duration_ms: 30_000,
                idempotency_key: id_key("claim-next"),
            })
            .expect("claim review")
            .expect("review claim");
        covey
            .start_subtask(StartSubtaskReq {
                session_token: reviewer.clone(),
                claim_id: review_claim.claim_id.clone(),
                fence_seq: review_claim.fence_seq,
                idempotency_key: id_key("start-subtask"),
            })
            .expect("start review");
        covey
            .decide_review(DecideReviewReq {
                session_token: reviewer,
                review_id: review_id.clone(),
                claim_id: review_claim.claim_id,
                fence_seq: review_claim.fence_seq,
                verdict: covey::ReviewVerdict::Approve,
                findings_digest: "blake3:findings".into(),
                idempotency_key: id_key("decide-review"),
            })
            .expect("decide");
        covey
            .enqueue_for_apply(EnqueueForApplyReq {
                session_token: orch.clone(),
                artifact_digest: digest.into(),
                subtask_id: subtask_id.to_string(),
                settlement_target: SettlementTarget::Canonical,
                idempotency_key: id_key("enqueue-for-apply"),
            })
            .expect("enqueue");
        review_records.push((subtask_id.to_string(), digest.to_string(), review_id));
        rig.tick(1);
    }

    let queued = covey.fetch_ready_queue(10).expect("fetch queue");
    assert_eq!(queued.len(), 2);
    assert_eq!(queued[0].subtask_id(), first);
    assert_eq!(queued[1].subtask_id(), second);

    let queue_claim = covey
        .claim_next_ready_queue_item(ClaimReadyQueueReq {
            session_token: gate.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-ready-queue"),
        })
        .expect("claim ready queue")
        .expect("queue claim");
    let (_, digest, review_id) = review_records
        .iter()
        .find(|(subtask_id, _, _)| subtask_id == &queue_claim.subtask_id)
        .expect("review record for queue claim");
    record_apply_verification(
        &covey,
        &gate,
        &queue_claim.queue_id,
        digest,
        review_id,
        "blake3:findings",
        queue_claim.claim_fence_seq,
    );
    covey
        .mark_applied(MarkAppliedReq {
            session_token: gate.clone(),
            queue_id: queue_claim.queue_id,
            claim_fence_seq: queue_claim.claim_fence_seq,
            idempotency_key: id_key("mark-applied"),
        })
        .expect("applied");
    assert_eq!(
        covey.subtask_status(&first).expect("status").subtask.state,
        SubtaskState::Applied
    );
}

#[test]
fn expired_ready_queue_claims_are_requeued_for_the_next_apply_gate() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(&covey, "orch", "orch", SessionRole::Orchestrator);
    let gate_a = register(&covey, "gate_a", "gate_a", SessionRole::ApplyGate);
    let gate_b = register(&covey, "gate_b", "gate_b", SessionRole::ApplyGate);
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);
    let reviewer = register(&covey, "reviewer", "reviewer", SessionRole::Reviewer);
    attest(&covey, &worker);
    attest(&covey, &reviewer);

    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "queue reclaim".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("meta");
    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id,
            subtask_id: Some("queue_reclaim".into()),
            title: "queue reclaim".into(),
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask");
    let claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start");
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:queue_reclaim".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "queue_reclaim.json".into(),
            changed_paths_digest: "blake3:paths_queue_reclaim".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish");
    let review_id = covey
        .request_review(RequestReviewReq {
            session_token: worker.clone(),
            subtask_id: subtask_id.clone(),
            artifact_digest: "blake3:queue_reclaim".into(),
            review_subtask_id: Some("review_queue_reclaim".into()),
            priority: 1,
            idempotency_key: id_key("request-review"),
        })
        .expect("review req");
    covey
        .release_claim(ReleaseClaimReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id,
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("release-claim"),
        })
        .expect("release");
    let review_claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: reviewer.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim review")
        .expect("review claim");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: reviewer.clone(),
            claim_id: review_claim.claim_id.clone(),
            fence_seq: review_claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start review");
    covey
        .decide_review(DecideReviewReq {
            session_token: reviewer,
            review_id: review_id.clone(),
            claim_id: review_claim.claim_id,
            fence_seq: review_claim.fence_seq,
            verdict: covey::ReviewVerdict::Approve,
            findings_digest: "blake3:findings_queue_reclaim".into(),
            idempotency_key: id_key("decide-review"),
        })
        .expect("approve");
    covey
        .enqueue_for_apply(EnqueueForApplyReq {
            session_token: orch,
            artifact_digest: "blake3:queue_reclaim".into(),
            subtask_id: subtask_id.clone(),
            settlement_target: SettlementTarget::Canonical,
            idempotency_key: id_key("enqueue-for-apply"),
        })
        .expect("enqueue");

    let first_claim = covey
        .claim_next_ready_queue_item(ClaimReadyQueueReq {
            session_token: gate_a.clone(),
            lease_duration_ms: 10_000,
            idempotency_key: id_key("claim-ready-queue"),
        })
        .expect("claim ready queue")
        .expect("first queue claim");
    rig.tick(10_001);

    assert!(matches!(
        covey.mark_applied(MarkAppliedReq {
            session_token: gate_a,
            queue_id: first_claim.queue_id.clone(),
            claim_fence_seq: first_claim.claim_fence_seq,
            idempotency_key: id_key("mark-applied"),
        }),
        Err(CoveyError::LeaseExpired { object_id }) if object_id == first_claim.queue_id
    ));

    let second_claim = covey
        .claim_next_ready_queue_item(ClaimReadyQueueReq {
            session_token: gate_b.clone(),
            lease_duration_ms: 10_000,
            idempotency_key: id_key("claim-ready-queue"),
        })
        .expect("reclaim ready queue")
        .expect("second queue claim");
    assert_eq!(second_claim.queue_id, first_claim.queue_id);
    assert!(second_claim.claim_fence_seq > first_claim.claim_fence_seq);

    record_apply_verification(
        &covey,
        &gate_b,
        &second_claim.queue_id,
        "blake3:queue_reclaim",
        &review_id,
        "blake3:findings_queue_reclaim",
        second_claim.claim_fence_seq,
    );
    covey
        .mark_applied(MarkAppliedReq {
            session_token: gate_b,
            queue_id: second_claim.queue_id,
            claim_fence_seq: second_claim.claim_fence_seq,
            idempotency_key: id_key("mark-applied"),
        })
        .expect("applied after reclaim");
    assert_eq!(
        covey
            .subtask_status(&subtask_id)
            .expect("status")
            .subtask
            .state,
        SubtaskState::Applied
    );
}

#[test]
fn reservation_overlap_query_only_returns_active_rows() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let orch = register(
        &covey,
        "orch_extra",
        "orch_extra",
        SessionRole::Orchestrator,
    );
    let active = covey
        .request_reservation(RequestReservationReq {
            session_token: orch.clone(),
            owner_subtask_id: subtask_id.clone(),
            scope_class: ScopeClass::Subtree,
            scope_key: "src/covey".into(),
            generated_members: vec![],
            lease_duration_ms: 60_000,
            idempotency_key: id_key("request-reservation"),
        })
        .expect("active reservation");
    rig.tick(1);
    let released = covey
        .request_reservation(RequestReservationReq {
            session_token: orch.clone(),
            owner_subtask_id: subtask_id,
            scope_class: ScopeClass::RepoGlobal,
            scope_key: "repo".into(),
            generated_members: vec![],
            lease_duration_ms: 60_000,
            idempotency_key: id_key("request-reservation"),
        })
        .expect("released reservation");
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);
    assert!(matches!(
        covey.release_reservation(ReleaseReservationReq {
            session_token: worker.clone(),
            reservation_id: released.clone(),
        idempotency_key: id_key("release-reservation"),
        }),
        Err(CoveyError::WrongRole { actual, .. }) if actual == SessionRole::Executor
    ));
    covey
        .release_reservation(ReleaseReservationReq {
            session_token: orch.clone(),
            reservation_id: released,
            idempotency_key: id_key("release-reservation"),
        })
        .expect("release reservation");

    let overlaps = covey
        .find_overlapping_reservations(OverlapQueryReq {
            scope_class: ScopeClass::ExactPath,
            scope_key: "src/covey/store.rs".into(),
            generated_members: vec![],
        })
        .expect("overlaps");
    assert_eq!(overlaps.len(), 1);
    assert_eq!(overlaps[0].reservation_id, active);
}

#[test]
fn reservation_renewal_extends_the_active_lease() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let orch = register(
        &covey,
        "orch_renew",
        "orch_renew",
        SessionRole::Orchestrator,
    );

    let reservation_id = covey
        .request_reservation(RequestReservationReq {
            session_token: orch.clone(),
            owner_subtask_id: subtask_id,
            scope_class: ScopeClass::ExactPath,
            scope_key: "src/covey/store.rs".into(),
            generated_members: vec![],
            lease_duration_ms: 10_000,
            idempotency_key: id_key("request-reservation"),
        })
        .expect("reservation");
    let original = covey
        .find_overlapping_reservations(OverlapQueryReq {
            scope_class: ScopeClass::ExactPath,
            scope_key: "src/covey/store.rs".into(),
            generated_members: vec![],
        })
        .expect("overlaps")
        .into_iter()
        .find(|reservation| reservation.reservation_id == reservation_id)
        .expect("reservation row");

    rig.tick(5_000);
    let renewed = covey
        .renew_reservation(RenewReservationReq {
            session_token: orch.clone(),
            reservation_id: reservation_id.clone(),
            extend_by_ms: 20_000,
            idempotency_key: id_key("renew-reservation"),
        })
        .expect("renew reservation");
    assert_eq!(renewed.lease_deadline, original.lease_deadline + 20_000);

    rig.tick(6_000);
    let expire_before = covey
        .expire_old_reservations()
        .expect("expire before renewed");
    assert_eq!(expire_before.expired_count, 0);

    rig.tick(20_000);
    let expire_after = covey
        .expire_old_reservations()
        .expect("expire after renewed");
    assert_eq!(expire_after.expired_count, 1);
}

#[test]
fn overlapping_reservations_surface_open_typed_conflicts() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (meta_task_id, first_subtask_id) = seed_work_subtask(&rig);
    let orch = register(
        &covey,
        "orch_extra",
        "orch_extra",
        SessionRole::Orchestrator,
    );
    let second_subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id,
            subtask_id: Some("shadow_work".into()),
            title: "shadow work".into(),
            priority: 2,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create second subtask");

    let first_reservation = covey
        .request_reservation(RequestReservationReq {
            session_token: orch.clone(),
            owner_subtask_id: first_subtask_id.clone(),
            scope_class: ScopeClass::Subtree,
            scope_key: "src/covey".into(),
            generated_members: vec![],
            lease_duration_ms: 60_000,
            idempotency_key: id_key("request-reservation"),
        })
        .expect("first reservation");
    rig.tick(1);
    let second_reservation = covey
        .request_reservation(RequestReservationReq {
            session_token: orch.clone(),
            owner_subtask_id: second_subtask_id.clone(),
            scope_class: ScopeClass::ExactPath,
            scope_key: "src/covey/store.rs".into(),
            generated_members: vec![],
            lease_duration_ms: 60_000,
            idempotency_key: id_key("request-reservation"),
        })
        .expect("second reservation");

    let conflicts = covey.list_conflicts().expect("list conflicts");
    let conflict = conflicts
        .iter()
        .find(|conflict| {
            conflict.conflict_kind == "reservation_overlap"
                && conflict.object_type == ObjectType::Reservation
        })
        .expect("reservation overlap conflict");
    let payload: ReservationOverlapConflictPayload =
        serde_json::from_str(&conflict.payload_json).expect("typed conflict payload");

    assert_eq!(
        conflict.resolution_state,
        covey::ConflictResolutionState::Open
    );
    assert_eq!(payload.reservation_id, second_reservation);
    assert_eq!(payload.overlapping_reservation_id, first_reservation);
    assert_eq!(payload.owner_subtask_id, second_subtask_id);
    assert_eq!(payload.overlapping_owner_subtask_id, first_subtask_id);
    assert_eq!(payload.scope_class, ScopeClass::ExactPath);
    assert_eq!(payload.scope_key, "src/covey/store.rs");
    assert_eq!(payload.overlapping_scope_class, ScopeClass::Subtree);
    assert_eq!(payload.overlapping_scope_key, "src/covey");
}

#[test]
fn concurrent_pool_claims_distribute_exactly_once() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(&covey, "orch", "orch", SessionRole::Orchestrator);
    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "parallel".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("meta task");
    let mut session_tokens = Vec::new();
    for idx in 0..10 {
        covey
            .create_subtask(CreateSubtaskRequest {
                session_token: orch.clone(),
                meta_task_id: meta_task_id.clone(),
                subtask_id: Some(format!("task_{idx}")),
                title: format!("task {idx}"),
                priority: idx,
                idempotency_key: id_key("create-subtask"),
            })
            .expect("create subtask");
        let session = format!("worker_{idx}");
        session_tokens.push(register(&covey, &session, &session, SessionRole::Executor));
    }

    let mut handles = Vec::new();
    for session_token in session_tokens {
        let db = rig.db_path.clone();
        let clock = rig.clock.clone();
        handles.push(thread::spawn(move || {
            let covey = Covey::open_with_clock(db, clock).expect("open worker covey");
            covey
                .claim_next_subtask(ClaimNextReq {
                    session_token,
                    lease_duration_ms: 30_000,
                    idempotency_key: id_key("claim-next"),
                })
                .expect("claim call")
                .map(|claim| claim.subtask_id)
        }));
    }

    let claimed = handles
        .into_iter()
        .map(|handle| handle.join().expect("join"))
        .collect::<Vec<_>>();
    assert_eq!(claimed.iter().filter(|item| item.is_some()).count(), 10);
    let unique = claimed.into_iter().flatten().collect::<HashSet<_>>();
    assert_eq!(unique.len(), 10);
}

#[test]
fn concurrent_claim_on_single_subtask_has_exactly_one_winner() {
    let rig = Rig::new();
    let covey = rig.covey();
    seed_work_subtask(&rig);
    let mut session_tokens = Vec::new();
    for idx in 0..10 {
        let session = format!("worker_{idx}");
        session_tokens.push(register(&covey, &session, &session, SessionRole::Executor));
    }

    let mut handles = Vec::new();
    for session_token in session_tokens {
        let db = rig.db_path.clone();
        let clock = rig.clock.clone();
        handles.push(thread::spawn(move || {
            let covey = Covey::open_with_clock(db, clock).expect("open worker covey");
            covey
                .claim_next_subtask(ClaimNextReq {
                    session_token,
                    lease_duration_ms: 30_000,
                    idempotency_key: id_key("claim-next"),
                })
                .ok()
                .flatten()
                .map(|claim| claim.claim_id)
        }));
    }

    let winners = handles
        .into_iter()
        .filter_map(|handle| handle.join().expect("join"))
        .collect::<Vec<_>>();
    assert_eq!(winners.len(), 1);
}

#[test]
fn stale_reap_immediately_expires_orphaned_claims_and_clears_session_and_subtask() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);

    let _claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim")
        .expect("claim result");

    rig.tick(60_000);
    let _reap_result = covey.reap_stale_sessions(45_000).expect("reap stale");

    let stale_session = covey.session_status(&worker).expect("session");
    assert_eq!(stale_session.session.state(), SessionState::Stale);
    assert!(stale_session.session.active_subtask_id().is_none());
    let before_expire = covey.subtask_status(&subtask_id).expect("subtask");
    assert!(before_expire.claim.is_none());
    assert_eq!(before_expire.subtask.state, SubtaskState::Available);

    let _ = covey.expire_old_claims().expect("expire claims");

    let after_expire = covey.subtask_status(&subtask_id).expect("subtask");
    assert!(after_expire.claim.is_none());
    assert_eq!(after_expire.subtask.state, SubtaskState::Available);
    let session = covey.session_status(&worker).expect("session after expire");
    assert_eq!(session.session.state(), SessionState::Stale);
    assert!(session.session.active_subtask_id().is_none());
}

#[test]
fn event_log_windows_are_strictly_monotonic_and_decode_to_typed_payloads() {
    let rig = Rig::new();
    let covey = rig.covey();

    let sess = register(&covey, "sess", "principal", SessionRole::Executor);
    rig.tick(1);
    send_heartbeat(&covey, &sess).expect("heartbeat");
    rig.tick(1);
    close_session(&covey, &sess).expect("exit");

    let events = covey.fetch_events(0, 10).expect("all events");
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].seq, 1);
    assert_eq!(events[1].seq, 2);
    assert_eq!(events[2].seq, 3);
    assert_eq!(events[0].event_type, EventType::SessionRegistered);
    assert_eq!(events[0].object_type, ObjectType::Session);
    assert_eq!(events[0].actor_kind, ActorKind::Session);
    assert_eq!(events[0].session_token.as_deref(), Some(sess.as_str()));
    assert_eq!(events[1].actor_kind, ActorKind::Session);
    assert_eq!(events[1].session_token.as_deref(), Some(sess.as_str()));
    assert_eq!(events[2].actor_kind, ActorKind::Session);
    assert_eq!(events[2].session_token.as_deref(), Some(sess.as_str()));

    let typed = events
        .iter()
        .map(covey::Event::typed)
        .collect::<Result<Vec<_>, _>>()
        .expect("typed events");
    assert!(matches!(
        &typed[0].payload,
        EventPayload::SessionRegistered(req) if req.session_token == sess
    ));
    assert!(matches!(
        &typed[1].payload,
        EventPayload::SessionHeartbeat(payload) if payload.session_token == sess
    ));
    assert!(matches!(
        &typed[2].payload,
        EventPayload::SessionExited(payload) if payload.session_token == sess
    ));

    let after_first = covey.fetch_events(1, 10).expect("windowed events");
    assert_eq!(after_first.len(), 2);
    assert!(after_first.iter().all(|event| event.seq > 1));
}

#[test]
fn observability_queries_report_stuck_work_expiring_claims_and_queue_metrics() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_meta_task_id, subtask_id) = seed_work_subtask(&rig);
    let gate = register(&covey, "gate", "gate", SessionRole::ApplyGate);
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);

    let claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start");
    rig.tick(5_000);

    let stuck = covey.list_stuck_subtasks(1_000, 10).expect("stuck");
    assert_eq!(stuck.len(), 1);
    assert_eq!(stuck[0].subtask.subtask_id, subtask_id);
    assert_eq!(
        stuck[0]
            .session
            .as_ref()
            .expect("stuck session")
            .session_token,
        worker
    );

    let expiring = covey.list_expiring_claims(30_000, 10).expect("expiring");
    assert_eq!(expiring.len(), 1);
    assert_eq!(expiring[0].claim.claim_id, claim.claim_id);
    assert_eq!(expiring[0].subtask.subtask_id, subtask_id);

    let conn = Connection::open(&rig.db_path).expect("open db");
    conn.execute(
        "INSERT INTO artifacts (
            artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session,
            manifest_path, changed_paths_digest, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            "blake3:ready_metrics",
            ArtifactKind::PatchBundle.to_string(),
            "base",
            subtask_id.clone(),
            worker.clone(),
            "metrics.json",
            "blake3:metrics_paths",
            1_700_000_005_000_i64,
        ],
    )
    .expect("insert artifact");
    conn.execute(
        "UPDATE subtasks SET state = ?2, artifact_digest = ?3 WHERE subtask_id = ?1",
        params![
            subtask_id,
            SubtaskState::Approved.to_string(),
            "blake3:ready_metrics"
        ],
    )
    .expect("approve subtask");
    let queue_id = covey
        .enqueue_for_apply(EnqueueForApplyReq {
            session_token: register(
                &covey,
                "orch_metrics",
                "orch_metrics",
                SessionRole::Orchestrator,
            ),
            artifact_digest: "blake3:ready_metrics".into(),
            subtask_id,
            settlement_target: SettlementTarget::Canonical,
            idempotency_key: id_key("enqueue-for-apply"),
        })
        .expect("enqueue");
    let _ = covey
        .mark_in_flight(MarkInFlightReq {
            session_token: gate,
            queue_id,
            lease_duration_ms: 30_000,
            idempotency_key: id_key("mark-in-flight"),
        })
        .expect("flight");

    let metrics = covey.ready_queue_metrics().expect("metrics");
    assert_eq!(metrics.queued_count, 0);
    assert_eq!(metrics.in_flight_count, 1);
    assert!(metrics.oldest_queued_age_ms.is_none());
    assert!(metrics.oldest_in_flight_age_ms.is_some());
}

#[test]
fn system_events_use_system_actor_without_session_token_and_generated_tokens_are_not_spoofable() {
    let rig = Rig::new();
    let covey = rig.covey();

    let handle = covey
        .register_session(RegisterSessionReq {
            agent_principal_id: "principal".into(),
            agent_instance_id: "instance".into(),
            role: SessionRole::Executor,
            idempotency_key: id_key("register-session"),
        })
        .expect("register");
    assert_ne!(handle.session_token, "system");

    let worker = register(&covey, "worker", "worker", SessionRole::Executor);
    assert_ne!(worker, "system");
    rig.tick(60_000);
    let _reap_result = covey.reap_stale_sessions(45_000).expect("reap");

    let events = covey.fetch_events(0, 10).expect("events");
    let system_event = events
        .iter()
        .find(|event| event.event_type == EventType::SessionsReaped)
        .expect("system reap event");
    assert_eq!(system_event.actor_kind, ActorKind::System);
    assert_eq!(system_event.session_token, None);
}

#[test]
fn request_review_rejects_stale_artifact_digest_after_republish() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);

    let claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start");
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:artifact_a".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "artifact_a.json".into(),
            changed_paths_digest: "blake3:paths_a".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish a");
    let first_review_id = covey
        .request_review(RequestReviewReq {
            session_token: worker.clone(),
            subtask_id: subtask_id.clone(),
            artifact_digest: "blake3:artifact_a".into(),
            review_subtask_id: Some("review_stale_a".into()),
            priority: 1,
            idempotency_key: id_key("request-review"),
        })
        .expect("request review a");
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:artifact_b".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "artifact_b.json".into(),
            changed_paths_digest: "blake3:paths_b".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish b");

    assert!(matches!(
        covey.request_review(RequestReviewReq {
            session_token: worker.clone(),
            subtask_id: subtask_id.clone(),
            artifact_digest: "blake3:artifact_a".into(),
            review_subtask_id: Some("review_stale_b".into()),
            priority: 1,
        idempotency_key: id_key("request-review"),
        }),
        Err(CoveyError::UnknownArtifactDigest { digest }) if digest == "blake3:artifact_a"
    ));

    let status = covey.subtask_status(&subtask_id).expect("status");
    let first_review = status
        .reviews
        .iter()
        .find(|review| review.review_id() == first_review_id)
        .expect("first review");
    assert_eq!(first_review.state(), covey::ReviewState::Superseded);

    covey
        .request_review(RequestReviewReq {
            session_token: worker.clone(),
            subtask_id,
            artifact_digest: "blake3:artifact_b".into(),
            review_subtask_id: Some("review_fresh".into()),
            priority: 1,
            idempotency_key: id_key("request-review"),
        })
        .expect("request review b");
}

#[test]
fn reservation_overlap_conflicts_resolve_when_reservations_release_or_expire() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(&covey, "orch", "orch", SessionRole::Orchestrator);

    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "reservations".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("meta");
    let subtask_a = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id: meta_task_id.clone(),
            subtask_id: Some("reservation_a".into()),
            title: "reservation a".into(),
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask a");
    let subtask_b = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id,
            subtask_id: Some("reservation_b".into()),
            title: "reservation b".into(),
            priority: 2,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask b");

    let reservation_a = covey
        .request_reservation(RequestReservationReq {
            session_token: orch.clone(),
            owner_subtask_id: subtask_a.clone(),
            scope_class: ScopeClass::ExactPath,
            scope_key: "src/lib.rs".into(),
            generated_members: Vec::new(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("request-reservation"),
        })
        .expect("reservation a");
    let reservation_b = covey
        .request_reservation(RequestReservationReq {
            session_token: orch.clone(),
            owner_subtask_id: subtask_b.clone(),
            scope_class: ScopeClass::ExactPath,
            scope_key: "src/lib.rs".into(),
            generated_members: Vec::new(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("request-reservation"),
        })
        .expect("reservation b");

    let release_conflict = covey
        .list_conflicts()
        .expect("conflicts")
        .into_iter()
        .find(|conflict| conflict.resolution_state == covey::ConflictResolutionState::Open)
        .expect("open conflict");
    let release_payload: ReservationOverlapConflictPayload =
        serde_json::from_str(&release_conflict.payload_json).expect("release payload");
    let release_pairs = HashSet::from([
        release_payload.reservation_id.clone(),
        release_payload.overlapping_reservation_id.clone(),
    ]);
    assert_eq!(
        release_pairs,
        HashSet::from([reservation_a.clone(), reservation_b.clone()])
    );

    covey
        .release_reservation(ReleaseReservationReq {
            session_token: orch.clone(),
            reservation_id: reservation_b,
            idempotency_key: id_key("release-reservation"),
        })
        .expect("release reservation");

    let release_conflict_state = covey
        .list_conflicts()
        .expect("conflicts after release")
        .into_iter()
        .find(|conflict| conflict.conflict_id == release_conflict.conflict_id)
        .expect("released conflict row");
    assert_eq!(
        release_conflict_state.resolution_state,
        covey::ConflictResolutionState::Resolved
    );

    let reservation_c = covey
        .request_reservation(RequestReservationReq {
            session_token: orch.clone(),
            owner_subtask_id: subtask_a,
            scope_class: ScopeClass::ExactPath,
            scope_key: "src/main.rs".into(),
            generated_members: Vec::new(),
            lease_duration_ms: 5,
            idempotency_key: id_key("request-reservation"),
        })
        .expect("reservation c");
    let reservation_d = covey
        .request_reservation(RequestReservationReq {
            session_token: orch.clone(),
            owner_subtask_id: subtask_b,
            scope_class: ScopeClass::ExactPath,
            scope_key: "src/main.rs".into(),
            generated_members: Vec::new(),
            lease_duration_ms: 5,
            idempotency_key: id_key("request-reservation"),
        })
        .expect("reservation d");
    let expire_conflict = covey
        .list_conflicts()
        .expect("conflicts before expire")
        .into_iter()
        .find(|conflict| {
            let payload: ReservationOverlapConflictPayload =
                serde_json::from_str(&conflict.payload_json).expect("expire payload");
            let ids = HashSet::from([
                payload.reservation_id.clone(),
                payload.overlapping_reservation_id.clone(),
            ]);
            ids == HashSet::from([reservation_c.clone(), reservation_d.clone()])
        })
        .expect("expiring conflict");

    rig.tick(10);
    let expired = covey
        .expire_old_reservations()
        .expect("expire reservations");
    assert_eq!(expired.expired_count, 2);

    let expire_conflict_state = covey
        .list_conflicts()
        .expect("conflicts after expire")
        .into_iter()
        .find(|conflict| conflict.conflict_id == expire_conflict.conflict_id)
        .expect("expired conflict row");
    assert_eq!(
        expire_conflict_state.resolution_state,
        covey::ConflictResolutionState::Resolved
    );
}

#[test]
fn malformed_event_payloads_fail_with_serialization_error() {
    let rig = Rig::new();
    let covey = rig.covey();
    let _sess = register(&covey, "sess", "principal", SessionRole::Executor);

    let conn = Connection::open(&rig.db_path).expect("open db");
    conn.execute(
        "UPDATE event_log SET payload_json = ?1 WHERE seq = 1",
        params!["{"],
    )
    .expect("corrupt payload");

    let event = covey
        .fetch_events(0, 10)
        .expect("events")
        .into_iter()
        .next()
        .expect("event");
    assert!(matches!(
        event.typed(),
        Err(CoveyError::SerializationError(_))
    ));
}

#[test]
fn queued_items_reject_direct_apply_and_missing_queue_items_are_typed_errors() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(&covey, "orch", "orch", SessionRole::Orchestrator);
    let gate = register(&covey, "gate", "gate", SessionRole::ApplyGate);
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);
    let reviewer = register(&covey, "reviewer", "reviewer", SessionRole::Reviewer);

    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "queue".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("meta task");
    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id,
            subtask_id: Some("queue_target".into()),
            title: "queue target".into(),
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask");

    let claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start");
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:queue".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "queue.json".into(),
            changed_paths_digest: "blake3:paths_queue".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish");
    let review_id = covey
        .request_review(RequestReviewReq {
            session_token: worker.clone(),
            subtask_id: subtask_id.clone(),
            artifact_digest: "blake3:queue".into(),
            review_subtask_id: Some("review_queue".into()),
            priority: 1,
            idempotency_key: id_key("request-review"),
        })
        .expect("review req");
    covey
        .release_claim(ReleaseClaimReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id,
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("release-claim"),
        })
        .expect("release");
    let review_claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: reviewer.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim review")
        .expect("review claim");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: reviewer.clone(),
            claim_id: review_claim.claim_id.clone(),
            fence_seq: review_claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start review");
    covey
        .decide_review(DecideReviewReq {
            session_token: reviewer.clone(),
            review_id: review_id.clone(),
            claim_id: review_claim.claim_id,
            fence_seq: review_claim.fence_seq,
            verdict: covey::ReviewVerdict::Approve,
            findings_digest: "blake3:findings".into(),
            idempotency_key: id_key("decide-review"),
        })
        .expect("approve");

    let queue_id = covey
        .enqueue_for_apply(EnqueueForApplyReq {
            session_token: orch.clone(),
            artifact_digest: "blake3:queue".into(),
            subtask_id,
            settlement_target: SettlementTarget::Canonical,
            idempotency_key: id_key("enqueue-for-apply"),
        })
        .expect("enqueue");

    assert!(matches!(
        covey.mark_applied(MarkAppliedReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            claim_fence_seq: 1,
            idempotency_key: id_key("mark-applied"),
        }),
        Err(CoveyError::IllegalTransition { from, to, object })
            if from == ReadyQueueState::Queued.into()
                && to == ReadyQueueState::Applied.into()
                && object == ObjectType::ReadyQueue
    ));
    assert!(matches!(
        covey.mark_in_flight(MarkInFlightReq {
            session_token: gate.clone(),
            queue_id: "missing".into(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("mark-in-flight"),
        }),
        Err(CoveyError::QueueItemNotFound)
    ));
}

#[test]
fn mark_applied_rejects_queue_items_when_subtask_digest_has_drifted() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(&covey, "orch", "orch", SessionRole::Orchestrator);
    let gate = register(&covey, "gate", "gate", SessionRole::ApplyGate);
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);
    let reviewer = register(&covey, "reviewer", "reviewer", SessionRole::Reviewer);

    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "queue drift".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("meta task");
    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id,
            subtask_id: Some("queue_drift_target".into()),
            title: "queue drift target".into(),
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask");

    let claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start");
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:queue_drift".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "queue_drift.json".into(),
            changed_paths_digest: "blake3:paths_queue_drift".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish");
    let review_id = covey
        .request_review(RequestReviewReq {
            session_token: worker.clone(),
            subtask_id: subtask_id.clone(),
            artifact_digest: "blake3:queue_drift".into(),
            review_subtask_id: Some("review_queue_drift".into()),
            priority: 1,
            idempotency_key: id_key("request-review"),
        })
        .expect("review req");
    covey
        .release_claim(ReleaseClaimReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id,
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("release-claim"),
        })
        .expect("release");

    let review_claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: reviewer.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim review")
        .expect("review claim");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: reviewer.clone(),
            claim_id: review_claim.claim_id.clone(),
            fence_seq: review_claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start review");
    covey
        .decide_review(DecideReviewReq {
            session_token: reviewer.clone(),
            review_id: review_id.clone(),
            claim_id: review_claim.claim_id,
            fence_seq: review_claim.fence_seq,
            verdict: covey::ReviewVerdict::Approve,
            findings_digest: "blake3:findings_queue_drift".into(),
            idempotency_key: id_key("decide-review"),
        })
        .expect("approve");

    let queue_id = covey
        .enqueue_for_apply(EnqueueForApplyReq {
            session_token: orch.clone(),
            artifact_digest: "blake3:queue_drift".into(),
            subtask_id: subtask_id.clone(),
            settlement_target: SettlementTarget::Canonical,
            idempotency_key: id_key("enqueue-for-apply"),
        })
        .expect("enqueue");
    let queue_claim = covey
        .mark_in_flight(MarkInFlightReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("mark-in-flight"),
        })
        .expect("flight");

    let conn = Connection::open(&rig.db_path).expect("open db");
    conn.execute(
        "INSERT INTO artifacts (
            artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session,
            manifest_path, changed_paths_digest, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            "blake3:queue_drifted",
            ArtifactKind::PatchBundle.to_string(),
            "base",
            subtask_id.clone(),
            worker.clone(),
            "queue_drifted.json",
            "blake3:paths_queue_drifted",
            1_700_000_000_999_i64,
        ],
    )
    .expect("insert drift artifact");
    conn.execute(
        "UPDATE subtasks SET artifact_digest = ?2 WHERE subtask_id = ?1",
        params![subtask_id, "blake3:queue_drifted"],
    )
    .expect("drift artifact digest");

    assert!(matches!(
        covey.mark_applied(MarkAppliedReq {
            session_token: gate.clone(),
            queue_id,
            claim_fence_seq: queue_claim.claim_fence_seq,
            idempotency_key: id_key("mark-applied"),
        }),
        Err(CoveyError::IllegalTransition { from, to, object })
            if from == SubtaskState::ReadyForApply.into()
                && to == SubtaskState::Applied.into()
                && object == ObjectType::Subtask
    ));
}

#[test]
fn error_variants_are_reachable_for_authz_missing_objects_and_conflicts() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let orch = register(
        &covey,
        "orch_extra",
        "orch_extra",
        SessionRole::Orchestrator,
    );
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);
    let other = register(&covey, "other", "other", SessionRole::Executor);

    assert!(matches!(
        covey.submit_meta_task(SubmitMetaTaskReq {
            session_token: worker.clone(),
            prompt_text: "nope".into(),
        idempotency_key: id_key("submit-meta-task"),
        }),
        Err(CoveyError::WrongRole { actual, .. }) if actual == SessionRole::Executor
    ));

    let claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 5,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim")
        .expect("claim result");

    let second_meta = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "second".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("meta");
    covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id: second_meta,
            subtask_id: Some("work_2".into()),
            title: "second work".into(),
            priority: 2,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("second subtask");

    assert!(matches!(
        covey.claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
        idempotency_key: id_key("claim-next"),
        }),
        Err(CoveyError::SessionAlreadyHasActiveSubtask { session_token, active_subtask_id })
            if session_token == worker && active_subtask_id == subtask_id
    ));

    assert!(matches!(
        covey.release_claim(ReleaseClaimReq {
            session_token: other.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
        idempotency_key: id_key("release-claim"),
        }),
        Err(CoveyError::NotClaimOwner { session_token, claim_owner })
            if session_token == other && claim_owner == worker
    ));

    rig.tick(10);
    assert!(matches!(
        covey.start_subtask(StartSubtaskReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
        idempotency_key: id_key("start-subtask"),
        }),
        Err(CoveyError::LeaseExpired { object_id }) if object_id == claim.claim_id
    ));

    assert!(matches!(
        covey.release_reservation(ReleaseReservationReq {
            session_token: orch.clone(),
            reservation_id: "missing".into(),
            idempotency_key: id_key("release-reservation"),
        }),
        Err(CoveyError::ReservationNotFound)
    ));
    assert!(matches!(
        covey.resolve_conflict(ResolveConflictReq {
            session_token: orch.clone(),
            conflict_id: "missing".into(),
            resolution_state: covey::ConflictResolutionState::Resolved,
            idempotency_key: id_key("resolve-conflict"),
        }),
        Err(CoveyError::ConflictNotFound)
    ));
    assert!(matches!(
        covey.create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id: "meta_does_not_exist".into(),
            subtask_id: Some("review_bad".into()),
            title: "bad review".into(),
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        }),
        Err(CoveyError::MetaTaskNotFound)
    ));

    let temp_dir = TempDir::new().expect("tempdir");
    assert!(matches!(
        Covey::open(temp_dir.path()),
        Err(CoveyError::DatabaseError(_))
    ));
}

#[test]
fn mismatch_and_missing_domain_errors_are_reachable() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(&covey, "orch", "orch", SessionRole::Orchestrator);
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);
    let reviewer = register(&covey, "reviewer", "reviewer", SessionRole::Reviewer);

    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "errors".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("meta");
    covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id: meta_task_id.clone(),
            subtask_id: Some("dup".into()),
            title: "dup".into(),
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("first subtask");

    assert!(matches!(
        covey.create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id: meta_task_id.clone(),
            subtask_id: Some("dup".into()),
            title: "dup".into(),
            priority: 1,
        idempotency_key: id_key("create-subtask"),
        }),
        Err(CoveyError::DuplicateSubtaskId { subtask_id }) if subtask_id == "dup"
    ));

    assert!(matches!(
        covey.subtask_status("missing_subtask"),
        Err(CoveyError::SubtaskNotFound)
    ));

    let work_claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: worker.clone(),
            claim_id: work_claim.claim_id.clone(),
            fence_seq: work_claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start");
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: work_claim.claim_id.clone(),
            fence_seq: work_claim.fence_seq,
            artifact_digest: "blake3:dup".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "dup.json".into(),
            changed_paths_digest: "blake3:paths_dup".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish");
    covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id: meta_task_id.clone(),
            subtask_id: Some("other_work".into()),
            title: "other work".into(),
            priority: 3,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("other work subtask");
    assert!(matches!(
        covey.request_review(RequestReviewReq {
            session_token: worker.clone(),
            subtask_id: "dup".into(),
            artifact_digest: "blake3:missing".into(),
            review_subtask_id: Some("review_missing".into()),
            priority: 1,
        idempotency_key: id_key("request-review"),
        }),
        Err(CoveyError::UnknownArtifactDigest { digest }) if digest == "blake3:missing"
    ));

    let review_id = covey
        .request_review(RequestReviewReq {
            session_token: worker.clone(),
            subtask_id: "dup".into(),
            artifact_digest: "blake3:dup".into(),
            review_subtask_id: Some("review_dup".into()),
            priority: 3,
            idempotency_key: id_key("request-review"),
        })
        .expect("request review");
    covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id,
            subtask_id: Some("wrong_claim_subtask".into()),
            title: "wrong claim subtask".into(),
            priority: 0,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("wrong claim subtask");
    covey
        .release_claim(ReleaseClaimReq {
            session_token: worker.clone(),
            claim_id: work_claim.claim_id,
            fence_seq: work_claim.fence_seq,
            idempotency_key: id_key("release-claim"),
        })
        .expect("release");

    let review_claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: reviewer.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim wrong subtask")
        .expect("claim result");
    assert_eq!(review_claim.subtask_id, "review_dup");

    assert!(matches!(
        covey.decide_review(DecideReviewReq {
            session_token: reviewer.clone(),
            review_id: "missing_review".into(),
            claim_id: review_claim.claim_id.clone(),
            fence_seq: review_claim.fence_seq,
            verdict: covey::ReviewVerdict::Approve,
            findings_digest: "blake3:findings".into(),
            idempotency_key: id_key("decide-review"),
        }),
        Err(CoveyError::ReviewNotFound)
    ));

    let worker_claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim work subtask")
        .expect("claim result");
    assert_eq!(worker_claim.subtask_id, "wrong_claim_subtask");
    assert!(matches!(
        covey.decide_review(DecideReviewReq {
            session_token: worker.clone(),
            review_id: review_id.clone(),
            claim_id: worker_claim.claim_id,
            fence_seq: worker_claim.fence_seq,
            verdict: covey::ReviewVerdict::Approve,
            findings_digest: "blake3:findings".into(),
            idempotency_key: id_key("decide-review"),
        }),
        Err(CoveyError::WrongRole { actual, .. }) if actual == SessionRole::Executor
    ));

    let conn = Connection::open(&rig.db_path).expect("open db");
    let corrupt = conn.execute(
        "UPDATE subtasks SET artifact_digest = ?2 WHERE subtask_id = ?1",
        params!["dup", "blake3:missing_artifact"],
    );
    assert!(corrupt.is_err());
}

#[test]
fn concurrent_heartbeats_succeed_for_all_sessions() {
    let rig = Rig::new();
    let covey = rig.covey();
    let mut session_tokens = Vec::new();
    for idx in 0..10 {
        let session = format!("worker_{idx}");
        session_tokens.push(register(&covey, &session, &session, SessionRole::Executor));
    }

    let mut handles = Vec::new();
    for session_token in session_tokens.clone() {
        let db = rig.db_path.clone();
        let clock = rig.clock.clone();
        handles.push(thread::spawn(move || {
            let covey = Covey::open_with_clock(db, clock).expect("open covey");
            for _ in 0..10 {
                send_heartbeat(&covey, &session_token).expect("heartbeat");
            }
        }));
    }

    for handle in handles {
        handle.join().expect("join");
    }

    for session_token in session_tokens {
        let status = covey
            .session_status(&session_token)
            .expect("session status");
        assert_eq!(status.session.state(), SessionState::Active);
        assert!(status.session.last_heartbeat_at >= status.session.created_at);
    }
}

#[test]
fn list_conflicts_is_bounded() {
    let rig = Rig::new();
    let covey = rig.covey();

    let conn = Connection::open(&rig.db_path).expect("open db");
    for idx in 0..1_005 {
        conn.execute(
            "INSERT INTO conflicts (conflict_id, object_type, object_id, conflict_kind, payload_json, detected_at, resolution_state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                format!("conflict_{idx}"),
                ObjectType::Subtask.to_string(),
                format!("subtask_{idx}"),
                "overlap",
                "{}",
                idx as i64,
                "open"
            ],
        )
        .expect("insert conflict");
    }

    let conflicts = covey.list_conflicts().expect("list conflicts");
    assert_eq!(conflicts.len(), 1_000);
    assert_eq!(
        conflicts.first().expect("first").conflict_id,
        "conflict_1004"
    );
    assert_eq!(conflicts.last().expect("last").conflict_id, "conflict_5");
}

#[test]
fn write_transactions_retry_when_the_database_is_temporarily_busy() {
    let rig = Rig::new();
    let covey = rig.covey();
    let mut conn = Connection::open(&rig.db_path).expect("open raw db");
    conn.pragma_update(None, "foreign_keys", "ON")
        .expect("foreign_keys");
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("hold writer lock");

    let handle = thread::spawn(move || {
        covey
            .register_session(RegisterSessionReq {
                agent_principal_id: "retry_principal".into(),
                agent_instance_id: "retry_instance".into(),
                role: SessionRole::Executor,
                idempotency_key: id_key("register-session"),
            })
            .expect("register after retry")
            .session_token
    });

    thread::sleep(std::time::Duration::from_millis(150));
    tx.commit().expect("release writer lock");
    let session_token = handle.join().expect("join");

    let session = rig.covey().session_status(&session_token).expect("session");
    assert_eq!(session.session.agent_principal_id, "retry_principal");
}

#[test]
fn crash_helper_aborts_mid_transaction() {
    if env::var_os(CRASH_HELPER_ENV).is_none() {
        return;
    }

    let db_path = env::var(CRASH_DB_PATH_ENV).expect("crash helper db path");
    let mut conn = Connection::open(db_path).expect("open crash helper db");
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = FULL;
        "#,
    )
    .expect("apply pragmas");

    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("begin immediate");
    tx.execute(
        "INSERT INTO sessions (
            session_token, agent_principal_id, agent_instance_id, role, state,
            active_subtask_id, last_heartbeat_at, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?6, ?6)",
        params![
            "crash_sess",
            "crash_principal",
            "crash_instance",
            SessionRole::Executor.to_string(),
            SessionState::Active.to_string(),
            1_700_000_000_000_i64,
        ],
    )
    .expect("insert session");
    tx.execute(
        "INSERT INTO event_log (
            event_type, object_type, object_id, session_token, payload_json, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            EventType::SessionRegistered.to_string(),
            ObjectType::Session.to_string(),
            "crash_sess",
            "crash_sess",
            r#"{"session_token":"crash_sess"}"#,
            1_700_000_000_000_i64,
        ],
    )
    .expect("insert event");

    std::process::abort();
}

#[test]
fn crash_mid_transaction_leaves_database_consistent() {
    if env::var_os(CRASH_HELPER_ENV).is_some() {
        return;
    }

    let rig = Rig::new();
    let _ = rig.covey();
    let current_exe = env::current_exe().expect("current exe");

    let status = Command::new(current_exe)
        .arg("--exact")
        .arg("crash_helper_aborts_mid_transaction")
        .arg("--nocapture")
        .env(CRASH_HELPER_ENV, "1")
        .env(CRASH_DB_PATH_ENV, &rig.db_path)
        .status()
        .expect("spawn crash helper");

    assert!(!status.success());

    let conn = Connection::open(&rig.db_path).expect("open crashed db");
    let session_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE session_token = 'crash_sess'",
            [],
            |row| row.get(0),
        )
        .expect("session count");
    let event_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM event_log WHERE object_id = 'crash_sess'",
            [],
            |row| row.get(0),
        )
        .expect("event count");

    assert_eq!(session_count, 0);
    assert_eq!(event_count, 0);
    assert_eq!(pragma_integrity_check(&rig.db_path), "ok");
}

#[test]
fn sqlite_fault_helper_rolls_back_mid_transaction() {
    if env::var_os(SQLITE_FAULT_HELPER_ENV).is_none() {
        return;
    }

    let rig = Rig::new();
    let covey = rig.covey();

    install_sqlite_fault_callback(SQLITE_SYNC_FAULT_CODE);
    let result = covey.register_session(RegisterSessionReq {
        agent_principal_id: "faulty_principal".into(),
        agent_instance_id: "faulty_instance".into(),
        role: SessionRole::Executor,
        idempotency_key: id_key("register-session"),
    });
    uninstall_sqlite_fault_callback();

    assert!(
        SQLITE_FAULT_SEEN_TARGET.load(Ordering::Relaxed),
        "SQLite fault hook did not observe test code {SQLITE_SYNC_FAULT_CODE}"
    );
    assert!(matches!(result, Err(CoveyError::DatabaseError(_))));

    let conn = Connection::open(&rig.db_path).expect("open fault db");
    let session_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE agent_principal_id = 'faulty_principal'",
            [],
            |row| row.get(0),
        )
        .expect("session count");
    let event_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM event_log WHERE payload_json LIKE '%faulty_principal%'",
            [],
            |row| row.get(0),
        )
        .expect("event count");

    assert_eq!(session_count, 0);
    assert_eq!(event_count, 0);
    assert_eq!(pragma_integrity_check(&rig.db_path), "ok");
}

#[test]
fn sqlite_fault_injection_mid_transaction_leaves_database_consistent() {
    if env::var_os(SQLITE_FAULT_HELPER_ENV).is_some() {
        return;
    }

    let current_exe = env::current_exe().expect("current exe");
    let status = Command::new(current_exe)
        .arg("--exact")
        .arg("sqlite_fault_helper_rolls_back_mid_transaction")
        .arg("--nocapture")
        .env(SQLITE_FAULT_HELPER_ENV, "1")
        .status()
        .expect("spawn sqlite fault helper");

    assert!(status.success());
}

#[test]
fn end_to_end_flow_tracks_work_review_apply_and_abandon() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(&covey, "orch", "orch", SessionRole::Orchestrator);
    let worker_a = register(&covey, "worker_a", "worker_a", SessionRole::Executor);
    let worker_b = register(&covey, "worker_b", "worker_b", SessionRole::Executor);
    let reviewer = register(&covey, "reviewer", "reviewer", SessionRole::Reviewer);
    let gate = register(&covey, "gate", "gate", SessionRole::ApplyGate);
    attest(&covey, &worker_a);
    attest(&covey, &reviewer);
    attest(&covey, &gate);

    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "full flow".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("submit");
    let apply_subtask = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id: meta_task_id.clone(),
            subtask_id: Some("apply_me".into()),
            title: "apply me".into(),
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask a");
    let abandon_subtask = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id,
            subtask_id: Some("abandon_me".into()),
            title: "abandon me".into(),
            priority: 2,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask b");

    let claim_a = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker_a.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim a")
        .expect("claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: worker_a.clone(),
            claim_id: claim_a.claim_id.clone(),
            fence_seq: claim_a.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start a");
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker_a.clone(),
            claim_id: claim_a.claim_id.clone(),
            fence_seq: claim_a.fence_seq,
            artifact_digest: "blake3:apply".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "apply.json".into(),
            changed_paths_digest: "blake3:apply_paths".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish");
    let review_id = covey
        .request_review(RequestReviewReq {
            session_token: worker_a.clone(),
            subtask_id: apply_subtask.clone(),
            artifact_digest: "blake3:apply".into(),
            review_subtask_id: Some("review_apply".into()),
            priority: 1,
            idempotency_key: id_key("request-review"),
        })
        .expect("request review");
    covey
        .release_claim(ReleaseClaimReq {
            session_token: worker_a.clone(),
            claim_id: claim_a.claim_id,
            fence_seq: claim_a.fence_seq,
            idempotency_key: id_key("release-claim"),
        })
        .expect("release a");

    let review_claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: reviewer.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim review")
        .expect("review claim");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: reviewer.clone(),
            claim_id: review_claim.claim_id.clone(),
            fence_seq: review_claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start review");
    covey
        .decide_review(DecideReviewReq {
            session_token: reviewer.clone(),
            review_id: review_id.clone(),
            claim_id: review_claim.claim_id,
            fence_seq: review_claim.fence_seq,
            verdict: covey::ReviewVerdict::Approve,
            findings_digest: "blake3:findings".into(),
            idempotency_key: id_key("decide-review"),
        })
        .expect("decide");
    let queue_id = covey
        .enqueue_for_apply(EnqueueForApplyReq {
            session_token: orch.clone(),
            artifact_digest: "blake3:apply".into(),
            subtask_id: apply_subtask.clone(),
            settlement_target: SettlementTarget::Canonical,
            idempotency_key: id_key("enqueue-for-apply"),
        })
        .expect("enqueue");
    let queue_claim = covey
        .mark_in_flight(MarkInFlightReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("mark-in-flight"),
        })
        .expect("flight");
    record_apply_verification(
        &covey,
        &gate,
        &queue_id,
        "blake3:apply",
        &review_id,
        "blake3:findings",
        queue_claim.claim_fence_seq,
    );
    covey
        .mark_applied(MarkAppliedReq {
            session_token: gate.clone(),
            queue_id,
            claim_fence_seq: queue_claim.claim_fence_seq,
            idempotency_key: id_key("mark-applied"),
        })
        .expect("applied");

    let claim_b = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker_b.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim b")
        .expect("claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: worker_b.clone(),
            claim_id: claim_b.claim_id.clone(),
            fence_seq: claim_b.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start b");
    covey
        .abandon_subtask(AbandonSubtaskReq {
            session_token: worker_b.clone(),
            claim_id: claim_b.claim_id,
            fence_seq: claim_b.fence_seq,
            idempotency_key: id_key("abandon-subtask"),
        })
        .expect("abandon");

    assert_eq!(
        covey
            .subtask_status(&apply_subtask)
            .expect("apply status")
            .subtask
            .state,
        SubtaskState::Applied
    );
    assert_eq!(
        covey
            .subtask_status(&abandon_subtask)
            .expect("abandon status")
            .subtask
            .state,
        SubtaskState::Abandoned
    );
    assert!(matches!(
        covey.meta_task_status("meta_does_not_exist"),
        Err(CoveyError::MetaTaskNotFound)
    ));
    let events = covey.fetch_events(0, 1_000).expect("events");
    assert!(!events.is_empty());
    for pair in events.windows(2) {
        assert!(pair[0].seq < pair[1].seq);
        let _ = pair[0].typed().expect("typed event");
    }
    let _ = events
        .last()
        .expect("last event")
        .typed()
        .expect("typed event");
}

#[test]
fn meta_task_state_moves_from_planning_to_active_to_completed() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(&covey, "orch", "orch", SessionRole::Orchestrator);
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);
    let reviewer = register(&covey, "reviewer", "reviewer", SessionRole::Reviewer);
    let gate = register(&covey, "gate", "gate", SessionRole::ApplyGate);
    attest(&covey, &worker);
    attest(&covey, &reviewer);
    attest(&covey, &gate);

    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "meta lifecycle".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("submit meta");
    assert_eq!(
        covey
            .meta_task_status(&meta_task_id)
            .expect("planning status")
            .meta_task
            .state,
        MetaTaskState::Planning
    );

    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id: meta_task_id.clone(),
            subtask_id: Some("meta_flow_work".into()),
            title: "meta flow work".into(),
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create subtask");
    assert_eq!(
        covey
            .meta_task_status(&meta_task_id)
            .expect("active status")
            .meta_task
            .state,
        MetaTaskState::Active
    );

    let work_claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: worker.clone(),
            claim_id: work_claim.claim_id.clone(),
            fence_seq: work_claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start");
    covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: work_claim.claim_id.clone(),
            fence_seq: work_claim.fence_seq,
            artifact_digest: "blake3:meta_flow".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "meta_flow.json".into(),
            changed_paths_digest: "blake3:meta_flow_paths".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish");
    let review_id = covey
        .request_review(RequestReviewReq {
            session_token: worker.clone(),
            subtask_id: subtask_id.clone(),
            artifact_digest: "blake3:meta_flow".into(),
            review_subtask_id: Some("meta_flow_review".into()),
            priority: 1,
            idempotency_key: id_key("request-review"),
        })
        .expect("request review");
    covey
        .release_claim(ReleaseClaimReq {
            session_token: worker,
            claim_id: work_claim.claim_id,
            fence_seq: work_claim.fence_seq,
            idempotency_key: id_key("release-claim"),
        })
        .expect("release work claim");

    let review_claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: reviewer.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("review claim")
        .expect("review claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: reviewer.clone(),
            claim_id: review_claim.claim_id.clone(),
            fence_seq: review_claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start review");
    covey
        .decide_review(DecideReviewReq {
            session_token: reviewer,
            review_id: review_id.clone(),
            claim_id: review_claim.claim_id,
            fence_seq: review_claim.fence_seq,
            verdict: covey::ReviewVerdict::Approve,
            findings_digest: "blake3:meta_flow_findings".into(),
            idempotency_key: id_key("decide-review"),
        })
        .expect("decide review");

    let queue_id = covey
        .enqueue_for_apply(EnqueueForApplyReq {
            session_token: orch,
            artifact_digest: "blake3:meta_flow".into(),
            subtask_id,
            settlement_target: SettlementTarget::Canonical,
            idempotency_key: id_key("enqueue-for-apply"),
        })
        .expect("enqueue");
    let queue_claim = covey
        .mark_in_flight(MarkInFlightReq {
            session_token: gate.clone(),
            queue_id: queue_id.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("mark-in-flight"),
        })
        .expect("mark in flight");
    record_apply_verification(
        &covey,
        &gate,
        &queue_id,
        "blake3:meta_flow",
        &review_id,
        "blake3:meta_flow_findings",
        queue_claim.claim_fence_seq,
    );
    covey
        .mark_applied(MarkAppliedReq {
            session_token: gate,
            queue_id,
            claim_fence_seq: queue_claim.claim_fence_seq,
            idempotency_key: id_key("mark-applied"),
        })
        .expect("mark applied");

    assert_eq!(
        covey
            .meta_task_status(&meta_task_id)
            .expect("completed status")
            .meta_task
            .state,
        MetaTaskState::Completed
    );
}

#[test]
fn oversized_identity_and_artifact_fields_are_rejected() {
    let rig = Rig::new();
    let covey = rig.covey();
    let long = "x".repeat(10_000);

    assert!(matches!(
        covey.register_session(RegisterSessionReq {
            agent_principal_id: long.clone(),
            agent_instance_id: "instance".into(),
            role: SessionRole::Executor,
            idempotency_key: id_key("register-session"),
        }),
        Err(CoveyError::InputTooLarge { field, .. }) if field == "agent_principal_id"
    ));
    assert!(matches!(
        covey.register_session(RegisterSessionReq {
            agent_principal_id: "principal".into(),
            agent_instance_id: long.clone(),
            role: SessionRole::Executor,
            idempotency_key: id_key("register-session"),
        }),
        Err(CoveyError::InputTooLarge { field, .. }) if field == "agent_instance_id"
    ));

    let orch = register(&covey, "orch", "orch", SessionRole::Orchestrator);
    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "bounds".into(),
            idempotency_key: id_key("submit-meta-task"),
        })
        .expect("submit meta");
    assert!(matches!(
        covey.create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id: meta_task_id.clone(),
            subtask_id: Some(long.clone()),
            title: "too long".into(),
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        }),
        Err(CoveyError::InputTooLarge { field, .. }) if field == "subtask_id"
    ));

    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: orch.clone(),
            meta_task_id,
            subtask_id: Some("bounds_work".into()),
            title: "bounds work".into(),
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create subtask");
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);
    let reviewer = register(&covey, "reviewer", "reviewer", SessionRole::Reviewer);

    let claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: worker.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start");

    assert!(matches!(
        covey.publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:bounds_a".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: long.clone(),
            manifest_path: "bounds.json".into(),
            changed_paths_digest: "blake3:bounds_paths".into(),
            idempotency_key: id_key("publish-artifact"),
        }),
        Err(CoveyError::InputTooLarge { field, .. }) if field == "base_rev"
    ));
    assert!(matches!(
        covey.publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:bounds_b".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: long.clone(),
            changed_paths_digest: "blake3:bounds_paths".into(),
            idempotency_key: id_key("publish-artifact"),
        }),
        Err(CoveyError::InputTooLarge { field, .. }) if field == "manifest_path"
    ));
    assert!(matches!(
        covey.publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:bounds_c".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "bounds.json".into(),
            changed_paths_digest: long.clone(),
            idempotency_key: id_key("publish-artifact"),
        }),
        Err(CoveyError::InputTooLarge { field, .. }) if field == "changed_paths_digest"
    ));

    covey
        .publish_artifact(PublishArtifactReq {
            session_token: worker.clone(),
            claim_id: claim.claim_id.clone(),
            fence_seq: claim.fence_seq,
            artifact_digest: "blake3:bounds_valid".into(),
            artifact_kind: ArtifactKind::PatchBundle,
            base_rev: "base".into(),
            manifest_path: "bounds.json".into(),
            changed_paths_digest: "blake3:bounds_paths".into(),
            idempotency_key: id_key("publish-artifact"),
        })
        .expect("publish valid artifact");
    let review_id = covey
        .request_review(RequestReviewReq {
            session_token: worker.clone(),
            subtask_id: subtask_id.clone(),
            artifact_digest: "blake3:bounds_valid".into(),
            review_subtask_id: Some("bounds_review".into()),
            priority: 1,
            idempotency_key: id_key("request-review"),
        })
        .expect("request review");
    covey
        .release_claim(ReleaseClaimReq {
            session_token: worker,
            claim_id: claim.claim_id,
            fence_seq: claim.fence_seq,
            idempotency_key: id_key("release-claim"),
        })
        .expect("release work claim");

    let review_claim = covey
        .claim_next_subtask(ClaimNextReq {
            session_token: reviewer.clone(),
            lease_duration_ms: 30_000,
            idempotency_key: id_key("claim-next"),
        })
        .expect("review claim")
        .expect("review claim result");
    covey
        .start_subtask(StartSubtaskReq {
            session_token: reviewer.clone(),
            claim_id: review_claim.claim_id.clone(),
            fence_seq: review_claim.fence_seq,
            idempotency_key: id_key("start-subtask"),
        })
        .expect("start review");

    assert!(matches!(
        covey.decide_review(DecideReviewReq {
            session_token: reviewer,
            review_id,
            claim_id: review_claim.claim_id,
            fence_seq: review_claim.fence_seq,
            verdict: covey::ReviewVerdict::Approve,
            findings_digest: long,
            idempotency_key: id_key("decide-review"),
        }),
        Err(CoveyError::InputTooLarge { field, .. }) if field == "findings_digest"
    ));
}

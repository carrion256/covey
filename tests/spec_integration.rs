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
    DecideReviewReq, EnqueueForApplyReq, EventPayload, EventType, ExitSessionReq, FenceSeq,
    HeartbeatReq, IdempotencyKey, ImportBdV1ItemResult, ImportBdV1Req, ImportBdV1Result,
    ImportBdV1SkipReason, LeaseDurationMs, ManualClock, MarkAppliedReq, MarkInFlightReq,
    MetaTaskState, ObjectType, OverlapQueryReq, PublishArtifactReq, ReadyQueueState,
    RecordApplyVerificationReq, RecordRuntimeAttestationReq, RegisterSessionReq, ReleaseClaimReq,
    ReleaseReservationReq, RenewClaimReq, RenewReservationReq, RequestReservationReq,
    RequestReviewReq, ReservationOverlapConflictPayload, ResolveConflictReq, ScopeClass,
    SessionRole, SessionState, SessionToken, SettlementTarget, StartSubtaskReq, StateValue,
    SubmitMetaTaskReq, SubtaskId, SubtaskKind, SubtaskState, SubtaskTitle,
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

fn parse_subtask_id(value: &str) -> SubtaskId {
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
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                principal,
                format!("{principal}-instance"),
                role,
                id_key("register-session"),
            )
            .expect("valid session registration request"),
        )
        .expect("register session")
        .session_token
        .to_string()
}

fn attest(covey: &Covey, session_token: &str) {
    covey
        .record_runtime_attestation(
            RecordRuntimeAttestationReq::try_from_parts(
                session_token.to_owned(),
                "covey-test",
                "test-model",
                format!("provider-run-{session_token}"),
                "covey-test-provider",
                Some(format!("pid-{session_token}")),
                None,
                format!("blake3:{session_token}-transcript"),
                1_700_000_000_000,
                1_700_000_000_000,
                format!("record-runtime-attestation-{session_token}"),
            )
            .expect("valid runtime attestation request"),
        )
        .expect("record runtime attestation");
}

fn seed_work_subtask(rig: &Rig) -> (String, String) {
    let covey = rig.covey();
    let orch = register(&covey, "orch", "orch", SessionRole::Orchestrator);
    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "operator quest",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");
    rig.tick(1);
    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id.clone())
                .expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("work_1")),
            title: SubtaskTitle::parse("implement covey").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
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
    claim_fence_seq: FenceSeq,
) {
    attest(covey, gate);
    covey
        .record_apply_verification(
            RecordApplyVerificationReq::try_from_raw_parts(
                gate.to_owned(),
                queue_id.to_owned(),
                artifact_digest.to_owned(),
                review_id.to_owned(),
                findings_digest.to_owned(),
                claim_fence_seq,
                "mutai-rs",
                format!("{artifact_digest}-verdict"),
                format!("{artifact_digest}-seal"),
                id_key("record-apply-verification"),
            )
            .expect("valid apply verification request"),
        )
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
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim work subtask")
        .expect("claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start work subtask");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "blake3:changes_requested_seed".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "changes_requested_seed.json".into(),
                "blake3:changes_requested_seed_paths".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish artifact");
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                "blake3:changes_requested_seed",
                Some("review_changes_requested_seed".into()),
                1,
                id_key("request-review"),
            )
            .expect("valid review request"),
        )
        .expect("request review");
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                worker,
                claim.claim_id,
                claim.fence_seq,
                id_key("release-claim"),
            )
            .expect("valid release-claim request"),
        )
        .expect("release work claim");

    let review_subtask_id = covey
        .subtask_status(&subtask_id)
        .expect("work status")
        .reviews()
        .into_iter()
        .find(|review| review.review_id() == review_id)
        .map(|review| review.review_subtask_id().to_owned())
        .expect("review subtask id");
    let review_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                reviewer.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim review subtask")
        .expect("review claim result");
    assert_eq!(review_claim.subtask_id, review_subtask_id);
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                review_claim.claim_id.clone(),
                review_claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start review subtask");
    covey
        .decide_review(
            DecideReviewReq::try_from_raw_parts(
                reviewer,
                review_id.clone(),
                review_claim.claim_id,
                review_claim.fence_seq,
                covey::ReviewVerdict::ChangesRequested,
                "blake3:changes_requested_findings".into(),
                id_key("decide-review"),
            )
            .expect("valid review decision request"),
        )
        .expect("request changes");

    assert_eq!(
        covey
            .subtask_status(&subtask_id)
            .expect("updated status")
            .subtask()
            .state(),
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

fn id_key(label: &str) -> IdempotencyKey {
    IdempotencyKey::parse(format!(
        "{label}-{}",
        NEXT_IDEMPOTENCY_KEY.fetch_add(1, Ordering::Relaxed)
    ))
    .expect("valid idempotency key")
}

fn claim_ready_queue_req(
    session_token: impl Into<String>,
    lease_duration_ms: i64,
    idempotency_key: impl Into<String>,
) -> ClaimReadyQueueReq {
    ClaimReadyQueueReq::try_from_raw_parts(session_token, lease_duration_ms, idempotency_key)
        .expect("valid ready-queue claim request")
}

fn mark_in_flight_req(
    session_token: impl Into<String>,
    queue_id: impl Into<String>,
    lease_duration_ms: i64,
    idempotency_key: impl Into<String>,
) -> MarkInFlightReq {
    MarkInFlightReq::try_from_raw_parts(session_token, queue_id, lease_duration_ms, idempotency_key)
        .expect("valid mark-in-flight request")
}

fn mark_applied_req(
    session_token: impl Into<String>,
    queue_id: impl Into<String>,
    claim_fence_seq: FenceSeq,
    idempotency_key: impl Into<String>,
) -> MarkAppliedReq {
    MarkAppliedReq::try_from_raw_parts(
        session_token,
        queue_id,
        claim_fence_seq.get(),
        idempotency_key,
    )
    .expect("valid mark-applied request")
}

fn send_heartbeat(covey: &Covey, session_token: &str) -> covey::Result<()> {
    covey.heartbeat(
        HeartbeatReq::try_from_raw_parts(session_token, id_key("heartbeat"))
            .expect("valid heartbeat request"),
    )
}

fn close_session(covey: &Covey, session_token: &str) -> covey::Result<()> {
    covey.exit_session(
        ExitSessionReq::try_from_raw_parts(session_token, id_key("exit-session"))
            .expect("valid exit-session request"),
    )
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
    let duplicate = covey.register_session(
        RegisterSessionReq::try_from_raw_parts(
            "principal_a",
            "principal_a-next",
            SessionRole::Executor,
            id_key("register-session"),
        )
        .expect("valid session registration request"),
    );
    assert!(matches!(
        duplicate,
        Err(CoveyError::SessionAlreadyActive { agent_principal_id }) if agent_principal_id == "principal_a"
    ));
    let active = covey
        .active_session_for_principal("principal_a", SessionRole::Executor)
        .expect("active session lookup")
        .expect("active executor session");
    assert_eq!(active.session_token(), sess_a);
    assert_eq!(
        covey
            .active_session_for_principal("principal_a", SessionRole::Reviewer)
            .expect("active session lookup with role mismatch"),
        None
    );

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
        covey.register_session(
            RegisterSessionReq::try_from_raw_parts(
                "principal_b",
                "principal_b-instance",
                SessionRole::Reviewer,
                id_key("register-session"),
            )
            .expect("valid session registration request"),
        ),
        Ok(handle) if handle.session_token != sess_a
    ));
    rig.tick(1);
    register(&covey, "sess_c", "principal_a", SessionRole::Executor);
}

#[test]
fn idempotent_mutations_replay_and_reject_payload_drift() {
    let rig = Rig::new();
    let covey = rig.covey();

    let request = RegisterSessionReq::try_from_raw_parts(
        "principal",
        "instance",
        SessionRole::Executor,
        "register-stable-key",
    )
    .expect("valid session registration request");
    let first = covey
        .register_session(request.clone())
        .expect("first register");
    let replay = covey.register_session(request).expect("idempotent replay");
    assert_eq!(first, replay);

    assert!(matches!(
        covey.register_session(
            RegisterSessionReq::try_from_raw_parts(
                "principal",
                "different-instance",
                SessionRole::Executor,
                "register-stable-key",
            )
            .expect("valid session registration request"),
        ),
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
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim next")
        .expect("claim exists");
    rig.tick(1);
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start work");
    rig.tick(1);
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "blake3:artifact_a".into(),
                ArtifactKind::PatchBundle,
                "deadbeef".into(),
                "artifacts/a.json".into(),
                "blake3:paths_a".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish artifact");

    let before_events = covey.fetch_events(0, 100).expect("events").len();
    let collision = covey.publish_artifact(
        PublishArtifactReq::try_from_raw_parts(
            worker,
            claim.claim_id,
            claim.fence_seq,
            "blake3:artifact_a".into(),
            ArtifactKind::PatchBundle,
            "deadbeef".into(),
            "artifacts/b.json".into(),
            "blake3:paths_b".into(),
            id_key("publish-artifact"),
        )
        .expect("valid artifact publication request"),
    );
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
            .subtask()
            .state(),
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
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(10_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("first claim")
        .expect("claim result");
    rig.tick(1);
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                worker.clone(),
                first.claim_id.clone(),
                first.fence_seq,
                id_key("release-claim"),
            )
            .expect("valid release-claim request"),
        )
        .expect("release first");

    rig.tick(1);
    let second = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(10_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("second claim")
        .expect("claim result");
    let stale = covey.release_claim(
        ReleaseClaimReq::try_from_raw_parts(
            worker.clone(),
            first.claim_id,
            first.fence_seq,
            id_key("release-claim"),
        )
        .expect("valid release-claim request"),
    );
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
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(10_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim")
        .expect("claim result");
    rig.tick(5_000);

    let renewed = covey
        .renew_claim(
            RenewClaimReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                LeaseDurationMs::parse(20_000).expect("valid lease duration"),
                id_key("renew-claim"),
            )
            .expect("valid renew-claim request"),
        )
        .expect("renew claim");
    assert_eq!(renewed.claim_id, claim.claim_id);
    assert_eq!(renewed.fence_seq, claim.fence_seq);
    assert_eq!(renewed.lease_deadline, claim.lease_deadline + 20_000);

    rig.tick(10_001);
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker,
                claim.claim_id,
                claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
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
    let destination_meta_task_id = status_after_first.subtask().meta_task_id.clone();

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
    assert_eq!(meta_status.subtasks().len(), 1);
    let imported = &meta_status.subtasks()[0];
    assert_eq!(imported.subtask_id, first_subtask_id);
    assert_eq!(imported.kind(), SubtaskKind::Work);
    assert_eq!(imported.state(), SubtaskState::Available);
    assert!(imported.active_claim_id().is_none());
    assert!(imported.review_target().is_none());
    assert!(imported.review_target().is_none());

    let event_log = covey.fetch_events(0, 100).expect("event log");
    let subtask_created_events = event_log
        .iter()
        .filter(|event| event.event_type() == EventType::SubtaskCreated)
        .count();
    let meta_submitted_events = event_log
        .iter()
        .filter(|event| event.event_type() == EventType::MetaTaskSubmitted)
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
        .import_bd_v1(ImportBdV1Req::new_meta_task(
            orch.clone(),
            beads_db.to_string_lossy().to_string(),
            "feature import destination",
            id_key("import-feature-first"),
        ))
        .expect("first import");

    assert!(first_result.meta_task_id.starts_with("meta_"));
    assert_eq!(first_result.imported_count(), 1);
    assert_eq!(first_result.skipped_count(), 0);
    assert!(first_result.items().iter().any(|item| {
        item.source_issue_id == "BD-FEATURE-1"
            && item.skip_reason().is_none()
            && item
                .subtask_id()
                .is_some_and(|subtask_id| subtask_id.starts_with("bdwork_bd_feature_1_"))
    }));
    let destination_meta_task_id = first_result.meta_task_id;

    let second_result = covey
        .import_bd_v1(ImportBdV1Req::existing_meta_task(
            orch.clone(),
            beads_db.to_string_lossy().to_string(),
            destination_meta_task_id.clone(),
            id_key("import-feature-second"),
        ))
        .expect("repeat import");

    assert_eq!(second_result.meta_task_id, destination_meta_task_id);
    assert_eq!(second_result.imported_count(), 0);
    assert_eq!(second_result.skipped_count(), 1);
    assert!(second_result.items().iter().any(|item| {
        item.source_issue_id == "BD-FEATURE-1"
            && item.skip_reason() == Some(&ImportBdV1SkipReason::DeterministicDuplicate)
    }));

    let meta_status = covey
        .meta_task_status(&destination_meta_task_id)
        .expect("meta-task status");
    assert_eq!(meta_status.meta_task().state(), MetaTaskState::Active);
    assert_eq!(meta_status.subtasks().len(), 1);
    let imported = &meta_status.subtasks()[0];
    assert!(imported.subtask_id.starts_with("bdwork_bd_feature_1_"));
    assert_eq!(imported.kind(), SubtaskKind::Work);
    assert_eq!(imported.state(), SubtaskState::Available);
    assert!(imported.active_claim_id().is_none());

    let event_log = covey.fetch_events(0, 100).expect("event log");
    let subtask_created_events = event_log
        .iter()
        .filter(|event| event.event_type() == EventType::SubtaskCreated)
        .count();
    let meta_submitted_events = event_log
        .iter()
        .filter(|event| event.event_type() == EventType::MetaTaskSubmitted)
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
        .import_bd_v1(ImportBdV1Req::new_meta_task(
            orch.clone(),
            beads_db.to_string_lossy().to_string(),
            "epic import first destination",
            id_key("import-epic-first"),
        ))
        .expect("first import");

    assert!(first_result.meta_task_id.starts_with("meta_"));
    assert_eq!(first_result.imported_count(), 1);
    assert_eq!(first_result.skipped_count(), 0);

    let conflict = covey.import_bd_v1(ImportBdV1Req::new_meta_task(
        orch.clone(),
        beads_db.to_string_lossy().to_string(),
        "epic import second destination",
        id_key("import-epic-second"),
    ));

    assert!(matches!(
        conflict,
        Err(CoveyError::ImportDuplicate { source_issue_id, subtask_id })
            if source_issue_id == "BD-EPIC-1" && subtask_id.starts_with("bdwork_bd_epic_1_")
    ));

    let event_log = covey.fetch_events(0, 100).expect("event log");
    let subtask_created_events = event_log
        .iter()
        .filter(|event| event.event_type() == EventType::SubtaskCreated)
        .count();
    let meta_submitted_events = event_log
        .iter()
        .filter(|event| event.event_type() == EventType::MetaTaskSubmitted)
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

    let req = ImportBdV1Req::new_meta_task(
        orch.clone(),
        "/nonexistent/path/beads.db".to_owned(),
        "test import".to_owned(),
        "import-smoke".to_owned(),
    );
    let req_json = serde_json::to_string(&req).expect("serialize request");
    let req_back: ImportBdV1Req = serde_json::from_str(&req_json).expect("deserialize request");
    assert_eq!(req, req_back);

    let result = ImportBdV1Result::new(
        "meta-1".to_owned(),
        vec![
            ImportBdV1ItemResult::imported("bd-1", "subtask-1"),
            ImportBdV1ItemResult::skipped(
                "bd-2",
                Some("subtask-2".to_owned()),
                ImportBdV1SkipReason::DeterministicDuplicate,
            ),
        ],
    )
    .expect("valid result");
    let result_json = serde_json::to_string(&result).expect("serialize result");
    let result_back: ImportBdV1Result =
        serde_json::from_str(&result_json).expect("deserialize result");
    assert_eq!(result, result_back);
    assert_eq!(result_back.items()[0].subtask_id(), Some("subtask-1"));
    assert!(result_back.items()[0].skip_reason().is_none());
    assert_eq!(result_back.items()[1].subtask_id(), Some("subtask-2"));
    assert_eq!(
        result_back.items()[1].skip_reason(),
        Some(&ImportBdV1SkipReason::DeterministicDuplicate)
    );

    let missing_outcome = r#"{"source_issue_id":"bd-3","subtask_id":null,"skip_reason":null}"#;
    let missing_outcome_err = serde_json::from_str::<ImportBdV1ItemResult>(missing_outcome)
        .expect_err("missing item outcome should be rejected");
    assert!(
        missing_outcome_err
            .to_string()
            .contains("bd import item must include either subtask_id or skip_reason")
    );

    let invalid_row_with_subtask = r#"{"source_issue_id":"bd-4","subtask_id":"subtask-4","skip_reason":{"InvalidRow":{"detail":"bad row"}}}"#;
    let invalid_row_err = serde_json::from_str::<ImportBdV1ItemResult>(invalid_row_with_subtask)
        .expect_err("invalid row with subtask id should be rejected");
    assert!(
        invalid_row_err
            .to_string()
            .contains("invalid bd import rows must not include subtask_id")
    );

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

    let valid_req = ImportBdV1Req::new_meta_task(
        orch.clone(),
        beads_db.to_string_lossy().to_string(),
        "test import valid".to_owned(),
        "import-smoke-valid".to_owned(),
    );
    let import_result = covey
        .import_bd_v1(valid_req)
        .expect("import should succeed");
    assert!(import_result.meta_task_id.starts_with("meta_"));
    assert_eq!(import_result.imported_count(), 0);
    assert_eq!(import_result.skipped_count(), 0);
    assert!(import_result.items().is_empty());
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
        .import_bd_v1(ImportBdV1Req::new_meta_task(
            orch.clone(),
            beads_db.to_string_lossy().to_string(),
            "import bd issue set".to_owned(),
            "import-bd-core".to_owned(),
        ))
        .expect("import should succeed");

    assert!(result.meta_task_id.starts_with("meta_"));
    assert_eq!(result.imported_count(), 4);
    assert_eq!(result.skipped_count(), 3);
    assert_eq!(result.items().len(), 7);

    assert!(result.items().iter().any(|item| {
        item.source_issue_id == "BD-1"
            && item.skip_reason().is_none()
            && item
                .subtask_id()
                .is_some_and(|subtask_id| subtask_id.starts_with("bdwork_bd_1_"))
    }));
    assert!(result.items().iter().any(|item| {
        item.source_issue_id == "BD-2"
            && item.skip_reason().is_none()
            && item
                .subtask_id()
                .is_some_and(|subtask_id| subtask_id.starts_with("bdwork_bd_2_"))
    }));
    assert!(result.items().iter().any(|item| {
        item.source_issue_id == "BD-4"
            && item.skip_reason().is_none()
            && item
                .subtask_id()
                .is_some_and(|subtask_id| subtask_id.starts_with("bdwork_bd_4_"))
    }));
    assert!(result.items().iter().any(|item| {
        item.source_issue_id == "BD-5"
            && item.skip_reason().is_none()
            && item
                .subtask_id()
                .is_some_and(|subtask_id| subtask_id.starts_with("bdwork_bd_5_"))
    }));

    assert!(result.items().iter().any(|item| {
        item.source_issue_id == "BD-3"
            && item.skip_reason()
                == Some(&ImportBdV1SkipReason::InvalidRow {
                    detail: "unsupported status closed".to_owned(),
                })
    }));
    assert!(result.items().iter().any(|item| {
        item.source_issue_id == "BD-6"
            && item.skip_reason()
                == Some(&ImportBdV1SkipReason::InvalidRow {
                    detail: "unsupported labeled issue".to_owned(),
                })
    }));
    assert!(result.items().iter().any(|item| {
        item.source_issue_id == "BD-7"
            && item.skip_reason()
                == Some(&ImportBdV1SkipReason::InvalidRow {
                    detail: "unsupported issue_type bug".to_owned(),
                })
    }));

    let meta_status = covey
        .meta_task_status(&result.meta_task_id)
        .expect("meta-task status");
    assert_eq!(meta_status.meta_task().state(), MetaTaskState::Active);
    assert_eq!(meta_status.subtasks().len(), 4);
    for subtask in meta_status.subtasks() {
        assert_eq!(subtask.kind(), SubtaskKind::Work);
        assert_eq!(subtask.state(), SubtaskState::Available);
        assert!(subtask.active_claim_id().is_none());
        assert!(subtask.review_target().is_none());
        assert!(subtask.review_target().is_none());
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
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "existing destination",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta task");
    let manual_subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id.clone())
                .expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("manual_existing_work")),
            title: SubtaskTitle::parse("manual existing work").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create manual subtask");

    let beads_dir = TempDir::new().expect("tempdir");
    let beads_db = beads_dir.path().join("beads.db");
    seed_bd_import_source(&beads_db);

    let result = covey
        .import_bd_v1(ImportBdV1Req::existing_meta_task(
            orch.clone(),
            beads_db.to_string_lossy().to_string(),
            meta_task_id.clone(),
            id_key("import-bd-existing"),
        ))
        .expect("import should succeed");

    assert_eq!(result.meta_task_id, meta_task_id);
    assert_eq!(result.imported_count(), 4);
    assert_eq!(result.skipped_count(), 3);

    let meta_status = covey
        .meta_task_status(&result.meta_task_id)
        .expect("meta-task status");
    assert_eq!(meta_status.meta_task().state(), MetaTaskState::Active);
    assert_eq!(meta_status.subtasks().len(), 5);
    assert!(
        meta_status
            .subtasks()
            .iter()
            .any(|subtask| subtask.subtask_id == manual_subtask_id)
    );

    let imported_subtasks: Vec<_> = meta_status
        .subtasks()
        .iter()
        .filter(|subtask| subtask.subtask_id != manual_subtask_id)
        .collect();
    assert_eq!(imported_subtasks.len(), 4);
    for subtask in imported_subtasks {
        assert_eq!(subtask.kind(), SubtaskKind::Work);
        assert_eq!(subtask.state(), SubtaskState::Available);
        assert!(subtask.active_claim_id().is_none());
    }

    let session_status = covey.session_status(&orch).expect("session status");
    assert!(session_status.session().active_subtask_id().is_none());
    assert!(session_status.active_subtask().is_none());

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
        .import_bd_v1(ImportBdV1Req::new_meta_task(
            orch.clone(),
            beads_db.to_string_lossy().to_string(),
            "empty import destination",
            id_key("import-bd-empty"),
        ))
        .expect("import should succeed");

    assert_eq!(result.imported_count(), 0);
    assert_eq!(result.skipped_count(), 0);
    assert!(result.items().is_empty());

    let meta_status = covey
        .meta_task_status(&result.meta_task_id)
        .expect("meta-task status");
    assert_eq!(meta_status.meta_task().state(), MetaTaskState::Planning);
    assert!(meta_status.subtasks().is_empty());

    let session_status = covey.session_status(&orch).expect("session status");
    assert!(session_status.session().active_subtask_id().is_none());
    assert!(session_status.active_subtask().is_none());

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
        .import_bd_v1(ImportBdV1Req::new_meta_task(
            orch.clone(),
            beads_db.to_string_lossy().to_string(),
            "invalid import destination",
            id_key("import-bd-invalid-rows"),
        ))
        .expect("import should succeed");

    assert_eq!(result.imported_count(), 0);
    assert_eq!(result.skipped_count(), 4);
    assert_eq!(result.items().len(), 4);
    assert!(
        result
            .items()
            .iter()
            .all(|item| item.subtask_id().is_none())
    );
    assert!(result.items().iter().any(|item| {
        item.skip_reason()
            == Some(&ImportBdV1SkipReason::InvalidRow {
                detail: "missing issue id".to_owned(),
            })
    }));
    assert!(result.items().iter().any(|item| {
        item.skip_reason()
            == Some(&ImportBdV1SkipReason::InvalidRow {
                detail: "missing title".to_owned(),
            })
    }));
    assert!(result.items().iter().any(|item| {
        item.skip_reason()
            == Some(&ImportBdV1SkipReason::InvalidRow {
                detail: "issue id exceeds max length 256".to_owned(),
            })
    }));
    assert!(result.items().iter().any(|item| {
        item.skip_reason()
            == Some(&ImportBdV1SkipReason::InvalidRow {
                detail: "title exceeds max length 512".to_owned(),
            })
    }));

    let meta_status = covey
        .meta_task_status(&result.meta_task_id)
        .expect("meta-task status");
    assert!(meta_status.subtasks().is_empty());

    let session_status = covey.session_status(&orch).expect("session status");
    assert!(session_status.session().active_subtask_id().is_none());
    assert!(session_status.active_subtask().is_none());

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
        .import_bd_v1(ImportBdV1Req::new_meta_task(
            orch.clone(),
            beads_db.to_string_lossy().to_string(),
            "casefold and label edge cases",
            id_key("import-bd-casefold"),
        ))
        .expect("import should succeed");

    assert!(result.meta_task_id.starts_with("meta_"));
    assert_eq!(result.imported_count(), 2);
    assert_eq!(result.skipped_count(), 3);
    assert_eq!(result.items().len(), 5);

    assert!(result.items().iter().any(|item| {
        item.source_issue_id == "BD-FEATURE-MIXED"
            && item.skip_reason().is_none()
            && item
                .subtask_id()
                .is_some_and(|subtask_id| subtask_id.starts_with("bdwork_bd_feature_mixed_"))
    }));
    assert!(result.items().iter().any(|item| {
        item.source_issue_id == "BD-EPIC-MIXED"
            && item.skip_reason().is_none()
            && item
                .subtask_id()
                .is_some_and(|subtask_id| subtask_id.starts_with("bdwork_bd_epic_mixed_"))
    }));

    assert!(result.items().iter().any(|item| {
        item.source_issue_id == "BD-REVIEW-TASK"
            && item.skip_reason()
                == Some(&ImportBdV1SkipReason::InvalidRow {
                    detail: "unsupported labeled issue".to_owned(),
                })
    }));
    assert!(result.items().iter().any(|item| {
        item.source_issue_id == "BD-SKIP-TASK"
            && item.skip_reason()
                == Some(&ImportBdV1SkipReason::InvalidRow {
                    detail: "unsupported labeled issue".to_owned(),
                })
    }));
    assert!(result.items().iter().any(|item| {
        item.source_issue_id == "BD-BUG-LOWER"
            && item.skip_reason()
                == Some(&ImportBdV1SkipReason::InvalidRow {
                    detail: "unsupported issue_type bug".to_owned(),
                })
    }));

    let meta_status = covey
        .meta_task_status(&result.meta_task_id)
        .expect("meta-task status");
    assert_eq!(meta_status.meta_task().state(), MetaTaskState::Active);
    assert_eq!(meta_status.subtasks().len(), 2);
    for subtask in meta_status.subtasks() {
        assert_eq!(subtask.kind(), SubtaskKind::Work);
        assert_eq!(subtask.state(), SubtaskState::Available);
        assert!(subtask.active_claim_id().is_none());
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
        .import_bd_v1(ImportBdV1Req::new_meta_task(
            orch.clone(),
            beads_db.to_string_lossy().to_string(),
            "should not import".to_owned(),
            "import-malformed-source".to_owned(),
        ))
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
        .import_bd_v1(ImportBdV1Req::new_meta_task(
            orch.clone(),
            beads_db.to_string_lossy().to_string(),
            "targeted claim boundary test".to_owned(),
            id_key("import-targeted-claim-boundary"),
        ))
        .expect("import should succeed");

    // All imported subtasks remain available; no claims are created.
    let meta_status = covey
        .meta_task_status(&result.meta_task_id)
        .expect("meta-task status");
    for subtask in meta_status.subtasks() {
        assert_eq!(subtask.state(), SubtaskState::Available);
        assert!(subtask.active_claim_id().is_none());
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
    let req = ClaimSubtaskReq::try_from_raw_parts(
        "session_abc123".to_owned(),
        covey::SubtaskId::parse("subtask_xyz789".to_owned()).expect("valid subtask id"),
        covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
        "idem_key_001".to_owned(),
    )
    .expect("valid claim-subtask request");

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
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                worker.clone(),
                covey::SubtaskId::parse(subtask_id.clone()).expect("valid subtask id"),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-subtask"),
            )
            .expect("valid claim-subtask request"),
        )
        .expect("claim known subtask");

    assert_eq!(claim.subtask_id, subtask_id);
    assert!(claim.fence_seq > 0);

    let status = covey.subtask_status(&subtask_id).expect("subtask status");
    assert_eq!(status.subtask().state(), SubtaskState::Claimed);
    assert_eq!(
        status.subtask().active_claim_id().map(AsRef::as_ref),
        Some(claim.claim_id.as_str())
    );
    let held_claim = status.claim().expect("held claim status");
    assert_eq!(held_claim.claim_id, claim.claim_id);
    assert_eq!(held_claim.owner_session_token, worker);
    assert_eq!(held_claim.fence_seq, claim.fence_seq);
    assert_eq!(held_claim.lease_deadline, claim.lease_deadline);

    let session_status = covey
        .session_status(&held_claim.owner_session_token)
        .expect("session status");
    assert_eq!(
        session_status
            .session()
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
fn claim_subtask_rejects_changes_requested_work_subtask() {
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

    assert!(matches!(
        covey.claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                worker.clone(),
                covey::SubtaskId::parse(subtask_id.clone()).expect("valid subtask id"),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-subtask-changes-requested"),
            )
            .expect("valid claim-subtask request"),
        ),
        Err(CoveyError::IllegalTransition { from, to, object })
            if from == SubtaskState::ChangesRequested.into()
                && to == SubtaskState::Claimed.into()
                && object == ObjectType::Subtask
    ));
    assert_eq!(count_subtask_claim_events(&covey), before_events);
    assert_eq!(
        covey
            .session_status(&worker)
            .expect("session status")
            .session()
            .active_subtask_id(),
        None
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
        covey.claim_subtask(ClaimSubtaskReq::try_from_raw_parts(reviewer.clone(), covey::SubtaskId::parse(work_subtask_id.clone()).expect("valid subtask id"), covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"), id_key("claim-subtask-wrong-role-work")).expect("valid claim-subtask request")),
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
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                executor.clone(),
                covey::SubtaskId::parse(work_subtask_id.clone()).expect("valid subtask id"),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-subtask-review-seed"),
            )
            .expect("valid claim-subtask request"),
        )
        .expect("claim work target");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                executor.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start work target");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                executor.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                "blake3:wrong_role_review_target".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "wrong_role_review_target.json".into(),
                "blake3:wrong_role_review_target_paths".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish work target artifact");
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                executor.clone(),
                work_subtask_id.clone(),
                "blake3:wrong_role_review_target",
                Some("wrong_role_review_target_review".into()),
                1,
                id_key("request-review"),
            )
            .expect("valid review request"),
        )
        .expect("request review target");
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                executor.clone(),
                work_claim.claim_id,
                work_claim.fence_seq,
                id_key("release-claim"),
            )
            .expect("valid release-claim request"),
        )
        .expect("release work target claim");

    let review_subtask_id = covey
        .subtask_status(&work_subtask_id)
        .expect("work status")
        .reviews()
        .into_iter()
        .find(|review| review.review_id() == review_id)
        .map(|review| review.review_subtask_id().to_owned())
        .expect("review subtask id");
    let before_review_events = count_subtask_claim_events(&covey);

    assert!(matches!(
        covey.claim_subtask(ClaimSubtaskReq::try_from_raw_parts(executor, covey::SubtaskId::parse(review_subtask_id).expect("valid subtask id"), covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"), id_key("claim-subtask-wrong-role-review")).expect("valid claim-subtask request")),
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
        .cancel_meta_task(
            CancelMetaTaskReq::try_from_raw_parts(
                orch,
                meta_task_id.clone(),
                id_key("cancel-meta-task"),
            )
            .expect("valid cancel-meta-task request"),
        )
        .expect("cancel meta task");

    assert!(matches!(
        covey.claim_subtask(ClaimSubtaskReq::try_from_raw_parts(worker, covey::SubtaskId::parse(subtask_id).expect("valid subtask id"), covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"), id_key("claim-subtask-terminal-meta")).expect("valid claim-subtask request")),
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
            session_token: covey::SessionToken::parse(orch).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id).expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("work_2")),
            title: SubtaskTitle::parse("second work").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(2).expect("valid subtask priority"),
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
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                worker.clone(),
                covey::SubtaskId::parse(first_subtask_id.clone()).expect("valid subtask id"),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-subtask-first"),
            )
            .expect("valid claim-subtask request"),
        )
        .expect("claim first subtask");
    let before_events = count_subtask_claim_events(&covey);

    assert!(matches!(
        covey.claim_subtask(ClaimSubtaskReq::try_from_raw_parts(worker.clone(), covey::SubtaskId::parse(second_subtask_id).expect("valid subtask id"), covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"), id_key("claim-subtask-second")).expect("valid claim-subtask request")),
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
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                first_worker.clone(),
                covey::SubtaskId::parse(subtask_id.clone()).expect("valid subtask id"),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-subtask-live-claim-a"),
            )
            .expect("valid claim-subtask request"),
        )
        .expect("first claim succeeds");
    let before_events = count_subtask_claim_events(&covey);

    assert!(matches!(
        covey.claim_subtask(ClaimSubtaskReq::try_from_raw_parts(second_worker, covey::SubtaskId::parse(subtask_id.clone()).expect("valid subtask id"), covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"), id_key("claim-subtask-live-claim-b")).expect("valid claim-subtask request")),
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
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                first_worker.clone(),
                covey::SubtaskId::parse(subtask_id.clone()).expect("valid subtask id"),
                covey::LeaseDurationMs::parse(5).expect("valid lease duration"),
                id_key("claim-subtask-stale-a"),
            )
            .expect("valid claim-subtask request"),
        )
        .expect("first targeted claim");
    let before_reclaim_events = count_subtask_claim_events(&covey);

    rig.tick(6);
    let second_claim = covey
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                second_worker.clone(),
                covey::SubtaskId::parse(subtask_id.clone()).expect("valid subtask id"),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-subtask-stale-b"),
            )
            .expect("valid claim-subtask request"),
        )
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
    assert!(stale_session.session().active_subtask_id().is_none());
    let fresh_session = covey
        .session_status(&second_worker)
        .expect("fresh owner session");
    assert_eq!(
        fresh_session
            .session()
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
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                worker.clone(),
                covey::SubtaskId::parse(subtask_id.clone()).expect("valid subtask id"),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                idempotency_key.clone(),
            )
            .expect("valid claim-subtask request"),
        )
        .expect("initial targeted claim");
    let events_after_first = count_subtask_claim_events(&covey);

    let replay = covey
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                worker.clone(),
                covey::SubtaskId::parse(subtask_id).expect("valid subtask id"),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                idempotency_key,
            )
            .expect("valid claim-subtask request"),
        )
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
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                worker.clone(),
                covey::SubtaskId::parse(subtask_id).expect("valid subtask id"),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                idempotency_key.clone(),
            )
            .expect("valid claim-subtask request"),
        )
        .expect("initial targeted claim");
    let before_events = count_subtask_claim_events(&covey);

    assert!(matches!(
        covey.claim_subtask(ClaimSubtaskReq::try_from_raw_parts(worker.clone(), covey::SubtaskId::parse("work_1").expect("valid subtask id"), covey::LeaseDurationMs::parse(60_000).expect("valid lease duration"), idempotency_key.clone()).expect("valid claim-subtask request")),
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
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                first_worker.clone(),
                covey::SubtaskId::parse(subtask_id.clone()).expect("valid subtask id"),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-subtask-illegal-state-seed"),
            )
            .expect("valid claim-subtask request"),
        )
        .expect("seed claim");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                first_worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start seeded subtask");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                first_worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "blake3:illegal_state_target".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "illegal_state_target.json".into(),
                "blake3:illegal_state_target_paths".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish seeded artifact");
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                first_worker,
                claim.claim_id,
                claim.fence_seq,
                id_key("release-claim"),
            )
            .expect("valid release-claim request"),
        )
        .expect("release seeded claim");
    let before_events = count_subtask_claim_events(&covey);

    assert!(matches!(
        covey.claim_subtask(ClaimSubtaskReq::try_from_raw_parts(second_worker, covey::SubtaskId::parse(subtask_id).expect("valid subtask id"), covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"), id_key("claim-subtask-illegal-state")).expect("valid claim-subtask request")),
        Err(CoveyError::IllegalTransition { from, to, object })
            if from == SubtaskState::ArtifactPublished.into()
                && to == SubtaskState::Claimed.into()
                && object == ObjectType::Subtask
    ));
    assert_eq!(count_subtask_claim_events(&covey), before_events);
}

#[test]
fn claim_next_claims_appended_followup_for_changes_requested_work() {
    let rig = Rig::new();
    let covey = rig.covey();
    let subtask_id = seed_changes_requested_work_subtask(&rig);
    let conn = Connection::open(&rig.db_path).expect("open db");
    let followup_subtask_id: String = conn
        .query_row(
            "SELECT followup_subtask_id FROM review_followup_subtasks WHERE source_subtask_id = ?1",
            params![subtask_id.as_str()],
            |row| row.get(0),
        )
        .expect("follow-up subtask id");
    let worker = register(
        &covey,
        "worker-next-skips-changes-requested",
        "worker-next-skips-changes-requested",
        SessionRole::Executor,
    );
    let before_events = count_subtask_claim_events(&covey);

    let next = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next-skips-changes-requested"),
            )
            .expect("valid claim-next request"),
        )
        .expect("next claim result");

    let claim = next.expect("follow-up work should be claimable");
    assert_eq!(claim.subtask_id, followup_subtask_id);
    assert_ne!(claim.subtask_id, subtask_id);
    assert_eq!(count_subtask_claim_events(&covey), before_events + 1);
    let followup_subtask =
        covey::SubtaskId::parse(followup_subtask_id).expect("valid follow-up id");
    assert_eq!(
        covey
            .subtask_status(&subtask_id)
            .expect("status")
            .subtask()
            .state(),
        SubtaskState::ChangesRequested
    );
    assert_eq!(
        covey
            .session_status(&worker)
            .expect("worker status")
            .session()
            .active_subtask_id(),
        Some(&followup_subtask)
    );
}

#[test]
fn claim_next_repairs_missing_followup_for_changes_requested_work() {
    let rig = Rig::new();
    let covey = rig.covey();
    let subtask_id = seed_changes_requested_work_subtask(&rig);
    let conn = Connection::open(&rig.db_path).expect("open db");
    let review_id: String = conn
        .query_row(
            "SELECT review_id FROM review_followup_subtasks WHERE source_subtask_id = ?1",
            params![subtask_id.as_str()],
            |row| row.get(0),
        )
        .expect("review id");
    let stale_followup_subtask_id: String = conn
        .query_row(
            "SELECT followup_subtask_id FROM review_followup_subtasks WHERE review_id = ?1",
            params![review_id.as_str()],
            |row| row.get(0),
        )
        .expect("stale follow-up id");
    conn.execute(
        "DELETE FROM review_followup_subtasks WHERE review_id = ?1",
        params![review_id.as_str()],
    )
    .expect("remove follow-up mapping");
    conn.execute(
        "DELETE FROM subtask_fence_counter WHERE subtask_id = ?1",
        params![stale_followup_subtask_id.as_str()],
    )
    .expect("remove stale follow-up fence counter");
    conn.execute(
        "DELETE FROM subtasks WHERE subtask_id = ?1",
        params![stale_followup_subtask_id.as_str()],
    )
    .expect("remove stale follow-up block");
    drop(conn);

    let worker = register(
        &covey,
        "worker-next-repairs-missing-followup",
        "worker-next-repairs-missing-followup",
        SessionRole::Executor,
    );
    let before_events = count_subtask_claim_events(&covey);

    let claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next-repairs-missing-followup"),
            )
            .expect("valid claim-next request"),
        )
        .expect("next claim result")
        .expect("missing changes-requested follow-up should be repaired and claimed");

    assert_ne!(claim.subtask_id, subtask_id);
    assert_eq!(count_subtask_claim_events(&covey), before_events + 1);
    assert_eq!(
        covey
            .subtask_status(&subtask_id)
            .expect("source status")
            .subtask()
            .state(),
        SubtaskState::ChangesRequested
    );
    assert_eq!(
        covey
            .subtask_status(&claim.subtask_id)
            .expect("follow-up status")
            .subtask()
            .state(),
        SubtaskState::Claimed
    );
    assert_eq!(
        covey
            .session_status(&worker)
            .expect("worker status")
            .session()
            .active_subtask_id()
            .map(|subtask_id| subtask_id.as_str()),
        Some(claim.subtask_id.as_str())
    );

    let conn = Connection::open(&rig.db_path).expect("open db");
    let repaired_followup_id: String = conn
        .query_row(
            "SELECT followup_subtask_id FROM review_followup_subtasks WHERE review_id = ?1",
            params![review_id.as_str()],
            |row| row.get(0),
        )
        .expect("repaired follow-up id");
    assert_eq!(repaired_followup_id, claim.subtask_id);
}

#[test]
fn claim_next_treats_applied_review_followup_as_satisfying_source_dependency() {
    let rig = Rig::new();
    let covey = rig.covey();
    let source_subtask_id = seed_changes_requested_work_subtask(&rig);
    let meta_task_id = covey
        .subtask_status(&source_subtask_id)
        .expect("source status")
        .subtask()
        .meta_task_id
        .to_string();
    let conn = Connection::open(&rig.db_path).expect("open db");
    let followup_subtask_id: String = conn
        .query_row(
            "SELECT followup_subtask_id FROM review_followup_subtasks WHERE source_subtask_id = ?1",
            params![source_subtask_id.as_str()],
            |row| row.get(0),
        )
        .expect("follow-up subtask id");
    drop(conn);

    let orchestrator = register(
        &covey,
        "orchestrator-followup-unblocks-dependent",
        "orchestrator-followup-unblocks-dependent",
        SessionRole::Orchestrator,
    );
    let dependent_subtask_id = covey
        .create_subtask(
            CreateSubtaskRequest::try_from_raw_parts(
                orchestrator.clone(),
                meta_task_id,
                Some("dependent_after_review_followup".into()),
                "dependent after review follow-up",
                100,
                id_key("create-dependent"),
            )
            .expect("valid dependent create request"),
        )
        .expect("create dependent");
    let conn = Connection::open(&rig.db_path).expect("open db");
    conn.execute(
        "INSERT INTO subtask_dependencies (subtask_id, depends_on_subtask_id, source_ref, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            dependent_subtask_id.as_str(),
            source_subtask_id.as_str(),
            "source-dependency",
            1_700_000_000_000_i64
        ],
    )
    .expect("insert dependency on failed source");
    drop(conn);

    let worker = register(
        &covey,
        "worker-followup-unblocks-dependent",
        "worker-followup-unblocks-dependent",
        SessionRole::Executor,
    );
    attest(&covey, &worker);
    let reviewer = register(
        &covey,
        "reviewer-followup-unblocks-dependent",
        "reviewer-followup-unblocks-dependent",
        SessionRole::Reviewer,
    );
    attest(&covey, &reviewer);
    let gate = register(
        &covey,
        "gate-followup-unblocks-dependent",
        "gate-followup-unblocks-dependent",
        SessionRole::ApplyGate,
    );

    let followup_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-followup"),
            )
            .expect("valid claim request"),
        )
        .expect("claim follow-up")
        .expect("follow-up claim");
    assert_eq!(followup_claim.subtask_id, followup_subtask_id);
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                followup_claim.claim_id.clone(),
                followup_claim.fence_seq,
                id_key("start-followup"),
            )
            .expect("valid start request"),
        )
        .expect("start follow-up");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                followup_claim.claim_id.clone(),
                followup_claim.fence_seq,
                "blake3:followup_unblocks_dependent".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "followup-unblocks-dependent.json".into(),
                "blake3:followup_unblocks_dependent_paths".into(),
                id_key("publish-followup"),
            )
            .expect("valid follow-up publish"),
        )
        .expect("publish follow-up");
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                followup_subtask_id.clone(),
                "blake3:followup_unblocks_dependent",
                Some("review_followup_unblocks_dependent".into()),
                1,
                id_key("request-followup-review"),
            )
            .expect("valid review request"),
        )
        .expect("request follow-up review");
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                worker.clone(),
                followup_claim.claim_id,
                followup_claim.fence_seq,
                id_key("release-followup"),
            )
            .expect("valid release request"),
        )
        .expect("release follow-up claim");

    let review_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                reviewer.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-review"),
            )
            .expect("valid review claim request"),
        )
        .expect("claim review")
        .expect("review claim");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                review_claim.claim_id.clone(),
                review_claim.fence_seq,
                id_key("start-review"),
            )
            .expect("valid review start request"),
        )
        .expect("start review");
    covey
        .decide_review(
            DecideReviewReq::try_from_raw_parts(
                reviewer,
                review_id.clone(),
                review_claim.claim_id,
                review_claim.fence_seq,
                covey::ReviewVerdict::Approve,
                "blake3:followup_unblocks_dependent_findings".into(),
                id_key("approve-followup"),
            )
            .expect("valid review decision request"),
        )
        .expect("approve follow-up");
    let queue_id = covey
        .enqueue_for_apply(
            EnqueueForApplyReq::try_from_raw_parts(
                orchestrator,
                "blake3:followup_unblocks_dependent".into(),
                followup_subtask_id.clone(),
                SettlementTarget::Canonical,
                id_key("enqueue-followup"),
            )
            .expect("valid enqueue request"),
        )
        .expect("enqueue follow-up");
    let queue_claim = covey
        .claim_next_ready_queue_item(claim_ready_queue_req(
            gate.clone(),
            30_000,
            id_key("claim-ready-queue"),
        ))
        .expect("claim ready queue")
        .expect("queue claim");
    assert_eq!(queue_claim.queue_id, queue_id);
    record_apply_verification(
        &covey,
        &gate,
        &queue_claim.queue_id,
        "blake3:followup_unblocks_dependent",
        &review_id,
        "blake3:followup_unblocks_dependent_findings",
        queue_claim.claim_fence_seq,
    );
    covey
        .mark_applied(mark_applied_req(
            gate,
            queue_claim.queue_id.to_string(),
            queue_claim.claim_fence_seq,
            id_key("mark-followup-applied"),
        ))
        .expect("mark follow-up applied");

    assert_eq!(
        covey
            .subtask_status(&source_subtask_id)
            .expect("source status")
            .subtask()
            .state(),
        SubtaskState::ChangesRequested
    );
    assert_eq!(
        covey
            .subtask_status(&followup_subtask_id)
            .expect("follow-up status")
            .subtask()
            .state(),
        SubtaskState::Applied
    );

    let dependent_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker,
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-dependent"),
            )
            .expect("valid dependent claim request"),
        )
        .expect("claim dependent")
        .expect("dependent should become claimable after follow-up is applied");
    assert_eq!(dependent_claim.subtask_id, dependent_subtask_id);
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
        covey.claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                targeted_session,
                covey::SubtaskId::parse(targeted_subtask_id).expect("valid subtask id"),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-subtask-race"),
            )
            .expect("valid claim-subtask request"),
        )
    });

    let next_db = rig.db_path.clone();
    let next_clock = rig.clock.clone();
    let next_barrier = barrier.clone();
    let next_session = next_worker.clone();
    let next_handle = thread::spawn(move || {
        let covey = Covey::open_with_clock(next_db, next_clock).expect("open next covey");
        next_barrier.wait();
        covey.claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                next_session,
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next-race"),
            )
            .expect("valid claim-next request"),
        )
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
    assert_eq!(status.subtask().state(), SubtaskState::Claimed);
    assert_eq!(
        status.subtask().active_claim_id().map(AsRef::as_ref),
        Some(winner.claim_id.as_str())
    );
    assert_eq!(
        status.claim().as_ref().map(|claim| claim.claim_id.as_str()),
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
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start");
    rig.tick(1);
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "blake3:a".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "a.json".into(),
                "blake3:paths_a".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish a");
    rig.tick(1);
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                "blake3:a",
                Some("review_a".into()),
                2,
                id_key("request-review"),
            )
            .expect("valid review request"),
        )
        .expect("request review");

    rig.tick(1);
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id,
                claim.fence_seq,
                "blake3:b".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "b.json".into(),
                "blake3:paths_b".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish b");

    let status = covey.subtask_status(&subtask_id).expect("status");
    assert_eq!(
        status.subtask().artifact_digest().map(AsRef::as_ref),
        Some("blake3:b")
    );
    let review = status
        .reviews()
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
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "blake3:a".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "a.json".into(),
                "blake3:paths_a".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish a");
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                "blake3:a",
                Some("review_stale".into()),
                2,
                id_key("request-review"),
            )
            .expect("valid review request"),
        )
        .expect("request review");
    rig.tick(1);
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "blake3:b".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "b.json".into(),
                "blake3:paths_b".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish b");
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id,
                claim.fence_seq,
                id_key("release-claim"),
            )
            .expect("valid release-claim request"),
        )
        .expect("release worker");

    let review_subtask_id = covey
        .subtask_status(&subtask_id)
        .expect("status")
        .reviews()
        .into_iter()
        .find(|review| review.review_id() == review_id)
        .map(|review| review.review_subtask_id().to_owned())
        .expect("review subtask");

    let review_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                reviewer.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim review")
        .expect("review claim");
    assert_eq!(review_claim.subtask_id, review_subtask_id);
    assert!(matches!(
        covey.start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                review_claim.claim_id.clone(),
                review_claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        ),
        Err(CoveyError::IllegalTransition { object, .. }) if object == ObjectType::Review
    ));

    let status = covey.subtask_status(&subtask_id).expect("status");
    assert_eq!(
        status.subtask().artifact_digest().map(AsRef::as_ref),
        Some("blake3:b")
    );
    assert_eq!(status.subtask().state(), SubtaskState::ArtifactPublished);
    assert_eq!(
        covey
            .subtask_status(&review_subtask_id)
            .expect("review subtask status")
            .subtask()
            .state(),
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
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "queue",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("meta task");
    let first = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id.clone())
                .expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("ready_1")),
            title: SubtaskTitle::parse("first").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask1");
    let second = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id).expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("ready_2")),
            title: SubtaskTitle::parse("second").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(2).expect("valid subtask priority"),
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
            .claim_next_subtask(
                ClaimNextReq::try_from_raw_parts(worker.clone(), 30_000, id_key("claim-next"))
                    .expect("valid claim-next request"),
            )
            .expect("claim")
            .expect("claim result");
        covey
            .start_subtask(
                StartSubtaskReq::try_from_raw_parts(
                    worker.clone(),
                    claim.claim_id.clone(),
                    claim.fence_seq,
                    id_key("start-subtask"),
                )
                .expect("valid start-subtask request"),
            )
            .expect("start");
        covey
            .publish_artifact(
                PublishArtifactReq::try_from_raw_parts(
                    worker.clone(),
                    claim.claim_id.clone(),
                    claim.fence_seq,
                    digest.into(),
                    ArtifactKind::PatchBundle,
                    "base".into(),
                    format!("{digest}.json"),
                    format!("blake3:paths_{created_at}"),
                    id_key("publish-artifact"),
                )
                .expect("valid artifact publication request"),
            )
            .expect("publish");
        let review_id = covey
            .request_review(
                RequestReviewReq::try_from_raw_parts(
                    worker.clone(),
                    subtask_id.to_string(),
                    digest,
                    Some(format!("review_{digest}")),
                    1,
                    id_key("request-review"),
                )
                .expect("valid review request"),
            )
            .expect("review req");
        covey
            .release_claim(
                ReleaseClaimReq::try_from_raw_parts(
                    worker.clone(),
                    claim.claim_id,
                    claim.fence_seq,
                    id_key("release-claim"),
                )
                .expect("valid release-claim request"),
            )
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
            .claim_next_subtask(
                ClaimNextReq::try_from_raw_parts(reviewer.clone(), 30_000, id_key("claim-next"))
                    .expect("valid claim-next request"),
            )
            .expect("claim review")
            .expect("review claim");
        covey
            .start_subtask(
                StartSubtaskReq::try_from_raw_parts(
                    reviewer.clone(),
                    review_claim.claim_id.clone(),
                    review_claim.fence_seq,
                    id_key("start-subtask"),
                )
                .expect("valid start-subtask request"),
            )
            .expect("start review");
        covey
            .decide_review(
                DecideReviewReq::try_from_raw_parts(
                    reviewer,
                    review_id.clone(),
                    review_claim.claim_id,
                    review_claim.fence_seq,
                    covey::ReviewVerdict::Approve,
                    "blake3:findings".into(),
                    id_key("decide-review"),
                )
                .expect("valid review decision request"),
            )
            .expect("decide");
        covey
            .enqueue_for_apply(
                EnqueueForApplyReq::try_from_raw_parts(
                    orch.clone(),
                    digest.into(),
                    subtask_id.to_string(),
                    SettlementTarget::Canonical,
                    id_key("enqueue-for-apply"),
                )
                .expect("valid enqueue-for-apply request"),
            )
            .expect("enqueue");
        review_records.push((subtask_id.to_string(), digest.to_string(), review_id));
        rig.tick(1);
    }

    let queued = covey.fetch_ready_queue(10).expect("fetch queue");
    assert_eq!(queued.len(), 2);
    assert_eq!(queued[0].subtask_id(), first);
    assert_eq!(queued[1].subtask_id(), second);

    let queue_claim = covey
        .claim_next_ready_queue_item(claim_ready_queue_req(
            gate.clone(),
            30_000,
            id_key("claim-ready-queue"),
        ))
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
        .mark_applied(mark_applied_req(
            gate.clone(),
            queue_claim.queue_id.to_string(),
            queue_claim.claim_fence_seq,
            id_key("mark-applied"),
        ))
        .expect("applied");
    assert_eq!(
        covey
            .subtask_status(&first)
            .expect("status")
            .subtask()
            .state(),
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
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "queue reclaim",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("meta");
    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id).expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("queue_reclaim")),
            title: SubtaskTitle::parse("queue reclaim").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask");
    let claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "blake3:queue_reclaim".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "queue_reclaim.json".into(),
                "blake3:paths_queue_reclaim".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish");
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                "blake3:queue_reclaim",
                Some("review_queue_reclaim".into()),
                1,
                id_key("request-review"),
            )
            .expect("valid review request"),
        )
        .expect("review req");
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id,
                claim.fence_seq,
                id_key("release-claim"),
            )
            .expect("valid release-claim request"),
        )
        .expect("release");
    let review_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                reviewer.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim review")
        .expect("review claim");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                review_claim.claim_id.clone(),
                review_claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start review");
    covey
        .decide_review(
            DecideReviewReq::try_from_raw_parts(
                reviewer,
                review_id.clone(),
                review_claim.claim_id,
                review_claim.fence_seq,
                covey::ReviewVerdict::Approve,
                "blake3:findings_queue_reclaim".into(),
                id_key("decide-review"),
            )
            .expect("valid review decision request"),
        )
        .expect("approve");
    covey
        .enqueue_for_apply(
            EnqueueForApplyReq::try_from_raw_parts(
                orch,
                "blake3:queue_reclaim".into(),
                subtask_id.clone(),
                SettlementTarget::Canonical,
                id_key("enqueue-for-apply"),
            )
            .expect("valid enqueue-for-apply request"),
        )
        .expect("enqueue");

    let first_claim = covey
        .claim_next_ready_queue_item(claim_ready_queue_req(
            gate_a.clone(),
            10_000,
            id_key("claim-ready-queue"),
        ))
        .expect("claim ready queue")
        .expect("first queue claim");
    rig.tick(10_001);

    assert!(matches!(
        covey.mark_applied(mark_applied_req(
            gate_a,
            first_claim.queue_id.to_string(),
            first_claim.claim_fence_seq,
            id_key("mark-applied"),
        )),
        Err(CoveyError::LeaseExpired { object_id }) if object_id == first_claim.queue_id
    ));

    let second_claim = covey
        .claim_next_ready_queue_item(claim_ready_queue_req(
            gate_b.clone(),
            10_000,
            id_key("claim-ready-queue"),
        ))
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
        .mark_applied(mark_applied_req(
            gate_b,
            second_claim.queue_id.to_string(),
            second_claim.claim_fence_seq,
            id_key("mark-applied"),
        ))
        .expect("applied after reclaim");
    assert_eq!(
        covey
            .subtask_status(&subtask_id)
            .expect("status")
            .subtask()
            .state(),
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
        .request_reservation(
            RequestReservationReq::try_from_raw_parts(
                orch.clone(),
                subtask_id.clone(),
                ScopeClass::Subtree,
                "src/covey",
                vec![],
                60_000,
                id_key("request-reservation"),
            )
            .expect("valid reservation request"),
        )
        .expect("active reservation");
    rig.tick(1);
    let released = covey
        .request_reservation(
            RequestReservationReq::try_from_raw_parts(
                orch.clone(),
                subtask_id,
                ScopeClass::RepoGlobal,
                "repo",
                vec![],
                60_000,
                id_key("request-reservation"),
            )
            .expect("valid reservation request"),
        )
        .expect("released reservation");
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);
    assert!(matches!(
        covey.release_reservation(
            ReleaseReservationReq::try_from_raw_parts(
                worker.clone(),
                released.clone(),
                id_key("release-reservation"),
            )
            .expect("valid release reservation request"),
        ),
        Err(CoveyError::WrongRole { actual, .. }) if actual == SessionRole::Executor
    ));
    covey
        .release_reservation(
            ReleaseReservationReq::try_from_raw_parts(
                orch.clone(),
                released,
                id_key("release-reservation"),
            )
            .expect("valid release reservation request"),
        )
        .expect("release reservation");

    let overlaps = covey
        .find_overlapping_reservations(
            OverlapQueryReq::try_from_parts(ScopeClass::ExactPath, "src/covey/store.rs", vec![])
                .expect("valid overlap query"),
        )
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
        .request_reservation(
            RequestReservationReq::try_from_raw_parts(
                orch.clone(),
                subtask_id,
                ScopeClass::ExactPath,
                "src/covey/store.rs",
                vec![],
                10_000,
                id_key("request-reservation"),
            )
            .expect("valid reservation request"),
        )
        .expect("reservation");
    let original = covey
        .find_overlapping_reservations(
            OverlapQueryReq::try_from_parts(ScopeClass::ExactPath, "src/covey/store.rs", vec![])
                .expect("valid overlap query"),
        )
        .expect("overlaps")
        .into_iter()
        .find(|reservation| reservation.reservation_id == reservation_id)
        .expect("reservation row");

    rig.tick(5_000);
    let renewed = covey
        .renew_reservation(
            RenewReservationReq::try_from_raw_parts(
                orch.clone(),
                reservation_id.clone(),
                20_000,
                id_key("renew-reservation"),
            )
            .expect("valid renew reservation request"),
        )
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
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id).expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("shadow_work")),
            title: SubtaskTitle::parse("shadow work").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(2).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create second subtask");

    let first_reservation = covey
        .request_reservation(
            RequestReservationReq::try_from_raw_parts(
                orch.clone(),
                first_subtask_id.clone(),
                ScopeClass::Subtree,
                "src/covey",
                vec![],
                60_000,
                id_key("request-reservation"),
            )
            .expect("valid reservation request"),
        )
        .expect("first reservation");
    rig.tick(1);
    let second_reservation = covey
        .request_reservation(
            RequestReservationReq::try_from_raw_parts(
                orch.clone(),
                second_subtask_id.clone(),
                ScopeClass::ExactPath,
                "src/covey/store.rs",
                vec![],
                60_000,
                id_key("request-reservation"),
            )
            .expect("valid reservation request"),
        )
        .expect("second reservation");

    let conflicts = covey.list_conflicts().expect("list conflicts");
    let conflict = conflicts
        .iter()
        .find(|conflict| {
            conflict.conflict_kind() == "reservation_overlap"
                && conflict.object_type() == ObjectType::Reservation
        })
        .expect("reservation overlap conflict");
    let payload: ReservationOverlapConflictPayload =
        serde_json::from_str(&conflict.payload_json()).expect("typed conflict payload");

    assert_eq!(
        conflict.resolution_state(),
        covey::ConflictResolutionState::Open
    );
    assert_eq!(payload.reservation_id(), second_reservation);
    assert_eq!(payload.overlapping_reservation_id(), first_reservation);
    assert_eq!(payload.owner_subtask_id(), second_subtask_id);
    assert_eq!(payload.overlapping_owner_subtask_id(), first_subtask_id);
    assert_eq!(payload.scope_class(), ScopeClass::ExactPath);
    assert_eq!(payload.scope_key(), "src/covey/store.rs");
    assert_eq!(payload.overlapping_scope_class(), ScopeClass::Subtree);
    assert_eq!(payload.overlapping_scope_key(), "src/covey");
}

#[test]
fn concurrent_pool_claims_distribute_exactly_once() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(&covey, "orch", "orch", SessionRole::Orchestrator);
    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "parallel",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("meta task");
    let mut session_tokens = Vec::new();
    for idx in 0..10 {
        covey
            .create_subtask(CreateSubtaskRequest {
                session_token: covey::SessionToken::parse(orch.clone())
                    .expect("valid session token"),
                meta_task_id: covey::MetaTaskId::parse(meta_task_id.clone())
                    .expect("valid meta-task id"),
                subtask_id: Some(parse_subtask_id(&format!("task_{idx}"))),
                title: SubtaskTitle::parse(format!("task {idx}")).expect("valid subtask title"),
                priority: covey::SubtaskPriority::parse(idx).expect("valid subtask priority"),
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
                .claim_next_subtask(
                    ClaimNextReq::try_from_raw_parts(session_token, 30_000, id_key("claim-next"))
                        .expect("valid claim-next request"),
                )
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
                .claim_next_subtask(
                    ClaimNextReq::try_from_raw_parts(session_token, 30_000, id_key("claim-next"))
                        .expect("valid claim-next request"),
                )
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
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim")
        .expect("claim result");

    rig.tick(60_000);
    let _reap_result = covey.reap_stale_sessions(45_000).expect("reap stale");

    let stale_session = covey.session_status(&worker).expect("session");
    assert_eq!(stale_session.session().state(), SessionState::Stale);
    assert!(stale_session.session().active_subtask_id().is_none());
    let before_expire = covey.subtask_status(&subtask_id).expect("subtask");
    assert!(before_expire.claim().is_none());
    assert_eq!(before_expire.subtask().state(), SubtaskState::Available);

    let _ = covey.expire_old_claims().expect("expire claims");

    let after_expire = covey.subtask_status(&subtask_id).expect("subtask");
    assert!(after_expire.claim().is_none());
    assert_eq!(after_expire.subtask().state(), SubtaskState::Available);
    let session = covey.session_status(&worker).expect("session after expire");
    assert_eq!(session.session().state(), SessionState::Stale);
    assert!(session.session().active_subtask_id().is_none());
}

#[test]
fn exiting_session_immediately_expires_held_claim_and_clears_subtask() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let worker = register(
        &covey,
        "worker-exit-claim",
        "worker-exit-claim",
        SessionRole::Executor,
    );

    let _claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next-before-exit"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim")
        .expect("claim result");

    close_session(&covey, &worker).expect("exit session with active claim");

    let exited_session = covey.session_status(&worker).expect("session");
    assert_eq!(exited_session.session().state(), SessionState::Exited);
    assert!(exited_session.session().active_subtask_id().is_none());

    let subtask = covey.subtask_status(&subtask_id).expect("subtask");
    assert!(subtask.claim().is_none());
    assert_eq!(subtask.subtask().state(), SubtaskState::Available);
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
    assert_eq!(events[0].seq(), 1);
    assert_eq!(events[1].seq(), 2);
    assert_eq!(events[2].seq(), 3);
    assert_eq!(events[0].event_type(), EventType::SessionRegistered);
    assert_eq!(events[0].object_type(), ObjectType::Session);
    assert_eq!(events[0].actor_kind(), ActorKind::Session);
    assert_eq!(
        events[0].session_token().map(SessionToken::as_str),
        Some(sess.as_str())
    );
    assert_eq!(events[1].actor_kind(), ActorKind::Session);
    assert_eq!(
        events[1].session_token().map(SessionToken::as_str),
        Some(sess.as_str())
    );
    assert_eq!(events[2].actor_kind(), ActorKind::Session);
    assert_eq!(
        events[2].session_token().map(SessionToken::as_str),
        Some(sess.as_str())
    );

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
    assert!(after_first.iter().all(|event| event.seq() > 1));
}

#[test]
fn observability_queries_report_stuck_work_expiring_claims_and_queue_metrics() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_meta_task_id, subtask_id) = seed_work_subtask(&rig);
    let gate = register(&covey, "gate", "gate", SessionRole::ApplyGate);
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);

    let claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start");
    rig.tick(5_000);

    let stuck = covey.list_stuck_subtasks(1_000, 10).expect("stuck");
    assert_eq!(stuck.len(), 1);
    assert_eq!(stuck[0].subtask().subtask_id, subtask_id);
    assert_eq!(
        stuck[0].session().expect("stuck session").session_token,
        worker
    );

    let expiring = covey.list_expiring_claims(30_000, 10).expect("expiring");
    assert_eq!(expiring.len(), 1);
    assert_eq!(expiring[0].claim().claim_id, claim.claim_id);
    assert_eq!(expiring[0].subtask().subtask_id, subtask_id);

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
        .enqueue_for_apply(
            EnqueueForApplyReq::try_from_raw_parts(
                register(
                    &covey,
                    "orch_metrics",
                    "orch_metrics",
                    SessionRole::Orchestrator,
                ),
                "blake3:ready_metrics".into(),
                subtask_id,
                SettlementTarget::Canonical,
                id_key("enqueue-for-apply"),
            )
            .expect("valid enqueue-for-apply request"),
        )
        .expect("enqueue");
    let _ = covey
        .mark_in_flight(mark_in_flight_req(
            gate,
            queue_id,
            30_000,
            id_key("mark-in-flight"),
        ))
        .expect("flight");

    let metrics = covey.ready_queue_metrics().expect("metrics");
    assert_eq!(metrics.queued_count(), 0);
    assert_eq!(metrics.in_flight_count(), 1);
    assert!(metrics.oldest_queued_age_ms().is_none());
    assert!(metrics.oldest_in_flight_age_ms().is_some());
}

#[test]
fn system_events_use_system_actor_without_session_token_and_generated_tokens_are_not_spoofable() {
    let rig = Rig::new();
    let covey = rig.covey();

    let handle = covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                "principal",
                "instance",
                SessionRole::Executor,
                id_key("register-session"),
            )
            .expect("valid session registration request"),
        )
        .expect("register");
    assert_ne!(handle.session_token, "system");

    let worker = register(&covey, "worker", "worker", SessionRole::Executor);
    assert_ne!(worker, "system");
    rig.tick(60_000);
    let _reap_result = covey.reap_stale_sessions(45_000).expect("reap");

    let events = covey.fetch_events(0, 10).expect("events");
    let system_event = events
        .iter()
        .find(|event| event.event_type() == EventType::SessionsReaped)
        .expect("system reap event");
    assert_eq!(system_event.actor_kind(), ActorKind::System);
    assert_eq!(system_event.session_token(), None);
}

#[test]
fn request_review_rejects_stale_artifact_digest_after_republish() {
    let rig = Rig::new();
    let covey = rig.covey();
    let (_, subtask_id) = seed_work_subtask(&rig);
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);

    let claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "blake3:artifact_a".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "artifact_a.json".into(),
                "blake3:paths_a".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish a");
    let first_review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                "blake3:artifact_a",
                Some("review_stale_a".into()),
                1,
                id_key("request-review"),
            )
            .expect("valid review request"),
        )
        .expect("request review a");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "blake3:artifact_b".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "artifact_b.json".into(),
                "blake3:paths_b".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish b");

    assert!(matches!(
        covey.request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                "blake3:artifact_a",
                Some("review_stale_b".into()),
                1,
                id_key("request-review"),
            )
            .expect("valid review request"),
        ),
        Err(CoveyError::UnknownArtifactDigest { digest }) if digest == "blake3:artifact_a"
    ));

    let status = covey.subtask_status(&subtask_id).expect("status");
    let first_review = status
        .reviews()
        .iter()
        .find(|review| review.review_id() == first_review_id)
        .expect("first review");
    assert_eq!(first_review.state(), covey::ReviewState::Superseded);

    covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                subtask_id,
                "blake3:artifact_b",
                Some("review_fresh".into()),
                1,
                id_key("request-review"),
            )
            .expect("valid review request"),
        )
        .expect("request review b");
}

#[test]
fn reservation_overlap_conflicts_resolve_when_reservations_release_or_expire() {
    let rig = Rig::new();
    let covey = rig.covey();
    let orch = register(&covey, "orch", "orch", SessionRole::Orchestrator);

    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "reservations",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("meta");
    let subtask_a = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id.clone())
                .expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("reservation_a")),
            title: SubtaskTitle::parse("reservation a").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask a");
    let subtask_b = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id).expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("reservation_b")),
            title: SubtaskTitle::parse("reservation b").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(2).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask b");

    let reservation_a = covey
        .request_reservation(
            RequestReservationReq::try_from_raw_parts(
                orch.clone(),
                subtask_a.clone(),
                ScopeClass::ExactPath,
                "src/lib.rs",
                Vec::new(),
                30_000,
                id_key("request-reservation"),
            )
            .expect("valid reservation request"),
        )
        .expect("reservation a");
    let reservation_b = covey
        .request_reservation(
            RequestReservationReq::try_from_raw_parts(
                orch.clone(),
                subtask_b.clone(),
                ScopeClass::ExactPath,
                "src/lib.rs",
                Vec::new(),
                30_000,
                id_key("request-reservation"),
            )
            .expect("valid reservation request"),
        )
        .expect("reservation b");

    let release_conflict = covey
        .list_conflicts()
        .expect("conflicts")
        .into_iter()
        .find(|conflict| conflict.resolution_state() == covey::ConflictResolutionState::Open)
        .expect("open conflict");
    let release_payload: ReservationOverlapConflictPayload =
        serde_json::from_str(&release_conflict.payload_json()).expect("release payload");
    let release_pairs = HashSet::from([
        release_payload.reservation_id().to_owned(),
        release_payload.overlapping_reservation_id().to_owned(),
    ]);
    assert_eq!(
        release_pairs,
        HashSet::from([reservation_a.clone(), reservation_b.clone()])
    );

    covey
        .release_reservation(
            ReleaseReservationReq::try_from_raw_parts(
                orch.clone(),
                reservation_b,
                id_key("release-reservation"),
            )
            .expect("valid release reservation request"),
        )
        .expect("release reservation");

    let release_conflict_state = covey
        .list_conflicts()
        .expect("conflicts after release")
        .into_iter()
        .find(|conflict| conflict.conflict_id() == release_conflict.conflict_id())
        .expect("released conflict row");
    assert_eq!(
        release_conflict_state.resolution_state(),
        covey::ConflictResolutionState::Resolved
    );

    let reservation_c = covey
        .request_reservation(
            RequestReservationReq::try_from_raw_parts(
                orch.clone(),
                subtask_a,
                ScopeClass::ExactPath,
                "src/main.rs",
                Vec::new(),
                5,
                id_key("request-reservation"),
            )
            .expect("valid reservation request"),
        )
        .expect("reservation c");
    let reservation_d = covey
        .request_reservation(
            RequestReservationReq::try_from_raw_parts(
                orch.clone(),
                subtask_b,
                ScopeClass::ExactPath,
                "src/main.rs",
                Vec::new(),
                5,
                id_key("request-reservation"),
            )
            .expect("valid reservation request"),
        )
        .expect("reservation d");
    let expire_conflict = covey
        .list_conflicts()
        .expect("conflicts before expire")
        .into_iter()
        .find(|conflict| {
            let payload: ReservationOverlapConflictPayload =
                serde_json::from_str(&conflict.payload_json()).expect("expire payload");
            let ids = HashSet::from([
                payload.reservation_id().to_owned(),
                payload.overlapping_reservation_id().to_owned(),
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
        .find(|conflict| conflict.conflict_id() == expire_conflict.conflict_id())
        .expect("expired conflict row");
    assert_eq!(
        expire_conflict_state.resolution_state(),
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

    let err = covey
        .fetch_events(0, 10)
        .expect_err("malformed raw event payloads must fail at load time");
    assert!(matches!(err, CoveyError::DatabaseError(_)));
    assert!(
        err.to_string()
            .contains("event payload does not match session_registered"),
        "unexpected error: {err}"
    );
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
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "queue",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("meta task");
    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id).expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("queue_target")),
            title: SubtaskTitle::parse("queue target").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask");

    let claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "blake3:queue".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "queue.json".into(),
                "blake3:paths_queue".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish");
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                "blake3:queue",
                Some("review_queue".into()),
                1,
                id_key("request-review"),
            )
            .expect("valid review request"),
        )
        .expect("review req");
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id,
                claim.fence_seq,
                id_key("release-claim"),
            )
            .expect("valid release-claim request"),
        )
        .expect("release");
    let review_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                reviewer.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim review")
        .expect("review claim");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                review_claim.claim_id.clone(),
                review_claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start review");
    covey
        .decide_review(
            DecideReviewReq::try_from_raw_parts(
                reviewer.clone(),
                review_id.clone(),
                review_claim.claim_id,
                review_claim.fence_seq,
                covey::ReviewVerdict::Approve,
                "blake3:findings".into(),
                id_key("decide-review"),
            )
            .expect("valid review decision request"),
        )
        .expect("approve");

    let queue_id = covey
        .enqueue_for_apply(
            EnqueueForApplyReq::try_from_raw_parts(
                orch.clone(),
                "blake3:queue".into(),
                subtask_id,
                SettlementTarget::Canonical,
                id_key("enqueue-for-apply"),
            )
            .expect("valid enqueue-for-apply request"),
        )
        .expect("enqueue");

    assert!(matches!(
        covey.mark_applied(mark_applied_req(
            gate.clone(),
            queue_id.clone(),
            FenceSeq::parse(1).expect("valid fence"),
            id_key("mark-applied"),
        )),
        Err(CoveyError::IllegalTransition { from, to, object })
            if from == ReadyQueueState::Queued.into()
                && to == ReadyQueueState::Applied.into()
                && object == ObjectType::ReadyQueue
    ));
    assert!(matches!(
        covey.mark_in_flight(mark_in_flight_req(
            gate.clone(),
            "missing",
            30_000,
            id_key("mark-in-flight"),
        )),
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
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "queue drift",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("meta task");
    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id).expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("queue_drift_target")),
            title: SubtaskTitle::parse("queue drift target").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask");

    let claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "blake3:queue_drift".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "queue_drift.json".into(),
                "blake3:paths_queue_drift".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish");
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                "blake3:queue_drift",
                Some("review_queue_drift".into()),
                1,
                id_key("request-review"),
            )
            .expect("valid review request"),
        )
        .expect("review req");
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id,
                claim.fence_seq,
                id_key("release-claim"),
            )
            .expect("valid release-claim request"),
        )
        .expect("release");

    let review_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                reviewer.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim review")
        .expect("review claim");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                review_claim.claim_id.clone(),
                review_claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start review");
    covey
        .decide_review(
            DecideReviewReq::try_from_raw_parts(
                reviewer.clone(),
                review_id.clone(),
                review_claim.claim_id,
                review_claim.fence_seq,
                covey::ReviewVerdict::Approve,
                "blake3:findings_queue_drift".into(),
                id_key("decide-review"),
            )
            .expect("valid review decision request"),
        )
        .expect("approve");

    let queue_id = covey
        .enqueue_for_apply(
            EnqueueForApplyReq::try_from_raw_parts(
                orch.clone(),
                "blake3:queue_drift".into(),
                subtask_id.clone(),
                SettlementTarget::Canonical,
                id_key("enqueue-for-apply"),
            )
            .expect("valid enqueue-for-apply request"),
        )
        .expect("enqueue");
    let queue_claim = covey
        .mark_in_flight(mark_in_flight_req(
            gate.clone(),
            queue_id.clone(),
            30_000,
            id_key("mark-in-flight"),
        ))
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
        covey.mark_applied(mark_applied_req(
            gate.clone(),
            queue_id,
            queue_claim.claim_fence_seq,
            id_key("mark-applied"),
        )),
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
        covey.submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(worker.clone(), "nope", id_key("submit-meta-task"))
                .expect("valid submit-meta-task request"),
        ),
        Err(CoveyError::WrongRole { actual, .. }) if actual == SessionRole::Executor
    ));

    let claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(5).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim")
        .expect("claim result");

    let second_meta = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "second",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("meta");
    covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(second_meta).expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("work_2")),
            title: SubtaskTitle::parse("second work").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(2).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("second subtask");

    assert!(matches!(
        covey.claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(worker.clone(), 30_000, id_key("claim-next"))
                .expect("valid claim-next request"),
        ),
        Err(CoveyError::SessionAlreadyHasActiveSubtask { session_token, active_subtask_id })
            if session_token == worker && active_subtask_id == subtask_id
    ));

    assert!(matches!(
        covey.release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                other.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                id_key("release-claim"),
            )
            .expect("valid release-claim request"),
        ),
        Err(CoveyError::NotClaimOwner { session_token, claim_owner })
            if session_token == other && claim_owner == worker
    ));

    rig.tick(10);
    assert!(matches!(
        covey.start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        ),
        Err(CoveyError::LeaseExpired { object_id }) if object_id == claim.claim_id
    ));

    assert!(matches!(
        covey.release_reservation(
            ReleaseReservationReq::try_from_raw_parts(
                orch.clone(),
                "missing",
                id_key("release-reservation"),
            )
            .expect("valid release reservation request"),
        ),
        Err(CoveyError::ReservationNotFound)
    ));
    assert!(matches!(
        covey.resolve_conflict(
            ResolveConflictReq::try_from_raw_parts(
                orch.clone(),
                "missing",
                covey::ConflictResolutionState::Resolved,
                id_key("resolve-conflict"),
            )
            .expect("valid conflict resolution request"),
        ),
        Err(CoveyError::ConflictNotFound)
    ));
    assert!(matches!(
        covey.create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse("meta_does_not_exist")
                .expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("review_bad")),
            title: SubtaskTitle::parse("bad review").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
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
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "errors",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("meta");
    covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id.clone())
                .expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("dup")),
            title: SubtaskTitle::parse("dup").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("first subtask");

    assert!(matches!(
        covey.create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id.clone()).expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("dup")),
            title: SubtaskTitle::parse("dup").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
        idempotency_key: id_key("create-subtask"),
        }),
        Err(CoveyError::DuplicateSubtaskId { subtask_id }) if subtask_id == "dup"
    ));

    assert!(matches!(
        covey.subtask_status("missing_subtask"),
        Err(CoveyError::SubtaskNotFound)
    ));

    let work_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                "blake3:dup".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "dup.json".into(),
                "blake3:paths_dup".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish");
    covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id.clone())
                .expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("other_work")),
            title: SubtaskTitle::parse("other work").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(3).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("other work subtask");
    assert!(matches!(
        covey.request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                "dup",
                "blake3:missing",
                Some("review_missing".into()),
                1,
                id_key("request-review"),
            )
            .expect("valid review request"),
        ),
        Err(CoveyError::UnknownArtifactDigest { digest }) if digest == "blake3:missing"
    ));

    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                "dup",
                "blake3:dup",
                Some("review_dup".into()),
                3,
                id_key("request-review"),
            )
            .expect("valid review request"),
        )
        .expect("request review");
    covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id).expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("wrong_claim_subtask")),
            title: SubtaskTitle::parse("wrong claim subtask").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(0).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("wrong claim subtask");
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                worker.clone(),
                work_claim.claim_id,
                work_claim.fence_seq,
                id_key("release-claim"),
            )
            .expect("valid release-claim request"),
        )
        .expect("release");

    let review_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                reviewer.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim wrong subtask")
        .expect("claim result");
    assert_eq!(review_claim.subtask_id, "review_dup");

    assert!(matches!(
        covey.decide_review(
            DecideReviewReq::try_from_raw_parts(
                reviewer.clone(),
                "missing_review".into(),
                review_claim.claim_id.clone(),
                review_claim.fence_seq,
                covey::ReviewVerdict::Approve,
                "blake3:findings".into(),
                id_key("decide-review")
            )
            .expect("valid review decision request")
        ),
        Err(CoveyError::ReviewNotFound)
    ));

    let worker_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim work subtask")
        .expect("claim result");
    assert_eq!(worker_claim.subtask_id, "wrong_claim_subtask");
    assert!(matches!(
        covey.decide_review(DecideReviewReq::try_from_raw_parts(worker.clone(), review_id.clone(), worker_claim.claim_id, worker_claim.fence_seq, covey::ReviewVerdict::Approve, "blake3:findings".into(), id_key("decide-review")).expect("valid review decision request")),
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
        assert_eq!(status.session().state(), SessionState::Active);
        assert!(status.session().last_heartbeat_at() >= status.session().created_at());
    }
}

#[test]
fn list_conflicts_is_bounded() {
    let rig = Rig::new();
    let covey = rig.covey();

    let conn = Connection::open(&rig.db_path).expect("open db");
    for idx in 0..1_005 {
        let payload = format!(
            r#"{{"reservation_id":"reservation_{idx}","overlapping_reservation_id":"reservation_other_{idx}","owner_subtask_id":"subtask_{idx}","overlapping_owner_subtask_id":"subtask_other_{idx}","scope_class":"exact_path","scope_key":"src/{idx}.rs","overlapping_scope_class":"exact_path","overlapping_scope_key":"src/{idx}.rs"}}"#
        );
        conn.execute(
            "INSERT INTO conflicts (conflict_id, object_type, object_id, conflict_kind, payload_json, detected_at, resolution_state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                format!("conflict_{idx}"),
                ObjectType::Reservation.to_string(),
                format!("reservation_{idx}"),
                "reservation_overlap",
                payload,
                idx as i64,
                "open"
            ],
        )
        .expect("insert conflict");
    }

    let conflicts = covey.list_conflicts().expect("list conflicts");
    assert_eq!(conflicts.len(), 1_000);
    assert_eq!(
        conflicts.first().expect("first").conflict_id(),
        "conflict_1004"
    );
    assert_eq!(conflicts.last().expect("last").conflict_id(), "conflict_5");
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
            .register_session(
                RegisterSessionReq::try_from_raw_parts(
                    "retry_principal",
                    "retry_instance",
                    SessionRole::Executor,
                    id_key("register-session"),
                )
                .expect("valid session registration request"),
            )
            .expect("register after retry")
            .session_token
    });

    thread::sleep(std::time::Duration::from_millis(150));
    tx.commit().expect("release writer lock");
    let session_token = handle.join().expect("join");

    let session = rig.covey().session_status(&session_token).expect("session");
    assert_eq!(session.session().agent_principal_id, "retry_principal");
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
    let result = covey.register_session(
        RegisterSessionReq::try_from_raw_parts(
            "faulty_principal",
            "faulty_instance",
            SessionRole::Executor,
            id_key("register-session"),
        )
        .expect("valid session registration request"),
    );
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
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "full flow",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit");
    let apply_subtask = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id.clone())
                .expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("apply_me")),
            title: SubtaskTitle::parse("apply me").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask a");
    let abandon_subtask = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id).expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("abandon_me")),
            title: SubtaskTitle::parse("abandon me").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(2).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("subtask b");

    let claim_a = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker_a.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim a")
        .expect("claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker_a.clone(),
                claim_a.claim_id.clone(),
                claim_a.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start a");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker_a.clone(),
                claim_a.claim_id.clone(),
                claim_a.fence_seq,
                "blake3:apply".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "apply.json".into(),
                "blake3:apply_paths".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish");
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker_a.clone(),
                apply_subtask.clone(),
                "blake3:apply",
                Some("review_apply".into()),
                1,
                id_key("request-review"),
            )
            .expect("valid review request"),
        )
        .expect("request review");
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                worker_a.clone(),
                claim_a.claim_id,
                claim_a.fence_seq,
                id_key("release-claim"),
            )
            .expect("valid release-claim request"),
        )
        .expect("release a");

    let review_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                reviewer.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim review")
        .expect("review claim");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                review_claim.claim_id.clone(),
                review_claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start review");
    covey
        .decide_review(
            DecideReviewReq::try_from_raw_parts(
                reviewer.clone(),
                review_id.clone(),
                review_claim.claim_id,
                review_claim.fence_seq,
                covey::ReviewVerdict::Approve,
                "blake3:findings".into(),
                id_key("decide-review"),
            )
            .expect("valid review decision request"),
        )
        .expect("decide");
    let queue_id = covey
        .enqueue_for_apply(
            EnqueueForApplyReq::try_from_raw_parts(
                orch.clone(),
                "blake3:apply".into(),
                apply_subtask.clone(),
                SettlementTarget::Canonical,
                id_key("enqueue-for-apply"),
            )
            .expect("valid enqueue-for-apply request"),
        )
        .expect("enqueue");
    let queue_claim = covey
        .mark_in_flight(mark_in_flight_req(
            gate.clone(),
            queue_id.clone(),
            30_000,
            id_key("mark-in-flight"),
        ))
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
        .mark_applied(mark_applied_req(
            gate.clone(),
            queue_id,
            queue_claim.claim_fence_seq,
            id_key("mark-applied"),
        ))
        .expect("applied");

    let claim_b = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker_b.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim b")
        .expect("claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker_b.clone(),
                claim_b.claim_id.clone(),
                claim_b.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start b");
    covey
        .abandon_subtask(
            AbandonSubtaskReq::try_from_raw_parts(
                worker_b.clone(),
                claim_b.claim_id,
                claim_b.fence_seq,
                id_key("abandon-subtask"),
            )
            .expect("valid abandon-subtask request"),
        )
        .expect("abandon");

    assert_eq!(
        covey
            .subtask_status(&apply_subtask)
            .expect("apply status")
            .subtask()
            .state(),
        SubtaskState::Applied
    );
    assert_eq!(
        covey
            .subtask_status(&abandon_subtask)
            .expect("abandon status")
            .subtask()
            .state(),
        SubtaskState::Abandoned
    );
    assert!(matches!(
        covey.meta_task_status("meta_does_not_exist"),
        Err(CoveyError::MetaTaskNotFound)
    ));
    let events = covey.fetch_events(0, 1_000).expect("events");
    assert!(!events.is_empty());
    for pair in events.windows(2) {
        assert!(pair[0].seq() < pair[1].seq());
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
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "meta lifecycle",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta");
    assert_eq!(
        covey
            .meta_task_status(&meta_task_id)
            .expect("planning status")
            .meta_task()
            .state(),
        MetaTaskState::Planning
    );

    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id.clone())
                .expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("meta_flow_work")),
            title: SubtaskTitle::parse("meta flow work").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create subtask");
    assert_eq!(
        covey
            .meta_task_status(&meta_task_id)
            .expect("active status")
            .meta_task()
            .state(),
        MetaTaskState::Active
    );

    let work_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start");
    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                work_claim.claim_id.clone(),
                work_claim.fence_seq,
                "blake3:meta_flow".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "meta_flow.json".into(),
                "blake3:meta_flow_paths".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish");
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                "blake3:meta_flow",
                Some("meta_flow_review".into()),
                1,
                id_key("request-review"),
            )
            .expect("valid review request"),
        )
        .expect("request review");
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                worker,
                work_claim.claim_id,
                work_claim.fence_seq,
                id_key("release-claim"),
            )
            .expect("valid release-claim request"),
        )
        .expect("release work claim");

    let review_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                reviewer.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("review claim")
        .expect("review claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                review_claim.claim_id.clone(),
                review_claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start review");
    covey
        .decide_review(
            DecideReviewReq::try_from_raw_parts(
                reviewer,
                review_id.clone(),
                review_claim.claim_id,
                review_claim.fence_seq,
                covey::ReviewVerdict::Approve,
                "blake3:meta_flow_findings".into(),
                id_key("decide-review"),
            )
            .expect("valid review decision request"),
        )
        .expect("decide review");

    let queue_id = covey
        .enqueue_for_apply(
            EnqueueForApplyReq::try_from_raw_parts(
                orch,
                "blake3:meta_flow".into(),
                subtask_id,
                SettlementTarget::Canonical,
                id_key("enqueue-for-apply"),
            )
            .expect("valid enqueue-for-apply request"),
        )
        .expect("enqueue");
    let queue_claim = covey
        .mark_in_flight(mark_in_flight_req(
            gate.clone(),
            queue_id.clone(),
            30_000,
            id_key("mark-in-flight"),
        ))
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
        .mark_applied(mark_applied_req(
            gate,
            queue_id,
            queue_claim.claim_fence_seq,
            id_key("mark-applied"),
        ))
        .expect("mark applied");

    assert_eq!(
        covey
            .meta_task_status(&meta_task_id)
            .expect("completed status")
            .meta_task()
            .state(),
        MetaTaskState::Completed
    );
}

#[test]
fn oversized_identity_and_artifact_fields_are_rejected() {
    let rig = Rig::new();
    let covey = rig.covey();
    let long = "x".repeat(10_000);

    let oversized_principal = RegisterSessionReq::try_from_raw_parts(
        long.clone(),
        "instance",
        SessionRole::Executor,
        id_key("register-session"),
    )
    .expect_err("oversized agent principal id should be rejected before operation execution");
    assert!(
        oversized_principal
            .to_string()
            .contains("invalid agent_principal_id"),
        "unexpected error: {oversized_principal}"
    );
    let oversized_instance = RegisterSessionReq::try_from_raw_parts(
        "principal",
        long.clone(),
        SessionRole::Executor,
        id_key("register-session"),
    )
    .expect_err("oversized agent instance id should be rejected before operation execution");
    assert!(
        oversized_instance
            .to_string()
            .contains("invalid agent_instance_id"),
        "unexpected error: {oversized_instance}"
    );

    let orch = register(&covey, "orch", "orch", SessionRole::Orchestrator);
    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orch.clone(),
                "bounds",
                id_key("submit-meta-task"),
            )
            .expect("valid submit-meta-task request"),
        )
        .expect("submit meta");
    let oversized_subtask_id = CreateSubtaskRequest::try_from_raw_parts(
        orch.clone(),
        meta_task_id.clone(),
        Some(long.clone()),
        "too long",
        1,
        id_key("create-subtask"),
    )
    .expect_err("oversized subtask id should be rejected before operation execution");
    assert!(
        oversized_subtask_id.to_string().contains("subtask_id"),
        "unexpected error: {oversized_subtask_id}"
    );

    let subtask_id = covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id).expect("valid meta-task id"),
            subtask_id: Some(parse_subtask_id("bounds_work")),
            title: SubtaskTitle::parse("bounds work").expect("valid subtask title"),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create subtask");
    let worker = register(&covey, "worker", "worker", SessionRole::Executor);
    let reviewer = register(&covey, "reviewer", "reviewer", SessionRole::Reviewer);

    let claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                worker.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("claim")
        .expect("claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start");

    let base_rev_err = PublishArtifactReq::try_from_raw_parts(
        worker.clone(),
        claim.claim_id.clone(),
        claim.fence_seq,
        "blake3:bounds_a".into(),
        ArtifactKind::PatchBundle,
        long.clone(),
        "bounds.json".into(),
        "blake3:bounds_paths".into(),
        id_key("publish-artifact"),
    )
    .expect_err("base revision bounds should be enforced by the request type");
    assert!(
        base_rev_err.to_string().contains("base_rev"),
        "unexpected error: {base_rev_err}"
    );
    let manifest_path_err = PublishArtifactReq::try_from_raw_parts(
        worker.clone(),
        claim.claim_id.clone(),
        claim.fence_seq,
        "blake3:bounds_b".into(),
        ArtifactKind::PatchBundle,
        "base".into(),
        long.clone(),
        "blake3:bounds_paths".into(),
        id_key("publish-artifact"),
    )
    .expect_err("manifest path bounds should be enforced by the request type");
    assert!(
        manifest_path_err.to_string().contains("manifest_path"),
        "unexpected error: {manifest_path_err}"
    );
    let changed_paths_err = PublishArtifactReq::try_from_raw_parts(
        worker.clone(),
        claim.claim_id.clone(),
        claim.fence_seq,
        "blake3:bounds_c".into(),
        ArtifactKind::PatchBundle,
        "base".into(),
        "bounds.json".into(),
        long.clone(),
        id_key("publish-artifact"),
    )
    .expect_err("changed paths digest bounds should be enforced by the request type");
    assert!(
        changed_paths_err
            .to_string()
            .contains("changed_paths_digest"),
        "unexpected error: {changed_paths_err}"
    );

    covey
        .publish_artifact(
            PublishArtifactReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "blake3:bounds_valid".into(),
                ArtifactKind::PatchBundle,
                "base".into(),
                "bounds.json".into(),
                "blake3:bounds_paths".into(),
                id_key("publish-artifact"),
            )
            .expect("valid artifact publication request"),
        )
        .expect("publish valid artifact");
    let review_id = covey
        .request_review(
            RequestReviewReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                "blake3:bounds_valid",
                Some("bounds_review".into()),
                1,
                id_key("request-review"),
            )
            .expect("valid review request"),
        )
        .expect("request review");
    covey
        .release_claim(
            ReleaseClaimReq::try_from_raw_parts(
                worker,
                claim.claim_id,
                claim.fence_seq,
                id_key("release-claim"),
            )
            .expect("valid release-claim request"),
        )
        .expect("release work claim");

    let review_claim = covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(
                reviewer.clone(),
                covey::LeaseDurationMs::parse(30_000).expect("valid lease duration"),
                id_key("claim-next"),
            )
            .expect("valid claim-next request"),
        )
        .expect("review claim")
        .expect("review claim result");
    covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                reviewer.clone(),
                review_claim.claim_id.clone(),
                review_claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start-subtask request"),
        )
        .expect("start review");

    let findings_err = DecideReviewReq::try_from_raw_parts(
        reviewer,
        review_id,
        review_claim.claim_id,
        review_claim.fence_seq,
        covey::ReviewVerdict::Approve,
        long,
        id_key("decide-review"),
    )
    .expect_err("findings digest bounds should be enforced by the request type");
    assert!(
        findings_err.to_string().contains("findings_digest"),
        "unexpected error: {findings_err}"
    );
}

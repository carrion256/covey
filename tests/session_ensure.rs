//! Session ensure: one active session per principal and role.
//!
//! `ensure_session` answers one question inside one transaction: does this
//! principal already hold an active session in the role the caller needs? These
//! cases pin the write-free path, the single write path, a principal holding
//! several roles at once, and the invariant that no sequence of ensure and exit
//! leaves a principal with two active sessions in one role.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use covey::{
    Covey, CoveyError, EventType, ExitSessionReq, HeartbeatReq, ManualClock, RegisterSessionReq,
    SessionEnsured, SessionRole,
};
use proptest::prelude::*;
use rstest::rstest;
use rusqlite::Connection;
use tempfile::TempDir;

const NOW_MS: i64 = 1_700_000_000_000;
const PRINCIPALS: [&str; 3] = ["principal-alpha", "principal-beta", "principal-gamma"];
const ROLES: [SessionRole; 4] = [
    SessionRole::Executor,
    SessionRole::Orchestrator,
    SessionRole::ApplyGate,
    SessionRole::Reviewer,
];

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

fn id_key(label: &str) -> String {
    format!("{label}-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

struct Rig {
    _dir: TempDir,
    db_path: PathBuf,
    covey: Covey,
}

impl Rig {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("covey.db");
        let covey = Covey::open_with_clock(&db_path, Arc::new(ManualClock::new(NOW_MS)))
            .expect("create store");
        Self {
            _dir: dir,
            db_path,
            covey,
        }
    }

    fn request(&self, principal: &str, instance: &str, role: SessionRole) -> RegisterSessionReq {
        RegisterSessionReq::try_from_raw_parts(principal, instance, role, id_key("ensure-session"))
            .expect("valid session registration request")
    }

    fn ensure(
        &self,
        principal: &str,
        instance: &str,
        role: SessionRole,
    ) -> covey::Result<SessionEnsured> {
        self.covey
            .ensure_session(self.request(principal, instance, role))
    }

    fn exit(&self, session_token: &str) -> covey::Result<()> {
        self.covey.exit_session(
            ExitSessionReq::try_from_raw_parts(session_token, id_key("exit-session"))
                .expect("valid session exit request"),
        )
    }

    fn heartbeat(&self, session_token: &str) -> covey::Result<()> {
        self.covey.heartbeat(
            HeartbeatReq::try_from_raw_parts(session_token, id_key("heartbeat"))
                .expect("valid heartbeat request"),
        )
    }

    fn registered_event_count(&self) -> usize {
        self.covey
            .fetch_events(0, 10_000)
            .expect("fetch events")
            .iter()
            .filter(|event| event.event_type() == EventType::SessionRegistered)
            .count()
    }

    fn database(&self) -> Connection {
        Connection::open(&self.db_path).expect("open database")
    }

    fn mutation_idempotency_count(&self) -> i64 {
        self.database()
            .query_row("SELECT COUNT(*) FROM mutation_idempotency", [], |row| {
                row.get(0)
            })
            .expect("count idempotency rows")
    }

    /// The `(principal, role)` of every active session, sorted.
    fn active_sessions(&self) -> Vec<(String, String)> {
        self.database()
            .prepare(
                "SELECT agent_principal_id, role FROM sessions WHERE state = 'active'
                 ORDER BY agent_principal_id, role",
            )
            .expect("prepare active session query")
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query active sessions")
            .collect::<Result<Vec<_>, _>>()
            .expect("read active sessions")
    }

    fn active_session_count(&self, principal: &str, role: SessionRole) -> usize {
        let role = role.to_string();
        self.active_sessions()
            .iter()
            .filter(|(active_principal, active_role)| {
                active_principal == principal && *active_role == role
            })
            .count()
    }
}

#[test]
fn ensure_registers_when_the_principal_has_no_active_session() {
    let rig = Rig::new();
    let before = rig.registered_event_count();
    assert_eq!(
        rig.active_session_count(PRINCIPALS[0], SessionRole::Orchestrator),
        0
    );

    let ensured = rig
        .ensure(PRINCIPALS[0], "alpha-instance-1", SessionRole::Orchestrator)
        .expect("ensure registers a session");

    assert!(ensured.registered);
    assert_eq!(ensured.session.role, SessionRole::Orchestrator);
    assert_eq!(ensured.session.agent_principal_id, PRINCIPALS[0]);
    assert_eq!(ensured.session.agent_instance_id, "alpha-instance-1");
    assert_eq!(
        rig.registered_event_count(),
        before + 1,
        "the registration appends exactly one session_registered event"
    );
    rig.heartbeat(ensured.session.session_token.as_str())
        .expect("the ensured token heartbeats");
}

#[test]
fn ensure_returns_the_active_session_for_the_same_principal_and_role() {
    let rig = Rig::new();
    let first = rig
        .ensure(PRINCIPALS[0], "alpha-instance-1", SessionRole::Executor)
        .expect("first ensure registers");
    let events = rig.registered_event_count();
    let idempotency_rows = rig.mutation_idempotency_count();

    let second = rig
        .ensure(PRINCIPALS[0], "alpha-instance-2", SessionRole::Executor)
        .expect("second ensure reuses the active session");

    assert!(!second.registered);
    assert_eq!(second.session.session_token, first.session.session_token);
    assert_eq!(
        rig.registered_event_count(),
        events,
        "reusing an active session appends no event"
    );
    assert_eq!(
        rig.mutation_idempotency_count(),
        idempotency_rows,
        "reusing an active session writes no idempotency row"
    );
}

#[test]
fn ensure_returns_an_active_session_from_another_instance_as_it_is() {
    let rig = Rig::new();
    let first = rig
        .ensure(PRINCIPALS[0], "alpha-instance-1", SessionRole::Reviewer)
        .expect("first ensure registers");
    let second = rig
        .ensure(PRINCIPALS[0], "alpha-instance-2", SessionRole::Reviewer)
        .expect("second ensure reuses");

    assert!(!second.registered);
    assert_eq!(
        second.session, first.session,
        "the active session is returned as it is"
    );
    assert_eq!(second.session.agent_instance_id, "alpha-instance-1");
}

#[rstest]
#[case::reviewer_beside_orchestrator(SessionRole::Orchestrator, SessionRole::Reviewer)]
#[case::executor_beside_apply_gate(SessionRole::ApplyGate, SessionRole::Executor)]
#[case::executor_beside_orchestrator(SessionRole::Orchestrator, SessionRole::Executor)]
fn ensure_registers_a_second_session_for_another_role(
    #[case] held_role: SessionRole,
    #[case] requested_role: SessionRole,
) {
    let rig = Rig::new();
    let held = rig
        .ensure(PRINCIPALS[0], "alpha-instance-1", held_role)
        .expect("register the first session");
    let events = rig.registered_event_count();

    let second = rig
        .ensure(PRINCIPALS[0], "alpha-instance-2", requested_role)
        .expect("a session in another role does not block the ensure");

    assert!(second.registered);
    assert_eq!(second.session.role, requested_role);
    assert_ne!(second.session.session_token, held.session.session_token);
    assert_eq!(
        rig.registered_event_count(),
        events + 1,
        "the second role appends its own session_registered event"
    );
    for (role, session) in [
        (held_role, &held.session),
        (requested_role, &second.session),
    ] {
        assert_eq!(
            rig.covey
                .active_session_for_principal(PRINCIPALS[0], role)
                .expect("look up the active session")
                .as_ref(),
            Some(session),
            "each role keeps its own active session"
        );
    }
    rig.heartbeat(held.session.session_token.as_str())
        .expect("the first session stays usable");
}

#[test]
fn ensure_registers_a_new_session_after_the_active_one_exits() {
    let rig = Rig::new();
    let first = rig
        .ensure(PRINCIPALS[0], "alpha-instance-1", SessionRole::Executor)
        .expect("first ensure registers");
    rig.exit(first.session.session_token.as_str())
        .expect("exit the active session");
    assert_eq!(
        rig.active_session_count(PRINCIPALS[0], SessionRole::Executor),
        0
    );

    let events = rig.registered_event_count();
    let second = rig
        .ensure(PRINCIPALS[0], "alpha-instance-1", SessionRole::Executor)
        .expect("ensure registers again");

    assert!(second.registered);
    assert_ne!(
        second.session.session_token, first.session.session_token,
        "the exited session is not revived"
    );
    assert_eq!(rig.registered_event_count(), events + 1);
    assert_eq!(
        rig.active_session_count(PRINCIPALS[0], SessionRole::Executor),
        1
    );
}

#[test]
fn ensure_needs_no_session_token_in_the_request() {
    let rig = Rig::new();
    let request = rig.request(PRINCIPALS[0], "alpha-instance-1", SessionRole::Orchestrator);
    let payload = serde_json::to_value(&request).expect("request serializes");
    assert!(
        payload.get("session_token").is_none(),
        "the ensure request names no session token"
    );

    let ensured = rig
        .covey
        .ensure_session(request)
        .expect("ensure works without a session token");
    assert!(ensured.registered);
}

#[test]
fn ensure_returns_a_session_registered_directly() {
    let rig = Rig::new();
    let registered = rig
        .covey
        .register_session(rig.request(PRINCIPALS[1], "beta-instance-1", SessionRole::ApplyGate))
        .expect("register a session");
    let ensured = rig
        .ensure(PRINCIPALS[1], "beta-instance-2", SessionRole::ApplyGate)
        .expect("ensure finds the registered session");

    assert!(!ensured.registered);
    assert_eq!(ensured.session, registered);
}

#[test]
fn register_refuses_a_second_session_only_in_the_same_role() {
    let rig = Rig::new();
    let executor = rig
        .covey
        .register_session(rig.request(PRINCIPALS[2], "gamma-instance-1", SessionRole::Executor))
        .expect("register the executor session");

    let orchestrator = rig
        .covey
        .register_session(rig.request(PRINCIPALS[2], "gamma-instance-1", SessionRole::Orchestrator))
        .expect("the same principal registers in another role");
    assert_ne!(orchestrator.session_token, executor.session_token);

    let error = rig
        .covey
        .register_session(rig.request(PRINCIPALS[2], "gamma-instance-2", SessionRole::Executor))
        .expect_err("a second executor session for the principal is refused");
    assert!(
        matches!(
            &error,
            CoveyError::SessionAlreadyActive { agent_principal_id, role }
                if agent_principal_id == PRINCIPALS[2] && *role == SessionRole::Executor
        ),
        "the refusal names the principal and the role, got {error:?}"
    );
    assert_eq!(
        rig.active_session_count(PRINCIPALS[2], SessionRole::Executor),
        1
    );
}

#[derive(Debug, Clone, Copy)]
enum Step {
    Ensure { principal: usize, role: usize },
    Exit { principal: usize, role: usize },
}

fn step_from_bytes(principal_byte: u8, choice_byte: u8) -> Step {
    let principal = usize::from(principal_byte) % PRINCIPALS.len();
    let choice = usize::from(choice_byte);
    let role = (choice / 5) % ROLES.len();
    if choice % 5 == 0 {
        Step::Exit { principal, role }
    } else {
        Step::Ensure { principal, role }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// Any run of up to twenty ensure and exit steps over three principals and
    /// four roles leaves each principal with at most one active session per
    /// role: every ensure succeeds in the role it was asked for, registers only
    /// when that role had no active session, and leaves the other roles alone.
    #[test]
    fn at_most_one_active_session_per_role_survives_any_ensure_and_exit_sequence(
        steps in proptest::collection::vec((any::<u8>(), any::<u8>()), 0..=20),
    ) {
        let rig = Rig::new();
        let mut active: [[Option<String>; ROLES.len()]; PRINCIPALS.len()] = Default::default();

        for (index, (principal_byte, choice_byte)) in steps.iter().enumerate() {
            match step_from_bytes(*principal_byte, *choice_byte) {
                Step::Ensure { principal, role } => {
                    let instance = format!("{}-instance-{index}", PRINCIPALS[principal]);
                    let ensured = rig
                        .ensure(PRINCIPALS[principal], &instance, ROLES[role])
                        .expect("ensure succeeds whatever other roles are active");
                    prop_assert_eq!(
                        ensured.session.role,
                        ROLES[role],
                        "an ensured session carries the requested role"
                    );
                    let session_token = ensured.session.session_token.to_string();
                    match &active[principal][role] {
                        Some(tracked) => {
                            prop_assert!(!ensured.registered, "an active session is reused");
                            prop_assert_eq!(&session_token, tracked);
                        }
                        None => prop_assert!(ensured.registered, "an absent session is registered"),
                    }
                    active[principal][role] = Some(session_token);
                }
                Step::Exit { principal, role } => {
                    if let Some(session_token) = active[principal][role].take() {
                        rig.exit(&session_token)
                            .expect("the tracked active session exits");
                    }
                }
            }

            let mut expected = Vec::new();
            for (principal, roles) in PRINCIPALS.iter().zip(&active) {
                for (role, tracked) in ROLES.iter().zip(roles) {
                    if tracked.is_some() {
                        expected.push(((*principal).to_owned(), role.to_string()));
                    }
                }
            }
            expected.sort();
            prop_assert_eq!(
                rig.active_sessions(),
                expected,
                "each principal holds exactly its tracked sessions, one per role"
            );
        }
    }
}

//! Session ensure: one active session per principal, carrying the requested role.
//!
//! `ensure_session` answers one question inside one transaction: does this
//! principal already hold an active session, and is that session in the role the
//! caller needs? These cases pin the write-free paths, the single write path, the
//! refusal, and the invariant that no sequence of ensure and exit leaves a
//! principal with two active sessions.

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
use rusqlite::{Connection, OptionalExtension};
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

    fn active_session_count(&self, principal: &str) -> i64 {
        self.database()
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE agent_principal_id = ?1 AND state = 'active'",
                [principal],
                |row| row.get(0),
            )
            .expect("count active sessions")
    }

    fn active_session_token(&self, principal: &str) -> Option<String> {
        self.database()
            .query_row(
                "SELECT session_token FROM sessions WHERE agent_principal_id = ?1 AND state = 'active'",
                [principal],
                |row| row.get(0),
            )
            .optional()
            .expect("read active session token")
    }
}

#[test]
fn ensure_registers_when_the_principal_has_no_active_session() {
    let rig = Rig::new();
    let before = rig.registered_event_count();
    assert_eq!(rig.active_session_count(PRINCIPALS[0]), 0);

    let ensured = rig
        .ensure(PRINCIPALS[0], "alpha-instance-1", SessionRole::Orchestrator)
        .expect("ensure registers a session");

    assert!(ensured.registered);
    assert_eq!(ensured.session.role, SessionRole::Orchestrator);
    assert_eq!(ensured.session.agent_principal_id(), PRINCIPALS[0]);
    assert_eq!(ensured.session.agent_instance_id(), "alpha-instance-1");
    assert_eq!(
        rig.registered_event_count(),
        before + 1,
        "the registration appends exactly one session_registered event"
    );
    rig.heartbeat(ensured.session.session_token())
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
    assert_eq!(second.session.agent_instance_id(), "alpha-instance-1");
}

#[rstest]
#[case::reviewer_against_orchestrator(SessionRole::Orchestrator, SessionRole::Reviewer)]
#[case::executor_against_apply_gate(SessionRole::ApplyGate, SessionRole::Executor)]
#[case::apply_gate_against_executor(SessionRole::Executor, SessionRole::ApplyGate)]
fn ensure_refuses_another_role_for_the_same_principal(
    #[case] active_role: SessionRole,
    #[case] requested_role: SessionRole,
) {
    let rig = Rig::new();
    let active = rig
        .ensure(PRINCIPALS[0], "alpha-instance-1", active_role)
        .expect("register the active session");
    let events = rig.registered_event_count();

    let error = rig
        .ensure(PRINCIPALS[0], "alpha-instance-2", requested_role)
        .expect_err("an active session in another role is refused");

    match error {
        CoveyError::WrongRole { expected, actual } => {
            assert_eq!(
                expected,
                vec![requested_role],
                "only the requested role is expected"
            );
            assert_eq!(actual, active_role, "the refusal names the active role");
        }
        other => panic!("expected a wrong-role refusal, got {other:?}"),
    }
    assert_eq!(
        rig.registered_event_count(),
        events,
        "a refused role appends no event"
    );
    assert_eq!(
        rig.active_session_token(PRINCIPALS[0]),
        Some(active.session.session_token.to_string())
    );
}

#[test]
fn ensure_registers_a_new_session_after_the_active_one_exits() {
    let rig = Rig::new();
    let first = rig
        .ensure(PRINCIPALS[0], "alpha-instance-1", SessionRole::Executor)
        .expect("first ensure registers");
    rig.exit(first.session.session_token())
        .expect("exit the active session");
    assert_eq!(rig.active_session_count(PRINCIPALS[0]), 0);

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
    assert_eq!(rig.active_session_count(PRINCIPALS[0]), 1);
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

#[derive(Debug, Clone, Copy)]
enum Step {
    Ensure { principal: usize, role: usize },
    Exit { principal: usize },
}

fn step_from_bytes(principal_byte: u8, choice_byte: u8) -> Step {
    let principal = usize::from(principal_byte) % PRINCIPALS.len();
    let choice = usize::from(choice_byte);
    if choice % 5 == 0 {
        Step::Exit { principal }
    } else {
        Step::Ensure {
            principal,
            role: (choice / 5) % ROLES.len(),
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// Any run of up to twenty ensure and exit steps over three principals and
    /// four roles leaves at most one active session per principal, and every
    /// successful ensure returns the role it was asked for.
    #[test]
    fn at_most_one_active_session_survives_any_ensure_and_exit_sequence(
        steps in proptest::collection::vec((any::<u8>(), any::<u8>()), 0..=20),
    ) {
        let rig = Rig::new();
        let mut active: [Option<String>; PRINCIPALS.len()] = [None, None, None];

        for (index, (principal_byte, choice_byte)) in steps.iter().enumerate() {
            match step_from_bytes(*principal_byte, *choice_byte) {
                Step::Ensure { principal, role } => {
                    let requested = ROLES[role];
                    let instance = format!("{}-instance-{index}", PRINCIPALS[principal]);
                    match rig.ensure(PRINCIPALS[principal], &instance, requested) {
                        Ok(ensured) => {
                            prop_assert_eq!(
                                ensured.session.role,
                                requested,
                                "an ensured session carries the requested role"
                            );
                            active[principal] = Some(ensured.session.session_token.to_string());
                        }
                        Err(CoveyError::WrongRole { expected, actual }) => {
                            prop_assert_eq!(expected, vec![requested]);
                            prop_assert_ne!(actual, requested);
                        }
                        Err(error) => prop_assert!(false, "unexpected ensure failure: {error}"),
                    }
                }
                Step::Exit { principal } => {
                    if let Some(session_token) = active[principal].take() {
                        rig.exit(&session_token)
                            .expect("the tracked active session exits");
                    }
                }
            }

            for principal in PRINCIPALS {
                prop_assert!(
                    rig.active_session_count(principal) <= 1,
                    "principal {principal} holds more than one active session"
                );
            }
        }
    }
}

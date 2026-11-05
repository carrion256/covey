//! `subtask create --depends-on` and the dependency view of `subtask status`.
//!
//! Dependencies gate CLAIMING only: they add no review, evidence, or other
//! governance requirement, so every case here asserts scheduler-visible
//! behaviour and persisted edges, nothing else.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

mod support;

use covey::{
    ClaimNextReq, Covey, CoveyError, CreateSubtaskRequest, EventType, FinishSubtaskReq,
    ManualClock, RegisterSessionReq, SessionRole, StartSubtaskReq, SubmitMetaTaskReq, SubtaskState,
};
use proptest::prelude::*;
use rstest::rstest;
use rusqlite::Connection;
use tempfile::TempDir;

const NOW_MS: i64 = 1_700_000_000_000;
const LEASE_MS: i64 = 30_000;

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

fn id_key(label: &str) -> String {
    format!("{label}-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

struct Harness {
    _dir: TempDir,
    db_path: PathBuf,
    covey: Covey,
    orchestrator: String,
    meta_task_id: String,
}

impl Harness {
    fn new() -> Self {
        support::enable_info_logging();
        let dir = TempDir::new().expect("tempdir");
        let db_path = dir.path().join("covey.db");
        let covey = open(&db_path);
        let orchestrator = register(&covey, "orchestrator", SessionRole::Orchestrator);
        let meta_task_id = covey
            .submit_meta_task(
                SubmitMetaTaskReq::try_from_raw_parts(
                    orchestrator.clone(),
                    "dependency scenarios",
                    id_key("submit-meta"),
                )
                .expect("valid submit-meta-task request"),
            )
            .expect("submit meta task");
        Self {
            _dir: dir,
            db_path,
            covey,
            orchestrator,
            meta_task_id,
        }
    }

    fn second_meta_task(&self) -> String {
        self.covey
            .submit_meta_task(
                SubmitMetaTaskReq::try_from_raw_parts(
                    self.orchestrator.clone(),
                    "second meta task",
                    id_key("submit-meta"),
                )
                .expect("valid submit-meta-task request"),
            )
            .expect("submit meta task")
    }

    /// Creates work through the same request path the CLI uses: the declared
    /// dependencies go through `try_with_dependencies`.
    fn create(
        &self,
        meta_task_id: &str,
        subtask_id: Option<&str>,
        title: &str,
        depends_on: &[&str],
    ) -> covey::Result<String> {
        let depends_on: Vec<String> = depends_on.iter().map(|id| (*id).to_owned()).collect();
        self.covey.create_subtask(
            CreateSubtaskRequest::try_from_raw_parts(
                self.orchestrator.clone(),
                meta_task_id.to_owned(),
                subtask_id.map(str::to_owned),
                title.to_owned(),
                100,
                id_key("create-subtask"),
            )
            .expect("valid create-subtask request")
            .try_with_dependencies(&depends_on)
            .expect("valid dependency declaration"),
        )
    }

    fn created(&self, subtask_id: &str, title: &str, depends_on: &[&str]) -> String {
        self.create(&self.meta_task_id, Some(subtask_id), title, depends_on)
            .expect("create subtask")
    }

    fn create_fails(&self, subtask_id: &str, title: &str, depends_on: &[&str]) -> CoveyError {
        self.create(&self.meta_task_id, Some(subtask_id), title, depends_on)
            .expect_err("create must fail")
    }

    /// Completes a subtask through the ordinary claim/start/finish path with a
    /// dedicated executor, so no session ever holds two active subtasks.
    fn complete(&self, subtask_id: &str) {
        let worker = register(
            &self.covey,
            &format!("worker-{subtask_id}"),
            SessionRole::Executor,
        );
        let claim = self
            .covey
            .claim_subtask(
                covey::ClaimSubtaskReq::try_from_raw_parts(
                    worker.clone(),
                    subtask_id.to_owned(),
                    LEASE_MS,
                    id_key("claim-subtask"),
                )
                .expect("valid claim request"),
            )
            .expect("subtask is claimable");
        self.covey
            .start_subtask(
                StartSubtaskReq::try_from_raw_parts(
                    worker.clone(),
                    claim.claim_id.clone(),
                    claim.fence_seq,
                    id_key("start-subtask"),
                )
                .expect("valid start request"),
            )
            .expect("start subtask");
        let _outcome = self
            .covey
            .finish_subtask(
                FinishSubtaskReq::try_from_raw_parts(
                    worker,
                    claim.claim_id,
                    claim.fence_seq,
                    None,
                    format!("finished {subtask_id}"),
                    id_key("finish-subtask"),
                )
                .expect("valid finish request"),
            )
            .expect("finish subtask");
    }

    fn claim_next(&self, principal: &str) -> Option<covey::ClaimResult> {
        let worker = register(&self.covey, principal, SessionRole::Executor);
        self.covey
            .claim_next_subtask(
                ClaimNextReq::try_from_raw_parts(worker, LEASE_MS, id_key("claim-next"))
                    .expect("valid claim-next request"),
            )
            .expect("claim next")
    }

    fn candidates(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .covey
            .subtask_candidates(SessionRole::Executor, 50, None)
            .expect("candidates")
            .into_iter()
            .map(|candidate| candidate.subtask_id.to_string())
            .collect();
        ids.sort();
        ids
    }

    fn connection(&self) -> Connection {
        Connection::open(&self.db_path).expect("open test db")
    }

    // A tail-expression iterator would borrow `rows` past its own drop, hence the binding.
    #[allow(clippy::let_and_return)]
    fn subtask_rows(&self) -> Vec<String> {
        let conn = self.connection();
        let mut rows = conn
            .prepare("SELECT subtask_id FROM subtasks ORDER BY subtask_id")
            .expect("prepare subtasks");
        let ids = rows
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query subtasks")
            .map(|row| row.expect("subtask row"))
            .collect();
        ids
    }

    #[allow(clippy::let_and_return)]
    fn dependency_rows(&self, subtask_id: &str) -> Vec<(String, String)> {
        let conn = self.connection();
        let mut rows = conn
            .prepare(
                "SELECT depends_on_subtask_id, source_ref FROM subtask_dependencies
                 WHERE subtask_id = ?1 ORDER BY depends_on_subtask_id",
            )
            .expect("prepare dependencies");
        let edges = rows
            .query_map([subtask_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .expect("query dependencies")
            .map(|row| row.expect("dependency row"))
            .collect();
        edges
    }

    fn dependency_row_count(&self) -> i64 {
        self.connection()
            .query_row("SELECT COUNT(*) FROM subtask_dependencies", [], |row| {
                row.get(0)
            })
            .expect("count dependencies")
    }
}

fn open(db_path: &Path) -> Covey {
    Covey::open_with_clock(db_path, Arc::new(ManualClock::new(NOW_MS))).expect("open covey")
}

fn register(covey: &Covey, principal: &str, role: SessionRole) -> String {
    let principal = format!("{principal}-{}", id_key("principal"));
    covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                principal.clone(),
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

#[rstest]
#[case::no_dependencies(&[] as &[&str], &[] as &[&str])]
#[case::one_dependency(&["dependency-a"], &["dependency-a"])]
#[case::three_dependencies(
    &["dependency-a", "dependency-b", "dependency-c"],
    &["dependency-a", "dependency-b", "dependency-c"]
)]
#[case::repeats_collapse(
    &["dependency-a", "dependency-a", "dependency-b", "dependency-a"],
    &["dependency-a", "dependency-b"]
)]
fn declared_dependencies_are_persisted_once_each(
    #[case] declared: &[&str],
    #[case] expected: &[&str],
) {
    let harness = Harness::new();
    for dependency in expected {
        harness.created(dependency, "prerequisite", &[]);
    }

    harness.created("dependent", "depends on the prerequisites", declared);

    assert_eq!(
        harness
            .dependency_rows("dependent")
            .into_iter()
            .map(|(dependency, _)| dependency)
            .collect::<Vec<String>>(),
        expected,
        "first-seen order must be preserved and repeats collapsed"
    );
    for (_, source_ref) in harness.dependency_rows("dependent") {
        assert_eq!(source_ref, "cli", "the create path records its provenance");
    }
    // Declaring dependencies is not a review or evidence requirement: the
    // dependent subtask itself is created in the ordinary available state.
    assert!(harness.subtask_rows().contains(&"dependent".to_owned()));
}

#[test]
fn self_dependency_is_rejected_naming_the_id() {
    let harness = Harness::new();

    let error = harness.create_fails("self-dependency", "self dependent", &["self-dependency"]);

    assert!(matches!(error, CoveyError::InvalidSubtaskDependency { .. }));
    assert!(
        error.to_string().contains("self-dependency"),
        "the message must name the id: {error}"
    );
    assert!(
        !harness
            .subtask_rows()
            .contains(&"self-dependency".to_owned()),
        "nothing may be created"
    );
    assert_eq!(harness.dependency_row_count(), 0);
}

#[test]
fn more_than_sixty_four_dependencies_are_rejected() {
    let harness = Harness::new();
    let prerequisites: Vec<String> = (0..64)
        .map(|index| format!("prerequisite-{index}"))
        .collect();
    for prerequisite in &prerequisites {
        harness.created(prerequisite, "prerequisite", &[]);
    }
    let references: Vec<&str> = prerequisites.iter().map(String::as_str).collect();

    // The 64-dependency boundary itself is legal.
    harness.created("at-the-limit", "exactly 64 dependencies", &references);
    assert_eq!(harness.dependency_rows("at-the-limit").len(), 64);

    let mut too_many = references.clone();
    too_many.push("prerequisite-64");
    let error = harness.create_fails("over-the-limit", "65 dependencies", &too_many);

    assert!(matches!(error, CoveyError::InvalidSubtaskDependency { .. }));
    assert!(
        error.to_string().contains("64"),
        "the message must state the limit: {error}"
    );
    assert!(
        !harness
            .subtask_rows()
            .contains(&"over-the-limit".to_owned()),
        "nothing may be created"
    );
    assert!(
        harness.dependency_rows("over-the-limit").is_empty(),
        "no partial edges may be written"
    );
}

#[test]
fn unknown_dependency_creates_nothing_at_all() {
    let harness = Harness::new();
    harness.created("known-dependency", "prerequisite", &[]);

    let error = harness.create_fails(
        "dependent",
        "depends on a missing subtask",
        &["known-dependency", "no-such-subtask"],
    );

    assert!(
        matches!(
            error,
            CoveyError::SubtaskDependencyNotFound {
                ref depends_on_subtask_id,
                ..
            } if depends_on_subtask_id.as_str() == "no-such-subtask"
        ),
        "the error must name the missing id: {error}"
    );
    assert_eq!(
        harness.subtask_rows(),
        vec![String::from("known-dependency")],
        "the subtask row and its earlier dependency rows must roll back"
    );
    assert_eq!(harness.dependency_row_count(), 0);
}

#[test]
fn dependencies_on_another_meta_task_are_allowed() {
    let harness = Harness::new();
    harness.created("other-lane", "prerequisite in the first meta task", &[]);
    let other_meta_task = harness.second_meta_task();

    harness
        .create(
            &other_meta_task,
            Some("cross-meta"),
            "cross lane",
            &["other-lane"],
        )
        .expect("cross-meta-task dependency");

    assert_eq!(
        harness
            .dependency_rows("cross-meta")
            .into_iter()
            .map(|(dependency, _)| dependency)
            .collect::<Vec<String>>(),
        vec![String::from("other-lane")]
    );
}

#[test]
fn already_finished_dependencies_are_accepted_at_create_time() {
    let harness = Harness::new();
    harness.created("finished-dependency", "already done", &[]);
    // Finishing a meta-task's last subtask completes the meta-task, and a completed
    // meta-task takes no new subtasks: keep one open so the create below is legal.
    harness.created("keeps-meta-task-open", "still pending", &[]);
    harness.complete("finished-dependency");

    harness
        .create(
            &harness.meta_task_id,
            Some("after-finish"),
            "depends on completed work",
            &["finished-dependency"],
        )
        .expect("a completed dependency is not a refusal");

    // The dependency is already satisfied, so the new subtask is claimable at once.
    let mut claimable = harness.candidates();
    claimable.sort();
    assert_eq!(claimable, vec!["after-finish", "keeps-meta-task-open"]);
}

#[test]
fn idempotent_replay_returns_the_original_and_changed_dependencies_conflict() {
    let harness = Harness::new();
    harness.created("dependency-a", "prerequisite a", &[]);
    harness.created("dependency-b", "prerequisite b", &[]);
    let idempotency_key = "fixed-create-key";
    let request = |depends_on: &[&str]| {
        let depends_on: Vec<String> = depends_on.iter().map(|id| (*id).to_owned()).collect();
        CreateSubtaskRequest::try_from_raw_parts(
            harness.orchestrator.clone(),
            harness.meta_task_id.clone(),
            Some("idempotent".to_owned()),
            "idempotent work",
            100,
            idempotency_key,
        )
        .expect("valid create-subtask request")
        .try_with_dependencies(&depends_on)
        .expect("valid dependency declaration")
    };

    let first = harness
        .covey
        .create_subtask(request(&["dependency-a"]))
        .expect("first create");
    let replay = harness
        .covey
        .create_subtask(request(&["dependency-a"]))
        .expect("replay");
    assert_eq!(first, replay, "the same key and deps replay");
    assert_eq!(harness.dependency_rows("idempotent").len(), 1);

    let conflict = harness.covey.create_subtask(request(&["dependency-b"]));
    assert!(
        matches!(conflict, Err(CoveyError::IdempotencyConflict { .. })),
        "the same key with different deps must conflict: {conflict:?}"
    );
    assert_eq!(
        harness.dependency_rows("idempotent").len(),
        1,
        "the conflicting call must write nothing"
    );
}

#[test]
fn creation_event_payload_carries_the_dependencies() {
    let harness = Harness::new();
    harness.created("dependency-a", "prerequisite a", &[]);
    harness.created("dependent", "depends on a", &["dependency-a"]);

    let events = harness.covey.fetch_events(0, 100).expect("event log");
    let event = events
        .iter()
        .find(|event| {
            event.event_type() == EventType::WorkSubtaskCreated && event.object_id() == "dependent"
        })
        .expect("creation event");

    let covey::EventPayload::WorkSubtaskCreated(payload) =
        event.typed().expect("typed creation payload").payload
    else {
        panic!("expected a work creation payload");
    };
    assert_eq!(
        payload
            .depends_on
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<String>>(),
        vec![String::from("dependency-a")]
    );
}

#[test]
fn dependencies_gate_claiming_until_the_prerequisite_completes() {
    let harness = Harness::new();
    harness.created("a", "first", &[]);
    harness.created("b", "blocked on a", &["a"]);

    assert_eq!(
        harness.candidates(),
        vec![String::from("a")],
        "the dependent subtask exists but is not offered"
    );
    assert!(harness.subtask_rows().contains(&"b".to_owned()));

    let worker = register(&harness.covey, "worker-a", SessionRole::Executor);
    let claim = harness
        .covey
        .claim_next_subtask(
            ClaimNextReq::try_from_raw_parts(worker.clone(), LEASE_MS, id_key("claim-next"))
                .expect("valid claim-next request"),
        )
        .expect("claim next")
        .expect("a is claimable");
    assert_eq!(claim.subtask_id.to_string(), "a");
    harness
        .covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                id_key("start-subtask"),
            )
            .expect("valid start request"),
        )
        .expect("start a");

    // Still blocked while `a` is only claimed/in-progress.
    assert_eq!(harness.candidates(), Vec::<String>::new());

    let _outcome = harness
        .covey
        .finish_subtask(
            FinishSubtaskReq::try_from_raw_parts(
                worker,
                claim.claim_id,
                claim.fence_seq,
                None,
                "a is done",
                id_key("finish-subtask"),
            )
            .expect("valid finish request"),
        )
        .expect("finish a");

    assert_eq!(harness.candidates(), vec![String::from("b")]);
    let claimed_b = harness
        .claim_next("worker-b")
        .expect("b is claimable once a is completed");
    assert_eq!(claimed_b.subtask_id.to_string(), "b");
}

#[test]
fn status_reports_dependencies_and_flips_satisfied() {
    let harness = Harness::new();
    harness.created("dependency-a", "prerequisite a", &[]);
    harness.created("dependency-b", "prerequisite b", &[]);
    harness.created(
        "dependent",
        "depends on a and b",
        &["dependency-a", "dependency-b"],
    );

    let before = harness.covey.subtask_status("dependent").expect("status");
    assert_eq!(
        before
            .dependencies()
            .iter()
            .map(|dependency| (dependency.subtask_id.to_string(), dependency.satisfied))
            .collect::<Vec<(String, bool)>>(),
        vec![
            (String::from("dependency-a"), false),
            (String::from("dependency-b"), false),
        ]
    );
    assert_eq!(before.dependencies()[0].state, SubtaskState::Available);

    harness.complete("dependency-a");

    let after = harness.covey.subtask_status("dependent").expect("status");
    assert_eq!(
        after
            .dependencies()
            .iter()
            .map(|dependency| (dependency.subtask_id.to_string(), dependency.satisfied))
            .collect::<Vec<(String, bool)>>(),
        vec![
            (String::from("dependency-a"), true),
            (String::from("dependency-b"), false),
        ],
        "satisfaction must follow the claim-gate predicate per dependency"
    );
    assert_eq!(after.dependencies()[0].state, SubtaskState::Completed);

    // The status view stays additive: the existing subtask view is intact.
    assert_eq!(after.subtask().subtask_id.to_string(), "dependent");

    harness.complete("dependency-b");
    let satisfied = harness.covey.subtask_status("dependent").expect("status");
    assert!(
        satisfied
            .dependencies()
            .iter()
            .all(|dependency| dependency.satisfied)
    );
    assert_eq!(harness.candidates(), vec![String::from("dependent")]);
}

#[test]
fn subtask_without_dependencies_is_unaffected() {
    let harness = Harness::new();
    harness.created("plain", "no dependencies", &[]);

    let status = harness.covey.subtask_status("plain").expect("status");
    assert!(status.dependencies().is_empty());
    assert_eq!(
        harness.dependency_rows("plain"),
        Vec::<(String, String)>::new()
    );
    assert_eq!(harness.candidates(), vec![String::from("plain")]);
    let claim = harness
        .claim_next("worker-plain")
        .expect("claimable immediately");
    assert_eq!(claim.subtask_id.to_string(), "plain");
}

/// Subtask budget in the generated shapes; keeps each case small enough that one
/// in-memory database per case stays cheap.
const MAX_SUBTASKS: usize = 6;

fn edge_present(mask: &[bool], from: usize, to: usize) -> bool {
    mask[from * MAX_SUBTASKS + to]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// For any acyclic creation order (each new subtask depending on a random
    /// subset of the earlier ones) and any completed set, the scheduler offers
    /// exactly the subtasks whose dependencies are all completed.
    #[test]
    fn claimable_set_is_exactly_the_unblocked_subtasks(
        edge_mask in proptest::collection::vec(any::<bool>(), MAX_SUBTASKS * MAX_SUBTASKS),
        completion_mask in proptest::collection::vec(any::<bool>(), MAX_SUBTASKS),
        count in 1usize..=MAX_SUBTASKS,
    ) {
        let harness = Harness::new();
        let ids: Vec<String> = (0..count).map(|index| format!("task-{index}")).collect();
        for index in 0..count {
            let dependencies: Vec<&str> = (0..index)
                .filter(|earlier| edge_present(&edge_mask, index, *earlier))
                .map(|earlier| ids[earlier].as_str())
                .collect();
            harness.created(&ids[index], "generated", &dependencies);
        }

        let mut completed = vec![false; count];
        for index in 0..count {
            if !completion_mask[index] {
                continue;
            }
            let dependencies_ready = (0..index)
                .filter(|earlier| edge_present(&edge_mask, index, *earlier))
                .all(|earlier| completed[earlier]);
            if dependencies_ready {
                harness.complete(&ids[index]);
                completed[index] = true;
            }
        }

        let expected: Vec<String> = (0..count)
            .filter(|index| {
                !completed[*index]
                    && (0..*index)
                        .filter(|earlier| edge_present(&edge_mask, *index, *earlier))
                        .all(|earlier| completed[earlier])
            })
            .map(|index| ids[index].clone())
            .collect();
        assert_eq!(harness.candidates(), expected);

        if expected.is_empty() {
            prop_assert!(harness.claim_next("generated-worker").is_none());
        } else {
            let claim = harness
                .claim_next("generated-worker")
                .expect("a claimable subtask must be claimable");
            prop_assert!(
                expected.contains(&claim.subtask_id.to_string()),
                "claim-next offered {} outside the unblocked set {expected:?}",
                claim.subtask_id
            );
        }
    }
}

//! Typed meta-task tracking: typed fields, dedupe, supersede, completion,
//! annotations, discovery pages, handles, and the refresh rules.
//!
//! Every case drives the library API against a disposable store, so the
//! assertions describe what a caller observes: the status view, a discovery
//! row, and the typed event payload recorded for the same mutation.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use covey::{
    AnnotateMetaTaskReq, CancelMetaTaskReq, CompleteMetaTaskReq, CompletionPolicy, Covey,
    CoveyError, CreateWorkSubtaskReq, DueDate, Event, EventPayload, EventType, ManualClock,
    MetaTaskAnnotationChange, MetaTaskAnnotationValue, MetaTaskClosure, MetaTaskDedupeKey,
    MetaTaskHandle, MetaTaskId, MetaTaskListQuery, MetaTaskListState, MetaTaskSort,
    MetaTaskSourceLinkDraft, MetaTaskState, MetaTaskStatus, MetaTaskSummary, MetaTaskTitle,
    MetaTaskUrgency, RegisterSessionReq, SessionRole, SubmitMetaTaskReq,
};
use rstest::rstest;
use rusqlite::Connection;
use tempfile::TempDir;

const NOW_MS: i64 = 1_700_000_000_000;
const THREAD_ID: &str = "intake-thread-1";
const RECEIVED_AT: &str = "2026-11-06T04:11:02Z";
const SOURCE_PATH: &str = "/data/intake/2026-11-06/orders.md";

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

fn id_key(label: &str) -> String {
    format!("{label}-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

fn parsed_id(value: &str) -> MetaTaskId {
    MetaTaskId::parse(value.to_owned()).expect("valid meta task id")
}

fn register_with_principal(covey: &Covey, principal: &str, role: SessionRole) -> String {
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

fn register(covey: &Covey, label: &str, role: SessionRole) -> String {
    register_with_principal(covey, &format!("{label}-{}", id_key("principal")), role)
}

fn submitted_link(path: &str) -> MetaTaskSourceLinkDraft {
    MetaTaskSourceLinkDraft::submitted(THREAD_ID, RECEIVED_AT, path, None)
        .expect("valid submitted source link")
}

fn ids(page: &[MetaTaskSummary]) -> Vec<String> {
    page.iter()
        .map(|row| row.meta_task_id().as_str().to_owned())
        .collect()
}

struct Rig {
    _dir: TempDir,
    db_path: PathBuf,
    covey: Covey,
    clock: Arc<ManualClock>,
    orchestrator: String,
}

impl Rig {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("covey.db");
        let clock = Arc::new(ManualClock::new(NOW_MS));
        let covey = Covey::open_with_clock(&db_path, clock.clone()).expect("open");
        let orchestrator = register(&covey, "orchestrator", SessionRole::Orchestrator);
        Self {
            _dir: dir,
            db_path,
            covey,
            clock,
            orchestrator,
        }
    }

    fn request(&self) -> SubmitMetaTaskReq {
        SubmitMetaTaskReq::try_from_raw_parts(
            self.orchestrator.clone(),
            "typed meta task",
            id_key("submit-meta"),
        )
        .expect("valid submit-meta-task request")
    }

    fn submit(&self, request: SubmitMetaTaskReq) -> covey::MetaTaskSubmission {
        self.covey
            .submit_meta_task_reporting(request)
            .expect("submit meta task")
    }

    fn submit_id(&self, request: SubmitMetaTaskReq) -> String {
        self.submit(request).meta_task_id
    }

    fn complete_request(&self, meta_task_id: &str) -> CompleteMetaTaskReq {
        CompleteMetaTaskReq::try_from_raw_parts(
            self.orchestrator.clone(),
            meta_task_id,
            id_key("complete-meta"),
        )
        .expect("valid complete-meta-task request")
    }

    fn cancel_request(&self, meta_task_id: &str) -> CancelMetaTaskReq {
        CancelMetaTaskReq::try_from_raw_parts(
            self.orchestrator.clone(),
            meta_task_id,
            id_key("cancel-meta"),
        )
        .expect("valid cancel-meta-task request")
    }

    fn annotate(&self, meta_task_id: &str, change: MetaTaskAnnotationChange) -> covey::Result<()> {
        self.covey.annotate_meta_task(
            AnnotateMetaTaskReq::try_from_raw_parts(
                self.orchestrator.clone(),
                meta_task_id,
                change,
                id_key("annotate-meta"),
            )
            .expect("valid annotate-meta-task request"),
        )
    }

    fn status(&self, meta_task_id: &str) -> MetaTaskStatus {
        self.covey
            .meta_task_status(meta_task_id)
            .expect("meta task status")
    }

    fn list(&self, query: MetaTaskListQuery) -> Vec<MetaTaskSummary> {
        self.covey
            .list_meta_tasks_with(&query)
            .expect("list meta tasks")
    }

    fn work_request(&self, meta_task_id: &str) -> CreateWorkSubtaskReq {
        CreateWorkSubtaskReq::try_from_raw_parts(
            self.orchestrator.clone(),
            meta_task_id,
            None,
            "tracked work",
            1,
            CompletionPolicy::Direct,
            "default",
            id_key("create-work"),
        )
        .expect("valid work request")
    }

    fn create_subtask(&self, meta_task_id: &str) -> String {
        self.covey
            .create_work_subtask(self.work_request(meta_task_id))
            .expect("create work")
    }

    fn events_of(&self, meta_task_id: &str, event_type: EventType) -> Vec<Event> {
        self.covey
            .fetch_events(0, 1_000)
            .expect("fetch events")
            .into_iter()
            .filter(|event| event.object_id() == meta_task_id && event.event_type() == event_type)
            .collect()
    }

    /// The closure carried by the one cancellation event of a meta task.
    fn cancelled_event_closure(&self, meta_task_id: &str) -> Option<MetaTaskClosure> {
        let events = self.events_of(meta_task_id, EventType::MetaTaskCancelled);
        assert_eq!(events.len(), 1, "exactly one cancellation event");
        match events[0]
            .typed()
            .expect("typed cancellation payload")
            .payload
        {
            EventPayload::MetaTaskCancelled(request) => request.closure,
            other => panic!("unexpected cancellation payload: {other:?}"),
        }
    }

    fn table_count(&self, table: &str) -> i64 {
        let connection = Connection::open(&self.db_path).expect("open database");
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count rows")
    }

    /// Settles one subtask directly, standing in for a finished work lane.
    fn settle_subtask(&self, subtask_id: &str) {
        let connection = Connection::open(&self.db_path).expect("open database");
        connection
            .execute(
                "UPDATE subtasks SET state = 'completed' WHERE subtask_id = ?1",
                [subtask_id],
            )
            .expect("settle subtask");
    }
}

#[test]
fn typed_tracking_fields_survive_submit_and_render_in_status_and_list() {
    let rig = Rig::new();
    let request = rig
        .request()
        .with_title(MetaTaskTitle::parse("typed intake".to_owned()).expect("valid title"))
        .with_urgency(MetaTaskUrgency::High)
        .with_due_on(DueDate::parse("2026-12-01".to_owned()).expect("valid due date"));
    let meta_task_id = rig.submit_id(request);

    let status = rig.status(&meta_task_id);
    let meta_task = status.meta_task();
    assert_eq!(
        meta_task.title.as_ref().map(MetaTaskTitle::as_str),
        Some("typed intake")
    );
    assert_eq!(meta_task.urgency, Some(MetaTaskUrgency::High));
    assert_eq!(
        meta_task.due_on.as_ref().map(DueDate::as_str),
        Some("2026-12-01")
    );
    assert_eq!(meta_task.closure, None);
    assert!(status.supersedes().is_empty());
    assert_eq!(status.superseded_by(), None);
    assert_eq!(status.annotations().verdict, None);
    assert_eq!(status.annotations().acked, None);

    let handle = meta_task
        .handle
        .expect("a migrated row always has a handle");
    assert!(handle.get() >= 1);

    let page = rig.list(MetaTaskListQuery::new(10, 0));
    assert_eq!(page.len(), 1);
    let row = &page[0];
    assert_eq!(row.meta_task_id().as_str(), meta_task_id);
    assert_eq!(row.title().map(MetaTaskTitle::as_str), Some("typed intake"));
    assert_eq!(row.urgency(), Some(MetaTaskUrgency::High));
    assert_eq!(row.due_on().map(DueDate::as_str), Some("2026-12-01"));
    assert_eq!(row.closure(), None);
    assert_eq!(row.handle(), &handle);
    assert_eq!(row.dedupe_key(), None);
    assert_eq!(row.subtask_total(), 0);
    assert_eq!(row.subtask_open(), 0);
}

#[test]
fn submit_meta_task_still_returns_the_bare_id() {
    let rig = Rig::new();
    let meta_task_id = rig
        .covey
        .submit_meta_task(rig.request())
        .expect("submit meta task");
    assert_eq!(
        rig.status(&meta_task_id).meta_task().state(),
        MetaTaskState::Planning
    );
}

#[test]
fn dedupe_hit_returns_the_same_id_and_writes_nothing() {
    let rig = Rig::new();
    let dedupe_key = MetaTaskDedupeKey::parse("intake-42".to_owned()).expect("valid dedupe key");
    let first = rig.submit(rig.request().with_dedupe_key(dedupe_key.clone()));
    assert!(!first.deduplicated);
    let tasks_after_first = rig.table_count("meta_tasks");
    let idempotency_after_first = rig.table_count("mutation_idempotency");

    let second = rig.submit(rig.request().with_dedupe_key(dedupe_key));
    assert!(second.deduplicated);
    assert_eq!(second.meta_task_id, first.meta_task_id);
    assert_eq!(rig.table_count("meta_tasks"), tasks_after_first);
    assert_eq!(
        rig.table_count("mutation_idempotency"),
        idempotency_after_first,
        "a dedupe hit writes no idempotency row"
    );
    assert_eq!(rig.table_count("meta_task_dedupe_keys"), 1);
    assert_eq!(
        rig.events_of(&first.meta_task_id, EventType::MetaTaskSubmitted)
            .len(),
        1,
        "a dedupe hit appends no second submission event"
    );
}

#[test]
fn the_same_dedupe_key_from_another_principal_creates_a_new_task() {
    let rig = Rig::new();
    let dedupe_key = MetaTaskDedupeKey::parse("intake-7".to_owned()).expect("valid dedupe key");
    let first = rig.submit(rig.request().with_dedupe_key(dedupe_key.clone()));
    let other = register(&rig.covey, "other", SessionRole::Orchestrator);
    let second = rig.submit(
        SubmitMetaTaskReq::try_from_raw_parts(other, "typed meta task", id_key("submit-meta"))
            .expect("valid submit request")
            .with_dedupe_key(dedupe_key),
    );

    assert!(!second.deduplicated);
    assert_ne!(second.meta_task_id, first.meta_task_id);
    assert_eq!(rig.table_count("meta_tasks"), 2);
}

#[test]
fn supersede_retires_an_open_task_with_the_superseded_closure() {
    let rig = Rig::new();
    let replaced = rig.submit_id(rig.request());
    let replacement = rig.submit_id(rig.request().with_supersedes([parsed_id(&replaced)]));

    let status = rig.status(&replaced);
    assert_eq!(status.meta_task().state(), MetaTaskState::Cancelled);
    assert_eq!(
        status.meta_task().closure,
        Some(MetaTaskClosure::Superseded)
    );
    assert_eq!(
        status.superseded_by().map(MetaTaskId::as_str),
        Some(replacement.as_str())
    );
    assert_eq!(
        rig.cancelled_event_closure(&replaced),
        Some(MetaTaskClosure::Superseded)
    );

    let replacement_status = rig.status(&replacement);
    assert_eq!(replacement_status.supersedes(), [parsed_id(&replaced)]);
    assert_eq!(replacement_status.superseded_by(), None);
    assert_eq!(rig.table_count("meta_task_supersedes"), 1);
}

#[test]
fn supersede_of_a_terminal_task_keeps_its_state_and_closure() {
    let rig = Rig::new();
    let replaced = rig.submit_id(rig.request());
    rig.covey
        .complete_meta_task(rig.complete_request(&replaced))
        .expect("complete the task to replace");
    let replacement = rig.submit_id(rig.request().with_supersedes([parsed_id(&replaced)]));

    let status = rig.status(&replaced);
    assert_eq!(status.meta_task().state(), MetaTaskState::Completed);
    assert_eq!(status.meta_task().closure, Some(MetaTaskClosure::Done));
    assert_eq!(
        status.superseded_by().map(MetaTaskId::as_str),
        Some(replacement.as_str())
    );
    assert!(
        rig.events_of(&replaced, EventType::MetaTaskCancelled)
            .is_empty(),
        "a finished task is not retired again"
    );
    assert_eq!(rig.table_count("meta_task_supersedes"), 1);
}

#[test]
fn superseding_a_replaced_task_is_refused_and_creates_nothing() {
    let rig = Rig::new();
    let replaced = rig.submit_id(rig.request());
    let replacement = rig.submit_id(rig.request().with_supersedes([parsed_id(&replaced)]));
    let tasks_before = rig.table_count("meta_tasks");

    let error = rig
        .covey
        .submit_meta_task_reporting(rig.request().with_supersedes([parsed_id(&replaced)]))
        .expect_err("a replaced task cannot be replaced twice");
    match error {
        CoveyError::MetaTaskAlreadySuperseded {
            meta_task_id: reported,
            superseded_by,
        } => {
            assert_eq!(reported, replaced);
            assert_eq!(superseded_by, replacement);
        }
        other => panic!("unexpected error: {other}"),
    }
    assert_eq!(rig.table_count("meta_tasks"), tasks_before);
    assert_eq!(rig.table_count("meta_task_supersedes"), 1);
    assert_eq!(
        rig.status(&replaced).meta_task().closure,
        Some(MetaTaskClosure::Superseded)
    );
}

#[test]
fn superseding_an_unknown_task_is_refused_and_creates_nothing() {
    let rig = Rig::new();
    let tasks_before = rig.table_count("meta_tasks");

    let error = rig
        .covey
        .submit_meta_task_reporting(rig.request().with_supersedes([parsed_id("meta_unknown")]))
        .expect_err("an unknown replaced task is refused");
    assert_eq!(error, CoveyError::MetaTaskNotFound);
    assert_eq!(rig.table_count("meta_tasks"), tasks_before);
    assert_eq!(rig.table_count("meta_task_supersedes"), 0);
}

#[test]
fn duplicate_supersede_ids_are_folded_and_order_kept() {
    let rig = Rig::new();
    let first = rig.submit_id(rig.request());
    rig.clock.advance(1_000);
    let second = rig.submit_id(rig.request());
    let mut request = rig.request();
    request.supersedes = vec![parsed_id(&second), parsed_id(&first), parsed_id(&second)];
    let replacement = rig.submit_id(request);

    assert_eq!(
        rig.status(&replacement).supersedes(),
        [parsed_id(&second), parsed_id(&first)],
        "duplicates fold away and the named order survives"
    );
    assert_eq!(rig.table_count("meta_task_supersedes"), 2);
    assert_eq!(
        rig.status(&first).meta_task().state(),
        MetaTaskState::Cancelled
    );
    assert_eq!(
        rig.status(&second).meta_task().state(),
        MetaTaskState::Cancelled
    );
}

#[test]
fn cancel_stores_the_dropped_default_and_carries_it_in_the_event() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_id(rig.request());
    rig.covey
        .cancel_meta_task(rig.cancel_request(&meta_task_id))
        .expect("cancel the meta task");

    let status = rig.status(&meta_task_id);
    assert_eq!(status.meta_task().state(), MetaTaskState::Cancelled);
    assert_eq!(status.meta_task().closure, Some(MetaTaskClosure::Dropped));
    assert_eq!(
        rig.cancelled_event_closure(&meta_task_id),
        Some(MetaTaskClosure::Dropped)
    );
}

#[test]
fn cancel_stores_an_explicit_closure_and_carries_it_in_the_event() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_id(rig.request());
    rig.covey
        .cancel_meta_task(
            rig.cancel_request(&meta_task_id)
                .with_closure(MetaTaskClosure::UpstreamClosed),
        )
        .expect("cancel the meta task");

    let status = rig.status(&meta_task_id);
    assert_eq!(
        status.meta_task().closure,
        Some(MetaTaskClosure::UpstreamClosed)
    );
    assert_eq!(
        rig.cancelled_event_closure(&meta_task_id),
        Some(MetaTaskClosure::UpstreamClosed)
    );
}

#[test]
fn complete_a_childless_planning_task_stores_done_and_one_event() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_id(rig.request());
    rig.covey
        .complete_meta_task(rig.complete_request(&meta_task_id))
        .expect("complete the meta task");

    let status = rig.status(&meta_task_id);
    assert_eq!(status.meta_task().state(), MetaTaskState::Completed);
    assert_eq!(status.meta_task().closure, Some(MetaTaskClosure::Done));
    assert_eq!(
        rig.events_of(&meta_task_id, EventType::MetaTaskCompleted)
            .len(),
        1
    );
    assert!(
        rig.events_of(&meta_task_id, EventType::MetaTaskCancelled)
            .is_empty(),
        "completing tears nothing down"
    );
}

#[test]
fn complete_records_a_new_closure_on_an_already_finished_task() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_id(rig.request());
    rig.covey
        .complete_meta_task(rig.complete_request(&meta_task_id))
        .expect("complete the childless task");
    rig.covey
        .complete_meta_task(
            rig.complete_request(&meta_task_id)
                .with_closure(MetaTaskClosure::UpstreamClosed),
        )
        .expect("record why a finished task closed");

    let status = rig.status(&meta_task_id);
    assert_eq!(status.meta_task().state(), MetaTaskState::Completed);
    assert_eq!(
        status.meta_task().closure,
        Some(MetaTaskClosure::UpstreamClosed),
        "a finished task can still record its closure"
    );
    assert_eq!(
        rig.events_of(&meta_task_id, EventType::MetaTaskCompleted)
            .len(),
        2,
        "each completion call records its own event"
    );
}

#[test]
fn complete_refuses_a_task_with_an_open_subtask() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_id(rig.request());
    rig.create_subtask(&meta_task_id);

    let error = rig
        .covey
        .complete_meta_task(rig.complete_request(&meta_task_id))
        .expect_err("a task with open subtasks cannot be completed");
    match error {
        CoveyError::MetaTaskHasOpenSubtasks {
            meta_task_id: reported,
            open_subtasks,
        } => {
            assert_eq!(reported, meta_task_id);
            assert_eq!(open_subtasks, 1);
        }
        other => panic!("unexpected error: {other}"),
    }

    let status = rig.status(&meta_task_id);
    assert_eq!(status.meta_task().state(), MetaTaskState::Active);
    assert_eq!(status.meta_task().closure, None);
}

#[test]
fn complete_refuses_a_cancelled_task() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_id(rig.request());
    rig.covey
        .cancel_meta_task(rig.cancel_request(&meta_task_id))
        .expect("cancel the meta task");

    let error = rig
        .covey
        .complete_meta_task(rig.complete_request(&meta_task_id))
        .expect_err("a cancelled task cannot be completed");
    assert!(matches!(error, CoveyError::IllegalTransition { .. }));
    assert_eq!(
        rig.status(&meta_task_id).meta_task().closure,
        Some(MetaTaskClosure::Dropped)
    );
}

#[test]
fn replayed_complete_key_appends_one_event() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_id(rig.request());
    let key = id_key("complete-meta");
    let request = || {
        CompleteMetaTaskReq::try_from_raw_parts(
            rig.orchestrator.clone(),
            meta_task_id.clone(),
            key.clone(),
        )
        .expect("valid complete request")
    };

    rig.covey
        .complete_meta_task(request())
        .expect("first completion");
    rig.covey
        .complete_meta_task(request())
        .expect("the same key replays the first completion");

    assert_eq!(
        rig.events_of(&meta_task_id, EventType::MetaTaskCompleted)
            .len(),
        1
    );
}

#[test]
fn annotate_sets_every_key_then_clears_and_records_one_event_per_call() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_id(rig.request());

    rig.annotate(
        &meta_task_id,
        MetaTaskAnnotationChange::Set(
            MetaTaskAnnotationValue::verdict("looks right".to_owned()).expect("valid verdict"),
        ),
    )
    .expect("annotate verdict");
    rig.annotate(
        &meta_task_id,
        MetaTaskAnnotationChange::Set(MetaTaskAnnotationValue::acked(true)),
    )
    .expect("annotate acked");
    rig.annotate(
        &meta_task_id,
        MetaTaskAnnotationChange::Set(
            MetaTaskAnnotationValue::tier("core".to_owned()).expect("valid tier"),
        ),
    )
    .expect("annotate tier");
    rig.annotate(
        &meta_task_id,
        MetaTaskAnnotationChange::Set(
            MetaTaskAnnotationValue::entity("intake".to_owned()).expect("valid entity"),
        ),
    )
    .expect("annotate entity");

    let annotations = rig.status(&meta_task_id).annotations().clone();
    assert_eq!(annotations.verdict.as_deref(), Some("looks right"));
    assert_eq!(annotations.acked, Some(true));
    assert_eq!(annotations.tier.as_deref(), Some("core"));
    assert_eq!(annotations.entity.as_deref(), Some("intake"));
    assert_eq!(rig.table_count("meta_task_annotations"), 4);

    rig.annotate(
        &meta_task_id,
        MetaTaskAnnotationChange::Clear(covey::MetaTaskAnnotationKey::Tier),
    )
    .expect("clear a set key");
    rig.annotate(
        &meta_task_id,
        MetaTaskAnnotationChange::Clear(covey::MetaTaskAnnotationKey::Entity),
    )
    .expect("clear a set key");
    rig.annotate(
        &meta_task_id,
        MetaTaskAnnotationChange::Clear(covey::MetaTaskAnnotationKey::Entity),
    )
    .expect("clearing an absent key succeeds");

    let annotations = rig.status(&meta_task_id).annotations().clone();
    assert_eq!(annotations.verdict.as_deref(), Some("looks right"));
    assert_eq!(annotations.acked, Some(true));
    assert_eq!(annotations.tier, None);
    assert_eq!(annotations.entity, None);
    assert_eq!(rig.table_count("meta_task_annotations"), 2);
    assert_eq!(
        rig.events_of(&meta_task_id, EventType::MetaTaskAnnotated)
            .len(),
        7,
        "each call appends exactly one annotation event"
    );
}

#[test]
fn annotate_requires_an_orchestrator_session() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_id(rig.request());
    let executor = register(&rig.covey, "executor", SessionRole::Executor);

    let error = rig
        .covey
        .annotate_meta_task(
            AnnotateMetaTaskReq::try_from_raw_parts(
                executor,
                meta_task_id.clone(),
                MetaTaskAnnotationChange::Set(MetaTaskAnnotationValue::acked(true)),
                id_key("annotate-meta"),
            )
            .expect("valid annotate request"),
        )
        .expect_err("an executor may not annotate");
    assert!(matches!(error, CoveyError::WrongRole { .. }));
    assert_eq!(rig.table_count("meta_task_annotations"), 0);
}

#[test]
fn blank_annotation_values_are_type_validation_errors() {
    for refused in [
        MetaTaskAnnotationValue::verdict("  "),
        MetaTaskAnnotationValue::tier("".to_owned()),
        MetaTaskAnnotationValue::entity("\t".to_owned()),
    ] {
        let error: CoveyError = refused.expect_err("blank annotation text").into();
        assert!(matches!(error, CoveyError::TypeValidationError(_)));
    }
}

#[rstest]
#[case(MetaTaskListState::Planning, MetaTaskState::Planning)]
#[case(MetaTaskListState::Active, MetaTaskState::Active)]
#[case(MetaTaskListState::Completed, MetaTaskState::Completed)]
#[case(MetaTaskListState::Cancelled, MetaTaskState::Cancelled)]
fn list_state_filter_selects_exactly_one_state(
    #[case] filter: MetaTaskListState,
    #[case] expected: MetaTaskState,
) {
    let rig = Rig::new();
    let planning = rig.submit_id(rig.request());
    let active = rig.submit_id(rig.request());
    rig.create_subtask(&active);
    let completed = rig.submit_id(rig.request());
    rig.covey
        .complete_meta_task(rig.complete_request(&completed))
        .expect("complete a childless task");
    let cancelled = rig.submit_id(rig.request());
    rig.covey
        .cancel_meta_task(rig.cancel_request(&cancelled))
        .expect("cancel a task");

    let page = rig.list(MetaTaskListQuery::new(10, 0).with_state(filter));
    assert_eq!(page.len(), 1, "one task is in {filter}");
    let row = &page[0];
    assert_eq!(row.state(), expected);
    let expected_id: &str = match expected {
        MetaTaskState::Planning => planning.as_str(),
        MetaTaskState::Active => active.as_str(),
        MetaTaskState::Completed => completed.as_str(),
        MetaTaskState::Cancelled => cancelled.as_str(),
    };
    assert_eq!(row.meta_task_id().as_str(), expected_id);
}

#[test]
fn list_open_filter_returns_planning_and_active_tasks() {
    let rig = Rig::new();
    let planning = rig.submit_id(rig.request());
    let active = rig.submit_id(rig.request());
    rig.create_subtask(&active);
    let completed = rig.submit_id(rig.request());
    rig.covey
        .complete_meta_task(rig.complete_request(&completed))
        .expect("complete a childless task");

    let mut open =
        ids(&rig.list(MetaTaskListQuery::new(10, 0).with_state(MetaTaskListState::Open)));
    open.sort();
    let mut expected = vec![planning, active];
    expected.sort();
    assert_eq!(open, expected);
}

#[test]
fn list_due_before_excludes_rows_without_a_due_date() {
    let rig = Rig::new();
    let early = rig.submit_id(
        rig.request()
            .with_due_on(DueDate::parse("2026-01-01".to_owned()).expect("valid due date")),
    );
    let late = rig.submit_id(
        rig.request()
            .with_due_on(DueDate::parse("2026-06-01".to_owned()).expect("valid due date")),
    );
    let undated = rig.submit_id(rig.request());

    let page = rig.list(
        MetaTaskListQuery::new(10, 0)
            .with_due_before(DueDate::parse("2026-03-01".to_owned()).expect("valid due date")),
    );
    assert_eq!(ids(&page), [early]);
    let returned = ids(&page);
    assert!(!returned.contains(&late));
    assert!(!returned.contains(&undated));
}

#[test]
fn list_source_thread_filter_matches_the_linked_task() {
    let rig = Rig::new();
    let linked = rig.submit_id(rig.request().with_source_link(submitted_link(SOURCE_PATH)));
    let unlinked = rig.submit_id(rig.request());

    let page = rig.list(MetaTaskListQuery::new(10, 0).with_source_thread_id(THREAD_ID));
    assert_eq!(ids(&page), [linked]);
    assert!(!ids(&page).contains(&unlinked));
    assert_eq!(
        page[0]
            .source_link()
            .expect("the listed row carries its link")
            .thread_id(),
        THREAD_ID
    );
}

#[test]
fn list_dedupe_key_filter_matches_the_filed_key() {
    let rig = Rig::new();
    let dedupe_key = MetaTaskDedupeKey::parse("intake-9".to_owned()).expect("valid dedupe key");
    let keyed = rig.submit_id(rig.request().with_dedupe_key(dedupe_key.clone()));
    let plain = rig.submit_id(rig.request());

    let page = rig.list(MetaTaskListQuery::new(10, 0).with_dedupe_key(dedupe_key));
    assert_eq!(ids(&page), [keyed]);
    assert!(!ids(&page).contains(&plain));
    assert_eq!(
        page[0].dedupe_key().map(MetaTaskDedupeKey::as_str),
        Some("intake-9")
    );
}

#[test]
fn list_sorts_place_undated_and_unurged_rows_last() {
    let rig = Rig::new();
    let undated = rig.submit_id(rig.request());
    rig.clock.advance(1_000);
    let dated = rig.submit_id(
        rig.request()
            .with_due_on(DueDate::parse("2026-02-01".to_owned()).expect("valid due date")),
    );
    rig.clock.advance(1_000);
    let plain = rig.submit_id(rig.request());
    rig.clock.advance(1_000);
    let urgent = rig.submit_id(rig.request().with_urgency(MetaTaskUrgency::High));

    let by_creation =
        ids(&rig.list(MetaTaskListQuery::new(10, 0).with_sort(MetaTaskSort::CreatedAt)));
    assert_eq!(
        by_creation,
        [
            urgent.clone(),
            plain.clone(),
            dated.clone(),
            undated.clone()
        ]
    );

    let by_due = ids(&rig.list(MetaTaskListQuery::new(10, 0).with_sort(MetaTaskSort::Due)));
    assert_eq!(
        by_due,
        [
            dated.clone(),
            urgent.clone(),
            plain.clone(),
            undated.clone()
        ],
        "the dated row leads and the undated row trails"
    );

    let by_urgency = ids(&rig.list(MetaTaskListQuery::new(10, 0).with_sort(MetaTaskSort::Urgency)));
    assert_eq!(
        by_urgency,
        [urgent, plain, dated, undated],
        "the urgent row leads and rows without an urgency trail"
    );
}

#[test]
fn list_counts_total_and_open_subtasks() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_id(rig.request());
    let first = rig.create_subtask(&meta_task_id);
    rig.create_subtask(&meta_task_id);
    rig.settle_subtask(&first);

    let page = rig.list(MetaTaskListQuery::new(10, 0));
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].subtask_total(), 2);
    assert_eq!(page[0].subtask_open(), 1);
}

#[test]
fn list_rows_carry_annotations() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_id(rig.request());
    rig.annotate(
        &meta_task_id,
        MetaTaskAnnotationChange::Set(MetaTaskAnnotationValue::acked(false)),
    )
    .expect("annotate acked");
    rig.annotate(
        &meta_task_id,
        MetaTaskAnnotationChange::Set(
            MetaTaskAnnotationValue::verdict("accepted".to_owned()).expect("valid verdict"),
        ),
    )
    .expect("annotate verdict");

    let page = rig.list(MetaTaskListQuery::new(10, 0));
    let annotations = page[0].annotations();
    assert_eq!(annotations.acked, Some(false));
    assert_eq!(annotations.verdict.as_deref(), Some("accepted"));
    assert_eq!(annotations.tier, None);
}

#[test]
fn handle_resolves_the_meta_task_and_unknown_handles_are_refused() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_id(rig.request());
    let handle = rig
        .status(&meta_task_id)
        .meta_task()
        .handle
        .expect("a migrated row always has a handle");

    assert_eq!(
        rig.covey
            .meta_task_id_for_handle(handle)
            .expect("resolve a known handle")
            .as_str(),
        meta_task_id
    );
    assert_eq!(
        handle,
        MetaTaskHandle::parse(handle.to_string()).expect("handle text round-trips")
    );

    let unknown = MetaTaskHandle::parse("9999").expect("valid handle");
    let error = rig
        .covey
        .meta_task_id_for_handle(unknown)
        .expect_err("an unknown handle is refused");
    assert_eq!(error, CoveyError::MetaTaskHandleNotFound { handle: 9999 });
}

#[test]
fn a_completed_childless_task_takes_no_new_subtask_and_stays_closed() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_id(rig.request());
    rig.covey
        .complete_meta_task(rig.complete_request(&meta_task_id))
        .expect("complete a childless task");

    let error = rig
        .covey
        .create_work_subtask(rig.work_request(&meta_task_id))
        .expect_err("a finished task takes no new work");

    assert!(
        matches!(error, CoveyError::MetaTaskUnavailable { .. }),
        "unexpected error: {error}"
    );
    let status = rig.status(&meta_task_id);
    assert_eq!(
        status.meta_task().state(),
        MetaTaskState::Completed,
        "an explicit completion is durable"
    );
    assert_eq!(status.meta_task().closure, Some(MetaTaskClosure::Done));
    assert!(
        status.subtasks().is_empty(),
        "the refused request writes no subtask"
    );
}

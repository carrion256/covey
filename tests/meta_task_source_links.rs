//! Meta-task source backlinks: submit-time provenance and set-once backfill.
//!
//! The link is the intake source of a meta task, stored as a typed side row so
//! provenance stops living inside `prompt_text`. These cases cover what the
//! status surface renders, the set-once rule for backfills, the authority rule
//! shared with `meta cancel`, and the idempotency behaviour of both writers.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use covey::{
    Covey, CoveyError, LinkMetaTaskSourceReq, ManualClock, MetaTaskSourceLinkDraft,
    MetaTaskSourceLinkOrigin, RegisterSessionReq, SessionRole, SubmitMetaTaskReq,
};
use rstest::rstest;
use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

const NOW_MS: i64 = 1_700_000_000_000;
const THREAD_ID: &str = "hermes-thread-7";
const RECEIVED_AT: &str = "2026-11-06T04:11:02Z";
const SOURCE_PATH: &str = "/data/intake/2026-11-06/orders.md";
const SOURCE_URL: &str = "https://example.test/thread/7";
const METHOD: &str = "commander transcribed the originating message";

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

fn id_key(label: &str) -> String {
    format!("{label}-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
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

fn submitted_draft(path: &str) -> covey::Result<MetaTaskSourceLinkDraft> {
    MetaTaskSourceLinkDraft::submitted(THREAD_ID, RECEIVED_AT, path, Some(SOURCE_URL.to_owned()))
        .map_err(CoveyError::from)
}

/// The rendered `source_link` object of a status payload.
fn rendered_link(status: &Value) -> Value {
    status["meta_task"]["source_link"].clone()
}

struct Rig {
    _dir: TempDir,
    db_path: PathBuf,
    covey: Covey,
    orchestrator: String,
}

impl Rig {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("covey.db");
        let covey =
            Covey::open_with_clock(&db_path, Arc::new(ManualClock::new(NOW_MS))).expect("open");
        let orchestrator = register(&covey, "orchestrator", SessionRole::Orchestrator);
        Self {
            _dir: dir,
            db_path,
            covey,
            orchestrator,
        }
    }

    fn submit(&self, source_link: Option<MetaTaskSourceLinkDraft>) -> String {
        let mut request = SubmitMetaTaskReq::try_from_raw_parts(
            self.orchestrator.clone(),
            "intake scenario",
            id_key("submit-meta"),
        )
        .expect("valid submit-meta-task request");
        if let Some(source_link) = source_link {
            request = request.with_source_link(source_link);
        }
        self.covey
            .submit_meta_task(request)
            .expect("submit meta task")
    }

    fn submit_unlinked(&self) -> String {
        self.submit(None)
    }

    fn submit_linked(&self) -> String {
        self.submit(Some(
            submitted_draft(SOURCE_PATH).expect("valid submitted source link"),
        ))
    }

    fn link_source(&self, meta_task_id: &str) -> covey::Result<()> {
        self.covey.link_meta_task_source(
            LinkMetaTaskSourceReq::try_from_raw_parts(
                self.orchestrator.clone(),
                meta_task_id,
                THREAD_ID,
                RECEIVED_AT,
                SOURCE_PATH,
                Some(SOURCE_URL.to_owned()),
                METHOD,
                id_key("link-source"),
            )
            .expect("valid link-source request"),
        )
    }

    fn link_source_with_key(&self, meta_task_id: &str, key: &str) -> covey::Result<()> {
        self.covey.link_meta_task_source(
            LinkMetaTaskSourceReq::try_from_raw_parts(
                self.orchestrator.clone(),
                meta_task_id,
                THREAD_ID,
                RECEIVED_AT,
                SOURCE_PATH,
                None,
                METHOD,
                key,
            )
            .expect("valid link-source request"),
        )
    }

    fn status_json(&self, meta_task_id: &str) -> Value {
        let status = self
            .covey
            .meta_task_status(meta_task_id)
            .expect("meta task status");
        serde_json::to_value(&status).expect("status serializes")
    }

    fn link_row_count(&self, meta_task_id: &str) -> i64 {
        let connection = Connection::open(&self.db_path).expect("open database");
        connection
            .query_row(
                "SELECT COUNT(*) FROM meta_task_source_links WHERE meta_task_id = ?1",
                [meta_task_id],
                |row| row.get(0),
            )
            .expect("count source link rows")
    }

    /// Rows the backfill appended to the event log for this meta task.
    fn source_link_event_count(&self, meta_task_id: &str) -> i64 {
        let connection = Connection::open(&self.db_path).expect("open database");
        connection
            .query_row(
                "SELECT COUNT(*) FROM event_log WHERE event_type = 'meta_task_source_linked' AND object_id = ?1",
                [meta_task_id],
                |row| row.get(0),
            )
            .expect("count source link events")
    }
}

#[test]
fn submit_with_source_link_reports_it_as_submitted_provenance() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_linked();

    let link = rendered_link(&rig.status_json(&meta_task_id));
    assert_eq!(link["thread_id"], Value::String(THREAD_ID.to_owned()));
    assert_eq!(link["url"], Value::String(SOURCE_URL.to_owned()));
    assert_eq!(link["received_at"], Value::String(RECEIVED_AT.to_owned()));
    assert_eq!(link["path"], Value::String(SOURCE_PATH.to_owned()));
    assert_eq!(link["origin"], Value::String("submitted".to_owned()));
    assert_eq!(link["method"], Value::Null);
    assert_eq!(link["linked_at"], Value::from(NOW_MS));

    let status = rig
        .covey
        .meta_task_status(&meta_task_id)
        .expect("meta task status");
    let typed = status
        .meta_task()
        .source_link()
        .expect("source link is present");
    assert_eq!(typed.origin(), MetaTaskSourceLinkOrigin::Submitted);
    assert_eq!(typed.method(), None);
    assert_eq!(typed.thread_id(), THREAD_ID);
    assert_eq!(typed.path(), SOURCE_PATH);
}

#[test]
fn a_linking_session_is_stored_but_never_rendered() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_unlinked();
    let linker = register(&rig.covey, "linker", SessionRole::Orchestrator);

    rig.covey
        .link_meta_task_source(
            LinkMetaTaskSourceReq::try_from_raw_parts(
                linker.clone(),
                meta_task_id.clone(),
                THREAD_ID,
                RECEIVED_AT,
                SOURCE_PATH,
                None,
                METHOD,
                id_key("link-source"),
            )
            .expect("valid link-source request"),
        )
        .expect("link source");

    let status = rig.status_json(&meta_task_id);
    let link = status["meta_task"]["source_link"]
        .as_object()
        .expect("link object");
    assert!(
        !link.contains_key("linked_by"),
        "the linking session is stored, never rendered"
    );
    assert!(
        !status.to_string().contains(&linker),
        "the linking session token must not reach the status view: {status}"
    );
}

#[test]
fn submit_without_source_link_reports_no_link() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_unlinked();

    assert_eq!(rendered_link(&rig.status_json(&meta_task_id)), Value::Null);
    assert_eq!(rig.link_row_count(&meta_task_id), 0);
    assert_eq!(
        rig.source_link_event_count(&meta_task_id),
        0,
        "submitting without provenance writes no source-link event"
    );
}

#[test]
fn link_source_attaches_backfilled_provenance_to_an_unlinked_task() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_unlinked();
    assert_eq!(rendered_link(&rig.status_json(&meta_task_id)), Value::Null);

    rig.link_source(&meta_task_id).expect("link source");

    let link = rendered_link(&rig.status_json(&meta_task_id));
    assert_eq!(link["origin"], Value::String("backfilled".to_owned()));
    assert_eq!(link["method"], Value::String(METHOD.to_owned()));
    assert_eq!(link["thread_id"], Value::String(THREAD_ID.to_owned()));
    assert_eq!(link["path"], Value::String(SOURCE_PATH.to_owned()));
    assert_eq!(link["url"], Value::String(SOURCE_URL.to_owned()));
    assert_eq!(link["linked_at"], Value::from(NOW_MS));
    assert_eq!(rig.link_row_count(&meta_task_id), 1);
    assert_eq!(
        rig.source_link_event_count(&meta_task_id),
        1,
        "the backfill appends exactly one meta_task_source_linked event"
    );
}

#[test]
fn second_link_source_is_refused_and_leaves_the_original_link_intact() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_unlinked();
    rig.link_source(&meta_task_id).expect("first link source");
    let before = rig.status_json(&meta_task_id);

    let error = rig
        .link_source(&meta_task_id)
        .expect_err("a meta task holds exactly one source link");
    assert_eq!(
        error,
        CoveyError::SourceLinkExists {
            meta_task_id: meta_task_id.clone()
        }
    );
    assert_eq!(
        error.to_string(),
        format!("meta-task {meta_task_id} already has a source link")
    );

    assert_eq!(rig.status_json(&meta_task_id), before);
    assert_eq!(rig.link_row_count(&meta_task_id), 1);
}

#[test]
fn link_source_on_an_unknown_task_reports_the_existing_not_found_error() {
    let rig = Rig::new();

    assert_eq!(
        rig.link_source("meta-does-not-exist")
            .expect_err("unknown meta task"),
        CoveyError::MetaTaskNotFound
    );
}

#[rstest]
#[case::refused_at_intake(true)]
#[case::refused_on_backfill(false)]
fn a_relative_source_path_is_refused_by_both_writers(#[case] through_submit: bool) {
    let rig = Rig::new();
    let relative_path = "relative/orders.md";

    let error = if through_submit {
        MetaTaskSourceLinkDraft::submitted(THREAD_ID, RECEIVED_AT, relative_path, None)
            .expect_err("a relative intake path must be refused")
    } else {
        LinkMetaTaskSourceReq::try_from_raw_parts(
            rig.orchestrator.clone(),
            rig.submit_unlinked(),
            THREAD_ID,
            RECEIVED_AT,
            relative_path,
            None,
            METHOD,
            id_key("link-source"),
        )
        .expect_err("a relative backfill path must be refused")
    };

    assert!(
        error.to_string().contains("absolute"),
        "the refusal must name the absolute-path rule, got {error}"
    );
}

#[test]
fn blank_source_fields_are_refused_at_the_boundary() {
    assert!(
        MetaTaskSourceLinkDraft::submitted("  ", RECEIVED_AT, SOURCE_PATH, None).is_err(),
        "a blank thread id must not be accepted"
    );
    assert!(
        MetaTaskSourceLinkDraft::submitted(THREAD_ID, "", SOURCE_PATH, None).is_err(),
        "a blank receipt stamp must not be accepted"
    );
    assert!(
        MetaTaskSourceLinkDraft::backfilled(THREAD_ID, RECEIVED_AT, SOURCE_PATH, None, " ")
            .is_err(),
        "a backfill must name a real method"
    );
}

#[test]
fn illegal_origin_and_method_pairings_cannot_be_read_back() {
    // The typed model makes these unconstructible; a stored row claiming one is
    // refused when deserialized, so the table CHECK is not the only guard.
    let submitted_with_method = r#"{
        "thread_id":"t","url":null,"received_at":"r","path":"/p",
        "origin":"submitted","method":"guessed","linked_at":1
    }"#;
    assert!(
        serde_json::from_str::<covey::MetaTaskSourceLink>(submitted_with_method).is_err(),
        "a submitted link cannot carry a method"
    );
    let backfilled_without_method = r#"{
        "thread_id":"t","url":null,"received_at":"r","path":"/p",
        "origin":"backfilled","method":null,"linked_at":1
    }"#;
    assert!(
        serde_json::from_str::<covey::MetaTaskSourceLink>(backfilled_without_method).is_err(),
        "a backfilled link must carry a method"
    );
}

#[test]
fn link_source_requires_the_authority_that_cancel_requires() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_unlinked();
    let executor = register(&rig.covey, "executor", SessionRole::Executor);

    let link_error = rig
        .covey
        .link_meta_task_source(
            LinkMetaTaskSourceReq::try_from_raw_parts(
                executor.clone(),
                meta_task_id.clone(),
                THREAD_ID,
                RECEIVED_AT,
                SOURCE_PATH,
                None,
                METHOD,
                id_key("link-source"),
            )
            .expect("valid link-source request"),
        )
        .expect_err("an executor may not attach provenance");
    let cancel_error = rig
        .covey
        .cancel_meta_task(
            covey::CancelMetaTaskReq::try_from_raw_parts(
                executor,
                meta_task_id.clone(),
                id_key("cancel-meta"),
            )
            .expect("valid cancel request"),
        )
        .expect_err("an executor may not cancel");

    assert_eq!(link_error, cancel_error);
    assert!(matches!(link_error, CoveyError::WrongRole { .. }));
    assert_eq!(rig.link_row_count(&meta_task_id), 0);
}

#[test]
fn replaying_the_same_link_source_key_writes_no_second_row() {
    let rig = Rig::new();
    let meta_task_id = rig.submit_unlinked();
    let key = id_key("link-source");

    rig.link_source_with_key(&meta_task_id, &key)
        .expect("first link source");
    rig.link_source_with_key(&meta_task_id, &key)
        .expect("the same key replays the original success");

    assert_eq!(rig.link_row_count(&meta_task_id), 1);
    assert_eq!(
        rig.source_link_event_count(&meta_task_id),
        1,
        "a replayed key appends no second event"
    );
    let link = rendered_link(&rig.status_json(&meta_task_id));
    assert_eq!(link["origin"], Value::String("backfilled".to_owned()));
    assert_eq!(link["url"], Value::Null, "an omitted URL stays absent");
}

#[test]
fn a_source_link_participates_in_the_submit_idempotency_fingerprint() {
    let rig = Rig::new();
    let key = id_key("submit-meta");
    let request = |path: &str| {
        SubmitMetaTaskReq::try_from_raw_parts(rig.orchestrator.clone(), "same key", key.clone())
            .expect("valid submit-meta-task request")
            .with_source_link(submitted_draft(path).expect("valid submitted source link"))
    };

    rig.covey
        .submit_meta_task(request(SOURCE_PATH))
        .expect("first submission");

    let error = rig
        .covey
        .submit_meta_task(request("/data/intake/other.md"))
        .expect_err("the same key with a different link must conflict");
    assert!(matches!(error, CoveyError::IdempotencyConflict { .. }));
}

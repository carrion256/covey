use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use serde_json::Value;
use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_covey")
}

fn command(db_path: &Path) -> Command {
    let mut command = Command::new(bin());
    command
        .env_remove("COVEY_DB_PATH")
        .env_remove("COVEY_LOG")
        .arg("--db")
        .arg(db_path);
    command
}

fn run_db(db_path: &Path, args: &[&str]) -> Output {
    command(db_path)
        .args(args)
        .output()
        .expect("run covey with db")
}

fn run_db_with_stdin(db_path: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = command(db_path)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn covey with stdin");
    child
        .stdin
        .as_mut()
        .expect("child stdin")
        .write_all(stdin.as_bytes())
        .expect("write child stdin");
    drop(child.stdin.take());
    child.wait_with_output().expect("wait for covey")
}

fn exit_code(output: &Output) -> i32 {
    output
        .status
        .code()
        .expect("covey exited without a status code")
}

fn success_data(output: &Output) -> Value {
    assert_eq!(
        exit_code(output),
        0,
        "stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("stdout json");
    assert_eq!(payload["ok"], Value::Bool(true));
    payload["data"].clone()
}

fn error_code(output: &Output) -> String {
    let payload: Value = serde_json::from_slice(&output.stderr).expect("stderr json");
    assert_eq!(payload["ok"], Value::Bool(false));
    payload["code"].as_str().expect("error code").to_owned()
}

fn id_of(value: &Value, field: &str) -> String {
    value[field]
        .as_str()
        .unwrap_or_else(|| panic!("missing {field} in {value}"))
        .to_owned()
}

fn register_orchestrator(db_path: &Path, principal: &str) -> String {
    let instance = format!("{principal}-instance");
    id_of(
        &success_data(&run_db(
            db_path,
            &[
                "session",
                "register",
                "--agent-principal-id",
                principal,
                "--agent-instance-id",
                &instance,
                "--role",
                "orchestrator",
            ],
        )),
        "session_token",
    )
}

fn submit(db_path: &Path, session_token: &str, prompt_text: &str) -> String {
    id_of(
        &success_data(&run_db(
            db_path,
            &[
                "meta",
                "submit",
                "--session-token",
                session_token,
                "--prompt-text",
                prompt_text,
            ],
        )),
        "meta_task_id",
    )
}

fn status(db_path: &Path, selector: &[&str]) -> Value {
    let mut args = vec!["meta", "status"];
    args.extend_from_slice(selector);
    success_data(&run_db(db_path, &args))
}

fn list_ids(db_path: &Path, args: &[&str]) -> Vec<String> {
    let mut full_args = vec!["meta", "list"];
    full_args.extend_from_slice(args);
    success_data(&run_db(db_path, &full_args))
        .as_array()
        .expect("list rows")
        .iter()
        .map(|row| id_of(row, "meta_task_id"))
        .collect()
}

#[test]
fn submit_records_typed_fields_in_status_and_list() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let orch = register_orchestrator(&db, "typed-orch");

    let meta_task_id = id_of(
        &success_data(&run_db(
            &db,
            &[
                "meta",
                "submit",
                "--session-token",
                &orch,
                "--prompt-text",
                "ship the typed fields",
                "--title",
                "Ship typed fields",
                "--urgency",
                "high",
                "--due",
                "2026-12-01",
            ],
        )),
        "meta_task_id",
    );

    let status = status(&db, &["--meta-task-id", &meta_task_id]);
    assert_eq!(status["meta_task"]["title"], "Ship typed fields");
    assert_eq!(status["meta_task"]["urgency"], "high");
    assert_eq!(status["meta_task"]["due_on"], "2026-12-01");
    assert_eq!(status["meta_task"]["handle"], Value::from(1));

    let listed = success_data(&run_db(&db, &["meta", "list"]));
    assert_eq!(
        listed[0]["meta_task_id"].as_str(),
        Some(meta_task_id.as_str())
    );
    assert_eq!(listed[0]["title"], "Ship typed fields");
    assert_eq!(listed[0]["urgency"], "high");
    assert_eq!(listed[0]["due_on"], "2026-12-01");
    assert_eq!(listed[0]["handle"], Value::from(1));
}

#[test]
fn prompt_file_reads_file_and_stdin_and_strips_one_newline() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let orch = register_orchestrator(&db, "prompt-orch");

    let prompt_path = tmp.path().join("prompt.txt");
    fs::write(&prompt_path, "from a file\n").expect("write prompt file");
    let from_file = id_of(
        &success_data(&run_db(
            &db,
            &[
                "meta",
                "submit",
                "--session-token",
                &orch,
                "--prompt-file",
                prompt_path.to_str().expect("prompt path"),
            ],
        )),
        "meta_task_id",
    );
    assert_eq!(
        status(&db, &["--meta-task-id", &from_file])["meta_task"]["prompt_text"],
        "from a file"
    );

    let from_stdin = id_of(
        &success_data(&run_db_with_stdin(
            &db,
            &[
                "meta",
                "submit",
                "--session-token",
                &orch,
                "--prompt-file",
                "-",
            ],
            "from stdin\r\n",
        )),
        "meta_task_id",
    );
    assert_eq!(
        status(&db, &["--meta-task-id", &from_stdin])["meta_task"]["prompt_text"],
        "from stdin"
    );
}

#[test]
fn prompt_source_flags_are_exclusive_and_required() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let orch = register_orchestrator(&db, "prompt-flags-orch");
    let prompt_path = tmp.path().join("prompt.txt");
    fs::write(&prompt_path, "text\n").expect("write prompt file");

    let both = run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "text",
            "--prompt-file",
            prompt_path.to_str().expect("prompt path"),
        ],
    );
    assert_eq!(exit_code(&both), 2);
    assert_eq!(error_code(&both), "invalid_args");

    let neither = run_db(&db, &["meta", "submit", "--session-token", &orch]);
    assert_eq!(exit_code(&neither), 2);
    assert_eq!(error_code(&neither), "invalid_args");
}

#[test]
fn handle_selectors_resolve_and_unknown_handle_is_not_found() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let orch = register_orchestrator(&db, "handle-orch");
    let meta_task_id = submit(&db, &orch, "handle me");

    for selector in ["1", "#1"] {
        assert_eq!(
            status(&db, &["--handle", selector])["meta_task"]["meta_task_id"].as_str(),
            Some(meta_task_id.as_str())
        );
    }

    let missing = run_db(&db, &["meta", "status", "--handle", "99"]);
    assert_eq!(exit_code(&missing), 1);
    assert_eq!(error_code(&missing), "not_found");
}

#[test]
fn complete_closes_childless_task_and_refuses_open_subtasks_and_cancelled_tasks() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let orch = register_orchestrator(&db, "complete-orch");

    let childless = submit(&db, &orch, "no children");
    let ack = success_data(&run_db(
        &db,
        &[
            "meta",
            "complete",
            "--session-token",
            &orch,
            "--handle",
            "1",
        ],
    ));
    assert_eq!(ack["operation"], "complete");
    assert_eq!(ack["meta_task_id"].as_str(), Some(childless.as_str()));
    assert_eq!(ack["closure"], "done");
    let closed = status(&db, &["--handle", "1"]);
    assert_eq!(closed["meta_task"]["state"], "completed");
    assert_eq!(closed["meta_task"]["closure"], "done");

    let with_child = submit(&db, &orch, "has a child");
    success_data(&run_db(
        &db,
        &[
            "subtask",
            "create",
            "--session-token",
            &orch,
            "--meta-task-id",
            &with_child,
            "--title",
            "child",
            "--kind",
            "work",
            "--priority",
            "1",
            "--subtask-id",
            "child_work",
        ],
    ));
    let refused = run_db(
        &db,
        &[
            "meta",
            "complete",
            "--session-token",
            &orch,
            "--meta-task-id",
            &with_child,
        ],
    );
    assert_eq!(exit_code(&refused), 4);
    assert_eq!(error_code(&refused), "conflict");

    let cancelled = submit(&db, &orch, "already cancelled");
    success_data(&run_db(
        &db,
        &[
            "meta",
            "cancel",
            "--session-token",
            &orch,
            "--meta-task-id",
            &cancelled,
        ],
    ));
    let again = run_db(
        &db,
        &[
            "meta",
            "complete",
            "--session-token",
            &orch,
            "--meta-task-id",
            &cancelled,
        ],
    );
    assert_eq!(exit_code(&again), 4);
    assert_eq!(error_code(&again), "conflict");
}

#[test]
fn cancel_records_typed_closure_in_status() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let orch = register_orchestrator(&db, "cancel-orch");
    let meta_task_id = submit(&db, &orch, "upstream went away");

    let ack = success_data(&run_db(
        &db,
        &[
            "meta",
            "cancel",
            "--session-token",
            &orch,
            "--meta-task-id",
            &meta_task_id,
            "--closure",
            "upstream_closed",
        ],
    ));
    assert_eq!(ack["operation"], "cancel");
    assert_eq!(ack["closure"], "upstream_closed");

    let cancelled = status(&db, &["--meta-task-id", &meta_task_id]);
    assert_eq!(cancelled["meta_task"]["state"], "cancelled");
    assert_eq!(cancelled["meta_task"]["closure"], "upstream_closed");
}

#[test]
fn supersedes_retires_the_old_task_and_refuses_a_second_replacement() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let orch = register_orchestrator(&db, "supersede-orch");

    let first = submit(&db, &orch, "first plan");
    let second = id_of(
        &success_data(&run_db(
            &db,
            &[
                "meta",
                "submit",
                "--session-token",
                &orch,
                "--prompt-text",
                "replacement plan",
                "--supersedes",
                &first,
            ],
        )),
        "meta_task_id",
    );

    let retired = status(&db, &["--meta-task-id", &first]);
    assert_eq!(retired["meta_task"]["state"], "cancelled");
    assert_eq!(retired["meta_task"]["closure"], "superseded");
    assert_eq!(retired["superseded_by"].as_str(), Some(second.as_str()));
    let replacement = status(&db, &["--meta-task-id", &second]);
    assert_eq!(replacement["supersedes"][0].as_str(), Some(first.as_str()));

    let twice = run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "third plan",
            "--supersedes",
            &first,
        ],
    );
    assert_eq!(exit_code(&twice), 4);
    assert_eq!(error_code(&twice), "conflict");

    let before = list_ids(&db, &[]).len();
    let unknown = run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "fourth plan",
            "--supersedes",
            "meta_missing",
        ],
    );
    assert_eq!(exit_code(&unknown), 1);
    assert_eq!(error_code(&unknown), "not_found");
    assert_eq!(
        before,
        list_ids(&db, &[]).len(),
        "a refused supersede must create no task"
    );
}

#[test]
fn dedupe_key_returns_the_existing_task() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let orch = register_orchestrator(&db, "dedupe-orch");

    let first = success_data(&run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "one task per key",
            "--dedupe-key",
            "digest-weekly",
        ],
    ));
    assert_eq!(first["deduplicated"], Value::Null);

    let again = success_data(&run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "one task per key",
            "--dedupe-key",
            "digest-weekly",
        ],
    ));
    assert_eq!(again["deduplicated"], Value::Bool(true));
    assert_eq!(again["meta_task_id"], first["meta_task_id"]);
}

#[test]
fn annotate_sets_and_clears_values() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let orch = register_orchestrator(&db, "annotate-orch");
    let meta_task_id = submit(&db, &orch, "annotate me");

    let set = success_data(&run_db(
        &db,
        &[
            "meta",
            "annotate",
            "--session-token",
            &orch,
            "--meta-task-id",
            &meta_task_id,
            "--verdict",
            "approved",
        ],
    ));
    assert_eq!(set["operation"], "annotate");
    assert_eq!(set["key"], "verdict");
    assert_eq!(set["value"], "approved");
    assert_eq!(
        status(&db, &["--meta-task-id", &meta_task_id])["annotations"]["verdict"],
        "approved"
    );

    let acked = success_data(&run_db(
        &db,
        &[
            "meta",
            "annotate",
            "--session-token",
            &orch,
            "--meta-task-id",
            &meta_task_id,
            "--acked",
            "true",
        ],
    ));
    assert_eq!(acked["key"], "acked");
    assert_eq!(acked["value"], "true");
    assert_eq!(
        status(&db, &["--meta-task-id", &meta_task_id])["annotations"]["acked"],
        Value::Bool(true)
    );

    let cleared = success_data(&run_db(
        &db,
        &[
            "meta",
            "annotate",
            "--session-token",
            &orch,
            "--meta-task-id",
            &meta_task_id,
            "--clear",
            "verdict",
        ],
    ));
    assert_eq!(cleared["key"], "verdict");
    assert_eq!(cleared["value"], Value::Null);
    assert_eq!(
        status(&db, &["--meta-task-id", &meta_task_id])["annotations"]["verdict"],
        Value::Null
    );
}

#[test]
fn list_filters_and_sorts() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let orch = register_orchestrator(&db, "list-orch");

    let early = id_of(
        &success_data(&run_db(
            &db,
            &[
                "meta",
                "submit",
                "--session-token",
                &orch,
                "--prompt-text",
                "due early",
                "--due",
                "2026-12-01",
                "--urgency",
                "low",
            ],
        )),
        "meta_task_id",
    );
    let late = id_of(
        &success_data(&run_db(
            &db,
            &[
                "meta",
                "submit",
                "--session-token",
                &orch,
                "--prompt-text",
                "due late",
                "--due",
                "2027-01-01",
                "--urgency",
                "high",
            ],
        )),
        "meta_task_id",
    );
    let undated = submit(&db, &orch, "no due date");

    let before = list_ids(&db, &["--due-before", "2027-01-01"]);
    assert_eq!(before, vec![early.clone()]);

    let sorted = list_ids(&db, &["--sort", "due"]);
    assert_eq!(sorted, vec![early.clone(), late.clone(), undated.clone()]);
    let sorted_rows = success_data(&run_db(&db, &["meta", "list", "--sort", "due"]));
    assert_eq!(sorted_rows[2]["due_on"], Value::Null);

    let urgent = list_ids(&db, &["--sort", "urgency"]);
    assert_eq!(urgent, vec![late.clone(), early.clone(), undated.clone()]);

    success_data(&run_db(
        &db,
        &[
            "meta",
            "cancel",
            "--session-token",
            &orch,
            "--meta-task-id",
            &undated,
        ],
    ));
    let open = list_ids(&db, &["--state", "open"]);
    assert!(open.contains(&early));
    assert!(open.contains(&late));
    assert!(!open.contains(&undated));
}

#[test]
fn session_ensure_registers_then_reuses_and_adds_another_role() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");

    let first = success_data(&run_db(
        &db,
        &[
            "session",
            "ensure",
            "--agent-principal-id",
            "ensure-agent",
            "--agent-instance-id",
            "ensure-instance-1",
            "--role",
            "orchestrator",
        ],
    ));
    assert_eq!(first["registered"], Value::Bool(true));
    let token = id_of(&first["session"], "session_token");

    let second = success_data(&run_db(
        &db,
        &[
            "session",
            "ensure",
            "--agent-principal-id",
            "ensure-agent",
            "--agent-instance-id",
            "ensure-instance-2",
            "--role",
            "orchestrator",
        ],
    ));
    assert_eq!(second["registered"], Value::Bool(false));
    assert_eq!(
        second["session"]["session_token"].as_str(),
        Some(token.as_str())
    );

    let other_role = success_data(&run_db(
        &db,
        &[
            "session",
            "ensure",
            "--agent-principal-id",
            "ensure-agent",
            "--role",
            "executor",
        ],
    ));
    assert_eq!(other_role["registered"], Value::Bool(true));
    assert_eq!(other_role["session"]["role"], "executor");
    assert_ne!(
        other_role["session"]["session_token"].as_str(),
        Some(token.as_str()),
        "the executor session stands beside the orchestrator session"
    );
    assert!(
        other_role["session"]["agent_instance_id"]
            .as_str()
            .is_some_and(|instance| !instance.is_empty()),
        "an omitted instance id is generated"
    );
}

#[test]
fn submit_ack_carries_the_state_and_an_ended_task_frees_its_dedupe_key() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let orch = register_orchestrator(&db, "dedupe-state-orch");
    let submit_keyed = || {
        success_data(&run_db(
            &db,
            &[
                "meta",
                "submit",
                "--session-token",
                &orch,
                "--prompt-text",
                "keyed intake",
                "--dedupe-key",
                "intake-31",
            ],
        ))
    };

    let first = submit_keyed();
    assert_eq!(first["state"], "planning");
    assert!(
        first.get("deduplicated").is_none(),
        "a fresh task omits deduplicated"
    );

    let hit = submit_keyed();
    assert_eq!(hit["meta_task_id"], first["meta_task_id"]);
    assert_eq!(hit["deduplicated"], Value::Bool(true));
    assert_eq!(hit["state"], "planning");

    success_data(&run_db(
        &db,
        &[
            "meta",
            "cancel",
            "--session-token",
            &orch,
            "--meta-task-id",
            &id_of(&first, "meta_task_id"),
        ],
    ));
    let after_cancel = submit_keyed();
    assert_ne!(after_cancel["meta_task_id"], first["meta_task_id"]);
    assert!(after_cancel.get("deduplicated").is_none());
    assert_eq!(list_ids(&db, &["--dedupe-key", "intake-31"]).len(), 2);
}

#[test]
fn submit_with_subtask_titles_acks_the_filed_subtasks() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let orch = register_orchestrator(&db, "subtask-title-orch");

    let plain = success_data(&run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "no subtasks",
        ],
    ));
    assert_eq!(plain["state"], "planning");
    assert_eq!(plain["subtasks"], Value::Array(Vec::new()));

    let filed = success_data(&run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "two steps",
            "--subtask-title",
            "first",
            "--subtask-title",
            "second",
        ],
    ));
    assert_eq!(filed["state"], "active");
    let subtasks = filed["subtasks"].as_array().expect("subtasks array");
    assert_eq!(
        subtasks
            .iter()
            .map(|subtask| subtask["title"].as_str())
            .collect::<Vec<_>>(),
        [Some("first"), Some("second")]
    );
    assert!(
        subtasks.iter().all(|subtask| subtask["subtask_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())),
        "every filed subtask is acked with its id"
    );
    let active = success_data(&run_db(&db, &["meta", "list", "--state", "active"]));
    assert_eq!(active[0]["meta_task_id"], filed["meta_task_id"]);
    assert_eq!(active[0]["subtask_total"], 2);
}

#[test]
fn list_compact_omits_prompt_text_and_changes_nothing_else() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let orch = register_orchestrator(&db, "compact-orch");
    submit(&db, &orch, "a long prompt nobody scans");

    let full = success_data(&run_db(&db, &["meta", "list"]));
    let compact = success_data(&run_db(&db, &["meta", "list", "--compact"]));

    assert_eq!(full[0]["prompt_text"], "a long prompt nobody scans");
    let mut expected = full.clone();
    expected[0]
        .as_object_mut()
        .expect("a row is a JSON object")
        .remove("prompt_text")
        .expect("the full row carries prompt_text");
    assert_eq!(compact, expected);
}

#[test]
fn update_sets_and_clears_fields_and_answers_the_list_row() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let orch = register_orchestrator(&db, "update-orch");
    success_data(&run_db(
        &db,
        &[
            "meta",
            "submit",
            "--session-token",
            &orch,
            "--prompt-text",
            "update me",
            "--title",
            "First title",
            "--urgency",
            "low",
            "--due",
            "2026-12-01",
        ],
    ));

    let updated = success_data(&run_db(
        &db,
        &[
            "meta",
            "update",
            "--session-token",
            &orch,
            "--handle",
            "1",
            "--title",
            "Second title",
            "--clear-urgency",
        ],
    ));

    assert_eq!(updated["title"], "Second title");
    assert!(updated["urgency"].is_null(), "the urgency is cleared");
    assert_eq!(updated["due_on"], "2026-12-01");
    let listed = success_data(&run_db(&db, &["meta", "list"]));
    assert_eq!(
        listed[0], updated,
        "the update answers the row a list page returns"
    );
}

#[test]
fn update_without_a_change_or_with_a_set_and_clear_pair_is_invalid_args() {
    let tmp = TempDir::new().expect("tempdir");
    let db = tmp.path().join("covey.db");
    let orch = register_orchestrator(&db, "update-usage-orch");
    submit(&db, &orch, "left alone");
    let before = success_data(&run_db(&db, &["meta", "list"]));

    let refused_changes: [&[&str]; 3] = [
        &[],
        &["--title", "Named", "--clear-title"],
        &["--due", "2026-12-01", "--clear-due"],
    ];
    for changes in refused_changes {
        let mut args = vec!["meta", "update", "--session-token", &orch, "--handle", "1"];
        args.extend_from_slice(changes);
        let output = run_db(&db, &args);
        assert_eq!(exit_code(&output), 2, "refused changes: {changes:?}");
        assert_eq!(error_code(&output), "invalid_args");
    }

    assert_eq!(success_data(&run_db(&db, &["meta", "list"])), before);
}

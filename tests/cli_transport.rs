use std::{
    path::Path,
    process::{Command, Output},
};

use serde::Deserialize;
use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_covey")
}

/// A command that starts from a clean environment for the variables the CLI
/// reads; each test states the ones it needs.
fn covey(dir: &TempDir) -> Command {
    let mut command = Command::new(bin());
    command
        .current_dir(dir.path())
        .env_remove("COVEY_DB_PATH")
        .env_remove("COVEY_LOG");
    command
}

#[derive(Deserialize)]
struct SuccessEnvelope<T> {
    ok: bool,
    data: T,
}

#[derive(Deserialize)]
struct OkEnvelope {
    ok: bool,
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    ok: bool,
    code: String,
    reason: String,
    #[serde(default)]
    suggestions: Vec<String>,
}

#[derive(Deserialize)]
struct SessionData {
    session_token: String,
}

#[derive(Deserialize)]
struct MetaTaskRow {
    meta_task_id: String,
}

#[derive(Deserialize)]
struct ClaimData {
    claim_id: String,
    fence_seq: i64,
}

#[derive(Deserialize)]
struct EventRow {
    event_type: String,
}

#[derive(Deserialize)]
struct PruneData {
    older_than_ms: i64,
    idempotency_rows: usize,
    event_rows: usize,
}

#[derive(Deserialize)]
struct VersionData {
    command: String,
    version: String,
    commit: String,
    dirty: bool,
}

fn success_data<T>(output: &Output) -> T
where
    T: for<'de> Deserialize<'de>,
{
    assert!(
        output.status.success(),
        "stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: SuccessEnvelope<T> =
        serde_json::from_slice(&output.stdout).expect("stdout json envelope");
    assert!(envelope.ok, "the success envelope must report ok");
    envelope.data
}

fn stderr_error(output: &Output) -> ErrorEnvelope {
    serde_json::from_slice(&output.stderr).expect("stderr json envelope")
}

const REGISTER_ORCHESTRATOR: [&str; 8] = [
    "session",
    "register",
    "--agent-principal-id",
    "principal",
    "--agent-instance-id",
    "instance-1",
    "--role",
    "orchestrator",
];

fn register_orchestrator(dir: &TempDir, db: &Path) -> String {
    let output = covey(dir)
        .arg("--db")
        .arg(db)
        .args(REGISTER_ORCHESTRATOR)
        .output()
        .expect("run covey");
    success_data::<SessionData>(&output).session_token
}

fn submit_meta_task(dir: &TempDir, db: &Path, session_token: &str, prompt: &str) -> String {
    let output = covey(dir)
        .arg("--db")
        .arg(db)
        .args([
            "meta",
            "submit",
            "--session-token",
            session_token,
            "--prompt-text",
            prompt,
        ])
        .output()
        .expect("run covey");
    success_data::<MetaTaskRow>(&output).meta_task_id
}

fn commit_is_shaped(commit: &str) -> bool {
    commit == "unknown" || (commit.len() == 12 && commit.chars().all(|c| c.is_ascii_hexdigit()))
}

fn meta_task_count(db: &Path) -> i64 {
    let conn = rusqlite::Connection::open(db).expect("open the store for counting");
    conn.query_row("SELECT COUNT(*) FROM meta_tasks", [], |row| row.get(0))
        .expect("count meta tasks")
}

#[test]
fn read_command_on_a_missing_store_reports_store_not_found() {
    let dir = TempDir::new().expect("tempdir");
    let db = dir.path().join("missing.db");
    let output = covey(&dir)
        .arg("--db")
        .arg(&db)
        .args(["meta", "list"])
        .output()
        .expect("run covey");

    assert_eq!(output.status.code(), Some(1));
    let error = stderr_error(&output);
    assert!(!error.ok);
    assert_eq!(error.code, "store_not_found");
    assert_eq!(error.reason, "store_not_found");
    assert!(!db.exists(), "the read command must not create the store");
}

#[test]
fn write_command_creates_a_missing_store() {
    let dir = TempDir::new().expect("tempdir");
    let db = dir.path().join("covey.db");
    let session_token = register_orchestrator(&dir, &db);

    assert!(!session_token.is_empty(), "the session token is reported");
    assert!(db.is_file(), "the write command must create the store");
}

#[test]
fn no_command_creates_a_store_at_the_default_path() {
    let dir = TempDir::new().expect("tempdir");
    let default_db = dir.path().join("covey.db");

    let output = covey(&dir)
        .args(REGISTER_ORCHESTRATOR)
        .output()
        .expect("run covey");

    assert_eq!(output.status.code(), Some(1));
    let error = stderr_error(&output);
    assert!(!error.ok);
    assert_eq!(error.code, "store_not_found");
    assert!(
        error
            .suggestions
            .iter()
            .any(|hint| hint.contains("--db") && hint.contains("COVEY_DB_PATH")),
        "the refusal must name both ways to choose a store: {:?}",
        error.suggestions
    );
    assert!(
        !default_db.exists(),
        "a write command without --db or COVEY_DB_PATH must not create ./covey.db"
    );

    // Once the default store exists, a command without --db uses it.
    let session_token = register_orchestrator(&dir, &default_db);
    let output = covey(&dir)
        .args([
            "meta",
            "submit",
            "--session-token",
            session_token.as_str(),
            "--prompt-text",
            "default store task",
        ])
        .output()
        .expect("run covey");
    success_data::<MetaTaskRow>(&output);
    assert_eq!(meta_task_count(&default_db), 1);
}

#[test]
fn refusal_reason_names_the_specific_error_under_a_coarse_code() {
    let dir = TempDir::new().expect("tempdir");
    let db = dir.path().join("covey.db");
    register_orchestrator(&dir, &db);

    let output = covey(&dir)
        .arg("--db")
        .arg(&db)
        .args(REGISTER_ORCHESTRATOR)
        .output()
        .expect("run covey");

    assert_eq!(output.status.code(), Some(4));
    let error = stderr_error(&output);
    assert!(!error.ok);
    assert_eq!(error.code, "conflict");
    assert_eq!(error.reason, "session_already_active");
}

#[test]
fn a_repeated_fenced_command_replays_instead_of_running_twice() {
    let dir = TempDir::new().expect("tempdir");
    let db = dir.path().join("covey.db");
    let orchestrator = register_orchestrator(&dir, &db);
    let meta_task_id = submit_meta_task(&dir, &db, &orchestrator, "replayed work");
    let run = |args: &[&str]| {
        covey(&dir)
            .arg("--db")
            .arg(&db)
            .args(args)
            .output()
            .expect("run covey")
    };
    success_data::<serde::de::IgnoredAny>(&run(&[
        "subtask",
        "create",
        "--session-token",
        &orchestrator,
        "--meta-task-id",
        &meta_task_id,
        "--title",
        "work",
    ]));
    let worker = success_data::<SessionData>(&run(&[
        "session",
        "register",
        "--agent-principal-id",
        "worker",
        "--agent-instance-id",
        "worker-1",
        "--role",
        "executor",
    ]))
    .session_token;
    let claim =
        success_data::<ClaimData>(&run(&["subtask", "claim-next", "--session-token", &worker]));
    let fence_seq = claim.fence_seq.to_string();
    let fenced = |verb: &'static str, extra: &[&str]| {
        let mut args = vec![
            "subtask",
            verb,
            "--session-token",
            worker.as_str(),
            "--claim-id",
            claim.claim_id.as_str(),
            "--fence-seq",
            fence_seq.as_str(),
        ];
        args.extend_from_slice(extra);
        run(&args)
    };

    // No command names an idempotency key, so each repeat reuses the key
    // derived from its claim and fence and replays the first result.
    for _ in 0..2 {
        success_data::<serde::de::IgnoredAny>(&fenced("start", &[]));
    }
    let first = fenced("finish", &["--summary", "done"]);
    let second = fenced("finish", &["--summary", "done"]);
    success_data::<serde::de::IgnoredAny>(&first);
    assert_eq!(
        second.stdout, first.stdout,
        "the repeat answers the first result"
    );

    let events = success_data::<Vec<EventRow>>(&run(&["events", "list", "--limit", "200"]));
    for event_type in ["subtask_started", "subtask_finished"] {
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == event_type)
                .count(),
            1,
            "{event_type} must be recorded once"
        );
    }
}

#[test]
fn claim_next_with_start_leaves_only_the_finish_to_do() {
    let dir = TempDir::new().expect("tempdir");
    let db = dir.path().join("covey.db");
    let orchestrator = register_orchestrator(&dir, &db);
    let meta_task_id = submit_meta_task(&dir, &db, &orchestrator, "started on claim");
    let run = |args: &[&str]| {
        covey(&dir)
            .arg("--db")
            .arg(&db)
            .args(args)
            .output()
            .expect("run covey")
    };
    success_data::<serde::de::IgnoredAny>(&run(&[
        "subtask",
        "create",
        "--session-token",
        &orchestrator,
        "--meta-task-id",
        &meta_task_id,
        "--title",
        "work",
    ]));
    let worker = success_data::<SessionData>(&run(&[
        "session",
        "register",
        "--agent-principal-id",
        "worker",
        "--agent-instance-id",
        "worker-1",
        "--role",
        "executor",
    ]))
    .session_token;

    let claim = success_data::<ClaimData>(&run(&[
        "subtask",
        "claim-next",
        "--session-token",
        &worker,
        "--start",
    ]));
    success_data::<serde::de::IgnoredAny>(&run(&[
        "subtask",
        "finish",
        "--session-token",
        &worker,
        "--claim-id",
        &claim.claim_id,
        "--fence-seq",
        &claim.fence_seq.to_string(),
        "--summary",
        "done",
    ]));

    let events = success_data::<Vec<EventRow>>(&run(&["events", "list", "--limit", "200"]));
    let subtask_events: Vec<&str> = events
        .iter()
        .map(|event| event.event_type.as_str())
        .filter(|event_type| event_type.starts_with("subtask_"))
        .collect();
    assert_eq!(
        subtask_events,
        ["subtask_claimed", "subtask_started", "subtask_finished"]
    );
}

#[test]
fn maint_prune_reports_both_counts_and_refuses_a_non_positive_age() {
    let dir = TempDir::new().expect("tempdir");
    let db = dir.path().join("covey.db");
    register_orchestrator(&dir, &db);
    let prune = |age: &str| {
        covey(&dir)
            .arg("--db")
            .arg(&db)
            .args(["maint", "prune", "--older-than-ms", age, "--events"])
            .output()
            .expect("run covey")
    };

    let refused = prune("0");
    assert_eq!(refused.status.code(), Some(2));
    assert_eq!(stderr_error(&refused).code, "invalid_args");

    // Nothing is a day old yet, so the pass deletes nothing and says so.
    let pruned = success_data::<PruneData>(&prune("86400000"));
    assert_eq!(pruned.older_than_ms, 86_400_000);
    assert_eq!(pruned.idempotency_rows, 0);
    assert_eq!(pruned.event_rows, 0);
}

#[test]
fn depending_on_retired_work_is_a_conflict_that_names_its_reason() {
    let dir = TempDir::new().expect("tempdir");
    let db = dir.path().join("covey.db");
    let orchestrator = register_orchestrator(&dir, &db);
    let meta_task_id = submit_meta_task(&dir, &db, &orchestrator, "retired work");
    let run = |args: &[&str]| {
        covey(&dir)
            .arg("--db")
            .arg(&db)
            .args(args)
            .output()
            .expect("run covey")
    };
    let create = |subtask_id: &str, depends_on: &[&str]| {
        let mut args = vec![
            "subtask",
            "create",
            "--session-token",
            orchestrator.as_str(),
            "--meta-task-id",
            meta_task_id.as_str(),
            "--title",
            "work",
            "--subtask-id",
            subtask_id,
        ];
        for dependency in depends_on {
            args.extend_from_slice(&["--depends-on", dependency]);
        }
        run(&args)
    };
    success_data::<serde::de::IgnoredAny>(&create("retired", &[]));
    success_data::<serde::de::IgnoredAny>(&create("kept", &[]));

    success_data::<serde::de::IgnoredAny>(&run(&[
        "subtask",
        "retire",
        "--session-token",
        &orchestrator,
        "--subtask-id",
        "retired",
    ]));

    let refused = create("dependent", &["retired"]);
    assert_eq!(refused.status.code(), Some(4));
    let error = stderr_error(&refused);
    assert_eq!(error.code, "conflict");
    assert_eq!(error.reason, "subtask_dependency_unsatisfiable");
}

#[test]
fn read_only_refuses_a_write_command_without_writing() {
    let dir = TempDir::new().expect("tempdir");
    let db = dir.path().join("covey.db");
    let session_token = register_orchestrator(&dir, &db);
    let before = meta_task_count(&db);

    let output = covey(&dir)
        .arg("--db")
        .arg(&db)
        .arg("--read-only")
        .args([
            "meta",
            "submit",
            "--session-token",
            session_token.as_str(),
            "--prompt-text",
            "refused write",
        ])
        .output()
        .expect("run covey");

    assert_eq!(output.status.code(), Some(3));
    let error = stderr_error(&output);
    assert!(!error.ok);
    assert_eq!(error.code, "store_read_only");
    assert_eq!(
        meta_task_count(&db),
        before,
        "a refused write must not change the row count"
    );
}

#[test]
fn read_only_allows_a_read_command() {
    let dir = TempDir::new().expect("tempdir");
    let db = dir.path().join("covey.db");
    let session_token = register_orchestrator(&dir, &db);
    submit_meta_task(&dir, &db, &session_token, "readable task");

    let output = covey(&dir)
        .arg("--db")
        .arg(&db)
        .arg("--read-only")
        .args(["meta", "list"])
        .output()
        .expect("run covey");
    let rows = success_data::<Vec<MetaTaskRow>>(&output);

    assert_eq!(rows.len(), 1);
    assert!(!rows[0].meta_task_id.is_empty());
}

#[test]
fn db_path_environment_variable_is_used_and_the_flag_wins() {
    let dir = TempDir::new().expect("tempdir");
    let env_db = dir.path().join("env.db");
    let flag_db = dir.path().join("flag.db");

    let env_token = success_data::<SessionData>(
        &covey(&dir)
            .env("COVEY_DB_PATH", &env_db)
            .args(REGISTER_ORCHESTRATOR)
            .output()
            .expect("run covey"),
    )
    .session_token;
    assert!(
        env_db.is_file(),
        "COVEY_DB_PATH must supply the store path when --db is absent"
    );
    submit_meta_task(&dir, &env_db, &env_token, "env task");

    let flag_token = success_data::<SessionData>(
        &covey(&dir)
            .env("COVEY_DB_PATH", &env_db)
            .arg("--db")
            .arg(&flag_db)
            .args(REGISTER_ORCHESTRATOR)
            .output()
            .expect("run covey"),
    )
    .session_token;
    assert!(
        flag_db.is_file(),
        "the --db flag must win over COVEY_DB_PATH"
    );
    submit_meta_task(&dir, &flag_db, &flag_token, "flag task one");
    submit_meta_task(&dir, &flag_db, &flag_token, "flag task two");

    let from_environment = success_data::<Vec<MetaTaskRow>>(
        &covey(&dir)
            .env("COVEY_DB_PATH", &env_db)
            .args(["meta", "list"])
            .output()
            .expect("run covey"),
    );
    assert_eq!(
        from_environment.len(),
        1,
        "COVEY_DB_PATH must select the environment store"
    );

    let from_flag = success_data::<Vec<MetaTaskRow>>(
        &covey(&dir)
            .env("COVEY_DB_PATH", &env_db)
            .arg("--db")
            .arg(&flag_db)
            .args(["meta", "list"])
            .output()
            .expect("run covey"),
    );
    assert_eq!(
        from_flag.len(),
        2,
        "the --db flag must win over COVEY_DB_PATH"
    );
}

#[test]
fn version_json_reports_the_commit_and_dirty_flag() {
    let dir = TempDir::new().expect("tempdir");
    let output = covey(&dir)
        .args(["--json", "--version"])
        .output()
        .expect("run covey");
    let data = success_data::<VersionData>(&output);

    assert_eq!(data.command, "covey");
    assert_eq!(data.version, env!("CARGO_PKG_VERSION"));
    assert!(
        commit_is_shaped(&data.commit),
        "unexpected commit: {}",
        data.commit
    );
    assert!(
        data.commit != "unknown" || !data.dirty,
        "a build without git must report a clean tree"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn version_in_human_mode_prints_the_package_version_and_commit() {
    let dir = TempDir::new().expect("tempdir");
    let mut script = Command::new("script");
    let command_line = format!("{} --version", shell_quote(bin()));
    script
        .args(["-qec", command_line.as_str(), "/dev/null"])
        .current_dir(dir.path())
        .env_remove("COVEY_DB_PATH")
        .env_remove("COVEY_LOG");
    let output = script.output().expect("run covey under a pty");

    assert!(
        output.status.success(),
        "the version command must exit successfully"
    );
    let stdout = String::from_utf8_lossy(&output.stdout).replace('\r', "");
    let line = stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .expect("a version line on the pty");
    let prefix = format!("covey {} (", env!("CARGO_PKG_VERSION"));
    let commit = line
        .strip_prefix(&prefix)
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or_else(|| panic!("unexpected version line: {line}"));
    assert!(commit_is_shaped(commit), "unexpected commit: {commit}");
}

#[cfg(target_os = "linux")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[test]
fn log_level_writes_operation_lines_to_stderr_and_keeps_stdout_json() {
    let dir = TempDir::new().expect("tempdir");
    let db = dir.path().join("covey.db");
    let session_token = register_orchestrator(&dir, &db);

    let output = covey(&dir)
        .arg("--db")
        .arg(&db)
        .env("COVEY_LOG", "info")
        .args([
            "session",
            "status",
            "--session-token",
            session_token.as_str(),
        ])
        .output()
        .expect("run covey");

    assert!(output.status.success());
    let envelope: OkEnvelope =
        serde_json::from_slice(&output.stdout).expect("stdout json envelope");
    assert!(envelope.ok, "stdout must stay a success envelope");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("operation="),
        "stderr must carry the operation log line: {stderr}"
    );
}

#[test]
fn invalid_log_level_is_invalid_args() {
    let dir = TempDir::new().expect("tempdir");
    let output = covey(&dir)
        .env("COVEY_LOG", "bogus")
        .args(["meta", "list"])
        .output()
        .expect("run covey");

    assert_eq!(output.status.code(), Some(2));
    let error = stderr_error(&output);
    assert!(!error.ok);
    assert_eq!(error.code, "invalid_args");
    assert_eq!(error.reason, "invalid_args");
    assert!(
        !dir.path().join("covey.db").exists(),
        "the rejected run must not create a store"
    );
}

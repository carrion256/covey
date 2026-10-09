use std::{path::PathBuf, process::Command};

use covey::{
    ClaimResult, ClaimSubtaskReq, CompletionPolicy, Covey, CoveyError, CreateSubtaskRequest,
    CreateWorkSubtaskReq, EvidencePolicy, ManualClock, RegisterSessionReq, RetrySubtaskReq,
    RoutingKey, SessionRole, StartSubtaskReq, SubmitMetaTaskReq, SubtaskState, WorkflowProfile,
};
use proptest::prelude::*;
use rstest::rstest;
use rusqlite::{Connection, OpenFlags};
use tempfile::TempDir;

const NOW_MS: i64 = 1_700_000_000_000;

struct Rig {
    _dir: TempDir,
    db_path: PathBuf,
    covey: Covey,
}

impl Rig {
    fn open(profile: WorkflowProfile) -> Rig {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("covey.db");
        let covey = Covey::open_with_clock_and_workflow(
            &db_path,
            std::sync::Arc::new(ManualClock::new(NOW_MS)),
            profile,
        )
        .expect("open Covey");
        Rig {
            _dir: dir,
            db_path,
            covey,
        }
    }
}

#[rstest::fixture]
fn default_rig() -> Rig {
    Rig::open(WorkflowProfile::default())
}

#[rstest::fixture]
fn required_rig() -> Rig {
    Rig::open(WorkflowProfile {
        evidence_policy: EvidencePolicy::Required,
        ..WorkflowProfile::default()
    })
}

fn register(covey: &Covey, principal: &str, role: SessionRole) -> String {
    covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                principal,
                format!("{principal}-instance"),
                role,
                format!("register-{principal}"),
            )
            .expect("valid session registration"),
        )
        .expect("register session")
        .session_token
        .to_string()
}

fn outcome_count(rig: &Rig) -> i64 {
    let connection =
        Connection::open_with_flags(&rig.db_path, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open");
    connection
        .query_row("SELECT COUNT(*) FROM subtask_attempt_outcomes", [], |row| {
            row.get(0)
        })
        .expect("count outcomes")
}

/// Orchestrator/executor/meta session and one claimed in-progress subtask
/// created through `Covey::create_subtask`, so the rig profile's routing lane
/// and evidence policy are the ones actually exercised.
fn seed_claimed_work(rig: &Rig) -> (String, ClaimResult, String) {
    let orchestrator = register(&rig.covey, "orchestrator", SessionRole::Orchestrator);
    let worker = register(&rig.covey, "worker", SessionRole::Executor);
    let meta_task_id = rig
        .covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(
                orchestrator.clone(),
                "profile test",
                "submit-meta",
            )
            .expect("valid meta-task request"),
        )
        .expect("submit meta task");
    let subtask_id = rig
        .covey
        .create_subtask(
            CreateSubtaskRequest::try_from_raw_parts(
                orchestrator,
                meta_task_id,
                None,
                "profile work",
                100,
                "create-work",
            )
            .expect("valid work request"),
        )
        .expect("create work");
    let claim = rig
        .covey
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                worker.clone(),
                subtask_id.clone(),
                30_000,
                "claim-work",
            )
            .expect("valid claim request"),
        )
        .expect("claim work");
    rig.covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "start-work",
            )
            .expect("valid start request"),
        )
        .expect("start work");
    (worker, claim, subtask_id)
}

#[rstest]
#[case(covey::AttemptOutcomeKind::Succeeded, SubtaskState::Completed)]
#[case(covey::AttemptOutcomeKind::RetryableFailure, SubtaskState::Available)]
#[case(covey::AttemptOutcomeKind::TerminalFailure, SubtaskState::Failed)]
fn default_profile_records_outcomes_without_evidence(
    #[from(default_rig)] rig: Rig,
    #[case] kind: covey::AttemptOutcomeKind,
    #[case] state: SubtaskState,
) {
    let (worker, claim, subtask_id) = seed_claimed_work(&rig);
    let outcome = match kind {
        covey::AttemptOutcomeKind::Succeeded => rig.covey.finish_subtask(
            covey::FinishSubtaskReq::try_from_raw_parts(
                worker,
                claim.claim_id,
                claim.fence_seq,
                None,
                "finished",
                "outcome",
            )
            .expect("finish request"),
        ),
        covey::AttemptOutcomeKind::RetryableFailure => rig.covey.retry_subtask(
            RetrySubtaskReq::try_from_raw_parts(
                worker,
                claim.claim_id,
                claim.fence_seq,
                None,
                "transient",
                "retry",
                "outcome",
            )
            .expect("retry request"),
        ),
        covey::AttemptOutcomeKind::TerminalFailure => rig.covey.fail_subtask(
            covey::FailSubtaskReq::try_from_raw_parts(
                worker,
                claim.claim_id,
                claim.fence_seq,
                None,
                "terminal",
                "failed",
                "outcome",
            )
            .expect("failure request"),
        ),
    }
    .expect("default profile accepts outcome without evidence");
    assert!(outcome.evidence_digest.is_none());
    assert_eq!(outcome.outcome_kind(), kind);
    assert_eq!(outcome_count(&rig), 1);
    let status = rig.covey.subtask_status(&subtask_id).expect("status");
    assert_eq!(status.subtask().evidence_policy(), EvidencePolicy::Optional);
    assert_eq!(status.subtask().state(), state);
    assert!(status.claim().is_none());
}

#[rstest]
fn required_profile_rejects_evidence_free_finish_without_writing(#[from(required_rig)] rig: Rig) {
    let (worker, claim, subtask_id) = seed_claimed_work(&rig);
    let result = rig.covey.finish_subtask(
        covey::FinishSubtaskReq::try_from_raw_parts(
            worker,
            claim.claim_id,
            claim.fence_seq,
            None,
            "missing evidence",
            "finish-gov",
        )
        .expect("valid finish request"),
    );
    assert!(
        matches!(result, Err(CoveyError::AttemptEvidenceRequired { .. })),
        "required profile must reject evidence-free finish"
    );
    assert_eq!(outcome_count(&rig), 0, "rejected finish writes nothing");
    assert_eq!(
        rig.covey
            .subtask_status(&subtask_id)
            .expect("status")
            .subtask()
            .state(),
        SubtaskState::InProgress
    );
}

#[rstest]
fn required_profile_accepts_digest_and_policy_survives_default_reopen(
    #[from(required_rig)] rig: Rig,
) {
    let (worker, claim, subtask_id) = seed_claimed_work(&rig);
    let outcome = rig
        .covey
        .finish_subtask(
            covey::FinishSubtaskReq::try_from_raw_parts(
                worker,
                claim.claim_id,
                claim.fence_seq,
                Some("blake3:governed".to_owned()),
                "governed finish",
                "finish-gov",
            )
            .expect("valid finish request"),
        )
        .expect("required profile accepts supplied evidence");
    assert!(outcome.evidence_digest.is_some());

    // Reopen the same database under the *default* profile: the per-task
    // persisted policy keeps enforcing required evidence.
    let reopened = Covey::open(&rig.db_path).expect("reopen with default profile");
    let status = reopened
        .subtask_status(&subtask_id)
        .expect("status after reopen");
    assert_eq!(status.subtask().state(), SubtaskState::Completed);
    assert_eq!(
        status.subtask().evidence_policy(),
        EvidencePolicy::Required,
        "evidence policy is immutable per task, independent of the reopening profile"
    );
}

#[rstest]
fn default_profile_task_lands_on_default_lane(#[from(default_rig)] rig: Rig) {
    let (_, _, subtask_id) = seed_claimed_work(&rig);
    let connection = Connection::open(&rig.db_path).expect("open");
    let route: String = connection
        .query_row(
            "SELECT routing_key FROM subtasks WHERE subtask_id = ?1",
            rusqlite::params![subtask_id],
            |row| row.get(0),
        )
        .expect("read route");
    assert_eq!(
        route, "default",
        "no profile config selects the default lane"
    );
}

#[rstest]
fn configured_profile_routes_and_governs_new_tasks() {
    let rig = Rig::open(WorkflowProfile {
        routing_key: RoutingKey::parse("nightly").expect("valid route"),
        evidence_policy: EvidencePolicy::Required,
        ..WorkflowProfile::default()
    });
    let (worker, claim, subtask_id) = seed_claimed_work(&rig);

    let route: String = {
        let connection = Connection::open(&rig.db_path).expect("open");
        connection
            .query_row(
                "SELECT routing_key FROM subtasks WHERE subtask_id = ?1",
                rusqlite::params![subtask_id],
                |row| row.get(0),
            )
            .expect("read route")
    };
    assert_eq!(route, "nightly", "profile routing lane applies");

    // The profile's evidence policy is enforced on receipts.
    let missing = rig.covey.finish_subtask(
        covey::FinishSubtaskReq::try_from_raw_parts(
            worker.clone(),
            claim.claim_id.clone(),
            claim.fence_seq,
            None,
            "no evidence",
            "finish-missing",
        )
        .expect("valid finish request"),
    );
    assert!(matches!(
        missing,
        Err(CoveyError::AttemptEvidenceRequired { .. })
    ));
    let ok = rig
        .covey
        .finish_subtask(
            covey::FinishSubtaskReq::try_from_raw_parts(
                worker,
                claim.claim_id,
                claim.fence_seq,
                Some("blake3:nightly".to_owned()),
                "evidence present",
                "finish-ok",
            )
            .expect("valid finish request"),
        )
        .expect("supplied evidence accepted");
    assert!(ok.evidence_digest.is_some());
    assert_eq!(outcome_count(&rig), 1);
}

#[rstest]
#[case::reviewed(CompletionPolicy::Reviewed)]
#[case::canonical_apply(CompletionPolicy::CanonicalApply)]
fn governed_completion_policy_rejects_direct_finish(#[case] policy: CompletionPolicy) {
    let rig = Rig::open(WorkflowProfile {
        completion_policy: policy,
        ..WorkflowProfile::default()
    });
    let orchestrator = register(&rig.covey, "orchestrator", SessionRole::Orchestrator);
    let worker = register(&rig.covey, "worker", SessionRole::Executor);
    let meta_task_id = rig
        .covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(orchestrator.clone(), "governed", "submit-meta")
                .expect("valid meta-task request"),
        )
        .expect("submit meta task");
    let subtask_id = rig
        .covey
        .create_work_subtask(
            CreateWorkSubtaskReq::try_from_raw_parts(
                orchestrator,
                meta_task_id,
                None,
                "governed work",
                100,
                policy,
                rig.covey.workflow().routing_key.as_str().to_owned(),
                "create-governed",
            )
            .expect("valid governed work request"),
        )
        .expect("create governed work");
    let claim = rig
        .covey
        .claim_subtask(
            ClaimSubtaskReq::try_from_raw_parts(
                worker.clone(),
                subtask_id,
                30_000,
                "claim-governed",
            )
            .expect("valid claim request"),
        )
        .expect("claim governed work");
    rig.covey
        .start_subtask(
            StartSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                "start-governed",
            )
            .expect("valid start request"),
        )
        .expect("start governed work");

    for evidence in [None, Some("blake3:governed")] {
        let result = rig.covey.finish_subtask(
            covey::FinishSubtaskReq::try_from_raw_parts(
                worker.clone(),
                claim.claim_id.clone(),
                claim.fence_seq,
                evidence.map(str::to_owned),
                "governed finish",
                "finish-governed",
            )
            .expect("valid finish request"),
        );
        assert!(
            matches!(result, Err(CoveyError::CompletionPolicyViolation { .. })),
            "policy {policy:?} must reject direct finish (evidence: {evidence:?})"
        );
    }
    assert_eq!(
        outcome_count(&rig),
        0,
        "policy rejection must not persist an outcome"
    );
}

// -- proptest: evidence enforcement is policy-driven, validation is not

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]

    #[test]
    fn finish_is_accepted_exactly_when_policy_allows_missing_evidence(
        required in any::<bool>(),
        evidence_present in any::<bool>(),
    ) {
        let profile = if required {
            WorkflowProfile { evidence_policy: EvidencePolicy::Required, ..WorkflowProfile::default() }
        } else {
            WorkflowProfile::default()
        };
        let rig = Rig::open(profile);
        let (worker, claim, _) = seed_claimed_work(&rig);
        let result = rig.covey.finish_subtask(
            covey::FinishSubtaskReq::try_from_raw_parts(
                worker,
                claim.claim_id,
                claim.fence_seq,
                if evidence_present { Some("blake3:prop".to_owned()) } else { None },
                "property finish",
                "finish-prop",
            )
            .expect("valid finish request"),
        );
        if !required || evidence_present {
            prop_assert!(result.is_ok());
            prop_assert_eq!(outcome_count(&rig), 1);
        } else {
            prop_assert!(result.is_err());
            prop_assert_eq!(outcome_count(&rig), 0);
        }
    }

    #[test]
    fn supplied_digest_with_whitespace_is_rejected(junk in "[a-z0-9]{1,32}") {
        let result = covey::FinishSubtaskReq::try_from_raw_parts(
            "session-worker",
            "claim-work",
            1,
            Some(format!("blake3:bad {junk}")),
            "finished",
            "finish-invalid",
        );
        prop_assert!(result.is_err());
    }
}

// -- CLI: config validation happens before the database is created -----------

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_covey")
}

/// Relative `--config` values resolve against `dir`; all other args pass
/// through unchanged and the child cwd stays at the workspace.
fn run_cli(dir: &tempfile::TempDir, extra: &[&str]) -> std::process::Output {
    let mut next_is_config_value = false;
    let args: Vec<std::ffi::OsString> = extra
        .iter()
        .map(|arg| {
            let resolved = if next_is_config_value {
                dir.path().join(arg).into_os_string()
            } else {
                std::ffi::OsString::from(arg)
            };
            next_is_config_value = *arg == "--config";
            resolved
        })
        .collect();
    Command::new(bin())
        .arg("--db")
        .arg(dir.path().join("covey.db"))
        .arg("--json")
        .args(args)
        .output()
        .expect("run covey")
}

fn data_field(output: &std::process::Output, field: &str) -> String {
    assert!(
        output.status.success(),
        "cli call failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: serde_json::Value = serde_json::from_slice(&output.stdout).expect("stdout json");
    match &payload["data"][field] {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Number(value) => value.to_string(),
        _ => panic!("missing scalar field {field}"),
    }
}

#[rstest]
#[case::profile_without_config(&["--profile", "gov"])]
#[case::unreadable_config(&["--config", "no-such-file.json"])]
#[case::malformed_json(&["--config", "bad.json"])]
#[case::unknown_field(&["--config", "fields.json"])]
#[case::dangling_default(&["--config", "default.json"])]
#[case::unknown_profile(&["--config", "wf.json", "--profile", "nope"])]
fn cli_bad_config_inputs_fail_before_database_creation(#[case] args: &[&str]) {
    let dir = TempDir::new().expect("temp dir");
    std::fs::write(dir.path().join("bad.json"), "{ not json").expect("write config");
    std::fs::write(
        dir.path().join("fields.json"),
        r#"{"extra":true,"profiles":{}}"#,
    )
    .expect("write config");
    std::fs::write(
        dir.path().join("default.json"),
        r#"{"default_profile":"missing","profiles":{}}"#,
    )
    .expect("write config");
    std::fs::write(
        dir.path().join("wf.json"),
        r#"{"profiles":{"gov":{"evidence_policy":"required"}}}"#,
    )
    .expect("write config");

    let output = run_cli(
        &dir,
        &[args, &["subtask", "status", "--subtask-id", "x"]].concat(),
    );
    assert!(
        !output.status.success(),
        "invalid config input must be rejected before any database access"
    );
    assert!(
        !dir.path().join("covey.db").exists(),
        "database file must not exist after rejection"
    );
}

#[test]
fn cli_required_profile_enforces_evidence_and_persists_policy() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::write(
        dir.path().join("gov.json"),
        r#"{"profiles":{"gov":{"evidence_policy":"required"}}}"#,
    )
    .expect("write config");
    let gov = ["--config", "gov.json", "--profile", "gov"];

    let orch = run_cli(
        &dir,
        &[
            &gov[..],
            &[
                "session",
                "register",
                "--agent-principal-id",
                "orch",
                "--agent-instance-id",
                "orch-1",
                "--role",
                "orchestrator",
            ],
        ]
        .concat(),
    );
    let meta = run_cli(
        &dir,
        &[
            &gov[..],
            &[
                "meta",
                "submit",
                "--session-token",
                &data_field(&orch, "session_token"),
                "--prompt-text",
                "gov quest",
            ],
        ]
        .concat(),
    );
    let exec = run_cli(
        &dir,
        &[
            &gov[..],
            &[
                "session",
                "register",
                "--agent-principal-id",
                "exec",
                "--agent-instance-id",
                "exec-1",
                "--role",
                "executor",
            ],
        ]
        .concat(),
    );
    run_cli(
        &dir,
        &[
            &gov[..],
            &[
                "subtask",
                "create",
                "--session-token",
                &data_field(&orch, "session_token"),
                "--meta-task-id",
                &data_field(&meta, "meta_task_id"),
                "--title",
                "gov cli work",
                "--subtask-id",
                "gov_cli_work",
            ],
        ]
        .concat(),
    );
    let claimed = run_cli(
        &dir,
        &[
            &gov[..],
            &[
                "subtask",
                "claim-next",
                "--session-token",
                &data_field(&exec, "session_token"),
                "--lease-duration-ms",
                "30000",
            ],
        ]
        .concat(),
    );
    assert!(claimed.status.success(), "claim-next failed");
    let claim_id = data_field(&claimed, "claim_id");
    let fence_seq = data_field(&claimed, "fence_seq");
    run_cli(
        &dir,
        &[
            &gov[..],
            &[
                "subtask",
                "start",
                "--session-token",
                &data_field(&exec, "session_token"),
                "--claim-id",
                &claim_id,
                "--fence-seq",
                &fence_seq,
            ],
        ]
        .concat(),
    );

    let no_evidence = run_cli(
        &dir,
        &[
            &gov[..],
            &[
                "subtask",
                "finish",
                "--session-token",
                &data_field(&exec, "session_token"),
                "--claim-id",
                &claim_id,
                "--fence-seq",
                &fence_seq,
                "--summary",
                "evidence-free finish",
            ],
        ]
        .concat(),
    );
    assert!(
        !no_evidence.status.success(),
        "required profile must reject evidence-free finish"
    );
    let code: serde_json::Value = serde_json::from_slice(&no_evidence.stderr).expect("stderr json");
    assert_eq!(
        code["code"],
        serde_json::Value::String("invalid_args".into())
    );

    let with_evidence = run_cli(
        &dir,
        &[
            &gov[..],
            &[
                "subtask",
                "finish",
                "--session-token",
                &data_field(&exec, "session_token"),
                "--claim-id",
                &claim_id,
                "--fence-seq",
                &fence_seq,
                "--evidence-digest",
                "blake3:gov",
                "--summary",
                "governed finish",
            ],
        ]
        .concat(),
    );
    assert!(
        with_evidence.status.success(),
        "required profile must accept supplied evidence"
    );

    let connection = Connection::open(dir.path().join("covey.db")).expect("open db");
    let policy: String = connection
        .query_row(
            "SELECT evidence_policy FROM subtasks WHERE subtask_id = 'gov_cli_work'",
            [],
            |row| row.get(0),
        )
        .expect("read evidence policy");
    assert_eq!(policy, "required", "profile policy persists on the task");
}

#[test]
fn cli_create_replays_same_resolved_request_and_conflicts_on_policy_change() {
    let dir = TempDir::new().expect("temp dir");
    let orch = run_cli(
        &dir,
        &[
            "session",
            "register",
            "--agent-principal-id",
            "orch",
            "--agent-instance-id",
            "orch-1",
            "--role",
            "orchestrator",
        ],
    );
    let meta = run_cli(
        &dir,
        &[
            "meta",
            "submit",
            "--session-token",
            &data_field(&orch, "session_token"),
            "--prompt-text",
            "idem quest",
        ],
    );
    let base = [
        "subtask",
        "create",
        "--session-token",
        &data_field(&orch, "session_token"),
        "--meta-task-id",
        &data_field(&meta, "meta_task_id"),
        "--title",
        "idem work",
        "--subtask-id",
        "idem_work",
        "--idempotency-key",
        "idem-key",
    ];
    let first = run_cli(&dir, &base);
    assert!(first.status.success());

    // Explicit flags resolving to the same defaults replay the same task.
    let replay = run_cli(
        &dir,
        &[
            &base[..],
            &[
                "--completion-policy",
                "direct",
                "--evidence-policy",
                "optional",
                "--routing-key",
                "default",
            ],
        ]
        .concat(),
    );
    assert!(
        replay.status.success(),
        "resolved-identical flags must replay, never duplicate"
    );

    // Same key with a changed resolved policy conflicts and changes nothing.
    let conflict = run_cli(
        &dir,
        &[&base[..], &["--evidence-policy", "required"]].concat(),
    );
    assert!(
        !conflict.status.success(),
        "changed resolved policy on the same key must conflict"
    );
    let connection = Connection::open(dir.path().join("covey.db")).expect("open db");
    let (count, policy): (i64, String) = connection
        .query_row(
            "SELECT COUNT(*), (SELECT evidence_policy FROM subtasks WHERE subtask_id = 'idem_work')
             FROM subtasks WHERE subtask_id = 'idem_work'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read stored task");
    assert_eq!(count, 1, "no duplicate across flag shapes");
    assert_eq!(policy, "optional", "conflicting replay altered nothing");
}

#[rstest]
fn created_event_decodes_with_resolved_policy(#[from(required_rig)] rig: Rig) {
    let (_, _, subtask_id) = seed_claimed_work(&rig);
    let events = rig.covey.fetch_events(0, 100).expect("fetch events");
    let event = events
        .iter()
        .find(|event| event.event_type() == covey::EventType::WorkSubtaskCreated)
        .expect("creation event");
    assert_eq!(event.object_id(), subtask_id);
    let covey::EventPayload::WorkSubtaskCreated(request) =
        event.typed().expect("decode consumer event").payload
    else {
        panic!("expected work creation payload");
    };
    assert_eq!(request.evidence_policy, EvidencePolicy::Required);
}

#[rstest]
fn claim_replay_cannot_silently_change_profile_routes(#[from(default_rig)] rig: Rig) {
    let worker = register(&rig.covey, "worker", SessionRole::Executor);
    let request =
        covey::ClaimNextReq::try_from_raw_parts(worker, 30_000, "claim").expect("claim request");
    assert!(
        rig.covey
            .claim_next_subtask(request.clone())
            .expect("empty lane")
            .is_none()
    );
    assert!(
        rig.covey
            .claim_next_subtask(request.clone())
            .expect("same lane replay")
            .is_none()
    );
    let other_lane = Covey::open_with_clock_and_workflow(
        &rig.db_path,
        std::sync::Arc::new(ManualClock::new(NOW_MS)),
        WorkflowProfile {
            routing_key: RoutingKey::parse("other").expect("route"),
            ..WorkflowProfile::default()
        },
    )
    .expect("reopen with another route");
    assert!(matches!(
        other_lane.claim_next_subtask(request),
        Err(CoveyError::IdempotencyConflict { .. })
    ));
}

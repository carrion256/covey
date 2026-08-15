use crate::lint::{is_empty_or_none, split_path_list, task_field};
use crate::model::{
    ARTIFACT_NAMES, CANONICAL_PROTECTED_FORBIDDEN_PATHS, CompiledTask, SourceSnapshot,
};
use crate::report::{LintState, source_digest_map, task_counts};
use crate::source::canonical_json_digest;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn build_artifacts(
    source: &SourceSnapshot,
    lint: &LintState,
) -> BTreeMap<&'static str, Value> {
    let tasks = compiled_tasks(source, lint);
    let source_digests = source_digest_map(source);
    let source_digest = canonical_json_digest(
        &serde_json::to_value(&source_digests).expect("source digest map is serializable"),
    )
    .expect("source digest map canonicalization succeeds");
    let objective = mission_objective(source, &tasks);
    let done_definition = mission_done_definition(source, &tasks);
    let scope_in = mission_scope_in(source, &tasks);
    let scope_out = mission_scope_out(source, &tasks);
    let affected_capabilities = mission_affected_capabilities(source);
    let risk_level = mission_risk_level(source, &tasks);
    let operator_approval = operator_approval_from_source(source, &tasks);
    let validation_checks = validation_checks_from_tasks(source, &tasks);
    let path_policy = path_policy_from_tasks(source, &tasks);

    let mission = json!({
        "schema": "better-droid.mission.v1",
        "change_id": source.change_id,
        "planning_class": source.planning_class,
        "change_path": source.relative_change_path,
        "source_digests": source_digests,
        "artifact_digests": {},
        "objective": objective,
        "done_definition": done_definition,
        "scope_in": scope_in,
        "scope_out": scope_out,
        "affected_capabilities": affected_capabilities,
        "risk_level": risk_level,
        "operator_approval": operator_approval,
        "tasks": tasks,
    });

    let trace_rows = source
        .scenarios
        .iter()
        .map(|scenario| {
            let mapped_task_ids = tasks
                .iter()
                .filter(|task| task.scenario_ids.contains(&scenario.id))
                .map(|task| task.task_id.clone())
                .collect::<Vec<_>>();
            let expected_paths = mapped_task_ids
                .iter()
                .filter_map(|task_id| tasks.iter().find(|task| &task.task_id == task_id))
                .flat_map(|task| task.allowed_write_paths.iter().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let validation_refs = validation_checks
                .iter()
                .filter(|check| {
                    check
                        .get("scenario_ids")
                        .and_then(Value::as_array)
                        .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(scenario.id.as_str())))
                })
                .filter_map(|check| check.get("id").and_then(Value::as_str).map(str::to_owned))
                .collect::<Vec<_>>();
            json!({
                "requirement_id": scenario.requirement_id,
                "requirement_title": source.requirements.iter().find(|req| req.id == scenario.requirement_id).map(|req| req.title.as_str()).unwrap_or("unknown"),
                "scenario_id": scenario.id,
                "scenario_title": scenario.title,
                "task_ids": mapped_task_ids,
                "expected_paths": expected_paths,
                "validation_refs": validation_refs,
                "artifact_digest": null,
                "review_verdict": null,
                "status": if mapped_task_ids.is_empty() { "planned" } else { "covered" },
                "deferral_reason": null
            })
        })
        .collect::<Vec<_>>();

    let review_items = review_rubric_items();

    let mut artifacts = BTreeMap::new();
    artifacts.insert("mission.json", mission);
    artifacts.insert(
        "mission-packet.json",
        build_mission_packet(
            source,
            &tasks,
            &validation_checks,
            &review_items,
            &source_digest,
            &path_policy,
            &objective,
        ),
    );
    artifacts.insert(
        "traceability.json",
        json!({
            "schema": "better-droid.traceability.v1",
            "change_id": source.change_id,
            "rows": trace_rows
        }),
    );
    artifacts.insert(
        "validation.json",
        json!({
            "schema": "better-droid.validation.v1",
            "change_id": source.change_id,
            "checks": validation_checks
        }),
    );
    artifacts.insert("path-policy.json", path_policy);
    artifacts.insert(
        "review-rubric.json",
        json!({
            "schema": "better-droid.review-rubric.v1",
            "change_id": source.change_id,
            "items": review_items
        }),
    );
    artifacts.insert(
        "assumptions.json",
        json!({
            "schema": "better-droid.assumptions.v1",
            "change_id": source.change_id,
            "assumptions": [],
            "blocked_task_refs": [],
            "approval_summary": {
                "high_or_critical_pending": 0
            }
        }),
    );
    artifacts.insert(
        "compile-report.json",
        json!({
            "schema": "better-droid.compile-report.v1",
            "change_id": source.change_id,
            "planning_class": source.planning_class,
            "packet_kind": "planning_only",
            "status": "planning_ready",
            "readiness": {
                "planning_ready": true,
                "planning_ready_blocked": false,
                "covey_import_ready": false,
                "covey_imported": false,
                "implementation_ready": false,
                "execution_ready": false,
                "review_approved": false,
                "apply_queued": false,
                "apply_authorized": false,
                "landed": false,
                "shipped_verified": false,
                "not_imported": true
            },
            "product_impact": {
                "product_files_changed": false,
                "product_tests_run": false,
                "covey_imported": false,
                "apply_receipt": false,
                "shipped_evidence": false,
                "product_write_paths": [],
                "validation_commands": []
            },
            "requires_implementation_packet": false,
            "import_ready": false,
            "created_artifacts": ARTIFACT_NAMES,
            "source_digests": source_digest_map(source),
            "artifact_digests": {},
            "task_counts": task_counts(&lint.classifications),
            "blockers": [],
            "warnings": lint.warnings,
            "task_classifications": lint.classifications,
            "non_canonical_run_metadata": {}
        }),
    );
    artifacts
}

pub(crate) fn build_mission_packet(
    source: &SourceSnapshot,
    tasks: &[CompiledTask],
    validation_checks: &[Value],
    review_items: &[Value],
    source_digest: &str,
    path_policy: &Value,
    objective: &str,
) -> Value {
    let change_id = source.change_id.as_str();
    let prompt = mission_packet_prompt(change_id, objective, tasks);
    let source_digest_ref = source_digest.to_owned();

    json!({
        "schema_version": "mission_packet.v1",
        "mission": {
            "id": change_id,
            "title": objective,
        },
        "provenance": {
            "compiler": "better-droid",
            "planning_class": source.planning_class,
            "source_revision": "unknown",
            "source_digest": source_digest,
        },
        "scheduler": mission_packet_scheduler(change_id, &source_digest_ref),
        "runtime": mission_packet_runtime(change_id),
        "provider": {
            "provider_id": "better-droid",
            "model_id": "compiled-mission",
            "auth_identity_id": null,
            "project_path": ".",
            "session_id": format!("session-{change_id}"),
            "cache_lane": null,
            "messages": [{
                "role": "User",
                "parts": [{
                    "Text": {
                        "text": prompt,
                    }
                }]
            }],
            "tools": [],
            "tool_choice": "None",
            "tool_results": [],
            "system": null,
            "max_tokens": null,
            "temperature": null,
            "billing_identity_id": null,
        },
        "path_policy": mission_packet_path_policy(path_policy),
        "repoops": null,
        "validation": validation_checks.iter().map(|check| {
            json!({
                "id": check.get("id").cloned().unwrap_or(Value::Null),
                "command": check.get("command").cloned().unwrap_or(Value::Null),
                "working_directory": check.get("working_directory").cloned().unwrap_or(Value::Null),
            })
        }).collect::<Vec<_>>(),
        "review_rubric": review_items.iter().map(|item| {
            json!({
                "id": item.get("id").cloned().unwrap_or(Value::Null),
                "criterion": item.get("question").cloned().unwrap_or(Value::Null),
            })
        }).collect::<Vec<_>>(),
        "assumptions": [],
    })
}

fn mission_packet_scheduler(change_id: &str, source_digest: &str) -> Value {
    json!({
        "request_id": change_id,
        "observed_at": "2026-05-21T09:00:00Z",
        "readiness_domain": "planning_ready_not_execution_or_apply",
        "identity_refs": {
            "queue_item_ref": format!("queue/ref:{change_id}"),
            "review_ref": format!("review/ref:{change_id}"),
            "artifact_ref": format!("artifact/ref:{change_id}"),
            "apply_gate_ref": null,
            "selected_attempt": {
                "subtask_ref": format!("covey:subtask:{change_id}"),
                "claim_ref": null,
                "fence_seq": null,
                "attempt_id": null,
                "snapshot_digest": format!("blake3:snapshot:{change_id}"),
                "covey_event_watermark": "covey-event-seq:1",
                "evaluator_version": "better-droid:mission-packet"
            }
        },
        "activation_result": {
            "PlanningReady": {
                "classification": "CompiledPlanningInputOnly",
                "lineage": {
                    "artifact_digest": source_digest,
                    "apply_attempt_id": null,
                    "landing_token": null,
                    "repo_identity": "repo-main",
                    "pending_commit_token": null
                }
            }
        }
    })
}

fn mission_packet_runtime(change_id: &str) -> Value {
    json!({
        "fleet_snapshot": {
            "observed_at": "2026-05-21T09:00:01Z",
            "agents": [
                {
                    "agent_id": format!("agent-{change_id}"),
                    "agent_type": "codex:smart",
                    "state": "Ready",
                    "in_flight": 0,
                    "max_in_flight": 1,
                    "capabilities": [
                        "better-droid",
                        "mission-runner"
                    ],
                    "retry_after_seconds": null,
                    "rate_limit": null
                }
            ],
            "global_rate_limit": null
        },
        "intent_available_at": "2026-05-21T09:00:02Z",
        "executor_worker_id": format!("worker-{change_id}"),
        "executor_lease_id": format!("lease-{change_id}"),
        "executor_now": "2026-05-21T09:00:10Z",
        "executor_lease_until": "2026-05-21T09:00:20Z",
        "executor_max_attempts": 3,
        "dispatch_observed_at": "2026-05-21T09:00:12Z",
        "retry_available_at": "2026-05-21T09:01:00Z",
        "provider_mode": "FakeDeliver"
    })
}

fn mission_packet_path_policy(path_policy: &Value) -> Value {
    let mut allowed_paths = BTreeSet::new();
    for field in ["allowed_read_paths", "allowed_write_paths", "allowed_paths"] {
        if let Some(paths) = path_policy.get(field).and_then(Value::as_array) {
            allowed_paths.extend(paths.iter().filter_map(Value::as_str).map(str::to_owned));
        }
    }
    json!({
        "mutation_allowed": path_policy
            .get("mutation_allowed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "allowed_paths": allowed_paths.into_iter().collect::<Vec<_>>()
    })
}

pub(crate) fn mission_packet_prompt(
    change_id: &str,
    objective: &str,
    tasks: &[CompiledTask],
) -> String {
    let mut prompt = format!("Execute Better Droid mission {change_id}: {objective}\nTasks:");
    for task in tasks {
        prompt.push_str(&format!(
            "\n- {} {}: {}",
            task.task_id, task.title, task.purpose
        ));
    }
    prompt
}

pub(crate) fn mission_objective(source: &SourceSnapshot, tasks: &[CompiledTask]) -> String {
    if let Some(objective) = source.proposal.text.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("- **Objective:**")
            .or_else(|| trimmed.strip_prefix("Objective:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    }) {
        return objective;
    }

    if let Some(title) = source.proposal.text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("# Proposal:")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("implement {value}"))
    }) {
        return title;
    }

    if let Some(sentence) = section_items(&source.proposal.text, "What Changes")
        .into_iter()
        .next()
        .or_else(|| first_section_sentence(&source.proposal.text, "What Changes"))
    {
        return sentence;
    }

    tasks
        .iter()
        .find_map(|task| {
            if task.purpose.is_empty() {
                None
            } else {
                Some(task.purpose.clone())
            }
        })
        .unwrap_or_else(|| {
            format!(
                "compile OpenSpec change {} into mission artifacts",
                source.change_id
            )
        })
}

pub(crate) fn mission_scope_in(source: &SourceSnapshot, tasks: &[CompiledTask]) -> Vec<String> {
    let mut items = section_items(&source.proposal.text, "Scope In");
    if items.is_empty() {
        items.extend(
            tasks
                .iter()
                .flat_map(|task| task.allowed_write_paths.iter().cloned()),
        );
    }
    dedupe_or_default(items, vec![format!("{}/**", source.relative_change_path)])
}

pub(crate) fn mission_scope_out(source: &SourceSnapshot, tasks: &[CompiledTask]) -> Vec<String> {
    let mut items = section_items(&source.proposal.text, "Scope Out");
    if items.is_empty() {
        items.extend(
            tasks
                .iter()
                .flat_map(|task| task.forbidden_write_paths.iter().cloned()),
        );
    }
    dedupe_or_default(
        items,
        CANONICAL_PROTECTED_FORBIDDEN_PATHS
            .iter()
            .map(|path| (*path).to_owned())
            .collect(),
    )
}

pub(crate) fn mission_done_definition(
    source: &SourceSnapshot,
    tasks: &[CompiledTask],
) -> Vec<String> {
    let mut items = section_items(&source.proposal.text, "Success Criteria");
    if items.is_empty() {
        items.extend(
            tasks
                .iter()
                .flat_map(|task| task.acceptance_criteria.iter().cloned()),
        );
    }
    dedupe_or_default(
        items,
        vec!["all task validation evidence passes".to_owned()],
    )
}

pub(crate) fn mission_affected_capabilities(source: &SourceSnapshot) -> Vec<String> {
    dedupe_or_default(
        section_items(&source.proposal.text, "Affected Capabilities"),
        vec![source.change_id.clone()],
    )
}

pub(crate) fn mission_risk_level(source: &SourceSnapshot, tasks: &[CompiledTask]) -> String {
    let text = format!("{}\n{}", source.proposal.text, source.tasks.text).to_ascii_lowercase();
    if text.contains("critical") {
        "critical".to_owned()
    } else if text.contains("live zellij")
        || text.contains("operator approval")
        || tasks
            .iter()
            .any(|task| !task.allowed_write_paths.is_empty())
    {
        "high".to_owned()
    } else {
        "medium".to_owned()
    }
}

pub(crate) fn operator_approval_from_source(
    source: &SourceSnapshot,
    tasks: &[CompiledTask],
) -> Value {
    let required = source
        .proposal
        .text
        .to_ascii_lowercase()
        .contains("operator approval")
        || tasks
            .iter()
            .any(|task| !task.allowed_write_paths.is_empty());
    json!({
        "required": required,
        "status": if required { "pending" } else { "not-required" },
        "reason": if required {
            "implementation may mutate source, tests, migrations, docs, or optionally exercise live local tooling"
        } else {
            "read-only mission projection"
        }
    })
}

pub(crate) fn validation_checks_from_tasks(
    source: &SourceSnapshot,
    tasks: &[CompiledTask],
) -> Vec<Value> {
    let mut checks = Vec::new();
    for task in &source.tasks_parsed {
        let Some(compiled) = tasks.iter().find(|candidate| candidate.task_id == task.id) else {
            continue;
        };
        let Some(command) = task_field(task, "Command / Action") else {
            continue;
        };
        if is_empty_or_none(command) {
            continue;
        }
        let refs = if compiled.validation_refs.is_empty() {
            vec![format!(
                "VAL-{}-{}",
                source.change_id.to_ascii_uppercase().replace('-', "_"),
                task.id.replace('.', "_")
            )]
        } else {
            compiled.validation_refs.clone()
        };
        for id in refs {
            checks.push(json!({
                "id": id,
                "phase": "worker",
                "task_ids": [task.id.clone()],
                "scenario_ids": compiled.scenario_ids,
                "command": command.trim(),
                "working_directory": task_field(task, "Working Directory").unwrap_or("."),
                "expected_exit_code": expected_exit_code(task_field(task, "Expected Exit Code / Observation")),
                "evidence_required": evidence_required(compiled),
            }));
        }
    }

    if checks.is_empty() {
        checks.push(json!({
            "id": "VAL-BDLCF-openspec",
            "phase": "worker",
            "task_ids": tasks.iter().map(|task| task.task_id.clone()).collect::<Vec<_>>(),
            "scenario_ids": source.scenarios.iter().map(|scenario| scenario.id.clone()).collect::<Vec<_>>(),
            "command": format!("openspec validate {} --type change --strict", source.change_id),
            "working_directory": ".",
            "expected_exit_code": 0,
            "evidence_required": ["command", "working_directory", "exit_code", "stdout_stderr", "source_revision"]
        }));
    }

    checks
}

pub(crate) fn expected_exit_code(value: Option<&str>) -> i32 {
    value
        .and_then(|value| {
            value
                .split(|ch: char| !ch.is_ascii_digit())
                .find(|part| !part.is_empty())
        })
        .and_then(|digits| digits.parse::<i32>().ok())
        .unwrap_or(0)
}

pub(crate) fn evidence_required(task: &CompiledTask) -> Vec<String> {
    let mut evidence = BTreeSet::from([
        "command".to_owned(),
        "working_directory".to_owned(),
        "exit_code".to_owned(),
        "stdout_stderr".to_owned(),
        "source_revision".to_owned(),
    ]);
    evidence.extend(task.expected_evidence.iter().cloned());
    evidence.into_iter().collect()
}

pub(crate) fn path_policy_from_tasks(source: &SourceSnapshot, tasks: &[CompiledTask]) -> Value {
    let mut reads = BTreeSet::from([
        format!("{}/.openspec.yaml", source.relative_change_path),
        format!("{}/proposal.md", source.relative_change_path),
        format!("{}/design.md", source.relative_change_path),
        format!("{}/tasks.md", source.relative_change_path),
        format!("{}/specs/**", source.relative_change_path),
        "openspec/config.yaml".to_owned(),
        "openspec/schemas/better-droid/**".to_owned(),
    ]);
    let mut writes = BTreeSet::new();
    let mut forbidden = CANONICAL_PROTECTED_FORBIDDEN_PATHS
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<BTreeSet<_>>();

    for task in tasks {
        reads.extend(task.allowed_read_paths.iter().cloned());
        writes.extend(task.allowed_write_paths.iter().cloned());
        forbidden.extend(task.forbidden_write_paths.iter().cloned());
    }

    let task_overrides = tasks
        .iter()
        .map(|task| {
            json!({
                "task_id": task.task_id,
                "allowed_read_paths": task.allowed_read_paths,
                "allowed_write_paths": task.allowed_write_paths,
                "forbidden_write_paths": task.forbidden_write_paths,
            })
        })
        .collect::<Vec<_>>();
    let mutation_allowed = writes
        .iter()
        .any(|path| !path.starts_with(&format!("{}/mission/", source.relative_change_path)));

    json!({
        "schema": "better-droid.path-policy.v1",
        "change_id": source.change_id,
        "default_policy": "deny-write-unless-allowed",
        "allowed_read_paths": reads.into_iter().collect::<Vec<_>>(),
        "allowed_write_paths": writes.into_iter().collect::<Vec<_>>(),
        "forbidden_write_paths": forbidden.into_iter().collect::<Vec<_>>(),
        "generated_paths": [format!(".codex/state/better-droid/{}/mission/*.json", source.change_id)],
        "task_overrides": task_overrides,
        "mutation_allowed": mutation_allowed,
        "reservation_policy": {
            "required_for_autonomous_mutation": true,
            "scope_class": "task-scoped"
        }
    })
}

pub(crate) fn section_items(markdown: &str, heading: &str) -> Vec<String> {
    let Some(section) = markdown_section(markdown, heading) else {
        return Vec::new();
    };
    section
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- "))
        .map(|line| line.trim().trim_matches('`').to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

pub(crate) fn first_section_sentence(markdown: &str, heading: &str) -> Option<String> {
    markdown_section(markdown, heading).and_then(|section| {
        section
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('-'))
            .map(|line| line.split('.').next().unwrap_or(line).trim().to_owned())
            .filter(|line| !line.is_empty())
    })
}

pub(crate) fn markdown_section<'a>(markdown: &'a str, heading: &str) -> Option<&'a str> {
    let marker = format!("## {heading}");
    let start = markdown.find(&marker)?;
    let body_start = markdown[start + marker.len()..]
        .find('\n')
        .map(|offset| start + marker.len() + offset + 1)?;
    let body = &markdown[body_start..];
    let end = body.find("\n## ").unwrap_or(body.len());
    Some(&body[..end])
}

pub(crate) fn dedupe_or_default(items: Vec<String>, default: Vec<String>) -> Vec<String> {
    let values = items
        .into_iter()
        .map(|item| item.trim().trim_matches('`').to_owned())
        .filter(|item| !item.is_empty())
        .collect::<BTreeSet<_>>();
    if values.is_empty() {
        default
    } else {
        values.into_iter().collect()
    }
}

pub(crate) fn compiled_tasks(source: &SourceSnapshot, lint: &LintState) -> Vec<CompiledTask> {
    source
        .tasks_parsed
        .iter()
        .map(|task| {
            let classification = lint
                .classifications
                .iter()
                .find(|candidate| candidate.task_id == task.id)
                .expect("lint classification exists for parsed task");
            CompiledTask {
                task_id: task.id.clone(),
                title: task.title.clone(),
                task_type: task_field(task, "Type").unwrap_or("unknown").to_owned(),
                import_status: classification.import_status,
                purpose: task_field(task, "Purpose").unwrap_or_default().to_owned(),
                acceptance_criteria: lines_from_field(task_field(task, "Acceptance Criteria")),
                requirement_ids: ids_in_text(&task.raw_block, "REQ-"),
                scenario_ids: ids_in_text(&task.raw_block, "SCN-"),
                dependencies: lines_from_field(task_field(task, "Dependencies")),
                allowed_read_paths: split_path_list(
                    task_field(task, "Allowed Read Paths").unwrap_or_default(),
                ),
                allowed_write_paths: split_path_list(
                    task_field(task, "Allowed Write Paths").unwrap_or_default(),
                ),
                forbidden_write_paths: split_path_list(
                    task_field(task, "Forbidden Paths").unwrap_or_default(),
                ),
                validation_refs: ids_in_text(&task.raw_block, "VAL-"),
                expected_evidence: lines_from_field(task_field(task, "Validation / Evidence")),
                expected_artifact_kind: task_field(task, "Expected Artifact Kind")
                    .unwrap_or("none")
                    .to_owned(),
                review_rubric_refs: vec![
                    "RR-artifact-digest".to_owned(),
                    "RR-traceability-complete".to_owned(),
                    "RR-validation-evidence".to_owned(),
                ],
                stale_if: lines_from_field(task_field(task, "Stale If")),
                task_digest: classification.task_digest.clone(),
            }
        })
        .collect()
}

pub(crate) fn lines_from_field(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .lines()
        .map(|line| line.trim().trim_start_matches("- ").to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

pub(crate) fn ids_in_text(text: &str, prefix: &str) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for token in text.split(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                ',' | ';' | ':' | ')' | '(' | '[' | ']' | '`' | '"' | '\'' | '.'
            )
    }) {
        if token.starts_with(prefix) {
            ids.insert(
                token
                    .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
                    .to_owned(),
            );
        }
    }
    ids.into_iter().collect()
}

pub(crate) fn review_rubric_items() -> Vec<Value> {
    vec![
        json!({
            "id": "RR-artifact-digest",
            "severity": "blocker",
            "question": "Does the reviewed artifact digest match the artifact under review?",
            "required_for_approval": true,
            "evidence_refs": ["artifact_digests"]
        }),
        json!({
            "id": "RR-traceability-complete",
            "severity": "blocker",
            "question": "Are all requirements and scenarios covered by task and validation evidence or explicit deferral?",
            "required_for_approval": true,
            "evidence_refs": ["traceability.json"]
        }),
        json!({
            "id": "RR-validation-evidence",
            "severity": "blocker",
            "question": "Does validation evidence include command, working directory, exit code, and output summary?",
            "required_for_approval": true,
            "evidence_refs": ["validation.json"]
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PlanningClass, Requirement, Scenario, SourceFile};
    use crate::report::ImportStatus;

    #[test]
    fn synthesized_openspec_validation_targets_change_namespace() {
        let source = SourceSnapshot {
            change_id: "same-id-change-and-spec".to_owned(),
            relative_change_path: "openspec/changes/same-id-change-and-spec".to_owned(),
            schema_name: "better-droid".to_owned(),
            change_status: None,
            planning_class: PlanningClass::WorkPacket,
            planning_class_raw: Some("work_packet".to_owned()),
            openspec_yaml: source_file("openspec/changes/same-id-change-and-spec/.openspec.yaml"),
            proposal: source_file("openspec/changes/same-id-change-and-spec/proposal.md"),
            design: source_file("openspec/changes/same-id-change-and-spec/design.md"),
            tasks: source_file("openspec/changes/same-id-change-and-spec/tasks.md"),
            specs: vec![source_file(
                "openspec/changes/same-id-change-and-spec/specs/same-id-change-and-spec/spec.md",
            )],
            tasks_parsed: vec![],
            requirements: vec![Requirement {
                id: "REQ-1".to_owned(),
                title: "requirement".to_owned(),
            }],
            scenarios: vec![Scenario {
                id: "SCN-1".to_owned(),
                title: "scenario".to_owned(),
                requirement_id: "REQ-1".to_owned(),
            }],
        };
        let tasks = vec![CompiledTask {
            task_id: "1.1".to_owned(),
            title: "Implement slice".to_owned(),
            task_type: "implementation".to_owned(),
            import_status: ImportStatus::Importable,
            purpose: "exercise fallback validation".to_owned(),
            acceptance_criteria: vec!["fallback validation exists".to_owned()],
            requirement_ids: vec!["REQ-1".to_owned()],
            scenario_ids: vec!["SCN-1".to_owned()],
            dependencies: vec![],
            allowed_read_paths: vec!["openspec/changes/**".to_owned()],
            allowed_write_paths: vec!["src/lib.rs".to_owned()],
            forbidden_write_paths: vec![".git/**".to_owned()],
            validation_refs: vec![],
            expected_evidence: vec![],
            expected_artifact_kind: "patch-bundle".to_owned(),
            review_rubric_refs: vec![],
            stale_if: vec!["source changes".to_owned()],
            task_digest: "blake3:0000000000000000000000000000000000000000000000000000000000000000"
                .to_owned(),
        }];

        let checks = validation_checks_from_tasks(&source, &tasks);

        assert_eq!(
            checks[0]["command"],
            "openspec validate same-id-change-and-spec --type change --strict"
        );
    }

    fn source_file(relative_path: &str) -> SourceFile {
        SourceFile {
            relative_path: relative_path.to_owned(),
            text: String::new(),
            digest: "blake3:0000000000000000000000000000000000000000000000000000000000000000"
                .to_owned(),
        }
    }
}

#![cfg_attr(coverage_nightly, coverage(off))]

use std::time::Instant;

use rusqlite::{Transaction, params};
use serde::Serialize;

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        CompletionPolicy, CreateSubtaskRequest, CreateWorkSubtaskReq, EventType, ObjectType,
        RoutingKey, SessionRole, SubtaskId, SubtaskKind, SubtaskState, completion_policy_name,
        subtask_kind_name, subtask_state_name,
    },
    store::{append_session_event, refresh_meta_task_state},
    validators::{
        MAX_OBJECT_ID_LEN, MAX_TITLE_LEN, ensure_length, ensure_meta_task_exists,
        ensure_meta_task_is_schedulable, normalize_subtask_dependencies, subtask_exists,
    },
};

/// Provenance recorded in `subtask_dependencies.source_ref` by the create path,
/// which is the local CLI's writer (importers record their own source refs).
const DEPENDENCY_SOURCE_REF: &str = "cli";

impl Covey {
    /// Creates work using the opened workflow profile.
    ///
    /// Resolved policies are persisted and included in the idempotency request
    /// and creation event, just as for explicit work creation.
    pub fn create_subtask(&self, req: CreateSubtaskRequest) -> Result<String> {
        self.create_work_subtask(CreateWorkSubtaskReq {
            session_token: req.session_token,
            meta_task_id: req.meta_task_id,
            subtask_id: req.subtask_id,
            title: req.title,
            priority: req.priority,
            completion_policy: self.workflow.completion_policy,
            routing_key: self.workflow.routing_key.clone(),
            evidence_policy: self.workflow.evidence_policy,
            depends_on: req.depends_on,
            idempotency_key: req.idempotency_key,
        })
    }

    /// Creates work with an explicit immutable completion policy and routing key.
    pub fn create_work_subtask(&self, req: CreateWorkSubtaskReq) -> Result<String> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                req.session_token.as_str(),
                "create_work_subtask",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || create_work_subtask_tx(tx, &req, now),
            )
        });
        self.log_operation(
            "create_work_subtask",
            req.session_token.as_str(),
            started_at,
            &result,
            |subtask_id| vec![format!("subtask:{subtask_id}")],
        );
        result
    }
}

pub(crate) fn create_work_subtask_tx(
    tx: &Transaction<'_>,
    req: &CreateWorkSubtaskReq,
    now: i64,
) -> Result<String> {
    let subtask_id = insert_work_subtask_tx(
        tx,
        req.session_token.as_str(),
        &req.meta_task_id,
        req.subtask_id.as_ref(),
        &req.title,
        req.priority,
        req.completion_policy,
        &req.routing_key,
        EventType::WorkSubtaskCreated,
        req,
        req.evidence_policy,
        now,
    )?;
    let subtask_id = SubtaskId::parse(subtask_id.as_str())?;
    insert_subtask_dependencies_tx(tx, &subtask_id, &req.depends_on, now)?;
    Ok(subtask_id.to_string())
}

/// Records the declared dependency edges of a freshly created subtask in the
/// same write transaction as its insert, so either everything commits or
/// nothing does.
///
/// A subtask created here has no dependents yet, so no dependency cycle is
/// possible at create time and no cycle machinery is needed: the existing
/// claim-time satisfaction gate in `store::subtask_dependencies_satisfied` is
/// the only place ordering is enforced. Dependencies may live on any meta-task
/// and be in any state; the existing satisfaction rules decide claimability.
fn insert_subtask_dependencies_tx(
    tx: &Transaction<'_>,
    subtask_id: &SubtaskId,
    depends_on: &[SubtaskId],
    now: i64,
) -> Result<()> {
    let dependencies = normalize_subtask_dependencies(subtask_id, depends_on)?;
    for dependency in &dependencies {
        if !subtask_exists(tx, dependency.as_str())? {
            return Err(CoveyError::SubtaskDependencyNotFound {
                subtask_id: subtask_id.clone(),
                depends_on_subtask_id: dependency.clone(),
            });
        }
        tx.execute(
            r#"
            INSERT INTO subtask_dependencies (
                subtask_id, depends_on_subtask_id, source_ref, created_at
            ) VALUES (?1, ?2, ?3, ?4)
            "#,
            params![
                subtask_id.as_str(),
                dependency.as_str(),
                DEPENDENCY_SOURCE_REF,
                now,
            ],
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_work_subtask_tx<T: Serialize>(
    tx: &Transaction<'_>,
    session_token: &str,
    meta_task_id: &crate::model::MetaTaskId,
    requested_subtask_id: Option<&SubtaskId>,
    title: &crate::model::SubtaskTitle,
    priority: crate::model::SubtaskPriority,
    completion_policy: CompletionPolicy,
    routing_key: &RoutingKey,
    event_type: EventType,
    event_payload: &T,
    evidence_policy: crate::model::EvidencePolicy,
    now: i64,
) -> Result<String> {
    crate::validators::require_role(tx, session_token, &[SessionRole::Orchestrator])?;
    ensure_length("title", title, MAX_TITLE_LEN)?;
    ensure_length("meta_task_id", meta_task_id, MAX_OBJECT_ID_LEN)?;
    ensure_meta_task_exists(tx, meta_task_id.as_str())?;
    ensure_meta_task_is_schedulable(tx, meta_task_id.as_str())?;

    let subtask_id = requested_subtask_id.cloned().unwrap_or_else(|| {
        SubtaskId::parse(crate::model::make_id("subtask")).expect("generated subtask ids are valid")
    });
    ensure_length("subtask_id", &subtask_id, MAX_OBJECT_ID_LEN)?;
    if subtask_exists(tx, &subtask_id)? {
        return Err(CoveyError::DuplicateSubtaskId {
            subtask_id: subtask_id.clone(),
        });
    }

    tx.execute(
        r#"
        INSERT INTO subtasks (
            subtask_id, meta_task_id, title, kind, review_target_subtask_id,
            review_target_artifact_digest, state, current_claim_id, artifact_digest,
            priority, completion_policy, routing_key, evidence_policy, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, ?8, ?9, ?10, ?11, ?12, ?12)
        "#,
        params![
            subtask_id.as_str(),
            meta_task_id.as_str(),
            title.as_str(),
            subtask_kind_name(SubtaskKind::Work),
            Option::<String>::None,
            Option::<String>::None,
            subtask_state_name(SubtaskState::Available),
            priority.get(),
            completion_policy_name(completion_policy),
            routing_key.as_str(),
            evidence_policy.as_ref(),
            now,
        ],
    )?;
    tx.execute(
        "INSERT INTO subtask_fence_counter (subtask_id, next_fence_seq) VALUES (?1, 1)",
        params![subtask_id.as_str()],
    )?;
    append_session_event(
        tx,
        event_type,
        ObjectType::Subtask,
        subtask_id.as_str(),
        session_token,
        event_payload,
        now,
    )?;
    refresh_meta_task_state(tx, meta_task_id.as_str(), now)?;
    Ok(String::from(subtask_id))
}

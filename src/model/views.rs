use derive_new::new;
use serde::{Deserialize, Serialize};

use super::{Artifact, Claim, MetaTask, ReadyQueueItem, Review, Session, Subtask};

/// Snapshot view of a session and its currently active subtask, if any.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct SessionStatus {
    pub session: Session,
    pub active_subtask: Option<Subtask>,
}

/// Snapshot view of a subtask and its attached stateful records.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct SubtaskStatus {
    pub subtask: Subtask,
    pub claim: Option<Claim>,
    pub artifact: Option<Artifact>,
    pub reviews: Vec<Review>,
    pub ready_queue: Vec<ReadyQueueItem>,
}

/// Snapshot view of a meta-task and all of its subtasks.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct MetaTaskStatus {
    pub meta_task: MetaTask,
    pub subtasks: Vec<Subtask>,
}

/// Observability row for a subtask that has not moved recently enough to merit attention.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct StuckSubtask {
    pub subtask: Subtask,
    pub claim: Option<Claim>,
    pub session: Option<Session>,
    pub idle_for_ms: i64,
}

/// Observability row for a held claim whose lease deadline is approaching.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct ExpiringClaim {
    pub claim: Claim,
    pub subtask: Subtask,
    pub session: Session,
    pub expires_in_ms: i64,
}

/// Aggregate counts and queue ages for the ready queue.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct ReadyQueueMetrics {
    pub queued_count: usize,
    pub in_flight_count: usize,
    pub oldest_queued_age_ms: Option<i64>,
    pub oldest_in_flight_age_ms: Option<i64>,
}

/// Live authorization check for a git landing side effect.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct LandingAuthorizationStatus {
    pub accepted: bool,
    pub queue_id: String,
    pub artifact_digest: String,
    pub review_id: String,
    pub findings_digest: String,
    pub claim_fence_seq: i64,
    pub verifier: String,
    pub verdict_digest: String,
    pub seal_digest: String,
    pub recorded_by_session: String,
}

/// Policy facts passed through to mutAI repoops preflight.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct RepoopsAuthorityPolicyFact {
    pub mode: String,
    pub phase: u8,
    pub denied_rule_id: Option<String>,
}

/// Claim facts passed through to mutAI repoops preflight.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct RepoopsAuthorityClaimFact {
    pub claim_id: String,
    pub status: String,
    pub owner: String,
    pub scope_in: Vec<String>,
    pub scope_out: Vec<String>,
    pub has_required_contract_fields: bool,
    pub active_ownership_token: Option<String>,
}

/// Scope facts derived from Covey lifecycle and reservation state.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct RepoopsAuthorityScopeFact {
    #[serde(rename = "in")]
    pub scope_in: Vec<String>,
    #[serde(rename = "out")]
    pub scope_out: Vec<String>,
}

/// Path ownership fact passed through to mutAI repoops preflight.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct RepoopsAuthorityLockFact {
    pub path: String,
    pub owner: String,
    pub claim_id: String,
    pub status: String,
}

/// Git context facts known to Covey for repoops preflight.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct RepoopsAuthorityGitContextFact {
    pub policy_project_path: Option<String>,
    pub execution_project_path: Option<String>,
    pub repo_path_prefix: Option<String>,
    pub ownership_token_required: bool,
}

/// Covey lifecycle fact snapshot for mutAI repoops preflight.
#[allow(clippy::too_many_arguments)]
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct RepoopsAuthoritySnapshot {
    pub schema_version: String,
    pub agent_id: String,
    pub claim_id: Option<String>,
    pub ownership_token: Option<String>,
    pub override_token: Option<String>,
    pub policy: RepoopsAuthorityPolicyFact,
    pub claim: Option<RepoopsAuthorityClaimFact>,
    pub scope: RepoopsAuthorityScopeFact,
    pub locks: Vec<RepoopsAuthorityLockFact>,
    pub git_context: Option<RepoopsAuthorityGitContextFact>,
    pub constraint_reason: Option<String>,
    pub fact_sources: Vec<String>,
}

/// Result of a stale-session reap pass.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct ReapResult {
    pub stale_sessions: usize,
}

/// Result of a lease-expiration maintenance pass.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct ExpireResult {
    pub expired_count: usize,
}

use derive_new::new;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeError};
use strum::Display;

use super::{
    Artifact, ArtifactDigest, Claim, ClaimId, FenceSeq, FindingsDigest, MetaTask, MetaTaskId,
    QueueId, ReadyQueueItem, RepoopsClaimRef, Review, ReviewId, ReviewTarget, Session,
    SessionToken, Subtask, SubtaskId, SubtaskKind, SubtaskLifecycle, SubtaskRow, SubtaskState,
    TimestampMs,
};

/// Read model for CLI and API responses that expose subtask lifecycle state.
#[must_use]
#[allow(clippy::too_many_arguments)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtaskView {
    pub subtask_id: SubtaskId,
    pub meta_task_id: MetaTaskId,
    pub title: String,
    kind: SubtaskViewKind,
    lifecycle: SubtaskLifecycle,
    pub priority: i64,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SubtaskViewKind {
    Work,
    Review { review_target: ReviewTarget },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawSubtaskView {
    subtask_id: SubtaskId,
    meta_task_id: MetaTaskId,
    title: String,
    kind: SubtaskKind,
    review_target: Option<ReviewTarget>,
    state: SubtaskState,
    active_claim_id: Option<ClaimId>,
    artifact_digest: Option<ArtifactDigest>,
    priority: i64,
    created_at: TimestampMs,
    updated_at: TimestampMs,
}

impl SubtaskView {
    #[allow(clippy::too_many_arguments)]
    fn new(
        subtask_id: SubtaskId,
        meta_task_id: MetaTaskId,
        title: String,
        kind: SubtaskViewKind,
        lifecycle: SubtaskLifecycle,
        priority: i64,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> Self {
        Self {
            subtask_id,
            meta_task_id,
            title,
            kind,
            lifecycle,
            priority,
            created_at,
            updated_at,
        }
    }

    /// Returns whether this view describes work or review work.
    #[must_use]
    pub const fn kind(&self) -> SubtaskKind {
        self.kind.kind()
    }

    /// Returns the review target for review subtasks.
    #[must_use]
    pub const fn review_target(&self) -> Option<&ReviewTarget> {
        self.kind.review_target()
    }

    /// Returns the state encoded by the view lifecycle.
    #[must_use]
    pub const fn state(&self) -> SubtaskState {
        self.lifecycle.state()
    }

    /// Returns the active claim id when the lifecycle state allows one.
    #[must_use]
    pub fn active_claim_id(&self) -> Option<&ClaimId> {
        self.lifecycle.active_claim_id()
    }

    /// Returns the artifact digest when the lifecycle state allows one.
    #[must_use]
    pub fn artifact_digest(&self) -> Option<&ArtifactDigest> {
        self.lifecycle.artifact_digest()
    }
}

impl SubtaskViewKind {
    fn from_parts(
        kind: SubtaskKind,
        review_target: Option<ReviewTarget>,
    ) -> rusqlite::Result<Self> {
        match kind {
            SubtaskKind::Work => {
                if review_target.is_some() {
                    return Err(invalid_subtask_view(
                        "work subtask view cannot carry review target",
                    ));
                }
                Ok(Self::Work)
            }
            SubtaskKind::Review => {
                let Some(review_target) = review_target else {
                    return Err(invalid_subtask_view(
                        "review subtask view is missing review target",
                    ));
                };
                Ok(Self::Review { review_target })
            }
        }
    }

    const fn kind(&self) -> SubtaskKind {
        match self {
            Self::Work => SubtaskKind::Work,
            Self::Review { .. } => SubtaskKind::Review,
        }
    }

    const fn review_target(&self) -> Option<&ReviewTarget> {
        match self {
            Self::Work => None,
            Self::Review { review_target } => Some(review_target),
        }
    }
}

impl TryFrom<SubtaskRow> for SubtaskView {
    type Error = rusqlite::Error;

    fn try_from(row: SubtaskRow) -> Result<Self, Self::Error> {
        let domain = Subtask::try_from(row.clone())?;
        let lifecycle = domain.lifecycle();

        Ok(Self::new(
            row.subtask_id,
            row.meta_task_id,
            row.title,
            SubtaskViewKind::from_parts(domain.kind(), domain.review_target().cloned())?,
            lifecycle.clone(),
            row.priority,
            row.created_at,
            row.updated_at,
        ))
    }
}

impl From<&SubtaskView> for RawSubtaskView {
    fn from(view: &SubtaskView) -> Self {
        Self {
            subtask_id: view.subtask_id.clone(),
            meta_task_id: view.meta_task_id.clone(),
            title: view.title.clone(),
            kind: view.kind(),
            review_target: view.review_target().cloned(),
            state: view.state(),
            active_claim_id: view.active_claim_id().cloned(),
            artifact_digest: view.artifact_digest().cloned(),
            priority: view.priority,
            created_at: view.created_at,
            updated_at: view.updated_at,
        }
    }
}

impl TryFrom<RawSubtaskView> for SubtaskView {
    type Error = rusqlite::Error;

    fn try_from(raw: RawSubtaskView) -> Result<Self, Self::Error> {
        let lifecycle =
            SubtaskLifecycle::from_row_parts(raw.state, raw.active_claim_id, raw.artifact_digest)?;
        let kind = SubtaskViewKind::from_parts(raw.kind, raw.review_target)?;
        Ok(Self::new(
            raw.subtask_id,
            raw.meta_task_id,
            raw.title,
            kind,
            lifecycle,
            raw.priority,
            raw.created_at,
            raw.updated_at,
        ))
    }
}

impl Serialize for SubtaskView {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawSubtaskView::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SubtaskView {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawSubtaskView::deserialize(deserializer)?
            .try_into()
            .map_err(D::Error::custom)
    }
}

fn invalid_subtask_view(reason: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            reason.to_owned(),
        )),
    )
}

/// Snapshot view of a session and its currently active subtask, if any.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct SessionStatus {
    pub session: Session,
    pub active_subtask: Option<SubtaskView>,
}

/// Snapshot view of a subtask and its attached stateful records.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct SubtaskStatus {
    pub subtask: SubtaskView,
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
    pub subtasks: Vec<SubtaskView>,
}

/// Observability row for a subtask that has not moved recently enough to merit attention.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct StuckSubtask {
    pub subtask: SubtaskView,
    pub claim: Option<Claim>,
    pub session: Option<Session>,
    pub idle_for_ms: i64,
}

/// Observability row for a held claim whose lease deadline is approaching.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct ExpiringClaim {
    pub claim: Claim,
    pub subtask: SubtaskView,
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
#[allow(clippy::too_many_arguments)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandingAuthorizationStatus {
    status: LandingAuthorizationState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LandingAuthorizationState {
    Accepted(LandingAuthorizationAccepted),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LandingAuthorizationAccepted {
    queue_id: QueueId,
    artifact_digest: ArtifactDigest,
    review_id: ReviewId,
    findings_digest: FindingsDigest,
    claim_fence_seq: FenceSeq,
    verifier: String,
    verdict_digest: ArtifactDigest,
    seal_digest: ArtifactDigest,
    recorded_by_session: SessionToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawLandingAuthorizationStatus {
    accepted: bool,
    queue_id: QueueId,
    artifact_digest: ArtifactDigest,
    review_id: ReviewId,
    findings_digest: FindingsDigest,
    claim_fence_seq: FenceSeq,
    verifier: String,
    verdict_digest: ArtifactDigest,
    seal_digest: ArtifactDigest,
    recorded_by_session: SessionToken,
}

impl LandingAuthorizationStatus {
    /// Builds an accepted landing authorization status.
    #[allow(clippy::too_many_arguments)]
    pub fn accepted(
        queue_id: QueueId,
        artifact_digest: ArtifactDigest,
        review_id: ReviewId,
        findings_digest: FindingsDigest,
        claim_fence_seq: FenceSeq,
        verifier: String,
        verdict_digest: ArtifactDigest,
        seal_digest: ArtifactDigest,
        recorded_by_session: SessionToken,
    ) -> Self {
        Self {
            status: LandingAuthorizationState::Accepted(LandingAuthorizationAccepted {
                queue_id,
                artifact_digest,
                review_id,
                findings_digest,
                claim_fence_seq,
                verifier,
                verdict_digest,
                seal_digest,
                recorded_by_session,
            }),
        }
    }

    /// Returns whether Covey accepted the live authorization check.
    #[must_use]
    pub const fn accepted_flag(&self) -> bool {
        true
    }

    /// Returns the authorized ready-queue id.
    #[must_use]
    pub const fn queue_id(&self) -> &QueueId {
        &self.accepted_fields().queue_id
    }

    /// Returns the authorized artifact digest.
    #[must_use]
    pub const fn artifact_digest(&self) -> &ArtifactDigest {
        &self.accepted_fields().artifact_digest
    }

    /// Returns the review id bound to the authorization.
    #[must_use]
    pub const fn review_id(&self) -> &ReviewId {
        &self.accepted_fields().review_id
    }

    /// Returns the reviewer findings digest bound to the authorization.
    #[must_use]
    pub const fn findings_digest(&self) -> &FindingsDigest {
        &self.accepted_fields().findings_digest
    }

    /// Returns the accepted claim fence sequence.
    #[must_use]
    pub const fn claim_fence_seq(&self) -> FenceSeq {
        self.accepted_fields().claim_fence_seq
    }

    /// Returns the verifier identity.
    #[must_use]
    pub fn verifier(&self) -> &str {
        &self.accepted_fields().verifier
    }

    /// Returns the verdict digest bound to the authorization.
    #[must_use]
    pub const fn verdict_digest(&self) -> &ArtifactDigest {
        &self.accepted_fields().verdict_digest
    }

    /// Returns the apply-verification seal digest.
    #[must_use]
    pub const fn seal_digest(&self) -> &ArtifactDigest {
        &self.accepted_fields().seal_digest
    }

    /// Returns the session that recorded the accepted verifier evidence.
    #[must_use]
    pub const fn recorded_by_session(&self) -> &SessionToken {
        &self.accepted_fields().recorded_by_session
    }

    const fn accepted_fields(&self) -> &LandingAuthorizationAccepted {
        match &self.status {
            LandingAuthorizationState::Accepted(status) => status,
        }
    }
}

impl Serialize for LandingAuthorizationStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawLandingAuthorizationStatus::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LandingAuthorizationStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawLandingAuthorizationStatus::deserialize(deserializer)?
            .try_into()
            .map_err(DeError::custom)
    }
}

impl From<&LandingAuthorizationStatus> for RawLandingAuthorizationStatus {
    fn from(status: &LandingAuthorizationStatus) -> Self {
        let accepted = status.accepted_fields();
        Self {
            accepted: true,
            queue_id: accepted.queue_id.clone(),
            artifact_digest: accepted.artifact_digest.clone(),
            review_id: accepted.review_id.clone(),
            findings_digest: accepted.findings_digest.clone(),
            claim_fence_seq: accepted.claim_fence_seq,
            verifier: accepted.verifier.clone(),
            verdict_digest: accepted.verdict_digest.clone(),
            seal_digest: accepted.seal_digest.clone(),
            recorded_by_session: accepted.recorded_by_session.clone(),
        }
    }
}

impl TryFrom<RawLandingAuthorizationStatus> for LandingAuthorizationStatus {
    type Error = String;

    fn try_from(raw: RawLandingAuthorizationStatus) -> Result<Self, Self::Error> {
        if !raw.accepted {
            return Err("landing authorization status is only emitted for accepted checks".into());
        }
        Ok(Self::accepted(
            raw.queue_id,
            raw.artifact_digest,
            raw.review_id,
            raw.findings_digest,
            raw.claim_fence_seq,
            raw.verifier,
            raw.verdict_digest,
            raw.seal_digest,
            raw.recorded_by_session,
        ))
    }
}

/// Policy facts passed through to mutAI repoops preflight.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoopsAuthorityPolicyFact {
    policy: RepoopsAuthorityPolicy,
}

/// Repoops policy fact shape with state-dependent payloads encoded in variants.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
enum RepoopsAuthorityPolicy {
    Enforce { phase: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawRepoopsAuthorityPolicyFact {
    mode: RepoopsAuthorityPolicyMode,
    phase: u8,
    denied_rule_id: Option<String>,
}

impl RepoopsAuthorityPolicyFact {
    /// Builds one enforce-mode policy fact.
    pub const fn enforce(phase: u8) -> Self {
        Self {
            policy: RepoopsAuthorityPolicy::Enforce { phase },
        }
    }

    /// Returns the policy enforcement mode.
    pub const fn mode(&self) -> RepoopsAuthorityPolicyMode {
        self.policy.mode()
    }

    /// Returns the policy phase.
    #[must_use]
    pub const fn phase(&self) -> u8 {
        self.policy.phase()
    }

    /// Returns the denied rule id when the policy mode supports one.
    #[must_use]
    pub const fn denied_rule_id(&self) -> Option<&String> {
        self.policy.denied_rule_id()
    }
}

impl RepoopsAuthorityPolicy {
    fn try_from_parts(
        mode: RepoopsAuthorityPolicyMode,
        phase: u8,
        denied_rule_id: Option<String>,
    ) -> Result<Self, String> {
        match mode {
            RepoopsAuthorityPolicyMode::Enforce => {
                if denied_rule_id.is_some() {
                    return Err(
                        "enforce repoops policy fact must not include denied_rule_id".into(),
                    );
                }
                Ok(Self::Enforce { phase })
            }
        }
    }

    const fn mode(&self) -> RepoopsAuthorityPolicyMode {
        match self {
            Self::Enforce { .. } => RepoopsAuthorityPolicyMode::Enforce,
        }
    }

    const fn phase(&self) -> u8 {
        match self {
            Self::Enforce { phase } => *phase,
        }
    }

    const fn denied_rule_id(&self) -> Option<&String> {
        match self {
            Self::Enforce { .. } => None,
        }
    }
}

impl Serialize for RepoopsAuthorityPolicyFact {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawRepoopsAuthorityPolicyFact {
            mode: self.mode(),
            phase: self.phase(),
            denied_rule_id: self.denied_rule_id().cloned(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RepoopsAuthorityPolicyFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawRepoopsAuthorityPolicyFact::deserialize(deserializer)?;
        let policy =
            RepoopsAuthorityPolicy::try_from_parts(raw.mode, raw.phase, raw.denied_rule_id)
                .map_err(serde::de::Error::custom)?;
        Ok(Self { policy })
    }
}

/// Repoops policy enforcement mode exposed to mutAI authority.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RepoopsAuthorityPolicyMode {
    Enforce,
}

/// Claim facts passed through to mutAI repoops preflight.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoopsAuthorityClaimFact {
    pub claim_id: ClaimId,
    pub owner: String,
    pub scope_in: Vec<String>,
    pub scope_out: Vec<String>,
    pub has_required_contract_fields: bool,
    lifecycle: RepoopsAuthorityClaimLifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RepoopsAuthorityClaimLifecycle {
    InProgress { active_ownership_token: String },
    Open,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawRepoopsAuthorityClaimFact {
    claim_id: ClaimId,
    status: RepoopsAuthorityClaimStatus,
    owner: String,
    scope_in: Vec<String>,
    scope_out: Vec<String>,
    has_required_contract_fields: bool,
    active_ownership_token: Option<String>,
}

/// Claim lifecycle status exposed to mutAI repoops authority.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RepoopsAuthorityClaimStatus {
    InProgress,
    Open,
}

impl RepoopsAuthorityClaimFact {
    /// Builds an in-progress claim fact with its active ownership token reference.
    pub fn in_progress(
        claim_id: ClaimId,
        owner: String,
        scope_in: Vec<String>,
        scope_out: Vec<String>,
        has_required_contract_fields: bool,
        active_ownership_token: String,
    ) -> Self {
        Self {
            claim_id,
            owner,
            scope_in,
            scope_out,
            has_required_contract_fields,
            lifecycle: RepoopsAuthorityClaimLifecycle::InProgress {
                active_ownership_token,
            },
        }
    }

    /// Builds an open claim fact. Open claims do not carry active ownership tokens.
    pub fn open(
        claim_id: ClaimId,
        owner: String,
        scope_in: Vec<String>,
        scope_out: Vec<String>,
        has_required_contract_fields: bool,
    ) -> Self {
        Self {
            claim_id,
            owner,
            scope_in,
            scope_out,
            has_required_contract_fields,
            lifecycle: RepoopsAuthorityClaimLifecycle::Open,
        }
    }

    /// Returns the claim status.
    pub const fn status(&self) -> RepoopsAuthorityClaimStatus {
        self.lifecycle.status()
    }

    /// Returns the active ownership token reference for in-progress claims.
    #[must_use]
    pub fn active_ownership_token(&self) -> Option<&str> {
        self.lifecycle.active_ownership_token()
    }
}

impl RepoopsAuthorityClaimLifecycle {
    fn try_from_parts(
        status: RepoopsAuthorityClaimStatus,
        active_ownership_token: Option<String>,
    ) -> Result<Self, String> {
        match (status, active_ownership_token) {
            (RepoopsAuthorityClaimStatus::InProgress, Some(active_ownership_token)) => {
                Ok(Self::InProgress {
                    active_ownership_token,
                })
            }
            (RepoopsAuthorityClaimStatus::InProgress, None) => {
                Err("in-progress repoops claim facts require active_ownership_token".into())
            }
            (RepoopsAuthorityClaimStatus::Open, None) => Ok(Self::Open),
            (RepoopsAuthorityClaimStatus::Open, Some(_)) => {
                Err("open repoops claim facts must not include active_ownership_token".into())
            }
        }
    }

    const fn status(&self) -> RepoopsAuthorityClaimStatus {
        match self {
            Self::InProgress { .. } => RepoopsAuthorityClaimStatus::InProgress,
            Self::Open => RepoopsAuthorityClaimStatus::Open,
        }
    }

    fn active_ownership_token(&self) -> Option<&str> {
        match self {
            Self::InProgress {
                active_ownership_token,
            } => Some(active_ownership_token),
            Self::Open => None,
        }
    }
}

impl Serialize for RepoopsAuthorityClaimFact {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawRepoopsAuthorityClaimFact {
            claim_id: self.claim_id.clone(),
            status: self.status(),
            owner: self.owner.clone(),
            scope_in: self.scope_in.clone(),
            scope_out: self.scope_out.clone(),
            has_required_contract_fields: self.has_required_contract_fields,
            active_ownership_token: self.active_ownership_token().map(str::to_owned),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RepoopsAuthorityClaimFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawRepoopsAuthorityClaimFact::deserialize(deserializer)?;
        let lifecycle =
            RepoopsAuthorityClaimLifecycle::try_from_parts(raw.status, raw.active_ownership_token)
                .map_err(serde::de::Error::custom)?;
        Ok(Self {
            claim_id: raw.claim_id,
            owner: raw.owner,
            scope_in: raw.scope_in,
            scope_out: raw.scope_out,
            has_required_contract_fields: raw.has_required_contract_fields,
            lifecycle,
        })
    }
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
    pub claim_id: RepoopsClaimRef,
    pub status: RepoopsAuthorityLockStatus,
}

/// Path lock ownership status exposed to mutAI repoops authority.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RepoopsAuthorityLockStatus {
    Owned,
    ForeignOwner,
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
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoopsAuthoritySnapshot {
    pub schema_version: String,
    pub agent_id: String,
    subject: RepoopsAuthoritySnapshotSubject,
    pub policy: RepoopsAuthorityPolicyFact,
    pub scope: RepoopsAuthorityScopeFact,
    pub locks: Vec<RepoopsAuthorityLockFact>,
    pub git_context: Option<RepoopsAuthorityGitContextFact>,
    pub fact_sources: Vec<String>,
}

/// Fields shared by all repoops authority snapshot subjects.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoopsAuthoritySnapshotCommon {
    pub schema_version: String,
    pub agent_id: String,
    pub policy: RepoopsAuthorityPolicyFact,
    pub scope: RepoopsAuthorityScopeFact,
    pub locks: Vec<RepoopsAuthorityLockFact>,
    pub git_context: Option<RepoopsAuthorityGitContextFact>,
    pub fact_sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RepoopsAuthoritySnapshotSubject {
    ClaimBound {
        claim_id: ClaimId,
        ownership_token: String,
        claim: RepoopsAuthorityClaimFact,
    },
    Constrained {
        constraint_reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawRepoopsAuthoritySnapshot {
    schema_version: String,
    agent_id: String,
    claim_id: Option<ClaimId>,
    ownership_token: Option<String>,
    override_token: Option<String>,
    policy: RepoopsAuthorityPolicyFact,
    claim: Option<RepoopsAuthorityClaimFact>,
    scope: RepoopsAuthorityScopeFact,
    locks: Vec<RepoopsAuthorityLockFact>,
    git_context: Option<RepoopsAuthorityGitContextFact>,
    constraint_reason: Option<String>,
    fact_sources: Vec<String>,
}

impl RepoopsAuthoritySnapshot {
    /// Builds a snapshot for one current Covey claim selected by repoops preflight.
    pub fn claim_bound(
        common: RepoopsAuthoritySnapshotCommon,
        ownership_token: String,
        claim: RepoopsAuthorityClaimFact,
    ) -> Self {
        let claim_id = claim.claim_id.clone();
        Self {
            schema_version: common.schema_version,
            agent_id: common.agent_id,
            subject: RepoopsAuthoritySnapshotSubject::ClaimBound {
                claim_id,
                ownership_token,
                claim,
            },
            policy: common.policy,
            scope: common.scope,
            locks: common.locks,
            git_context: common.git_context,
            fact_sources: common.fact_sources,
        }
    }

    /// Builds a constrained snapshot that carries no live claim authority.
    pub fn constrained(common: RepoopsAuthoritySnapshotCommon, constraint_reason: String) -> Self {
        Self {
            schema_version: common.schema_version,
            agent_id: common.agent_id,
            subject: RepoopsAuthoritySnapshotSubject::Constrained { constraint_reason },
            policy: common.policy,
            scope: common.scope,
            locks: common.locks,
            git_context: common.git_context,
            fact_sources: common.fact_sources,
        }
    }

    /// Returns the current claim id when this is a claim-bound snapshot.
    #[must_use]
    pub const fn claim_id(&self) -> Option<&ClaimId> {
        match &self.subject {
            RepoopsAuthoritySnapshotSubject::ClaimBound { claim_id, .. } => Some(claim_id),
            RepoopsAuthoritySnapshotSubject::Constrained { .. } => None,
        }
    }

    /// Returns the caller ownership token reference for claim-bound snapshots.
    #[must_use]
    pub fn ownership_token(&self) -> Option<&str> {
        match &self.subject {
            RepoopsAuthoritySnapshotSubject::ClaimBound {
                ownership_token, ..
            } => Some(ownership_token),
            RepoopsAuthoritySnapshotSubject::Constrained { .. } => None,
        }
    }

    /// Returns the override token reference, when a future supported subject carries one.
    #[must_use]
    pub const fn override_token(&self) -> Option<&str> {
        None
    }

    /// Returns the current claim fact when this is a claim-bound snapshot.
    #[must_use]
    pub const fn claim(&self) -> Option<&RepoopsAuthorityClaimFact> {
        match &self.subject {
            RepoopsAuthoritySnapshotSubject::ClaimBound { claim, .. } => Some(claim),
            RepoopsAuthoritySnapshotSubject::Constrained { .. } => None,
        }
    }

    /// Returns the constraint reason when this snapshot has no claim authority.
    #[must_use]
    pub fn constraint_reason(&self) -> Option<&str> {
        match &self.subject {
            RepoopsAuthoritySnapshotSubject::ClaimBound { .. } => None,
            RepoopsAuthoritySnapshotSubject::Constrained { constraint_reason } => {
                Some(constraint_reason)
            }
        }
    }
}

impl RepoopsAuthoritySnapshotSubject {
    fn try_from_parts(
        claim_id: Option<ClaimId>,
        ownership_token: Option<String>,
        override_token: Option<String>,
        claim: Option<RepoopsAuthorityClaimFact>,
        constraint_reason: Option<String>,
    ) -> Result<Self, String> {
        if override_token.is_some() {
            return Err("repoops authority snapshots do not support override_token yet".into());
        }
        match (claim_id, ownership_token, claim, constraint_reason) {
            (Some(claim_id), Some(ownership_token), Some(claim), None) => {
                if claim_id != claim.claim_id {
                    return Err(
                        "repoops authority snapshot claim_id must match claim.claim_id".into(),
                    );
                }
                Ok(Self::ClaimBound {
                    claim_id,
                    ownership_token,
                    claim,
                })
            }
            (None, None, None, Some(constraint_reason)) => {
                Ok(Self::Constrained { constraint_reason })
            }
            _ => Err(
                "repoops authority snapshots must be claim-bound or constrained, not mixed".into(),
            ),
        }
    }
}

impl Serialize for RepoopsAuthoritySnapshot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawRepoopsAuthoritySnapshot {
            schema_version: self.schema_version.clone(),
            agent_id: self.agent_id.clone(),
            claim_id: self.claim_id().cloned(),
            ownership_token: self.ownership_token().map(str::to_owned),
            override_token: self.override_token().map(str::to_owned),
            policy: self.policy.clone(),
            claim: self.claim().cloned(),
            scope: self.scope.clone(),
            locks: self.locks.clone(),
            git_context: self.git_context.clone(),
            constraint_reason: self.constraint_reason().map(str::to_owned),
            fact_sources: self.fact_sources.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RepoopsAuthoritySnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawRepoopsAuthoritySnapshot::deserialize(deserializer)?;
        let subject = RepoopsAuthoritySnapshotSubject::try_from_parts(
            raw.claim_id,
            raw.ownership_token,
            raw.override_token,
            raw.claim,
            raw.constraint_reason,
        )
        .map_err(serde::de::Error::custom)?;
        Ok(Self {
            schema_version: raw.schema_version,
            agent_id: raw.agent_id,
            subject,
            policy: raw.policy,
            scope: raw.scope,
            locks: raw.locks,
            git_context: raw.git_context,
            fact_sources: raw.fact_sources,
        })
    }
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

use std::{collections::HashSet, num::NonZeroUsize};

use derive_new::new;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer, de::Error as DeError, ser::SerializeStruct,
};
use strum::Display;

use super::{
    AgentPrincipalId, ApplyGateBlocker, ApplyGateBlockerKind, Artifact, ArtifactDigest,
    ArtifactKind, ArtifactManifestPath, AttemptOutcome, BaseRev, ChangedPathsDigest, Claim,
    ClaimId, CompletionPolicy, FailedReviewVerdict, FenceSeq, FindingsDigest, MetaTask, MetaTaskId,
    OpenSpecArchiveStatus, OpenSpecChangeId, OperatorBlocker, ProseTasksetId, QueueId,
    ReadyQueueItem, ReadyQueueState, RepoopsClaimRef, RepoopsPath, Review, ReviewId, ReviewState,
    ReviewTarget, ReviewVerdict, RoutingKey, Session, SessionToken, SettlementReconcileBlocker,
    SettlementReconcileReason, SettlementTarget, Subtask, SubtaskId, SubtaskKind, SubtaskLifecycle,
    SubtaskPriority, SubtaskRow, SubtaskState, SubtaskTitle, TimestampMs, VcsWorkspace,
    VcsWorkspaceCleanliness, VcsWorkspaceState, VerifierId,
};

/// Read model for CLI and API responses that expose subtask lifecycle state.
#[must_use]
#[allow(clippy::too_many_arguments)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtaskView {
    pub subtask_id: SubtaskId,
    pub meta_task_id: MetaTaskId,
    pub title: SubtaskTitle,
    kind: SubtaskViewKind,
    lifecycle: SubtaskLifecycle,
    pub priority: SubtaskPriority,
    completion_policy: CompletionPolicy,
    routing_key: RoutingKey,
    timestamps: SubtaskViewTimestamps,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SubtaskViewKind {
    Work,
    Review { review_target: ReviewTarget },
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SubtaskViewTimestamps {
    created_at: TimestampMs,
    updated_at: TimestampMs,
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
    priority: SubtaskPriority,
    #[serde(default = "default_completion_policy")]
    completion_policy: CompletionPolicy,
    #[serde(default = "default_routing_key")]
    routing_key: RoutingKey,
    created_at: TimestampMs,
    updated_at: TimestampMs,
}

impl SubtaskView {
    #[allow(clippy::too_many_arguments)]
    fn new(
        subtask_id: SubtaskId,
        meta_task_id: MetaTaskId,
        title: SubtaskTitle,
        kind: SubtaskViewKind,
        lifecycle: SubtaskLifecycle,
        priority: SubtaskPriority,
        completion_policy: CompletionPolicy,
        routing_key: RoutingKey,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> rusqlite::Result<Self> {
        let timestamps = SubtaskViewTimestamps::new(created_at, updated_at)?;
        Ok(Self {
            subtask_id,
            meta_task_id,
            title,
            kind,
            lifecycle,
            priority,
            completion_policy,
            routing_key,
            timestamps,
        })
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

    #[must_use]
    pub const fn completion_policy(&self) -> CompletionPolicy {
        self.completion_policy
    }

    #[must_use]
    pub const fn routing_key(&self) -> &RoutingKey {
        &self.routing_key
    }

    /// Returns when this subtask view was created.
    #[must_use]
    pub const fn created_at(&self) -> TimestampMs {
        self.timestamps.created_at()
    }

    /// Returns when this subtask view was last updated.
    #[must_use]
    pub const fn updated_at(&self) -> TimestampMs {
        self.timestamps.updated_at()
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
            SubtaskKind::Cleanup => {
                if review_target.is_some() {
                    return Err(invalid_subtask_view(
                        "cleanup subtask view cannot carry review target",
                    ));
                }
                Ok(Self::Cleanup)
            }
        }
    }

    const fn kind(&self) -> SubtaskKind {
        match self {
            Self::Work => SubtaskKind::Work,
            Self::Review { .. } => SubtaskKind::Review,
            Self::Cleanup => SubtaskKind::Cleanup,
        }
    }

    const fn review_target(&self) -> Option<&ReviewTarget> {
        match self {
            Self::Work | Self::Cleanup => None,
            Self::Review { review_target } => Some(review_target),
        }
    }
}

impl TryFrom<SubtaskRow> for SubtaskView {
    type Error = rusqlite::Error;

    fn try_from(row: SubtaskRow) -> Result<Self, Self::Error> {
        let domain = Subtask::try_from(row.clone())?;
        let lifecycle = domain.lifecycle();
        let created_at = row.created_at();
        let updated_at = row.updated_at();
        let completion_policy = row.completion_policy();
        let routing_key = row.routing_key().clone();

        Self::new(
            row.subtask_id,
            row.meta_task_id,
            row.title,
            SubtaskViewKind::from_parts(domain.kind(), domain.review_target().cloned())?,
            lifecycle.clone(),
            row.priority,
            completion_policy,
            routing_key,
            created_at,
            updated_at,
        )
    }
}

impl From<&SubtaskView> for RawSubtaskView {
    fn from(view: &SubtaskView) -> Self {
        Self {
            subtask_id: view.subtask_id.clone(),
            meta_task_id: view.meta_task_id.clone(),
            title: view.title.as_str().to_owned(),
            kind: view.kind(),
            review_target: view.review_target().cloned(),
            state: view.state(),
            active_claim_id: view.active_claim_id().cloned(),
            artifact_digest: view.artifact_digest().cloned(),
            priority: view.priority,
            completion_policy: view.completion_policy(),
            routing_key: view.routing_key().clone(),
            created_at: view.created_at(),
            updated_at: view.updated_at(),
        }
    }
}

impl TryFrom<RawSubtaskView> for SubtaskView {
    type Error = rusqlite::Error;

    fn try_from(raw: RawSubtaskView) -> Result<Self, Self::Error> {
        let kind = SubtaskViewKind::from_parts(raw.kind, raw.review_target)?;
        let lifecycle = SubtaskLifecycle::from_row_parts_for_kind(
            kind.kind(),
            raw.state,
            raw.active_claim_id,
            raw.artifact_digest,
        )?;
        if kind.kind() == SubtaskKind::Work {
            lifecycle.ensure_allowed_for_completion_policy(raw.completion_policy)?;
        }
        Self::new(
            raw.subtask_id,
            raw.meta_task_id,
            SubtaskTitle::parse(raw.title)
                .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?,
            kind,
            lifecycle,
            raw.priority,
            raw.completion_policy,
            raw.routing_key,
            raw.created_at,
            raw.updated_at,
        )
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

/// Result returned after a review verdict is recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ReviewDecisionResult {
    Approved {
        review_id: ReviewId,
    },
    Failed {
        review_id: ReviewId,
        verdict: FailedReviewVerdict,
        followup_subtask_id: SubtaskId,
    },
}

impl ReviewDecisionResult {
    #[must_use]
    pub const fn review_id(&self) -> &ReviewId {
        match self {
            Self::Approved { review_id } | Self::Failed { review_id, .. } => review_id,
        }
    }

    #[must_use]
    pub const fn verdict(&self) -> ReviewVerdict {
        match self {
            Self::Approved { .. } => ReviewVerdict::Approve,
            Self::Failed { verdict, .. } => match verdict {
                FailedReviewVerdict::ChangesRequested => ReviewVerdict::ChangesRequested,
                FailedReviewVerdict::Blocked => ReviewVerdict::Blocked,
            },
        }
    }

    #[must_use]
    pub const fn followup_subtask_id(&self) -> Option<&SubtaskId> {
        match self {
            Self::Approved { .. } => None,
            Self::Failed {
                followup_subtask_id,
                ..
            } => Some(followup_subtask_id),
        }
    }
}

impl SubtaskViewTimestamps {
    fn new(created_at: TimestampMs, updated_at: TimestampMs) -> rusqlite::Result<Self> {
        if updated_at < created_at {
            return Err(invalid_subtask_view(
                "subtask view updated_at must be greater than or equal to created_at",
            ));
        }
        Ok(Self {
            created_at,
            updated_at,
        })
    }

    const fn created_at(self) -> TimestampMs {
        self.created_at
    }

    const fn updated_at(self) -> TimestampMs {
        self.updated_at
    }
}

/// Snapshot view of a session and its currently active subtask, if any.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStatus {
    state: SessionStatusState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionStatusState {
    WithoutActiveSubtask {
        session: Session,
    },
    WithActiveSubtask {
        session: Session,
        active_subtask: Box<SubtaskView>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawSessionStatus {
    session: Session,
    active_subtask: Option<SubtaskView>,
}

impl SessionStatus {
    /// Builds a session status view for a session with no active subtask.
    ///
    /// # Errors
    ///
    /// Returns an error when the session lifecycle carries an active subtask id.
    pub fn without_active_subtask(session: Session) -> Result<Self, String> {
        Self::from_parts(session, None)
    }

    /// Builds a session status view for a session with a matching active subtask.
    ///
    /// # Errors
    ///
    /// Returns an error when the active subtask view is stale, or when the
    /// session does not carry a matching active subtask id.
    pub fn with_active_subtask(
        session: Session,
        active_subtask: SubtaskView,
    ) -> Result<Self, String> {
        Self::from_parts(session, Some(active_subtask))
    }

    pub(crate) fn from_parts(
        session: Session,
        active_subtask: Option<SubtaskView>,
    ) -> Result<Self, String> {
        let state = match (session.active_subtask_id(), active_subtask) {
            (Some(expected), Some(active_subtask)) if &active_subtask.subtask_id == expected => {
                SessionStatusState::WithActiveSubtask {
                    session,
                    active_subtask: Box::new(active_subtask),
                }
            }
            (Some(_), Some(_)) => {
                return Err("session status active_subtask must match session state".to_owned());
            }
            (Some(_), None) => {
                return Err("session status requires active_subtask view".to_owned());
            }
            (None, Some(_)) => {
                return Err("session status must not include active_subtask".to_owned());
            }
            (None, None) => SessionStatusState::WithoutActiveSubtask { session },
        };
        Ok(Self { state })
    }

    /// Returns the session row.
    #[must_use]
    pub const fn session(&self) -> &Session {
        match &self.state {
            SessionStatusState::WithoutActiveSubtask { session }
            | SessionStatusState::WithActiveSubtask { session, .. } => session,
        }
    }

    /// Returns the active subtask view, when the session has one.
    #[must_use]
    pub const fn active_subtask(&self) -> Option<&SubtaskView> {
        match &self.state {
            SessionStatusState::WithoutActiveSubtask { .. } => None,
            SessionStatusState::WithActiveSubtask { active_subtask, .. } => Some(active_subtask),
        }
    }
}

impl Serialize for SessionStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawSessionStatus::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SessionStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawSessionStatus::deserialize(deserializer)?
            .try_into()
            .map_err(D::Error::custom)
    }
}

impl From<&SessionStatus> for RawSessionStatus {
    fn from(status: &SessionStatus) -> Self {
        Self {
            session: status.session().clone(),
            active_subtask: status.active_subtask().cloned(),
        }
    }
}

impl TryFrom<RawSessionStatus> for SessionStatus {
    type Error = String;

    fn try_from(raw: RawSessionStatus) -> Result<Self, Self::Error> {
        Self::from_parts(raw.session, raw.active_subtask)
    }
}

/// Snapshot view of a subtask and its attached stateful records.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtaskStatus {
    subtask: SubtaskView,
    attachments: SubtaskStatusAttachments,
    attempt_outcomes: Vec<AttemptOutcome>,
    reviews: Vec<Review>,
    ready_queue: Vec<ReadyQueueItem>,
    readiness: SubtaskReadinessStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SubtaskStatusAttachments {
    Detached,
    Claimed { claim: Claim },
    Artifact { artifact: Artifact },
    ClaimedArtifact { claim: Claim, artifact: Artifact },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawSubtaskStatus {
    subtask: SubtaskView,
    claim: Option<Claim>,
    artifact: Option<Artifact>,
    #[serde(default)]
    attempt_outcomes: Vec<AttemptOutcome>,
    reviews: Vec<Review>,
    ready_queue: Vec<ReadyQueueItem>,
    #[serde(default)]
    readiness: SubtaskReadinessStatus,
}

impl SubtaskStatus {
    /// Builds a subtask status view with attachments that match the subtask lifecycle.
    ///
    /// # Errors
    ///
    /// Returns an error when claim, artifact, review, or queue attachments do
    /// not belong to the subtask or contradict its lifecycle fields.
    pub fn new(
        subtask: SubtaskView,
        claim: Option<Claim>,
        artifact: Option<Artifact>,
        reviews: Vec<Review>,
        ready_queue: Vec<ReadyQueueItem>,
    ) -> Result<Self, String> {
        Self::new_with_landing_receipt_and_attempt_outcomes(
            subtask,
            claim,
            artifact,
            Vec::new(),
            reviews,
            ready_queue,
            false,
        )
    }

    /// Builds a subtask status view with explicit landing receipt evidence.
    ///
    /// # Errors
    ///
    /// Returns an error when attachments do not belong to the subtask or
    /// contradict its lifecycle fields.
    pub fn new_with_landing_receipt(
        subtask: SubtaskView,
        claim: Option<Claim>,
        artifact: Option<Artifact>,
        reviews: Vec<Review>,
        ready_queue: Vec<ReadyQueueItem>,
        landing_receipt_recorded: bool,
    ) -> Result<Self, String> {
        Self::new_with_landing_receipt_and_attempt_outcomes(
            subtask,
            claim,
            artifact,
            Vec::new(),
            reviews,
            ready_queue,
            landing_receipt_recorded,
        )
    }

    /// Builds a subtask status view including immutable execution-attempt outcomes.
    ///
    /// # Errors
    ///
    /// Returns an error when an attachment or attempt outcome does not belong
    /// to the subtask, or contradicts its lifecycle fields.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_landing_receipt_and_attempt_outcomes(
        subtask: SubtaskView,
        claim: Option<Claim>,
        artifact: Option<Artifact>,
        attempt_outcomes: Vec<AttemptOutcome>,
        reviews: Vec<Review>,
        ready_queue: Vec<ReadyQueueItem>,
        landing_receipt_recorded: bool,
    ) -> Result<Self, String> {
        let attachments = SubtaskStatusAttachments::from_parts(&subtask, claim, artifact)?;
        for outcome in &attempt_outcomes {
            if outcome.subtask_id != subtask.subtask_id {
                return Err("subtask status attempt outcomes must belong to the subtask".to_owned());
            }
        }
        for review in &reviews {
            if review.subtask_id() != subtask.subtask_id.as_str() {
                return Err("subtask status reviews must belong to the subtask".to_owned());
            }
        }
        for item in &ready_queue {
            if item.subtask_id() != subtask.subtask_id.as_str() {
                return Err(
                    "subtask status ready-queue items must belong to the subtask".to_owned(),
                );
            }
        }
        Ok(Self {
            readiness: SubtaskReadinessStatus::from_parts(
                &subtask,
                &reviews,
                &ready_queue,
                landing_receipt_recorded,
            ),
            subtask,
            attachments,
            attempt_outcomes,
            reviews,
            ready_queue,
        })
    }

    /// Returns the subtask view.
    #[must_use]
    pub const fn subtask(&self) -> &SubtaskView {
        &self.subtask
    }

    /// Returns the active claim when the subtask lifecycle carries one.
    #[must_use]
    pub const fn claim(&self) -> Option<&Claim> {
        self.attachments.claim()
    }

    /// Returns the artifact when the subtask lifecycle carries one.
    #[must_use]
    pub const fn artifact(&self) -> Option<&Artifact> {
        self.attachments.artifact()
    }

    /// Returns immutable outcomes for completed execution attempts.
    pub fn attempt_outcomes(&self) -> &[AttemptOutcome] {
        &self.attempt_outcomes
    }

    /// Returns reviews associated with the subtask.
    #[must_use]
    pub fn reviews(&self) -> &[Review] {
        &self.reviews
    }

    /// Returns ready-queue items associated with the subtask.
    #[must_use]
    pub fn ready_queue(&self) -> &[ReadyQueueItem] {
        &self.ready_queue
    }

    /// Returns the domain-specific readiness projection for this subtask.
    pub const fn readiness(&self) -> &SubtaskReadinessStatus {
        &self.readiness
    }
}

/// Explicit readiness projection for subtask status output.
#[must_use]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtaskReadinessStatus {
    pub planning_ready: bool,
    pub covey_imported: bool,
    pub execution_ready: bool,
    pub review_approved: bool,
    pub apply_queued: bool,
    pub apply_authorized: bool,
    pub landed: bool,
    pub shipped_verified: bool,
}

impl SubtaskReadinessStatus {
    fn from_parts(
        subtask: &SubtaskView,
        reviews: &[Review],
        ready_queue: &[ReadyQueueItem],
        landing_receipt_recorded: bool,
    ) -> Self {
        Self {
            planning_ready: false,
            covey_imported: true,
            execution_ready: subtask.kind() == SubtaskKind::Work
                && matches!(subtask.state(), SubtaskState::Available),
            review_approved: reviews
                .iter()
                .any(|review| review.verdict() == Some(ReviewVerdict::Approve)),
            apply_queued: ready_queue.iter().any(|item| {
                matches!(
                    item.state(),
                    ReadyQueueState::Queued | ReadyQueueState::InFlight
                )
            }),
            apply_authorized: false,
            landed: landing_receipt_recorded,
            shipped_verified: false,
        }
    }
}

impl SubtaskStatusAttachments {
    fn from_parts(
        subtask: &SubtaskView,
        claim: Option<Claim>,
        artifact: Option<Artifact>,
    ) -> Result<Self, String> {
        validate_subtask_status_attachments(subtask, claim.as_ref(), artifact.as_ref())?;
        match (claim, artifact) {
            (Some(claim), Some(artifact)) => Ok(Self::ClaimedArtifact { claim, artifact }),
            (Some(claim), None) => Ok(Self::Claimed { claim }),
            (None, Some(artifact)) => Ok(Self::Artifact { artifact }),
            (None, None) => Ok(Self::Detached),
        }
    }

    const fn claim(&self) -> Option<&Claim> {
        match self {
            Self::Claimed { claim } | Self::ClaimedArtifact { claim, .. } => Some(claim),
            Self::Artifact { .. } | Self::Detached => None,
        }
    }

    const fn artifact(&self) -> Option<&Artifact> {
        match self {
            Self::Artifact { artifact } | Self::ClaimedArtifact { artifact, .. } => Some(artifact),
            Self::Claimed { .. } | Self::Detached => None,
        }
    }
}

fn validate_subtask_status_attachments(
    subtask: &SubtaskView,
    claim: Option<&Claim>,
    artifact: Option<&Artifact>,
) -> Result<(), String> {
    match (subtask.active_claim_id(), claim) {
        (Some(expected), Some(claim)) if &claim.claim_id == expected => {
            if claim.subtask_id != subtask.subtask_id {
                return Err("subtask status claim must belong to the subtask".to_owned());
            }
        }
        (Some(_), Some(_)) => {
            return Err("subtask status claim_id must match active claim".to_owned());
        }
        (Some(_), None) => return Err("subtask status requires active claim row".to_owned()),
        (None, Some(_)) => {
            return Err("subtask status must not include claim without active claim".to_owned());
        }
        (None, None) => {}
    }

    match (subtask.artifact_digest(), artifact) {
        (Some(expected), Some(artifact)) if &artifact.artifact_digest == expected => {
            if artifact.produced_by_subtask_id != subtask.subtask_id {
                return Err("subtask status artifact must belong to the subtask".to_owned());
            }
        }
        (Some(_), Some(_)) => {
            return Err("subtask status artifact_digest must match lifecycle artifact".to_owned());
        }
        (Some(_), None) => return Err("subtask status requires artifact row".to_owned()),
        (None, Some(_)) => {
            return Err(
                "subtask status must not include artifact without lifecycle artifact".to_owned(),
            );
        }
        (None, None) => {}
    }

    Ok(())
}

impl Serialize for SubtaskStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawSubtaskStatus::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SubtaskStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawSubtaskStatus::deserialize(deserializer)?
            .try_into()
            .map_err(D::Error::custom)
    }
}

impl From<&SubtaskStatus> for RawSubtaskStatus {
    fn from(status: &SubtaskStatus) -> Self {
        Self {
            subtask: status.subtask.clone(),
            claim: status.claim().cloned(),
            artifact: status.artifact().cloned(),
            attempt_outcomes: status.attempt_outcomes.clone(),
            reviews: status.reviews.clone(),
            ready_queue: status.ready_queue.clone(),
            readiness: status.readiness.clone(),
        }
    }
}

impl TryFrom<RawSubtaskStatus> for SubtaskStatus {
    type Error = String;

    fn try_from(raw: RawSubtaskStatus) -> Result<Self, Self::Error> {
        Self::new_with_landing_receipt_and_attempt_outcomes(
            raw.subtask,
            raw.claim,
            raw.artifact,
            raw.attempt_outcomes,
            raw.reviews,
            raw.ready_queue,
            raw.readiness.landed,
        )
    }
}

/// Snapshot view of a meta-task and all of its subtasks.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaTaskStatus {
    meta_task: MetaTask,
    subtasks: Vec<SubtaskView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawMetaTaskStatus {
    meta_task: MetaTask,
    subtasks: Vec<SubtaskView>,
}

impl MetaTaskStatus {
    /// Builds a meta-task status view whose subtasks all belong to the meta-task.
    ///
    /// # Errors
    ///
    /// Returns an error when any subtask is attached to a different meta-task.
    pub fn new(meta_task: MetaTask, subtasks: Vec<SubtaskView>) -> Result<Self, String> {
        for subtask in &subtasks {
            if subtask.meta_task_id != meta_task.meta_task_id {
                return Err("meta-task status subtasks must belong to the meta-task".to_owned());
            }
        }
        Ok(Self {
            meta_task,
            subtasks,
        })
    }

    /// Returns the meta-task row.
    #[must_use]
    pub const fn meta_task(&self) -> &MetaTask {
        &self.meta_task
    }

    /// Returns subtasks attached to this meta-task.
    #[must_use]
    pub fn subtasks(&self) -> &[SubtaskView] {
        &self.subtasks
    }
}

impl Serialize for MetaTaskStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawMetaTaskStatus::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MetaTaskStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawMetaTaskStatus::deserialize(deserializer)?
            .try_into()
            .map_err(D::Error::custom)
    }
}

impl From<&MetaTaskStatus> for RawMetaTaskStatus {
    fn from(status: &MetaTaskStatus) -> Self {
        Self {
            meta_task: status.meta_task.clone(),
            subtasks: status.subtasks.clone(),
        }
    }
}

impl TryFrom<RawMetaTaskStatus> for MetaTaskStatus {
    type Error = String;

    fn try_from(raw: RawMetaTaskStatus) -> Result<Self, Self::Error> {
        Self::new(raw.meta_task, raw.subtasks)
    }
}

/// Read-only summary of currently claimable subtask lanes.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimableSubtaskAvailability {
    executor_claimable_count: usize,
    reviewer_claimable_count: usize,
}

impl ClaimableSubtaskAvailability {
    /// Builds a claimable-subtask availability summary.
    pub const fn new(executor_claimable_count: usize, reviewer_claimable_count: usize) -> Self {
        Self {
            executor_claimable_count,
            reviewer_claimable_count,
        }
    }

    /// Returns how many work subtasks are claimable by executor sessions.
    #[must_use]
    pub const fn executor_claimable_count(&self) -> usize {
        self.executor_claimable_count
    }

    /// Returns how many review subtasks are claimable by reviewer sessions.
    #[must_use]
    pub const fn reviewer_claimable_count(&self) -> usize {
        self.reviewer_claimable_count
    }
}

/// Read-only scheduler candidate for an executor or reviewer lane.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtaskCandidate {
    pub subtask_id: SubtaskId,
    pub meta_task_id: MetaTaskId,
    pub title: SubtaskTitle,
    pub kind: SubtaskKind,
    pub review_target: Option<ReviewTarget>,
    pub state: SubtaskState,
    pub artifact_digest: Option<ArtifactDigest>,
    pub priority: SubtaskPriority,
    #[serde(default = "default_completion_policy")]
    pub completion_policy: CompletionPolicy,
    #[serde(default = "default_routing_key")]
    pub routing_key: RoutingKey,
    pub effective_priority: i64,
    pub is_repair_followup: bool,
    pub blocked_dependents_count: usize,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

impl SubtaskCandidate {
    /// Builds one read-only subtask candidate from persisted facts.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        subtask_id: SubtaskId,
        meta_task_id: MetaTaskId,
        title: SubtaskTitle,
        kind: SubtaskKind,
        review_target_subtask_id: Option<SubtaskId>,
        review_target_artifact_digest: Option<ArtifactDigest>,
        state: SubtaskState,
        artifact_digest: Option<ArtifactDigest>,
        priority: SubtaskPriority,
        effective_priority: i64,
        is_repair_followup: bool,
        blocked_dependents_count: usize,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> Result<Self, String> {
        Self::new_with_execution_contract(
            subtask_id,
            meta_task_id,
            title,
            kind,
            review_target_subtask_id,
            review_target_artifact_digest,
            state,
            artifact_digest,
            priority,
            CompletionPolicy::Direct,
            default_routing_key(),
            effective_priority,
            is_repair_followup,
            blocked_dependents_count,
            created_at,
            updated_at,
        )
    }

    /// Builds a candidate with its persisted completion and routing contract.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_execution_contract(
        subtask_id: SubtaskId,
        meta_task_id: MetaTaskId,
        title: SubtaskTitle,
        kind: SubtaskKind,
        review_target_subtask_id: Option<SubtaskId>,
        review_target_artifact_digest: Option<ArtifactDigest>,
        state: SubtaskState,
        artifact_digest: Option<ArtifactDigest>,
        priority: SubtaskPriority,
        completion_policy: CompletionPolicy,
        routing_key: RoutingKey,
        effective_priority: i64,
        is_repair_followup: bool,
        blocked_dependents_count: usize,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> Result<Self, String> {
        let review_target = match (
            kind,
            review_target_subtask_id,
            review_target_artifact_digest,
        ) {
            (SubtaskKind::Work, None, None) => None,
            (SubtaskKind::Cleanup, None, None) => None,
            (SubtaskKind::Review, Some(subtask_id), Some(artifact_digest)) => {
                Some(ReviewTarget::new(subtask_id, artifact_digest))
            }
            (SubtaskKind::Work, _, _) => {
                return Err("work candidate cannot carry review target".to_owned());
            }
            (SubtaskKind::Cleanup, _, _) => {
                return Err("cleanup candidate cannot carry review target".to_owned());
            }
            (SubtaskKind::Review, _, _) => {
                return Err("review candidate requires complete review target".to_owned());
            }
        };
        if updated_at < created_at {
            return Err(
                "candidate updated_at must be greater than or equal to created_at".to_owned(),
            );
        }
        Ok(Self {
            subtask_id,
            meta_task_id,
            title,
            kind,
            review_target,
            state,
            artifact_digest,
            priority,
            completion_policy,
            routing_key,
            effective_priority,
            is_repair_followup,
            blocked_dependents_count,
            created_at,
            updated_at,
        })
    }
}

fn default_routing_key() -> RoutingKey {
    RoutingKey::parse("default").expect("the built-in default routing key is valid")
}

const fn default_completion_policy() -> CompletionPolicy {
    CompletionPolicy::Direct
}

/// Read-only scheduler candidate for the apply queue lane.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyQueueCandidate {
    pub queue_id: QueueId,
    pub artifact_digest: ArtifactDigest,
    pub subtask_id: SubtaskId,
    pub settlement_target: SettlementTarget,
    pub state: ReadyQueueState,
    pub last_claim_fence_seq: Option<i64>,
    pub enqueued_at: TimestampMs,
    pub updated_at: TimestampMs,
}

impl ReadyQueueCandidate {
    /// Builds an apply-lane candidate from a queued ready-queue item.
    pub fn from_item(item: &ReadyQueueItem) -> Result<Self, String> {
        if item.state() != ReadyQueueState::Queued {
            return Err("ready-queue candidate must be queued".to_owned());
        }
        Ok(Self {
            queue_id: QueueId::parse(item.queue_id().to_owned()).map_err(|err| err.to_string())?,
            artifact_digest: ArtifactDigest::parse(item.artifact_digest().to_owned())
                .map_err(|err| err.to_string())?,
            subtask_id: SubtaskId::parse(item.subtask_id().to_owned())
                .map_err(|err| err.to_string())?,
            settlement_target: item.settlement_target(),
            state: item.state(),
            last_claim_fence_seq: item.claim_fence_seq(),
            enqueued_at: TimestampMs::parse(item.enqueued_at()).map_err(|err| err.to_string())?,
            updated_at: TimestampMs::parse(item.updated_at()).map_err(|err| err.to_string())?,
        })
    }
}

/// Result of reconciling approved or ready-for-apply work into the apply queue.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyQueueReconcileResult {
    pub approved_enqueued_count: usize,
    pub ready_for_apply_enqueued_count: usize,
    pub queue_ids: Vec<QueueId>,
}

impl ApplyQueueReconcileResult {
    /// Builds a typed reconciliation result.
    ///
    /// # Errors
    ///
    /// Returns an error when any generated queue id is invalid.
    pub fn new(
        approved_enqueued_count: usize,
        ready_for_apply_enqueued_count: usize,
        queue_ids: Vec<String>,
    ) -> Result<Self, String> {
        let queue_ids = queue_ids
            .into_iter()
            .map(QueueId::parse)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| err.to_string())?;
        Ok(Self {
            approved_enqueued_count,
            ready_for_apply_enqueued_count,
            queue_ids,
        })
    }

    /// Returns total queue rows created by reconciliation.
    #[must_use]
    pub const fn enqueued_count(&self) -> usize {
        self.approved_enqueued_count + self.ready_for_apply_enqueued_count
    }
}

/// Result of reconciling changes-requested reviews into repair follow-up subtasks.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangesRequestedFollowupReconcileResult {
    pub created_count: usize,
    pub followup_subtask_ids: Vec<SubtaskId>,
}

impl ChangesRequestedFollowupReconcileResult {
    /// Builds a typed reconciliation result.
    ///
    /// # Errors
    ///
    /// Returns an error when any generated subtask id is invalid.
    pub fn new(followup_subtask_ids: Vec<String>) -> Result<Self, String> {
        let created_count = followup_subtask_ids.len();
        let followup_subtask_ids = followup_subtask_ids
            .into_iter()
            .map(SubtaskId::parse)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| err.to_string())?;
        Ok(Self {
            created_count,
            followup_subtask_ids,
        })
    }
}

/// Covey-owned archive readiness facts for one OpenSpec change.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenSpecArchiveEligibility {
    pub openspec_change_id: OpenSpecChangeId,
    pub scoped_subtasks: Vec<SubtaskView>,
    pub open_archive_blockers: Vec<OpenSpecArchiveStatus>,
    pub pending_subtasks: Vec<SubtaskView>,
    pub safe_to_archive: bool,
}

impl OpenSpecArchiveEligibility {
    /// Builds archive eligibility facts.
    #[must_use]
    pub fn new(
        openspec_change_id: OpenSpecChangeId,
        scoped_subtasks: Vec<SubtaskView>,
        open_archive_blockers: Vec<OpenSpecArchiveStatus>,
        pending_subtasks: Vec<SubtaskView>,
    ) -> Self {
        let safe_to_archive = !scoped_subtasks.is_empty()
            && pending_subtasks.is_empty()
            && !open_archive_blockers.is_empty();
        Self {
            openspec_change_id,
            scoped_subtasks,
            open_archive_blockers,
            pending_subtasks,
            safe_to_archive,
        }
    }
}

/// Orchestrator-owned cleanup claim facts for one OpenSpec archive operation.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenSpecArchiveCleanupClaim {
    pub openspec_change_id: OpenSpecChangeId,
    pub cleanup_subtask_id: SubtaskId,
    pub cleanup_claim_id: ClaimId,
    pub fence_seq: FenceSeq,
    pub archive_paths: Vec<RepoopsPath>,
    pub open_archive_blockers: Vec<OpenSpecArchiveStatus>,
}

impl OpenSpecArchiveCleanupClaim {
    /// Builds cleanup claim facts.
    #[must_use]
    pub fn new(
        openspec_change_id: OpenSpecChangeId,
        cleanup_subtask_id: SubtaskId,
        cleanup_claim_id: ClaimId,
        fence_seq: FenceSeq,
        archive_paths: Vec<RepoopsPath>,
        open_archive_blockers: Vec<OpenSpecArchiveStatus>,
    ) -> Self {
        Self {
            openspec_change_id,
            cleanup_subtask_id,
            cleanup_claim_id,
            fence_seq,
            archive_paths,
            open_archive_blockers,
        }
    }
}

/// Result of resolving OpenSpec archive blockers for one change.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenSpecArchiveCleanupFinish {
    pub openspec_change_id: OpenSpecChangeId,
    pub archive_proof_digest: ArtifactDigest,
    pub archived_queue_ids: Vec<QueueId>,
    pub cleanup_subtask_id: SubtaskId,
    pub cleanup_claim_id: ClaimId,
}

/// Covey-derived current-work label for one OpenSpec work packet.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenSpecCurrentWorkState {
    Imported,
    Claimed,
    Reviewing,
    Applying,
    Archived,
    Blocked,
}

/// Surface that owns the next move for the current-work projection.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenSpecCurrentWorkOwner {
    Covey,
    Executor,
    Reviewer,
    ApplyGate,
    OpenSpecArchive,
    Authority,
    Operator,
}

/// Covey-derived current-work label for one lightweight prose taskset.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProseCurrentWorkState {
    Imported,
    Claimed,
    Reviewing,
    Applying,
    Applied,
    Blocked,
}

/// Surface that owns the next move for a prose current-work projection.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProseCurrentWorkOwner {
    Covey,
    Executor,
    Reviewer,
    SchedulerApply,
    Operator,
}

/// One named blocker for a lightweight prose taskset.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProseCurrentWorkBlocker {
    pub blocker_id: String,
    pub evidence_id: String,
    pub owner: ProseCurrentWorkOwner,
    pub queue_id: Option<QueueId>,
    pub artifact_digest: Option<ArtifactDigest>,
    pub review_id: Option<ReviewId>,
    pub reason: String,
}

/// Batch-level current-work projection for a lightweight prose taskset.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProseCurrentWork {
    pub taskset_id: ProseTasksetId,
    pub meta_task_id: MetaTaskId,
    pub provenance_tier: String,
    pub preview_digest: ArtifactDigest,
    pub state: ProseCurrentWorkState,
    pub next_owner: ProseCurrentWorkOwner,
    pub subtask_ids: Vec<SubtaskId>,
    pub queue_ids: Vec<QueueId>,
    pub artifact_digests: Vec<ArtifactDigest>,
    pub review_ids: Vec<ReviewId>,
    pub blockers: Vec<ProseCurrentWorkBlocker>,
}

/// Stable blocker kind emitted by the current-work projection.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenSpecCurrentWorkBlockerKind {
    MissingImport,
    AppliedButUnarchived,
    SubtaskBlocked,
    ExpiredClaim,
    StaleClaim,
    SchedulerStateLoss,
    HookStateStaleClaim,
    HookStateStaleLandingAuthorization,
    HookStateInvalidLandingAuthorization,
    AuthorityHold,
    GitApplyUncertainty,
    OperatorBlocked,
}

/// Bounded recovery selected for a current-work blocker.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenSpecCurrentWorkRepairAction {
    RunOpenSpec,
    RecoverSubtask,
    RecoverExpiredClaim,
    RecoverDeadClaim,
    RecoverQueue,
    RecoverWorkspace,
    ArchiveOpenSpec,
    RecoverLandingReceipt,
    ResolveOperatorBlocker,
    FailClosed,
}

/// Whether a repair playbook can inspect only, or may mutate Covey/repo state.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenSpecCurrentWorkRepairSafety {
    Safe,
    Mutating,
}

/// Derived operator playbook for one current-work blocker.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenSpecCurrentWorkRepairPlaybook {
    pub repair_action: OpenSpecCurrentWorkRepairAction,
    pub repair_safety: OpenSpecCurrentWorkRepairSafety,
    pub required_evidence_id: String,
    pub expected_postcondition: String,
    pub rollback_retry_note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_command: Option<String>,
}

impl OpenSpecCurrentWorkRepairPlaybook {
    fn new(
        repair_action: OpenSpecCurrentWorkRepairAction,
        repair_safety: OpenSpecCurrentWorkRepairSafety,
        required_evidence_id: impl Into<String>,
        expected_postcondition: impl Into<String>,
        rollback_retry_note: impl Into<String>,
    ) -> Self {
        Self {
            repair_action,
            repair_safety,
            required_evidence_id: required_evidence_id.into(),
            expected_postcondition: expected_postcondition.into(),
            rollback_retry_note: rollback_retry_note.into(),
            repair_command: None,
        }
    }
}

/// Covey-derived stale claim fact for one OpenSpec current-work projection.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSpecCurrentWorkStaleClaim {
    pub claim: Claim,
    pub idle_for_ms: i64,
    pub threshold_ms: i64,
}

/// One named blocker for a current-work projection.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenSpecCurrentWorkBlocker {
    pub blocker_id: String,
    pub evidence_id: String,
    pub kind: OpenSpecCurrentWorkBlockerKind,
    pub owner: OpenSpecCurrentWorkOwner,
    #[serde(default)]
    pub allowed_repairs: Vec<String>,
    pub repair_playbook: OpenSpecCurrentWorkRepairPlaybook,
    pub subtask_id: Option<SubtaskId>,
    pub claim_id: Option<ClaimId>,
    pub queue_id: Option<QueueId>,
    pub artifact_digest: Option<ArtifactDigest>,
    pub review_id: Option<ReviewId>,
    pub reason: String,
}

impl OpenSpecCurrentWorkBlocker {
    /// Builds a missing-import blocker.
    #[must_use]
    pub fn missing_import(openspec_change_id: &OpenSpecChangeId) -> Self {
        let evidence_id = format!(
            "openspec_current_work:missing_import:{}",
            openspec_change_id.as_str()
        );
        Self {
            blocker_id: format!(
                "blocker_openspec_current_work_missing_import_{}",
                openspec_change_id.as_str()
            ),
            evidence_id: evidence_id.clone(),
            kind: OpenSpecCurrentWorkBlockerKind::MissingImport,
            owner: OpenSpecCurrentWorkOwner::Covey,
            allowed_repairs: repair_commands(&["mutai-scheduler orchestrator run-openspec"]),
            repair_playbook: OpenSpecCurrentWorkRepairPlaybook::new(
                OpenSpecCurrentWorkRepairAction::RunOpenSpec,
                OpenSpecCurrentWorkRepairSafety::Mutating,
                evidence_id,
                "Covey current-work contains scoped subtasks for the OpenSpec change",
                "rerun repair after fixing OpenSpec import readiness; no partial Covey import is assumed",
            ),
            subtask_id: None,
            claim_id: None,
            queue_id: None,
            artifact_digest: None,
            review_id: None,
            reason: format!(
                "OpenSpec change {} has no Covey scoped subtasks",
                openspec_change_id.as_str()
            ),
        }
    }

    /// Builds a blocker for an applied queue item that still needs archive cleanup.
    #[must_use]
    pub fn applied_but_unarchived(status: &OpenSpecArchiveStatus) -> Self {
        let evidence_id = format!(
            "openspec_current_work:applied_but_unarchived:{}:{}",
            status.queue_id.as_str(),
            status.artifact_digest.as_str()
        );
        Self {
            blocker_id: format!(
                "blocker_openspec_current_work_applied_but_unarchived_{}",
                status.queue_id.as_str()
            ),
            evidence_id: evidence_id.clone(),
            kind: OpenSpecCurrentWorkBlockerKind::AppliedButUnarchived,
            owner: OpenSpecCurrentWorkOwner::OpenSpecArchive,
            allowed_repairs: repair_commands(&[
                "mutai-scheduler orchestrator archive-openspec",
                "mutai-scheduler orchestrator recover open-spec-archive-status",
            ]),
            repair_playbook: OpenSpecCurrentWorkRepairPlaybook::new(
                OpenSpecCurrentWorkRepairAction::ArchiveOpenSpec,
                OpenSpecCurrentWorkRepairSafety::Mutating,
                evidence_id,
                "OpenSpec archive status is archived for the applied queue artifact",
                "retry archive-openspec after resolving authority preflight or OpenSpec archive errors",
            ),
            subtask_id: Some(status.subtask_id.clone()),
            claim_id: None,
            queue_id: Some(status.queue_id.clone()),
            artifact_digest: Some(status.artifact_digest.clone()),
            review_id: None,
            reason: status
                .blocked_reason
                .as_ref()
                .map_or_else(|| "applied_but_unarchived".to_owned(), ToString::to_string),
        }
    }

    /// Builds a blocker for an applied queue item that has no archive status row yet.
    #[must_use]
    pub fn applied_queue_unarchived(queue_item: &ReadyQueueItem) -> Self {
        let evidence_id = format!(
            "openspec_current_work:applied_but_unarchived:{}:{}",
            queue_item.queue_id(),
            queue_item.artifact_digest()
        );
        Self {
            blocker_id: format!(
                "blocker_openspec_current_work_applied_but_unarchived_{}",
                queue_item.queue_id()
            ),
            evidence_id: evidence_id.clone(),
            kind: OpenSpecCurrentWorkBlockerKind::AppliedButUnarchived,
            owner: OpenSpecCurrentWorkOwner::OpenSpecArchive,
            allowed_repairs: repair_commands(&[
                "mutai-scheduler orchestrator archive-openspec",
                "mutai-scheduler orchestrator recover open-spec-archive-status",
            ]),
            repair_playbook: OpenSpecCurrentWorkRepairPlaybook::new(
                OpenSpecCurrentWorkRepairAction::ArchiveOpenSpec,
                OpenSpecCurrentWorkRepairSafety::Mutating,
                evidence_id,
                "OpenSpec archive status is archived for the applied queue artifact",
                "retry archive-openspec after materializing the archive blocker or fixing archive preflight",
            ),
            subtask_id: Some(
                SubtaskId::parse(queue_item.subtask_id().to_owned())
                    .expect("loaded queue subtask id is valid"),
            ),
            claim_id: None,
            queue_id: Some(
                QueueId::parse(queue_item.queue_id().to_owned()).expect("loaded queue id is valid"),
            ),
            artifact_digest: Some(
                ArtifactDigest::parse(queue_item.artifact_digest().to_owned())
                    .expect("loaded artifact digest is valid"),
            ),
            review_id: None,
            reason: "applied_but_unarchived".to_owned(),
        }
    }

    /// Builds a blocker for a direct-applied artifact that has no archive status row yet.
    #[must_use]
    pub fn direct_applied_unarchived(subtask: &SubtaskView) -> Self {
        let artifact_digest = subtask
            .artifact_digest()
            .expect("direct applied archive blocker requires artifact digest");
        let evidence_id = format!(
            "openspec_current_work:applied_but_unarchived:{}:{}",
            subtask.subtask_id.as_str(),
            artifact_digest.as_str()
        );
        Self {
            blocker_id: format!(
                "blocker_openspec_current_work_applied_but_unarchived_{}",
                subtask.subtask_id.as_str()
            ),
            evidence_id: evidence_id.clone(),
            kind: OpenSpecCurrentWorkBlockerKind::AppliedButUnarchived,
            owner: OpenSpecCurrentWorkOwner::OpenSpecArchive,
            allowed_repairs: repair_commands(&[
                "mutai-scheduler orchestrator archive-openspec",
                "mutai-scheduler orchestrator recover open-spec-archive-status",
            ]),
            repair_playbook: OpenSpecCurrentWorkRepairPlaybook::new(
                OpenSpecCurrentWorkRepairAction::ArchiveOpenSpec,
                OpenSpecCurrentWorkRepairSafety::Mutating,
                evidence_id,
                "OpenSpec archive status is archived for the direct-applied artifact",
                "retry archive-openspec after materializing the archive blocker or fixing archive preflight",
            ),
            subtask_id: Some(subtask.subtask_id.clone()),
            claim_id: None,
            queue_id: None,
            artifact_digest: Some(artifact_digest.clone()),
            review_id: None,
            reason: "applied_but_unarchived".to_owned(),
        }
    }

    /// Builds a blocker for an applied queue item that lacks a durable landing receipt.
    #[must_use]
    pub fn applied_without_landing_receipt(queue_item: &ReadyQueueItem) -> Self {
        let evidence_id = format!(
            "openspec_current_work:landing_receipt_missing:{}:{}",
            queue_item.queue_id(),
            queue_item.artifact_digest()
        );
        Self {
            blocker_id: format!(
                "blocker_openspec_current_work_landing_receipt_missing_{}",
                queue_item.queue_id()
            ),
            evidence_id: evidence_id.clone(),
            kind: OpenSpecCurrentWorkBlockerKind::GitApplyUncertainty,
            owner: OpenSpecCurrentWorkOwner::ApplyGate,
            allowed_repairs: repair_commands(&[
                "mutai-scheduler orchestrator current-work",
                "mutai-scheduler orchestrator recover landing-receipt",
            ]),
            repair_playbook: OpenSpecCurrentWorkRepairPlaybook::new(
                OpenSpecCurrentWorkRepairAction::RecoverLandingReceipt,
                OpenSpecCurrentWorkRepairSafety::Mutating,
                evidence_id,
                "A durable landing receipt exists for the applied queue artifact and fence",
                "fail closed unless accepted local landing authorization and landed commit evidence are derivable",
            ),
            subtask_id: Some(
                SubtaskId::parse(queue_item.subtask_id().to_owned())
                    .expect("loaded queue subtask id is valid"),
            ),
            claim_id: None,
            queue_id: Some(
                QueueId::parse(queue_item.queue_id().to_owned()).expect("loaded queue id is valid"),
            ),
            artifact_digest: Some(
                ArtifactDigest::parse(queue_item.artifact_digest().to_owned())
                    .expect("loaded artifact digest is valid"),
            ),
            review_id: None,
            reason: "landing_receipt_missing".to_owned(),
        }
    }

    /// Builds a blocker from native apply-gate evidence.
    #[must_use]
    pub fn apply_gate_blocked(
        blocker: &ApplyGateBlocker,
        queue_item: Option<&ReadyQueueItem>,
    ) -> Self {
        let (kind, owner) = match blocker.blocker_kind {
            ApplyGateBlockerKind::AuthorityHold => (
                OpenSpecCurrentWorkBlockerKind::AuthorityHold,
                OpenSpecCurrentWorkOwner::Authority,
            ),
            ApplyGateBlockerKind::GitApplyUncertainty => (
                OpenSpecCurrentWorkBlockerKind::GitApplyUncertainty,
                OpenSpecCurrentWorkOwner::ApplyGate,
            ),
        };
        let repair_action = match kind {
            OpenSpecCurrentWorkBlockerKind::AuthorityHold
            | OpenSpecCurrentWorkBlockerKind::GitApplyUncertainty => {
                OpenSpecCurrentWorkRepairAction::FailClosed
            }
            _ => OpenSpecCurrentWorkRepairAction::FailClosed,
        };
        Self {
            blocker_id: format!(
                "blocker_openspec_current_work_apply_gate_{}_{}",
                blocker.queue_id.as_str(),
                typed_ref_fragment(blocker.evidence_id.as_str())
            ),
            evidence_id: blocker.evidence_id.as_str().to_owned(),
            kind,
            owner,
            allowed_repairs: repair_commands(&[
                "mutai-scheduler orchestrator current-work",
                "mutai-scheduler orchestrator recover operator-blocked",
            ]),
            repair_playbook: OpenSpecCurrentWorkRepairPlaybook::new(
                repair_action,
                OpenSpecCurrentWorkRepairSafety::Safe,
                blocker.evidence_id.as_str(),
                "Authority or apply-gate evidence is supplied externally and the blocker is absent from current-work",
                "do not synthesize Authority/operator evidence; rerun after authoritative reconcile evidence exists",
            ),
            subtask_id: queue_item.map(|item| {
                SubtaskId::parse(item.subtask_id().to_owned()).expect("loaded subtask id is valid")
            }),
            claim_id: None,
            queue_id: Some(blocker.queue_id.clone()),
            artifact_digest: Some(blocker.artifact_digest.clone()),
            review_id: Some(blocker.review_id.clone()),
            reason: blocker.reason.as_str().to_owned(),
        }
    }

    /// Builds a blocker from native Authority settlement reconcile evidence.
    #[must_use]
    pub fn settlement_reconcile_blocked(
        blocker: &SettlementReconcileBlocker,
        queue_item: Option<&ReadyQueueItem>,
    ) -> Self {
        let (kind, owner) = match blocker.reconcile_reason {
            SettlementReconcileReason::AuthorityLost | SettlementReconcileReason::StaleFence => (
                OpenSpecCurrentWorkBlockerKind::AuthorityHold,
                OpenSpecCurrentWorkOwner::Authority,
            ),
            SettlementReconcileReason::CommitUnknown
            | SettlementReconcileReason::PartialPrepare
            | SettlementReconcileReason::PartialFinalize
            | SettlementReconcileReason::FailedCanonicalApply
            | SettlementReconcileReason::DuplicateCompletion => (
                OpenSpecCurrentWorkBlockerKind::GitApplyUncertainty,
                OpenSpecCurrentWorkOwner::ApplyGate,
            ),
        };
        let repair_action = match kind {
            OpenSpecCurrentWorkBlockerKind::AuthorityHold
            | OpenSpecCurrentWorkBlockerKind::GitApplyUncertainty => {
                OpenSpecCurrentWorkRepairAction::FailClosed
            }
            _ => OpenSpecCurrentWorkRepairAction::FailClosed,
        };
        Self {
            blocker_id: format!(
                "blocker_openspec_current_work_settlement_reconcile_{}_{}",
                blocker.queue_id.as_str(),
                typed_ref_fragment(blocker.authority_evidence_id.as_str())
            ),
            evidence_id: blocker.authority_evidence_id.as_str().to_owned(),
            kind,
            owner,
            allowed_repairs: repair_commands(&[
                "mutai-scheduler orchestrator current-work",
                "mutai-scheduler orchestrator recover operator-blocked",
            ]),
            repair_playbook: OpenSpecCurrentWorkRepairPlaybook::new(
                repair_action,
                OpenSpecCurrentWorkRepairSafety::Safe,
                blocker.authority_evidence_id.as_str(),
                "Settlement reconcile evidence is supplied externally and the blocker is absent from current-work",
                "do not synthesize Authority/operator evidence; rerun after authoritative reconcile evidence exists",
            ),
            subtask_id: queue_item.map(|item| {
                SubtaskId::parse(item.subtask_id().to_owned()).expect("loaded subtask id is valid")
            }),
            claim_id: None,
            queue_id: Some(blocker.queue_id.clone()),
            artifact_digest: Some(blocker.artifact_digest.clone()),
            review_id: Some(blocker.review_id.clone()),
            reason: blocker.reconcile_reason.to_string(),
        }
    }

    /// Builds a blocker for a terminal blocked or changes-requested subtask.
    #[must_use]
    pub fn subtask_blocked(subtask: &SubtaskView) -> Self {
        let evidence_id = format!(
            "openspec_current_work:subtask_blocked:{}:{}",
            subtask.subtask_id.as_str(),
            subtask.state()
        );
        Self {
            blocker_id: format!(
                "blocker_openspec_current_work_subtask_blocked_{}",
                subtask.subtask_id.as_str()
            ),
            evidence_id: evidence_id.clone(),
            kind: OpenSpecCurrentWorkBlockerKind::SubtaskBlocked,
            owner: OpenSpecCurrentWorkOwner::Executor,
            allowed_repairs: repair_commands(&[
                "mutai-scheduler orchestrator recover subtask",
                "mutai-scheduler orchestrator recover redispatch",
            ]),
            repair_playbook: OpenSpecCurrentWorkRepairPlaybook::new(
                OpenSpecCurrentWorkRepairAction::RecoverSubtask,
                OpenSpecCurrentWorkRepairSafety::Mutating,
                evidence_id,
                "The subtask is no longer blocked without a repair follow-up or explicit operator blocker",
                "retry after creating or resolving the repair follow-up; do not abandon unrelated subtasks",
            ),
            subtask_id: Some(subtask.subtask_id.clone()),
            claim_id: subtask.active_claim_id().cloned(),
            queue_id: None,
            artifact_digest: subtask.artifact_digest().cloned(),
            review_id: None,
            reason: format!("subtask state is {}", subtask.state()),
        }
    }

    /// Builds a blocker for a scoped held claim whose lease has expired.
    #[must_use]
    pub fn expired_claim(claim: &Claim, lease_now_ms: i64) -> Self {
        let evidence_id = format!(
            "openspec_current_work:expired_claim:{}:{}",
            claim.subtask_id.as_str(),
            claim.claim_id.as_str()
        );
        Self {
            blocker_id: format!(
                "blocker_openspec_current_work_expired_claim_{}",
                claim.claim_id.as_str()
            ),
            evidence_id: evidence_id.clone(),
            kind: OpenSpecCurrentWorkBlockerKind::ExpiredClaim,
            owner: OpenSpecCurrentWorkOwner::Covey,
            allowed_repairs: repair_commands(&[
                "mutai-scheduler orchestrator recover expired-claim",
                "mutai-scheduler orchestrator recover redispatch",
            ]),
            repair_playbook: OpenSpecCurrentWorkRepairPlaybook::new(
                OpenSpecCurrentWorkRepairAction::RecoverExpiredClaim,
                OpenSpecCurrentWorkRepairSafety::Mutating,
                evidence_id,
                "The expired claim is released or no longer attached to the scoped subtask",
                "retry only for the same claim id; do not downgrade to unlocked work",
            ),
            subtask_id: Some(claim.subtask_id.clone()),
            claim_id: Some(claim.claim_id.clone()),
            queue_id: None,
            artifact_digest: None,
            review_id: None,
            reason: format!(
                "claim {} lease_deadline {} is at or before lease clock {}",
                claim.claim_id.as_str(),
                claim.lease_deadline.get(),
                lease_now_ms
            ),
        }
    }

    /// Builds a blocker for a scoped held claim that has exceeded the caller's
    /// explicit current-work idleness threshold but has not expired.
    #[must_use]
    pub fn stale_claim(stale: &OpenSpecCurrentWorkStaleClaim) -> Self {
        let evidence_id = format!(
            "openspec_current_work:stale_claim:{}:{}:{}",
            stale.claim.subtask_id.as_str(),
            stale.claim.claim_id.as_str(),
            stale.threshold_ms
        );
        Self {
            blocker_id: format!(
                "blocker_openspec_current_work_stale_claim_{}",
                stale.claim.claim_id.as_str()
            ),
            evidence_id: evidence_id.clone(),
            kind: OpenSpecCurrentWorkBlockerKind::StaleClaim,
            owner: OpenSpecCurrentWorkOwner::Covey,
            allowed_repairs: repair_commands(&[
                "mutai-scheduler orchestrator recover dead-claim",
                "mutai-scheduler orchestrator recover operator-blocked",
            ]),
            repair_playbook: OpenSpecCurrentWorkRepairPlaybook::new(
                OpenSpecCurrentWorkRepairAction::RecoverDeadClaim,
                OpenSpecCurrentWorkRepairSafety::Mutating,
                evidence_id,
                "The stale scheduler-owned claim is released only when pane/artifact safety checks pass",
                "if safety checks fail, leave the blocker open with evidence and retry after operator inspection",
            ),
            subtask_id: Some(stale.claim.subtask_id.clone()),
            claim_id: Some(stale.claim.claim_id.clone()),
            queue_id: None,
            artifact_digest: None,
            review_id: None,
            reason: format!(
                "claim {} has been idle for {}ms, meeting stale threshold {}ms",
                stale.claim.claim_id.as_str(),
                stale.idle_for_ms,
                stale.threshold_ms
            ),
        }
    }

    /// Builds a blocker for a registered scheduler workspace/cache that cannot
    /// safely be treated as an invisible execution detail.
    #[must_use]
    pub fn vcs_workspace_unusable(workspace: &VcsWorkspace) -> Self {
        let evidence_id = format!(
            "openspec_current_work:vcs_workspace_unusable:{}:{}",
            workspace.workspace_id.as_str(),
            workspace.last_cleanliness
        );
        let mut repair_playbook = OpenSpecCurrentWorkRepairPlaybook::new(
            OpenSpecCurrentWorkRepairAction::RecoverWorkspace,
            OpenSpecCurrentWorkRepairSafety::Mutating,
            evidence_id.clone(),
            "The registered VCS workspace is clean, recovered, archived, or no longer blocks current work",
            "do not inspect or mutate unregistered filesystem paths; rerun current-work after scheduler repair or janitor evidence exists",
        );
        repair_playbook.repair_command =
            Some("mutai-scheduler orchestrator current-work".to_owned());
        Self {
            blocker_id: format!(
                "blocker_openspec_current_work_vcs_workspace_unusable_{}",
                workspace.workspace_id.as_str()
            ),
            evidence_id,
            kind: OpenSpecCurrentWorkBlockerKind::SchedulerStateLoss,
            owner: OpenSpecCurrentWorkOwner::Operator,
            allowed_repairs: repair_commands(&[
                "mutai-scheduler orchestrator current-work",
                "mutai-scheduler orchestrator run-openspec",
                "mutai-scheduler orchestrator recover workspace",
                "mutai-scheduler orchestrator recover dead-claim",
                "mutai-scheduler orchestrator recover redispatch",
                "mutai-scheduler janitor vcs-workspaces",
            ]),
            repair_playbook,
            subtask_id: workspace.subtask_id.clone(),
            claim_id: workspace.claim_id.clone(),
            queue_id: workspace.queue_id.clone(),
            artifact_digest: workspace.artifact_digest.clone(),
            review_id: None,
            reason: format!(
                "registered {:?} VCS workspace {} at {} is {:?}/{:?}; bookmark={:?} change={:?} commit={:?}",
                workspace.kind,
                workspace.workspace_id.as_str(),
                workspace.path.as_str(),
                workspace.state,
                workspace.last_cleanliness,
                workspace
                    .current_bookmark
                    .as_ref()
                    .map(|value| value.as_str()),
                workspace
                    .current_change_id
                    .as_ref()
                    .map(|value| value.as_str()),
                workspace
                    .current_commit_id
                    .as_ref()
                    .map(|value| value.as_str())
            ),
        }
    }

    /// Builds a blocker from a durable operator-blocker row.
    #[must_use]
    pub fn operator_blocked(blocker: &OperatorBlocker) -> Self {
        let kind = operator_blocker_kind(blocker.reason.as_str());
        let evidence_id = blocker.source_evidence_id.as_ref().map_or_else(
            || {
                format!(
                    "openspec_current_work:operator_blocked:{}",
                    blocker.blocker_id.as_str()
                )
            },
            |evidence| evidence.as_str().to_owned(),
        );
        let repair_action = operator_blocker_repair_action(kind, blocker.queue_id.is_some());
        let repair_safety = match repair_action {
            OpenSpecCurrentWorkRepairAction::FailClosed => OpenSpecCurrentWorkRepairSafety::Safe,
            _ => OpenSpecCurrentWorkRepairSafety::Mutating,
        };
        Self {
            blocker_id: format!(
                "blocker_openspec_current_work_operator_blocked_{}",
                blocker.blocker_id.as_str()
            ),
            evidence_id: evidence_id.clone(),
            kind,
            owner: OpenSpecCurrentWorkOwner::Operator,
            allowed_repairs: operator_blocker_repairs(kind),
            repair_playbook: OpenSpecCurrentWorkRepairPlaybook::new(
                repair_action,
                repair_safety,
                evidence_id,
                operator_blocker_expected_postcondition(kind),
                operator_blocker_rollback_note(kind),
            ),
            subtask_id: Some(blocker.subtask_id.clone()),
            claim_id: None,
            queue_id: blocker.queue_id.clone(),
            artifact_digest: blocker.artifact_digest.clone(),
            review_id: None,
            reason: blocker.reason.as_str().to_owned(),
        }
    }
}

/// Resolved blocker lookup result.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSpecCurrentWorkBlockerResolution {
    pub openspec_change_id: OpenSpecChangeId,
    pub current_work: OpenSpecCurrentWork,
    pub blocker: OpenSpecCurrentWorkBlocker,
}

fn operator_blocker_kind(reason: &str) -> OpenSpecCurrentWorkBlockerKind {
    match reason {
        "scheduler_state_loss"
        | "assignment_pane_missing"
        | "missing_assignments_json"
        | "unreadable_assignments_json"
        | "execution_workspace_unusable" => OpenSpecCurrentWorkBlockerKind::SchedulerStateLoss,
        "stale_claim" => OpenSpecCurrentWorkBlockerKind::StaleClaim,
        "hook_state_stale_claim" | "hook_state_stale_claim_context" => {
            OpenSpecCurrentWorkBlockerKind::HookStateStaleClaim
        }
        "hook_state_stale_landing_authorization"
        | "hook_state_stale_landing_authorization_context" => {
            OpenSpecCurrentWorkBlockerKind::HookStateStaleLandingAuthorization
        }
        "hook_state_invalid_landing_authorization"
        | "hook_state_invalid_landing_authorization_context" => {
            OpenSpecCurrentWorkBlockerKind::HookStateInvalidLandingAuthorization
        }
        "authority_hold" | "authority_denied" | "authority_lost" => {
            OpenSpecCurrentWorkBlockerKind::AuthorityHold
        }
        "git_apply_uncertainty" | "commit_unknown" | "landing_unknown" => {
            OpenSpecCurrentWorkBlockerKind::GitApplyUncertainty
        }
        _ => OpenSpecCurrentWorkBlockerKind::OperatorBlocked,
    }
}

fn operator_blocker_repairs(kind: OpenSpecCurrentWorkBlockerKind) -> Vec<String> {
    let commands = match kind {
        OpenSpecCurrentWorkBlockerKind::SchedulerStateLoss => &[
            "mutai-scheduler orchestrator current-work",
            "mutai-scheduler orchestrator recover operator-blocked",
            "mutai-scheduler orchestrator recover resolve-operator-blocker",
        ][..],
        OpenSpecCurrentWorkBlockerKind::HookStateStaleClaim
        | OpenSpecCurrentWorkBlockerKind::HookStateStaleLandingAuthorization
        | OpenSpecCurrentWorkBlockerKind::HookStateInvalidLandingAuthorization => &[
            "mutai-scheduler orchestrator current-work",
            "mutai-scheduler orchestrator recover operator-blocked",
            "mutai-scheduler orchestrator recover resolve-operator-blocker",
        ],
        OpenSpecCurrentWorkBlockerKind::AuthorityHold
        | OpenSpecCurrentWorkBlockerKind::GitApplyUncertainty => &[
            "mutai-scheduler orchestrator current-work",
            "mutai-scheduler orchestrator recover operator-blocked",
            "mutai-scheduler orchestrator recover resolve-operator-blocker",
        ],
        OpenSpecCurrentWorkBlockerKind::OperatorBlocked => &[
            "mutai-scheduler orchestrator recover operator-blocked",
            "mutai-scheduler orchestrator recover resolve-operator-blocker",
        ],
        OpenSpecCurrentWorkBlockerKind::MissingImport
        | OpenSpecCurrentWorkBlockerKind::AppliedButUnarchived
        | OpenSpecCurrentWorkBlockerKind::SubtaskBlocked
        | OpenSpecCurrentWorkBlockerKind::ExpiredClaim
        | OpenSpecCurrentWorkBlockerKind::StaleClaim => {
            &["mutai-scheduler orchestrator recover operator-blocked"]
        }
    };
    repair_commands(commands)
}

fn operator_blocker_repair_action(
    kind: OpenSpecCurrentWorkBlockerKind,
    has_queue_target: bool,
) -> OpenSpecCurrentWorkRepairAction {
    match kind {
        OpenSpecCurrentWorkBlockerKind::SchedulerStateLoss => {
            if has_queue_target {
                OpenSpecCurrentWorkRepairAction::RecoverQueue
            } else {
                OpenSpecCurrentWorkRepairAction::RecoverSubtask
            }
        }
        OpenSpecCurrentWorkBlockerKind::StaleClaim => {
            OpenSpecCurrentWorkRepairAction::RecoverDeadClaim
        }
        OpenSpecCurrentWorkBlockerKind::HookStateStaleClaim
        | OpenSpecCurrentWorkBlockerKind::HookStateStaleLandingAuthorization
        | OpenSpecCurrentWorkBlockerKind::HookStateInvalidLandingAuthorization
        | OpenSpecCurrentWorkBlockerKind::AuthorityHold
        | OpenSpecCurrentWorkBlockerKind::GitApplyUncertainty
        | OpenSpecCurrentWorkBlockerKind::OperatorBlocked => {
            OpenSpecCurrentWorkRepairAction::FailClosed
        }
        OpenSpecCurrentWorkBlockerKind::MissingImport => {
            OpenSpecCurrentWorkRepairAction::RunOpenSpec
        }
        OpenSpecCurrentWorkBlockerKind::AppliedButUnarchived => {
            OpenSpecCurrentWorkRepairAction::ArchiveOpenSpec
        }
        OpenSpecCurrentWorkBlockerKind::SubtaskBlocked => {
            OpenSpecCurrentWorkRepairAction::RecoverSubtask
        }
        OpenSpecCurrentWorkBlockerKind::ExpiredClaim => {
            OpenSpecCurrentWorkRepairAction::RecoverExpiredClaim
        }
    }
}

fn operator_blocker_expected_postcondition(kind: OpenSpecCurrentWorkBlockerKind) -> &'static str {
    match kind {
        OpenSpecCurrentWorkBlockerKind::SchedulerStateLoss => {
            "The named scheduler target has been reconciled and the durable operator blocker is resolved"
        }
        OpenSpecCurrentWorkBlockerKind::StaleClaim => {
            "The stale scheduler-owned claim is released only when pane/artifact safety checks pass"
        }
        OpenSpecCurrentWorkBlockerKind::HookStateStaleClaim
        | OpenSpecCurrentWorkBlockerKind::HookStateStaleLandingAuthorization
        | OpenSpecCurrentWorkBlockerKind::HookStateInvalidLandingAuthorization => {
            "Hook-local state no longer contradicts live Covey current-work facts"
        }
        OpenSpecCurrentWorkBlockerKind::AuthorityHold
        | OpenSpecCurrentWorkBlockerKind::GitApplyUncertainty => {
            "Authoritative reconcile evidence exists and current-work no longer reports the blocker"
        }
        _ => {
            "The durable operator blocker is resolved only after its typed postcondition is externally satisfied"
        }
    }
}

fn operator_blocker_rollback_note(kind: OpenSpecCurrentWorkBlockerKind) -> &'static str {
    match kind {
        OpenSpecCurrentWorkBlockerKind::SchedulerStateLoss => {
            "retry the bounded target recovery; leave the durable blocker open if the target still lacks evidence"
        }
        OpenSpecCurrentWorkBlockerKind::StaleClaim => {
            "if pane/artifact checks fail, keep the blocker open and rerun current-work after inspection"
        }
        OpenSpecCurrentWorkBlockerKind::AuthorityHold
        | OpenSpecCurrentWorkBlockerKind::GitApplyUncertainty
        | OpenSpecCurrentWorkBlockerKind::OperatorBlocked => {
            "fail closed without mutation; do not synthesize missing operator or Authority evidence"
        }
        _ => "fail closed without broad maintenance fallback",
    }
}

fn repair_commands(commands: &[&str]) -> Vec<String> {
    commands
        .iter()
        .map(|command| (*command).to_owned())
        .collect()
}

fn typed_ref_fragment(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

/// Covey-first current-work projection for one OpenSpec work packet.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenSpecCurrentWork {
    pub openspec_change_id: OpenSpecChangeId,
    pub state: OpenSpecCurrentWorkState,
    pub next_owner: OpenSpecCurrentWorkOwner,
    pub subtask_ids: Vec<SubtaskId>,
    pub claim_ids: Vec<ClaimId>,
    pub queue_ids: Vec<QueueId>,
    pub artifact_digests: Vec<ArtifactDigest>,
    pub review_ids: Vec<ReviewId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vcs_workspaces: Vec<VcsWorkspace>,
    pub archive_blockers: Vec<OpenSpecArchiveStatus>,
    pub blockers: Vec<OpenSpecCurrentWorkBlocker>,
}

impl OpenSpecCurrentWork {
    /// Builds the current-work projection from Covey-owned lifecycle facts.
    #[must_use]
    pub fn from_parts(
        openspec_change_id: OpenSpecChangeId,
        subtasks: Vec<SubtaskView>,
        reviews: Vec<Review>,
        queue_items: Vec<ReadyQueueItem>,
        archive_statuses: Vec<OpenSpecArchiveStatus>,
        landing_receipt_queue_ids: Vec<QueueId>,
        apply_gate_blockers: Vec<ApplyGateBlocker>,
        settlement_reconcile_blockers: Vec<SettlementReconcileBlocker>,
        operator_blockers: Vec<OperatorBlocker>,
        active_claims: Vec<Claim>,
        repaired_source_subtask_ids: Vec<SubtaskId>,
        stale_claims: Vec<OpenSpecCurrentWorkStaleClaim>,
        vcs_workspaces: Vec<VcsWorkspace>,
        lease_now_ms: i64,
    ) -> Self {
        let open_archive_blockers = archive_statuses
            .iter()
            .filter(|status| status.state == super::OpenSpecArchiveStatusState::Blocked)
            .cloned()
            .collect::<Vec<_>>();
        let blockers = current_work_blockers(
            &openspec_change_id,
            &subtasks,
            &queue_items,
            &archive_statuses,
            &landing_receipt_queue_ids,
            &open_archive_blockers,
            &apply_gate_blockers,
            &settlement_reconcile_blockers,
            &operator_blockers,
            &active_claims,
            &repaired_source_subtask_ids,
            &stale_claims,
            &vcs_workspaces,
            lease_now_ms,
        );
        let state = current_work_state(
            &subtasks,
            &reviews,
            &queue_items,
            &archive_statuses,
            &repaired_source_subtask_ids,
            &blockers,
        );
        let next_owner = current_work_next_owner(state, &blockers);
        Self {
            openspec_change_id,
            state,
            next_owner,
            subtask_ids: subtasks
                .iter()
                .map(|subtask| subtask.subtask_id.clone())
                .collect(),
            claim_ids: subtasks
                .iter()
                .filter_map(|subtask| subtask.active_claim_id().cloned())
                .collect(),
            queue_ids: queue_items
                .iter()
                .map(|item| {
                    QueueId::parse(item.queue_id().to_owned()).expect("loaded queue id is valid")
                })
                .collect(),
            artifact_digests: current_work_artifact_digests(
                &subtasks,
                &queue_items,
                &archive_statuses,
            ),
            review_ids: reviews
                .iter()
                .map(|review| {
                    ReviewId::parse(review.review_id().to_owned())
                        .expect("loaded review id is valid")
                })
                .collect(),
            vcs_workspaces,
            archive_blockers: open_archive_blockers,
            blockers,
        }
    }
}

fn current_work_state(
    subtasks: &[SubtaskView],
    reviews: &[Review],
    queue_items: &[ReadyQueueItem],
    archive_statuses: &[OpenSpecArchiveStatus],
    repaired_source_subtask_ids: &[SubtaskId],
    blockers: &[OpenSpecCurrentWorkBlocker],
) -> OpenSpecCurrentWorkState {
    if !blockers.is_empty() {
        return OpenSpecCurrentWorkState::Blocked;
    }
    if !subtasks.is_empty()
        && subtasks.iter().all(|subtask| {
            subtask.state() == SubtaskState::Applied
                || repaired_source_subtask_ids
                    .iter()
                    .any(|repaired| repaired == &subtask.subtask_id)
        })
        && archive_statuses
            .iter()
            .any(|status| status.state == super::OpenSpecArchiveStatusState::Archived)
        && archived_status_count(archive_statuses) >= archive_target_count(subtasks, queue_items)
    {
        return OpenSpecCurrentWorkState::Archived;
    }
    if queue_items.iter().any(|item| {
        matches!(
            item.state(),
            ReadyQueueState::Queued | ReadyQueueState::InFlight
        )
    }) || subtasks.iter().any(|subtask| {
        matches!(
            subtask.state(),
            SubtaskState::Approved | SubtaskState::ReadyForApply
        )
    }) {
        return OpenSpecCurrentWorkState::Applying;
    }
    if reviews.iter().any(|review| {
        matches!(
            review.state(),
            ReviewState::Requested | ReviewState::InProgress
        )
    }) || subtasks.iter().any(|subtask| {
        matches!(
            subtask.state(),
            SubtaskState::ArtifactPublished | SubtaskState::ReviewPending
        )
    }) {
        return OpenSpecCurrentWorkState::Reviewing;
    }
    if subtasks.iter().any(|subtask| {
        subtask.active_claim_id().is_some()
            || matches!(
                subtask.state(),
                SubtaskState::Claimed | SubtaskState::InProgress
            )
    }) {
        return OpenSpecCurrentWorkState::Claimed;
    }
    OpenSpecCurrentWorkState::Imported
}

fn current_work_next_owner(
    state: OpenSpecCurrentWorkState,
    blockers: &[OpenSpecCurrentWorkBlocker],
) -> OpenSpecCurrentWorkOwner {
    if let Some(blocker) = blockers.first() {
        return blocker.owner;
    }
    match state {
        OpenSpecCurrentWorkState::Imported => OpenSpecCurrentWorkOwner::Executor,
        OpenSpecCurrentWorkState::Claimed => OpenSpecCurrentWorkOwner::Executor,
        OpenSpecCurrentWorkState::Reviewing => OpenSpecCurrentWorkOwner::Reviewer,
        OpenSpecCurrentWorkState::Applying => OpenSpecCurrentWorkOwner::ApplyGate,
        OpenSpecCurrentWorkState::Archived => OpenSpecCurrentWorkOwner::Operator,
        OpenSpecCurrentWorkState::Blocked => OpenSpecCurrentWorkOwner::Operator,
    }
}

fn current_work_blockers(
    openspec_change_id: &OpenSpecChangeId,
    subtasks: &[SubtaskView],
    queue_items: &[ReadyQueueItem],
    archive_statuses: &[OpenSpecArchiveStatus],
    landing_receipt_queue_ids: &[QueueId],
    open_archive_blockers: &[OpenSpecArchiveStatus],
    apply_gate_blockers: &[ApplyGateBlocker],
    settlement_reconcile_blockers: &[SettlementReconcileBlocker],
    operator_blockers: &[OperatorBlocker],
    active_claims: &[Claim],
    repaired_source_subtask_ids: &[SubtaskId],
    stale_claims: &[OpenSpecCurrentWorkStaleClaim],
    vcs_workspaces: &[VcsWorkspace],
    lease_now_ms: i64,
) -> Vec<OpenSpecCurrentWorkBlocker> {
    if subtasks.is_empty() {
        return vec![OpenSpecCurrentWorkBlocker::missing_import(
            openspec_change_id,
        )];
    }
    let all_scoped_subtasks_terminal = subtasks.iter().all(|subtask| {
        subtask.state() == SubtaskState::Applied
            || repaired_source_subtask_ids
                .iter()
                .any(|repaired| repaired == &subtask.subtask_id)
    });
    let mut blockers = Vec::new();
    if all_scoped_subtasks_terminal {
        blockers.extend(
            queue_items
                .iter()
                .filter(|item| item.state() == ReadyQueueState::Applied)
                .filter(|item| {
                    !landing_receipt_queue_ids
                        .iter()
                        .any(|queue_id| queue_id.as_str() == item.queue_id())
                })
                .map(OpenSpecCurrentWorkBlocker::applied_without_landing_receipt),
        );
        blockers.extend(
            open_archive_blockers
                .iter()
                .map(OpenSpecCurrentWorkBlocker::applied_but_unarchived),
        );
    }
    blockers.extend(apply_gate_blockers.iter().map(|blocker| {
        OpenSpecCurrentWorkBlocker::apply_gate_blocked(
            blocker,
            queue_item_by_id(queue_items, &blocker.queue_id),
        )
    }));
    blockers.extend(settlement_reconcile_blockers.iter().map(|blocker| {
        OpenSpecCurrentWorkBlocker::settlement_reconcile_blocked(
            blocker,
            queue_item_by_id(queue_items, &blocker.queue_id),
        )
    }));
    if all_scoped_subtasks_terminal {
        blockers.extend(
            queue_items
                .iter()
                .filter(|item| item.state() == ReadyQueueState::Applied)
                .filter(|item| {
                    !archive_statuses
                        .iter()
                        .any(|status| status.queue_id.as_str() == item.queue_id())
                })
                .map(OpenSpecCurrentWorkBlocker::applied_queue_unarchived),
        );
        blockers.extend(
            subtasks
                .iter()
                .filter(|subtask| subtask.state() == SubtaskState::Applied)
                .filter(|subtask| {
                    let Some(artifact_digest) = subtask.artifact_digest() else {
                        return false;
                    };
                    !queue_items.iter().any(|item| {
                        item.subtask_id() == subtask.subtask_id.as_str()
                            && item.artifact_digest() == artifact_digest.as_str()
                    }) && !archive_statuses.iter().any(|status| {
                        status.subtask_id == subtask.subtask_id
                            && status.artifact_digest == *artifact_digest
                    })
                })
                .map(OpenSpecCurrentWorkBlocker::direct_applied_unarchived),
        );
    }
    blockers.extend(subtasks.iter().filter_map(|subtask| {
        (matches!(
            subtask.state(),
            SubtaskState::Blocked | SubtaskState::ChangesRequested | SubtaskState::Abandoned
        ) && !repaired_source_subtask_ids
            .iter()
            .any(|repaired| repaired == &subtask.subtask_id))
        .then(|| OpenSpecCurrentWorkBlocker::subtask_blocked(subtask))
    }));
    blockers.extend(
        operator_blockers
            .iter()
            .map(OpenSpecCurrentWorkBlocker::operator_blocked),
    );
    blockers.extend(
        active_claims
            .iter()
            .filter(|claim| claim.lease_deadline.get() <= lease_now_ms)
            .map(|claim| OpenSpecCurrentWorkBlocker::expired_claim(claim, lease_now_ms)),
    );
    let stale_blockers = stale_claims
        .iter()
        .filter(|stale| {
            !blockers.iter().any(|blocker| {
                blocker.claim_id.as_ref() == Some(&stale.claim.claim_id)
                    && blocker.kind == OpenSpecCurrentWorkBlockerKind::ExpiredClaim
            })
        })
        .map(OpenSpecCurrentWorkBlocker::stale_claim)
        .collect::<Vec<_>>();
    blockers.extend(stale_blockers);
    blockers.extend(vcs_workspaces.iter().filter_map(|workspace| {
        (workspace.state == VcsWorkspaceState::Active
            && matches!(
                workspace.last_cleanliness,
                VcsWorkspaceCleanliness::Dirty
                    | VcsWorkspaceCleanliness::Missing
                    | VcsWorkspaceCleanliness::Stale
                    | VcsWorkspaceCleanliness::Unusable
            ))
        .then(|| OpenSpecCurrentWorkBlocker::vcs_workspace_unusable(workspace))
    }));
    blockers
}

fn archived_status_count(archive_statuses: &[OpenSpecArchiveStatus]) -> usize {
    archive_statuses
        .iter()
        .filter(|status| status.state == super::OpenSpecArchiveStatusState::Archived)
        .count()
}

fn archive_target_count(subtasks: &[SubtaskView], queue_items: &[ReadyQueueItem]) -> usize {
    let applied_queue_targets = queue_items
        .iter()
        .filter(|item| item.state() == ReadyQueueState::Applied)
        .count();
    let direct_applied_targets = subtasks
        .iter()
        .filter(|subtask| subtask.state() == SubtaskState::Applied)
        .filter(|subtask| {
            let Some(artifact_digest) = subtask.artifact_digest() else {
                return false;
            };
            !queue_items.iter().any(|item| {
                item.subtask_id() == subtask.subtask_id.as_str()
                    && item.artifact_digest() == artifact_digest.as_str()
            })
        })
        .count();
    applied_queue_targets + direct_applied_targets
}

fn queue_item_by_id<'a>(
    queue_items: &'a [ReadyQueueItem],
    queue_id: &QueueId,
) -> Option<&'a ReadyQueueItem> {
    queue_items
        .iter()
        .find(|item| item.queue_id() == queue_id.as_str())
}

fn current_work_artifact_digests(
    subtasks: &[SubtaskView],
    queue_items: &[ReadyQueueItem],
    archive_statuses: &[OpenSpecArchiveStatus],
) -> Vec<ArtifactDigest> {
    let mut digests = subtasks
        .iter()
        .filter_map(|subtask| subtask.artifact_digest().cloned())
        .chain(queue_items.iter().map(|item| {
            ArtifactDigest::parse(item.artifact_digest().to_owned())
                .expect("loaded artifact digest is valid")
        }))
        .chain(
            archive_statuses
                .iter()
                .map(|status| status.artifact_digest.clone()),
        )
        .collect::<Vec<_>>();
    digests.sort_unstable();
    digests.dedup();
    digests
}

/// Observability row for a subtask that has not moved recently enough to merit attention.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StuckSubtask {
    subtask: SubtaskView,
    attachment: StuckSubtaskAttachment,
    idle_for_ms: StuckIdleDurationMs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StuckSubtaskAttachment {
    Unclaimed,
    Claimed { claim: Claim, session: Session },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawStuckSubtask {
    subtask: SubtaskView,
    claim: Option<Claim>,
    session: Option<Session>,
    idle_for_ms: i64,
}

/// Observability row for a held claim whose lease deadline is approaching.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpiringClaim {
    claim: Claim,
    subtask: SubtaskView,
    session: Session,
    expires_in_ms: ClaimExpiresInDurationMs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawExpiringClaim {
    claim: Claim,
    subtask: SubtaskView,
    session: Session,
    expires_in_ms: i64,
}

macro_rules! non_negative_duration_newtype {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(try_from = "i64", into = "i64")]
        struct $name(i64);

        impl $name {
            fn parse(value: i64) -> Result<Self, String> {
                Self::try_from(value)
            }

            const fn get(self) -> i64 {
                self.0
            }
        }

        impl TryFrom<i64> for $name {
            type Error = String;

            fn try_from(value: i64) -> Result<Self, Self::Error> {
                if value < 0 {
                    Err(format!("{} must not be negative", $field))
                } else {
                    Ok(Self(value))
                }
            }
        }

        impl From<$name> for i64 {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

non_negative_duration_newtype!(StuckIdleDurationMs, "idle_for_ms");
non_negative_duration_newtype!(ClaimExpiresInDurationMs, "expires_in_ms");

impl StuckSubtask {
    /// Builds a stuck-subtask observability row.
    ///
    /// # Errors
    ///
    /// Returns an error when `idle_for_ms` is negative, or when the attached
    /// claim/session rows do not match the subtask.
    pub fn new(
        subtask: SubtaskView,
        claim: Option<Claim>,
        session: Option<Session>,
        idle_for_ms: i64,
    ) -> Result<Self, String> {
        let idle_for_ms = StuckIdleDurationMs::parse(idle_for_ms)?;
        let attachment = StuckSubtaskAttachment::from_parts(&subtask, claim, session)?;
        Ok(Self {
            subtask,
            attachment,
            idle_for_ms,
        })
    }

    /// Returns the idle subtask view.
    #[must_use]
    pub const fn subtask(&self) -> &SubtaskView {
        &self.subtask
    }

    /// Returns the active claim attached to the subtask, when present.
    #[must_use]
    pub const fn claim(&self) -> Option<&Claim> {
        self.attachment.claim()
    }

    /// Returns the claim owner session, when a claim is attached.
    #[must_use]
    pub const fn session(&self) -> Option<&Session> {
        self.attachment.session()
    }

    /// Returns how long the subtask has been idle.
    #[must_use]
    pub const fn idle_for_ms(&self) -> i64 {
        self.idle_for_ms.get()
    }
}

impl StuckSubtaskAttachment {
    fn from_parts(
        subtask: &SubtaskView,
        claim: Option<Claim>,
        session: Option<Session>,
    ) -> Result<Self, String> {
        validate_stuck_subtask_attachments(subtask, claim.as_ref(), session.as_ref())?;
        match (claim, session) {
            (Some(claim), Some(session)) => Ok(Self::Claimed { claim, session }),
            (None, None) => Ok(Self::Unclaimed),
            (Some(_), None) | (None, Some(_)) => {
                Err("stuck subtask attachment validation accepted an inconsistent shape".to_owned())
            }
        }
    }

    const fn claim(&self) -> Option<&Claim> {
        match self {
            Self::Claimed { claim, .. } => Some(claim),
            Self::Unclaimed => None,
        }
    }

    const fn session(&self) -> Option<&Session> {
        match self {
            Self::Claimed { session, .. } => Some(session),
            Self::Unclaimed => None,
        }
    }
}

fn validate_stuck_subtask_attachments(
    subtask: &SubtaskView,
    claim: Option<&Claim>,
    session: Option<&Session>,
) -> Result<(), String> {
    validate_optional_claim_matches_subtask(
        "stuck subtask",
        subtask,
        claim,
        "requires active claim row",
        "must not include claim without active claim",
    )?;
    validate_optional_session_matches_claim(
        "stuck subtask",
        subtask,
        claim,
        session,
        "requires session row for active claim",
        "must not include session without claim",
    )
}

impl Serialize for StuckSubtask {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawStuckSubtask::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StuckSubtask {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawStuckSubtask::deserialize(deserializer)?
            .try_into()
            .map_err(D::Error::custom)
    }
}

impl From<&StuckSubtask> for RawStuckSubtask {
    fn from(row: &StuckSubtask) -> Self {
        Self {
            subtask: row.subtask.clone(),
            claim: row.claim().cloned(),
            session: row.session().cloned(),
            idle_for_ms: row.idle_for_ms(),
        }
    }
}

impl TryFrom<RawStuckSubtask> for StuckSubtask {
    type Error = String;

    fn try_from(raw: RawStuckSubtask) -> Result<Self, Self::Error> {
        Self::new(raw.subtask, raw.claim, raw.session, raw.idle_for_ms)
    }
}

impl ExpiringClaim {
    /// Builds an expiring-claim observability row.
    ///
    /// # Errors
    ///
    /// Returns an error when `expires_in_ms` is negative, or when the claim,
    /// subtask, and session do not describe the same active claim.
    pub fn new(
        claim: Claim,
        subtask: SubtaskView,
        session: Session,
        expires_in_ms: i64,
    ) -> Result<Self, String> {
        let expires_in_ms = ClaimExpiresInDurationMs::parse(expires_in_ms)?;
        validate_expiring_claim_attachments(&claim, &subtask, &session)?;
        Ok(Self {
            claim,
            subtask,
            session,
            expires_in_ms,
        })
    }

    /// Returns the expiring held claim.
    #[must_use]
    pub const fn claim(&self) -> &Claim {
        &self.claim
    }

    /// Returns the subtask owned by the expiring claim.
    #[must_use]
    pub const fn subtask(&self) -> &SubtaskView {
        &self.subtask
    }

    /// Returns the owner session for the expiring claim.
    #[must_use]
    pub const fn session(&self) -> &Session {
        &self.session
    }

    /// Returns how long until the claim lease expires.
    #[must_use]
    pub const fn expires_in_ms(&self) -> i64 {
        self.expires_in_ms.get()
    }
}

fn validate_expiring_claim_attachments(
    claim: &Claim,
    subtask: &SubtaskView,
    session: &Session,
) -> Result<(), String> {
    validate_claim_matches_subtask("expiring claim", subtask, claim)?;
    validate_session_matches_claim("expiring claim", subtask, claim, session)
}

fn validate_optional_claim_matches_subtask(
    context: &str,
    subtask: &SubtaskView,
    claim: Option<&Claim>,
    missing_message: &str,
    unexpected_message: &str,
) -> Result<(), String> {
    match (subtask.active_claim_id(), claim) {
        (Some(_), Some(claim)) => validate_claim_matches_subtask(context, subtask, claim),
        (Some(_), None) => Err(format!("{context} {missing_message}")),
        (None, Some(_)) => Err(format!("{context} {unexpected_message}")),
        (None, None) => Ok(()),
    }
}

fn validate_claim_matches_subtask(
    context: &str,
    subtask: &SubtaskView,
    claim: &Claim,
) -> Result<(), String> {
    if claim.subtask_id != subtask.subtask_id {
        return Err(format!("{context} claim must belong to the subtask"));
    }
    if Some(&claim.claim_id) != subtask.active_claim_id() {
        return Err(format!(
            "{context} claim_id must match the subtask active claim"
        ));
    }
    Ok(())
}

fn validate_optional_session_matches_claim(
    context: &str,
    subtask: &SubtaskView,
    claim: Option<&Claim>,
    session: Option<&Session>,
    missing_message: &str,
    unexpected_message: &str,
) -> Result<(), String> {
    match (claim, session) {
        (Some(claim), Some(session)) => {
            validate_session_matches_claim(context, subtask, claim, session)
        }
        (Some(_), None) => Err(format!("{context} {missing_message}")),
        (None, Some(_)) => Err(format!("{context} {unexpected_message}")),
        (None, None) => Ok(()),
    }
}

fn validate_session_matches_claim(
    context: &str,
    subtask: &SubtaskView,
    claim: &Claim,
    session: &Session,
) -> Result<(), String> {
    if session.session_token != claim.owner_session_token {
        return Err(format!("{context} session must own the claim"));
    }
    match session.active_subtask_id() {
        Some(active_subtask_id) if active_subtask_id == &subtask.subtask_id => Ok(()),
        Some(_) => Err(format!(
            "{context} session active_subtask_id must match the subtask"
        )),
        None => Err(format!(
            "{context} session must be active on the claimed subtask"
        )),
    }
}

impl Serialize for ExpiringClaim {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawExpiringClaim::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExpiringClaim {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawExpiringClaim::deserialize(deserializer)?
            .try_into()
            .map_err(D::Error::custom)
    }
}

impl From<&ExpiringClaim> for RawExpiringClaim {
    fn from(row: &ExpiringClaim) -> Self {
        Self {
            claim: row.claim.clone(),
            subtask: row.subtask.clone(),
            session: row.session.clone(),
            expires_in_ms: row.expires_in_ms(),
        }
    }
}

impl TryFrom<RawExpiringClaim> for ExpiringClaim {
    type Error = String;

    fn try_from(raw: RawExpiringClaim) -> Result<Self, Self::Error> {
        Self::new(raw.claim, raw.subtask, raw.session, raw.expires_in_ms)
    }
}

/// Aggregate counts and queue ages for the ready queue.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyQueueMetrics {
    queued: QueueMetricBucket,
    in_flight: QueueMetricBucket,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum QueueMetricBucket {
    Empty,
    NonEmpty {
        count: NonZeroUsize,
        oldest_age_ms: QueueMetricAgeMs,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QueueMetricAgeMs(i64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawReadyQueueMetrics {
    queued_count: usize,
    in_flight_count: usize,
    oldest_queued_age_ms: Option<i64>,
    oldest_in_flight_age_ms: Option<i64>,
}

impl ReadyQueueMetrics {
    /// Builds queue metrics from the flat SQL/API shape.
    ///
    /// # Errors
    ///
    /// Returns an error when a non-empty queue is missing an age, when an empty
    /// queue carries an age, or when an age is negative.
    pub fn new(
        queued_count: usize,
        in_flight_count: usize,
        oldest_queued_age_ms: Option<i64>,
        oldest_in_flight_age_ms: Option<i64>,
    ) -> Result<Self, String> {
        Ok(Self {
            queued: QueueMetricBucket::from_parts("queued", queued_count, oldest_queued_age_ms)?,
            in_flight: QueueMetricBucket::from_parts(
                "in_flight",
                in_flight_count,
                oldest_in_flight_age_ms,
            )?,
        })
    }

    /// Returns the number of queued apply items.
    #[must_use]
    pub const fn queued_count(&self) -> usize {
        self.queued.count()
    }

    /// Returns the number of in-flight apply items.
    #[must_use]
    pub const fn in_flight_count(&self) -> usize {
        self.in_flight.count()
    }

    /// Returns the age of the oldest queued apply item.
    #[must_use]
    pub const fn oldest_queued_age_ms(&self) -> Option<i64> {
        self.queued.oldest_age_ms()
    }

    /// Returns the age of the oldest in-flight apply item.
    #[must_use]
    pub const fn oldest_in_flight_age_ms(&self) -> Option<i64> {
        self.in_flight.oldest_age_ms()
    }
}

impl QueueMetricBucket {
    fn from_parts(
        label: &'static str,
        count: usize,
        oldest_age_ms: Option<i64>,
    ) -> Result<Self, String> {
        match (count, oldest_age_ms) {
            (0, None) => Ok(Self::Empty),
            (0, Some(_)) => Err(format!(
                "empty {label} ready-queue metrics must not include oldest age"
            )),
            (count, Some(oldest_age_ms)) => Ok(Self::NonEmpty {
                count: NonZeroUsize::new(count)
                    .expect("non-empty queue metric bucket count should be non-zero"),
                oldest_age_ms: QueueMetricAgeMs::parse(label, oldest_age_ms)?,
            }),
            (_, None) => Err(format!(
                "non-empty {label} ready-queue metrics require oldest age"
            )),
        }
    }

    const fn count(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::NonEmpty { count, .. } => count.get(),
        }
    }

    const fn oldest_age_ms(&self) -> Option<i64> {
        match self {
            Self::Empty => None,
            Self::NonEmpty { oldest_age_ms, .. } => Some(oldest_age_ms.get()),
        }
    }
}

impl QueueMetricAgeMs {
    fn parse(label: &'static str, value: i64) -> Result<Self, String> {
        if value < 0 {
            Err(format!(
                "{label} ready-queue oldest age must not be negative"
            ))
        } else {
            Ok(Self(value))
        }
    }

    const fn get(self) -> i64 {
        self.0
    }
}

impl Serialize for ReadyQueueMetrics {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawReadyQueueMetrics {
            queued_count: self.queued_count(),
            in_flight_count: self.in_flight_count(),
            oldest_queued_age_ms: self.oldest_queued_age_ms(),
            oldest_in_flight_age_ms: self.oldest_in_flight_age_ms(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ReadyQueueMetrics {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawReadyQueueMetrics::deserialize(deserializer)?;
        Self::new(
            raw.queued_count,
            raw.in_flight_count,
            raw.oldest_queued_age_ms,
            raw.oldest_in_flight_age_ms,
        )
        .map_err(serde::de::Error::custom)
    }
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
    artifact_kind: ArtifactKind,
    base_rev: BaseRev,
    manifest_path: ArtifactManifestPath,
    changed_paths_digest: ChangedPathsDigest,
    review_id: ReviewId,
    findings_digest: FindingsDigest,
    claim_fence_seq: FenceSeq,
    verifier: VerifierId,
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
        artifact_kind: ArtifactKind,
        base_rev: BaseRev,
        manifest_path: ArtifactManifestPath,
        changed_paths_digest: ChangedPathsDigest,
        review_id: ReviewId,
        findings_digest: FindingsDigest,
        claim_fence_seq: FenceSeq,
        verifier: VerifierId,
        verdict_digest: ArtifactDigest,
        seal_digest: ArtifactDigest,
        recorded_by_session: SessionToken,
    ) -> Self {
        Self {
            status: LandingAuthorizationState::Accepted(LandingAuthorizationAccepted {
                queue_id,
                artifact_digest,
                artifact_kind,
                base_rev,
                manifest_path,
                changed_paths_digest,
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

    /// Returns the authorized artifact kind.
    #[must_use]
    pub const fn artifact_kind(&self) -> ArtifactKind {
        self.accepted_fields().artifact_kind
    }

    /// Returns the artifact base revision context.
    #[must_use]
    pub const fn base_rev(&self) -> &BaseRev {
        &self.accepted_fields().base_rev
    }

    /// Returns the local manifest path recorded for the artifact.
    #[must_use]
    pub const fn manifest_path(&self) -> &ArtifactManifestPath {
        &self.accepted_fields().manifest_path
    }

    /// Returns the changed-paths digest recorded for the artifact.
    #[must_use]
    pub const fn changed_paths_digest(&self) -> &ChangedPathsDigest {
        &self.accepted_fields().changed_paths_digest
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
        self.accepted_fields().verifier.as_str()
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
        let accepted = self.accepted_fields();
        let mut status = serializer.serialize_struct("LandingAuthorizationStatus", 14)?;
        status.serialize_field("accepted", &true)?;
        status.serialize_field("queue_id", &accepted.queue_id)?;
        status.serialize_field("artifact_digest", &accepted.artifact_digest)?;
        status.serialize_field("artifact_kind", &accepted.artifact_kind)?;
        status.serialize_field("base_rev", &accepted.base_rev)?;
        status.serialize_field("manifest_path", &accepted.manifest_path)?;
        status.serialize_field("changed_paths_digest", &accepted.changed_paths_digest)?;
        status.serialize_field("review_id", &accepted.review_id)?;
        status.serialize_field("findings_digest", &accepted.findings_digest)?;
        status.serialize_field("claim_fence_seq", &accepted.claim_fence_seq)?;
        status.serialize_field("verifier", &accepted.verifier)?;
        status.serialize_field("verdict_digest", &accepted.verdict_digest)?;
        status.serialize_field("seal_digest", &accepted.seal_digest)?;
        status.serialize_field("recorded_by_session", &accepted.recorded_by_session)?;
        status.end()
    }
}

impl<'de> Deserialize<'de> for LandingAuthorizationStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        enum Field {
            Accepted,
            QueueId,
            ArtifactDigest,
            ArtifactKind,
            BaseRev,
            ManifestPath,
            ChangedPathsDigest,
            ReviewId,
            FindingsDigest,
            ClaimFenceSeq,
            Verifier,
            VerdictDigest,
            SealDigest,
            RecordedBySession,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> serde::de::Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(
                        &self,
                        formatter: &mut std::fmt::Formatter<'_>,
                    ) -> std::fmt::Result {
                        formatter.write_str("a landing authorization status field")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "accepted" => Ok(Field::Accepted),
                            "queue_id" => Ok(Field::QueueId),
                            "artifact_digest" => Ok(Field::ArtifactDigest),
                            "artifact_kind" => Ok(Field::ArtifactKind),
                            "base_rev" => Ok(Field::BaseRev),
                            "manifest_path" => Ok(Field::ManifestPath),
                            "changed_paths_digest" => Ok(Field::ChangedPathsDigest),
                            "review_id" => Ok(Field::ReviewId),
                            "findings_digest" => Ok(Field::FindingsDigest),
                            "claim_fence_seq" => Ok(Field::ClaimFenceSeq),
                            "verifier" => Ok(Field::Verifier),
                            "verdict_digest" => Ok(Field::VerdictDigest),
                            "seal_digest" => Ok(Field::SealDigest),
                            "recorded_by_session" => Ok(Field::RecordedBySession),
                            _ => Err(serde::de::Error::unknown_field(
                                value,
                                LANDING_AUTHORIZATION_STATUS_FIELDS,
                            )),
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct StatusVisitor;

        impl<'de> serde::de::Visitor<'de> for StatusVisitor {
            type Value = LandingAuthorizationStatus;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an accepted landing authorization status")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut accepted: Option<bool> = None;
                let mut queue_id: Option<QueueId> = None;
                let mut artifact_digest: Option<ArtifactDigest> = None;
                let mut artifact_kind: Option<ArtifactKind> = None;
                let mut base_rev: Option<BaseRev> = None;
                let mut manifest_path: Option<ArtifactManifestPath> = None;
                let mut changed_paths_digest: Option<ChangedPathsDigest> = None;
                let mut review_id: Option<ReviewId> = None;
                let mut findings_digest: Option<FindingsDigest> = None;
                let mut claim_fence_seq: Option<FenceSeq> = None;
                let mut verifier: Option<VerifierId> = None;
                let mut verdict_digest: Option<ArtifactDigest> = None;
                let mut seal_digest: Option<ArtifactDigest> = None;
                let mut recorded_by_session: Option<SessionToken> = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Accepted => {
                            if accepted.is_some() {
                                return Err(serde::de::Error::duplicate_field("accepted"));
                            }
                            accepted = Some(map.next_value()?);
                        }
                        Field::QueueId => {
                            if queue_id.is_some() {
                                return Err(serde::de::Error::duplicate_field("queue_id"));
                            }
                            queue_id = Some(map.next_value()?);
                        }
                        Field::ArtifactDigest => {
                            if artifact_digest.is_some() {
                                return Err(serde::de::Error::duplicate_field("artifact_digest"));
                            }
                            artifact_digest = Some(map.next_value()?);
                        }
                        Field::ArtifactKind => {
                            if artifact_kind.is_some() {
                                return Err(serde::de::Error::duplicate_field("artifact_kind"));
                            }
                            artifact_kind = Some(map.next_value()?);
                        }
                        Field::BaseRev => {
                            if base_rev.is_some() {
                                return Err(serde::de::Error::duplicate_field("base_rev"));
                            }
                            base_rev = Some(map.next_value()?);
                        }
                        Field::ManifestPath => {
                            if manifest_path.is_some() {
                                return Err(serde::de::Error::duplicate_field("manifest_path"));
                            }
                            manifest_path = Some(map.next_value()?);
                        }
                        Field::ChangedPathsDigest => {
                            if changed_paths_digest.is_some() {
                                return Err(serde::de::Error::duplicate_field(
                                    "changed_paths_digest",
                                ));
                            }
                            changed_paths_digest = Some(map.next_value()?);
                        }
                        Field::ReviewId => {
                            if review_id.is_some() {
                                return Err(serde::de::Error::duplicate_field("review_id"));
                            }
                            review_id = Some(map.next_value()?);
                        }
                        Field::FindingsDigest => {
                            if findings_digest.is_some() {
                                return Err(serde::de::Error::duplicate_field("findings_digest"));
                            }
                            findings_digest = Some(map.next_value()?);
                        }
                        Field::ClaimFenceSeq => {
                            if claim_fence_seq.is_some() {
                                return Err(serde::de::Error::duplicate_field("claim_fence_seq"));
                            }
                            claim_fence_seq = Some(map.next_value()?);
                        }
                        Field::Verifier => {
                            if verifier.is_some() {
                                return Err(serde::de::Error::duplicate_field("verifier"));
                            }
                            verifier = Some(map.next_value()?);
                        }
                        Field::VerdictDigest => {
                            if verdict_digest.is_some() {
                                return Err(serde::de::Error::duplicate_field("verdict_digest"));
                            }
                            verdict_digest = Some(map.next_value()?);
                        }
                        Field::SealDigest => {
                            if seal_digest.is_some() {
                                return Err(serde::de::Error::duplicate_field("seal_digest"));
                            }
                            seal_digest = Some(map.next_value()?);
                        }
                        Field::RecordedBySession => {
                            if recorded_by_session.is_some() {
                                return Err(serde::de::Error::duplicate_field(
                                    "recorded_by_session",
                                ));
                            }
                            recorded_by_session = Some(map.next_value()?);
                        }
                    }
                }

                let accepted =
                    accepted.ok_or_else(|| serde::de::Error::missing_field("accepted"))?;
                if !accepted {
                    return Err(serde::de::Error::custom(
                        "landing authorization status is only emitted for accepted checks",
                    ));
                }

                Ok(LandingAuthorizationStatus::accepted(
                    queue_id.ok_or_else(|| serde::de::Error::missing_field("queue_id"))?,
                    artifact_digest
                        .ok_or_else(|| serde::de::Error::missing_field("artifact_digest"))?,
                    artifact_kind
                        .ok_or_else(|| serde::de::Error::missing_field("artifact_kind"))?,
                    base_rev.ok_or_else(|| serde::de::Error::missing_field("base_rev"))?,
                    manifest_path
                        .ok_or_else(|| serde::de::Error::missing_field("manifest_path"))?,
                    changed_paths_digest
                        .ok_or_else(|| serde::de::Error::missing_field("changed_paths_digest"))?,
                    review_id.ok_or_else(|| serde::de::Error::missing_field("review_id"))?,
                    findings_digest
                        .ok_or_else(|| serde::de::Error::missing_field("findings_digest"))?,
                    claim_fence_seq
                        .ok_or_else(|| serde::de::Error::missing_field("claim_fence_seq"))?,
                    verifier.ok_or_else(|| serde::de::Error::missing_field("verifier"))?,
                    verdict_digest
                        .ok_or_else(|| serde::de::Error::missing_field("verdict_digest"))?,
                    seal_digest.ok_or_else(|| serde::de::Error::missing_field("seal_digest"))?,
                    recorded_by_session
                        .ok_or_else(|| serde::de::Error::missing_field("recorded_by_session"))?,
                ))
            }
        }

        const LANDING_AUTHORIZATION_STATUS_FIELDS: &[&str] = &[
            "accepted",
            "queue_id",
            "artifact_digest",
            "artifact_kind",
            "base_rev",
            "manifest_path",
            "changed_paths_digest",
            "review_id",
            "findings_digest",
            "claim_fence_seq",
            "verifier",
            "verdict_digest",
            "seal_digest",
            "recorded_by_session",
        ];

        deserializer.deserialize_struct(
            "LandingAuthorizationStatus",
            LANDING_AUTHORIZATION_STATUS_FIELDS,
            StatusVisitor,
        )
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
    Enforce,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawRepoopsAuthorityPolicyFact {
    mode: RepoopsAuthorityPolicyMode,
    phase: u8,
    denied_rule_id: Option<String>,
}

impl RepoopsAuthorityPolicyFact {
    /// Builds one enforce-mode policy fact.
    pub const fn enforce() -> Self {
        Self {
            policy: RepoopsAuthorityPolicy::Enforce,
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
    const ENFORCE_PHASE: u8 = 2;

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
                if phase != Self::ENFORCE_PHASE {
                    return Err("enforce repoops policy fact phase is not supported".into());
                }
                Ok(Self::Enforce)
            }
        }
    }

    const fn mode(&self) -> RepoopsAuthorityPolicyMode {
        match self {
            Self::Enforce => RepoopsAuthorityPolicyMode::Enforce,
        }
    }

    const fn phase(&self) -> u8 {
        match self {
            Self::Enforce => Self::ENFORCE_PHASE,
        }
    }

    const fn denied_rule_id(&self) -> Option<&String> {
        match self {
            Self::Enforce => None,
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
    owner: AgentPrincipalId,
    scope_in: Vec<RepoopsScopePattern>,
    scope_out: Vec<RepoopsScopePattern>,
    has_required_contract_fields: bool,
    lifecycle: RepoopsAuthorityClaimLifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RepoopsAuthorityClaimLifecycle {
    InProgress {
        active_ownership_token: SessionToken,
    },
    Open,
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
    ///
    /// # Errors
    ///
    /// Returns an error when owner or scope fields are not normalized, or when
    /// required contract fields are marked present without an inclusion scope.
    pub fn in_progress(
        claim_id: ClaimId,
        owner: impl Into<String>,
        scope_in: Vec<String>,
        scope_out: Vec<String>,
        has_required_contract_fields: bool,
        active_ownership_token: SessionToken,
    ) -> Result<Self, String> {
        Self::from_parts(
            claim_id,
            owner,
            scope_in,
            scope_out,
            has_required_contract_fields,
            RepoopsAuthorityClaimLifecycle::InProgress {
                active_ownership_token,
            },
        )
    }

    /// Builds an open claim fact. Open claims do not carry active ownership tokens.
    ///
    /// # Errors
    ///
    /// Returns an error when owner or scope fields are not normalized, or when
    /// required contract fields are marked present without an inclusion scope.
    pub fn open(
        claim_id: ClaimId,
        owner: impl Into<String>,
        scope_in: Vec<String>,
        scope_out: Vec<String>,
        has_required_contract_fields: bool,
    ) -> Result<Self, String> {
        Self::from_parts(
            claim_id,
            owner,
            scope_in,
            scope_out,
            has_required_contract_fields,
            RepoopsAuthorityClaimLifecycle::Open,
        )
    }

    /// Returns the claim status.
    pub const fn status(&self) -> RepoopsAuthorityClaimStatus {
        self.lifecycle.status()
    }

    /// Returns the owner reference for this claim fact.
    #[must_use]
    pub fn owner(&self) -> &str {
        self.owner.as_str()
    }

    /// Returns inclusion scope patterns for this claim fact.
    #[must_use]
    pub fn scope_in(&self) -> Vec<String> {
        self.scope_in.iter().map(ToString::to_string).collect()
    }

    /// Returns exclusion scope patterns for this claim fact.
    #[must_use]
    pub fn scope_out(&self) -> Vec<String> {
        self.scope_out.iter().map(ToString::to_string).collect()
    }

    /// Returns whether this fact carries the required repoops contract fields.
    #[must_use]
    pub const fn has_required_contract_fields(&self) -> bool {
        self.has_required_contract_fields
    }

    /// Returns the active ownership token reference for in-progress claims.
    #[must_use]
    pub fn active_ownership_token(&self) -> Option<&str> {
        self.lifecycle.active_ownership_token()
    }

    fn from_parts(
        claim_id: ClaimId,
        owner: impl Into<String>,
        scope_in: Vec<String>,
        scope_out: Vec<String>,
        has_required_contract_fields: bool,
        lifecycle: RepoopsAuthorityClaimLifecycle,
    ) -> Result<Self, String> {
        let owner = AgentPrincipalId::parse(owner.into()).map_err(|err| err.to_string())?;
        let scope_in = parse_repoops_scope_patterns("claim.scope_in", scope_in)?;
        let scope_out = parse_repoops_scope_patterns("claim.scope_out", scope_out)?;
        if has_required_contract_fields && scope_in.is_empty() {
            return Err("repoops claim facts with required contract fields need scope_in".into());
        }
        Ok(Self {
            claim_id,
            owner,
            scope_in,
            scope_out,
            has_required_contract_fields,
            lifecycle,
        })
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
                    active_ownership_token: SessionToken::parse(active_ownership_token)
                        .map_err(|err| err.to_string())?,
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
            } => Some(active_ownership_token.as_str()),
            Self::Open => None,
        }
    }
}

impl Serialize for RepoopsAuthorityClaimFact {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut claim = serializer.serialize_struct("RepoopsAuthorityClaimFact", 7)?;
        claim.serialize_field("claim_id", &self.claim_id)?;
        claim.serialize_field("status", &self.status())?;
        claim.serialize_field("owner", self.owner())?;
        claim.serialize_field("scope_in", &self.scope_in())?;
        claim.serialize_field("scope_out", &self.scope_out())?;
        claim.serialize_field(
            "has_required_contract_fields",
            &self.has_required_contract_fields,
        )?;
        claim.serialize_field("active_ownership_token", &self.active_ownership_token())?;
        claim.end()
    }
}

impl<'de> Deserialize<'de> for RepoopsAuthorityClaimFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        enum Field {
            ClaimId,
            Status,
            Owner,
            ScopeIn,
            ScopeOut,
            HasRequiredContractFields,
            ActiveOwnershipToken,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> serde::de::Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(
                        &self,
                        formatter: &mut std::fmt::Formatter<'_>,
                    ) -> std::fmt::Result {
                        formatter.write_str("a repoops authority claim fact field")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "claim_id" => Ok(Field::ClaimId),
                            "status" => Ok(Field::Status),
                            "owner" => Ok(Field::Owner),
                            "scope_in" => Ok(Field::ScopeIn),
                            "scope_out" => Ok(Field::ScopeOut),
                            "has_required_contract_fields" => Ok(Field::HasRequiredContractFields),
                            "active_ownership_token" => Ok(Field::ActiveOwnershipToken),
                            _ => Err(serde::de::Error::unknown_field(
                                value,
                                REPOOPS_AUTHORITY_CLAIM_FIELDS,
                            )),
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct ClaimFactVisitor;

        impl<'de> serde::de::Visitor<'de> for ClaimFactVisitor {
            type Value = RepoopsAuthorityClaimFact;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a repoops authority claim fact")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut claim_id: Option<ClaimId> = None;
                let mut status: Option<RepoopsAuthorityClaimStatus> = None;
                let mut owner: Option<String> = None;
                let mut scope_in: Option<Vec<String>> = None;
                let mut scope_out: Option<Vec<String>> = None;
                let mut has_required_contract_fields: Option<bool> = None;
                let mut active_ownership_token: Option<Option<String>> = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::ClaimId => {
                            if claim_id.is_some() {
                                return Err(serde::de::Error::duplicate_field("claim_id"));
                            }
                            claim_id = Some(map.next_value()?);
                        }
                        Field::Status => {
                            if status.is_some() {
                                return Err(serde::de::Error::duplicate_field("status"));
                            }
                            status = Some(map.next_value()?);
                        }
                        Field::Owner => {
                            if owner.is_some() {
                                return Err(serde::de::Error::duplicate_field("owner"));
                            }
                            owner = Some(map.next_value()?);
                        }
                        Field::ScopeIn => {
                            if scope_in.is_some() {
                                return Err(serde::de::Error::duplicate_field("scope_in"));
                            }
                            scope_in = Some(map.next_value()?);
                        }
                        Field::ScopeOut => {
                            if scope_out.is_some() {
                                return Err(serde::de::Error::duplicate_field("scope_out"));
                            }
                            scope_out = Some(map.next_value()?);
                        }
                        Field::HasRequiredContractFields => {
                            if has_required_contract_fields.is_some() {
                                return Err(serde::de::Error::duplicate_field(
                                    "has_required_contract_fields",
                                ));
                            }
                            has_required_contract_fields = Some(map.next_value()?);
                        }
                        Field::ActiveOwnershipToken => {
                            if active_ownership_token.is_some() {
                                return Err(serde::de::Error::duplicate_field(
                                    "active_ownership_token",
                                ));
                            }
                            active_ownership_token = Some(map.next_value()?);
                        }
                    }
                }

                let claim_id =
                    claim_id.ok_or_else(|| serde::de::Error::missing_field("claim_id"))?;
                let status = status.ok_or_else(|| serde::de::Error::missing_field("status"))?;
                let owner = owner.ok_or_else(|| serde::de::Error::missing_field("owner"))?;
                let scope_in =
                    scope_in.ok_or_else(|| serde::de::Error::missing_field("scope_in"))?;
                let scope_out =
                    scope_out.ok_or_else(|| serde::de::Error::missing_field("scope_out"))?;
                let has_required_contract_fields =
                    has_required_contract_fields.ok_or_else(|| {
                        serde::de::Error::missing_field("has_required_contract_fields")
                    })?;
                let lifecycle = RepoopsAuthorityClaimLifecycle::try_from_parts(
                    status,
                    active_ownership_token.unwrap_or(None),
                )
                .map_err(serde::de::Error::custom)?;

                RepoopsAuthorityClaimFact::from_parts(
                    claim_id,
                    owner,
                    scope_in,
                    scope_out,
                    has_required_contract_fields,
                    lifecycle,
                )
                .map_err(serde::de::Error::custom)
            }
        }

        const REPOOPS_AUTHORITY_CLAIM_FIELDS: &[&str] = &[
            "claim_id",
            "status",
            "owner",
            "scope_in",
            "scope_out",
            "has_required_contract_fields",
            "active_ownership_token",
        ];

        deserializer.deserialize_struct(
            "RepoopsAuthorityClaimFact",
            REPOOPS_AUTHORITY_CLAIM_FIELDS,
            ClaimFactVisitor,
        )
    }
}

/// Scope facts derived from Covey lifecycle and reservation state.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoopsAuthorityScopeFact {
    scope_in: Vec<RepoopsScopePattern>,
    scope_out: Vec<RepoopsScopePattern>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawRepoopsAuthorityScopeFact {
    #[serde(rename = "in")]
    scope_in: Vec<String>,
    #[serde(rename = "out")]
    scope_out: Vec<String>,
}

impl RepoopsAuthorityScopeFact {
    /// Builds scope facts from normalized inclusion and exclusion patterns.
    ///
    /// # Errors
    ///
    /// Returns an error when a scope pattern is blank, padded, or duplicated
    /// within its inclusion or exclusion list.
    pub fn new(scope_in: Vec<String>, scope_out: Vec<String>) -> Result<Self, String> {
        let scope_in = parse_repoops_scope_patterns("scope.in", scope_in)?;
        let scope_out = parse_repoops_scope_patterns("scope.out", scope_out)?;
        Ok(Self {
            scope_in,
            scope_out,
        })
    }

    /// Returns inclusion scope patterns.
    #[must_use]
    pub fn scope_in(&self) -> Vec<String> {
        self.scope_in.iter().map(ToString::to_string).collect()
    }

    /// Returns exclusion scope patterns.
    #[must_use]
    pub fn scope_out(&self) -> Vec<String> {
        self.scope_out.iter().map(ToString::to_string).collect()
    }
}

impl Serialize for RepoopsAuthorityScopeFact {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawRepoopsAuthorityScopeFact {
            scope_in: self.scope_in.iter().map(ToString::to_string).collect(),
            scope_out: self.scope_out.iter().map(ToString::to_string).collect(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RepoopsAuthorityScopeFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawRepoopsAuthorityScopeFact::deserialize(deserializer)?;
        Self::new(raw.scope_in, raw.scope_out).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RepoopsScopePattern(String);

impl RepoopsScopePattern {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RepoopsScopePattern {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_repoops_scope_pattern(&value)?;
        Ok(Self(value))
    }
}

impl From<RepoopsScopePattern> for String {
    fn from(value: RepoopsScopePattern) -> Self {
        value.0
    }
}

impl std::fmt::Display for RepoopsScopePattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn parse_repoops_scope_patterns(
    label: &str,
    patterns: Vec<String>,
) -> Result<Vec<RepoopsScopePattern>, String> {
    let mut seen: HashSet<&str> = HashSet::with_capacity(patterns.len());
    for pattern in &patterns {
        validate_repoops_scope_pattern(pattern).map_err(|reason| {
            if reason == "patterns must not be empty" {
                format!("{label} patterns must not be empty")
            } else if reason == "patterns must be normalized" {
                format!("{label} patterns must be normalized")
            } else {
                format!("{label} {reason}")
            }
        })?;
        if !seen.insert(pattern.as_str()) {
            return Err(format!("{label} patterns must not contain duplicates"));
        }
    }
    let parsed = patterns.into_iter().map(RepoopsScopePattern).collect();
    Ok(parsed)
}

fn validate_repoops_scope_pattern(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("patterns must not be empty".to_owned());
    }
    if value.trim() != value {
        return Err("patterns must be normalized".to_owned());
    }
    if value.chars().any(char::is_control) {
        return Err("patterns must not contain control characters".to_owned());
    }
    Ok(())
}

/// Path ownership fact passed through to mutAI repoops preflight.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoopsAuthorityLockFact {
    fact: RepoopsAuthorityLock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RepoopsAuthorityLock {
    Owned {
        path: RepoopsLockPath,
        owner: RepoopsLockOwner,
        claim_id: RepoopsClaimRef,
    },
    ForeignOwner {
        path: RepoopsLockPath,
        owner: RepoopsLockOwner,
        claim_id: RepoopsClaimRef,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepoopsLockPath(String);

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepoopsLockOwner(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawRepoopsAuthorityLockFact {
    path: String,
    owner: String,
    claim_id: RepoopsClaimRef,
    status: RepoopsAuthorityLockStatus,
}

impl RepoopsAuthorityLockFact {
    /// Builds a lock fact owned by the current claim.
    ///
    /// # Errors
    ///
    /// Returns an error when the lock path or owner reference is blank,
    /// padded, or otherwise not in normalized repoops form.
    pub fn owned(
        path: impl Into<String>,
        owner: impl Into<String>,
        claim_id: RepoopsClaimRef,
    ) -> Result<Self, String> {
        Ok(Self {
            fact: RepoopsAuthorityLock::from_parts(
                path.into(),
                owner.into(),
                claim_id,
                RepoopsAuthorityLockStatus::Owned,
            )?,
        })
    }

    /// Builds a lock fact owned by another claim or reservation.
    ///
    /// # Errors
    ///
    /// Returns an error when the lock path or owner reference is blank,
    /// padded, or otherwise not in normalized repoops form.
    pub fn foreign_owner(
        path: impl Into<String>,
        owner: impl Into<String>,
        claim_id: RepoopsClaimRef,
    ) -> Result<Self, String> {
        Ok(Self {
            fact: RepoopsAuthorityLock::from_parts(
                path.into(),
                owner.into(),
                claim_id,
                RepoopsAuthorityLockStatus::ForeignOwner,
            )?,
        })
    }

    /// Returns the path covered by the lock fact.
    #[must_use]
    pub fn path(&self) -> &str {
        self.fact.path()
    }

    /// Returns the owner reference for the lock fact.
    #[must_use]
    pub fn owner(&self) -> &str {
        self.fact.owner()
    }

    /// Returns the claim reference associated with the lock fact.
    #[must_use]
    pub const fn claim_id(&self) -> &RepoopsClaimRef {
        self.fact.claim_id()
    }

    /// Returns whether this lock is owned by the current claim or by a foreign owner.
    #[must_use]
    pub const fn status(&self) -> RepoopsAuthorityLockStatus {
        self.fact.status()
    }
}

impl RepoopsAuthorityLock {
    fn from_parts(
        path: String,
        owner: String,
        claim_id: RepoopsClaimRef,
        status: RepoopsAuthorityLockStatus,
    ) -> Result<Self, String> {
        let path = RepoopsLockPath::parse(path)?;
        let owner = RepoopsLockOwner::parse(owner)?;
        Ok(match status {
            RepoopsAuthorityLockStatus::Owned => Self::Owned {
                path,
                owner,
                claim_id,
            },
            RepoopsAuthorityLockStatus::ForeignOwner => Self::ForeignOwner {
                path,
                owner,
                claim_id,
            },
        })
    }

    fn path(&self) -> &str {
        match self {
            Self::Owned { path, .. } | Self::ForeignOwner { path, .. } => path.as_str(),
        }
    }

    fn owner(&self) -> &str {
        match self {
            Self::Owned { owner, .. } | Self::ForeignOwner { owner, .. } => owner.as_str(),
        }
    }

    const fn claim_id(&self) -> &RepoopsClaimRef {
        match self {
            Self::Owned { claim_id, .. } | Self::ForeignOwner { claim_id, .. } => claim_id,
        }
    }

    const fn status(&self) -> RepoopsAuthorityLockStatus {
        match self {
            Self::Owned { .. } => RepoopsAuthorityLockStatus::Owned,
            Self::ForeignOwner { .. } => RepoopsAuthorityLockStatus::ForeignOwner,
        }
    }
}

impl Serialize for RepoopsAuthorityLockFact {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawRepoopsAuthorityLockFact::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RepoopsAuthorityLockFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawRepoopsAuthorityLockFact::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl From<&RepoopsAuthorityLockFact> for RawRepoopsAuthorityLockFact {
    fn from(lock: &RepoopsAuthorityLockFact) -> Self {
        Self {
            path: lock.path().to_owned(),
            owner: lock.owner().to_owned(),
            claim_id: lock.claim_id().clone(),
            status: lock.status(),
        }
    }
}

impl TryFrom<RawRepoopsAuthorityLockFact> for RepoopsAuthorityLockFact {
    type Error = String;

    fn try_from(raw: RawRepoopsAuthorityLockFact) -> Result<Self, Self::Error> {
        Ok(Self {
            fact: RepoopsAuthorityLock::from_parts(raw.path, raw.owner, raw.claim_id, raw.status)?,
        })
    }
}

impl RepoopsLockPath {
    fn parse(path: String) -> Result<Self, String> {
        validate_repoops_project_path("repoops lock path", &path)?;
        if path.starts_with('/') || path.starts_with('\\') {
            return Err("repoops lock path must be repo-relative".to_owned());
        }
        if path.contains('\\') {
            return Err("repoops lock path must be normalized".to_owned());
        }
        for part in path.split('/') {
            if matches!(part, "" | "." | "..") {
                return Err("repoops lock path must be normalized".to_owned());
            }
        }
        Ok(Self(path))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl RepoopsLockOwner {
    fn parse(owner: String) -> Result<Self, String> {
        validate_repoops_token_ref("repoops lock owner", &owner)?;
        Ok(Self(owner))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoopsAuthorityGitContextFact {
    context: RepoopsAuthorityGitContext,
    ownership_token_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RepoopsAuthorityGitContext {
    Unknown,
    KnownPaths {
        policy_project_path: RepoopsProjectPath,
        execution_project_path: RepoopsProjectPath,
        repo_path_prefix: Option<RepoopsRepoPathPrefix>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepoopsProjectPath(String);

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepoopsRepoPathPrefix(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawRepoopsAuthorityGitContextFact {
    policy_project_path: Option<String>,
    execution_project_path: Option<String>,
    repo_path_prefix: Option<String>,
    ownership_token_required: bool,
}

impl RepoopsAuthorityGitContextFact {
    /// Builds git context facts from the flat storage/API shape.
    ///
    /// # Errors
    ///
    /// Returns an error when only one project path is present, when a prefix is
    /// present without project paths, or when any path field is blank, padded,
    /// absolute where repo-relative is required, or traversing.
    pub fn new(
        policy_project_path: Option<String>,
        execution_project_path: Option<String>,
        repo_path_prefix: Option<String>,
        ownership_token_required: bool,
    ) -> Result<Self, String> {
        let context = RepoopsAuthorityGitContext::from_parts(
            policy_project_path,
            execution_project_path,
            repo_path_prefix,
        )?;
        Ok(Self {
            context,
            ownership_token_required,
        })
    }

    /// Builds git context facts when Covey has no concrete project paths.
    #[must_use]
    pub const fn unknown(ownership_token_required: bool) -> Self {
        Self {
            context: RepoopsAuthorityGitContext::Unknown,
            ownership_token_required,
        }
    }

    /// Builds git context facts with concrete project paths.
    ///
    /// # Errors
    ///
    /// Returns an error when any path field is blank, padded, absolute where
    /// repo-relative is required, or traversing.
    pub fn known_paths(
        policy_project_path: impl Into<String>,
        execution_project_path: impl Into<String>,
        repo_path_prefix: Option<String>,
        ownership_token_required: bool,
    ) -> Result<Self, String> {
        Self::new(
            Some(policy_project_path.into()),
            Some(execution_project_path.into()),
            repo_path_prefix,
            ownership_token_required,
        )
    }

    /// Returns the policy project path when Covey knows it.
    #[must_use]
    pub fn policy_project_path(&self) -> Option<&str> {
        self.context.policy_project_path()
    }

    /// Returns the execution project path when Covey knows it.
    #[must_use]
    pub fn execution_project_path(&self) -> Option<&str> {
        self.context.execution_project_path()
    }

    /// Returns the repo-relative execution prefix when Covey knows one.
    #[must_use]
    pub fn repo_path_prefix(&self) -> Option<&str> {
        self.context.repo_path_prefix()
    }

    /// Returns whether an ownership token is required for git mutations.
    #[must_use]
    pub const fn ownership_token_required(&self) -> bool {
        self.ownership_token_required
    }
}

impl RepoopsAuthorityGitContext {
    fn from_parts(
        policy_project_path: Option<String>,
        execution_project_path: Option<String>,
        repo_path_prefix: Option<String>,
    ) -> Result<Self, String> {
        match (
            policy_project_path,
            execution_project_path,
            repo_path_prefix,
        ) {
            (None, None, None) => Ok(Self::Unknown),
            (Some(policy_project_path), Some(execution_project_path), repo_path_prefix) => {
                let policy_project_path = RepoopsProjectPath::parse(
                    "git_context.policy_project_path",
                    policy_project_path,
                )?;
                let execution_project_path = RepoopsProjectPath::parse(
                    "git_context.execution_project_path",
                    execution_project_path,
                )?;
                let repo_path_prefix = repo_path_prefix
                    .map(RepoopsRepoPathPrefix::parse)
                    .transpose()?;
                Ok(Self::KnownPaths {
                    policy_project_path,
                    execution_project_path,
                    repo_path_prefix,
                })
            }
            (None, None, Some(_)) => {
                Err("repoops git context prefix requires project paths".to_owned())
            }
            _ => Err("repoops git context requires both project paths or neither".to_owned()),
        }
    }

    fn policy_project_path(&self) -> Option<&str> {
        match self {
            Self::Unknown => None,
            Self::KnownPaths {
                policy_project_path,
                ..
            } => Some(policy_project_path.as_str()),
        }
    }

    fn execution_project_path(&self) -> Option<&str> {
        match self {
            Self::Unknown => None,
            Self::KnownPaths {
                execution_project_path,
                ..
            } => Some(execution_project_path.as_str()),
        }
    }

    fn repo_path_prefix(&self) -> Option<&str> {
        match self {
            Self::Unknown => None,
            Self::KnownPaths {
                repo_path_prefix, ..
            } => repo_path_prefix.as_ref().map(RepoopsRepoPathPrefix::as_str),
        }
    }
}

fn validate_repoops_project_path(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if value.trim() != value {
        return Err(format!("{label} must be normalized"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label} must not contain control characters"));
    }
    Ok(())
}

fn validate_repoops_token_ref(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if value.trim() != value {
        return Err(format!("{label} must be normalized"));
    }
    if value.len() > 256 {
        return Err(format!("{label} exceeds 256 bytes"));
    }
    if value
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(format!(
            "{label} must not contain whitespace or control characters"
        ));
    }
    Ok(())
}

fn validate_repoops_repo_path_prefix(prefix: &str) -> Result<(), String> {
    validate_repoops_project_path("git_context.repo_path_prefix", prefix)?;
    if prefix.starts_with('/') || prefix.starts_with('\\') {
        return Err("git_context.repo_path_prefix must be repo-relative".to_owned());
    }
    if prefix.contains('\\') {
        return Err("git_context.repo_path_prefix must be normalized".to_owned());
    }
    for part in prefix.split('/') {
        if matches!(part, "" | "." | "..") {
            return Err("git_context.repo_path_prefix must be normalized".to_owned());
        }
    }
    Ok(())
}

impl RepoopsProjectPath {
    fn parse(label: &str, path: String) -> Result<Self, String> {
        validate_repoops_project_path(label, &path)?;
        Ok(Self(path))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl RepoopsRepoPathPrefix {
    fn parse(prefix: String) -> Result<Self, String> {
        validate_repoops_repo_path_prefix(&prefix)?;
        Ok(Self(prefix))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for RepoopsAuthorityGitContextFact {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawRepoopsAuthorityGitContextFact {
            policy_project_path: self.policy_project_path().map(str::to_owned),
            execution_project_path: self.execution_project_path().map(str::to_owned),
            repo_path_prefix: self.repo_path_prefix().map(str::to_owned),
            ownership_token_required: self.ownership_token_required(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RepoopsAuthorityGitContextFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawRepoopsAuthorityGitContextFact::deserialize(deserializer)?;
        Self::new(
            raw.policy_project_path,
            raw.execution_project_path,
            raw.repo_path_prefix,
            raw.ownership_token_required,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Covey lifecycle fact snapshot for mutAI repoops preflight.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoopsAuthoritySnapshot {
    schema_version: RepoopsAuthoritySnapshotSchemaVersion,
    agent_id: AgentPrincipalId,
    subject: RepoopsAuthoritySnapshotSubject,
    policy: RepoopsAuthorityPolicyFact,
    scope: RepoopsAuthorityScopeFact,
    locks: Vec<RepoopsAuthorityLockFact>,
    git_context: Option<RepoopsAuthorityGitContextFact>,
    fact_sources: RepoopsAuthorityFactSources,
}

/// Fields shared by all repoops authority snapshot subjects.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoopsAuthoritySnapshotCommon {
    schema_version: RepoopsAuthoritySnapshotSchemaVersion,
    agent_id: AgentPrincipalId,
    policy: RepoopsAuthorityPolicyFact,
    scope: RepoopsAuthorityScopeFact,
    locks: Vec<RepoopsAuthorityLockFact>,
    git_context: Option<RepoopsAuthorityGitContextFact>,
    fact_sources: RepoopsAuthorityFactSources,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RepoopsAuthoritySnapshotSchemaVersion;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepoopsAuthorityFactSource(String);

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepoopsAuthorityFactSources(Vec<RepoopsAuthorityFactSource>);

#[derive(Debug, Clone, PartialEq, Eq)]
enum RepoopsAuthoritySnapshotSubject {
    ClaimBound {
        claim_id: ClaimId,
        ownership_token: SessionToken,
        claim: RepoopsAuthorityClaimFact,
    },
    Constrained {
        constraint_reason: RepoopsAuthorityConstraintReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepoopsAuthorityConstraintReason(String);

impl RepoopsAuthorityConstraintReason {
    fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err("constrained repoops authority snapshots require constraint_reason".into());
        }
        if value.trim() != value {
            return Err(
                "constrained repoops authority snapshot constraint_reason must be normalized"
                    .into(),
            );
        }
        if value.chars().any(char::is_control) {
            return Err(
                "constrained repoops authority snapshot constraint_reason must not contain control characters"
                    .into(),
            );
        }
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl RepoopsAuthoritySnapshotSchemaVersion {
    const V1: &'static str = "covey_repoops_authority_snapshot.v1";

    fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err("repoops authority snapshots require schema_version".into());
        }
        if value.trim() != value {
            return Err("repoops authority snapshot schema_version must be normalized".into());
        }
        if value != Self::V1 {
            return Err("repoops authority snapshot schema_version is not supported".into());
        }
        Ok(Self)
    }

    const fn as_str(&self) -> &str {
        Self::V1
    }
}

impl RepoopsAuthorityFactSource {
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl RepoopsAuthorityFactSources {
    fn parse(values: Vec<String>) -> Result<Self, String> {
        let mut seen: HashSet<&str> = HashSet::with_capacity(values.len());
        for value in &values {
            validate_repoops_token_ref("repoops authority snapshot fact_sources", value)?;
            if !seen.insert(value.as_str()) {
                return Err("repoops authority snapshot fact_sources must be unique".into());
            }
        }
        let parsed = values.into_iter().map(RepoopsAuthorityFactSource).collect();
        Ok(Self(parsed))
    }

    fn as_refs(&self) -> Vec<&str> {
        self.0
            .iter()
            .map(RepoopsAuthorityFactSource::as_str)
            .collect()
    }

    fn as_strings(&self) -> Vec<String> {
        self.0
            .iter()
            .map(|source| source.as_str().to_owned())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawRepoopsAuthoritySnapshot {
    schema_version: String,
    agent_id: String,
    policy: RepoopsAuthorityPolicyFact,
    scope: RepoopsAuthorityScopeFact,
    locks: Vec<RepoopsAuthorityLockFact>,
    git_context: Option<RepoopsAuthorityGitContextFact>,
    fact_sources: Vec<String>,
    subject: RepoopsAuthoritySnapshotSubject,
}

impl RepoopsAuthoritySnapshotCommon {
    /// Builds validated common repoops authority snapshot facts.
    ///
    /// # Errors
    ///
    /// Returns an error when identity or provenance fields are invalid.
    pub fn new(
        schema_version: String,
        agent_id: String,
        policy: RepoopsAuthorityPolicyFact,
        scope: RepoopsAuthorityScopeFact,
        locks: Vec<RepoopsAuthorityLockFact>,
        git_context: Option<RepoopsAuthorityGitContextFact>,
        fact_sources: Vec<String>,
    ) -> Result<Self, String> {
        Ok(Self {
            schema_version: RepoopsAuthoritySnapshotSchemaVersion::parse(schema_version)?,
            agent_id: AgentPrincipalId::parse(agent_id).map_err(|err| err.to_string())?,
            policy,
            scope,
            locks,
            git_context,
            fact_sources: RepoopsAuthorityFactSources::parse(fact_sources)?,
        })
    }
}

impl RepoopsAuthoritySnapshot {
    /// Builds a snapshot for one current Covey claim selected by repoops preflight.
    pub fn claim_bound(
        common: RepoopsAuthoritySnapshotCommon,
        ownership_token: SessionToken,
        claim: RepoopsAuthorityClaimFact,
    ) -> Result<Self, String> {
        let claim_id = claim.claim_id.clone();
        validate_repoops_authority_locks(
            common.agent_id.as_str(),
            Some((&claim_id, claim.owner.as_str())),
            &common.locks,
        )?;
        Ok(Self {
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
        })
    }

    /// Builds a constrained snapshot that carries no live claim authority.
    pub fn constrained(
        common: RepoopsAuthoritySnapshotCommon,
        constraint_reason: String,
    ) -> Result<Self, String> {
        let constraint_reason = RepoopsAuthorityConstraintReason::parse(constraint_reason)?;
        validate_repoops_authority_locks(common.agent_id.as_str(), None, &common.locks)?;
        Ok(Self {
            schema_version: common.schema_version,
            agent_id: common.agent_id,
            subject: RepoopsAuthoritySnapshotSubject::Constrained { constraint_reason },
            policy: common.policy,
            scope: common.scope,
            locks: common.locks,
            git_context: common.git_context,
            fact_sources: common.fact_sources,
        })
    }

    /// Returns the current claim id when this is a claim-bound snapshot.
    #[must_use]
    pub const fn claim_id(&self) -> Option<&ClaimId> {
        match &self.subject {
            RepoopsAuthoritySnapshotSubject::ClaimBound { claim_id, .. } => Some(claim_id),
            RepoopsAuthoritySnapshotSubject::Constrained { .. } => None,
        }
    }

    /// Returns the snapshot schema version.
    #[must_use]
    pub fn schema_version(&self) -> &str {
        self.schema_version.as_str()
    }

    /// Returns the agent selected by Covey for these authority facts.
    #[must_use]
    pub fn agent_id(&self) -> &str {
        self.agent_id.as_str()
    }

    /// Returns the caller ownership token reference for claim-bound snapshots.
    #[must_use]
    pub fn ownership_token(&self) -> Option<&str> {
        match &self.subject {
            RepoopsAuthoritySnapshotSubject::ClaimBound {
                ownership_token, ..
            } => Some(ownership_token.as_str()),
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

    /// Returns policy facts bound into this authority snapshot.
    #[must_use]
    pub const fn policy(&self) -> &RepoopsAuthorityPolicyFact {
        &self.policy
    }

    /// Returns requested-scope facts bound into this authority snapshot.
    #[must_use]
    pub const fn scope(&self) -> &RepoopsAuthorityScopeFact {
        &self.scope
    }

    /// Returns the constraint reason when this snapshot has no claim authority.
    #[must_use]
    pub fn constraint_reason(&self) -> Option<&str> {
        match &self.subject {
            RepoopsAuthoritySnapshotSubject::ClaimBound { .. } => None,
            RepoopsAuthoritySnapshotSubject::Constrained { constraint_reason } => {
                Some(constraint_reason.as_str())
            }
        }
    }

    /// Returns lock facts bound into this authority snapshot.
    #[must_use]
    pub fn locks(&self) -> &[RepoopsAuthorityLockFact] {
        &self.locks
    }

    /// Returns optional git-context facts bound into this authority snapshot.
    #[must_use]
    pub const fn git_context(&self) -> Option<&RepoopsAuthorityGitContextFact> {
        self.git_context.as_ref()
    }

    /// Returns provenance strings for this authority snapshot.
    #[must_use]
    pub fn fact_sources(&self) -> Vec<&str> {
        self.fact_sources.as_refs()
    }
}

impl RepoopsAuthoritySnapshotSubject {
    const fn claim_id(&self) -> Option<&ClaimId> {
        match self {
            Self::ClaimBound { claim_id, .. } => Some(claim_id),
            Self::Constrained { .. } => None,
        }
    }

    fn ownership_token(&self) -> Option<&str> {
        match self {
            Self::ClaimBound {
                ownership_token, ..
            } => Some(ownership_token.as_str()),
            Self::Constrained { .. } => None,
        }
    }

    const fn override_token(&self) -> Option<&str> {
        None
    }

    const fn claim(&self) -> Option<&RepoopsAuthorityClaimFact> {
        match self {
            Self::ClaimBound { claim, .. } => Some(claim),
            Self::Constrained { .. } => None,
        }
    }

    fn constraint_reason(&self) -> Option<&str> {
        match self {
            Self::ClaimBound { .. } => None,
            Self::Constrained { constraint_reason } => Some(constraint_reason.as_str()),
        }
    }

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
                    ownership_token: SessionToken::parse(ownership_token)
                        .map_err(|err| err.to_string())?,
                    claim,
                })
            }
            (None, None, None, Some(constraint_reason)) => Ok(Self::Constrained {
                constraint_reason: RepoopsAuthorityConstraintReason::parse(constraint_reason)?,
            }),
            _ => Err(
                "repoops authority snapshots must be claim-bound or constrained, not mixed".into(),
            ),
        }
    }
}

impl Serialize for RawRepoopsAuthoritySnapshot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut snapshot = serializer.serialize_struct("RepoopsAuthoritySnapshot", 12)?;
        snapshot.serialize_field("schema_version", &self.schema_version)?;
        snapshot.serialize_field("agent_id", &self.agent_id)?;
        snapshot.serialize_field("claim_id", &self.subject.claim_id())?;
        snapshot.serialize_field("ownership_token", &self.subject.ownership_token())?;
        snapshot.serialize_field("override_token", &self.subject.override_token())?;
        snapshot.serialize_field("policy", &self.policy)?;
        snapshot.serialize_field("claim", &self.subject.claim())?;
        snapshot.serialize_field("scope", &self.scope)?;
        snapshot.serialize_field("locks", &self.locks)?;
        snapshot.serialize_field("git_context", &self.git_context)?;
        snapshot.serialize_field("constraint_reason", &self.subject.constraint_reason())?;
        snapshot.serialize_field("fact_sources", &self.fact_sources)?;
        snapshot.end()
    }
}

impl<'de> Deserialize<'de> for RawRepoopsAuthoritySnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        enum Field {
            SchemaVersion,
            AgentId,
            ClaimId,
            OwnershipToken,
            OverrideToken,
            Policy,
            Claim,
            Scope,
            Locks,
            GitContext,
            ConstraintReason,
            FactSources,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> serde::de::Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(
                        &self,
                        formatter: &mut std::fmt::Formatter<'_>,
                    ) -> std::fmt::Result {
                        formatter.write_str("a repoops authority snapshot field")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "schema_version" => Ok(Field::SchemaVersion),
                            "agent_id" => Ok(Field::AgentId),
                            "claim_id" => Ok(Field::ClaimId),
                            "ownership_token" => Ok(Field::OwnershipToken),
                            "override_token" => Ok(Field::OverrideToken),
                            "policy" => Ok(Field::Policy),
                            "claim" => Ok(Field::Claim),
                            "scope" => Ok(Field::Scope),
                            "locks" => Ok(Field::Locks),
                            "git_context" => Ok(Field::GitContext),
                            "constraint_reason" => Ok(Field::ConstraintReason),
                            "fact_sources" => Ok(Field::FactSources),
                            _ => Err(serde::de::Error::unknown_field(
                                value,
                                REPOOPS_AUTHORITY_SNAPSHOT_FIELDS,
                            )),
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct SnapshotVisitor;

        impl<'de> serde::de::Visitor<'de> for SnapshotVisitor {
            type Value = RawRepoopsAuthoritySnapshot;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a repoops authority snapshot")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut schema_version: Option<String> = None;
                let mut agent_id: Option<String> = None;
                let mut claim_id: Option<Option<ClaimId>> = None;
                let mut ownership_token: Option<Option<String>> = None;
                let mut override_token: Option<Option<String>> = None;
                let mut policy: Option<RepoopsAuthorityPolicyFact> = None;
                let mut claim: Option<Option<RepoopsAuthorityClaimFact>> = None;
                let mut scope: Option<RepoopsAuthorityScopeFact> = None;
                let mut locks: Option<Vec<RepoopsAuthorityLockFact>> = None;
                let mut git_context: Option<Option<RepoopsAuthorityGitContextFact>> = None;
                let mut constraint_reason: Option<Option<String>> = None;
                let mut fact_sources: Option<Vec<String>> = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::SchemaVersion => {
                            if schema_version.is_some() {
                                return Err(serde::de::Error::duplicate_field("schema_version"));
                            }
                            schema_version = Some(map.next_value()?);
                        }
                        Field::AgentId => {
                            if agent_id.is_some() {
                                return Err(serde::de::Error::duplicate_field("agent_id"));
                            }
                            agent_id = Some(map.next_value()?);
                        }
                        Field::ClaimId => {
                            if claim_id.is_some() {
                                return Err(serde::de::Error::duplicate_field("claim_id"));
                            }
                            claim_id = Some(map.next_value()?);
                        }
                        Field::OwnershipToken => {
                            if ownership_token.is_some() {
                                return Err(serde::de::Error::duplicate_field("ownership_token"));
                            }
                            ownership_token = Some(map.next_value()?);
                        }
                        Field::OverrideToken => {
                            if override_token.is_some() {
                                return Err(serde::de::Error::duplicate_field("override_token"));
                            }
                            override_token = Some(map.next_value()?);
                        }
                        Field::Policy => {
                            if policy.is_some() {
                                return Err(serde::de::Error::duplicate_field("policy"));
                            }
                            policy = Some(map.next_value()?);
                        }
                        Field::Claim => {
                            if claim.is_some() {
                                return Err(serde::de::Error::duplicate_field("claim"));
                            }
                            claim = Some(map.next_value()?);
                        }
                        Field::Scope => {
                            if scope.is_some() {
                                return Err(serde::de::Error::duplicate_field("scope"));
                            }
                            scope = Some(map.next_value()?);
                        }
                        Field::Locks => {
                            if locks.is_some() {
                                return Err(serde::de::Error::duplicate_field("locks"));
                            }
                            locks = Some(map.next_value()?);
                        }
                        Field::GitContext => {
                            if git_context.is_some() {
                                return Err(serde::de::Error::duplicate_field("git_context"));
                            }
                            git_context = Some(map.next_value()?);
                        }
                        Field::ConstraintReason => {
                            if constraint_reason.is_some() {
                                return Err(serde::de::Error::duplicate_field("constraint_reason"));
                            }
                            constraint_reason = Some(map.next_value()?);
                        }
                        Field::FactSources => {
                            if fact_sources.is_some() {
                                return Err(serde::de::Error::duplicate_field("fact_sources"));
                            }
                            fact_sources = Some(map.next_value()?);
                        }
                    }
                }

                let subject = RepoopsAuthoritySnapshotSubject::try_from_parts(
                    claim_id.unwrap_or(None),
                    ownership_token.unwrap_or(None),
                    override_token.unwrap_or(None),
                    claim.unwrap_or(None),
                    constraint_reason.unwrap_or(None),
                )
                .map_err(serde::de::Error::custom)?;

                Ok(RawRepoopsAuthoritySnapshot {
                    schema_version: schema_version
                        .ok_or_else(|| serde::de::Error::missing_field("schema_version"))?,
                    agent_id: agent_id
                        .ok_or_else(|| serde::de::Error::missing_field("agent_id"))?,
                    policy: policy.ok_or_else(|| serde::de::Error::missing_field("policy"))?,
                    scope: scope.ok_or_else(|| serde::de::Error::missing_field("scope"))?,
                    locks: locks.ok_or_else(|| serde::de::Error::missing_field("locks"))?,
                    git_context: git_context.unwrap_or(None),
                    fact_sources: fact_sources
                        .ok_or_else(|| serde::de::Error::missing_field("fact_sources"))?,
                    subject,
                })
            }
        }

        const REPOOPS_AUTHORITY_SNAPSHOT_FIELDS: &[&str] = &[
            "schema_version",
            "agent_id",
            "claim_id",
            "ownership_token",
            "override_token",
            "policy",
            "claim",
            "scope",
            "locks",
            "git_context",
            "constraint_reason",
            "fact_sources",
        ];

        deserializer.deserialize_struct(
            "RepoopsAuthoritySnapshot",
            REPOOPS_AUTHORITY_SNAPSHOT_FIELDS,
            SnapshotVisitor,
        )
    }
}

impl Serialize for RepoopsAuthoritySnapshot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawRepoopsAuthoritySnapshot {
            schema_version: self.schema_version().to_owned(),
            agent_id: self.agent_id().to_owned(),
            policy: self.policy().clone(),
            scope: self.scope().clone(),
            locks: self.locks.clone(),
            git_context: self.git_context().cloned(),
            fact_sources: self.fact_sources.as_strings(),
            subject: self.subject.clone(),
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
        let common = RepoopsAuthoritySnapshotCommon::new(
            raw.schema_version,
            raw.agent_id,
            raw.policy,
            raw.scope,
            raw.locks,
            raw.git_context,
            raw.fact_sources,
        )
        .map_err(serde::de::Error::custom)?;
        let subject = raw.subject;
        validate_repoops_authority_locks(
            common.agent_id.as_str(),
            subject.claim_owner_ref(),
            &common.locks,
        )
        .map_err(serde::de::Error::custom)?;
        Ok(Self {
            schema_version: common.schema_version,
            agent_id: common.agent_id,
            subject,
            policy: common.policy,
            scope: common.scope,
            locks: common.locks,
            git_context: common.git_context,
            fact_sources: common.fact_sources,
        })
    }
}

impl RepoopsAuthoritySnapshotSubject {
    fn claim_owner_ref(&self) -> Option<(&ClaimId, &str)> {
        match self {
            Self::ClaimBound {
                claim_id, claim, ..
            } => Some((claim_id, claim.owner.as_str())),
            Self::Constrained { .. } => None,
        }
    }
}

fn validate_repoops_authority_locks(
    agent_id: &str,
    claim_owner: Option<(&ClaimId, &str)>,
    locks: &[RepoopsAuthorityLockFact],
) -> Result<(), String> {
    for lock in locks {
        match (lock.status(), claim_owner) {
            (RepoopsAuthorityLockStatus::Owned, Some((claim_id, owner))) => {
                if lock.claim_id().as_str() != claim_id.as_str() || lock.owner() != owner {
                    return Err(
                        "owned repoops lock facts must match snapshot claim and owner".into(),
                    );
                }
            }
            (RepoopsAuthorityLockStatus::Owned, None) => {
                return Err("constrained repoops snapshots must not include owned locks".into());
            }
            (RepoopsAuthorityLockStatus::ForeignOwner, Some((claim_id, owner))) => {
                if lock.claim_id().as_str() == claim_id.as_str() || lock.owner() == owner {
                    return Err(
                        "foreign repoops lock facts must not match snapshot claim or owner".into(),
                    );
                }
            }
            (RepoopsAuthorityLockStatus::ForeignOwner, None) => {
                if lock.owner() == agent_id {
                    return Err(
                        "constrained repoops snapshots must not include locks owned by agent"
                            .into(),
                    );
                }
            }
        }
    }
    Ok(())
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

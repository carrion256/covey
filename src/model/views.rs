use std::collections::HashSet;

use derive_new::new;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeError};
use strum::Display;

use super::{
    Artifact, ArtifactDigest, Claim, ClaimId, FenceSeq, FindingsDigest, MetaTask, MetaTaskId,
    QueueId, ReadyQueueItem, RepoopsClaimRef, Review, ReviewId, ReviewTarget, Session,
    SessionToken, Subtask, SubtaskId, SubtaskKind, SubtaskLifecycle, SubtaskPriority, SubtaskRow,
    SubtaskState, TimestampMs, VerifierId,
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
    pub priority: SubtaskPriority,
    timestamps: SubtaskViewTimestamps,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SubtaskViewKind {
    Work,
    Review { review_target: ReviewTarget },
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
        priority: SubtaskPriority,
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
        let created_at = row.created_at();
        let updated_at = row.updated_at();

        Self::new(
            row.subtask_id,
            row.meta_task_id,
            row.title,
            SubtaskViewKind::from_parts(domain.kind(), domain.review_target().cloned())?,
            lifecycle.clone(),
            row.priority,
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
            title: view.title.clone(),
            kind: view.kind(),
            review_target: view.review_target().cloned(),
            state: view.state(),
            active_claim_id: view.active_claim_id().cloned(),
            artifact_digest: view.artifact_digest().cloned(),
            priority: view.priority,
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
        Self::new(
            raw.subtask_id,
            raw.meta_task_id,
            raw.title,
            kind,
            lifecycle,
            raw.priority,
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
    session: Session,
    active_subtask: Option<SubtaskView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawSessionStatus {
    session: Session,
    active_subtask: Option<SubtaskView>,
}

impl SessionStatus {
    /// Builds a session status view whose optional active subtask matches the session lifecycle.
    ///
    /// # Errors
    ///
    /// Returns an error when the active subtask view is missing, stale, or
    /// attached to a session without an active subtask.
    pub fn new(session: Session, active_subtask: Option<SubtaskView>) -> Result<Self, String> {
        match (session.active_subtask_id(), active_subtask.as_ref()) {
            (Some(expected), Some(subtask)) if &subtask.subtask_id == expected => {}
            (Some(_), Some(_)) => {
                return Err("session status active_subtask must match session state".to_owned());
            }
            (Some(_), None) => {
                return Err("session status requires active_subtask view".to_owned());
            }
            (None, Some(_)) => {
                return Err("session status must not include active_subtask".to_owned());
            }
            (None, None) => {}
        }
        Ok(Self {
            session,
            active_subtask,
        })
    }

    /// Returns the session row.
    #[must_use]
    pub const fn session(&self) -> &Session {
        &self.session
    }

    /// Returns the active subtask view, when the session has one.
    #[must_use]
    pub const fn active_subtask(&self) -> Option<&SubtaskView> {
        self.active_subtask.as_ref()
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
            session: status.session.clone(),
            active_subtask: status.active_subtask.clone(),
        }
    }
}

impl TryFrom<RawSessionStatus> for SessionStatus {
    type Error = String;

    fn try_from(raw: RawSessionStatus) -> Result<Self, Self::Error> {
        Self::new(raw.session, raw.active_subtask)
    }
}

/// Snapshot view of a subtask and its attached stateful records.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtaskStatus {
    subtask: SubtaskView,
    attachments: SubtaskStatusAttachments,
    reviews: Vec<Review>,
    ready_queue: Vec<ReadyQueueItem>,
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
    reviews: Vec<Review>,
    ready_queue: Vec<ReadyQueueItem>,
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
        let attachments = SubtaskStatusAttachments::from_parts(&subtask, claim, artifact)?;
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
            subtask,
            attachments,
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
            reviews: status.reviews.clone(),
            ready_queue: status.ready_queue.clone(),
        }
    }
}

impl TryFrom<RawSubtaskStatus> for SubtaskStatus {
    type Error = String;

    fn try_from(raw: RawSubtaskStatus) -> Result<Self, Self::Error> {
        Self::new(
            raw.subtask,
            raw.claim,
            raw.artifact,
            raw.reviews,
            raw.ready_queue,
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
        count: usize,
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
                count,
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
            Self::NonEmpty { count, .. } => *count,
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
    review_id: ReviewId,
    findings_digest: FindingsDigest,
    claim_fence_seq: FenceSeq,
    verifier: VerifierId,
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
    owner: String,
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
    ///
    /// # Errors
    ///
    /// Returns an error when owner or scope fields are not normalized, or when
    /// required contract fields are marked present without an inclusion scope.
    pub fn in_progress(
        claim_id: ClaimId,
        owner: String,
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
        owner: String,
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
        &self.owner
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
        owner: String,
        scope_in: Vec<String>,
        scope_out: Vec<String>,
        has_required_contract_fields: bool,
        lifecycle: RepoopsAuthorityClaimLifecycle,
    ) -> Result<Self, String> {
        if owner.trim().is_empty() {
            return Err("repoops claim facts require an owner".into());
        }
        if owner.trim() != owner {
            return Err("repoops claim fact owner must be normalized".into());
        }
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
        RawRepoopsAuthorityClaimFact {
            claim_id: self.claim_id.clone(),
            status: self.status(),
            owner: self.owner.clone(),
            scope_in: self.scope_in.iter().map(ToString::to_string).collect(),
            scope_out: self.scope_out.iter().map(ToString::to_string).collect(),
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
        Self::from_parts(
            raw.claim_id,
            raw.owner,
            raw.scope_in,
            raw.scope_out,
            raw.has_required_contract_fields,
            lifecycle,
        )
        .map_err(serde::de::Error::custom)
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
    fn parse(value: impl Into<String>) -> Result<Self, String> {
        Self::try_from(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RepoopsScopePattern {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            return Err("patterns must not be empty".to_owned());
        }
        if value.trim() != value {
            return Err("patterns must be normalized".to_owned());
        }
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
    let mut seen = HashSet::with_capacity(patterns.len());
    let mut parsed = Vec::with_capacity(patterns.len());
    for pattern in patterns {
        let pattern = RepoopsScopePattern::parse(pattern).map_err(|reason| {
            if reason == "patterns must not be empty" {
                format!("{label} patterns must not be empty")
            } else if reason == "patterns must be normalized" {
                format!("{label} patterns must be normalized")
            } else {
                format!("{label} {reason}")
            }
        })?;
        if !seen.insert(pattern.to_string()) {
            return Err(format!("{label} patterns must not contain duplicates"));
        }
        parsed.push(pattern);
    }
    Ok(parsed)
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
        path: String,
        owner: String,
        claim_id: RepoopsClaimRef,
    },
    ForeignOwner {
        path: String,
        owner: String,
        claim_id: RepoopsClaimRef,
    },
}

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
        validate_repoops_lock_path(&path)?;
        validate_repoops_lock_owner(&owner)?;
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
            Self::Owned { path, .. } | Self::ForeignOwner { path, .. } => path,
        }
    }

    fn owner(&self) -> &str {
        match self {
            Self::Owned { owner, .. } | Self::ForeignOwner { owner, .. } => owner,
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

fn validate_repoops_lock_path(path: &str) -> Result<(), String> {
    validate_repoops_project_path("repoops lock path", path)?;
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
    Ok(())
}

fn validate_repoops_lock_owner(owner: &str) -> Result<(), String> {
    validate_repoops_project_path("repoops lock owner", owner)
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
        policy_project_path: String,
        execution_project_path: String,
        repo_path_prefix: Option<String>,
    },
}

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
                validate_repoops_project_path(
                    "git_context.policy_project_path",
                    &policy_project_path,
                )?;
                validate_repoops_project_path(
                    "git_context.execution_project_path",
                    &execution_project_path,
                )?;
                let repo_path_prefix = repo_path_prefix
                    .map(|prefix| {
                        validate_repoops_repo_path_prefix(&prefix)?;
                        Ok::<_, String>(prefix)
                    })
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
            } => Some(policy_project_path),
        }
    }

    fn execution_project_path(&self) -> Option<&str> {
        match self {
            Self::Unknown => None,
            Self::KnownPaths {
                execution_project_path,
                ..
            } => Some(execution_project_path),
        }
    }

    fn repo_path_prefix(&self) -> Option<&str> {
        match self {
            Self::Unknown => None,
            Self::KnownPaths {
                repo_path_prefix, ..
            } => repo_path_prefix.as_deref(),
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
    Ok(())
}

fn validate_repoops_repo_path_prefix(prefix: &str) -> Result<(), String> {
    validate_repoops_project_path("git_context.repo_path_prefix", prefix)?;
    if prefix.starts_with('/') || prefix.starts_with('\\') {
        return Err("git_context.repo_path_prefix must be repo-relative".to_owned());
    }
    for part in prefix.replace('\\', "/").split('/') {
        if matches!(part, "" | "." | "..") {
            return Err("git_context.repo_path_prefix must be normalized".to_owned());
        }
    }
    Ok(())
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
    schema_version: String,
    agent_id: String,
    subject: RepoopsAuthoritySnapshotSubject,
    policy: RepoopsAuthorityPolicyFact,
    scope: RepoopsAuthorityScopeFact,
    locks: Vec<RepoopsAuthorityLockFact>,
    git_context: Option<RepoopsAuthorityGitContextFact>,
    fact_sources: Vec<String>,
}

/// Fields shared by all repoops authority snapshot subjects.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoopsAuthoritySnapshotCommon {
    schema_version: String,
    agent_id: String,
    policy: RepoopsAuthorityPolicyFact,
    scope: RepoopsAuthorityScopeFact,
    locks: Vec<RepoopsAuthorityLockFact>,
    git_context: Option<RepoopsAuthorityGitContextFact>,
    fact_sources: Vec<String>,
}

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
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
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

impl RepoopsAuthoritySnapshotCommon {
    /// Builds validated common repoops authority snapshot facts.
    ///
    /// # Errors
    ///
    /// Returns an error when identity or provenance fields are blank or padded.
    pub fn new(
        schema_version: String,
        agent_id: String,
        policy: RepoopsAuthorityPolicyFact,
        scope: RepoopsAuthorityScopeFact,
        locks: Vec<RepoopsAuthorityLockFact>,
        git_context: Option<RepoopsAuthorityGitContextFact>,
        fact_sources: Vec<String>,
    ) -> Result<Self, String> {
        validate_repoops_authority_snapshot_common(&schema_version, &agent_id, &fact_sources)?;
        Ok(Self {
            schema_version,
            agent_id,
            policy,
            scope,
            locks,
            git_context,
            fact_sources,
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
            &common.agent_id,
            Some((&claim_id, &claim.owner)),
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
        validate_repoops_authority_locks(&common.agent_id, None, &common.locks)?;
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
        &self.schema_version
    }

    /// Returns the agent selected by Covey for these authority facts.
    #[must_use]
    pub fn agent_id(&self) -> &str {
        &self.agent_id
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
    pub fn fact_sources(&self) -> &[String] {
        &self.fact_sources
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

impl Serialize for RepoopsAuthoritySnapshot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawRepoopsAuthoritySnapshot {
            schema_version: self.schema_version().to_owned(),
            agent_id: self.agent_id().to_owned(),
            claim_id: self.claim_id().cloned(),
            ownership_token: self.ownership_token().map(str::to_owned),
            override_token: self.override_token().map(str::to_owned),
            policy: self.policy().clone(),
            claim: self.claim().cloned(),
            scope: self.scope().clone(),
            locks: self.locks.clone(),
            git_context: self.git_context().cloned(),
            constraint_reason: self.constraint_reason().map(str::to_owned),
            fact_sources: self.fact_sources().to_vec(),
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
        let subject = RepoopsAuthoritySnapshotSubject::try_from_parts(
            raw.claim_id,
            raw.ownership_token,
            raw.override_token,
            raw.claim,
            raw.constraint_reason,
        )
        .map_err(serde::de::Error::custom)?;
        validate_repoops_authority_locks(
            &common.agent_id,
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

fn validate_repoops_authority_snapshot_common(
    schema_version: &str,
    agent_id: &str,
    fact_sources: &[String],
) -> Result<(), String> {
    if schema_version.trim().is_empty() {
        return Err("repoops authority snapshots require schema_version".into());
    }
    if schema_version.trim() != schema_version {
        return Err("repoops authority snapshot schema_version must be normalized".into());
    }
    if agent_id.trim().is_empty() {
        return Err("repoops authority snapshots require agent_id".into());
    }
    if agent_id.trim() != agent_id {
        return Err("repoops authority snapshot agent_id must be normalized".into());
    }
    for source in fact_sources {
        if source.trim().is_empty() {
            return Err("repoops authority snapshot fact_sources must be non-empty".into());
        }
        if source.trim() != source {
            return Err("repoops authority snapshot fact_sources must be normalized".into());
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

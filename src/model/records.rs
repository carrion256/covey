use derive_new::new;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{
    AbandonSubtaskReq, ActorKind, ArtifactDigest, ArtifactKind, BaseRev, CancelMetaTaskReq,
    ClaimId, ClaimResult, ClaimState, CommandTranscriptDigest, ConflictResolutionState,
    CreateSubtaskRequest, DecideReviewReq, EnqueueForApplyReq, EventType, ExitSessionReq, FenceSeq,
    FindingsDigest, HeartbeatReq, ImportOpenSpecEvent, LeaseDeadlineMs, MarkAppliedReq, MetaTaskId,
    MetaTaskState, ModelId, ObjectType, ProviderId, PublishArtifactReq, QueueId, ReadyQueueClaim,
    ReadyQueueState, RecordApplyVerificationReq, RecordRuntimeAttestationReq, ReleaseClaimReq,
    RequestReservationReq, RequestReviewReq, ReservationId, ReservationState, ResolveConflictReq,
    ReviewId, ReviewState, ReviewVerdict, ScopeClass, SessionHandle, SessionRole, SessionState,
    SessionToken, SettlementTarget, StartSubtaskReq, SubmitMetaTaskReq, SubtaskId, SubtaskKind,
    SubtaskState, SupersedeQueueItemReq, TimestampMs,
};

/// Persisted session row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub session_token: SessionToken,
    pub agent_principal_id: String,
    pub agent_instance_id: String,
    pub role: SessionRole,
    lifecycle: SessionLifecycle,
    pub last_heartbeat_at: TimestampMs,
    pub last_heartbeat_tick: i64,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionLifecycle {
    Active {
        active_subtask_id: Option<SubtaskId>,
    },
    Stale,
    Exited,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawSession {
    session_token: SessionToken,
    agent_principal_id: String,
    agent_instance_id: String,
    role: SessionRole,
    state: SessionState,
    active_subtask_id: Option<SubtaskId>,
    last_heartbeat_at: TimestampMs,
    last_heartbeat_tick: i64,
    created_at: TimestampMs,
    updated_at: TimestampMs,
}

impl Session {
    /// Builds one session from the flat storage shape, rejecting invalid lifecycle fields.
    #[allow(clippy::too_many_arguments)]
    pub fn try_from_parts(
        session_token: SessionToken,
        agent_principal_id: impl Into<String>,
        agent_instance_id: impl Into<String>,
        role: SessionRole,
        state: SessionState,
        active_subtask_id: Option<SubtaskId>,
        last_heartbeat_at: TimestampMs,
        last_heartbeat_tick: i64,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> Result<Self, String> {
        let lifecycle = SessionLifecycle::from_parts(state, active_subtask_id)?;
        Ok(Self {
            session_token,
            agent_principal_id: agent_principal_id.into(),
            agent_instance_id: agent_instance_id.into(),
            role,
            lifecycle,
            last_heartbeat_at,
            last_heartbeat_tick,
            created_at,
            updated_at,
        })
    }

    /// Returns the persisted session lifecycle state.
    #[must_use]
    pub const fn state(&self) -> SessionState {
        self.lifecycle.state()
    }

    /// Returns the active subtask id for active sessions that are currently occupied.
    #[must_use]
    pub const fn active_subtask_id(&self) -> Option<&SubtaskId> {
        self.lifecycle.active_subtask_id()
    }
}

impl SessionLifecycle {
    fn from_parts(
        state: SessionState,
        active_subtask_id: Option<SubtaskId>,
    ) -> Result<Self, String> {
        match state {
            SessionState::Active => Ok(Self::Active { active_subtask_id }),
            SessionState::Stale => {
                if active_subtask_id.is_some() {
                    Err("stale session must not include active_subtask_id".to_owned())
                } else {
                    Ok(Self::Stale)
                }
            }
            SessionState::Exited => {
                if active_subtask_id.is_some() {
                    Err("exited session must not include active_subtask_id".to_owned())
                } else {
                    Ok(Self::Exited)
                }
            }
        }
    }

    const fn state(&self) -> SessionState {
        match self {
            Self::Active { .. } => SessionState::Active,
            Self::Stale => SessionState::Stale,
            Self::Exited => SessionState::Exited,
        }
    }

    const fn active_subtask_id(&self) -> Option<&SubtaskId> {
        match self {
            Self::Active { active_subtask_id } => active_subtask_id.as_ref(),
            Self::Stale | Self::Exited => None,
        }
    }
}

impl Serialize for Session {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawSession::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Session {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawSession::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl From<&Session> for RawSession {
    fn from(session: &Session) -> Self {
        Self {
            session_token: session.session_token.clone(),
            agent_principal_id: session.agent_principal_id.clone(),
            agent_instance_id: session.agent_instance_id.clone(),
            role: session.role,
            state: session.state(),
            active_subtask_id: session.active_subtask_id().cloned(),
            last_heartbeat_at: session.last_heartbeat_at,
            last_heartbeat_tick: session.last_heartbeat_tick,
            created_at: session.created_at,
            updated_at: session.updated_at,
        }
    }
}

impl TryFrom<RawSession> for Session {
    type Error = String;

    fn try_from(raw: RawSession) -> Result<Self, Self::Error> {
        Self::try_from_parts(
            raw.session_token,
            raw.agent_principal_id,
            raw.agent_instance_id,
            raw.role,
            raw.state,
            raw.active_subtask_id,
            raw.last_heartbeat_at,
            raw.last_heartbeat_tick,
            raw.created_at,
            raw.updated_at,
        )
    }
}

/// Runtime identity evidence bound to one Covey session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAttestation {
    pub session_token: SessionToken,
    pub agent_principal_id: String,
    pub agent_instance_id: String,
    pub role: SessionRole,
    pub provider: ProviderId,
    pub model: ModelId,
    pub provider_run_id: String,
    pub provider_run_id_issuer: String,
    pub process_id: Option<String>,
    pub container_id: Option<String>,
    pub command_transcript_digest: CommandTranscriptDigest,
    pub started_at: TimestampMs,
    pub ended_at: TimestampMs,
    pub recorded_at: TimestampMs,
}

/// Persisted meta-task row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaTask {
    pub meta_task_id: MetaTaskId,
    pub prompt_text: String,
    pub state: MetaTaskState,
    pub created_by: SessionToken,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// Exact persisted shape of a row in the `subtasks` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SubtaskRow {
    pub subtask_id: SubtaskId,
    pub meta_task_id: MetaTaskId,
    pub title: String,
    pub kind: SubtaskKind,
    pub review_target_subtask_id: Option<SubtaskId>,
    pub review_target_artifact_digest: Option<ArtifactDigest>,
    pub state: SubtaskState,
    pub current_claim_id: Option<ClaimId>,
    pub artifact_digest: Option<ArtifactDigest>,
    pub priority: i64,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// Review target encoded by a review subtask.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct ReviewTarget {
    pub subtask_id: SubtaskId,
    pub artifact_digest: ArtifactDigest,
}

/// Mutable lifecycle data shared by work and review subtasks.
///
/// The variant shape encodes the required claim/artifact fields for each
/// lifecycle state instead of exposing `state` plus nullable columns.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubtaskLifecycle {
    Available,
    Claimed {
        active_claim_id: ClaimId,
    },
    InProgress {
        active_claim_id: ClaimId,
    },
    ArtifactPublished {
        active_claim_id: Option<ClaimId>,
        artifact_digest: ArtifactDigest,
    },
    ReviewPending {
        active_claim_id: Option<ClaimId>,
        artifact_digest: ArtifactDigest,
    },
    ChangesRequested {
        active_claim_id: Option<ClaimId>,
        artifact_digest: ArtifactDigest,
    },
    Approved {
        active_claim_id: Option<ClaimId>,
        artifact_digest: ArtifactDigest,
    },
    Decided,
    ReadyForApply {
        active_claim_id: Option<ClaimId>,
        artifact_digest: ArtifactDigest,
    },
    Applied {
        active_claim_id: Option<ClaimId>,
        artifact_digest: ArtifactDigest,
    },
    Abandoned {
        artifact_digest: Option<ArtifactDigest>,
    },
}

impl SubtaskLifecycle {
    pub(super) fn from_row_parts(
        state: SubtaskState,
        active_claim_id: Option<ClaimId>,
        artifact_digest: Option<ArtifactDigest>,
    ) -> rusqlite::Result<Self> {
        match state {
            SubtaskState::Available => {
                if active_claim_id.is_some() || artifact_digest.is_some() {
                    return Err(invalid_subtask_row(
                        "available subtask cannot carry claim or artifact state",
                    ));
                }
                Ok(Self::Available)
            }
            SubtaskState::Claimed => {
                let Some(active_claim_id) = active_claim_id else {
                    return Err(invalid_subtask_row(
                        "claimed subtask is missing active claim",
                    ));
                };
                if artifact_digest.is_some() {
                    return Err(invalid_subtask_row(
                        "claimed subtask cannot carry artifact state",
                    ));
                }
                Ok(Self::Claimed { active_claim_id })
            }
            SubtaskState::InProgress => {
                let Some(active_claim_id) = active_claim_id else {
                    return Err(invalid_subtask_row(
                        "in-progress subtask is missing active claim",
                    ));
                };
                if artifact_digest.is_some() {
                    return Err(invalid_subtask_row(
                        "in-progress subtask cannot carry artifact state",
                    ));
                }
                Ok(Self::InProgress { active_claim_id })
            }
            SubtaskState::ArtifactPublished => Ok(Self::ArtifactPublished {
                active_claim_id,
                artifact_digest: require_subtask_artifact(
                    artifact_digest,
                    "artifact-published subtask is missing artifact digest",
                )?,
            }),
            SubtaskState::ReviewPending => Ok(Self::ReviewPending {
                active_claim_id,
                artifact_digest: require_subtask_artifact(
                    artifact_digest,
                    "review-pending subtask is missing artifact digest",
                )?,
            }),
            SubtaskState::ChangesRequested => Ok(Self::ChangesRequested {
                active_claim_id,
                artifact_digest: require_subtask_artifact(
                    artifact_digest,
                    "changes-requested subtask is missing artifact digest",
                )?,
            }),
            SubtaskState::Approved => Ok(Self::Approved {
                active_claim_id,
                artifact_digest: require_subtask_artifact(
                    artifact_digest,
                    "approved subtask is missing artifact digest",
                )?,
            }),
            SubtaskState::Decided => {
                if active_claim_id.is_some() || artifact_digest.is_some() {
                    return Err(invalid_subtask_row(
                        "decided review subtask cannot carry claim or artifact state",
                    ));
                }
                Ok(Self::Decided)
            }
            SubtaskState::ReadyForApply => Ok(Self::ReadyForApply {
                active_claim_id,
                artifact_digest: require_subtask_artifact(
                    artifact_digest,
                    "ready-for-apply subtask is missing artifact digest",
                )?,
            }),
            SubtaskState::Applied => Ok(Self::Applied {
                active_claim_id,
                artifact_digest: require_subtask_artifact(
                    artifact_digest,
                    "applied subtask is missing artifact digest",
                )?,
            }),
            SubtaskState::Abandoned => {
                if active_claim_id.is_some() {
                    return Err(invalid_subtask_row(
                        "abandoned subtask cannot carry active claim",
                    ));
                }
                Ok(Self::Abandoned { artifact_digest })
            }
        }
    }

    #[must_use]
    pub const fn state(&self) -> SubtaskState {
        match self {
            Self::Available => SubtaskState::Available,
            Self::Claimed { .. } => SubtaskState::Claimed,
            Self::InProgress { .. } => SubtaskState::InProgress,
            Self::ArtifactPublished { .. } => SubtaskState::ArtifactPublished,
            Self::ReviewPending { .. } => SubtaskState::ReviewPending,
            Self::ChangesRequested { .. } => SubtaskState::ChangesRequested,
            Self::Approved { .. } => SubtaskState::Approved,
            Self::Decided => SubtaskState::Decided,
            Self::ReadyForApply { .. } => SubtaskState::ReadyForApply,
            Self::Applied { .. } => SubtaskState::Applied,
            Self::Abandoned { .. } => SubtaskState::Abandoned,
        }
    }

    #[must_use]
    pub fn active_claim_id(&self) -> Option<&ClaimId> {
        match self {
            Self::Claimed { active_claim_id } | Self::InProgress { active_claim_id } => {
                Some(active_claim_id)
            }
            Self::ArtifactPublished {
                active_claim_id, ..
            }
            | Self::ReviewPending {
                active_claim_id, ..
            }
            | Self::ChangesRequested {
                active_claim_id, ..
            }
            | Self::Approved {
                active_claim_id, ..
            }
            | Self::ReadyForApply {
                active_claim_id, ..
            }
            | Self::Applied {
                active_claim_id, ..
            } => active_claim_id.as_ref(),
            Self::Available | Self::Decided | Self::Abandoned { .. } => None,
        }
    }

    #[must_use]
    pub fn artifact_digest(&self) -> Option<&ArtifactDigest> {
        match self {
            Self::ArtifactPublished {
                artifact_digest, ..
            }
            | Self::ReviewPending {
                artifact_digest, ..
            }
            | Self::ChangesRequested {
                artifact_digest, ..
            }
            | Self::Approved {
                artifact_digest, ..
            }
            | Self::ReadyForApply {
                artifact_digest, ..
            }
            | Self::Applied {
                artifact_digest, ..
            } => Some(artifact_digest),
            Self::Abandoned { artifact_digest } => artifact_digest.as_ref(),
            Self::Available | Self::Claimed { .. } | Self::InProgress { .. } | Self::Decided => {
                None
            }
        }
    }
}

fn require_subtask_artifact(
    artifact_digest: Option<ArtifactDigest>,
    missing_reason: &str,
) -> rusqlite::Result<ArtifactDigest> {
    artifact_digest.ok_or_else(|| invalid_subtask_row(missing_reason))
}

/// Domain object for executable work.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct WorkSubtask {
    pub subtask_id: SubtaskId,
    pub meta_task_id: MetaTaskId,
    pub title: String,
    pub lifecycle: SubtaskLifecycle,
    pub priority: i64,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// Domain object for review work bound to one artifact of one work subtask.
#[must_use]
#[allow(clippy::too_many_arguments)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct ReviewSubtask {
    pub subtask_id: SubtaskId,
    pub meta_task_id: MetaTaskId,
    pub title: String,
    pub review_target: ReviewTarget,
    pub lifecycle: SubtaskLifecycle,
    pub priority: i64,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// Domain representation of a subtask.
///
/// Work subtasks and review subtasks are distinct variants so the domain API
/// cannot represent a review target on normal work or a review without a
/// target artifact.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Subtask {
    Work(WorkSubtask),
    Review(ReviewSubtask),
}

impl Subtask {
    pub fn subtask_id(&self) -> &str {
        match self {
            Self::Work(subtask) => &subtask.subtask_id,
            Self::Review(subtask) => &subtask.subtask_id,
        }
    }

    pub fn meta_task_id(&self) -> &str {
        match self {
            Self::Work(subtask) => &subtask.meta_task_id,
            Self::Review(subtask) => &subtask.meta_task_id,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Work(subtask) => &subtask.title,
            Self::Review(subtask) => &subtask.title,
        }
    }

    pub fn kind(&self) -> SubtaskKind {
        match self {
            Self::Work(_) => SubtaskKind::Work,
            Self::Review(_) => SubtaskKind::Review,
        }
    }

    pub fn lifecycle(&self) -> &SubtaskLifecycle {
        match self {
            Self::Work(subtask) => &subtask.lifecycle,
            Self::Review(subtask) => &subtask.lifecycle,
        }
    }

    pub fn review_target(&self) -> Option<&ReviewTarget> {
        match self {
            Self::Work(_) => None,
            Self::Review(subtask) => Some(&subtask.review_target),
        }
    }
}

impl TryFrom<SubtaskRow> for Subtask {
    type Error = rusqlite::Error;

    fn try_from(row: SubtaskRow) -> Result<Self, Self::Error> {
        let lifecycle =
            SubtaskLifecycle::from_row_parts(row.state, row.current_claim_id, row.artifact_digest)?;
        match row.kind {
            SubtaskKind::Work => {
                if row.review_target_subtask_id.is_some()
                    || row.review_target_artifact_digest.is_some()
                {
                    return Err(invalid_subtask_row("work subtask has a review target"));
                }
                Ok(Self::Work(WorkSubtask::new(
                    row.subtask_id,
                    row.meta_task_id,
                    row.title,
                    lifecycle,
                    row.priority,
                    row.created_at,
                    row.updated_at,
                )))
            }
            SubtaskKind::Review => {
                let Some(target_subtask_id) = row.review_target_subtask_id else {
                    return Err(invalid_subtask_row(
                        "review subtask is missing target subtask",
                    ));
                };
                let Some(target_artifact_digest) = row.review_target_artifact_digest else {
                    return Err(invalid_subtask_row(
                        "review subtask is missing target artifact",
                    ));
                };
                Ok(Self::Review(ReviewSubtask::new(
                    row.subtask_id,
                    row.meta_task_id,
                    row.title,
                    ReviewTarget::new(target_subtask_id, target_artifact_digest),
                    lifecycle,
                    row.priority,
                    row.created_at,
                    row.updated_at,
                )))
            }
        }
    }
}

fn invalid_subtask_row(reason: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            reason.to_owned(),
        )),
    )
}

/// Persisted claim row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub claim_id: ClaimId,
    pub subtask_id: SubtaskId,
    pub owner_session_token: SessionToken,
    pub fence_seq: FenceSeq,
    pub lease_deadline: LeaseDeadlineMs,
    pub state: ClaimState,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// Persisted artifact row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub artifact_digest: ArtifactDigest,
    pub artifact_kind: ArtifactKind,
    pub base_rev: BaseRev,
    pub produced_by_subtask_id: SubtaskId,
    pub produced_by_session: SessionToken,
    pub manifest_path: String,
    pub changed_paths_digest: ArtifactDigest,
    pub created_at: TimestampMs,
}

/// Fields shared by every persisted review state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewCommon {
    pub review_id: ReviewId,
    pub subtask_id: SubtaskId,
    pub artifact_digest: ArtifactDigest,
    pub reviewer_session: SessionToken,
    pub review_subtask_id: SubtaskId,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// Persisted review row in the requested state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestedReview {
    pub common: ReviewCommon,
}

/// Persisted review row in the in-progress state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InProgressReview {
    pub common: ReviewCommon,
}

/// Persisted review row after a reviewer decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecidedReview {
    pub common: ReviewCommon,
    pub verdict: ReviewVerdict,
    pub findings_digest: FindingsDigest,
}

/// Persisted review row superseded by a newer artifact or review round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupersededReview {
    pub common: ReviewCommon,
}

/// Persisted review row.
///
/// The enum shape makes decision evidence state-dependent: decided reviews
/// always carry a verdict and findings digest, while non-decided reviews cannot.
/// Serialization remains the flat database/API row shape for compatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Review {
    Requested(RequestedReview),
    InProgress(InProgressReview),
    Decided(DecidedReview),
    Superseded(SupersededReview),
}

/// Persisted reservation row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reservation {
    pub reservation_id: ReservationId,
    pub owner_subtask_id: SubtaskId,
    scope: ReservationScope,
    pub lease_deadline: LeaseDeadlineMs,
    pub state: ReservationState,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// Scope covered by a persisted reservation.
///
/// The enum prevents mixing generated-set members with exact, subtree, or
/// repo-global scopes, and prevents generated-set reservations with no members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReservationScope {
    ExactPath {
        scope_key: String,
    },
    Subtree {
        scope_key: String,
    },
    RepoGlobal,
    GeneratedSet {
        scope_key: String,
        generated_members: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawReservation {
    reservation_id: ReservationId,
    owner_subtask_id: SubtaskId,
    scope_class: ScopeClass,
    scope_key: String,
    generated_members: Vec<String>,
    lease_deadline: LeaseDeadlineMs,
    state: ReservationState,
    created_at: TimestampMs,
    updated_at: TimestampMs,
}

impl Reservation {
    #[allow(clippy::too_many_arguments)]
    pub fn try_from_parts(
        reservation_id: ReservationId,
        owner_subtask_id: SubtaskId,
        scope_class: ScopeClass,
        scope_key: impl Into<String>,
        generated_members: Vec<String>,
        lease_deadline: LeaseDeadlineMs,
        state: ReservationState,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> Result<Self, String> {
        let scope = ReservationScope::from_parts(scope_class, scope_key.into(), generated_members)?;
        Ok(Self {
            reservation_id,
            owner_subtask_id,
            scope,
            lease_deadline,
            state,
            created_at,
            updated_at,
        })
    }

    #[must_use]
    pub const fn scope(&self) -> &ReservationScope {
        &self.scope
    }

    #[must_use]
    pub const fn scope_class(&self) -> ScopeClass {
        self.scope.scope_class()
    }

    #[must_use]
    pub fn scope_key(&self) -> &str {
        self.scope.scope_key()
    }

    #[must_use]
    pub fn generated_members(&self) -> &[String] {
        self.scope.generated_members()
    }
}

impl ReservationScope {
    fn from_parts(
        scope_class: ScopeClass,
        scope_key: String,
        generated_members: Vec<String>,
    ) -> Result<Self, String> {
        match scope_class {
            ScopeClass::ExactPath => {
                reject_generated_members(scope_class, &generated_members)?;
                require_non_empty_scope_key(scope_class, &scope_key)?;
                Ok(Self::ExactPath { scope_key })
            }
            ScopeClass::Subtree => {
                reject_generated_members(scope_class, &generated_members)?;
                require_non_empty_scope_key(scope_class, &scope_key)?;
                Ok(Self::Subtree { scope_key })
            }
            ScopeClass::RepoGlobal => {
                reject_generated_members(scope_class, &generated_members)?;
                if scope_key != "repo" {
                    return Err("repo-global reservations require scope_key `repo`".to_owned());
                }
                Ok(Self::RepoGlobal)
            }
            ScopeClass::GeneratedSet => {
                require_non_empty_scope_key(scope_class, &scope_key)?;
                if generated_members.is_empty() {
                    return Err("generated-set reservations require generated_members".to_owned());
                }
                if generated_members
                    .iter()
                    .any(|member| member.trim().is_empty())
                {
                    return Err(
                        "generated-set reservations require non-empty generated_members".to_owned(),
                    );
                }
                Ok(Self::GeneratedSet {
                    scope_key,
                    generated_members,
                })
            }
        }
    }

    const fn scope_class(&self) -> ScopeClass {
        match self {
            Self::ExactPath { .. } => ScopeClass::ExactPath,
            Self::Subtree { .. } => ScopeClass::Subtree,
            Self::RepoGlobal => ScopeClass::RepoGlobal,
            Self::GeneratedSet { .. } => ScopeClass::GeneratedSet,
        }
    }

    fn scope_key(&self) -> &str {
        match self {
            Self::ExactPath { scope_key }
            | Self::Subtree { scope_key }
            | Self::GeneratedSet { scope_key, .. } => scope_key,
            Self::RepoGlobal => "repo",
        }
    }

    fn generated_members(&self) -> &[String] {
        match self {
            Self::GeneratedSet {
                generated_members, ..
            } => generated_members,
            Self::ExactPath { .. } | Self::Subtree { .. } | Self::RepoGlobal => &[],
        }
    }
}

fn require_non_empty_scope_key(scope_class: ScopeClass, scope_key: &str) -> Result<(), String> {
    if scope_key.trim().is_empty() {
        Err(format!("{scope_class} reservations require scope_key"))
    } else {
        Ok(())
    }
}

fn reject_generated_members(
    scope_class: ScopeClass,
    generated_members: &[String],
) -> Result<(), String> {
    if generated_members.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{scope_class} reservations must not include generated_members"
        ))
    }
}

impl Serialize for Reservation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawReservation::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Reservation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawReservation::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl From<&Reservation> for RawReservation {
    fn from(reservation: &Reservation) -> Self {
        Self {
            reservation_id: reservation.reservation_id.clone(),
            owner_subtask_id: reservation.owner_subtask_id.clone(),
            scope_class: reservation.scope_class(),
            scope_key: reservation.scope_key().to_owned(),
            generated_members: reservation.generated_members().to_vec(),
            lease_deadline: reservation.lease_deadline,
            state: reservation.state,
            created_at: reservation.created_at,
            updated_at: reservation.updated_at,
        }
    }
}

impl TryFrom<RawReservation> for Reservation {
    type Error = String;

    fn try_from(raw: RawReservation) -> Result<Self, Self::Error> {
        Self::try_from_parts(
            raw.reservation_id,
            raw.owner_subtask_id,
            raw.scope_class,
            raw.scope_key,
            raw.generated_members,
            raw.lease_deadline,
            raw.state,
            raw.created_at,
            raw.updated_at,
        )
    }
}

/// Fields shared by every persisted ready-queue state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyQueueCommon {
    pub queue_id: QueueId,
    pub artifact_digest: ArtifactDigest,
    pub subtask_id: SubtaskId,
    pub settlement_target: SettlementTarget,
    pub enqueued_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// Queued apply item. A prior fence may be retained as a monotonic counter, but
/// there is no active queue claim in this state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedReadyQueueItem {
    pub common: ReadyQueueCommon,
    pub last_claim_fence_seq: Option<FenceSeq>,
}

/// Active claim fields for an in-flight apply item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyQueueActiveClaim {
    pub claimed_by_session_token: SessionToken,
    pub claim_fence_seq: FenceSeq,
    pub claim_lease_deadline: LeaseDeadlineMs,
}

/// In-flight apply item. All active claim fields are required together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InFlightReadyQueueItem {
    pub common: ReadyQueueCommon,
    pub claim: ReadyQueueActiveClaim,
}

/// Applied queue item bound to the fence that was accepted by the apply gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedReadyQueueItem {
    pub common: ReadyQueueCommon,
    pub claim_fence_seq: FenceSeq,
}

/// Superseded apply item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupersededReadyQueueItem {
    pub common: ReadyQueueCommon,
    pub last_claim_fence_seq: Option<FenceSeq>,
}

/// Cancelled apply item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelledReadyQueueItem {
    pub common: ReadyQueueCommon,
    pub last_claim_fence_seq: Option<FenceSeq>,
}

/// Persisted ready-queue row.
///
/// The enum shape prevents an in-flight item without active claim fields, and
/// prevents queued/cancelled/superseded rows from carrying an active claimant or
/// lease deadline. Serialization remains the flat database/API row shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadyQueueItem {
    Queued(QueuedReadyQueueItem),
    InFlight(InFlightReadyQueueItem),
    Applied(AppliedReadyQueueItem),
    Superseded(SupersededReadyQueueItem),
    Cancelled(CancelledReadyQueueItem),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawReview {
    review_id: String,
    subtask_id: String,
    artifact_digest: String,
    reviewer_session: String,
    review_subtask_id: Option<String>,
    verdict: Option<ReviewVerdict>,
    findings_digest: Option<String>,
    state: ReviewState,
    created_at: i64,
    updated_at: i64,
}

impl Review {
    #[must_use]
    pub fn review_id(&self) -> &str {
        self.common().review_id.as_str()
    }

    #[must_use]
    pub fn subtask_id(&self) -> &str {
        self.common().subtask_id.as_str()
    }

    #[must_use]
    pub fn artifact_digest(&self) -> &str {
        self.common().artifact_digest.as_str()
    }

    #[must_use]
    pub fn reviewer_session(&self) -> &str {
        self.common().reviewer_session.as_str()
    }

    #[must_use]
    pub fn review_subtask_id(&self) -> &str {
        self.common().review_subtask_id.as_str()
    }

    #[must_use]
    pub const fn verdict(&self) -> Option<ReviewVerdict> {
        match self {
            Self::Decided(review) => Some(review.verdict),
            Self::Requested(_) | Self::InProgress(_) | Self::Superseded(_) => None,
        }
    }

    #[must_use]
    pub fn findings_digest(&self) -> Option<&str> {
        match self {
            Self::Decided(review) => Some(review.findings_digest.as_str()),
            Self::Requested(_) | Self::InProgress(_) | Self::Superseded(_) => None,
        }
    }

    #[must_use]
    pub const fn state(&self) -> ReviewState {
        match self {
            Self::Requested(_) => ReviewState::Requested,
            Self::InProgress(_) => ReviewState::InProgress,
            Self::Decided(_) => ReviewState::Decided,
            Self::Superseded(_) => ReviewState::Superseded,
        }
    }

    #[must_use]
    pub const fn created_at(&self) -> i64 {
        self.common().created_at.get()
    }

    #[must_use]
    pub const fn updated_at(&self) -> i64 {
        self.common().updated_at.get()
    }

    const fn common(&self) -> &ReviewCommon {
        match self {
            Self::Requested(review) => &review.common,
            Self::InProgress(review) => &review.common,
            Self::Decided(review) => &review.common,
            Self::Superseded(review) => &review.common,
        }
    }
}

impl TryFrom<RawReview> for Review {
    type Error = String;

    fn try_from(raw: RawReview) -> Result<Self, Self::Error> {
        let review_subtask_id = raw
            .review_subtask_id
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "review_subtask_id is required for persisted review rows".to_owned())?;
        let common = ReviewCommon {
            review_id: ReviewId::parse(raw.review_id).map_err(|err| err.to_string())?,
            subtask_id: SubtaskId::parse(raw.subtask_id).map_err(|err| err.to_string())?,
            artifact_digest: ArtifactDigest::parse(raw.artifact_digest)
                .map_err(|err| err.to_string())?,
            reviewer_session: SessionToken::parse(raw.reviewer_session)
                .map_err(|err| err.to_string())?,
            review_subtask_id: SubtaskId::parse(review_subtask_id)
                .map_err(|err| err.to_string())?,
            created_at: TimestampMs::parse(raw.created_at).map_err(|err| err.to_string())?,
            updated_at: TimestampMs::parse(raw.updated_at).map_err(|err| err.to_string())?,
        };

        match raw.state {
            ReviewState::Requested => {
                if raw.verdict.is_some() || raw.findings_digest.is_some() {
                    return Err("requested reviews cannot carry decision evidence".to_owned());
                }
                Ok(Self::Requested(RequestedReview { common }))
            }
            ReviewState::InProgress => {
                if raw.verdict.is_some() || raw.findings_digest.is_some() {
                    return Err("in-progress reviews cannot carry decision evidence".to_owned());
                }
                Ok(Self::InProgress(InProgressReview { common }))
            }
            ReviewState::Decided => {
                let verdict = raw
                    .verdict
                    .ok_or_else(|| "decided reviews require verdict".to_owned())?;
                let findings_digest = raw
                    .findings_digest
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        "decided reviews require non-empty findings_digest".to_owned()
                    })?;
                Ok(Self::Decided(DecidedReview {
                    common,
                    verdict,
                    findings_digest: FindingsDigest::parse(findings_digest)
                        .map_err(|err| err.to_string())?,
                }))
            }
            ReviewState::Superseded => {
                if raw.verdict.is_some() || raw.findings_digest.is_some() {
                    return Err("superseded reviews cannot carry decision evidence".to_owned());
                }
                Ok(Self::Superseded(SupersededReview { common }))
            }
        }
    }
}

impl From<&Review> for RawReview {
    fn from(review: &Review) -> Self {
        Self {
            review_id: review.review_id().to_owned(),
            subtask_id: review.subtask_id().to_owned(),
            artifact_digest: review.artifact_digest().to_owned(),
            reviewer_session: review.reviewer_session().to_owned(),
            review_subtask_id: Some(review.review_subtask_id().to_owned()),
            verdict: review.verdict(),
            findings_digest: review.findings_digest().map(ToOwned::to_owned),
            state: review.state(),
            created_at: review.created_at(),
            updated_at: review.updated_at(),
        }
    }
}

impl Serialize for Review {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawReview::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Review {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawReview::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawReadyQueueItem {
    queue_id: String,
    artifact_digest: String,
    subtask_id: String,
    settlement_target: SettlementTarget,
    state: ReadyQueueState,
    claimed_by_session_token: Option<String>,
    claim_fence_seq: Option<i64>,
    claim_lease_deadline: Option<i64>,
    enqueued_at: i64,
    updated_at: i64,
}

impl ReadyQueueItem {
    #[must_use]
    pub fn queue_id(&self) -> &str {
        self.common().queue_id.as_str()
    }

    #[must_use]
    pub fn artifact_digest(&self) -> &str {
        self.common().artifact_digest.as_str()
    }

    #[must_use]
    pub fn subtask_id(&self) -> &str {
        self.common().subtask_id.as_str()
    }

    #[must_use]
    pub const fn settlement_target(&self) -> SettlementTarget {
        self.common().settlement_target
    }

    #[must_use]
    pub const fn state(&self) -> ReadyQueueState {
        match self {
            Self::Queued(_) => ReadyQueueState::Queued,
            Self::InFlight(_) => ReadyQueueState::InFlight,
            Self::Applied(_) => ReadyQueueState::Applied,
            Self::Superseded(_) => ReadyQueueState::Superseded,
            Self::Cancelled(_) => ReadyQueueState::Cancelled,
        }
    }

    #[must_use]
    pub fn claimed_by_session_token(&self) -> Option<&str> {
        match self {
            Self::InFlight(item) => Some(item.claim.claimed_by_session_token.as_str()),
            Self::Queued(_) | Self::Applied(_) | Self::Superseded(_) | Self::Cancelled(_) => None,
        }
    }

    #[must_use]
    pub fn claim_fence_seq(&self) -> Option<i64> {
        match self {
            Self::Queued(item) => item.last_claim_fence_seq.map(FenceSeq::get),
            Self::InFlight(item) => Some(item.claim.claim_fence_seq.get()),
            Self::Applied(item) => Some(item.claim_fence_seq.get()),
            Self::Superseded(item) => item.last_claim_fence_seq.map(FenceSeq::get),
            Self::Cancelled(item) => item.last_claim_fence_seq.map(FenceSeq::get),
        }
    }

    #[must_use]
    pub fn claim_lease_deadline(&self) -> Option<i64> {
        match self {
            Self::InFlight(item) => Some(item.claim.claim_lease_deadline.get()),
            Self::Queued(_) | Self::Applied(_) | Self::Superseded(_) | Self::Cancelled(_) => None,
        }
    }

    #[must_use]
    pub const fn enqueued_at(&self) -> i64 {
        self.common().enqueued_at.get()
    }

    #[must_use]
    pub const fn updated_at(&self) -> i64 {
        self.common().updated_at.get()
    }

    const fn common(&self) -> &ReadyQueueCommon {
        match self {
            Self::Queued(item) => &item.common,
            Self::InFlight(item) => &item.common,
            Self::Applied(item) => &item.common,
            Self::Superseded(item) => &item.common,
            Self::Cancelled(item) => &item.common,
        }
    }
}

impl TryFrom<RawReadyQueueItem> for ReadyQueueItem {
    type Error = String;

    fn try_from(raw: RawReadyQueueItem) -> Result<Self, Self::Error> {
        let common = ReadyQueueCommon {
            queue_id: QueueId::parse(raw.queue_id).map_err(|err| err.to_string())?,
            artifact_digest: ArtifactDigest::parse(raw.artifact_digest)
                .map_err(|err| err.to_string())?,
            subtask_id: SubtaskId::parse(raw.subtask_id).map_err(|err| err.to_string())?,
            settlement_target: raw.settlement_target,
            enqueued_at: TimestampMs::parse(raw.enqueued_at).map_err(|err| err.to_string())?,
            updated_at: TimestampMs::parse(raw.updated_at).map_err(|err| err.to_string())?,
        };

        match raw.state {
            ReadyQueueState::Queued => {
                if raw.claimed_by_session_token.is_some() || raw.claim_lease_deadline.is_some() {
                    return Err("queued ready-queue items cannot carry an active claim".to_owned());
                }
                Ok(Self::Queued(QueuedReadyQueueItem {
                    common,
                    last_claim_fence_seq: raw
                        .claim_fence_seq
                        .map(FenceSeq::parse)
                        .transpose()
                        .map_err(|err| err.to_string())?,
                }))
            }
            ReadyQueueState::InFlight => {
                let claimed_by_session_token = raw
                    .claimed_by_session_token
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        "in-flight ready-queue items require claimed_by_session_token".to_owned()
                    })?;
                let claim_fence_seq = raw.claim_fence_seq.ok_or_else(|| {
                    "in-flight ready-queue items require claim_fence_seq".to_owned()
                })?;
                let claim_lease_deadline = raw.claim_lease_deadline.ok_or_else(|| {
                    "in-flight ready-queue items require claim_lease_deadline".to_owned()
                })?;
                Ok(Self::InFlight(InFlightReadyQueueItem {
                    common,
                    claim: ReadyQueueActiveClaim {
                        claimed_by_session_token: SessionToken::parse(claimed_by_session_token)
                            .map_err(|err| err.to_string())?,
                        claim_fence_seq: FenceSeq::parse(claim_fence_seq)
                            .map_err(|err| err.to_string())?,
                        claim_lease_deadline: LeaseDeadlineMs::parse(claim_lease_deadline)
                            .map_err(|err| err.to_string())?,
                    },
                }))
            }
            ReadyQueueState::Applied => {
                if raw.claimed_by_session_token.is_some() || raw.claim_lease_deadline.is_some() {
                    return Err("applied ready-queue items cannot carry an active claim".to_owned());
                }
                let claim_fence_seq = raw.claim_fence_seq.ok_or_else(|| {
                    "applied ready-queue items require claim_fence_seq".to_owned()
                })?;
                Ok(Self::Applied(AppliedReadyQueueItem {
                    common,
                    claim_fence_seq: FenceSeq::parse(claim_fence_seq)
                        .map_err(|err| err.to_string())?,
                }))
            }
            ReadyQueueState::Superseded => {
                if raw.claimed_by_session_token.is_some() || raw.claim_lease_deadline.is_some() {
                    return Err(
                        "superseded ready-queue items cannot carry an active claim".to_owned()
                    );
                }
                Ok(Self::Superseded(SupersededReadyQueueItem {
                    common,
                    last_claim_fence_seq: raw
                        .claim_fence_seq
                        .map(FenceSeq::parse)
                        .transpose()
                        .map_err(|err| err.to_string())?,
                }))
            }
            ReadyQueueState::Cancelled => {
                if raw.claimed_by_session_token.is_some() || raw.claim_lease_deadline.is_some() {
                    return Err(
                        "cancelled ready-queue items cannot carry an active claim".to_owned()
                    );
                }
                Ok(Self::Cancelled(CancelledReadyQueueItem {
                    common,
                    last_claim_fence_seq: raw
                        .claim_fence_seq
                        .map(FenceSeq::parse)
                        .transpose()
                        .map_err(|err| err.to_string())?,
                }))
            }
        }
    }
}

impl From<&ReadyQueueItem> for RawReadyQueueItem {
    fn from(item: &ReadyQueueItem) -> Self {
        Self {
            queue_id: item.queue_id().to_owned(),
            artifact_digest: item.artifact_digest().to_owned(),
            subtask_id: item.subtask_id().to_owned(),
            settlement_target: item.settlement_target(),
            state: item.state(),
            claimed_by_session_token: item.claimed_by_session_token().map(ToOwned::to_owned),
            claim_fence_seq: item.claim_fence_seq(),
            claim_lease_deadline: item.claim_lease_deadline(),
            enqueued_at: item.enqueued_at(),
            updated_at: item.updated_at(),
        }
    }
}

impl Serialize for ReadyQueueItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawReadyQueueItem::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ReadyQueueItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawReadyQueueItem::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// Accepted verifier evidence bound to one apply attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyVerification {
    pub queue_id: QueueId,
    pub artifact_digest: ArtifactDigest,
    pub review_id: ReviewId,
    pub findings_digest: FindingsDigest,
    pub claim_fence_seq: FenceSeq,
    pub verifier: String,
    pub verdict_digest: ArtifactDigest,
    pub seal_digest: ArtifactDigest,
    pub recorded_by_session: SessionToken,
    pub created_at: TimestampMs,
}

/// Raw event-log row with JSON payload.
///
/// This log is an audit trail for subscribers, not an event-sourced state engine.
/// The relational tables remain authoritative; replaying `event_log` is not a
/// supported recovery path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub seq: i64,
    pub event_type: EventType,
    pub object_type: ObjectType,
    pub object_id: String,
    pub(super) actor: EventActor,
    pub payload_json: String,
    pub created_at: TimestampMs,
}

/// Decoded event-log row with typed payload.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedEvent {
    pub seq: i64,
    pub event_type: EventType,
    pub object_type: ObjectType,
    pub object_id: String,
    pub(super) actor: EventActor,
    pub payload: EventPayload,
    pub created_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EventActor {
    Session { session_token: SessionToken },
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawEvent {
    seq: i64,
    event_type: EventType,
    object_type: ObjectType,
    object_id: String,
    actor_kind: ActorKind,
    session_token: Option<SessionToken>,
    payload_json: String,
    created_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawTypedEvent {
    seq: i64,
    event_type: EventType,
    object_type: ObjectType,
    object_id: String,
    actor_kind: ActorKind,
    session_token: Option<SessionToken>,
    payload: EventPayload,
    created_at: TimestampMs,
}

impl Event {
    /// Builds a session-authored event-log record.
    #[must_use]
    pub fn session(
        seq: i64,
        event_type: EventType,
        object_type: ObjectType,
        object_id: String,
        session_token: SessionToken,
        payload_json: String,
        created_at: TimestampMs,
    ) -> Self {
        Self {
            seq,
            event_type,
            object_type,
            object_id,
            actor: EventActor::Session { session_token },
            payload_json,
            created_at,
        }
    }

    /// Builds a system-authored event-log record.
    #[must_use]
    pub fn system(
        seq: i64,
        event_type: EventType,
        object_type: ObjectType,
        object_id: String,
        payload_json: String,
        created_at: TimestampMs,
    ) -> Self {
        Self {
            seq,
            event_type,
            object_type,
            object_id,
            actor: EventActor::System,
            payload_json,
            created_at,
        }
    }

    fn from_raw(raw: RawEvent) -> Result<Self, String> {
        let actor = EventActor::try_from_parts(raw.actor_kind, raw.session_token)?;
        Ok(Self {
            seq: raw.seq,
            event_type: raw.event_type,
            object_type: raw.object_type,
            object_id: raw.object_id,
            actor,
            payload_json: raw.payload_json,
            created_at: raw.created_at,
        })
    }

    /// Returns the event actor class.
    #[must_use]
    pub const fn actor_kind(&self) -> ActorKind {
        self.actor.actor_kind()
    }

    /// Returns the event session token for session-authored events.
    #[must_use]
    pub const fn session_token(&self) -> Option<&SessionToken> {
        self.actor.session_token()
    }
}

impl TypedEvent {
    /// Returns the event actor class.
    #[must_use]
    pub const fn actor_kind(&self) -> ActorKind {
        self.actor.actor_kind()
    }

    /// Returns the event session token for session-authored events.
    #[must_use]
    pub const fn session_token(&self) -> Option<&SessionToken> {
        self.actor.session_token()
    }
}

impl EventActor {
    fn try_from_parts(
        actor_kind: ActorKind,
        session_token: Option<SessionToken>,
    ) -> Result<Self, String> {
        match (actor_kind, session_token) {
            (ActorKind::Session, Some(session_token)) => Ok(Self::Session { session_token }),
            (ActorKind::Session, None) => Err("session actor events require session_token".into()),
            (ActorKind::System, None) => Ok(Self::System),
            (ActorKind::System, Some(_)) => {
                Err("system actor events must not include session_token".into())
            }
        }
    }

    const fn actor_kind(&self) -> ActorKind {
        match self {
            Self::Session { .. } => ActorKind::Session,
            Self::System => ActorKind::System,
        }
    }

    const fn session_token(&self) -> Option<&SessionToken> {
        match self {
            Self::Session { session_token } => Some(session_token),
            Self::System => None,
        }
    }
}

impl Serialize for Event {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawEvent {
            seq: self.seq,
            event_type: self.event_type,
            object_type: self.object_type,
            object_id: self.object_id.clone(),
            actor_kind: self.actor.actor_kind(),
            session_token: self.actor.session_token().cloned(),
            payload_json: self.payload_json.clone(),
            created_at: self.created_at,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Event {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawEvent::deserialize(deserializer)?;
        Self::from_raw(raw).map_err(serde::de::Error::custom)
    }
}

impl Serialize for TypedEvent {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawTypedEvent {
            seq: self.seq,
            event_type: self.event_type,
            object_type: self.object_type,
            object_id: self.object_id.clone(),
            actor_kind: self.actor.actor_kind(),
            session_token: self.actor.session_token().cloned(),
            payload: self.payload.clone(),
            created_at: self.created_at,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TypedEvent {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawTypedEvent::deserialize(deserializer)?;
        let actor = EventActor::try_from_parts(raw.actor_kind, raw.session_token)
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            seq: raw.seq,
            event_type: raw.event_type,
            object_type: raw.object_type,
            object_id: raw.object_id,
            actor,
            payload: raw.payload,
            created_at: raw.created_at,
        })
    }
}

/// Payload emitted when stale sessions are reaped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct StaleSessionsPayload {
    pub stale_sessions: usize,
}

/// Payload emitted when leases are expired in bulk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct ExpiredCountPayload {
    pub expired_count: usize,
}

/// Typed event payload union for the append-only event log audit trail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventPayload {
    SessionRegistered(SessionHandle),
    SessionHeartbeat(HeartbeatReq),
    SessionExited(ExitSessionReq),
    RuntimeAttestationRecorded(RecordRuntimeAttestationReq),
    MetaTaskSubmitted(SubmitMetaTaskReq),
    MetaTaskCancelled(CancelMetaTaskReq),
    SubtaskCreated(CreateSubtaskRequest),
    SubtaskClaimed(ClaimResult),
    SubtaskStarted(StartSubtaskReq),
    SubtaskAbandoned(AbandonSubtaskReq),
    ClaimReleased(ReleaseClaimReq),
    ClaimRenewed(ClaimResult),
    ArtifactPublished(PublishArtifactReq),
    ReviewRequested(RequestReviewReq),
    ReviewDecided(DecideReviewReq),
    ReadyQueueEnqueued(EnqueueForApplyReq),
    ReadyQueueInFlight(ReadyQueueClaim),
    ApplyVerificationRecorded(RecordApplyVerificationReq),
    ReadyQueueApplied(MarkAppliedReq),
    ReadyQueueSuperseded(SupersedeQueueItemReq),
    ReservationRequested(RequestReservationReq),
    ReservationReleased(Reservation),
    ReservationRenewed(Reservation),
    ConflictResolved(ResolveConflictReq),
    SessionsReaped(StaleSessionsPayload),
    ClaimsExpired(ExpiredCountPayload),
    ReservationsExpired(ExpiredCountPayload),
    OpenSpecImported(Box<ImportOpenSpecEvent>),
}

/// Persisted unresolved conflict row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub conflict_id: String,
    pub object_type: ObjectType,
    pub object_id: String,
    pub conflict_kind: String,
    pub payload_json: String,
    pub detected_at: TimestampMs,
    pub resolution_state: ConflictResolutionState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MutationIdempotencyRecord {
    pub actor_key: String,
    pub operation: String,
    pub idempotency_key: String,
    pub request_hash: String,
    pub response_json: String,
    pub created_at: TimestampMs,
}

/// Conflict payload describing an overlapping reservation pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReservationOverlapConflictPayload {
    pub reservation_id: String,
    pub overlapping_reservation_id: String,
    pub owner_subtask_id: String,
    pub overlapping_owner_subtask_id: String,
    pub scope_class: ScopeClass,
    pub scope_key: String,
    pub overlapping_scope_class: ScopeClass,
    pub overlapping_scope_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub(crate) struct OverlapCandidate {
    pub scope_class: ScopeClass,
    pub scope_key: String,
    pub generated_members: Vec<String>,
}

use derive_new::new;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashSet;

use super::{
    AbandonSubtaskReq, ActorKind, AgentInstanceId, AgentPrincipalId, ApplyGateBlockerEvidenceId,
    ApplyGateBlockerKind, ApplyGateBlockerReason, ApplyWorktreePath, ApplyWorktreeState,
    ArtifactDigest, ArtifactKind, ArtifactManifestPath, BaseRev, CancelMetaTaskReq,
    ChangedPathsDigest, ClaimId, ClaimResult, ClaimState, CommandTranscriptDigest, ConflictId,
    ConflictKind, ConflictResolutionState, CoveyTypeValidationError, CreateSubtaskRequest,
    DecideReviewReq, EnqueueForApplyReq, EventObjectId, EventSeq, EventType, ExitSessionReq,
    FenceSeq, FindingsDigest, HeartbeatReq, ImportOpenSpecEvent, LeaseDeadlineMs, MarkAppliedReq,
    MarkApplyWorktreeStateReq, MetaTaskId, MetaTaskState, ModelId, ObjectType,
    OpenSpecArchiveBlockedReason, OpenSpecArchiveStatusState, OpenSpecChangeId,
    OperatorBlockerEvidenceId, OperatorBlockerId, OperatorBlockerReason, OperatorBlockerState,
    OperatorBlockerTargetKind, PromptText, ProviderId, ProviderRunId, ProviderRunIdIssuer,
    PublishArtifactReq, QueueId, ReadyQueueClaim, ReadyQueueState, RecordApplyGateBlockerReq,
    RecordApplyVerificationReq, RecordApplyWorktreeReq, RecordOpenSpecArchiveStatusReq,
    RecordOperatorBlockerReq, RecordPermissiveLandingReceiptReq, RecordProseApplyBlockerReq,
    RecordRuntimeAttestationReq, RecordSettlementReconcileBlockerReq, ReleaseClaimReq,
    RequestReservationReq, RequestReviewReq, ReservationId, ReservationState, ResolveConflictReq,
    ResolveOperatorBlockerReq, ReviewId, ReviewState, ReviewVerdict, RuntimeContainerId,
    RuntimeProcessId, ScopeClass, SessionHandle, SessionHeartbeatTick, SessionRole, SessionState,
    SessionToken, SettlementReconcileEvidenceId, SettlementReconcileReason, SettlementTarget,
    StartSubtaskReq, SubmitMetaTaskReq, SubtaskId, SubtaskKind, SubtaskPriority, SubtaskState,
    SubtaskTitle, SupersedeQueueItemReq, TimestampMs, VerifierId,
};

/// Persisted session row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub session_token: SessionToken,
    pub agent_principal_id: AgentPrincipalId,
    pub agent_instance_id: AgentInstanceId,
    pub role: SessionRole,
    lifecycle: SessionLifecycle,
    timestamps: SessionTimestamps,
    pub last_heartbeat_tick: SessionHeartbeatTick,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionLifecycle {
    ActiveIdle,
    ActiveWithSubtask { active_subtask_id: SubtaskId },
    Stale,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SessionTimestamps {
    last_heartbeat_at: TimestampMs,
    created_at: TimestampMs,
    updated_at: TimestampMs,
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
        let timestamps = SessionTimestamps::new(last_heartbeat_at, created_at, updated_at)?;
        Ok(Self {
            session_token,
            agent_principal_id: AgentPrincipalId::parse(agent_principal_id)
                .map_err(|err| err.to_string())?,
            agent_instance_id: AgentInstanceId::parse(agent_instance_id)
                .map_err(|err| err.to_string())?,
            role,
            lifecycle,
            timestamps,
            last_heartbeat_tick: SessionHeartbeatTick::parse(last_heartbeat_tick)
                .map_err(|err| err.to_string())?,
        })
    }

    #[must_use]
    pub fn agent_principal_id(&self) -> &str {
        self.agent_principal_id.as_str()
    }

    #[must_use]
    pub fn agent_instance_id(&self) -> &str {
        self.agent_instance_id.as_str()
    }

    #[must_use]
    pub const fn last_heartbeat_tick(&self) -> i64 {
        self.last_heartbeat_tick.get()
    }

    #[must_use]
    pub const fn last_heartbeat_at(&self) -> TimestampMs {
        self.timestamps.last_heartbeat_at()
    }

    #[must_use]
    pub const fn created_at(&self) -> TimestampMs {
        self.timestamps.created_at()
    }

    #[must_use]
    pub const fn updated_at(&self) -> TimestampMs {
        self.timestamps.updated_at()
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
            SessionState::Active => Ok(match active_subtask_id {
                Some(active_subtask_id) => Self::ActiveWithSubtask { active_subtask_id },
                None => Self::ActiveIdle,
            }),
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
            Self::ActiveIdle | Self::ActiveWithSubtask { .. } => SessionState::Active,
            Self::Stale => SessionState::Stale,
            Self::Exited => SessionState::Exited,
        }
    }

    const fn active_subtask_id(&self) -> Option<&SubtaskId> {
        match self {
            Self::ActiveIdle => None,
            Self::ActiveWithSubtask { active_subtask_id } => Some(active_subtask_id),
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
            agent_principal_id: session.agent_principal_id().to_owned(),
            agent_instance_id: session.agent_instance_id().to_owned(),
            role: session.role,
            state: session.state(),
            active_subtask_id: session.active_subtask_id().cloned(),
            last_heartbeat_at: session.last_heartbeat_at(),
            last_heartbeat_tick: session.last_heartbeat_tick(),
            created_at: session.created_at(),
            updated_at: session.updated_at(),
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

impl SessionTimestamps {
    fn new(
        last_heartbeat_at: TimestampMs,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> Result<Self, String> {
        if last_heartbeat_at < created_at {
            return Err(
                "session last_heartbeat_at must be greater than or equal to created_at".into(),
            );
        }
        if updated_at < created_at {
            return Err("session updated_at must be greater than or equal to created_at".into());
        }
        Ok(Self {
            last_heartbeat_at,
            created_at,
            updated_at,
        })
    }

    const fn last_heartbeat_at(self) -> TimestampMs {
        self.last_heartbeat_at
    }

    const fn created_at(self) -> TimestampMs {
        self.created_at
    }

    const fn updated_at(self) -> TimestampMs {
        self.updated_at
    }
}

const MISSING_PROVIDER_RUN_ID: &str = "__covey_missing_provider_run_id__";
const MISSING_PROVIDER_RUN_ID_ISSUER: &str = "__covey_missing_provider_run_id_issuer__";

/// Runtime identity evidence bound to one Covey session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeAttestation {
    pub session_token: SessionToken,
    pub agent_principal_id: AgentPrincipalId,
    pub agent_instance_id: AgentInstanceId,
    pub role: SessionRole,
    pub provider: ProviderId,
    pub model: ModelId,
    provider_run_identity: ProviderRunIdentity,
    runtime_identity: RuntimeIdentity,
    pub command_transcript_digest: CommandTranscriptDigest,
    timestamps: RuntimeAttestationTimestamps,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeIdentity {
    Process {
        process_id: RuntimeProcessId,
    },
    Container {
        container_id: RuntimeContainerId,
    },
    ProcessAndContainer {
        process_id: RuntimeProcessId,
        container_id: RuntimeContainerId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProviderRunIdentity {
    Observed {
        provider_run_id: ProviderRunId,
        provider_run_id_issuer: ProviderRunIdIssuer,
    },
    MissingLegacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RuntimeAttestationTimestamps {
    started_at: TimestampMs,
    ended_at: TimestampMs,
    recorded_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawRuntimeAttestation {
    session_token: SessionToken,
    agent_principal_id: String,
    agent_instance_id: String,
    role: SessionRole,
    provider: ProviderId,
    model: ModelId,
    provider_run_id: String,
    provider_run_id_issuer: String,
    process_id: Option<String>,
    container_id: Option<String>,
    command_transcript_digest: CommandTranscriptDigest,
    started_at: TimestampMs,
    ended_at: TimestampMs,
    recorded_at: TimestampMs,
}

impl RuntimeAttestation {
    /// Builds one runtime attestation from the flat DB/API shape.
    #[allow(clippy::too_many_arguments)]
    pub fn try_from_parts(
        session_token: SessionToken,
        agent_principal_id: impl Into<String>,
        agent_instance_id: impl Into<String>,
        role: SessionRole,
        provider: ProviderId,
        model: ModelId,
        provider_run_id: impl Into<String>,
        provider_run_id_issuer: impl Into<String>,
        process_id: Option<String>,
        container_id: Option<String>,
        command_transcript_digest: CommandTranscriptDigest,
        started_at: TimestampMs,
        ended_at: TimestampMs,
        recorded_at: TimestampMs,
    ) -> Result<Self, String> {
        let provider_run_identity =
            ProviderRunIdentity::from_parts(provider_run_id.into(), provider_run_id_issuer.into())?;
        let runtime_identity = RuntimeIdentity::from_parts(process_id, container_id)?;
        let timestamps = RuntimeAttestationTimestamps::new(started_at, ended_at, recorded_at)?;
        Ok(Self {
            session_token,
            agent_principal_id: AgentPrincipalId::parse(agent_principal_id)
                .map_err(|err| err.to_string())?,
            agent_instance_id: AgentInstanceId::parse(agent_instance_id)
                .map_err(|err| err.to_string())?,
            role,
            provider,
            model,
            provider_run_identity,
            runtime_identity,
            command_transcript_digest,
            timestamps,
        })
    }

    /// Returns the observed provider run id when this row is not a retained placeholder.
    #[must_use]
    pub fn provider_run_id(&self) -> Option<&str> {
        self.provider_run_identity.provider_run_id()
    }

    /// Returns the observed provider run issuer when this row is not a retained placeholder.
    #[must_use]
    pub fn provider_run_id_issuer(&self) -> Option<&str> {
        self.provider_run_identity.provider_run_id_issuer()
    }

    #[must_use]
    pub fn agent_principal_id(&self) -> &str {
        self.agent_principal_id.as_str()
    }

    #[must_use]
    pub fn agent_instance_id(&self) -> &str {
        self.agent_instance_id.as_str()
    }

    /// Returns true for old migrated rows that predate provider run identity.
    #[must_use]
    pub const fn provider_run_identity_missing(&self) -> bool {
        matches!(
            self.provider_run_identity,
            ProviderRunIdentity::MissingLegacy
        )
    }

    /// Returns the observed process id, when present.
    #[must_use]
    pub fn process_id(&self) -> Option<&str> {
        self.runtime_identity.process_id()
    }

    /// Returns the observed container id, when present.
    #[must_use]
    pub fn container_id(&self) -> Option<&str> {
        self.runtime_identity.container_id()
    }

    /// Returns the runtime identity tuple used for actor separation checks.
    #[must_use]
    pub fn runtime_ref(&self) -> (Option<&str>, Option<&str>) {
        (self.process_id(), self.container_id())
    }

    /// Returns the provider run identity tuple used for actor separation checks.
    #[must_use]
    pub fn provider_run_ref(&self) -> Option<(&str, &str)> {
        self.provider_run_identity.provider_run_ref()
    }

    /// Returns when the attested runtime span started.
    #[must_use]
    pub const fn started_at(&self) -> TimestampMs {
        self.timestamps.started_at()
    }

    /// Returns when the attested runtime span ended.
    #[must_use]
    pub const fn ended_at(&self) -> TimestampMs {
        self.timestamps.ended_at()
    }

    /// Returns when this attestation was recorded.
    #[must_use]
    pub const fn recorded_at(&self) -> TimestampMs {
        self.timestamps.recorded_at()
    }
}

impl RuntimeIdentity {
    fn from_parts(
        process_id: Option<String>,
        container_id: Option<String>,
    ) -> Result<Self, String> {
        let process_id = parse_optional_process_id(process_id)?;
        let container_id = parse_optional_container_id(container_id)?;
        match (process_id, container_id) {
            (Some(process_id), Some(container_id)) => Ok(Self::ProcessAndContainer {
                process_id,
                container_id,
            }),
            (Some(process_id), None) => Ok(Self::Process { process_id }),
            (None, Some(container_id)) => Ok(Self::Container { container_id }),
            (None, None) => Err("process_id or container_id is required".to_owned()),
        }
    }

    fn process_id(&self) -> Option<&str> {
        match self {
            Self::Process { process_id } | Self::ProcessAndContainer { process_id, .. } => {
                Some(process_id.as_str())
            }
            Self::Container { .. } => None,
        }
    }

    fn container_id(&self) -> Option<&str> {
        match self {
            Self::Container { container_id } | Self::ProcessAndContainer { container_id, .. } => {
                Some(container_id.as_str())
            }
            Self::Process { .. } => None,
        }
    }
}

impl ProviderRunIdentity {
    fn from_parts(provider_run_id: String, provider_run_id_issuer: String) -> Result<Self, String> {
        let provider_run_id = ProviderRunId::parse(provider_run_id)
            .map_err(runtime_identity_error("provider_run_id"))?;
        let provider_run_id_issuer = ProviderRunIdIssuer::parse(provider_run_id_issuer)
            .map_err(runtime_identity_error("provider_run_id_issuer"))?;
        match (
            provider_run_id.as_str() == MISSING_PROVIDER_RUN_ID,
            provider_run_id_issuer.as_str() == MISSING_PROVIDER_RUN_ID_ISSUER,
        ) {
            (true, true) => Ok(Self::MissingLegacy),
            (false, false) => Ok(Self::Observed {
                provider_run_id,
                provider_run_id_issuer,
            }),
            _ => Err("provider run identity must include both id and issuer".to_owned()),
        }
    }

    fn provider_run_id(&self) -> Option<&str> {
        match self {
            Self::Observed {
                provider_run_id, ..
            } => Some(provider_run_id.as_str()),
            Self::MissingLegacy => None,
        }
    }

    fn provider_run_id_issuer(&self) -> Option<&str> {
        match self {
            Self::Observed {
                provider_run_id_issuer,
                ..
            } => Some(provider_run_id_issuer.as_str()),
            Self::MissingLegacy => None,
        }
    }

    fn provider_run_ref(&self) -> Option<(&str, &str)> {
        match self {
            Self::Observed {
                provider_run_id,
                provider_run_id_issuer,
            } => Some((provider_run_id_issuer.as_str(), provider_run_id.as_str())),
            Self::MissingLegacy => None,
        }
    }
}

impl RuntimeAttestationTimestamps {
    fn new(
        started_at: TimestampMs,
        ended_at: TimestampMs,
        recorded_at: TimestampMs,
    ) -> Result<Self, String> {
        if ended_at < started_at {
            return Err("ended_at must be greater than or equal to started_at".to_owned());
        }
        if recorded_at < ended_at {
            return Err("recorded_at must be greater than or equal to ended_at".to_owned());
        }
        Ok(Self {
            started_at,
            ended_at,
            recorded_at,
        })
    }

    const fn started_at(self) -> TimestampMs {
        self.started_at
    }

    const fn ended_at(self) -> TimestampMs {
        self.ended_at
    }

    const fn recorded_at(self) -> TimestampMs {
        self.recorded_at
    }
}

impl Serialize for RuntimeAttestation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawRuntimeAttestation::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RuntimeAttestation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawRuntimeAttestation::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl From<&RuntimeAttestation> for RawRuntimeAttestation {
    fn from(attestation: &RuntimeAttestation) -> Self {
        Self {
            session_token: attestation.session_token.clone(),
            agent_principal_id: attestation.agent_principal_id().to_owned(),
            agent_instance_id: attestation.agent_instance_id().to_owned(),
            role: attestation.role,
            provider: attestation.provider.clone(),
            model: attestation.model.clone(),
            provider_run_id: attestation
                .provider_run_id()
                .unwrap_or(MISSING_PROVIDER_RUN_ID)
                .to_owned(),
            provider_run_id_issuer: attestation
                .provider_run_id_issuer()
                .unwrap_or(MISSING_PROVIDER_RUN_ID_ISSUER)
                .to_owned(),
            process_id: attestation.process_id().map(ToOwned::to_owned),
            container_id: attestation.container_id().map(ToOwned::to_owned),
            command_transcript_digest: attestation.command_transcript_digest.clone(),
            started_at: attestation.started_at(),
            ended_at: attestation.ended_at(),
            recorded_at: attestation.recorded_at(),
        }
    }
}

impl TryFrom<RawRuntimeAttestation> for RuntimeAttestation {
    type Error = String;

    fn try_from(raw: RawRuntimeAttestation) -> Result<Self, Self::Error> {
        Self::try_from_parts(
            raw.session_token,
            raw.agent_principal_id,
            raw.agent_instance_id,
            raw.role,
            raw.provider,
            raw.model,
            raw.provider_run_id,
            raw.provider_run_id_issuer,
            raw.process_id,
            raw.container_id,
            raw.command_transcript_digest,
            raw.started_at,
            raw.ended_at,
            raw.recorded_at,
        )
    }
}

fn parse_optional_process_id(value: Option<String>) -> Result<Option<RuntimeProcessId>, String> {
    value
        .map(|value| RuntimeProcessId::parse(value).map_err(runtime_identity_error("process_id")))
        .transpose()
}

fn parse_optional_container_id(
    value: Option<String>,
) -> Result<Option<RuntimeContainerId>, String> {
    value
        .map(|value| {
            RuntimeContainerId::parse(value).map_err(runtime_identity_error("container_id"))
        })
        .transpose()
}

fn runtime_identity_error(
    field: &'static str,
) -> impl Fn(CoveyTypeValidationError) -> String + Copy {
    move |err| format!("{field} {}", err.reason())
}

#[cfg(test)]
mod runtime_attestation_tests {
    use super::*;

    fn valid_runtime_attestation() -> RuntimeAttestation {
        RuntimeAttestation::try_from_parts(
            SessionToken::parse("session-1").expect("valid session token"),
            "agent-1",
            "instance-1",
            SessionRole::Executor,
            ProviderId::parse("provider-1").expect("valid provider"),
            ModelId::parse("model-1").expect("valid model"),
            "provider-run-1",
            "provider-issuer-1",
            Some("1234".to_owned()),
            None,
            CommandTranscriptDigest::parse("blake3:transcript").expect("valid transcript digest"),
            TimestampMs::parse(10).expect("valid started_at"),
            TimestampMs::parse(11).expect("valid ended_at"),
            TimestampMs::parse(12).expect("valid recorded_at"),
        )
        .expect("valid runtime attestation")
    }

    #[test]
    fn runtime_attestation_serializes_flat_storage_shape() {
        let attestation = valid_runtime_attestation();
        let value = serde_json::to_value(&attestation).expect("serialize runtime attestation");

        assert_eq!(value["provider_run_id"], "provider-run-1");
        assert_eq!(value["provider_run_id_issuer"], "provider-issuer-1");
        assert_eq!(value["process_id"], "1234");
        assert_eq!(value["container_id"], serde_json::Value::Null);
    }

    #[test]
    fn runtime_attestation_rejects_missing_runtime_identity() {
        let err = RuntimeAttestation::try_from_parts(
            SessionToken::parse("session-1").expect("valid session token"),
            "agent-1",
            "instance-1",
            SessionRole::Executor,
            ProviderId::parse("provider-1").expect("valid provider"),
            ModelId::parse("model-1").expect("valid model"),
            "provider-run-1",
            "provider-issuer-1",
            None,
            None,
            CommandTranscriptDigest::parse("blake3:transcript").expect("valid transcript digest"),
            TimestampMs::parse(10).expect("valid started_at"),
            TimestampMs::parse(11).expect("valid ended_at"),
            TimestampMs::parse(12).expect("valid recorded_at"),
        )
        .expect_err("runtime identity should be required");

        assert_eq!(err, "process_id or container_id is required");

        let padded_process = RuntimeAttestation::try_from_parts(
            SessionToken::parse("session-1").expect("valid session token"),
            "agent-1",
            "instance-1",
            SessionRole::Executor,
            ProviderId::parse("provider-1").expect("valid provider"),
            ModelId::parse("model-1").expect("valid model"),
            "provider-run-1",
            "provider-issuer-1",
            Some(" 1234".to_owned()),
            None,
            CommandTranscriptDigest::parse("blake3:transcript").expect("valid transcript digest"),
            TimestampMs::parse(10).expect("valid started_at"),
            TimestampMs::parse(11).expect("valid ended_at"),
            TimestampMs::parse(12).expect("valid recorded_at"),
        )
        .expect_err("runtime identity should be normalized");
        assert!(
            padded_process.contains("process_id must not include leading or trailing whitespace"),
            "unexpected error: {padded_process}"
        );
    }

    #[test]
    fn runtime_attestation_rejects_invalid_agent_identity() {
        let invalid_principal = RuntimeAttestation::try_from_parts(
            SessionToken::parse("session-1").expect("valid session token"),
            "agent 1",
            "instance-1",
            SessionRole::Executor,
            ProviderId::parse("provider-1").expect("valid provider"),
            ModelId::parse("model-1").expect("valid model"),
            "provider-run-1",
            "provider-issuer-1",
            Some("1234".to_owned()),
            None,
            CommandTranscriptDigest::parse("blake3:transcript").expect("valid transcript digest"),
            TimestampMs::parse(10).expect("valid started_at"),
            TimestampMs::parse(11).expect("valid ended_at"),
            TimestampMs::parse(12).expect("valid recorded_at"),
        )
        .expect_err("agent principal ids must be token-shaped");
        assert!(
            invalid_principal.contains("invalid agent_principal_id"),
            "unexpected error: {invalid_principal}"
        );

        let invalid_instance = RuntimeAttestation::try_from_parts(
            SessionToken::parse("session-1").expect("valid session token"),
            "agent-1",
            "",
            SessionRole::Executor,
            ProviderId::parse("provider-1").expect("valid provider"),
            ModelId::parse("model-1").expect("valid model"),
            "provider-run-1",
            "provider-issuer-1",
            Some("1234".to_owned()),
            None,
            CommandTranscriptDigest::parse("blake3:transcript").expect("valid transcript digest"),
            TimestampMs::parse(10).expect("valid started_at"),
            TimestampMs::parse(11).expect("valid ended_at"),
            TimestampMs::parse(12).expect("valid recorded_at"),
        )
        .expect_err("agent instance ids must be token-shaped");
        assert!(
            invalid_instance.contains("invalid agent_instance_id"),
            "unexpected error: {invalid_instance}"
        );
    }

    #[test]
    fn runtime_attestation_rejects_partial_legacy_provider_run_identity() {
        let err = RuntimeAttestation::try_from_parts(
            SessionToken::parse("session-1").expect("valid session token"),
            "agent-1",
            "instance-1",
            SessionRole::Executor,
            ProviderId::parse("provider-1").expect("valid provider"),
            ModelId::parse("model-1").expect("valid model"),
            MISSING_PROVIDER_RUN_ID,
            "provider-issuer-1",
            Some("1234".to_owned()),
            None,
            CommandTranscriptDigest::parse("blake3:transcript").expect("valid transcript digest"),
            TimestampMs::parse(10).expect("valid started_at"),
            TimestampMs::parse(11).expect("valid ended_at"),
            TimestampMs::parse(12).expect("valid recorded_at"),
        )
        .expect_err("partial provider run identity should be rejected");

        assert_eq!(err, "provider run identity must include both id and issuer");

        let padded_provider_run = RuntimeAttestation::try_from_parts(
            SessionToken::parse("session-1").expect("valid session token"),
            "agent-1",
            "instance-1",
            SessionRole::Executor,
            ProviderId::parse("provider-1").expect("valid provider"),
            ModelId::parse("model-1").expect("valid model"),
            "provider-run-1 ",
            "provider-issuer-1",
            Some("1234".to_owned()),
            None,
            CommandTranscriptDigest::parse("blake3:transcript").expect("valid transcript digest"),
            TimestampMs::parse(10).expect("valid started_at"),
            TimestampMs::parse(11).expect("valid ended_at"),
            TimestampMs::parse(12).expect("valid recorded_at"),
        )
        .expect_err("provider run identity should be normalized");
        assert!(
            padded_provider_run
                .contains("provider_run_id must not include leading or trailing whitespace"),
            "unexpected error: {padded_provider_run}"
        );
    }

    #[test]
    fn runtime_attestation_rejects_ended_before_started() {
        let err = RuntimeAttestation::try_from_parts(
            SessionToken::parse("session-1").expect("valid session token"),
            "agent-1",
            "instance-1",
            SessionRole::Executor,
            ProviderId::parse("provider-1").expect("valid provider"),
            ModelId::parse("model-1").expect("valid model"),
            "provider-run-1",
            "provider-issuer-1",
            Some("1234".to_owned()),
            None,
            CommandTranscriptDigest::parse("blake3:transcript").expect("valid transcript digest"),
            TimestampMs::parse(11).expect("valid started_at"),
            TimestampMs::parse(10).expect("valid ended_at"),
            TimestampMs::parse(12).expect("valid recorded_at"),
        )
        .expect_err("runtime attestation time range should be ordered");

        assert_eq!(err, "ended_at must be greater than or equal to started_at");
    }

    #[test]
    fn runtime_attestation_rejects_recorded_before_ended() {
        let err = RuntimeAttestation::try_from_parts(
            SessionToken::parse("session-1").expect("valid session token"),
            "agent-1",
            "instance-1",
            SessionRole::Executor,
            ProviderId::parse("provider-1").expect("valid provider"),
            ModelId::parse("model-1").expect("valid model"),
            "provider-run-1",
            "provider-issuer-1",
            Some("1234".to_owned()),
            None,
            CommandTranscriptDigest::parse("blake3:transcript").expect("valid transcript digest"),
            TimestampMs::parse(10).expect("valid started_at"),
            TimestampMs::parse(12).expect("valid ended_at"),
            TimestampMs::parse(11).expect("valid recorded_at"),
        )
        .expect_err("runtime attestation recording time should follow the attested span");

        assert_eq!(err, "recorded_at must be greater than or equal to ended_at");
    }

    #[test]
    fn runtime_attestation_models_legacy_missing_provider_run_identity_explicitly() {
        let attestation = RuntimeAttestation::try_from_parts(
            SessionToken::parse("session-1").expect("valid session token"),
            "agent-1",
            "instance-1",
            SessionRole::Executor,
            ProviderId::parse("provider-1").expect("valid provider"),
            ModelId::parse("model-1").expect("valid model"),
            MISSING_PROVIDER_RUN_ID,
            MISSING_PROVIDER_RUN_ID_ISSUER,
            Some("1234".to_owned()),
            None,
            CommandTranscriptDigest::parse("blake3:transcript").expect("valid transcript digest"),
            TimestampMs::parse(10).expect("valid started_at"),
            TimestampMs::parse(11).expect("valid ended_at"),
            TimestampMs::parse(12).expect("valid recorded_at"),
        )
        .expect("retained provider run placeholders remain explicit");

        assert!(attestation.provider_run_identity_missing());
        assert_eq!(attestation.provider_run_ref(), None);
    }
}

/// Persisted meta-task row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaTask {
    pub meta_task_id: MetaTaskId,
    pub prompt_text: PromptText,
    lifecycle: MetaTaskLifecycle,
    pub created_by: SessionToken,
    timestamps: MetaTaskTimestamps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetaTaskLifecycle {
    Planning,
    Active,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MetaTaskTimestamps {
    created_at: TimestampMs,
    updated_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawMetaTask {
    meta_task_id: MetaTaskId,
    prompt_text: String,
    state: MetaTaskState,
    created_by: SessionToken,
    created_at: TimestampMs,
    updated_at: TimestampMs,
}

impl MetaTask {
    /// Builds a meta-task from the flat storage/API shape.
    ///
    /// # Errors
    ///
    /// Returns an error when `updated_at` predates `created_at`.
    pub fn try_from_parts(
        meta_task_id: MetaTaskId,
        prompt_text: String,
        state: MetaTaskState,
        created_by: SessionToken,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> Result<Self, String> {
        Ok(Self {
            meta_task_id,
            prompt_text: PromptText::parse(prompt_text).map_err(|err| err.to_string())?,
            lifecycle: MetaTaskLifecycle::from(state),
            created_by,
            timestamps: MetaTaskTimestamps::new(created_at, updated_at)?,
        })
    }

    /// Builds a meta-task from the flat storage/API shape.
    #[must_use]
    pub fn from_parts(
        meta_task_id: MetaTaskId,
        prompt_text: String,
        state: MetaTaskState,
        created_by: SessionToken,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> Self {
        Self::try_from_parts(
            meta_task_id,
            prompt_text,
            state,
            created_by,
            created_at,
            updated_at,
        )
        .expect("meta-task timestamps must be monotonic")
    }

    /// Returns the persisted meta-task lifecycle state.
    #[must_use]
    pub const fn state(&self) -> MetaTaskState {
        self.lifecycle.state()
    }

    /// Returns the creation timestamp.
    #[must_use]
    pub const fn created_at(&self) -> TimestampMs {
        self.timestamps.created_at()
    }

    /// Returns the latest update timestamp.
    #[must_use]
    pub const fn updated_at(&self) -> TimestampMs {
        self.timestamps.updated_at()
    }
}

impl MetaTaskLifecycle {
    const fn state(self) -> MetaTaskState {
        match self {
            Self::Planning => MetaTaskState::Planning,
            Self::Active => MetaTaskState::Active,
            Self::Completed => MetaTaskState::Completed,
            Self::Cancelled => MetaTaskState::Cancelled,
        }
    }
}

impl From<MetaTaskState> for MetaTaskLifecycle {
    fn from(state: MetaTaskState) -> Self {
        match state {
            MetaTaskState::Planning => Self::Planning,
            MetaTaskState::Active => Self::Active,
            MetaTaskState::Completed => Self::Completed,
            MetaTaskState::Cancelled => Self::Cancelled,
        }
    }
}

impl Serialize for MetaTask {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawMetaTask::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MetaTask {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawMetaTask::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl TryFrom<RawMetaTask> for MetaTask {
    type Error = String;

    fn try_from(raw: RawMetaTask) -> Result<Self, Self::Error> {
        Self::try_from_parts(
            raw.meta_task_id,
            raw.prompt_text,
            raw.state,
            raw.created_by,
            raw.created_at,
            raw.updated_at,
        )
    }
}

impl From<&MetaTask> for RawMetaTask {
    fn from(meta_task: &MetaTask) -> Self {
        Self {
            meta_task_id: meta_task.meta_task_id.clone(),
            prompt_text: meta_task.prompt_text.as_str().to_owned(),
            state: meta_task.state(),
            created_by: meta_task.created_by.clone(),
            created_at: meta_task.created_at(),
            updated_at: meta_task.updated_at(),
        }
    }
}

impl MetaTaskTimestamps {
    fn new(created_at: TimestampMs, updated_at: TimestampMs) -> Result<Self, String> {
        if updated_at < created_at {
            return Err("meta-task updated_at must be greater than or equal to created_at".into());
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

/// Persisted subtask row.
///
/// Serialization remains the exact flat `subtasks` table shape, while the
/// lifecycle columns are stored as [`SubtaskLifecycle`] so loaded rows cannot
/// carry impossible `state`/claim/artifact combinations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubtaskRow {
    pub subtask_id: SubtaskId,
    pub meta_task_id: MetaTaskId,
    pub title: SubtaskTitle,
    kind: SubtaskRowKind,
    lifecycle: SubtaskLifecycle,
    pub priority: SubtaskPriority,
    timestamps: SubtaskTimestamps,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SubtaskRowKind {
    Work,
    Review { review_target: ReviewTarget },
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SubtaskTimestamps {
    created_at: TimestampMs,
    updated_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawSubtaskRow {
    subtask_id: SubtaskId,
    meta_task_id: MetaTaskId,
    title: String,
    kind: SubtaskKind,
    review_target_subtask_id: Option<SubtaskId>,
    review_target_artifact_digest: Option<ArtifactDigest>,
    state: SubtaskState,
    current_claim_id: Option<ClaimId>,
    artifact_digest: Option<ArtifactDigest>,
    priority: SubtaskPriority,
    created_at: TimestampMs,
    updated_at: TimestampMs,
}

impl SubtaskRow {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn try_from_parts(
        subtask_id: SubtaskId,
        meta_task_id: MetaTaskId,
        title: String,
        kind: SubtaskKind,
        review_target_subtask_id: Option<SubtaskId>,
        review_target_artifact_digest: Option<ArtifactDigest>,
        state: SubtaskState,
        current_claim_id: Option<ClaimId>,
        artifact_digest: Option<ArtifactDigest>,
        priority: SubtaskPriority,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> rusqlite::Result<Self> {
        let kind = SubtaskRowKind::from_parts(
            kind,
            review_target_subtask_id,
            review_target_artifact_digest,
        )?;
        let subtask_kind = kind.kind();
        let lifecycle = SubtaskLifecycle::from_row_parts_for_kind(
            subtask_kind,
            state,
            current_claim_id,
            artifact_digest,
        )?;
        let timestamps = SubtaskTimestamps::new(created_at, updated_at)?;
        Ok(Self {
            subtask_id,
            meta_task_id,
            title: SubtaskTitle::parse(title)
                .map_err(|err| invalid_subtask_row(&err.to_string()))?,
            kind,
            lifecycle,
            priority,
            timestamps,
        })
    }

    #[must_use]
    pub(crate) const fn state(&self) -> SubtaskState {
        self.lifecycle.state()
    }

    #[must_use]
    pub(crate) const fn kind(&self) -> SubtaskKind {
        self.kind.kind()
    }

    #[must_use]
    pub(crate) const fn review_target(&self) -> Option<&ReviewTarget> {
        self.kind.review_target()
    }

    #[must_use]
    pub(crate) fn current_claim_id(&self) -> Option<&ClaimId> {
        self.lifecycle.active_claim_id()
    }

    #[must_use]
    pub(crate) fn artifact_digest(&self) -> Option<&ArtifactDigest> {
        self.lifecycle.artifact_digest()
    }

    #[must_use]
    pub(crate) const fn created_at(&self) -> TimestampMs {
        self.timestamps.created_at()
    }

    #[must_use]
    pub(crate) const fn updated_at(&self) -> TimestampMs {
        self.timestamps.updated_at()
    }
}

impl SubtaskRowKind {
    fn from_parts(
        kind: SubtaskKind,
        review_target_subtask_id: Option<SubtaskId>,
        review_target_artifact_digest: Option<ArtifactDigest>,
    ) -> rusqlite::Result<Self> {
        match kind {
            SubtaskKind::Work => {
                if review_target_subtask_id.is_some() || review_target_artifact_digest.is_some() {
                    return Err(invalid_subtask_row("work subtask has a review target"));
                }
                Ok(Self::Work)
            }
            SubtaskKind::Review => {
                let Some(target_subtask_id) = review_target_subtask_id else {
                    return Err(invalid_subtask_row(
                        "review subtask is missing target subtask",
                    ));
                };
                let Some(target_artifact_digest) = review_target_artifact_digest else {
                    return Err(invalid_subtask_row(
                        "review subtask is missing target artifact",
                    ));
                };
                Ok(Self::Review {
                    review_target: ReviewTarget::new(target_subtask_id, target_artifact_digest),
                })
            }
            SubtaskKind::Cleanup => {
                if review_target_subtask_id.is_some() || review_target_artifact_digest.is_some() {
                    return Err(invalid_subtask_row("cleanup subtask has a review target"));
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

impl From<&SubtaskRow> for RawSubtaskRow {
    fn from(row: &SubtaskRow) -> Self {
        Self {
            subtask_id: row.subtask_id.clone(),
            meta_task_id: row.meta_task_id.clone(),
            title: row.title.as_str().to_owned(),
            kind: row.kind(),
            review_target_subtask_id: row.review_target().map(|target| target.subtask_id.clone()),
            review_target_artifact_digest: row
                .review_target()
                .map(|target| target.artifact_digest.clone()),
            state: row.state(),
            current_claim_id: row.current_claim_id().cloned(),
            artifact_digest: row.artifact_digest().cloned(),
            priority: row.priority,
            created_at: row.created_at(),
            updated_at: row.updated_at(),
        }
    }
}

impl TryFrom<RawSubtaskRow> for SubtaskRow {
    type Error = rusqlite::Error;

    fn try_from(raw: RawSubtaskRow) -> Result<Self, Self::Error> {
        Self::try_from_parts(
            raw.subtask_id,
            raw.meta_task_id,
            raw.title,
            raw.kind,
            raw.review_target_subtask_id,
            raw.review_target_artifact_digest,
            raw.state,
            raw.current_claim_id,
            raw.artifact_digest,
            raw.priority,
            raw.created_at,
            raw.updated_at,
        )
    }
}

impl Serialize for SubtaskRow {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawSubtaskRow::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SubtaskRow {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawSubtaskRow::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
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
    Blocked {
        artifact_digest: ArtifactDigest,
    },
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
            SubtaskState::Blocked => {
                if active_claim_id.is_some() {
                    return Err(invalid_subtask_row(
                        "blocked subtask cannot carry active claim state",
                    ));
                }
                Ok(Self::Blocked {
                    artifact_digest: require_subtask_artifact(
                        artifact_digest,
                        "blocked subtask is missing artifact digest",
                    )?,
                })
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

    pub(super) fn from_row_parts_for_kind(
        kind: SubtaskKind,
        state: SubtaskState,
        active_claim_id: Option<ClaimId>,
        artifact_digest: Option<ArtifactDigest>,
    ) -> rusqlite::Result<Self> {
        let lifecycle = Self::from_row_parts(state, active_claim_id, artifact_digest)?;
        lifecycle.ensure_allowed_for_kind(kind)?;
        Ok(lifecycle)
    }

    #[must_use]
    pub const fn state(&self) -> SubtaskState {
        match self {
            Self::Available => SubtaskState::Available,
            Self::Blocked { .. } => SubtaskState::Blocked,
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
            Self::Available | Self::Blocked { .. } | Self::Decided | Self::Abandoned { .. } => None,
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
            Self::Blocked { artifact_digest } => Some(artifact_digest),
            Self::Abandoned { artifact_digest } => artifact_digest.as_ref(),
            Self::Available | Self::Claimed { .. } | Self::InProgress { .. } | Self::Decided => {
                None
            }
        }
    }

    fn ensure_allowed_for_kind(&self, kind: SubtaskKind) -> rusqlite::Result<()> {
        match (kind, self.state()) {
            (SubtaskKind::Work, SubtaskState::Decided) => Err(invalid_subtask_row(
                "work subtasks cannot use decided review lifecycle state",
            )),
            (SubtaskKind::Cleanup, SubtaskState::Decided) => Err(invalid_subtask_row(
                "cleanup subtasks cannot use decided review lifecycle state",
            )),
            (SubtaskKind::Review, SubtaskState::Blocked) => Err(invalid_subtask_row(
                "review subtasks cannot use blocked work lifecycle state",
            )),
            (
                SubtaskKind::Review,
                SubtaskState::ArtifactPublished
                | SubtaskState::ReviewPending
                | SubtaskState::ChangesRequested
                | SubtaskState::Approved
                | SubtaskState::ReadyForApply
                | SubtaskState::Applied,
            ) => Err(invalid_subtask_row(
                "review subtasks cannot use work artifact lifecycle states",
            )),
            (
                SubtaskKind::Cleanup,
                SubtaskState::Blocked
                | SubtaskState::ArtifactPublished
                | SubtaskState::ReviewPending
                | SubtaskState::ChangesRequested
                | SubtaskState::Approved
                | SubtaskState::ReadyForApply,
            ) => Err(invalid_subtask_row(
                "cleanup subtasks cannot use work artifact or review lifecycle states",
            )),
            _ => Ok(()),
        }
    }
}

fn require_subtask_artifact(
    artifact_digest: Option<ArtifactDigest>,
    missing_reason: &str,
) -> rusqlite::Result<ArtifactDigest> {
    artifact_digest.ok_or_else(|| invalid_subtask_row(missing_reason))
}

impl SubtaskTimestamps {
    fn new(created_at: TimestampMs, updated_at: TimestampMs) -> rusqlite::Result<Self> {
        if updated_at < created_at {
            return Err(invalid_subtask_row(
                "subtask updated_at must be greater than or equal to created_at",
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

/// Domain object for executable work.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkSubtask {
    subtask_id: SubtaskId,
    meta_task_id: MetaTaskId,
    title: SubtaskTitle,
    lifecycle: SubtaskLifecycle,
    priority: SubtaskPriority,
    timestamps: SubtaskTimestamps,
}

/// Domain object for review work bound to one artifact of one work subtask.
#[must_use]
#[allow(clippy::too_many_arguments)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewSubtask {
    subtask_id: SubtaskId,
    meta_task_id: MetaTaskId,
    title: SubtaskTitle,
    review_target: ReviewTarget,
    lifecycle: SubtaskLifecycle,
    priority: SubtaskPriority,
    timestamps: SubtaskTimestamps,
}

/// Orchestrator-owned cleanup subtask that is not dispatchable to worker lanes.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::too_many_arguments)]
pub struct CleanupSubtask {
    subtask_id: SubtaskId,
    meta_task_id: MetaTaskId,
    title: SubtaskTitle,
    lifecycle: SubtaskLifecycle,
    priority: SubtaskPriority,
    timestamps: SubtaskTimestamps,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawWorkSubtask {
    subtask_id: SubtaskId,
    meta_task_id: MetaTaskId,
    title: String,
    lifecycle: SubtaskLifecycle,
    priority: SubtaskPriority,
    created_at: TimestampMs,
    updated_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawReviewSubtask {
    subtask_id: SubtaskId,
    meta_task_id: MetaTaskId,
    title: String,
    review_target: ReviewTarget,
    lifecycle: SubtaskLifecycle,
    priority: SubtaskPriority,
    created_at: TimestampMs,
    updated_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawCleanupSubtask {
    subtask_id: SubtaskId,
    meta_task_id: MetaTaskId,
    title: String,
    lifecycle: SubtaskLifecycle,
    priority: SubtaskPriority,
    created_at: TimestampMs,
    updated_at: TimestampMs,
}

impl WorkSubtask {
    /// Builds a work subtask, rejecting review-only lifecycle states.
    ///
    /// # Errors
    ///
    /// Returns an error when `lifecycle` is not legal for work subtasks.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        subtask_id: SubtaskId,
        meta_task_id: MetaTaskId,
        title: String,
        lifecycle: SubtaskLifecycle,
        priority: SubtaskPriority,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> rusqlite::Result<Self> {
        lifecycle.ensure_allowed_for_kind(SubtaskKind::Work)?;
        let title =
            SubtaskTitle::parse(title).map_err(|err| invalid_subtask_row(&err.to_string()))?;
        let timestamps = SubtaskTimestamps::new(created_at, updated_at)?;
        Ok(Self {
            subtask_id,
            meta_task_id,
            title,
            lifecycle,
            priority,
            timestamps,
        })
    }
}

impl ReviewSubtask {
    /// Builds a review subtask, rejecting work-artifact lifecycle states.
    ///
    /// # Errors
    ///
    /// Returns an error when `lifecycle` is not legal for review subtasks.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        subtask_id: SubtaskId,
        meta_task_id: MetaTaskId,
        title: String,
        review_target: ReviewTarget,
        lifecycle: SubtaskLifecycle,
        priority: SubtaskPriority,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> rusqlite::Result<Self> {
        lifecycle.ensure_allowed_for_kind(SubtaskKind::Review)?;
        let title =
            SubtaskTitle::parse(title).map_err(|err| invalid_subtask_row(&err.to_string()))?;
        let timestamps = SubtaskTimestamps::new(created_at, updated_at)?;
        Ok(Self {
            subtask_id,
            meta_task_id,
            title,
            review_target,
            lifecycle,
            priority,
            timestamps,
        })
    }
}

impl CleanupSubtask {
    /// Builds a cleanup subtask, rejecting worker/review-only lifecycle states.
    ///
    /// # Errors
    ///
    /// Returns an error when `lifecycle` is not legal for cleanup subtasks.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        subtask_id: SubtaskId,
        meta_task_id: MetaTaskId,
        title: String,
        lifecycle: SubtaskLifecycle,
        priority: SubtaskPriority,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> rusqlite::Result<Self> {
        lifecycle.ensure_allowed_for_kind(SubtaskKind::Cleanup)?;
        let title =
            SubtaskTitle::parse(title).map_err(|err| invalid_subtask_row(&err.to_string()))?;
        let timestamps = SubtaskTimestamps::new(created_at, updated_at)?;
        Ok(Self {
            subtask_id,
            meta_task_id,
            title,
            lifecycle,
            priority,
            timestamps,
        })
    }
}

impl TryFrom<RawWorkSubtask> for WorkSubtask {
    type Error = rusqlite::Error;

    fn try_from(raw: RawWorkSubtask) -> Result<Self, Self::Error> {
        Self::new(
            raw.subtask_id,
            raw.meta_task_id,
            raw.title,
            raw.lifecycle,
            raw.priority,
            raw.created_at,
            raw.updated_at,
        )
    }
}

impl From<&WorkSubtask> for RawWorkSubtask {
    fn from(subtask: &WorkSubtask) -> Self {
        Self {
            subtask_id: subtask.subtask_id.clone(),
            meta_task_id: subtask.meta_task_id.clone(),
            title: subtask.title.as_str().to_owned(),
            lifecycle: subtask.lifecycle.clone(),
            priority: subtask.priority,
            created_at: subtask.timestamps.created_at(),
            updated_at: subtask.timestamps.updated_at(),
        }
    }
}

impl Serialize for WorkSubtask {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawWorkSubtask::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WorkSubtask {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawWorkSubtask::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl TryFrom<RawReviewSubtask> for ReviewSubtask {
    type Error = rusqlite::Error;

    fn try_from(raw: RawReviewSubtask) -> Result<Self, Self::Error> {
        Self::new(
            raw.subtask_id,
            raw.meta_task_id,
            raw.title,
            raw.review_target,
            raw.lifecycle,
            raw.priority,
            raw.created_at,
            raw.updated_at,
        )
    }
}

impl From<&ReviewSubtask> for RawReviewSubtask {
    fn from(subtask: &ReviewSubtask) -> Self {
        Self {
            subtask_id: subtask.subtask_id.clone(),
            meta_task_id: subtask.meta_task_id.clone(),
            title: subtask.title.as_str().to_owned(),
            review_target: subtask.review_target.clone(),
            lifecycle: subtask.lifecycle.clone(),
            priority: subtask.priority,
            created_at: subtask.timestamps.created_at(),
            updated_at: subtask.timestamps.updated_at(),
        }
    }
}

impl Serialize for ReviewSubtask {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawReviewSubtask::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ReviewSubtask {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawReviewSubtask::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl TryFrom<RawCleanupSubtask> for CleanupSubtask {
    type Error = rusqlite::Error;

    fn try_from(raw: RawCleanupSubtask) -> Result<Self, Self::Error> {
        Self::new(
            raw.subtask_id,
            raw.meta_task_id,
            raw.title,
            raw.lifecycle,
            raw.priority,
            raw.created_at,
            raw.updated_at,
        )
    }
}

impl From<&CleanupSubtask> for RawCleanupSubtask {
    fn from(subtask: &CleanupSubtask) -> Self {
        Self {
            subtask_id: subtask.subtask_id.clone(),
            meta_task_id: subtask.meta_task_id.clone(),
            title: subtask.title.as_str().to_owned(),
            lifecycle: subtask.lifecycle.clone(),
            priority: subtask.priority,
            created_at: subtask.timestamps.created_at(),
            updated_at: subtask.timestamps.updated_at(),
        }
    }
}

impl Serialize for CleanupSubtask {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawCleanupSubtask::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CleanupSubtask {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawCleanupSubtask::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
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
    Cleanup(CleanupSubtask),
}

impl Subtask {
    pub fn subtask_id(&self) -> &str {
        match self {
            Self::Work(subtask) => &subtask.subtask_id,
            Self::Review(subtask) => &subtask.subtask_id,
            Self::Cleanup(subtask) => &subtask.subtask_id,
        }
    }

    pub fn meta_task_id(&self) -> &str {
        match self {
            Self::Work(subtask) => &subtask.meta_task_id,
            Self::Review(subtask) => &subtask.meta_task_id,
            Self::Cleanup(subtask) => &subtask.meta_task_id,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Work(subtask) => &subtask.title,
            Self::Review(subtask) => &subtask.title,
            Self::Cleanup(subtask) => &subtask.title,
        }
    }

    pub fn kind(&self) -> SubtaskKind {
        match self {
            Self::Work(_) => SubtaskKind::Work,
            Self::Review(_) => SubtaskKind::Review,
            Self::Cleanup(_) => SubtaskKind::Cleanup,
        }
    }

    pub fn lifecycle(&self) -> &SubtaskLifecycle {
        match self {
            Self::Work(subtask) => &subtask.lifecycle,
            Self::Review(subtask) => &subtask.lifecycle,
            Self::Cleanup(subtask) => &subtask.lifecycle,
        }
    }

    pub fn review_target(&self) -> Option<&ReviewTarget> {
        match self {
            Self::Work(_) | Self::Cleanup(_) => None,
            Self::Review(subtask) => Some(&subtask.review_target),
        }
    }
}

impl TryFrom<SubtaskRow> for Subtask {
    type Error = rusqlite::Error;

    fn try_from(row: SubtaskRow) -> Result<Self, Self::Error> {
        let created_at = row.created_at();
        let updated_at = row.updated_at();
        match row.kind {
            SubtaskRowKind::Work => Ok(Self::Work(WorkSubtask::new(
                row.subtask_id,
                row.meta_task_id,
                row.title.into(),
                row.lifecycle,
                row.priority,
                created_at,
                updated_at,
            )?)),
            SubtaskRowKind::Review { review_target } => Ok(Self::Review(ReviewSubtask::new(
                row.subtask_id,
                row.meta_task_id,
                row.title.into(),
                review_target,
                row.lifecycle,
                row.priority,
                created_at,
                updated_at,
            )?)),
            SubtaskRowKind::Cleanup => Ok(Self::Cleanup(CleanupSubtask::new(
                row.subtask_id,
                row.meta_task_id,
                row.title.into(),
                row.lifecycle,
                row.priority,
                created_at,
                updated_at,
            )?)),
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub claim_id: ClaimId,
    pub subtask_id: SubtaskId,
    pub owner_session_token: SessionToken,
    pub fence_seq: FenceSeq,
    pub lease_deadline: LeaseDeadlineMs,
    lifecycle: ClaimLifecycle,
    timestamps: ClaimTimestamps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaimLifecycle {
    Held,
    Released,
    Expired,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClaimTimestamps {
    created_at: TimestampMs,
    updated_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawClaim {
    claim_id: ClaimId,
    subtask_id: SubtaskId,
    owner_session_token: SessionToken,
    fence_seq: FenceSeq,
    lease_deadline: LeaseDeadlineMs,
    state: ClaimState,
    created_at: TimestampMs,
    updated_at: TimestampMs,
}

impl Claim {
    /// Builds a claim from the flat storage/API shape.
    ///
    /// # Errors
    ///
    /// Returns an error when `updated_at` predates `created_at`.
    #[allow(clippy::too_many_arguments)]
    pub fn try_from_parts(
        claim_id: ClaimId,
        subtask_id: SubtaskId,
        owner_session_token: SessionToken,
        fence_seq: FenceSeq,
        lease_deadline: LeaseDeadlineMs,
        state: ClaimState,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> Result<Self, String> {
        let timestamps = ClaimTimestamps::new(created_at, updated_at)?;
        Ok(Self {
            claim_id,
            subtask_id,
            owner_session_token,
            fence_seq,
            lease_deadline,
            lifecycle: ClaimLifecycle::from(state),
            timestamps,
        })
    }

    /// Returns the persisted claim lifecycle state.
    #[must_use]
    pub const fn state(&self) -> ClaimState {
        self.lifecycle.state()
    }

    #[must_use]
    pub const fn created_at(&self) -> TimestampMs {
        self.timestamps.created_at()
    }

    #[must_use]
    pub const fn updated_at(&self) -> TimestampMs {
        self.timestamps.updated_at()
    }
}

impl ClaimLifecycle {
    const fn state(self) -> ClaimState {
        match self {
            Self::Held => ClaimState::Held,
            Self::Released => ClaimState::Released,
            Self::Expired => ClaimState::Expired,
            Self::Revoked => ClaimState::Revoked,
        }
    }
}

impl From<ClaimState> for ClaimLifecycle {
    fn from(state: ClaimState) -> Self {
        match state {
            ClaimState::Held => Self::Held,
            ClaimState::Released => Self::Released,
            ClaimState::Expired => Self::Expired,
            ClaimState::Revoked => Self::Revoked,
        }
    }
}

impl Serialize for Claim {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawClaim::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Claim {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawClaim::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl TryFrom<RawClaim> for Claim {
    type Error = String;

    fn try_from(raw: RawClaim) -> Result<Self, Self::Error> {
        Self::try_from_parts(
            raw.claim_id,
            raw.subtask_id,
            raw.owner_session_token,
            raw.fence_seq,
            raw.lease_deadline,
            raw.state,
            raw.created_at,
            raw.updated_at,
        )
    }
}

impl From<&Claim> for RawClaim {
    fn from(claim: &Claim) -> Self {
        Self {
            claim_id: claim.claim_id.clone(),
            subtask_id: claim.subtask_id.clone(),
            owner_session_token: claim.owner_session_token.clone(),
            fence_seq: claim.fence_seq,
            lease_deadline: claim.lease_deadline,
            state: claim.state(),
            created_at: claim.created_at(),
            updated_at: claim.updated_at(),
        }
    }
}

impl ClaimTimestamps {
    fn new(created_at: TimestampMs, updated_at: TimestampMs) -> Result<Self, String> {
        if updated_at < created_at {
            return Err("claim updated_at must be greater than or equal to created_at".to_owned());
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

/// Persisted artifact row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub artifact_digest: ArtifactDigest,
    pub artifact_kind: ArtifactKind,
    pub base_rev: BaseRev,
    pub produced_by_subtask_id: SubtaskId,
    pub produced_by_session: SessionToken,
    pub manifest_path: ArtifactManifestPath,
    pub changed_paths_digest: ChangedPathsDigest,
    pub created_at: TimestampMs,
}

/// Fields shared by every persisted review state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewCommon {
    review_id: ReviewId,
    subtask_id: SubtaskId,
    artifact_digest: ArtifactDigest,
    reviewer_session: SessionToken,
    review_subtask_id: SubtaskId,
    created_at: TimestampMs,
    updated_at: TimestampMs,
}

/// Persisted review row in the requested state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestedReview {
    common: ReviewCommon,
}

/// Persisted review row in the in-progress state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InProgressReview {
    common: ReviewCommon,
}

/// Persisted review row after a reviewer decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecidedReview {
    common: ReviewCommon,
    verdict: ReviewVerdict,
    findings_digest: FindingsDigest,
}

/// Persisted review row superseded by a newer artifact or review round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupersededReview {
    common: ReviewCommon,
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
    lifecycle: ReservationLifecycle,
    timestamps: ReservationTimestamps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReservationLifecycle {
    Active,
    Released,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReservationTimestamps {
    created_at: TimestampMs,
    updated_at: TimestampMs,
}

/// Scope covered by a persisted reservation.
///
/// The enum prevents mixing generated-set members with exact, subtree, or
/// repo-global scopes, and prevents generated-set reservations with no members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReservationScope {
    ExactPath {
        scope_key: ReservationScopeKey,
    },
    Subtree {
        scope_key: ReservationScopeKey,
    },
    RepoGlobal,
    GeneratedSet {
        scope_key: ReservationScopeKey,
        generated_members: GeneratedReservationMembers,
    },
}

/// Validated non-empty reservation scope key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ReservationScopeKey(String);

/// Validated non-empty member set for generated-set reservations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<String>", into = "Vec<String>")]
pub struct GeneratedReservationMembers(Vec<GeneratedReservationMember>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedReservationMember(String);

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
        let timestamps = ReservationTimestamps::new(created_at, updated_at)?;
        Ok(Self {
            reservation_id,
            owner_subtask_id,
            scope,
            lease_deadline,
            lifecycle: ReservationLifecycle::from(state),
            timestamps,
        })
    }

    #[must_use]
    pub const fn state(&self) -> ReservationState {
        self.lifecycle.state()
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
    pub fn generated_members(&self) -> Vec<String> {
        self.scope.generated_members()
    }

    pub(crate) fn generated_member_strs(&self) -> impl Iterator<Item = &str> {
        self.scope.generated_member_strs()
    }

    #[must_use]
    pub const fn created_at(&self) -> TimestampMs {
        self.timestamps.created_at()
    }

    #[must_use]
    pub const fn updated_at(&self) -> TimestampMs {
        self.timestamps.updated_at()
    }

    pub(crate) fn into_flat_parts(
        self,
    ) -> (
        ReservationId,
        SubtaskId,
        ScopeClass,
        String,
        Vec<String>,
        LeaseDeadlineMs,
        TimestampMs,
    ) {
        let Self {
            reservation_id,
            owner_subtask_id,
            scope,
            lease_deadline,
            timestamps,
            ..
        } = self;
        let (scope_class, scope_key, generated_members) = scope.into_parts();
        (
            reservation_id,
            owner_subtask_id,
            scope_class,
            scope_key,
            generated_members,
            lease_deadline,
            timestamps.created_at(),
        )
    }
}

impl ReservationLifecycle {
    const fn state(self) -> ReservationState {
        match self {
            Self::Active => ReservationState::Active,
            Self::Released => ReservationState::Released,
            Self::Expired => ReservationState::Expired,
        }
    }
}

impl From<ReservationState> for ReservationLifecycle {
    fn from(state: ReservationState) -> Self {
        match state {
            ReservationState::Active => Self::Active,
            ReservationState::Released => Self::Released,
            ReservationState::Expired => Self::Expired,
        }
    }
}

impl ReservationScope {
    /// Builds a reservation scope from the flat storage/API shape.
    ///
    /// # Errors
    ///
    /// Returns an error when the scope class, key, and generated members are
    /// inconsistent with each other.
    pub fn from_parts(
        scope_class: ScopeClass,
        scope_key: String,
        generated_members: Vec<String>,
    ) -> Result<Self, String> {
        match scope_class {
            ScopeClass::ExactPath => {
                reject_generated_members(scope_class, &generated_members)?;
                Ok(Self::ExactPath {
                    scope_key: ReservationScopeKey::parse_for_scope(scope_class, scope_key)?,
                })
            }
            ScopeClass::Subtree => {
                reject_generated_members(scope_class, &generated_members)?;
                Ok(Self::Subtree {
                    scope_key: ReservationScopeKey::parse_for_scope(scope_class, scope_key)?,
                })
            }
            ScopeClass::RepoGlobal => {
                reject_generated_members(scope_class, &generated_members)?;
                if scope_key != "repo" {
                    return Err("repo-global reservations require scope_key `repo`".to_owned());
                }
                Ok(Self::RepoGlobal)
            }
            ScopeClass::GeneratedSet => Ok(Self::GeneratedSet {
                scope_key: ReservationScopeKey::parse_for_scope(scope_class, scope_key)?,
                generated_members: GeneratedReservationMembers::parse_for_scope(generated_members)?,
            }),
        }
    }

    #[must_use]
    pub const fn scope_class(&self) -> ScopeClass {
        match self {
            Self::ExactPath { .. } => ScopeClass::ExactPath,
            Self::Subtree { .. } => ScopeClass::Subtree,
            Self::RepoGlobal => ScopeClass::RepoGlobal,
            Self::GeneratedSet { .. } => ScopeClass::GeneratedSet,
        }
    }

    #[must_use]
    pub fn scope_key(&self) -> &str {
        match self {
            Self::ExactPath { scope_key }
            | Self::Subtree { scope_key }
            | Self::GeneratedSet { scope_key, .. } => scope_key.as_str(),
            Self::RepoGlobal => "repo",
        }
    }

    #[must_use]
    pub fn generated_members(&self) -> Vec<String> {
        match self {
            Self::GeneratedSet {
                generated_members, ..
            } => generated_members.to_vec(),
            Self::ExactPath { .. } | Self::Subtree { .. } | Self::RepoGlobal => Vec::new(),
        }
    }

    pub(crate) fn generated_member_strs(&self) -> impl Iterator<Item = &str> {
        self.generated_member_slice()
            .iter()
            .map(GeneratedReservationMember::as_str)
    }

    fn generated_member_slice(&self) -> &[GeneratedReservationMember] {
        match self {
            Self::GeneratedSet {
                generated_members, ..
            } => generated_members.as_slice(),
            Self::ExactPath { .. } | Self::Subtree { .. } | Self::RepoGlobal => &[],
        }
    }

    fn into_parts(self) -> (ScopeClass, String, Vec<String>) {
        match self {
            Self::ExactPath { scope_key } => (ScopeClass::ExactPath, scope_key.into(), Vec::new()),
            Self::Subtree { scope_key } => (ScopeClass::Subtree, scope_key.into(), Vec::new()),
            Self::RepoGlobal => (ScopeClass::RepoGlobal, "repo".to_owned(), Vec::new()),
            Self::GeneratedSet {
                scope_key,
                generated_members,
            } => (
                ScopeClass::GeneratedSet,
                scope_key.into(),
                generated_members.into(),
            ),
        }
    }
}

impl ReservationScopeKey {
    fn parse_for_scope(scope_class: ScopeClass, scope_key: String) -> Result<Self, String> {
        if scope_key.trim().is_empty() {
            Err(format!("{scope_class} reservations require scope_key"))
        } else if scope_key.trim() != scope_key {
            Err(format!(
                "{scope_class} reservation scope_key must be normalized"
            ))
        } else {
            Ok(Self(scope_key))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ReservationScopeKey {
    type Error = String;

    fn try_from(scope_key: String) -> Result<Self, Self::Error> {
        if scope_key.trim().is_empty() {
            Err("reservation scope_key must not be empty".to_owned())
        } else if scope_key.trim() != scope_key {
            Err("reservation scope_key must be normalized".to_owned())
        } else {
            Ok(Self(scope_key))
        }
    }
}

impl From<ReservationScopeKey> for String {
    fn from(scope_key: ReservationScopeKey) -> Self {
        scope_key.0
    }
}

impl GeneratedReservationMembers {
    fn parse_for_scope(generated_members: Vec<String>) -> Result<Self, String> {
        if generated_members.is_empty() {
            return Err("generated-set reservations require generated_members".to_owned());
        }
        let mut seen: HashSet<&str> = HashSet::with_capacity(generated_members.len());
        for member in &generated_members {
            validate_generated_reservation_member_for_scope(member)?;
            if !seen.insert(member.as_str()) {
                return Err(
                    "generated-set reservations require unique generated_members".to_owned(),
                );
            }
        }
        let parsed = generated_members
            .into_iter()
            .map(GeneratedReservationMember)
            .collect();
        Ok(Self(parsed))
    }

    #[must_use]
    pub fn to_vec(&self) -> Vec<String> {
        self.0
            .iter()
            .map(GeneratedReservationMember::to_string)
            .collect()
    }

    fn as_slice(&self) -> &[GeneratedReservationMember] {
        &self.0
    }
}

impl GeneratedReservationMember {
    fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_generated_reservation_member_for_scope(member: &str) -> Result<(), String> {
    if member.trim().is_empty() {
        return Err("generated-set reservations require non-empty generated_members".to_owned());
    }
    if member.trim() != member || member.chars().any(char::is_control) {
        return Err("generated-set reservations require normalized generated_members".to_owned());
    }
    Ok(())
}

impl std::fmt::Display for GeneratedReservationMember {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl TryFrom<Vec<String>> for GeneratedReservationMembers {
    type Error = String;

    fn try_from(generated_members: Vec<String>) -> Result<Self, Self::Error> {
        Self::parse_for_scope(generated_members)
    }
}

impl From<GeneratedReservationMembers> for Vec<String> {
    fn from(generated_members: GeneratedReservationMembers) -> Self {
        generated_members
            .0
            .into_iter()
            .map(|member| member.0)
            .collect()
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
            generated_members: reservation.generated_members(),
            lease_deadline: reservation.lease_deadline,
            state: reservation.state(),
            created_at: reservation.created_at(),
            updated_at: reservation.updated_at(),
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

impl ReservationTimestamps {
    fn new(created_at: TimestampMs, updated_at: TimestampMs) -> Result<Self, String> {
        if updated_at < created_at {
            return Err(
                "reservation updated_at must be greater than or equal to created_at".into(),
            );
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

/// Fields shared by every persisted ready-queue state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyQueueCommon {
    queue_id: QueueId,
    artifact_digest: ArtifactDigest,
    subtask_id: SubtaskId,
    settlement_target: SettlementTarget,
    enqueued_at: TimestampMs,
    updated_at: TimestampMs,
}

/// Queued apply item. A prior fence may be retained as a monotonic counter, but
/// there is no active queue claim in this state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedReadyQueueItem {
    common: ReadyQueueCommon,
    last_claim_fence_seq: Option<FenceSeq>,
}

/// Active claim fields for an in-flight apply item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyQueueActiveClaim {
    claimed_by_session_token: SessionToken,
    claim_fence_seq: FenceSeq,
    claim_lease_deadline: LeaseDeadlineMs,
}

/// In-flight apply item. All active claim fields are required together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InFlightReadyQueueItem {
    common: ReadyQueueCommon,
    claim: ReadyQueueActiveClaim,
}

/// Applied queue item bound to the fence that was accepted by the apply gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedReadyQueueItem {
    common: ReadyQueueCommon,
    claim_fence_seq: FenceSeq,
}

/// Superseded apply item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupersededReadyQueueItem {
    common: ReadyQueueCommon,
    last_claim_fence_seq: Option<FenceSeq>,
}

/// Cancelled apply item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelledReadyQueueItem {
    common: ReadyQueueCommon,
    last_claim_fence_seq: Option<FenceSeq>,
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

impl ReviewCommon {
    /// Builds shared review facts with monotonic timestamps.
    ///
    /// # Errors
    ///
    /// Returns an error when `updated_at` predates `created_at`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        review_id: ReviewId,
        subtask_id: SubtaskId,
        artifact_digest: ArtifactDigest,
        reviewer_session: SessionToken,
        review_subtask_id: SubtaskId,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> Result<Self, String> {
        if updated_at < created_at {
            return Err("review updated_at must be greater than or equal to created_at".to_owned());
        }
        Ok(Self {
            review_id,
            subtask_id,
            artifact_digest,
            reviewer_session,
            review_subtask_id,
            created_at,
            updated_at,
        })
    }

    #[must_use]
    pub fn review_id(&self) -> &str {
        self.review_id.as_str()
    }

    #[must_use]
    pub fn subtask_id(&self) -> &str {
        self.subtask_id.as_str()
    }

    #[must_use]
    pub fn artifact_digest(&self) -> &str {
        self.artifact_digest.as_str()
    }

    #[must_use]
    pub fn reviewer_session(&self) -> &str {
        self.reviewer_session.as_str()
    }

    #[must_use]
    pub fn review_subtask_id(&self) -> &str {
        self.review_subtask_id.as_str()
    }

    #[must_use]
    pub const fn created_at(&self) -> TimestampMs {
        self.created_at
    }

    #[must_use]
    pub const fn updated_at(&self) -> TimestampMs {
        self.updated_at
    }
}

impl RequestedReview {
    /// Builds a requested review row from validated common facts.
    #[must_use]
    pub const fn new(common: ReviewCommon) -> Self {
        Self { common }
    }

    #[must_use]
    pub const fn common(&self) -> &ReviewCommon {
        &self.common
    }
}

impl InProgressReview {
    /// Builds an in-progress review row from validated common facts.
    #[must_use]
    pub const fn new(common: ReviewCommon) -> Self {
        Self { common }
    }

    #[must_use]
    pub const fn common(&self) -> &ReviewCommon {
        &self.common
    }
}

impl DecidedReview {
    /// Builds a decided review row with required decision evidence.
    #[must_use]
    pub const fn new(
        common: ReviewCommon,
        verdict: ReviewVerdict,
        findings_digest: FindingsDigest,
    ) -> Self {
        Self {
            common,
            verdict,
            findings_digest,
        }
    }

    #[must_use]
    pub const fn common(&self) -> &ReviewCommon {
        &self.common
    }

    #[must_use]
    pub const fn verdict(&self) -> ReviewVerdict {
        self.verdict
    }

    #[must_use]
    pub fn findings_digest(&self) -> &str {
        self.findings_digest.as_str()
    }
}

impl SupersededReview {
    /// Builds a superseded review row from validated common facts.
    #[must_use]
    pub const fn new(common: ReviewCommon) -> Self {
        Self { common }
    }

    #[must_use]
    pub const fn common(&self) -> &ReviewCommon {
        &self.common
    }
}

impl Review {
    #[must_use]
    pub fn review_id(&self) -> &str {
        self.common().review_id()
    }

    #[must_use]
    pub fn subtask_id(&self) -> &str {
        self.common().subtask_id()
    }

    #[must_use]
    pub fn artifact_digest(&self) -> &str {
        self.common().artifact_digest()
    }

    #[must_use]
    pub fn reviewer_session(&self) -> &str {
        self.common().reviewer_session()
    }

    #[must_use]
    pub fn review_subtask_id(&self) -> &str {
        self.common().review_subtask_id()
    }

    #[must_use]
    pub const fn verdict(&self) -> Option<ReviewVerdict> {
        match self {
            Self::Decided(review) => Some(review.verdict()),
            Self::Requested(_) | Self::InProgress(_) | Self::Superseded(_) => None,
        }
    }

    #[must_use]
    pub fn findings_digest(&self) -> Option<&str> {
        match self {
            Self::Decided(review) => Some(review.findings_digest()),
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
        self.common().created_at().get()
    }

    #[must_use]
    pub const fn updated_at(&self) -> i64 {
        self.common().updated_at().get()
    }

    const fn common(&self) -> &ReviewCommon {
        match self {
            Self::Requested(review) => review.common(),
            Self::InProgress(review) => review.common(),
            Self::Decided(review) => review.common(),
            Self::Superseded(review) => review.common(),
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
        let common = ReviewCommon::new(
            ReviewId::parse(raw.review_id).map_err(|err| err.to_string())?,
            SubtaskId::parse(raw.subtask_id).map_err(|err| err.to_string())?,
            ArtifactDigest::parse(raw.artifact_digest).map_err(|err| err.to_string())?,
            SessionToken::parse(raw.reviewer_session).map_err(|err| err.to_string())?,
            SubtaskId::parse(review_subtask_id).map_err(|err| err.to_string())?,
            TimestampMs::parse(raw.created_at).map_err(|err| err.to_string())?,
            TimestampMs::parse(raw.updated_at).map_err(|err| err.to_string())?,
        )?;

        match raw.state {
            ReviewState::Requested => {
                if raw.verdict.is_some() || raw.findings_digest.is_some() {
                    return Err("requested reviews cannot carry decision evidence".to_owned());
                }
                Ok(Self::Requested(RequestedReview::new(common)))
            }
            ReviewState::InProgress => {
                if raw.verdict.is_some() || raw.findings_digest.is_some() {
                    return Err("in-progress reviews cannot carry decision evidence".to_owned());
                }
                Ok(Self::InProgress(InProgressReview::new(common)))
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
                Ok(Self::Decided(DecidedReview::new(
                    common,
                    verdict,
                    FindingsDigest::parse(findings_digest).map_err(|err| err.to_string())?,
                )))
            }
            ReviewState::Superseded => {
                if raw.verdict.is_some() || raw.findings_digest.is_some() {
                    return Err("superseded reviews cannot carry decision evidence".to_owned());
                }
                Ok(Self::Superseded(SupersededReview::new(common)))
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
    claim_fence_seq: Option<FenceSeq>,
    claim_lease_deadline: Option<LeaseDeadlineMs>,
    enqueued_at: i64,
    updated_at: i64,
}

impl ReadyQueueCommon {
    /// Builds shared ready-queue facts with monotonic timestamps.
    ///
    /// # Errors
    ///
    /// Returns an error when `updated_at` predates `enqueued_at`.
    pub fn new(
        queue_id: QueueId,
        artifact_digest: ArtifactDigest,
        subtask_id: SubtaskId,
        settlement_target: SettlementTarget,
        enqueued_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> Result<Self, String> {
        if updated_at < enqueued_at {
            return Err(
                "ready-queue updated_at must be greater than or equal to enqueued_at".to_owned(),
            );
        }
        Ok(Self {
            queue_id,
            artifact_digest,
            subtask_id,
            settlement_target,
            enqueued_at,
            updated_at,
        })
    }

    #[must_use]
    pub fn queue_id(&self) -> &str {
        self.queue_id.as_str()
    }

    #[must_use]
    pub fn artifact_digest(&self) -> &str {
        self.artifact_digest.as_str()
    }

    #[must_use]
    pub fn subtask_id(&self) -> &str {
        self.subtask_id.as_str()
    }

    #[must_use]
    pub const fn settlement_target(&self) -> SettlementTarget {
        self.settlement_target
    }

    #[must_use]
    pub const fn enqueued_at(&self) -> TimestampMs {
        self.enqueued_at
    }

    #[must_use]
    pub const fn updated_at(&self) -> TimestampMs {
        self.updated_at
    }
}

impl QueuedReadyQueueItem {
    /// Builds a queued apply item without active claim fields.
    #[must_use]
    pub const fn new(common: ReadyQueueCommon, last_claim_fence_seq: Option<FenceSeq>) -> Self {
        Self {
            common,
            last_claim_fence_seq,
        }
    }

    #[must_use]
    pub const fn common(&self) -> &ReadyQueueCommon {
        &self.common
    }

    #[must_use]
    pub const fn last_claim_fence_seq(&self) -> Option<FenceSeq> {
        self.last_claim_fence_seq
    }
}

impl ReadyQueueActiveClaim {
    /// Builds active ready-queue claim fields.
    #[must_use]
    pub const fn new(
        claimed_by_session_token: SessionToken,
        claim_fence_seq: FenceSeq,
        claim_lease_deadline: LeaseDeadlineMs,
    ) -> Self {
        Self {
            claimed_by_session_token,
            claim_fence_seq,
            claim_lease_deadline,
        }
    }

    #[must_use]
    pub fn claimed_by_session_token(&self) -> &str {
        self.claimed_by_session_token.as_str()
    }

    #[must_use]
    pub const fn claim_fence_seq(&self) -> FenceSeq {
        self.claim_fence_seq
    }

    #[must_use]
    pub const fn claim_lease_deadline(&self) -> LeaseDeadlineMs {
        self.claim_lease_deadline
    }
}

impl InFlightReadyQueueItem {
    /// Builds an in-flight apply item with complete active claim fields.
    #[must_use]
    pub const fn new(common: ReadyQueueCommon, claim: ReadyQueueActiveClaim) -> Self {
        Self { common, claim }
    }

    #[must_use]
    pub const fn common(&self) -> &ReadyQueueCommon {
        &self.common
    }

    #[must_use]
    pub const fn claim(&self) -> &ReadyQueueActiveClaim {
        &self.claim
    }
}

impl AppliedReadyQueueItem {
    /// Builds an applied queue item bound to an accepted fence.
    #[must_use]
    pub const fn new(common: ReadyQueueCommon, claim_fence_seq: FenceSeq) -> Self {
        Self {
            common,
            claim_fence_seq,
        }
    }

    #[must_use]
    pub const fn common(&self) -> &ReadyQueueCommon {
        &self.common
    }

    #[must_use]
    pub const fn claim_fence_seq(&self) -> FenceSeq {
        self.claim_fence_seq
    }
}

impl SupersededReadyQueueItem {
    /// Builds a superseded queue item without active claim fields.
    #[must_use]
    pub const fn new(common: ReadyQueueCommon, last_claim_fence_seq: Option<FenceSeq>) -> Self {
        Self {
            common,
            last_claim_fence_seq,
        }
    }

    #[must_use]
    pub const fn common(&self) -> &ReadyQueueCommon {
        &self.common
    }

    #[must_use]
    pub const fn last_claim_fence_seq(&self) -> Option<FenceSeq> {
        self.last_claim_fence_seq
    }
}

impl CancelledReadyQueueItem {
    /// Builds a cancelled queue item without active claim fields.
    #[must_use]
    pub const fn new(common: ReadyQueueCommon, last_claim_fence_seq: Option<FenceSeq>) -> Self {
        Self {
            common,
            last_claim_fence_seq,
        }
    }

    #[must_use]
    pub const fn common(&self) -> &ReadyQueueCommon {
        &self.common
    }

    #[must_use]
    pub const fn last_claim_fence_seq(&self) -> Option<FenceSeq> {
        self.last_claim_fence_seq
    }
}

impl ReadyQueueItem {
    #[must_use]
    pub fn queue_id(&self) -> &str {
        self.common().queue_id()
    }

    #[must_use]
    pub fn artifact_digest(&self) -> &str {
        self.common().artifact_digest()
    }

    #[must_use]
    pub fn subtask_id(&self) -> &str {
        self.common().subtask_id()
    }

    #[must_use]
    pub const fn settlement_target(&self) -> SettlementTarget {
        self.common().settlement_target()
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
            Self::InFlight(item) => Some(item.claim().claimed_by_session_token()),
            Self::Queued(_) | Self::Applied(_) | Self::Superseded(_) | Self::Cancelled(_) => None,
        }
    }

    #[must_use]
    pub fn claim_fence_seq(&self) -> Option<i64> {
        match self {
            Self::Queued(item) => item.last_claim_fence_seq().map(FenceSeq::get),
            Self::InFlight(item) => Some(item.claim().claim_fence_seq().get()),
            Self::Applied(item) => Some(item.claim_fence_seq().get()),
            Self::Superseded(item) => item.last_claim_fence_seq().map(FenceSeq::get),
            Self::Cancelled(item) => item.last_claim_fence_seq().map(FenceSeq::get),
        }
    }

    #[must_use]
    pub fn claim_lease_deadline(&self) -> Option<i64> {
        match self {
            Self::InFlight(item) => Some(item.claim().claim_lease_deadline().get()),
            Self::Queued(_) | Self::Applied(_) | Self::Superseded(_) | Self::Cancelled(_) => None,
        }
    }

    #[must_use]
    pub const fn enqueued_at(&self) -> i64 {
        self.common().enqueued_at().get()
    }

    #[must_use]
    pub const fn updated_at(&self) -> i64 {
        self.common().updated_at().get()
    }

    const fn common(&self) -> &ReadyQueueCommon {
        match self {
            Self::Queued(item) => item.common(),
            Self::InFlight(item) => item.common(),
            Self::Applied(item) => item.common(),
            Self::Superseded(item) => item.common(),
            Self::Cancelled(item) => item.common(),
        }
    }
}

impl TryFrom<RawReadyQueueItem> for ReadyQueueItem {
    type Error = String;

    fn try_from(raw: RawReadyQueueItem) -> Result<Self, Self::Error> {
        let common = ReadyQueueCommon::new(
            QueueId::parse(raw.queue_id).map_err(|err| err.to_string())?,
            ArtifactDigest::parse(raw.artifact_digest).map_err(|err| err.to_string())?,
            SubtaskId::parse(raw.subtask_id).map_err(|err| err.to_string())?,
            raw.settlement_target,
            TimestampMs::parse(raw.enqueued_at).map_err(|err| err.to_string())?,
            TimestampMs::parse(raw.updated_at).map_err(|err| err.to_string())?,
        )?;

        match raw.state {
            ReadyQueueState::Queued => {
                if raw.claimed_by_session_token.is_some() || raw.claim_lease_deadline.is_some() {
                    return Err("queued ready-queue items cannot carry an active claim".to_owned());
                }
                Ok(Self::Queued(QueuedReadyQueueItem::new(
                    common,
                    raw.claim_fence_seq,
                )))
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
                Ok(Self::InFlight(InFlightReadyQueueItem::new(
                    common,
                    ReadyQueueActiveClaim::new(
                        SessionToken::parse(claimed_by_session_token)
                            .map_err(|err| err.to_string())?,
                        claim_fence_seq,
                        claim_lease_deadline,
                    ),
                )))
            }
            ReadyQueueState::Applied => {
                if raw.claimed_by_session_token.is_some() || raw.claim_lease_deadline.is_some() {
                    return Err("applied ready-queue items cannot carry an active claim".to_owned());
                }
                let claim_fence_seq = raw.claim_fence_seq.ok_or_else(|| {
                    "applied ready-queue items require claim_fence_seq".to_owned()
                })?;
                Ok(Self::Applied(AppliedReadyQueueItem::new(
                    common,
                    claim_fence_seq,
                )))
            }
            ReadyQueueState::Superseded => {
                if raw.claimed_by_session_token.is_some() || raw.claim_lease_deadline.is_some() {
                    return Err(
                        "superseded ready-queue items cannot carry an active claim".to_owned()
                    );
                }
                Ok(Self::Superseded(SupersededReadyQueueItem::new(
                    common,
                    raw.claim_fence_seq,
                )))
            }
            ReadyQueueState::Cancelled => {
                if raw.claimed_by_session_token.is_some() || raw.claim_lease_deadline.is_some() {
                    return Err(
                        "cancelled ready-queue items cannot carry an active claim".to_owned()
                    );
                }
                Ok(Self::Cancelled(CancelledReadyQueueItem::new(
                    common,
                    raw.claim_fence_seq,
                )))
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
            claim_fence_seq: match item {
                ReadyQueueItem::Queued(item) => item.last_claim_fence_seq(),
                ReadyQueueItem::InFlight(item) => Some(item.claim().claim_fence_seq()),
                ReadyQueueItem::Applied(item) => Some(item.claim_fence_seq()),
                ReadyQueueItem::Superseded(item) => item.last_claim_fence_seq(),
                ReadyQueueItem::Cancelled(item) => item.last_claim_fence_seq(),
            },
            claim_lease_deadline: match item {
                ReadyQueueItem::InFlight(item) => Some(item.claim().claim_lease_deadline()),
                ReadyQueueItem::Queued(_)
                | ReadyQueueItem::Applied(_)
                | ReadyQueueItem::Superseded(_)
                | ReadyQueueItem::Cancelled(_) => None,
            },
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

/// Covey-owned registry row for an apply-gate-created worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyWorktree {
    pub path: ApplyWorktreePath,
    pub queue_id: QueueId,
    pub artifact_digest: ArtifactDigest,
    pub state: ApplyWorktreeState,
    pub recorded_by_session: SessionToken,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// Durable cleanup status for an applied OpenSpec-imported queue item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSpecArchiveStatus {
    pub queue_id: QueueId,
    pub subtask_id: SubtaskId,
    pub artifact_digest: ArtifactDigest,
    pub openspec_change_id: OpenSpecChangeId,
    pub state: OpenSpecArchiveStatusState,
    pub blocked_reason: Option<OpenSpecArchiveBlockedReason>,
    pub archive_proof_digest: Option<ArtifactDigest>,
    pub recorded_by_session: SessionToken,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawOpenSpecArchiveStatus {
    queue_id: String,
    subtask_id: String,
    artifact_digest: String,
    openspec_change_id: String,
    state: OpenSpecArchiveStatusState,
    blocked_reason: Option<String>,
    archive_proof_digest: Option<String>,
    recorded_by_session: String,
    created_at: i64,
    updated_at: i64,
}

impl OpenSpecArchiveStatus {
    /// Builds a cleanup status row, rejecting invalid state/evidence shapes.
    ///
    /// # Errors
    ///
    /// Returns an error when timestamps are not monotonic or state-specific
    /// evidence fields are missing or contradictory.
    #[allow(clippy::too_many_arguments)]
    pub fn try_from_parts(
        queue_id: QueueId,
        subtask_id: SubtaskId,
        artifact_digest: ArtifactDigest,
        openspec_change_id: OpenSpecChangeId,
        state: OpenSpecArchiveStatusState,
        blocked_reason: Option<OpenSpecArchiveBlockedReason>,
        archive_proof_digest: Option<ArtifactDigest>,
        recorded_by_session: SessionToken,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> Result<Self, String> {
        if updated_at < created_at {
            return Err(
                "openspec archive status updated_at must be greater than or equal to created_at"
                    .to_owned(),
            );
        }
        match state {
            OpenSpecArchiveStatusState::Blocked => {
                if blocked_reason.is_none() || archive_proof_digest.is_some() {
                    return Err(
                        "blocked OpenSpec archive status requires blocked_reason only".to_owned(),
                    );
                }
            }
            OpenSpecArchiveStatusState::Archived => {
                if blocked_reason.is_some() || archive_proof_digest.is_none() {
                    return Err(
                        "archived OpenSpec archive status requires archive_proof_digest only"
                            .to_owned(),
                    );
                }
            }
        }
        Ok(Self {
            queue_id,
            subtask_id,
            artifact_digest,
            openspec_change_id,
            state,
            blocked_reason,
            archive_proof_digest,
            recorded_by_session,
            created_at,
            updated_at,
        })
    }

    #[must_use]
    pub fn queue_id(&self) -> &str {
        self.queue_id.as_str()
    }

    #[must_use]
    pub fn subtask_id(&self) -> &str {
        self.subtask_id.as_str()
    }

    #[must_use]
    pub fn artifact_digest(&self) -> &str {
        self.artifact_digest.as_str()
    }

    #[must_use]
    pub fn openspec_change_id(&self) -> &str {
        self.openspec_change_id.as_str()
    }
}

impl TryFrom<RawOpenSpecArchiveStatus> for OpenSpecArchiveStatus {
    type Error = String;

    fn try_from(raw: RawOpenSpecArchiveStatus) -> Result<Self, Self::Error> {
        Self::try_from_parts(
            QueueId::parse(raw.queue_id).map_err(|err| err.to_string())?,
            SubtaskId::parse(raw.subtask_id).map_err(|err| err.to_string())?,
            ArtifactDigest::parse(raw.artifact_digest).map_err(|err| err.to_string())?,
            OpenSpecChangeId::parse(raw.openspec_change_id).map_err(|err| err.to_string())?,
            raw.state,
            raw.blocked_reason
                .map(OpenSpecArchiveBlockedReason::parse)
                .transpose()
                .map_err(|err| err.to_string())?,
            raw.archive_proof_digest
                .map(ArtifactDigest::parse)
                .transpose()
                .map_err(|err| err.to_string())?,
            SessionToken::parse(raw.recorded_by_session).map_err(|err| err.to_string())?,
            TimestampMs::parse(raw.created_at).map_err(|err| err.to_string())?,
            TimestampMs::parse(raw.updated_at).map_err(|err| err.to_string())?,
        )
    }
}

impl From<&OpenSpecArchiveStatus> for RawOpenSpecArchiveStatus {
    fn from(status: &OpenSpecArchiveStatus) -> Self {
        Self {
            queue_id: status.queue_id().to_owned(),
            subtask_id: status.subtask_id().to_owned(),
            artifact_digest: status.artifact_digest().to_owned(),
            openspec_change_id: status.openspec_change_id().to_owned(),
            state: status.state,
            blocked_reason: status
                .blocked_reason
                .as_ref()
                .map(|reason| reason.as_str().to_owned()),
            archive_proof_digest: status
                .archive_proof_digest
                .as_ref()
                .map(|digest| digest.as_str().to_owned()),
            recorded_by_session: status.recorded_by_session.as_str().to_owned(),
            created_at: status.created_at.get(),
            updated_at: status.updated_at.get(),
        }
    }
}

impl Serialize for OpenSpecArchiveStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawOpenSpecArchiveStatus::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OpenSpecArchiveStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawOpenSpecArchiveStatus::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// Durable explicit operator blocker for one OpenSpec current-work target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorBlocker {
    pub blocker_id: OperatorBlockerId,
    pub openspec_change_id: OpenSpecChangeId,
    pub target_kind: OperatorBlockerTargetKind,
    pub subtask_id: SubtaskId,
    pub queue_id: Option<QueueId>,
    pub artifact_digest: Option<ArtifactDigest>,
    pub reason: OperatorBlockerReason,
    pub source_evidence_id: Option<OperatorBlockerEvidenceId>,
    pub state: OperatorBlockerState,
    pub recorded_by_session: SessionToken,
    pub resolved_reason: Option<OperatorBlockerReason>,
    pub resolved_by_session: Option<SessionToken>,
    pub resolved_at: Option<TimestampMs>,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawOperatorBlocker {
    blocker_id: String,
    openspec_change_id: String,
    target_kind: OperatorBlockerTargetKind,
    subtask_id: String,
    queue_id: Option<String>,
    artifact_digest: Option<String>,
    reason: String,
    source_evidence_id: Option<String>,
    state: OperatorBlockerState,
    recorded_by_session: String,
    resolved_reason: Option<String>,
    resolved_by_session: Option<String>,
    resolved_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

impl OperatorBlocker {
    /// Builds an operator blocker row, rejecting contradictory target shapes.
    ///
    /// # Errors
    ///
    /// Returns an error when timestamps are not monotonic or target-specific
    /// fields are missing or contradictory.
    #[allow(clippy::too_many_arguments)]
    pub fn try_from_parts(
        blocker_id: OperatorBlockerId,
        openspec_change_id: OpenSpecChangeId,
        target_kind: OperatorBlockerTargetKind,
        subtask_id: SubtaskId,
        queue_id: Option<QueueId>,
        artifact_digest: Option<ArtifactDigest>,
        reason: OperatorBlockerReason,
        source_evidence_id: Option<OperatorBlockerEvidenceId>,
        state: OperatorBlockerState,
        recorded_by_session: SessionToken,
        resolved_reason: Option<OperatorBlockerReason>,
        resolved_by_session: Option<SessionToken>,
        resolved_at: Option<TimestampMs>,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> Result<Self, String> {
        if updated_at < created_at {
            return Err(
                "operator blocker updated_at must be greater than or equal to created_at"
                    .to_owned(),
            );
        }
        match target_kind {
            OperatorBlockerTargetKind::Subtask => {
                if queue_id.is_some() || artifact_digest.is_some() {
                    return Err(
                        "subtask operator blocker must not include queue or artifact".to_owned(),
                    );
                }
            }
            OperatorBlockerTargetKind::ReadyQueue => {
                if queue_id.is_none() || artifact_digest.is_none() {
                    return Err(
                        "ready-queue operator blocker requires queue and artifact".to_owned()
                    );
                }
            }
        }
        match state {
            OperatorBlockerState::Open => {
                if resolved_reason.is_some()
                    || resolved_by_session.is_some()
                    || resolved_at.is_some()
                {
                    return Err(
                        "open operator blocker must not include resolution metadata".to_owned()
                    );
                }
            }
            OperatorBlockerState::Resolved => {
                if resolved_reason.is_none()
                    || resolved_by_session.is_none()
                    || resolved_at.is_none()
                {
                    return Err("resolved operator blocker requires resolution metadata".to_owned());
                }
            }
        }
        Ok(Self {
            blocker_id,
            openspec_change_id,
            target_kind,
            subtask_id,
            queue_id,
            artifact_digest,
            reason,
            source_evidence_id,
            state,
            recorded_by_session,
            resolved_reason,
            resolved_by_session,
            resolved_at,
            created_at,
            updated_at,
        })
    }
}

impl TryFrom<RawOperatorBlocker> for OperatorBlocker {
    type Error = String;

    fn try_from(raw: RawOperatorBlocker) -> Result<Self, Self::Error> {
        Self::try_from_parts(
            OperatorBlockerId::parse(raw.blocker_id).map_err(|err| err.to_string())?,
            OpenSpecChangeId::parse(raw.openspec_change_id).map_err(|err| err.to_string())?,
            raw.target_kind,
            SubtaskId::parse(raw.subtask_id).map_err(|err| err.to_string())?,
            raw.queue_id
                .map(QueueId::parse)
                .transpose()
                .map_err(|err| err.to_string())?,
            raw.artifact_digest
                .map(ArtifactDigest::parse)
                .transpose()
                .map_err(|err| err.to_string())?,
            OperatorBlockerReason::parse(raw.reason).map_err(|err| err.to_string())?,
            raw.source_evidence_id
                .map(OperatorBlockerEvidenceId::parse)
                .transpose()
                .map_err(|err| err.to_string())?,
            raw.state,
            SessionToken::parse(raw.recorded_by_session).map_err(|err| err.to_string())?,
            raw.resolved_reason
                .map(OperatorBlockerReason::parse)
                .transpose()
                .map_err(|err| err.to_string())?,
            raw.resolved_by_session
                .map(SessionToken::parse)
                .transpose()
                .map_err(|err| err.to_string())?,
            raw.resolved_at
                .map(TimestampMs::parse)
                .transpose()
                .map_err(|err| err.to_string())?,
            TimestampMs::parse(raw.created_at).map_err(|err| err.to_string())?,
            TimestampMs::parse(raw.updated_at).map_err(|err| err.to_string())?,
        )
    }
}

impl From<&OperatorBlocker> for RawOperatorBlocker {
    fn from(blocker: &OperatorBlocker) -> Self {
        Self {
            blocker_id: blocker.blocker_id.as_str().to_owned(),
            openspec_change_id: blocker.openspec_change_id.as_str().to_owned(),
            target_kind: blocker.target_kind,
            subtask_id: blocker.subtask_id.as_str().to_owned(),
            queue_id: blocker.queue_id.as_ref().map(|id| id.as_str().to_owned()),
            artifact_digest: blocker
                .artifact_digest
                .as_ref()
                .map(|digest| digest.as_str().to_owned()),
            reason: blocker.reason.as_str().to_owned(),
            source_evidence_id: blocker
                .source_evidence_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            state: blocker.state,
            recorded_by_session: blocker.recorded_by_session.as_str().to_owned(),
            resolved_reason: blocker
                .resolved_reason
                .as_ref()
                .map(|reason| reason.as_str().to_owned()),
            resolved_by_session: blocker
                .resolved_by_session
                .as_ref()
                .map(|session| session.as_str().to_owned()),
            resolved_at: blocker.resolved_at.map(TimestampMs::get),
            created_at: blocker.created_at.get(),
            updated_at: blocker.updated_at.get(),
        }
    }
}

impl Serialize for OperatorBlocker {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawOperatorBlocker::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OperatorBlocker {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawOperatorBlocker::deserialize(deserializer)?
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
    pub verifier: VerifierId,
    pub verdict_digest: ArtifactDigest,
    pub seal_digest: ArtifactDigest,
    pub recorded_by_session: SessionToken,
    pub created_at: TimestampMs,
}

/// Native blocker evidence emitted by the apply gate for one current attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyGateBlocker {
    pub queue_id: QueueId,
    pub artifact_digest: ArtifactDigest,
    pub review_id: ReviewId,
    pub findings_digest: FindingsDigest,
    pub claim_fence_seq: FenceSeq,
    pub verifier: VerifierId,
    pub blocker_kind: ApplyGateBlockerKind,
    pub reason: ApplyGateBlockerReason,
    pub evidence_id: ApplyGateBlockerEvidenceId,
    pub recorded_by_session: SessionToken,
    pub created_at: TimestampMs,
}

/// Native Authority reconcile evidence retained for one settlement attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementReconcileBlocker {
    pub queue_id: QueueId,
    pub artifact_digest: ArtifactDigest,
    pub review_id: ReviewId,
    pub findings_digest: FindingsDigest,
    pub claim_fence_seq: FenceSeq,
    pub reconcile_reason: SettlementReconcileReason,
    pub authority_evidence_id: SettlementReconcileEvidenceId,
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
    pub(super) seq: EventSeq,
    event_type: EventType,
    object_type: ObjectType,
    pub(super) object_id: EventObjectId,
    pub(super) actor: EventActor,
    payload_json: EventPayloadJson,
    pub created_at: TimestampMs,
}

/// Decoded event-log row with typed payload.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedEvent {
    pub(super) seq: EventSeq,
    pub(super) object_id: EventObjectId,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct EventPayloadJson(String);

impl Event {
    /// Builds a session-authored event-log record.
    ///
    /// # Errors
    ///
    /// Returns an error when the declared event/object type does not match the
    /// JSON payload shape.
    pub fn session(
        seq: i64,
        event_type: EventType,
        object_type: ObjectType,
        object_id: String,
        session_token: SessionToken,
        payload_json: String,
        created_at: TimestampMs,
    ) -> Result<Self, String> {
        let payload_json = EventPayloadJson::parse(event_type, object_type, payload_json)?;
        Ok(Self {
            seq: EventSeq::parse(seq).map_err(|err| err.to_string())?,
            event_type,
            object_type,
            object_id: EventObjectId::parse(object_id).map_err(|err| err.to_string())?,
            actor: EventActor::Session { session_token },
            payload_json,
            created_at,
        })
    }

    /// Builds a system-authored event-log record.
    ///
    /// # Errors
    ///
    /// Returns an error when the declared event/object type does not match the
    /// JSON payload shape.
    pub fn system(
        seq: i64,
        event_type: EventType,
        object_type: ObjectType,
        object_id: String,
        payload_json: String,
        created_at: TimestampMs,
    ) -> Result<Self, String> {
        let payload_json = EventPayloadJson::parse(event_type, object_type, payload_json)?;
        Ok(Self {
            seq: EventSeq::parse(seq).map_err(|err| err.to_string())?,
            event_type,
            object_type,
            object_id: EventObjectId::parse(object_id).map_err(|err| err.to_string())?,
            actor: EventActor::System,
            payload_json,
            created_at,
        })
    }

    fn from_raw(raw: RawEvent) -> Result<Self, String> {
        let actor = EventActor::try_from_parts(raw.actor_kind, raw.session_token)?;
        let payload_json =
            EventPayloadJson::parse(raw.event_type, raw.object_type, raw.payload_json)?;
        Ok(Self {
            seq: EventSeq::parse(raw.seq).map_err(|err| err.to_string())?,
            event_type: raw.event_type,
            object_type: raw.object_type,
            object_id: EventObjectId::parse(raw.object_id).map_err(|err| err.to_string())?,
            actor,
            payload_json,
            created_at: raw.created_at,
        })
    }

    pub(crate) fn from_stored_row(
        seq: i64,
        event_type: EventType,
        object_type: ObjectType,
        object_id: String,
        actor_kind: ActorKind,
        session_token: Option<SessionToken>,
        payload_json: String,
        created_at: TimestampMs,
    ) -> Result<Self, String> {
        let actor = EventActor::try_from_parts(actor_kind, session_token)?;
        Ok(Self {
            seq: EventSeq::parse(seq).map_err(|err| err.to_string())?,
            event_type,
            object_type,
            object_id: EventObjectId::parse(object_id).map_err(|err| err.to_string())?,
            actor,
            payload_json: EventPayloadJson::from_stored(payload_json),
            created_at,
        })
    }

    /// Returns the positive event-log sequence number.
    #[must_use]
    pub const fn seq(&self) -> i64 {
        self.seq.get()
    }

    /// Returns the event kind declared by this raw event.
    #[must_use]
    pub const fn event_type(&self) -> EventType {
        self.event_type
    }

    /// Returns the object kind declared by this raw event.
    #[must_use]
    pub const fn object_type(&self) -> ObjectType {
        self.object_type
    }

    /// Returns the event object's validated identifier.
    #[must_use]
    pub fn object_id(&self) -> &str {
        self.object_id.as_str()
    }

    /// Returns the raw JSON payload stored for this event.
    ///
    /// Events built through mutation APIs validate this against `event_type`,
    /// while persisted event-log reads preserve historical rows so operators can
    /// inspect historical payloads that no longer pass current typed validation.
    #[must_use]
    pub fn payload_json(&self) -> &str {
        self.payload_json.as_str()
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

impl EventPayloadJson {
    fn parse(
        event_type: EventType,
        object_type: ObjectType,
        payload_json: String,
    ) -> Result<Self, String> {
        let payload = EventPayload::from_json(event_type, &payload_json)
            .map_err(|error| format!("event payload does not match {event_type}: {error}"))?;
        let expected_object_type = payload.object_type();
        if object_type != expected_object_type {
            return Err(format!(
                "event payload implies object_type {expected_object_type}, got {object_type}"
            ));
        }
        Ok(Self(payload_json))
    }

    fn as_str(&self) -> &str {
        &self.0
    }

    const fn from_stored(payload_json: String) -> Self {
        Self(payload_json)
    }
}

impl TypedEvent {
    /// Returns the positive event-log sequence number.
    #[must_use]
    pub const fn seq(&self) -> i64 {
        self.seq.get()
    }

    /// Returns the event kind implied by the typed payload variant.
    #[must_use]
    pub const fn event_type(&self) -> EventType {
        self.payload.event_type()
    }

    /// Returns the object kind implied by the typed payload variant.
    #[must_use]
    pub fn object_type(&self) -> ObjectType {
        self.payload.object_type()
    }

    /// Returns the event object's validated identifier.
    #[must_use]
    pub fn object_id(&self) -> &str {
        self.object_id.as_str()
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
            seq: self.seq(),
            event_type: self.event_type,
            object_type: self.object_type,
            object_id: self.object_id().to_owned(),
            actor_kind: self.actor.actor_kind(),
            session_token: self.actor.session_token().cloned(),
            payload_json: self.payload_json().to_owned(),
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
            seq: self.seq(),
            event_type: self.event_type(),
            object_type: self.object_type(),
            object_id: self.object_id().to_owned(),
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
        let expected_event_type = raw.payload.event_type();
        if raw.event_type != expected_event_type {
            return Err(serde::de::Error::custom(format!(
                "typed event payload implies event_type {expected_event_type}, got {}",
                raw.event_type
            )));
        }
        let expected_object_type = raw.payload.object_type();
        if raw.object_type != expected_object_type {
            return Err(serde::de::Error::custom(format!(
                "typed event payload implies object_type {expected_object_type}, got {}",
                raw.object_type
            )));
        }
        let actor = EventActor::try_from_parts(raw.actor_kind, raw.session_token)
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            seq: EventSeq::parse(raw.seq).map_err(serde::de::Error::custom)?,
            object_id: EventObjectId::parse(raw.object_id).map_err(serde::de::Error::custom)?,
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
    PermissiveLandingRecorded(RecordPermissiveLandingReceiptReq),
    ReadyQueueEnqueued(EnqueueForApplyReq),
    ReadyQueueInFlight(ReadyQueueClaim),
    ApplyVerificationRecorded(RecordApplyVerificationReq),
    ApplyGateBlockerRecorded(RecordApplyGateBlockerReq),
    SettlementReconcileBlockerRecorded(RecordSettlementReconcileBlockerReq),
    ReadyQueueApplied(MarkAppliedReq),
    ApplyWorktreeRecorded(RecordApplyWorktreeReq),
    ApplyWorktreeStateRecorded(MarkApplyWorktreeStateReq),
    ProseApplyBlockerRecorded(RecordProseApplyBlockerReq),
    OpenSpecArchiveStatusRecorded(RecordOpenSpecArchiveStatusReq),
    OperatorBlockerRecorded(RecordOperatorBlockerReq),
    OperatorBlockerResolved(ResolveOperatorBlockerReq),
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

impl EventPayload {
    /// Returns the event kind implied by this payload variant.
    #[must_use]
    pub const fn event_type(&self) -> EventType {
        match self {
            Self::SessionRegistered(_) => EventType::SessionRegistered,
            Self::SessionHeartbeat(_) => EventType::SessionHeartbeat,
            Self::SessionExited(_) => EventType::SessionExited,
            Self::RuntimeAttestationRecorded(_) => EventType::RuntimeAttestationRecorded,
            Self::MetaTaskSubmitted(_) => EventType::MetaTaskSubmitted,
            Self::MetaTaskCancelled(_) => EventType::MetaTaskCancelled,
            Self::SubtaskCreated(_) => EventType::SubtaskCreated,
            Self::SubtaskClaimed(_) => EventType::SubtaskClaimed,
            Self::SubtaskStarted(_) => EventType::SubtaskStarted,
            Self::SubtaskAbandoned(_) => EventType::SubtaskAbandoned,
            Self::ClaimReleased(_) => EventType::ClaimReleased,
            Self::ClaimRenewed(_) => EventType::ClaimRenewed,
            Self::ArtifactPublished(_) => EventType::ArtifactPublished,
            Self::ReviewRequested(_) => EventType::ReviewRequested,
            Self::ReviewDecided(_) => EventType::ReviewDecided,
            Self::PermissiveLandingRecorded(_) => EventType::PermissiveLandingRecorded,
            Self::ReadyQueueEnqueued(_) => EventType::ReadyQueueEnqueued,
            Self::ReadyQueueInFlight(_) => EventType::ReadyQueueInFlight,
            Self::ApplyVerificationRecorded(_) => EventType::ApplyVerificationRecorded,
            Self::ApplyGateBlockerRecorded(_) => EventType::ApplyGateBlockerRecorded,
            Self::SettlementReconcileBlockerRecorded(_) => {
                EventType::SettlementReconcileBlockerRecorded
            }
            Self::ReadyQueueApplied(_) => EventType::ReadyQueueApplied,
            Self::ApplyWorktreeRecorded(_) => EventType::ApplyWorktreeRecorded,
            Self::ApplyWorktreeStateRecorded(_) => EventType::ApplyWorktreeStateRecorded,
            Self::ProseApplyBlockerRecorded(_) => EventType::ProseApplyBlockerRecorded,
            Self::OpenSpecArchiveStatusRecorded(_) => EventType::OpenSpecArchiveStatusRecorded,
            Self::OperatorBlockerRecorded(_) => EventType::OperatorBlockerRecorded,
            Self::OperatorBlockerResolved(_) => EventType::OperatorBlockerResolved,
            Self::ReadyQueueSuperseded(_) => EventType::ReadyQueueSuperseded,
            Self::ReservationRequested(_) => EventType::ReservationRequested,
            Self::ReservationReleased(_) => EventType::ReservationReleased,
            Self::ReservationRenewed(_) => EventType::ReservationRenewed,
            Self::ConflictResolved(_) => EventType::ConflictResolved,
            Self::SessionsReaped(_) => EventType::SessionsReaped,
            Self::ClaimsExpired(_) => EventType::ClaimsExpired,
            Self::ReservationsExpired(_) => EventType::ReservationsExpired,
            Self::OpenSpecImported(_) => EventType::OpenSpecImported,
        }
    }

    /// Returns the object kind implied by this payload variant.
    #[must_use]
    pub fn object_type(&self) -> ObjectType {
        match self {
            Self::SessionRegistered(_)
            | Self::SessionHeartbeat(_)
            | Self::SessionExited(_)
            | Self::SessionsReaped(_) => ObjectType::Session,
            Self::RuntimeAttestationRecorded(_) => ObjectType::RuntimeAttestation,
            Self::MetaTaskSubmitted(_) | Self::MetaTaskCancelled(_) => ObjectType::MetaTask,
            Self::SubtaskCreated(_) | Self::SubtaskStarted(_) | Self::SubtaskAbandoned(_) => {
                ObjectType::Subtask
            }
            Self::SubtaskClaimed(_)
            | Self::ClaimReleased(_)
            | Self::ClaimRenewed(_)
            | Self::ClaimsExpired(_) => ObjectType::Claim,
            Self::ArtifactPublished(_) => ObjectType::Artifact,
            Self::ReviewRequested(_)
            | Self::ReviewDecided(_)
            | Self::PermissiveLandingRecorded(_) => ObjectType::Review,
            Self::ReadyQueueEnqueued(_)
            | Self::ReadyQueueInFlight(_)
            | Self::ApplyVerificationRecorded(_)
            | Self::ApplyGateBlockerRecorded(_)
            | Self::SettlementReconcileBlockerRecorded(_)
            | Self::ReadyQueueApplied(_)
            | Self::ReadyQueueSuperseded(_)
            | Self::ProseApplyBlockerRecorded(_) => ObjectType::ReadyQueue,
            Self::ApplyWorktreeRecorded(_) | Self::ApplyWorktreeStateRecorded(_) => {
                ObjectType::ApplyWorktree
            }
            Self::OpenSpecArchiveStatusRecorded(_) => ObjectType::ReadyQueue,
            Self::OperatorBlockerRecorded(_) | Self::OperatorBlockerResolved(_) => {
                ObjectType::OperatorBlocker
            }
            Self::ReservationRequested(_)
            | Self::ReservationReleased(_)
            | Self::ReservationRenewed(_)
            | Self::ReservationsExpired(_) => ObjectType::Reservation,
            Self::ConflictResolved(_) => ObjectType::Conflict,
            Self::OpenSpecImported(event) => event.object_type(),
        }
    }
}

/// Persisted unresolved conflict row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    conflict_id: ConflictId,
    object_id: ReservationId,
    payload: ConflictPayload,
    detected_at: TimestampMs,
    resolution_state: ConflictResolutionState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConflictPayload {
    ReservationOverlap(ReservationOverlapConflictPayload),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawConflict {
    conflict_id: ConflictId,
    object_type: ObjectType,
    object_id: String,
    conflict_kind: ConflictKind,
    payload_json: String,
    detected_at: TimestampMs,
    resolution_state: ConflictResolutionState,
}

impl Conflict {
    fn try_from_raw(raw: RawConflict) -> Result<Self, String> {
        let payload =
            ConflictPayload::from_parts(raw.conflict_kind, raw.object_type, &raw.payload_json)?;
        let object_id = ReservationId::parse(raw.object_id).map_err(|err| err.to_string())?;
        payload.validate_object_id(&object_id)?;
        Ok(Self {
            conflict_id: raw.conflict_id,
            object_id,
            payload,
            detected_at: raw.detected_at,
            resolution_state: raw.resolution_state,
        })
    }

    /// Returns the stable conflict id.
    #[must_use]
    pub fn conflict_id(&self) -> &str {
        self.conflict_id.as_str()
    }

    /// Returns the object class this conflict targets.
    #[must_use]
    pub const fn object_type(&self) -> ObjectType {
        self.payload.object_type()
    }

    /// Returns the target object id.
    #[must_use]
    pub fn object_id(&self) -> &str {
        self.object_id.as_str()
    }

    /// Returns the typed conflict kind.
    #[must_use]
    pub const fn conflict_kind(&self) -> ConflictKind {
        self.payload.conflict_kind()
    }

    /// Returns the validated JSON payload stored for this conflict.
    #[must_use]
    pub fn payload_json(&self) -> String {
        self.payload.payload_json()
    }

    /// Returns the current operator resolution state.
    #[must_use]
    pub const fn resolution_state(&self) -> ConflictResolutionState {
        self.resolution_state
    }

    /// Returns the detection timestamp.
    #[must_use]
    pub const fn detected_at(&self) -> TimestampMs {
        self.detected_at
    }
}

impl Serialize for Conflict {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawConflict {
            conflict_id: self.conflict_id.clone(),
            object_type: self.object_type(),
            object_id: self.object_id().to_owned(),
            conflict_kind: self.conflict_kind(),
            payload_json: self.payload_json(),
            detected_at: self.detected_at,
            resolution_state: self.resolution_state,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Conflict {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawConflict::deserialize(deserializer)?;
        Self::try_from_raw(raw).map_err(serde::de::Error::custom)
    }
}

impl ConflictPayload {
    fn from_parts(
        conflict_kind: ConflictKind,
        object_type: ObjectType,
        payload_json: &str,
    ) -> Result<Self, String> {
        match conflict_kind {
            ConflictKind::ReservationOverlap => {
                if object_type != ObjectType::Reservation {
                    return Err(
                        "reservation_overlap conflicts must target reservation objects".into(),
                    );
                }
                let payload =
                    serde_json::from_str::<ReservationOverlapConflictPayload>(payload_json)
                        .map_err(|error| {
                            format!("reservation_overlap conflicts require typed payload: {error}")
                        })?;
                Ok(Self::ReservationOverlap(payload))
            }
        }
    }

    const fn object_type(&self) -> ObjectType {
        match self {
            Self::ReservationOverlap(_) => ObjectType::Reservation,
        }
    }

    const fn conflict_kind(&self) -> ConflictKind {
        match self {
            Self::ReservationOverlap(_) => ConflictKind::ReservationOverlap,
        }
    }

    fn payload_json(&self) -> String {
        match self {
            Self::ReservationOverlap(payload) => serde_json::to_string(payload)
                .expect("typed reservation overlap conflict payload should serialize"),
        }
    }

    fn validate_object_id(&self, object_id: &ReservationId) -> Result<(), String> {
        match self {
            Self::ReservationOverlap(payload) => {
                if object_id.as_str() == payload.reservation_id()
                    || object_id.as_str() == payload.overlapping_reservation_id()
                {
                    Ok(())
                } else {
                    Err(
                        "reservation_overlap conflict object_id must match one overlapping reservation"
                            .into(),
                    )
                }
            }
        }
    }
}

const MUTATION_IDEMPOTENCY_KEY_MAX_BYTES: usize = 256;
const MUTATION_REQUEST_HASH_HEX_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MutationIdempotencyRecord {
    actor_key: MutationActorKey,
    operation: MutationOperation,
    idempotency_key: MutationIdempotencyKey,
    request_hash: MutationRequestHash,
    response_json: MutationResponseJson,
    pub created_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MutationActorKey(String);

#[derive(Debug, Clone, PartialEq, Eq)]
struct MutationOperation(String);

#[derive(Debug, Clone, PartialEq, Eq)]
struct MutationIdempotencyKey(String);

#[derive(Debug, Clone, PartialEq, Eq)]
struct MutationRequestHash(String);

#[derive(Debug, Clone, PartialEq, Eq)]
struct MutationResponseJson(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawMutationIdempotencyRecord {
    actor_key: String,
    operation: String,
    idempotency_key: String,
    request_hash: String,
    response_json: String,
    created_at: TimestampMs,
}

impl MutationIdempotencyRecord {
    /// Builds a typed idempotency record from the flat storage shape.
    ///
    /// # Errors
    ///
    /// Returns an error when any key field is blank or padded, the request hash
    /// is not a 64-byte lowercase hex digest, or the stored response is not JSON.
    pub(crate) fn try_from_parts(
        actor_key: impl Into<String>,
        operation: impl Into<String>,
        idempotency_key: impl Into<String>,
        request_hash: impl Into<String>,
        response_json: impl Into<String>,
        created_at: TimestampMs,
    ) -> Result<Self, String> {
        Ok(Self {
            actor_key: MutationActorKey::parse(actor_key)?,
            operation: MutationOperation::parse(operation)?,
            idempotency_key: MutationIdempotencyKey::parse(idempotency_key)?,
            request_hash: MutationRequestHash::parse(request_hash)?,
            response_json: MutationResponseJson::parse(response_json)?,
            created_at,
        })
    }

    #[must_use]
    pub(crate) fn actor_key(&self) -> &str {
        self.actor_key.as_str()
    }

    #[must_use]
    pub(crate) fn operation(&self) -> &str {
        self.operation.as_str()
    }

    #[must_use]
    pub(crate) fn idempotency_key(&self) -> &str {
        self.idempotency_key.as_str()
    }

    #[must_use]
    pub(crate) fn request_hash(&self) -> &str {
        self.request_hash.as_str()
    }

    #[must_use]
    pub(crate) fn response_json(&self) -> &str {
        self.response_json.as_str()
    }
}

impl MutationActorKey {
    fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        validate_mutation_normalized_text("actor_key", &value)?;
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl MutationOperation {
    fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        validate_mutation_normalized_text("operation", &value)?;
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl MutationIdempotencyKey {
    fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err("idempotency_key must not be empty".to_owned());
        }
        if value.len() > MUTATION_IDEMPOTENCY_KEY_MAX_BYTES {
            return Err(format!(
                "idempotency_key exceeds {MUTATION_IDEMPOTENCY_KEY_MAX_BYTES} bytes"
            ));
        }
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl MutationRequestHash {
    fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.len() != MUTATION_REQUEST_HASH_HEX_BYTES
            || !value
                .chars()
                .all(|ch| ch.is_ascii_digit() || matches!(ch, 'a'..='f'))
        {
            return Err("request_hash must be a 64-byte lowercase hex blake3 digest".to_owned());
        }
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl MutationResponseJson {
    fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        serde_json::from_str::<serde_json::Value>(&value)
            .map_err(|err| format!("response_json must be valid JSON: {err}"))?;
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_mutation_normalized_text(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if value.trim() != value {
        return Err(format!(
            "{field} must not include leading or trailing whitespace"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{field} must not contain control characters"));
    }
    Ok(())
}

impl Serialize for MutationIdempotencyRecord {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawMutationIdempotencyRecord::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MutationIdempotencyRecord {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawMutationIdempotencyRecord::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl From<&MutationIdempotencyRecord> for RawMutationIdempotencyRecord {
    fn from(record: &MutationIdempotencyRecord) -> Self {
        Self {
            actor_key: record.actor_key().to_owned(),
            operation: record.operation().to_owned(),
            idempotency_key: record.idempotency_key().to_owned(),
            request_hash: record.request_hash().to_owned(),
            response_json: record.response_json().to_owned(),
            created_at: record.created_at,
        }
    }
}

impl TryFrom<RawMutationIdempotencyRecord> for MutationIdempotencyRecord {
    type Error = String;

    fn try_from(raw: RawMutationIdempotencyRecord) -> Result<Self, Self::Error> {
        Self::try_from_parts(
            raw.actor_key,
            raw.operation,
            raw.idempotency_key,
            raw.request_hash,
            raw.response_json,
            raw.created_at,
        )
    }
}

#[cfg(test)]
mod mutation_idempotency_record_tests {
    use super::*;

    fn valid_hash() -> String {
        "a".repeat(MUTATION_REQUEST_HASH_HEX_BYTES)
    }

    fn valid_record() -> MutationIdempotencyRecord {
        MutationIdempotencyRecord::try_from_parts(
            "session-1",
            "claim_subtask",
            "idempotency key with spaces",
            valid_hash(),
            r#"{"ok":true}"#,
            TimestampMs::parse(10).expect("valid timestamp"),
        )
        .expect("valid mutation idempotency record")
    }

    #[test]
    fn mutation_idempotency_record_preserves_flat_storage_shape() {
        let record = valid_record();

        let value = serde_json::to_value(&record).expect("serialize idempotency record");
        assert_eq!(
            value,
            serde_json::json!({
                "actor_key": "session-1",
                "operation": "claim_subtask",
                "idempotency_key": "idempotency key with spaces",
                "request_hash": valid_hash(),
                "response_json": r#"{"ok":true}"#,
                "created_at": 10
            })
        );

        let round_trip: MutationIdempotencyRecord =
            serde_json::from_value(value).expect("flat idempotency record should deserialize");
        assert_eq!(round_trip, record);
        assert_eq!(round_trip.actor_key(), "session-1");
        assert_eq!(round_trip.operation(), "claim_subtask");
        assert_eq!(round_trip.idempotency_key(), "idempotency key with spaces");
        assert_eq!(round_trip.request_hash(), valid_hash());
        assert_eq!(round_trip.response_json(), r#"{"ok":true}"#);
    }

    #[test]
    fn mutation_idempotency_record_rejects_invalid_request_hash_and_response_json() {
        let invalid_hash = serde_json::json!({
            "actor_key": "session-1",
            "operation": "claim_subtask",
            "idempotency_key": "idem-1",
            "request_hash": "not-a-hash",
            "response_json": "{}",
            "created_at": 10
        });
        let err = serde_json::from_value::<MutationIdempotencyRecord>(invalid_hash)
            .expect_err("invalid request hashes should be rejected");
        assert!(
            err.to_string()
                .contains("request_hash must be a 64-byte lowercase hex blake3 digest"),
            "unexpected error: {err}"
        );

        let invalid_json = serde_json::json!({
            "actor_key": "session-1",
            "operation": "claim_subtask",
            "idempotency_key": "idem-1",
            "request_hash": valid_hash(),
            "response_json": "not-json",
            "created_at": 10
        });
        let err = serde_json::from_value::<MutationIdempotencyRecord>(invalid_json)
            .expect_err("non-json responses should be rejected");
        assert!(
            err.to_string().contains("response_json must be valid JSON"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn mutation_idempotency_record_rejects_invalid_key_fields() {
        let padded_actor = MutationIdempotencyRecord::try_from_parts(
            " session-1",
            "claim_subtask",
            "idem-1",
            valid_hash(),
            "{}",
            TimestampMs::parse(10).expect("valid timestamp"),
        )
        .expect_err("padded actor keys should be rejected");
        assert_eq!(
            padded_actor,
            "actor_key must not include leading or trailing whitespace"
        );

        let blank_operation = MutationIdempotencyRecord::try_from_parts(
            "session-1",
            " ",
            "idem-1",
            valid_hash(),
            "{}",
            TimestampMs::parse(10).expect("valid timestamp"),
        )
        .expect_err("blank operations should be rejected");
        assert_eq!(blank_operation, "operation must not be empty");

        let blank_idempotency_key = MutationIdempotencyRecord::try_from_parts(
            "session-1",
            "claim_subtask",
            " ",
            valid_hash(),
            "{}",
            TimestampMs::parse(10).expect("valid timestamp"),
        )
        .expect_err("blank idempotency keys should be rejected");
        assert_eq!(blank_idempotency_key, "idempotency_key must not be empty");
    }
}

/// Conflict payload describing an overlapping reservation pair.
///
/// The flat stored shape is preserved for conflict JSON, but the Rust value
/// carries validated reservation/subtask identifiers and scope variants instead
/// of raw `ScopeClass` plus arbitrary key pairs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservationOverlapConflictPayload {
    reservation_id: ReservationId,
    overlapping_reservation_id: ReservationId,
    owner_subtask_id: SubtaskId,
    overlapping_owner_subtask_id: SubtaskId,
    scope: ReservationOverlapScope,
    overlapping_scope: ReservationOverlapScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReservationOverlapScope {
    ExactPath { scope_key: ReservationScopeKey },
    Subtree { scope_key: ReservationScopeKey },
    RepoGlobal,
    GeneratedSet { scope_key: ReservationScopeKey },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawReservationOverlapConflictPayload {
    reservation_id: String,
    overlapping_reservation_id: String,
    owner_subtask_id: String,
    overlapping_owner_subtask_id: String,
    scope_class: ScopeClass,
    scope_key: String,
    overlapping_scope_class: ScopeClass,
    overlapping_scope_key: String,
}

impl ReservationOverlapConflictPayload {
    /// Builds a typed reservation-overlap conflict payload from flat fields.
    ///
    /// # Errors
    ///
    /// Returns an error when any identifier is malformed or a scope class/key
    /// pair is internally inconsistent.
    #[allow(clippy::too_many_arguments)]
    pub fn try_from_raw_parts(
        reservation_id: impl Into<String>,
        overlapping_reservation_id: impl Into<String>,
        owner_subtask_id: impl Into<String>,
        overlapping_owner_subtask_id: impl Into<String>,
        scope_class: ScopeClass,
        scope_key: impl Into<String>,
        overlapping_scope_class: ScopeClass,
        overlapping_scope_key: impl Into<String>,
    ) -> Result<Self, String> {
        Ok(Self {
            reservation_id: ReservationId::parse(reservation_id).map_err(|err| err.to_string())?,
            overlapping_reservation_id: ReservationId::parse(overlapping_reservation_id)
                .map_err(|err| err.to_string())?,
            owner_subtask_id: SubtaskId::parse(owner_subtask_id).map_err(|err| err.to_string())?,
            overlapping_owner_subtask_id: SubtaskId::parse(overlapping_owner_subtask_id)
                .map_err(|err| err.to_string())?,
            scope: ReservationOverlapScope::from_parts(scope_class, scope_key.into())?,
            overlapping_scope: ReservationOverlapScope::from_parts(
                overlapping_scope_class,
                overlapping_scope_key.into(),
            )?,
        })
    }

    /// Returns the newly requested reservation id.
    #[must_use]
    pub fn reservation_id(&self) -> &str {
        self.reservation_id.as_str()
    }

    /// Returns the existing overlapping reservation id.
    #[must_use]
    pub fn overlapping_reservation_id(&self) -> &str {
        self.overlapping_reservation_id.as_str()
    }

    /// Returns the owner subtask for the newly requested reservation.
    #[must_use]
    pub fn owner_subtask_id(&self) -> &str {
        self.owner_subtask_id.as_str()
    }

    /// Returns the owner subtask for the existing overlapping reservation.
    #[must_use]
    pub fn overlapping_owner_subtask_id(&self) -> &str {
        self.overlapping_owner_subtask_id.as_str()
    }

    /// Returns the requested reservation scope class.
    #[must_use]
    pub const fn scope_class(&self) -> ScopeClass {
        self.scope.scope_class()
    }

    /// Returns the requested reservation scope key.
    #[must_use]
    pub fn scope_key(&self) -> &str {
        self.scope.scope_key()
    }

    /// Returns the existing overlapping reservation scope class.
    #[must_use]
    pub const fn overlapping_scope_class(&self) -> ScopeClass {
        self.overlapping_scope.scope_class()
    }

    /// Returns the existing overlapping reservation scope key.
    #[must_use]
    pub fn overlapping_scope_key(&self) -> &str {
        self.overlapping_scope.scope_key()
    }
}

impl ReservationOverlapScope {
    fn from_parts(scope_class: ScopeClass, scope_key: String) -> Result<Self, String> {
        match scope_class {
            ScopeClass::ExactPath => {
                let scope_key = parse_overlap_scope_key(scope_class, scope_key)?;
                Ok(Self::ExactPath { scope_key })
            }
            ScopeClass::Subtree => {
                let scope_key = parse_overlap_scope_key(scope_class, scope_key)?;
                Ok(Self::Subtree { scope_key })
            }
            ScopeClass::RepoGlobal => {
                if scope_key != "repo" {
                    return Err(
                        "repo-global reservation overlap scopes require scope_key `repo`"
                            .to_owned(),
                    );
                }
                Ok(Self::RepoGlobal)
            }
            ScopeClass::GeneratedSet => {
                let scope_key = parse_overlap_scope_key(scope_class, scope_key)?;
                Ok(Self::GeneratedSet { scope_key })
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
            | Self::GeneratedSet { scope_key } => scope_key.as_str(),
            Self::RepoGlobal => "repo",
        }
    }
}

fn parse_overlap_scope_key(
    scope_class: ScopeClass,
    scope_key: String,
) -> Result<ReservationScopeKey, String> {
    if scope_key.trim().is_empty() {
        return Err(format!(
            "{scope_class} reservation overlap scopes require scope_key"
        ));
    }
    if scope_key.trim() != scope_key {
        return Err(format!(
            "{scope_class} reservation overlap scope_key must be normalized"
        ));
    }
    ReservationScopeKey::try_from(scope_key)
}

impl From<&ReservationOverlapConflictPayload> for RawReservationOverlapConflictPayload {
    fn from(payload: &ReservationOverlapConflictPayload) -> Self {
        Self {
            reservation_id: payload.reservation_id().to_owned(),
            overlapping_reservation_id: payload.overlapping_reservation_id().to_owned(),
            owner_subtask_id: payload.owner_subtask_id().to_owned(),
            overlapping_owner_subtask_id: payload.overlapping_owner_subtask_id().to_owned(),
            scope_class: payload.scope_class(),
            scope_key: payload.scope_key().to_owned(),
            overlapping_scope_class: payload.overlapping_scope_class(),
            overlapping_scope_key: payload.overlapping_scope_key().to_owned(),
        }
    }
}

impl TryFrom<RawReservationOverlapConflictPayload> for ReservationOverlapConflictPayload {
    type Error = String;

    fn try_from(raw: RawReservationOverlapConflictPayload) -> Result<Self, Self::Error> {
        Self::try_from_raw_parts(
            raw.reservation_id,
            raw.overlapping_reservation_id,
            raw.owner_subtask_id,
            raw.overlapping_owner_subtask_id,
            raw.scope_class,
            raw.scope_key,
            raw.overlapping_scope_class,
            raw.overlapping_scope_key,
        )
    }
}

impl Serialize for ReservationOverlapConflictPayload {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawReservationOverlapConflictPayload::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ReservationOverlapConflictPayload {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawReservationOverlapConflictPayload::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OverlapCandidate {
    scope: ReservationScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawOverlapCandidate {
    scope_class: ScopeClass,
    scope_key: String,
    generated_members: Vec<String>,
}

impl OverlapCandidate {
    pub(crate) fn try_new(
        scope_class: ScopeClass,
        scope_key: impl Into<String>,
        generated_members: Vec<String>,
    ) -> Result<Self, String> {
        Ok(Self {
            scope: ReservationScope::from_parts(scope_class, scope_key.into(), generated_members)?,
        })
    }

    pub(crate) fn new(
        scope_class: ScopeClass,
        scope_key: impl Into<String>,
        generated_members: Vec<String>,
    ) -> Self {
        Self::try_new(scope_class, scope_key, generated_members)
            .expect("overlap candidate scope must be internally consistent")
    }

    pub(crate) const fn scope_class(&self) -> ScopeClass {
        self.scope.scope_class()
    }

    pub(crate) fn scope_key(&self) -> &str {
        self.scope.scope_key()
    }

    pub(crate) fn generated_members(&self) -> Vec<String> {
        self.scope.generated_members()
    }

    pub(crate) fn generated_member_strs(&self) -> impl Iterator<Item = &str> {
        self.scope.generated_member_strs()
    }
}

impl Serialize for OverlapCandidate {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawOverlapCandidate::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OverlapCandidate {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawOverlapCandidate::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl From<&OverlapCandidate> for RawOverlapCandidate {
    fn from(candidate: &OverlapCandidate) -> Self {
        Self {
            scope_class: candidate.scope_class(),
            scope_key: candidate.scope_key().to_owned(),
            generated_members: candidate.generated_members(),
        }
    }
}

impl TryFrom<RawOverlapCandidate> for OverlapCandidate {
    type Error = String;

    fn try_from(raw: RawOverlapCandidate) -> Result<Self, Self::Error> {
        Self::try_new(raw.scope_class, raw.scope_key, raw.generated_members)
    }
}

#[cfg(test)]
mod overlap_candidate_tests {
    use super::*;

    #[test]
    fn overlap_candidate_preserves_flat_shape() {
        let candidate = OverlapCandidate::try_new(
            ScopeClass::GeneratedSet,
            "artifact-manifest",
            vec!["src/generated.rs".to_owned()],
        )
        .expect("valid generated-set overlap candidate");

        let value = serde_json::to_value(&candidate).expect("serialize overlap candidate");
        assert_eq!(
            value,
            serde_json::json!({
                "scope_class": "generated_set",
                "scope_key": "artifact-manifest",
                "generated_members": ["src/generated.rs"]
            })
        );

        let round_trip: OverlapCandidate =
            serde_json::from_value(value).expect("flat overlap candidate should deserialize");
        assert_eq!(round_trip, candidate);
        assert_eq!(round_trip.scope_class(), ScopeClass::GeneratedSet);
        assert_eq!(round_trip.scope_key(), "artifact-manifest");
        assert_eq!(
            round_trip.generated_members(),
            &["src/generated.rs".to_owned()]
        );
    }

    #[test]
    fn overlap_candidate_rejects_stale_generated_members_for_path_scopes() {
        let err = OverlapCandidate::try_new(
            ScopeClass::ExactPath,
            "src/lib.rs",
            vec!["src/generated.rs".to_owned()],
        )
        .expect_err("path candidates must not carry generated members");

        assert_eq!(
            err,
            "exact_path reservations must not include generated_members"
        );
    }

    #[test]
    fn overlap_candidate_rejects_generated_set_without_members() {
        let err = OverlapCandidate::try_new(ScopeClass::GeneratedSet, "artifact-manifest", vec![])
            .expect_err("generated-set candidates require members");

        assert_eq!(err, "generated-set reservations require generated_members");
    }
}

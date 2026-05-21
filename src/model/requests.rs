//! Wire request/response DTOs for the public Covey API.
//!
//! These structs preserve JSON-compatible wire shapes while parsing fields with
//! domain invariants into validated newtypes at the API boundary.

use std::collections::HashSet;

use derive_new::new;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{
    AgentInstanceId, AgentPrincipalId, ArtifactDigest, ArtifactKind, ArtifactManifestPath, BaseRev,
    ChangedPathsDigest, ClaimId, CommandTranscriptDigest, ConflictId, ConflictResolutionState,
    CoveyTypeValidationError, FenceSeq, FindingsDigest, IdempotencyKey, LeaseDeadlineMs,
    LeaseDurationMs, MetaTaskId, ModelId, PromptText, ProviderId, ProviderRunId,
    ProviderRunIdIssuer, QueueId, RepoopsPath, ReservationId, ReservationScope, ReviewId,
    ReviewVerdict, RuntimeContainerId, RuntimeProcessId, ScopeClass, SessionRole, SessionToken,
    SettlementTarget, SubtaskId, SubtaskPriority, SubtaskTitle, TimestampMs, VerifierId,
};

fn parse_idempotency_key(
    value: impl Into<String>,
) -> Result<IdempotencyKey, CoveyTypeValidationError> {
    IdempotencyKey::parse(value.into())
}

fn parse_idempotency_key_string(value: impl Into<String>) -> Result<IdempotencyKey, String> {
    parse_idempotency_key(value).map_err(|err| err.to_string())
}

/// Request to register a session with immutable identity metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterSessionReq {
    pub agent_principal_id: AgentPrincipalId,
    pub agent_instance_id: AgentInstanceId,
    pub role: SessionRole,
    pub idempotency_key: IdempotencyKey,
}

impl RegisterSessionReq {
    /// Builds a registration request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when either agent identity is not token-shaped.
    pub fn try_from_raw_parts(
        agent_principal_id: impl Into<String>,
        agent_instance_id: impl Into<String>,
        role: SessionRole,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            agent_principal_id: AgentPrincipalId::parse(agent_principal_id.into())?,
            agent_instance_id: AgentInstanceId::parse(agent_instance_id.into())?,
            role,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }

    /// Returns the validated stable agent principal identity.
    #[must_use]
    pub fn agent_principal_id(&self) -> &str {
        self.agent_principal_id.as_str()
    }

    /// Returns the validated concrete agent process identity.
    #[must_use]
    pub fn agent_instance_id(&self) -> &str {
        self.agent_instance_id.as_str()
    }
}

/// Session identity returned after successful registration.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionHandle {
    pub session_token: SessionToken,
    pub agent_principal_id: AgentPrincipalId,
    pub agent_instance_id: AgentInstanceId,
    pub role: SessionRole,
}

impl SessionHandle {
    /// Builds a session handle from unvalidated scalar values.
    ///
    /// # Panics
    ///
    /// Panics if any identity field is not token-shaped. Use
    /// [`SessionHandle::try_from_raw_parts`] when parsing untrusted input.
    #[must_use]
    pub fn new(
        session_token: impl Into<String>,
        agent_principal_id: impl Into<String>,
        agent_instance_id: impl Into<String>,
        role: SessionRole,
    ) -> Self {
        Self::try_from_raw_parts(session_token, agent_principal_id, agent_instance_id, role)
            .expect("session handle fields must be valid Covey identities")
    }

    /// Parses a session handle from raw scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when any identity field is not token-shaped.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        agent_principal_id: impl Into<String>,
        agent_instance_id: impl Into<String>,
        role: SessionRole,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            agent_principal_id: AgentPrincipalId::parse(agent_principal_id.into())?,
            agent_instance_id: AgentInstanceId::parse(agent_instance_id.into())?,
            role,
        })
    }

    /// Returns the validated session token.
    #[must_use]
    pub fn session_token(&self) -> &str {
        self.session_token.as_str()
    }

    /// Returns the validated stable agent principal identity.
    #[must_use]
    pub fn agent_principal_id(&self) -> &str {
        self.agent_principal_id.as_str()
    }

    /// Returns the validated concrete agent process identity.
    #[must_use]
    pub fn agent_instance_id(&self) -> &str {
        self.agent_instance_id.as_str()
    }
}

/// Request to create a meta-task from operator intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitMetaTaskReq {
    pub session_token: SessionToken,
    pub prompt_text: PromptText,
    pub idempotency_key: IdempotencyKey,
}

impl SubmitMetaTaskReq {
    /// Builds a submit-meta-task request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token or prompt text is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        prompt_text: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            prompt_text: PromptText::parse(prompt_text.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to cancel a meta-task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelMetaTaskReq {
    pub session_token: SessionToken,
    pub meta_task_id: MetaTaskId,
    pub idempotency_key: IdempotencyKey,
}

impl CancelMetaTaskReq {
    /// Builds a cancel-meta-task request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token or meta-task id is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        meta_task_id: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            meta_task_id: MetaTaskId::parse(meta_task_id.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to heartbeat an active session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatReq {
    pub session_token: SessionToken,
    pub idempotency_key: IdempotencyKey,
}

impl HeartbeatReq {
    /// Builds a heartbeat request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to exit an active session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitSessionReq {
    pub session_token: SessionToken,
    pub idempotency_key: IdempotencyKey,
}

impl ExitSessionReq {
    /// Builds a session-exit request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to bind runtime identity evidence to a Covey session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordRuntimeAttestationReq {
    pub session_token: SessionToken,
    pub provider: ProviderId,
    pub model: ModelId,
    provider_run_id: ProviderRunId,
    provider_run_id_issuer: ProviderRunIdIssuer,
    runtime_identity: RuntimeAttestationIdentityReq,
    pub command_transcript_digest: CommandTranscriptDigest,
    started_at: TimestampMs,
    ended_at: TimestampMs,
    pub idempotency_key: IdempotencyKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeAttestationIdentityReq {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawRecordRuntimeAttestationReq {
    session_token: String,
    provider: String,
    model: String,
    provider_run_id: String,
    provider_run_id_issuer: String,
    process_id: Option<String>,
    container_id: Option<String>,
    command_transcript_digest: String,
    started_at: i64,
    ended_at: i64,
    idempotency_key: String,
}

impl RecordRuntimeAttestationReq {
    /// Builds a runtime attestation request from the legacy flat identity shape.
    ///
    /// # Errors
    ///
    /// Returns an error unless at least one non-empty runtime identity field is present.
    #[allow(clippy::too_many_arguments)]
    pub fn try_from_parts(
        session_token: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
        provider_run_id: impl Into<String>,
        provider_run_id_issuer: impl Into<String>,
        process_id: Option<String>,
        container_id: Option<String>,
        command_transcript_digest: impl Into<String>,
        started_at: i64,
        ended_at: i64,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, String> {
        let runtime_identity =
            RuntimeAttestationIdentityReq::try_from_parts(process_id, container_id)?;
        let started_at = TimestampMs::parse(started_at).map_err(|err| err.to_string())?;
        let ended_at = TimestampMs::parse(ended_at).map_err(|err| err.to_string())?;
        if ended_at < started_at {
            return Err("ended_at must be greater than or equal to started_at".to_owned());
        }
        let provider_run_id = ProviderRunId::parse(provider_run_id.into())
            .map_err(runtime_text_error("provider_run_id"))?;
        let provider_run_id_issuer = ProviderRunIdIssuer::parse(provider_run_id_issuer.into())
            .map_err(runtime_text_error("provider_run_id_issuer"))?;
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())
                .map_err(|err| err.to_string())?,
            provider: ProviderId::parse(provider.into()).map_err(|err| err.to_string())?,
            model: ModelId::parse(model.into()).map_err(|err| err.to_string())?,
            provider_run_id,
            provider_run_id_issuer,
            runtime_identity,
            command_transcript_digest: CommandTranscriptDigest::parse(
                command_transcript_digest.into(),
            )
            .map_err(|err| err.to_string())?,
            started_at,
            ended_at,
            idempotency_key: parse_idempotency_key_string(idempotency_key)?,
        })
    }

    /// Returns the process identity, when present.
    #[must_use]
    pub fn process_id(&self) -> Option<&str> {
        self.runtime_identity.process_id()
    }

    /// Returns the container identity, when present.
    #[must_use]
    pub fn container_id(&self) -> Option<&str> {
        self.runtime_identity.container_id()
    }

    /// Returns the provider-specific runtime run identifier.
    #[must_use]
    pub fn provider_run_id(&self) -> &str {
        self.provider_run_id.as_str()
    }

    /// Returns the authority that issued the provider run identifier.
    #[must_use]
    pub fn provider_run_id_issuer(&self) -> &str {
        self.provider_run_id_issuer.as_str()
    }

    /// Returns the non-negative runtime start timestamp.
    #[must_use]
    pub const fn started_at(&self) -> TimestampMs {
        self.started_at
    }

    /// Returns the non-negative runtime end timestamp.
    #[must_use]
    pub const fn ended_at(&self) -> TimestampMs {
        self.ended_at
    }
}

impl RuntimeAttestationIdentityReq {
    fn try_from_parts(
        process_id: Option<String>,
        container_id: Option<String>,
    ) -> Result<Self, String> {
        Self::try_from_runtime_parts(
            parse_optional_runtime_process_id(process_id)?,
            parse_optional_runtime_container_id(container_id)?,
        )
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

fn parse_optional_runtime_process_id(
    value: Option<String>,
) -> Result<Option<RuntimeProcessId>, String> {
    value
        .map(|value| RuntimeProcessId::parse(value).map_err(runtime_text_error("process_id")))
        .transpose()
}

fn parse_optional_runtime_container_id(
    value: Option<String>,
) -> Result<Option<RuntimeContainerId>, String> {
    value
        .map(|value| RuntimeContainerId::parse(value).map_err(runtime_text_error("container_id")))
        .transpose()
}

fn runtime_text_error(field: &'static str) -> impl Fn(CoveyTypeValidationError) -> String + Copy {
    move |err| format!("{field} {}", err.reason())
}

impl RuntimeAttestationIdentityReq {
    fn try_from_runtime_parts(
        process_id: Option<RuntimeProcessId>,
        container_id: Option<RuntimeContainerId>,
    ) -> Result<Self, String> {
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
}

impl From<&RecordRuntimeAttestationReq> for RawRecordRuntimeAttestationReq {
    fn from(req: &RecordRuntimeAttestationReq) -> Self {
        Self {
            session_token: req.session_token.to_string(),
            provider: req.provider.to_string(),
            model: req.model.to_string(),
            provider_run_id: req.provider_run_id().to_owned(),
            provider_run_id_issuer: req.provider_run_id_issuer().to_owned(),
            process_id: req.process_id().map(str::to_owned),
            container_id: req.container_id().map(str::to_owned),
            command_transcript_digest: req.command_transcript_digest.to_string(),
            started_at: req.started_at().get(),
            ended_at: req.ended_at().get(),
            idempotency_key: req.idempotency_key.to_string(),
        }
    }
}

impl Serialize for RecordRuntimeAttestationReq {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawRecordRuntimeAttestationReq::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RecordRuntimeAttestationReq {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawRecordRuntimeAttestationReq::deserialize(deserializer)?;
        Self::try_from_parts(
            raw.session_token,
            raw.provider,
            raw.model,
            raw.provider_run_id,
            raw.provider_run_id_issuer,
            raw.process_id,
            raw.container_id,
            raw.command_transcript_digest,
            raw.started_at,
            raw.ended_at,
            raw.idempotency_key,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Request to create a work subtask from orchestrator-owned input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSubtaskRequest {
    pub session_token: SessionToken,
    pub meta_task_id: MetaTaskId,
    pub subtask_id: Option<SubtaskId>,
    pub title: SubtaskTitle,
    pub priority: SubtaskPriority,
    pub idempotency_key: IdempotencyKey,
}

impl CreateSubtaskRequest {
    /// Builds a subtask creation request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token, meta-task id, optional subtask
    /// id, title, or priority is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        meta_task_id: impl Into<String>,
        subtask_id: Option<String>,
        title: impl Into<String>,
        priority: i64,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            meta_task_id: MetaTaskId::parse(meta_task_id.into())?,
            subtask_id: subtask_id.map(SubtaskId::parse).transpose()?,
            title: SubtaskTitle::parse(title.into())?,
            priority: SubtaskPriority::parse(priority)?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to claim the next available subtask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimNextReq {
    pub session_token: SessionToken,
    pub lease_duration_ms: LeaseDurationMs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta_task_id: Option<MetaTaskId>,
    pub idempotency_key: IdempotencyKey,
}

impl ClaimNextReq {
    /// Builds a claim-next request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token is invalid or lease duration is
    /// not positive.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        lease_duration_ms: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            lease_duration_ms: LeaseDurationMs::parse(lease_duration_ms.into())?,
            meta_task_id: None,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }

    /// Builds a claim-next request constrained to one meta-task.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token, meta-task id, or lease
    /// duration is invalid.
    pub fn try_from_raw_parts_scoped(
        session_token: impl Into<String>,
        lease_duration_ms: impl Into<i64>,
        meta_task_id: Option<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            lease_duration_ms: LeaseDurationMs::parse(lease_duration_ms.into())?,
            meta_task_id: meta_task_id.map(MetaTaskId::parse).transpose()?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to claim a specific subtask by ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimSubtaskReq {
    pub session_token: SessionToken,
    pub subtask_id: SubtaskId,
    pub lease_duration_ms: LeaseDurationMs,
    pub idempotency_key: IdempotencyKey,
}

impl ClaimSubtaskReq {
    /// Builds a claim-subtask request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token, subtask id, or lease duration is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        subtask_id: impl Into<String>,
        lease_duration_ms: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            subtask_id: SubtaskId::parse(subtask_id.into())?,
            lease_duration_ms: LeaseDurationMs::parse(lease_duration_ms.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Claim token and lease metadata returned after a successful claim.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct ClaimResult {
    pub claim_id: ClaimId,
    pub subtask_id: SubtaskId,
    pub fence_seq: FenceSeq,
    pub lease_deadline: LeaseDeadlineMs,
}

/// Request to start work on a claimed subtask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartSubtaskReq {
    pub session_token: SessionToken,
    pub claim_id: ClaimId,
    pub fence_seq: FenceSeq,
    pub idempotency_key: IdempotencyKey,
}

impl StartSubtaskReq {
    /// Builds a start-subtask request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token, claim id, or fence sequence is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        claim_id: impl Into<String>,
        fence_seq: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            claim_id: ClaimId::parse(claim_id.into())?,
            fence_seq: FenceSeq::parse(fence_seq.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to abandon a claimed subtask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbandonSubtaskReq {
    pub session_token: SessionToken,
    pub claim_id: ClaimId,
    pub fence_seq: FenceSeq,
    pub idempotency_key: IdempotencyKey,
}

impl AbandonSubtaskReq {
    /// Builds an abandon-subtask request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token, claim id, or fence sequence is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        claim_id: impl Into<String>,
        fence_seq: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            claim_id: ClaimId::parse(claim_id.into())?,
            fence_seq: FenceSeq::parse(fence_seq.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to release a held claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseClaimReq {
    pub session_token: SessionToken,
    pub claim_id: ClaimId,
    pub fence_seq: FenceSeq,
    pub idempotency_key: IdempotencyKey,
}

impl ReleaseClaimReq {
    /// Builds a release-claim request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token, claim id, or fence sequence is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        claim_id: impl Into<String>,
        fence_seq: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            claim_id: ClaimId::parse(claim_id.into())?,
            fence_seq: FenceSeq::parse(fence_seq.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to renew an active claim lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenewClaimReq {
    pub session_token: SessionToken,
    pub claim_id: ClaimId,
    pub fence_seq: FenceSeq,
    pub extend_by_ms: LeaseDurationMs,
    pub idempotency_key: IdempotencyKey,
}

impl RenewClaimReq {
    /// Builds a renew-claim request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token, claim id, fence sequence, or
    /// lease extension is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        claim_id: impl Into<String>,
        fence_seq: impl Into<i64>,
        extend_by_ms: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            claim_id: ClaimId::parse(claim_id.into())?,
            fence_seq: FenceSeq::parse(fence_seq.into())?,
            extend_by_ms: LeaseDurationMs::parse(extend_by_ms.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to publish an immutable artifact for a work subtask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishArtifactReq {
    pub session_token: SessionToken,
    pub claim_id: ClaimId,
    pub fence_seq: FenceSeq,
    pub artifact_digest: ArtifactDigest,
    pub artifact_kind: ArtifactKind,
    pub base_rev: BaseRev,
    pub manifest_path: ArtifactManifestPath,
    pub changed_paths_digest: ChangedPathsDigest,
    pub idempotency_key: IdempotencyKey,
}

impl PublishArtifactReq {
    /// Builds an artifact publication request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token, claim id, fence sequence,
    /// artifact digest, base revision, manifest path, or changed paths digest
    /// is invalid.
    #[allow(clippy::too_many_arguments)]
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        claim_id: impl Into<String>,
        fence_seq: impl Into<i64>,
        artifact_digest: String,
        artifact_kind: ArtifactKind,
        base_rev: String,
        manifest_path: String,
        changed_paths_digest: String,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            claim_id: ClaimId::parse(claim_id.into())?,
            fence_seq: FenceSeq::parse(fence_seq.into())?,
            artifact_digest: ArtifactDigest::parse(artifact_digest)?,
            artifact_kind,
            base_rev: BaseRev::parse(base_rev)?,
            manifest_path: ArtifactManifestPath::parse(manifest_path)?,
            changed_paths_digest: ChangedPathsDigest::parse(changed_paths_digest)?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to create a review subtask for an exact artifact digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestReviewReq {
    pub session_token: SessionToken,
    pub subtask_id: SubtaskId,
    pub artifact_digest: ArtifactDigest,
    pub review_subtask_id: Option<SubtaskId>,
    pub priority: SubtaskPriority,
    pub idempotency_key: IdempotencyKey,
}

impl RequestReviewReq {
    /// Builds a review request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token, work subtask id, artifact
    /// digest, explicit review subtask id, or priority is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        subtask_id: impl Into<String>,
        artifact_digest: impl Into<String>,
        review_subtask_id: Option<String>,
        priority: i64,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, String> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())
                .map_err(|err| err.to_string())?,
            subtask_id: SubtaskId::parse(subtask_id.into()).map_err(|err| err.to_string())?,
            artifact_digest: ArtifactDigest::parse(artifact_digest.into())
                .map_err(|err| err.to_string())?,
            review_subtask_id: review_subtask_id
                .map(SubtaskId::parse)
                .transpose()
                .map_err(|err| err.to_string())?,
            priority: SubtaskPriority::parse(priority).map_err(|err| err.to_string())?,
            idempotency_key: parse_idempotency_key_string(idempotency_key)?,
        })
    }
}

/// Request to decide a review while holding the matching review-subtask claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecideReviewReq {
    pub session_token: SessionToken,
    pub review_id: ReviewId,
    pub claim_id: ClaimId,
    pub fence_seq: FenceSeq,
    pub verdict: ReviewVerdict,
    pub findings_digest: FindingsDigest,
    pub idempotency_key: IdempotencyKey,
}

impl DecideReviewReq {
    /// Builds a review decision request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token, review id, claim id, fence
    /// sequence, or findings digest is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        review_id: String,
        claim_id: impl Into<String>,
        fence_seq: impl Into<i64>,
        verdict: ReviewVerdict,
        findings_digest: String,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            review_id: ReviewId::parse(review_id)?,
            claim_id: ClaimId::parse(claim_id.into())?,
            fence_seq: FenceSeq::parse(fence_seq.into())?,
            verdict,
            findings_digest: FindingsDigest::parse(findings_digest)?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to enqueue an approved artifact for apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnqueueForApplyReq {
    pub session_token: SessionToken,
    pub artifact_digest: ArtifactDigest,
    pub subtask_id: SubtaskId,
    pub settlement_target: SettlementTarget,
    pub idempotency_key: IdempotencyKey,
}

impl EnqueueForApplyReq {
    /// Builds an apply-queue enqueue request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token, artifact digest, or subtask id
    /// is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        artifact_digest: String,
        subtask_id: String,
        settlement_target: SettlementTarget,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            artifact_digest: ArtifactDigest::parse(artifact_digest)?,
            subtask_id: SubtaskId::parse(subtask_id)?,
            settlement_target,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to atomically claim the next ready-queue item for apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimReadyQueueReq {
    pub session_token: SessionToken,
    pub lease_duration_ms: LeaseDurationMs,
    pub idempotency_key: IdempotencyKey,
}

impl ClaimReadyQueueReq {
    /// Builds a ready-queue claim request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token is invalid or the lease duration
    /// is not positive.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        lease_duration_ms: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            lease_duration_ms: LeaseDurationMs::parse(lease_duration_ms.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Claimed ready-queue item with apply fence and lease metadata.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct ReadyQueueClaim {
    pub queue_id: QueueId,
    pub artifact_digest: ArtifactDigest,
    pub subtask_id: SubtaskId,
    pub settlement_target: SettlementTarget,
    pub claim_fence_seq: FenceSeq,
    pub lease_deadline: LeaseDeadlineMs,
}

/// Request to mark a ready-queue item in flight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkInFlightReq {
    pub session_token: SessionToken,
    pub queue_id: QueueId,
    pub lease_duration_ms: LeaseDurationMs,
    pub idempotency_key: IdempotencyKey,
}

impl MarkInFlightReq {
    /// Builds a mark-in-flight request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token, queue id, or lease duration is
    /// invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        queue_id: impl Into<String>,
        lease_duration_ms: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            queue_id: QueueId::parse(queue_id.into())?,
            lease_duration_ms: LeaseDurationMs::parse(lease_duration_ms.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to record an accepted verifier verdict for one apply attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordApplyVerificationReq {
    pub session_token: SessionToken,
    pub queue_id: QueueId,
    pub artifact_digest: ArtifactDigest,
    pub review_id: ReviewId,
    pub findings_digest: FindingsDigest,
    pub claim_fence_seq: FenceSeq,
    pub verifier: VerifierId,
    pub verdict_digest: ArtifactDigest,
    pub seal_digest: ArtifactDigest,
    pub idempotency_key: IdempotencyKey,
}

impl RecordApplyVerificationReq {
    /// Builds an apply-verification request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token or any queue, artifact, review,
    /// findings, fence, verifier, or digest field is invalid.
    #[allow(clippy::too_many_arguments)]
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        queue_id: impl Into<String>,
        artifact_digest: impl Into<String>,
        review_id: impl Into<String>,
        findings_digest: impl Into<String>,
        claim_fence_seq: impl Into<i64>,
        verifier: impl Into<String>,
        verdict_digest: impl Into<String>,
        seal_digest: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            queue_id: QueueId::parse(queue_id.into())?,
            artifact_digest: ArtifactDigest::parse(artifact_digest.into())?,
            review_id: ReviewId::parse(review_id.into())?,
            findings_digest: FindingsDigest::parse(findings_digest.into())?,
            claim_fence_seq: FenceSeq::parse(claim_fence_seq.into())?,
            verifier: VerifierId::parse(verifier.into())?,
            verdict_digest: ArtifactDigest::parse(verdict_digest.into())?,
            seal_digest: ArtifactDigest::parse(seal_digest.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to verify that a landing authorization is still backed by live Covey state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyLandingAuthorizationReq {
    pub session_token: SessionToken,
    pub queue_id: QueueId,
    pub artifact_digest: ArtifactDigest,
    pub review_id: ReviewId,
    pub findings_digest: FindingsDigest,
    pub claim_fence_seq: FenceSeq,
    pub verifier: VerifierId,
    pub verdict_digest: ArtifactDigest,
    pub seal_digest: ArtifactDigest,
}

impl VerifyLandingAuthorizationReq {
    /// Builds a landing-authorization request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token or any queue, artifact, review,
    /// findings, fence, verifier, or digest field is invalid.
    #[allow(clippy::too_many_arguments)]
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        queue_id: impl Into<String>,
        artifact_digest: impl Into<String>,
        review_id: impl Into<String>,
        findings_digest: impl Into<String>,
        claim_fence_seq: impl Into<i64>,
        verifier: impl Into<String>,
        verdict_digest: impl Into<String>,
        seal_digest: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            queue_id: QueueId::parse(queue_id.into())?,
            artifact_digest: ArtifactDigest::parse(artifact_digest.into())?,
            review_id: ReviewId::parse(review_id.into())?,
            findings_digest: FindingsDigest::parse(findings_digest.into())?,
            claim_fence_seq: FenceSeq::parse(claim_fence_seq.into())?,
            verifier: VerifierId::parse(verifier.into())?,
            verdict_digest: ArtifactDigest::parse(verdict_digest.into())?,
            seal_digest: ArtifactDigest::parse(seal_digest.into())?,
        })
    }
}

/// Request to mark an in-flight queue item applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkAppliedReq {
    pub session_token: SessionToken,
    pub queue_id: QueueId,
    pub claim_fence_seq: FenceSeq,
    pub idempotency_key: IdempotencyKey,
}

impl MarkAppliedReq {
    /// Builds a mark-applied request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token, queue id, or fence sequence is
    /// invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        queue_id: impl Into<String>,
        claim_fence_seq: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            queue_id: QueueId::parse(queue_id.into())?,
            claim_fence_seq: FenceSeq::parse(claim_fence_seq.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to supersede a queued or in-flight queue item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupersedeQueueItemReq {
    pub session_token: SessionToken,
    pub queue_id: QueueId,
    pub idempotency_key: IdempotencyKey,
}

impl SupersedeQueueItemReq {
    /// Builds a supersede request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token or queue id is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        queue_id: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            queue_id: QueueId::parse(queue_id.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to create an advisory reservation for a subtask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestReservationReq {
    pub session_token: SessionToken,
    pub owner_subtask_id: SubtaskId,
    scope: ReservationScope,
    pub lease_duration_ms: LeaseDurationMs,
    pub idempotency_key: IdempotencyKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawRequestReservationReq {
    session_token: String,
    owner_subtask_id: SubtaskId,
    scope_class: ScopeClass,
    scope_key: String,
    generated_members: Vec<String>,
    lease_duration_ms: LeaseDurationMs,
    idempotency_key: String,
}

impl RequestReservationReq {
    /// Builds a reservation request from the flat CLI/API shape.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token or scope class/key/member shape
    /// is invalid.
    #[allow(clippy::too_many_arguments)]
    pub fn try_from_parts(
        session_token: impl Into<String>,
        owner_subtask_id: SubtaskId,
        scope_class: ScopeClass,
        scope_key: impl Into<String>,
        generated_members: Vec<String>,
        lease_duration_ms: LeaseDurationMs,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, String> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())
                .map_err(|err| err.to_string())?,
            owner_subtask_id,
            scope: ReservationScope::from_parts(scope_class, scope_key.into(), generated_members)?,
            lease_duration_ms,
            idempotency_key: parse_idempotency_key_string(idempotency_key)?,
        })
    }

    /// Builds a reservation request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token, owner id, lease duration, or
    /// scope shape is invalid.
    #[allow(clippy::too_many_arguments)]
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        owner_subtask_id: impl Into<String>,
        scope_class: ScopeClass,
        scope_key: impl Into<String>,
        generated_members: Vec<String>,
        lease_duration_ms: i64,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, String> {
        Self::try_from_parts(
            session_token,
            SubtaskId::parse(owner_subtask_id.into()).map_err(|err| err.to_string())?,
            scope_class,
            scope_key,
            generated_members,
            LeaseDurationMs::parse(lease_duration_ms).map_err(|err| err.to_string())?,
            idempotency_key,
        )
    }

    /// Returns the reservation scope.
    #[must_use]
    pub const fn scope(&self) -> &ReservationScope {
        &self.scope
    }

    /// Returns the reservation scope class.
    #[must_use]
    pub const fn scope_class(&self) -> ScopeClass {
        self.scope.scope_class()
    }

    /// Returns the reservation scope key.
    #[must_use]
    pub fn scope_key(&self) -> &str {
        self.scope.scope_key()
    }

    /// Returns generated members for generated-set reservations.
    #[must_use]
    pub fn generated_members(&self) -> Vec<String> {
        self.scope.generated_members()
    }
}

impl From<&RequestReservationReq> for RawRequestReservationReq {
    fn from(req: &RequestReservationReq) -> Self {
        Self {
            session_token: req.session_token.to_string(),
            owner_subtask_id: req.owner_subtask_id.clone(),
            scope_class: req.scope_class(),
            scope_key: req.scope_key().to_owned(),
            generated_members: req.generated_members(),
            lease_duration_ms: req.lease_duration_ms,
            idempotency_key: req.idempotency_key.to_string(),
        }
    }
}

impl TryFrom<RawRequestReservationReq> for RequestReservationReq {
    type Error = String;

    fn try_from(raw: RawRequestReservationReq) -> Result<Self, Self::Error> {
        Self::try_from_parts(
            raw.session_token,
            raw.owner_subtask_id,
            raw.scope_class,
            raw.scope_key,
            raw.generated_members,
            raw.lease_duration_ms,
            raw.idempotency_key,
        )
    }
}

impl Serialize for RequestReservationReq {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawRequestReservationReq::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RequestReservationReq {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawRequestReservationReq::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// Request to release an existing reservation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseReservationReq {
    pub session_token: SessionToken,
    pub reservation_id: ReservationId,
    pub idempotency_key: IdempotencyKey,
}

impl ReleaseReservationReq {
    /// Builds a release request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token or reservation id is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        reservation_id: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            reservation_id: ReservationId::parse(reservation_id.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Request to renew an active reservation lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenewReservationReq {
    pub session_token: SessionToken,
    pub reservation_id: ReservationId,
    pub extend_by_ms: LeaseDurationMs,
    pub idempotency_key: IdempotencyKey,
}

impl RenewReservationReq {
    /// Builds a renewal request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token or reservation id is invalid, or
    /// the extension is not positive.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        reservation_id: impl Into<String>,
        extend_by_ms: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            reservation_id: ReservationId::parse(reservation_id.into())?,
            extend_by_ms: LeaseDurationMs::parse(extend_by_ms.into())?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }
}

/// Query for overlapping active reservations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlapQueryReq {
    scope: ReservationScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawOverlapQueryReq {
    scope_class: ScopeClass,
    scope_key: String,
    generated_members: Vec<String>,
}

impl OverlapQueryReq {
    /// Builds an overlap query from the flat CLI/API shape.
    ///
    /// # Errors
    ///
    /// Returns an error when the scope class/key/member shape is invalid.
    pub fn try_from_parts(
        scope_class: ScopeClass,
        scope_key: impl Into<String>,
        generated_members: Vec<String>,
    ) -> Result<Self, String> {
        Ok(Self {
            scope: ReservationScope::from_parts(scope_class, scope_key.into(), generated_members)?,
        })
    }

    /// Returns the reservation scope class.
    #[must_use]
    pub const fn scope_class(&self) -> ScopeClass {
        self.scope.scope_class()
    }

    /// Returns the reservation scope key.
    #[must_use]
    pub fn scope_key(&self) -> &str {
        self.scope.scope_key()
    }

    /// Returns generated members for generated-set overlap queries.
    #[must_use]
    pub fn generated_members(&self) -> Vec<String> {
        self.scope.generated_members()
    }
}

impl From<&OverlapQueryReq> for RawOverlapQueryReq {
    fn from(req: &OverlapQueryReq) -> Self {
        Self {
            scope_class: req.scope_class(),
            scope_key: req.scope_key().to_owned(),
            generated_members: req.generated_members(),
        }
    }
}

impl TryFrom<RawOverlapQueryReq> for OverlapQueryReq {
    type Error = String;

    fn try_from(raw: RawOverlapQueryReq) -> Result<Self, Self::Error> {
        Self::try_from_parts(raw.scope_class, raw.scope_key, raw.generated_members)
    }
}

impl Serialize for OverlapQueryReq {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawOverlapQueryReq::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OverlapQueryReq {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawOverlapQueryReq::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// Query for the Covey lifecycle facts needed by mutAI repoops preflight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoopsAuthoritySnapshotReq {
    pub session_token: SessionToken,
    pub claim_id: ClaimId,
    pub fence_seq: FenceSeq,
    paths: NonEmptyRepoopsPaths,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NonEmptyRepoopsPaths(Vec<RepoopsPath>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawRepoopsAuthoritySnapshotReq {
    session_token: String,
    claim_id: String,
    fence_seq: i64,
    paths: Vec<String>,
}

impl RepoopsAuthoritySnapshotReq {
    /// Builds a repoops authority snapshot request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token, claim id, or fence sequence is
    /// invalid, when no path is supplied, when a path is duplicated after
    /// normalization, or when any path is empty or traverses outside the
    /// repository.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        claim_id: impl Into<String>,
        fence_seq: impl Into<i64>,
        paths: Vec<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            claim_id: ClaimId::parse(claim_id.into())?,
            fence_seq: FenceSeq::parse(fence_seq.into())?,
            paths: NonEmptyRepoopsPaths::try_from_raw(paths)?,
        })
    }

    /// Returns the non-empty, normalized mutation paths for the snapshot request.
    #[must_use]
    pub fn paths(&self) -> &[RepoopsPath] {
        self.paths.as_slice()
    }
}

impl NonEmptyRepoopsPaths {
    fn try_from_raw(paths: Vec<String>) -> Result<Self, CoveyTypeValidationError> {
        if paths.is_empty() {
            return Err(CoveyTypeValidationError::new(
                "paths",
                "must include at least one path",
            ));
        }
        let paths = paths
            .into_iter()
            .map(RepoopsPath::parse)
            .collect::<Result<Vec<_>, _>>()?;
        Self::try_from_paths(paths)
    }

    fn try_from_paths(paths: Vec<RepoopsPath>) -> Result<Self, CoveyTypeValidationError> {
        if paths.is_empty() {
            return Err(CoveyTypeValidationError::new(
                "paths",
                "must include at least one path",
            ));
        }
        let mut seen = HashSet::with_capacity(paths.len());
        for path in &paths {
            if !seen.insert(path.as_str().to_owned()) {
                return Err(CoveyTypeValidationError::new(
                    "paths",
                    "must not contain duplicate paths",
                ));
            }
        }
        Ok(Self(paths))
    }

    fn as_slice(&self) -> &[RepoopsPath] {
        &self.0
    }

    fn as_strings(&self) -> Vec<String> {
        self.0.iter().map(ToString::to_string).collect()
    }
}

impl Serialize for RepoopsAuthoritySnapshotReq {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawRepoopsAuthoritySnapshotReq {
            session_token: self.session_token.to_string(),
            claim_id: self.claim_id.to_string(),
            fence_seq: self.fence_seq.get(),
            paths: self.paths.as_strings(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RepoopsAuthoritySnapshotReq {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawRepoopsAuthoritySnapshotReq::deserialize(deserializer)?;
        Self::try_from_raw_parts(raw.session_token, raw.claim_id, raw.fence_seq, raw.paths)
            .map_err(serde::de::Error::custom)
    }
}

/// Request to update the resolution state of a surfaced conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveConflictReq {
    session_token: SessionToken,
    conflict_id: ConflictId,
    resolution: ConflictResolutionUpdate,
    idempotency_key: IdempotencyKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConflictResolutionUpdate {
    Acknowledged,
    Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawResolveConflictReq {
    session_token: SessionToken,
    conflict_id: ConflictId,
    resolution_state: ConflictResolutionState,
    idempotency_key: IdempotencyKey,
}

impl ResolveConflictReq {
    /// Builds a typed conflict resolution request from raw wire or CLI parts.
    ///
    /// # Errors
    ///
    /// Returns an error when the session token or conflict id is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        conflict_id: impl Into<String>,
        resolution_state: ConflictResolutionState,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            conflict_id: ConflictId::parse(conflict_id.into())?,
            resolution: ConflictResolutionUpdate::try_from(resolution_state)?,
            idempotency_key: parse_idempotency_key(idempotency_key)?,
        })
    }

    /// Returns the session making the resolution update.
    #[must_use]
    pub const fn session_token(&self) -> &SessionToken {
        &self.session_token
    }

    /// Returns the target conflict id.
    #[must_use]
    pub const fn conflict_id(&self) -> &ConflictId {
        &self.conflict_id
    }

    /// Returns the non-open resolution state to apply.
    #[must_use]
    pub const fn resolution_state(&self) -> ConflictResolutionState {
        self.resolution.as_state()
    }

    /// Returns the request idempotency key.
    #[must_use]
    pub const fn idempotency_key(&self) -> &IdempotencyKey {
        &self.idempotency_key
    }
}

impl ConflictResolutionUpdate {
    const fn as_state(self) -> ConflictResolutionState {
        match self {
            Self::Acknowledged => ConflictResolutionState::Acknowledged,
            Self::Resolved => ConflictResolutionState::Resolved,
        }
    }
}

impl TryFrom<ConflictResolutionState> for ConflictResolutionUpdate {
    type Error = CoveyTypeValidationError;

    fn try_from(state: ConflictResolutionState) -> Result<Self, Self::Error> {
        match state {
            ConflictResolutionState::Acknowledged => Ok(Self::Acknowledged),
            ConflictResolutionState::Resolved => Ok(Self::Resolved),
            ConflictResolutionState::Open => Err(CoveyTypeValidationError::new(
                "resolution_state",
                "resolve conflict requests must acknowledge or resolve conflicts",
            )),
        }
    }
}

impl Serialize for ResolveConflictReq {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        RawResolveConflictReq::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ResolveConflictReq {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RawResolveConflictReq::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl From<&ResolveConflictReq> for RawResolveConflictReq {
    fn from(req: &ResolveConflictReq) -> Self {
        Self {
            session_token: req.session_token.clone(),
            conflict_id: req.conflict_id.clone(),
            resolution_state: req.resolution_state(),
            idempotency_key: req.idempotency_key.clone(),
        }
    }
}

impl TryFrom<RawResolveConflictReq> for ResolveConflictReq {
    type Error = CoveyTypeValidationError;

    fn try_from(raw: RawResolveConflictReq) -> Result<Self, Self::Error> {
        Ok(Self {
            session_token: raw.session_token,
            conflict_id: raw.conflict_id,
            resolution: ConflictResolutionUpdate::try_from(raw.resolution_state)?,
            idempotency_key: raw.idempotency_key,
        })
    }
}

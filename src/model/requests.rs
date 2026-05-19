//! Wire request/response DTOs for the public Covey API.
//!
//! These structs preserve JSON-compatible wire shapes while parsing fields with
//! domain invariants into validated newtypes at the API boundary.

use derive_new::new;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{
    ArtifactDigest, ArtifactKind, BaseRev, ChangedPathsDigest, ClaimId, ConflictResolutionState,
    CoveyTypeValidationError, FenceSeq, FindingsDigest, LeaseDeadlineMs, LeaseDurationMs,
    MetaTaskId, QueueId, ReservationId, ReservationScope, ReviewId, ReviewVerdict, ScopeClass,
    SessionRole, SettlementTarget, SubtaskId,
};

/// Request to register a session with immutable identity metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterSessionReq {
    pub agent_principal_id: String,
    pub agent_instance_id: String,
    pub role: SessionRole,
    pub idempotency_key: String,
}

/// Session identity returned after successful registration.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct SessionHandle {
    pub session_token: String,
    pub agent_principal_id: String,
    pub agent_instance_id: String,
    pub role: SessionRole,
}

/// Request to create a meta-task from operator intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitMetaTaskReq {
    pub session_token: String,
    pub prompt_text: String,
    pub idempotency_key: String,
}

/// Request to cancel a meta-task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelMetaTaskReq {
    pub session_token: String,
    pub meta_task_id: MetaTaskId,
    pub idempotency_key: String,
}

impl CancelMetaTaskReq {
    /// Builds a cancel-meta-task request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the meta-task id is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        meta_task_id: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: session_token.into(),
            meta_task_id: MetaTaskId::parse(meta_task_id.into())?,
            idempotency_key: idempotency_key.into(),
        })
    }
}

/// Request to heartbeat an active session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatReq {
    pub session_token: String,
    pub idempotency_key: String,
}

/// Request to exit an active session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitSessionReq {
    pub session_token: String,
    pub idempotency_key: String,
}

/// Request to bind runtime identity evidence to a Covey session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordRuntimeAttestationReq {
    pub session_token: String,
    pub provider: String,
    pub model: String,
    pub provider_run_id: String,
    pub provider_run_id_issuer: String,
    runtime_identity: RuntimeAttestationIdentityReq,
    pub command_transcript_digest: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeAttestationIdentityReq {
    Process {
        process_id: String,
    },
    Container {
        container_id: String,
    },
    ProcessAndContainer {
        process_id: String,
        container_id: String,
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
        Ok(Self {
            session_token: session_token.into(),
            provider: provider.into(),
            model: model.into(),
            provider_run_id: provider_run_id.into(),
            provider_run_id_issuer: provider_run_id_issuer.into(),
            runtime_identity,
            command_transcript_digest: command_transcript_digest.into(),
            started_at,
            ended_at,
            idempotency_key: idempotency_key.into(),
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
}

impl RuntimeAttestationIdentityReq {
    fn try_from_parts(
        process_id: Option<String>,
        container_id: Option<String>,
    ) -> Result<Self, String> {
        let process_id = normalize_optional_runtime_identity(process_id, "process_id")?;
        let container_id = normalize_optional_runtime_identity(container_id, "container_id")?;
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
                Some(process_id)
            }
            Self::Container { .. } => None,
        }
    }

    fn container_id(&self) -> Option<&str> {
        match self {
            Self::Container { container_id } | Self::ProcessAndContainer { container_id, .. } => {
                Some(container_id)
            }
            Self::Process { .. } => None,
        }
    }
}

fn normalize_optional_runtime_identity(
    value: Option<String>,
    field: &str,
) -> Result<Option<String>, String> {
    value
        .map(|value| {
            if value.trim().is_empty() {
                Err(format!("{field} must not be empty"))
            } else {
                Ok(value)
            }
        })
        .transpose()
}

impl From<&RecordRuntimeAttestationReq> for RawRecordRuntimeAttestationReq {
    fn from(req: &RecordRuntimeAttestationReq) -> Self {
        Self {
            session_token: req.session_token.clone(),
            provider: req.provider.clone(),
            model: req.model.clone(),
            provider_run_id: req.provider_run_id.clone(),
            provider_run_id_issuer: req.provider_run_id_issuer.clone(),
            process_id: req.process_id().map(str::to_owned),
            container_id: req.container_id().map(str::to_owned),
            command_transcript_digest: req.command_transcript_digest.clone(),
            started_at: req.started_at,
            ended_at: req.ended_at,
            idempotency_key: req.idempotency_key.clone(),
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
    pub session_token: String,
    pub meta_task_id: MetaTaskId,
    pub subtask_id: Option<SubtaskId>,
    pub title: String,
    pub priority: i64,
    pub idempotency_key: String,
}

impl CreateSubtaskRequest {
    /// Builds a subtask creation request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the meta-task id or optional subtask id is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        meta_task_id: impl Into<String>,
        subtask_id: Option<String>,
        title: impl Into<String>,
        priority: i64,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: session_token.into(),
            meta_task_id: MetaTaskId::parse(meta_task_id.into())?,
            subtask_id: subtask_id.map(SubtaskId::parse).transpose()?,
            title: title.into(),
            priority,
            idempotency_key: idempotency_key.into(),
        })
    }
}

/// Request to claim the next available subtask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimNextReq {
    pub session_token: String,
    pub lease_duration_ms: LeaseDurationMs,
    pub idempotency_key: String,
}

impl ClaimNextReq {
    /// Builds a claim-next request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the lease duration is not positive.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        lease_duration_ms: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: session_token.into(),
            lease_duration_ms: LeaseDurationMs::parse(lease_duration_ms.into())?,
            idempotency_key: idempotency_key.into(),
        })
    }
}

/// Request to claim a specific subtask by ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimSubtaskReq {
    pub session_token: String,
    pub subtask_id: SubtaskId,
    pub lease_duration_ms: LeaseDurationMs,
    pub idempotency_key: String,
}

impl ClaimSubtaskReq {
    /// Builds a claim-subtask request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the subtask id or lease duration is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        subtask_id: impl Into<String>,
        lease_duration_ms: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: session_token.into(),
            subtask_id: SubtaskId::parse(subtask_id.into())?,
            lease_duration_ms: LeaseDurationMs::parse(lease_duration_ms.into())?,
            idempotency_key: idempotency_key.into(),
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
    pub session_token: String,
    pub claim_id: ClaimId,
    pub fence_seq: FenceSeq,
    pub idempotency_key: String,
}

/// Request to abandon a claimed subtask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbandonSubtaskReq {
    pub session_token: String,
    pub claim_id: ClaimId,
    pub fence_seq: FenceSeq,
    pub idempotency_key: String,
}

/// Request to release a held claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseClaimReq {
    pub session_token: String,
    pub claim_id: ClaimId,
    pub fence_seq: FenceSeq,
    pub idempotency_key: String,
}

/// Request to renew an active claim lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenewClaimReq {
    pub session_token: String,
    pub claim_id: ClaimId,
    pub fence_seq: FenceSeq,
    pub extend_by_ms: LeaseDurationMs,
    pub idempotency_key: String,
}

/// Request to publish an immutable artifact for a work subtask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishArtifactReq {
    pub session_token: String,
    pub claim_id: ClaimId,
    pub fence_seq: FenceSeq,
    pub artifact_digest: ArtifactDigest,
    pub artifact_kind: ArtifactKind,
    pub base_rev: BaseRev,
    pub manifest_path: String,
    pub changed_paths_digest: ChangedPathsDigest,
    pub idempotency_key: String,
}

impl PublishArtifactReq {
    /// Builds an artifact publication request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the claim id, fence sequence, artifact digest,
    /// base revision, or changed paths digest is invalid.
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
        idempotency_key: String,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: session_token.into(),
            claim_id: ClaimId::parse(claim_id.into())?,
            fence_seq: FenceSeq::parse(fence_seq.into())?,
            artifact_digest: ArtifactDigest::parse(artifact_digest)?,
            artifact_kind,
            base_rev: BaseRev::parse(base_rev)?,
            manifest_path,
            changed_paths_digest: ChangedPathsDigest::parse(changed_paths_digest)?,
            idempotency_key,
        })
    }
}

/// Request to create a review subtask for an exact artifact digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestReviewReq {
    pub session_token: String,
    pub subtask_id: SubtaskId,
    pub artifact_digest: ArtifactDigest,
    pub review_subtask_id: Option<SubtaskId>,
    pub priority: i64,
    pub idempotency_key: String,
}

impl RequestReviewReq {
    /// Builds a review request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the work subtask id, artifact digest, or explicit
    /// review subtask id is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        subtask_id: impl Into<String>,
        artifact_digest: impl Into<String>,
        review_subtask_id: Option<String>,
        priority: i64,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, String> {
        Ok(Self {
            session_token: session_token.into(),
            subtask_id: SubtaskId::parse(subtask_id.into()).map_err(|err| err.to_string())?,
            artifact_digest: ArtifactDigest::parse(artifact_digest.into())
                .map_err(|err| err.to_string())?,
            review_subtask_id: review_subtask_id
                .map(SubtaskId::parse)
                .transpose()
                .map_err(|err| err.to_string())?,
            priority,
            idempotency_key: idempotency_key.into(),
        })
    }
}

/// Request to decide a review while holding the matching review-subtask claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecideReviewReq {
    pub session_token: String,
    pub review_id: ReviewId,
    pub claim_id: ClaimId,
    pub fence_seq: FenceSeq,
    pub verdict: ReviewVerdict,
    pub findings_digest: FindingsDigest,
    pub idempotency_key: String,
}

impl DecideReviewReq {
    /// Builds a review decision request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the review id, claim id, fence sequence, or
    /// findings digest is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        review_id: String,
        claim_id: impl Into<String>,
        fence_seq: impl Into<i64>,
        verdict: ReviewVerdict,
        findings_digest: String,
        idempotency_key: String,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: session_token.into(),
            review_id: ReviewId::parse(review_id)?,
            claim_id: ClaimId::parse(claim_id.into())?,
            fence_seq: FenceSeq::parse(fence_seq.into())?,
            verdict,
            findings_digest: FindingsDigest::parse(findings_digest)?,
            idempotency_key,
        })
    }
}

/// Request to enqueue an approved artifact for apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnqueueForApplyReq {
    pub session_token: String,
    pub artifact_digest: ArtifactDigest,
    pub subtask_id: SubtaskId,
    pub settlement_target: SettlementTarget,
    pub idempotency_key: String,
}

impl EnqueueForApplyReq {
    /// Builds an apply-queue enqueue request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the artifact digest or subtask id is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        artifact_digest: String,
        subtask_id: String,
        settlement_target: SettlementTarget,
        idempotency_key: String,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: session_token.into(),
            artifact_digest: ArtifactDigest::parse(artifact_digest)?,
            subtask_id: SubtaskId::parse(subtask_id)?,
            settlement_target,
            idempotency_key,
        })
    }
}

/// Request to atomically claim the next ready-queue item for apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimReadyQueueReq {
    pub session_token: String,
    pub lease_duration_ms: LeaseDurationMs,
    pub idempotency_key: String,
}

impl ClaimReadyQueueReq {
    /// Builds a ready-queue claim request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the lease duration is not positive.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        lease_duration_ms: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: session_token.into(),
            lease_duration_ms: LeaseDurationMs::parse(lease_duration_ms.into())?,
            idempotency_key: idempotency_key.into(),
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
    pub session_token: String,
    pub queue_id: QueueId,
    pub lease_duration_ms: LeaseDurationMs,
    pub idempotency_key: String,
}

impl MarkInFlightReq {
    /// Builds a mark-in-flight request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the queue id or lease duration is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        queue_id: impl Into<String>,
        lease_duration_ms: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: session_token.into(),
            queue_id: QueueId::parse(queue_id.into())?,
            lease_duration_ms: LeaseDurationMs::parse(lease_duration_ms.into())?,
            idempotency_key: idempotency_key.into(),
        })
    }
}

/// Request to record an accepted verifier verdict for one apply attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordApplyVerificationReq {
    pub session_token: String,
    pub queue_id: QueueId,
    pub artifact_digest: ArtifactDigest,
    pub review_id: ReviewId,
    pub findings_digest: FindingsDigest,
    pub claim_fence_seq: FenceSeq,
    pub verifier: String,
    pub verdict_digest: ArtifactDigest,
    pub seal_digest: ArtifactDigest,
    pub idempotency_key: String,
}

impl RecordApplyVerificationReq {
    /// Builds an apply-verification request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when any queue, artifact, review, findings, fence, or
    /// verifier digest field is invalid.
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
            session_token: session_token.into(),
            queue_id: QueueId::parse(queue_id.into())?,
            artifact_digest: ArtifactDigest::parse(artifact_digest.into())?,
            review_id: ReviewId::parse(review_id.into())?,
            findings_digest: FindingsDigest::parse(findings_digest.into())?,
            claim_fence_seq: FenceSeq::parse(claim_fence_seq.into())?,
            verifier: verifier.into(),
            verdict_digest: ArtifactDigest::parse(verdict_digest.into())?,
            seal_digest: ArtifactDigest::parse(seal_digest.into())?,
            idempotency_key: idempotency_key.into(),
        })
    }
}

/// Request to verify that a landing authorization is still backed by live Covey state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyLandingAuthorizationReq {
    pub session_token: String,
    pub queue_id: QueueId,
    pub artifact_digest: ArtifactDigest,
    pub review_id: ReviewId,
    pub findings_digest: FindingsDigest,
    pub claim_fence_seq: FenceSeq,
    pub verifier: String,
    pub verdict_digest: ArtifactDigest,
    pub seal_digest: ArtifactDigest,
}

impl VerifyLandingAuthorizationReq {
    /// Builds a landing-authorization request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when any queue, artifact, review, findings, fence, or
    /// verifier digest field is invalid.
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
            session_token: session_token.into(),
            queue_id: QueueId::parse(queue_id.into())?,
            artifact_digest: ArtifactDigest::parse(artifact_digest.into())?,
            review_id: ReviewId::parse(review_id.into())?,
            findings_digest: FindingsDigest::parse(findings_digest.into())?,
            claim_fence_seq: FenceSeq::parse(claim_fence_seq.into())?,
            verifier: verifier.into(),
            verdict_digest: ArtifactDigest::parse(verdict_digest.into())?,
            seal_digest: ArtifactDigest::parse(seal_digest.into())?,
        })
    }
}

/// Request to mark an in-flight queue item applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkAppliedReq {
    pub session_token: String,
    pub queue_id: QueueId,
    pub claim_fence_seq: FenceSeq,
    pub idempotency_key: String,
}

impl MarkAppliedReq {
    /// Builds a mark-applied request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the queue id or fence sequence is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        queue_id: impl Into<String>,
        claim_fence_seq: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: session_token.into(),
            queue_id: QueueId::parse(queue_id.into())?,
            claim_fence_seq: FenceSeq::parse(claim_fence_seq.into())?,
            idempotency_key: idempotency_key.into(),
        })
    }
}

/// Request to supersede a queued or in-flight queue item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupersedeQueueItemReq {
    pub session_token: String,
    pub queue_id: QueueId,
    pub idempotency_key: String,
}

impl SupersedeQueueItemReq {
    /// Builds a supersede request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the queue id is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        queue_id: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: session_token.into(),
            queue_id: QueueId::parse(queue_id.into())?,
            idempotency_key: idempotency_key.into(),
        })
    }
}

/// Request to create an advisory reservation for a subtask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestReservationReq {
    pub session_token: String,
    pub owner_subtask_id: SubtaskId,
    scope: ReservationScope,
    pub lease_duration_ms: LeaseDurationMs,
    pub idempotency_key: String,
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
    /// Returns an error when the scope class/key/member shape is invalid.
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
            session_token: session_token.into(),
            owner_subtask_id,
            scope: ReservationScope::from_parts(scope_class, scope_key.into(), generated_members)?,
            lease_duration_ms,
            idempotency_key: idempotency_key.into(),
        })
    }

    /// Builds a reservation request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the owner id, lease duration, or scope shape is invalid.
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
    pub fn generated_members(&self) -> &[String] {
        self.scope.generated_members()
    }
}

impl From<&RequestReservationReq> for RawRequestReservationReq {
    fn from(req: &RequestReservationReq) -> Self {
        Self {
            session_token: req.session_token.clone(),
            owner_subtask_id: req.owner_subtask_id.clone(),
            scope_class: req.scope_class(),
            scope_key: req.scope_key().to_owned(),
            generated_members: req.generated_members().to_vec(),
            lease_duration_ms: req.lease_duration_ms,
            idempotency_key: req.idempotency_key.clone(),
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
    pub session_token: String,
    pub reservation_id: ReservationId,
    pub idempotency_key: String,
}

impl ReleaseReservationReq {
    /// Builds a release request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the reservation id is invalid.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        reservation_id: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: session_token.into(),
            reservation_id: ReservationId::parse(reservation_id.into())?,
            idempotency_key: idempotency_key.into(),
        })
    }
}

/// Request to renew an active reservation lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenewReservationReq {
    pub session_token: String,
    pub reservation_id: ReservationId,
    pub extend_by_ms: LeaseDurationMs,
    pub idempotency_key: String,
}

impl RenewReservationReq {
    /// Builds a renewal request from unvalidated scalar values.
    ///
    /// # Errors
    ///
    /// Returns an error when the reservation id is invalid or the extension is
    /// not positive.
    pub fn try_from_raw_parts(
        session_token: impl Into<String>,
        reservation_id: impl Into<String>,
        extend_by_ms: impl Into<i64>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        Ok(Self {
            session_token: session_token.into(),
            reservation_id: ReservationId::parse(reservation_id.into())?,
            extend_by_ms: LeaseDurationMs::parse(extend_by_ms.into())?,
            idempotency_key: idempotency_key.into(),
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
    pub fn generated_members(&self) -> &[String] {
        self.scope.generated_members()
    }
}

impl From<&OverlapQueryReq> for RawOverlapQueryReq {
    fn from(req: &OverlapQueryReq) -> Self {
        Self {
            scope_class: req.scope_class(),
            scope_key: req.scope_key().to_owned(),
            generated_members: req.generated_members().to_vec(),
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoopsAuthoritySnapshotReq {
    pub session_token: String,
    pub claim_id: String,
    pub fence_seq: i64,
    pub paths: Vec<String>,
}

/// Request to update the resolution state of a surfaced conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveConflictReq {
    pub session_token: String,
    pub conflict_id: String,
    pub resolution_state: ConflictResolutionState,
    pub idempotency_key: String,
}

//! Covey-owned apply proof replay and sealing.

use crate::model::{
    AgentInstanceId, AgentPrincipalId, ArtifactDigest, ArtifactManifestPath, ChangedPathsDigest,
    CommandTranscriptDigest, FenceSeq, FindingsDigest, LeaseDeadlineMs, ModelId, ProviderId,
    ProviderRunId, ProviderRunIdIssuer, QueueId, ReadyQueueState, ReviewId, ReviewState,
    ReviewVerdict, RuntimeContainerId, RuntimeProcessId, SessionRole, SessionState, SessionToken,
    SubtaskId, TimestampMs, VerifierId,
};
use clap::Parser;
use rusqlite::{Connection, Row, params};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::Command,
    str::FromStr,
};
use thiserror::Error;

const REQUEST_SCHEMA: &str = "covey_apply_proof_verify_request";
const SEAL_SCHEMA: &str = "mutai_covey_apply_proof_seal";
const BATCH_SEAL_SCHEMA: &str = "mutai_covey_apply_proof_batch_seal.v1";

#[derive(Debug, Parser)]
pub struct ApplyProofVerifyArgs {
    #[arg(long)]
    input: Option<PathBuf>,
    #[arg(long)]
    repo: Option<PathBuf>,
    #[arg(long = "covey-db")]
    covey_db: Option<PathBuf>,
    #[arg(long = "evidence-dir")]
    evidence_dir: Option<PathBuf>,
    #[arg(long = "subtask-id")]
    subtask_id: Option<SubtaskId>,
    #[arg(long = "artifact-digest")]
    artifact_digest: Option<ArtifactDigest>,
    #[arg(long = "review-id")]
    review_id: Option<ReviewId>,
    #[arg(long = "queue-id")]
    queue_id: Option<QueueId>,
    #[arg(long = "reviewer-findings-digest")]
    reviewer_findings_digest: Option<FindingsDigest>,
    #[arg(long = "apply-gate-session-token")]
    apply_gate_session_token: Option<SessionToken>,
    #[arg(long, default_value = "mutai-rs")]
    verifier: VerifierId,
    #[arg(long = "verdict-digest")]
    verdict_digest: Option<ArtifactDigest>,
    #[arg(long = "apply-verification-seal-digest")]
    apply_verification_seal_digest: Option<ArtifactDigest>,
    #[arg(long = "mainline-ref")]
    mainline_ref: Option<GitRef>,
    #[arg(long = "subject-ref")]
    subject_ref: Option<GitRef>,
    #[arg(long = "artifact-file", default_value = "feature.patch")]
    artifact_file: EvidenceFilePath,
    #[arg(long = "verdict-file", default_value = "apply-gate-output.json")]
    verdict_file: EvidenceFilePath,
    #[arg(long = "success-file", default_value = "full-suite-output.txt")]
    success_file: EvidenceFilePath,
    #[arg(long = "success-text")]
    success_text: Option<ProofSuccessText>,
    #[arg(long = "mission-packet-file")]
    mission_packet_file: Option<EvidenceFilePath>,
    #[arg(long = "enforce-promoted-mission-identity-contract")]
    enforce_promoted_mission_identity_contract: bool,
    #[arg(long = "require-observed-process-ids")]
    require_observed_process_ids: bool,
    #[arg(long = "require-host-signed-runtime-claims")]
    require_host_signed_runtime_claims: bool,
    #[arg(long = "require-provider-run-ids")]
    require_provider_run_ids: bool,
    #[arg(long = "trusted-provider-run-id-issuer")]
    trusted_provider_run_id_issuer: Vec<ProviderRunIdIssuer>,
    #[arg(long = "forbidden-provider-run-id-issuer")]
    forbidden_provider_run_id_issuer: Vec<ProviderRunIdIssuer>,
    #[arg(long = "target-ref")]
    target_ref: Option<GitRef>,
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Parser)]
pub struct ApplyProofBatchArgs {
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long = "seal-dir")]
    seal_dir: Option<PathBuf>,
    #[arg(long = "require-observed-process-ids")]
    require_observed_process_ids: bool,
    #[arg(long = "require-host-signed-runtime-claims")]
    require_host_signed_runtime_claims: bool,
    #[arg(long = "require-provider-run-ids")]
    require_provider_run_ids: bool,
    #[arg(long = "trusted-provider-run-id-issuer")]
    trusted_provider_run_id_issuer: Vec<ProviderRunIdIssuer>,
    #[arg(long = "forbidden-provider-run-id-issuer")]
    forbidden_provider_run_id_issuer: Vec<ProviderRunIdIssuer>,
    #[arg(long = "enforce-promoted-mission-identity-contract")]
    enforce_promoted_mission_identity_contract: bool,
}

#[derive(Debug, Error)]
pub enum ApplyProofError {
    #[error("{message}")]
    Request {
        message: String,
        output: Option<PathBuf>,
    },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("proof verification error: {0}")]
    Verification(String),
}

impl ApplyProofError {
    #[must_use]
    pub fn output_path(&self) -> Option<PathBuf> {
        match self {
            Self::Request { output, .. } => output.clone(),
            Self::Io(_) | Self::Json(_) | Self::Sqlite(_) | Self::Verification(_) => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct VerifyRequestFile {
    schema: Option<String>,
    repo: Option<PathBuf>,
    covey_db: Option<PathBuf>,
    evidence_dir: Option<PathBuf>,
    subtask_id: Option<SubtaskId>,
    artifact_digest: Option<ArtifactDigest>,
    review_id: Option<ReviewId>,
    queue_id: Option<QueueId>,
    reviewer_findings_digest: Option<FindingsDigest>,
    apply_gate_session_token: Option<SessionToken>,
    verifier: Option<VerifierId>,
    verdict_digest: Option<ArtifactDigest>,
    apply_verification_seal_digest: Option<ArtifactDigest>,
    mainline_ref: Option<GitRef>,
    subject_ref: Option<GitRef>,
    artifact_file: Option<EvidenceFilePath>,
    verdict_file: Option<EvidenceFilePath>,
    success_file: Option<EvidenceFilePath>,
    success_text: Option<ProofSuccessText>,
    mission_packet_file: Option<EvidenceFilePath>,
    enforce_promoted_mission_identity_contract: Option<bool>,
    require_observed_process_ids: Option<bool>,
    require_host_signed_runtime_claims: Option<bool>,
    require_provider_run_ids: Option<bool>,
    trusted_provider_run_id_issuer: Option<Vec<ProviderRunIdIssuer>>,
    forbidden_provider_run_id_issuer: Option<Vec<ProviderRunIdIssuer>>,
    target_ref: Option<GitRef>,
    output: Option<PathBuf>,
}

#[derive(Debug)]
struct VerifyRequest {
    repo: PathBuf,
    covey_db: PathBuf,
    evidence_dir: PathBuf,
    subtask_id: Option<SubtaskId>,
    artifact_digest: Option<ArtifactDigest>,
    review_id: Option<ReviewId>,
    queue_id: Option<QueueId>,
    reviewer_findings_digest: Option<FindingsDigest>,
    apply_gate_session_token: Option<SessionToken>,
    verifier: VerifierId,
    verdict_digest: Option<ArtifactDigest>,
    apply_verification_seal_digest: Option<ArtifactDigest>,
    mainline_ref: GitRef,
    subject_ref: Option<GitRef>,
    artifact_file: EvidenceFilePath,
    verdict_file: EvidenceFilePath,
    success_file: EvidenceFilePath,
    success_text: Option<ProofSuccessText>,
    mission_packet_file: Option<EvidenceFilePath>,
    enforce_promoted_mission_identity_contract: bool,
    require_observed_process_ids: bool,
    require_host_signed_runtime_claims: bool,
    require_provider_run_ids: bool,
    trusted_provider_run_id_issuer: Vec<ProviderRunIdIssuer>,
    forbidden_provider_run_id_issuer: Vec<ProviderRunIdIssuer>,
    target_ref: Option<GitRef>,
    output: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceFilePath(String);

impl EvidenceFilePath {
    fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err("evidence file path must not be empty".into());
        }
        if value.trim() != value {
            return Err(
                "evidence file path must not include leading or trailing whitespace".into(),
            );
        }
        if value.chars().any(char::is_control) {
            return Err("evidence file path must not contain control characters".into());
        }
        let path = Path::new(&value);
        if path.is_absolute() {
            return Err("evidence file path must be relative".into());
        }
        if path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err("evidence file path must not escape upward".into());
        }
        Ok(Self(value))
    }

    #[must_use]
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EvidenceFilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EvidenceFilePath {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value.to_owned())
    }
}

impl Serialize for EvidenceFilePath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for EvidenceFilePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GitRef(String);

impl GitRef {
    fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err("git ref must not be empty".into());
        }
        if value.trim() != value {
            return Err("git ref must not include leading or trailing whitespace".into());
        }
        if value.chars().any(char::is_control) {
            return Err("git ref must not contain control characters".into());
        }
        if value.chars().any(char::is_whitespace) {
            return Err("git ref must not contain whitespace".into());
        }
        if value.starts_with('-') {
            return Err("git ref must not start with '-'".into());
        }
        Ok(Self(value))
    }

    #[must_use]
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GitRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for GitRef {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value.to_owned())
    }
}

impl Serialize for GitRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GitRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProofSuccessText(String);

impl ProofSuccessText {
    fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err("success text must not be empty".into());
        }
        if value.chars().any(char::is_control) {
            return Err("success text must not contain control characters".into());
        }
        Ok(Self(value))
    }

    #[must_use]
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProofSuccessText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProofSuccessText {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value.to_owned())
    }
}

impl Serialize for ProofSuccessText {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProofSuccessText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize)]
struct ArtifactRow {
    artifact_digest: ArtifactDigest,
    produced_by_subtask_id: SubtaskId,
    produced_by_session: SessionToken,
    manifest_path: ArtifactManifestPath,
    changed_paths_digest: ChangedPathsDigest,
}

impl ArtifactRow {
    fn from_db_parts(
        artifact_digest: String,
        produced_by_subtask_id: String,
        produced_by_session: String,
        manifest_path: String,
        changed_paths_digest: String,
    ) -> Result<Self, ApplyProofError> {
        Ok(Self {
            artifact_digest: parse_proof_field(artifact_digest, "artifact_digest")?,
            produced_by_subtask_id: parse_proof_field(
                produced_by_subtask_id,
                "produced_by_subtask_id",
            )?,
            produced_by_session: parse_proof_field(produced_by_session, "produced_by_session")?,
            manifest_path: parse_proof_field(manifest_path, "manifest_path")?,
            changed_paths_digest: parse_proof_field(changed_paths_digest, "changed_paths_digest")?,
        })
    }
}

/// Replays artifact proof row parsing for external formal-model tests.
///
/// This is not a scheduling or settlement API. It exists so integration tests
/// can bind Quint traces to the same private row constructor used by apply
/// proof verification.
#[doc(hidden)]
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn artifact_proof_row_accepts_for_model(
    artifact_digest_valid: bool,
    produced_by_subtask_valid: bool,
    produced_by_session_valid: bool,
    manifest_path_shape: &str,
    changed_paths_digest_valid: bool,
) -> bool {
    let manifest_path = match manifest_path_shape {
        "EmptyPath" => "",
        "ControlPath" => "manifest\n.json",
        _ => "artifact bundle/manifest.json",
    };
    ArtifactRow::from_db_parts(
        if artifact_digest_valid {
            "blake3:artifact"
        } else {
            "artifact"
        }
        .into(),
        if produced_by_subtask_valid {
            "subtask-1"
        } else {
            "subtask 1"
        }
        .into(),
        if produced_by_session_valid {
            "session-1"
        } else {
            "session 1"
        }
        .into(),
        manifest_path.into(),
        if changed_paths_digest_valid {
            "blake3:paths"
        } else {
            "paths"
        }
        .into(),
    )
    .is_ok()
}

#[derive(Debug, Clone)]
struct ReviewRow {
    review_id: ReviewId,
    subtask_id: SubtaskId,
    artifact_digest: ArtifactDigest,
    reviewer_session: SessionToken,
    review_subtask_id: SubtaskId,
    lifecycle: ReviewProofLifecycle,
}

#[derive(Debug, Clone)]
enum ReviewProofLifecycle {
    Requested,
    InProgress,
    Decided {
        verdict: ReviewVerdict,
        findings_digest: FindingsDigest,
    },
    Superseded,
}

#[derive(Debug, Clone)]
enum ReviewProofDecisionEvidence {
    Absent,
    Forbidden,
    Decided {
        verdict: ReviewVerdict,
        findings_digest: FindingsDigest,
    },
}

impl ReviewRow {
    #[allow(clippy::too_many_arguments)]
    fn from_db_parts(
        review_id: String,
        subtask_id: String,
        artifact_digest: String,
        reviewer_session: String,
        review_subtask_id: String,
        state: ReviewState,
        decision_evidence: ReviewProofDecisionEvidence,
    ) -> Result<Self, ApplyProofError> {
        let review_id = parse_proof_field(review_id, "review_id")?;
        let subtask_id = parse_proof_field(subtask_id, "subtask_id")?;
        let artifact_digest = parse_proof_field(artifact_digest, "artifact_digest")?;
        let reviewer_session = parse_proof_field(reviewer_session, "reviewer_session")?;
        let review_subtask_id = parse_proof_field(review_subtask_id, "review_subtask_id")?;
        let lifecycle = ReviewProofLifecycle::from_parts(state, decision_evidence)?;
        Ok(Self {
            review_id,
            subtask_id,
            artifact_digest,
            reviewer_session,
            review_subtask_id,
            lifecycle,
        })
    }

    fn state(&self) -> ReviewState {
        self.lifecycle.state()
    }

    fn verdict(&self) -> Option<ReviewVerdict> {
        self.lifecycle.verdict()
    }

    fn findings_digest(&self) -> Option<&str> {
        self.lifecycle.findings_digest()
    }

    fn approved(&self) -> bool {
        matches!(
            self.lifecycle,
            ReviewProofLifecycle::Decided {
                verdict: ReviewVerdict::Approve,
                ..
            }
        )
    }
}

impl ReviewProofLifecycle {
    fn from_parts(
        state: ReviewState,
        decision_evidence: ReviewProofDecisionEvidence,
    ) -> Result<Self, ApplyProofError> {
        match state {
            ReviewState::Requested => {
                reject_review_decision_evidence("requested", decision_evidence)?;
                Ok(Self::Requested)
            }
            ReviewState::InProgress => {
                reject_review_decision_evidence("in-progress", decision_evidence)?;
                Ok(Self::InProgress)
            }
            ReviewState::Decided => {
                let ReviewProofDecisionEvidence::Decided {
                    verdict,
                    findings_digest,
                } = decision_evidence
                else {
                    return Err(ApplyProofError::Verification(
                        "decided review proof row requires decision evidence".into(),
                    ));
                };
                Ok(Self::Decided {
                    verdict,
                    findings_digest,
                })
            }
            ReviewState::Superseded => {
                reject_review_decision_evidence("superseded", decision_evidence)?;
                Ok(Self::Superseded)
            }
        }
    }

    fn state(&self) -> ReviewState {
        match self {
            Self::Requested => ReviewState::Requested,
            Self::InProgress => ReviewState::InProgress,
            Self::Decided { .. } => ReviewState::Decided,
            Self::Superseded => ReviewState::Superseded,
        }
    }

    fn verdict(&self) -> Option<ReviewVerdict> {
        match self {
            Self::Decided { verdict, .. } => Some(*verdict),
            Self::Requested | Self::InProgress | Self::Superseded => None,
        }
    }

    fn findings_digest(&self) -> Option<&str> {
        match self {
            Self::Decided {
                findings_digest, ..
            } => Some(findings_digest.as_str()),
            Self::Requested | Self::InProgress | Self::Superseded => None,
        }
    }
}

impl ReviewProofDecisionEvidence {
    fn from_raw_for_state(
        state: ReviewState,
        raw_verdict: Option<String>,
        raw_findings_digest: Option<String>,
    ) -> Result<Self, ApplyProofError> {
        match state {
            ReviewState::Decided => {
                let verdict = raw_verdict
                    .ok_or_else(|| {
                        ApplyProofError::Verification(
                            "decided review proof row requires verdict".into(),
                        )
                    })
                    .and_then(|verdict| {
                        ReviewVerdict::from_str(&verdict).map_err(|err| {
                            ApplyProofError::Verification(format!(
                                "invalid review verdict in proof row: {err}"
                            ))
                        })
                    })?;
                let findings_digest = raw_findings_digest
                    .filter(|value| !value.trim().is_empty())
                    .map(|digest| parse_proof_field(digest, "findings_digest"))
                    .transpose()?
                    .ok_or_else(|| {
                        ApplyProofError::Verification(
                            "decided review proof row requires non-empty findings_digest".into(),
                        )
                    })?;
                Ok(Self::Decided {
                    verdict,
                    findings_digest,
                })
            }
            ReviewState::Requested | ReviewState::InProgress | ReviewState::Superseded => {
                if raw_verdict.is_some() || raw_findings_digest.is_some() {
                    Ok(Self::Forbidden)
                } else {
                    Ok(Self::Absent)
                }
            }
        }
    }
}

fn reject_review_decision_evidence(
    state: &str,
    decision_evidence: ReviewProofDecisionEvidence,
) -> Result<(), ApplyProofError> {
    if matches!(
        decision_evidence,
        ReviewProofDecisionEvidence::Decided { .. } | ReviewProofDecisionEvidence::Forbidden
    ) {
        return Err(ApplyProofError::Verification(format!(
            "{state} review proof row must not carry decision evidence"
        )));
    }
    Ok(())
}

impl Serialize for ReviewRow {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let verdict = self.verdict();
        let findings_digest = self.findings_digest();
        let state = self.state();
        let mut row = serializer.serialize_struct("ReviewRow", 8)?;
        row.serialize_field("review_id", self.review_id.as_str())?;
        row.serialize_field("subtask_id", self.subtask_id.as_str())?;
        row.serialize_field("artifact_digest", self.artifact_digest.as_str())?;
        row.serialize_field("reviewer_session", self.reviewer_session.as_str())?;
        row.serialize_field("review_subtask_id", self.review_subtask_id.as_str())?;
        row.serialize_field("verdict", &verdict)?;
        row.serialize_field("findings_digest", &findings_digest)?;
        row.serialize_field("state", &state)?;
        row.end()
    }
}

#[derive(Debug, Clone)]
struct ReadyQueueRow {
    queue_id: QueueId,
    artifact_digest: ArtifactDigest,
    subtask_id: SubtaskId,
    lifecycle: ReadyQueueProofLifecycle,
}

#[derive(Debug, Clone)]
enum ReadyQueueProofLifecycle {
    Queued {
        last_claim_fence_seq: Option<FenceSeq>,
    },
    InFlight {
        claimed_by_session_token: SessionToken,
        claim_fence_seq: FenceSeq,
        claim_lease_deadline: LeaseDeadlineMs,
    },
    Applied {
        claim_fence_seq: FenceSeq,
    },
    Superseded {
        last_claim_fence_seq: Option<FenceSeq>,
    },
    Cancelled {
        last_claim_fence_seq: Option<FenceSeq>,
    },
}

#[derive(Debug, Clone)]
enum ReadyQueueProofClaimEvidence {
    Absent {
        last_claim_fence_seq: Option<FenceSeq>,
    },
    Active {
        claimed_by_session_token: SessionToken,
        claim_fence_seq: FenceSeq,
        claim_lease_deadline: LeaseDeadlineMs,
    },
    Applied {
        claim_fence_seq: FenceSeq,
    },
    Forbidden,
}

impl ReadyQueueRow {
    fn from_db_parts(
        queue_id: String,
        artifact_digest: String,
        subtask_id: String,
        state: ReadyQueueState,
        claim_evidence: ReadyQueueProofClaimEvidence,
    ) -> Result<Self, ApplyProofError> {
        let queue_id = parse_proof_field(queue_id, "queue_id")?;
        let artifact_digest = parse_proof_field(artifact_digest, "artifact_digest")?;
        let subtask_id = parse_proof_field(subtask_id, "subtask_id")?;
        let lifecycle = ReadyQueueProofLifecycle::from_parts(state, claim_evidence)?;
        Ok(Self {
            queue_id,
            artifact_digest,
            subtask_id,
            lifecycle,
        })
    }

    fn state(&self) -> ReadyQueueState {
        match &self.lifecycle {
            ReadyQueueProofLifecycle::Queued { .. } => ReadyQueueState::Queued,
            ReadyQueueProofLifecycle::InFlight { .. } => ReadyQueueState::InFlight,
            ReadyQueueProofLifecycle::Applied { .. } => ReadyQueueState::Applied,
            ReadyQueueProofLifecycle::Superseded { .. } => ReadyQueueState::Superseded,
            ReadyQueueProofLifecycle::Cancelled { .. } => ReadyQueueState::Cancelled,
        }
    }

    fn applied_claim_fence_seq(&self) -> Option<i64> {
        match self.lifecycle {
            ReadyQueueProofLifecycle::Applied { claim_fence_seq } => Some(claim_fence_seq.get()),
            ReadyQueueProofLifecycle::Queued { .. }
            | ReadyQueueProofLifecycle::InFlight { .. }
            | ReadyQueueProofLifecycle::Superseded { .. }
            | ReadyQueueProofLifecycle::Cancelled { .. } => None,
        }
    }

    fn claim_fence_seq(&self) -> Option<i64> {
        if let Some((_, claim_fence_seq, _)) = self.active_claim() {
            return Some(claim_fence_seq);
        }

        match self.lifecycle {
            ReadyQueueProofLifecycle::Queued {
                last_claim_fence_seq,
            }
            | ReadyQueueProofLifecycle::Superseded {
                last_claim_fence_seq,
            }
            | ReadyQueueProofLifecycle::Cancelled {
                last_claim_fence_seq,
            } => last_claim_fence_seq.map(FenceSeq::get),
            ReadyQueueProofLifecycle::InFlight { .. } => None,
            ReadyQueueProofLifecycle::Applied { claim_fence_seq } => Some(claim_fence_seq.get()),
        }
    }

    fn claimed_by_session_token(&self) -> Option<&str> {
        self.active_claim()
            .map(|(claimed_by_session_token, _, _)| claimed_by_session_token)
    }

    fn active_claim(&self) -> Option<(&str, i64, i64)> {
        match &self.lifecycle {
            ReadyQueueProofLifecycle::InFlight {
                claimed_by_session_token,
                claim_fence_seq,
                claim_lease_deadline,
                ..
            } => Some((
                claimed_by_session_token.as_str(),
                claim_fence_seq.get(),
                claim_lease_deadline.get(),
            )),
            ReadyQueueProofLifecycle::Queued { .. }
            | ReadyQueueProofLifecycle::Applied { .. }
            | ReadyQueueProofLifecycle::Superseded { .. }
            | ReadyQueueProofLifecycle::Cancelled { .. } => None,
        }
    }
}

impl ReadyQueueProofLifecycle {
    fn from_parts(
        state: ReadyQueueState,
        claim_evidence: ReadyQueueProofClaimEvidence,
    ) -> Result<Self, ApplyProofError> {
        match state {
            ReadyQueueState::Queued => {
                let ReadyQueueProofClaimEvidence::Absent {
                    last_claim_fence_seq,
                } = claim_evidence
                else {
                    return reject_ready_queue_claim_evidence(state);
                };
                Ok(Self::Queued {
                    last_claim_fence_seq,
                })
            }
            ReadyQueueState::InFlight => {
                let ReadyQueueProofClaimEvidence::Active {
                    claimed_by_session_token,
                    claim_fence_seq,
                    claim_lease_deadline,
                } = claim_evidence
                else {
                    return Err(ApplyProofError::Verification(
                        "in-flight ready queue row requires complete active claim evidence".into(),
                    ));
                };
                Ok(Self::InFlight {
                    claimed_by_session_token,
                    claim_fence_seq,
                    claim_lease_deadline,
                })
            }
            ReadyQueueState::Applied => {
                let ReadyQueueProofClaimEvidence::Applied { claim_fence_seq } = claim_evidence
                else {
                    return reject_ready_queue_claim_evidence(state);
                };
                Ok(Self::Applied { claim_fence_seq })
            }
            ReadyQueueState::Superseded => {
                let ReadyQueueProofClaimEvidence::Absent {
                    last_claim_fence_seq,
                } = claim_evidence
                else {
                    return reject_ready_queue_claim_evidence(state);
                };
                Ok(Self::Superseded {
                    last_claim_fence_seq,
                })
            }
            ReadyQueueState::Cancelled => {
                let ReadyQueueProofClaimEvidence::Absent {
                    last_claim_fence_seq,
                } = claim_evidence
                else {
                    return reject_ready_queue_claim_evidence(state);
                };
                Ok(Self::Cancelled {
                    last_claim_fence_seq,
                })
            }
        }
    }
}

impl ReadyQueueProofClaimEvidence {
    fn from_raw_for_state(
        state: ReadyQueueState,
        raw_claimed_by_session_token: Option<String>,
        raw_claim_fence_seq: Option<i64>,
        raw_claim_lease_deadline: Option<i64>,
    ) -> Result<Self, ApplyProofError> {
        match state {
            ReadyQueueState::Queued | ReadyQueueState::Superseded | ReadyQueueState::Cancelled => {
                if raw_claimed_by_session_token.is_some() || raw_claim_lease_deadline.is_some() {
                    Ok(Self::Forbidden)
                } else {
                    Ok(Self::Absent {
                        last_claim_fence_seq: parse_optional_fence_seq(raw_claim_fence_seq)?,
                    })
                }
            }
            ReadyQueueState::InFlight => {
                let claimed_by_session_token = raw_claimed_by_session_token
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        ApplyProofError::Verification(
                            "in-flight ready queue row requires claimed_by_session_token".into(),
                        )
                    })?;
                let claimed_by_session_token =
                    parse_proof_field(claimed_by_session_token, "claimed_by_session_token")?;
                let claim_fence_seq = raw_claim_fence_seq.ok_or_else(|| {
                    ApplyProofError::Verification(
                        "in-flight ready queue row requires claim_fence_seq".into(),
                    )
                })?;
                let claim_fence_seq = parse_fence_seq(claim_fence_seq)?;
                let claim_lease_deadline = raw_claim_lease_deadline.ok_or_else(|| {
                    ApplyProofError::Verification(
                        "in-flight ready queue row requires claim_lease_deadline".into(),
                    )
                })?;
                let claim_lease_deadline = parse_lease_deadline(claim_lease_deadline)?;
                Ok(Self::Active {
                    claimed_by_session_token,
                    claim_fence_seq,
                    claim_lease_deadline,
                })
            }
            ReadyQueueState::Applied => {
                if raw_claimed_by_session_token.is_some() || raw_claim_lease_deadline.is_some() {
                    return Ok(Self::Forbidden);
                }
                let claim_fence_seq = raw_claim_fence_seq.ok_or_else(|| {
                    ApplyProofError::Verification(
                        "applied ready queue row requires claim_fence_seq".into(),
                    )
                })?;
                Ok(Self::Applied {
                    claim_fence_seq: parse_fence_seq(claim_fence_seq)?,
                })
            }
        }
    }
}

fn reject_ready_queue_claim_evidence<T>(state: ReadyQueueState) -> Result<T, ApplyProofError> {
    Err(ApplyProofError::Verification(format!(
        "{state} ready queue row must not carry active claim fields"
    )))
}

fn parse_optional_fence_seq(value: Option<i64>) -> Result<Option<FenceSeq>, ApplyProofError> {
    value.map(parse_fence_seq).transpose()
}

fn parse_fence_seq(value: i64) -> Result<FenceSeq, ApplyProofError> {
    FenceSeq::parse(value).map_err(|error| {
        ApplyProofError::Verification(format!(
            "invalid ready queue claim_fence_seq in proof row: {error}"
        ))
    })
}

fn parse_lease_deadline(value: i64) -> Result<LeaseDeadlineMs, ApplyProofError> {
    LeaseDeadlineMs::parse(value).map_err(|error| {
        ApplyProofError::Verification(format!(
            "invalid ready queue claim_lease_deadline in proof row: {error}"
        ))
    })
}

/// Replays review proof row lifecycle parsing for external formal-model tests.
///
/// This is not a scheduling or settlement API. It exists so integration tests
/// can bind Quint traces to the same private row constructors used by apply
/// proof verification.
#[doc(hidden)]
#[must_use]
pub fn review_proof_row_lifecycle_accepts_for_model(
    state: ReviewState,
    verdict: Option<ReviewVerdict>,
    findings_digest_present: bool,
) -> bool {
    let decision_evidence = ReviewProofDecisionEvidence::from_raw_for_state(
        state,
        verdict.map(|verdict| verdict.to_string()),
        findings_digest_present.then(|| "blake3:findings".to_owned()),
    );
    let Ok(decision_evidence) = decision_evidence else {
        return false;
    };
    ReviewRow::from_db_parts(
        "review-1".into(),
        "subtask-1".into(),
        "blake3:artifact".into(),
        "reviewer-session-1".into(),
        "review-subtask-1".into(),
        state,
        decision_evidence,
    )
    .is_ok()
}

/// Replays ready-queue proof row lifecycle parsing for external formal-model
/// tests.
///
/// This is not a scheduling or settlement API. It exists so integration tests
/// can bind Quint traces to the same private row constructors used by apply
/// proof verification.
#[doc(hidden)]
#[must_use]
pub fn ready_queue_proof_row_lifecycle_accepts_for_model(
    state: ReadyQueueState,
    claimed_by_session_present: bool,
    claim_fence_seq: Option<i64>,
    claim_lease_deadline_present: bool,
) -> bool {
    let claim_evidence = ReadyQueueProofClaimEvidence::from_raw_for_state(
        state,
        claimed_by_session_present.then(|| "session-apply".to_owned()),
        claim_fence_seq,
        claim_lease_deadline_present.then_some(10_000),
    );
    let Ok(claim_evidence) = claim_evidence else {
        return false;
    };
    ReadyQueueRow::from_db_parts(
        "queue-1".into(),
        "blake3:artifact".into(),
        "subtask-1".into(),
        state,
        claim_evidence,
    )
    .is_ok()
}

/// Replays apply-verification proof row parsing for external formal-model
/// tests.
///
/// This is not a scheduling or settlement API. It exists so integration tests
/// can bind Quint traces to the same private row constructor used by apply
/// proof verification.
#[doc(hidden)]
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn apply_verification_proof_row_accepts_for_model(
    queue_id_valid: bool,
    artifact_digest_valid: bool,
    review_id_valid: bool,
    findings_digest_valid: bool,
    claim_fence_seq: i64,
    verifier_valid: bool,
    verdict_digest_valid: bool,
    seal_digest_valid: bool,
    recorded_by_session_valid: bool,
    created_at: i64,
) -> bool {
    ApplyVerificationRow::from_db_parts(
        if queue_id_valid { "queue-1" } else { "queue 1" }.into(),
        if artifact_digest_valid {
            "blake3:artifact"
        } else {
            "artifact"
        }
        .into(),
        if review_id_valid {
            "review-1"
        } else {
            "review 1"
        }
        .into(),
        if findings_digest_valid {
            "blake3:findings"
        } else {
            "findings"
        }
        .into(),
        claim_fence_seq,
        if verifier_valid {
            "mutai-rs"
        } else {
            "mutai rs"
        }
        .into(),
        if verdict_digest_valid {
            "blake3:verdict"
        } else {
            "verdict"
        }
        .into(),
        if seal_digest_valid {
            "blake3:seal"
        } else {
            "seal"
        }
        .into(),
        if recorded_by_session_valid {
            "session-apply"
        } else {
            "session apply"
        }
        .into(),
        created_at,
    )
    .is_ok()
}

impl Serialize for ReadyQueueRow {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let state = self.state();
        let claimed_by_session_token = self.claimed_by_session_token();
        let claim_fence_seq = self.claim_fence_seq();
        let mut row = serializer.serialize_struct("ReadyQueueRow", 6)?;
        row.serialize_field("queue_id", self.queue_id.as_str())?;
        row.serialize_field("artifact_digest", self.artifact_digest.as_str())?;
        row.serialize_field("subtask_id", self.subtask_id.as_str())?;
        row.serialize_field("state", &state)?;
        row.serialize_field("claimed_by_session_token", &claimed_by_session_token)?;
        row.serialize_field("claim_fence_seq", &claim_fence_seq)?;
        row.end()
    }
}

#[derive(Debug, Clone, Serialize)]
struct ApplyVerificationRow {
    queue_id: QueueId,
    artifact_digest: ArtifactDigest,
    review_id: ReviewId,
    findings_digest: FindingsDigest,
    claim_fence_seq: FenceSeq,
    verifier: VerifierId,
    verdict_digest: ArtifactDigest,
    seal_digest: ArtifactDigest,
    recorded_by_session: SessionToken,
    created_at: i64,
}

impl ApplyVerificationRow {
    #[allow(clippy::too_many_arguments)]
    fn from_db_parts(
        queue_id: String,
        artifact_digest: String,
        review_id: String,
        findings_digest: String,
        claim_fence_seq: i64,
        verifier: String,
        verdict_digest: String,
        seal_digest: String,
        recorded_by_session: String,
        created_at: i64,
    ) -> Result<Self, ApplyProofError> {
        let queue_id = parse_proof_field(queue_id, "queue_id")?;
        let artifact_digest = parse_proof_field(artifact_digest, "artifact_digest")?;
        let review_id = parse_proof_field(review_id, "review_id")?;
        let findings_digest = parse_proof_field(findings_digest, "findings_digest")?;
        let claim_fence_seq = parse_fence_seq(claim_fence_seq)?;
        let verifier = parse_proof_field(verifier, "verifier")?;
        let verdict_digest = parse_proof_field(verdict_digest, "verdict_digest")?;
        let seal_digest = parse_proof_field(seal_digest, "seal_digest")?;
        let recorded_by_session = parse_proof_field(recorded_by_session, "recorded_by_session")?;
        let created_at = parse_timestamp_ms(created_at, "apply verification created_at")?;
        Ok(Self {
            queue_id,
            artifact_digest,
            review_id,
            findings_digest,
            claim_fence_seq,
            verifier,
            verdict_digest,
            seal_digest,
            recorded_by_session,
            created_at: created_at.get(),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
struct SessionRow {
    session_token: SessionToken,
    agent_principal_id: AgentPrincipalId,
    agent_instance_id: AgentInstanceId,
    role: SessionRole,
    state: SessionState,
}

#[derive(Debug, Clone)]
struct RuntimeAttestationRow {
    session_token: SessionToken,
    agent_principal_id: AgentPrincipalId,
    agent_instance_id: AgentInstanceId,
    role: SessionRole,
    provider: ProviderId,
    model: ModelId,
    runtime_identity: RuntimeProofIdentity,
    command_transcript_digest: CommandTranscriptDigest,
    started_at: TimestampMs,
    ended_at: TimestampMs,
    recorded_at: TimestampMs,
    provider_run_identity: Option<ProviderRunProofIdentity>,
}

#[derive(Debug, Clone)]
enum RuntimeProofIdentity {
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

#[derive(Debug, Clone)]
struct ProviderRunProofIdentity {
    provider_run_id: ProviderRunId,
    provider_run_id_issuer: ProviderRunIdIssuer,
}

#[derive(Debug, Clone, Serialize)]
struct RawRuntimeAttestationRow<'a> {
    session_token: &'a str,
    agent_principal_id: &'a str,
    agent_instance_id: &'a str,
    role: String,
    provider: &'a str,
    model: &'a str,
    process_id: Option<&'a str>,
    container_id: Option<&'a str>,
    command_transcript_digest: &'a str,
    started_at: i64,
    ended_at: i64,
    recorded_at: i64,
    provider_run_id: Option<&'a str>,
    provider_run_id_issuer: Option<&'a str>,
}

impl RuntimeAttestationRow {
    #[allow(clippy::too_many_arguments)]
    fn from_db_parts(
        session_token: String,
        agent_principal_id: String,
        agent_instance_id: String,
        role: String,
        provider: String,
        model: String,
        process_id: Option<String>,
        container_id: Option<String>,
        command_transcript_digest: String,
        started_at: i64,
        ended_at: i64,
        recorded_at: i64,
        provider_run_id: Option<String>,
        provider_run_id_issuer: Option<String>,
    ) -> Result<Self, ApplyProofError> {
        let runtime_identity = RuntimeProofIdentity::from_parts(process_id, container_id)?;
        let provider_run_identity =
            ProviderRunProofIdentity::optional_from_parts(provider_run_id, provider_run_id_issuer)?;
        let started_at = parse_timestamp_ms(started_at, "started_at")?;
        let ended_at = parse_timestamp_ms(ended_at, "ended_at")?;
        let recorded_at = parse_timestamp_ms(recorded_at, "recorded_at")?;
        if ended_at < started_at {
            return Err(ApplyProofError::Verification(
                "runtime attestation ended_at must be greater than or equal to started_at".into(),
            ));
        }
        if recorded_at < ended_at {
            return Err(ApplyProofError::Verification(
                "runtime attestation recorded_at must be greater than or equal to ended_at".into(),
            ));
        }
        Ok(Self {
            session_token: parse_proof_field(session_token, "session_token")?,
            agent_principal_id: parse_proof_field(agent_principal_id, "agent_principal_id")?,
            agent_instance_id: parse_proof_field(agent_instance_id, "agent_instance_id")?,
            role: parse_proof_enum(role, "role")?,
            provider: parse_proof_field(provider, "provider")?,
            model: parse_proof_field(model, "model")?,
            runtime_identity,
            command_transcript_digest: parse_proof_field(
                command_transcript_digest,
                "command_transcript_digest",
            )?,
            started_at,
            ended_at,
            recorded_at,
            provider_run_identity,
        })
    }

    fn process_id(&self) -> Option<&str> {
        self.runtime_identity.process_id()
    }

    fn container_id(&self) -> Option<&str> {
        self.runtime_identity.container_id()
    }

    fn provider_run_id(&self) -> Option<&str> {
        self.provider_run_identity
            .as_ref()
            .map(|identity| identity.provider_run_id.as_str())
    }

    fn provider_run_id_issuer(&self) -> Option<&str> {
        self.provider_run_identity
            .as_ref()
            .map(|identity| identity.provider_run_id_issuer.as_str())
    }
}

/// Replays runtime-attestation proof row parsing for external formal-model tests.
///
/// This is not a scheduling or settlement API. It exists so integration tests
/// can bind Quint traces to the same private row constructor used by apply
/// proof verification.
#[doc(hidden)]
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn runtime_attestation_proof_row_accepts_for_model(
    runtime_identity_shape: &str,
    provider_run_shape: &str,
    timestamp_shape: &str,
    session_token_valid: bool,
    agent_principal_valid: bool,
    agent_instance_valid: bool,
    role_valid: bool,
    provider_valid: bool,
    model_valid: bool,
    transcript_valid: bool,
) -> bool {
    let (process_id, container_id) = match runtime_identity_shape {
        "ContainerOnly" => (None, Some("container-1".to_owned())),
        "ProcessAndContainer" => (Some("1234".to_owned()), Some("container-1".to_owned())),
        "MissingRuntimeIdentityShape" => (None, None),
        "InvalidProcessIdShape" => (Some(" 1234".to_owned()), None),
        "InvalidContainerIdShape" => (None, Some(" container-1".to_owned())),
        _ => (Some("1234".to_owned()), None),
    };
    let (provider_run_id, provider_run_id_issuer) = match provider_run_shape {
        "LegacyMissingProviderRun" => (None, None),
        "PartialProviderRunIdShape" => (Some("provider-run-1".to_owned()), None),
        "PartialProviderRunIssuerShape" => (None, Some("provider-issuer-1".to_owned())),
        "InvalidProviderRunIdShape" => (
            Some(" provider-run-1".to_owned()),
            Some("provider-issuer-1".to_owned()),
        ),
        "InvalidProviderRunIssuerShape" => (
            Some("provider-run-1".to_owned()),
            Some(" provider-issuer-1".to_owned()),
        ),
        _ => (
            Some("provider-run-1".to_owned()),
            Some("provider-issuer-1".to_owned()),
        ),
    };
    let (started_at, ended_at, recorded_at) = match timestamp_shape {
        "NegativeStartedAtShape" => (-1, 11, 12),
        "NegativeEndedAtShape" => (10, -1, 12),
        "NegativeRecordedAtShape" => (10, 11, -1),
        "EndedBeforeStartedShape" => (12, 11, 13),
        "RecordedBeforeEndedShape" => (10, 12, 11),
        _ => (10, 11, 12),
    };
    RuntimeAttestationRow::from_db_parts(
        if session_token_valid {
            "session-1"
        } else {
            "session 1"
        }
        .into(),
        if agent_principal_valid {
            "agent-1"
        } else {
            "agent 1"
        }
        .into(),
        if agent_instance_valid {
            "instance-1"
        } else {
            "instance 1"
        }
        .into(),
        if role_valid { "executor" } else { "worker" }.into(),
        if provider_valid {
            "provider-1"
        } else {
            "provider 1"
        }
        .into(),
        if model_valid { "model-1" } else { "model 1" }.into(),
        process_id,
        container_id,
        if transcript_valid {
            "blake3:transcript"
        } else {
            "transcript"
        }
        .into(),
        started_at,
        ended_at,
        recorded_at,
        provider_run_id,
        provider_run_id_issuer,
    )
    .is_ok()
}

impl RuntimeProofIdentity {
    fn from_parts(
        process_id: Option<String>,
        container_id: Option<String>,
    ) -> Result<Self, ApplyProofError> {
        let process_id = parse_optional_runtime_field(process_id, "process_id")?;
        let container_id = parse_optional_runtime_field(container_id, "container_id")?;
        match (process_id, container_id) {
            (Some(process_id), Some(container_id)) => Ok(Self::ProcessAndContainer {
                process_id,
                container_id,
            }),
            (Some(process_id), None) => Ok(Self::Process { process_id }),
            (None, Some(container_id)) => Ok(Self::Container { container_id }),
            (None, None) => Err(ApplyProofError::Verification(
                "runtime attestation requires process_id or container_id".into(),
            )),
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

impl ProviderRunProofIdentity {
    fn optional_from_parts(
        provider_run_id: Option<String>,
        provider_run_id_issuer: Option<String>,
    ) -> Result<Option<Self>, ApplyProofError> {
        let provider_run_id = parse_optional_runtime_field(provider_run_id, "provider_run_id")?;
        let provider_run_id_issuer =
            parse_optional_runtime_field(provider_run_id_issuer, "provider_run_id_issuer")?;
        match (provider_run_id, provider_run_id_issuer) {
            (Some(provider_run_id), Some(provider_run_id_issuer)) => Ok(Some(Self {
                provider_run_id,
                provider_run_id_issuer,
            })),
            (None, None) => Ok(None),
            _ => Err(ApplyProofError::Verification(
                "runtime attestation provider run identity must include both id and issuer".into(),
            )),
        }
    }
}

impl Serialize for RuntimeAttestationRow {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        RawRuntimeAttestationRow {
            session_token: self.session_token.as_str(),
            agent_principal_id: self.agent_principal_id.as_str(),
            agent_instance_id: self.agent_instance_id.as_str(),
            role: self.role.to_string(),
            provider: self.provider.as_str(),
            model: self.model.as_str(),
            process_id: self.process_id(),
            container_id: self.container_id(),
            command_transcript_digest: self.command_transcript_digest.as_str(),
            started_at: self.started_at.get(),
            ended_at: self.ended_at.get(),
            recorded_at: self.recorded_at.get(),
            provider_run_id: self.provider_run_id(),
            provider_run_id_issuer: self.provider_run_id_issuer(),
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Serialize)]
struct ActorSeal {
    session_token: SessionToken,
    agent_principal_id: AgentPrincipalId,
    agent_instance_id: AgentInstanceId,
    role: SessionRole,
    state: SessionState,
    runtime_attestation: RuntimeAttestationRow,
}

#[derive(Debug, Serialize)]
struct Blocker {
    code: ProofBlockerCode,
    message: ProofBlockerMessage,
}

impl Blocker {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Result<Self, String> {
        Ok(Self {
            code: ProofBlockerCode::parse(code.into())?,
            message: ProofBlockerMessage::parse(message.into())?,
        })
    }

    fn failed_check(code: &str) -> Self {
        Self::new(
            code.to_owned(),
            format!("required proof check failed: {code}"),
        )
        .expect("proof check blocker names are non-empty static check identifiers")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProofBlockerCode(String);

impl ProofBlockerCode {
    fn parse(value: String) -> Result<Self, String> {
        if value.trim().is_empty() {
            return Err("proof blocker code must not be empty".into());
        }
        if value.trim() != value {
            return Err(
                "proof blocker code must not include leading or trailing whitespace".into(),
            );
        }
        if value.chars().any(char::is_control) {
            return Err("proof blocker code must not contain control characters".into());
        }
        if !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            return Err(
                "proof blocker code must contain only ASCII letters, digits, or underscores".into(),
            );
        }
        Ok(Self(value))
    }
}

impl Serialize for ProofBlockerCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProofBlockerMessage(String);

impl ProofBlockerMessage {
    fn parse(value: String) -> Result<Self, String> {
        if value.trim().is_empty() {
            return Err("proof blocker message must not be empty".into());
        }
        if value.trim() != value {
            return Err(
                "proof blocker message must not include leading or trailing whitespace".into(),
            );
        }
        if value.chars().any(char::is_control) {
            return Err("proof blocker message must not contain control characters".into());
        }
        Ok(Self(value))
    }
}

impl Serialize for ProofBlockerMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

#[derive(Debug)]
struct LandingAuthorization {
    schema_version: &'static str,
    queue_id: QueueId,
    artifact_digest: ArtifactDigest,
    review_id: ReviewId,
    findings_digest: FindingsDigest,
    claim_fence_seq: FenceSeq,
    verifier: VerifierId,
    verdict_digest: ArtifactDigest,
    apply_verification_seal_digest: ArtifactDigest,
    seal_digest: ArtifactDigest,
    base_tree_oid: String,
    changed_paths_digest: ChangedPathsDigest,
    target_ref: String,
}

impl Serialize for LandingAuthorization {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut record = serializer.serialize_struct("LandingAuthorization", 14)?;
        record.serialize_field("schema_version", self.schema_version)?;
        record.serialize_field("accepted", &true)?;
        record.serialize_field("queue_id", &self.queue_id)?;
        record.serialize_field("artifact_digest", &self.artifact_digest)?;
        record.serialize_field("review_id", &self.review_id)?;
        record.serialize_field("findings_digest", &self.findings_digest)?;
        record.serialize_field("claim_fence_seq", &self.claim_fence_seq)?;
        record.serialize_field("verifier", &self.verifier)?;
        record.serialize_field("verdict_digest", &self.verdict_digest)?;
        record.serialize_field(
            "apply_verification_seal_digest",
            &self.apply_verification_seal_digest,
        )?;
        record.serialize_field("seal_digest", &self.seal_digest)?;
        record.serialize_field("base_tree_oid", &self.base_tree_oid)?;
        record.serialize_field("changed_paths_digest", &self.changed_paths_digest)?;
        record.serialize_field("target_ref", &self.target_ref)?;
        record.end()
    }
}

pub fn apply_proof_contract() -> Value {
    let required = ["covey_db", "evidence_dir", "mainline_ref", "output", "repo"];
    let optional = [
        "apply_gate_session_token",
        "apply_verification_seal_digest",
        "artifact_digest",
        "artifact_file",
        "enforce_promoted_mission_identity_contract",
        "forbidden_provider_run_id_issuer",
        "mission_packet_file",
        "queue_id",
        "require_host_signed_runtime_claims",
        "require_observed_process_ids",
        "require_provider_run_ids",
        "review_id",
        "reviewer_findings_digest",
        "subject_ref",
        "subtask_id",
        "success_file",
        "success_text",
        "target_ref",
        "trusted_provider_run_id_issuer",
        "verdict_digest",
        "verdict_file",
        "verifier",
    ];
    object([
        (
            "schema",
            Value::String("covey_apply_proof_verifier_contract".into()),
        ),
        ("request_schema", Value::String(REQUEST_SCHEMA.into())),
        ("required_request_fields", strings(required)),
        ("optional_request_fields", strings(optional)),
        (
            "default_request_fields",
            object([
                ("verifier", Value::String("mutai-rs".into())),
                ("artifact_file", Value::String("feature.patch".into())),
                (
                    "verdict_file",
                    Value::String("apply-gate-output.json".into()),
                ),
                (
                    "success_file",
                    Value::String("full-suite-output.txt".into()),
                ),
                (
                    "enforce_promoted_mission_identity_contract",
                    Value::Bool(false),
                ),
                ("require_observed_process_ids", Value::Bool(false)),
                ("require_host_signed_runtime_claims", Value::Bool(false)),
                ("require_provider_run_ids", Value::Bool(false)),
                ("trusted_provider_run_id_issuer", Value::Array(Vec::new())),
                ("forbidden_provider_run_id_issuer", Value::Array(Vec::new())),
            ]),
        ),
        (
            "accepted_output_fields",
            strings([
                "accepted",
                "seal_digest",
                "checks",
                "blockers",
                "covey_rows",
                "evidence_files",
                "landing_authorization",
            ]),
        ),
        (
            "rejected_output_fields",
            strings([
                "accepted",
                "seal_digest",
                "checks",
                "blockers",
                "covey_rows",
                "evidence_files",
            ]),
        ),
        ("blocker_fields", strings(["code", "message"])),
    ])
}

pub fn verify_apply_proof(args: ApplyProofVerifyArgs) -> Result<u8, ApplyProofError> {
    let request = VerifyRequest::from_args(args)?;
    let summary = verify_apply_request(&request)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(if summary["accepted"].as_bool().unwrap_or(false) {
        0
    } else {
        1
    })
}

fn verify_apply_request(req: &VerifyRequest) -> Result<Value, ApplyProofError> {
    let landing_path = req.evidence_dir.join("landing-proof.json");
    let landing = read_json_optional(&landing_path)?;
    let artifact_digest = resolve_landing_ref(
        req.artifact_digest.clone(),
        &landing,
        "artifact_digest",
        "missing artifact digest",
    )?;
    let review_id = resolve_landing_ref(
        req.review_id.clone(),
        &landing,
        "review_id",
        "missing review id",
    )?;
    let queue_id = resolve_landing_ref(
        req.queue_id.clone(),
        &landing,
        "queue_id",
        "missing queue id",
    )?;
    let findings_digest = resolve_landing_ref(
        req.reviewer_findings_digest.clone(),
        &landing,
        "reviewer_findings_digest",
        "missing reviewer findings digest",
    )?;

    let conn = Connection::open(&req.covey_db)?;
    let artifact = load_artifact(&conn, artifact_digest.as_str())?;
    let review = load_review(&conn, review_id.as_str())?;
    let queue = load_queue(&conn, queue_id.as_str())?;
    let applied_claim_fence_seq = queue.applied_claim_fence_seq();
    let (apply_verification, apply_verification_lookup_error) = match applied_claim_fence_seq {
        Some(claim_fence_seq) => {
            let apply_verification = load_apply_verification(
                &conn,
                queue_id.as_str(),
                artifact_digest.as_str(),
                review_id.as_str(),
                findings_digest.as_str(),
                claim_fence_seq,
                req.verifier.as_str(),
                req.verdict_digest.as_ref().map(ArtifactDigest::as_str),
                req.apply_verification_seal_digest
                    .as_ref()
                    .map(ArtifactDigest::as_str),
            )?;
            let lookup_error = if apply_verification.is_none() {
                Some("expected exactly one apply verification row, got 0".to_owned())
            } else {
                None
            };
            (apply_verification, lookup_error)
        }
        None => (
            None,
            Some("ready queue row is not applied; apply verification lookup skipped".to_owned()),
        ),
    };
    let producer = load_session(&conn, artifact.produced_by_session.as_str())?;
    let reviewer = load_session(&conn, &review.reviewer_session)?;
    let apply_gate_session_token = match &req.apply_gate_session_token {
        Some(token) => token.clone(),
        None => parse_proof_field(
            required_string(
                &read_json(&req.evidence_dir.join("apply-gate-identity.json"))?,
                "session_token",
            )?,
            "session_token",
        )?,
    };
    let apply_gate = load_session(&conn, apply_gate_session_token.as_str())?;
    let producer_attestation = load_runtime_attestation(&conn, &producer.session_token)?;
    let reviewer_attestation = load_runtime_attestation(&conn, &reviewer.session_token)?;
    let apply_gate_attestation = load_runtime_attestation(&conn, &apply_gate.session_token)?;

    let artifact_file = req.evidence_dir.join(req.artifact_file.as_str());
    let verdict_file = req.evidence_dir.join(req.verdict_file.as_str());
    let success_file = req.evidence_dir.join(req.success_file.as_str());
    let trusted_issuers = normalized_set(&req.trusted_provider_run_id_issuer);
    let mut forbidden_issuers = normalized_set(&req.forbidden_provider_run_id_issuer);

    let mut checks = BTreeMap::new();
    checks.insert(
        "artifact_digest_matches_expected".into(),
        artifact.artifact_digest == artifact_digest,
    );
    checks.insert(
        "review_targets_artifact".into(),
        review.artifact_digest == artifact_digest,
    );
    checks.insert(
        "review_targets_subtask".into(),
        review.subtask_id == artifact.produced_by_subtask_id,
    );
    checks.insert(
        "artifact_targets_subtask".into(),
        req.subtask_id
            .as_ref()
            .is_none_or(|id| artifact.produced_by_subtask_id == *id),
    );
    checks.insert(
        "review_targets_requested_subtask".into(),
        req.subtask_id
            .as_ref()
            .is_none_or(|id| review.subtask_id == *id),
    );
    checks.insert(
        "queue_targets_requested_subtask".into(),
        req.subtask_id
            .as_ref()
            .is_none_or(|id| queue.subtask_id == *id),
    );
    checks.insert("review_approved".into(), review.approved());
    checks.insert(
        "review_findings_digest_matches".into(),
        review
            .findings_digest()
            .is_some_and(|digest| digest == findings_digest.as_str()),
    );
    checks.insert(
        "queue_targets_artifact".into(),
        queue.artifact_digest == artifact_digest,
    );
    checks.insert(
        "queue_targets_reviewed_subtask".into(),
        queue.subtask_id == review.subtask_id,
    );
    checks.insert(
        "queue_applied".into(),
        queue.state() == ReadyQueueState::Applied,
    );
    checks.insert(
        "apply_verification_recorded".into(),
        apply_verification.is_some(),
    );
    if let Some(apply) = &apply_verification {
        checks.insert(
            "apply_verification_targets_queue".into(),
            apply.queue_id == queue_id,
        );
        checks.insert(
            "apply_verification_targets_artifact".into(),
            apply.artifact_digest == artifact_digest,
        );
        checks.insert(
            "apply_verification_targets_review".into(),
            apply.review_id == review_id,
        );
        checks.insert(
            "apply_verification_targets_findings".into(),
            apply.findings_digest == findings_digest,
        );
        checks.insert(
            "apply_verification_targets_fence".into(),
            Some(apply.claim_fence_seq.get()) == applied_claim_fence_seq,
        );
        checks.insert(
            "apply_verification_uses_expected_verifier".into(),
            apply.verifier == req.verifier,
        );
        checks.insert(
            "apply_verification_recorded_by_apply_gate".into(),
            apply.recorded_by_session == apply_gate.session_token,
        );
        checks.insert(
            "apply_verification_verdict_digest_matches_file".into(),
            verdict_file.is_file() && apply.verdict_digest == blake3_file(&verdict_file)?,
        );
        checks.insert(
            "apply_verification_verdict_digest_matches_requested".into(),
            req.verdict_digest
                .as_ref()
                .is_none_or(|digest| apply.verdict_digest == *digest),
        );
        checks.insert(
            "apply_verification_seal_digest_matches_requested".into(),
            req.apply_verification_seal_digest
                .as_ref()
                .is_none_or(|digest| apply.seal_digest == *digest),
        );
    } else {
        for name in [
            "apply_verification_targets_queue",
            "apply_verification_targets_artifact",
            "apply_verification_targets_review",
            "apply_verification_targets_findings",
            "apply_verification_targets_fence",
            "apply_verification_uses_expected_verifier",
            "apply_verification_recorded_by_apply_gate",
            "apply_verification_verdict_digest_matches_file",
            "apply_verification_verdict_digest_matches_requested",
            "apply_verification_seal_digest_matches_requested",
        ] {
            checks.insert(name.into(), false);
        }
    }
    checks.insert(
        "worker_role_verified".into(),
        producer.role == SessionRole::Executor,
    );
    checks.insert(
        "reviewer_role_verified".into(),
        reviewer.role == SessionRole::Reviewer,
    );
    checks.insert(
        "apply_gate_role_verified".into(),
        apply_gate.role == SessionRole::ApplyGate,
    );
    checks.insert(
        "worker_runtime_attestation_matches".into(),
        attestation_matches(&producer, &producer_attestation),
    );
    checks.insert(
        "reviewer_runtime_attestation_matches".into(),
        attestation_matches(&reviewer, &reviewer_attestation),
    );
    checks.insert(
        "apply_gate_runtime_attestation_matches".into(),
        attestation_matches(&apply_gate, &apply_gate_attestation),
    );
    checks.insert(
        "worker_reviewer_principals_differ".into(),
        producer.agent_principal_id != reviewer.agent_principal_id,
    );
    checks.insert(
        "worker_apply_gate_principals_differ".into(),
        producer.agent_principal_id != apply_gate.agent_principal_id,
    );
    checks.insert(
        "reviewer_apply_gate_principals_differ".into(),
        reviewer.agent_principal_id != apply_gate.agent_principal_id,
    );
    checks.insert(
        "worker_reviewer_runtime_refs_differ".into(),
        runtime_ref(&producer_attestation) != runtime_ref(&reviewer_attestation),
    );
    checks.insert(
        "worker_apply_gate_runtime_refs_differ".into(),
        runtime_ref(&producer_attestation) != runtime_ref(&apply_gate_attestation),
    );
    checks.insert(
        "reviewer_apply_gate_runtime_refs_differ".into(),
        runtime_ref(&reviewer_attestation) != runtime_ref(&apply_gate_attestation),
    );
    checks.insert(
        "worker_reviewer_transcript_digests_differ".into(),
        producer_attestation.command_transcript_digest
            != reviewer_attestation.command_transcript_digest,
    );
    checks.insert(
        "worker_apply_gate_transcript_digests_differ".into(),
        producer_attestation.command_transcript_digest
            != apply_gate_attestation.command_transcript_digest,
    );
    checks.insert(
        "reviewer_apply_gate_transcript_digests_differ".into(),
        reviewer_attestation.command_transcript_digest
            != apply_gate_attestation.command_transcript_digest,
    );
    checks.insert(
        "reviewer_findings_file_digest_matches".into(),
        blake3_file(&req.evidence_dir.join("reviewer-findings.json")).unwrap_or_default()
            == findings_digest,
    );
    checks.insert(
        "artifact_file_digest_matches_artifact".into(),
        artifact_file.is_file() && blake3_file(&artifact_file)? == artifact_digest,
    );
    checks.insert(
        "success_text_found".into(),
        req.success_text.as_ref().is_none_or(|text| {
            fs::read_to_string(&success_file)
                .map(|contents| contents.contains(text.as_str()))
                .unwrap_or(false)
        }),
    );

    let mut mission_contract = None;
    if req.mission_packet_file.is_some() || req.enforce_promoted_mission_identity_contract {
        let path = resolve_optional_file(&req.evidence_dir, req.mission_packet_file.as_ref());
        let (mission_checks, contract, mission_forbidden) =
            mission_packet_identity_contract_checks(
                &path,
                req.enforce_promoted_mission_identity_contract,
            )?;
        checks.extend(mission_checks);
        forbidden_issuers.extend(mission_forbidden);
        mission_contract = contract;
        if req.enforce_promoted_mission_identity_contract {
            checks.insert(
                "mission_contract_trusted_provider_run_id_issuer_configured".into(),
                !trusted_issuers.is_empty(),
            );
        }
    }
    if req.require_observed_process_ids {
        checks.insert(
            "worker_observed_process_id".into(),
            observed_process_id(&producer_attestation),
        );
        checks.insert(
            "reviewer_observed_process_id".into(),
            observed_process_id(&reviewer_attestation),
        );
        checks.insert(
            "apply_gate_observed_process_id".into(),
            observed_process_id(&apply_gate_attestation),
        );
    }

    let require_provider_run_ids = req.require_provider_run_ids
        || !trusted_issuers.is_empty()
        || !forbidden_issuers.is_empty()
        || req.enforce_promoted_mission_identity_contract;
    let mut host_claims = BTreeMap::new();
    if req.require_host_signed_runtime_claims || require_provider_run_ids {
        host_claims.insert(
            "worker".to_owned(),
            verify_host_signed_runtime_claim(
                &req.evidence_dir.join("worker-runtime-claim.json"),
                "worker",
                &producer_attestation,
            )?,
        );
        host_claims.insert(
            "reviewer".to_owned(),
            verify_host_signed_runtime_claim(
                &req.evidence_dir.join("reviewer-runtime-claim.json"),
                "reviewer",
                &reviewer_attestation,
            )?,
        );
        host_claims.insert(
            "apply_gate".to_owned(),
            verify_host_signed_runtime_claim(
                &req.evidence_dir.join("apply-gate-runtime-claim.json"),
                "apply_gate",
                &apply_gate_attestation,
            )?,
        );
        checks.insert(
            "worker_host_signed_runtime_claim".into(),
            claim_accepted(host_claims.get("worker")),
        );
        checks.insert(
            "reviewer_host_signed_runtime_claim".into(),
            claim_accepted(host_claims.get("reviewer")),
        );
        checks.insert(
            "apply_gate_host_signed_runtime_claim".into(),
            claim_accepted(host_claims.get("apply_gate")),
        );
        let signers: BTreeSet<String> = host_claims
            .values()
            .filter_map(|claim| claim.get("public_key_blake3").and_then(Value::as_str))
            .map(ToOwned::to_owned)
            .collect();
        checks.insert(
            "host_signed_runtime_claims_share_signer".into(),
            signers.len() == 1,
        );
    }
    if require_provider_run_ids {
        checks.insert(
            "worker_provider_run_id".into(),
            provider_run_id_present(host_claims.get("worker")),
        );
        checks.insert(
            "reviewer_provider_run_id".into(),
            provider_run_id_present(host_claims.get("reviewer")),
        );
        checks.insert(
            "apply_gate_provider_run_id".into(),
            provider_run_id_present(host_claims.get("apply_gate")),
        );
    }
    if !trusted_issuers.is_empty() {
        checks.insert(
            "worker_provider_run_id_trusted_issuer".into(),
            provider_run_id_issuer_trusted(host_claims.get("worker"), &trusted_issuers),
        );
        checks.insert(
            "reviewer_provider_run_id_trusted_issuer".into(),
            provider_run_id_issuer_trusted(host_claims.get("reviewer"), &trusted_issuers),
        );
        checks.insert(
            "apply_gate_provider_run_id_trusted_issuer".into(),
            provider_run_id_issuer_trusted(host_claims.get("apply_gate"), &trusted_issuers),
        );
    }
    if !forbidden_issuers.is_empty() {
        checks.insert(
            "worker_provider_run_id_forbidden_issuer_absent".into(),
            provider_run_id_issuer_not_forbidden(host_claims.get("worker"), &forbidden_issuers),
        );
        checks.insert(
            "reviewer_provider_run_id_forbidden_issuer_absent".into(),
            provider_run_id_issuer_not_forbidden(host_claims.get("reviewer"), &forbidden_issuers),
        );
        checks.insert(
            "apply_gate_provider_run_id_forbidden_issuer_absent".into(),
            provider_run_id_issuer_not_forbidden(host_claims.get("apply_gate"), &forbidden_issuers),
        );
    }

    let head = git(&req.repo, ["rev-parse", "HEAD"])?;
    let mainline = git(&req.repo, ["rev-parse", req.mainline_ref.as_str()])?;
    let merge_base = git(
        &req.repo,
        ["merge-base", head.as_str(), req.mainline_ref.as_str()],
    )?;
    checks.insert("head_reachable_from_mainline".into(), merge_base == head);
    checks.insert("mainline_matches_head".into(), mainline == head);
    let mut landing_summary = None;
    if landing_path.exists() {
        let commit_hash_claimed = landing.get("commit_hash").is_some();
        let commit_hash = landing
            .get("commit_hash")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        checks.insert(
            "landing_proof_commit_hash_absent_or_non_empty".into(),
            !commit_hash_claimed || commit_hash.as_ref().is_some_and(|value| !value.is_empty()),
        );
        let resolved = commit_hash
            .as_ref()
            .and_then(|hash| git(&req.repo, ["rev-parse", hash.as_str()]).ok());
        checks.insert(
            "landing_proof_commit_hash_absent_or_resolves".into(),
            !commit_hash_claimed || resolved.is_some(),
        );
        checks.insert(
            "landing_proof_commit_hash_absent_or_matches_head".into(),
            !commit_hash_claimed || resolved.as_deref() == Some(head.as_str()),
        );
        checks.insert(
            "landing_proof_commit_hash_absent_or_matches_mainline".into(),
            !commit_hash_claimed || resolved.as_deref() == Some(mainline.as_str()),
        );
        landing_summary = Some(object([
            ("path", Value::String(landing_path.display().to_string())),
            ("commit_hash_claimed", Value::Bool(commit_hash_claimed)),
            ("commit_hash", option_string(commit_hash)),
            ("resolved_commit", option_string(resolved)),
            (
                "branch",
                landing.get("branch").cloned().unwrap_or(Value::Null),
            ),
            (
                "initial_commit_hash",
                landing
                    .get("initial_commit_hash")
                    .cloned()
                    .unwrap_or(Value::Null),
            ),
        ]));
    }
    let mut subject_commit = None;
    if let Some(subject_ref) = &req.subject_ref {
        let subject = git(&req.repo, ["rev-parse", subject_ref.as_str()])?;
        let subject_merge_base = git(
            &req.repo,
            ["merge-base", subject.as_str(), req.mainline_ref.as_str()],
        )?;
        checks.insert(
            "subject_reachable_from_mainline".into(),
            subject_merge_base == subject,
        );
        subject_commit = Some(subject);
    }
    checks.insert(
        "worktree_clean".into(),
        git(&req.repo, ["status", "--short"])?.is_empty(),
    );

    let blockers = check_blockers(&checks);
    let mut manifest = object([
        ("schema", Value::String(SEAL_SCHEMA.into())),
        ("accepted", Value::Bool(blockers.is_empty())),
        ("repo", Value::String(req.repo.display().to_string())),
        (
            "covey_db",
            Value::String(req.covey_db.display().to_string()),
        ),
        ("mainline_ref", Value::String(req.mainline_ref.to_string())),
        ("head", Value::String(head.clone())),
        ("mainline_commit", Value::String(mainline.clone())),
        (
            "subtask_id",
            option_string(req.subtask_id.clone().map(String::from)),
        ),
        (
            "artifact_digest",
            Value::String(artifact_digest.to_string()),
        ),
        ("review_id", Value::String(review_id.to_string())),
        ("queue_id", Value::String(queue_id.to_string())),
        (
            "actors",
            object([
                (
                    "worker",
                    serde_json::to_value(actor(&producer, &producer_attestation))?,
                ),
                (
                    "reviewer",
                    serde_json::to_value(actor(&reviewer, &reviewer_attestation))?,
                ),
                (
                    "apply_gate",
                    serde_json::to_value(actor(&apply_gate, &apply_gate_attestation))?,
                ),
            ]),
        ),
        (
            "covey_rows",
            object([
                ("artifact", serde_json::to_value(&artifact)?),
                ("review", serde_json::to_value(&review)?),
                ("ready_queue", serde_json::to_value(&queue)?),
                (
                    "apply_verification",
                    apply_verification
                        .as_ref()
                        .map(serde_json::to_value)
                        .transpose()?
                        .unwrap_or(Value::Null),
                ),
            ]),
        ),
        (
            "apply_verification_lookup_error",
            option_string(apply_verification_lookup_error),
        ),
        ("checks", serde_json::to_value(&checks)?),
        ("blockers", serde_json::to_value(&blockers)?),
        (
            "evidence_files",
            serde_json::to_value(file_digests(&req.evidence_dir, &req.output)?)?,
        ),
    ]);
    if let Some(summary) = landing_summary {
        insert_object(&mut manifest, "landing_proof", summary);
    }
    if let Some(subject) = subject_commit {
        insert_object(
            &mut manifest,
            "subject_ref",
            option_string(req.subject_ref.clone().map(|value| value.to_string())),
        );
        insert_object(&mut manifest, "subject_commit", Value::String(subject));
    }
    if !host_claims.is_empty() {
        insert_object(
            &mut manifest,
            "host_signed_runtime_claims",
            serde_json::to_value(host_claims)?,
        );
    }
    if let Some(contract) = mission_contract {
        insert_object(&mut manifest, "mission_packet_identity_contract", contract);
    }
    if !trusted_issuers.is_empty() {
        insert_object(
            &mut manifest,
            "trusted_provider_run_id_issuers",
            strings(trusted_issuers.iter().map(String::as_str)),
        );
    }
    if !forbidden_issuers.is_empty() {
        insert_object(
            &mut manifest,
            "forbidden_provider_run_id_issuers",
            strings(forbidden_issuers.iter().map(String::as_str)),
        );
    }

    let seal_digest = ArtifactDigest::parse(blake3_prefixed_bytes(&canonical_json(&manifest)))
        .map_err(|error| {
            ApplyProofError::Verification(format!("generated seal_digest is invalid: {error}"))
        })?;
    insert_object(
        &mut manifest,
        "seal_digest",
        Value::String(seal_digest.to_string()),
    );
    if blockers.is_empty()
        && let Some(apply) = &apply_verification
    {
        let target_ref = req
            .target_ref
            .clone()
            .unwrap_or_else(|| req.mainline_ref.clone());
        let base_tree_oid =
            artifact_base_tree_oid(&req.repo, &artifact, req.mainline_ref.as_str())?;
        let auth = LandingAuthorization {
            schema_version: "codex_hook_landing_authorization.v1",
            queue_id: queue_id.clone(),
            artifact_digest: artifact_digest.clone(),
            review_id: review_id.clone(),
            findings_digest: findings_digest.clone(),
            claim_fence_seq: apply.claim_fence_seq,
            verifier: apply.verifier.clone(),
            verdict_digest: apply.verdict_digest.clone(),
            apply_verification_seal_digest: apply.seal_digest.clone(),
            seal_digest: seal_digest.clone(),
            base_tree_oid,
            changed_paths_digest: artifact.changed_paths_digest.clone(),
            target_ref: target_ref.to_string(),
        };
        insert_object(
            &mut manifest,
            "landing_authorization",
            serde_json::to_value(auth)?,
        );
    }

    write_json_file(&req.output, &manifest)?;
    Ok(object([
        ("accepted", Value::Bool(blockers.is_empty())),
        ("blockers", serde_json::to_value(&blockers)?),
        ("seal", Value::String(req.output.display().to_string())),
        ("seal_digest", Value::String(seal_digest.to_string())),
    ]))
}

pub fn verify_apply_proof_batch(args: ApplyProofBatchArgs) -> Result<u8, ApplyProofError> {
    let manifest_path = args.manifest.canonicalize()?;
    let output = absolute(&args.output)?;
    let seal_dir = args
        .seal_dir
        .clone()
        .map(|path| absolute(&path))
        .transpose()?
        .unwrap_or_else(|| {
            output.with_file_name(format!(
                "{}-seals",
                output.file_stem().unwrap().to_string_lossy()
            ))
        });
    fs::create_dir_all(&seal_dir)?;
    let request = read_json(&manifest_path)?;
    let proofs = request
        .get("proofs")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ApplyProofError::Verification(
                "batch manifest must contain a non-empty proofs list".into(),
            )
        })?;
    if proofs.is_empty() {
        return Err(ApplyProofError::Verification(
            "batch manifest must contain a non-empty proofs list".into(),
        ));
    }
    let mut results = Vec::new();
    for proof in proofs {
        let proof_id = required_string(proof, "id")?;
        let child_output = seal_dir.join(format!("{proof_id}.seal.json"));
        let mut child_args = batch_child_args(proof, &child_output)?;
        if args.require_observed_process_ids {
            child_args.require_observed_process_ids = true;
        }
        if args.require_host_signed_runtime_claims {
            child_args.require_host_signed_runtime_claims = true;
        }
        if args.require_provider_run_ids {
            child_args.require_provider_run_ids = true;
        }
        if args.enforce_promoted_mission_identity_contract {
            child_args.enforce_promoted_mission_identity_contract = true;
        }
        child_args
            .trusted_provider_run_id_issuer
            .extend(args.trusted_provider_run_id_issuer.clone());
        child_args
            .forbidden_provider_run_id_issuer
            .extend(args.forbidden_provider_run_id_issuer.clone());
        let child_request = VerifyRequest::from_args(child_args)?;
        let summary = verify_apply_request(&child_request)?;
        let accepted = summary["accepted"].as_bool().unwrap_or(false);
        let child_manifest = read_json_optional(&child_output)?;
        results.push(object([
            ("id", Value::String(proof_id)),
            ("accepted", Value::Bool(accepted)),
            (
                "returncode",
                Value::Number((if accepted { 0 } else { 1 }).into()),
            ),
            ("seal", Value::String(child_output.display().to_string())),
            (
                "seal_digest",
                summary
                    .get("seal_digest")
                    .cloned()
                    .or_else(|| child_manifest.get("seal_digest").cloned())
                    .unwrap_or(Value::Null),
            ),
            (
                "seal_file_blake3",
                if child_output.exists() {
                    Value::String(blake3_file(&child_output)?)
                } else {
                    Value::Null
                },
            ),
            (
                "blockers",
                summary
                    .get("blockers")
                    .cloned()
                    .or_else(|| child_manifest.get("blockers").cloned())
                    .unwrap_or_else(|| Value::Array(Vec::new())),
            ),
            ("stderr", Value::String(String::new())),
        ]));
    }
    let accepted = results
        .iter()
        .all(|result| result["accepted"].as_bool() == Some(true));
    let mut aggregate = object([
        ("schema", Value::String(BATCH_SEAL_SCHEMA.into())),
        ("accepted", Value::Bool(accepted)),
        (
            "manifest",
            Value::String(manifest_path.display().to_string()),
        ),
        ("proof_count", Value::Number(results.len().into())),
        ("proofs", Value::Array(results)),
    ]);
    let digest = blake3_prefixed_bytes(&canonical_json(&aggregate));
    insert_object(&mut aggregate, "seal_digest", Value::String(digest.clone()));
    write_json_file(&output, &aggregate)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&object([
            ("accepted", Value::Bool(accepted)),
            ("proof_count", Value::Number(proofs.len().into())),
            ("seal", Value::String(output.display().to_string())),
            ("seal_digest", Value::String(digest)),
        ]))?
    );
    Ok(if accepted { 0 } else { 1 })
}

pub fn emit_apply_proof_error(error: ApplyProofError, output: Option<&Path>) -> u8 {
    let code = match error {
        ApplyProofError::Request { ref message, .. } => ("request_invalid", message.clone()),
        other => ("proof_verification_error", other.to_string()),
    };
    let mut manifest = object([
        ("schema", Value::String(SEAL_SCHEMA.into())),
        ("accepted", Value::Bool(false)),
        (
            "blockers",
            Value::Array(vec![object([
                ("code", Value::String(code.0.into())),
                ("message", Value::String(code.1)),
            ])]),
        ),
    ]);
    let digest = blake3_prefixed_bytes(&canonical_json(&manifest));
    insert_object(&mut manifest, "seal_digest", Value::String(digest.clone()));
    if let Some(path) = output {
        let _ = write_json_file(path, &manifest);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&object([
            ("accepted", Value::Bool(false)),
            ("blockers", manifest["blockers"].clone()),
            (
                "seal",
                output
                    .map(|path| Value::String(path.display().to_string()))
                    .unwrap_or(Value::Null),
            ),
            ("seal_digest", Value::String(digest)),
        ]))
        .expect("error json")
    );
    1
}

impl VerifyRequest {
    fn from_args(mut args: ApplyProofVerifyArgs) -> Result<Self, ApplyProofError> {
        if let Some(input) = &args.input {
            let file: VerifyRequestFile = serde_json::from_str(&fs::read_to_string(input)?)?;
            if let Some(schema) = &file.schema
                && schema != REQUEST_SCHEMA
            {
                return Err(request_error(
                    format!("--input schema must be {REQUEST_SCHEMA}"),
                    file.output.or(args.output),
                ));
            }
            args.repo = args.repo.or(file.repo);
            args.covey_db = args.covey_db.or(file.covey_db);
            args.evidence_dir = args.evidence_dir.or(file.evidence_dir);
            args.subtask_id = args.subtask_id.or(file.subtask_id);
            args.artifact_digest = args.artifact_digest.or(file.artifact_digest);
            args.review_id = args.review_id.or(file.review_id);
            args.queue_id = args.queue_id.or(file.queue_id);
            args.reviewer_findings_digest = args
                .reviewer_findings_digest
                .or(file.reviewer_findings_digest);
            args.apply_gate_session_token = args
                .apply_gate_session_token
                .or(file.apply_gate_session_token);
            if args.verifier == "mutai-rs"
                && let Some(verifier) = file.verifier
            {
                args.verifier = verifier;
            }
            args.verdict_digest = args.verdict_digest.or(file.verdict_digest);
            args.apply_verification_seal_digest = args
                .apply_verification_seal_digest
                .or(file.apply_verification_seal_digest);
            args.mainline_ref = args.mainline_ref.or(file.mainline_ref);
            args.subject_ref = args.subject_ref.or(file.subject_ref);
            if args.artifact_file.as_str() == "feature.patch"
                && let Some(value) = file.artifact_file
            {
                args.artifact_file = value;
            }
            if args.verdict_file.as_str() == "apply-gate-output.json"
                && let Some(value) = file.verdict_file
            {
                args.verdict_file = value;
            }
            if args.success_file.as_str() == "full-suite-output.txt"
                && let Some(value) = file.success_file
            {
                args.success_file = value;
            }
            args.success_text = args.success_text.or(file.success_text);
            args.mission_packet_file = args.mission_packet_file.or(file.mission_packet_file);
            args.enforce_promoted_mission_identity_contract |= file
                .enforce_promoted_mission_identity_contract
                .unwrap_or(false);
            args.require_observed_process_ids |= file.require_observed_process_ids.unwrap_or(false);
            args.require_host_signed_runtime_claims |=
                file.require_host_signed_runtime_claims.unwrap_or(false);
            args.require_provider_run_ids |= file.require_provider_run_ids.unwrap_or(false);
            if args.trusted_provider_run_id_issuer.is_empty() {
                args.trusted_provider_run_id_issuer =
                    file.trusted_provider_run_id_issuer.unwrap_or_default();
            }
            if args.forbidden_provider_run_id_issuer.is_empty() {
                args.forbidden_provider_run_id_issuer =
                    file.forbidden_provider_run_id_issuer.unwrap_or_default();
            }
            args.target_ref = args.target_ref.or(file.target_ref);
            args.output = args.output.or(file.output);
        }
        let output = require_path(args.output.clone(), "output")?;
        let error_output = Some(output.clone());
        Ok(Self {
            repo: absolute(&require_path_with_output(
                args.repo,
                "repo",
                error_output.clone(),
            )?)?,
            covey_db: absolute(&require_path_with_output(
                args.covey_db,
                "covey-db",
                error_output.clone(),
            )?)?,
            evidence_dir: absolute(&require_path_with_output(
                args.evidence_dir,
                "evidence-dir",
                error_output.clone(),
            )?)?,
            subtask_id: args.subtask_id,
            artifact_digest: args.artifact_digest,
            review_id: args.review_id,
            queue_id: args.queue_id,
            reviewer_findings_digest: args.reviewer_findings_digest,
            apply_gate_session_token: args.apply_gate_session_token,
            verifier: args.verifier,
            verdict_digest: args.verdict_digest,
            apply_verification_seal_digest: args.apply_verification_seal_digest,
            mainline_ref: require_git_ref_with_output(
                args.mainline_ref,
                "mainline-ref",
                error_output,
            )?,
            subject_ref: args.subject_ref,
            artifact_file: args.artifact_file,
            verdict_file: args.verdict_file,
            success_file: args.success_file,
            success_text: args.success_text,
            mission_packet_file: args.mission_packet_file,
            enforce_promoted_mission_identity_contract: args
                .enforce_promoted_mission_identity_contract,
            require_observed_process_ids: args.require_observed_process_ids,
            require_host_signed_runtime_claims: args.require_host_signed_runtime_claims,
            require_provider_run_ids: args.require_provider_run_ids,
            trusted_provider_run_id_issuer: args.trusted_provider_run_id_issuer,
            forbidden_provider_run_id_issuer: args.forbidden_provider_run_id_issuer,
            target_ref: args.target_ref,
            output: absolute(&output)?,
        })
    }
}

fn request_error(message: String, output: Option<PathBuf>) -> ApplyProofError {
    ApplyProofError::Request { message, output }
}

fn require_path(path: Option<PathBuf>, name: &str) -> Result<PathBuf, ApplyProofError> {
    path.ok_or_else(|| request_error(format!("--{name} is required"), None))
}

fn require_path_with_output(
    path: Option<PathBuf>,
    name: &str,
    output: Option<PathBuf>,
) -> Result<PathBuf, ApplyProofError> {
    path.ok_or_else(|| request_error(format!("--{name} is required"), output))
}

fn require_git_ref_with_output(
    value: Option<GitRef>,
    name: &str,
    output: Option<PathBuf>,
) -> Result<GitRef, ApplyProofError> {
    value.ok_or_else(|| request_error(format!("--{name} is required"), output))
}

fn parse_proof_field<T>(value: String, field: &str) -> Result<T, ApplyProofError>
where
    T: TryFrom<String>,
    T::Error: std::fmt::Display,
{
    T::try_from(value).map_err(|error| {
        ApplyProofError::Verification(format!("invalid typed Covey scalar field {field}: {error}"))
    })
}

fn parse_proof_enum<T>(value: String, field: &str) -> Result<T, ApplyProofError>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    T::from_str(&value)
        .map_err(|error| ApplyProofError::Verification(format!("invalid {field}: {error}")))
}

fn parse_review_state(raw_state: String) -> Result<ReviewState, ApplyProofError> {
    ReviewState::from_str(&raw_state).map_err(|err| {
        ApplyProofError::Verification(format!("invalid review state in proof row: {err}"))
    })
}

fn parse_ready_queue_state(raw_state: String) -> Result<ReadyQueueState, ApplyProofError> {
    ReadyQueueState::from_str(&raw_state).map_err(|error| {
        ApplyProofError::Verification(format!("ready queue row has invalid state: {error}"))
    })
}

fn parse_timestamp_ms(value: i64, field: &str) -> Result<TimestampMs, ApplyProofError> {
    TimestampMs::parse(value)
        .map_err(|error| ApplyProofError::Verification(format!("invalid {field}: {error}")))
}

fn resolve_landing_ref<T>(
    explicit: Option<T>,
    landing: &Value,
    field: &str,
    missing_message: &str,
) -> Result<T, ApplyProofError>
where
    T: TryFrom<String>,
    T::Error: std::fmt::Display,
{
    match explicit {
        Some(value) => Ok(value),
        None => match required_string(landing, field) {
            Ok(value) => parse_proof_field(value, field),
            Err(_) => Err(ApplyProofError::Verification(missing_message.into())),
        },
    }
}

fn absolute(path: &Path) -> Result<PathBuf, ApplyProofError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn load_artifact(conn: &Connection, digest: &str) -> Result<ArtifactRow, ApplyProofError> {
    conn.query_row(
        "SELECT artifact_digest, produced_by_subtask_id, produced_by_session, manifest_path, changed_paths_digest FROM artifacts WHERE artifact_digest = ?1",
        params![digest],
        |row| {
            ArtifactRow::from_db_parts(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            )
            .map_err(|err| rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(err),
            ))
        },
    )
    .map_err(Into::into)
}

fn artifact_base_tree_oid(
    repo: &Path,
    artifact: &ArtifactRow,
    fallback_ref: &str,
) -> Result<String, ApplyProofError> {
    let manifest_path = PathBuf::from(artifact.manifest_path.as_str());
    let base_rev = read_json(&manifest_path)
        .ok()
        .and_then(|manifest| artifact_manifest_base_tree_rev(&manifest))
        .unwrap_or_else(|| fallback_ref.to_owned());
    let base_tree_rev = format!("{base_rev}^{{tree}}");
    git(repo, ["rev-parse", base_tree_rev.as_str()])
}

fn artifact_manifest_base_tree_rev(manifest: &Value) -> Option<String> {
    if manifest
        .get("schema")
        .and_then(Value::as_str)
        .is_some_and(|schema| schema == "mutai_scheduler_patch_artifact.v1")
        && let Some(baseline_tree) = manifest.get("baseline_tree").and_then(Value::as_str)
        && !baseline_tree.is_empty()
    {
        return Some(baseline_tree.to_owned());
    }
    manifest
        .get("base_rev")
        .and_then(Value::as_str)
        .filter(|base_rev| !base_rev.is_empty())
        .map(ToOwned::to_owned)
}

fn load_review(conn: &Connection, review_id: &str) -> Result<ReviewRow, ApplyProofError> {
    conn.query_row(
        "SELECT review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id, verdict, findings_digest, state FROM reviews WHERE review_id = ?1",
        params![review_id],
        |row| {
            let state = parse_review_state(row.get(7)?).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    7,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?;
            let decision_evidence = ReviewProofDecisionEvidence::from_raw_for_state(
                state,
                row.get(5)?,
                row.get(6)?,
            )
            .map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?;
            ReviewRow::from_db_parts(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                state,
                decision_evidence,
            )
            .map_err(|err| rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(err),
            ))
        },
    )
    .map_err(Into::into)
}

fn load_queue(conn: &Connection, queue_id: &str) -> Result<ReadyQueueRow, ApplyProofError> {
    conn.query_row(
        "SELECT queue_id, artifact_digest, subtask_id, state, claimed_by_session_token, claim_fence_seq, claim_lease_deadline FROM ready_queue WHERE queue_id = ?1",
        params![queue_id],
        |row| {
            let state = parse_ready_queue_state(row.get(3)?).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?;
            let claim_evidence = ReadyQueueProofClaimEvidence::from_raw_for_state(
                state,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            )
            .map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    4,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?;
            ReadyQueueRow::from_db_parts(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                state,
                claim_evidence,
            )
            .map_err(|err| rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(err),
            ))
        },
    )
    .map_err(Into::into)
}

#[allow(clippy::too_many_arguments)]
fn load_apply_verification(
    conn: &Connection,
    queue_id: &str,
    artifact_digest: &str,
    review_id: &str,
    findings_digest: &str,
    claim_fence_seq: i64,
    verifier: &str,
    verdict_digest: Option<&str>,
    seal_digest: Option<&str>,
) -> Result<Option<ApplyVerificationRow>, ApplyProofError> {
    let mut query = "SELECT queue_id, artifact_digest, review_id, findings_digest, claim_fence_seq, verifier, verdict_digest, seal_digest, recorded_by_session, created_at FROM apply_verifications WHERE queue_id = ?1 AND artifact_digest = ?2 AND review_id = ?3 AND findings_digest = ?4 AND claim_fence_seq = ?5 AND verifier = ?6".to_owned();
    if verdict_digest.is_some() {
        query.push_str(" AND verdict_digest = ?7");
    }
    if seal_digest.is_some() {
        query.push_str(if verdict_digest.is_some() {
            " AND seal_digest = ?8"
        } else {
            " AND seal_digest = ?7"
        });
    }
    let mut stmt = conn.prepare(&query)?;
    let mut rows = match (verdict_digest, seal_digest) {
        (Some(verdict), Some(seal)) => stmt.query(params![
            queue_id,
            artifact_digest,
            review_id,
            findings_digest,
            claim_fence_seq,
            verifier,
            verdict,
            seal
        ])?,
        (Some(verdict), None) => stmt.query(params![
            queue_id,
            artifact_digest,
            review_id,
            findings_digest,
            claim_fence_seq,
            verifier,
            verdict
        ])?,
        (None, Some(seal)) => stmt.query(params![
            queue_id,
            artifact_digest,
            review_id,
            findings_digest,
            claim_fence_seq,
            verifier,
            seal
        ])?,
        (None, None) => stmt.query(params![
            queue_id,
            artifact_digest,
            review_id,
            findings_digest,
            claim_fence_seq,
            verifier
        ])?,
    };
    let mut found = Vec::new();
    while let Some(row) = rows.next()? {
        found.push(apply_verification_from_row(row)?);
    }
    Ok(if found.len() == 1 { found.pop() } else { None })
}

fn apply_verification_from_row(row: &Row<'_>) -> rusqlite::Result<ApplyVerificationRow> {
    ApplyVerificationRow::from_db_parts(
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
    )
    .map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
    })
}

fn load_session(conn: &Connection, token: &str) -> Result<SessionRow, ApplyProofError> {
    conn.query_row(
        "SELECT session_token, agent_principal_id, agent_instance_id, role, state FROM sessions WHERE session_token = ?1",
        params![token],
        |row| {
            session_from_raw_row_parts(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            )
            .map_err(|err| rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(err),
            ))
        },
    )
    .map_err(Into::into)
}

fn session_from_raw_row_parts(
    session_token: String,
    agent_principal_id: String,
    agent_instance_id: String,
    raw_role: String,
    raw_state: String,
) -> Result<SessionRow, ApplyProofError> {
    let role = parse_proof_enum(raw_role, "role")?;
    let state = parse_proof_enum(raw_state, "state")?;
    SessionRow::from_db_parts(
        session_token,
        agent_principal_id,
        agent_instance_id,
        role,
        state,
    )
}

/// Replays session proof row parsing for external formal-model tests.
///
/// This is not a scheduling or settlement API. It exists so integration tests
/// can bind Quint traces to the same private row constructor used by apply
/// proof verification.
#[doc(hidden)]
#[must_use]
pub fn session_proof_row_accepts_for_model(
    session_token_valid: bool,
    agent_principal_valid: bool,
    agent_instance_valid: bool,
    role_shape: &str,
    state_shape: &str,
    role_value: &str,
    session_state_value: &str,
) -> bool {
    let raw_role = if role_shape == "InvalidRoleShape" {
        "worker"
    } else {
        match role_value {
            "ReviewerRole" => "reviewer",
            "ApplyGateRole" => "apply_gate",
            _ => "executor",
        }
    };
    let raw_state = if state_shape == "InvalidStateShape" {
        "running"
    } else {
        match session_state_value {
            "StaleState" => "stale",
            "ExitedState" => "exited",
            _ => "active",
        }
    };
    session_from_raw_row_parts(
        if session_token_valid {
            "session-1"
        } else {
            "session 1"
        }
        .into(),
        if agent_principal_valid {
            "principal-1"
        } else {
            "principal 1"
        }
        .into(),
        if agent_instance_valid {
            "instance-1"
        } else {
            "instance 1"
        }
        .into(),
        raw_role.into(),
        raw_state.into(),
    )
    .is_ok()
}

impl SessionRow {
    fn from_db_parts(
        session_token: String,
        agent_principal_id: String,
        agent_instance_id: String,
        role: SessionRole,
        state: SessionState,
    ) -> Result<Self, ApplyProofError> {
        Ok(SessionRow {
            session_token: parse_proof_field(session_token, "session_token")?,
            agent_principal_id: parse_proof_field(agent_principal_id, "agent_principal_id")?,
            agent_instance_id: parse_proof_field(agent_instance_id, "agent_instance_id")?,
            role,
            state,
        })
    }
}

fn load_runtime_attestation(
    conn: &Connection,
    token: &str,
) -> Result<RuntimeAttestationRow, ApplyProofError> {
    let columns = table_columns(conn, "runtime_attestations")?;
    let provider_columns =
        if columns.contains("provider_run_id") && columns.contains("provider_run_id_issuer") {
            "provider_run_id, provider_run_id_issuer"
        } else {
            "NULL AS provider_run_id, NULL AS provider_run_id_issuer"
        };
    let query = format!(
        "SELECT session_token, agent_principal_id, agent_instance_id, role, provider, model, process_id, container_id, command_transcript_digest, started_at, ended_at, recorded_at, {provider_columns} FROM runtime_attestations WHERE session_token = ?1"
    );
    conn.query_row(&query, params![token], |row| {
        RuntimeAttestationRow::from_db_parts(
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
            row.get(8)?,
            row.get(9)?,
            row.get(10)?,
            row.get(11)?,
            row.get(12)?,
            row.get(13)?,
        )
        .map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
        })
    })
    .map_err(Into::into)
}

fn table_columns(conn: &Connection, table: &str) -> Result<BTreeSet<String>, ApplyProofError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = BTreeSet::new();
    for row in rows {
        columns.insert(row?);
    }
    Ok(columns)
}

fn attestation_matches(session: &SessionRow, attestation: &RuntimeAttestationRow) -> bool {
    attestation.session_token == session.session_token
        && attestation.agent_principal_id == session.agent_principal_id
        && attestation.agent_instance_id == session.agent_instance_id
        && attestation.role == session.role
        && attestation.ended_at >= attestation.started_at
}

fn runtime_ref(attestation: &RuntimeAttestationRow) -> String {
    if let Some(container_id) = attestation.container_id() {
        return format!("container:{container_id}");
    }
    if let Some(process_id) = attestation.process_id() {
        return format!("process:{process_id}");
    }
    String::new()
}

fn observed_process_id(attestation: &RuntimeAttestationRow) -> bool {
    attestation
        .process_id()
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|value| value > 0)
}

fn actor(session: &SessionRow, attestation: &RuntimeAttestationRow) -> ActorSeal {
    ActorSeal {
        session_token: session.session_token.clone(),
        agent_principal_id: session.agent_principal_id.clone(),
        agent_instance_id: session.agent_instance_id.clone(),
        role: session.role,
        state: session.state,
        runtime_attestation: attestation.clone(),
    }
}

fn verify_host_signed_runtime_claim(
    path: &Path,
    actor_role: &str,
    attestation: &RuntimeAttestationRow,
) -> Result<Value, ApplyProofError> {
    let mut result = object([
        ("path", Value::String(path.display().to_string())),
        ("actor_role", Value::String(actor_role.to_owned())),
        ("accepted", Value::Bool(false)),
        ("blockers", Value::Array(Vec::new())),
    ]);
    if !path.exists() {
        push_blocker(&mut result, "runtime_claim_missing");
        return Ok(result);
    }
    let claim = read_json(path)?;
    let payload = claim.get("payload").cloned().unwrap_or(Value::Null);
    let public_key = claim
        .get("public_key_pem")
        .and_then(Value::as_str)
        .unwrap_or("");
    let signature = claim
        .get("signature_base64")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !payload.is_object() {
        push_blocker(&mut result, "runtime_claim_payload_missing");
    }
    if !public_key.contains("BEGIN PUBLIC KEY") {
        push_blocker(&mut result, "runtime_claim_public_key_missing");
    }
    if signature.is_empty() {
        push_blocker(&mut result, "runtime_claim_signature_missing");
    }
    if !result["blockers"].as_array().unwrap().is_empty() {
        return Ok(result);
    }
    let mut expected = runtime_claim_payload(actor_role, attestation);
    if payload.get("provider_run_id").is_some() || payload.get("provider_run_id_issuer").is_some() {
        insert_object(
            &mut expected,
            "provider_run_id",
            option_string(attestation.provider_run_id().map(str::to_owned)),
        );
        insert_object(
            &mut expected,
            "provider_run_id_issuer",
            option_string(attestation.provider_run_id_issuer().map(str::to_owned)),
        );
    }
    if payload != expected {
        push_blocker(&mut result, "runtime_claim_payload_mismatch");
        insert_object(&mut result, "expected_payload", expected);
        insert_object(&mut result, "actual_payload", payload);
        return Ok(result);
    }
    let payload_bytes = canonical_json(&payload);
    if !openssl_verify_ed25519(public_key, &payload_bytes, signature)? {
        push_blocker(&mut result, "runtime_claim_signature_verify_failed");
        return Ok(result);
    }
    insert_object(&mut result, "accepted", Value::Bool(true));
    insert_object(&mut result, "payload", payload);
    insert_object(
        &mut result,
        "payload_digest",
        Value::String(blake3_prefixed_bytes(&payload_bytes)),
    );
    insert_object(
        &mut result,
        "public_key_blake3",
        Value::String(blake3_prefixed_bytes(public_key.as_bytes())),
    );
    Ok(result)
}

fn runtime_claim_payload(actor_role: &str, attestation: &RuntimeAttestationRow) -> Value {
    object([
        (
            "schema",
            Value::String("mutai_host_signed_runtime_claim".into()),
        ),
        ("actor_role", Value::String(actor_role.into())),
        (
            "session_token",
            Value::String(attestation.session_token.to_string()),
        ),
        (
            "agent_principal_id",
            Value::String(attestation.agent_principal_id.to_string()),
        ),
        (
            "agent_instance_id",
            Value::String(attestation.agent_instance_id.to_string()),
        ),
        ("role", Value::String(attestation.role.to_string())),
        ("provider", Value::String(attestation.provider.to_string())),
        ("model", Value::String(attestation.model.to_string())),
        (
            "process_id",
            option_string(attestation.process_id().map(str::to_owned)),
        ),
        (
            "container_id",
            option_string(attestation.container_id().map(str::to_owned)),
        ),
        (
            "command_transcript_digest",
            Value::String(attestation.command_transcript_digest.to_string()),
        ),
        (
            "started_at",
            Value::Number(attestation.started_at.get().into()),
        ),
        ("ended_at", Value::Number(attestation.ended_at.get().into())),
    ])
}

fn openssl_verify_ed25519(
    public_key: &str,
    payload: &[u8],
    signature_base64: &str,
) -> Result<bool, ApplyProofError> {
    let root = make_temp_dir("mutai-runtime-claim-verify")?;
    let public_key_path = root.join("public.pem");
    let payload_path = root.join("payload.json");
    let signature_base64_path = root.join("signature.b64");
    let signature_path = root.join("signature.bin");
    fs::write(&public_key_path, public_key)?;
    fs::write(&payload_path, payload)?;
    fs::write(&signature_base64_path, signature_base64)?;
    let decode = Command::new("openssl")
        .args([
            "base64",
            "-d",
            "-A",
            "-in",
            signature_base64_path.to_str().unwrap(),
            "-out",
            signature_path.to_str().unwrap(),
        ])
        .output()?;
    if !decode.status.success() {
        let _ = fs::remove_dir_all(&root);
        return Ok(false);
    }
    let verify = Command::new("openssl")
        .args([
            "pkeyutl",
            "-verify",
            "-pubin",
            "-inkey",
            public_key_path.to_str().unwrap(),
            "-rawin",
            "-in",
            payload_path.to_str().unwrap(),
            "-sigfile",
            signature_path.to_str().unwrap(),
        ])
        .output()?;
    let _ = fs::remove_dir_all(root);
    Ok(verify.status.success())
}

type MissionPacketIdentityContractChecks =
    (BTreeMap<String, bool>, Option<Value>, BTreeSet<String>);

fn mission_packet_identity_contract_checks(
    path: &Path,
    enforce: bool,
) -> Result<MissionPacketIdentityContractChecks, ApplyProofError> {
    let mut checks = BTreeMap::new();
    checks.insert("mission_packet_file_present".into(), path.is_file());
    if !path.is_file() {
        return Ok((checks, None, BTreeSet::new()));
    }
    let mission = match read_json(path) {
        Ok(value) => value,
        Err(_) => {
            checks.insert("mission_packet_json_readable".into(), false);
            return Ok((checks, None, BTreeSet::new()));
        }
    };
    checks.insert("mission_packet_json_readable".into(), true);
    let runtime = mission.get("runtime").and_then(Value::as_object);
    let boundary = runtime
        .and_then(|runtime| runtime.get("authority_boundary"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let contract = runtime
        .and_then(|runtime| runtime.get("promoted_fleet_identity_contract"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let required_for = string_set(contract.get("required_for"));
    let actor_roles = string_set(contract.get("actor_roles"));
    let runtime_fields = string_set(contract.get("required_runtime_fields"));
    let provider_fields = string_set(contract.get("required_provider_identity_fields"));
    let forbidden = string_set(contract.get("forbidden_provider_run_id_issuers"));
    let separation = string_set(contract.get("separation_invariants"));
    let covey_bindings = string_set(contract.get("covey_binding_fields"));
    checks.insert(
        "mission_packet_authority_boundary_present".into(),
        !boundary.is_empty(),
    );
    checks.insert(
        "mission_packet_better_droid_cannot_schedule_or_settle".into(),
        boundary
            .get("better_droid_may_schedule_or_settle")
            .and_then(Value::as_bool)
            == Some(false),
    );
    checks.insert(
        "mission_packet_covey_owns_lifecycle".into(),
        boundary
            .get("covey_owns_lifecycle")
            .and_then(Value::as_bool)
            == Some(true),
    );
    checks.insert(
        "mission_packet_mutai_rs_single_attempt".into(),
        boundary
            .get("mutai_rs_evaluates_single_covey_selected_attempt")
            .and_then(Value::as_bool)
            == Some(true),
    );
    checks.insert(
        "mission_packet_hooks_enforce_boundaries".into(),
        boundary
            .get("codex_hooks_enforce_side_effect_boundaries")
            .and_then(Value::as_bool)
            == Some(true),
    );
    checks.insert(
        "mission_packet_promoted_identity_contract_present".into(),
        !contract.is_empty(),
    );
    checks.insert(
        "mission_contract_schema_current".into(),
        contract.get("schema").and_then(Value::as_str)
            == Some("mutai.runtime-identity-contract.v1"),
    );
    checks.insert(
        "mission_contract_required_for_landing".into(),
        required_for.contains("landing"),
    );
    checks.insert(
        "mission_contract_required_for_promoted_fleet_proof".into(),
        required_for.contains("promoted_fleet_proof"),
    );
    checks.insert(
        "mission_contract_actor_roles_cover_required_split".into(),
        contains_all(
            &actor_roles,
            ["executor", "reviewer", "apply_gate", "closer"],
        ),
    );
    checks.insert(
        "mission_contract_runtime_fields_cover_required_attestation".into(),
        contains_all(
            &runtime_fields,
            [
                "session_token",
                "agent_principal_id",
                "agent_instance_id",
                "role",
                "provider",
                "model",
                "process_id_or_container_id",
                "command_transcript_digest",
                "started_at",
                "ended_at",
            ],
        ),
    );
    checks.insert(
        "mission_contract_provider_identity_fields_required".into(),
        contains_all(
            &provider_fields,
            ["provider_run_id", "provider_run_id_issuer"],
        ),
    );
    checks.insert(
        "mission_contract_trusted_provider_run_id_issuer_required".into(),
        contract
            .get("trusted_provider_run_id_issuer_required")
            .and_then(Value::as_bool)
            == Some(true),
    );
    checks.insert(
        "mission_contract_forbids_local_provider_run_issuers".into(),
        contains_all(&forbidden, ["mutai-local-proof-runner", "codex-env"]),
    );
    checks.insert(
        "mission_contract_principal_separation_required".into(),
        contains_all(
            &separation,
            [
                "executor.agent_principal_id != reviewer.agent_principal_id",
                "executor.agent_principal_id != apply_gate.agent_principal_id",
                "reviewer.agent_principal_id != apply_gate.agent_principal_id",
            ],
        ),
    );
    checks.insert(
        "mission_contract_provider_run_separation_required".into(),
        contains_all(
            &separation,
            [
                "executor.provider_run_id != reviewer.provider_run_id",
                "executor.provider_run_id != apply_gate.provider_run_id",
                "reviewer.provider_run_id != apply_gate.provider_run_id",
            ],
        ),
    );
    checks.insert(
        "mission_contract_covey_bindings_cover_apply_attempt".into(),
        contains_all(
            &covey_bindings,
            [
                "queue_id",
                "artifact_digest",
                "review_id",
                "claim_fence_seq",
                "apply_verification_seal_digest",
            ],
        ),
    );
    let summary = object([
        ("path", Value::String(path.display().to_string())),
        ("enforced", Value::Bool(enforce)),
        ("authority_boundary", Value::Object(boundary)),
        ("promoted_fleet_identity_contract", Value::Object(contract)),
    ]);
    Ok((checks, Some(summary), forbidden))
}

fn batch_child_args(proof: &Value, output: &Path) -> Result<ApplyProofVerifyArgs, ApplyProofError> {
    Ok(ApplyProofVerifyArgs {
        input: None,
        repo: Some(PathBuf::from(required_string(proof, "repo")?)),
        covey_db: Some(PathBuf::from(required_string(proof, "covey_db")?)),
        evidence_dir: Some(PathBuf::from(required_string(proof, "evidence_dir")?)),
        subtask_id: Some(parse_proof_field(
            required_string(proof, "subtask_id")?,
            "subtask_id",
        )?),
        artifact_digest: Some(parse_proof_field(
            required_string(proof, "artifact_digest")?,
            "artifact_digest",
        )?),
        review_id: Some(parse_proof_field(
            required_string(proof, "review_id")?,
            "review_id",
        )?),
        queue_id: Some(parse_proof_field(
            required_string(proof, "queue_id")?,
            "queue_id",
        )?),
        reviewer_findings_digest: Some(parse_proof_field(
            required_string(proof, "reviewer_findings_digest")?,
            "reviewer_findings_digest",
        )?),
        apply_gate_session_token: Some(parse_proof_field(
            required_string(proof, "apply_gate_session_token")?,
            "apply_gate_session_token",
        )?),
        verifier: parse_proof_field(
            proof_string(proof, "verifier").unwrap_or_else(|| "mutai-rs".into()),
            "verifier",
        )?,
        verdict_digest: proof_string(proof, "verdict_digest")
            .map(|value| parse_proof_field(value, "verdict_digest"))
            .transpose()?,
        apply_verification_seal_digest: proof_string(proof, "apply_verification_seal_digest")
            .map(|value| parse_proof_field(value, "apply_verification_seal_digest"))
            .transpose()?,
        mainline_ref: Some(proof_git_ref_required(proof, "mainline_ref")?),
        subject_ref: proof_git_ref(proof, "subject_ref")?,
        artifact_file: proof_file_path(proof, "artifact_file", "feature.patch")?,
        verdict_file: proof_file_path(proof, "verdict_file", "apply-gate-output.json")?,
        success_file: proof_file_path(proof, "success_file", "full-suite-output.txt")?,
        success_text: proof_string(proof, "success_text")
            .map(ProofSuccessText::parse)
            .transpose()
            .map_err(ApplyProofError::Verification)?,
        mission_packet_file: proof_string(proof, "mission_packet_file")
            .map(EvidenceFilePath::parse)
            .transpose()
            .map_err(ApplyProofError::Verification)?,
        enforce_promoted_mission_identity_contract: proof_bool(
            proof,
            "enforce_promoted_mission_identity_contract",
        ),
        require_observed_process_ids: proof_bool(proof, "require_observed_process_ids"),
        require_host_signed_runtime_claims: proof_bool(proof, "require_host_signed_runtime_claims"),
        require_provider_run_ids: proof_bool(proof, "require_provider_run_ids"),
        trusted_provider_run_id_issuer: proof_issuer_list(
            proof,
            "trusted_provider_run_id_issuers",
        )?,
        forbidden_provider_run_id_issuer: proof_issuer_list(
            proof,
            "forbidden_provider_run_id_issuers",
        )?,
        target_ref: proof_git_ref(proof, "target_ref")?,
        output: Some(output.to_path_buf()),
    })
}

fn proof_git_ref_required(proof: &Value, field: &str) -> Result<GitRef, ApplyProofError> {
    GitRef::parse(required_string(proof, field)?).map_err(ApplyProofError::Verification)
}

fn proof_git_ref(proof: &Value, field: &str) -> Result<Option<GitRef>, ApplyProofError> {
    proof_string(proof, field)
        .map(GitRef::parse)
        .transpose()
        .map_err(ApplyProofError::Verification)
}

fn proof_string(proof: &Value, field: &str) -> Option<String> {
    proof
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn proof_file_path(
    proof: &Value,
    field: &str,
    default: &str,
) -> Result<EvidenceFilePath, ApplyProofError> {
    EvidenceFilePath::parse(proof_string(proof, field).unwrap_or_else(|| default.to_owned()))
        .map_err(ApplyProofError::Verification)
}

fn proof_bool(proof: &Value, field: &str) -> bool {
    proof.get(field).and_then(Value::as_bool).unwrap_or(false)
}

fn proof_string_list(proof: &Value, field: &str) -> Result<Vec<String>, ApplyProofError> {
    match proof.get(field) {
        None => Ok(Vec::new()),
        Some(Value::String(value)) => Ok(vec![value.clone()]),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                    ApplyProofError::Verification(format!("{field} entries must be strings"))
                })
            })
            .collect(),
        Some(_) => Err(ApplyProofError::Verification(format!(
            "{field} must be a string or list of strings"
        ))),
    }
}

fn proof_issuer_list(
    proof: &Value,
    field: &str,
) -> Result<Vec<ProviderRunIdIssuer>, ApplyProofError> {
    proof_string_list(proof, field)?
        .into_iter()
        .map(|issuer| parse_proof_field(issuer, field))
        .collect()
}

fn claim_accepted(claim: Option<&Value>) -> bool {
    claim
        .and_then(|claim| claim.get("accepted"))
        .and_then(Value::as_bool)
        == Some(true)
}

fn provider_run_id_present(claim: Option<&Value>) -> bool {
    if !claim_accepted(claim) {
        return false;
    }
    let payload = claim.and_then(|claim| claim.get("payload"));
    payload
        .and_then(|payload| payload.get("provider_run_id"))
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
        && payload
            .and_then(|payload| payload.get("provider_run_id_issuer"))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
}

fn provider_run_id_issuer_trusted(claim: Option<&Value>, trusted: &BTreeSet<String>) -> bool {
    provider_run_id_present(claim)
        && claim
            .and_then(|claim| claim.get("payload"))
            .and_then(|payload| payload.get("provider_run_id_issuer"))
            .and_then(Value::as_str)
            .is_some_and(|issuer| trusted.contains(issuer))
}

fn provider_run_id_issuer_not_forbidden(
    claim: Option<&Value>,
    forbidden: &BTreeSet<String>,
) -> bool {
    provider_run_id_present(claim)
        && claim
            .and_then(|claim| claim.get("payload"))
            .and_then(|payload| payload.get("provider_run_id_issuer"))
            .and_then(Value::as_str)
            .is_some_and(|issuer| !forbidden.contains(issuer))
}

fn check_blockers(checks: &BTreeMap<String, bool>) -> Vec<Blocker> {
    checks
        .iter()
        .filter(|(_, passed)| !**passed)
        .map(|(code, _)| Blocker::failed_check(code))
        .collect()
}

fn resolve_optional_file(evidence_dir: &Path, value: Option<&EvidenceFilePath>) -> PathBuf {
    let path = value
        .map(|value| PathBuf::from(value.as_str()))
        .unwrap_or_else(|| evidence_dir.join("mission-packet.json"));
    if path.is_absolute() {
        path
    } else {
        evidence_dir.join(path)
    }
}

fn file_digests(root: &Path, output: &Path) -> Result<BTreeMap<String, String>, ApplyProofError> {
    let mut files = BTreeMap::new();
    visit_files(root, root, output, &mut files)?;
    Ok(files)
}

fn visit_files(
    root: &Path,
    dir: &Path,
    output: &Path,
    files: &mut BTreeMap<String, String>,
) -> Result<(), ApplyProofError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            visit_files(root, &path, output, files)?;
        } else if path.is_file() && path != output {
            files.insert(
                path.strip_prefix(root).unwrap().display().to_string(),
                blake3_file(&path)?,
            );
        }
    }
    Ok(())
}

fn git<const N: usize>(repo: &Path, args: [&str; N]) -> Result<String, ApplyProofError> {
    let output = Command::new("git").args(args).current_dir(repo).output()?;
    if !output.status.success() {
        return Err(ApplyProofError::Verification(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn read_json(path: &Path) -> Result<Value, ApplyProofError> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn read_json_optional(path: &Path) -> Result<Value, ApplyProofError> {
    if path.exists() {
        read_json(path)
    } else {
        Ok(Value::Object(Map::new()))
    }
}

fn required_string(value: &Value, field: &str) -> Result<String, ApplyProofError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            ApplyProofError::Verification(format!("missing required string field {field}"))
        })
}

fn string_set(value: Option<&Value>) -> BTreeSet<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn normalized_set(values: &[ProviderRunIdIssuer]) -> BTreeSet<String> {
    values
        .iter()
        .map(|value| value.as_str().to_owned())
        .collect()
}

fn contains_all<const N: usize>(values: &BTreeSet<String>, required: [&str; N]) -> bool {
    required.iter().all(|value| values.contains(*value))
}

fn parse_optional_runtime_field<T>(
    value: Option<String>,
    field: &str,
) -> Result<Option<T>, ApplyProofError>
where
    T: TryFrom<String>,
    T::Error: std::fmt::Display,
{
    value
        .map(|value| parse_proof_field(value, field))
        .transpose()
}

fn blake3_file(path: &Path) -> Result<String, ApplyProofError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(blake3_prefixed_hash(hasher.finalize()))
}

fn blake3_prefixed_bytes(bytes: &[u8]) -> String {
    blake3_prefixed_hash(blake3::hash(bytes))
}

fn blake3_prefixed_hash(hash: blake3::Hash) -> String {
    const PREFIX: &str = "blake3:";
    let hex = hash.to_hex();
    let mut digest = String::with_capacity(PREFIX.len() + hex.len());
    digest.push_str(PREFIX);
    digest.push_str(hex.as_str());
    digest
}

fn write_json_file(path: &Path, value: &Value) -> Result<(), ApplyProofError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        format!("{}\n", String::from_utf8(canonical_json(value)).unwrap()),
    )?;
    Ok(())
}

fn canonical_json(value: &Value) -> Vec<u8> {
    match value {
        Value::Null => b"null".to_vec(),
        Value::Bool(value) => {
            if *value {
                b"true".to_vec()
            } else {
                b"false".to_vec()
            }
        }
        Value::Number(value) => value.to_string().into_bytes(),
        Value::String(value) => serde_json::to_vec(value).expect("string json"),
        Value::Array(values) => {
            let mut out = Vec::from("[");
            for (index, item) in values.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                out.extend(canonical_json(item));
            }
            out.push(b']');
            out
        }
        Value::Object(values) => {
            let mut out = Vec::from("{");
            let sorted: BTreeMap<_, _> = values.iter().collect();
            for (index, (key, value)) in sorted.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                out.extend(serde_json::to_vec(key).expect("key json"));
                out.push(b':');
                out.extend(canonical_json(value));
            }
            out.push(b'}');
            out
        }
    }
}

fn object<const N: usize>(entries: [(&str, Value); N]) -> Value {
    let mut map = Map::new();
    for (key, value) in entries {
        map.insert(key.to_owned(), value);
    }
    Value::Object(map)
}

fn insert_object(object: &mut Value, key: &str, value: Value) {
    object
        .as_object_mut()
        .unwrap()
        .insert(key.to_owned(), value);
}

fn push_blocker(object: &mut Value, blocker: &str) {
    object
        .get_mut("blockers")
        .and_then(Value::as_array_mut)
        .unwrap()
        .push(Value::String(blocker.to_owned()));
}

fn option_string(value: Option<String>) -> Value {
    value.map(Value::String).unwrap_or(Value::Null)
}

fn strings<I, S>(values: I) -> Value
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    Value::Array(
        values
            .into_iter()
            .map(|value| Value::String(value.as_ref().to_owned()))
            .collect(),
    )
}

fn make_temp_dir(prefix: &str) -> Result<PathBuf, ApplyProofError> {
    let parent = if Path::new("/data/tmp").is_dir() {
        PathBuf::from("/data/tmp")
    } else {
        std::env::temp_dir()
    };
    let dir = parent.join(format!(
        "{prefix}.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verify_args() -> ApplyProofVerifyArgs {
        ApplyProofVerifyArgs {
            input: None,
            repo: Some(PathBuf::from(".")),
            covey_db: Some(PathBuf::from("covey.db")),
            evidence_dir: Some(PathBuf::from("evidence")),
            subtask_id: None,
            artifact_digest: None,
            review_id: None,
            queue_id: None,
            reviewer_findings_digest: None,
            apply_gate_session_token: None,
            verifier: VerifierId::parse("mutai-rs").expect("valid verifier"),
            verdict_digest: None,
            apply_verification_seal_digest: None,
            mainline_ref: Some(GitRef::parse("main").expect("valid mainline ref")),
            subject_ref: None,
            artifact_file: EvidenceFilePath::parse("feature.patch").expect("valid artifact file"),
            verdict_file: EvidenceFilePath::parse("apply-gate-output.json")
                .expect("valid verdict file"),
            success_file: EvidenceFilePath::parse("full-suite-output.txt")
                .expect("valid success file"),
            success_text: None,
            mission_packet_file: None,
            enforce_promoted_mission_identity_contract: false,
            require_observed_process_ids: false,
            require_host_signed_runtime_claims: false,
            require_provider_run_ids: false,
            trusted_provider_run_id_issuer: Vec::new(),
            forbidden_provider_run_id_issuer: Vec::new(),
            target_ref: None,
            output: Some(PathBuf::from("proof.json")),
        }
    }

    #[test]
    fn landing_authorization_serializes_tree_and_artifact_context_without_head_commit() {
        let authorization = LandingAuthorization {
            schema_version: "codex_hook_landing_authorization.v1",
            queue_id: QueueId::parse("queue-1").expect("valid queue id"),
            artifact_digest: ArtifactDigest::parse("blake3:artifact").expect("valid digest"),
            review_id: ReviewId::parse("review-1").expect("valid review id"),
            findings_digest: FindingsDigest::parse("blake3:findings").expect("valid findings"),
            claim_fence_seq: FenceSeq::parse(7).expect("valid fence"),
            verifier: VerifierId::parse("mutai-rs:settlement-apply-gate").expect("valid verifier"),
            verdict_digest: ArtifactDigest::parse("blake3:verdict").expect("valid verdict"),
            apply_verification_seal_digest: ArtifactDigest::parse("blake3:apply-seal")
                .expect("valid apply seal"),
            seal_digest: ArtifactDigest::parse("blake3:seal").expect("valid seal"),
            base_tree_oid: "tree-oid".to_owned(),
            changed_paths_digest: ChangedPathsDigest::parse("blake3:paths")
                .expect("valid changed paths"),
            target_ref: "origin/main".to_owned(),
        };

        let payload = serde_json::to_value(authorization).expect("serialize authorization");

        assert_eq!(payload["base_tree_oid"], "tree-oid");
        assert_eq!(payload["changed_paths_digest"], "blake3:paths");
        assert!(payload.get("head_commit").is_none());
    }

    #[test]
    fn scheduler_artifact_manifest_base_tree_prefers_assignment_baseline() {
        let scheduler_manifest = serde_json::json!({
            "schema": "mutai_scheduler_patch_artifact.v1",
            "base_rev": "HEAD",
            "baseline_tree": "tree-from-assignment-accept",
        });
        let hook_manifest = serde_json::json!({
            "schema_version": "codex_hook_patch_bundle.v1",
            "base_rev": "origin/main",
            "baseline_tree": "ignored-for-hook-manifest",
        });

        assert_eq!(
            artifact_manifest_base_tree_rev(&scheduler_manifest).as_deref(),
            Some("tree-from-assignment-accept")
        );
        assert_eq!(
            artifact_manifest_base_tree_rev(&hook_manifest).as_deref(),
            Some("origin/main")
        );
    }

    fn runtime_attestation_row(
        process_id: Option<String>,
        container_id: Option<String>,
        provider_run_id: Option<String>,
        provider_run_id_issuer: Option<String>,
    ) -> Result<RuntimeAttestationRow, ApplyProofError> {
        RuntimeAttestationRow::from_db_parts(
            "session-1".into(),
            "agent-1".into(),
            "instance-1".into(),
            "executor".into(),
            "provider-1".into(),
            "model-1".into(),
            process_id,
            container_id,
            "blake3:transcript".into(),
            10,
            11,
            12,
            provider_run_id,
            provider_run_id_issuer,
        )
    }

    fn review_row(
        raw_state: &str,
        raw_verdict: Option<String>,
        raw_findings_digest: Option<String>,
    ) -> Result<ReviewRow, ApplyProofError> {
        let state = parse_review_state(raw_state.to_owned())?;
        let decision_evidence = ReviewProofDecisionEvidence::from_raw_for_state(
            state,
            raw_verdict,
            raw_findings_digest,
        )?;
        ReviewRow::from_db_parts(
            "review-1".into(),
            "subtask-1".into(),
            "blake3:artifact".into(),
            "reviewer-session-1".into(),
            "review-subtask-1".into(),
            state,
            decision_evidence,
        )
    }

    fn ready_queue_row(
        raw_state: &str,
        raw_claimed_by_session_token: Option<String>,
        raw_claim_fence_seq: Option<i64>,
        raw_claim_lease_deadline: Option<i64>,
    ) -> Result<ReadyQueueRow, ApplyProofError> {
        let state = parse_ready_queue_state(raw_state.to_owned())?;
        let claim_evidence = ReadyQueueProofClaimEvidence::from_raw_for_state(
            state,
            raw_claimed_by_session_token,
            raw_claim_fence_seq,
            raw_claim_lease_deadline,
        )?;
        ReadyQueueRow::from_db_parts(
            "queue-1".into(),
            "blake3:artifact".into(),
            "subtask-1".into(),
            state,
            claim_evidence,
        )
    }

    #[test]
    fn artifact_proof_row_rejects_invalid_typed_fields() {
        let invalid_artifact_digest = ArtifactRow::from_db_parts(
            "artifact".into(),
            "subtask-1".into(),
            "session-1".into(),
            "manifest.json".into(),
            "blake3:paths".into(),
        )
        .expect_err("artifact proof rows should require typed artifact digests");
        assert!(
            invalid_artifact_digest
                .to_string()
                .contains("invalid typed Covey scalar field artifact_digest"),
            "unexpected error: {invalid_artifact_digest}"
        );

        let invalid_session = ArtifactRow::from_db_parts(
            "blake3:artifact".into(),
            "subtask-1".into(),
            "session 1".into(),
            "manifest.json".into(),
            "blake3:paths".into(),
        )
        .expect_err("artifact proof rows should require typed producer sessions");
        assert!(
            invalid_session
                .to_string()
                .contains("invalid typed Covey scalar field produced_by_session"),
            "unexpected error: {invalid_session}"
        );

        let invalid_changed_paths = ArtifactRow::from_db_parts(
            "blake3:artifact".into(),
            "subtask-1".into(),
            "session-1".into(),
            "manifest.json".into(),
            "paths".into(),
        )
        .expect_err("artifact proof rows should require typed changed-path digests");
        assert!(
            invalid_changed_paths
                .to_string()
                .contains("invalid typed Covey scalar field changed_paths_digest"),
            "unexpected error: {invalid_changed_paths}"
        );
    }

    #[test]
    fn verify_request_parses_optional_refs_into_typed_scalars() {
        let mut args = verify_args();
        args.subtask_id = Some(SubtaskId::parse("subtask-1").expect("valid subtask id"));
        args.artifact_digest =
            Some(ArtifactDigest::parse("blake3:artifact").expect("valid artifact digest"));
        args.review_id = Some(ReviewId::parse("review-1").expect("valid review id"));
        args.queue_id = Some(QueueId::parse("queue-1").expect("valid queue id"));
        args.reviewer_findings_digest =
            Some(FindingsDigest::parse("blake3:findings").expect("valid findings digest"));
        args.apply_gate_session_token =
            Some(SessionToken::parse("session-apply").expect("valid session token"));
        args.verdict_digest =
            Some(ArtifactDigest::parse("blake3:verdict").expect("valid verdict digest"));
        args.apply_verification_seal_digest =
            Some(ArtifactDigest::parse("blake3:seal").expect("valid seal digest"));
        args.trusted_provider_run_id_issuer
            .push(ProviderRunIdIssuer::parse("provider-authority").expect("valid trusted issuer"));
        args.forbidden_provider_run_id_issuer
            .push(ProviderRunIdIssuer::parse("local-runner").expect("valid forbidden issuer"));
        args.subject_ref = Some(GitRef::parse("feature/apply-proof").expect("valid subject ref"));
        args.target_ref = Some(GitRef::parse("main").expect("valid target ref"));

        let request = VerifyRequest::from_args(args).expect("typed request refs should parse");

        assert_eq!(request.mainline_ref.as_str(), "main");
        assert_eq!(
            request.subject_ref.as_ref().map(GitRef::as_str),
            Some("feature/apply-proof")
        );
        assert_eq!(
            request.target_ref.as_ref().map(GitRef::as_str),
            Some("main")
        );
        assert_eq!(
            request.subtask_id.as_ref().map(SubtaskId::as_str),
            Some("subtask-1")
        );
        assert_eq!(
            request.artifact_digest.as_ref().map(ArtifactDigest::as_str),
            Some("blake3:artifact")
        );
        assert_eq!(
            request.review_id.as_ref().map(ReviewId::as_str),
            Some("review-1")
        );
        assert_eq!(
            request.queue_id.as_ref().map(QueueId::as_str),
            Some("queue-1")
        );
        assert_eq!(
            request
                .reviewer_findings_digest
                .as_ref()
                .map(FindingsDigest::as_str),
            Some("blake3:findings")
        );
        assert_eq!(
            request
                .apply_gate_session_token
                .as_ref()
                .map(SessionToken::as_str),
            Some("session-apply")
        );
        assert_eq!(
            request.verdict_digest.as_ref().map(ArtifactDigest::as_str),
            Some("blake3:verdict")
        );
        assert_eq!(
            request
                .apply_verification_seal_digest
                .as_ref()
                .map(ArtifactDigest::as_str),
            Some("blake3:seal")
        );
        assert_eq!(request.verifier.as_str(), "mutai-rs");
        assert_eq!(
            request
                .trusted_provider_run_id_issuer
                .iter()
                .map(ProviderRunIdIssuer::as_str)
                .collect::<Vec<_>>(),
            vec!["provider-authority"]
        );
        assert_eq!(
            request
                .forbidden_provider_run_id_issuer
                .iter()
                .map(ProviderRunIdIssuer::as_str)
                .collect::<Vec<_>>(),
            vec!["local-runner"]
        );
        assert_eq!(request.artifact_file.as_str(), "feature.patch");
        assert_eq!(request.verdict_file.as_str(), "apply-gate-output.json");
        assert_eq!(request.success_file.as_str(), "full-suite-output.txt");
    }

    #[test]
    fn proof_blocker_rejects_invalid_code_and_message() {
        let blank_code = Blocker::new("", "required proof check failed: worktree_clean")
            .expect_err("proof blockers should reject blank codes");
        assert!(
            blank_code.contains("proof blocker code must not be empty"),
            "unexpected error: {blank_code}"
        );

        let control_code = Blocker::new("worktree\nclean", "required proof check failed")
            .expect_err("proof blockers should reject control characters in codes");
        assert!(
            control_code.contains("proof blocker code must not contain control characters"),
            "unexpected error: {control_code}"
        );

        let blank_message =
            Blocker::new("worktree_clean", " ").expect_err("proof blockers reject blank messages");
        assert!(
            blank_message.contains("proof blocker message must not be empty"),
            "unexpected error: {blank_message}"
        );

        let blocker = Blocker::failed_check("worktree_clean");
        let json = serde_json::to_value(blocker).expect("blocker serializes");
        assert_eq!(json["code"], "worktree_clean");
        assert_eq!(
            json["message"],
            "required proof check failed: worktree_clean"
        );
    }

    #[test]
    fn verify_request_rejects_invalid_typed_refs_before_verification() {
        let err = ApplyProofVerifyArgs::try_parse_from(["proof", "--artifact-digest", "artifact"])
            .expect_err("invalid artifact digest should reject the CLI request");
        assert!(
            err.to_string().contains("artifact_digest"),
            "unexpected error: {err}"
        );

        let err = ApplyProofVerifyArgs::try_parse_from([
            "proof",
            "--apply-gate-session-token",
            "session apply",
        ])
        .expect_err("invalid apply-gate session token should reject the CLI request");
        assert!(
            err.to_string().contains("session_token"),
            "unexpected error: {err}"
        );

        let err = ApplyProofVerifyArgs::try_parse_from(["proof", "--verdict-digest", "verdict"])
            .expect_err("invalid verdict digest should reject the CLI request");
        assert!(
            err.to_string().contains("artifact_digest"),
            "unexpected error: {err}"
        );

        let err = ApplyProofVerifyArgs::try_parse_from([
            "proof",
            "--apply-verification-seal-digest",
            "seal",
        ])
        .expect_err("invalid apply verification seal digest should reject the CLI request");
        assert!(
            err.to_string().contains("artifact_digest"),
            "unexpected error: {err}"
        );

        let invalid_file = serde_json::json!({
            "schema": REQUEST_SCHEMA,
            "artifact_digest": "artifact"
        });
        serde_json::from_value::<VerifyRequestFile>(invalid_file)
            .expect_err("input files should reject invalid typed proof refs during decoding");

        let err = ApplyProofVerifyArgs::try_parse_from(["proof", "--mainline-ref", " main"])
            .expect_err("padded mainline refs should reject the CLI request");
        assert!(
            err.to_string().contains("git ref"),
            "unexpected error: {err}"
        );

        let invalid_git_ref_file = serde_json::json!({
            "schema": REQUEST_SCHEMA,
            "mainline_ref": "main branch"
        });
        serde_json::from_value::<VerifyRequestFile>(invalid_git_ref_file)
            .expect_err("input files should reject invalid git refs");

        let err = ApplyProofVerifyArgs::try_parse_from([
            "proof",
            "--trusted-provider-run-id-issuer",
            " provider-authority",
        ])
        .expect_err("invalid trusted provider run issuer should reject the CLI request");
        assert!(
            err.to_string().contains("provider_run_id_issuer"),
            "unexpected error: {err}"
        );

        let invalid_issuer_file = serde_json::json!({
            "schema": REQUEST_SCHEMA,
            "trusted_provider_run_id_issuer": ["provider-authority", " local-runner"]
        });
        serde_json::from_value::<VerifyRequestFile>(invalid_issuer_file)
            .expect_err("input files should reject invalid provider run issuers");

        let err =
            ApplyProofVerifyArgs::try_parse_from(["proof", "--artifact-file", "../feature.patch"])
                .expect_err("escaping artifact file paths should reject the CLI request");
        assert!(
            err.to_string().contains("evidence file path"),
            "unexpected error: {err}"
        );

        let invalid_file_path = serde_json::json!({
            "schema": REQUEST_SCHEMA,
            "mission_packet_file": "/tmp/mission-packet.json"
        });
        serde_json::from_value::<VerifyRequestFile>(invalid_file_path)
            .expect_err("input files should reject absolute evidence file paths");

        let err = ApplyProofVerifyArgs::try_parse_from(["proof", "--success-text", ""])
            .expect_err("empty success text should reject the CLI request");
        assert!(
            err.to_string().contains("success text"),
            "unexpected error: {err}"
        );

        let invalid_success_text = serde_json::json!({
            "schema": REQUEST_SCHEMA,
            "success_text": " "
        });
        serde_json::from_value::<VerifyRequestFile>(invalid_success_text)
            .expect_err("input files should reject blank success text");
    }

    #[test]
    fn batch_child_args_rejects_invalid_provider_run_issuers() {
        let mut proof = object([
            ("repo", Value::String(".".into())),
            ("covey_db", Value::String("covey.db".into())),
            ("evidence_dir", Value::String("evidence".into())),
            ("subtask_id", Value::String("subtask-1".into())),
            ("artifact_digest", Value::String("blake3:artifact".into())),
            ("review_id", Value::String("review-1".into())),
            ("queue_id", Value::String("queue-1".into())),
            (
                "reviewer_findings_digest",
                Value::String("blake3:findings".into()),
            ),
            (
                "apply_gate_session_token",
                Value::String("session-apply".into()),
            ),
            ("mainline_ref", Value::String("main".into())),
            (
                "trusted_provider_run_id_issuers",
                Value::Array(vec![Value::String("provider-authority".into())]),
            ),
        ]);
        let args =
            batch_child_args(&proof, Path::new("proof.json")).expect("valid batch proof args");
        assert_eq!(args.artifact_file.as_str(), "feature.patch");
        assert_eq!(
            args.trusted_provider_run_id_issuer
                .iter()
                .map(ProviderRunIdIssuer::as_str)
                .collect::<Vec<_>>(),
            vec!["provider-authority"]
        );

        proof["forbidden_provider_run_id_issuers"] =
            Value::Array(vec![Value::String(" local-runner".into())]);
        batch_child_args(&proof, Path::new("proof.json"))
            .expect_err("batch child args should reject invalid provider run issuers");

        proof["forbidden_provider_run_id_issuers"] = Value::Array(Vec::new());
        proof["success_file"] = Value::String("../full-suite-output.txt".into());
        batch_child_args(&proof, Path::new("proof.json"))
            .expect_err("batch child args should reject escaping evidence file paths");

        proof["success_file"] = Value::String("full-suite-output.txt".into());
        proof["mainline_ref"] = Value::String("main branch".into());
        batch_child_args(&proof, Path::new("proof.json"))
            .expect_err("batch child args should reject invalid git refs");
    }

    #[test]
    fn review_proof_row_preserves_flat_decided_shape() {
        let row = review_row(
            "decided",
            Some("approve".into()),
            Some("blake3:findings".into()),
        )
        .expect("valid decided review proof row");
        let value = serde_json::to_value(&row).expect("review proof row should serialize");

        assert!(row.approved());
        assert_eq!(row.state(), ReviewState::Decided);
        assert_eq!(row.verdict(), Some(ReviewVerdict::Approve));
        assert_eq!(row.findings_digest(), Some("blake3:findings"));
        assert_eq!(value["state"], "decided");
        assert_eq!(value["verdict"], "approve");
        assert_eq!(value["findings_digest"], "blake3:findings");
    }

    #[test]
    fn review_proof_row_requires_decision_evidence_for_decided_state() {
        let missing_verdict = review_row("decided", None, Some("blake3:findings".into()))
            .expect_err("decided proof rows require verdict");
        assert!(
            missing_verdict
                .to_string()
                .contains("decided review proof row requires verdict"),
            "unexpected error: {missing_verdict}"
        );

        let missing_findings = review_row("decided", Some("approve".into()), None)
            .expect_err("decided proof rows require findings digest");
        assert!(
            missing_findings
                .to_string()
                .contains("decided review proof row requires non-empty findings_digest"),
            "unexpected error: {missing_findings}"
        );

        let invalid_findings =
            review_row("decided", Some("approve".into()), Some("findings".into()))
                .expect_err("decided proof rows require typed findings digest");
        assert!(
            invalid_findings
                .to_string()
                .contains("invalid typed Covey scalar field findings_digest"),
            "unexpected error: {invalid_findings}"
        );
    }

    #[test]
    fn review_proof_row_rejects_decision_evidence_for_non_decided_state() {
        let err = review_row(
            "requested",
            Some("approve".into()),
            Some("blake3:findings".into()),
        )
        .expect_err("requested proof rows must not carry decision evidence");

        assert!(
            err.to_string()
                .contains("requested review proof row must not carry decision evidence"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn ready_queue_proof_row_requires_fence_for_applied_state() {
        let err = ready_queue_row("applied", None, None, None)
            .expect_err("applied ready queue proof rows require a fence");

        assert!(
            err.to_string()
                .contains("applied ready queue row requires claim_fence_seq"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn ready_queue_proof_row_rejects_active_claimant_for_applied_state() {
        let err = ready_queue_row("applied", Some("session-1".into()), Some(7), None)
            .expect_err("applied ready queue proof rows must not carry active claimants");

        assert!(
            err.to_string()
                .contains("applied ready queue row must not carry active claim fields"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn ready_queue_proof_row_preserves_queued_rows_without_fabricated_fence() {
        let row = ready_queue_row("queued", None, None, None)
            .expect("queued queue rows remain observable proof blockers");
        let value = serde_json::to_value(&row).expect("ready queue row should serialize");

        assert_eq!(row.state(), ReadyQueueState::Queued);
        assert_eq!(row.applied_claim_fence_seq(), None);
        assert_eq!(value["claim_fence_seq"], Value::Null);
    }

    #[test]
    fn ready_queue_proof_row_requires_complete_active_claim_for_in_flight_state() {
        let missing_lease = ready_queue_row("in_flight", Some("session-1".into()), Some(7), None)
            .expect_err("in-flight proof rows require a lease deadline");

        assert!(
            missing_lease
                .to_string()
                .contains("in-flight ready queue row requires claim_lease_deadline"),
            "unexpected error: {missing_lease}"
        );

        let row = ready_queue_row("in_flight", Some("session-1".into()), Some(7), Some(10_000))
            .expect("complete in-flight queue row should be representable");
        let value = serde_json::to_value(&row).expect("ready queue row should serialize");

        assert_eq!(row.state(), ReadyQueueState::InFlight);
        assert_eq!(row.claimed_by_session_token(), Some("session-1"));
        assert_eq!(row.claim_fence_seq(), Some(7));
        assert_eq!(row.active_claim(), Some(("session-1", 7, 10_000)));
        assert_eq!(value["state"], "in_flight");
        assert_eq!(value["claim_fence_seq"], 7);
    }

    #[test]
    fn ready_queue_proof_row_rejects_invalid_typed_claim_fields() {
        let invalid_session =
            ready_queue_row("in_flight", Some("session 1".into()), Some(7), Some(10_000))
                .expect_err("in-flight queue row should require typed session token");
        assert!(
            invalid_session
                .to_string()
                .contains("invalid typed Covey scalar field claimed_by_session_token"),
            "unexpected error: {invalid_session}"
        );

        let invalid_fence = ready_queue_row("applied", None, Some(0), None)
            .expect_err("applied queue row should require a valid fence sequence");
        assert!(
            invalid_fence
                .to_string()
                .contains("invalid ready queue claim_fence_seq in proof row"),
            "unexpected error: {invalid_fence}"
        );
    }

    #[test]
    fn apply_verification_row_rejects_invalid_typed_fields() {
        let invalid_verdict_digest = ApplyVerificationRow::from_db_parts(
            "queue-1".into(),
            "blake3:artifact".into(),
            "review-1".into(),
            "blake3:findings".into(),
            7,
            "mutai-rs".into(),
            "verdict".into(),
            "blake3:seal".into(),
            "session-apply".into(),
            10,
        )
        .expect_err("apply verification proof rows should require typed verdict digests");
        assert!(
            invalid_verdict_digest
                .to_string()
                .contains("invalid typed Covey scalar field verdict_digest"),
            "unexpected error: {invalid_verdict_digest}"
        );

        let invalid_fence = ApplyVerificationRow::from_db_parts(
            "queue-1".into(),
            "blake3:artifact".into(),
            "review-1".into(),
            "blake3:findings".into(),
            0,
            "mutai-rs".into(),
            "blake3:verdict".into(),
            "blake3:seal".into(),
            "session-apply".into(),
            10,
        )
        .expect_err("apply verification proof rows should require valid fence sequences");
        assert!(
            invalid_fence
                .to_string()
                .contains("invalid ready queue claim_fence_seq in proof row"),
            "unexpected error: {invalid_fence}"
        );

        let invalid_created_at = ApplyVerificationRow::from_db_parts(
            "queue-1".into(),
            "blake3:artifact".into(),
            "review-1".into(),
            "blake3:findings".into(),
            7,
            "mutai-rs".into(),
            "blake3:verdict".into(),
            "blake3:seal".into(),
            "session-apply".into(),
            -1,
        )
        .expect_err("apply verification proof rows should require non-negative created_at");
        assert!(
            invalid_created_at
                .to_string()
                .contains("invalid apply verification created_at"),
            "unexpected error: {invalid_created_at}"
        );
    }

    #[test]
    fn ready_queue_proof_row_rejects_active_claim_fields_for_terminal_states() {
        for state in ["queued", "superseded", "cancelled"] {
            let err = ready_queue_row(state, Some("session-1".into()), Some(7), Some(10_000))
                .expect_err("non-in-flight proof rows must not carry active claim fields");

            assert!(
                err.to_string()
                    .contains("ready queue row must not carry active claim fields"),
                "unexpected error for {state}: {err}"
            );
        }
    }

    #[test]
    fn runtime_attestation_proof_row_requires_runtime_identity() {
        let err = runtime_attestation_row(
            None,
            None,
            Some("provider-run-1".into()),
            Some("provider-issuer-1".into()),
        )
        .expect_err("runtime proof rows require a process or container identity");

        assert!(
            err.to_string()
                .contains("runtime attestation requires process_id or container_id"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn runtime_attestation_proof_row_rejects_partial_provider_run_identity() {
        let err = runtime_attestation_row(
            Some("1234".into()),
            None,
            Some("provider-run-1".into()),
            None,
        )
        .expect_err("provider run proof identity requires both fields");

        assert!(
            err.to_string().contains(
                "runtime attestation provider run identity must include both id and issuer"
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn runtime_attestation_proof_row_rejects_invalid_typed_fields() {
        let invalid_agent_principal = RuntimeAttestationRow::from_db_parts(
            "session-1".into(),
            "agent 1".into(),
            "instance-1".into(),
            "executor".into(),
            "provider-1".into(),
            "model-1".into(),
            Some("1234".into()),
            None,
            "blake3:transcript".into(),
            10,
            11,
            12,
            None,
            None,
        )
        .expect_err("runtime proof rows should require typed agent principal ids");
        assert!(
            invalid_agent_principal
                .to_string()
                .contains("invalid typed Covey scalar field agent_principal_id"),
            "unexpected error: {invalid_agent_principal}"
        );

        let invalid_provider = RuntimeAttestationRow::from_db_parts(
            "session-1".into(),
            "agent-1".into(),
            "instance-1".into(),
            "executor".into(),
            "provider 1".into(),
            "model-1".into(),
            Some("1234".into()),
            None,
            "blake3:transcript".into(),
            10,
            11,
            12,
            None,
            None,
        )
        .expect_err("runtime proof rows should require typed provider ids");
        assert!(
            invalid_provider
                .to_string()
                .contains("invalid typed Covey scalar field provider"),
            "unexpected error: {invalid_provider}"
        );

        let invalid_transcript = RuntimeAttestationRow::from_db_parts(
            "session-1".into(),
            "agent-1".into(),
            "instance-1".into(),
            "executor".into(),
            "provider-1".into(),
            "model-1".into(),
            Some("1234".into()),
            None,
            "transcript".into(),
            10,
            11,
            12,
            None,
            None,
        )
        .expect_err("runtime proof rows should require typed transcript digests");
        assert!(
            invalid_transcript
                .to_string()
                .contains("invalid typed Covey scalar field command_transcript_digest"),
            "unexpected error: {invalid_transcript}"
        );

        let invalid_started_at = RuntimeAttestationRow::from_db_parts(
            "session-1".into(),
            "agent-1".into(),
            "instance-1".into(),
            "executor".into(),
            "provider-1".into(),
            "model-1".into(),
            Some("1234".into()),
            None,
            "blake3:transcript".into(),
            -1,
            11,
            12,
            None,
            None,
        )
        .expect_err("runtime proof rows should require non-negative timestamps");
        assert!(
            invalid_started_at
                .to_string()
                .contains("invalid started_at"),
            "unexpected error: {invalid_started_at}"
        );

        let invalid_role = session_from_raw_row_parts(
            "session-1".into(),
            "agent-1".into(),
            "instance-1".into(),
            "worker".into(),
            "active".into(),
        )
        .expect_err("session proof rows should require typed session roles");
        assert!(
            invalid_role.to_string().contains("invalid role"),
            "unexpected error: {invalid_role}"
        );

        let invalid_agent_instance = session_from_raw_row_parts(
            "session-1".into(),
            "agent-1".into(),
            "instance 1".into(),
            "executor".into(),
            "active".into(),
        )
        .expect_err("session proof rows should require typed agent instance ids");
        assert!(
            invalid_agent_instance
                .to_string()
                .contains("invalid typed Covey scalar field agent_instance_id"),
            "unexpected error: {invalid_agent_instance}"
        );
    }

    #[test]
    fn runtime_attestation_proof_row_rejects_recorded_before_ended() {
        let err = RuntimeAttestationRow::from_db_parts(
            "session-1".into(),
            "agent-1".into(),
            "instance-1".into(),
            "executor".into(),
            "provider-1".into(),
            "model-1".into(),
            Some("1234".into()),
            None,
            "blake3:transcript".into(),
            10,
            12,
            11,
            None,
            None,
        )
        .expect_err("runtime proof rows should record after the attested runtime span");

        assert!(
            err.to_string().contains(
                "runtime attestation recorded_at must be greater than or equal to ended_at"
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn runtime_attestation_proof_row_preserves_old_schema_provider_nulls() {
        let row = runtime_attestation_row(Some("1234".into()), None, None, None)
            .expect("old runtime rows without provider run columns remain observable");
        let value = serde_json::to_value(&row).expect("runtime row should serialize");

        assert_eq!(runtime_ref(&row), "process:1234");
        assert_eq!(value["provider_run_id"], Value::Null);
        assert_eq!(value["provider_run_id_issuer"], Value::Null);
    }
}

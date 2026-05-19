//! Covey-owned apply proof replay and sealing.

use clap::Parser;
use rusqlite::{Connection, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};
use thiserror::Error;

const REQUEST_SCHEMA: &str = "covey_apply_proof_verify_request";
const SEAL_SCHEMA: &str = "mutai_covey_apply_proof_seal";
const BATCH_SEAL_SCHEMA: &str = "mutai_covey_apply_proof_batch_seal.v1";

#[derive(Debug, Parser)]
pub struct ApplyProofVerifyArgs {
    #[arg(long)]
    pub input: Option<PathBuf>,
    #[arg(long)]
    pub repo: Option<PathBuf>,
    #[arg(long = "covey-db")]
    pub covey_db: Option<PathBuf>,
    #[arg(long = "evidence-dir")]
    pub evidence_dir: Option<PathBuf>,
    #[arg(long = "subtask-id")]
    pub subtask_id: Option<String>,
    #[arg(long = "artifact-digest")]
    pub artifact_digest: Option<String>,
    #[arg(long = "review-id")]
    pub review_id: Option<String>,
    #[arg(long = "queue-id")]
    pub queue_id: Option<String>,
    #[arg(long = "reviewer-findings-digest")]
    pub reviewer_findings_digest: Option<String>,
    #[arg(long = "apply-gate-session-token")]
    pub apply_gate_session_token: Option<String>,
    #[arg(long, default_value = "mutai-rs")]
    pub verifier: String,
    #[arg(long = "verdict-digest")]
    pub verdict_digest: Option<String>,
    #[arg(long = "apply-verification-seal-digest")]
    pub apply_verification_seal_digest: Option<String>,
    #[arg(long = "mainline-ref")]
    pub mainline_ref: Option<String>,
    #[arg(long = "subject-ref")]
    pub subject_ref: Option<String>,
    #[arg(long = "artifact-file", default_value = "feature.patch")]
    pub artifact_file: String,
    #[arg(long = "verdict-file", default_value = "apply-gate-output.json")]
    pub verdict_file: String,
    #[arg(long = "success-file", default_value = "full-suite-output.txt")]
    pub success_file: String,
    #[arg(long = "success-text")]
    pub success_text: Option<String>,
    #[arg(long = "mission-packet-file")]
    pub mission_packet_file: Option<String>,
    #[arg(long = "enforce-promoted-mission-identity-contract")]
    pub enforce_promoted_mission_identity_contract: bool,
    #[arg(long = "require-observed-process-ids")]
    pub require_observed_process_ids: bool,
    #[arg(long = "require-host-signed-runtime-claims")]
    pub require_host_signed_runtime_claims: bool,
    #[arg(long = "require-provider-run-ids")]
    pub require_provider_run_ids: bool,
    #[arg(long = "trusted-provider-run-id-issuer")]
    pub trusted_provider_run_id_issuer: Vec<String>,
    #[arg(long = "forbidden-provider-run-id-issuer")]
    pub forbidden_provider_run_id_issuer: Vec<String>,
    #[arg(long = "target-ref")]
    pub target_ref: Option<String>,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Parser)]
pub struct ApplyProofBatchArgs {
    #[arg(long)]
    pub manifest: PathBuf,
    #[arg(long)]
    pub output: PathBuf,
    #[arg(long = "seal-dir")]
    pub seal_dir: Option<PathBuf>,
    #[arg(long = "require-observed-process-ids")]
    pub require_observed_process_ids: bool,
    #[arg(long = "require-host-signed-runtime-claims")]
    pub require_host_signed_runtime_claims: bool,
    #[arg(long = "require-provider-run-ids")]
    pub require_provider_run_ids: bool,
    #[arg(long = "trusted-provider-run-id-issuer")]
    pub trusted_provider_run_id_issuer: Vec<String>,
    #[arg(long = "forbidden-provider-run-id-issuer")]
    pub forbidden_provider_run_id_issuer: Vec<String>,
    #[arg(long = "enforce-promoted-mission-identity-contract")]
    pub enforce_promoted_mission_identity_contract: bool,
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
    subtask_id: Option<String>,
    artifact_digest: Option<String>,
    review_id: Option<String>,
    queue_id: Option<String>,
    reviewer_findings_digest: Option<String>,
    apply_gate_session_token: Option<String>,
    verifier: Option<String>,
    verdict_digest: Option<String>,
    apply_verification_seal_digest: Option<String>,
    mainline_ref: Option<String>,
    subject_ref: Option<String>,
    artifact_file: Option<String>,
    verdict_file: Option<String>,
    success_file: Option<String>,
    success_text: Option<String>,
    mission_packet_file: Option<String>,
    enforce_promoted_mission_identity_contract: Option<bool>,
    require_observed_process_ids: Option<bool>,
    require_host_signed_runtime_claims: Option<bool>,
    require_provider_run_ids: Option<bool>,
    trusted_provider_run_id_issuer: Option<Vec<String>>,
    forbidden_provider_run_id_issuer: Option<Vec<String>>,
    target_ref: Option<String>,
    output: Option<PathBuf>,
}

#[derive(Debug)]
struct VerifyRequest {
    repo: PathBuf,
    covey_db: PathBuf,
    evidence_dir: PathBuf,
    subtask_id: Option<String>,
    artifact_digest: Option<String>,
    review_id: Option<String>,
    queue_id: Option<String>,
    reviewer_findings_digest: Option<String>,
    apply_gate_session_token: Option<String>,
    verifier: String,
    verdict_digest: Option<String>,
    apply_verification_seal_digest: Option<String>,
    mainline_ref: String,
    subject_ref: Option<String>,
    artifact_file: String,
    verdict_file: String,
    success_file: String,
    success_text: Option<String>,
    mission_packet_file: Option<String>,
    enforce_promoted_mission_identity_contract: bool,
    require_observed_process_ids: bool,
    require_host_signed_runtime_claims: bool,
    require_provider_run_ids: bool,
    trusted_provider_run_id_issuer: Vec<String>,
    forbidden_provider_run_id_issuer: Vec<String>,
    target_ref: Option<String>,
    output: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
struct ArtifactRow {
    artifact_digest: String,
    produced_by_subtask_id: String,
    produced_by_session: String,
    manifest_path: String,
    changed_paths_digest: String,
}

#[derive(Debug, Clone, Serialize)]
struct ReviewRow {
    review_id: String,
    subtask_id: String,
    artifact_digest: String,
    reviewer_session: String,
    review_subtask_id: String,
    verdict: String,
    findings_digest: String,
    state: String,
}

#[derive(Debug, Clone)]
struct ReadyQueueRow {
    queue_id: String,
    artifact_digest: String,
    subtask_id: String,
    lifecycle: ReadyQueueProofLifecycle,
}

#[derive(Debug, Clone)]
enum ReadyQueueProofLifecycle {
    Applied {
        claim_fence_seq: i64,
    },
    Other {
        state: String,
        claimed_by_session_token: Option<String>,
        claim_fence_seq: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize)]
struct RawReadyQueueRow<'a> {
    queue_id: &'a str,
    artifact_digest: &'a str,
    subtask_id: &'a str,
    state: &'a str,
    claimed_by_session_token: Option<&'a str>,
    claim_fence_seq: Option<i64>,
}

impl ReadyQueueRow {
    fn from_db_parts(
        queue_id: String,
        artifact_digest: String,
        subtask_id: String,
        state: String,
        claimed_by_session_token: Option<String>,
        claim_fence_seq: Option<i64>,
    ) -> Result<Self, ApplyProofError> {
        let lifecycle = if state == "applied" {
            if claimed_by_session_token.is_some() {
                return Err(ApplyProofError::Verification(
                    "applied ready queue row must not carry active claimant".into(),
                ));
            }
            ReadyQueueProofLifecycle::Applied {
                claim_fence_seq: claim_fence_seq.ok_or_else(|| {
                    ApplyProofError::Verification(
                        "applied ready queue row requires claim_fence_seq".into(),
                    )
                })?,
            }
        } else {
            ReadyQueueProofLifecycle::Other {
                state,
                claimed_by_session_token,
                claim_fence_seq,
            }
        };
        Ok(Self {
            queue_id,
            artifact_digest,
            subtask_id,
            lifecycle,
        })
    }

    fn state(&self) -> &str {
        match &self.lifecycle {
            ReadyQueueProofLifecycle::Applied { .. } => "applied",
            ReadyQueueProofLifecycle::Other { state, .. } => state.as_str(),
        }
    }

    fn applied_claim_fence_seq(&self) -> Option<i64> {
        match self.lifecycle {
            ReadyQueueProofLifecycle::Applied { claim_fence_seq } => Some(claim_fence_seq),
            ReadyQueueProofLifecycle::Other { .. } => None,
        }
    }

    fn claim_fence_seq(&self) -> Option<i64> {
        match self.lifecycle {
            ReadyQueueProofLifecycle::Applied { claim_fence_seq } => Some(claim_fence_seq),
            ReadyQueueProofLifecycle::Other {
                claim_fence_seq, ..
            } => claim_fence_seq,
        }
    }

    fn claimed_by_session_token(&self) -> Option<&str> {
        match &self.lifecycle {
            ReadyQueueProofLifecycle::Applied { .. } => None,
            ReadyQueueProofLifecycle::Other {
                claimed_by_session_token,
                ..
            } => claimed_by_session_token.as_deref(),
        }
    }
}

impl Serialize for ReadyQueueRow {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        RawReadyQueueRow {
            queue_id: &self.queue_id,
            artifact_digest: &self.artifact_digest,
            subtask_id: &self.subtask_id,
            state: self.state(),
            claimed_by_session_token: self.claimed_by_session_token(),
            claim_fence_seq: self.claim_fence_seq(),
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, Serialize)]
struct ApplyVerificationRow {
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
}

#[derive(Debug, Clone, Serialize)]
struct SessionRow {
    session_token: String,
    agent_principal_id: String,
    agent_instance_id: String,
    role: String,
    state: String,
}

#[derive(Debug, Clone)]
struct RuntimeAttestationRow {
    session_token: String,
    agent_principal_id: String,
    agent_instance_id: String,
    role: String,
    provider: String,
    model: String,
    runtime_identity: RuntimeProofIdentity,
    command_transcript_digest: String,
    started_at: i64,
    ended_at: i64,
    recorded_at: i64,
    provider_run_identity: Option<ProviderRunProofIdentity>,
}

#[derive(Debug, Clone)]
enum RuntimeProofIdentity {
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

#[derive(Debug, Clone)]
struct ProviderRunProofIdentity {
    provider_run_id: String,
    provider_run_id_issuer: String,
}

#[derive(Debug, Clone, Serialize)]
struct RawRuntimeAttestationRow<'a> {
    session_token: &'a str,
    agent_principal_id: &'a str,
    agent_instance_id: &'a str,
    role: &'a str,
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
        if ended_at < started_at {
            return Err(ApplyProofError::Verification(
                "runtime attestation ended_at must be greater than or equal to started_at".into(),
            ));
        }
        Ok(Self {
            session_token,
            agent_principal_id,
            agent_instance_id,
            role,
            provider,
            model,
            runtime_identity,
            command_transcript_digest,
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

impl RuntimeProofIdentity {
    fn from_parts(
        process_id: Option<String>,
        container_id: Option<String>,
    ) -> Result<Self, ApplyProofError> {
        let process_id = normalize_optional_runtime_field(process_id, "process_id")?;
        let container_id = normalize_optional_runtime_field(container_id, "container_id")?;
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

impl ProviderRunProofIdentity {
    fn optional_from_parts(
        provider_run_id: Option<String>,
        provider_run_id_issuer: Option<String>,
    ) -> Result<Option<Self>, ApplyProofError> {
        let provider_run_id = normalize_optional_runtime_field(provider_run_id, "provider_run_id")?;
        let provider_run_id_issuer =
            normalize_optional_runtime_field(provider_run_id_issuer, "provider_run_id_issuer")?;
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
            session_token: &self.session_token,
            agent_principal_id: &self.agent_principal_id,
            agent_instance_id: &self.agent_instance_id,
            role: &self.role,
            provider: &self.provider,
            model: &self.model,
            process_id: self.process_id(),
            container_id: self.container_id(),
            command_transcript_digest: &self.command_transcript_digest,
            started_at: self.started_at,
            ended_at: self.ended_at,
            recorded_at: self.recorded_at,
            provider_run_id: self.provider_run_id(),
            provider_run_id_issuer: self.provider_run_id_issuer(),
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Serialize)]
struct ActorSeal {
    session_token: String,
    agent_principal_id: String,
    agent_instance_id: String,
    role: String,
    state: String,
    runtime_attestation: RuntimeAttestationRow,
}

#[derive(Debug, Serialize)]
struct Blocker {
    code: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct LandingAuthorization {
    schema_version: &'static str,
    accepted: bool,
    queue_id: String,
    artifact_digest: String,
    review_id: String,
    findings_digest: String,
    claim_fence_seq: i64,
    verifier: String,
    verdict_digest: String,
    apply_verification_seal_digest: String,
    seal_digest: String,
    head_commit: String,
    target_ref: String,
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
    let artifact_digest = req
        .artifact_digest
        .clone()
        .or_else(|| required_string(&landing, "artifact_digest").ok())
        .ok_or_else(|| ApplyProofError::Verification("missing artifact digest".into()))?;
    let review_id = req
        .review_id
        .clone()
        .or_else(|| required_string(&landing, "review_id").ok())
        .ok_or_else(|| ApplyProofError::Verification("missing review id".into()))?;
    let queue_id = req
        .queue_id
        .clone()
        .or_else(|| required_string(&landing, "queue_id").ok())
        .ok_or_else(|| ApplyProofError::Verification("missing queue id".into()))?;
    let findings_digest = req
        .reviewer_findings_digest
        .clone()
        .or_else(|| required_string(&landing, "reviewer_findings_digest").ok())
        .ok_or_else(|| ApplyProofError::Verification("missing reviewer findings digest".into()))?;

    let conn = Connection::open(&req.covey_db)?;
    let artifact = load_artifact(&conn, &artifact_digest)?;
    let review = load_review(&conn, &review_id)?;
    let queue = load_queue(&conn, &queue_id)?;
    let applied_claim_fence_seq = queue.applied_claim_fence_seq();
    let (apply_verification, apply_verification_lookup_error) = match applied_claim_fence_seq {
        Some(claim_fence_seq) => {
            let apply_verification = load_apply_verification(
                &conn,
                &queue_id,
                &artifact_digest,
                &review_id,
                &findings_digest,
                claim_fence_seq,
                &req.verifier,
                req.verdict_digest.as_deref(),
                req.apply_verification_seal_digest.as_deref(),
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
    let producer = load_session(&conn, &artifact.produced_by_session)?;
    let reviewer = load_session(&conn, &review.reviewer_session)?;
    let apply_gate_session_token = match &req.apply_gate_session_token {
        Some(token) => token.clone(),
        None => required_string(
            &read_json(&req.evidence_dir.join("apply-gate-identity.json"))?,
            "session_token",
        )?,
    };
    let apply_gate = load_session(&conn, &apply_gate_session_token)?;
    let producer_attestation = load_runtime_attestation(&conn, &producer.session_token)?;
    let reviewer_attestation = load_runtime_attestation(&conn, &reviewer.session_token)?;
    let apply_gate_attestation = load_runtime_attestation(&conn, &apply_gate.session_token)?;

    let artifact_file = req.evidence_dir.join(&req.artifact_file);
    let verdict_file = req.evidence_dir.join(&req.verdict_file);
    let success_file = req.evidence_dir.join(&req.success_file);
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
    checks.insert(
        "review_approved".into(),
        review.state == "decided" && review.verdict == "approve",
    );
    checks.insert(
        "review_findings_digest_matches".into(),
        review.findings_digest == findings_digest,
    );
    checks.insert(
        "queue_targets_artifact".into(),
        queue.artifact_digest == artifact_digest,
    );
    checks.insert(
        "queue_targets_reviewed_subtask".into(),
        queue.subtask_id == review.subtask_id,
    );
    checks.insert("queue_applied".into(), queue.state() == "applied");
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
            Some(apply.claim_fence_seq) == applied_claim_fence_seq,
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
            verdict_file.is_file() && blake3_file(&verdict_file)? == apply.verdict_digest,
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
    checks.insert("worker_role_verified".into(), producer.role == "executor");
    checks.insert("reviewer_role_verified".into(), reviewer.role == "reviewer");
    checks.insert(
        "apply_gate_role_verified".into(),
        apply_gate.role == "apply_gate",
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
                .map(|contents| contents.contains(text))
                .unwrap_or(false)
        }),
    );

    let mut mission_contract = None;
    if req.mission_packet_file.is_some() || req.enforce_promoted_mission_identity_contract {
        let path = resolve_optional_file(&req.evidence_dir, req.mission_packet_file.as_deref());
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
        ("mainline_ref", Value::String(req.mainline_ref.clone())),
        ("head", Value::String(head.clone())),
        ("mainline_commit", Value::String(mainline.clone())),
        ("subtask_id", option_string(req.subtask_id.clone())),
        ("artifact_digest", Value::String(artifact_digest.clone())),
        ("review_id", Value::String(review_id.clone())),
        ("queue_id", Value::String(queue_id.clone())),
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
            option_string(req.subject_ref.clone()),
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

    let seal_digest = format!("blake3:{}", blake3_bytes(&canonical_json(&manifest)));
    insert_object(
        &mut manifest,
        "seal_digest",
        Value::String(seal_digest.clone()),
    );
    if blockers.is_empty()
        && let Some(apply) = &apply_verification
    {
        let target_ref = req
            .target_ref
            .clone()
            .unwrap_or_else(|| req.mainline_ref.clone());
        let auth = LandingAuthorization {
            schema_version: "codex_hook_landing_authorization.v1",
            accepted: true,
            queue_id: queue_id.clone(),
            artifact_digest: artifact_digest.clone(),
            review_id: review_id.clone(),
            findings_digest: findings_digest.clone(),
            claim_fence_seq: applied_claim_fence_seq
                .expect("accepted proof requires an applied ready queue fence"),
            verifier: apply.verifier.clone(),
            verdict_digest: apply.verdict_digest.clone(),
            apply_verification_seal_digest: apply.seal_digest.clone(),
            seal_digest: seal_digest.clone(),
            head_commit: head,
            target_ref,
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
        ("seal_digest", Value::String(seal_digest)),
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
    let digest = format!("blake3:{}", blake3_bytes(&canonical_json(&aggregate)));
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
    let digest = format!("blake3:{}", blake3_bytes(&canonical_json(&manifest)));
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
            if args.artifact_file == "feature.patch"
                && let Some(value) = file.artifact_file
            {
                args.artifact_file = value;
            }
            if args.verdict_file == "apply-gate-output.json"
                && let Some(value) = file.verdict_file
            {
                args.verdict_file = value;
            }
            if args.success_file == "full-suite-output.txt"
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
            mainline_ref: require_string_with_output(
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

fn require_string_with_output(
    value: Option<String>,
    name: &str,
    output: Option<PathBuf>,
) -> Result<String, ApplyProofError> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| request_error(format!("--{name} is required"), output))
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
            Ok(ArtifactRow {
                artifact_digest: row.get(0)?,
                produced_by_subtask_id: row.get(1)?,
                produced_by_session: row.get(2)?,
                manifest_path: row.get(3)?,
                changed_paths_digest: row.get(4)?,
            })
        },
    )
    .map_err(Into::into)
}

fn load_review(conn: &Connection, review_id: &str) -> Result<ReviewRow, ApplyProofError> {
    conn.query_row(
        "SELECT review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id, verdict, findings_digest, state FROM reviews WHERE review_id = ?1",
        params![review_id],
        |row| {
            Ok(ReviewRow {
                review_id: row.get(0)?,
                subtask_id: row.get(1)?,
                artifact_digest: row.get(2)?,
                reviewer_session: row.get(3)?,
                review_subtask_id: row.get(4)?,
                verdict: row.get(5)?,
                findings_digest: row.get(6)?,
                state: row.get(7)?,
            })
        },
    )
    .map_err(Into::into)
}

fn load_queue(conn: &Connection, queue_id: &str) -> Result<ReadyQueueRow, ApplyProofError> {
    conn.query_row(
        "SELECT queue_id, artifact_digest, subtask_id, state, claimed_by_session_token, claim_fence_seq FROM ready_queue WHERE queue_id = ?1",
        params![queue_id],
        |row| {
            ReadyQueueRow::from_db_parts(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
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
    Ok(ApplyVerificationRow {
        queue_id: row.get(0)?,
        artifact_digest: row.get(1)?,
        review_id: row.get(2)?,
        findings_digest: row.get(3)?,
        claim_fence_seq: row.get(4)?,
        verifier: row.get(5)?,
        verdict_digest: row.get(6)?,
        seal_digest: row.get(7)?,
        recorded_by_session: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn load_session(conn: &Connection, token: &str) -> Result<SessionRow, ApplyProofError> {
    conn.query_row(
        "SELECT session_token, agent_principal_id, agent_instance_id, role, state FROM sessions WHERE session_token = ?1",
        params![token],
        |row| {
            Ok(SessionRow {
                session_token: row.get(0)?,
                agent_principal_id: row.get(1)?,
                agent_instance_id: row.get(2)?,
                role: row.get(3)?,
                state: row.get(4)?,
            })
        },
    )
    .map_err(Into::into)
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
        && !attestation.provider.is_empty()
        && !attestation.model.is_empty()
        && !attestation.command_transcript_digest.is_empty()
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
        role: session.role.clone(),
        state: session.state.clone(),
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
        Value::String(format!("blake3:{}", blake3_bytes(&payload_bytes))),
    );
    insert_object(
        &mut result,
        "public_key_blake3",
        Value::String(format!("blake3:{}", blake3_bytes(public_key.as_bytes()))),
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
            Value::String(attestation.session_token.clone()),
        ),
        (
            "agent_principal_id",
            Value::String(attestation.agent_principal_id.clone()),
        ),
        (
            "agent_instance_id",
            Value::String(attestation.agent_instance_id.clone()),
        ),
        ("role", Value::String(attestation.role.clone())),
        ("provider", Value::String(attestation.provider.clone())),
        ("model", Value::String(attestation.model.clone())),
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
            Value::String(attestation.command_transcript_digest.clone()),
        ),
        ("started_at", Value::Number(attestation.started_at.into())),
        ("ended_at", Value::Number(attestation.ended_at.into())),
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
        subtask_id: Some(required_string(proof, "subtask_id")?),
        artifact_digest: Some(required_string(proof, "artifact_digest")?),
        review_id: Some(required_string(proof, "review_id")?),
        queue_id: Some(required_string(proof, "queue_id")?),
        reviewer_findings_digest: Some(required_string(proof, "reviewer_findings_digest")?),
        apply_gate_session_token: Some(required_string(proof, "apply_gate_session_token")?),
        verifier: proof_string(proof, "verifier").unwrap_or_else(|| "mutai-rs".into()),
        verdict_digest: proof_string(proof, "verdict_digest"),
        apply_verification_seal_digest: proof_string(proof, "apply_verification_seal_digest"),
        mainline_ref: Some(required_string(proof, "mainline_ref")?),
        subject_ref: proof_string(proof, "subject_ref"),
        artifact_file: proof_string(proof, "artifact_file")
            .unwrap_or_else(|| "feature.patch".into()),
        verdict_file: proof_string(proof, "verdict_file")
            .unwrap_or_else(|| "apply-gate-output.json".into()),
        success_file: proof_string(proof, "success_file")
            .unwrap_or_else(|| "full-suite-output.txt".into()),
        success_text: proof_string(proof, "success_text"),
        mission_packet_file: proof_string(proof, "mission_packet_file"),
        enforce_promoted_mission_identity_contract: proof_bool(
            proof,
            "enforce_promoted_mission_identity_contract",
        ),
        require_observed_process_ids: proof_bool(proof, "require_observed_process_ids"),
        require_host_signed_runtime_claims: proof_bool(proof, "require_host_signed_runtime_claims"),
        require_provider_run_ids: proof_bool(proof, "require_provider_run_ids"),
        trusted_provider_run_id_issuer: proof_string_list(
            proof,
            "trusted_provider_run_id_issuers",
        )?,
        forbidden_provider_run_id_issuer: proof_string_list(
            proof,
            "forbidden_provider_run_id_issuers",
        )?,
        target_ref: proof_string(proof, "target_ref"),
        output: Some(output.to_path_buf()),
    })
}

fn proof_string(proof: &Value, field: &str) -> Option<String> {
    proof
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
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
        .map(|(code, _)| Blocker {
            code: code.clone(),
            message: format!("required proof check failed: {code}"),
        })
        .collect()
}

fn resolve_optional_file(evidence_dir: &Path, value: Option<&str>) -> PathBuf {
    let path = value
        .map(PathBuf::from)
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

fn normalized_set(values: &[String]) -> BTreeSet<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn contains_all<const N: usize>(values: &BTreeSet<String>, required: [&str; N]) -> bool {
    required.iter().all(|value| values.contains(*value))
}

fn normalize_optional_runtime_field(
    value: Option<String>,
    field: &str,
) -> Result<Option<String>, ApplyProofError> {
    value
        .map(|value| {
            if value.trim().is_empty() {
                Err(ApplyProofError::Verification(format!(
                    "runtime attestation {field} must not be empty"
                )))
            } else {
                Ok(value)
            }
        })
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
    Ok(format!("blake3:{}", hasher.finalize().to_hex()))
}

fn blake3_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
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

    #[test]
    fn ready_queue_proof_row_requires_fence_for_applied_state() {
        let err = ReadyQueueRow::from_db_parts(
            "queue-1".into(),
            "blake3:artifact".into(),
            "subtask-1".into(),
            "applied".into(),
            None,
            None,
        )
        .expect_err("applied ready queue proof rows require a fence");

        assert!(
            err.to_string()
                .contains("applied ready queue row requires claim_fence_seq"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn ready_queue_proof_row_rejects_active_claimant_for_applied_state() {
        let err = ReadyQueueRow::from_db_parts(
            "queue-1".into(),
            "blake3:artifact".into(),
            "subtask-1".into(),
            "applied".into(),
            Some("session-1".into()),
            Some(7),
        )
        .expect_err("applied ready queue proof rows must not carry active claimants");

        assert!(
            err.to_string()
                .contains("applied ready queue row must not carry active claimant"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn ready_queue_proof_row_preserves_non_applied_rows_without_fabricated_fence() {
        let row = ReadyQueueRow::from_db_parts(
            "queue-1".into(),
            "blake3:artifact".into(),
            "subtask-1".into(),
            "queued".into(),
            None,
            None,
        )
        .expect("non-applied queue rows remain observable proof blockers");
        let value = serde_json::to_value(&row).expect("ready queue row should serialize");

        assert_eq!(row.state(), "queued");
        assert_eq!(row.applied_claim_fence_seq(), None);
        assert_eq!(value["claim_fence_seq"], Value::Null);
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
    fn runtime_attestation_proof_row_preserves_old_schema_provider_nulls() {
        let row = runtime_attestation_row(Some("1234".into()), None, None, None)
            .expect("old runtime rows without provider run columns remain observable");
        let value = serde_json::to_value(&row).expect("runtime row should serialize");

        assert_eq!(runtime_ref(&row), "process:1234");
        assert_eq!(value["provider_run_id"], Value::Null);
        assert_eq!(value["provider_run_id_issuer"], Value::Null);
    }
}

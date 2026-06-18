use clap::{Args, Subcommand, ValueEnum};

use covey::{ApplyGateBlockerKind, SettlementReconcileReason};

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
pub(crate) enum OpenSpecArchiveStatusStateArg {
    Blocked,
    Archived,
}

#[derive(Subcommand, Debug)]
pub(crate) enum QueueCommand {
    List(ListQueueArgs),
    Candidates(QueueCandidatesArgs),
    Enqueue(EnqueueQueueArgs),
    #[command(name = "reconcile-apply")]
    ReconcileApply(ReconcileApplyQueueArgs),
    #[command(name = "claim-next")]
    ClaimNext(ClaimNextQueueArgs),
    #[command(name = "mark-in-flight")]
    MarkInFlight(MarkInFlightArgs),
    #[command(name = "record-apply-verification")]
    RecordApplyVerification(RecordApplyVerificationArgs),
    #[command(name = "record-apply-gate-blocker")]
    RecordApplyGateBlocker(RecordApplyGateBlockerArgs),
    #[command(name = "record-settlement-reconcile-blocker")]
    RecordSettlementReconcileBlocker(RecordSettlementReconcileBlockerArgs),
    #[command(name = "verify-landing-authorization")]
    VerifyLandingAuthorization(VerifyLandingAuthorizationArgs),
    #[command(name = "record-landing-receipt")]
    RecordLandingReceipt(RecordLandingReceiptArgs),
    #[command(name = "mark-applied")]
    MarkApplied(MarkAppliedArgs),
    #[command(name = "record-openspec-archive-status")]
    RecordOpenSpecArchiveStatus(RecordOpenSpecArchiveStatusArgs),
    #[command(name = "list-openspec-archive-blockers")]
    ListOpenSpecArchiveBlockers(ListOpenSpecArchiveBlockersArgs),
    Supersede(SupersedeQueueArgs),
    Metrics,
}

#[derive(Args, Debug)]
pub(crate) struct QueueCandidatesArgs {
    #[arg(long, default_value_t = 100)]
    pub(crate) limit: usize,
}

#[derive(Args, Debug)]
pub(crate) struct ListQueueArgs {
    #[arg(long, default_value_t = 100)]
    pub(crate) limit: usize,
}

#[derive(Args, Debug)]
pub(crate) struct EnqueueQueueArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) artifact_digest: String,
    #[arg(long)]
    pub(crate) subtask_id: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ReconcileApplyQueueArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ClaimNextQueueArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) lease_duration_ms: i64,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct MarkInFlightArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) queue_id: String,
    #[arg(long)]
    pub(crate) lease_duration_ms: i64,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct RecordApplyVerificationArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) queue_id: String,
    #[arg(long)]
    pub(crate) artifact_digest: String,
    #[arg(long)]
    pub(crate) review_id: String,
    #[arg(long)]
    pub(crate) findings_digest: String,
    #[arg(long)]
    pub(crate) claim_fence_seq: i64,
    #[arg(long)]
    pub(crate) verifier: String,
    #[arg(long)]
    pub(crate) verdict_digest: String,
    #[arg(long)]
    pub(crate) seal_digest: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct RecordApplyGateBlockerArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) queue_id: String,
    #[arg(long)]
    pub(crate) artifact_digest: String,
    #[arg(long)]
    pub(crate) claim_fence_seq: i64,
    #[arg(long)]
    pub(crate) verifier: String,
    #[arg(long, value_enum)]
    pub(crate) blocker_kind: ApplyGateBlockerKindArg,
    #[arg(long)]
    pub(crate) reason: String,
    #[arg(long)]
    pub(crate) evidence_id: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
pub(crate) enum ApplyGateBlockerKindArg {
    AuthorityHold,
    GitApplyUncertainty,
}

impl From<ApplyGateBlockerKindArg> for ApplyGateBlockerKind {
    fn from(value: ApplyGateBlockerKindArg) -> Self {
        match value {
            ApplyGateBlockerKindArg::AuthorityHold => Self::AuthorityHold,
            ApplyGateBlockerKindArg::GitApplyUncertainty => Self::GitApplyUncertainty,
        }
    }
}

#[derive(Args, Debug)]
pub(crate) struct RecordSettlementReconcileBlockerArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) queue_id: String,
    #[arg(long)]
    pub(crate) artifact_digest: String,
    #[arg(long)]
    pub(crate) claim_fence_seq: i64,
    #[arg(long, value_enum)]
    pub(crate) reconcile_reason: SettlementReconcileReasonArg,
    #[arg(long)]
    pub(crate) authority_evidence_id: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
pub(crate) enum SettlementReconcileReasonArg {
    CommitUnknown,
    AuthorityLost,
    StaleFence,
    PartialPrepare,
    PartialFinalize,
    FailedCanonicalApply,
    DuplicateCompletion,
}

impl From<SettlementReconcileReasonArg> for SettlementReconcileReason {
    fn from(value: SettlementReconcileReasonArg) -> Self {
        match value {
            SettlementReconcileReasonArg::CommitUnknown => Self::CommitUnknown,
            SettlementReconcileReasonArg::AuthorityLost => Self::AuthorityLost,
            SettlementReconcileReasonArg::StaleFence => Self::StaleFence,
            SettlementReconcileReasonArg::PartialPrepare => Self::PartialPrepare,
            SettlementReconcileReasonArg::PartialFinalize => Self::PartialFinalize,
            SettlementReconcileReasonArg::FailedCanonicalApply => Self::FailedCanonicalApply,
            SettlementReconcileReasonArg::DuplicateCompletion => Self::DuplicateCompletion,
        }
    }
}

#[derive(Args, Debug)]
pub(crate) struct VerifyLandingAuthorizationArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) queue_id: String,
    #[arg(long)]
    pub(crate) artifact_digest: String,
    #[arg(long)]
    pub(crate) review_id: String,
    #[arg(long)]
    pub(crate) findings_digest: String,
    #[arg(long)]
    pub(crate) claim_fence_seq: i64,
    #[arg(long)]
    pub(crate) verifier: String,
    #[arg(long)]
    pub(crate) verdict_digest: String,
    #[arg(long)]
    pub(crate) seal_digest: String,
}

#[derive(Args, Debug)]
pub(crate) struct RecordLandingReceiptArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) queue_id: String,
    #[arg(long)]
    pub(crate) artifact_digest: String,
    #[arg(long)]
    pub(crate) claim_fence_seq: i64,
    #[arg(long)]
    pub(crate) target_ref: String,
    #[arg(long)]
    pub(crate) landed_commit_oid: String,
}

#[derive(Args, Debug)]
pub(crate) struct MarkAppliedArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) queue_id: String,
    #[arg(long)]
    pub(crate) claim_fence_seq: i64,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct RecordOpenSpecArchiveStatusArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) queue_id: String,
    #[arg(long)]
    pub(crate) artifact_digest: String,
    #[arg(long)]
    pub(crate) openspec_change_id: String,
    #[arg(long, value_enum)]
    pub(crate) state: OpenSpecArchiveStatusStateArg,
    #[arg(long)]
    pub(crate) blocked_reason: Option<String>,
    #[arg(long)]
    pub(crate) archive_proof_digest: Option<String>,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ListOpenSpecArchiveBlockersArgs {
    #[arg(long, default_value_t = 100)]
    pub(crate) limit: usize,
}

#[derive(Args, Debug)]
pub(crate) struct SupersedeQueueArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) queue_id: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

use clap::{Args, Subcommand};

#[derive(Subcommand, Debug)]
pub(crate) enum QueueCommand {
    List(ListQueueArgs),
    Enqueue(EnqueueQueueArgs),
    #[command(name = "claim-next")]
    ClaimNext(ClaimNextQueueArgs),
    #[command(name = "mark-in-flight")]
    MarkInFlight(MarkInFlightArgs),
    #[command(name = "record-apply-verification")]
    RecordApplyVerification(RecordApplyVerificationArgs),
    #[command(name = "verify-landing-authorization")]
    VerifyLandingAuthorization(VerifyLandingAuthorizationArgs),
    #[command(name = "record-landing-receipt")]
    RecordLandingReceipt(RecordLandingReceiptArgs),
    #[command(name = "mark-applied")]
    MarkApplied(MarkAppliedArgs),
    Supersede(SupersedeQueueArgs),
    Metrics,
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
pub(crate) struct SupersedeQueueArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) queue_id: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

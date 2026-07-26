use clap::{Args, Subcommand, ValueEnum};
use covey::{ArtifactKind, CompletionPolicy, ReviewVerdict, SubtaskKind};

#[derive(Subcommand, Debug)]
pub(crate) enum SubtaskCommand {
    Create(CreateSubtaskArgs),
    #[command(name = "claim-next")]
    ClaimNext(ClaimNextArgs),
    Claim(ClaimSubtaskArgs),
    Start(StartSubtaskArgs),
    Finish(FinishSubtaskArgs),
    Retry(RetrySubtaskArgs),
    Fail(FailSubtaskArgs),
    Abandon(AbandonSubtaskArgs),
    Status(SubtaskStatusArgs),
    Candidates(SubtaskCandidatesArgs),
    Availability(SubtaskAvailabilityArgs),
    Stuck(StuckSubtasksArgs),
}

#[derive(Subcommand, Debug)]
pub(crate) enum ClaimCommand {
    Renew(RenewClaimArgs),
    Release(ReleaseClaimArgs),
    Expiring(ExpiringClaimsArgs),
}

#[derive(Subcommand, Debug)]
pub(crate) enum ArtifactCommand {
    Publish(PublishArtifactArgs),
}

#[derive(Subcommand, Debug)]
pub(crate) enum ReviewCommand {
    Request(RequestReviewArgs),
    Decide(DecideReviewArgs),
    #[command(name = "permissive-land")]
    PermissiveLand(PermissiveLandReviewArgs),
}
#[derive(Args, Debug)]
pub(crate) struct CreateSubtaskArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) meta_task_id: String,
    #[arg(long)]
    pub(crate) title: String,
    #[arg(long)]
    pub(crate) kind: SubtaskKindArg,
    #[arg(long)]
    pub(crate) priority: i64,
    #[arg(long)]
    pub(crate) subtask_id: Option<String>,
    #[arg(long)]
    pub(crate) review_target_subtask_id: Option<String>,
    #[arg(long)]
    pub(crate) review_target_artifact_digest: Option<String>,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
    #[arg(long, value_enum)]
    pub(crate) completion_policy: Option<CompletionPolicyArg>,
    #[arg(long)]
    pub(crate) routing_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ClaimNextArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) lease_duration_ms: i64,
    #[arg(long)]
    pub(crate) meta_task_id: Option<String>,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
    #[arg(long)]
    pub(crate) routing_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ClaimSubtaskArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) subtask_id: String,
    #[arg(long)]
    pub(crate) lease_duration_ms: i64,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct StartSubtaskArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) claim_id: String,
    #[arg(long)]
    pub(crate) fence_seq: i64,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct FinishSubtaskArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) claim_id: String,
    #[arg(long)]
    pub(crate) fence_seq: i64,
    #[arg(long)]
    pub(crate) evidence_digest: String,
    #[arg(long)]
    pub(crate) summary: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct RetrySubtaskArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) claim_id: String,
    #[arg(long)]
    pub(crate) fence_seq: i64,
    #[arg(long)]
    pub(crate) evidence_digest: String,
    #[arg(long)]
    pub(crate) failure_code: String,
    #[arg(long)]
    pub(crate) summary: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct FailSubtaskArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) claim_id: String,
    #[arg(long)]
    pub(crate) fence_seq: i64,
    #[arg(long)]
    pub(crate) evidence_digest: String,
    #[arg(long)]
    pub(crate) failure_code: String,
    #[arg(long)]
    pub(crate) summary: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct AbandonSubtaskArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) claim_id: String,
    #[arg(long)]
    pub(crate) fence_seq: i64,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct SubtaskStatusArgs {
    #[arg(long)]
    pub(crate) subtask_id: String,
}

#[derive(Args, Debug)]
pub(crate) struct SubtaskCandidatesArgs {
    #[arg(long)]
    pub(crate) role: CandidateRoleArg,
    #[arg(long, default_value_t = 100)]
    pub(crate) limit: usize,
    #[arg(long)]
    pub(crate) meta_task_id: Option<String>,
    #[arg(long)]
    pub(crate) routing_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct SubtaskAvailabilityArgs {
    #[arg(long)]
    pub(crate) meta_task_id: Option<String>,
    #[arg(long)]
    pub(crate) routing_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct StuckSubtasksArgs {
    #[arg(long)]
    pub(crate) older_than_ms: i64,
    #[arg(long, default_value_t = 100)]
    pub(crate) limit: usize,
}

#[derive(Args, Debug)]
pub(crate) struct RenewClaimArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) claim_id: String,
    #[arg(long)]
    pub(crate) fence_seq: i64,
    #[arg(long)]
    pub(crate) extend_by_ms: i64,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ReleaseClaimArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) claim_id: String,
    #[arg(long)]
    pub(crate) fence_seq: i64,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ExpiringClaimsArgs {
    #[arg(long)]
    pub(crate) within_ms: i64,
    #[arg(long, default_value_t = 100)]
    pub(crate) limit: usize,
}

#[derive(Args, Debug)]
pub(crate) struct PublishArtifactArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) claim_id: String,
    #[arg(long)]
    pub(crate) fence_seq: i64,
    #[arg(long)]
    pub(crate) artifact_digest: String,
    #[arg(long)]
    pub(crate) artifact_kind: ArtifactKindArg,
    #[arg(long)]
    pub(crate) base_rev: String,
    #[arg(long)]
    pub(crate) manifest_path: String,
    #[arg(long)]
    pub(crate) changed_paths_digest: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct RequestReviewArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) subtask_id: String,
    #[arg(long)]
    pub(crate) artifact_digest: String,
    #[arg(long, default_value_t = 1)]
    pub(crate) priority: i64,
    #[arg(long)]
    pub(crate) review_subtask_id: Option<String>,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct DecideReviewArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) review_id: String,
    #[arg(long)]
    pub(crate) claim_id: String,
    #[arg(long)]
    pub(crate) fence_seq: i64,
    #[arg(long)]
    pub(crate) verdict: ReviewVerdictArg,
    #[arg(long)]
    pub(crate) findings_digest: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct PermissiveLandReviewArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) review_id: String,
    #[arg(long)]
    pub(crate) claim_id: String,
    #[arg(long)]
    pub(crate) fence_seq: i64,
    #[arg(long)]
    pub(crate) artifact_digest: String,
    #[arg(long)]
    pub(crate) findings_digest: String,
    #[arg(long)]
    pub(crate) target_ref: String,
    #[arg(long)]
    pub(crate) landed_commit_oid: Option<String>,
    #[arg(long)]
    pub(crate) receipt_digest: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum SubtaskKindArg {
    Work,
    Review,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum CandidateRoleArg {
    Executor,
    Reviewer,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
pub(crate) enum CompletionPolicyArg {
    Direct,
    Reviewed,
    CanonicalApply,
}

impl From<CompletionPolicyArg> for CompletionPolicy {
    fn from(value: CompletionPolicyArg) -> Self {
        match value {
            CompletionPolicyArg::Direct => CompletionPolicy::Direct,
            CompletionPolicyArg::Reviewed => CompletionPolicy::Reviewed,
            CompletionPolicyArg::CanonicalApply => CompletionPolicy::CanonicalApply,
        }
    }
}

impl From<CandidateRoleArg> for covey::SessionRole {
    fn from(value: CandidateRoleArg) -> Self {
        match value {
            CandidateRoleArg::Executor => covey::SessionRole::Executor,
            CandidateRoleArg::Reviewer => covey::SessionRole::Reviewer,
        }
    }
}

impl From<SubtaskKindArg> for SubtaskKind {
    fn from(value: SubtaskKindArg) -> Self {
        match value {
            SubtaskKindArg::Work => SubtaskKind::Work,
            SubtaskKindArg::Review => SubtaskKind::Review,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum ArtifactKindArg {
    PatchBundle,
    IsolatedCommitRef,
    TreeBundle,
    FindingsBundle,
    VerificationBundle,
}

impl From<ArtifactKindArg> for ArtifactKind {
    fn from(value: ArtifactKindArg) -> Self {
        match value {
            ArtifactKindArg::PatchBundle => ArtifactKind::PatchBundle,
            ArtifactKindArg::IsolatedCommitRef => ArtifactKind::IsolatedCommitRef,
            ArtifactKindArg::TreeBundle => ArtifactKind::TreeBundle,
            ArtifactKindArg::FindingsBundle => ArtifactKind::FindingsBundle,
            ArtifactKindArg::VerificationBundle => ArtifactKind::VerificationBundle,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum ReviewVerdictArg {
    Approve,
    ChangesRequested,
    Blocked,
}

impl From<ReviewVerdictArg> for ReviewVerdict {
    fn from(value: ReviewVerdictArg) -> Self {
        match value {
            ReviewVerdictArg::Approve => ReviewVerdict::Approve,
            ReviewVerdictArg::ChangesRequested => ReviewVerdict::ChangesRequested,
            ReviewVerdictArg::Blocked => ReviewVerdict::Blocked,
        }
    }
}

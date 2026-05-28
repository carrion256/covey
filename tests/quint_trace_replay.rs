use covey::{
    AbandonSubtaskReq, ArtifactDigest, ArtifactKind, CancelMetaTaskReq, Claim, ClaimId,
    ClaimNextReq, ClaimReadyQueueReq, ClaimState, ClaimSubtaskReq, CommandTranscriptDigest,
    ConflictResolutionState, CreateSubtaskRequest, DecideReviewReq, EnqueueForApplyReq,
    ExitSessionReq, FenceSeq, HeartbeatReq, LeaseDeadlineMs, MarkAppliedReq, MarkInFlightReq,
    MetaTask, MetaTaskId, MetaTaskState, ModelId, OverlapQueryReq, ProviderId, PublishArtifactReq,
    ReadyQueueState, RecordApplyVerificationReq, RecordLandingReceiptReq,
    RecordRuntimeAttestationReq, RegisterSessionReq, ReleaseClaimReq, ReleaseReservationReq,
    RenewClaimReq, RenewReservationReq, RepoopsAuthoritySnapshotReq, RequestReservationReq,
    RequestReviewReq, Reservation, ReservationState, ResolveConflictReq, ReviewState,
    ReviewSubtask, ReviewTarget, ReviewVerdict, RuntimeAttestation, ScopeClass, Session,
    SessionHandle, SessionRole, SessionState, SessionToken, SettlementTarget, StartSubtaskReq,
    SubmitMetaTaskReq, SubtaskId, SubtaskLifecycle, SubtaskPriority, SupersedeQueueItemReq,
    TimestampMs, VerifyLandingAuthorizationReq, WorkSubtask,
    proof_apply::{
        ready_queue_proof_row_lifecycle_accepts_for_model,
        review_proof_row_lifecycle_accepts_for_model,
    },
};
use rstest::{fixture, rstest};
use serde::Deserialize;
use std::fmt;

const COVEY_REVIEW_FOLLOWUP_ITF: &str = include_str!("fixtures/quint/CoveyReviewFollowup.itf.json");
const COVEY_REVIEW_CLAIM_RECLAIM_ITF: &str =
    include_str!("fixtures/quint/CoveyReviewClaimReclaim.itf.json");
const COVEY_REVIEW_CLAIM_RECLAIM_CHANGES_REQUESTED_ITF: &str =
    include_str!("fixtures/quint/CoveyReviewClaimReclaimChangesRequested.itf.json");
const COVEY_CORE_LIFECYCLE_ITF: &str = include_str!("fixtures/quint/CoveyCoreLifecycle.itf.json");
const COVEY_STALE_CLAIM_RECOVERY_REAP_ITF: &str =
    include_str!("fixtures/quint/CoveyStaleClaimRecoveryReap.itf.json");
const COVEY_STALE_CLAIM_RECOVERY_LEASE_ITF: &str =
    include_str!("fixtures/quint/CoveyStaleClaimRecoveryLease.itf.json");
const COVEY_STALE_CLAIM_RECOVERY_EXIT_ITF: &str =
    include_str!("fixtures/quint/CoveyStaleClaimRecoveryExit.itf.json");
const COVEY_QUEUE_RESERVATION_ITF: &str =
    include_str!("fixtures/quint/CoveyQueueReservation.itf.json");
const COVEY_READY_QUEUE_CLAIM_SELECTION_ITF: &str =
    include_str!("fixtures/quint/CoveyReadyQueueClaimSelection.itf.json");
const COVEY_READY_QUEUE_METRICS_ITF: &str =
    include_str!("fixtures/quint/CoveyReadyQueueMetrics.itf.json");
const COVEY_RESERVATION_OVERLAP_ITF: &str =
    include_str!("fixtures/quint/CoveyReservationOverlap.itf.json");
const COVEY_APPLY_GATE_EVIDENCE_ITF: &str =
    include_str!("fixtures/quint/CoveyApplyGateEvidence.itf.json");
const COVEY_SESSION_META_TASK_ITF: &str =
    include_str!("fixtures/quint/CoveySessionMetaTask.itf.json");
const COVEY_SESSION_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveySessionRequestShape.itf.json");
const COVEY_SESSION_HANDLE_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveySessionHandleShape.itf.json");
const COVEY_META_TASK_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyMetaTaskRequestShape.itf.json");
const COVEY_CREATE_SUBTASK_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyCreateSubtaskRequestShape.itf.json");
const COVEY_CLAIM_ACQUISITION_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyClaimAcquisitionRequestShape.itf.json");
const COVEY_SUPERSEDE_QUEUE_ITEM_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveySupersedeQueueItemRequestShape.itf.json");
const COVEY_VIEW_ATTACHMENT_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyViewAttachmentShape.itf.json");
const COVEY_RUNTIME_ATTESTATION_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyRuntimeAttestationRequestShape.itf.json");
const COVEY_PUBLISH_ARTIFACT_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyPublishArtifactRequestShape.itf.json");
const COVEY_DECIDE_REVIEW_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyDecideReviewRequestShape.itf.json");
const COVEY_REQUEST_REVIEW_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyRequestReviewRequestShape.itf.json");
const COVEY_ENQUEUE_FOR_APPLY_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyEnqueueForApplyRequestShape.itf.json");
const COVEY_CLAIM_READY_QUEUE_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyClaimReadyQueueRequestShape.itf.json");
const COVEY_CLAIM_LIFECYCLE_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyClaimLifecycleRequestShape.itf.json");
const COVEY_MARK_IN_FLIGHT_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyMarkInFlightRequestShape.itf.json");
const COVEY_MARK_APPLIED_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyMarkAppliedRequestShape.itf.json");
const COVEY_APPLY_VERIFICATION_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyApplyVerificationRequestShape.itf.json");
const COVEY_LANDING_AUTHORIZATION_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyLandingAuthorizationRequestShape.itf.json");
const COVEY_LANDING_RECEIPT_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyLandingReceiptRequestShape.itf.json");
const COVEY_RESERVATION_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyReservationRequestShape.itf.json");
const COVEY_RESERVATION_LIFECYCLE_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyReservationLifecycleRequestShape.itf.json");
const COVEY_CONFLICT_RESOLUTION_REQUEST_ITF: &str =
    include_str!("fixtures/quint/CoveyConflictResolutionRequest.itf.json");
const COVEY_LANDING_RECEIPT_ITF: &str = include_str!("fixtures/quint/CoveyLandingReceipt.itf.json");
const COVEY_OPENSPEC_IMPORT_ITF: &str = include_str!("fixtures/quint/CoveyOpenSpecImport.itf.json");
const COVEY_BD_IMPORT_ITF: &str = include_str!("fixtures/quint/CoveyBdImport.itf.json");
const COVEY_BD_IMPORT_ITEM_OUTCOME_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyBdImportItemOutcomeShape.itf.json");
const COVEY_CLAIM_DEPENDENCY_GATE_ITF: &str =
    include_str!("fixtures/quint/CoveyClaimDependencyGate.itf.json");
const COVEY_MUTATION_IDEMPOTENCY_ITF: &str =
    include_str!("fixtures/quint/CoveyMutationIdempotency.itf.json");
const COVEY_EVENT_LOG_ITF: &str = include_str!("fixtures/quint/CoveyEventLog.itf.json");
const COVEY_REPOOPS_SNAPSHOT_ITF: &str =
    include_str!("fixtures/quint/CoveyRepoopsSnapshot.itf.json");
const COVEY_REPOOPS_SNAPSHOT_REQUEST_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyRepoopsSnapshotRequestShape.itf.json");
const COVEY_TRANSITION_MATRIX_ITF: &str =
    include_str!("fixtures/quint/CoveyTransitionMatrix.itf.json");
const COVEY_SUBTASK_DOMAIN_LIFECYCLE_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveySubtaskDomainLifecycleShape.itf.json");
const COVEY_APPLY_PROOF_ROW_LIFECYCLE_ITF: &str =
    include_str!("fixtures/quint/CoveyApplyProofRowLifecycle.itf.json");
const COVEY_RECORD_LIFECYCLE_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyRecordLifecycleShape.itf.json");
const COVEY_RUNTIME_ATTESTATION_RECORD_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyRuntimeAttestationRecordShape.itf.json");
const COVEY_RESERVATION_RECORD_SHAPE_ITF: &str =
    include_str!("fixtures/quint/CoveyReservationRecordShape.itf.json");

#[derive(Debug, Deserialize)]
struct ItfTrace {
    states: Vec<ItfState>,
}

#[derive(Debug, Deserialize)]
struct ItfState {
    m: ReviewFollowupState,
}

#[derive(Debug, Deserialize)]
struct ReviewClaimReclaimItfTrace {
    states: Vec<ReviewClaimReclaimItfState>,
}

#[derive(Debug, Deserialize)]
struct ReviewClaimReclaimItfState {
    s: ReviewClaimReclaimState,
}

#[derive(Debug, Deserialize)]
struct CoreItfTrace {
    states: Vec<CoreItfState>,
}

#[derive(Debug, Deserialize)]
struct CoreItfState {
    s: CoreLifecycleState,
}

#[derive(Debug, Deserialize)]
struct StaleClaimRecoveryItfTrace {
    states: Vec<StaleClaimRecoveryItfState>,
}

#[derive(Debug, Deserialize)]
struct StaleClaimRecoveryItfState {
    s: StaleClaimRecoveryState,
}

#[derive(Debug, Deserialize)]
struct QueueReservationItfTrace {
    states: Vec<QueueReservationItfState>,
}

#[derive(Debug, Deserialize)]
struct QueueReservationItfState {
    s: QueueReservationState,
}

#[derive(Debug, Deserialize)]
struct ReadyQueueClaimSelectionItfTrace {
    states: Vec<ReadyQueueClaimSelectionItfState>,
}

#[derive(Debug, Deserialize)]
struct ReadyQueueClaimSelectionItfState {
    s: ReadyQueueClaimSelectionState,
}

#[derive(Debug, Deserialize)]
struct ReadyQueueMetricsItfTrace {
    states: Vec<ReadyQueueMetricsItfState>,
}

#[derive(Debug, Deserialize)]
struct ReadyQueueMetricsItfState {
    s: ReadyQueueMetricsState,
}

#[derive(Debug, Deserialize)]
struct ReservationOverlapItfTrace {
    states: Vec<ReservationOverlapItfState>,
}

#[derive(Debug, Deserialize)]
struct ReservationOverlapItfState {
    s: ReservationOverlapState,
}

#[derive(Debug, Deserialize)]
struct RepoopsSnapshotItfTrace {
    states: Vec<RepoopsSnapshotItfState>,
}

#[derive(Debug, Deserialize)]
struct RepoopsSnapshotItfState {
    s: RepoopsSnapshotState,
}

#[derive(Debug, Deserialize)]
struct RepoopsSnapshotRequestShapeItfTrace {
    states: Vec<RepoopsSnapshotRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct RepoopsSnapshotRequestShapeItfState {
    s: RepoopsSnapshotRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct TransitionMatrixItfTrace {
    states: Vec<TransitionMatrixItfState>,
}

#[derive(Debug, Deserialize)]
struct TransitionMatrixItfState {
    s: TransitionMatrixState,
}

#[derive(Debug, Deserialize)]
struct SubtaskDomainLifecycleShapeItfTrace {
    states: Vec<SubtaskDomainLifecycleShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct SubtaskDomainLifecycleShapeItfState {
    s: SubtaskDomainLifecycleShapeState,
}

#[derive(Debug, Deserialize)]
struct ApplyProofRowLifecycleItfTrace {
    states: Vec<ApplyProofRowLifecycleItfState>,
}

#[derive(Debug, Deserialize)]
struct ApplyProofRowLifecycleItfState {
    s: ApplyProofRowLifecycleState,
}

#[derive(Debug, Deserialize)]
struct RecordLifecycleShapeItfTrace {
    states: Vec<RecordLifecycleShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct RecordLifecycleShapeItfState {
    s: RecordLifecycleShapeState,
}

#[derive(Debug, Deserialize)]
struct RuntimeAttestationRecordShapeItfTrace {
    states: Vec<RuntimeAttestationRecordShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct RuntimeAttestationRecordShapeItfState {
    s: RuntimeAttestationRecordShapeState,
}

#[derive(Debug, Deserialize)]
struct ReservationRecordShapeItfTrace {
    states: Vec<ReservationRecordShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct ReservationRecordShapeItfState {
    s: ReservationRecordShapeState,
}

#[derive(Debug, Deserialize)]
struct ApplyGateEvidenceItfTrace {
    states: Vec<ApplyGateEvidenceItfState>,
}

#[derive(Debug, Deserialize)]
struct ApplyGateEvidenceItfState {
    s: ApplyGateEvidenceState,
}

#[derive(Debug, Deserialize)]
struct SessionMetaTaskItfTrace {
    states: Vec<SessionMetaTaskItfState>,
}

#[derive(Debug, Deserialize)]
struct SessionMetaTaskItfState {
    s: SessionMetaTaskState,
}

#[derive(Debug, Deserialize)]
struct SessionRequestShapeItfTrace {
    states: Vec<SessionRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct SessionRequestShapeItfState {
    s: SessionRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct SessionHandleShapeItfTrace {
    states: Vec<SessionHandleShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct SessionHandleShapeItfState {
    s: SessionHandleShapeState,
}

#[derive(Debug, Deserialize)]
struct MetaTaskRequestShapeItfTrace {
    states: Vec<MetaTaskRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct MetaTaskRequestShapeItfState {
    s: MetaTaskRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct CreateSubtaskRequestShapeItfTrace {
    states: Vec<CreateSubtaskRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct CreateSubtaskRequestShapeItfState {
    s: CreateSubtaskRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct ClaimAcquisitionRequestShapeItfTrace {
    states: Vec<ClaimAcquisitionRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct ClaimAcquisitionRequestShapeItfState {
    s: ClaimAcquisitionRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct SupersedeQueueItemRequestShapeItfTrace {
    states: Vec<SupersedeQueueItemRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct SupersedeQueueItemRequestShapeItfState {
    s: SupersedeQueueItemRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct ViewAttachmentShapeItfTrace {
    states: Vec<ViewAttachmentShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct ViewAttachmentShapeItfState {
    s: ViewAttachmentShapeState,
}

#[derive(Debug, Deserialize)]
struct RuntimeAttestationRequestShapeItfTrace {
    states: Vec<RuntimeAttestationRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct RuntimeAttestationRequestShapeItfState {
    s: RuntimeAttestationRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct PublishArtifactRequestShapeItfTrace {
    states: Vec<PublishArtifactRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct PublishArtifactRequestShapeItfState {
    s: PublishArtifactRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct DecideReviewRequestShapeItfTrace {
    states: Vec<DecideReviewRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct DecideReviewRequestShapeItfState {
    s: DecideReviewRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct RequestReviewRequestShapeItfTrace {
    states: Vec<RequestReviewRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct RequestReviewRequestShapeItfState {
    s: RequestReviewRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct EnqueueForApplyRequestShapeItfTrace {
    states: Vec<EnqueueForApplyRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct EnqueueForApplyRequestShapeItfState {
    s: EnqueueForApplyRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct ClaimReadyQueueRequestShapeItfTrace {
    states: Vec<ClaimReadyQueueRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct ClaimReadyQueueRequestShapeItfState {
    s: ClaimReadyQueueRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct ClaimLifecycleRequestShapeItfTrace {
    states: Vec<ClaimLifecycleRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct ClaimLifecycleRequestShapeItfState {
    s: ClaimLifecycleRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct MarkInFlightRequestShapeItfTrace {
    states: Vec<MarkInFlightRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct MarkInFlightRequestShapeItfState {
    s: MarkInFlightRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct MarkAppliedRequestShapeItfTrace {
    states: Vec<MarkAppliedRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct MarkAppliedRequestShapeItfState {
    s: MarkAppliedRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct ApplyVerificationRequestShapeItfTrace {
    states: Vec<ApplyVerificationRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct ApplyVerificationRequestShapeItfState {
    s: ApplyVerificationRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct LandingAuthorizationRequestShapeItfTrace {
    states: Vec<LandingAuthorizationRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct LandingAuthorizationRequestShapeItfState {
    s: LandingAuthorizationRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct LandingReceiptRequestShapeItfTrace {
    states: Vec<LandingReceiptRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct LandingReceiptRequestShapeItfState {
    s: LandingReceiptRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct ReservationRequestShapeItfTrace {
    states: Vec<ReservationRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct ReservationRequestShapeItfState {
    s: ReservationRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct ReservationLifecycleRequestShapeItfTrace {
    states: Vec<ReservationLifecycleRequestShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct ReservationLifecycleRequestShapeItfState {
    s: ReservationLifecycleRequestShapeState,
}

#[derive(Debug, Deserialize)]
struct ConflictResolutionRequestItfTrace {
    states: Vec<ConflictResolutionRequestItfState>,
}

#[derive(Debug, Deserialize)]
struct ConflictResolutionRequestItfState {
    s: ConflictResolutionRequestState,
}

#[derive(Debug, Deserialize)]
struct LandingReceiptItfTrace {
    states: Vec<LandingReceiptItfState>,
}

#[derive(Debug, Deserialize)]
struct LandingReceiptItfState {
    s: LandingReceiptState,
}

#[derive(Debug, Deserialize)]
struct OpenSpecImportItfTrace {
    states: Vec<OpenSpecImportItfState>,
}

#[derive(Debug, Deserialize)]
struct OpenSpecImportItfState {
    s: OpenSpecImportState,
}

#[derive(Debug, Deserialize)]
struct BdImportItfTrace {
    states: Vec<BdImportItfState>,
}

#[derive(Debug, Deserialize)]
struct BdImportItfState {
    s: BdImportState,
}

#[derive(Debug, Deserialize)]
struct BdImportItemOutcomeShapeItfTrace {
    states: Vec<BdImportItemOutcomeShapeItfState>,
}

#[derive(Debug, Deserialize)]
struct BdImportItemOutcomeShapeItfState {
    s: BdImportItemOutcomeShapeState,
}

#[derive(Debug, Deserialize)]
struct ClaimDependencyGateItfTrace {
    states: Vec<ClaimDependencyGateItfState>,
}

#[derive(Debug, Deserialize)]
struct ClaimDependencyGateItfState {
    s: ClaimDependencyGateState,
}

#[derive(Debug, Deserialize)]
struct MutationIdempotencyItfTrace {
    states: Vec<MutationIdempotencyItfState>,
}

#[derive(Debug, Deserialize)]
struct MutationIdempotencyItfState {
    s: MutationIdempotencyState,
}

#[derive(Debug, Deserialize)]
struct EventLogItfTrace {
    states: Vec<EventLogItfState>,
}

#[derive(Debug, Deserialize)]
struct EventLogItfState {
    s: EventLogState,
}

#[derive(Debug, Deserialize)]
struct CoreLifecycleState {
    #[serde(deserialize_with = "deserialize_itf_variant")]
    subtask: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    claim: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    session: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    review: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    queue: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    fence: String,
    #[serde(rename = "activeSubtask")]
    active_subtask: bool,
    #[serde(rename = "artifactPresent")]
    artifact_present: bool,
    #[serde(rename = "reviewApproved")]
    review_approved: bool,
    #[serde(rename = "applyVerified")]
    apply_verified: bool,
    terminal: bool,
}

#[derive(Debug, Deserialize)]
struct StaleClaimRecoveryState {
    #[serde(rename = "oldSession", deserialize_with = "deserialize_itf_variant")]
    old_session: String,
    #[serde(rename = "newSession", deserialize_with = "deserialize_itf_variant")]
    new_session: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    claim: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    owner: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    subtask: String,
    #[serde(rename = "oldActiveSubtask")]
    old_active_subtask: bool,
    #[serde(rename = "newActiveSubtask")]
    new_active_subtask: bool,
    #[serde(rename = "currentFence", deserialize_with = "deserialize_itf_bigint")]
    current_fence: i64,
    #[serde(rename = "expiredFence", deserialize_with = "deserialize_itf_bigint")]
    expired_fence: i64,
    #[serde(rename = "staleReaped")]
    stale_reaped: bool,
    #[serde(rename = "leaseExpired")]
    lease_expired: bool,
    #[serde(rename = "exitedWithHeldClaim")]
    exited_with_held_claim: bool,
    #[serde(rename = "staleMutationRejected")]
    stale_mutation_rejected: bool,
}

#[derive(Debug, Deserialize)]
struct QueueReservationState {
    #[serde(deserialize_with = "deserialize_itf_variant")]
    queue: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    reservation: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    conflict: String,
    #[serde(rename = "queueFence", deserialize_with = "deserialize_itf_variant")]
    queue_fence: String,
    #[serde(rename = "queueClaimLive")]
    queue_claim_live: bool,
    #[serde(rename = "queueLeaseLive")]
    queue_lease_live: bool,
    #[serde(rename = "applyVerified")]
    apply_verified: bool,
    #[serde(rename = "subtaskReady")]
    subtask_ready: bool,
    #[serde(rename = "artifactMatches")]
    artifact_matches: bool,
    #[serde(rename = "metaSchedulable")]
    meta_schedulable: bool,
    #[serde(rename = "reservationLeaseLive")]
    reservation_lease_live: bool,
    #[serde(rename = "overlapDetected")]
    overlap_detected: bool,
    #[serde(rename = "conflictBound")]
    conflict_bound: bool,
    #[serde(
        rename = "conflictRankFloor",
        deserialize_with = "deserialize_itf_bigint"
    )]
    conflict_rank_floor: i64,
}

#[derive(Debug, Deserialize)]
struct ReadyQueueClaimSelectionState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_ready_queue_claim_selection_enum")]
    case: ReadyQueueClaimSelectionCase,
    #[serde(rename = "roleApplyGate")]
    role_apply_gate: bool,
    #[serde(
        rename = "headState",
        deserialize_with = "deserialize_ready_queue_claim_selection_enum"
    )]
    head_state: ReadyQueueClaimSelectionQueueState,
    #[serde(rename = "headExpired")]
    head_expired: bool,
    #[serde(rename = "headSubtaskReady")]
    head_subtask_ready: bool,
    #[serde(rename = "headArtifactMatches")]
    head_artifact_matches: bool,
    #[serde(rename = "headMetaSchedulable")]
    head_meta_schedulable: bool,
    #[serde(rename = "tailPresent")]
    tail_present: bool,
    #[serde(rename = "tailClaimable")]
    tail_claimable: bool,
    #[serde(
        rename = "headAction",
        deserialize_with = "deserialize_ready_queue_claim_selection_enum"
    )]
    head_action: ReadyQueueClaimSelectionHeadAction,
    #[serde(deserialize_with = "deserialize_ready_queue_claim_selection_enum")]
    result: ReadyQueueClaimSelectionResult,
    #[serde(rename = "selectedHead")]
    selected_head: bool,
    #[serde(rename = "selectedTail")]
    selected_tail: bool,
    #[serde(
        rename = "headFenceBefore",
        deserialize_with = "deserialize_itf_bigint"
    )]
    head_fence_before: i64,
    #[serde(rename = "headFenceAfter", deserialize_with = "deserialize_itf_bigint")]
    head_fence_after: i64,
    #[serde(
        rename = "tailFenceBefore",
        deserialize_with = "deserialize_itf_bigint"
    )]
    tail_fence_before: i64,
    #[serde(rename = "tailFenceAfter", deserialize_with = "deserialize_itf_bigint")]
    tail_fence_after: i64,
    #[serde(rename = "eventEmitted")]
    event_emitted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct ReadyQueueMetricsState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_ready_queue_metrics_enum")]
    case: ReadyQueueMetricsCase,
    #[serde(rename = "queuedCount", deserialize_with = "deserialize_itf_bigint")]
    queued_count: i64,
    #[serde(rename = "inFlightCount", deserialize_with = "deserialize_itf_bigint")]
    in_flight_count: i64,
    #[serde(rename = "queuedAgePresent")]
    queued_age_present: bool,
    #[serde(rename = "inFlightAgePresent")]
    in_flight_age_present: bool,
    #[serde(rename = "queuedAgeNonNegative")]
    queued_age_non_negative: bool,
    #[serde(rename = "inFlightAgeNonNegative")]
    in_flight_age_non_negative: bool,
    #[serde(
        rename = "queuedShape",
        deserialize_with = "deserialize_ready_queue_metrics_enum"
    )]
    queued_shape: ReadyQueueMetricsBucketShape,
    #[serde(
        rename = "inFlightShape",
        deserialize_with = "deserialize_ready_queue_metrics_enum"
    )]
    in_flight_shape: ReadyQueueMetricsBucketShape,
    #[serde(deserialize_with = "deserialize_ready_queue_metrics_enum")]
    outcome: ReadyQueueMetricsOutcome,
    #[serde(
        rename = "rejectReason",
        deserialize_with = "deserialize_ready_queue_metrics_enum"
    )]
    reject_reason: ReadyQueueMetricsRejectReason,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct ReservationOverlapState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(
        rename = "candidateScope",
        deserialize_with = "deserialize_itf_variant"
    )]
    candidate_scope: String,
    #[serde(rename = "existingScope", deserialize_with = "deserialize_itf_variant")]
    existing_scope: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    relation: String,
    #[serde(rename = "existingActive")]
    existing_active: bool,
    #[serde(rename = "candidatePathValid")]
    candidate_path_valid: bool,
    #[serde(rename = "candidateMembersPresent")]
    candidate_members_present: bool,
    #[serde(rename = "overlapReturned")]
    overlap_returned: bool,
    #[serde(rename = "conflictRecorded")]
    conflict_recorded: bool,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct RepoopsSnapshotState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "currentClaimValid")]
    current_claim_valid: bool,
    #[serde(rename = "requestedPathValid")]
    requested_path_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    subtask: String,
    #[serde(rename = "ownerReservationActive")]
    owner_reservation_active: bool,
    #[serde(rename = "foreignReservationActive")]
    foreign_reservation_active: bool,
    #[serde(rename = "reservationCoversRequestedPath")]
    reservation_covers_requested_path: bool,
    #[serde(rename = "ownerReservationInScope")]
    owner_reservation_in_scope: bool,
    #[serde(
        rename = "claimFactStatus",
        deserialize_with = "deserialize_itf_variant"
    )]
    claim_fact_status: String,
    #[serde(
        rename = "activeOwnershipToken",
        deserialize_with = "deserialize_itf_variant"
    )]
    active_ownership_token: String,
    #[serde(
        rename = "callerOwnershipToken",
        deserialize_with = "deserialize_itf_variant"
    )]
    caller_ownership_token: String,
    #[serde(rename = "scopeIncludesOwnerReservation")]
    scope_includes_owner_reservation: bool,
    #[serde(rename = "lockKind", deserialize_with = "deserialize_itf_variant")]
    lock_kind: String,
    #[serde(rename = "lockOwnerMatchesSession")]
    lock_owner_matches_session: bool,
    #[serde(rename = "lockClaimRefMatchesClaim")]
    lock_claim_ref_matches_claim: bool,
    #[serde(rename = "factSourcesUseTokenRefs")]
    fact_sources_use_token_refs: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
}

#[derive(Debug, Deserialize)]
struct RepoopsSnapshotRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "claimIdValid")]
    claim_id_valid: bool,
    #[serde(rename = "fenceSeqPositive")]
    fence_seq_positive: bool,
    #[serde(rename = "pathSetNonEmpty")]
    path_set_non_empty: bool,
    #[serde(rename = "everyPathValid")]
    every_path_valid: bool,
    #[serde(rename = "duplicateAfterNormalization")]
    duplicate_after_normalization: bool,
    #[serde(rename = "normalizesPathSyntax")]
    normalizes_path_syntax: bool,
    #[serde(rename = "serializedPathsNormalized")]
    serialized_paths_normalized: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct TransitionMatrixState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    object: String,
    #[serde(rename = "from", deserialize_with = "deserialize_itf_variant")]
    from_state: String,
    #[serde(rename = "to", deserialize_with = "deserialize_itf_variant")]
    to_state: String,
    #[serde(rename = "allowedByMatrix")]
    allowed_by_matrix: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct SubtaskDomainLifecycleShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    kind: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    lifecycle: String,
    #[serde(rename = "carriesArtifact")]
    carries_artifact: bool,
    #[serde(rename = "carriesActiveClaim")]
    carries_active_claim: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct ApplyProofRowLifecycleState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "rowKind", deserialize_with = "deserialize_itf_variant")]
    row_kind: String,
    #[serde(rename = "reviewState", deserialize_with = "deserialize_itf_variant")]
    review_state: String,
    #[serde(
        rename = "readyQueueState",
        deserialize_with = "deserialize_itf_variant"
    )]
    ready_queue_state: String,
    #[serde(rename = "verdictPresent")]
    verdict_present: bool,
    #[serde(rename = "findingsDigestPresent")]
    findings_digest_present: bool,
    #[serde(rename = "claimedBySessionPresent")]
    claimed_by_session_present: bool,
    #[serde(rename = "claimFence", deserialize_with = "deserialize_itf_variant")]
    claim_fence: String,
    #[serde(rename = "claimLeaseDeadlinePresent")]
    claim_lease_deadline_present: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct RecordLifecycleShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "rowKind", deserialize_with = "deserialize_itf_variant")]
    row_kind: String,
    #[serde(
        rename = "sessionLifecycle",
        deserialize_with = "deserialize_itf_variant"
    )]
    session_lifecycle: String,
    #[serde(
        rename = "metaTaskLifecycle",
        deserialize_with = "deserialize_itf_variant"
    )]
    meta_task_lifecycle: String,
    #[serde(
        rename = "claimLifecycle",
        deserialize_with = "deserialize_itf_variant"
    )]
    claim_lifecycle: String,
    #[serde(rename = "activeSubtaskPresent")]
    active_subtask_present: bool,
    #[serde(
        rename = "timestampShape",
        deserialize_with = "deserialize_itf_variant"
    )]
    timestamp_shape: String,
    #[serde(rename = "textShape", deserialize_with = "deserialize_itf_variant")]
    text_shape: String,
    #[serde(rename = "tickShape", deserialize_with = "deserialize_itf_variant")]
    tick_shape: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct RuntimeAttestationRecordShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(
        rename = "providerRunShape",
        deserialize_with = "deserialize_itf_variant"
    )]
    provider_run_shape: String,
    #[serde(
        rename = "runtimeIdentityShape",
        deserialize_with = "deserialize_itf_variant"
    )]
    runtime_identity_shape: String,
    #[serde(
        rename = "timestampShape",
        deserialize_with = "deserialize_itf_variant"
    )]
    timestamp_shape: String,
    #[serde(rename = "agentShape", deserialize_with = "deserialize_itf_variant")]
    agent_shape: String,
    #[serde(rename = "processPresent")]
    process_present: bool,
    #[serde(rename = "containerPresent")]
    container_present: bool,
    #[serde(rename = "providerRunRefPresent")]
    provider_run_ref_present: bool,
    #[serde(rename = "legacyProviderRunMissing")]
    legacy_provider_run_missing: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct ApplyGateEvidenceState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    queue: String,
    #[serde(rename = "queueOwnerIsApplyGate")]
    queue_owner_is_apply_gate: bool,
    #[serde(rename = "queueFenceMatches")]
    queue_fence_matches: bool,
    #[serde(rename = "queueLeaseLive")]
    queue_lease_live: bool,
    #[serde(rename = "queueArtifactMatchesRequest")]
    queue_artifact_matches_request: bool,
    #[serde(rename = "subtaskReadyForApply")]
    subtask_ready_for_apply: bool,
    #[serde(rename = "reviewExists")]
    review_exists: bool,
    #[serde(rename = "reviewDecided")]
    review_decided: bool,
    #[serde(rename = "reviewApproved")]
    review_approved: bool,
    #[serde(rename = "findingsDigestPresent")]
    findings_digest_present: bool,
    #[serde(rename = "artifactBoundToReview")]
    artifact_bound_to_review: bool,
    #[serde(rename = "producerReviewerPrincipalSeparated")]
    producer_reviewer_principal_separated: bool,
    #[serde(rename = "applyGateSeparatedFromProducer")]
    apply_gate_separated_from_producer: bool,
    #[serde(rename = "applyGateSeparatedFromReviewer")]
    apply_gate_separated_from_reviewer: bool,
    #[serde(rename = "producerAttested")]
    producer_attested: bool,
    #[serde(rename = "reviewerAttested")]
    reviewer_attested: bool,
    #[serde(rename = "applyGateAttested")]
    apply_gate_attested: bool,
    #[serde(rename = "producerReviewerRuntimeSeparated")]
    producer_reviewer_runtime_separated: bool,
    #[serde(rename = "producerApplyGateRuntimeSeparated")]
    producer_apply_gate_runtime_separated: bool,
    #[serde(rename = "reviewerApplyGateRuntimeSeparated")]
    reviewer_apply_gate_runtime_separated: bool,
    #[serde(rename = "producerReviewerProviderRunSeparated")]
    producer_reviewer_provider_run_separated: bool,
    #[serde(rename = "producerApplyGateProviderRunSeparated")]
    producer_apply_gate_provider_run_separated: bool,
    #[serde(rename = "reviewerApplyGateProviderRunSeparated")]
    reviewer_apply_gate_provider_run_separated: bool,
    #[serde(rename = "producerReviewerTranscriptSeparated")]
    producer_reviewer_transcript_separated: bool,
    #[serde(rename = "producerApplyGateTranscriptSeparated")]
    producer_apply_gate_transcript_separated: bool,
    #[serde(rename = "reviewerApplyGateTranscriptSeparated")]
    reviewer_apply_gate_transcript_separated: bool,
    #[serde(rename = "verificationAttempted")]
    verification_attempted: bool,
    #[serde(rename = "verificationRecorded")]
    verification_recorded: bool,
    #[serde(rename = "verificationReviewMatches")]
    verification_review_matches: bool,
    #[serde(rename = "markAppliedAttempted")]
    mark_applied_attempted: bool,
    #[serde(rename = "landingAuthorizationAccepted")]
    landing_authorization_accepted: bool,
    #[serde(rename = "landingAuthorizationArtifactMatches")]
    landing_authorization_artifact_matches: bool,
    #[serde(rename = "landingAuthorizationFenceMatches")]
    landing_authorization_fence_matches: bool,
    #[serde(rename = "landingAuthorizationVerificationMatches")]
    landing_authorization_verification_matches: bool,
    #[serde(rename = "receiptRecorded")]
    receipt_recorded: bool,
    #[serde(rename = "receiptArtifactMatches")]
    receipt_artifact_matches: bool,
    #[serde(rename = "receiptFenceMatches")]
    receipt_fence_matches: bool,
    #[serde(rename = "duplicateReceiptDivergent")]
    duplicate_receipt_divergent: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
}

#[derive(Debug, Deserialize)]
struct SessionMetaTaskState {
    #[serde(deserialize_with = "deserialize_itf_variant")]
    session: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    meta: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    subtasks: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    claim: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    queue: String,
    #[serde(rename = "heartbeatFresh")]
    heartbeat_fresh: bool,
}

#[derive(Debug, Deserialize)]
struct SessionRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    operation: String,
    #[serde(rename = "agentPrincipalValid")]
    agent_principal_valid: bool,
    #[serde(rename = "agentInstanceValid")]
    agent_instance_valid: bool,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct SessionHandleShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    role: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "agentPrincipalValid")]
    agent_principal_valid: bool,
    #[serde(rename = "agentInstanceValid")]
    agent_instance_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct MetaTaskRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    operation: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "promptTextValid")]
    prompt_text_valid: bool,
    #[serde(rename = "metaTaskIdValid")]
    meta_task_id_valid: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct CreateSubtaskRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "metaTaskIdValid")]
    meta_task_id_valid: bool,
    #[serde(rename = "optionalSubtaskIdPresent")]
    optional_subtask_id_present: bool,
    #[serde(rename = "optionalSubtaskIdValid")]
    optional_subtask_id_valid: bool,
    #[serde(rename = "titleValid")]
    title_valid: bool,
    #[serde(rename = "priorityValid")]
    priority_valid: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct ClaimAcquisitionRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    operation: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "leaseDurationPositive")]
    lease_duration_positive: bool,
    #[serde(rename = "metaTaskIdPresent")]
    meta_task_id_present: bool,
    #[serde(rename = "metaTaskIdValid")]
    meta_task_id_valid: bool,
    #[serde(rename = "subtaskIdValid")]
    subtask_id_valid: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct SupersedeQueueItemRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "queueIdValid")]
    queue_id_valid: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct ViewAttachmentShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "sessionHasActiveSubtaskId")]
    session_has_active_subtask_id: bool,
    #[serde(rename = "activeSubtaskViewPresent")]
    active_subtask_view_present: bool,
    #[serde(rename = "activeSubtaskMatchesSession")]
    active_subtask_matches_session: bool,
    #[serde(rename = "subtaskHasActiveClaimId")]
    subtask_has_active_claim_id: bool,
    #[serde(rename = "claimRowPresent")]
    claim_row_present: bool,
    #[serde(rename = "claimIdMatchesActive")]
    claim_id_matches_active: bool,
    #[serde(rename = "claimBelongsToSubtask")]
    claim_belongs_to_subtask: bool,
    #[serde(rename = "subtaskHasArtifactDigest")]
    subtask_has_artifact_digest: bool,
    #[serde(rename = "artifactRowPresent")]
    artifact_row_present: bool,
    #[serde(rename = "artifactDigestMatches")]
    artifact_digest_matches: bool,
    #[serde(rename = "artifactBelongsToSubtask")]
    artifact_belongs_to_subtask: bool,
    #[serde(rename = "reviewsBelongToSubtask")]
    reviews_belong_to_subtask: bool,
    #[serde(rename = "readyQueueBelongsToSubtask")]
    ready_queue_belongs_to_subtask: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct RuntimeAttestationRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "providerValid")]
    provider_valid: bool,
    #[serde(rename = "modelValid")]
    model_valid: bool,
    #[serde(rename = "providerRunIdValid")]
    provider_run_id_valid: bool,
    #[serde(rename = "providerRunIdIssuerValid")]
    provider_run_id_issuer_valid: bool,
    #[serde(rename = "processIdPresent")]
    process_id_present: bool,
    #[serde(rename = "processIdValid")]
    process_id_valid: bool,
    #[serde(rename = "containerIdPresent")]
    container_id_present: bool,
    #[serde(rename = "containerIdValid")]
    container_id_valid: bool,
    #[serde(rename = "commandTranscriptDigestValid")]
    command_transcript_digest_valid: bool,
    #[serde(rename = "startedAtNonNegative")]
    started_at_non_negative: bool,
    #[serde(rename = "endedAtNonNegative")]
    ended_at_non_negative: bool,
    #[serde(rename = "timestampsOrdered")]
    timestamps_ordered: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(rename = "flatSerializationPreservesIdentity")]
    flat_serialization_preserves_identity: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct PublishArtifactRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "artifactKind", deserialize_with = "deserialize_itf_variant")]
    artifact_kind: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "claimIdValid")]
    claim_id_valid: bool,
    #[serde(rename = "fenceSeqPositive")]
    fence_seq_positive: bool,
    #[serde(rename = "artifactDigestValid")]
    artifact_digest_valid: bool,
    #[serde(rename = "baseRevValid")]
    base_rev_valid: bool,
    #[serde(rename = "manifestPathPresent")]
    manifest_path_present: bool,
    #[serde(rename = "manifestPathNoControl")]
    manifest_path_no_control: bool,
    #[serde(rename = "changedPathsDigestValid")]
    changed_paths_digest_valid: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct DecideReviewRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    verdict: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "reviewIdValid")]
    review_id_valid: bool,
    #[serde(rename = "claimIdValid")]
    claim_id_valid: bool,
    #[serde(rename = "fenceSeqPositive")]
    fence_seq_positive: bool,
    #[serde(rename = "findingsDigestValid")]
    findings_digest_valid: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct RequestReviewRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "subtaskIdValid")]
    subtask_id_valid: bool,
    #[serde(rename = "artifactDigestValid")]
    artifact_digest_valid: bool,
    #[serde(rename = "explicitReviewSubtaskPresent")]
    explicit_review_subtask_present: bool,
    #[serde(rename = "reviewSubtaskIdValid")]
    review_subtask_id_valid: bool,
    #[serde(rename = "priorityValid")]
    priority_valid: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct EnqueueForApplyRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(
        rename = "settlementTarget",
        deserialize_with = "deserialize_itf_variant"
    )]
    settlement_target: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "artifactDigestValid")]
    artifact_digest_valid: bool,
    #[serde(rename = "subtaskIdValid")]
    subtask_id_valid: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct ClaimReadyQueueRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "leaseDurationPositive")]
    lease_duration_positive: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct ClaimLifecycleRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    operation: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "claimIdValid")]
    claim_id_valid: bool,
    #[serde(rename = "fenceSeqPositive")]
    fence_seq_positive: bool,
    #[serde(rename = "extendDurationPositive")]
    extend_duration_positive: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct MarkInFlightRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "queueIdValid")]
    queue_id_valid: bool,
    #[serde(rename = "leaseDurationPositive")]
    lease_duration_positive: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct MarkAppliedRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "queueIdValid")]
    queue_id_valid: bool,
    #[serde(rename = "fenceSeqPositive")]
    fence_seq_positive: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct ApplyVerificationRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "queueIdValid")]
    queue_id_valid: bool,
    #[serde(rename = "artifactDigestValid")]
    artifact_digest_valid: bool,
    #[serde(rename = "reviewIdValid")]
    review_id_valid: bool,
    #[serde(rename = "findingsDigestValid")]
    findings_digest_valid: bool,
    #[serde(rename = "fenceSeqPositive")]
    fence_seq_positive: bool,
    #[serde(rename = "verifierValid")]
    verifier_valid: bool,
    #[serde(rename = "verdictDigestValid")]
    verdict_digest_valid: bool,
    #[serde(rename = "sealDigestValid")]
    seal_digest_valid: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct LandingAuthorizationRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "queueIdValid")]
    queue_id_valid: bool,
    #[serde(rename = "artifactDigestValid")]
    artifact_digest_valid: bool,
    #[serde(rename = "reviewIdValid")]
    review_id_valid: bool,
    #[serde(rename = "findingsDigestValid")]
    findings_digest_valid: bool,
    #[serde(rename = "fenceSeqPositive")]
    fence_seq_positive: bool,
    #[serde(rename = "verifierValid")]
    verifier_valid: bool,
    #[serde(rename = "verdictDigestValid")]
    verdict_digest_valid: bool,
    #[serde(rename = "sealDigestValid")]
    seal_digest_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct LandingReceiptRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "queueIdValid")]
    queue_id_valid: bool,
    #[serde(rename = "artifactDigestValid")]
    artifact_digest_valid: bool,
    #[serde(rename = "fenceSeqPositive")]
    fence_seq_positive: bool,
    #[serde(rename = "targetRefValid")]
    target_ref_valid: bool,
    #[serde(rename = "landedCommitOidValid")]
    landed_commit_oid_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct ReservationRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    operation: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "ownerSubtaskIdValid")]
    owner_subtask_id_valid: bool,
    #[serde(rename = "leaseDurationPositive")]
    lease_duration_positive: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    scope: String,
    #[serde(rename = "scopeKeyPresent")]
    scope_key_present: bool,
    #[serde(rename = "scopeKeyNormalized")]
    scope_key_normalized: bool,
    #[serde(rename = "repoGlobalKeyCanonical")]
    repo_global_key_canonical: bool,
    #[serde(rename = "generatedMembersPresent")]
    generated_members_present: bool,
    #[serde(rename = "generatedMembersAllowed")]
    generated_members_allowed: bool,
    #[serde(rename = "generatedMembersNormalized")]
    generated_members_normalized: bool,
    #[serde(rename = "generatedMembersUnique")]
    generated_members_unique: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct ReservationLifecycleRequestShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    operation: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "reservationIdValid")]
    reservation_id_valid: bool,
    #[serde(rename = "extendDurationPositive")]
    extend_duration_positive: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct ReservationRecordShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    scope: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    lifecycle: String,
    #[serde(rename = "scopeKeyPresent")]
    scope_key_present: bool,
    #[serde(rename = "scopeKeyNormalized")]
    scope_key_normalized: bool,
    #[serde(rename = "repoGlobalKeyCanonical")]
    repo_global_key_canonical: bool,
    #[serde(rename = "generatedMembersPresent")]
    generated_members_present: bool,
    #[serde(rename = "generatedMembersAllowed")]
    generated_members_allowed: bool,
    #[serde(rename = "generatedMembersNormalized")]
    generated_members_normalized: bool,
    #[serde(rename = "generatedMembersUnique")]
    generated_members_unique: bool,
    #[serde(
        rename = "timestampShape",
        deserialize_with = "deserialize_itf_variant"
    )]
    timestamp_shape: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    accepted: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct ConflictResolutionRequestState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "currentState", deserialize_with = "deserialize_itf_variant")]
    current_state: String,
    #[serde(
        rename = "requestedState",
        deserialize_with = "deserialize_itf_variant"
    )]
    requested_state: String,
    #[serde(rename = "sessionTokenValid")]
    session_token_valid: bool,
    #[serde(rename = "conflictIdValid")]
    conflict_id_valid: bool,
    #[serde(rename = "idempotencyKeyValid")]
    idempotency_key_valid: bool,
    #[serde(rename = "requestShapeAccepted")]
    request_shape_accepted: bool,
    #[serde(rename = "transitionAllowed")]
    transition_allowed: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct ReviewFollowupState {
    #[serde(deserialize_with = "deserialize_review_followup_enum")]
    b0: ReviewFollowupBlockState,
    #[serde(deserialize_with = "deserialize_review_followup_enum")]
    b1: ReviewFollowupBlockState,
    #[serde(deserialize_with = "deserialize_review_followup_enum")]
    b2: ReviewFollowupBlockState,
    #[serde(deserialize_with = "deserialize_review_followup_enum")]
    b3: ReviewFollowupBlockState,
    #[serde(deserialize_with = "deserialize_review_followup_enum")]
    p0: ReviewFollowupBlockRef,
    #[serde(deserialize_with = "deserialize_review_followup_enum")]
    p1: ReviewFollowupBlockRef,
    #[serde(deserialize_with = "deserialize_review_followup_enum")]
    p2: ReviewFollowupBlockRef,
    #[serde(deserialize_with = "deserialize_review_followup_enum")]
    p3: ReviewFollowupBlockRef,
    #[serde(deserialize_with = "deserialize_review_followup_enum")]
    active: ReviewFollowupBlockRef,
    #[serde(rename = "nextBlock", deserialize_with = "deserialize_itf_bigint")]
    next_block: i64,
    #[serde(rename = "idleObserved")]
    idle_observed: bool,
    r0: bool,
    r1: bool,
    r2: bool,
    r3: bool,
}

#[derive(Debug, Deserialize)]
struct LandingReceiptState {
    #[serde(deserialize_with = "deserialize_itf_variant")]
    queue: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    receipt: String,
    #[serde(rename = "lastAttempt", deserialize_with = "deserialize_itf_variant")]
    last_attempt: String,
    #[serde(rename = "actorAuthorized")]
    actor_authorized: bool,
    #[serde(rename = "artifactMatches")]
    artifact_matches: bool,
    #[serde(rename = "fenceMatches")]
    fence_matches: bool,
    #[serde(rename = "receiptActorAuthorized")]
    receipt_actor_authorized: bool,
    #[serde(rename = "receiptArtifactMatches")]
    receipt_artifact_matches: bool,
    #[serde(rename = "receiptFenceMatches")]
    receipt_fence_matches: bool,
    #[serde(rename = "receiptTarget", deserialize_with = "deserialize_itf_variant")]
    receipt_target: String,
    #[serde(rename = "receiptCommit", deserialize_with = "deserialize_itf_variant")]
    receipt_commit: String,
    #[serde(
        rename = "attemptedTarget",
        deserialize_with = "deserialize_itf_variant"
    )]
    attempted_target: String,
    #[serde(
        rename = "attemptedCommit",
        deserialize_with = "deserialize_itf_variant"
    )]
    attempted_commit: String,
    #[serde(rename = "receiptCreatedByLastAttempt")]
    receipt_created_by_last_attempt: bool,
    #[serde(rename = "divergentAttemptRejected")]
    divergent_attempt_rejected: bool,
}

#[derive(Debug, Deserialize)]
struct OpenSpecImportState {
    #[serde(deserialize_with = "deserialize_itf_variant")]
    store: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    source: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    mode: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    role: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    diff: String,
    #[serde(
        rename = "conflictReason",
        deserialize_with = "deserialize_itf_variant"
    )]
    conflict_reason: String,
    #[serde(rename = "applyResult", deserialize_with = "deserialize_itf_variant")]
    apply_result: String,
    #[serde(rename = "metaWritten")]
    meta_written: bool,
    #[serde(rename = "subtaskWritten")]
    subtask_written: bool,
    #[serde(rename = "provenanceWritten")]
    provenance_written: bool,
    #[serde(rename = "importEventWritten")]
    import_event_written: bool,
    #[serde(rename = "dependenciesWritten")]
    dependencies_written: bool,
    #[serde(rename = "claimLive")]
    claim_live: bool,
    #[serde(rename = "claimCreatedByImport")]
    claim_created_by_import: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct BdImportState {
    #[serde(deserialize_with = "deserialize_itf_variant")]
    destination: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    role: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    source: String,
    #[serde(
        rename = "conflictReason",
        deserialize_with = "deserialize_itf_variant"
    )]
    conflict_reason: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "metaWritten")]
    meta_written: bool,
    #[serde(rename = "subtaskWritten")]
    subtask_written: bool,
    #[serde(rename = "eventWritten")]
    event_written: bool,
    #[serde(rename = "importedCount", deserialize_with = "deserialize_itf_bigint")]
    imported_count: i64,
    #[serde(rename = "skippedCount", deserialize_with = "deserialize_itf_bigint")]
    skipped_count: i64,
    #[serde(rename = "subtaskShape", deserialize_with = "deserialize_itf_variant")]
    subtask_shape: String,
    #[serde(rename = "itemOrder", deserialize_with = "deserialize_itf_variant")]
    item_order: String,
    #[serde(rename = "duplicateSkipHasSubtask")]
    duplicate_skip_has_subtask: bool,
    #[serde(rename = "invalidSkipHasSubtask")]
    invalid_skip_has_subtask: bool,
    #[serde(rename = "claimCreated")]
    claim_created: bool,
    #[serde(rename = "sessionActiveSubtaskSet")]
    session_active_subtask_set: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct BdImportItemOutcomeShapeState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    case: String,
    #[serde(rename = "subtaskIdPresent")]
    subtask_id_present: bool,
    #[serde(rename = "skipReason", deserialize_with = "deserialize_itf_variant")]
    skip_reason: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    outcome: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct ClaimDependencyGateState {
    #[serde(rename = "caseIndex", deserialize_with = "deserialize_itf_bigint")]
    case_index: i64,
    #[serde(
        rename = "path",
        deserialize_with = "deserialize_claim_dependency_enum"
    )]
    claim_path: ClaimDependencyPath,
    #[serde(deserialize_with = "deserialize_claim_dependency_enum")]
    role: ClaimDependencyRole,
    #[serde(deserialize_with = "deserialize_claim_dependency_enum")]
    session: ClaimDependencySession,
    #[serde(deserialize_with = "deserialize_claim_dependency_enum")]
    meta: ClaimDependencyMeta,
    #[serde(deserialize_with = "deserialize_claim_dependency_enum")]
    kind: ClaimDependencyKind,
    #[serde(deserialize_with = "deserialize_claim_dependency_enum")]
    lineage: ClaimDependencyLineage,
    #[serde(deserialize_with = "deserialize_claim_dependency_enum")]
    candidate: ClaimDependencyCandidate,
    #[serde(deserialize_with = "deserialize_claim_dependency_enum")]
    dependency: ClaimDependencyDependency,
    #[serde(deserialize_with = "deserialize_claim_dependency_enum")]
    decision: ClaimDependencyDecision,
    #[serde(
        rename = "rejectReason",
        deserialize_with = "deserialize_claim_dependency_enum"
    )]
    reject_reason: ClaimDependencyRejectReason,
    #[serde(rename = "dependencySatisfied")]
    dependency_satisfied: bool,
    #[serde(rename = "candidateSelected")]
    candidate_selected: bool,
    #[serde(rename = "claimCreated")]
    claim_created: bool,
    #[serde(rename = "subtaskClaimed")]
    subtask_claimed: bool,
    #[serde(rename = "sessionActiveSubtaskSet")]
    session_active_subtask_set: bool,
    #[serde(rename = "fenceIssued")]
    fence_issued: bool,
    evaluated: bool,
}

#[derive(Debug, Deserialize)]
struct MutationIdempotencyRecordState {
    present: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    actor: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    operation: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    key: String,
    #[serde(rename = "requestHash", deserialize_with = "deserialize_itf_variant")]
    request_hash: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    response: String,
    #[serde(rename = "responseJsonValid")]
    response_json_valid: bool,
}

#[derive(Debug, Deserialize)]
struct MutationIdempotencyState {
    r0: MutationIdempotencyRecordState,
    r1: MutationIdempotencyRecordState,
    #[serde(rename = "lastActor", deserialize_with = "deserialize_itf_variant")]
    last_actor: String,
    #[serde(rename = "lastOperation", deserialize_with = "deserialize_itf_variant")]
    last_operation: String,
    #[serde(rename = "lastKey", deserialize_with = "deserialize_itf_variant")]
    last_key: String,
    #[serde(
        rename = "lastRequestHash",
        deserialize_with = "deserialize_itf_variant"
    )]
    last_request_hash: String,
    #[serde(rename = "lastResponse", deserialize_with = "deserialize_itf_variant")]
    last_response: String,
    #[serde(rename = "lastClosureOk")]
    last_closure_ok: bool,
    #[serde(rename = "lastSerializeOk")]
    last_serialize_ok: bool,
    #[serde(rename = "lastOutcome", deserialize_with = "deserialize_itf_variant")]
    last_outcome: String,
    #[serde(rename = "sideEffects", deserialize_with = "deserialize_itf_bigint")]
    side_effects: i64,
    #[serde(rename = "recordWrites", deserialize_with = "deserialize_itf_bigint")]
    record_writes: i64,
    #[serde(
        rename = "lastSideEffectDelta",
        deserialize_with = "deserialize_itf_bigint"
    )]
    last_side_effect_delta: i64,
    #[serde(
        rename = "lastRecordWriteDelta",
        deserialize_with = "deserialize_itf_bigint"
    )]
    last_record_write_delta: i64,
    #[serde(rename = "identityMatchedBeforeAttempt")]
    identity_matched_before_attempt: bool,
}

#[derive(Debug, Deserialize)]
struct EventLogRecordState {
    present: bool,
    #[serde(deserialize_with = "deserialize_itf_bigint")]
    seq: i64,
    #[serde(rename = "eventType", deserialize_with = "deserialize_itf_variant")]
    event_type: String,
    #[serde(rename = "objectType", deserialize_with = "deserialize_itf_variant")]
    object_type: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    actor: String,
    #[serde(rename = "visibleSessionToken")]
    visible_session_token: bool,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    payload: String,
    readable: bool,
}

#[derive(Debug, Deserialize)]
struct EventLogState {
    e0: EventLogRecordState,
    e1: EventLogRecordState,
    #[serde(rename = "nextSeq", deserialize_with = "deserialize_itf_bigint")]
    next_seq: i64,
    #[serde(rename = "lastOutcome", deserialize_with = "deserialize_itf_variant")]
    last_outcome: String,
    #[serde(rename = "lastEventType", deserialize_with = "deserialize_itf_variant")]
    last_event_type: String,
    #[serde(
        rename = "lastObjectType",
        deserialize_with = "deserialize_itf_variant"
    )]
    last_object_type: String,
    #[serde(rename = "lastActor", deserialize_with = "deserialize_itf_variant")]
    last_actor: String,
    #[serde(rename = "lastToken", deserialize_with = "deserialize_itf_variant")]
    last_token: String,
    #[serde(rename = "lastPayload", deserialize_with = "deserialize_itf_variant")]
    last_payload: String,
    #[serde(rename = "lastMutationOk")]
    last_mutation_ok: bool,
    #[serde(rename = "lastPayloadMatches")]
    last_payload_matches: bool,
    #[serde(rename = "lastObjectMatches")]
    last_object_matches: bool,
    #[serde(rename = "lastActorValid")]
    last_actor_valid: bool,
    #[serde(rename = "lastReadable")]
    last_readable: bool,
    #[serde(
        rename = "lastSeqAssigned",
        deserialize_with = "deserialize_itf_bigint"
    )]
    last_seq_assigned: i64,
    #[serde(rename = "lastCountDelta", deserialize_with = "deserialize_itf_bigint")]
    last_count_delta: i64,
}

#[derive(Debug, Deserialize)]
struct ReviewClaimReclaimState {
    #[serde(deserialize_with = "deserialize_itf_variant")]
    review: String,
    #[serde(rename = "reviewSubtask", deserialize_with = "deserialize_itf_variant")]
    review_subtask: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    claim: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    owner: String,
    #[serde(rename = "currentFence", deserialize_with = "deserialize_itf_bigint")]
    current_fence: i64,
    #[serde(rename = "expiredFence", deserialize_with = "deserialize_itf_bigint")]
    expired_fence: i64,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    verdict: String,
    #[serde(rename = "artifactCurrent")]
    artifact_current: bool,
    #[serde(rename = "followupAvailable")]
    followup_available: bool,
    #[serde(rename = "followupCount", deserialize_with = "deserialize_itf_bigint")]
    followup_count: i64,
    #[serde(rename = "followupReviewBound")]
    followup_review_bound: bool,
    #[serde(rename = "followupSourceSubtaskBound")]
    followup_source_subtask_bound: bool,
    #[serde(rename = "followupSourceArtifactBound")]
    followup_source_artifact_bound: bool,
    #[serde(rename = "followupFindingsBound")]
    followup_findings_bound: bool,
    #[serde(rename = "followupCreatedByReviewer")]
    followup_created_by_reviewer: bool,
    #[serde(rename = "followupWorkAvailable")]
    followup_work_available: bool,
    #[serde(rename = "executorClaimedFollowup")]
    executor_claimed_followup: bool,
    #[serde(rename = "staleDecisionRejected")]
    stale_decision_rejected: bool,
    #[serde(rename = "duplicateDecisionRejected")]
    duplicate_decision_rejected: bool,
}

#[derive(Debug, Deserialize)]
struct ItfVariant {
    tag: String,
}

#[derive(Debug, Deserialize)]
struct ItfBigInt {
    #[serde(rename = "#bigint")]
    value: String,
}

trait ClaimDependencyEnum: Sized {
    fn from_itf_tag(tag: &str) -> Option<Self>;
}

trait ReadyQueueMetricsEnum: Sized {
    fn from_itf_tag(tag: &str) -> Option<Self>;
}

trait ReadyQueueClaimSelectionEnum: Sized {
    fn from_itf_tag(tag: &str) -> Option<Self>;
}

trait ReviewFollowupEnum: Sized {
    fn from_itf_tag(tag: &str) -> Option<Self>;
}

macro_rules! claim_dependency_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum $name {
            $($variant),+
        }

        impl ClaimDependencyEnum for $name {
            fn from_itf_tag(tag: &str) -> Option<Self> {
                match tag {
                    $(stringify!($variant) => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

macro_rules! ready_queue_metrics_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum $name {
            $($variant),+
        }

        impl ReadyQueueMetricsEnum for $name {
            fn from_itf_tag(tag: &str) -> Option<Self> {
                match tag {
                    $(stringify!($variant) => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

macro_rules! ready_queue_claim_selection_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum $name {
            $($variant),+
        }

        impl ReadyQueueClaimSelectionEnum for $name {
            fn from_itf_tag(tag: &str) -> Option<Self> {
                match tag {
                    $(stringify!($variant) => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

macro_rules! review_followup_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum $name {
            $($variant),+
        }

        impl ReviewFollowupEnum for $name {
            fn from_itf_tag(tag: &str) -> Option<Self> {
                match tag {
                    $(stringify!($variant) => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

ready_queue_claim_selection_enum!(ReadyQueueClaimSelectionCase {
    ValidHeadClaimed,
    InvalidHeadSupersededTailClaimed,
    MetaUnavailableHeadCancelledTailClaimed,
    ExpiredHeadRequeuedThenClaimed,
    ActiveInFlightHeadIgnoredTailClaimed,
    OnlyInvalidHeadNoClaim,
    OnlyMetaUnavailableHeadNoClaim,
    EmptyQueueNoClaim,
    WrongRoleRejected,
});
ready_queue_claim_selection_enum!(ReadyQueueClaimSelectionQueueState {
    Missing,
    Queued,
    InFlight,
});
ready_queue_claim_selection_enum!(ReadyQueueClaimSelectionHeadAction {
    NoHeadAction,
    ClaimedHead,
    SupersededHead,
    CancelledHead,
    RequeuedHead,
    IgnoredHead,
});
ready_queue_claim_selection_enum!(ReadyQueueClaimSelectionResult {
    NotEvaluated,
    ClaimHead,
    ClaimTail,
    NoClaim,
    Rejected,
});

ready_queue_metrics_enum!(ReadyQueueMetricsCase {
    EmptyBoth,
    QueuedNonEmpty,
    InFlightNonEmpty,
    BothNonEmpty,
    EmptyQueuedWithAge,
    QueuedMissingAge,
    QueuedNegativeAge,
    EmptyInFlightWithAge,
    InFlightMissingAge,
    InFlightNegativeAge,
});
ready_queue_metrics_enum!(ReadyQueueMetricsBucketShape {
    EmptyBucket,
    NonEmptyBucket,
    InvalidBucket,
});
ready_queue_metrics_enum!(ReadyQueueMetricsOutcome {
    NotEvaluated,
    Accepted,
    Rejected,
});
ready_queue_metrics_enum!(ReadyQueueMetricsRejectReason {
    NoReject,
    EmptyQueuedHasAge,
    NonEmptyQueuedMissingAge,
    NegativeQueuedAge,
    EmptyInFlightHasAge,
    NonEmptyInFlightMissingAge,
    NegativeInFlightAge,
});

claim_dependency_enum!(ClaimDependencyPath {
    ClaimNext,
    TargetedClaim,
});
claim_dependency_enum!(ClaimDependencyRole {
    Executor,
    Reviewer,
    Orchestrator,
});
claim_dependency_enum!(ClaimDependencySession {
    SessionFree,
    SessionOccupied,
    SessionInactive,
});
claim_dependency_enum!(ClaimDependencyMeta {
    MetaActive,
    MetaPlanning,
    MetaCompleted,
    MetaCancelled,
});
claim_dependency_enum!(ClaimDependencyKind { Work, Review });
claim_dependency_enum!(ClaimDependencyLineage {
    PlainCandidate,
    ChangesRequestedSourceCandidate,
    ReviewFollowupCandidate,
});
claim_dependency_enum!(ClaimDependencyCandidate {
    CandidateAvailable,
    CandidateClaimed,
    CandidateInProgress,
    NoCandidate,
});
claim_dependency_enum!(ClaimDependencyDependency {
    NoDependency,
    DepOpen,
    DepApproved,
    DepReadyForApply,
    DepApplied,
    DepDecided,
    DepChangesRequestedNoFollowup,
    DepChangesRequestedFollowupAvailable,
    DepChangesRequestedFollowupApproved,
    DepChangesRequestedFollowupReadyForApply,
    DepChangesRequestedFollowupApplied,
    DepChangesRequestedFollowupDecided,
});
claim_dependency_enum!(ClaimDependencyDecision {
    NotEvaluated,
    ClaimCreated,
    NoClaimableCandidate,
    Rejected,
});
claim_dependency_enum!(ClaimDependencyRejectReason {
    NoReject,
    WrongRole,
    SessionUnavailable,
    SessionAlreadyOccupied,
    MetaUnavailable,
    IllegalTransition,
    DependencyUnsatisfied,
});

review_followup_enum!(ReviewFollowupBlockRef {
    B0,
    B1,
    B2,
    B3,
    NoBlock,
});
review_followup_enum!(ReviewFollowupBlockState {
    Absent,
    Available,
    Claimed,
    InProgress,
    ReviewPending,
    Approved,
    ChangesRequested,
    ReadyForApply,
    Applied,
});

const REVIEW_FOLLOWUP_BLOCKS: [ReviewFollowupBlockRef; 4] = [
    ReviewFollowupBlockRef::B0,
    ReviewFollowupBlockRef::B1,
    ReviewFollowupBlockRef::B2,
    ReviewFollowupBlockRef::B3,
];

fn deserialize_itf_variant<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(ItfVariant::deserialize(deserializer)?.tag)
}

fn deserialize_itf_bigint<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    ItfBigInt::deserialize(deserializer)?
        .value
        .parse::<i64>()
        .map_err(serde::de::Error::custom)
}

fn deserialize_claim_dependency_enum<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: ClaimDependencyEnum,
{
    let tag = ItfVariant::deserialize(deserializer)?.tag;
    T::from_itf_tag(&tag)
        .ok_or_else(|| serde::de::Error::custom(format!("unknown claim dependency tag {tag}")))
}

fn deserialize_ready_queue_metrics_enum<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: ReadyQueueMetricsEnum,
{
    let tag = ItfVariant::deserialize(deserializer)?.tag;
    T::from_itf_tag(&tag)
        .ok_or_else(|| serde::de::Error::custom(format!("unknown ready-queue metrics tag {tag}")))
}

fn deserialize_ready_queue_claim_selection_enum<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: ReadyQueueClaimSelectionEnum,
{
    let tag = ItfVariant::deserialize(deserializer)?.tag;
    T::from_itf_tag(&tag).ok_or_else(|| {
        serde::de::Error::custom(format!("unknown ready-queue claim-selection tag {tag}"))
    })
}

fn deserialize_review_followup_enum<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: ReviewFollowupEnum,
{
    let tag = ItfVariant::deserialize(deserializer)?.tag;
    T::from_itf_tag(&tag)
        .ok_or_else(|| serde::de::Error::custom(format!("unknown review-followup tag {tag}")))
}

impl ReviewFollowupBlockRef {
    fn as_str(self) -> &'static str {
        match self {
            Self::B0 => "B0",
            Self::B1 => "B1",
            Self::B2 => "B2",
            Self::B3 => "B3",
            Self::NoBlock => "NoBlock",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::B0 => 0,
            Self::B1 => 1,
            Self::B2 => 2,
            Self::B3 => 3,
            Self::NoBlock => 4,
        }
    }
}

impl fmt::Display for ReviewFollowupBlockRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl ReviewFollowupBlockState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "Absent",
            Self::Available => "Available",
            Self::Claimed => "Claimed",
            Self::InProgress => "InProgress",
            Self::ReviewPending => "ReviewPending",
            Self::Approved => "Approved",
            Self::ChangesRequested => "ChangesRequested",
            Self::ReadyForApply => "ReadyForApply",
            Self::Applied => "Applied",
        }
    }
}

impl fmt::Display for ReviewFollowupBlockState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl ReviewFollowupState {
    fn status(&self, block: ReviewFollowupBlockRef) -> ReviewFollowupBlockState {
        match block {
            ReviewFollowupBlockRef::B0 => self.b0,
            ReviewFollowupBlockRef::B1 => self.b1,
            ReviewFollowupBlockRef::B2 => self.b2,
            ReviewFollowupBlockRef::B3 => self.b3,
            ReviewFollowupBlockRef::NoBlock => ReviewFollowupBlockState::Absent,
        }
    }

    fn parent(&self, block: ReviewFollowupBlockRef) -> ReviewFollowupBlockRef {
        match block {
            ReviewFollowupBlockRef::B0 => self.p0,
            ReviewFollowupBlockRef::B1 => self.p1,
            ReviewFollowupBlockRef::B2 => self.p2,
            ReviewFollowupBlockRef::B3 => self.p3,
            ReviewFollowupBlockRef::NoBlock => ReviewFollowupBlockRef::NoBlock,
        }
    }

    fn rejected(&self, block: ReviewFollowupBlockRef) -> bool {
        match block {
            ReviewFollowupBlockRef::B0 => self.r0,
            ReviewFollowupBlockRef::B1 => self.r1,
            ReviewFollowupBlockRef::B2 => self.r2,
            ReviewFollowupBlockRef::B3 => self.r3,
            ReviewFollowupBlockRef::NoBlock => false,
        }
    }
}

fn children_of(
    state: &ReviewFollowupState,
    block: ReviewFollowupBlockRef,
) -> Vec<ReviewFollowupBlockRef> {
    REVIEW_FOLLOWUP_BLOCKS
        .into_iter()
        .filter(|child| {
            state.parent(*child) == block
                && state.status(*child) != ReviewFollowupBlockState::Absent
        })
        .collect()
}

fn available_block_exists(state: &ReviewFollowupState) -> bool {
    REVIEW_FOLLOWUP_BLOCKS
        .into_iter()
        .any(|block| state.status(block) == ReviewFollowupBlockState::Available)
}

fn repairable_missing_followup(state: &ReviewFollowupState) -> bool {
    let Ok(next_index) = usize::try_from(state.next_block) else {
        return false;
    };
    let Some(candidate) = REVIEW_FOLLOWUP_BLOCKS.get(next_index).copied() else {
        return false;
    };
    if state.status(candidate) != ReviewFollowupBlockState::Absent {
        return false;
    }
    REVIEW_FOLLOWUP_BLOCKS.into_iter().any(|block| {
        state.status(block) == ReviewFollowupBlockState::ChangesRequested
            && children_of(state, block).is_empty()
    })
}

fn replay_review_followup_trace(trace: &ItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.m;
        for block in REVIEW_FOLLOWUP_BLOCKS {
            let status = state.status(block);
            let parent = state.parent(block);
            let children = children_of(state, block);
            if status == ReviewFollowupBlockState::Absent
                && parent != ReviewFollowupBlockRef::NoBlock
            {
                violations.push(format!(
                    "state[{index}]: absent block {} has parent {parent}",
                    block.as_str()
                ));
            }
            if state.rejected(block) {
                if status != ReviewFollowupBlockState::ChangesRequested {
                    violations.push(format!(
                        "state[{index}]: rejected block {} is {status}",
                        block.as_str()
                    ));
                }
                if children.len() != 1 {
                    violations.push(format!(
                        "state[{index}]: rejected block {} has {} followups",
                        block.as_str(),
                        children.len()
                    ));
                }
            }
            if children.len() > 1 {
                violations.push(format!(
                    "state[{index}]: block {} has forked followups",
                    block.as_str()
                ));
            }
            if parent != ReviewFollowupBlockRef::NoBlock && parent.index() >= block.index() {
                violations.push(format!(
                    "state[{index}]: followup {} does not point backward",
                    block.as_str()
                ));
            }
        }
        if state.idle_observed
            && (available_block_exists(state) || repairable_missing_followup(state))
        {
            violations.push(format!(
                "state[{index}]: idle observed while work or repair exists"
            ));
        }
        if state.active != ReviewFollowupBlockRef::NoBlock
            && !matches!(
                state.status(state.active),
                ReviewFollowupBlockState::Claimed | ReviewFollowupBlockState::InProgress
            )
        {
            violations.push(format!(
                "state[{index}]: active block {} is not claimed or in progress",
                state.active.as_str()
            ));
        }
    }
    violations
}

fn replay_review_claim_reclaim_trace(trace: &ReviewClaimReclaimItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if state.claim == "Held" && state.owner == "NoReviewer" {
            violations.push(format!(
                "state[{index}]: held review claim has no reviewer owner"
            ));
        }
        if state.claim != "Held" && state.owner != "NoReviewer" {
            violations.push(format!(
                "state[{index}]: non-held review claim retained owner"
            ));
        }
        if state.review == "InProgress"
            && !(state.review_subtask == "SubtaskInProgress" && state.claim == "Held")
        {
            violations.push(format!(
                "state[{index}]: in-progress review lacks held started review subtask"
            ));
        }
        if state.claim == "Expired"
            && state.review == "Requested"
            && state.review_subtask != "Available"
        {
            violations.push(format!(
                "state[{index}]: expired review claim did not reset to claimable"
            ));
        }
        if state.review == "Decided"
            && !(state.review_subtask == "SubtaskDecided"
                && state.claim == "Released"
                && state.owner == "NoReviewer")
        {
            violations.push(format!(
                "state[{index}]: decided review retained live claim state"
            ));
        }
        if state.claim == "Expired" && state.review == "Decided" {
            violations.push(format!(
                "state[{index}]: expired review claim decided review"
            ));
        }
        if state.stale_decision_rejected
            && !(state.review == "Requested" && state.review_subtask == "Available")
        {
            violations.push(format!(
                "state[{index}]: stale decision rejection mutated review state"
            ));
        }
        if matches!(state.verdict.as_str(), "ChangesRequested" | "Blocked")
            && !(state.followup_available
                && state.followup_count == 1
                && (state.followup_work_available || state.executor_claimed_followup))
        {
            violations.push(format!(
                "state[{index}]: non-approval review decision lacks follow-up"
            ));
        }
        if state.verdict == "Approve" && (state.followup_available || state.followup_count != 0) {
            violations.push(format!(
                "state[{index}]: approved review unexpectedly created follow-up"
            ));
        }
        if state.followup_count != 0
            && !matches!(state.verdict.as_str(), "ChangesRequested" | "Blocked")
        {
            violations.push(format!(
                "state[{index}]: follow-up exists without failed review decision"
            ));
        }
        if state.followup_count != 0
            && !(state.followup_review_bound
                && state.followup_source_subtask_bound
                && state.followup_source_artifact_bound
                && state.followup_findings_bound
                && state.followup_created_by_reviewer)
        {
            violations.push(format!(
                "state[{index}]: follow-up record lacks review/source/artifact/findings/reviewer binding"
            ));
        }
        if state.verdict == "ChangesRequested"
            && !(state.followup_work_available || state.executor_claimed_followup)
        {
            violations.push(format!(
                "state[{index}]: changes-requested follow-up is not executor claimable"
            ));
        }
        if state.verdict == "Blocked"
            && !(state.followup_work_available || state.executor_claimed_followup)
        {
            violations.push(format!(
                "state[{index}]: blocked follow-up is not executor claimable"
            ));
        }
        if state.executor_claimed_followup
            && !(state.followup_count == 1
                && matches!(state.verdict.as_str(), "ChangesRequested" | "Blocked"))
        {
            violations.push(format!(
                "state[{index}]: executor claimed follow-up without failed review"
            ));
        }
        if state.executor_claimed_followup && state.followup_work_available {
            violations.push(format!(
                "state[{index}]: executor follow-up claim did not consume availability"
            ));
        }
        if state.duplicate_decision_rejected && state.followup_count > 1 {
            violations.push(format!(
                "state[{index}]: duplicate review decision duplicated follow-up"
            ));
        }
        if state.review == "Decided" && state.verdict == "NoVerdict" {
            violations.push(format!("state[{index}]: decided review lacks verdict"));
        }
        if state.review == "Superseded"
            && (state.verdict != "NoVerdict"
                || state.followup_available
                || state.followup_count != 0)
        {
            violations.push(format!(
                "state[{index}]: superseded review decided or created follow-up"
            ));
        }
        if !state.artifact_current && state.review == "Decided" {
            violations.push(format!("state[{index}]: stale artifact review was decided"));
        }
        if state.stale_decision_rejected && state.followup_count != 0 {
            violations.push(format!(
                "state[{index}]: stale review decision created follow-up"
            ));
        }
        if state.claim == "Held" && state.current_fence <= state.expired_fence {
            violations.push(format!(
                "state[{index}]: reclaimed held review claim did not advance fence"
            ));
        }
    }
    violations
}

fn core_terminal_subtask(state: &CoreLifecycleState) -> bool {
    matches!(state.subtask.as_str(), "Applied" | "Abandoned")
}

fn core_claim_live_subtask(state: &CoreLifecycleState) -> bool {
    matches!(
        state.subtask.as_str(),
        "Claimed" | "InProgress" | "ArtifactPublished" | "ReviewPending"
    )
}

fn core_queue_open(state: &CoreLifecycleState) -> bool {
    matches!(state.queue.as_str(), "Queued" | "QueueInFlight")
}

fn queue_reservation_terminal_queue(state: &QueueReservationState) -> bool {
    matches!(state.queue.as_str(), "Applied" | "Superseded" | "Cancelled")
}

fn queue_reservation_conflict_rank(state: &QueueReservationState) -> i64 {
    match state.conflict.as_str() {
        "Acknowledged" => 1,
        "Resolved" => 2,
        _ => 0,
    }
}

fn session_meta_terminal_meta(state: &SessionMetaTaskState) -> bool {
    matches!(state.meta.as_str(), "Completed" | "MetaCancelled")
}

fn session_meta_open_queue(state: &SessionMetaTaskState) -> bool {
    matches!(state.queue.as_str(), "Queued" | "InFlight")
}

fn replay_session_meta_task_trace(trace: &SessionMetaTaskItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if state.claim == "Held" && state.session != "ActiveWithSubtask" {
            violations.push(format!(
                "state[{index}]: held claim is not bound to active session occupancy"
            ));
        }
        if state.session == "ActiveWithSubtask" && state.claim != "Held" {
            violations.push(format!(
                "state[{index}]: active subtask occupancy lacks held claim"
            ));
        }
        if matches!(state.session.as_str(), "Stale" | "Exited") && state.claim == "Held" {
            violations.push(format!(
                "state[{index}]: inactive session still owns held claim"
            ));
        }
        if matches!(state.session.as_str(), "Stale" | "Exited") && state.heartbeat_fresh {
            violations.push(format!(
                "state[{index}]: inactive session still has fresh heartbeat"
            ));
        }
        if session_meta_terminal_meta(state) && state.claim == "Held" {
            violations.push(format!(
                "state[{index}]: terminal meta-task still has held claim"
            ));
        }
        if session_meta_terminal_meta(state) && session_meta_open_queue(state) {
            violations.push(format!(
                "state[{index}]: terminal meta-task still has open ready queue"
            ));
        }
        if state.meta == "Completed" && state.subtasks != "TerminalSubtasks" {
            violations.push(format!(
                "state[{index}]: completed meta-task lacks terminal subtask summary"
            ));
        }
        if state.meta == "MetaCancelled" && state.subtasks != "TerminalSubtasks" {
            violations.push(format!(
                "state[{index}]: cancelled meta-task lacks terminal subtask summary"
            ));
        }
        if state.meta == "Planning" && state.subtasks != "NoSubtasks" {
            violations.push(format!("state[{index}]: planning meta-task has subtasks"));
        }
        if state.meta == "Active" && state.subtasks != "OpenSubtasks" {
            violations.push(format!(
                "state[{index}]: active meta-task lacks open subtask summary"
            ));
        }
        if state.meta == "NoMeta"
            && !(state.subtasks == "NoSubtasks"
                && state.claim == "NoClaim"
                && state.queue == "NoQueue")
        {
            violations.push(format!(
                "state[{index}]: missing meta-task still has work state"
            ));
        }
    }
    violations
}

fn session_request_expected_reject(state: &SessionRequestShapeState) -> &'static str {
    if state.operation == "RegisterSession" && !state.agent_principal_valid {
        "AgentPrincipalInvalid"
    } else if state.operation == "RegisterSession" && !state.agent_instance_valid {
        "AgentInstanceInvalid"
    } else if state.operation != "RegisterSession" && !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn session_request_actual_accepts(state: &SessionRequestShapeState) -> bool {
    let idempotency_key = if state.idempotency_key_valid {
        "idem-session"
    } else {
        " "
    };
    match state.operation.as_str() {
        "RegisterSession" => RegisterSessionReq::try_from_raw_parts(
            if state.agent_principal_valid {
                "agent-principal-1"
            } else {
                "agent principal 1"
            },
            if state.agent_instance_valid {
                "agent-instance-1"
            } else {
                ""
            },
            SessionRole::Executor,
            idempotency_key,
        )
        .is_ok(),
        "Heartbeat" => HeartbeatReq::try_from_raw_parts(
            if state.session_token_valid {
                "session-1"
            } else {
                "session 1"
            },
            idempotency_key,
        )
        .is_ok(),
        "ExitSession" => ExitSessionReq::try_from_raw_parts(
            if state.session_token_valid {
                "session-1"
            } else {
                "session 1"
            },
            idempotency_key,
        )
        .is_ok(),
        _ => false,
    }
}

fn replay_session_request_shape_trace(trace: &SessionRequestShapeItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: session request scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = session_request_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: session request reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: session request outcome disagrees with validation facts"
            ));
        }
        if session_request_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: session request parser disagrees with model"
            ));
        }
        if expected_accepted && !state.idempotency_key_valid {
            violations.push(format!(
                "state[{index}]: accepted session request lacks idempotency key"
            ));
        }
        if expected_accepted
            && state.operation == "RegisterSession"
            && (!state.agent_principal_valid || !state.agent_instance_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted register session lacks agent identity"
            ));
        }
        if expected_accepted && state.operation != "RegisterSession" && !state.session_token_valid {
            violations.push(format!(
                "state[{index}]: accepted session lifecycle request lacks session token"
            ));
        }
    }
    violations
}

fn session_handle_expected_reject(state: &SessionHandleShapeState) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.agent_principal_valid {
        "AgentPrincipalInvalid"
    } else if !state.agent_instance_valid {
        "AgentInstanceInvalid"
    } else {
        "NoReject"
    }
}

fn session_handle_role(state: &SessionHandleShapeState) -> SessionRole {
    match state.role.as_str() {
        "Reviewer" => SessionRole::Reviewer,
        "Orchestrator" => SessionRole::Orchestrator,
        _ => SessionRole::Executor,
    }
}

fn session_handle_actual_accepts(state: &SessionHandleShapeState) -> bool {
    SessionHandle::try_from_raw_parts(
        if state.session_token_valid {
            "session-1"
        } else {
            "session 1"
        },
        if state.agent_principal_valid {
            "agent-principal-1"
        } else {
            "agent principal 1"
        },
        if state.agent_instance_valid {
            "agent-instance-1"
        } else {
            ""
        },
        session_handle_role(state),
    )
    .is_ok()
}

fn replay_session_handle_shape_trace(trace: &SessionHandleShapeItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: session handle scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = session_handle_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: session handle reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: session handle outcome disagrees with validation facts"
            ));
        }
        if session_handle_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: session handle parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid
                || !state.agent_principal_valid
                || !state.agent_instance_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted session handle lacks validated identity"
            ));
        }
    }
    violations
}

fn meta_task_request_expected_reject(state: &MetaTaskRequestShapeState) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if state.operation == "SubmitMetaTask" && !state.prompt_text_valid {
        "PromptTextInvalid"
    } else if state.operation == "CancelMetaTask" && !state.meta_task_id_valid {
        "MetaTaskIdInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn meta_task_request_actual_accepts(state: &MetaTaskRequestShapeState) -> bool {
    let session_token = if state.session_token_valid {
        "session-1"
    } else {
        "session 1"
    };
    let idempotency_key = if state.idempotency_key_valid {
        "idem-meta-task"
    } else {
        " "
    };
    match state.operation.as_str() {
        "SubmitMetaTask" => SubmitMetaTaskReq::try_from_raw_parts(
            session_token,
            if state.prompt_text_valid {
                "do work"
            } else {
                " "
            },
            idempotency_key,
        )
        .is_ok(),
        "CancelMetaTask" => CancelMetaTaskReq::try_from_raw_parts(
            session_token,
            if state.meta_task_id_valid {
                "meta-1"
            } else {
                ""
            },
            idempotency_key,
        )
        .is_ok(),
        _ => false,
    }
}

fn replay_meta_task_request_shape_trace(trace: &MetaTaskRequestShapeItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: meta-task request scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = meta_task_request_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: meta-task request reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: meta-task request outcome disagrees with validation facts"
            ));
        }
        if meta_task_request_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: meta-task request parser disagrees with model"
            ));
        }
        if expected_accepted && (!state.session_token_valid || !state.idempotency_key_valid) {
            violations.push(format!(
                "state[{index}]: accepted meta-task request lacks session or idempotency key"
            ));
        }
        if expected_accepted && state.operation == "SubmitMetaTask" && !state.prompt_text_valid {
            violations.push(format!(
                "state[{index}]: accepted submit-meta-task request lacks prompt"
            ));
        }
        if expected_accepted && state.operation == "CancelMetaTask" && !state.meta_task_id_valid {
            violations.push(format!(
                "state[{index}]: accepted cancel-meta-task request lacks meta-task id"
            ));
        }
    }
    violations
}

fn create_subtask_request_expected_reject(state: &CreateSubtaskRequestShapeState) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.meta_task_id_valid {
        "MetaTaskIdInvalid"
    } else if !state.optional_subtask_id_valid {
        "SubtaskIdInvalid"
    } else if !state.title_valid {
        "SubtaskTitleInvalid"
    } else if !state.priority_valid {
        "PriorityInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn create_subtask_request_priority(state: &CreateSubtaskRequestShapeState) -> i64 {
    match state.case.as_str() {
        "NegativePriority" => -1,
        "PriorityTooHigh" => 1001,
        _ => 10,
    }
}

fn create_subtask_request_title(state: &CreateSubtaskRequestShapeState) -> &'static str {
    match state.case.as_str() {
        "BlankTitle" => "",
        "PaddedTitle" => " padded ",
        _ => "implement",
    }
}

fn create_subtask_request_actual_accepts(state: &CreateSubtaskRequestShapeState) -> bool {
    CreateSubtaskRequest::try_from_raw_parts(
        if state.session_token_valid {
            "session-1"
        } else {
            "session 1"
        },
        if state.meta_task_id_valid {
            "meta-1"
        } else {
            "meta 1"
        },
        if state.optional_subtask_id_present {
            Some(
                if state.optional_subtask_id_valid {
                    "subtask-1"
                } else {
                    "subtask 1"
                }
                .to_owned(),
            )
        } else {
            None
        },
        create_subtask_request_title(state),
        create_subtask_request_priority(state),
        if state.idempotency_key_valid {
            "idem-create-subtask"
        } else {
            " "
        },
    )
    .is_ok()
}

fn replay_create_subtask_request_shape_trace(
    trace: &CreateSubtaskRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: create-subtask request scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = create_subtask_request_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: create-subtask request reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: create-subtask request outcome disagrees with validation facts"
            ));
        }
        if create_subtask_request_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: create-subtask request parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid
                || !state.meta_task_id_valid
                || !state.idempotency_key_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted create-subtask request lacks session, meta-task, or idempotency key"
            ));
        }
        if expected_accepted
            && (!state.optional_subtask_id_valid || !state.title_valid || !state.priority_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted create-subtask request has invalid subtask payload"
            ));
        }
    }
    violations
}

fn claim_acquisition_expected_reject(state: &ClaimAcquisitionRequestShapeState) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if state.operation == "ClaimSubtask" && !state.subtask_id_valid {
        "SubtaskIdInvalid"
    } else if !state.lease_duration_positive {
        "LeaseDurationInvalid"
    } else if state.operation == "ClaimNextScoped" && !state.meta_task_id_valid {
        "MetaTaskIdInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn claim_acquisition_lease_duration(state: &ClaimAcquisitionRequestShapeState) -> i64 {
    match state.case.as_str() {
        "ClaimNextZeroLease" | "ClaimSubtaskZeroLease" => 0,
        "ClaimNextNegativeLease" | "ClaimSubtaskNegativeLease" => -1,
        _ => 30_000,
    }
}

fn claim_acquisition_actual_accepts(state: &ClaimAcquisitionRequestShapeState) -> bool {
    let session_token = if state.session_token_valid {
        "session-1"
    } else {
        "session 1"
    };
    let idempotency_key = if state.idempotency_key_valid {
        "idem-claim-acquisition"
    } else {
        " "
    };
    match state.operation.as_str() {
        "ClaimNext" => ClaimNextReq::try_from_raw_parts(
            session_token,
            claim_acquisition_lease_duration(state),
            idempotency_key,
        )
        .is_ok(),
        "ClaimNextScoped" => ClaimNextReq::try_from_raw_parts_scoped(
            session_token,
            claim_acquisition_lease_duration(state),
            if state.meta_task_id_present {
                Some(
                    if state.meta_task_id_valid {
                        "meta-1"
                    } else {
                        "meta 1"
                    }
                    .to_owned(),
                )
            } else {
                None
            },
            idempotency_key,
        )
        .is_ok(),
        "ClaimSubtask" => ClaimSubtaskReq::try_from_raw_parts(
            session_token,
            if state.subtask_id_valid {
                "subtask-1"
            } else {
                "subtask 1"
            },
            claim_acquisition_lease_duration(state),
            idempotency_key,
        )
        .is_ok(),
        _ => false,
    }
}

fn replay_claim_acquisition_request_shape_trace(
    trace: &ClaimAcquisitionRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: claim acquisition request scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = claim_acquisition_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: claim acquisition request reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: claim acquisition request outcome disagrees with validation facts"
            ));
        }
        if claim_acquisition_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: claim acquisition request parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid
                || !state.lease_duration_positive
                || !state.idempotency_key_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted claim acquisition request lacks session, lease, or idempotency key"
            ));
        }
        if expected_accepted
            && state.operation == "ClaimNextScoped"
            && (!state.meta_task_id_present || !state.meta_task_id_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted scoped claim-next request lacks meta-task id"
            ));
        }
        if expected_accepted && state.operation == "ClaimSubtask" && !state.subtask_id_valid {
            violations.push(format!(
                "state[{index}]: accepted claim-subtask request lacks subtask id"
            ));
        }
    }
    violations
}

fn supersede_queue_item_expected_reject(
    state: &SupersedeQueueItemRequestShapeState,
) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.queue_id_valid {
        "QueueIdInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn supersede_queue_item_actual_accepts(state: &SupersedeQueueItemRequestShapeState) -> bool {
    SupersedeQueueItemReq::try_from_raw_parts(
        if state.session_token_valid {
            "session-1"
        } else {
            "session 1"
        },
        if state.queue_id_valid { "queue-1" } else { "" },
        if state.idempotency_key_valid {
            "idem-supersede"
        } else {
            " "
        },
    )
    .is_ok()
}

fn replay_supersede_queue_item_request_shape_trace(
    trace: &SupersedeQueueItemRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: supersede queue item request scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = supersede_queue_item_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: supersede queue item request reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: supersede queue item request outcome disagrees with validation facts"
            ));
        }
        if supersede_queue_item_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: supersede queue item request parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid || !state.queue_id_valid || !state.idempotency_key_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted supersede queue item request lacks session, queue, or idempotency key"
            ));
        }
    }
    violations
}

fn view_attachment_expected_reject(state: &ViewAttachmentShapeState) -> &'static str {
    if state.session_has_active_subtask_id && !state.active_subtask_view_present {
        "SessionRequiresActiveSubtask"
    } else if state.session_has_active_subtask_id
        && state.active_subtask_view_present
        && !state.active_subtask_matches_session
    {
        "SessionActiveSubtaskMismatch"
    } else if !state.session_has_active_subtask_id && state.active_subtask_view_present {
        "SessionUnexpectedActiveSubtask"
    } else if state.subtask_has_active_claim_id && !state.claim_row_present {
        "SubtaskRequiresActiveClaim"
    } else if state.subtask_has_active_claim_id
        && state.claim_row_present
        && !state.claim_id_matches_active
    {
        "SubtaskClaimMismatch"
    } else if state.subtask_has_active_claim_id
        && state.claim_row_present
        && state.claim_id_matches_active
        && !state.claim_belongs_to_subtask
    {
        "SubtaskClaimForeign"
    } else if !state.subtask_has_active_claim_id && state.claim_row_present {
        "SubtaskUnexpectedClaimReject"
    } else if state.subtask_has_artifact_digest && !state.artifact_row_present {
        "SubtaskRequiresArtifact"
    } else if state.subtask_has_artifact_digest
        && state.artifact_row_present
        && !state.artifact_digest_matches
    {
        "SubtaskArtifactMismatch"
    } else if state.subtask_has_artifact_digest
        && state.artifact_row_present
        && state.artifact_digest_matches
        && !state.artifact_belongs_to_subtask
    {
        "SubtaskArtifactForeign"
    } else if !state.subtask_has_artifact_digest && state.artifact_row_present {
        "SubtaskUnexpectedArtifactReject"
    } else if !state.reviews_belong_to_subtask {
        "SubtaskReviewForeign"
    } else if !state.ready_queue_belongs_to_subtask {
        "SubtaskReadyQueueForeign"
    } else {
        "NoReject"
    }
}

fn replay_view_attachment_shape_trace(trace: &ViewAttachmentShapeItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: view attachment scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = view_attachment_expected_reject(state);
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: view attachment reject reason does not match first failed binding"
            ));
        }
        let accepted = state.outcome == "Accepted";
        if accepted != (expected_reject == "NoReject") {
            violations.push(format!(
                "state[{index}]: view attachment outcome disagrees with binding checks"
            ));
        }
        if accepted
            && !((state.session_has_active_subtask_id
                && state.active_subtask_view_present
                && state.active_subtask_matches_session)
                || (!state.session_has_active_subtask_id && !state.active_subtask_view_present))
        {
            violations.push(format!(
                "state[{index}]: accepted session status has invalid active subtask attachment"
            ));
        }
        if accepted
            && !((state.subtask_has_active_claim_id
                && state.claim_row_present
                && state.claim_id_matches_active
                && state.claim_belongs_to_subtask)
                || (!state.subtask_has_active_claim_id && !state.claim_row_present))
        {
            violations.push(format!(
                "state[{index}]: accepted subtask status has invalid claim attachment"
            ));
        }
        if accepted
            && !((state.subtask_has_artifact_digest
                && state.artifact_row_present
                && state.artifact_digest_matches
                && state.artifact_belongs_to_subtask)
                || (!state.subtask_has_artifact_digest && !state.artifact_row_present))
        {
            violations.push(format!(
                "state[{index}]: accepted subtask status has invalid artifact attachment"
            ));
        }
        if accepted && !(state.reviews_belong_to_subtask && state.ready_queue_belongs_to_subtask) {
            violations.push(format!(
                "state[{index}]: accepted subtask status has foreign collection attachment"
            ));
        }
    }
    violations
}

fn runtime_attestation_expected_reject(
    state: &RuntimeAttestationRequestShapeState,
) -> &'static str {
    if state.process_id_present && !state.process_id_valid {
        "InvalidProcessId"
    } else if state.container_id_present && !state.container_id_valid {
        "InvalidContainerId"
    } else if !state.process_id_present && !state.container_id_present {
        "MissingRuntimeIdentity"
    } else if !state.started_at_non_negative {
        "InvalidStartedAt"
    } else if !state.ended_at_non_negative {
        "InvalidEndedAt"
    } else if !state.timestamps_ordered {
        "InvertedTimeRange"
    } else if !state.provider_run_id_valid {
        "InvalidProviderRunId"
    } else if !state.provider_run_id_issuer_valid {
        "InvalidProviderRunIdIssuer"
    } else if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.provider_valid {
        "ProviderInvalid"
    } else if !state.model_valid {
        "ModelInvalid"
    } else if !state.command_transcript_digest_valid {
        "CommandTranscriptDigestInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn runtime_attestation_actual_shape(state: &RuntimeAttestationRequestShapeState) -> (bool, bool) {
    let started_at = if state.started_at_non_negative {
        if state.timestamps_ordered { 100 } else { 102 }
    } else {
        -1
    };
    let ended_at = if state.ended_at_non_negative { 101 } else { -1 };
    let process_id = if state.process_id_present {
        Some(if state.process_id_valid {
            "process-1"
        } else {
            " "
        })
    } else {
        None
    };
    let container_id = if state.container_id_present {
        Some(if state.container_id_valid {
            "container-1"
        } else {
            "container-1 "
        })
    } else {
        None
    };
    let raw = serde_json::json!({
        "session_token": if state.session_token_valid { "session-1" } else { "" },
        "provider": if state.provider_valid { "provider-1" } else { "provider 1" },
        "model": if state.model_valid { "model-1" } else { "model 1" },
        "provider_run_id": if state.provider_run_id_valid { "run-1" } else { " " },
        "provider_run_id_issuer": if state.provider_run_id_issuer_valid { "issuer-1" } else { " issuer-1" },
        "process_id": process_id,
        "container_id": container_id,
        "command_transcript_digest": if state.command_transcript_digest_valid { "blake3:transcript" } else { "transcript" },
        "started_at": started_at,
        "ended_at": ended_at,
        "idempotency_key": if state.idempotency_key_valid { "idem-1" } else { " " },
    });
    let Ok(req) = serde_json::from_value::<RecordRuntimeAttestationReq>(raw) else {
        return (false, false);
    };
    let serialized =
        serde_json::to_value(req).expect("valid runtime attestation request should serialize");
    let expected_process_id = if state.process_id_present {
        serde_json::json!("process-1")
    } else {
        serde_json::Value::Null
    };
    let expected_container_id = if state.container_id_present {
        serde_json::json!("container-1")
    } else {
        serde_json::Value::Null
    };
    let flat_identity_preserved = serialized["process_id"] == expected_process_id
        && serialized["container_id"] == expected_container_id;
    (true, flat_identity_preserved)
}

fn replay_runtime_attestation_request_shape_trace(
    trace: &RuntimeAttestationRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: runtime attestation request scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = runtime_attestation_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: runtime attestation request reject reason does not match parser order"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: runtime attestation request outcome disagrees with validation facts"
            ));
        }
        let (actual_accepted, actual_flat_identity_preserved) =
            runtime_attestation_actual_shape(state);
        if actual_accepted != expected_accepted {
            violations.push(format!(
                "state[{index}]: runtime attestation request parser disagrees with model"
            ));
        }
        if expected_accepted
            && state.flat_serialization_preserves_identity
            && !actual_flat_identity_preserved
        {
            violations.push(format!(
                "state[{index}]: runtime attestation request serialization lost flat identity"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid
                || !state.provider_valid
                || !state.model_valid
                || !state.provider_run_id_valid
                || !state.provider_run_id_issuer_valid
                || !state.command_transcript_digest_valid
                || !state.idempotency_key_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted runtime attestation request has invalid scalar field"
            ));
        }
        if expected_accepted
            && (!state.started_at_non_negative
                || !state.ended_at_non_negative
                || !state.timestamps_ordered)
        {
            violations.push(format!(
                "state[{index}]: accepted runtime attestation request has invalid time range"
            ));
        }
        if expected_accepted && !state.process_id_present && !state.container_id_present {
            violations.push(format!(
                "state[{index}]: accepted runtime attestation request lacks runtime identity"
            ));
        }
    }
    violations
}

fn publish_artifact_expected_reject(state: &PublishArtifactRequestShapeState) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.claim_id_valid {
        "ClaimIdInvalid"
    } else if !state.fence_seq_positive {
        "FenceSeqInvalid"
    } else if !state.artifact_digest_valid {
        "ArtifactDigestInvalid"
    } else if !state.base_rev_valid {
        "BaseRevInvalid"
    } else if !(state.manifest_path_present && state.manifest_path_no_control) {
        "ManifestPathInvalid"
    } else if !state.changed_paths_digest_valid {
        "ChangedPathsDigestInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn publish_artifact_kind(state: &PublishArtifactRequestShapeState) -> ArtifactKind {
    match state.artifact_kind.as_str() {
        "PatchBundle" => ArtifactKind::PatchBundle,
        "TreeBundle" => ArtifactKind::TreeBundle,
        artifact_kind => panic!("unexpected artifact kind from ITF trace: {artifact_kind}"),
    }
}

fn publish_artifact_actual_accepts(state: &PublishArtifactRequestShapeState) -> bool {
    PublishArtifactReq::try_from_raw_parts(
        if state.session_token_valid {
            "session-1"
        } else {
            ""
        },
        if state.claim_id_valid { "claim-1" } else { "" },
        if state.fence_seq_positive { 1 } else { 0 },
        if state.artifact_digest_valid {
            "blake3:artifact".to_owned()
        } else {
            "artifact".to_owned()
        },
        publish_artifact_kind(state),
        if state.base_rev_valid {
            "base".to_owned()
        } else {
            "base rev".to_owned()
        },
        if !state.manifest_path_present {
            String::new()
        } else if !state.manifest_path_no_control {
            "manifest\n.json".to_owned()
        } else {
            "manifest.json".to_owned()
        },
        if state.changed_paths_digest_valid {
            "blake3:paths".to_owned()
        } else {
            "paths".to_owned()
        },
        if state.idempotency_key_valid {
            "idem-artifact"
        } else {
            " "
        },
    )
    .is_ok()
}

fn replay_publish_artifact_request_shape_trace(
    trace: &PublishArtifactRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: publish artifact request scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = publish_artifact_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: publish artifact request reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: publish artifact request outcome disagrees with validation facts"
            ));
        }
        if publish_artifact_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: publish artifact request parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid || !state.claim_id_valid || !state.fence_seq_positive)
        {
            violations.push(format!(
                "state[{index}]: accepted publish artifact request has invalid claim/fence identity"
            ));
        }
        if expected_accepted
            && (!state.artifact_digest_valid
                || !state.base_rev_valid
                || !state.manifest_path_present
                || !state.manifest_path_no_control
                || !state.changed_paths_digest_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted publish artifact request has invalid artifact evidence"
            ));
        }
        if expected_accepted && !state.idempotency_key_valid {
            violations.push(format!(
                "state[{index}]: accepted publish artifact request lacks idempotency key"
            ));
        }
    }
    violations
}

fn decide_review_expected_reject(state: &DecideReviewRequestShapeState) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.review_id_valid {
        "ReviewIdInvalid"
    } else if !state.claim_id_valid {
        "ClaimIdInvalid"
    } else if !state.fence_seq_positive {
        "FenceSeqInvalid"
    } else if !state.findings_digest_valid {
        "FindingsDigestInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn decide_review_verdict(state: &DecideReviewRequestShapeState) -> ReviewVerdict {
    match state.verdict.as_str() {
        "Approve" => ReviewVerdict::Approve,
        "ChangesRequested" => ReviewVerdict::ChangesRequested,
        "Blocked" => ReviewVerdict::Blocked,
        verdict => panic!("unexpected review verdict from ITF trace: {verdict}"),
    }
}

fn decide_review_actual_accepts(state: &DecideReviewRequestShapeState) -> bool {
    DecideReviewReq::try_from_raw_parts(
        if state.session_token_valid {
            "session-reviewer"
        } else {
            "session reviewer"
        },
        if state.review_id_valid {
            "review-1".to_owned()
        } else {
            "review 1".to_owned()
        },
        if state.claim_id_valid {
            "claim-review"
        } else {
            ""
        },
        if state.fence_seq_positive { 1 } else { 0 },
        decide_review_verdict(state),
        if state.findings_digest_valid {
            "blake3:findings".to_owned()
        } else {
            "findings".to_owned()
        },
        if state.idempotency_key_valid {
            "idem-decision"
        } else {
            " "
        },
    )
    .is_ok()
}

fn replay_decide_review_request_shape_trace(
    trace: &DecideReviewRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: decide review scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = decide_review_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: decide review reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: decide review outcome disagrees with validation facts"
            ));
        }
        if decide_review_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: decide review parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid
                || !state.review_id_valid
                || !state.claim_id_valid
                || !state.fence_seq_positive)
        {
            violations.push(format!(
                "state[{index}]: accepted decide review has invalid review claim or fence"
            ));
        }
        if expected_accepted && (!state.findings_digest_valid || !state.idempotency_key_valid) {
            violations.push(format!(
                "state[{index}]: accepted decide review has invalid findings or idempotency key"
            ));
        }
    }
    violations
}

fn request_review_expected_reject(state: &RequestReviewRequestShapeState) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.subtask_id_valid {
        "SubtaskIdInvalid"
    } else if !state.artifact_digest_valid {
        "ArtifactDigestInvalid"
    } else if state.explicit_review_subtask_present && !state.review_subtask_id_valid {
        "ReviewSubtaskIdInvalid"
    } else if !state.priority_valid {
        "PriorityInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn request_review_priority(state: &RequestReviewRequestShapeState) -> i64 {
    match state.case.as_str() {
        "NegativePriority" => -1,
        "TooHighPriority" => 1001,
        _ => 7,
    }
}

fn request_review_actual_accepts(state: &RequestReviewRequestShapeState) -> bool {
    RequestReviewReq::try_from_raw_parts(
        if state.session_token_valid {
            "session-1"
        } else {
            ""
        },
        if state.subtask_id_valid {
            "subtask-1"
        } else {
            "subtask 1"
        },
        if state.artifact_digest_valid {
            "blake3:artifact"
        } else {
            "artifact"
        },
        if state.explicit_review_subtask_present {
            Some(
                if state.review_subtask_id_valid {
                    "review-subtask-1"
                } else {
                    "review subtask"
                }
                .to_owned(),
            )
        } else {
            None
        },
        request_review_priority(state),
        if state.idempotency_key_valid {
            "idem-review"
        } else {
            " "
        },
    )
    .is_ok()
}

fn replay_request_review_request_shape_trace(
    trace: &RequestReviewRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: request review scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = request_review_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: request review reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: request review outcome disagrees with validation facts"
            ));
        }
        if request_review_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: request review parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid
                || !state.subtask_id_valid
                || !state.artifact_digest_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted request review has invalid session, subtask, or artifact"
            ));
        }
        if expected_accepted
            && state.explicit_review_subtask_present
            && !state.review_subtask_id_valid
        {
            violations.push(format!(
                "state[{index}]: accepted request review has invalid explicit review subtask"
            ));
        }
        if expected_accepted && (!state.priority_valid || !state.idempotency_key_valid) {
            violations.push(format!(
                "state[{index}]: accepted request review has invalid priority or idempotency key"
            ));
        }
    }
    violations
}

fn enqueue_for_apply_expected_reject(state: &EnqueueForApplyRequestShapeState) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.artifact_digest_valid {
        "ArtifactDigestInvalid"
    } else if !state.subtask_id_valid {
        "SubtaskIdInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn enqueue_for_apply_actual_accepts(state: &EnqueueForApplyRequestShapeState) -> bool {
    EnqueueForApplyReq::try_from_raw_parts(
        if state.session_token_valid {
            "session-1"
        } else {
            ""
        },
        if state.artifact_digest_valid {
            "blake3:artifact".to_owned()
        } else {
            "artifact".to_owned()
        },
        if state.subtask_id_valid {
            "subtask-1".to_owned()
        } else {
            "subtask 1".to_owned()
        },
        SettlementTarget::Canonical,
        if state.idempotency_key_valid {
            "idem-enqueue"
        } else {
            " "
        },
    )
    .is_ok()
}

fn replay_enqueue_for_apply_request_shape_trace(
    trace: &EnqueueForApplyRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: enqueue for apply scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = enqueue_for_apply_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: enqueue for apply reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: enqueue for apply outcome disagrees with validation facts"
            ));
        }
        if enqueue_for_apply_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: enqueue for apply parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid
                || !state.artifact_digest_valid
                || !state.subtask_id_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted enqueue for apply has invalid session, artifact, or subtask"
            ));
        }
        if expected_accepted && state.settlement_target != "Canonical" {
            violations.push(format!(
                "state[{index}]: accepted enqueue for apply has non-canonical settlement target"
            ));
        }
        if expected_accepted && !state.idempotency_key_valid {
            violations.push(format!(
                "state[{index}]: accepted enqueue for apply lacks idempotency key"
            ));
        }
    }
    violations
}

fn claim_ready_queue_expected_reject(state: &ClaimReadyQueueRequestShapeState) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.lease_duration_positive {
        "LeaseDurationInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn claim_ready_queue_lease_duration(state: &ClaimReadyQueueRequestShapeState) -> i64 {
    match state.case.as_str() {
        "ZeroLeaseDuration" => 0,
        "NegativeLeaseDuration" => -1,
        _ => 30_000,
    }
}

fn claim_ready_queue_actual_accepts(state: &ClaimReadyQueueRequestShapeState) -> bool {
    ClaimReadyQueueReq::try_from_raw_parts(
        if state.session_token_valid {
            "session-1"
        } else {
            ""
        },
        claim_ready_queue_lease_duration(state),
        if state.idempotency_key_valid {
            "idem-claim-ready"
        } else {
            " "
        },
    )
    .is_ok()
}

fn replay_claim_ready_queue_request_shape_trace(
    trace: &ClaimReadyQueueRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: claim ready queue scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = claim_ready_queue_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: claim ready queue reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: claim ready queue outcome disagrees with validation facts"
            ));
        }
        if claim_ready_queue_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: claim ready queue parser disagrees with model"
            ));
        }
        if expected_accepted && (!state.session_token_valid || !state.lease_duration_positive) {
            violations.push(format!(
                "state[{index}]: accepted claim ready queue has invalid session or lease"
            ));
        }
        if expected_accepted && !state.idempotency_key_valid {
            violations.push(format!(
                "state[{index}]: accepted claim ready queue lacks idempotency key"
            ));
        }
    }
    violations
}

fn claim_lifecycle_expected_reject(state: &ClaimLifecycleRequestShapeState) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.claim_id_valid {
        "ClaimIdInvalid"
    } else if !state.fence_seq_positive {
        "FenceSeqInvalid"
    } else if state.operation == "RenewClaim" && !state.extend_duration_positive {
        "LeaseDurationInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn claim_lifecycle_fence_seq(state: &ClaimLifecycleRequestShapeState) -> i64 {
    match state.case.as_str() {
        "ZeroFence" => 0,
        "NegativeFence" => -1,
        _ => 1,
    }
}

fn claim_lifecycle_extend_by_ms(state: &ClaimLifecycleRequestShapeState) -> i64 {
    match state.case.as_str() {
        "ZeroRenewExtension" => 0,
        "NegativeRenewExtension" => -1,
        _ => 30_000,
    }
}

fn claim_lifecycle_actual_accepts(state: &ClaimLifecycleRequestShapeState) -> bool {
    let session_token = if state.session_token_valid {
        "session-1"
    } else {
        "session 1"
    };
    let claim_id = if state.claim_id_valid { "claim-1" } else { "" };
    let fence_seq = claim_lifecycle_fence_seq(state);
    let idempotency_key = if state.idempotency_key_valid {
        "idem-claim-lifecycle"
    } else {
        " "
    };

    match state.operation.as_str() {
        "StartSubtask" => {
            StartSubtaskReq::try_from_raw_parts(session_token, claim_id, fence_seq, idempotency_key)
                .is_ok()
        }
        "AbandonSubtask" => AbandonSubtaskReq::try_from_raw_parts(
            session_token,
            claim_id,
            fence_seq,
            idempotency_key,
        )
        .is_ok(),
        "ReleaseClaim" => {
            ReleaseClaimReq::try_from_raw_parts(session_token, claim_id, fence_seq, idempotency_key)
                .is_ok()
        }
        "RenewClaim" => RenewClaimReq::try_from_raw_parts(
            session_token,
            claim_id,
            fence_seq,
            claim_lifecycle_extend_by_ms(state),
            idempotency_key,
        )
        .is_ok(),
        _ => false,
    }
}

fn replay_claim_lifecycle_request_shape_trace(
    trace: &ClaimLifecycleRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: claim lifecycle scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = claim_lifecycle_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: claim lifecycle reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: claim lifecycle outcome disagrees with validation facts"
            ));
        }
        if claim_lifecycle_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: claim lifecycle parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid
                || !state.claim_id_valid
                || !state.fence_seq_positive
                || !state.idempotency_key_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted claim lifecycle request has invalid session, claim, fence, or idempotency key"
            ));
        }
        if expected_accepted && state.operation == "RenewClaim" && !state.extend_duration_positive {
            violations.push(format!(
                "state[{index}]: accepted renew claim request has non-positive extension"
            ));
        }
    }
    violations
}

fn mark_in_flight_expected_reject(state: &MarkInFlightRequestShapeState) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.queue_id_valid {
        "QueueIdInvalid"
    } else if !state.lease_duration_positive {
        "LeaseDurationInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn mark_in_flight_lease_duration(state: &MarkInFlightRequestShapeState) -> i64 {
    match state.case.as_str() {
        "ZeroLeaseDuration" => 0,
        "NegativeLeaseDuration" => -1,
        _ => 30_000,
    }
}

fn mark_in_flight_actual_accepts(state: &MarkInFlightRequestShapeState) -> bool {
    MarkInFlightReq::try_from_raw_parts(
        if state.session_token_valid {
            "session-1"
        } else {
            "session 1"
        },
        if state.queue_id_valid { "queue-1" } else { "" },
        mark_in_flight_lease_duration(state),
        if state.idempotency_key_valid {
            "idem-mark-in-flight"
        } else {
            " "
        },
    )
    .is_ok()
}

fn replay_mark_in_flight_request_shape_trace(
    trace: &MarkInFlightRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: mark in flight scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = mark_in_flight_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: mark in flight reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: mark in flight outcome disagrees with validation facts"
            ));
        }
        if mark_in_flight_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: mark in flight parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid
                || !state.queue_id_valid
                || !state.lease_duration_positive)
        {
            violations.push(format!(
                "state[{index}]: accepted mark in flight has invalid session, queue, or lease"
            ));
        }
        if expected_accepted && !state.idempotency_key_valid {
            violations.push(format!(
                "state[{index}]: accepted mark in flight lacks idempotency key"
            ));
        }
    }
    violations
}

fn mark_applied_expected_reject(state: &MarkAppliedRequestShapeState) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.queue_id_valid {
        "QueueIdInvalid"
    } else if !state.fence_seq_positive {
        "FenceSeqInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn mark_applied_fence_seq(state: &MarkAppliedRequestShapeState) -> i64 {
    match state.case.as_str() {
        "ZeroFence" => 0,
        "NegativeFence" => -1,
        _ => 1,
    }
}

fn mark_applied_actual_accepts(state: &MarkAppliedRequestShapeState) -> bool {
    MarkAppliedReq::try_from_raw_parts(
        if state.session_token_valid {
            "session-1"
        } else {
            ""
        },
        if state.queue_id_valid { "queue-1" } else { "" },
        mark_applied_fence_seq(state),
        if state.idempotency_key_valid {
            "idem-mark-applied"
        } else {
            " "
        },
    )
    .is_ok()
}

fn replay_mark_applied_request_shape_trace(trace: &MarkAppliedRequestShapeItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: mark applied scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = mark_applied_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: mark applied reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: mark applied outcome disagrees with validation facts"
            ));
        }
        if mark_applied_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: mark applied parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid || !state.queue_id_valid || !state.fence_seq_positive)
        {
            violations.push(format!(
                "state[{index}]: accepted mark applied has invalid session, queue, or fence"
            ));
        }
        if expected_accepted && !state.idempotency_key_valid {
            violations.push(format!(
                "state[{index}]: accepted mark applied lacks idempotency key"
            ));
        }
    }
    violations
}

fn apply_verification_expected_reject(state: &ApplyVerificationRequestShapeState) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.queue_id_valid {
        "QueueIdInvalid"
    } else if !state.artifact_digest_valid {
        "ArtifactDigestInvalid"
    } else if !state.review_id_valid {
        "ReviewIdInvalid"
    } else if !state.findings_digest_valid {
        "FindingsDigestInvalid"
    } else if !state.fence_seq_positive {
        "FenceSeqInvalid"
    } else if !state.verifier_valid {
        "VerifierInvalid"
    } else if !state.verdict_digest_valid {
        "VerdictDigestInvalid"
    } else if !state.seal_digest_valid {
        "SealDigestInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn apply_verification_actual_accepts(state: &ApplyVerificationRequestShapeState) -> bool {
    RecordApplyVerificationReq::try_from_raw_parts(
        if state.session_token_valid {
            "session-1"
        } else {
            ""
        },
        if state.queue_id_valid { "queue-1" } else { "" },
        if state.artifact_digest_valid {
            "blake3:artifact"
        } else {
            "artifact"
        },
        if state.review_id_valid {
            "review-1"
        } else {
            "review 1"
        },
        if state.findings_digest_valid {
            "blake3:findings"
        } else {
            "findings"
        },
        if state.fence_seq_positive { 1 } else { 0 },
        if state.verifier_valid {
            "mutai-rs"
        } else {
            "mutai rs"
        },
        if state.verdict_digest_valid {
            "blake3:verdict"
        } else {
            "verdict"
        },
        if state.seal_digest_valid {
            "blake3:seal"
        } else {
            "seal"
        },
        if state.idempotency_key_valid {
            "idem-verification"
        } else {
            " "
        },
    )
    .is_ok()
}

fn replay_apply_verification_request_shape_trace(
    trace: &ApplyVerificationRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: apply verification scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = apply_verification_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: apply verification reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: apply verification outcome disagrees with validation facts"
            ));
        }
        if apply_verification_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: apply verification parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid
                || !state.queue_id_valid
                || !state.review_id_valid
                || !state.fence_seq_positive)
        {
            violations.push(format!(
                "state[{index}]: accepted apply verification has invalid queue review or fence"
            ));
        }
        if expected_accepted && (!state.artifact_digest_valid || !state.findings_digest_valid) {
            violations.push(format!(
                "state[{index}]: accepted apply verification has invalid artifact or findings digest"
            ));
        }
        if expected_accepted
            && (!state.verifier_valid
                || !state.verdict_digest_valid
                || !state.seal_digest_valid
                || !state.idempotency_key_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted apply verification has invalid verifier evidence"
            ));
        }
    }
    violations
}

fn landing_authorization_expected_reject(
    state: &LandingAuthorizationRequestShapeState,
) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.queue_id_valid {
        "QueueIdInvalid"
    } else if !state.artifact_digest_valid {
        "ArtifactDigestInvalid"
    } else if !state.review_id_valid {
        "ReviewIdInvalid"
    } else if !state.findings_digest_valid {
        "FindingsDigestInvalid"
    } else if !state.fence_seq_positive {
        "FenceSeqInvalid"
    } else if !state.verifier_valid {
        "VerifierInvalid"
    } else if !state.verdict_digest_valid {
        "VerdictDigestInvalid"
    } else if !state.seal_digest_valid {
        "SealDigestInvalid"
    } else {
        "NoReject"
    }
}

fn landing_authorization_actual_accepts(state: &LandingAuthorizationRequestShapeState) -> bool {
    VerifyLandingAuthorizationReq::try_from_raw_parts(
        if state.session_token_valid {
            "session-1"
        } else {
            "session 1"
        },
        if state.queue_id_valid { "queue-1" } else { "" },
        if state.artifact_digest_valid {
            "blake3:artifact"
        } else {
            "artifact"
        },
        if state.review_id_valid {
            "review-1"
        } else {
            "review 1"
        },
        if state.findings_digest_valid {
            "blake3:findings"
        } else {
            "findings"
        },
        if state.fence_seq_positive { 1 } else { 0 },
        if state.verifier_valid { "mutai-rs" } else { "" },
        if state.verdict_digest_valid {
            "blake3:verdict"
        } else {
            "verdict"
        },
        if state.seal_digest_valid {
            "blake3:seal"
        } else {
            "seal"
        },
    )
    .is_ok()
}

fn replay_landing_authorization_request_shape_trace(
    trace: &LandingAuthorizationRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: landing authorization scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = landing_authorization_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: landing authorization reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: landing authorization outcome disagrees with validation facts"
            ));
        }
        if landing_authorization_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: landing authorization parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid
                || !state.queue_id_valid
                || !state.review_id_valid
                || !state.fence_seq_positive)
        {
            violations.push(format!(
                "state[{index}]: accepted landing authorization has invalid queue review or fence"
            ));
        }
        if expected_accepted && (!state.artifact_digest_valid || !state.findings_digest_valid) {
            violations.push(format!(
                "state[{index}]: accepted landing authorization has invalid artifact or findings digest"
            ));
        }
        if expected_accepted
            && (!state.verifier_valid || !state.verdict_digest_valid || !state.seal_digest_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted landing authorization has invalid verifier evidence"
            ));
        }
    }
    violations
}

fn landing_receipt_expected_reject(state: &LandingReceiptRequestShapeState) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.queue_id_valid {
        "QueueIdInvalid"
    } else if !state.artifact_digest_valid {
        "ArtifactDigestInvalid"
    } else if !state.fence_seq_positive {
        "FenceSeqInvalid"
    } else if !state.target_ref_valid {
        "TargetRefInvalid"
    } else if !state.landed_commit_oid_valid {
        "LandedCommitOidInvalid"
    } else {
        "NoReject"
    }
}

fn landing_receipt_actual_accepts(state: &LandingReceiptRequestShapeState) -> bool {
    RecordLandingReceiptReq::try_from_raw_parts(
        if state.session_token_valid {
            "session-1"
        } else {
            "session 1"
        },
        if state.queue_id_valid { "queue-1" } else { "" },
        if state.artifact_digest_valid {
            "blake3:artifact"
        } else {
            "artifact"
        },
        if state.fence_seq_positive { 1 } else { 0 },
        if state.target_ref_valid {
            "refs/heads/main"
        } else {
            "refs heads main"
        },
        if state.landed_commit_oid_valid {
            "0123456789abcdef"
        } else {
            "not-a-hex-oid"
        },
    )
    .is_ok()
}

fn replay_landing_receipt_request_shape_trace(
    trace: &LandingReceiptRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: landing receipt scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = landing_receipt_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: landing receipt request reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: landing receipt request outcome disagrees with validation facts"
            ));
        }
        if landing_receipt_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: landing receipt request parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid
                || !state.queue_id_valid
                || !state.artifact_digest_valid
                || !state.fence_seq_positive)
        {
            violations.push(format!(
                "state[{index}]: accepted landing receipt request has invalid queue artifact or fence"
            ));
        }
        if expected_accepted && (!state.target_ref_valid || !state.landed_commit_oid_valid) {
            violations.push(format!(
                "state[{index}]: accepted landing receipt request has invalid landing identity"
            ));
        }
    }
    violations
}

fn reservation_request_expected_reject(state: &ReservationRequestShapeState) -> &'static str {
    let is_request = state.operation == "ReservationRequest";
    if is_request && !state.session_token_valid {
        "SessionTokenInvalid"
    } else if is_request && !state.owner_subtask_id_valid {
        "OwnerSubtaskIdInvalid"
    } else if is_request && !state.lease_duration_positive {
        "LeaseDurationInvalid"
    } else if !state.scope_key_present {
        "ScopeKeyMissing"
    } else if !state.scope_key_normalized {
        "ScopeKeyNotNormalized"
    } else if state.scope != "GeneratedSet" && state.generated_members_present {
        "GeneratedMembersForbidden"
    } else if state.scope == "RepoGlobal" && !state.repo_global_key_canonical {
        "RepoGlobalKeyInvalid"
    } else if state.scope == "GeneratedSet" && !state.generated_members_present {
        "GeneratedMembersMissing"
    } else if state.scope == "GeneratedSet" && !state.generated_members_normalized {
        "GeneratedMemberInvalid"
    } else if state.scope == "GeneratedSet" && !state.generated_members_unique {
        "GeneratedMemberDuplicate"
    } else if is_request && !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn reservation_scope_class(state: &ReservationRequestShapeState) -> ScopeClass {
    match state.scope.as_str() {
        "ExactPath" => ScopeClass::ExactPath,
        "Subtree" => ScopeClass::Subtree,
        "RepoGlobal" => ScopeClass::RepoGlobal,
        "GeneratedSet" => ScopeClass::GeneratedSet,
        scope => panic!("unexpected reservation scope from ITF trace: {scope}"),
    }
}

fn reservation_scope_key(state: &ReservationRequestShapeState) -> &'static str {
    if !state.scope_key_present {
        return " ";
    }
    if !state.scope_key_normalized {
        return " src/lib.rs";
    }
    match state.scope.as_str() {
        "ExactPath" => "src/lib.rs",
        "Subtree" => "src",
        "RepoGlobal" if state.repo_global_key_canonical => "repo",
        "RepoGlobal" => "src",
        "GeneratedSet" => "artifact-manifest",
        scope => panic!("unexpected reservation scope from ITF trace: {scope}"),
    }
}

fn reservation_generated_members(state: &ReservationRequestShapeState) -> Vec<String> {
    if !state.generated_members_present {
        Vec::new()
    } else if !state.generated_members_normalized {
        vec![" src/generated.rs".to_owned()]
    } else if !state.generated_members_unique {
        vec!["src/generated.rs".to_owned(), "src/generated.rs".to_owned()]
    } else {
        vec!["src/generated.rs".to_owned()]
    }
}

fn reservation_request_actual_accepts(state: &ReservationRequestShapeState) -> bool {
    let scope_class = reservation_scope_class(state);
    let scope_key = reservation_scope_key(state);
    let generated_members = reservation_generated_members(state);
    if state.operation == "OverlapQuery" {
        return OverlapQueryReq::try_from_parts(scope_class, scope_key, generated_members).is_ok();
    }
    RequestReservationReq::try_from_raw_parts(
        if state.session_token_valid {
            "session-1"
        } else {
            ""
        },
        if state.owner_subtask_id_valid {
            "subtask-1"
        } else {
            "subtask 1"
        },
        scope_class,
        scope_key,
        generated_members,
        if state.lease_duration_positive {
            60_000
        } else {
            0
        },
        if state.idempotency_key_valid {
            "idem-reservation"
        } else {
            " "
        },
    )
    .is_ok()
}

fn replay_reservation_request_shape_trace(trace: &ReservationRequestShapeItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: reservation request scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = reservation_request_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: reservation request reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: reservation request outcome disagrees with validation facts"
            ));
        }
        let actual_accepted = reservation_request_actual_accepts(state);
        if actual_accepted != expected_accepted {
            violations.push(format!(
                "state[{index}]: reservation request parser disagrees with model"
            ));
        }
        if expected_accepted
            && state.operation == "ReservationRequest"
            && (!state.session_token_valid
                || !state.owner_subtask_id_valid
                || !state.lease_duration_positive
                || !state.idempotency_key_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted reservation request has invalid request field"
            ));
        }
        if expected_accepted && (!state.scope_key_present || !state.scope_key_normalized) {
            violations.push(format!(
                "state[{index}]: accepted reservation request has invalid scope key"
            ));
        }
        if expected_accepted && state.scope != "GeneratedSet" && state.generated_members_present {
            violations.push(format!(
                "state[{index}]: accepted non-generated reservation carries generated members"
            ));
        }
        if expected_accepted && state.scope == "RepoGlobal" && !state.repo_global_key_canonical {
            violations.push(format!(
                "state[{index}]: accepted repo-global reservation has non-canonical key"
            ));
        }
        if expected_accepted
            && state.scope == "GeneratedSet"
            && (!state.generated_members_present
                || !state.generated_members_allowed
                || !state.generated_members_normalized
                || !state.generated_members_unique)
        {
            violations.push(format!(
                "state[{index}]: accepted generated-set reservation has invalid members"
            ));
        }
    }
    violations
}

fn reservation_lifecycle_expected_reject(
    state: &ReservationLifecycleRequestShapeState,
) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.reservation_id_valid {
        "ReservationIdInvalid"
    } else if state.operation == "RenewReservation" && !state.extend_duration_positive {
        "LeaseDurationInvalid"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else {
        "NoReject"
    }
}

fn reservation_lifecycle_extend_by_ms(state: &ReservationLifecycleRequestShapeState) -> i64 {
    match state.case.as_str() {
        "ZeroRenewExtension" => 0,
        "NegativeRenewExtension" => -1,
        _ => 10_000,
    }
}

fn reservation_lifecycle_actual_accepts(state: &ReservationLifecycleRequestShapeState) -> bool {
    let session_token = if state.session_token_valid {
        "session-1"
    } else {
        "session 1"
    };
    let reservation_id = if state.reservation_id_valid {
        "reservation-1"
    } else {
        ""
    };
    let idempotency_key = if state.idempotency_key_valid {
        "idem-reservation-lifecycle"
    } else {
        " "
    };
    match state.operation.as_str() {
        "ReleaseReservation" => ReleaseReservationReq::try_from_raw_parts(
            session_token,
            reservation_id,
            idempotency_key,
        )
        .is_ok(),
        "RenewReservation" => RenewReservationReq::try_from_raw_parts(
            session_token,
            reservation_id,
            reservation_lifecycle_extend_by_ms(state),
            idempotency_key,
        )
        .is_ok(),
        _ => false,
    }
}

fn replay_reservation_lifecycle_request_shape_trace(
    trace: &ReservationLifecycleRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: reservation lifecycle request scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = reservation_lifecycle_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: reservation lifecycle request reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: reservation lifecycle request outcome disagrees with validation facts"
            ));
        }
        if reservation_lifecycle_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: reservation lifecycle request parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid
                || !state.reservation_id_valid
                || !state.idempotency_key_valid)
        {
            violations.push(format!(
                "state[{index}]: accepted reservation lifecycle request lacks session, reservation, or idempotency key"
            ));
        }
        if expected_accepted
            && state.operation == "RenewReservation"
            && !state.extend_duration_positive
        {
            violations.push(format!(
                "state[{index}]: accepted reservation renewal has non-positive extension"
            ));
        }
    }
    violations
}

fn reservation_record_expected_reject(state: &ReservationRecordShapeState) -> &'static str {
    if !state.scope_key_present {
        "ScopeKeyMissing"
    } else if !state.scope_key_normalized {
        "ScopeKeyNotNormalized"
    } else if state.scope != "GeneratedSet" && state.generated_members_present {
        "GeneratedMembersForbidden"
    } else if state.scope == "RepoGlobal" && !state.repo_global_key_canonical {
        "RepoGlobalKeyInvalid"
    } else if state.scope == "GeneratedSet" && !state.generated_members_present {
        "GeneratedMembersMissing"
    } else if state.scope == "GeneratedSet" && !state.generated_members_normalized {
        "GeneratedMemberInvalid"
    } else if state.scope == "GeneratedSet" && !state.generated_members_unique {
        "GeneratedMemberDuplicate"
    } else if state.timestamp_shape == "UpdatedBeforeCreatedShape" {
        "UpdatedBeforeCreatedReject"
    } else {
        "NoReject"
    }
}

fn reservation_record_scope_class(state: &ReservationRecordShapeState) -> ScopeClass {
    match state.scope.as_str() {
        "ExactPath" => ScopeClass::ExactPath,
        "Subtree" => ScopeClass::Subtree,
        "RepoGlobal" => ScopeClass::RepoGlobal,
        "GeneratedSet" => ScopeClass::GeneratedSet,
        scope => panic!("unexpected reservation record scope from ITF trace: {scope}"),
    }
}

fn reservation_record_state(state: &ReservationRecordShapeState) -> ReservationState {
    match state.lifecycle.as_str() {
        "Active" => ReservationState::Active,
        "Released" => ReservationState::Released,
        "Expired" => ReservationState::Expired,
        lifecycle => panic!("unexpected reservation record lifecycle from ITF trace: {lifecycle}"),
    }
}

fn reservation_record_scope_key(state: &ReservationRecordShapeState) -> &'static str {
    if !state.scope_key_present {
        return " ";
    }
    if !state.scope_key_normalized {
        return " src/lib.rs";
    }
    match state.scope.as_str() {
        "ExactPath" => "src/lib.rs",
        "Subtree" => "src",
        "RepoGlobal" if state.repo_global_key_canonical => "repo",
        "RepoGlobal" => "src",
        "GeneratedSet" => "artifact-manifest",
        scope => panic!("unexpected reservation record scope from ITF trace: {scope}"),
    }
}

fn reservation_record_generated_members(state: &ReservationRecordShapeState) -> Vec<String> {
    if !state.generated_members_present {
        Vec::new()
    } else if state.case == "GeneratedSetBlankMember" {
        vec!["".to_owned()]
    } else if !state.generated_members_normalized {
        vec![" src/generated.rs".to_owned()]
    } else if !state.generated_members_unique {
        vec!["src/generated.rs".to_owned(), "src/generated.rs".to_owned()]
    } else {
        vec!["src/generated.rs".to_owned()]
    }
}

fn reservation_record_timestamps(
    state: &ReservationRecordShapeState,
) -> (TimestampMs, TimestampMs) {
    match state.timestamp_shape.as_str() {
        "Monotonic" => (
            TimestampMs::parse(100).expect("valid created_at"),
            TimestampMs::parse(200).expect("valid updated_at"),
        ),
        "UpdatedBeforeCreatedShape" => (
            TimestampMs::parse(200).expect("valid created_at"),
            TimestampMs::parse(100).expect("valid updated_at"),
        ),
        other => panic!("unexpected reservation record timestamp shape: {other}"),
    }
}

fn reservation_record_actual(state: &ReservationRecordShapeState) -> Option<Reservation> {
    let (created_at, updated_at) = reservation_record_timestamps(state);
    Reservation::try_from_parts(
        covey::ReservationId::parse("reservation-1").expect("valid reservation id"),
        SubtaskId::parse("subtask-1").expect("valid subtask id"),
        reservation_record_scope_class(state),
        reservation_record_scope_key(state),
        reservation_record_generated_members(state),
        LeaseDeadlineMs::parse(1_000).expect("valid lease deadline"),
        reservation_record_state(state),
        created_at,
        updated_at,
    )
    .ok()
}

fn replay_reservation_record_shape_trace(trace: &ReservationRecordShapeItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: reservation record scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = reservation_record_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: reservation record reject reason disagrees with row facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: reservation record outcome disagrees with row facts"
            ));
        }
        let actual = reservation_record_actual(state);
        if actual.is_some() != expected_accepted {
            violations.push(format!(
                "state[{index}]: reservation record constructor disagrees with model"
            ));
        }
        if let Some(reservation) = actual {
            if reservation.state() != reservation_record_state(state) {
                violations.push(format!(
                    "state[{index}]: reservation record lifecycle projection disagrees with model"
                ));
            }
            if reservation.scope_class() != reservation_record_scope_class(state) {
                violations.push(format!(
                    "state[{index}]: reservation record scope projection disagrees with model"
                ));
            }
            if expected_accepted
                && reservation.scope_class() != ScopeClass::GeneratedSet
                && !reservation.generated_members().is_empty()
            {
                violations.push(format!(
                    "state[{index}]: accepted non-generated reservation retained generated members"
                ));
            }
            if expected_accepted
                && reservation.scope_class() == ScopeClass::GeneratedSet
                && reservation.generated_members().is_empty()
            {
                violations.push(format!(
                    "state[{index}]: accepted generated-set reservation lost generated members"
                ));
            }
            if expected_accepted
                && reservation.scope_class() == ScopeClass::GeneratedSet
                && !state.generated_members_allowed
            {
                violations.push(format!(
                    "state[{index}]: accepted generated-set reservation had disallowed members"
                ));
            }
        }
    }
    violations
}

fn conflict_resolution_expected_reject(state: &ConflictResolutionRequestState) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.conflict_id_valid {
        "ConflictIdInvalid"
    } else if state.requested_state == "Open" {
        "OpenResolutionForbidden"
    } else if !state.idempotency_key_valid {
        "IdempotencyKeyInvalid"
    } else if !conflict_resolution_transition_allowed(state) {
        "ResolutionDowngradeForbidden"
    } else {
        "NoReject"
    }
}

fn conflict_resolution_transition_allowed(state: &ConflictResolutionRequestState) -> bool {
    matches!(
        (state.current_state.as_str(), state.requested_state.as_str()),
        ("Open", "Acknowledged")
            | ("Open", "Resolved")
            | ("Acknowledged", "Acknowledged")
            | ("Acknowledged", "Resolved")
            | ("Resolved", "Resolved")
    )
}

fn conflict_resolution_requested_state(
    state: &ConflictResolutionRequestState,
) -> ConflictResolutionState {
    match state.requested_state.as_str() {
        "Open" => ConflictResolutionState::Open,
        "Acknowledged" => ConflictResolutionState::Acknowledged,
        "Resolved" => ConflictResolutionState::Resolved,
        requested => {
            panic!("unexpected conflict resolution request state from ITF trace: {requested}")
        }
    }
}

fn conflict_resolution_request_actual_accepts(state: &ConflictResolutionRequestState) -> bool {
    ResolveConflictReq::try_from_raw_parts(
        if state.session_token_valid {
            "session-1"
        } else {
            ""
        },
        if state.conflict_id_valid {
            "conflict-1"
        } else {
            "conflict 1"
        },
        conflict_resolution_requested_state(state),
        if state.idempotency_key_valid {
            "idem-resolve"
        } else {
            " "
        },
    )
    .is_ok()
}

fn replay_conflict_resolution_request_trace(
    trace: &ConflictResolutionRequestItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: conflict resolution scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = conflict_resolution_expected_reject(state);
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: conflict resolution reject reason does not match validation facts"
            ));
        }
        let expected_request_accepted = !matches!(
            expected_reject,
            "SessionTokenInvalid"
                | "ConflictIdInvalid"
                | "OpenResolutionForbidden"
                | "IdempotencyKeyInvalid"
        );
        if state.request_shape_accepted != expected_request_accepted {
            violations.push(format!(
                "state[{index}]: conflict resolution request-shape acceptance disagrees with validation facts"
            ));
        }
        let actual_request_accepted = conflict_resolution_request_actual_accepts(state);
        if actual_request_accepted != expected_request_accepted {
            violations.push(format!(
                "state[{index}]: conflict resolution request parser disagrees with model"
            ));
        }
        let expected_transition_allowed = conflict_resolution_transition_allowed(state);
        if state.transition_allowed != expected_transition_allowed {
            violations.push(format!(
                "state[{index}]: conflict resolution transition flag disagrees with transition matrix"
            ));
        }
        let expected_outcome = if !expected_request_accepted {
            "Rejected"
        } else if expected_transition_allowed {
            "Accepted"
        } else {
            "IllegalTransition"
        };
        if state.outcome != expected_outcome {
            violations.push(format!(
                "state[{index}]: conflict resolution outcome disagrees with request and transition facts"
            ));
        }
        if state.outcome == "Accepted"
            && (!state.request_shape_accepted || !state.transition_allowed)
        {
            violations.push(format!(
                "state[{index}]: conflict resolution accepted without valid request and transition"
            ));
        }
    }
    violations
}

fn replay_core_lifecycle_trace(trace: &CoreItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if state.claim == "Held"
            && !(state.session == "ActiveSession"
                && state.active_subtask
                && core_claim_live_subtask(state))
        {
            violations.push(format!(
                "state[{index}]: held claim is not bound to active session/subtask"
            ));
        }
        if state.active_subtask && state.claim != "Held" {
            violations.push(format!("state[{index}]: active subtask without held claim"));
        }
        if core_terminal_subtask(state) && state.claim == "Held" {
            violations.push(format!("state[{index}]: terminal subtask has held claim"));
        }
        if state.review != "NoReview" && !state.artifact_present {
            violations.push(format!("state[{index}]: review exists without artifact"));
        }
        if state.review == "Decided"
            && !matches!(
                state.subtask.as_str(),
                "ChangesRequested" | "Approved" | "ReadyForApply" | "Applied"
            )
        {
            violations.push(format!(
                "state[{index}]: decided review is not reflected in subtask state"
            ));
        }
        if state.queue != "NoQueue" && !state.review_approved {
            violations.push(format!(
                "state[{index}]: ready queue exists without approved review"
            ));
        }
        if core_queue_open(state) && state.subtask != "ReadyForApply" {
            violations.push(format!(
                "state[{index}]: open ready queue is not bound to ready_for_apply subtask"
            ));
        }
        if state.queue == "QueueApplied" && !(state.subtask == "Applied" && state.apply_verified) {
            violations.push(format!(
                "state[{index}]: applied queue lacks apply verification"
            ));
        }
        if state.subtask == "Applied" && state.queue != "QueueApplied" {
            violations.push(format!(
                "state[{index}]: applied subtask lacks applied queue"
            ));
        }
        if state.terminal != core_terminal_subtask(state) {
            violations.push(format!(
                "state[{index}]: terminal marker disagrees with subtask state"
            ));
        }
        if matches!(state.fence.as_str(), "F1" | "F2") && state.claim == "NoClaim" {
            violations.push(format!(
                "state[{index}]: issued fence exists before any claim lifecycle"
            ));
        }
    }
    violations
}

fn stale_recovery_occurred(state: &StaleClaimRecoveryState) -> bool {
    state.stale_reaped || state.lease_expired || state.exited_with_held_claim
}

fn replay_stale_claim_recovery_trace(trace: &StaleClaimRecoveryItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        let recovered = stale_recovery_occurred(state);
        let active_count =
            i32::from(state.old_active_subtask) + i32::from(state.new_active_subtask);
        if state.claim == "Held" {
            let old_holds = state.owner == "OldSession"
                && state.old_active_subtask
                && !state.new_active_subtask;
            let new_holds = state.owner == "NewSession"
                && state.new_active_subtask
                && !state.old_active_subtask;
            if !(old_holds || new_holds) {
                violations.push(format!(
                    "state[{index}]: held stale-recovery claim lacks exactly one active owner"
                ));
            }
        }
        if state.old_session != "ActiveSession" && state.old_active_subtask {
            violations.push(format!(
                "state[{index}]: inactive old session retained active subtask"
            ));
        }
        if recovered && state.owner != "NewSession" {
            if state.old_active_subtask || state.owner != "NoOwner" || state.subtask != "Available"
            {
                violations.push(format!(
                    "state[{index}]: recovered old claim did not detach session and subtask"
                ));
            }
        }
        if recovered && state.expired_fence <= 0 {
            violations.push(format!(
                "state[{index}]: recovered claim lacks expired fence"
            ));
        }
        if state.old_session != "ActiveSession" && state.owner == "OldSession" {
            violations.push(format!(
                "state[{index}]: stale or exited session still owns claim"
            ));
        }
        if recovered && !matches!(state.subtask.as_str(), "Available" | "Claimed") {
            violations.push(format!(
                "state[{index}]: recovered subtask is neither claimable nor reclaimed"
            ));
        }
        if state.owner == "NewSession"
            && state.claim == "Held"
            && state.current_fence <= state.expired_fence
        {
            violations.push(format!(
                "state[{index}]: resumed claim did not advance fence"
            ));
        }
        if active_count > 1 {
            violations.push(format!(
                "state[{index}]: stale recovery has dual active claims"
            ));
        }
        if state.stale_mutation_rejected
            && (state.owner == "OldSession" || state.old_active_subtask)
        {
            violations.push(format!(
                "state[{index}]: stale mutation reattached old claim"
            ));
        }
        if matches!(state.claim.as_str(), "Released" | "Expired")
            && state.owner == "NoOwner"
            && (state.old_active_subtask || state.new_active_subtask)
        {
            violations.push(format!(
                "state[{index}]: terminal recovered claim retained active subtask"
            ));
        }
        if state.new_session != "ActiveSession" && state.new_active_subtask {
            violations.push(format!(
                "state[{index}]: inactive new session retained active subtask"
            ));
        }
    }
    violations
}

fn replay_queue_reservation_trace(trace: &QueueReservationItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if state.queue_claim_live != (state.queue == "InFlight") {
            violations.push(format!(
                "state[{index}]: queue claim liveness disagrees with queue state"
            ));
        }
        if state.queue == "InFlight" && (!state.queue_lease_live || state.queue_fence == "F0") {
            violations.push(format!(
                "state[{index}]: in-flight queue lacks live lease or issued fence"
            ));
        }
        if queue_reservation_terminal_queue(state)
            && (state.queue_claim_live || state.queue_lease_live)
        {
            violations.push(format!("state[{index}]: terminal queue has live claim"));
        }
        if state.queue == "Applied" && !state.apply_verified {
            violations.push(format!(
                "state[{index}]: applied queue lacks apply verification"
            ));
        }
        if state.queue == "Queued" && (!state.subtask_ready || !state.artifact_matches) {
            violations.push(format!(
                "state[{index}]: queued item is not bound to a ready matching artifact"
            ));
        }
        if state.queue == "Queued"
            && (state.queue_claim_live || state.queue_lease_live || state.apply_verified)
        {
            violations.push(format!(
                "state[{index}]: queued item retained stale claim or verification"
            ));
        }
        if state.queue == "InFlight"
            && (!state.subtask_ready || !state.artifact_matches || !state.meta_schedulable)
        {
            violations.push(format!(
                "state[{index}]: in-flight queue lacks ready artifact or schedulable meta-task"
            ));
        }
        if state.reservation == "Active" && !state.reservation_lease_live {
            violations.push(format!(
                "state[{index}]: active reservation lacks live lease"
            ));
        }
        if matches!(state.reservation.as_str(), "Released" | "Expired")
            && state.reservation_lease_live
        {
            violations.push(format!(
                "state[{index}]: terminal reservation has live lease"
            ));
        }
        if matches!(state.conflict.as_str(), "Open" | "Acknowledged")
            && !(state.reservation == "Active" && state.overlap_detected)
        {
            violations.push(format!(
                "state[{index}]: unresolved conflict is not bound to active overlap"
            ));
        }
        if state.conflict != "NoConflict" && !(state.overlap_detected && state.conflict_bound) {
            violations.push(format!(
                "state[{index}]: conflict exists without recorded overlap binding"
            ));
        }
        if queue_reservation_conflict_rank(state) < state.conflict_rank_floor {
            violations.push(format!(
                "state[{index}]: conflict resolution moved below recorded floor"
            ));
        }
        if state.conflict_rank_floor >= 2 && state.conflict != "Resolved" {
            violations.push(format!(
                "state[{index}]: resolved conflict floor was downgraded"
            ));
        }
    }
    violations
}

fn replay_ready_queue_claim_selection_trace(
    trace: &ReadyQueueClaimSelectionItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        let prefix = format!("state[{index}]");
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "{prefix}: ready-queue claim-selection scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }

        if matches!(
            state.result,
            ReadyQueueClaimSelectionResult::ClaimHead | ReadyQueueClaimSelectionResult::ClaimTail
        ) && !state.role_apply_gate
        {
            violations.push(format!(
                "{prefix}: ready-queue claim was granted to non-apply-gate role"
            ));
        }
        if state.result == ReadyQueueClaimSelectionResult::ClaimHead
            && !(state.head_subtask_ready
                && state.head_artifact_matches
                && state.head_meta_schedulable)
        {
            violations.push(format!(
                "{prefix}: head queue claim did not require ready artifact and schedulable meta-task"
            ));
        }
        if state.case == ReadyQueueClaimSelectionCase::InvalidHeadSupersededTailClaimed
            && !(state.head_action == ReadyQueueClaimSelectionHeadAction::SupersededHead
                && state.result == ReadyQueueClaimSelectionResult::ClaimTail
                && !state.selected_head)
        {
            violations.push(format!(
                "{prefix}: invalid head was not superseded before claiming tail"
            ));
        }
        if state.case == ReadyQueueClaimSelectionCase::MetaUnavailableHeadCancelledTailClaimed
            && !(state.head_action == ReadyQueueClaimSelectionHeadAction::CancelledHead
                && state.result == ReadyQueueClaimSelectionResult::ClaimTail
                && !state.selected_head)
        {
            violations.push(format!(
                "{prefix}: unavailable head was not cancelled before claiming tail"
            ));
        }
        if state.case == ReadyQueueClaimSelectionCase::ExpiredHeadRequeuedThenClaimed
            && !(state.head_expired
                && state.head_action == ReadyQueueClaimSelectionHeadAction::RequeuedHead
                && state.result == ReadyQueueClaimSelectionResult::ClaimHead
                && state.selected_head)
        {
            violations.push(format!(
                "{prefix}: expired in-flight head was not requeued before claim"
            ));
        }
        if state.case == ReadyQueueClaimSelectionCase::ActiveInFlightHeadIgnoredTailClaimed
            && !(state.head_state == ReadyQueueClaimSelectionQueueState::InFlight
                && !state.head_expired
                && state.head_action == ReadyQueueClaimSelectionHeadAction::IgnoredHead
                && state.result == ReadyQueueClaimSelectionResult::ClaimTail
                && state.selected_tail)
        {
            violations.push(format!(
                "{prefix}: active in-flight head blocked or displaced tail claim"
            ));
        }
        if state.event_emitted
            != matches!(
                state.result,
                ReadyQueueClaimSelectionResult::ClaimHead
                    | ReadyQueueClaimSelectionResult::ClaimTail
            )
        {
            violations.push(format!(
                "{prefix}: ready-queue in-flight event emission disagrees with claim result"
            ));
        }
        let head_delta = state.head_fence_after - state.head_fence_before;
        let tail_delta = state.tail_fence_after - state.tail_fence_before;
        let expected_head_delta = i64::from(state.selected_head);
        let expected_tail_delta = i64::from(state.selected_tail);
        if head_delta != expected_head_delta || tail_delta != expected_tail_delta {
            violations.push(format!(
                "{prefix}: ready-queue fence advancement did not match selected queue"
            ));
        }
        if state.selected_head && state.selected_tail {
            violations.push(format!("{prefix}: ready-queue selected both head and tail"));
        }
        if state.result == ReadyQueueClaimSelectionResult::ClaimTail
            && !(state.tail_present && state.tail_claimable)
        {
            violations.push(format!(
                "{prefix}: tail claim was returned without a claimable tail"
            ));
        }
    }
    violations
}

fn ready_queue_metrics_expected_reject(
    state: &ReadyQueueMetricsState,
) -> ReadyQueueMetricsRejectReason {
    if state.queued_count == 0 && state.queued_age_present {
        ReadyQueueMetricsRejectReason::EmptyQueuedHasAge
    } else if state.queued_count > 0 && !state.queued_age_present {
        ReadyQueueMetricsRejectReason::NonEmptyQueuedMissingAge
    } else if state.queued_age_present && !state.queued_age_non_negative {
        ReadyQueueMetricsRejectReason::NegativeQueuedAge
    } else if state.in_flight_count == 0 && state.in_flight_age_present {
        ReadyQueueMetricsRejectReason::EmptyInFlightHasAge
    } else if state.in_flight_count > 0 && !state.in_flight_age_present {
        ReadyQueueMetricsRejectReason::NonEmptyInFlightMissingAge
    } else if state.in_flight_age_present && !state.in_flight_age_non_negative {
        ReadyQueueMetricsRejectReason::NegativeInFlightAge
    } else {
        ReadyQueueMetricsRejectReason::NoReject
    }
}

fn ready_queue_metrics_bucket_shape(
    count: i64,
    age_present: bool,
    age_non_negative: bool,
) -> ReadyQueueMetricsBucketShape {
    if count == 0 && !age_present {
        ReadyQueueMetricsBucketShape::EmptyBucket
    } else if count > 0 && age_present && age_non_negative {
        ReadyQueueMetricsBucketShape::NonEmptyBucket
    } else {
        ReadyQueueMetricsBucketShape::InvalidBucket
    }
}

fn replay_ready_queue_metrics_trace(trace: &ReadyQueueMetricsItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        let prefix = format!("state[{index}]");
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "{prefix}: ready-queue metrics scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }

        let expected_reject = ready_queue_metrics_expected_reject(state);
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "{prefix}: ready-queue metrics reject reason does not match bucket shape"
            ));
        }
        let queued_shape = ready_queue_metrics_bucket_shape(
            state.queued_count,
            state.queued_age_present,
            state.queued_age_non_negative,
        );
        let in_flight_shape = ready_queue_metrics_bucket_shape(
            state.in_flight_count,
            state.in_flight_age_present,
            state.in_flight_age_non_negative,
        );
        if state.queued_shape != queued_shape || state.in_flight_shape != in_flight_shape {
            violations.push(format!(
                "{prefix}: ready-queue metric bucket projection disagrees with count and age"
            ));
        }
        if (state.outcome == ReadyQueueMetricsOutcome::Accepted)
            != (queued_shape != ReadyQueueMetricsBucketShape::InvalidBucket
                && in_flight_shape != ReadyQueueMetricsBucketShape::InvalidBucket)
        {
            violations.push(format!(
                "{prefix}: ready-queue metrics acceptance disagrees with bucket validity"
            ));
        }
        if state.outcome == ReadyQueueMetricsOutcome::Accepted
            && ((state.queued_count == 0 && state.queued_age_present)
                || (state.in_flight_count == 0 && state.in_flight_age_present))
        {
            violations.push(format!(
                "{prefix}: empty ready-queue metric bucket retained oldest age"
            ));
        }
        if state.outcome == ReadyQueueMetricsOutcome::Accepted
            && ((state.queued_count > 0 && !state.queued_age_present)
                || (state.in_flight_count > 0 && !state.in_flight_age_present))
        {
            violations.push(format!(
                "{prefix}: non-empty ready-queue metric bucket lacked oldest age"
            ));
        }
        if state.outcome == ReadyQueueMetricsOutcome::Accepted
            && ((state.queued_age_present && !state.queued_age_non_negative)
                || (state.in_flight_age_present && !state.in_flight_age_non_negative))
        {
            violations.push(format!(
                "{prefix}: accepted ready-queue metrics had negative age"
            ));
        }
    }
    violations
}

fn replay_reservation_overlap_trace(trace: &ReservationOverlapItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: reservation overlap scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = reservation_overlap_expected_reject(state);
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: reservation overlap reject reason disagrees with candidate validity"
            ));
        }
        let expected_overlap = reservation_overlap_expected_overlap(state);
        if state.overlap_returned != expected_overlap {
            violations.push(format!(
                "state[{index}]: reservation overlap result disagrees with scope semantics"
            ));
        }
        if matches!(state.candidate_scope.as_str(), "RepoGlobal")
            || matches!(state.existing_scope.as_str(), "RepoGlobal")
        {
            if state.reject_reason == "NoReject" && state.existing_active && !state.overlap_returned
            {
                violations.push(format!(
                    "state[{index}]: repo-global scope did not overlap an active reservation"
                ));
            }
        }
        if !state.existing_active && state.overlap_returned {
            violations.push(format!(
                "state[{index}]: expired reservation was returned as an overlap"
            ));
        }
        if state.reject_reason != "NoReject" && (state.overlap_returned || state.conflict_recorded)
        {
            violations.push(format!(
                "state[{index}]: invalid overlap candidate returned overlap or conflict"
            ));
        }
        if state.conflict_recorded && !state.overlap_returned {
            violations.push(format!(
                "state[{index}]: conflict recorded without returned overlap"
            ));
        }
    }
    violations
}

fn reservation_overlap_expected_reject(state: &ReservationOverlapState) -> &'static str {
    if !state.candidate_path_valid {
        "InvalidPath"
    } else if state.candidate_scope == "GeneratedSet" && !state.candidate_members_present {
        "EmptyGeneratedMembers"
    } else {
        "NoReject"
    }
}

fn reservation_overlap_scopes_intersect(state: &ReservationOverlapState) -> bool {
    state.candidate_scope == "RepoGlobal"
        || state.existing_scope == "RepoGlobal"
        || (state.candidate_scope == "ExactPath"
            && ((state.existing_scope == "ExactPath" && state.relation == "SamePath")
                || (state.existing_scope == "Subtree"
                    && state.relation == "CandidateUnderExisting")
                || (state.existing_scope == "GeneratedSet"
                    && state.relation == "SharedGeneratedMember")))
        || (state.candidate_scope == "Subtree"
            && matches!(
                state.relation.as_str(),
                "SamePath" | "CandidateUnderExisting" | "ExistingUnderCandidate"
            ))
        || (state.candidate_scope == "GeneratedSet"
            && ((state.existing_scope == "ExactPath" && state.relation == "SharedGeneratedMember")
                || (state.existing_scope == "Subtree"
                    && state.relation == "CandidateUnderExisting")
                || (state.existing_scope == "GeneratedSet"
                    && state.relation == "SharedGeneratedMember")))
}

fn reservation_overlap_expected_overlap(state: &ReservationOverlapState) -> bool {
    reservation_overlap_expected_reject(state) == "NoReject"
        && state.existing_active
        && reservation_overlap_scopes_intersect(state)
}

fn repoops_snapshot_expected_claim_status(state: &RepoopsSnapshotState) -> &'static str {
    if state.subtask == "InProgress" {
        "ClaimInProgress"
    } else {
        "ClaimOpen"
    }
}

fn repoops_snapshot_claim_fact_ok(state: &RepoopsSnapshotState) -> bool {
    state.claim_fact_status == repoops_snapshot_expected_claim_status(state)
        && ((state.claim_fact_status == "ClaimInProgress"
            && state.active_ownership_token != "NoToken")
            || (state.claim_fact_status == "ClaimOpen"
                && state.active_ownership_token == "NoToken"))
}

fn repoops_snapshot_scope_ok(state: &RepoopsSnapshotState) -> bool {
    !state.owner_reservation_active
        || (state.owner_reservation_in_scope && state.scope_includes_owner_reservation)
}

fn repoops_snapshot_lock_ok(state: &RepoopsSnapshotState) -> bool {
    if !state.reservation_covers_requested_path {
        state.lock_kind == "NoLock"
    } else if state.owner_reservation_active {
        state.lock_kind == "OwnedLock"
            && state.lock_owner_matches_session
            && state.lock_claim_ref_matches_claim
    } else if state.foreign_reservation_active {
        state.lock_kind == "ForeignLock"
            && !state.lock_owner_matches_session
            && !state.lock_claim_ref_matches_claim
    } else {
        state.lock_kind == "NoLock"
    }
}

fn repoops_snapshot_ownership_token_ok(state: &RepoopsSnapshotState) -> bool {
    state.active_ownership_token != "RawToken"
        && state.caller_ownership_token != "RawToken"
        && state.fact_sources_use_token_refs
}

fn repoops_snapshot_expected_reject(state: &RepoopsSnapshotState) -> &'static str {
    if !state.current_claim_valid {
        "CurrentClaimInvalid"
    } else if !state.requested_path_valid {
        "PathInvalid"
    } else if !repoops_snapshot_claim_fact_ok(state) {
        "ClaimFactInvalid"
    } else if !repoops_snapshot_scope_ok(state) {
        "ScopeFactInvalid"
    } else if !repoops_snapshot_lock_ok(state) {
        "LockFactInvalid"
    } else if !repoops_snapshot_ownership_token_ok(state) {
        "OwnershipTokenInvalid"
    } else {
        "NoReject"
    }
}

fn replay_repoops_snapshot_trace(trace: &RepoopsSnapshotItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: repoops snapshot scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if state.outcome == "NotEvaluated" {
            continue;
        }
        let expected_reject = repoops_snapshot_expected_reject(state);
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: repoops snapshot reject reason does not match first failed gate"
            ));
        }
        if state.accepted && expected_reject != "NoReject" {
            violations.push(format!(
                "state[{index}]: repoops snapshot accepted with failed gate"
            ));
        }
        if state.accepted && state.outcome != "Accepted" {
            violations.push(format!(
                "state[{index}]: repoops accepted flag disagrees with outcome"
            ));
        }
        if state.accepted && !(state.current_claim_valid && state.requested_path_valid) {
            violations.push(format!(
                "state[{index}]: repoops snapshot accepted without current claim or valid path"
            ));
        }
        if state.accepted
            && state.claim_fact_status != repoops_snapshot_expected_claim_status(state)
        {
            violations.push(format!(
                "state[{index}]: repoops claim fact status disagrees with subtask state"
            ));
        }
        if state.accepted
            && state.claim_fact_status == "ClaimInProgress"
            && state.active_ownership_token != "TokenRef"
        {
            violations.push(format!(
                "state[{index}]: in-progress repoops claim lacks token reference"
            ));
        }
        if state.accepted
            && state.claim_fact_status == "ClaimOpen"
            && state.active_ownership_token != "NoToken"
        {
            violations.push(format!(
                "state[{index}]: open repoops claim carried active token"
            ));
        }
        if state.accepted && state.lock_kind == "OwnedLock" && !state.owner_reservation_active {
            violations.push(format!(
                "state[{index}]: owned lock lacks owner reservation"
            ));
        }
        if state.accepted && state.lock_kind == "ForeignLock" && !state.foreign_reservation_active {
            violations.push(format!(
                "state[{index}]: foreign lock lacks foreign reservation"
            ));
        }
        if state.accepted
            && state.owner_reservation_active
            && !(state.owner_reservation_in_scope && state.scope_includes_owner_reservation)
        {
            violations.push(format!(
                "state[{index}]: owner reservation missing from repoops scope"
            ));
        }
        if state.accepted && !repoops_snapshot_ownership_token_ok(state) {
            violations.push(format!(
                "state[{index}]: repoops snapshot exposed raw session token"
            ));
        }
    }
    violations
}

fn repoops_snapshot_request_expected_reject(
    state: &RepoopsSnapshotRequestShapeState,
) -> &'static str {
    if !state.session_token_valid {
        "SessionTokenInvalid"
    } else if !state.claim_id_valid {
        "ClaimIdInvalid"
    } else if !state.fence_seq_positive {
        "FenceSeqInvalid"
    } else if !state.path_set_non_empty {
        "EmptyPathSetInvalid"
    } else if !state.every_path_valid {
        "PathInvalid"
    } else if state.duplicate_after_normalization {
        "DuplicatePathInvalid"
    } else {
        "NoReject"
    }
}

fn repoops_snapshot_request_paths(state: &RepoopsSnapshotRequestShapeState) -> Vec<String> {
    match state.case.as_str() {
        "ValidSinglePath" => vec!["src/lib.rs".to_owned()],
        "ValidNormalizedPath" => vec!["./src/lib.rs".to_owned(), "docs\\guide.md".to_owned()],
        "ValidTwoPaths" => vec!["src/lib.rs".to_owned(), "docs/guide.md".to_owned()],
        "EmptyPathSet" => Vec::new(),
        "EmptyPath" => vec!["".to_owned()],
        "EscapingPath" => vec!["../outside".to_owned()],
        "ControlCharacterPath" => vec!["src/\u{0007}bad.rs".to_owned()],
        "DuplicateAfterNormalization" => vec!["./src/lib.rs".to_owned(), "src/lib.rs".to_owned()],
        _ => vec!["src/lib.rs".to_owned()],
    }
}

fn repoops_snapshot_request_actual_shape(state: &RepoopsSnapshotRequestShapeState) -> (bool, bool) {
    let Ok(req) = RepoopsAuthoritySnapshotReq::try_from_raw_parts(
        if state.session_token_valid {
            "session-1"
        } else {
            ""
        },
        if state.claim_id_valid {
            "claim-1"
        } else {
            "claim 1"
        },
        if state.fence_seq_positive { 1 } else { 0 },
        repoops_snapshot_request_paths(state),
    ) else {
        return (false, false);
    };
    let serialized = serde_json::to_value(&req)
        .expect("valid repoops authority snapshot request should serialize");
    let paths = serialized
        .get("paths")
        .and_then(serde_json::Value::as_array)
        .expect("serialized snapshot request should carry path array");
    let normalized = paths.iter().all(|path| {
        path.as_str()
            .is_some_and(|path| !path.starts_with("./") && !path.contains('\\') && !path.is_empty())
    });
    (true, normalized)
}

fn replay_repoops_snapshot_request_shape_trace(
    trace: &RepoopsSnapshotRequestShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: repoops snapshot request scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = repoops_snapshot_request_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: repoops snapshot request reject reason does not match validation facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: repoops snapshot request outcome disagrees with validation facts"
            ));
        }
        let (actual_accepted, actual_normalized) = repoops_snapshot_request_actual_shape(state);
        if actual_accepted != expected_accepted {
            violations.push(format!(
                "state[{index}]: repoops snapshot request parser disagrees with model"
            ));
        }
        if expected_accepted
            && (!state.session_token_valid || !state.claim_id_valid || !state.fence_seq_positive)
        {
            violations.push(format!(
                "state[{index}]: accepted repoops snapshot request has invalid identity or fence"
            ));
        }
        if expected_accepted
            && (!state.path_set_non_empty
                || !state.every_path_valid
                || state.duplicate_after_normalization)
        {
            violations.push(format!(
                "state[{index}]: accepted repoops snapshot request has invalid path set"
            ));
        }
        if expected_accepted && state.serialized_paths_normalized && !actual_normalized {
            violations.push(format!(
                "state[{index}]: repoops snapshot request serialization kept unnormalized paths"
            ));
        }
        if state.normalizes_path_syntax && expected_accepted && !actual_normalized {
            violations.push(format!(
                "state[{index}]: repoops snapshot request did not normalize accepted paths"
            ));
        }
    }
    violations
}

fn transition_matrix_expected_allowed(state: &TransitionMatrixState) -> bool {
    match state.object.as_str() {
        "WorkSubtask" => matches!(
            (state.from_state.as_str(), state.to_state.as_str()),
            ("Available", "Claimed")
                | ("Claimed", "InProgress")
                | ("InProgress", "ArtifactPublished")
                | ("ArtifactPublished", "ArtifactPublished")
                | ("ReviewPending", "ArtifactPublished")
                | ("ArtifactPublished", "ReviewPending")
                | ("ReviewPending", "ChangesRequested")
                | ("ReviewPending", "Blocked")
                | ("ReviewPending", "Approved")
                | ("Approved", "ReadyForApply")
                | ("ReadyForApply", "Applied")
        ),
        "ReviewSubtask" => matches!(
            (state.from_state.as_str(), state.to_state.as_str()),
            ("Available", "Claimed") | ("Claimed", "InProgress") | ("InProgress", "Decided")
        ),
        "ReviewObject" => matches!(
            (state.from_state.as_str(), state.to_state.as_str()),
            ("ReviewRequested", "ReviewInProgress")
                | ("ReviewRequested", "ReviewSuperseded")
                | ("ReviewInProgress", "ReviewDecided")
                | ("ReviewInProgress", "ReviewSuperseded")
        ),
        "ReadyQueue" => matches!(
            (state.from_state.as_str(), state.to_state.as_str()),
            ("QueueQueued", "QueueInFlight")
                | ("QueueInFlight", "QueueApplied")
                | ("QueueQueued", "QueueSuperseded")
                | ("QueueInFlight", "QueueSuperseded")
                | ("QueueQueued", "QueueCancelled")
                | ("QueueInFlight", "QueueCancelled")
        ),
        "Conflict" => matches!(
            (state.from_state.as_str(), state.to_state.as_str()),
            ("ConflictOpen", "ConflictAcknowledged")
                | ("ConflictOpen", "ConflictResolved")
                | ("ConflictAcknowledged", "ConflictAcknowledged")
                | ("ConflictAcknowledged", "ConflictResolved")
                | ("ConflictResolved", "ConflictResolved")
        ),
        "Reservation" => matches!(
            (state.from_state.as_str(), state.to_state.as_str()),
            ("ReservationActive", "ReservationReleased")
                | ("ReservationActive", "ReservationExpired")
        ),
        _ => false,
    }
}

fn replay_transition_matrix_trace(trace: &TransitionMatrixItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: transition matrix scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_allowed = transition_matrix_expected_allowed(state);
        if state.allowed_by_matrix != expected_allowed {
            violations.push(format!(
                "state[{index}]: transition matrix allowed flag disagrees with expected edge set"
            ));
        }
        let expected_outcome = if expected_allowed {
            "Allowed"
        } else {
            "Rejected"
        };
        if state.outcome != expected_outcome {
            violations.push(format!(
                "state[{index}]: transition matrix outcome disagrees with expected edge set"
            ));
        }
        if state.allowed_by_matrix
            && state.object == "WorkSubtask"
            && state.to_state == "Applied"
            && state.from_state != "ReadyForApply"
        {
            violations.push(format!(
                "state[{index}]: work subtask bypassed ready_for_apply before applied"
            ));
        }
        if state.allowed_by_matrix
            && state.object == "WorkSubtask"
            && matches!(state.from_state.as_str(), "ChangesRequested" | "Blocked")
        {
            violations.push(format!(
                "state[{index}]: failed work terminal state transitioned in place"
            ));
        }
        if state.allowed_by_matrix
            && state.object == "ReviewSubtask"
            && state.to_state == "InProgress"
            && state.from_state != "Claimed"
        {
            violations.push(format!(
                "state[{index}]: review subtask bypassed claimed before in_progress"
            ));
        }
        if state.allowed_by_matrix
            && state.object == "ReviewObject"
            && state.from_state == "ReviewDecided"
        {
            violations.push(format!("state[{index}]: decided review transitioned"));
        }
        if state.allowed_by_matrix
            && state.object == "ReadyQueue"
            && matches!(state.from_state.as_str(), "QueueApplied" | "QueueCancelled")
        {
            violations.push(format!("state[{index}]: terminal queue transitioned"));
        }
        if state.allowed_by_matrix
            && state.object == "Conflict"
            && state.from_state == "ConflictResolved"
            && state.to_state != "ConflictResolved"
        {
            violations.push(format!("state[{index}]: resolved conflict downgraded"));
        }
        if state.allowed_by_matrix
            && state.object == "Reservation"
            && matches!(
                state.from_state.as_str(),
                "ReservationReleased" | "ReservationExpired"
            )
        {
            violations.push(format!("state[{index}]: terminal reservation transitioned"));
        }
    }
    violations
}

fn subtask_domain_lifecycle_expected_reject(
    state: &SubtaskDomainLifecycleShapeState,
) -> &'static str {
    if state.kind == "Work" && state.lifecycle == "Decided" {
        "WorkCannotUseDecided"
    } else if state.kind == "Review" && state.lifecycle == "Blocked" {
        "ReviewCannotUseBlocked"
    } else if state.kind == "Review"
        && matches!(
            state.lifecycle.as_str(),
            "ArtifactPublished"
                | "ReviewPending"
                | "ChangesRequested"
                | "Approved"
                | "ReadyForApply"
                | "Applied"
        )
    {
        "ReviewCannotUseWorkArtifactState"
    } else {
        "NoReject"
    }
}

fn subtask_domain_lifecycle_value(state: &SubtaskDomainLifecycleShapeState) -> SubtaskLifecycle {
    let claim_id = || ClaimId::parse("claim-1").expect("valid claim id");
    let artifact_digest =
        || ArtifactDigest::parse("blake3:aaaaaaaa").expect("valid artifact digest");
    match state.lifecycle.as_str() {
        "Blocked" => SubtaskLifecycle::Blocked {
            artifact_digest: artifact_digest(),
        },
        "Claimed" => SubtaskLifecycle::Claimed {
            active_claim_id: claim_id(),
        },
        "InProgress" => SubtaskLifecycle::InProgress {
            active_claim_id: claim_id(),
        },
        "ArtifactPublished" => SubtaskLifecycle::ArtifactPublished {
            active_claim_id: Some(claim_id()),
            artifact_digest: artifact_digest(),
        },
        "ReviewPending" => SubtaskLifecycle::ReviewPending {
            active_claim_id: Some(claim_id()),
            artifact_digest: artifact_digest(),
        },
        "ChangesRequested" => SubtaskLifecycle::ChangesRequested {
            active_claim_id: None,
            artifact_digest: artifact_digest(),
        },
        "Approved" => SubtaskLifecycle::Approved {
            active_claim_id: None,
            artifact_digest: artifact_digest(),
        },
        "Decided" => SubtaskLifecycle::Decided,
        "ReadyForApply" => SubtaskLifecycle::ReadyForApply {
            active_claim_id: None,
            artifact_digest: artifact_digest(),
        },
        "Applied" => SubtaskLifecycle::Applied {
            active_claim_id: None,
            artifact_digest: artifact_digest(),
        },
        "Abandoned" => SubtaskLifecycle::Abandoned {
            artifact_digest: None,
        },
        _ => SubtaskLifecycle::Available,
    }
}

fn subtask_domain_lifecycle_actual_accepts(state: &SubtaskDomainLifecycleShapeState) -> bool {
    let subtask_id = SubtaskId::parse("subtask-1").expect("valid subtask id");
    let meta_task_id = MetaTaskId::parse("meta-1").expect("valid meta-task id");
    let priority = SubtaskPriority::parse(10).expect("valid subtask priority");
    let created_at = TimestampMs::parse(100).expect("valid created timestamp");
    let updated_at = TimestampMs::parse(101).expect("valid updated timestamp");
    let lifecycle = subtask_domain_lifecycle_value(state);
    match state.kind.as_str() {
        "Work" => WorkSubtask::new(
            subtask_id,
            meta_task_id,
            "ship it".to_owned(),
            lifecycle,
            priority,
            created_at,
            updated_at,
        )
        .is_ok(),
        "Review" => {
            let review_target = ReviewTarget::new(
                SubtaskId::parse("work-subtask-1").expect("valid target subtask id"),
                ArtifactDigest::parse("blake3:bbbbbbbb").expect("valid target artifact digest"),
            );
            ReviewSubtask::new(
                subtask_id,
                meta_task_id,
                "review it".to_owned(),
                review_target,
                lifecycle,
                priority,
                created_at,
                updated_at,
            )
            .is_ok()
        }
        _ => false,
    }
}

fn replay_subtask_domain_lifecycle_shape_trace(
    trace: &SubtaskDomainLifecycleShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: subtask domain lifecycle scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = subtask_domain_lifecycle_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: subtask domain lifecycle reject reason disagrees with kind/lifecycle facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: subtask domain lifecycle outcome disagrees with kind/lifecycle facts"
            ));
        }
        if subtask_domain_lifecycle_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: subtask domain lifecycle constructor disagrees with model"
            ));
        }
        if state.accepted && state.kind == "Work" && state.lifecycle == "Decided" {
            violations.push(format!(
                "state[{index}]: work subtask accepted review-only decided lifecycle"
            ));
        }
        if state.accepted
            && state.kind == "Review"
            && matches!(
                state.lifecycle.as_str(),
                "Blocked"
                    | "ArtifactPublished"
                    | "ReviewPending"
                    | "ChangesRequested"
                    | "Approved"
                    | "ReadyForApply"
                    | "Applied"
            )
        {
            violations.push(format!(
                "state[{index}]: review subtask accepted work-only lifecycle"
            ));
        }
        if state.accepted
            && state.carries_artifact
                != matches!(
                    state.lifecycle.as_str(),
                    "Blocked"
                        | "ArtifactPublished"
                        | "ReviewPending"
                        | "ChangesRequested"
                        | "Approved"
                        | "ReadyForApply"
                        | "Applied"
                )
        {
            violations.push(format!(
                "state[{index}]: artifact carrier flag disagrees with lifecycle"
            ));
        }
        if state.accepted
            && state.carries_active_claim
                != matches!(
                    state.lifecycle.as_str(),
                    "Claimed" | "InProgress" | "ArtifactPublished" | "ReviewPending"
                )
        {
            violations.push(format!(
                "state[{index}]: active-claim carrier flag disagrees with lifecycle"
            ));
        }
    }
    violations
}

fn apply_proof_review_state(state: &ApplyProofRowLifecycleState) -> ReviewState {
    match state.review_state.as_str() {
        "Requested" => ReviewState::Requested,
        "InProgress" => ReviewState::InProgress,
        "Decided" => ReviewState::Decided,
        "Superseded" => ReviewState::Superseded,
        other => panic!("unexpected review proof state {other}"),
    }
}

fn apply_proof_ready_queue_state(state: &ApplyProofRowLifecycleState) -> ReadyQueueState {
    match state.ready_queue_state.as_str() {
        "Queued" => ReadyQueueState::Queued,
        "QueueInFlight" => ReadyQueueState::InFlight,
        "Applied" => ReadyQueueState::Applied,
        "QueueSuperseded" => ReadyQueueState::Superseded,
        "Cancelled" => ReadyQueueState::Cancelled,
        other => panic!("unexpected ready queue proof state {other}"),
    }
}

fn apply_proof_verdict(state: &ApplyProofRowLifecycleState) -> Option<ReviewVerdict> {
    if !state.verdict_present {
        return None;
    }
    if state.case == "ReviewDecidedBlockedFindings" {
        Some(ReviewVerdict::Blocked)
    } else {
        Some(ReviewVerdict::Approve)
    }
}

fn apply_proof_claim_fence(state: &ApplyProofRowLifecycleState) -> Option<i64> {
    match state.claim_fence.as_str() {
        "NoFence" => None,
        "ZeroFence" => Some(0),
        "PositiveFence" => Some(7),
        other => panic!("unexpected claim fence shape {other}"),
    }
}

fn apply_proof_terminal_queue_state(state: &ApplyProofRowLifecycleState) -> bool {
    matches!(
        state.ready_queue_state.as_str(),
        "Queued" | "QueueSuperseded" | "Cancelled"
    )
}

fn apply_proof_row_lifecycle_expected_reject(state: &ApplyProofRowLifecycleState) -> &'static str {
    if state.row_kind == "ReviewProofRow" {
        if state.review_state == "Decided" && !state.verdict_present {
            "DecidedMissingVerdict"
        } else if state.review_state == "Decided" && !state.findings_digest_present {
            "DecidedMissingFindingsDigest"
        } else if state.review_state != "Decided"
            && (state.verdict_present || state.findings_digest_present)
        {
            "NonDecidedReviewCarriesDecisionEvidence"
        } else {
            "NoReject"
        }
    } else if apply_proof_terminal_queue_state(state)
        && (state.claimed_by_session_present || state.claim_lease_deadline_present)
    {
        "TerminalQueueCarriesActiveClaimFields"
    } else if apply_proof_terminal_queue_state(state) && state.claim_fence == "ZeroFence" {
        "InvalidClaimFence"
    } else if state.ready_queue_state == "QueueInFlight" && !state.claimed_by_session_present {
        "InFlightMissingClaimedSession"
    } else if state.ready_queue_state == "QueueInFlight" && state.claim_fence == "NoFence" {
        "InFlightMissingClaimFence"
    } else if state.ready_queue_state == "QueueInFlight" && state.claim_fence == "ZeroFence" {
        "InvalidClaimFence"
    } else if state.ready_queue_state == "QueueInFlight" && !state.claim_lease_deadline_present {
        "InFlightMissingLeaseDeadline"
    } else if state.ready_queue_state == "Applied"
        && (state.claimed_by_session_present || state.claim_lease_deadline_present)
    {
        "AppliedCarriesActiveClaimFields"
    } else if state.ready_queue_state == "Applied" && state.claim_fence == "NoFence" {
        "AppliedMissingClaimFence"
    } else if state.ready_queue_state == "Applied" && state.claim_fence == "ZeroFence" {
        "InvalidClaimFence"
    } else {
        "NoReject"
    }
}

fn apply_proof_row_lifecycle_actual_accepts(state: &ApplyProofRowLifecycleState) -> bool {
    if state.row_kind == "ReviewProofRow" {
        return review_proof_row_lifecycle_accepts_for_model(
            apply_proof_review_state(state),
            apply_proof_verdict(state),
            state.findings_digest_present,
        );
    }
    ready_queue_proof_row_lifecycle_accepts_for_model(
        apply_proof_ready_queue_state(state),
        state.claimed_by_session_present,
        apply_proof_claim_fence(state),
        state.claim_lease_deadline_present,
    )
}

fn replay_apply_proof_row_lifecycle_trace(trace: &ApplyProofRowLifecycleItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: apply-proof row lifecycle scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = apply_proof_row_lifecycle_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: apply-proof row lifecycle reject reason disagrees with row facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: apply-proof row lifecycle outcome disagrees with row facts"
            ));
        }
        if apply_proof_row_lifecycle_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: apply-proof row lifecycle constructor disagrees with model"
            ));
        }
        if state.accepted
            && state.row_kind == "ReviewProofRow"
            && state.review_state == "Decided"
            && !(state.verdict_present && state.findings_digest_present)
        {
            violations.push(format!(
                "state[{index}]: decided review row accepted without complete decision evidence"
            ));
        }
        if state.accepted
            && state.row_kind == "ReviewProofRow"
            && state.review_state != "Decided"
            && (state.verdict_present || state.findings_digest_present)
        {
            violations.push(format!(
                "state[{index}]: non-decided review row accepted decision evidence"
            ));
        }
        if state.accepted
            && state.row_kind == "ReadyQueueProofRow"
            && state.ready_queue_state == "QueueInFlight"
            && !(state.claimed_by_session_present
                && state.claim_fence == "PositiveFence"
                && state.claim_lease_deadline_present)
        {
            violations.push(format!(
                "state[{index}]: in-flight queue row accepted without complete active claim evidence"
            ));
        }
        if state.accepted
            && state.row_kind == "ReadyQueueProofRow"
            && state.ready_queue_state == "Applied"
            && (state.claimed_by_session_present
                || state.claim_fence != "PositiveFence"
                || state.claim_lease_deadline_present)
        {
            violations.push(format!(
                "state[{index}]: applied queue row accepted without fence-only evidence"
            ));
        }
    }
    violations
}

fn record_lifecycle_session_state(state: &RecordLifecycleShapeState) -> SessionState {
    match state.session_lifecycle.as_str() {
        "Active" => SessionState::Active,
        "Stale" => SessionState::Stale,
        "Exited" => SessionState::Exited,
        other => panic!("unexpected session lifecycle {other}"),
    }
}

fn record_lifecycle_meta_task_state(state: &RecordLifecycleShapeState) -> MetaTaskState {
    match state.meta_task_lifecycle.as_str() {
        "Planning" => MetaTaskState::Planning,
        "MetaActiveState" => MetaTaskState::Active,
        "Completed" => MetaTaskState::Completed,
        "Cancelled" => MetaTaskState::Cancelled,
        other => panic!("unexpected meta-task lifecycle {other}"),
    }
}

fn record_lifecycle_claim_state(state: &RecordLifecycleShapeState) -> ClaimState {
    match state.claim_lifecycle.as_str() {
        "Held" => ClaimState::Held,
        "Released" => ClaimState::Released,
        "Expired" => ClaimState::Expired,
        "Revoked" => ClaimState::Revoked,
        other => panic!("unexpected claim lifecycle {other}"),
    }
}

fn record_lifecycle_timestamps(
    state: &RecordLifecycleShapeState,
) -> (TimestampMs, TimestampMs, TimestampMs) {
    let created_at = TimestampMs::parse(10).expect("valid created_at");
    let last_heartbeat_at =
        TimestampMs::parse(if state.timestamp_shape == "HeartbeatBeforeCreated" {
            5
        } else {
            20
        })
        .expect("valid heartbeat timestamp");
    let updated_at = TimestampMs::parse(if state.timestamp_shape == "UpdatedBeforeCreated" {
        5
    } else {
        20
    })
    .expect("valid updated_at");
    (created_at, last_heartbeat_at, updated_at)
}

fn record_lifecycle_expected_reject(state: &RecordLifecycleShapeState) -> &'static str {
    if state.row_kind == "SessionRow"
        && state.session_lifecycle != "Active"
        && state.active_subtask_present
    {
        "TerminalSessionCarriesActiveSubtask"
    } else if state.row_kind == "SessionRow" && state.timestamp_shape == "HeartbeatBeforeCreated" {
        "SessionHeartbeatBeforeCreatedReject"
    } else if state.timestamp_shape == "UpdatedBeforeCreated" {
        "UpdatedBeforeCreatedReject"
    } else if state.row_kind == "SessionRow" && state.tick_shape == "NegativeTick" {
        "NegativeHeartbeatTickReject"
    } else if state.row_kind == "SessionRow" && state.text_shape == "InvalidPrincipal" {
        "InvalidPrincipalReject"
    } else if state.row_kind == "SessionRow" && state.text_shape == "InvalidInstance" {
        "InvalidInstanceReject"
    } else if state.row_kind == "MetaTaskRow" && state.text_shape == "BlankPrompt" {
        "BlankPromptReject"
    } else {
        "NoReject"
    }
}

fn record_lifecycle_actual_accepts(state: &RecordLifecycleShapeState) -> bool {
    let (created_at, last_heartbeat_at, updated_at) = record_lifecycle_timestamps(state);
    match state.row_kind.as_str() {
        "SessionRow" => Session::try_from_parts(
            SessionToken::parse("session-1").expect("valid session token"),
            if state.text_shape == "InvalidPrincipal" {
                "bad principal"
            } else {
                "principal-1"
            },
            if state.text_shape == "InvalidInstance" {
                "bad instance"
            } else {
                "instance-1"
            },
            SessionRole::Executor,
            record_lifecycle_session_state(state),
            state
                .active_subtask_present
                .then(|| SubtaskId::parse("subtask-1").expect("valid subtask id")),
            last_heartbeat_at,
            if state.tick_shape == "NegativeTick" {
                -1
            } else {
                1
            },
            created_at,
            updated_at,
        )
        .is_ok(),
        "MetaTaskRow" => MetaTask::try_from_parts(
            MetaTaskId::parse("meta-1").expect("valid meta task id"),
            if state.text_shape == "BlankPrompt" {
                " ".to_owned()
            } else {
                "do work".to_owned()
            },
            record_lifecycle_meta_task_state(state),
            SessionToken::parse("session-1").expect("valid session token"),
            created_at,
            updated_at,
        )
        .is_ok(),
        "ClaimRow" => Claim::try_from_parts(
            ClaimId::parse("claim-1").expect("valid claim id"),
            SubtaskId::parse("subtask-1").expect("valid subtask id"),
            SessionToken::parse("session-1").expect("valid session token"),
            FenceSeq::parse(1).expect("valid fence"),
            LeaseDeadlineMs::parse(20).expect("valid lease deadline"),
            record_lifecycle_claim_state(state),
            created_at,
            updated_at,
        )
        .is_ok(),
        other => panic!("unexpected record lifecycle row kind {other}"),
    }
}

fn replay_record_lifecycle_shape_trace(trace: &RecordLifecycleShapeItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: record lifecycle scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = record_lifecycle_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: record lifecycle reject reason disagrees with row facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: record lifecycle outcome disagrees with row facts"
            ));
        }
        if record_lifecycle_actual_accepts(state) != expected_accepted {
            violations.push(format!(
                "state[{index}]: record lifecycle constructor disagrees with model"
            ));
        }
        if state.accepted
            && state.row_kind == "SessionRow"
            && state.session_lifecycle != "Active"
            && state.active_subtask_present
        {
            violations.push(format!(
                "state[{index}]: terminal session accepted with active subtask"
            ));
        }
        if state.accepted
            && state.row_kind == "SessionRow"
            && (state.timestamp_shape != "Monotonic" || state.tick_shape != "NonNegativeTick")
        {
            violations.push(format!(
                "state[{index}]: session accepted non-monotonic timestamps or negative heartbeat tick"
            ));
        }
        if state.accepted && state.row_kind != "SessionRow" && state.timestamp_shape != "Monotonic"
        {
            violations.push(format!(
                "state[{index}]: meta-task or claim accepted non-monotonic timestamps"
            ));
        }
    }
    violations
}

fn runtime_attestation_record_expected_reject(
    state: &RuntimeAttestationRecordShapeState,
) -> &'static str {
    match state.provider_run_shape.as_str() {
        "InvalidProviderRunId" => "InvalidProviderRunIdReject",
        "InvalidProviderRunIssuer" => "InvalidProviderRunIssuerReject",
        "PartialLegacyId" | "PartialLegacyIssuer" => "PartialProviderRunIdentityReject",
        _ => match state.runtime_identity_shape.as_str() {
            "InvalidProcessId" => "InvalidProcessIdReject",
            "InvalidContainerId" => "InvalidContainerIdReject",
            "MissingRuntimeIdentityShape" => "MissingRuntimeIdentityReject",
            _ => match state.timestamp_shape.as_str() {
                "EndedBeforeStartedShape" => "EndedBeforeStartedReject",
                "RecordedBeforeEndedShape" => "RecordedBeforeEndedReject",
                _ => match state.agent_shape.as_str() {
                    "InvalidPrincipal" => "InvalidPrincipalReject",
                    "InvalidInstance" => "InvalidInstanceReject",
                    _ => "NoReject",
                },
            },
        },
    }
}

fn runtime_attestation_record_provider_run(
    state: &RuntimeAttestationRecordShapeState,
) -> (&'static str, &'static str) {
    match state.provider_run_shape.as_str() {
        "LegacyMissingProviderRun" => (
            "__covey_missing_provider_run_id__",
            "__covey_missing_provider_run_id_issuer__",
        ),
        "PartialLegacyId" => ("__covey_missing_provider_run_id__", "provider-issuer-1"),
        "PartialLegacyIssuer" => ("provider-run-1", "__covey_missing_provider_run_id_issuer__"),
        "InvalidProviderRunId" => ("provider-run-1 ", "provider-issuer-1"),
        "InvalidProviderRunIssuer" => ("provider-run-1", " provider-issuer-1"),
        "ObservedProviderRun" => ("provider-run-1", "provider-issuer-1"),
        other => panic!("unexpected provider run shape {other}"),
    }
}

fn runtime_attestation_record_runtime_identity(
    state: &RuntimeAttestationRecordShapeState,
) -> (Option<String>, Option<String>) {
    match state.runtime_identity_shape.as_str() {
        "ProcessOnly" => (Some("1234".to_owned()), None),
        "ContainerOnly" => (None, Some("container-1".to_owned())),
        "ProcessAndContainer" => (Some("1234".to_owned()), Some("container-1".to_owned())),
        "MissingRuntimeIdentityShape" => (None, None),
        "InvalidProcessId" => (Some(" 1234".to_owned()), None),
        "InvalidContainerId" => (None, Some("container-1 ".to_owned())),
        other => panic!("unexpected runtime identity shape {other}"),
    }
}

fn runtime_attestation_record_timestamps(
    state: &RuntimeAttestationRecordShapeState,
) -> (TimestampMs, TimestampMs, TimestampMs) {
    match state.timestamp_shape.as_str() {
        "MonotonicRuntimeSpan" => (
            TimestampMs::parse(10).expect("valid started_at"),
            TimestampMs::parse(11).expect("valid ended_at"),
            TimestampMs::parse(12).expect("valid recorded_at"),
        ),
        "EndedBeforeStartedShape" => (
            TimestampMs::parse(11).expect("valid started_at"),
            TimestampMs::parse(10).expect("valid ended_at"),
            TimestampMs::parse(12).expect("valid recorded_at"),
        ),
        "RecordedBeforeEndedShape" => (
            TimestampMs::parse(10).expect("valid started_at"),
            TimestampMs::parse(12).expect("valid ended_at"),
            TimestampMs::parse(11).expect("valid recorded_at"),
        ),
        other => panic!("unexpected runtime attestation timestamp shape {other}"),
    }
}

fn runtime_attestation_record_actual(
    state: &RuntimeAttestationRecordShapeState,
) -> Option<RuntimeAttestation> {
    let (provider_run_id, provider_run_id_issuer) = runtime_attestation_record_provider_run(state);
    let (process_id, container_id) = runtime_attestation_record_runtime_identity(state);
    let (started_at, ended_at, recorded_at) = runtime_attestation_record_timestamps(state);
    RuntimeAttestation::try_from_parts(
        SessionToken::parse("session-1").expect("valid session token"),
        if state.agent_shape == "InvalidPrincipal" {
            "agent 1"
        } else {
            "agent-1"
        },
        if state.agent_shape == "InvalidInstance" {
            ""
        } else {
            "instance-1"
        },
        SessionRole::Executor,
        ProviderId::parse("provider-1").expect("valid provider"),
        ModelId::parse("model-1").expect("valid model"),
        provider_run_id,
        provider_run_id_issuer,
        process_id,
        container_id,
        CommandTranscriptDigest::parse("blake3:transcript").expect("valid transcript digest"),
        started_at,
        ended_at,
        recorded_at,
    )
    .ok()
}

fn replay_runtime_attestation_record_shape_trace(
    trace: &RuntimeAttestationRecordShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: runtime attestation record scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = runtime_attestation_record_expected_reject(state);
        let expected_accepted = expected_reject == "NoReject";
        let actual = runtime_attestation_record_actual(state);
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: runtime attestation record reject reason disagrees with row facts"
            ));
        }
        if state.accepted != expected_accepted || (state.outcome == "Accepted") != expected_accepted
        {
            violations.push(format!(
                "state[{index}]: runtime attestation record outcome disagrees with row facts"
            ));
        }
        if actual.is_some() != expected_accepted {
            violations.push(format!(
                "state[{index}]: runtime attestation record constructor disagrees with model"
            ));
        }
        if let Some(attestation) = actual {
            if state.process_present != attestation.process_id().is_some()
                || state.container_present != attestation.container_id().is_some()
            {
                violations.push(format!(
                    "state[{index}]: runtime attestation identity projection disagrees with model"
                ));
            }
            if state.provider_run_ref_present != attestation.provider_run_ref().is_some()
                || state.legacy_provider_run_missing != attestation.provider_run_identity_missing()
            {
                violations.push(format!(
                    "state[{index}]: provider-run projection disagrees with model"
                ));
            }
            if attestation.provider_run_ref().is_some()
                && attestation.provider_run_identity_missing()
            {
                violations.push(format!(
                    "state[{index}]: provider-run observed and legacy-missing states overlap"
                ));
            }
        }
    }
    violations
}

fn apply_gate_live_review_ok(state: &ApplyGateEvidenceState) -> bool {
    state.review_exists
        && state.review_decided
        && state.review_approved
        && state.findings_digest_present
        && state.artifact_bound_to_review
}

fn apply_gate_principal_separation_ok(state: &ApplyGateEvidenceState) -> bool {
    state.producer_reviewer_principal_separated
        && state.apply_gate_separated_from_producer
        && state.apply_gate_separated_from_reviewer
}

fn apply_gate_attestations_ok(state: &ApplyGateEvidenceState) -> bool {
    state.producer_attested && state.reviewer_attested && state.apply_gate_attested
}

fn apply_gate_runtime_separation_ok(state: &ApplyGateEvidenceState) -> bool {
    state.producer_reviewer_runtime_separated
        && state.producer_apply_gate_runtime_separated
        && state.reviewer_apply_gate_runtime_separated
        && state.producer_reviewer_provider_run_separated
        && state.producer_apply_gate_provider_run_separated
        && state.reviewer_apply_gate_provider_run_separated
        && state.producer_reviewer_transcript_separated
        && state.producer_apply_gate_transcript_separated
        && state.reviewer_apply_gate_transcript_separated
}

fn apply_gate_verification_ok(state: &ApplyGateEvidenceState) -> bool {
    !state.verification_attempted
        || (state.queue == "InFlight"
            && state.queue_artifact_matches_request
            && state.queue_fence_matches
            && state.verification_review_matches)
}

fn apply_gate_mark_applied_ok(state: &ApplyGateEvidenceState) -> bool {
    !state.mark_applied_attempted
        || (state.queue == "InFlight"
            && state.queue_owner_is_apply_gate
            && state.queue_fence_matches
            && state.queue_lease_live
            && state.subtask_ready_for_apply
            && state.queue_artifact_matches_request
            && state.verification_recorded
            && apply_gate_verification_ok(state))
}

fn apply_gate_landing_authorization_ok(state: &ApplyGateEvidenceState) -> bool {
    !state.landing_authorization_accepted
        || (state.queue == "Applied"
            && state.landing_authorization_artifact_matches
            && state.landing_authorization_fence_matches
            && state.landing_authorization_verification_matches
            && state.verification_recorded)
}

fn apply_gate_receipt_ok(state: &ApplyGateEvidenceState) -> bool {
    !state.receipt_recorded
        || (state.queue == "Applied"
            && state.receipt_artifact_matches
            && state.receipt_fence_matches
            && !state.duplicate_receipt_divergent)
}

fn apply_gate_expected_reject(state: &ApplyGateEvidenceState) -> &'static str {
    if !apply_gate_live_review_ok(state) {
        "LiveReviewInvalid"
    } else if !apply_gate_principal_separation_ok(state) {
        "PrincipalSeparationInvalid"
    } else if !apply_gate_attestations_ok(state) {
        "RuntimeAttestationInvalid"
    } else if !apply_gate_runtime_separation_ok(state) {
        "RuntimeSeparationInvalid"
    } else if !apply_gate_verification_ok(state) {
        "ApplyVerificationInvalid"
    } else if !apply_gate_mark_applied_ok(state) {
        "MarkAppliedInvalid"
    } else if !apply_gate_landing_authorization_ok(state) {
        "LandingAuthorizationInvalid"
    } else if !apply_gate_receipt_ok(state) {
        "LandingReceiptInvalid"
    } else {
        "NoReject"
    }
}

fn replay_apply_gate_evidence_trace(trace: &ApplyGateEvidenceItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "state[{index}]: apply-gate evidence scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if state.outcome == "NotEvaluated" {
            continue;
        }
        let expected_reject = apply_gate_expected_reject(state);
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "state[{index}]: apply-gate evidence reject reason does not match first failed gate"
            ));
        }
        if state.accepted && expected_reject != "NoReject" {
            violations.push(format!(
                "state[{index}]: apply-gate evidence accepted with failed gate"
            ));
        }
        if state.accepted && state.outcome != "Accepted" {
            violations.push(format!(
                "state[{index}]: apply-gate accepted flag disagrees with outcome"
            ));
        }
        if state.accepted
            && !(apply_gate_live_review_ok(state)
                && apply_gate_principal_separation_ok(state)
                && apply_gate_attestations_ok(state)
                && apply_gate_runtime_separation_ok(state))
        {
            violations.push(format!(
                "state[{index}]: apply-gate evidence lacks live approved review or separation"
            ));
        }
        if state.verification_attempted
            && state.accepted
            && !(state.queue == "InFlight"
                && state.queue_fence_matches
                && state.queue_artifact_matches_request
                && state.verification_review_matches)
        {
            violations.push(format!(
                "state[{index}]: apply verification was not bound to current in-flight queue fence"
            ));
        }
        if state.mark_applied_attempted
            && state.accepted
            && !(state.verification_recorded
                && state.queue_owner_is_apply_gate
                && state.queue_lease_live
                && state.subtask_ready_for_apply)
        {
            violations.push(format!(
                "state[{index}]: mark-applied accepted without current verification and queue ownership"
            ));
        }
        if state.landing_authorization_accepted
            && state.accepted
            && !(state.queue == "Applied"
                && state.landing_authorization_artifact_matches
                && state.landing_authorization_fence_matches
                && state.landing_authorization_verification_matches)
        {
            violations.push(format!(
                "state[{index}]: landing authorization accepted without applied queue binding"
            ));
        }
        if state.receipt_recorded
            && state.accepted
            && !(state.queue == "Applied"
                && state.receipt_artifact_matches
                && state.receipt_fence_matches
                && !state.duplicate_receipt_divergent)
        {
            violations.push(format!(
                "state[{index}]: landing receipt recorded without applied artifact/fence binding"
            ));
        }
    }
    violations
}

fn replay_landing_receipt_trace(trace: &LandingReceiptItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        let receipt_recorded = state.receipt == "ReceiptRecorded";
        if receipt_recorded && state.queue != "Applied" {
            violations.push(format!(
                "state[{index}]: landing receipt exists before applied queue"
            ));
        }
        if receipt_recorded && !state.receipt_actor_authorized {
            violations.push(format!(
                "state[{index}]: landing receipt lacks authorized recorder"
            ));
        }
        if receipt_recorded && !(state.receipt_artifact_matches && state.receipt_fence_matches) {
            violations.push(format!(
                "state[{index}]: landing receipt lacks artifact/fence match"
            ));
        }
        if receipt_recorded
            && (state.receipt_target == "NoTarget" || state.receipt_commit == "NoCommit")
        {
            violations.push(format!(
                "state[{index}]: landing receipt lacks target or commit binding"
            ));
        }
        if state.queue == "Superseded" && receipt_recorded {
            violations.push(format!(
                "state[{index}]: superseded queue recorded landing receipt"
            ));
        }
        if state.last_attempt == "Accepted" && !receipt_recorded {
            violations.push(format!(
                "state[{index}]: accepted landing receipt attempt did not record receipt"
            ));
        }
        if state.last_attempt == "ReplayedSame"
            && !(receipt_recorded
                && state.receipt_target == state.attempted_target
                && state.receipt_commit == state.attempted_commit)
        {
            violations.push(format!(
                "state[{index}]: replayed landing receipt changed recorded receipt"
            ));
        }
        let divergent_attempt = receipt_recorded
            && state.attempted_target != "NoTarget"
            && state.attempted_commit != "NoCommit"
            && (state.receipt_target != state.attempted_target
                || state.receipt_commit != state.attempted_commit);
        if divergent_attempt
            && !(state.last_attempt == "Rejected" && state.divergent_attempt_rejected)
        {
            violations.push(format!(
                "state[{index}]: divergent landing receipt attempt was not rejected"
            ));
        }
        if state.last_attempt == "Rejected" && state.receipt_created_by_last_attempt {
            violations.push(format!(
                "state[{index}]: rejected landing receipt attempt created receipt"
            ));
        }
        if state.receipt_actor_authorized && !receipt_recorded {
            violations.push(format!(
                "state[{index}]: receipt recorder binding exists without receipt"
            ));
        }
        if (state.receipt_artifact_matches || state.receipt_fence_matches) && !receipt_recorded {
            violations.push(format!(
                "state[{index}]: receipt match binding exists without receipt"
            ));
        }
        if state.receipt_created_by_last_attempt && state.last_attempt != "Accepted" {
            violations.push(format!(
                "state[{index}]: non-accepted attempt marked receipt creation"
            ));
        }
        if state.last_attempt == "Accepted"
            && !(state.actor_authorized && state.artifact_matches && state.fence_matches)
        {
            violations.push(format!(
                "state[{index}]: accepted receipt attempt lacked live preconditions"
            ));
        }
    }
    violations
}

fn openspec_source_version(source: &str) -> i64 {
    if source == "SourceV1" { 1 } else { 2 }
}

fn openspec_store_version(store: &str) -> i64 {
    match store {
        "ImportedV1" | "ActiveClaimV1" | "TerminalSameTitle" => 1,
        "ImportedV2" => 2,
        _ => 0,
    }
}

fn openspec_store_has_imported_subtask(store: &str) -> bool {
    matches!(
        store,
        "ImportedV1" | "ImportedV2" | "ActiveClaimV1" | "TerminalSameTitle"
    )
}

fn openspec_expected_diff(state: &OpenSpecImportState) -> &'static str {
    if state.store == "Empty" {
        "CreateDiff"
    } else if state.store == "DifferentMetaTask"
        || (state.store == "ActiveClaimV1" && state.source == "SourceV2")
    {
        "ConflictDiff"
    } else if openspec_store_version(&state.store) == openspec_source_version(&state.source) {
        "UnchangedDiff"
    } else {
        "UpdateDiff"
    }
}

fn openspec_expected_conflict(state: &OpenSpecImportState) -> &'static str {
    if state.store == "DifferentMetaTask" {
        "ExistingSubtaskDifferentMetaTask"
    } else if state.store == "ActiveClaimV1" && state.source == "SourceV2" {
        "ActiveClaimChangedSource"
    } else {
        "NoConflict"
    }
}

fn replay_openspec_import_trace(trace: &OpenSpecImportItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        let prefix = format!("state[{index}]");
        if !state.evaluated {
            continue;
        }
        let any_write = state.meta_written
            || state.subtask_written
            || state.provenance_written
            || state.import_event_written
            || state.dependencies_written;
        if state.diff != openspec_expected_diff(state) {
            violations.push(format!(
                "{prefix}: OpenSpec diff does not match store/source"
            ));
        }
        if state.conflict_reason != openspec_expected_conflict(state) {
            violations.push(format!(
                "{prefix}: OpenSpec conflict reason does not match store/source"
            ));
        }
        if state.mode == "DryRun" && any_write {
            violations.push(format!("{prefix}: dry-run OpenSpec import wrote state"));
        }
        if state.apply_result == "Applied"
            && !(state.mode == "Write" && state.role == "Orchestrator")
        {
            violations.push(format!(
                "{prefix}: OpenSpec write applied without orchestrator role"
            ));
        }
        if state.conflict_reason != "NoConflict"
            && state.mode == "Write"
            && state.role == "Orchestrator"
            && !(state.apply_result == "Rejected" && !any_write)
        {
            violations.push(format!(
                "{prefix}: conflicting OpenSpec import partially applied"
            ));
        }
        if state.store == "ActiveClaimV1"
            && state.source == "SourceV2"
            && !(state.diff == "ConflictDiff"
                && state.conflict_reason == "ActiveClaimChangedSource"
                && state.claim_live)
        {
            violations.push(format!(
                "{prefix}: active claimed OpenSpec task update did not conflict"
            ));
        }
        if state.store == "DifferentMetaTask"
            && !(state.diff == "ConflictDiff"
                && state.conflict_reason == "ExistingSubtaskDifferentMetaTask")
        {
            violations.push(format!(
                "{prefix}: imported OpenSpec subtask in different meta-task did not conflict"
            ));
        }
        if (state.provenance_written || state.import_event_written)
            && !(state.apply_result == "Applied"
                && matches!(state.diff.as_str(), "CreateDiff" | "UpdateDiff"))
        {
            violations.push(format!(
                "{prefix}: OpenSpec provenance/event written without applied diff"
            ));
        }
        if state.dependencies_written
            && !(state.apply_result == "Applied" && state.conflict_reason == "NoConflict")
        {
            violations.push(format!(
                "{prefix}: OpenSpec dependencies written without applied conflict-free diff"
            ));
        }
        if state.diff == "UnchangedDiff" && any_write {
            violations.push(format!("{prefix}: unchanged OpenSpec import wrote state"));
        }
        if state.diff == "CreateDiff" && openspec_store_has_imported_subtask(&state.store) {
            violations.push(format!(
                "{prefix}: OpenSpec import created over imported subtask"
            ));
        }
        if state.diff == "UpdateDiff" && state.claim_live {
            violations.push(format!(
                "{prefix}: OpenSpec import updated a live-claimed task"
            ));
        }
        if state.claim_created_by_import {
            violations.push(format!("{prefix}: OpenSpec import created a live claim"));
        }
    }
    violations
}

fn bd_import_expected_conflict(state: &BdImportState) -> &'static str {
    match (
        state.destination.as_str(),
        state.role.as_str(),
        state.source.as_str(),
    ) {
        ("NoDestination" | "BothDestinations", _, _) => "InvalidDestination",
        ("ExistingTerminalMeta", _, _) => "MetaUnavailable",
        (_, _, "MissingDb") => "SourceUnavailable",
        (_, _, "BadSchema") => "SourceSchemaInvalid",
        (_, role, _) if role != "Orchestrator" => "RoleNotOrchestrator",
        (_, _, "DuplicateDifferentMeta") => "DuplicateDifferentMetaConflict",
        _ => "NoConflict",
    }
}

fn bd_import_expected_imported_count(source: &str) -> i64 {
    match source {
        "OneImportable" => 1,
        "MixedValidInvalid" => 2,
        "OrderedByPriorityDependencyId" => 3,
        _ => 0,
    }
}

fn bd_import_expected_skipped_count(source: &str) -> i64 {
    match source {
        "MixedValidInvalid" => 3,
        "DuplicateSameMeta" => 1,
        _ => 0,
    }
}

fn replay_bd_import_trace(trace: &BdImportItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        let prefix = format!("state[{index}]");
        if !state.evaluated {
            continue;
        }
        let any_write = state.meta_written || state.subtask_written || state.event_written;
        let expected_conflict = bd_import_expected_conflict(state);
        if state.conflict_reason != expected_conflict {
            violations.push(format!(
                "{prefix}: BD import conflict reason does not match inputs"
            ));
        }
        if matches!(
            state.destination.as_str(),
            "NoDestination" | "BothDestinations"
        ) && !(state.outcome == "Rejected" && !any_write)
        {
            violations.push(format!(
                "{prefix}: invalid BD import destination wrote state"
            ));
        }
        if state.destination == "ExistingTerminalMeta"
            && !(state.conflict_reason == "MetaUnavailable"
                && state.outcome == "Rejected"
                && !any_write)
        {
            violations.push(format!(
                "{prefix}: terminal BD import destination did not reject atomically"
            ));
        }
        if matches!(state.source.as_str(), "MissingDb" | "BadSchema")
            && !(state.outcome == "Rejected" && !any_write)
        {
            violations.push(format!(
                "{prefix}: failed BD import source wrote destination or subtasks"
            ));
        }
        if state.role != "Orchestrator"
            && !(state.outcome == "Rejected"
                && state.conflict_reason == "RoleNotOrchestrator"
                && !any_write)
        {
            violations.push(format!(
                "{prefix}: non-orchestrator BD import mutated state"
            ));
        }
        if state.source == "DuplicateDifferentMeta"
            && !(state.outcome == "Rejected"
                && state.conflict_reason == "DuplicateDifferentMetaConflict"
                && !any_write)
        {
            violations.push(format!(
                "{prefix}: duplicate-different-meta BD import was not atomic"
            ));
        }
        if state.source == "DuplicateSameMeta"
            && !(state.outcome == "Applied"
                && state.imported_count == 0
                && state.skipped_count == 1
                && state.duplicate_skip_has_subtask)
        {
            violations.push(format!(
                "{prefix}: duplicate-same-meta BD import was not a deterministic skip"
            ));
        }
        if state.invalid_skip_has_subtask {
            violations.push(format!(
                "{prefix}: invalid BD import skip item carried subtask id"
            ));
        }
        if state.imported_count > 0
            && !(state.subtask_written && state.subtask_shape == "AvailableWorkOnly")
        {
            violations.push(format!(
                "{prefix}: imported BD rows did not become available work subtasks"
            ));
        }
        if state.source == "EmptySource"
            && !(state.outcome == "Applied"
                && state.meta_written == (state.destination == "NewMeta")
                && !state.subtask_written
                && state.imported_count == 0
                && state.skipped_count == 0)
        {
            violations.push(format!(
                "{prefix}: empty BD import created unexpected subtasks or counts"
            ));
        }
        if state.source == "OrderedByPriorityDependencyId"
            && !(state.outcome == "Applied"
                && state.item_order == "PriorityDependencyId"
                && state.imported_count == 3)
        {
            violations.push(format!(
                "{prefix}: BD import ordering did not follow priority/dependency/id"
            ));
        }
        if state.claim_created || state.session_active_subtask_set {
            violations.push(format!("{prefix}: BD import created live claim state"));
        }
        if state.outcome == "Applied"
            && (state.imported_count != bd_import_expected_imported_count(&state.source)
                || state.skipped_count != bd_import_expected_skipped_count(&state.source))
        {
            violations.push(format!(
                "{prefix}: BD import counts do not match source eligibility"
            ));
        }
        if state.outcome == "Rejected"
            && (state.imported_count != 0
                || state.skipped_count != 0
                || state.item_order != "NoItems")
        {
            violations.push(format!("{prefix}: rejected BD import reported items"));
        }
    }
    violations
}

fn bd_import_item_expected_reject(state: &BdImportItemOutcomeShapeState) -> &'static str {
    if state.skip_reason == "DeterministicDuplicate" && !state.subtask_id_present {
        "DuplicateMissingSubtaskReject"
    } else if state.skip_reason == "InvalidRow" && state.subtask_id_present {
        "InvalidRowHasSubtaskReject"
    } else if state.skip_reason == "NoSkipReason" && !state.subtask_id_present {
        "MissingSubtaskAndSkipReasonReject"
    } else {
        "NoReject"
    }
}

fn bd_import_item_expected_outcome(state: &BdImportItemOutcomeShapeState) -> &'static str {
    let reject = bd_import_item_expected_reject(state);
    if reject != "NoReject" {
        "Rejected"
    } else if state.skip_reason == "NoSkipReason" {
        "Imported"
    } else if state.skip_reason == "DeterministicDuplicate" {
        "SkippedDuplicate"
    } else {
        "SkippedInvalidRow"
    }
}

fn replay_bd_import_item_outcome_shape_trace(
    trace: &BdImportItemOutcomeShapeItfTrace,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        let prefix = format!("state[{index}]");
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "{prefix}: BD import item scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_reject = bd_import_item_expected_reject(state);
        if state.reject_reason != expected_reject {
            violations.push(format!(
                "{prefix}: BD import item reject reason does not match outcome shape"
            ));
        }
        if state.outcome != bd_import_item_expected_outcome(state) {
            violations.push(format!(
                "{prefix}: BD import item outcome does not match subtask/skip shape"
            ));
        }
        if state.outcome == "Imported"
            && !(state.subtask_id_present && state.skip_reason == "NoSkipReason")
        {
            violations.push(format!(
                "{prefix}: imported BD item lacks subtask or has skip reason"
            ));
        }
        if state.outcome == "SkippedDuplicate"
            && !(state.subtask_id_present && state.skip_reason == "DeterministicDuplicate")
        {
            violations.push(format!(
                "{prefix}: duplicate BD item lacks duplicate subtask binding"
            ));
        }
        if state.outcome == "SkippedInvalidRow"
            && (state.subtask_id_present || state.skip_reason != "InvalidRow")
        {
            violations.push(format!(
                "{prefix}: invalid-row BD item carried subtask or wrong skip reason"
            ));
        }
    }
    violations
}

fn claim_dependency_role_can_claim(role: ClaimDependencyRole, kind: ClaimDependencyKind) -> bool {
    matches!(
        (role, kind),
        (ClaimDependencyRole::Executor, ClaimDependencyKind::Work)
            | (ClaimDependencyRole::Reviewer, ClaimDependencyKind::Review)
    )
}

fn claim_dependency_meta_claimable(meta: ClaimDependencyMeta) -> bool {
    matches!(
        meta,
        ClaimDependencyMeta::MetaActive | ClaimDependencyMeta::MetaPlanning
    )
}

fn claim_dependency_satisfied(
    kind: ClaimDependencyKind,
    lineage: ClaimDependencyLineage,
    dependency: ClaimDependencyDependency,
) -> bool {
    if kind == ClaimDependencyKind::Review {
        return true;
    }
    if lineage == ClaimDependencyLineage::ReviewFollowupCandidate {
        return true;
    }
    matches!(
        dependency,
        ClaimDependencyDependency::NoDependency
            | ClaimDependencyDependency::DepApproved
            | ClaimDependencyDependency::DepReadyForApply
            | ClaimDependencyDependency::DepApplied
            | ClaimDependencyDependency::DepDecided
            | ClaimDependencyDependency::DepChangesRequestedFollowupApproved
            | ClaimDependencyDependency::DepChangesRequestedFollowupReadyForApply
            | ClaimDependencyDependency::DepChangesRequestedFollowupApplied
            | ClaimDependencyDependency::DepChangesRequestedFollowupDecided
    )
}

fn claim_dependency_expected_reject(
    state: &ClaimDependencyGateState,
) -> ClaimDependencyRejectReason {
    if state.session == ClaimDependencySession::SessionInactive {
        ClaimDependencyRejectReason::SessionUnavailable
    } else if state.session == ClaimDependencySession::SessionOccupied {
        ClaimDependencyRejectReason::SessionAlreadyOccupied
    } else if !claim_dependency_role_can_claim(state.role, state.kind) {
        ClaimDependencyRejectReason::WrongRole
    } else if !claim_dependency_meta_claimable(state.meta) {
        ClaimDependencyRejectReason::MetaUnavailable
    } else if state.candidate != ClaimDependencyCandidate::CandidateAvailable
        || (state.kind == ClaimDependencyKind::Review
            && state.lineage != ClaimDependencyLineage::PlainCandidate)
    {
        ClaimDependencyRejectReason::IllegalTransition
    } else if !claim_dependency_satisfied(state.kind, state.lineage, state.dependency) {
        ClaimDependencyRejectReason::DependencyUnsatisfied
    } else {
        ClaimDependencyRejectReason::NoReject
    }
}

fn claim_dependency_expected_decision(state: &ClaimDependencyGateState) -> ClaimDependencyDecision {
    let reject = claim_dependency_expected_reject(state);
    if reject == ClaimDependencyRejectReason::NoReject {
        ClaimDependencyDecision::ClaimCreated
    } else if state.claim_path == ClaimDependencyPath::ClaimNext
        && matches!(
            reject,
            ClaimDependencyRejectReason::MetaUnavailable
                | ClaimDependencyRejectReason::IllegalTransition
                | ClaimDependencyRejectReason::DependencyUnsatisfied
        )
    {
        ClaimDependencyDecision::NoClaimableCandidate
    } else {
        ClaimDependencyDecision::Rejected
    }
}

fn replay_claim_dependency_gate_trace(trace: &ClaimDependencyGateItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous_case_index = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        let prefix = format!("state[{index}]");
        if let Some(previous_case_index) = previous_case_index {
            if state.case_index < previous_case_index {
                violations.push(format!(
                    "{prefix}: claim dependency scenario index moved backward"
                ));
            }
        }
        previous_case_index = Some(state.case_index);
        if !state.evaluated {
            continue;
        }
        let expected_decision = claim_dependency_expected_decision(state);
        if state.decision != expected_decision {
            violations.push(format!(
                "{prefix}: claim dependency decision does not match inputs"
            ));
        }
        let expected_dependency =
            claim_dependency_satisfied(state.kind, state.lineage, state.dependency);
        if state.dependency_satisfied != expected_dependency {
            violations.push(format!(
                "{prefix}: dependency satisfaction marker does not match state"
            ));
        }
        if state.claim_created
            && state.kind == ClaimDependencyKind::Work
            && !state.dependency_satisfied
        {
            violations.push(format!(
                "{prefix}: work claim ignored unsatisfied dependency"
            ));
        }
        if state.kind == ClaimDependencyKind::Work
            && state.dependency == ClaimDependencyDependency::DepOpen
            && state.claim_created
        {
            violations.push(format!("{prefix}: open dependency allowed work claim"));
        }
        if state.kind == ClaimDependencyKind::Work
            && state.lineage == ClaimDependencyLineage::ChangesRequestedSourceCandidate
            && matches!(
                state.dependency,
                ClaimDependencyDependency::DepChangesRequestedNoFollowup
                    | ClaimDependencyDependency::DepChangesRequestedFollowupAvailable
            )
            && state.claim_created
        {
            violations.push(format!(
                "{prefix}: changes-requested dependency without terminal follow-up allowed claim"
            ));
        }
        if state.kind == ClaimDependencyKind::Work
            && state.lineage == ClaimDependencyLineage::ReviewFollowupCandidate
            && state.dependency == ClaimDependencyDependency::DepChangesRequestedFollowupAvailable
            && state.claim_path == ClaimDependencyPath::ClaimNext
            && !state.claim_created
        {
            violations.push(format!(
                "{prefix}: review follow-up candidate for changes-requested source was not claimed"
            ));
        }
        if state.kind == ClaimDependencyKind::Work
            && matches!(
                state.dependency,
                ClaimDependencyDependency::DepChangesRequestedFollowupApproved
                    | ClaimDependencyDependency::DepChangesRequestedFollowupReadyForApply
                    | ClaimDependencyDependency::DepChangesRequestedFollowupApplied
                    | ClaimDependencyDependency::DepChangesRequestedFollowupDecided
            )
            && !state.dependency_satisfied
        {
            violations.push(format!(
                "{prefix}: terminal follow-up did not satisfy source dependency"
            ));
        }
        if state.kind == ClaimDependencyKind::Work
            && matches!(
                state.dependency,
                ClaimDependencyDependency::DepApproved
                    | ClaimDependencyDependency::DepReadyForApply
                    | ClaimDependencyDependency::DepApplied
                    | ClaimDependencyDependency::DepDecided
            )
            && !state.dependency_satisfied
        {
            violations.push(format!(
                "{prefix}: terminal dependency state did not satisfy dependency"
            ));
        }
        if !state.claim_created
            && (state.subtask_claimed || state.session_active_subtask_set || state.fence_issued)
        {
            violations.push(format!("{prefix}: blocked claim mutated claim state"));
        }
        if state.claim_created
            && !(state.subtask_claimed && state.session_active_subtask_set && state.fence_issued)
        {
            violations.push(format!(
                "{prefix}: created claim lacks subtask/session/fence binding"
            ));
        }
        if state.claim_path == ClaimDependencyPath::ClaimNext
            && state.decision == ClaimDependencyDecision::NoClaimableCandidate
            && state.claim_created
        {
            violations.push(format!("{prefix}: claim-next created blocked candidate"));
        }
        if state.claim_path == ClaimDependencyPath::TargetedClaim
            && claim_dependency_expected_reject(state) != ClaimDependencyRejectReason::NoReject
            && state.decision != ClaimDependencyDecision::Rejected
        {
            violations.push(format!(
                "{prefix}: targeted blocked candidate did not reject"
            ));
        }
        if !claim_dependency_role_can_claim(state.role, state.kind) && state.claim_created {
            violations.push(format!("{prefix}: wrong role created claim"));
        }
        if state.session == ClaimDependencySession::SessionOccupied && state.claim_created {
            violations.push(format!("{prefix}: occupied session created claim"));
        }
        if !claim_dependency_meta_claimable(state.meta) && state.claim_created {
            violations.push(format!("{prefix}: terminal meta created claim"));
        }
        if state.candidate_selected && state.candidate == ClaimDependencyCandidate::NoCandidate {
            violations.push(format!("{prefix}: selected absent candidate"));
        }
    }
    violations
}

fn mutation_idempotency_valid_key(key: &str) -> bool {
    matches!(key, "Key1" | "Key2")
}

fn mutation_idempotency_same_identity(
    record: &MutationIdempotencyRecordState,
    actor: &str,
    operation: &str,
    key: &str,
) -> bool {
    record.present && record.actor == actor && record.operation == operation && record.key == key
}

fn mutation_idempotency_matching_record<'a>(
    state: &'a MutationIdempotencyState,
    actor: &str,
    operation: &str,
    key: &str,
) -> Option<&'a MutationIdempotencyRecordState> {
    if mutation_idempotency_same_identity(&state.r0, actor, operation, key) {
        Some(&state.r0)
    } else if mutation_idempotency_same_identity(&state.r1, actor, operation, key) {
        Some(&state.r1)
    } else {
        None
    }
}

fn replay_mutation_idempotency_trace(trace: &MutationIdempotencyItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous: Option<&MutationIdempotencyState> = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        let prefix = format!("state[{index}]");
        let matching = mutation_idempotency_matching_record(
            state,
            &state.last_actor,
            &state.last_operation,
            &state.last_key,
        );

        if let Some(previous_state) = previous {
            if state.side_effects < previous_state.side_effects {
                violations.push(format!("{prefix}: side effect count moved backward"));
            }
            if state.record_writes < previous_state.record_writes {
                violations.push(format!("{prefix}: idempotency record count moved backward"));
            }
            if state.side_effects - previous_state.side_effects != state.last_side_effect_delta {
                violations.push(format!(
                    "{prefix}: side effect delta disagrees with previous state"
                ));
            }
            if state.record_writes - previous_state.record_writes != state.last_record_write_delta {
                violations.push(format!(
                    "{prefix}: record write delta disagrees with previous state"
                ));
            }
        }

        if state.r0.present
            && state.r1.present
            && mutation_idempotency_same_identity(
                &state.r0,
                &state.r1.actor,
                &state.r1.operation,
                &state.r1.key,
            )
        {
            violations.push(format!(
                "{prefix}: duplicate idempotency records share one actor/operation/key"
            ));
        }

        if state.last_outcome == "InvalidKey"
            && (state.last_side_effect_delta != 0 || state.last_record_write_delta != 0)
        {
            violations.push(format!("{prefix}: invalid key ran mutation side effects"));
        }
        if state.last_outcome == "Created"
            && !(mutation_idempotency_valid_key(&state.last_key)
                && state.last_closure_ok
                && state.last_serialize_ok
                && !state.identity_matched_before_attempt
                && state.last_side_effect_delta == 1
                && state.last_record_write_delta == 1)
        {
            violations.push(format!(
                "{prefix}: idempotency record was created without one successful mutation"
            ));
        }
        if state.last_outcome == "Replayed" {
            match matching {
                Some(record)
                    if state.identity_matched_before_attempt
                        && record.request_hash == state.last_request_hash
                        && record.response == state.last_response
                        && state.last_side_effect_delta == 0
                        && state.last_record_write_delta == 0 => {}
                _ => violations.push(format!(
                    "{prefix}: idempotent replay did not return stored response without side effects"
                )),
            }
        }
        if state.last_outcome == "Conflict"
            && !(state.identity_matched_before_attempt
                && matching.is_some()
                && state.last_side_effect_delta == 0
                && state.last_record_write_delta == 0)
        {
            violations.push(format!(
                "{prefix}: request drift conflict mutated state or crossed namespace"
            ));
        }
        if matches!(
            state.last_outcome.as_str(),
            "ClosureFailed" | "SerializeFailed" | "InvalidKey" | "Conflict" | "CapacityFull"
        ) && (state.last_side_effect_delta != 0 || state.last_record_write_delta != 0)
        {
            violations.push(format!("{prefix}: failed idempotent mutation wrote state"));
        }
        if state.last_outcome == "DeserializeFailed" {
            match matching {
                Some(record)
                    if state.identity_matched_before_attempt
                        && !record.response_json_valid
                        && state.last_side_effect_delta == 0
                        && state.last_record_write_delta == 0 => {}
                _ => violations.push(format!(
                    "{prefix}: stored-response deserialize failure reran mutation or lacked record"
                )),
            }
        }

        previous = Some(state);
    }
    violations
}

fn event_log_record_count(state: &EventLogState) -> i64 {
    i64::from(state.e0.present) + i64::from(state.e1.present)
}

fn event_log_expected_event_type(payload: &str) -> &'static str {
    match payload {
        "HeartbeatPayload" => "SessionHeartbeat",
        "ExpiredCountPayload" => "ClaimsExpired",
        _ => "NoEvent",
    }
}

fn event_log_expected_object_type(payload: &str) -> &'static str {
    match payload {
        "HeartbeatPayload" => "Session",
        "ExpiredCountPayload" => "Claim",
        _ => "NoObject",
    }
}

fn event_log_payload_matches(event_type: &str, payload: &str) -> bool {
    !matches!(payload, "NoPayload" | "MalformedPayload")
        && event_log_expected_event_type(payload) == event_type
}

fn event_log_object_matches(object_type: &str, payload: &str) -> bool {
    !matches!(payload, "NoPayload" | "MalformedPayload")
        && event_log_expected_object_type(payload) == object_type
}

fn event_log_actor_valid(actor: &str, token: &str) -> bool {
    matches!(
        (actor, token),
        ("SessionActor", "ValidSessionToken") | ("SystemActor", "SystemSentinel")
    )
}

fn replay_event_log_trace(trace: &EventLogItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    let mut previous: Option<&EventLogState> = None;
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        let prefix = format!("state[{index}]");
        let count = event_log_record_count(state);
        if state.next_seq != count + 1 {
            violations.push(format!(
                "{prefix}: next event sequence does not match append-only count"
            ));
        }
        if state.e0.present && state.e0.seq != 1 {
            violations.push(format!("{prefix}: first event does not have sequence 1"));
        }
        if state.e1.present && !(state.e0.present && state.e1.seq == 2) {
            violations.push(format!(
                "{prefix}: second event is not contiguous after first event"
            ));
        }
        for (slot, record) in [("e0", &state.e0), ("e1", &state.e1)] {
            if !record.present {
                continue;
            }
            if !record.readable {
                violations.push(format!("{prefix}: {slot} committed unreadable event row"));
            }
            if !event_log_payload_matches(&record.event_type, &record.payload) {
                violations.push(format!(
                    "{prefix}: {slot} event_type does not match payload"
                ));
            }
            if !event_log_object_matches(&record.object_type, &record.payload) {
                violations.push(format!(
                    "{prefix}: {slot} object_type does not match payload"
                ));
            }
            if record.actor == "SessionActor" && !record.visible_session_token {
                violations.push(format!("{prefix}: {slot} session actor lacks token"));
            }
            if record.actor == "SystemActor" && record.visible_session_token {
                violations.push(format!("{prefix}: {slot} system actor exposed token"));
            }
        }
        let expected_payload_match =
            event_log_payload_matches(&state.last_event_type, &state.last_payload);
        let expected_object_match =
            event_log_object_matches(&state.last_object_type, &state.last_payload);
        let expected_actor_valid = event_log_actor_valid(&state.last_actor, &state.last_token);
        let expected_readable =
            expected_payload_match && expected_object_match && expected_actor_valid;
        if state.last_payload_matches != expected_payload_match {
            violations.push(format!(
                "{prefix}: last payload-match marker disagrees with event type"
            ));
        }
        if state.last_object_matches != expected_object_match {
            violations.push(format!(
                "{prefix}: last object-match marker disagrees with payload"
            ));
        }
        if state.last_actor_valid != expected_actor_valid {
            violations.push(format!(
                "{prefix}: last actor-valid marker disagrees with actor/token"
            ));
        }
        if state.last_readable != expected_readable {
            violations.push(format!(
                "{prefix}: last readable marker disagrees with event shape"
            ));
        }
        if matches!(
            state.last_outcome.as_str(),
            "Rejected" | "SerializeFailed" | "RolledBack" | "CapacityFull"
        ) && (state.last_count_delta != 0 || state.last_seq_assigned != 0)
        {
            violations.push(format!(
                "{prefix}: rejected or rolled-back append consumed event sequence"
            ));
        }
        if state.last_outcome == "Committed"
            && !(state.last_readable
                && state.last_mutation_ok
                && state.last_count_delta == 1
                && state.last_seq_assigned > 0)
        {
            violations.push(format!(
                "{prefix}: committed append did not add exactly one readable event"
            ));
        }
        if !state.last_readable
            && !matches!(
                state.last_outcome.as_str(),
                "NoAttempt" | "Rejected" | "SerializeFailed"
            )
        {
            violations.push(format!("{prefix}: unreadable append was not rejected"));
        }
        if let Some(previous_state) = previous {
            let previous_count = event_log_record_count(previous_state);
            if count < previous_count {
                violations.push(format!("{prefix}: event log count moved backward"));
            }
            if count - previous_count != state.last_count_delta {
                violations.push(format!(
                    "{prefix}: event count delta disagrees with previous state"
                ));
            }
        }
        previous = Some(state);
    }
    violations
}

#[fixture]
fn review_followup_trace() -> ItfTrace {
    serde_json::from_str(COVEY_REVIEW_FOLLOWUP_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn review_claim_reclaim_trace() -> ReviewClaimReclaimItfTrace {
    serde_json::from_str(COVEY_REVIEW_CLAIM_RECLAIM_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn review_claim_reclaim_changes_requested_trace() -> ReviewClaimReclaimItfTrace {
    serde_json::from_str(COVEY_REVIEW_CLAIM_RECLAIM_CHANGES_REQUESTED_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn core_lifecycle_trace() -> CoreItfTrace {
    serde_json::from_str(COVEY_CORE_LIFECYCLE_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn stale_claim_recovery_reap_trace() -> StaleClaimRecoveryItfTrace {
    serde_json::from_str(COVEY_STALE_CLAIM_RECOVERY_REAP_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn stale_claim_recovery_lease_trace() -> StaleClaimRecoveryItfTrace {
    serde_json::from_str(COVEY_STALE_CLAIM_RECOVERY_LEASE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn stale_claim_recovery_exit_trace() -> StaleClaimRecoveryItfTrace {
    serde_json::from_str(COVEY_STALE_CLAIM_RECOVERY_EXIT_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn queue_reservation_trace() -> QueueReservationItfTrace {
    serde_json::from_str(COVEY_QUEUE_RESERVATION_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn ready_queue_claim_selection_trace() -> ReadyQueueClaimSelectionItfTrace {
    serde_json::from_str(COVEY_READY_QUEUE_CLAIM_SELECTION_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn ready_queue_metrics_trace() -> ReadyQueueMetricsItfTrace {
    serde_json::from_str(COVEY_READY_QUEUE_METRICS_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn reservation_overlap_trace() -> ReservationOverlapItfTrace {
    serde_json::from_str(COVEY_RESERVATION_OVERLAP_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn repoops_snapshot_trace() -> RepoopsSnapshotItfTrace {
    serde_json::from_str(COVEY_REPOOPS_SNAPSHOT_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn repoops_snapshot_request_shape_trace() -> RepoopsSnapshotRequestShapeItfTrace {
    serde_json::from_str(COVEY_REPOOPS_SNAPSHOT_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn transition_matrix_trace() -> TransitionMatrixItfTrace {
    serde_json::from_str(COVEY_TRANSITION_MATRIX_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn subtask_domain_lifecycle_shape_trace() -> SubtaskDomainLifecycleShapeItfTrace {
    serde_json::from_str(COVEY_SUBTASK_DOMAIN_LIFECYCLE_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn apply_proof_row_lifecycle_trace() -> ApplyProofRowLifecycleItfTrace {
    serde_json::from_str(COVEY_APPLY_PROOF_ROW_LIFECYCLE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn record_lifecycle_shape_trace() -> RecordLifecycleShapeItfTrace {
    serde_json::from_str(COVEY_RECORD_LIFECYCLE_SHAPE_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn runtime_attestation_record_shape_trace() -> RuntimeAttestationRecordShapeItfTrace {
    serde_json::from_str(COVEY_RUNTIME_ATTESTATION_RECORD_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn reservation_record_shape_trace() -> ReservationRecordShapeItfTrace {
    serde_json::from_str(COVEY_RESERVATION_RECORD_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn apply_gate_evidence_trace() -> ApplyGateEvidenceItfTrace {
    serde_json::from_str(COVEY_APPLY_GATE_EVIDENCE_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn session_meta_task_trace() -> SessionMetaTaskItfTrace {
    serde_json::from_str(COVEY_SESSION_META_TASK_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn session_request_shape_trace() -> SessionRequestShapeItfTrace {
    serde_json::from_str(COVEY_SESSION_REQUEST_SHAPE_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn session_handle_shape_trace() -> SessionHandleShapeItfTrace {
    serde_json::from_str(COVEY_SESSION_HANDLE_SHAPE_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn meta_task_request_shape_trace() -> MetaTaskRequestShapeItfTrace {
    serde_json::from_str(COVEY_META_TASK_REQUEST_SHAPE_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn create_subtask_request_shape_trace() -> CreateSubtaskRequestShapeItfTrace {
    serde_json::from_str(COVEY_CREATE_SUBTASK_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn claim_acquisition_request_shape_trace() -> ClaimAcquisitionRequestShapeItfTrace {
    serde_json::from_str(COVEY_CLAIM_ACQUISITION_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn supersede_queue_item_request_shape_trace() -> SupersedeQueueItemRequestShapeItfTrace {
    serde_json::from_str(COVEY_SUPERSEDE_QUEUE_ITEM_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn view_attachment_shape_trace() -> ViewAttachmentShapeItfTrace {
    serde_json::from_str(COVEY_VIEW_ATTACHMENT_SHAPE_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn runtime_attestation_request_shape_trace() -> RuntimeAttestationRequestShapeItfTrace {
    serde_json::from_str(COVEY_RUNTIME_ATTESTATION_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn publish_artifact_request_shape_trace() -> PublishArtifactRequestShapeItfTrace {
    serde_json::from_str(COVEY_PUBLISH_ARTIFACT_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn decide_review_request_shape_trace() -> DecideReviewRequestShapeItfTrace {
    serde_json::from_str(COVEY_DECIDE_REVIEW_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn request_review_request_shape_trace() -> RequestReviewRequestShapeItfTrace {
    serde_json::from_str(COVEY_REQUEST_REVIEW_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn enqueue_for_apply_request_shape_trace() -> EnqueueForApplyRequestShapeItfTrace {
    serde_json::from_str(COVEY_ENQUEUE_FOR_APPLY_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn claim_ready_queue_request_shape_trace() -> ClaimReadyQueueRequestShapeItfTrace {
    serde_json::from_str(COVEY_CLAIM_READY_QUEUE_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn claim_lifecycle_request_shape_trace() -> ClaimLifecycleRequestShapeItfTrace {
    serde_json::from_str(COVEY_CLAIM_LIFECYCLE_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn mark_in_flight_request_shape_trace() -> MarkInFlightRequestShapeItfTrace {
    serde_json::from_str(COVEY_MARK_IN_FLIGHT_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn mark_applied_request_shape_trace() -> MarkAppliedRequestShapeItfTrace {
    serde_json::from_str(COVEY_MARK_APPLIED_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn apply_verification_request_shape_trace() -> ApplyVerificationRequestShapeItfTrace {
    serde_json::from_str(COVEY_APPLY_VERIFICATION_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn landing_authorization_request_shape_trace() -> LandingAuthorizationRequestShapeItfTrace {
    serde_json::from_str(COVEY_LANDING_AUTHORIZATION_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn landing_receipt_request_shape_trace() -> LandingReceiptRequestShapeItfTrace {
    serde_json::from_str(COVEY_LANDING_RECEIPT_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn reservation_request_shape_trace() -> ReservationRequestShapeItfTrace {
    serde_json::from_str(COVEY_RESERVATION_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn reservation_lifecycle_request_shape_trace() -> ReservationLifecycleRequestShapeItfTrace {
    serde_json::from_str(COVEY_RESERVATION_LIFECYCLE_REQUEST_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn conflict_resolution_request_trace() -> ConflictResolutionRequestItfTrace {
    serde_json::from_str(COVEY_CONFLICT_RESOLUTION_REQUEST_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn landing_receipt_trace() -> LandingReceiptItfTrace {
    serde_json::from_str(COVEY_LANDING_RECEIPT_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn openspec_import_trace() -> OpenSpecImportItfTrace {
    serde_json::from_str(COVEY_OPENSPEC_IMPORT_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn bd_import_trace() -> BdImportItfTrace {
    serde_json::from_str(COVEY_BD_IMPORT_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn bd_import_item_outcome_shape_trace() -> BdImportItemOutcomeShapeItfTrace {
    serde_json::from_str(COVEY_BD_IMPORT_ITEM_OUTCOME_SHAPE_ITF)
        .expect("fixture must be valid ITF JSON")
}

#[fixture]
fn claim_dependency_gate_trace() -> ClaimDependencyGateItfTrace {
    serde_json::from_str(COVEY_CLAIM_DEPENDENCY_GATE_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn mutation_idempotency_trace() -> MutationIdempotencyItfTrace {
    serde_json::from_str(COVEY_MUTATION_IDEMPOTENCY_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn event_log_trace() -> EventLogItfTrace {
    serde_json::from_str(COVEY_EVENT_LOG_ITF).expect("fixture must be valid ITF JSON")
}

#[rstest]
fn covey_replays_quint_review_followup_itf_trace(review_followup_trace: ItfTrace) {
    assert!(
        !review_followup_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    assert_eq!(
        replay_review_followup_trace(&review_followup_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_review_claim_reclaim_itf_trace(
    review_claim_reclaim_trace: ReviewClaimReclaimItfTrace,
) {
    assert!(
        !review_claim_reclaim_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    assert!(
        review_claim_reclaim_trace
            .states
            .iter()
            .any(|state| state.s.verdict == "Blocked"
                && state.s.followup_count == 1
                && state.s.followup_work_available),
        "fixture should cover blocked follow-up availability"
    );
    assert!(
        review_claim_reclaim_trace
            .states
            .iter()
            .any(|state| state.s.duplicate_decision_rejected),
        "fixture should cover duplicate decision rejection"
    );
    assert!(
        review_claim_reclaim_trace
            .states
            .iter()
            .any(|state| state.s.executor_claimed_followup && !state.s.followup_work_available),
        "fixture should cover executor follow-up claim consuming availability"
    );
    assert_eq!(
        replay_review_claim_reclaim_trace(&review_claim_reclaim_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_review_claim_reclaim_changes_requested_itf_trace(
    review_claim_reclaim_changes_requested_trace: ReviewClaimReclaimItfTrace,
) {
    assert!(
        !review_claim_reclaim_changes_requested_trace
            .states
            .is_empty(),
        "fixture should contain at least one state"
    );
    assert!(
        review_claim_reclaim_changes_requested_trace
            .states
            .iter()
            .any(|state| state.s.verdict == "ChangesRequested"
                && state.s.followup_count == 1
                && state.s.followup_work_available),
        "fixture should cover changes-requested follow-up availability"
    );
    assert!(
        review_claim_reclaim_changes_requested_trace
            .states
            .iter()
            .any(|state| state.s.executor_claimed_followup && !state.s.followup_work_available),
        "fixture should cover executor follow-up claim consuming changes-requested availability"
    );
    assert_eq!(
        replay_review_claim_reclaim_trace(&review_claim_reclaim_changes_requested_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_core_lifecycle_itf_trace(core_lifecycle_trace: CoreItfTrace) {
    assert!(
        !core_lifecycle_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    assert_eq!(
        replay_core_lifecycle_trace(&core_lifecycle_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_stale_claim_recovery_reap_itf_trace(
    stale_claim_recovery_reap_trace: StaleClaimRecoveryItfTrace,
) {
    assert!(
        stale_claim_recovery_reap_trace
            .states
            .iter()
            .any(|state| state.s.stale_reaped),
        "fixture should cover stale-session reap recovery"
    );
    assert!(
        stale_claim_recovery_reap_trace
            .states
            .iter()
            .any(|state| state.s.owner == "NewSession"
                && state.s.current_fence > state.s.expired_fence),
        "fixture should cover resumed claim with advanced fence"
    );
    assert_eq!(
        replay_stale_claim_recovery_trace(&stale_claim_recovery_reap_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_stale_claim_recovery_lease_itf_trace(
    stale_claim_recovery_lease_trace: StaleClaimRecoveryItfTrace,
) {
    assert!(
        stale_claim_recovery_lease_trace
            .states
            .iter()
            .any(|state| state.s.lease_expired),
        "fixture should cover lease-expiry recovery"
    );
    assert_eq!(
        replay_stale_claim_recovery_trace(&stale_claim_recovery_lease_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_stale_claim_recovery_exit_itf_trace(
    stale_claim_recovery_exit_trace: StaleClaimRecoveryItfTrace,
) {
    assert!(
        stale_claim_recovery_exit_trace
            .states
            .iter()
            .any(|state| state.s.exited_with_held_claim),
        "fixture should cover explicit session-exit recovery"
    );
    assert!(
        stale_claim_recovery_exit_trace
            .states
            .iter()
            .any(|state| state.s.stale_mutation_rejected),
        "fixture should cover stale owner mutation rejection"
    );
    assert_eq!(
        replay_stale_claim_recovery_trace(&stale_claim_recovery_exit_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_queue_reservation_itf_trace(
    queue_reservation_trace: QueueReservationItfTrace,
) {
    assert!(
        !queue_reservation_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    assert_eq!(
        replay_queue_reservation_trace(&queue_reservation_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_ready_queue_claim_selection_itf_trace(
    ready_queue_claim_selection_trace: ReadyQueueClaimSelectionItfTrace,
) {
    assert!(
        !ready_queue_claim_selection_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        ReadyQueueClaimSelectionCase::InvalidHeadSupersededTailClaimed,
        ReadyQueueClaimSelectionCase::MetaUnavailableHeadCancelledTailClaimed,
        ReadyQueueClaimSelectionCase::ExpiredHeadRequeuedThenClaimed,
        ReadyQueueClaimSelectionCase::ActiveInFlightHeadIgnoredTailClaimed,
        ReadyQueueClaimSelectionCase::WrongRoleRejected,
    ] {
        assert!(
            ready_queue_claim_selection_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected:?}"
        );
    }
    assert_eq!(
        replay_ready_queue_claim_selection_trace(&ready_queue_claim_selection_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_ready_queue_metrics_itf_trace(
    ready_queue_metrics_trace: ReadyQueueMetricsItfTrace,
) {
    assert!(
        !ready_queue_metrics_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        ReadyQueueMetricsRejectReason::EmptyQueuedHasAge,
        ReadyQueueMetricsRejectReason::NonEmptyQueuedMissingAge,
        ReadyQueueMetricsRejectReason::NegativeQueuedAge,
        ReadyQueueMetricsRejectReason::EmptyInFlightHasAge,
        ReadyQueueMetricsRejectReason::NonEmptyInFlightMissingAge,
        ReadyQueueMetricsRejectReason::NegativeInFlightAge,
    ] {
        assert!(
            ready_queue_metrics_trace
                .states
                .iter()
                .any(|state| state.s.reject_reason == expected),
            "fixture should cover {expected:?}"
        );
    }
    assert!(
        ready_queue_metrics_trace
            .states
            .iter()
            .any(|state| state.s.case == ReadyQueueMetricsCase::BothNonEmpty
                && state.s.outcome == ReadyQueueMetricsOutcome::Accepted),
        "fixture should cover both ready-queue metric buckets populated"
    );
    assert_eq!(
        replay_ready_queue_metrics_trace(&ready_queue_metrics_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_reservation_overlap_itf_trace(
    reservation_overlap_trace: ReservationOverlapItfTrace,
) {
    assert!(
        reservation_overlap_trace
            .states
            .iter()
            .any(|state| state.s.case == "GeneratedMatchesGeneratedMember"),
        "fixture should cover generated-set member intersections"
    );
    assert!(
        reservation_overlap_trace
            .states
            .iter()
            .any(|state| state.s.reject_reason == "EmptyGeneratedMembers"),
        "fixture should cover generated-set requests without members"
    );
    assert_eq!(
        replay_reservation_overlap_trace(&reservation_overlap_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_repoops_snapshot_itf_trace(repoops_snapshot_trace: RepoopsSnapshotItfTrace) {
    assert!(
        !repoops_snapshot_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "CurrentClaimInvalid",
        "PathInvalid",
        "ClaimFactInvalid",
        "ScopeFactInvalid",
        "OwnershipTokenInvalid",
    ] {
        assert!(
            repoops_snapshot_trace
                .states
                .iter()
                .any(|state| state.s.reject_reason == expected),
            "fixture should cover {expected:?}"
        );
    }
    assert!(
        repoops_snapshot_trace
            .states
            .iter()
            .any(|state| state.s.case == "ValidInProgressOwnedLock" && state.s.accepted),
        "fixture should cover accepted in-progress owned lock snapshot"
    );
    assert!(
        repoops_snapshot_trace
            .states
            .iter()
            .any(|state| state.s.case == "ForeignReservationLock"
                && state.s.lock_kind == "ForeignLock"
                && state.s.accepted),
        "fixture should cover accepted foreign reservation lock fact"
    );
    assert_eq!(
        replay_repoops_snapshot_trace(&repoops_snapshot_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_repoops_snapshot_request_shape_itf_trace(
    repoops_snapshot_request_shape_trace: RepoopsSnapshotRequestShapeItfTrace,
) {
    assert!(
        !repoops_snapshot_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidSinglePath",
        "ValidNormalizedPath",
        "ValidTwoPaths",
        "InvalidSessionToken",
        "InvalidClaimId",
        "NonPositiveFence",
        "EmptyPathSet",
        "EmptyPath",
        "EscapingPath",
        "ControlCharacterPath",
        "DuplicateAfterNormalization",
    ] {
        assert!(
            repoops_snapshot_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        repoops_snapshot_request_shape_trace
            .states
            .iter()
            .any(|state| state.s.case == "ValidNormalizedPath"
                && state.s.serialized_paths_normalized),
        "fixture should cover normalized path serialization"
    );
    assert_eq!(
        replay_repoops_snapshot_request_shape_trace(&repoops_snapshot_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_transition_matrix_itf_trace(
    transition_matrix_trace: TransitionMatrixItfTrace,
) {
    assert!(
        !transition_matrix_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "WorkReadyForApplyApplied",
        "IllegalWorkApprovedApplied",
        "IllegalWorkChangesRequestedClaimed",
        "IllegalReviewAvailableInProgress",
        "IllegalQueueAppliedQueued",
        "IllegalConflictResolvedAcknowledged",
        "IllegalReservationReleasedExpired",
    ] {
        assert!(
            transition_matrix_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected:?}"
        );
    }
    assert_eq!(
        replay_transition_matrix_trace(&transition_matrix_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_subtask_domain_lifecycle_shape_itf_trace(
    subtask_domain_lifecycle_shape_trace: SubtaskDomainLifecycleShapeItfTrace,
) {
    assert!(
        !subtask_domain_lifecycle_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "WorkDecided",
        "ReviewBlocked",
        "ReviewArtifactPublished",
        "ReviewReviewPending",
        "ReviewChangesRequested",
        "ReviewApproved",
        "ReviewReadyForApply",
        "ReviewApplied",
    ] {
        assert!(
            subtask_domain_lifecycle_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected && !state.s.accepted),
            "fixture should cover rejected {expected}"
        );
    }
    for expected in [
        "WorkAvailable",
        "WorkBlocked",
        "WorkArtifactPublished",
        "WorkReviewPending",
        "WorkChangesRequested",
        "WorkApproved",
        "WorkReadyForApply",
        "WorkApplied",
        "WorkAbandoned",
        "ReviewAvailable",
        "ReviewClaimed",
        "ReviewInProgress",
        "ReviewDecided",
        "ReviewAbandoned",
    ] {
        assert!(
            subtask_domain_lifecycle_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected && state.s.accepted),
            "fixture should cover accepted {expected}"
        );
    }
    assert_eq!(
        replay_subtask_domain_lifecycle_shape_trace(&subtask_domain_lifecycle_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_apply_proof_row_lifecycle_itf_trace(
    apply_proof_row_lifecycle_trace: ApplyProofRowLifecycleItfTrace,
) {
    assert!(
        !apply_proof_row_lifecycle_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ReviewDecidedMissingVerdict",
        "ReviewDecidedMissingFindings",
        "ReviewRequestedWithVerdict",
        "QueueInFlightMissingSession",
        "QueueInFlightMissingFence",
        "QueueInFlightMissingLease",
        "QueueAppliedMissingFence",
        "QueueAppliedActiveSession",
        "QueueQueuedInvalidLastFence",
    ] {
        assert!(
            apply_proof_row_lifecycle_trace
                .states
                .iter()
                .any(|state| state.s.case == expected && !state.s.accepted),
            "fixture should cover rejected {expected}"
        );
    }
    for expected in [
        "ReviewRequestedNoDecision",
        "ReviewInProgressNoDecision",
        "ReviewSupersededNoDecision",
        "ReviewDecidedApproveFindings",
        "QueueQueuedNoClaim",
        "QueueInFlightComplete",
        "QueueAppliedFence",
        "QueueSupersededLastFence",
        "QueueCancelledLastFence",
    ] {
        assert!(
            apply_proof_row_lifecycle_trace
                .states
                .iter()
                .any(|state| state.s.case == expected && state.s.accepted),
            "fixture should cover accepted {expected}"
        );
    }
    assert_eq!(
        replay_apply_proof_row_lifecycle_trace(&apply_proof_row_lifecycle_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_record_lifecycle_shape_itf_trace(
    record_lifecycle_shape_trace: RecordLifecycleShapeItfTrace,
) {
    assert!(
        !record_lifecycle_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "SessionActiveIdle",
        "SessionActiveWithSubtask",
        "SessionStaleIdle",
        "SessionExitedIdle",
        "MetaPlanning",
        "MetaActive",
        "MetaCompleted",
        "MetaCancelled",
        "ClaimHeld",
        "ClaimReleased",
        "ClaimExpired",
        "ClaimRevoked",
    ] {
        assert!(
            record_lifecycle_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected && state.s.accepted),
            "fixture should cover accepted {expected}"
        );
    }
    for expected in [
        "SessionStaleWithSubtask",
        "SessionExitedWithSubtask",
        "SessionHeartbeatBeforeCreated",
        "SessionUpdatedBeforeCreated",
        "SessionNegativeHeartbeatTick",
        "SessionInvalidPrincipal",
        "SessionInvalidInstance",
        "MetaUpdatedBeforeCreated",
        "MetaBlankPrompt",
        "ClaimUpdatedBeforeCreated",
    ] {
        assert!(
            record_lifecycle_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected && !state.s.accepted),
            "fixture should cover rejected {expected}"
        );
    }
    assert_eq!(
        replay_record_lifecycle_shape_trace(&record_lifecycle_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_runtime_attestation_record_shape_itf_trace(
    runtime_attestation_record_shape_trace: RuntimeAttestationRecordShapeItfTrace,
) {
    assert!(
        !runtime_attestation_record_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidProcessObservedProviderRun",
        "ValidContainerObservedProviderRun",
        "ValidProcessAndContainerObservedProviderRun",
        "ValidLegacyMissingProviderRun",
    ] {
        assert!(
            runtime_attestation_record_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected && state.s.accepted),
            "fixture should cover accepted {expected}"
        );
    }
    for expected in [
        "MissingRuntimeIdentity",
        "PaddedProcessId",
        "PaddedContainerId",
        "PartialLegacyMissingProviderRunId",
        "PartialLegacyMissingProviderRunIssuer",
        "PaddedProviderRunId",
        "PaddedProviderRunIssuer",
        "EndedBeforeStarted",
        "RecordedBeforeEnded",
        "InvalidAgentPrincipal",
        "InvalidAgentInstance",
    ] {
        assert!(
            runtime_attestation_record_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected && !state.s.accepted),
            "fixture should cover rejected {expected}"
        );
    }
    assert_eq!(
        replay_runtime_attestation_record_shape_trace(&runtime_attestation_record_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_apply_gate_evidence_itf_trace(
    apply_gate_evidence_trace: ApplyGateEvidenceItfTrace,
) {
    assert!(
        !apply_gate_evidence_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "LiveReviewInvalid",
        "PrincipalSeparationInvalid",
        "RuntimeAttestationInvalid",
        "RuntimeSeparationInvalid",
        "ApplyVerificationInvalid",
        "MarkAppliedInvalid",
        "LandingAuthorizationInvalid",
        "LandingReceiptInvalid",
    ] {
        assert!(
            apply_gate_evidence_trace
                .states
                .iter()
                .any(|state| state.s.reject_reason == expected),
            "fixture should cover {expected:?}"
        );
    }
    assert!(
        apply_gate_evidence_trace
            .states
            .iter()
            .any(|state| state.s.case == "ValidApplyGateEvidence" && state.s.accepted),
        "fixture should cover accepted apply-gate evidence"
    );
    assert!(
        apply_gate_evidence_trace
            .states
            .iter()
            .any(|state| state.s.case == "ProducerReviewerSamePrincipal"
                && state.s.reject_reason == "PrincipalSeparationInvalid"),
        "fixture should cover producer/reviewer principal separation"
    );
    assert!(
        apply_gate_evidence_trace
            .states
            .iter()
            .any(|state| state.s.case == "MarkAppliedStaleFence"
                && state.s.reject_reason == "MarkAppliedInvalid"),
        "fixture should cover stale-fence mark-applied rejection"
    );
    assert!(
        apply_gate_evidence_trace
            .states
            .iter()
            .any(|state| state.s.case == "DuplicateReceiptDivergent"
                && state.s.reject_reason == "LandingReceiptInvalid"),
        "fixture should cover divergent duplicate landing receipts"
    );
    assert_eq!(
        replay_apply_gate_evidence_trace(&apply_gate_evidence_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_session_meta_task_itf_trace(
    session_meta_task_trace: SessionMetaTaskItfTrace,
) {
    assert!(
        !session_meta_task_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    assert_eq!(
        replay_session_meta_task_trace(&session_meta_task_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_session_request_shape_itf_trace(
    session_request_shape_trace: SessionRequestShapeItfTrace,
) {
    assert!(
        !session_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidRegister",
        "ValidHeartbeat",
        "ValidExit",
        "RegisterInvalidPrincipal",
        "RegisterInvalidInstance",
        "RegisterBlankIdempotency",
        "HeartbeatInvalidSessionToken",
        "HeartbeatBlankIdempotency",
        "ExitInvalidSessionToken",
        "ExitBlankIdempotency",
    ] {
        assert!(
            session_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    for expected in ["RegisterSession", "Heartbeat", "ExitSession"] {
        assert!(
            session_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.operation == expected && state.s.accepted),
            "fixture should cover accepted {expected}"
        );
    }
    assert_eq!(
        replay_session_request_shape_trace(&session_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_session_handle_shape_itf_trace(
    session_handle_shape_trace: SessionHandleShapeItfTrace,
) {
    assert!(
        !session_handle_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidExecutorHandle",
        "ValidReviewerHandle",
        "ValidOrchestratorHandle",
        "InvalidSessionToken",
        "InvalidAgentPrincipal",
        "InvalidAgentInstance",
    ] {
        assert!(
            session_handle_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    for expected in ["Executor", "Reviewer", "Orchestrator"] {
        assert!(
            session_handle_shape_trace
                .states
                .iter()
                .any(|state| state.s.role == expected && state.s.accepted),
            "fixture should cover accepted {expected} handles"
        );
    }
    assert_eq!(
        replay_session_handle_shape_trace(&session_handle_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_meta_task_request_shape_itf_trace(
    meta_task_request_shape_trace: MetaTaskRequestShapeItfTrace,
) {
    assert!(
        !meta_task_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidSubmit",
        "ValidCancel",
        "SubmitInvalidSessionToken",
        "SubmitBlankPrompt",
        "SubmitBlankIdempotency",
        "CancelInvalidSessionToken",
        "CancelInvalidMetaTaskId",
        "CancelBlankIdempotency",
    ] {
        assert!(
            meta_task_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    for expected in ["SubmitMetaTask", "CancelMetaTask"] {
        assert!(
            meta_task_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.operation == expected && state.s.accepted),
            "fixture should cover accepted {expected}"
        );
    }
    assert_eq!(
        replay_meta_task_request_shape_trace(&meta_task_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_create_subtask_request_shape_itf_trace(
    create_subtask_request_shape_trace: CreateSubtaskRequestShapeItfTrace,
) {
    assert!(
        !create_subtask_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidWithoutSubtaskId",
        "ValidWithSubtaskId",
        "InvalidSessionToken",
        "InvalidMetaTaskId",
        "InvalidSubtaskId",
        "BlankTitle",
        "PaddedTitle",
        "NegativePriority",
        "PriorityTooHigh",
        "BlankIdempotencyKey",
    ] {
        assert!(
            create_subtask_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        create_subtask_request_shape_trace
            .states
            .iter()
            .any(|state| state.s.case == "ValidWithoutSubtaskId" && state.s.accepted),
        "fixture should cover accepted generated subtask ids"
    );
    assert!(
        create_subtask_request_shape_trace
            .states
            .iter()
            .any(|state| state.s.case == "ValidWithSubtaskId" && state.s.accepted),
        "fixture should cover accepted caller-supplied subtask ids"
    );
    assert_eq!(
        replay_create_subtask_request_shape_trace(&create_subtask_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_claim_acquisition_request_shape_itf_trace(
    claim_acquisition_request_shape_trace: ClaimAcquisitionRequestShapeItfTrace,
) {
    assert!(
        !claim_acquisition_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidClaimNext",
        "ValidClaimNextScoped",
        "ValidClaimSubtask",
        "ClaimNextInvalidSessionToken",
        "ClaimNextZeroLease",
        "ClaimNextNegativeLease",
        "ClaimNextBlankIdempotency",
        "ClaimNextScopedInvalidMetaTaskId",
        "ClaimSubtaskInvalidSessionToken",
        "ClaimSubtaskInvalidSubtaskId",
        "ClaimSubtaskZeroLease",
        "ClaimSubtaskNegativeLease",
        "ClaimSubtaskBlankIdempotency",
    ] {
        assert!(
            claim_acquisition_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    for expected in ["ClaimNext", "ClaimNextScoped", "ClaimSubtask"] {
        assert!(
            claim_acquisition_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.operation == expected && state.s.accepted),
            "fixture should cover accepted {expected}"
        );
    }
    assert_eq!(
        replay_claim_acquisition_request_shape_trace(&claim_acquisition_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_supersede_queue_item_request_shape_itf_trace(
    supersede_queue_item_request_shape_trace: SupersedeQueueItemRequestShapeItfTrace,
) {
    assert!(
        !supersede_queue_item_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidSupersede",
        "InvalidSessionToken",
        "InvalidQueueId",
        "BlankIdempotencyKey",
    ] {
        assert!(
            supersede_queue_item_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        supersede_queue_item_request_shape_trace
            .states
            .iter()
            .any(|state| state.s.case == "ValidSupersede" && state.s.accepted),
        "fixture should cover accepted supersede request"
    );
    assert_eq!(
        replay_supersede_queue_item_request_shape_trace(&supersede_queue_item_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_view_attachment_shape_itf_trace(
    view_attachment_shape_trace: ViewAttachmentShapeItfTrace,
) {
    assert!(
        !view_attachment_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidSessionNoActiveDetached",
        "ValidSessionActiveClaimArtifact",
        "SessionMissingActiveView",
        "SessionMismatchedActiveView",
        "SessionUnexpectedActiveView",
        "SubtaskMissingClaim",
        "SubtaskMismatchedClaim",
        "SubtaskForeignClaim",
        "SubtaskUnexpectedClaimCase",
        "SubtaskMissingArtifact",
        "SubtaskMismatchedArtifact",
        "SubtaskForeignArtifact",
        "SubtaskUnexpectedArtifactCase",
        "SubtaskForeignReview",
        "SubtaskForeignReadyQueue",
    ] {
        assert!(
            view_attachment_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        view_attachment_shape_trace
            .states
            .iter()
            .any(|state| state.s.outcome == "Accepted"),
        "fixture should cover accepted view attachments"
    );
    assert!(
        view_attachment_shape_trace
            .states
            .iter()
            .any(|state| state.s.outcome == "Rejected"),
        "fixture should cover rejected view attachments"
    );
    assert_eq!(
        replay_view_attachment_shape_trace(&view_attachment_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_runtime_attestation_request_shape_itf_trace(
    runtime_attestation_request_shape_trace: RuntimeAttestationRequestShapeItfTrace,
) {
    assert!(
        !runtime_attestation_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidProcessOnly",
        "ValidContainerOnly",
        "ValidProcessAndContainer",
        "MissingIdentity",
        "BlankProcess",
        "PaddedContainer",
        "NegativeStartedAt",
        "NegativeEndedAt",
        "InvertedTimestamps",
        "BlankProviderRunId",
        "PaddedProviderRunIdIssuer",
        "InvalidSessionToken",
        "InvalidProviderToken",
        "InvalidModelToken",
        "InvalidTranscriptDigest",
        "BlankIdempotencyKey",
    ] {
        assert!(
            runtime_attestation_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        runtime_attestation_request_shape_trace
            .states
            .iter()
            .any(|state| state.s.case == "ValidProcessAndContainer"
                && state.s.flat_serialization_preserves_identity),
        "fixture should cover flat serialization preserving both runtime identities"
    );
    assert_eq!(
        replay_runtime_attestation_request_shape_trace(&runtime_attestation_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_publish_artifact_request_shape_itf_trace(
    publish_artifact_request_shape_trace: PublishArtifactRequestShapeItfTrace,
) {
    assert!(
        !publish_artifact_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidPatchBundle",
        "ValidTreeBundle",
        "InvalidSessionToken",
        "InvalidClaimId",
        "NonPositiveFence",
        "InvalidArtifactDigest",
        "InvalidBaseRev",
        "EmptyManifestPath",
        "ControlManifestPath",
        "InvalidChangedPathsDigest",
        "BlankIdempotencyKey",
    ] {
        assert!(
            publish_artifact_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        publish_artifact_request_shape_trace
            .states
            .iter()
            .any(|state| state.s.artifact_kind == "TreeBundle" && state.s.accepted),
        "fixture should cover accepted non-patch artifact kinds"
    );
    assert_eq!(
        replay_publish_artifact_request_shape_trace(&publish_artifact_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_decide_review_request_shape_itf_trace(
    decide_review_request_shape_trace: DecideReviewRequestShapeItfTrace,
) {
    assert!(
        !decide_review_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidApprove",
        "ValidChangesRequested",
        "ValidBlocked",
        "InvalidSessionToken",
        "InvalidReviewId",
        "InvalidClaimId",
        "NonPositiveFence",
        "InvalidFindingsDigest",
        "BlankIdempotencyKey",
    ] {
        assert!(
            decide_review_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    for expected in ["Approve", "ChangesRequested", "Blocked"] {
        assert!(
            decide_review_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.verdict == expected && state.s.accepted),
            "fixture should cover accepted {expected} review decisions"
        );
    }
    assert_eq!(
        replay_decide_review_request_shape_trace(&decide_review_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_request_review_request_shape_itf_trace(
    request_review_request_shape_trace: RequestReviewRequestShapeItfTrace,
) {
    assert!(
        !request_review_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidAutoReviewSubtask",
        "ValidExplicitReviewSubtask",
        "InvalidSessionToken",
        "InvalidSubtaskId",
        "InvalidArtifactDigest",
        "InvalidReviewSubtaskId",
        "NegativePriority",
        "TooHighPriority",
        "BlankIdempotencyKey",
    ] {
        assert!(
            request_review_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        request_review_request_shape_trace
            .states
            .iter()
            .any(|state| {
                state.s.case == "ValidAutoReviewSubtask"
                    && state.s.accepted
                    && !state.s.explicit_review_subtask_present
            }),
        "fixture should cover accepted automatic review subtask allocation"
    );
    assert_eq!(
        replay_request_review_request_shape_trace(&request_review_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_enqueue_for_apply_request_shape_itf_trace(
    enqueue_for_apply_request_shape_trace: EnqueueForApplyRequestShapeItfTrace,
) {
    assert!(
        !enqueue_for_apply_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidCanonical",
        "InvalidSessionToken",
        "InvalidArtifactDigest",
        "InvalidSubtaskId",
        "BlankIdempotencyKey",
    ] {
        assert!(
            enqueue_for_apply_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        enqueue_for_apply_request_shape_trace
            .states
            .iter()
            .any(|state| state.s.settlement_target == "Canonical" && state.s.accepted),
        "fixture should cover accepted canonical settlement target"
    );
    assert_eq!(
        replay_enqueue_for_apply_request_shape_trace(&enqueue_for_apply_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_claim_ready_queue_request_shape_itf_trace(
    claim_ready_queue_request_shape_trace: ClaimReadyQueueRequestShapeItfTrace,
) {
    assert!(
        !claim_ready_queue_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidClaim",
        "InvalidSessionToken",
        "ZeroLeaseDuration",
        "NegativeLeaseDuration",
        "BlankIdempotencyKey",
    ] {
        assert!(
            claim_ready_queue_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        claim_ready_queue_request_shape_trace
            .states
            .iter()
            .any(|state| state.s.case == "ValidClaim" && state.s.accepted),
        "fixture should cover accepted ready-queue claim"
    );
    assert_eq!(
        replay_claim_ready_queue_request_shape_trace(&claim_ready_queue_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_claim_lifecycle_request_shape_itf_trace(
    claim_lifecycle_request_shape_trace: ClaimLifecycleRequestShapeItfTrace,
) {
    assert!(
        !claim_lifecycle_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidStart",
        "ValidAbandon",
        "ValidRelease",
        "ValidRenew",
        "InvalidSessionToken",
        "InvalidClaimId",
        "ZeroFence",
        "NegativeFence",
        "ZeroRenewExtension",
        "NegativeRenewExtension",
        "BlankIdempotencyKey",
    ] {
        assert!(
            claim_lifecycle_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    for expected in [
        "StartSubtask",
        "AbandonSubtask",
        "ReleaseClaim",
        "RenewClaim",
    ] {
        assert!(
            claim_lifecycle_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.operation == expected && state.s.accepted),
            "fixture should cover accepted {expected}"
        );
    }
    assert_eq!(
        replay_claim_lifecycle_request_shape_trace(&claim_lifecycle_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_mark_in_flight_request_shape_itf_trace(
    mark_in_flight_request_shape_trace: MarkInFlightRequestShapeItfTrace,
) {
    assert!(
        !mark_in_flight_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidMarkInFlight",
        "InvalidSessionToken",
        "InvalidQueueId",
        "ZeroLeaseDuration",
        "NegativeLeaseDuration",
        "BlankIdempotencyKey",
    ] {
        assert!(
            mark_in_flight_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        mark_in_flight_request_shape_trace
            .states
            .iter()
            .any(|state| state.s.case == "ValidMarkInFlight" && state.s.accepted),
        "fixture should cover accepted mark-in-flight request"
    );
    assert_eq!(
        replay_mark_in_flight_request_shape_trace(&mark_in_flight_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_mark_applied_request_shape_itf_trace(
    mark_applied_request_shape_trace: MarkAppliedRequestShapeItfTrace,
) {
    assert!(
        !mark_applied_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidMarkApplied",
        "InvalidSessionToken",
        "InvalidQueueId",
        "ZeroFence",
        "NegativeFence",
        "BlankIdempotencyKey",
    ] {
        assert!(
            mark_applied_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        mark_applied_request_shape_trace
            .states
            .iter()
            .any(|state| state.s.case == "ValidMarkApplied" && state.s.accepted),
        "fixture should cover accepted mark-applied request"
    );
    assert_eq!(
        replay_mark_applied_request_shape_trace(&mark_applied_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_apply_verification_request_shape_itf_trace(
    apply_verification_request_shape_trace: ApplyVerificationRequestShapeItfTrace,
) {
    assert!(
        !apply_verification_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidVerification",
        "InvalidSessionToken",
        "InvalidQueueId",
        "InvalidArtifactDigest",
        "InvalidReviewId",
        "InvalidFindingsDigest",
        "NonPositiveFence",
        "InvalidVerifier",
        "InvalidVerdictDigest",
        "InvalidSealDigest",
        "BlankIdempotencyKey",
    ] {
        assert!(
            apply_verification_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        apply_verification_request_shape_trace
            .states
            .iter()
            .any(|state| state.s.case == "ValidVerification" && state.s.accepted),
        "fixture should cover accepted apply verification request"
    );
    assert_eq!(
        replay_apply_verification_request_shape_trace(&apply_verification_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_landing_authorization_request_shape_itf_trace(
    landing_authorization_request_shape_trace: LandingAuthorizationRequestShapeItfTrace,
) {
    assert!(
        !landing_authorization_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidAuthorization",
        "InvalidSessionToken",
        "InvalidQueueId",
        "InvalidArtifactDigest",
        "InvalidReviewId",
        "InvalidFindingsDigest",
        "NonPositiveFence",
        "InvalidVerifier",
        "InvalidVerdictDigest",
        "InvalidSealDigest",
    ] {
        assert!(
            landing_authorization_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        landing_authorization_request_shape_trace
            .states
            .iter()
            .any(|state| state.s.case == "ValidAuthorization" && state.s.accepted),
        "fixture should cover accepted landing authorization request"
    );
    assert_eq!(
        replay_landing_authorization_request_shape_trace(
            &landing_authorization_request_shape_trace
        ),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_landing_receipt_request_shape_itf_trace(
    landing_receipt_request_shape_trace: LandingReceiptRequestShapeItfTrace,
) {
    assert!(
        !landing_receipt_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidReceipt",
        "InvalidSessionToken",
        "InvalidQueueId",
        "InvalidArtifactDigest",
        "NonPositiveFence",
        "InvalidTargetRef",
        "InvalidCommitOid",
    ] {
        assert!(
            landing_receipt_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        landing_receipt_request_shape_trace
            .states
            .iter()
            .any(|state| state.s.case == "ValidReceipt" && state.s.accepted),
        "fixture should cover accepted landing receipt request"
    );
    assert_eq!(
        replay_landing_receipt_request_shape_trace(&landing_receipt_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_reservation_request_shape_itf_trace(
    reservation_request_shape_trace: ReservationRequestShapeItfTrace,
) {
    assert!(
        !reservation_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidExactPathRequest",
        "ValidSubtreeQuery",
        "ValidRepoGlobalRequest",
        "ValidGeneratedSetQuery",
        "InvalidSessionToken",
        "InvalidOwnerSubtaskId",
        "NonPositiveLeaseDuration",
        "BlankIdempotencyKey",
        "ExactPathWithGeneratedMembers",
        "SubtreeWithGeneratedMembers",
        "RepoGlobalWrongKey",
        "RepoGlobalWithGeneratedMembers",
        "GeneratedSetWithoutMembers",
        "GeneratedSetBlankMember",
        "GeneratedSetPaddedMember",
        "GeneratedSetDuplicateMembers",
        "BlankScopeKey",
        "PaddedScopeKey",
    ] {
        assert!(
            reservation_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        reservation_request_shape_trace
            .states
            .iter()
            .any(|state| state.s.operation == "ReservationRequest" && state.s.accepted),
        "fixture should cover accepted reservation requests"
    );
    assert!(
        reservation_request_shape_trace
            .states
            .iter()
            .any(|state| state.s.operation == "OverlapQuery" && state.s.accepted),
        "fixture should cover accepted overlap queries"
    );
    assert_eq!(
        replay_reservation_request_shape_trace(&reservation_request_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_reservation_lifecycle_request_shape_itf_trace(
    reservation_lifecycle_request_shape_trace: ReservationLifecycleRequestShapeItfTrace,
) {
    assert!(
        !reservation_lifecycle_request_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidRelease",
        "ValidRenew",
        "InvalidSessionToken",
        "InvalidReservationId",
        "ZeroRenewExtension",
        "NegativeRenewExtension",
        "BlankIdempotencyKey",
    ] {
        assert!(
            reservation_lifecycle_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    for expected in ["ReleaseReservation", "RenewReservation"] {
        assert!(
            reservation_lifecycle_request_shape_trace
                .states
                .iter()
                .any(|state| state.s.operation == expected && state.s.accepted),
            "fixture should cover accepted {expected}"
        );
    }
    assert_eq!(
        replay_reservation_lifecycle_request_shape_trace(
            &reservation_lifecycle_request_shape_trace
        ),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_reservation_record_shape_itf_trace(
    reservation_record_shape_trace: ReservationRecordShapeItfTrace,
) {
    assert!(
        !reservation_record_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidExactPathActive",
        "ValidSubtreeReleased",
        "ValidRepoGlobalExpired",
        "ValidGeneratedSetActive",
    ] {
        assert!(
            reservation_record_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected && state.s.accepted),
            "fixture should cover accepted {expected}"
        );
    }
    for expected in [
        "ExactPathWithGeneratedMembers",
        "SubtreeWithGeneratedMembers",
        "RepoGlobalWrongKey",
        "RepoGlobalWithGeneratedMembers",
        "GeneratedSetWithoutMembers",
        "GeneratedSetBlankMember",
        "GeneratedSetPaddedMember",
        "GeneratedSetDuplicateMembers",
        "BlankScopeKey",
        "PaddedScopeKey",
        "UpdatedBeforeCreated",
    ] {
        assert!(
            reservation_record_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected && !state.s.accepted),
            "fixture should cover rejected {expected}"
        );
    }
    assert_eq!(
        replay_reservation_record_shape_trace(&reservation_record_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_conflict_resolution_request_itf_trace(
    conflict_resolution_request_trace: ConflictResolutionRequestItfTrace,
) {
    assert!(
        !conflict_resolution_request_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ValidAcknowledgeOpen",
        "ValidResolveOpen",
        "ValidAcknowledgeAcknowledged",
        "ValidResolveAcknowledged",
        "ValidResolveResolved",
        "InvalidSessionToken",
        "InvalidConflictId",
        "OpenResolutionRequest",
        "BlankIdempotencyKey",
        "DowngradeResolvedToAcknowledged",
    ] {
        assert!(
            conflict_resolution_request_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        conflict_resolution_request_trace
            .states
            .iter()
            .any(|state| state.s.outcome == "IllegalTransition"),
        "fixture should cover valid request shapes rejected by conflict transition"
    );
    assert_eq!(
        replay_conflict_resolution_request_trace(&conflict_resolution_request_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_landing_receipt_itf_trace(landing_receipt_trace: LandingReceiptItfTrace) {
    assert!(
        !landing_receipt_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    assert!(
        landing_receipt_trace
            .states
            .iter()
            .any(|state| state.s.receipt == "ReceiptRecorded"),
        "fixture should cover a recorded landing receipt"
    );
    assert!(
        landing_receipt_trace
            .states
            .iter()
            .any(|state| state.s.last_attempt == "ReplayedSame"),
        "fixture should cover idempotent same-receipt replay"
    );
    assert!(
        landing_receipt_trace
            .states
            .iter()
            .any(|state| state.s.divergent_attempt_rejected),
        "fixture should cover divergent receipt rejection"
    );
    assert_eq!(
        replay_landing_receipt_trace(&landing_receipt_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_openspec_import_itf_trace(openspec_import_trace: OpenSpecImportItfTrace) {
    assert!(
        !openspec_import_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    assert!(
        openspec_import_trace
            .states
            .iter()
            .any(|state| state.s.mode == "DryRun"),
        "fixture should cover dry-run import"
    );
    assert!(
        openspec_import_trace
            .states
            .iter()
            .any(|state| state.s.conflict_reason == "ActiveClaimChangedSource"),
        "fixture should cover active-claim source-change conflict"
    );
    assert_eq!(
        replay_openspec_import_trace(&openspec_import_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_bd_import_itf_trace(bd_import_trace: BdImportItfTrace) {
    assert!(
        !bd_import_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "InvalidDestination",
        "MetaUnavailable",
        "SourceUnavailable",
        "SourceSchemaInvalid",
        "RoleNotOrchestrator",
        "DuplicateDifferentMetaConflict",
    ] {
        assert!(
            bd_import_trace
                .states
                .iter()
                .any(|state| state.s.conflict_reason == expected),
            "fixture should cover {expected:?}"
        );
    }
    assert!(
        bd_import_trace.states.iter().any(
            |state| state.s.source == "DuplicateSameMeta" && state.s.duplicate_skip_has_subtask
        ),
        "fixture should cover deterministic duplicate skip"
    );
    assert!(
        bd_import_trace
            .states
            .iter()
            .any(|state| state.s.source == "OrderedByPriorityDependencyId"
                && state.s.item_order == "PriorityDependencyId"),
        "fixture should cover deterministic import ordering"
    );
    assert_eq!(
        replay_bd_import_trace(&bd_import_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_bd_import_item_outcome_shape_itf_trace(
    bd_import_item_outcome_shape_trace: BdImportItemOutcomeShapeItfTrace,
) {
    assert!(
        !bd_import_item_outcome_shape_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ImportedWithSubtask",
        "DuplicateWithSubtask",
        "InvalidRowWithoutSubtask",
        "DuplicateMissingSubtask",
        "InvalidRowWithSubtask",
        "MissingSubtaskAndSkipReason",
    ] {
        assert!(
            bd_import_item_outcome_shape_trace
                .states
                .iter()
                .any(|state| state.s.case == expected),
            "fixture should cover {expected}"
        );
    }
    for expected in [
        "Imported",
        "SkippedDuplicate",
        "SkippedInvalidRow",
        "Rejected",
    ] {
        assert!(
            bd_import_item_outcome_shape_trace
                .states
                .iter()
                .any(|state| state.s.outcome == expected),
            "fixture should cover {expected}"
        );
    }
    assert_eq!(
        replay_bd_import_item_outcome_shape_trace(&bd_import_item_outcome_shape_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_claim_dependency_gate_itf_trace(
    claim_dependency_gate_trace: ClaimDependencyGateItfTrace,
) {
    assert!(
        !claim_dependency_gate_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        ClaimDependencyDependency::DepOpen,
        ClaimDependencyDependency::DepApplied,
        ClaimDependencyDependency::DepChangesRequestedNoFollowup,
        ClaimDependencyDependency::DepChangesRequestedFollowupAvailable,
        ClaimDependencyDependency::DepChangesRequestedFollowupApplied,
        ClaimDependencyDependency::DepChangesRequestedFollowupDecided,
    ] {
        assert!(
            claim_dependency_gate_trace
                .states
                .iter()
                .any(|state| state.s.dependency == expected),
            "fixture should cover {expected:?}"
        );
    }
    assert!(
        claim_dependency_gate_trace.states.iter().any(|state| {
            state.s.lineage == ClaimDependencyLineage::ChangesRequestedSourceCandidate
                && state.s.dependency
                    == ClaimDependencyDependency::DepChangesRequestedFollowupAvailable
                && !state.s.claim_created
        }),
        "fixture should cover blocked original changes-requested source candidate"
    );
    assert!(
        claim_dependency_gate_trace.states.iter().any(|state| {
            state.s.lineage == ClaimDependencyLineage::ReviewFollowupCandidate
                && state.s.dependency
                    == ClaimDependencyDependency::DepChangesRequestedFollowupAvailable
                && state.s.claim_path == ClaimDependencyPath::ClaimNext
                && state.s.claim_created
        }),
        "fixture should cover claim-next selecting claimable review follow-up candidate"
    );
    assert!(
        claim_dependency_gate_trace.states.iter().any(|state| {
            state.s.claim_path == ClaimDependencyPath::TargetedClaim
                && state.s.decision == ClaimDependencyDecision::Rejected
                && state.s.reject_reason == ClaimDependencyRejectReason::DependencyUnsatisfied
        }),
        "fixture should cover targeted dependency rejection"
    );
    assert!(
        claim_dependency_gate_trace
            .states
            .iter()
            .any(|state| state.s.role == ClaimDependencyRole::Reviewer
                && state.s.kind == ClaimDependencyKind::Review
                && state.s.claim_created),
        "fixture should cover reviewer review claims"
    );
    assert!(
        claim_dependency_gate_trace
            .states
            .iter()
            .any(|state| state.s.decision == ClaimDependencyDecision::NoClaimableCandidate),
        "fixture should cover claim-next no-candidate result"
    );
    assert_eq!(
        replay_claim_dependency_gate_trace(&claim_dependency_gate_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_mutation_idempotency_itf_trace(
    mutation_idempotency_trace: MutationIdempotencyItfTrace,
) {
    assert!(
        !mutation_idempotency_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "ClosureFailed",
        "SerializeFailed",
        "InvalidKey",
        "Created",
        "DeserializeFailed",
        "Conflict",
        "Replayed",
        "CapacityFull",
    ] {
        assert!(
            mutation_idempotency_trace
                .states
                .iter()
                .any(|state| state.s.last_outcome == expected),
            "fixture should cover {expected:?}"
        );
    }
    assert!(
        mutation_idempotency_trace.states.iter().any(|state| {
            state.s.last_outcome == "Created"
                && state.s.last_actor == "ActorB"
                && state.s.last_key == "Key1"
                && !state.s.identity_matched_before_attempt
        }),
        "fixture should cover actor namespace isolation for reused idempotency keys"
    );
    assert_eq!(
        replay_mutation_idempotency_trace(&mutation_idempotency_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_event_log_itf_trace(event_log_trace: EventLogItfTrace) {
    assert!(
        !event_log_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "Rejected",
        "SerializeFailed",
        "RolledBack",
        "Committed",
        "CapacityFull",
    ] {
        assert!(
            event_log_trace
                .states
                .iter()
                .any(|state| state.s.last_outcome == expected),
            "fixture should cover {expected:?}"
        );
    }
    assert!(
        event_log_trace
            .states
            .iter()
            .any(|state| state.s.last_outcome == "Committed"
                && state.s.last_actor == "SystemActor"
                && !state.s.e1.visible_session_token),
        "fixture should cover system event token normalization"
    );
    assert_eq!(
        replay_event_log_trace(&event_log_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replay_reports_quint_counterexample_shape() {
    let state = ReviewFollowupState {
        b0: ReviewFollowupBlockState::Available,
        b1: ReviewFollowupBlockState::Available,
        b2: ReviewFollowupBlockState::Absent,
        b3: ReviewFollowupBlockState::Absent,
        p0: ReviewFollowupBlockRef::NoBlock,
        p1: ReviewFollowupBlockRef::B0,
        p2: ReviewFollowupBlockRef::NoBlock,
        p3: ReviewFollowupBlockRef::NoBlock,
        active: ReviewFollowupBlockRef::NoBlock,
        next_block: 2,
        idle_observed: true,
        r0: false,
        r1: false,
        r2: false,
        r3: false,
    };
    let trace = ItfTrace {
        states: vec![ItfState { m: state }],
    };

    assert_eq!(
        replay_review_followup_trace(&trace),
        vec!["state[0]: idle observed while work or repair exists"]
    );
}

#[rstest]
fn covey_review_claim_reclaim_replay_reports_counterexample_shape() {
    let state = ReviewClaimReclaimState {
        review: "Decided".to_owned(),
        review_subtask: "Available".to_owned(),
        claim: "Expired".to_owned(),
        owner: "ReviewerA".to_owned(),
        current_fence: 1,
        expired_fence: 1,
        verdict: "ChangesRequested".to_owned(),
        artifact_current: false,
        followup_available: false,
        followup_count: 0,
        followup_review_bound: false,
        followup_source_subtask_bound: false,
        followup_source_artifact_bound: false,
        followup_findings_bound: false,
        followup_created_by_reviewer: false,
        followup_work_available: false,
        executor_claimed_followup: false,
        stale_decision_rejected: true,
        duplicate_decision_rejected: false,
    };
    let trace = ReviewClaimReclaimItfTrace {
        states: vec![ReviewClaimReclaimItfState { s: state }],
    };

    assert_eq!(
        replay_review_claim_reclaim_trace(&trace),
        vec![
            "state[0]: non-held review claim retained owner",
            "state[0]: decided review retained live claim state",
            "state[0]: expired review claim decided review",
            "state[0]: stale decision rejection mutated review state",
            "state[0]: non-approval review decision lacks follow-up",
            "state[0]: changes-requested follow-up is not executor claimable",
            "state[0]: stale artifact review was decided",
        ]
    );
}

#[rstest]
fn covey_review_claim_reclaim_replay_reports_followup_binding_counterexample() {
    let state = ReviewClaimReclaimState {
        review: "Decided".to_owned(),
        review_subtask: "SubtaskDecided".to_owned(),
        claim: "Released".to_owned(),
        owner: "NoReviewer".to_owned(),
        current_fence: 1,
        expired_fence: 0,
        verdict: "ChangesRequested".to_owned(),
        artifact_current: true,
        followup_available: true,
        followup_count: 1,
        followup_review_bound: false,
        followup_source_subtask_bound: true,
        followup_source_artifact_bound: false,
        followup_findings_bound: true,
        followup_created_by_reviewer: false,
        followup_work_available: true,
        executor_claimed_followup: false,
        stale_decision_rejected: false,
        duplicate_decision_rejected: false,
    };
    let trace = ReviewClaimReclaimItfTrace {
        states: vec![ReviewClaimReclaimItfState { s: state }],
    };

    assert_eq!(
        replay_review_claim_reclaim_trace(&trace),
        vec!["state[0]: follow-up record lacks review/source/artifact/findings/reviewer binding"]
    );
}

#[rstest]
fn covey_ready_queue_claim_selection_replay_reports_counterexample_shape() {
    let state = ReadyQueueClaimSelectionState {
        case_index: 1,
        case: ReadyQueueClaimSelectionCase::InvalidHeadSupersededTailClaimed,
        role_apply_gate: false,
        head_state: ReadyQueueClaimSelectionQueueState::InFlight,
        head_expired: false,
        head_subtask_ready: false,
        head_artifact_matches: false,
        head_meta_schedulable: false,
        tail_present: false,
        tail_claimable: false,
        head_action: ReadyQueueClaimSelectionHeadAction::ClaimedHead,
        result: ReadyQueueClaimSelectionResult::ClaimHead,
        selected_head: true,
        selected_tail: true,
        head_fence_before: 0,
        head_fence_after: 0,
        tail_fence_before: 0,
        tail_fence_after: 2,
        event_emitted: false,
        evaluated: true,
    };
    let trace = ReadyQueueClaimSelectionItfTrace {
        states: vec![ReadyQueueClaimSelectionItfState { s: state }],
    };

    assert_eq!(
        replay_ready_queue_claim_selection_trace(&trace),
        vec![
            "state[0]: ready-queue claim was granted to non-apply-gate role",
            "state[0]: head queue claim did not require ready artifact and schedulable meta-task",
            "state[0]: invalid head was not superseded before claiming tail",
            "state[0]: ready-queue in-flight event emission disagrees with claim result",
            "state[0]: ready-queue fence advancement did not match selected queue",
            "state[0]: ready-queue selected both head and tail",
        ]
    );
}

#[rstest]
fn covey_ready_queue_metrics_replay_reports_counterexample_shape() {
    let state = ReadyQueueMetricsState {
        case_index: 1,
        case: ReadyQueueMetricsCase::BothNonEmpty,
        queued_count: 0,
        in_flight_count: 1,
        queued_age_present: true,
        in_flight_age_present: false,
        queued_age_non_negative: false,
        in_flight_age_non_negative: true,
        queued_shape: ReadyQueueMetricsBucketShape::NonEmptyBucket,
        in_flight_shape: ReadyQueueMetricsBucketShape::EmptyBucket,
        outcome: ReadyQueueMetricsOutcome::Accepted,
        reject_reason: ReadyQueueMetricsRejectReason::NoReject,
        evaluated: true,
    };
    let trace = ReadyQueueMetricsItfTrace {
        states: vec![ReadyQueueMetricsItfState { s: state }],
    };

    assert_eq!(
        replay_ready_queue_metrics_trace(&trace),
        vec![
            "state[0]: ready-queue metrics reject reason does not match bucket shape",
            "state[0]: ready-queue metric bucket projection disagrees with count and age",
            "state[0]: ready-queue metrics acceptance disagrees with bucket validity",
            "state[0]: empty ready-queue metric bucket retained oldest age",
            "state[0]: non-empty ready-queue metric bucket lacked oldest age",
            "state[0]: accepted ready-queue metrics had negative age",
        ]
    );
}

#[rstest]
fn covey_core_lifecycle_replay_reports_counterexample_shape() {
    let state = CoreLifecycleState {
        subtask: "Applied".to_owned(),
        claim: "Held".to_owned(),
        session: "ExitedSession".to_owned(),
        review: "Requested".to_owned(),
        queue: "QueueApplied".to_owned(),
        fence: "F1".to_owned(),
        active_subtask: true,
        artifact_present: false,
        review_approved: false,
        apply_verified: false,
        terminal: false,
    };
    let trace = CoreItfTrace {
        states: vec![CoreItfState { s: state }],
    };

    assert_eq!(
        replay_core_lifecycle_trace(&trace),
        vec![
            "state[0]: held claim is not bound to active session/subtask",
            "state[0]: terminal subtask has held claim",
            "state[0]: review exists without artifact",
            "state[0]: ready queue exists without approved review",
            "state[0]: applied queue lacks apply verification",
            "state[0]: terminal marker disagrees with subtask state",
        ]
    );
}

#[rstest]
fn covey_stale_claim_recovery_replay_reports_counterexample_shape() {
    let state = StaleClaimRecoveryState {
        old_session: "StaleSession".to_owned(),
        new_session: "ActiveSession".to_owned(),
        claim: "Held".to_owned(),
        owner: "OldSession".to_owned(),
        subtask: "InProgress".to_owned(),
        old_active_subtask: true,
        new_active_subtask: true,
        current_fence: 1,
        expired_fence: 1,
        stale_reaped: true,
        lease_expired: false,
        exited_with_held_claim: false,
        stale_mutation_rejected: true,
    };
    let trace = StaleClaimRecoveryItfTrace {
        states: vec![StaleClaimRecoveryItfState { s: state }],
    };

    assert_eq!(
        replay_stale_claim_recovery_trace(&trace),
        vec![
            "state[0]: held stale-recovery claim lacks exactly one active owner",
            "state[0]: inactive old session retained active subtask",
            "state[0]: recovered old claim did not detach session and subtask",
            "state[0]: stale or exited session still owns claim",
            "state[0]: recovered subtask is neither claimable nor reclaimed",
            "state[0]: stale recovery has dual active claims",
            "state[0]: stale mutation reattached old claim",
        ]
    );
}

#[rstest]
fn covey_queue_reservation_replay_reports_counterexample_shape() {
    let state = QueueReservationState {
        queue: "Applied".to_owned(),
        reservation: "Released".to_owned(),
        conflict: "Acknowledged".to_owned(),
        queue_fence: "F0".to_owned(),
        queue_claim_live: true,
        queue_lease_live: true,
        apply_verified: false,
        subtask_ready: false,
        artifact_matches: false,
        meta_schedulable: false,
        reservation_lease_live: true,
        overlap_detected: false,
        conflict_bound: false,
        conflict_rank_floor: 2,
    };
    let trace = QueueReservationItfTrace {
        states: vec![QueueReservationItfState { s: state }],
    };

    assert_eq!(
        replay_queue_reservation_trace(&trace),
        vec![
            "state[0]: queue claim liveness disagrees with queue state",
            "state[0]: terminal queue has live claim",
            "state[0]: applied queue lacks apply verification",
            "state[0]: terminal reservation has live lease",
            "state[0]: unresolved conflict is not bound to active overlap",
            "state[0]: conflict exists without recorded overlap binding",
            "state[0]: conflict resolution moved below recorded floor",
            "state[0]: resolved conflict floor was downgraded",
        ]
    );
}

#[rstest]
fn covey_queue_reservation_replay_reports_stale_queued_evidence_counterexample() {
    let state = QueueReservationState {
        queue: "Queued".to_owned(),
        reservation: "NoReservation".to_owned(),
        conflict: "NoConflict".to_owned(),
        queue_fence: "F1".to_owned(),
        queue_claim_live: true,
        queue_lease_live: true,
        apply_verified: true,
        subtask_ready: true,
        artifact_matches: true,
        meta_schedulable: true,
        reservation_lease_live: false,
        overlap_detected: false,
        conflict_bound: false,
        conflict_rank_floor: 0,
    };
    let trace = QueueReservationItfTrace {
        states: vec![QueueReservationItfState { s: state }],
    };

    assert_eq!(
        replay_queue_reservation_trace(&trace),
        vec![
            "state[0]: queue claim liveness disagrees with queue state",
            "state[0]: queued item retained stale claim or verification",
        ]
    );
}

#[rstest]
fn covey_repoops_snapshot_replay_reports_token_leak_counterexample_shape() {
    let state = RepoopsSnapshotState {
        case_index: 1,
        case: "ValidInProgressOwnedLock".to_owned(),
        current_claim_valid: true,
        requested_path_valid: true,
        subtask: "InProgress".to_owned(),
        owner_reservation_active: true,
        foreign_reservation_active: false,
        reservation_covers_requested_path: true,
        owner_reservation_in_scope: true,
        claim_fact_status: "ClaimInProgress".to_owned(),
        active_ownership_token: "RawToken".to_owned(),
        caller_ownership_token: "RawToken".to_owned(),
        scope_includes_owner_reservation: true,
        lock_kind: "OwnedLock".to_owned(),
        lock_owner_matches_session: true,
        lock_claim_ref_matches_claim: true,
        fact_sources_use_token_refs: false,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
    };
    let trace = RepoopsSnapshotItfTrace {
        states: vec![RepoopsSnapshotItfState { s: state }],
    };

    assert_eq!(
        replay_repoops_snapshot_trace(&trace),
        vec![
            "state[0]: repoops snapshot reject reason does not match first failed gate",
            "state[0]: repoops snapshot accepted with failed gate",
            "state[0]: in-progress repoops claim lacks token reference",
            "state[0]: repoops snapshot exposed raw session token",
        ]
    );
}

#[rstest]
fn covey_repoops_snapshot_request_shape_replay_reports_counterexample_shape() {
    let state = RepoopsSnapshotRequestShapeState {
        case_index: 11,
        case: "DuplicateAfterNormalization".to_owned(),
        session_token_valid: true,
        claim_id_valid: true,
        fence_seq_positive: true,
        path_set_non_empty: true,
        every_path_valid: true,
        duplicate_after_normalization: true,
        normalizes_path_syntax: true,
        serialized_paths_normalized: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = RepoopsSnapshotRequestShapeItfTrace {
        states: vec![RepoopsSnapshotRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_repoops_snapshot_request_shape_trace(&trace),
        vec![
            "state[0]: repoops snapshot request reject reason does not match validation facts",
            "state[0]: repoops snapshot request outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_transition_matrix_replay_reports_counterexample_shape() {
    let state = TransitionMatrixState {
        case_index: 1,
        case: "IllegalWorkApprovedApplied".to_owned(),
        object: "WorkSubtask".to_owned(),
        from_state: "Approved".to_owned(),
        to_state: "Applied".to_owned(),
        allowed_by_matrix: true,
        outcome: "Allowed".to_owned(),
        evaluated: true,
    };
    let trace = TransitionMatrixItfTrace {
        states: vec![TransitionMatrixItfState { s: state }],
    };

    assert_eq!(
        replay_transition_matrix_trace(&trace),
        vec![
            "state[0]: transition matrix allowed flag disagrees with expected edge set",
            "state[0]: transition matrix outcome disagrees with expected edge set",
            "state[0]: work subtask bypassed ready_for_apply before applied",
        ]
    );
}

#[rstest]
fn covey_subtask_domain_lifecycle_shape_replay_reports_counterexample_shape() {
    let state = SubtaskDomainLifecycleShapeState {
        case_index: 15,
        case: "ReviewBlocked".to_owned(),
        kind: "Review".to_owned(),
        lifecycle: "Blocked".to_owned(),
        carries_artifact: true,
        carries_active_claim: false,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = SubtaskDomainLifecycleShapeItfTrace {
        states: vec![SubtaskDomainLifecycleShapeItfState { s: state }],
    };

    assert_eq!(
        replay_subtask_domain_lifecycle_shape_trace(&trace),
        vec![
            "state[0]: subtask domain lifecycle reject reason disagrees with kind/lifecycle facts",
            "state[0]: subtask domain lifecycle outcome disagrees with kind/lifecycle facts",
            "state[0]: review subtask accepted work-only lifecycle",
        ]
    );
}

#[rstest]
fn covey_apply_proof_row_lifecycle_replay_reports_counterexample_shape() {
    let state = ApplyProofRowLifecycleState {
        case_index: 16,
        case: "QueueInFlightComplete".to_owned(),
        row_kind: "ReadyQueueProofRow".to_owned(),
        review_state: "Decided".to_owned(),
        ready_queue_state: "QueueInFlight".to_owned(),
        verdict_present: false,
        findings_digest_present: false,
        claimed_by_session_present: false,
        claim_fence: "NoFence".to_owned(),
        claim_lease_deadline_present: false,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = ApplyProofRowLifecycleItfTrace {
        states: vec![ApplyProofRowLifecycleItfState { s: state }],
    };

    assert_eq!(
        replay_apply_proof_row_lifecycle_trace(&trace),
        vec![
            "state[0]: apply-proof row lifecycle reject reason disagrees with row facts",
            "state[0]: apply-proof row lifecycle outcome disagrees with row facts",
            "state[0]: in-flight queue row accepted without complete active claim evidence",
        ]
    );
}

#[rstest]
fn covey_record_lifecycle_shape_replay_reports_counterexample_shape() {
    let state = RecordLifecycleShapeState {
        case_index: 5,
        case: "SessionStaleWithSubtask".to_owned(),
        row_kind: "SessionRow".to_owned(),
        session_lifecycle: "Stale".to_owned(),
        meta_task_lifecycle: "Planning".to_owned(),
        claim_lifecycle: "Held".to_owned(),
        active_subtask_present: true,
        timestamp_shape: "Monotonic".to_owned(),
        text_shape: "ValidText".to_owned(),
        tick_shape: "NonNegativeTick".to_owned(),
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = RecordLifecycleShapeItfTrace {
        states: vec![RecordLifecycleShapeItfState { s: state }],
    };

    assert_eq!(
        replay_record_lifecycle_shape_trace(&trace),
        vec![
            "state[0]: record lifecycle reject reason disagrees with row facts",
            "state[0]: record lifecycle outcome disagrees with row facts",
            "state[0]: terminal session accepted with active subtask",
        ]
    );
}

#[rstest]
fn covey_runtime_attestation_record_shape_replay_reports_counterexample_shape() {
    let state = RuntimeAttestationRecordShapeState {
        case_index: 4,
        case: "ValidLegacyMissingProviderRun".to_owned(),
        provider_run_shape: "LegacyMissingProviderRun".to_owned(),
        runtime_identity_shape: "ProcessOnly".to_owned(),
        timestamp_shape: "MonotonicRuntimeSpan".to_owned(),
        agent_shape: "ValidAgent".to_owned(),
        process_present: true,
        container_present: false,
        provider_run_ref_present: true,
        legacy_provider_run_missing: false,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = RuntimeAttestationRecordShapeItfTrace {
        states: vec![RuntimeAttestationRecordShapeItfState { s: state }],
    };

    assert_eq!(
        replay_runtime_attestation_record_shape_trace(&trace),
        vec!["state[0]: provider-run projection disagrees with model"]
    );
}

#[rstest]
fn covey_apply_gate_evidence_replay_reports_counterexample_shape() {
    let state = ApplyGateEvidenceState {
        case_index: 1,
        case: "ValidApplyGateEvidence".to_owned(),
        queue: "InFlight".to_owned(),
        queue_owner_is_apply_gate: false,
        queue_fence_matches: false,
        queue_lease_live: false,
        queue_artifact_matches_request: false,
        subtask_ready_for_apply: false,
        review_exists: false,
        review_decided: false,
        review_approved: false,
        findings_digest_present: false,
        artifact_bound_to_review: false,
        producer_reviewer_principal_separated: false,
        apply_gate_separated_from_producer: false,
        apply_gate_separated_from_reviewer: false,
        producer_attested: false,
        reviewer_attested: false,
        apply_gate_attested: false,
        producer_reviewer_runtime_separated: false,
        producer_apply_gate_runtime_separated: false,
        reviewer_apply_gate_runtime_separated: false,
        producer_reviewer_provider_run_separated: false,
        producer_apply_gate_provider_run_separated: false,
        reviewer_apply_gate_provider_run_separated: false,
        producer_reviewer_transcript_separated: false,
        producer_apply_gate_transcript_separated: false,
        reviewer_apply_gate_transcript_separated: false,
        verification_attempted: true,
        verification_recorded: false,
        verification_review_matches: false,
        mark_applied_attempted: true,
        landing_authorization_accepted: true,
        landing_authorization_artifact_matches: false,
        landing_authorization_fence_matches: false,
        landing_authorization_verification_matches: false,
        receipt_recorded: true,
        receipt_artifact_matches: false,
        receipt_fence_matches: false,
        duplicate_receipt_divergent: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
    };
    let trace = ApplyGateEvidenceItfTrace {
        states: vec![ApplyGateEvidenceItfState { s: state }],
    };

    assert_eq!(
        replay_apply_gate_evidence_trace(&trace),
        vec![
            "state[0]: apply-gate evidence reject reason does not match first failed gate",
            "state[0]: apply-gate evidence accepted with failed gate",
            "state[0]: apply-gate evidence lacks live approved review or separation",
            "state[0]: apply verification was not bound to current in-flight queue fence",
            "state[0]: mark-applied accepted without current verification and queue ownership",
            "state[0]: landing authorization accepted without applied queue binding",
            "state[0]: landing receipt recorded without applied artifact/fence binding",
        ]
    );
}

#[rstest]
fn covey_session_meta_task_replay_reports_counterexample_shape() {
    let state = SessionMetaTaskState {
        session: "Exited".to_owned(),
        meta: "Completed".to_owned(),
        subtasks: "OpenSubtasks".to_owned(),
        claim: "Held".to_owned(),
        queue: "InFlight".to_owned(),
        heartbeat_fresh: true,
    };
    let trace = SessionMetaTaskItfTrace {
        states: vec![SessionMetaTaskItfState { s: state }],
    };

    assert_eq!(
        replay_session_meta_task_trace(&trace),
        vec![
            "state[0]: held claim is not bound to active session occupancy",
            "state[0]: inactive session still owns held claim",
            "state[0]: inactive session still has fresh heartbeat",
            "state[0]: terminal meta-task still has held claim",
            "state[0]: terminal meta-task still has open ready queue",
            "state[0]: completed meta-task lacks terminal subtask summary",
        ]
    );
}

#[rstest]
fn covey_session_request_shape_replay_reports_counterexample_shape() {
    let state = SessionRequestShapeState {
        case_index: 8,
        case: "ExitInvalidSessionToken".to_owned(),
        operation: "ExitSession".to_owned(),
        agent_principal_valid: true,
        agent_instance_valid: true,
        session_token_valid: false,
        idempotency_key_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = SessionRequestShapeItfTrace {
        states: vec![SessionRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_session_request_shape_trace(&trace),
        vec![
            "state[0]: session request reject reason does not match validation facts",
            "state[0]: session request outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_session_handle_shape_replay_reports_counterexample_shape() {
    let state = SessionHandleShapeState {
        case_index: 4,
        case: "InvalidSessionToken".to_owned(),
        role: "Executor".to_owned(),
        session_token_valid: false,
        agent_principal_valid: true,
        agent_instance_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = SessionHandleShapeItfTrace {
        states: vec![SessionHandleShapeItfState { s: state }],
    };

    assert_eq!(
        replay_session_handle_shape_trace(&trace),
        vec![
            "state[0]: session handle reject reason does not match validation facts",
            "state[0]: session handle outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_meta_task_request_shape_replay_reports_counterexample_shape() {
    let state = MetaTaskRequestShapeState {
        case_index: 4,
        case: "SubmitBlankPrompt".to_owned(),
        operation: "SubmitMetaTask".to_owned(),
        session_token_valid: true,
        prompt_text_valid: false,
        meta_task_id_valid: true,
        idempotency_key_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = MetaTaskRequestShapeItfTrace {
        states: vec![MetaTaskRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_meta_task_request_shape_trace(&trace),
        vec![
            "state[0]: meta-task request reject reason does not match validation facts",
            "state[0]: meta-task request outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_create_subtask_request_shape_replay_reports_counterexample_shape() {
    let state = CreateSubtaskRequestShapeState {
        case_index: 7,
        case: "NegativePriority".to_owned(),
        session_token_valid: true,
        meta_task_id_valid: true,
        optional_subtask_id_present: true,
        optional_subtask_id_valid: true,
        title_valid: true,
        priority_valid: false,
        idempotency_key_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = CreateSubtaskRequestShapeItfTrace {
        states: vec![CreateSubtaskRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_create_subtask_request_shape_trace(&trace),
        vec![
            "state[0]: create-subtask request reject reason does not match validation facts",
            "state[0]: create-subtask request outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_landing_receipt_replay_reports_counterexample_shape() {
    let state = LandingReceiptState {
        queue: "Superseded".to_owned(),
        receipt: "ReceiptRecorded".to_owned(),
        last_attempt: "Rejected".to_owned(),
        actor_authorized: false,
        artifact_matches: false,
        fence_matches: false,
        receipt_actor_authorized: false,
        receipt_artifact_matches: false,
        receipt_fence_matches: true,
        receipt_target: "TargetMain".to_owned(),
        receipt_commit: "CommitA".to_owned(),
        attempted_target: "TargetRelease".to_owned(),
        attempted_commit: "CommitB".to_owned(),
        receipt_created_by_last_attempt: true,
        divergent_attempt_rejected: false,
    };
    let trace = LandingReceiptItfTrace {
        states: vec![LandingReceiptItfState { s: state }],
    };

    assert_eq!(
        replay_landing_receipt_trace(&trace),
        vec![
            "state[0]: landing receipt exists before applied queue",
            "state[0]: landing receipt lacks authorized recorder",
            "state[0]: landing receipt lacks artifact/fence match",
            "state[0]: superseded queue recorded landing receipt",
            "state[0]: divergent landing receipt attempt was not rejected",
            "state[0]: rejected landing receipt attempt created receipt",
            "state[0]: non-accepted attempt marked receipt creation",
        ]
    );
}

#[rstest]
fn covey_openspec_import_replay_reports_counterexample_shape() {
    let state = OpenSpecImportState {
        store: "ActiveClaimV1".to_owned(),
        source: "SourceV2".to_owned(),
        mode: "DryRun".to_owned(),
        role: "Executor".to_owned(),
        diff: "UpdateDiff".to_owned(),
        conflict_reason: "NoConflict".to_owned(),
        apply_result: "Applied".to_owned(),
        meta_written: true,
        subtask_written: true,
        provenance_written: true,
        import_event_written: true,
        dependencies_written: true,
        claim_live: true,
        claim_created_by_import: true,
        evaluated: true,
    };
    let trace = OpenSpecImportItfTrace {
        states: vec![OpenSpecImportItfState { s: state }],
    };

    assert_eq!(
        replay_openspec_import_trace(&trace),
        vec![
            "state[0]: OpenSpec diff does not match store/source",
            "state[0]: OpenSpec conflict reason does not match store/source",
            "state[0]: dry-run OpenSpec import wrote state",
            "state[0]: OpenSpec write applied without orchestrator role",
            "state[0]: active claimed OpenSpec task update did not conflict",
            "state[0]: OpenSpec import updated a live-claimed task",
            "state[0]: OpenSpec import created a live claim",
        ]
    );
}

#[rstest]
fn covey_bd_import_replay_reports_counterexample_shape() {
    let state = BdImportState {
        destination: "NoDestination".to_owned(),
        role: "Executor".to_owned(),
        source: "DuplicateDifferentMeta".to_owned(),
        conflict_reason: "NoConflict".to_owned(),
        outcome: "Applied".to_owned(),
        meta_written: true,
        subtask_written: true,
        event_written: true,
        imported_count: 1,
        skipped_count: 1,
        subtask_shape: "InvalidSkipOnly".to_owned(),
        item_order: "SingleItem".to_owned(),
        duplicate_skip_has_subtask: false,
        invalid_skip_has_subtask: true,
        claim_created: true,
        session_active_subtask_set: true,
        evaluated: true,
    };
    let trace = BdImportItfTrace {
        states: vec![BdImportItfState { s: state }],
    };

    assert_eq!(
        replay_bd_import_trace(&trace),
        vec![
            "state[0]: BD import conflict reason does not match inputs",
            "state[0]: invalid BD import destination wrote state",
            "state[0]: non-orchestrator BD import mutated state",
            "state[0]: duplicate-different-meta BD import was not atomic",
            "state[0]: invalid BD import skip item carried subtask id",
            "state[0]: imported BD rows did not become available work subtasks",
            "state[0]: BD import created live claim state",
            "state[0]: BD import counts do not match source eligibility",
        ]
    );
}

#[rstest]
fn covey_claim_acquisition_request_shape_replay_reports_counterexample_shape() {
    let state = ClaimAcquisitionRequestShapeState {
        case_index: 7,
        case: "ClaimNextScopedInvalidMetaTaskId".to_owned(),
        operation: "ClaimNextScoped".to_owned(),
        session_token_valid: true,
        lease_duration_positive: true,
        meta_task_id_present: true,
        meta_task_id_valid: false,
        subtask_id_valid: true,
        idempotency_key_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = ClaimAcquisitionRequestShapeItfTrace {
        states: vec![ClaimAcquisitionRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_claim_acquisition_request_shape_trace(&trace),
        vec![
            "state[0]: claim acquisition request reject reason does not match validation facts",
            "state[0]: claim acquisition request outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_supersede_queue_item_request_shape_replay_reports_counterexample_shape() {
    let state = SupersedeQueueItemRequestShapeState {
        case_index: 3,
        case: "InvalidQueueId".to_owned(),
        session_token_valid: true,
        queue_id_valid: false,
        idempotency_key_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = SupersedeQueueItemRequestShapeItfTrace {
        states: vec![SupersedeQueueItemRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_supersede_queue_item_request_shape_trace(&trace),
        vec![
            "state[0]: supersede queue item request reject reason does not match validation facts",
            "state[0]: supersede queue item request outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_view_attachment_shape_replay_reports_counterexample_shape() {
    let state = ViewAttachmentShapeState {
        case_index: 6,
        case: "SubtaskMissingClaim".to_owned(),
        session_has_active_subtask_id: true,
        active_subtask_view_present: false,
        active_subtask_matches_session: true,
        subtask_has_active_claim_id: true,
        claim_row_present: false,
        claim_id_matches_active: true,
        claim_belongs_to_subtask: true,
        subtask_has_artifact_digest: true,
        artifact_row_present: false,
        artifact_digest_matches: true,
        artifact_belongs_to_subtask: true,
        reviews_belong_to_subtask: false,
        ready_queue_belongs_to_subtask: false,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        evaluated: true,
    };
    let trace = ViewAttachmentShapeItfTrace {
        states: vec![ViewAttachmentShapeItfState { s: state }],
    };

    assert_eq!(
        replay_view_attachment_shape_trace(&trace),
        vec![
            "state[0]: view attachment reject reason does not match first failed binding",
            "state[0]: view attachment outcome disagrees with binding checks",
            "state[0]: accepted session status has invalid active subtask attachment",
            "state[0]: accepted subtask status has invalid claim attachment",
            "state[0]: accepted subtask status has invalid artifact attachment",
            "state[0]: accepted subtask status has foreign collection attachment",
        ]
    );
}

#[rstest]
fn covey_runtime_attestation_request_shape_replay_reports_counterexample_shape() {
    let state = RuntimeAttestationRequestShapeState {
        case_index: 4,
        case: "MissingIdentity".to_owned(),
        session_token_valid: true,
        provider_valid: true,
        model_valid: true,
        provider_run_id_valid: true,
        provider_run_id_issuer_valid: true,
        process_id_present: false,
        process_id_valid: true,
        container_id_present: false,
        container_id_valid: true,
        command_transcript_digest_valid: true,
        started_at_non_negative: true,
        ended_at_non_negative: true,
        timestamps_ordered: true,
        idempotency_key_valid: true,
        flat_serialization_preserves_identity: false,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = RuntimeAttestationRequestShapeItfTrace {
        states: vec![RuntimeAttestationRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_runtime_attestation_request_shape_trace(&trace),
        vec![
            "state[0]: runtime attestation request reject reason does not match parser order",
            "state[0]: runtime attestation request outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_publish_artifact_request_shape_replay_reports_counterexample_shape() {
    let state = PublishArtifactRequestShapeState {
        case_index: 8,
        case: "EmptyManifestPath".to_owned(),
        artifact_kind: "PatchBundle".to_owned(),
        session_token_valid: true,
        claim_id_valid: true,
        fence_seq_positive: true,
        artifact_digest_valid: true,
        base_rev_valid: true,
        manifest_path_present: false,
        manifest_path_no_control: true,
        changed_paths_digest_valid: true,
        idempotency_key_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = PublishArtifactRequestShapeItfTrace {
        states: vec![PublishArtifactRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_publish_artifact_request_shape_trace(&trace),
        vec![
            "state[0]: publish artifact request reject reason does not match validation facts",
            "state[0]: publish artifact request outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_decide_review_request_shape_replay_reports_counterexample_shape() {
    let state = DecideReviewRequestShapeState {
        case_index: 7,
        case: "NonPositiveFence".to_owned(),
        verdict: "Approve".to_owned(),
        session_token_valid: true,
        review_id_valid: true,
        claim_id_valid: true,
        fence_seq_positive: false,
        findings_digest_valid: true,
        idempotency_key_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = DecideReviewRequestShapeItfTrace {
        states: vec![DecideReviewRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_decide_review_request_shape_trace(&trace),
        vec![
            "state[0]: decide review reject reason does not match validation facts",
            "state[0]: decide review outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_request_review_request_shape_replay_reports_counterexample_shape() {
    let state = RequestReviewRequestShapeState {
        case_index: 6,
        case: "InvalidReviewSubtaskId".to_owned(),
        session_token_valid: true,
        subtask_id_valid: true,
        artifact_digest_valid: true,
        explicit_review_subtask_present: true,
        review_subtask_id_valid: false,
        priority_valid: true,
        idempotency_key_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = RequestReviewRequestShapeItfTrace {
        states: vec![RequestReviewRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_request_review_request_shape_trace(&trace),
        vec![
            "state[0]: request review reject reason does not match validation facts",
            "state[0]: request review outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_enqueue_for_apply_request_shape_replay_reports_counterexample_shape() {
    let state = EnqueueForApplyRequestShapeState {
        case_index: 4,
        case: "InvalidSubtaskId".to_owned(),
        settlement_target: "Canonical".to_owned(),
        session_token_valid: true,
        artifact_digest_valid: true,
        subtask_id_valid: false,
        idempotency_key_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = EnqueueForApplyRequestShapeItfTrace {
        states: vec![EnqueueForApplyRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_enqueue_for_apply_request_shape_trace(&trace),
        vec![
            "state[0]: enqueue for apply reject reason does not match validation facts",
            "state[0]: enqueue for apply outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_claim_ready_queue_request_shape_replay_reports_counterexample_shape() {
    let state = ClaimReadyQueueRequestShapeState {
        case_index: 3,
        case: "NegativeLeaseDuration".to_owned(),
        session_token_valid: true,
        lease_duration_positive: false,
        idempotency_key_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = ClaimReadyQueueRequestShapeItfTrace {
        states: vec![ClaimReadyQueueRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_claim_ready_queue_request_shape_trace(&trace),
        vec![
            "state[0]: claim ready queue reject reason does not match validation facts",
            "state[0]: claim ready queue outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_claim_lifecycle_request_shape_replay_reports_counterexample_shape() {
    let state = ClaimLifecycleRequestShapeState {
        case_index: 8,
        case: "ZeroRenewExtension".to_owned(),
        operation: "RenewClaim".to_owned(),
        session_token_valid: true,
        claim_id_valid: true,
        fence_seq_positive: true,
        extend_duration_positive: false,
        idempotency_key_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = ClaimLifecycleRequestShapeItfTrace {
        states: vec![ClaimLifecycleRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_claim_lifecycle_request_shape_trace(&trace),
        vec![
            "state[0]: claim lifecycle reject reason does not match validation facts",
            "state[0]: claim lifecycle outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_mark_in_flight_request_shape_replay_reports_counterexample_shape() {
    let state = MarkInFlightRequestShapeState {
        case_index: 3,
        case: "InvalidQueueId".to_owned(),
        session_token_valid: true,
        queue_id_valid: false,
        lease_duration_positive: true,
        idempotency_key_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = MarkInFlightRequestShapeItfTrace {
        states: vec![MarkInFlightRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_mark_in_flight_request_shape_trace(&trace),
        vec![
            "state[0]: mark in flight reject reason does not match validation facts",
            "state[0]: mark in flight outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_mark_applied_request_shape_replay_reports_counterexample_shape() {
    let state = MarkAppliedRequestShapeState {
        case_index: 4,
        case: "ZeroFence".to_owned(),
        session_token_valid: true,
        queue_id_valid: true,
        fence_seq_positive: false,
        idempotency_key_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = MarkAppliedRequestShapeItfTrace {
        states: vec![MarkAppliedRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_mark_applied_request_shape_trace(&trace),
        vec![
            "state[0]: mark applied reject reason does not match validation facts",
            "state[0]: mark applied outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_apply_verification_request_shape_replay_reports_counterexample_shape() {
    let state = ApplyVerificationRequestShapeState {
        case_index: 7,
        case: "NonPositiveFence".to_owned(),
        session_token_valid: true,
        queue_id_valid: true,
        artifact_digest_valid: true,
        review_id_valid: true,
        findings_digest_valid: true,
        fence_seq_positive: false,
        verifier_valid: true,
        verdict_digest_valid: true,
        seal_digest_valid: true,
        idempotency_key_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = ApplyVerificationRequestShapeItfTrace {
        states: vec![ApplyVerificationRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_apply_verification_request_shape_trace(&trace),
        vec![
            "state[0]: apply verification reject reason does not match validation facts",
            "state[0]: apply verification outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_landing_authorization_request_shape_replay_reports_counterexample_shape() {
    let state = LandingAuthorizationRequestShapeState {
        case_index: 10,
        case: "InvalidSealDigest".to_owned(),
        session_token_valid: true,
        queue_id_valid: true,
        artifact_digest_valid: true,
        review_id_valid: true,
        findings_digest_valid: true,
        fence_seq_positive: true,
        verifier_valid: true,
        verdict_digest_valid: true,
        seal_digest_valid: false,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = LandingAuthorizationRequestShapeItfTrace {
        states: vec![LandingAuthorizationRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_landing_authorization_request_shape_trace(&trace),
        vec![
            "state[0]: landing authorization reject reason does not match validation facts",
            "state[0]: landing authorization outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_landing_receipt_request_shape_replay_reports_counterexample_shape() {
    let state = LandingReceiptRequestShapeState {
        case_index: 7,
        case: "InvalidCommitOid".to_owned(),
        session_token_valid: true,
        queue_id_valid: true,
        artifact_digest_valid: true,
        fence_seq_positive: true,
        target_ref_valid: true,
        landed_commit_oid_valid: false,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = LandingReceiptRequestShapeItfTrace {
        states: vec![LandingReceiptRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_landing_receipt_request_shape_trace(&trace),
        vec![
            "state[0]: landing receipt request reject reason does not match validation facts",
            "state[0]: landing receipt request outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_reservation_request_shape_replay_reports_counterexample_shape() {
    let state = ReservationRequestShapeState {
        case_index: 18,
        case: "PaddedScopeKey".to_owned(),
        operation: "ReservationRequest".to_owned(),
        session_token_valid: true,
        owner_subtask_id_valid: true,
        lease_duration_positive: true,
        idempotency_key_valid: true,
        scope: "ExactPath".to_owned(),
        scope_key_present: true,
        scope_key_normalized: false,
        repo_global_key_canonical: true,
        generated_members_present: false,
        generated_members_allowed: false,
        generated_members_normalized: true,
        generated_members_unique: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = ReservationRequestShapeItfTrace {
        states: vec![ReservationRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_reservation_request_shape_trace(&trace),
        vec![
            "state[0]: reservation request reject reason does not match validation facts",
            "state[0]: reservation request outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_reservation_lifecycle_request_shape_replay_reports_counterexample_shape() {
    let state = ReservationLifecycleRequestShapeState {
        case_index: 5,
        case: "ZeroRenewExtension".to_owned(),
        operation: "RenewReservation".to_owned(),
        session_token_valid: true,
        reservation_id_valid: true,
        extend_duration_positive: false,
        idempotency_key_valid: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = ReservationLifecycleRequestShapeItfTrace {
        states: vec![ReservationLifecycleRequestShapeItfState { s: state }],
    };

    assert_eq!(
        replay_reservation_lifecycle_request_shape_trace(&trace),
        vec![
            "state[0]: reservation lifecycle request reject reason does not match validation facts",
            "state[0]: reservation lifecycle request outcome disagrees with validation facts",
        ]
    );
}

#[rstest]
fn covey_reservation_record_shape_replay_reports_counterexample_shape() {
    let state = ReservationRecordShapeState {
        case_index: 15,
        case: "UpdatedBeforeCreated".to_owned(),
        scope: "ExactPath".to_owned(),
        lifecycle: "Active".to_owned(),
        scope_key_present: true,
        scope_key_normalized: true,
        repo_global_key_canonical: true,
        generated_members_present: false,
        generated_members_allowed: false,
        generated_members_normalized: true,
        generated_members_unique: true,
        timestamp_shape: "UpdatedBeforeCreatedShape".to_owned(),
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        accepted: true,
        evaluated: true,
    };
    let trace = ReservationRecordShapeItfTrace {
        states: vec![ReservationRecordShapeItfState { s: state }],
    };

    assert_eq!(
        replay_reservation_record_shape_trace(&trace),
        vec![
            "state[0]: reservation record reject reason disagrees with row facts",
            "state[0]: reservation record outcome disagrees with row facts",
        ]
    );
}

#[rstest]
fn covey_conflict_resolution_request_replay_reports_counterexample_shape() {
    let state = ConflictResolutionRequestState {
        case_index: 10,
        case: "DowngradeResolvedToAcknowledged".to_owned(),
        current_state: "Resolved".to_owned(),
        requested_state: "Acknowledged".to_owned(),
        session_token_valid: true,
        conflict_id_valid: true,
        idempotency_key_valid: true,
        request_shape_accepted: true,
        transition_allowed: true,
        outcome: "Accepted".to_owned(),
        reject_reason: "NoReject".to_owned(),
        evaluated: true,
    };
    let trace = ConflictResolutionRequestItfTrace {
        states: vec![ConflictResolutionRequestItfState { s: state }],
    };

    assert_eq!(
        replay_conflict_resolution_request_trace(&trace),
        vec![
            "state[0]: conflict resolution reject reason does not match validation facts",
            "state[0]: conflict resolution transition flag disagrees with transition matrix",
            "state[0]: conflict resolution outcome disagrees with request and transition facts",
        ]
    );
}

#[rstest]
fn covey_bd_import_item_outcome_replay_reports_counterexample_shape() {
    let state = BdImportItemOutcomeShapeState {
        case_index: 4,
        case: "DuplicateMissingSubtask".to_owned(),
        subtask_id_present: false,
        skip_reason: "DeterministicDuplicate".to_owned(),
        outcome: "SkippedDuplicate".to_owned(),
        reject_reason: "NoReject".to_owned(),
        evaluated: true,
    };
    let trace = BdImportItemOutcomeShapeItfTrace {
        states: vec![BdImportItemOutcomeShapeItfState { s: state }],
    };

    assert_eq!(
        replay_bd_import_item_outcome_shape_trace(&trace),
        vec![
            "state[0]: BD import item reject reason does not match outcome shape",
            "state[0]: BD import item outcome does not match subtask/skip shape",
            "state[0]: duplicate BD item lacks duplicate subtask binding",
        ]
    );
}

#[rstest]
fn covey_claim_dependency_gate_replay_reports_counterexample_shape() {
    let state = ClaimDependencyGateState {
        case_index: 1,
        claim_path: ClaimDependencyPath::TargetedClaim,
        role: ClaimDependencyRole::Executor,
        session: ClaimDependencySession::SessionOccupied,
        meta: ClaimDependencyMeta::MetaCompleted,
        kind: ClaimDependencyKind::Work,
        lineage: ClaimDependencyLineage::PlainCandidate,
        candidate: ClaimDependencyCandidate::NoCandidate,
        dependency: ClaimDependencyDependency::DepOpen,
        decision: ClaimDependencyDecision::ClaimCreated,
        reject_reason: ClaimDependencyRejectReason::NoReject,
        dependency_satisfied: true,
        candidate_selected: true,
        claim_created: true,
        subtask_claimed: false,
        session_active_subtask_set: false,
        fence_issued: false,
        evaluated: true,
    };
    let trace = ClaimDependencyGateItfTrace {
        states: vec![ClaimDependencyGateItfState { s: state }],
    };

    assert_eq!(
        replay_claim_dependency_gate_trace(&trace),
        vec![
            "state[0]: claim dependency decision does not match inputs",
            "state[0]: dependency satisfaction marker does not match state",
            "state[0]: open dependency allowed work claim",
            "state[0]: created claim lacks subtask/session/fence binding",
            "state[0]: targeted blocked candidate did not reject",
            "state[0]: occupied session created claim",
            "state[0]: terminal meta created claim",
            "state[0]: selected absent candidate",
        ]
    );
}

#[rstest]
fn covey_claim_dependency_gate_replay_reports_unclaimed_review_followup_counterexample() {
    let state = ClaimDependencyGateState {
        case_index: 1,
        claim_path: ClaimDependencyPath::ClaimNext,
        role: ClaimDependencyRole::Executor,
        session: ClaimDependencySession::SessionFree,
        meta: ClaimDependencyMeta::MetaActive,
        kind: ClaimDependencyKind::Work,
        lineage: ClaimDependencyLineage::ReviewFollowupCandidate,
        candidate: ClaimDependencyCandidate::CandidateAvailable,
        dependency: ClaimDependencyDependency::DepChangesRequestedFollowupAvailable,
        decision: ClaimDependencyDecision::NoClaimableCandidate,
        reject_reason: ClaimDependencyRejectReason::NoReject,
        dependency_satisfied: true,
        candidate_selected: false,
        claim_created: false,
        subtask_claimed: false,
        session_active_subtask_set: false,
        fence_issued: false,
        evaluated: true,
    };
    let trace = ClaimDependencyGateItfTrace {
        states: vec![ClaimDependencyGateItfState { s: state }],
    };

    assert_eq!(
        replay_claim_dependency_gate_trace(&trace),
        vec![
            "state[0]: claim dependency decision does not match inputs",
            "state[0]: review follow-up candidate for changes-requested source was not claimed",
        ]
    );
}

#[rstest]
fn covey_mutation_idempotency_replay_reports_counterexample_shape() {
    let duplicate_record = MutationIdempotencyRecordState {
        present: true,
        actor: "ActorA".to_owned(),
        operation: "RegisterSession".to_owned(),
        key: "Key1".to_owned(),
        request_hash: "ReqA".to_owned(),
        response: "RespA".to_owned(),
        response_json_valid: true,
    };
    let state = MutationIdempotencyState {
        r0: duplicate_record,
        r1: MutationIdempotencyRecordState {
            present: true,
            actor: "ActorA".to_owned(),
            operation: "RegisterSession".to_owned(),
            key: "Key1".to_owned(),
            request_hash: "ReqB".to_owned(),
            response: "RespB".to_owned(),
            response_json_valid: true,
        },
        last_actor: "ActorA".to_owned(),
        last_operation: "RegisterSession".to_owned(),
        last_key: "Key1".to_owned(),
        last_request_hash: "ReqB".to_owned(),
        last_response: "NoResponse".to_owned(),
        last_closure_ok: true,
        last_serialize_ok: true,
        last_outcome: "Conflict".to_owned(),
        side_effects: 1,
        record_writes: 2,
        last_side_effect_delta: 1,
        last_record_write_delta: 1,
        identity_matched_before_attempt: false,
    };
    let trace = MutationIdempotencyItfTrace {
        states: vec![MutationIdempotencyItfState { s: state }],
    };

    assert_eq!(
        replay_mutation_idempotency_trace(&trace),
        vec![
            "state[0]: duplicate idempotency records share one actor/operation/key",
            "state[0]: request drift conflict mutated state or crossed namespace",
            "state[0]: failed idempotent mutation wrote state",
        ]
    );
}

#[rstest]
fn covey_event_log_replay_reports_counterexample_shape() {
    let bad_record = EventLogRecordState {
        present: true,
        seq: 1,
        event_type: "SessionHeartbeat".to_owned(),
        object_type: "Claim".to_owned(),
        actor: "SystemActor".to_owned(),
        visible_session_token: true,
        payload: "ExpiredCountPayload".to_owned(),
        readable: false,
    };
    let state = EventLogState {
        e0: bad_record,
        e1: EventLogRecordState {
            present: false,
            seq: 0,
            event_type: "NoEvent".to_owned(),
            object_type: "NoObject".to_owned(),
            actor: "NoActor".to_owned(),
            visible_session_token: false,
            payload: "NoPayload".to_owned(),
            readable: true,
        },
        next_seq: 3,
        last_outcome: "Committed".to_owned(),
        last_event_type: "SessionHeartbeat".to_owned(),
        last_object_type: "Session".to_owned(),
        last_actor: "SessionActor".to_owned(),
        last_token: "ValidSessionToken".to_owned(),
        last_payload: "MalformedPayload".to_owned(),
        last_mutation_ok: false,
        last_payload_matches: true,
        last_object_matches: true,
        last_actor_valid: true,
        last_readable: false,
        last_seq_assigned: 2,
        last_count_delta: 0,
    };
    let trace = EventLogItfTrace {
        states: vec![EventLogItfState { s: state }],
    };

    assert_eq!(
        replay_event_log_trace(&trace),
        vec![
            "state[0]: next event sequence does not match append-only count",
            "state[0]: e0 committed unreadable event row",
            "state[0]: e0 event_type does not match payload",
            "state[0]: e0 system actor exposed token",
            "state[0]: last payload-match marker disagrees with event type",
            "state[0]: last object-match marker disagrees with payload",
            "state[0]: committed append did not add exactly one readable event",
            "state[0]: unreadable append was not rejected",
        ]
    );
}

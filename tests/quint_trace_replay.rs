use rstest::{fixture, rstest};
use serde::Deserialize;

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
const COVEY_APPLY_GATE_EVIDENCE_ITF: &str =
    include_str!("fixtures/quint/CoveyApplyGateEvidence.itf.json");
const COVEY_SESSION_META_TASK_ITF: &str =
    include_str!("fixtures/quint/CoveySessionMetaTask.itf.json");
const COVEY_LANDING_RECEIPT_ITF: &str = include_str!("fixtures/quint/CoveyLandingReceipt.itf.json");
const COVEY_OPENSPEC_IMPORT_ITF: &str = include_str!("fixtures/quint/CoveyOpenSpecImport.itf.json");
const COVEY_BD_IMPORT_ITF: &str = include_str!("fixtures/quint/CoveyBdImport.itf.json");
const COVEY_CLAIM_DEPENDENCY_GATE_ITF: &str =
    include_str!("fixtures/quint/CoveyClaimDependencyGate.itf.json");
const COVEY_MUTATION_IDEMPOTENCY_ITF: &str =
    include_str!("fixtures/quint/CoveyMutationIdempotency.itf.json");
const COVEY_EVENT_LOG_ITF: &str = include_str!("fixtures/quint/CoveyEventLog.itf.json");

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
struct ReviewFollowupState {
    #[serde(deserialize_with = "deserialize_itf_variant")]
    b0: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    b1: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    b2: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    b3: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    p0: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    p1: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    p2: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    p3: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    active: String,
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
struct ClaimDependencyGateState {
    #[serde(rename = "path", deserialize_with = "deserialize_itf_variant")]
    claim_path: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    role: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    session: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    meta: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    kind: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    lineage: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    candidate: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    dependency: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    decision: String,
    #[serde(rename = "rejectReason", deserialize_with = "deserialize_itf_variant")]
    reject_reason: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    B0,
    B1,
    B2,
    B3,
}

const BLOCKS: [Block; 4] = [Block::B0, Block::B1, Block::B2, Block::B3];

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

impl Block {
    fn as_str(self) -> &'static str {
        match self {
            Block::B0 => "B0",
            Block::B1 => "B1",
            Block::B2 => "B2",
            Block::B3 => "B3",
        }
    }

    fn index(self) -> usize {
        match self {
            Block::B0 => 0,
            Block::B1 => 1,
            Block::B2 => 2,
            Block::B3 => 3,
        }
    }
}

impl ReviewFollowupState {
    fn status(&self, block: Block) -> &str {
        match block {
            Block::B0 => self.b0.as_str(),
            Block::B1 => self.b1.as_str(),
            Block::B2 => self.b2.as_str(),
            Block::B3 => self.b3.as_str(),
        }
    }

    fn parent(&self, block: Block) -> &str {
        match block {
            Block::B0 => self.p0.as_str(),
            Block::B1 => self.p1.as_str(),
            Block::B2 => self.p2.as_str(),
            Block::B3 => self.p3.as_str(),
        }
    }

    fn rejected(&self, block: Block) -> bool {
        match block {
            Block::B0 => self.r0,
            Block::B1 => self.r1,
            Block::B2 => self.r2,
            Block::B3 => self.r3,
        }
    }
}

fn block_from_str(value: &str) -> Option<Block> {
    match value {
        "B0" => Some(Block::B0),
        "B1" => Some(Block::B1),
        "B2" => Some(Block::B2),
        "B3" => Some(Block::B3),
        _ => None,
    }
}

fn children_of(state: &ReviewFollowupState, block: Block) -> Vec<Block> {
    BLOCKS
        .into_iter()
        .filter(|child| state.parent(*child) == block.as_str() && state.status(*child) != "Absent")
        .collect()
}

fn available_block_exists(state: &ReviewFollowupState) -> bool {
    BLOCKS
        .into_iter()
        .any(|block| state.status(block) == "Available")
}

fn repairable_missing_followup(state: &ReviewFollowupState) -> bool {
    let Ok(next_index) = usize::try_from(state.next_block) else {
        return false;
    };
    let Some(candidate) = BLOCKS.get(next_index).copied() else {
        return false;
    };
    if state.status(candidate) != "Absent" {
        return false;
    }
    BLOCKS.into_iter().any(|block| {
        state.status(block) == "ChangesRequested" && children_of(state, block).is_empty()
    })
}

fn replay_review_followup_trace(trace: &ItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.m;
        for block in BLOCKS {
            let status = state.status(block);
            let parent = state.parent(block);
            let children = children_of(state, block);
            if status == "Absent" && parent != "NoBlock" {
                violations.push(format!(
                    "state[{index}]: absent block {} has parent {parent}",
                    block.as_str()
                ));
            }
            if state.rejected(block) {
                if status != "ChangesRequested" {
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
            if let Some(parent_block) = block_from_str(parent)
                && parent_block.index() >= block.index()
            {
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
        if let Some(active) = block_from_str(&state.active)
            && !matches!(state.status(active), "Claimed" | "InProgress")
        {
            violations.push(format!(
                "state[{index}]: active block {} is not claimed or in progress",
                active.as_str()
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

fn claim_dependency_role_can_claim(role: &str, kind: &str) -> bool {
    matches!((role, kind), ("Executor", "Work") | ("Reviewer", "Review"))
}

fn claim_dependency_meta_claimable(meta: &str) -> bool {
    matches!(meta, "MetaActive" | "MetaPlanning")
}

fn claim_dependency_satisfied(kind: &str, lineage: &str, dependency: &str) -> bool {
    if kind == "Review" {
        return true;
    }
    if lineage == "ReviewFollowupCandidate" {
        return true;
    }
    matches!(
        dependency,
        "NoDependency"
            | "DepApproved"
            | "DepReadyForApply"
            | "DepApplied"
            | "DepDecided"
            | "DepChangesRequestedFollowupApproved"
            | "DepChangesRequestedFollowupReadyForApply"
            | "DepChangesRequestedFollowupApplied"
            | "DepChangesRequestedFollowupDecided"
    )
}

fn claim_dependency_expected_reject(state: &ClaimDependencyGateState) -> &'static str {
    if state.session == "SessionInactive" {
        "SessionUnavailable"
    } else if state.session == "SessionOccupied" {
        "SessionAlreadyOccupied"
    } else if !claim_dependency_role_can_claim(&state.role, &state.kind) {
        "WrongRole"
    } else if !claim_dependency_meta_claimable(&state.meta) {
        "MetaUnavailable"
    } else if state.candidate != "CandidateAvailable"
        || (state.kind == "Review" && state.lineage != "PlainCandidate")
    {
        "IllegalTransition"
    } else if !claim_dependency_satisfied(&state.kind, &state.lineage, &state.dependency) {
        "DependencyUnsatisfied"
    } else {
        "NoReject"
    }
}

fn claim_dependency_expected_decision(state: &ClaimDependencyGateState) -> &'static str {
    let reject = claim_dependency_expected_reject(state);
    if reject == "NoReject" {
        "ClaimCreated"
    } else if state.claim_path == "ClaimNext"
        && matches!(
            reject,
            "MetaUnavailable" | "IllegalTransition" | "DependencyUnsatisfied"
        )
    {
        "NoClaimableCandidate"
    } else {
        "Rejected"
    }
}

fn replay_claim_dependency_gate_trace(trace: &ClaimDependencyGateItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        let prefix = format!("state[{index}]");
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
            claim_dependency_satisfied(&state.kind, &state.lineage, &state.dependency);
        if state.dependency_satisfied != expected_dependency {
            violations.push(format!(
                "{prefix}: dependency satisfaction marker does not match state"
            ));
        }
        if state.claim_created && state.kind == "Work" && !state.dependency_satisfied {
            violations.push(format!(
                "{prefix}: work claim ignored unsatisfied dependency"
            ));
        }
        if state.kind == "Work" && state.dependency == "DepOpen" && state.claim_created {
            violations.push(format!("{prefix}: open dependency allowed work claim"));
        }
        if state.kind == "Work"
            && state.lineage == "ChangesRequestedSourceCandidate"
            && matches!(
                state.dependency.as_str(),
                "DepChangesRequestedNoFollowup" | "DepChangesRequestedFollowupAvailable"
            )
            && state.claim_created
        {
            violations.push(format!(
                "{prefix}: changes-requested dependency without terminal follow-up allowed claim"
            ));
        }
        if state.kind == "Work"
            && state.lineage == "ReviewFollowupCandidate"
            && state.dependency == "DepChangesRequestedFollowupAvailable"
            && state.claim_path == "ClaimNext"
            && !state.claim_created
        {
            violations.push(format!(
                "{prefix}: review follow-up candidate for changes-requested source was not claimed"
            ));
        }
        if state.kind == "Work"
            && matches!(
                state.dependency.as_str(),
                "DepChangesRequestedFollowupApproved"
                    | "DepChangesRequestedFollowupReadyForApply"
                    | "DepChangesRequestedFollowupApplied"
                    | "DepChangesRequestedFollowupDecided"
            )
            && !state.dependency_satisfied
        {
            violations.push(format!(
                "{prefix}: terminal follow-up did not satisfy source dependency"
            ));
        }
        if state.kind == "Work"
            && matches!(
                state.dependency.as_str(),
                "DepApproved" | "DepReadyForApply" | "DepApplied" | "DepDecided"
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
        if state.claim_path == "ClaimNext"
            && state.decision == "NoClaimableCandidate"
            && state.claim_created
        {
            violations.push(format!("{prefix}: claim-next created blocked candidate"));
        }
        if state.claim_path == "TargetedClaim"
            && claim_dependency_expected_reject(state) != "NoReject"
            && state.decision != "Rejected"
        {
            violations.push(format!(
                "{prefix}: targeted blocked candidate did not reject"
            ));
        }
        if !claim_dependency_role_can_claim(&state.role, &state.kind) && state.claim_created {
            violations.push(format!("{prefix}: wrong role created claim"));
        }
        if state.session == "SessionOccupied" && state.claim_created {
            violations.push(format!("{prefix}: occupied session created claim"));
        }
        if !claim_dependency_meta_claimable(&state.meta) && state.claim_created {
            violations.push(format!("{prefix}: terminal meta created claim"));
        }
        if state.candidate_selected && state.candidate == "NoCandidate" {
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
fn apply_gate_evidence_trace() -> ApplyGateEvidenceItfTrace {
    serde_json::from_str(COVEY_APPLY_GATE_EVIDENCE_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn session_meta_task_trace() -> SessionMetaTaskItfTrace {
    serde_json::from_str(COVEY_SESSION_META_TASK_ITF).expect("fixture must be valid ITF JSON")
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
            "fixture should cover {expected}"
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
            "fixture should cover {expected}"
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
fn covey_replays_quint_claim_dependency_gate_itf_trace(
    claim_dependency_gate_trace: ClaimDependencyGateItfTrace,
) {
    assert!(
        !claim_dependency_gate_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    for expected in [
        "DepOpen",
        "DepApplied",
        "DepChangesRequestedNoFollowup",
        "DepChangesRequestedFollowupAvailable",
        "DepChangesRequestedFollowupApplied",
        "DepChangesRequestedFollowupDecided",
    ] {
        assert!(
            claim_dependency_gate_trace
                .states
                .iter()
                .any(|state| state.s.dependency == expected),
            "fixture should cover {expected}"
        );
    }
    assert!(
        claim_dependency_gate_trace.states.iter().any(|state| {
            state.s.lineage == "ChangesRequestedSourceCandidate"
                && state.s.dependency == "DepChangesRequestedFollowupAvailable"
                && !state.s.claim_created
        }),
        "fixture should cover blocked original changes-requested source candidate"
    );
    assert!(
        claim_dependency_gate_trace.states.iter().any(|state| {
            state.s.lineage == "ReviewFollowupCandidate"
                && state.s.dependency == "DepChangesRequestedFollowupAvailable"
                && state.s.claim_created
        }),
        "fixture should cover claimable review follow-up candidate"
    );
    assert!(
        claim_dependency_gate_trace.states.iter().any(|state| {
            state.s.claim_path == "TargetedClaim"
                && state.s.decision == "Rejected"
                && state.s.reject_reason == "DependencyUnsatisfied"
        }),
        "fixture should cover targeted dependency rejection"
    );
    assert!(
        claim_dependency_gate_trace
            .states
            .iter()
            .any(|state| state.s.role == "Reviewer"
                && state.s.kind == "Review"
                && state.s.claim_created),
        "fixture should cover reviewer review claims"
    );
    assert!(
        claim_dependency_gate_trace
            .states
            .iter()
            .any(|state| state.s.decision == "NoClaimableCandidate"),
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
            "fixture should cover {expected}"
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
            "fixture should cover {expected}"
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
        b0: "Available".to_owned(),
        b1: "Available".to_owned(),
        b2: "Absent".to_owned(),
        b3: "Absent".to_owned(),
        p0: "NoBlock".to_owned(),
        p1: "B0".to_owned(),
        p2: "NoBlock".to_owned(),
        p3: "NoBlock".to_owned(),
        active: "NoBlock".to_owned(),
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
fn covey_claim_dependency_gate_replay_reports_counterexample_shape() {
    let state = ClaimDependencyGateState {
        claim_path: "TargetedClaim".to_owned(),
        role: "Executor".to_owned(),
        session: "SessionOccupied".to_owned(),
        meta: "MetaCompleted".to_owned(),
        kind: "Work".to_owned(),
        lineage: "PlainCandidate".to_owned(),
        candidate: "NoCandidate".to_owned(),
        dependency: "DepOpen".to_owned(),
        decision: "ClaimCreated".to_owned(),
        reject_reason: "NoReject".to_owned(),
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
        claim_path: "ClaimNext".to_owned(),
        role: "Executor".to_owned(),
        session: "SessionFree".to_owned(),
        meta: "MetaActive".to_owned(),
        kind: "Work".to_owned(),
        lineage: "ReviewFollowupCandidate".to_owned(),
        candidate: "CandidateAvailable".to_owned(),
        dependency: "DepChangesRequestedFollowupAvailable".to_owned(),
        decision: "NoClaimableCandidate".to_owned(),
        reject_reason: "NoReject".to_owned(),
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

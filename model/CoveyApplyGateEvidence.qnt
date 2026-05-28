module CoveyApplyGateEvidence {
  type Case =
    ValidApplyGateEvidence
    | MissingLiveReview
    | ReviewNotDecided
    | ReviewNotApproved
    | MissingFindingsDigest
    | ArtifactNotBoundToReview
    | ProducerReviewerSamePrincipal
    | ApplyGateIsProducer
    | ApplyGateIsReviewer
    | ProducerMissingAttestation
    | ReviewerMissingAttestation
    | ApplyGateMissingAttestation
    | ProducerReviewerRuntimeSame
    | ProducerApplyGateRuntimeSame
    | ReviewerApplyGateRuntimeSame
    | ProducerReviewerProviderRunSame
    | ProducerApplyGateProviderRunSame
    | ReviewerApplyGateProviderRunSame
    | ProducerReviewerTranscriptSame
    | ProducerApplyGateTranscriptSame
    | ReviewerApplyGateTranscriptSame
    | VerificationQueueNotInFlight
    | VerificationArtifactMismatch
    | VerificationFenceMismatch
    | VerificationReviewMismatch
    | MarkAppliedWrongOwner
    | MarkAppliedStaleFence
    | MarkAppliedLeaseExpired
    | MarkAppliedSubtaskNotReady
    | MarkAppliedMissingVerification
    | LandingAuthorizationBeforeApplied
    | LandingAuthorizationArtifactMismatch
    | LandingAuthorizationFenceMismatch
    | LandingAuthorizationVerificationMismatch
    | ReceiptBeforeApplied
    | ReceiptArtifactMismatch
    | ReceiptFenceMismatch
    | DuplicateReceiptDivergent

  type QueueState = Queued | InFlight | Applied | Superseded
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | LiveReviewInvalid
    | PrincipalSeparationInvalid
    | RuntimeAttestationInvalid
    | RuntimeSeparationInvalid
    | ApplyVerificationInvalid
    | MarkAppliedInvalid
    | LandingAuthorizationInvalid
    | LandingReceiptInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    queue: QueueState,
    queueOwnerIsApplyGate: bool,
    queueFenceMatches: bool,
    queueLeaseLive: bool,
    queueArtifactMatchesRequest: bool,
    subtaskReadyForApply: bool,
    reviewExists: bool,
    reviewDecided: bool,
    reviewApproved: bool,
    findingsDigestPresent: bool,
    artifactBoundToReview: bool,
    producerReviewerPrincipalSeparated: bool,
    applyGateSeparatedFromProducer: bool,
    applyGateSeparatedFromReviewer: bool,
    producerAttested: bool,
    reviewerAttested: bool,
    applyGateAttested: bool,
    producerReviewerRuntimeSeparated: bool,
    producerApplyGateRuntimeSeparated: bool,
    reviewerApplyGateRuntimeSeparated: bool,
    producerReviewerProviderRunSeparated: bool,
    producerApplyGateProviderRunSeparated: bool,
    reviewerApplyGateProviderRunSeparated: bool,
    producerReviewerTranscriptSeparated: bool,
    producerApplyGateTranscriptSeparated: bool,
    reviewerApplyGateTranscriptSeparated: bool,
    verificationAttempted: bool,
    verificationRecorded: bool,
    verificationReviewMatches: bool,
    markAppliedAttempted: bool,
    landingAuthorizationAccepted: bool,
    landingAuthorizationArtifactMatches: bool,
    landingAuthorizationFenceMatches: bool,
    landingAuthorizationVerificationMatches: bool,
    receiptRecorded: bool,
    receiptArtifactMatches: bool,
    receiptFenceMatches: bool,
    duplicateReceiptDivergent: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
  }

  var s: Model

  def factsForCase(case) = {
    queue:
      if (case == VerificationQueueNotInFlight) Queued
      else if (case == LandingAuthorizationBeforeApplied or case == ReceiptBeforeApplied) InFlight
      else if (case.in(Set(ValidApplyGateEvidence, LandingAuthorizationArtifactMismatch, LandingAuthorizationFenceMismatch, LandingAuthorizationVerificationMismatch, ReceiptArtifactMismatch, ReceiptFenceMismatch, DuplicateReceiptDivergent))) Applied
      else InFlight,
    queueOwnerIsApplyGate: case != MarkAppliedWrongOwner,
    queueFenceMatches: case != VerificationFenceMismatch and case != MarkAppliedStaleFence,
    queueLeaseLive: case != MarkAppliedLeaseExpired,
    queueArtifactMatchesRequest: case != VerificationArtifactMismatch,
    subtaskReadyForApply: case != MarkAppliedSubtaskNotReady,
    reviewExists: case != MissingLiveReview,
    reviewDecided: case != ReviewNotDecided,
    reviewApproved: case != ReviewNotApproved,
    findingsDigestPresent: case != MissingFindingsDigest,
    artifactBoundToReview: case != ArtifactNotBoundToReview,
    producerReviewerPrincipalSeparated: case != ProducerReviewerSamePrincipal,
    applyGateSeparatedFromProducer: case != ApplyGateIsProducer,
    applyGateSeparatedFromReviewer: case != ApplyGateIsReviewer,
    producerAttested: case != ProducerMissingAttestation,
    reviewerAttested: case != ReviewerMissingAttestation,
    applyGateAttested: case != ApplyGateMissingAttestation,
    producerReviewerRuntimeSeparated: case != ProducerReviewerRuntimeSame,
    producerApplyGateRuntimeSeparated: case != ProducerApplyGateRuntimeSame,
    reviewerApplyGateRuntimeSeparated: case != ReviewerApplyGateRuntimeSame,
    producerReviewerProviderRunSeparated: case != ProducerReviewerProviderRunSame,
    producerApplyGateProviderRunSeparated: case != ProducerApplyGateProviderRunSame,
    reviewerApplyGateProviderRunSeparated: case != ReviewerApplyGateProviderRunSame,
    producerReviewerTranscriptSeparated: case != ProducerReviewerTranscriptSame,
    producerApplyGateTranscriptSeparated: case != ProducerApplyGateTranscriptSame,
    reviewerApplyGateTranscriptSeparated: case != ReviewerApplyGateTranscriptSame,
    verificationAttempted: case.in(Set(VerificationQueueNotInFlight, VerificationArtifactMismatch, VerificationFenceMismatch, VerificationReviewMismatch)),
    verificationRecorded: case != MarkAppliedMissingVerification,
    verificationReviewMatches: case != VerificationReviewMismatch,
    markAppliedAttempted: case.in(Set(MarkAppliedWrongOwner, MarkAppliedStaleFence, MarkAppliedLeaseExpired, MarkAppliedSubtaskNotReady, MarkAppliedMissingVerification)),
    landingAuthorizationAccepted: not(case.in(Set(ReceiptBeforeApplied, ReceiptArtifactMismatch, ReceiptFenceMismatch, DuplicateReceiptDivergent))),
    landingAuthorizationArtifactMatches: case != LandingAuthorizationArtifactMismatch,
    landingAuthorizationFenceMatches: case != LandingAuthorizationFenceMismatch,
    landingAuthorizationVerificationMatches: case != LandingAuthorizationVerificationMismatch,
    receiptRecorded: case.in(Set(ValidApplyGateEvidence, ReceiptBeforeApplied, ReceiptArtifactMismatch, ReceiptFenceMismatch, DuplicateReceiptDivergent)),
    receiptArtifactMatches: case != ReceiptArtifactMismatch,
    receiptFenceMatches: case != ReceiptFenceMismatch,
    duplicateReceiptDivergent: case == DuplicateReceiptDivergent,
  }

  def liveReviewOk(facts) =
    facts.reviewExists
    and facts.reviewDecided
    and facts.reviewApproved
    and facts.findingsDigestPresent
    and facts.artifactBoundToReview

  def principalSeparationOk(facts) =
    facts.producerReviewerPrincipalSeparated
    and facts.applyGateSeparatedFromProducer
    and facts.applyGateSeparatedFromReviewer

  def attestationsOk(facts) =
    facts.producerAttested
    and facts.reviewerAttested
    and facts.applyGateAttested

  def runtimeSeparationOk(facts) =
    facts.producerReviewerRuntimeSeparated
    and facts.producerApplyGateRuntimeSeparated
    and facts.reviewerApplyGateRuntimeSeparated
    and facts.producerReviewerProviderRunSeparated
    and facts.producerApplyGateProviderRunSeparated
    and facts.reviewerApplyGateProviderRunSeparated
    and facts.producerReviewerTranscriptSeparated
    and facts.producerApplyGateTranscriptSeparated
    and facts.reviewerApplyGateTranscriptSeparated

  def applyVerificationOk(facts) =
    not(facts.verificationAttempted)
    or (
      facts.queue == InFlight
      and facts.queueArtifactMatchesRequest
      and facts.queueFenceMatches
      and facts.verificationReviewMatches
    )

  def markAppliedOk(facts) =
    not(facts.markAppliedAttempted)
    or (
      facts.queue == InFlight
      and facts.queueOwnerIsApplyGate
      and facts.queueFenceMatches
      and facts.queueLeaseLive
      and facts.subtaskReadyForApply
      and facts.queueArtifactMatchesRequest
      and facts.verificationRecorded
      and applyVerificationOk(facts)
    )

  def landingAuthorizationOk(facts) =
    not(facts.landingAuthorizationAccepted)
    or (
      facts.queue == Applied
      and facts.landingAuthorizationArtifactMatches
      and facts.landingAuthorizationFenceMatches
      and facts.landingAuthorizationVerificationMatches
      and facts.verificationRecorded
    )

  def landingReceiptOk(facts) =
    not(facts.receiptRecorded)
    or (
      facts.queue == Applied
      and facts.receiptArtifactMatches
      and facts.receiptFenceMatches
      and not(facts.duplicateReceiptDivergent)
    )

  def expectedReject(facts) =
    if (not(liveReviewOk(facts))) LiveReviewInvalid
    else if (not(principalSeparationOk(facts))) PrincipalSeparationInvalid
    else if (not(attestationsOk(facts))) RuntimeAttestationInvalid
    else if (not(runtimeSeparationOk(facts))) RuntimeSeparationInvalid
    else if (not(applyVerificationOk(facts))) ApplyVerificationInvalid
    else if (not(markAppliedOk(facts))) MarkAppliedInvalid
    else if (not(landingAuthorizationOk(facts))) LandingAuthorizationInvalid
    else if (not(landingReceiptOk(facts))) LandingReceiptInvalid
    else NoReject

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidApplyGateEvidence,
      queue: Queued,
      queueOwnerIsApplyGate: false,
      queueFenceMatches: false,
      queueLeaseLive: false,
      queueArtifactMatchesRequest: false,
      subtaskReadyForApply: false,
      reviewExists: false,
      reviewDecided: false,
      reviewApproved: false,
      findingsDigestPresent: false,
      artifactBoundToReview: false,
      producerReviewerPrincipalSeparated: false,
      applyGateSeparatedFromProducer: false,
      applyGateSeparatedFromReviewer: false,
      producerAttested: false,
      reviewerAttested: false,
      applyGateAttested: false,
      producerReviewerRuntimeSeparated: false,
      producerApplyGateRuntimeSeparated: false,
      reviewerApplyGateRuntimeSeparated: false,
      producerReviewerProviderRunSeparated: false,
      producerApplyGateProviderRunSeparated: false,
      reviewerApplyGateProviderRunSeparated: false,
      producerReviewerTranscriptSeparated: false,
      producerApplyGateTranscriptSeparated: false,
      reviewerApplyGateTranscriptSeparated: false,
      verificationAttempted: false,
      verificationRecorded: false,
      verificationReviewMatches: false,
      markAppliedAttempted: false,
      landingAuthorizationAccepted: false,
      landingAuthorizationArtifactMatches: false,
      landingAuthorizationFenceMatches: false,
      landingAuthorizationVerificationMatches: false,
      receiptRecorded: false,
      receiptArtifactMatches: false,
      receiptFenceMatches: false,
      duplicateReceiptDivergent: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
    }
  }

  action evaluate(case) = all {
    val facts = factsForCase(case)
    val reject = expectedReject(facts)
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      queue: facts.queue,
      queueOwnerIsApplyGate: facts.queueOwnerIsApplyGate,
      queueFenceMatches: facts.queueFenceMatches,
      queueLeaseLive: facts.queueLeaseLive,
      queueArtifactMatchesRequest: facts.queueArtifactMatchesRequest,
      subtaskReadyForApply: facts.subtaskReadyForApply,
      reviewExists: facts.reviewExists,
      reviewDecided: facts.reviewDecided,
      reviewApproved: facts.reviewApproved,
      findingsDigestPresent: facts.findingsDigestPresent,
      artifactBoundToReview: facts.artifactBoundToReview,
      producerReviewerPrincipalSeparated: facts.producerReviewerPrincipalSeparated,
      applyGateSeparatedFromProducer: facts.applyGateSeparatedFromProducer,
      applyGateSeparatedFromReviewer: facts.applyGateSeparatedFromReviewer,
      producerAttested: facts.producerAttested,
      reviewerAttested: facts.reviewerAttested,
      applyGateAttested: facts.applyGateAttested,
      producerReviewerRuntimeSeparated: facts.producerReviewerRuntimeSeparated,
      producerApplyGateRuntimeSeparated: facts.producerApplyGateRuntimeSeparated,
      reviewerApplyGateRuntimeSeparated: facts.reviewerApplyGateRuntimeSeparated,
      producerReviewerProviderRunSeparated: facts.producerReviewerProviderRunSeparated,
      producerApplyGateProviderRunSeparated: facts.producerApplyGateProviderRunSeparated,
      reviewerApplyGateProviderRunSeparated: facts.reviewerApplyGateProviderRunSeparated,
      producerReviewerTranscriptSeparated: facts.producerReviewerTranscriptSeparated,
      producerApplyGateTranscriptSeparated: facts.producerApplyGateTranscriptSeparated,
      reviewerApplyGateTranscriptSeparated: facts.reviewerApplyGateTranscriptSeparated,
      verificationAttempted: facts.verificationAttempted,
      verificationRecorded: facts.verificationRecorded,
      verificationReviewMatches: facts.verificationReviewMatches,
      markAppliedAttempted: facts.markAppliedAttempted,
      landingAuthorizationAccepted: facts.landingAuthorizationAccepted,
      landingAuthorizationArtifactMatches: facts.landingAuthorizationArtifactMatches,
      landingAuthorizationFenceMatches: facts.landingAuthorizationFenceMatches,
      landingAuthorizationVerificationMatches: facts.landingAuthorizationVerificationMatches,
      receiptRecorded: facts.receiptRecorded,
      receiptArtifactMatches: facts.receiptArtifactMatches,
      receiptFenceMatches: facts.receiptFenceMatches,
      duplicateReceiptDivergent: facts.duplicateReceiptDivergent,
      outcome: if (reject == NoReject) Accepted else Rejected,
      rejectReason: reject,
      accepted: reject == NoReject,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidApplyGateEvidence) },
    all { s.caseIndex == 1, evaluate(MissingLiveReview) },
    all { s.caseIndex == 2, evaluate(ReviewNotDecided) },
    all { s.caseIndex == 3, evaluate(ReviewNotApproved) },
    all { s.caseIndex == 4, evaluate(MissingFindingsDigest) },
    all { s.caseIndex == 5, evaluate(ArtifactNotBoundToReview) },
    all { s.caseIndex == 6, evaluate(ProducerReviewerSamePrincipal) },
    all { s.caseIndex == 7, evaluate(ApplyGateIsProducer) },
    all { s.caseIndex == 8, evaluate(ApplyGateIsReviewer) },
    all { s.caseIndex == 9, evaluate(ProducerMissingAttestation) },
    all { s.caseIndex == 10, evaluate(ReviewerMissingAttestation) },
    all { s.caseIndex == 11, evaluate(ApplyGateMissingAttestation) },
    all { s.caseIndex == 12, evaluate(ProducerReviewerRuntimeSame) },
    all { s.caseIndex == 13, evaluate(ProducerApplyGateRuntimeSame) },
    all { s.caseIndex == 14, evaluate(ReviewerApplyGateRuntimeSame) },
    all { s.caseIndex == 15, evaluate(ProducerReviewerProviderRunSame) },
    all { s.caseIndex == 16, evaluate(ProducerApplyGateProviderRunSame) },
    all { s.caseIndex == 17, evaluate(ReviewerApplyGateProviderRunSame) },
    all { s.caseIndex == 18, evaluate(ProducerReviewerTranscriptSame) },
    all { s.caseIndex == 19, evaluate(ProducerApplyGateTranscriptSame) },
    all { s.caseIndex == 20, evaluate(ReviewerApplyGateTranscriptSame) },
    all { s.caseIndex == 21, evaluate(VerificationQueueNotInFlight) },
    all { s.caseIndex == 22, evaluate(VerificationArtifactMismatch) },
    all { s.caseIndex == 23, evaluate(VerificationFenceMismatch) },
    all { s.caseIndex == 24, evaluate(VerificationReviewMismatch) },
    all { s.caseIndex == 25, evaluate(MarkAppliedWrongOwner) },
    all { s.caseIndex == 26, evaluate(MarkAppliedStaleFence) },
    all { s.caseIndex == 27, evaluate(MarkAppliedLeaseExpired) },
    all { s.caseIndex == 28, evaluate(MarkAppliedSubtaskNotReady) },
    all { s.caseIndex == 29, evaluate(MarkAppliedMissingVerification) },
    all { s.caseIndex == 30, evaluate(LandingAuthorizationBeforeApplied) },
    all { s.caseIndex == 31, evaluate(LandingAuthorizationArtifactMismatch) },
    all { s.caseIndex == 32, evaluate(LandingAuthorizationFenceMismatch) },
    all { s.caseIndex == 33, evaluate(LandingAuthorizationVerificationMismatch) },
    all { s.caseIndex == 34, evaluate(ReceiptBeforeApplied) },
    all { s.caseIndex == 35, evaluate(ReceiptArtifactMismatch) },
    all { s.caseIndex == 36, evaluate(ReceiptFenceMismatch) },
    all { s.caseIndex == 37, evaluate(DuplicateReceiptDivergent) },
    all { s.caseIndex >= 38, s' = s },
  }

  val typeOk = true
  val rejectReasonMatchesFirstFailedGate =
    s.outcome == NotEvaluated or s.rejectReason == expectedReject(s)
  val acceptedOnlyWhenAllEvidenceGatesPass =
    not(s.accepted)
    or (
      liveReviewOk(s)
      and principalSeparationOk(s)
      and attestationsOk(s)
      and runtimeSeparationOk(s)
      and applyVerificationOk(s)
      and markAppliedOk(s)
      and landingAuthorizationOk(s)
      and landingReceiptOk(s)
    )
  val applyVerificationBindsCurrentInFlightFence =
    not(s.verificationAttempted and s.accepted)
    or (
      s.queue == InFlight
      and s.queueFenceMatches
      and s.queueArtifactMatchesRequest
      and s.verificationReviewMatches
    )
  val markAppliedRequiresCurrentVerification =
    not(s.markAppliedAttempted and s.accepted)
    or (s.verificationRecorded and s.queueOwnerIsApplyGate and s.queueLeaseLive and s.subtaskReadyForApply)
  val landingAuthorizationRequiresAppliedQueue =
    not(s.landingAuthorizationAccepted and s.accepted) or s.queue == Applied
  val receiptRequiresAppliedArtifactAndFence =
    not(s.receiptRecorded and s.accepted)
    or (s.queue == Applied and s.receiptArtifactMatches and s.receiptFenceMatches and not(s.duplicateReceiptDivergent))
  val acceptedEvidenceHasSeparationOfDuties =
    not(s.accepted)
    or (
      principalSeparationOk(s)
      and runtimeSeparationOk(s)
    )
}

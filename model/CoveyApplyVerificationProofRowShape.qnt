module CoveyApplyVerificationProofRowShape {
  type Case =
    ValidProofRow
    | InvalidQueueId
    | InvalidArtifactDigest
    | InvalidReviewId
    | InvalidFindingsDigest
    | NonPositiveFence
    | InvalidVerifier
    | InvalidVerdictDigest
    | InvalidSealDigest
    | InvalidRecordedBySession
    | NegativeCreatedAt

  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | QueueIdInvalid
    | ArtifactDigestInvalid
    | ReviewIdInvalid
    | FindingsDigestInvalid
    | FenceSeqInvalid
    | VerifierInvalid
    | VerdictDigestInvalid
    | SealDigestInvalid
    | RecordedBySessionInvalid
    | CreatedAtInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    queueIdValid: bool,
    artifactDigestValid: bool,
    reviewIdValid: bool,
    findingsDigestValid: bool,
    fenceSeqPositive: bool,
    verifierValid: bool,
    verdictDigestValid: bool,
    sealDigestValid: bool,
    recordedBySessionValid: bool,
    createdAtNonNegative: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    scheduledWork: bool,
    mutationAuthorityGranted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    queueIdValid: case != InvalidQueueId,
    artifactDigestValid: case != InvalidArtifactDigest,
    reviewIdValid: case != InvalidReviewId,
    findingsDigestValid: case != InvalidFindingsDigest,
    fenceSeqPositive: case != NonPositiveFence,
    verifierValid: case != InvalidVerifier,
    verdictDigestValid: case != InvalidVerdictDigest,
    sealDigestValid: case != InvalidSealDigest,
    recordedBySessionValid: case != InvalidRecordedBySession,
    createdAtNonNegative: case != NegativeCreatedAt,
  }

  // Mirrors ApplyVerificationRow::from_db_parts validation order.
  def rejectFor(facts) =
    if (not(facts.queueIdValid)) QueueIdInvalid
    else if (not(facts.artifactDigestValid)) ArtifactDigestInvalid
    else if (not(facts.reviewIdValid)) ReviewIdInvalid
    else if (not(facts.findingsDigestValid)) FindingsDigestInvalid
    else if (not(facts.fenceSeqPositive)) FenceSeqInvalid
    else if (not(facts.verifierValid)) VerifierInvalid
    else if (not(facts.verdictDigestValid)) VerdictDigestInvalid
    else if (not(facts.sealDigestValid)) SealDigestInvalid
    else if (not(facts.recordedBySessionValid)) RecordedBySessionInvalid
    else if (not(facts.createdAtNonNegative)) CreatedAtInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidProofRow,
      queueIdValid: false,
      artifactDigestValid: false,
      reviewIdValid: false,
      findingsDigestValid: false,
      fenceSeqPositive: false,
      verifierValid: false,
      verdictDigestValid: false,
      sealDigestValid: false,
      recordedBySessionValid: false,
      createdAtNonNegative: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
      scheduledWork: false,
      mutationAuthorityGranted: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val facts = factsFor(case)
    val rejectReason = rejectFor(facts)
    val accepted = rejectReason == NoReject
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      queueIdValid: facts.queueIdValid,
      artifactDigestValid: facts.artifactDigestValid,
      reviewIdValid: facts.reviewIdValid,
      findingsDigestValid: facts.findingsDigestValid,
      fenceSeqPositive: facts.fenceSeqPositive,
      verifierValid: facts.verifierValid,
      verdictDigestValid: facts.verdictDigestValid,
      sealDigestValid: facts.sealDigestValid,
      recordedBySessionValid: facts.recordedBySessionValid,
      createdAtNonNegative: facts.createdAtNonNegative,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      scheduledWork: false,
      mutationAuthorityGranted: false,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidProofRow) },
    all { s.caseIndex == 1, evaluate(InvalidQueueId) },
    all { s.caseIndex == 2, evaluate(InvalidArtifactDigest) },
    all { s.caseIndex == 3, evaluate(InvalidReviewId) },
    all { s.caseIndex == 4, evaluate(InvalidFindingsDigest) },
    all { s.caseIndex == 5, evaluate(NonPositiveFence) },
    all { s.caseIndex == 6, evaluate(InvalidVerifier) },
    all { s.caseIndex == 7, evaluate(InvalidVerdictDigest) },
    all { s.caseIndex == 8, evaluate(InvalidSealDigest) },
    all { s.caseIndex == 9, evaluate(InvalidRecordedBySession) },
    all { s.caseIndex == 10, evaluate(NegativeCreatedAt) },
    all { s.caseIndex >= 11, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 11
    and s.case.in(Set(ValidProofRow, InvalidQueueId, InvalidArtifactDigest, InvalidReviewId, InvalidFindingsDigest, NonPositiveFence, InvalidVerifier, InvalidVerdictDigest, InvalidSealDigest, InvalidRecordedBySession, NegativeCreatedAt))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, QueueIdInvalid, ArtifactDigestInvalid, ReviewIdInvalid, FindingsDigestInvalid, FenceSeqInvalid, VerifierInvalid, VerdictDigestInvalid, SealDigestInvalid, RecordedBySessionInvalid, CreatedAtInvalid))

  val acceptedRequiresQueueReviewAndFence =
    not(s.accepted)
    or (s.queueIdValid and s.reviewIdValid and s.fenceSeqPositive)

  val acceptedRequiresArtifactAndFindings =
    not(s.accepted)
    or (s.artifactDigestValid and s.findingsDigestValid)

  val acceptedRequiresVerifierSealRecorderAndTimestamp =
    not(s.accepted)
    or (
      s.verifierValid
      and s.verdictDigestValid
      and s.sealDigestValid
      and s.recordedBySessionValid
      and s.createdAtNonNegative
    )

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      queueIdValid: s.queueIdValid,
      artifactDigestValid: s.artifactDigestValid,
      reviewIdValid: s.reviewIdValid,
      findingsDigestValid: s.findingsDigestValid,
      fenceSeqPositive: s.fenceSeqPositive,
      verifierValid: s.verifierValid,
      verdictDigestValid: s.verdictDigestValid,
      sealDigestValid: s.sealDigestValid,
      recordedBySessionValid: s.recordedBySessionValid,
      createdAtNonNegative: s.createdAtNonNegative,
    })

  val proofRowParsingDoesNotScheduleWork =
    not(s.scheduledWork)

  val proofRowParsingDoesNotGrantMutationAuthority =
    not(s.mutationAuthorityGranted)
}

module CoveyLandingAuthorizationRequestShape {
  type Case =
    ValidAuthorization
    | InvalidSessionToken
    | InvalidQueueId
    | InvalidArtifactDigest
    | InvalidReviewId
    | InvalidFindingsDigest
    | NonPositiveFence
    | InvalidVerifier
    | InvalidVerdictDigest
    | InvalidSealDigest

  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | QueueIdInvalid
    | ArtifactDigestInvalid
    | ReviewIdInvalid
    | FindingsDigestInvalid
    | FenceSeqInvalid
    | VerifierInvalid
    | VerdictDigestInvalid
    | SealDigestInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    sessionTokenValid: bool,
    queueIdValid: bool,
    artifactDigestValid: bool,
    reviewIdValid: bool,
    findingsDigestValid: bool,
    fenceSeqPositive: bool,
    verifierValid: bool,
    verdictDigestValid: bool,
    sealDigestValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    sessionTokenValid: case != InvalidSessionToken,
    queueIdValid: case != InvalidQueueId,
    artifactDigestValid: case != InvalidArtifactDigest,
    reviewIdValid: case != InvalidReviewId,
    findingsDigestValid: case != InvalidFindingsDigest,
    fenceSeqPositive: case != NonPositiveFence,
    verifierValid: case != InvalidVerifier,
    verdictDigestValid: case != InvalidVerdictDigest,
    sealDigestValid: case != InvalidSealDigest,
  }

  // Mirrors VerifyLandingAuthorizationReq::try_from_raw_parts validation order.
  def rejectFor(facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.queueIdValid)) QueueIdInvalid
    else if (not(facts.artifactDigestValid)) ArtifactDigestInvalid
    else if (not(facts.reviewIdValid)) ReviewIdInvalid
    else if (not(facts.findingsDigestValid)) FindingsDigestInvalid
    else if (not(facts.fenceSeqPositive)) FenceSeqInvalid
    else if (not(facts.verifierValid)) VerifierInvalid
    else if (not(facts.verdictDigestValid)) VerdictDigestInvalid
    else if (not(facts.sealDigestValid)) SealDigestInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidAuthorization,
      sessionTokenValid: false,
      queueIdValid: false,
      artifactDigestValid: false,
      reviewIdValid: false,
      findingsDigestValid: false,
      fenceSeqPositive: false,
      verifierValid: false,
      verdictDigestValid: false,
      sealDigestValid: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
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
      sessionTokenValid: facts.sessionTokenValid,
      queueIdValid: facts.queueIdValid,
      artifactDigestValid: facts.artifactDigestValid,
      reviewIdValid: facts.reviewIdValid,
      findingsDigestValid: facts.findingsDigestValid,
      fenceSeqPositive: facts.fenceSeqPositive,
      verifierValid: facts.verifierValid,
      verdictDigestValid: facts.verdictDigestValid,
      sealDigestValid: facts.sealDigestValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidAuthorization) },
    all { s.caseIndex == 1, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 2, evaluate(InvalidQueueId) },
    all { s.caseIndex == 3, evaluate(InvalidArtifactDigest) },
    all { s.caseIndex == 4, evaluate(InvalidReviewId) },
    all { s.caseIndex == 5, evaluate(InvalidFindingsDigest) },
    all { s.caseIndex == 6, evaluate(NonPositiveFence) },
    all { s.caseIndex == 7, evaluate(InvalidVerifier) },
    all { s.caseIndex == 8, evaluate(InvalidVerdictDigest) },
    all { s.caseIndex == 9, evaluate(InvalidSealDigest) },
    all { s.caseIndex >= 10, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 10
    and s.case.in(Set(ValidAuthorization, InvalidSessionToken, InvalidQueueId, InvalidArtifactDigest, InvalidReviewId, InvalidFindingsDigest, NonPositiveFence, InvalidVerifier, InvalidVerdictDigest, InvalidSealDigest))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, QueueIdInvalid, ArtifactDigestInvalid, ReviewIdInvalid, FindingsDigestInvalid, FenceSeqInvalid, VerifierInvalid, VerdictDigestInvalid, SealDigestInvalid))

  val acceptedRequiresQueueReviewAndFence =
    not(s.accepted)
    or (
      s.sessionTokenValid
      and s.queueIdValid
      and s.reviewIdValid
      and s.fenceSeqPositive
    )

  val acceptedRequiresArtifactAndFindings =
    not(s.accepted)
    or (s.artifactDigestValid and s.findingsDigestValid)

  val acceptedRequiresVerifierAndDigests =
    not(s.accepted)
    or (s.verifierValid and s.verdictDigestValid and s.sealDigestValid)

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      sessionTokenValid: s.sessionTokenValid,
      queueIdValid: s.queueIdValid,
      artifactDigestValid: s.artifactDigestValid,
      reviewIdValid: s.reviewIdValid,
      findingsDigestValid: s.findingsDigestValid,
      fenceSeqPositive: s.fenceSeqPositive,
      verifierValid: s.verifierValid,
      verdictDigestValid: s.verdictDigestValid,
      sealDigestValid: s.sealDigestValid,
    })
}

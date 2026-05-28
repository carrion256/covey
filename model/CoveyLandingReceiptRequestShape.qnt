module CoveyLandingReceiptRequestShape {
  type Case =
    ValidReceipt
    | InvalidSessionToken
    | InvalidQueueId
    | InvalidArtifactDigest
    | NonPositiveFence
    | InvalidTargetRef
    | InvalidCommitOid

  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | QueueIdInvalid
    | ArtifactDigestInvalid
    | FenceSeqInvalid
    | TargetRefInvalid
    | LandedCommitOidInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    sessionTokenValid: bool,
    queueIdValid: bool,
    artifactDigestValid: bool,
    fenceSeqPositive: bool,
    targetRefValid: bool,
    landedCommitOidValid: bool,
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
    fenceSeqPositive: case != NonPositiveFence,
    targetRefValid: case != InvalidTargetRef,
    landedCommitOidValid: case != InvalidCommitOid,
  }

  // Mirrors RecordLandingReceiptReq::try_from_raw_parts validation order.
  def rejectFor(facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.queueIdValid)) QueueIdInvalid
    else if (not(facts.artifactDigestValid)) ArtifactDigestInvalid
    else if (not(facts.fenceSeqPositive)) FenceSeqInvalid
    else if (not(facts.targetRefValid)) TargetRefInvalid
    else if (not(facts.landedCommitOidValid)) LandedCommitOidInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidReceipt,
      sessionTokenValid: false,
      queueIdValid: false,
      artifactDigestValid: false,
      fenceSeqPositive: false,
      targetRefValid: false,
      landedCommitOidValid: false,
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
      fenceSeqPositive: facts.fenceSeqPositive,
      targetRefValid: facts.targetRefValid,
      landedCommitOidValid: facts.landedCommitOidValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidReceipt) },
    all { s.caseIndex == 1, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 2, evaluate(InvalidQueueId) },
    all { s.caseIndex == 3, evaluate(InvalidArtifactDigest) },
    all { s.caseIndex == 4, evaluate(NonPositiveFence) },
    all { s.caseIndex == 5, evaluate(InvalidTargetRef) },
    all { s.caseIndex == 6, evaluate(InvalidCommitOid) },
    all { s.caseIndex >= 7, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 7
    and s.case.in(Set(ValidReceipt, InvalidSessionToken, InvalidQueueId, InvalidArtifactDigest, NonPositiveFence, InvalidTargetRef, InvalidCommitOid))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, QueueIdInvalid, ArtifactDigestInvalid, FenceSeqInvalid, TargetRefInvalid, LandedCommitOidInvalid))

  val acceptedRequiresQueueArtifactAndFence =
    not(s.accepted)
    or (
      s.sessionTokenValid
      and s.queueIdValid
      and s.artifactDigestValid
      and s.fenceSeqPositive
    )

  val acceptedRequiresLandingIdentity =
    not(s.accepted) or (s.targetRefValid and s.landedCommitOidValid)

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      sessionTokenValid: s.sessionTokenValid,
      queueIdValid: s.queueIdValid,
      artifactDigestValid: s.artifactDigestValid,
      fenceSeqPositive: s.fenceSeqPositive,
      targetRefValid: s.targetRefValid,
      landedCommitOidValid: s.landedCommitOidValid,
    })
}

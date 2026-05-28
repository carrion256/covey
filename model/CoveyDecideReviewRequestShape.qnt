module CoveyDecideReviewRequestShape {
  type Case =
    ValidApprove
    | ValidChangesRequested
    | ValidBlocked
    | InvalidSessionToken
    | InvalidReviewId
    | InvalidClaimId
    | NonPositiveFence
    | InvalidFindingsDigest
    | BlankIdempotencyKey

  type ReviewVerdict = Approve | ChangesRequested | Blocked
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | ReviewIdInvalid
    | ClaimIdInvalid
    | FenceSeqInvalid
    | FindingsDigestInvalid
    | IdempotencyKeyInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    verdict: ReviewVerdict,
    sessionTokenValid: bool,
    reviewIdValid: bool,
    claimIdValid: bool,
    fenceSeqPositive: bool,
    findingsDigestValid: bool,
    idempotencyKeyValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def verdictFor(case) =
    if (case == ValidChangesRequested) ChangesRequested
    else if (case == ValidBlocked) Blocked
    else Approve

  def factsFor(case) = {
    verdict: verdictFor(case),
    sessionTokenValid: case != InvalidSessionToken,
    reviewIdValid: case != InvalidReviewId,
    claimIdValid: case != InvalidClaimId,
    fenceSeqPositive: case != NonPositiveFence,
    findingsDigestValid: case != InvalidFindingsDigest,
    idempotencyKeyValid: case != BlankIdempotencyKey,
  }

  // Mirrors DecideReviewReq::try_from_raw_parts validation order.
  def rejectFor(facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.reviewIdValid)) ReviewIdInvalid
    else if (not(facts.claimIdValid)) ClaimIdInvalid
    else if (not(facts.fenceSeqPositive)) FenceSeqInvalid
    else if (not(facts.findingsDigestValid)) FindingsDigestInvalid
    else if (not(facts.idempotencyKeyValid)) IdempotencyKeyInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidApprove,
      verdict: Approve,
      sessionTokenValid: false,
      reviewIdValid: false,
      claimIdValid: false,
      fenceSeqPositive: false,
      findingsDigestValid: false,
      idempotencyKeyValid: false,
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
      verdict: facts.verdict,
      sessionTokenValid: facts.sessionTokenValid,
      reviewIdValid: facts.reviewIdValid,
      claimIdValid: facts.claimIdValid,
      fenceSeqPositive: facts.fenceSeqPositive,
      findingsDigestValid: facts.findingsDigestValid,
      idempotencyKeyValid: facts.idempotencyKeyValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidApprove) },
    all { s.caseIndex == 1, evaluate(ValidChangesRequested) },
    all { s.caseIndex == 2, evaluate(ValidBlocked) },
    all { s.caseIndex == 3, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 4, evaluate(InvalidReviewId) },
    all { s.caseIndex == 5, evaluate(InvalidClaimId) },
    all { s.caseIndex == 6, evaluate(NonPositiveFence) },
    all { s.caseIndex == 7, evaluate(InvalidFindingsDigest) },
    all { s.caseIndex == 8, evaluate(BlankIdempotencyKey) },
    all { s.caseIndex >= 9, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 9
    and s.case.in(Set(ValidApprove, ValidChangesRequested, ValidBlocked, InvalidSessionToken, InvalidReviewId, InvalidClaimId, NonPositiveFence, InvalidFindingsDigest, BlankIdempotencyKey))
    and s.verdict.in(Set(Approve, ChangesRequested, Blocked))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, ReviewIdInvalid, ClaimIdInvalid, FenceSeqInvalid, FindingsDigestInvalid, IdempotencyKeyInvalid))

  val acceptedRequiresSessionReviewClaimFence =
    not(s.accepted)
    or (
      s.sessionTokenValid
      and s.reviewIdValid
      and s.claimIdValid
      and s.fenceSeqPositive
    )

  val acceptedRequiresFindingsAndIdempotency =
    not(s.accepted)
    or (s.findingsDigestValid and s.idempotencyKeyValid)

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      verdict: s.verdict,
      sessionTokenValid: s.sessionTokenValid,
      reviewIdValid: s.reviewIdValid,
      claimIdValid: s.claimIdValid,
      fenceSeqPositive: s.fenceSeqPositive,
      findingsDigestValid: s.findingsDigestValid,
      idempotencyKeyValid: s.idempotencyKeyValid,
    })
}

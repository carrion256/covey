module CoveyClaimReadyQueueRequestShape {
  type Case =
    ValidClaim
    | InvalidSessionToken
    | ZeroLeaseDuration
    | NegativeLeaseDuration
    | BlankIdempotencyKey

  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | LeaseDurationInvalid
    | IdempotencyKeyInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    sessionTokenValid: bool,
    leaseDurationPositive: bool,
    idempotencyKeyValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    sessionTokenValid: case != InvalidSessionToken,
    leaseDurationPositive: not(case.in(Set(ZeroLeaseDuration, NegativeLeaseDuration))),
    idempotencyKeyValid: case != BlankIdempotencyKey,
  }

  // Mirrors ClaimReadyQueueReq::try_from_raw_parts validation order.
  def rejectFor(facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.leaseDurationPositive)) LeaseDurationInvalid
    else if (not(facts.idempotencyKeyValid)) IdempotencyKeyInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidClaim,
      sessionTokenValid: false,
      leaseDurationPositive: false,
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
      sessionTokenValid: facts.sessionTokenValid,
      leaseDurationPositive: facts.leaseDurationPositive,
      idempotencyKeyValid: facts.idempotencyKeyValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidClaim) },
    all { s.caseIndex == 1, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 2, evaluate(ZeroLeaseDuration) },
    all { s.caseIndex == 3, evaluate(NegativeLeaseDuration) },
    all { s.caseIndex == 4, evaluate(BlankIdempotencyKey) },
    all { s.caseIndex >= 5, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 5
    and s.case.in(Set(ValidClaim, InvalidSessionToken, ZeroLeaseDuration, NegativeLeaseDuration, BlankIdempotencyKey))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, LeaseDurationInvalid, IdempotencyKeyInvalid))

  val acceptedRequiresSessionLeaseAndIdempotency =
    not(s.accepted)
    or (s.sessionTokenValid and s.leaseDurationPositive and s.idempotencyKeyValid)

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      sessionTokenValid: s.sessionTokenValid,
      leaseDurationPositive: s.leaseDurationPositive,
      idempotencyKeyValid: s.idempotencyKeyValid,
    })
}

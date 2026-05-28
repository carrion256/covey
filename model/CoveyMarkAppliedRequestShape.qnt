module CoveyMarkAppliedRequestShape {
  type Case =
    ValidMarkApplied
    | InvalidSessionToken
    | InvalidQueueId
    | ZeroFence
    | NegativeFence
    | BlankIdempotencyKey

  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | QueueIdInvalid
    | FenceSeqInvalid
    | IdempotencyKeyInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    sessionTokenValid: bool,
    queueIdValid: bool,
    fenceSeqPositive: bool,
    idempotencyKeyValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    sessionTokenValid: case != InvalidSessionToken,
    queueIdValid: case != InvalidQueueId,
    fenceSeqPositive: not(case.in(Set(ZeroFence, NegativeFence))),
    idempotencyKeyValid: case != BlankIdempotencyKey,
  }

  // Mirrors MarkAppliedReq::try_from_raw_parts validation order.
  def rejectFor(facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.queueIdValid)) QueueIdInvalid
    else if (not(facts.fenceSeqPositive)) FenceSeqInvalid
    else if (not(facts.idempotencyKeyValid)) IdempotencyKeyInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidMarkApplied,
      sessionTokenValid: false,
      queueIdValid: false,
      fenceSeqPositive: false,
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
      queueIdValid: facts.queueIdValid,
      fenceSeqPositive: facts.fenceSeqPositive,
      idempotencyKeyValid: facts.idempotencyKeyValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidMarkApplied) },
    all { s.caseIndex == 1, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 2, evaluate(InvalidQueueId) },
    all { s.caseIndex == 3, evaluate(ZeroFence) },
    all { s.caseIndex == 4, evaluate(NegativeFence) },
    all { s.caseIndex == 5, evaluate(BlankIdempotencyKey) },
    all { s.caseIndex >= 6, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 6
    and s.case.in(Set(ValidMarkApplied, InvalidSessionToken, InvalidQueueId, ZeroFence, NegativeFence, BlankIdempotencyKey))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, QueueIdInvalid, FenceSeqInvalid, IdempotencyKeyInvalid))

  val acceptedRequiresSessionQueueFenceAndIdempotency =
    not(s.accepted)
    or (
      s.sessionTokenValid
      and s.queueIdValid
      and s.fenceSeqPositive
      and s.idempotencyKeyValid
    )

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      sessionTokenValid: s.sessionTokenValid,
      queueIdValid: s.queueIdValid,
      fenceSeqPositive: s.fenceSeqPositive,
      idempotencyKeyValid: s.idempotencyKeyValid,
    })
}

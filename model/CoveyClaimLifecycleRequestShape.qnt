module CoveyClaimLifecycleRequestShape {
  type Operation = StartSubtask | AbandonSubtask | ReleaseClaim | RenewClaim
  type Case =
    ValidStart
    | ValidAbandon
    | ValidRelease
    | ValidRenew
    | InvalidSessionToken
    | InvalidClaimId
    | ZeroFence
    | NegativeFence
    | ZeroRenewExtension
    | NegativeRenewExtension
    | BlankIdempotencyKey

  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | ClaimIdInvalid
    | FenceSeqInvalid
    | LeaseDurationInvalid
    | IdempotencyKeyInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    operation: Operation,
    sessionTokenValid: bool,
    claimIdValid: bool,
    fenceSeqPositive: bool,
    extendDurationPositive: bool,
    idempotencyKeyValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def operationFor(case) =
    if (case == ValidAbandon) AbandonSubtask
    else if (case == ValidRelease) ReleaseClaim
    else if (case.in(Set(ValidRenew, ZeroRenewExtension, NegativeRenewExtension))) RenewClaim
    else StartSubtask

  def factsFor(case) = {
    sessionTokenValid: case != InvalidSessionToken,
    claimIdValid: case != InvalidClaimId,
    fenceSeqPositive: not(case.in(Set(ZeroFence, NegativeFence))),
    extendDurationPositive: not(case.in(Set(ZeroRenewExtension, NegativeRenewExtension))),
    idempotencyKeyValid: case != BlankIdempotencyKey,
  }

  // Mirrors StartSubtaskReq, AbandonSubtaskReq, ReleaseClaimReq, and
  // RenewClaimReq::try_from_raw_parts validation order.
  def rejectFor(operation, facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.claimIdValid)) ClaimIdInvalid
    else if (not(facts.fenceSeqPositive)) FenceSeqInvalid
    else if (operation == RenewClaim and not(facts.extendDurationPositive)) LeaseDurationInvalid
    else if (not(facts.idempotencyKeyValid)) IdempotencyKeyInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidStart,
      operation: StartSubtask,
      sessionTokenValid: false,
      claimIdValid: false,
      fenceSeqPositive: false,
      extendDurationPositive: false,
      idempotencyKeyValid: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val operation = operationFor(case)
    val facts = factsFor(case)
    val rejectReason = rejectFor(operation, facts)
    val accepted = rejectReason == NoReject
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      operation: operation,
      sessionTokenValid: facts.sessionTokenValid,
      claimIdValid: facts.claimIdValid,
      fenceSeqPositive: facts.fenceSeqPositive,
      extendDurationPositive: facts.extendDurationPositive,
      idempotencyKeyValid: facts.idempotencyKeyValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidStart) },
    all { s.caseIndex == 1, evaluate(ValidAbandon) },
    all { s.caseIndex == 2, evaluate(ValidRelease) },
    all { s.caseIndex == 3, evaluate(ValidRenew) },
    all { s.caseIndex == 4, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 5, evaluate(InvalidClaimId) },
    all { s.caseIndex == 6, evaluate(ZeroFence) },
    all { s.caseIndex == 7, evaluate(NegativeFence) },
    all { s.caseIndex == 8, evaluate(ZeroRenewExtension) },
    all { s.caseIndex == 9, evaluate(NegativeRenewExtension) },
    all { s.caseIndex == 10, evaluate(BlankIdempotencyKey) },
    all { s.caseIndex >= 11, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 11
    and s.case.in(Set(ValidStart, ValidAbandon, ValidRelease, ValidRenew, InvalidSessionToken, InvalidClaimId, ZeroFence, NegativeFence, ZeroRenewExtension, NegativeRenewExtension, BlankIdempotencyKey))
    and s.operation.in(Set(StartSubtask, AbandonSubtask, ReleaseClaim, RenewClaim))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, ClaimIdInvalid, FenceSeqInvalid, LeaseDurationInvalid, IdempotencyKeyInvalid))

  val acceptedRequiresSessionClaimFenceAndIdempotency =
    not(s.accepted)
    or (
      s.sessionTokenValid
      and s.claimIdValid
      and s.fenceSeqPositive
      and s.idempotencyKeyValid
    )

  val renewAcceptedRequiresPositiveExtension =
    not(s.accepted and s.operation == RenewClaim) or s.extendDurationPositive

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor(
      s.operation,
      {
        sessionTokenValid: s.sessionTokenValid,
        claimIdValid: s.claimIdValid,
        fenceSeqPositive: s.fenceSeqPositive,
        extendDurationPositive: s.extendDurationPositive,
        idempotencyKeyValid: s.idempotencyKeyValid,
      }
    )
}

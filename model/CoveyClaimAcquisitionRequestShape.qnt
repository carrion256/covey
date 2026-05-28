module CoveyClaimAcquisitionRequestShape {
  type Operation = ClaimNext | ClaimNextScoped | ClaimSubtask
  type Case =
    ValidClaimNext
    | ValidClaimNextScoped
    | ValidClaimSubtask
    | ClaimNextInvalidSessionToken
    | ClaimNextZeroLease
    | ClaimNextNegativeLease
    | ClaimNextBlankIdempotency
    | ClaimNextScopedInvalidMetaTaskId
    | ClaimSubtaskInvalidSessionToken
    | ClaimSubtaskInvalidSubtaskId
    | ClaimSubtaskZeroLease
    | ClaimSubtaskNegativeLease
    | ClaimSubtaskBlankIdempotency

  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | SubtaskIdInvalid
    | LeaseDurationInvalid
    | MetaTaskIdInvalid
    | IdempotencyKeyInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    operation: Operation,
    sessionTokenValid: bool,
    leaseDurationPositive: bool,
    metaTaskIdPresent: bool,
    metaTaskIdValid: bool,
    subtaskIdValid: bool,
    idempotencyKeyValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def operationFor(case) =
    if (case.in(Set(ValidClaimNextScoped, ClaimNextScopedInvalidMetaTaskId))) ClaimNextScoped
    else if (case.in(Set(ValidClaimSubtask, ClaimSubtaskInvalidSessionToken, ClaimSubtaskInvalidSubtaskId, ClaimSubtaskZeroLease, ClaimSubtaskNegativeLease, ClaimSubtaskBlankIdempotency))) ClaimSubtask
    else ClaimNext

  def factsFor(case) = {
    sessionTokenValid: not(case.in(Set(ClaimNextInvalidSessionToken, ClaimSubtaskInvalidSessionToken))),
    leaseDurationPositive: not(case.in(Set(ClaimNextZeroLease, ClaimNextNegativeLease, ClaimSubtaskZeroLease, ClaimSubtaskNegativeLease))),
    metaTaskIdPresent: case.in(Set(ValidClaimNextScoped, ClaimNextScopedInvalidMetaTaskId)),
    metaTaskIdValid: case != ClaimNextScopedInvalidMetaTaskId,
    subtaskIdValid: case != ClaimSubtaskInvalidSubtaskId,
    idempotencyKeyValid: not(case.in(Set(ClaimNextBlankIdempotency, ClaimSubtaskBlankIdempotency))),
  }

  // Mirrors ClaimNextReq::try_from_raw_parts[_scoped] and
  // ClaimSubtaskReq::try_from_raw_parts validation order.
  def rejectFor(operation, facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (operation == ClaimSubtask and not(facts.subtaskIdValid)) SubtaskIdInvalid
    else if (not(facts.leaseDurationPositive)) LeaseDurationInvalid
    else if (operation == ClaimNextScoped and not(facts.metaTaskIdValid)) MetaTaskIdInvalid
    else if (not(facts.idempotencyKeyValid)) IdempotencyKeyInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidClaimNext,
      operation: ClaimNext,
      sessionTokenValid: false,
      leaseDurationPositive: false,
      metaTaskIdPresent: false,
      metaTaskIdValid: false,
      subtaskIdValid: false,
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
      leaseDurationPositive: facts.leaseDurationPositive,
      metaTaskIdPresent: facts.metaTaskIdPresent,
      metaTaskIdValid: facts.metaTaskIdValid,
      subtaskIdValid: facts.subtaskIdValid,
      idempotencyKeyValid: facts.idempotencyKeyValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidClaimNext) },
    all { s.caseIndex == 1, evaluate(ValidClaimNextScoped) },
    all { s.caseIndex == 2, evaluate(ValidClaimSubtask) },
    all { s.caseIndex == 3, evaluate(ClaimNextInvalidSessionToken) },
    all { s.caseIndex == 4, evaluate(ClaimNextZeroLease) },
    all { s.caseIndex == 5, evaluate(ClaimNextNegativeLease) },
    all { s.caseIndex == 6, evaluate(ClaimNextBlankIdempotency) },
    all { s.caseIndex == 7, evaluate(ClaimNextScopedInvalidMetaTaskId) },
    all { s.caseIndex == 8, evaluate(ClaimSubtaskInvalidSessionToken) },
    all { s.caseIndex == 9, evaluate(ClaimSubtaskInvalidSubtaskId) },
    all { s.caseIndex == 10, evaluate(ClaimSubtaskZeroLease) },
    all { s.caseIndex == 11, evaluate(ClaimSubtaskNegativeLease) },
    all { s.caseIndex == 12, evaluate(ClaimSubtaskBlankIdempotency) },
    all { s.caseIndex >= 13, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 13
    and s.case.in(Set(ValidClaimNext, ValidClaimNextScoped, ValidClaimSubtask, ClaimNextInvalidSessionToken, ClaimNextZeroLease, ClaimNextNegativeLease, ClaimNextBlankIdempotency, ClaimNextScopedInvalidMetaTaskId, ClaimSubtaskInvalidSessionToken, ClaimSubtaskInvalidSubtaskId, ClaimSubtaskZeroLease, ClaimSubtaskNegativeLease, ClaimSubtaskBlankIdempotency))
    and s.operation.in(Set(ClaimNext, ClaimNextScoped, ClaimSubtask))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, SubtaskIdInvalid, LeaseDurationInvalid, MetaTaskIdInvalid, IdempotencyKeyInvalid))

  val acceptedRequiresSessionLeaseAndIdempotency =
    not(s.accepted)
    or (s.sessionTokenValid and s.leaseDurationPositive and s.idempotencyKeyValid)

  val acceptedTargetedRequiresSubtask =
    not(s.accepted and s.operation == ClaimSubtask) or s.subtaskIdValid

  val acceptedScopedNextRequiresMetaTask =
    not(s.accepted and s.operation == ClaimNextScoped) or (s.metaTaskIdPresent and s.metaTaskIdValid)

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor(
      s.operation,
      {
        sessionTokenValid: s.sessionTokenValid,
        leaseDurationPositive: s.leaseDurationPositive,
        metaTaskIdValid: s.metaTaskIdValid,
        subtaskIdValid: s.subtaskIdValid,
        idempotencyKeyValid: s.idempotencyKeyValid,
      }
    )
}

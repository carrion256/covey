module CoveyReservationLifecycleRequestShape {
  type Operation = ReleaseReservation | RenewReservation
  type Case =
    ValidRelease
    | ValidRenew
    | InvalidSessionToken
    | InvalidReservationId
    | ZeroRenewExtension
    | NegativeRenewExtension
    | BlankIdempotencyKey

  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | ReservationIdInvalid
    | LeaseDurationInvalid
    | IdempotencyKeyInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    operation: Operation,
    sessionTokenValid: bool,
    reservationIdValid: bool,
    extendDurationPositive: bool,
    idempotencyKeyValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def operationFor(case) =
    if (case.in(Set(ValidRenew, ZeroRenewExtension, NegativeRenewExtension))) RenewReservation
    else ReleaseReservation

  def factsFor(case) = {
    sessionTokenValid: case != InvalidSessionToken,
    reservationIdValid: case != InvalidReservationId,
    extendDurationPositive: not(case.in(Set(ZeroRenewExtension, NegativeRenewExtension))),
    idempotencyKeyValid: case != BlankIdempotencyKey,
  }

  // Mirrors ReleaseReservationReq and RenewReservationReq validation order.
  def rejectFor(operation, facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.reservationIdValid)) ReservationIdInvalid
    else if (operation == RenewReservation and not(facts.extendDurationPositive)) LeaseDurationInvalid
    else if (not(facts.idempotencyKeyValid)) IdempotencyKeyInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidRelease,
      operation: ReleaseReservation,
      sessionTokenValid: false,
      reservationIdValid: false,
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
      reservationIdValid: facts.reservationIdValid,
      extendDurationPositive: facts.extendDurationPositive,
      idempotencyKeyValid: facts.idempotencyKeyValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidRelease) },
    all { s.caseIndex == 1, evaluate(ValidRenew) },
    all { s.caseIndex == 2, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 3, evaluate(InvalidReservationId) },
    all { s.caseIndex == 4, evaluate(ZeroRenewExtension) },
    all { s.caseIndex == 5, evaluate(NegativeRenewExtension) },
    all { s.caseIndex == 6, evaluate(BlankIdempotencyKey) },
    all { s.caseIndex >= 7, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 7
    and s.case.in(Set(ValidRelease, ValidRenew, InvalidSessionToken, InvalidReservationId, ZeroRenewExtension, NegativeRenewExtension, BlankIdempotencyKey))
    and s.operation.in(Set(ReleaseReservation, RenewReservation))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, ReservationIdInvalid, LeaseDurationInvalid, IdempotencyKeyInvalid))

  val acceptedRequiresSessionReservationAndIdempotency =
    not(s.accepted) or (s.sessionTokenValid and s.reservationIdValid and s.idempotencyKeyValid)

  val renewAcceptedRequiresPositiveExtension =
    not(s.accepted and s.operation == RenewReservation) or s.extendDurationPositive

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor(
      s.operation,
      {
        sessionTokenValid: s.sessionTokenValid,
        reservationIdValid: s.reservationIdValid,
        extendDurationPositive: s.extendDurationPositive,
        idempotencyKeyValid: s.idempotencyKeyValid,
      }
    )
}

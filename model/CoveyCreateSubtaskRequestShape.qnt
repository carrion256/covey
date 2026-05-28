module CoveyCreateSubtaskRequestShape {
  type Case =
    ValidWithoutSubtaskId
    | ValidWithSubtaskId
    | InvalidSessionToken
    | InvalidMetaTaskId
    | InvalidSubtaskId
    | BlankTitle
    | PaddedTitle
    | NegativePriority
    | PriorityTooHigh
    | BlankIdempotencyKey

  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | MetaTaskIdInvalid
    | SubtaskIdInvalid
    | SubtaskTitleInvalid
    | PriorityInvalid
    | IdempotencyKeyInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    sessionTokenValid: bool,
    metaTaskIdValid: bool,
    optionalSubtaskIdPresent: bool,
    optionalSubtaskIdValid: bool,
    titleValid: bool,
    priorityValid: bool,
    idempotencyKeyValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    sessionTokenValid: case != InvalidSessionToken,
    metaTaskIdValid: case != InvalidMetaTaskId,
    optionalSubtaskIdPresent: case.in(Set(ValidWithSubtaskId, InvalidSubtaskId)),
    optionalSubtaskIdValid: case != InvalidSubtaskId,
    titleValid: not(case.in(Set(BlankTitle, PaddedTitle))),
    priorityValid: not(case.in(Set(NegativePriority, PriorityTooHigh))),
    idempotencyKeyValid: case != BlankIdempotencyKey,
  }

  // Mirrors CreateSubtaskRequest::try_from_raw_parts validation order.
  def rejectFor(facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.metaTaskIdValid)) MetaTaskIdInvalid
    else if (not(facts.optionalSubtaskIdValid)) SubtaskIdInvalid
    else if (not(facts.titleValid)) SubtaskTitleInvalid
    else if (not(facts.priorityValid)) PriorityInvalid
    else if (not(facts.idempotencyKeyValid)) IdempotencyKeyInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidWithoutSubtaskId,
      sessionTokenValid: false,
      metaTaskIdValid: false,
      optionalSubtaskIdPresent: false,
      optionalSubtaskIdValid: false,
      titleValid: false,
      priorityValid: false,
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
      metaTaskIdValid: facts.metaTaskIdValid,
      optionalSubtaskIdPresent: facts.optionalSubtaskIdPresent,
      optionalSubtaskIdValid: facts.optionalSubtaskIdValid,
      titleValid: facts.titleValid,
      priorityValid: facts.priorityValid,
      idempotencyKeyValid: facts.idempotencyKeyValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidWithoutSubtaskId) },
    all { s.caseIndex == 1, evaluate(ValidWithSubtaskId) },
    all { s.caseIndex == 2, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 3, evaluate(InvalidMetaTaskId) },
    all { s.caseIndex == 4, evaluate(InvalidSubtaskId) },
    all { s.caseIndex == 5, evaluate(BlankTitle) },
    all { s.caseIndex == 6, evaluate(PaddedTitle) },
    all { s.caseIndex == 7, evaluate(NegativePriority) },
    all { s.caseIndex == 8, evaluate(PriorityTooHigh) },
    all { s.caseIndex == 9, evaluate(BlankIdempotencyKey) },
    all { s.caseIndex >= 10, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 10
    and s.case.in(Set(ValidWithoutSubtaskId, ValidWithSubtaskId, InvalidSessionToken, InvalidMetaTaskId, InvalidSubtaskId, BlankTitle, PaddedTitle, NegativePriority, PriorityTooHigh, BlankIdempotencyKey))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, MetaTaskIdInvalid, SubtaskIdInvalid, SubtaskTitleInvalid, PriorityInvalid, IdempotencyKeyInvalid))

  val acceptedRequiresSessionMetaAndIdempotency =
    not(s.accepted)
    or (s.sessionTokenValid and s.metaTaskIdValid and s.idempotencyKeyValid)

  val acceptedRequiresSubtaskPayload =
    not(s.accepted) or (s.optionalSubtaskIdValid and s.titleValid and s.priorityValid)

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      sessionTokenValid: s.sessionTokenValid,
      metaTaskIdValid: s.metaTaskIdValid,
      optionalSubtaskIdValid: s.optionalSubtaskIdValid,
      titleValid: s.titleValid,
      priorityValid: s.priorityValid,
      idempotencyKeyValid: s.idempotencyKeyValid,
    })
}

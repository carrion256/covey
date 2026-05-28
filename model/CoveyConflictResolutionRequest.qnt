module CoveyConflictResolutionRequest {
  type Case =
    ValidAcknowledgeOpen
    | ValidResolveOpen
    | ValidAcknowledgeAcknowledged
    | ValidResolveAcknowledged
    | ValidResolveResolved
    | InvalidSessionToken
    | InvalidConflictId
    | OpenResolutionRequest
    | BlankIdempotencyKey
    | DowngradeResolvedToAcknowledged

  type ConflictState = Open | Acknowledged | Resolved
  type Outcome = NotEvaluated | Accepted | Rejected | IllegalTransition
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | ConflictIdInvalid
    | OpenResolutionForbidden
    | IdempotencyKeyInvalid
    | ResolutionDowngradeForbidden

  type Model = {
    caseIndex: int,
    case: Case,
    currentState: ConflictState,
    requestedState: ConflictState,
    sessionTokenValid: bool,
    conflictIdValid: bool,
    idempotencyKeyValid: bool,
    requestShapeAccepted: bool,
    transitionAllowed: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    currentState:
      if (case.in(Set(ValidAcknowledgeAcknowledged, ValidResolveAcknowledged))) Acknowledged
      else if (case.in(Set(ValidResolveResolved, DowngradeResolvedToAcknowledged))) Resolved
      else Open,
    requestedState:
      if (case.in(Set(ValidAcknowledgeOpen, ValidAcknowledgeAcknowledged, DowngradeResolvedToAcknowledged))) Acknowledged
      else if (case == OpenResolutionRequest) Open
      else Resolved,
    sessionTokenValid: case != InvalidSessionToken,
    conflictIdValid: case != InvalidConflictId,
    idempotencyKeyValid: case != BlankIdempotencyKey,
  }

  def requestRejectFor(facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.conflictIdValid)) ConflictIdInvalid
    else if (facts.requestedState == Open) OpenResolutionForbidden
    else if (not(facts.idempotencyKeyValid)) IdempotencyKeyInvalid
    else NoReject

  def transitionAllowedFor(facts) =
    (
      facts.currentState == Open
      and facts.requestedState.in(Set(Acknowledged, Resolved))
    )
    or (
      facts.currentState == Acknowledged
      and facts.requestedState.in(Set(Acknowledged, Resolved))
    )
    or (facts.currentState == Resolved and facts.requestedState == Resolved)

  def outcomeFor(requestReject, transitionAllowed) =
    if (requestReject != NoReject) Rejected
    else if (not(transitionAllowed)) IllegalTransition
    else Accepted

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidAcknowledgeOpen,
      currentState: Open,
      requestedState: Open,
      sessionTokenValid: false,
      conflictIdValid: false,
      idempotencyKeyValid: false,
      requestShapeAccepted: false,
      transitionAllowed: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val facts = factsFor(case)
    val requestReject = requestRejectFor(facts)
    val transitionAllowed = transitionAllowedFor(facts)
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      currentState: facts.currentState,
      requestedState: facts.requestedState,
      sessionTokenValid: facts.sessionTokenValid,
      conflictIdValid: facts.conflictIdValid,
      idempotencyKeyValid: facts.idempotencyKeyValid,
      requestShapeAccepted: requestReject == NoReject,
      transitionAllowed: transitionAllowed,
      outcome: outcomeFor(requestReject, transitionAllowed),
      rejectReason:
        if (requestReject != NoReject) requestReject
        else if (not(transitionAllowed)) ResolutionDowngradeForbidden
        else NoReject,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidAcknowledgeOpen) },
    all { s.caseIndex == 1, evaluate(ValidResolveOpen) },
    all { s.caseIndex == 2, evaluate(ValidAcknowledgeAcknowledged) },
    all { s.caseIndex == 3, evaluate(ValidResolveAcknowledged) },
    all { s.caseIndex == 4, evaluate(ValidResolveResolved) },
    all { s.caseIndex == 5, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 6, evaluate(InvalidConflictId) },
    all { s.caseIndex == 7, evaluate(OpenResolutionRequest) },
    all { s.caseIndex == 8, evaluate(BlankIdempotencyKey) },
    all { s.caseIndex == 9, evaluate(DowngradeResolvedToAcknowledged) },
    all { s.caseIndex >= 10, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 10
    and s.case.in(Set(ValidAcknowledgeOpen, ValidResolveOpen, ValidAcknowledgeAcknowledged, ValidResolveAcknowledged, ValidResolveResolved, InvalidSessionToken, InvalidConflictId, OpenResolutionRequest, BlankIdempotencyKey, DowngradeResolvedToAcknowledged))
    and s.currentState.in(Set(Open, Acknowledged, Resolved))
    and s.requestedState.in(Set(Open, Acknowledged, Resolved))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected, IllegalTransition))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, ConflictIdInvalid, OpenResolutionForbidden, IdempotencyKeyInvalid, ResolutionDowngradeForbidden))

  val acceptedRequiresValidRequestShape =
    s.outcome != Accepted
    or (
      s.sessionTokenValid
      and s.conflictIdValid
      and s.idempotencyKeyValid
      and s.requestedState != Open
      and s.requestShapeAccepted
    )

  val acceptedRequiresAllowedTransition =
    s.outcome != Accepted or s.transitionAllowed

  val requestCannotSetOpenResolution =
    s.requestShapeAccepted or s.requestedState == Open or s.rejectReason != NoReject

  val resolvedConflictCannotDowngrade =
    not(s.currentState == Resolved and s.requestedState == Acknowledged)
    or s.outcome == IllegalTransition

  val idempotentAcknowledgedAndResolvedAreAllowed =
    not(s.evaluated and s.currentState == s.requestedState and s.requestedState != Open)
    or s.outcome == Accepted

  val rejectedIffRequestShapeInvalid =
    s.outcome == Rejected == (s.evaluated and not(s.requestShapeAccepted))

  val illegalTransitionRequiresValidRequest =
    s.outcome != IllegalTransition or (s.requestShapeAccepted and not(s.transitionAllowed))
}

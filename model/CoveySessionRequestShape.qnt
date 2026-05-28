module CoveySessionRequestShape {
  type Operation = RegisterSession | Heartbeat | ExitSession
  type Case =
    ValidRegister
    | ValidHeartbeat
    | ValidExit
    | RegisterInvalidPrincipal
    | RegisterInvalidInstance
    | RegisterBlankIdempotency
    | HeartbeatInvalidSessionToken
    | HeartbeatBlankIdempotency
    | ExitInvalidSessionToken
    | ExitBlankIdempotency

  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | AgentPrincipalInvalid
    | AgentInstanceInvalid
    | SessionTokenInvalid
    | IdempotencyKeyInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    operation: Operation,
    agentPrincipalValid: bool,
    agentInstanceValid: bool,
    sessionTokenValid: bool,
    idempotencyKeyValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def operationFor(case) =
    if (case.in(Set(ValidRegister, RegisterInvalidPrincipal, RegisterInvalidInstance, RegisterBlankIdempotency))) RegisterSession
    else if (case.in(Set(ValidHeartbeat, HeartbeatInvalidSessionToken, HeartbeatBlankIdempotency))) Heartbeat
    else ExitSession

  def factsFor(case) = {
    agentPrincipalValid: case != RegisterInvalidPrincipal,
    agentInstanceValid: case != RegisterInvalidInstance,
    sessionTokenValid: not(case.in(Set(HeartbeatInvalidSessionToken, ExitInvalidSessionToken))),
    idempotencyKeyValid: not(case.in(Set(RegisterBlankIdempotency, HeartbeatBlankIdempotency, ExitBlankIdempotency))),
  }

  // Mirrors RegisterSessionReq, HeartbeatReq, and ExitSessionReq validation order.
  def rejectFor(operation, facts) =
    if (operation == RegisterSession and not(facts.agentPrincipalValid)) AgentPrincipalInvalid
    else if (operation == RegisterSession and not(facts.agentInstanceValid)) AgentInstanceInvalid
    else if (operation != RegisterSession and not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.idempotencyKeyValid)) IdempotencyKeyInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidRegister,
      operation: RegisterSession,
      agentPrincipalValid: false,
      agentInstanceValid: false,
      sessionTokenValid: false,
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
      agentPrincipalValid: facts.agentPrincipalValid,
      agentInstanceValid: facts.agentInstanceValid,
      sessionTokenValid: facts.sessionTokenValid,
      idempotencyKeyValid: facts.idempotencyKeyValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidRegister) },
    all { s.caseIndex == 1, evaluate(ValidHeartbeat) },
    all { s.caseIndex == 2, evaluate(ValidExit) },
    all { s.caseIndex == 3, evaluate(RegisterInvalidPrincipal) },
    all { s.caseIndex == 4, evaluate(RegisterInvalidInstance) },
    all { s.caseIndex == 5, evaluate(RegisterBlankIdempotency) },
    all { s.caseIndex == 6, evaluate(HeartbeatInvalidSessionToken) },
    all { s.caseIndex == 7, evaluate(HeartbeatBlankIdempotency) },
    all { s.caseIndex == 8, evaluate(ExitInvalidSessionToken) },
    all { s.caseIndex == 9, evaluate(ExitBlankIdempotency) },
    all { s.caseIndex >= 10, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 10
    and s.case.in(Set(ValidRegister, ValidHeartbeat, ValidExit, RegisterInvalidPrincipal, RegisterInvalidInstance, RegisterBlankIdempotency, HeartbeatInvalidSessionToken, HeartbeatBlankIdempotency, ExitInvalidSessionToken, ExitBlankIdempotency))
    and s.operation.in(Set(RegisterSession, Heartbeat, ExitSession))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, AgentPrincipalInvalid, AgentInstanceInvalid, SessionTokenInvalid, IdempotencyKeyInvalid))

  val acceptedRequiresOperationIdentity =
    not(s.accepted)
    or (
      if (s.operation == RegisterSession) s.agentPrincipalValid and s.agentInstanceValid
      else s.sessionTokenValid
    )

  val acceptedRequiresIdempotencyKey =
    not(s.accepted) or s.idempotencyKeyValid

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor(
      s.operation,
      {
        agentPrincipalValid: s.agentPrincipalValid,
        agentInstanceValid: s.agentInstanceValid,
        sessionTokenValid: s.sessionTokenValid,
        idempotencyKeyValid: s.idempotencyKeyValid,
      }
    )
}

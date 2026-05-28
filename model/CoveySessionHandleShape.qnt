module CoveySessionHandleShape {
  type Case =
    ValidExecutorHandle
    | ValidReviewerHandle
    | ValidOrchestratorHandle
    | InvalidSessionToken
    | InvalidAgentPrincipal
    | InvalidAgentInstance

  type Role = Executor | Reviewer | Orchestrator
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | AgentPrincipalInvalid
    | AgentInstanceInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    role: Role,
    sessionTokenValid: bool,
    agentPrincipalValid: bool,
    agentInstanceValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def roleFor(case) =
    if (case == ValidReviewerHandle) Reviewer
    else if (case == ValidOrchestratorHandle) Orchestrator
    else Executor

  def factsFor(case) = {
    sessionTokenValid: case != InvalidSessionToken,
    agentPrincipalValid: case != InvalidAgentPrincipal,
    agentInstanceValid: case != InvalidAgentInstance,
  }

  // Mirrors SessionHandle::try_from_raw_parts validation order.
  def rejectFor(facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.agentPrincipalValid)) AgentPrincipalInvalid
    else if (not(facts.agentInstanceValid)) AgentInstanceInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidExecutorHandle,
      role: Executor,
      sessionTokenValid: false,
      agentPrincipalValid: false,
      agentInstanceValid: false,
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
      role: roleFor(case),
      sessionTokenValid: facts.sessionTokenValid,
      agentPrincipalValid: facts.agentPrincipalValid,
      agentInstanceValid: facts.agentInstanceValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidExecutorHandle) },
    all { s.caseIndex == 1, evaluate(ValidReviewerHandle) },
    all { s.caseIndex == 2, evaluate(ValidOrchestratorHandle) },
    all { s.caseIndex == 3, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 4, evaluate(InvalidAgentPrincipal) },
    all { s.caseIndex == 5, evaluate(InvalidAgentInstance) },
    all { s.caseIndex >= 6, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 6
    and s.case.in(Set(ValidExecutorHandle, ValidReviewerHandle, ValidOrchestratorHandle, InvalidSessionToken, InvalidAgentPrincipal, InvalidAgentInstance))
    and s.role.in(Set(Executor, Reviewer, Orchestrator))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, AgentPrincipalInvalid, AgentInstanceInvalid))

  val acceptedRequiresIdentity =
    not(s.accepted) or (s.sessionTokenValid and s.agentPrincipalValid and s.agentInstanceValid)

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      sessionTokenValid: s.sessionTokenValid,
      agentPrincipalValid: s.agentPrincipalValid,
      agentInstanceValid: s.agentInstanceValid,
    })
}

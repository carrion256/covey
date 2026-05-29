module CoveySessionProofRowShape {
  type Case =
    ValidExecutorActive
    | ValidReviewerStale
    | ValidApplyGateExited
    | InvalidSessionToken
    | InvalidAgentPrincipal
    | InvalidAgentInstance
    | InvalidRole
    | InvalidState

  type RoleShape = ValidRole | InvalidRoleShape
  type StateShape = ValidState | InvalidStateShape
  type RoleValue = ExecutorRole | ReviewerRole | ApplyGateRole | InvalidRoleValue
  type SessionStateValue = ActiveState | StaleState | ExitedState | InvalidStateValue
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | InvalidRoleReject
    | InvalidStateReject
    | SessionTokenInvalid
    | AgentPrincipalInvalid
    | AgentInstanceInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    sessionTokenValid: bool,
    agentPrincipalValid: bool,
    agentInstanceValid: bool,
    roleShape: RoleShape,
    stateShape: StateShape,
    roleValue: RoleValue,
    sessionStateValue: SessionStateValue,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    scheduledWork: bool,
    mutationAuthorityGranted: bool,
    evaluated: bool,
  }

  var s: Model

  def roleShapeFor(case) =
    if (case == InvalidRole) InvalidRoleShape else ValidRole

  def stateShapeFor(case) =
    if (case == InvalidState) InvalidStateShape else ValidState

  def roleValueFor(case) =
    if (case == ValidReviewerStale) ReviewerRole
    else if (case == ValidApplyGateExited) ApplyGateRole
    else if (case == InvalidRole) InvalidRoleValue
    else ExecutorRole

  def sessionStateValueFor(case) =
    if (case == ValidReviewerStale) StaleState
    else if (case == ValidApplyGateExited) ExitedState
    else if (case == InvalidState) InvalidStateValue
    else ActiveState

  def factsFor(case) = {
    sessionTokenValid: case != InvalidSessionToken,
    agentPrincipalValid: case != InvalidAgentPrincipal,
    agentInstanceValid: case != InvalidAgentInstance,
    roleShape: roleShapeFor(case),
    stateShape: stateShapeFor(case),
    roleValue: roleValueFor(case),
    sessionStateValue: sessionStateValueFor(case),
  }

  // Mirrors session_from_raw_row_parts: role/state parse before typed identity.
  def rejectFor(facts) =
    if (facts.roleShape == InvalidRoleShape) InvalidRoleReject
    else if (facts.stateShape == InvalidStateShape) InvalidStateReject
    else if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.agentPrincipalValid)) AgentPrincipalInvalid
    else if (not(facts.agentInstanceValid)) AgentInstanceInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidExecutorActive,
      sessionTokenValid: false,
      agentPrincipalValid: false,
      agentInstanceValid: false,
      roleShape: ValidRole,
      stateShape: ValidState,
      roleValue: ExecutorRole,
      sessionStateValue: ActiveState,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
      scheduledWork: false,
      mutationAuthorityGranted: false,
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
      agentPrincipalValid: facts.agentPrincipalValid,
      agentInstanceValid: facts.agentInstanceValid,
      roleShape: facts.roleShape,
      stateShape: facts.stateShape,
      roleValue: facts.roleValue,
      sessionStateValue: facts.sessionStateValue,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      scheduledWork: false,
      mutationAuthorityGranted: false,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidExecutorActive) },
    all { s.caseIndex == 1, evaluate(ValidReviewerStale) },
    all { s.caseIndex == 2, evaluate(ValidApplyGateExited) },
    all { s.caseIndex == 3, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 4, evaluate(InvalidAgentPrincipal) },
    all { s.caseIndex == 5, evaluate(InvalidAgentInstance) },
    all { s.caseIndex == 6, evaluate(InvalidRole) },
    all { s.caseIndex == 7, evaluate(InvalidState) },
    all { s.caseIndex >= 8, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 8
    and s.case.in(Set(ValidExecutorActive, ValidReviewerStale, ValidApplyGateExited, InvalidSessionToken, InvalidAgentPrincipal, InvalidAgentInstance, InvalidRole, InvalidState))
    and s.roleShape.in(Set(ValidRole, InvalidRoleShape))
    and s.stateShape.in(Set(ValidState, InvalidStateShape))
    and s.roleValue.in(Set(ExecutorRole, ReviewerRole, ApplyGateRole, InvalidRoleValue))
    and s.sessionStateValue.in(Set(ActiveState, StaleState, ExitedState, InvalidStateValue))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, InvalidRoleReject, InvalidStateReject, SessionTokenInvalid, AgentPrincipalInvalid, AgentInstanceInvalid))

  val acceptedRequiresValidRoleAndState =
    not(s.accepted)
    or (s.roleShape == ValidRole and s.stateShape == ValidState)

  val acceptedRequiresKnownRoleAndState =
    not(s.accepted)
    or (
      s.roleValue.in(Set(ExecutorRole, ReviewerRole, ApplyGateRole))
      and s.sessionStateValue.in(Set(ActiveState, StaleState, ExitedState))
    )

  val validCasesPreserveRoleState =
    not(s.evaluated)
    or (
      (s.case == ValidExecutorActive implies (s.roleValue == ExecutorRole and s.sessionStateValue == ActiveState))
      and (s.case == ValidReviewerStale implies (s.roleValue == ReviewerRole and s.sessionStateValue == StaleState))
      and (s.case == ValidApplyGateExited implies (s.roleValue == ApplyGateRole and s.sessionStateValue == ExitedState))
    )

  val acceptedRequiresTypedIdentity =
    not(s.accepted)
    or (s.sessionTokenValid and s.agentPrincipalValid and s.agentInstanceValid)

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      sessionTokenValid: s.sessionTokenValid,
      agentPrincipalValid: s.agentPrincipalValid,
      agentInstanceValid: s.agentInstanceValid,
      roleShape: s.roleShape,
      stateShape: s.stateShape,
      roleValue: s.roleValue,
      sessionStateValue: s.sessionStateValue,
    })

  val proofRowParsingDoesNotScheduleWork =
    not(s.scheduledWork)

  val proofRowParsingDoesNotGrantMutationAuthority =
    not(s.mutationAuthorityGranted)
}

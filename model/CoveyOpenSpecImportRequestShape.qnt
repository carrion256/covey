module CoveyOpenSpecImportRequestShape {
  type Case =
    DryRunAnonymousValid
    | DryRunAttributedValid
    | WriteValid
    | InvalidDryRunSession
    | InvalidWriteSession
    | MissingWriteSession
    | MissingWriteSessionInvalidChange
    | InvalidSessionInvalidProjectRoot
    | InvalidChangeId
    | EmptyProjectRoot
    | WhitespaceProjectRoot

  type Mode = DryRun | Write
  type SessionShape = NoSession | ValidSession | InvalidSession
  type ProjectRootShape = ValidRoot | EmptyRoot | WhitespaceRoot
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | WriteSessionRequired
    | ChangeIdInvalid
    | ProjectRootInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    mode: Mode,
    sessionShape: SessionShape,
    changeIdValid: bool,
    projectRootShape: ProjectRootShape,
    outcome: Outcome,
    rejectReason: RejectReason,
    sessionTokenPreserved: bool,
    writeSessionTokenPreserved: bool,
    changeIdPreserved: bool,
    projectRootPreserved: bool,
    coveyStateMutated: bool,
    schedulerAction: bool,
    evaluated: bool,
  }

  var s: Model

  def modeFor(case) =
    if (case.in(Set(DryRunAnonymousValid, DryRunAttributedValid, InvalidDryRunSession, InvalidSessionInvalidProjectRoot))) DryRun
    else Write

  def sessionFor(case) =
    if (case.in(Set(DryRunAnonymousValid))) NoSession
    else if (case.in(Set(InvalidDryRunSession, InvalidWriteSession, InvalidSessionInvalidProjectRoot))) InvalidSession
    else if (case.in(Set(MissingWriteSession, MissingWriteSessionInvalidChange))) NoSession
    else ValidSession

  def projectRootFor(case) =
    if (case == EmptyProjectRoot) EmptyRoot
    else if (case.in(Set(WhitespaceProjectRoot, InvalidSessionInvalidProjectRoot))) WhitespaceRoot
    else ValidRoot

  def changeIdValidFor(case) =
    not(case.in(Set(InvalidChangeId, MissingWriteSessionInvalidChange)))

  def rejectFor(case) =
    val mode = modeFor(case)
    val session = sessionFor(case)
    if (session == InvalidSession) SessionTokenInvalid
    else if (mode == Write and session == NoSession) WriteSessionRequired
    else if (not(changeIdValidFor(case))) ChangeIdInvalid
    else if (projectRootFor(case) != ValidRoot) ProjectRootInvalid
    else NoReject

  action init = {
    s' = {
      caseIndex: 0,
      case: DryRunAnonymousValid,
      mode: DryRun,
      sessionShape: NoSession,
      changeIdValid: true,
      projectRootShape: ValidRoot,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      sessionTokenPreserved: false,
      writeSessionTokenPreserved: false,
      changeIdPreserved: false,
      projectRootPreserved: false,
      coveyStateMutated: false,
      schedulerAction: false,
      evaluated: false,
    }
  }

  action evaluate(case) = {
    val rejectReason = rejectFor(case)
    val accepted = rejectReason == NoReject
    val mode = modeFor(case)
    val session = sessionFor(case)
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      mode: mode,
      sessionShape: session,
      changeIdValid: changeIdValidFor(case),
      projectRootShape: projectRootFor(case),
      outcome: if (accepted) Accepted else Rejected,
      rejectReason: rejectReason,
      sessionTokenPreserved: accepted and session == ValidSession,
      writeSessionTokenPreserved: accepted and mode == Write and session == ValidSession,
      changeIdPreserved: accepted,
      projectRootPreserved: accepted,
      coveyStateMutated: false,
      schedulerAction: false,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(DryRunAnonymousValid) },
    all { s.caseIndex == 1, evaluate(DryRunAttributedValid) },
    all { s.caseIndex == 2, evaluate(WriteValid) },
    all { s.caseIndex == 3, evaluate(InvalidDryRunSession) },
    all { s.caseIndex == 4, evaluate(InvalidWriteSession) },
    all { s.caseIndex == 5, evaluate(MissingWriteSession) },
    all { s.caseIndex == 6, evaluate(MissingWriteSessionInvalidChange) },
    all { s.caseIndex == 7, evaluate(InvalidSessionInvalidProjectRoot) },
    all { s.caseIndex == 8, evaluate(InvalidChangeId) },
    all { s.caseIndex == 9, evaluate(EmptyProjectRoot) },
    all { s.caseIndex == 10, evaluate(WhitespaceProjectRoot) },
    all { s.caseIndex >= 11, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 11

  val outcomeMatchesReject =
    not(s.evaluated)
    or s.outcome == (if (s.rejectReason == NoReject) Accepted else Rejected)

  val rejectReasonMatchesCase =
    not(s.evaluated) or s.rejectReason == rejectFor(s.case)

  val writeModeRequiresSession =
    not(s.evaluated and s.mode == Write and s.sessionShape == NoSession)
    or s.rejectReason == WriteSessionRequired

  val dryRunMayOmitSession =
    not(s.evaluated and s.case == DryRunAnonymousValid)
    or (s.outcome == Accepted and not(s.sessionTokenPreserved) and not(s.writeSessionTokenPreserved))

  val invalidSessionRejectsBeforeBoundaryFields =
    not(s.sessionShape == InvalidSession)
    or s.rejectReason == SessionTokenInvalid

  val missingWriteSessionRejectsBeforeChangeId =
    not(s.case == MissingWriteSessionInvalidChange)
    or s.rejectReason == WriteSessionRequired

  val acceptedPreservesModeSpecificSession =
    not(s.evaluated and s.outcome == Accepted)
    or (
      (s.mode == DryRun and s.sessionShape == NoSession and not(s.sessionTokenPreserved) and not(s.writeSessionTokenPreserved))
      or (s.mode == DryRun and s.sessionShape == ValidSession and s.sessionTokenPreserved and not(s.writeSessionTokenPreserved))
      or (s.mode == Write and s.sessionShape == ValidSession and s.sessionTokenPreserved and s.writeSessionTokenPreserved)
    )

  val acceptedPreservesChangeAndProjectRoot =
    not(s.evaluated and s.outcome == Accepted)
    or (s.changeIdPreserved and s.projectRootPreserved)

  val rejectedPreservesNoBoundaryFields =
    not(s.evaluated and s.outcome == Rejected)
    or (
      not(s.sessionTokenPreserved)
      and not(s.writeSessionTokenPreserved)
      and not(s.changeIdPreserved)
      and not(s.projectRootPreserved)
    )

  val requestShapeDoesNotMutateCoveyState =
    not(s.coveyStateMutated)

  val requestShapeDoesNotScheduleWork =
    not(s.schedulerAction)
}

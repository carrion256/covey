module CoveyRecordLifecycleShape {
  type Case =
    SessionActiveIdle
    | SessionActiveWithSubtask
    | SessionStaleIdle
    | SessionExitedIdle
    | SessionStaleWithSubtask
    | SessionExitedWithSubtask
    | SessionHeartbeatBeforeCreated
    | SessionUpdatedBeforeCreated
    | SessionNegativeHeartbeatTick
    | SessionInvalidPrincipal
    | SessionInvalidInstance
    | MetaPlanning
    | MetaActive
    | MetaCompleted
    | MetaCancelled
    | MetaUpdatedBeforeCreated
    | MetaBlankPrompt
    | ClaimHeld
    | ClaimReleased
    | ClaimExpired
    | ClaimRevoked
    | ClaimUpdatedBeforeCreated

  type RowKind = SessionRow | MetaTaskRow | ClaimRow
  type SessionLifecycle = Active | Stale | Exited
  type MetaTaskLifecycle = Planning | MetaActiveState | Completed | Cancelled
  type ClaimLifecycle = Held | Released | Expired | Revoked
  type TimestampShape = Monotonic | HeartbeatBeforeCreated | UpdatedBeforeCreated
  type TextShape = ValidText | InvalidPrincipal | InvalidInstance | BlankPrompt
  type TickShape = NonNegativeTick | NegativeTick
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | TerminalSessionCarriesActiveSubtask
    | SessionHeartbeatBeforeCreatedReject
    | UpdatedBeforeCreatedReject
    | NegativeHeartbeatTickReject
    | InvalidPrincipalReject
    | InvalidInstanceReject
    | BlankPromptReject

  type Model = {
    caseIndex: int,
    case: Case,
    rowKind: RowKind,
    sessionLifecycle: SessionLifecycle,
    metaTaskLifecycle: MetaTaskLifecycle,
    claimLifecycle: ClaimLifecycle,
    activeSubtaskPresent: bool,
    timestampShape: TimestampShape,
    textShape: TextShape,
    tickShape: TickShape,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def rowKindFor(case) =
    if (case.in(Set(
      SessionActiveIdle,
      SessionActiveWithSubtask,
      SessionStaleIdle,
      SessionExitedIdle,
      SessionStaleWithSubtask,
      SessionExitedWithSubtask,
      SessionHeartbeatBeforeCreated,
      SessionUpdatedBeforeCreated,
      SessionNegativeHeartbeatTick,
      SessionInvalidPrincipal,
      SessionInvalidInstance,
    ))) SessionRow
    else if (case.in(Set(
      MetaPlanning,
      MetaActive,
      MetaCompleted,
      MetaCancelled,
      MetaUpdatedBeforeCreated,
      MetaBlankPrompt,
    ))) MetaTaskRow
    else ClaimRow

  def sessionLifecycleFor(case) =
    if (case.in(Set(SessionStaleIdle, SessionStaleWithSubtask))) Stale
    else if (case.in(Set(SessionExitedIdle, SessionExitedWithSubtask))) Exited
    else Active

  def metaTaskLifecycleFor(case) =
    if (case == MetaActive) MetaActiveState
    else if (case == MetaCompleted) Completed
    else if (case == MetaCancelled) Cancelled
    else Planning

  def claimLifecycleFor(case) =
    if (case == ClaimReleased) Released
    else if (case == ClaimExpired) Expired
    else if (case == ClaimRevoked) Revoked
    else Held

  def activeSubtaskPresentFor(case) =
    case.in(Set(SessionActiveWithSubtask, SessionStaleWithSubtask, SessionExitedWithSubtask))

  def timestampShapeFor(case) =
    if (case == SessionHeartbeatBeforeCreated) HeartbeatBeforeCreated
    else if (case.in(Set(SessionUpdatedBeforeCreated, MetaUpdatedBeforeCreated, ClaimUpdatedBeforeCreated))) UpdatedBeforeCreated
    else Monotonic

  def textShapeFor(case) =
    if (case == SessionInvalidPrincipal) InvalidPrincipal
    else if (case == SessionInvalidInstance) InvalidInstance
    else if (case == MetaBlankPrompt) BlankPrompt
    else ValidText

  def tickShapeFor(case) =
    if (case == SessionNegativeHeartbeatTick) NegativeTick else NonNegativeTick

  def rejectFor(rowKind, sessionLifecycle, activeSubtaskPresent, timestampShape, textShape, tickShape) =
    if (rowKind == SessionRow and sessionLifecycle != Active and activeSubtaskPresent) TerminalSessionCarriesActiveSubtask
    else if (rowKind == SessionRow and timestampShape == HeartbeatBeforeCreated) SessionHeartbeatBeforeCreatedReject
    else if (timestampShape == UpdatedBeforeCreated) UpdatedBeforeCreatedReject
    else if (rowKind == SessionRow and tickShape == NegativeTick) NegativeHeartbeatTickReject
    else if (rowKind == SessionRow and textShape == InvalidPrincipal) InvalidPrincipalReject
    else if (rowKind == SessionRow and textShape == InvalidInstance) InvalidInstanceReject
    else if (rowKind == MetaTaskRow and textShape == BlankPrompt) BlankPromptReject
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: SessionActiveIdle,
      rowKind: SessionRow,
      sessionLifecycle: Active,
      metaTaskLifecycle: Planning,
      claimLifecycle: Held,
      activeSubtaskPresent: false,
      timestampShape: Monotonic,
      textShape: ValidText,
      tickShape: NonNegativeTick,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val rowKind = rowKindFor(case)
    val sessionLifecycle = sessionLifecycleFor(case)
    val metaTaskLifecycle = metaTaskLifecycleFor(case)
    val claimLifecycle = claimLifecycleFor(case)
    val activeSubtaskPresent = activeSubtaskPresentFor(case)
    val timestampShape = timestampShapeFor(case)
    val textShape = textShapeFor(case)
    val tickShape = tickShapeFor(case)
    val rejectReason = rejectFor(rowKind, sessionLifecycle, activeSubtaskPresent, timestampShape, textShape, tickShape)
    val accepted = rejectReason == NoReject
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      rowKind: rowKind,
      sessionLifecycle: sessionLifecycle,
      metaTaskLifecycle: metaTaskLifecycle,
      claimLifecycle: claimLifecycle,
      activeSubtaskPresent: activeSubtaskPresent,
      timestampShape: timestampShape,
      textShape: textShape,
      tickShape: tickShape,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(SessionActiveIdle) },
    all { s.caseIndex == 1, evaluate(SessionActiveWithSubtask) },
    all { s.caseIndex == 2, evaluate(SessionStaleIdle) },
    all { s.caseIndex == 3, evaluate(SessionExitedIdle) },
    all { s.caseIndex == 4, evaluate(SessionStaleWithSubtask) },
    all { s.caseIndex == 5, evaluate(SessionExitedWithSubtask) },
    all { s.caseIndex == 6, evaluate(SessionHeartbeatBeforeCreated) },
    all { s.caseIndex == 7, evaluate(SessionUpdatedBeforeCreated) },
    all { s.caseIndex == 8, evaluate(SessionNegativeHeartbeatTick) },
    all { s.caseIndex == 9, evaluate(SessionInvalidPrincipal) },
    all { s.caseIndex == 10, evaluate(SessionInvalidInstance) },
    all { s.caseIndex == 11, evaluate(MetaPlanning) },
    all { s.caseIndex == 12, evaluate(MetaActive) },
    all { s.caseIndex == 13, evaluate(MetaCompleted) },
    all { s.caseIndex == 14, evaluate(MetaCancelled) },
    all { s.caseIndex == 15, evaluate(MetaUpdatedBeforeCreated) },
    all { s.caseIndex == 16, evaluate(MetaBlankPrompt) },
    all { s.caseIndex == 17, evaluate(ClaimHeld) },
    all { s.caseIndex == 18, evaluate(ClaimReleased) },
    all { s.caseIndex == 19, evaluate(ClaimExpired) },
    all { s.caseIndex == 20, evaluate(ClaimRevoked) },
    all { s.caseIndex == 21, evaluate(ClaimUpdatedBeforeCreated) },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.rowKind.in(Set(SessionRow, MetaTaskRow, ClaimRow))
    and s.sessionLifecycle.in(Set(Active, Stale, Exited))
    and s.metaTaskLifecycle.in(Set(Planning, MetaActiveState, Completed, Cancelled))
    and s.claimLifecycle.in(Set(Held, Released, Expired, Revoked))
    and s.timestampShape.in(Set(Monotonic, HeartbeatBeforeCreated, UpdatedBeforeCreated))
    and s.textShape.in(Set(ValidText, InvalidPrincipal, InvalidInstance, BlankPrompt))
    and s.tickShape.in(Set(NonNegativeTick, NegativeTick))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(
      NoReject,
      TerminalSessionCarriesActiveSubtask,
      SessionHeartbeatBeforeCreatedReject,
      UpdatedBeforeCreatedReject,
      NegativeHeartbeatTickReject,
      InvalidPrincipalReject,
      InvalidInstanceReject,
      BlankPromptReject,
    ))

  val acceptedTerminalSessionsAreIdle =
    not(s.accepted) or s.rowKind != SessionRow or s.sessionLifecycle == Active or not(s.activeSubtaskPresent)

  val acceptedSessionTimestampsAreMonotonic =
    not(s.accepted) or s.rowKind != SessionRow or s.timestampShape == Monotonic

  val acceptedSessionTickIsNonNegative =
    not(s.accepted) or s.rowKind != SessionRow or s.tickShape == NonNegativeTick

  val acceptedMetaAndClaimTimestampsAreMonotonic =
    not(s.accepted) or s.rowKind == SessionRow or s.timestampShape == Monotonic

  val acceptedTextFieldsAreValid =
    not(s.accepted) or s.textShape == ValidText

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated)
    or s.rejectReason == rejectFor(
      s.rowKind,
      s.sessionLifecycle,
      s.activeSubtaskPresent,
      s.timestampShape,
      s.textShape,
      s.tickShape,
    )
}

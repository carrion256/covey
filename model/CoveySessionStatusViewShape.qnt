module CoveySessionStatusViewShape {
  type Case =
    ActiveIdleWithoutView
    | ActiveIdleWithView
    | ActiveWithMatchingView
    | ActiveWithMissingView
    | ActiveWithMismatchedView
    | StaleWithoutView
    | StaleWithView
    | ExitedWithoutView
    | ExitedWithView
    | StaleWithActiveSubtaskId
    | ExitedWithActiveSubtaskId

  type SessionLifecycle = ActiveIdle | ActiveWithSubtask | Stale | Exited | InvalidSession
  type ViewAttachment = NoView | MatchingView | MismatchedView
  type Outcome = NotEvaluated | AcceptedWithoutActiveSubtask | AcceptedWithActiveSubtask | Rejected
  type RejectReason =
    NoReject
    | InvalidSessionLifecycleReject
    | MissingActiveSubtaskViewReject
    | MismatchedActiveSubtaskViewReject
    | UnexpectedActiveSubtaskViewReject

  type Model = {
    caseIndex: int,
    case: Case,
    sessionLifecycle: SessionLifecycle,
    activeSubtaskView: ViewAttachment,
    sessionHasActiveSubtaskId: bool,
    acceptedSessionPreserved: bool,
    acceptedActiveSubtaskPreserved: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    evaluated: bool,
  }

  var s: Model

  def lifecycleFor(case) =
    if (case.in(Set(ActiveIdleWithoutView, ActiveIdleWithView))) ActiveIdle
    else if (case.in(Set(ActiveWithMatchingView, ActiveWithMissingView, ActiveWithMismatchedView))) ActiveWithSubtask
    else if (case.in(Set(StaleWithoutView, StaleWithView))) Stale
    else if (case.in(Set(ExitedWithoutView, ExitedWithView))) Exited
    else InvalidSession

  def viewFor(case) =
    if (case.in(Set(ActiveIdleWithoutView, ActiveWithMissingView, StaleWithoutView, ExitedWithoutView, StaleWithActiveSubtaskId, ExitedWithActiveSubtaskId))) NoView
    else if (case == ActiveWithMismatchedView) MismatchedView
    else MatchingView

  def sessionHasActiveSubtaskIdFor(lifecycle) =
    lifecycle == ActiveWithSubtask or lifecycle == InvalidSession

  def rejectFor(lifecycle, view) =
    if (lifecycle == InvalidSession) InvalidSessionLifecycleReject
    else if (lifecycle == ActiveWithSubtask and view == NoView) MissingActiveSubtaskViewReject
    else if (lifecycle == ActiveWithSubtask and view == MismatchedView) MismatchedActiveSubtaskViewReject
    else if (lifecycle != ActiveWithSubtask and view != NoView) UnexpectedActiveSubtaskViewReject
    else NoReject

  def outcomeFor(lifecycle, rejectReason) =
    if (rejectReason != NoReject) Rejected
    else if (lifecycle == ActiveWithSubtask) AcceptedWithActiveSubtask
    else AcceptedWithoutActiveSubtask

  action init = {
    s' = {
      caseIndex: 0,
      case: ActiveIdleWithoutView,
      sessionLifecycle: ActiveIdle,
      activeSubtaskView: NoView,
      sessionHasActiveSubtaskId: false,
      acceptedSessionPreserved: false,
      acceptedActiveSubtaskPreserved: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      evaluated: false,
    }
  }

  action evaluate(case) = {
    val lifecycle = lifecycleFor(case)
    val view = viewFor(case)
    val rejectReason = rejectFor(lifecycle, view)
    val accepted = rejectReason == NoReject
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      sessionLifecycle: lifecycle,
      activeSubtaskView: view,
      sessionHasActiveSubtaskId: sessionHasActiveSubtaskIdFor(lifecycle),
      acceptedSessionPreserved: accepted,
      acceptedActiveSubtaskPreserved: accepted and view == MatchingView,
      outcome: outcomeFor(lifecycle, rejectReason),
      rejectReason: rejectReason,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ActiveIdleWithoutView) },
    all { s.caseIndex == 1, evaluate(ActiveIdleWithView) },
    all { s.caseIndex == 2, evaluate(ActiveWithMatchingView) },
    all { s.caseIndex == 3, evaluate(ActiveWithMissingView) },
    all { s.caseIndex == 4, evaluate(ActiveWithMismatchedView) },
    all { s.caseIndex == 5, evaluate(StaleWithoutView) },
    all { s.caseIndex == 6, evaluate(StaleWithView) },
    all { s.caseIndex == 7, evaluate(ExitedWithoutView) },
    all { s.caseIndex == 8, evaluate(ExitedWithView) },
    all { s.caseIndex == 9, evaluate(StaleWithActiveSubtaskId) },
    all { s.caseIndex == 10, evaluate(ExitedWithActiveSubtaskId) },
    all { s.caseIndex >= 11, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 11
    and s.sessionLifecycle.in(Set(ActiveIdle, ActiveWithSubtask, Stale, Exited, InvalidSession))
    and s.activeSubtaskView.in(Set(NoView, MatchingView, MismatchedView))

  val outcomeMatchesReject =
    not(s.evaluated)
    or s.outcome == outcomeFor(s.sessionLifecycle, s.rejectReason)

  val rejectReasonMatchesShape =
    not(s.evaluated)
    or s.rejectReason == rejectFor(s.sessionLifecycle, s.activeSubtaskView)

  val activeSessionRequiresMatchingView =
    not(s.evaluated and s.sessionLifecycle == ActiveWithSubtask and s.outcome != Rejected)
    or s.activeSubtaskView == MatchingView

  val idleOrTerminalSessionsRejectViews =
    not(s.evaluated and s.sessionLifecycle.in(Set(ActiveIdle, Stale, Exited)) and s.activeSubtaskView != NoView)
    or s.outcome == Rejected

  val invalidSessionLifecycleRejected =
    not(s.evaluated and s.sessionLifecycle == InvalidSession)
    or s.outcome == Rejected

  val acceptedWithActiveSubtaskPreservesSessionAndView =
    s.outcome != AcceptedWithActiveSubtask
    or (s.sessionHasActiveSubtaskId and s.acceptedSessionPreserved and s.acceptedActiveSubtaskPreserved)

  val acceptedWithoutActiveSubtaskPreservesSessionOnly =
    s.outcome != AcceptedWithoutActiveSubtask
    or (not(s.sessionHasActiveSubtaskId) and s.acceptedSessionPreserved and not(s.acceptedActiveSubtaskPreserved))
}

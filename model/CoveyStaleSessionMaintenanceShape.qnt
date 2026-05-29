module CoveyStaleSessionMaintenanceShape {
  type Case =
    ActiveFreshNoClaim
    | ActiveStaleNoClaim
    | StaleTerminalNoClaim
    | ExitedTerminalNoClaim
    | ActiveStaleHeldClaimed
    | ActiveStaleHeldInProgress
    | ActiveFreshExpiredLeaseClaimed
    | ActiveFreshLiveHeldClaimed
    | StaleHeldClaimed
    | ExitedHeldInProgress

  type SessionState = Active | Stale | Exited
  type ClaimState = NoClaim | Held | Expired
  type SubtaskState = NoSubtask | Claimed | InProgress | Available | ReviewPending

  type Model = {
    caseIndex: int,
    case: Case,
    beforeSession: SessionState,
    afterSession: SessionState,
    heartbeatStale: bool,
    beforeClaim: ClaimState,
    afterClaim: ClaimState,
    leaseExpired: bool,
    beforeSubtask: SubtaskState,
    afterSubtask: SubtaskState,
    beforeSessionActiveSubtask: bool,
    afterSessionActiveSubtask: bool,
    selectedForSessionReap: bool,
    selectedForClaimExpiry: bool,
    staleSessionCount: int,
    expiredClaimCount: int,
    sessionsReapedEventEmitted: bool,
    claimEventEmitted: bool,
    executorClaimCreated: bool,
    mutationAuthorityGranted: bool,
    evaluated: bool,
  }

  var s: Model

  def beforeSessionFor(case) =
    if (case.in(Set(StaleTerminalNoClaim, StaleHeldClaimed))) Stale
    else if (case.in(Set(ExitedTerminalNoClaim, ExitedHeldInProgress))) Exited
    else Active

  def heartbeatStaleFor(case) =
    case.in(Set(ActiveStaleNoClaim, ActiveStaleHeldClaimed, ActiveStaleHeldInProgress))

  def beforeClaimFor(case) =
    if (case.in(Set(
      ActiveStaleHeldClaimed,
      ActiveStaleHeldInProgress,
      ActiveFreshExpiredLeaseClaimed,
      ActiveFreshLiveHeldClaimed,
      StaleHeldClaimed,
      ExitedHeldInProgress
    ))) Held
    else NoClaim

  def leaseExpiredFor(case) =
    case == ActiveFreshExpiredLeaseClaimed

  def beforeSubtaskFor(case) =
    if (case.in(Set(ActiveStaleHeldInProgress, ExitedHeldInProgress))) InProgress
    else if (case == StaleTerminalNoClaim) ReviewPending
    else if (beforeClaimFor(case) == Held) Claimed
    else NoSubtask

  def selectedForSessionReap(session, heartbeatStale) =
    session == Active and heartbeatStale

  def sessionAfter(session, selectedForSessionReap) =
    if (selectedForSessionReap) Stale else session

  def selectedForClaimExpiry(claim, leaseExpired, beforeSession, selectedForSessionReap) =
    claim == Held and (
      leaseExpired
      or selectedForSessionReap
      or beforeSession.in(Set(Stale, Exited))
    )

  def subtaskAfter(subtask, claimSelected) =
    if (claimSelected and subtask.in(Set(Claimed, InProgress))) Available else subtask

  action init = {
    s' = {
      caseIndex: 0,
      case: ActiveFreshNoClaim,
      beforeSession: Active,
      afterSession: Active,
      heartbeatStale: false,
      beforeClaim: NoClaim,
      afterClaim: NoClaim,
      leaseExpired: false,
      beforeSubtask: NoSubtask,
      afterSubtask: NoSubtask,
      beforeSessionActiveSubtask: false,
      afterSessionActiveSubtask: false,
      selectedForSessionReap: false,
      selectedForClaimExpiry: false,
      staleSessionCount: 0,
      expiredClaimCount: 0,
      sessionsReapedEventEmitted: false,
      claimEventEmitted: false,
      executorClaimCreated: false,
      mutationAuthorityGranted: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val beforeSession = beforeSessionFor(case)
    val heartbeatStale = heartbeatStaleFor(case)
    val beforeClaim = beforeClaimFor(case)
    val leaseExpired = leaseExpiredFor(case)
    val beforeSubtask = beforeSubtaskFor(case)
    val sessionSelected = selectedForSessionReap(beforeSession, heartbeatStale)
    val claimSelected = selectedForClaimExpiry(beforeClaim, leaseExpired, beforeSession, sessionSelected)
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      beforeSession: beforeSession,
      afterSession: sessionAfter(beforeSession, sessionSelected),
      heartbeatStale: heartbeatStale,
      beforeClaim: beforeClaim,
      afterClaim: if (claimSelected) Expired else beforeClaim,
      leaseExpired: leaseExpired,
      beforeSubtask: beforeSubtask,
      afterSubtask: subtaskAfter(beforeSubtask, claimSelected),
      beforeSessionActiveSubtask: beforeClaim == Held,
      afterSessionActiveSubtask: beforeClaim == Held and not(claimSelected),
      selectedForSessionReap: sessionSelected,
      selectedForClaimExpiry: claimSelected,
      staleSessionCount: if (sessionSelected) 1 else 0,
      expiredClaimCount: if (claimSelected) 1 else 0,
      sessionsReapedEventEmitted: sessionSelected,
      claimEventEmitted: false,
      executorClaimCreated: false,
      mutationAuthorityGranted: false,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ActiveFreshNoClaim) },
    all { s.caseIndex == 1, evaluate(ActiveStaleNoClaim) },
    all { s.caseIndex == 2, evaluate(StaleTerminalNoClaim) },
    all { s.caseIndex == 3, evaluate(ExitedTerminalNoClaim) },
    all { s.caseIndex == 4, evaluate(ActiveStaleHeldClaimed) },
    all { s.caseIndex == 5, evaluate(ActiveStaleHeldInProgress) },
    all { s.caseIndex == 6, evaluate(ActiveFreshExpiredLeaseClaimed) },
    all { s.caseIndex == 7, evaluate(ActiveFreshLiveHeldClaimed) },
    all { s.caseIndex == 8, evaluate(StaleHeldClaimed) },
    all { s.caseIndex == 9, evaluate(ExitedHeldInProgress) },
    all { s.caseIndex >= 10, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 10
    and s.case.in(Set(ActiveFreshNoClaim, ActiveStaleNoClaim, StaleTerminalNoClaim, ExitedTerminalNoClaim, ActiveStaleHeldClaimed, ActiveStaleHeldInProgress, ActiveFreshExpiredLeaseClaimed, ActiveFreshLiveHeldClaimed, StaleHeldClaimed, ExitedHeldInProgress))
    and s.beforeSession.in(Set(Active, Stale, Exited))
    and s.afterSession.in(Set(Active, Stale, Exited))
    and s.beforeClaim.in(Set(NoClaim, Held, Expired))
    and s.afterClaim.in(Set(NoClaim, Held, Expired))
    and s.beforeSubtask.in(Set(NoSubtask, Claimed, InProgress, Available, ReviewPending))
    and s.afterSubtask.in(Set(NoSubtask, Claimed, InProgress, Available, ReviewPending))
    and s.staleSessionCount >= 0
    and s.staleSessionCount <= 1
    and s.expiredClaimCount >= 0
    and s.expiredClaimCount <= 1

  val sessionReapSelectedIffActiveStale =
    not(s.evaluated) or s.selectedForSessionReap == selectedForSessionReap(s.beforeSession, s.heartbeatStale)

  val selectedActiveStaleSessionBecomesStale =
    not(s.evaluated and s.selectedForSessionReap) or s.afterSession == Stale

  val nonSelectedSessionUnchanged =
    not(s.evaluated and not(s.selectedForSessionReap)) or s.afterSession == s.beforeSession

  val staleSessionEventIffCountPositive =
    not(s.evaluated) or s.sessionsReapedEventEmitted == (s.staleSessionCount > 0)

  val claimExpirySelectedForExpiredLeaseOrInactiveOwner =
    not(s.evaluated) or s.selectedForClaimExpiry == selectedForClaimExpiry(
      s.beforeClaim,
      s.leaseExpired,
      s.beforeSession,
      s.selectedForSessionReap
    )

  val selectedHeldClaimExpires =
    not(s.evaluated and s.selectedForClaimExpiry) or s.afterClaim == Expired

  val nonSelectedClaimUnchanged =
    not(s.evaluated and not(s.selectedForClaimExpiry)) or s.afterClaim == s.beforeClaim

  val expiredClaimCountMatchesSelection =
    not(s.evaluated) or s.expiredClaimCount == (if (s.selectedForClaimExpiry) 1 else 0)

  val expiredClaimClearsSessionActiveSubtask =
    not(s.evaluated and s.selectedForClaimExpiry) or not(s.afterSessionActiveSubtask)

  val expiredClaimMakesClaimedOrInProgressSubtaskAvailable =
    not(s.evaluated and s.selectedForClaimExpiry and s.beforeSubtask.in(Set(Claimed, InProgress)))
    or s.afterSubtask == Available

  val nonSelectedSubtaskUnchanged =
    not(s.evaluated and not(s.selectedForClaimExpiry)) or s.afterSubtask == s.beforeSubtask

  val reapDoesNotEmitClaimExpiredEvent =
    not(s.claimEventEmitted)

  val maintenanceDoesNotClaimOrGrantMutationAuthority =
    not(s.executorClaimCreated) and not(s.mutationAuthorityGranted)
}

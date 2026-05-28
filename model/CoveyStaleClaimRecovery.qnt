module CoveyStaleClaimRecovery {
  type SessionState = ActiveSession | StaleSession | ExitedSession
  type ClaimState = NoClaim | Held | Expired | Released
  type ClaimOwner = NoOwner | OldSession | NewSession
  type SubtaskState = Available | Claimed | InProgress

  type Model = {
    oldSession: SessionState,
    newSession: SessionState,
    claim: ClaimState,
    owner: ClaimOwner,
    subtask: SubtaskState,
    oldActiveSubtask: bool,
    newActiveSubtask: bool,
    currentFence: int,
    expiredFence: int,
    staleReaped: bool,
    leaseExpired: bool,
    exitedWithHeldClaim: bool,
    staleMutationRejected: bool,
  }

  var s: Model

  def anyActiveSubtask(state) =
    state.oldActiveSubtask or state.newActiveSubtask

  def recoveryOccurred(state) =
    state.staleReaped or state.leaseExpired or state.exitedWithHeldClaim

  def canRecoverHeldOldClaim(state) =
    state.claim == Held
    and state.owner == OldSession
    and state.oldActiveSubtask
    and state.subtask.in(Set(Claimed, InProgress))

  action init = {
    s' = {
      oldSession: ActiveSession,
      newSession: ActiveSession,
      claim: NoClaim,
      owner: NoOwner,
      subtask: Available,
      oldActiveSubtask: false,
      newActiveSubtask: false,
      currentFence: 0,
      expiredFence: 0,
      staleReaped: false,
      leaseExpired: false,
      exitedWithHeldClaim: false,
      staleMutationRejected: false,
    }
  }

  action claimByOldSession = all {
    s.claim == NoClaim,
    s.owner == NoOwner,
    s.subtask == Available,
    s.oldSession == ActiveSession,
    s.currentFence < 3,
    s' = s
      .with("claim", Held)
      .with("owner", OldSession)
      .with("subtask", Claimed)
      .with("oldActiveSubtask", true)
      .with("currentFence", s.currentFence + 1),
  }

  action startOldClaim = all {
    s.claim == Held,
    s.owner == OldSession,
    s.subtask == Claimed,
    s.oldActiveSubtask,
    s' = s.with("subtask", InProgress),
  }

  action reapStaleOldSession = all {
    canRecoverHeldOldClaim(s),
    s' = s
      .with("oldSession", StaleSession)
      .with("claim", Expired)
      .with("owner", NoOwner)
      .with("subtask", Available)
      .with("oldActiveSubtask", false)
      .with("expiredFence", s.currentFence)
      .with("staleReaped", true),
  }

  action expireOldLease = all {
    canRecoverHeldOldClaim(s),
    s' = s
      .with("claim", Expired)
      .with("owner", NoOwner)
      .with("subtask", Available)
      .with("oldActiveSubtask", false)
      .with("expiredFence", s.currentFence)
      .with("leaseExpired", true),
  }

  action exitOldSessionWithHeldClaim = all {
    canRecoverHeldOldClaim(s),
    s' = s
      .with("oldSession", ExitedSession)
      .with("claim", Expired)
      .with("owner", NoOwner)
      .with("subtask", Available)
      .with("oldActiveSubtask", false)
      .with("expiredFence", s.currentFence)
      .with("exitedWithHeldClaim", true),
  }

  action rejectOldSessionMutation = all {
    recoveryOccurred(s),
    s.owner != OldSession,
    not(s.staleMutationRejected),
    s' = s.with("staleMutationRejected", true),
  }

  action claimRecoveredByNewSession = all {
    recoveryOccurred(s),
    s.claim == Expired,
    s.owner == NoOwner,
    s.subtask == Available,
    s.newSession == ActiveSession,
    not(s.newActiveSubtask),
    s.currentFence < 3,
    s' = s
      .with("claim", Held)
      .with("owner", NewSession)
      .with("subtask", Claimed)
      .with("newActiveSubtask", true)
      .with("currentFence", s.currentFence + 1),
  }

  action releaseNewClaim = all {
    s.claim == Held,
    s.owner == NewSession,
    s.newActiveSubtask,
    s' = s
      .with("claim", Released)
      .with("owner", NoOwner)
      .with("subtask", Available)
      .with("newActiveSubtask", false),
  }

  action step = any {
    claimByOldSession,
    startOldClaim,
    reapStaleOldSession,
    expireOldLease,
    exitOldSessionWithHeldClaim,
    rejectOldSessionMutation,
    claimRecoveredByNewSession,
    releaseNewClaim,
  }

  val typeOk =
    s.currentFence >= 0
    and s.currentFence <= 3
    and s.expiredFence >= 0
    and s.expiredFence <= s.currentFence

  val heldClaimHasExactlyOneActiveOwner =
    s.claim != Held
    or (
      (s.owner == OldSession and s.oldActiveSubtask and not(s.newActiveSubtask))
      or (s.owner == NewSession and s.newActiveSubtask and not(s.oldActiveSubtask))
    )

  val inactiveOldSessionHasNoActiveSubtask =
    s.oldSession == ActiveSession or not(s.oldActiveSubtask)

  val recoveredOldClaimDetachedFromSessionAndSubtask =
    not(recoveryOccurred(s) and s.owner != NewSession)
    or (not(s.oldActiveSubtask) and s.owner == NoOwner and s.subtask == Available)

  val recoveredClaimHasExpiredFence =
    not(recoveryOccurred(s)) or s.expiredFence > 0

  val staleOrExitedSessionCannotRetainHeldClaim =
    s.oldSession == ActiveSession or s.owner != OldSession

  val recoveredSubtaskIsClaimableOrReclaimed =
    not(recoveryOccurred(s))
    or s.subtask.in(Set(Available, Claimed))

  val resumedClaimAdvancesFence =
    not(s.owner == NewSession and s.claim == Held)
    or s.currentFence > s.expiredFence

  val noDualHeldClaim =
    not(s.oldActiveSubtask and s.newActiveSubtask)

  val staleMutationDoesNotReattachOldClaim =
    not(s.staleMutationRejected)
    or (s.owner != OldSession and not(s.oldActiveSubtask))

  val releaseNewClaimClearsNewActiveSubtask =
    not(s.claim.in(Set(Released, Expired)) and s.owner == NoOwner)
    or not(anyActiveSubtask(s))
}

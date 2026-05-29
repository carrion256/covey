module CoveyCurrentClaimFence {
  type Case =
    CurrentHeldLive
    | MissingSession
    | SessionExited
    | MissingClaim
    | WrongOwner
    | StaleFence
    | ReleasedClaim
    | ExpiredLease

  type RejectReason =
    NoReject
    | SessionMissing
    | SessionNotActive
    | ClaimMissing
    | NotClaimOwner
    | StaleFenceToken
    | ClaimNotHeld
    | LeaseExpired

  type Model = {
    caseIndex: int,
    case: Case,
    sessionPresent: bool,
    sessionActive: bool,
    claimPresent: bool,
    ownerMatches: bool,
    fenceMatches: bool,
    claimHeld: bool,
    leaseLive: bool,
    accepted: bool,
    rejectReason: RejectReason,
    claimReturned: bool,
    stateMutated: bool,
    mutationAuthorityGranted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    sessionPresent: not(case == MissingSession),
    sessionActive: not(case == MissingSession) and not(case == SessionExited),
    claimPresent: not(case == MissingClaim),
    ownerMatches: not(case.in(Set(MissingClaim, WrongOwner))),
    fenceMatches: not(case.in(Set(MissingClaim, WrongOwner, StaleFence))),
    claimHeld: not(case.in(Set(MissingClaim, WrongOwner, StaleFence, ReleasedClaim))),
    leaseLive: not(case.in(Set(MissingClaim, WrongOwner, StaleFence, ReleasedClaim, ExpiredLease))),
  }

  def rejectFor(facts) =
    if (not(facts.sessionPresent)) SessionMissing
    else if (not(facts.sessionActive)) SessionNotActive
    else if (not(facts.claimPresent)) ClaimMissing
    else if (not(facts.ownerMatches)) NotClaimOwner
    else if (not(facts.fenceMatches)) StaleFenceToken
    else if (not(facts.claimHeld)) ClaimNotHeld
    else if (not(facts.leaseLive)) LeaseExpired
    else NoReject

  action init = {
    s' = {
      caseIndex: 0,
      case: CurrentHeldLive,
      sessionPresent: false,
      sessionActive: false,
      claimPresent: false,
      ownerMatches: false,
      fenceMatches: false,
      claimHeld: false,
      leaseLive: false,
      accepted: false,
      rejectReason: NoReject,
      claimReturned: false,
      stateMutated: false,
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
      sessionPresent: facts.sessionPresent,
      sessionActive: facts.sessionActive,
      claimPresent: facts.claimPresent,
      ownerMatches: facts.ownerMatches,
      fenceMatches: facts.fenceMatches,
      claimHeld: facts.claimHeld,
      leaseLive: facts.leaseLive,
      accepted: accepted,
      rejectReason: rejectReason,
      claimReturned: accepted,
      stateMutated: false,
      mutationAuthorityGranted: false,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(CurrentHeldLive) },
    all { s.caseIndex == 1, evaluate(MissingSession) },
    all { s.caseIndex == 2, evaluate(SessionExited) },
    all { s.caseIndex == 3, evaluate(MissingClaim) },
    all { s.caseIndex == 4, evaluate(WrongOwner) },
    all { s.caseIndex == 5, evaluate(StaleFence) },
    all { s.caseIndex == 6, evaluate(ReleasedClaim) },
    all { s.caseIndex == 7, evaluate(ExpiredLease) },
    all { s.caseIndex >= 8, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 8
    and s.case.in(Set(
      CurrentHeldLive,
      MissingSession,
      SessionExited,
      MissingClaim,
      WrongOwner,
      StaleFence,
      ReleasedClaim,
      ExpiredLease,
    ))
    and s.rejectReason.in(Set(
      NoReject,
      SessionMissing,
      SessionNotActive,
      ClaimMissing,
      NotClaimOwner,
      StaleFenceToken,
      ClaimNotHeld,
      LeaseExpired,
    ))

  val rejectReasonMatchesValidationOrder =
    not(s.evaluated) or s.rejectReason == rejectFor({
      sessionPresent: s.sessionPresent,
      sessionActive: s.sessionActive,
      claimPresent: s.claimPresent,
      ownerMatches: s.ownerMatches,
      fenceMatches: s.fenceMatches,
      claimHeld: s.claimHeld,
      leaseLive: s.leaseLive,
    })

  val acceptedIffCurrentClaim =
    s.accepted == (
      s.evaluated
      and s.sessionPresent
      and s.sessionActive
      and s.claimPresent
      and s.ownerMatches
      and s.fenceMatches
      and s.claimHeld
      and s.leaseLive
    )

  val acceptedRequiresActiveSessionAndPresentClaim =
    not(s.accepted) or (s.sessionPresent and s.sessionActive and s.claimPresent)

  val acceptedRequiresOwnerFenceHeldLive =
    not(s.accepted) or (s.ownerMatches and s.fenceMatches and s.claimHeld and s.leaseLive)

  val rejectedDoesNotReturnClaim =
    s.accepted or not(s.claimReturned)

  val validatorDoesNotMutateStateOrGrantAuthority =
    not(s.stateMutated) and not(s.mutationAuthorityGranted)
}

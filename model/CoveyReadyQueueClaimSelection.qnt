module CoveyReadyQueueClaimSelection {
  type Case =
    ValidHeadClaimed
    | InvalidHeadSupersededTailClaimed
    | MetaUnavailableHeadCancelledTailClaimed
    | ExpiredHeadRequeuedThenClaimed
    | ActiveInFlightHeadIgnoredTailClaimed
    | OnlyInvalidHeadNoClaim
    | OnlyMetaUnavailableHeadNoClaim
    | EmptyQueueNoClaim
    | WrongRoleRejected

  type QueueState = Missing | Queued | InFlight
  type HeadAction = NoHeadAction | ClaimedHead | SupersededHead | CancelledHead | RequeuedHead | IgnoredHead
  type ClaimResult = NotEvaluated | ClaimHead | ClaimTail | NoClaim | Rejected

  type Model = {
    caseIndex: int,
    case: Case,
    roleApplyGate: bool,
    headState: QueueState,
    headExpired: bool,
    headSubtaskReady: bool,
    headArtifactMatches: bool,
    headMetaSchedulable: bool,
    tailPresent: bool,
    tailClaimable: bool,
    headAction: HeadAction,
    result: ClaimResult,
    selectedHead: bool,
    selectedTail: bool,
    headFenceBefore: int,
    headFenceAfter: int,
    tailFenceBefore: int,
    tailFenceAfter: int,
    eventEmitted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsForCase(case) = {
    roleApplyGate: case != WrongRoleRejected,
    headState:
      if (case == EmptyQueueNoClaim) Missing
      else if (case.in(Set(ExpiredHeadRequeuedThenClaimed, ActiveInFlightHeadIgnoredTailClaimed))) InFlight
      else Queued,
    headExpired: case == ExpiredHeadRequeuedThenClaimed,
    headSubtaskReady:
      not(case.in(Set(InvalidHeadSupersededTailClaimed, OnlyInvalidHeadNoClaim))),
    headArtifactMatches:
      not(case.in(Set(InvalidHeadSupersededTailClaimed, OnlyInvalidHeadNoClaim))),
    headMetaSchedulable:
      not(case.in(Set(MetaUnavailableHeadCancelledTailClaimed, OnlyMetaUnavailableHeadNoClaim))),
    tailPresent:
      case.in(Set(InvalidHeadSupersededTailClaimed, MetaUnavailableHeadCancelledTailClaimed, ActiveInFlightHeadIgnoredTailClaimed)),
    tailClaimable:
      case.in(Set(InvalidHeadSupersededTailClaimed, MetaUnavailableHeadCancelledTailClaimed, ActiveInFlightHeadIgnoredTailClaimed)),
  }

  def headQueuedAfterRequeue(facts) =
    facts.headState == Queued or (facts.headState == InFlight and facts.headExpired)

  def headValidClaimCandidate(facts) =
    facts.roleApplyGate
    and headQueuedAfterRequeue(facts)
    and facts.headSubtaskReady
    and facts.headArtifactMatches
    and facts.headMetaSchedulable

  def headActionFor(facts) =
    if (not(facts.roleApplyGate) or facts.headState == Missing) NoHeadAction
    else if (facts.headState == InFlight and not(facts.headExpired)) IgnoredHead
    else if (not(facts.headMetaSchedulable)) CancelledHead
    else if (not(facts.headSubtaskReady and facts.headArtifactMatches)) SupersededHead
    else if (facts.headState == InFlight and facts.headExpired) RequeuedHead
    else ClaimedHead

  def resultFor(facts) =
    if (not(facts.roleApplyGate)) Rejected
    else if (headValidClaimCandidate(facts)) ClaimHead
    else if (facts.tailPresent and facts.tailClaimable) ClaimTail
    else NoClaim

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidHeadClaimed,
      roleApplyGate: true,
      headState: Missing,
      headExpired: false,
      headSubtaskReady: false,
      headArtifactMatches: false,
      headMetaSchedulable: false,
      tailPresent: false,
      tailClaimable: false,
      headAction: NoHeadAction,
      result: NotEvaluated,
      selectedHead: false,
      selectedTail: false,
      headFenceBefore: 0,
      headFenceAfter: 0,
      tailFenceBefore: 0,
      tailFenceAfter: 0,
      eventEmitted: false,
      evaluated: false,
    }
  }

  action evaluate(caseIndex, case) = all {
    val facts = factsForCase(case)
    val result = resultFor(facts)
    val selectedHead = result == ClaimHead
    val selectedTail = result == ClaimTail
    val headFenceBefore = if (case == ExpiredHeadRequeuedThenClaimed) 1 else 0
    val tailFenceBefore = if (facts.tailPresent) 0 else 0
    s' = {
      caseIndex: caseIndex,
      case: case,
      roleApplyGate: facts.roleApplyGate,
      headState: facts.headState,
      headExpired: facts.headExpired,
      headSubtaskReady: facts.headSubtaskReady,
      headArtifactMatches: facts.headArtifactMatches,
      headMetaSchedulable: facts.headMetaSchedulable,
      tailPresent: facts.tailPresent,
      tailClaimable: facts.tailClaimable,
      headAction: headActionFor(facts),
      result: result,
      selectedHead: selectedHead,
      selectedTail: selectedTail,
      headFenceBefore: headFenceBefore,
      headFenceAfter: if (selectedHead) headFenceBefore + 1 else headFenceBefore,
      tailFenceBefore: tailFenceBefore,
      tailFenceAfter: if (selectedTail) tailFenceBefore + 1 else tailFenceBefore,
      eventEmitted: result.in(Set(ClaimHead, ClaimTail)),
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(1, ValidHeadClaimed) },
    all { s.caseIndex == 1, evaluate(2, InvalidHeadSupersededTailClaimed) },
    all { s.caseIndex == 2, evaluate(3, MetaUnavailableHeadCancelledTailClaimed) },
    all { s.caseIndex == 3, evaluate(4, ExpiredHeadRequeuedThenClaimed) },
    all { s.caseIndex == 4, evaluate(5, ActiveInFlightHeadIgnoredTailClaimed) },
    all { s.caseIndex == 5, evaluate(6, OnlyInvalidHeadNoClaim) },
    all { s.caseIndex == 6, evaluate(7, OnlyMetaUnavailableHeadNoClaim) },
    all { s.caseIndex == 7, evaluate(8, EmptyQueueNoClaim) },
    all { s.caseIndex == 8, evaluate(9, WrongRoleRejected) },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 9
    and s.case.in(Set(ValidHeadClaimed, InvalidHeadSupersededTailClaimed, MetaUnavailableHeadCancelledTailClaimed, ExpiredHeadRequeuedThenClaimed, ActiveInFlightHeadIgnoredTailClaimed, OnlyInvalidHeadNoClaim, OnlyMetaUnavailableHeadNoClaim, EmptyQueueNoClaim, WrongRoleRejected))
    and s.headState.in(Set(Missing, Queued, InFlight))
    and s.headAction.in(Set(NoHeadAction, ClaimedHead, SupersededHead, CancelledHead, RequeuedHead, IgnoredHead))
    and s.result.in(Set(NotEvaluated, ClaimHead, ClaimTail, NoClaim, Rejected))
    and s.headFenceAfter >= s.headFenceBefore
    and s.tailFenceAfter >= s.tailFenceBefore

  val claimRequiresApplyGate =
    not(s.result.in(Set(ClaimHead, ClaimTail))) or s.roleApplyGate

  val headClaimRequiresReadyArtifactAndSchedulable =
    s.result != ClaimHead
    or (s.headSubtaskReady and s.headArtifactMatches and s.headMetaSchedulable)

  val invalidHeadIsTerminalBeforeTailClaim =
    s.case != InvalidHeadSupersededTailClaimed
    or (s.headAction == SupersededHead and s.result == ClaimTail and not(s.selectedHead))

  val unavailableHeadIsCancelledBeforeTailClaim =
    s.case != MetaUnavailableHeadCancelledTailClaimed
    or (s.headAction == CancelledHead and s.result == ClaimTail and not(s.selectedHead))

  val expiredHeadIsRequeuedBeforeClaim =
    s.case != ExpiredHeadRequeuedThenClaimed
    or (s.headAction == RequeuedHead and s.result == ClaimHead and s.selectedHead)

  val activeInFlightHeadDoesNotBlockTail =
    s.case != ActiveInFlightHeadIgnoredTailClaimed
    or (s.headAction == IgnoredHead and s.result == ClaimTail and s.selectedTail)

  val returnedClaimEmitsEvent =
    s.eventEmitted iff s.result.in(Set(ClaimHead, ClaimTail))

  val onlySelectedQueueFenceAdvances =
    (
      (s.selectedHead and s.headFenceAfter == s.headFenceBefore + 1 and s.tailFenceAfter == s.tailFenceBefore)
      or (s.selectedTail and s.tailFenceAfter == s.tailFenceBefore + 1 and s.headFenceAfter == s.headFenceBefore)
      or (not(s.selectedHead or s.selectedTail) and s.headFenceAfter == s.headFenceBefore and s.tailFenceAfter == s.tailFenceBefore)
    )
}

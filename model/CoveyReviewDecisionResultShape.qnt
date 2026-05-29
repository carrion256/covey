module CoveyReviewDecisionResultShape {
  type Case =
    ApprovedResult
    | FailedChangesRequestedResult
    | FailedBlockedResult
    | InvalidApprovedWithFollowup
    | InvalidFailedMissingFollowup
    | InvalidFailedApproveVerdict
    | InvalidMissingReviewId

  type Outcome = Approved | Failed
  type Verdict = Approve | ChangesRequested | Blocked

  type Model = {
    caseIndex: int,
    case: Case,
    outcome: Outcome,
    reviewIdPresent: bool,
    failedVerdict: Verdict,
    followupPresent: bool,
    resultVerdict: Verdict,
    resultFollowupPresent: bool,
    shapeValid: bool,
    applyQueued: bool,
    landingAuthorized: bool,
    executorClaimCreated: bool,
    mutationAuthorityGranted: bool,
    evaluated: bool,
  }

  var s: Model

  def outcomeFor(case) =
    if (case.in(Set(ApprovedResult, InvalidApprovedWithFollowup))) Approved else Failed

  def failedVerdictFor(case) =
    if (case == FailedBlockedResult) Blocked
    else if (case == InvalidFailedApproveVerdict) Approve
    else ChangesRequested

  def followupPresentFor(case) =
    if (case.in(Set(ApprovedResult, InvalidFailedMissingFollowup))) false else true

  def reviewIdPresentFor(case) =
    case != InvalidMissingReviewId

  def resultVerdictFor(outcome, failedVerdict) =
    if (outcome == Approved) Approve else failedVerdict

  def shapeValidFor(outcome, reviewIdPresent, failedVerdict, followupPresent) =
    reviewIdPresent
    and (
      (outcome == Approved and not(followupPresent))
      or (
        outcome == Failed
        and failedVerdict.in(Set(ChangesRequested, Blocked))
        and followupPresent
      )
    )

  action init = {
    s' = {
      caseIndex: 0,
      case: ApprovedResult,
      outcome: Approved,
      reviewIdPresent: false,
      failedVerdict: Approve,
      followupPresent: false,
      resultVerdict: Approve,
      resultFollowupPresent: false,
      shapeValid: false,
      applyQueued: false,
      landingAuthorized: false,
      executorClaimCreated: false,
      mutationAuthorityGranted: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val outcome = outcomeFor(case)
    val failedVerdict = failedVerdictFor(case)
    val followupPresent = followupPresentFor(case)
    val reviewIdPresent = reviewIdPresentFor(case)
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      outcome: outcome,
      reviewIdPresent: reviewIdPresent,
      failedVerdict: failedVerdict,
      followupPresent: followupPresent,
      resultVerdict: resultVerdictFor(outcome, failedVerdict),
      resultFollowupPresent: followupPresent,
      shapeValid: shapeValidFor(outcome, reviewIdPresent, failedVerdict, followupPresent),
      applyQueued: false,
      landingAuthorized: false,
      executorClaimCreated: false,
      mutationAuthorityGranted: false,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ApprovedResult) },
    all { s.caseIndex == 1, evaluate(FailedChangesRequestedResult) },
    all { s.caseIndex == 2, evaluate(FailedBlockedResult) },
    all { s.caseIndex == 3, evaluate(InvalidApprovedWithFollowup) },
    all { s.caseIndex == 4, evaluate(InvalidFailedMissingFollowup) },
    all { s.caseIndex == 5, evaluate(InvalidFailedApproveVerdict) },
    all { s.caseIndex == 6, evaluate(InvalidMissingReviewId) },
    all { s.caseIndex >= 7, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 7
    and s.case.in(Set(ApprovedResult, FailedChangesRequestedResult, FailedBlockedResult, InvalidApprovedWithFollowup, InvalidFailedMissingFollowup, InvalidFailedApproveVerdict, InvalidMissingReviewId))
    and s.outcome.in(Set(Approved, Failed))
    and s.failedVerdict.in(Set(Approve, ChangesRequested, Blocked))
    and s.resultVerdict.in(Set(Approve, ChangesRequested, Blocked))

  val approvedCarriesNoFollowup =
    not(s.evaluated and s.shapeValid and s.outcome == Approved)
    or (s.resultVerdict == Approve and not(s.resultFollowupPresent))

  val failedCarriesFailedVerdictAndFollowup =
    not(s.evaluated and s.shapeValid and s.outcome == Failed)
    or (
      s.resultVerdict.in(Set(ChangesRequested, Blocked))
      and s.failedVerdict == s.resultVerdict
      and s.resultFollowupPresent
    )

  val validResultRequiresReviewId =
    not(s.shapeValid) or s.reviewIdPresent

  val resultVerdictMatchesOutcome =
    not(s.evaluated)
    or s.resultVerdict == resultVerdictFor(s.outcome, s.failedVerdict)

  val followupPresenceMatchesOutcome =
    not(s.shapeValid)
    or s.resultFollowupPresent == (s.outcome == Failed)

  val invalidShapeCarriesMismatch =
    not(s.evaluated)
    or s.shapeValid
    or not(shapeValidFor(s.outcome, s.reviewIdPresent, s.failedVerdict, s.followupPresent))

  val resultDoesNotQueueApplyOrAuthorizeLanding =
    not(s.applyQueued) and not(s.landingAuthorized)

  val resultDoesNotCreateExecutorClaimOrGrantMutation =
    not(s.executorClaimCreated) and not(s.mutationAuthorityGranted)
}

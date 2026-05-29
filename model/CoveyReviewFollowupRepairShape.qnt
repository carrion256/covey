module CoveyReviewFollowupRepairShape {
  type Case =
    ValidMissingChangesRequested
    | ExistingChangesRequestedFollowup
    | ApprovedReview
    | BlockedReview
    | RequestedReview
    | ChangesRequestedSourceNotWork
    | ChangesRequestedSourceWrongState
    | ChangesRequestedMissingFindings

  type ReviewLifecycle = Requested | InProgress | Decided
  type Verdict = NoVerdict | Approve | ChangesRequested | Blocked
  type SourceKind = Work | Review
  type SourceState = ReviewPending | Approved | ChangesRequestedState | BlockedState
  type Result = NotEvaluated | CreatedFollowup | NoRepairNeeded | RepairRejected
  type RejectReason = NoReject | MissingFindings

  type Model = {
    caseIndex: int,
    case: Case,
    reviewLifecycle: ReviewLifecycle,
    verdict: Verdict,
    sourceKind: SourceKind,
    sourceState: SourceState,
    existingFollowup: bool,
    findingsPresent: bool,
    selectedForRepair: bool,
    result: Result,
    rejectReason: RejectReason,
    createdCount: int,
    followupAvailable: bool,
    followupReviewBound: bool,
    followupSourceSubtaskBound: bool,
    followupSourceArtifactBound: bool,
    followupFindingsBound: bool,
    followupCreatedByReviewer: bool,
    claimCreated: bool,
    mutationAuthorityGranted: bool,
    evaluated: bool,
  }

  var s: Model

  def reviewLifecycleFor(case) =
    if (case == RequestedReview) Requested else Decided

  def verdictFor(case) =
    if (case == ApprovedReview) Approve
    else if (case == BlockedReview) Blocked
    else if (case == RequestedReview) NoVerdict
    else ChangesRequested

  def sourceKindFor(case) =
    if (case == ChangesRequestedSourceNotWork) Review else Work

  def sourceStateFor(case) =
    if (case == ApprovedReview) Approved
    else if (case == BlockedReview) BlockedState
    else if (case.in(Set(RequestedReview, ChangesRequestedSourceWrongState))) ReviewPending
    else ChangesRequestedState

  def existingFollowupFor(case) =
    case == ExistingChangesRequestedFollowup

  def findingsPresentFor(case) =
    case != ChangesRequestedMissingFindings

  // Mirrors ensure_changes_requested_followup_blocks_tx selection predicates.
  def selectedForRepair(reviewLifecycle, verdict, sourceKind, sourceState, existingFollowup) =
    reviewLifecycle == Decided
    and verdict == ChangesRequested
    and sourceKind == Work
    and sourceState == ChangesRequestedState
    and not(existingFollowup)

  def rejectFor(selected, findingsPresent) =
    if (selected and not(findingsPresent)) MissingFindings else NoReject

  def resultFor(selected, rejectReason) =
    if (rejectReason != NoReject) RepairRejected
    else if (selected) CreatedFollowup
    else NoRepairNeeded

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidMissingChangesRequested,
      reviewLifecycle: Requested,
      verdict: NoVerdict,
      sourceKind: Work,
      sourceState: ReviewPending,
      existingFollowup: false,
      findingsPresent: false,
      selectedForRepair: false,
      result: NotEvaluated,
      rejectReason: NoReject,
      createdCount: 0,
      followupAvailable: false,
      followupReviewBound: false,
      followupSourceSubtaskBound: false,
      followupSourceArtifactBound: false,
      followupFindingsBound: false,
      followupCreatedByReviewer: false,
      claimCreated: false,
      mutationAuthorityGranted: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val reviewLifecycle = reviewLifecycleFor(case)
    val verdict = verdictFor(case)
    val sourceKind = sourceKindFor(case)
    val sourceState = sourceStateFor(case)
    val existingFollowup = existingFollowupFor(case)
    val findingsPresent = findingsPresentFor(case)
    val selected = selectedForRepair(reviewLifecycle, verdict, sourceKind, sourceState, existingFollowup)
    val rejectReason = rejectFor(selected, findingsPresent)
    val result = resultFor(selected, rejectReason)
    val created = result == CreatedFollowup
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      reviewLifecycle: reviewLifecycle,
      verdict: verdict,
      sourceKind: sourceKind,
      sourceState: sourceState,
      existingFollowup: existingFollowup,
      findingsPresent: findingsPresent,
      selectedForRepair: selected,
      result: result,
      rejectReason: rejectReason,
      createdCount: if (created) 1 else 0,
      followupAvailable: created,
      followupReviewBound: created,
      followupSourceSubtaskBound: created,
      followupSourceArtifactBound: created,
      followupFindingsBound: created,
      followupCreatedByReviewer: created,
      claimCreated: false,
      mutationAuthorityGranted: false,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidMissingChangesRequested) },
    all { s.caseIndex == 1, evaluate(ExistingChangesRequestedFollowup) },
    all { s.caseIndex == 2, evaluate(ApprovedReview) },
    all { s.caseIndex == 3, evaluate(BlockedReview) },
    all { s.caseIndex == 4, evaluate(RequestedReview) },
    all { s.caseIndex == 5, evaluate(ChangesRequestedSourceNotWork) },
    all { s.caseIndex == 6, evaluate(ChangesRequestedSourceWrongState) },
    all { s.caseIndex == 7, evaluate(ChangesRequestedMissingFindings) },
    all { s.caseIndex >= 8, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 8
    and s.reviewLifecycle.in(Set(Requested, InProgress, Decided))
    and s.verdict.in(Set(NoVerdict, Approve, ChangesRequested, Blocked))
    and s.sourceKind.in(Set(Work, Review))
    and s.sourceState.in(Set(ReviewPending, Approved, ChangesRequestedState, BlockedState))
    and s.result.in(Set(NotEvaluated, CreatedFollowup, NoRepairNeeded, RepairRejected))
    and s.rejectReason.in(Set(NoReject, MissingFindings))
    and s.createdCount >= 0
    and s.createdCount <= 1

  val selectionMatchesChangesRequestedRepairPredicate =
    not(s.evaluated)
    or s.selectedForRepair == selectedForRepair(
      s.reviewLifecycle,
      s.verdict,
      s.sourceKind,
      s.sourceState,
      s.existingFollowup
    )

  val createsOnlyForMissingChangesRequestedWork =
    s.createdCount == 0
    or (
      s.reviewLifecycle == Decided
      and s.verdict == ChangesRequested
      and s.sourceKind == Work
      and s.sourceState == ChangesRequestedState
      and not(s.existingFollowup)
    )

  val existingFollowupIsNotDuplicated =
    not(s.existingFollowup) or s.createdCount == 0

  val approvedAndBlockedReviewsAreNotRepaired =
    not(s.verdict.in(Set(Approve, Blocked))) or s.createdCount == 0

  val repairRequiresFindings =
    not(s.selectedForRepair and not(s.findingsPresent))
    or (s.result == RepairRejected and s.createdCount == 0)

  val createdFollowupIsAvailableWork =
    s.createdCount == 0 or s.followupAvailable

  val createdFollowupBindsSourceReviewArtifactFindingsAndReviewer =
    s.createdCount == 0
    or (
      s.followupReviewBound
      and s.followupSourceSubtaskBound
      and s.followupSourceArtifactBound
      and s.followupFindingsBound
      and s.followupCreatedByReviewer
    )

  val repairDoesNotClaimExecutorWork =
    not(s.claimCreated)

  val repairDoesNotGrantMutationAuthority =
    not(s.mutationAuthorityGranted)
}

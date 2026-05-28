module CoveyReviewRecordShape {
  type Case =
    ValidRequested
    | ValidInProgress
    | ValidDecidedApprove
    | ValidDecidedBlocked
    | ValidSuperseded
    | MissingReviewSubtask
    | BlankReviewSubtask
    | InvalidReviewSubtask
    | RequestedWithVerdict
    | InProgressWithFindings
    | SupersededWithDecision
    | DecidedMissingVerdict
    | DecidedMissingFindings
    | DecidedBlankFindings
    | DecidedInvalidFindings
    | UpdatedBeforeCreated

  type ReviewLifecycle = Requested | InProgress | Decided | Superseded
  type DecisionShape = NoDecision | VerdictOnly | FindingsOnly | CompleteDecision | BlankFindings | InvalidFindings
  type ReviewSubtaskShape = ValidReviewSubtask | MissingReviewSubtaskShape | BlankReviewSubtaskShape | InvalidReviewSubtaskShape
  type TimestampShape = Monotonic | UpdatedBeforeCreatedShape
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | ReviewSubtaskMissing
    | ReviewSubtaskInvalid
    | NonDecidedCarriesDecisionEvidence
    | DecidedMissingVerdictReject
    | DecidedMissingFindingsReject
    | FindingsDigestInvalid
    | UpdatedBeforeCreatedReject

  type Model = {
    caseIndex: int,
    case: Case,
    lifecycle: ReviewLifecycle,
    decisionShape: DecisionShape,
    reviewSubtaskShape: ReviewSubtaskShape,
    timestampShape: TimestampShape,
    verdictPresent: bool,
    findingsDigestPresent: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def lifecycleFor(case) =
    if (case.in(Set(ValidInProgress, InProgressWithFindings))) InProgress
    else if (case.in(Set(ValidDecidedApprove, ValidDecidedBlocked, DecidedMissingVerdict, DecidedMissingFindings, DecidedBlankFindings, DecidedInvalidFindings))) Decided
    else if (case.in(Set(ValidSuperseded, SupersededWithDecision))) Superseded
    else Requested

  def decisionShapeFor(case) =
    if (case.in(Set(ValidDecidedApprove, ValidDecidedBlocked, SupersededWithDecision))) CompleteDecision
    else if (case == RequestedWithVerdict) VerdictOnly
    else if (case.in(Set(InProgressWithFindings, DecidedMissingVerdict))) FindingsOnly
    else if (case == DecidedMissingFindings) VerdictOnly
    else if (case == DecidedBlankFindings) BlankFindings
    else if (case == DecidedInvalidFindings) InvalidFindings
    else NoDecision

  def reviewSubtaskShapeFor(case) =
    if (case == MissingReviewSubtask) MissingReviewSubtaskShape
    else if (case == BlankReviewSubtask) BlankReviewSubtaskShape
    else if (case == InvalidReviewSubtask) InvalidReviewSubtaskShape
    else ValidReviewSubtask

  def timestampShapeFor(case) =
    if (case == UpdatedBeforeCreated) UpdatedBeforeCreatedShape else Monotonic

  def verdictPresentFor(decisionShape) =
    decisionShape.in(Set(VerdictOnly, CompleteDecision, BlankFindings, InvalidFindings))

  def findingsDigestPresentFor(decisionShape) =
    decisionShape.in(Set(FindingsOnly, CompleteDecision, BlankFindings, InvalidFindings))

  def rejectFor(lifecycle, decisionShape, reviewSubtaskShape, timestampShape) =
    if (reviewSubtaskShape.in(Set(MissingReviewSubtaskShape, BlankReviewSubtaskShape))) ReviewSubtaskMissing
    else if (reviewSubtaskShape == InvalidReviewSubtaskShape) ReviewSubtaskInvalid
    else if (timestampShape == UpdatedBeforeCreatedShape) UpdatedBeforeCreatedReject
    else if (lifecycle != Decided and decisionShape != NoDecision) NonDecidedCarriesDecisionEvidence
    else if (lifecycle == Decided and not(verdictPresentFor(decisionShape))) DecidedMissingVerdictReject
    else if (lifecycle == Decided and (not(findingsDigestPresentFor(decisionShape)) or decisionShape == BlankFindings)) DecidedMissingFindingsReject
    else if (lifecycle == Decided and decisionShape == InvalidFindings) FindingsDigestInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidRequested,
      lifecycle: Requested,
      decisionShape: NoDecision,
      reviewSubtaskShape: ValidReviewSubtask,
      timestampShape: Monotonic,
      verdictPresent: false,
      findingsDigestPresent: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val lifecycle = lifecycleFor(case)
    val decisionShape = decisionShapeFor(case)
    val reviewSubtaskShape = reviewSubtaskShapeFor(case)
    val timestampShape = timestampShapeFor(case)
    val rejectReason = rejectFor(lifecycle, decisionShape, reviewSubtaskShape, timestampShape)
    val accepted = rejectReason == NoReject
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      lifecycle: lifecycle,
      decisionShape: decisionShape,
      reviewSubtaskShape: reviewSubtaskShape,
      timestampShape: timestampShape,
      verdictPresent: verdictPresentFor(decisionShape),
      findingsDigestPresent: findingsDigestPresentFor(decisionShape),
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidRequested) },
    all { s.caseIndex == 1, evaluate(ValidInProgress) },
    all { s.caseIndex == 2, evaluate(ValidDecidedApprove) },
    all { s.caseIndex == 3, evaluate(ValidDecidedBlocked) },
    all { s.caseIndex == 4, evaluate(ValidSuperseded) },
    all { s.caseIndex == 5, evaluate(MissingReviewSubtask) },
    all { s.caseIndex == 6, evaluate(BlankReviewSubtask) },
    all { s.caseIndex == 7, evaluate(InvalidReviewSubtask) },
    all { s.caseIndex == 8, evaluate(RequestedWithVerdict) },
    all { s.caseIndex == 9, evaluate(InProgressWithFindings) },
    all { s.caseIndex == 10, evaluate(SupersededWithDecision) },
    all { s.caseIndex == 11, evaluate(DecidedMissingVerdict) },
    all { s.caseIndex == 12, evaluate(DecidedMissingFindings) },
    all { s.caseIndex == 13, evaluate(DecidedBlankFindings) },
    all { s.caseIndex == 14, evaluate(DecidedInvalidFindings) },
    all { s.caseIndex == 15, evaluate(UpdatedBeforeCreated) },
    all { s.caseIndex >= 16, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 16
    and s.case.in(Set(ValidRequested, ValidInProgress, ValidDecidedApprove, ValidDecidedBlocked, ValidSuperseded, MissingReviewSubtask, BlankReviewSubtask, InvalidReviewSubtask, RequestedWithVerdict, InProgressWithFindings, SupersededWithDecision, DecidedMissingVerdict, DecidedMissingFindings, DecidedBlankFindings, DecidedInvalidFindings, UpdatedBeforeCreated))
    and s.lifecycle.in(Set(Requested, InProgress, Decided, Superseded))
    and s.decisionShape.in(Set(NoDecision, VerdictOnly, FindingsOnly, CompleteDecision, BlankFindings, InvalidFindings))
    and s.reviewSubtaskShape.in(Set(ValidReviewSubtask, MissingReviewSubtaskShape, BlankReviewSubtaskShape, InvalidReviewSubtaskShape))
    and s.timestampShape.in(Set(Monotonic, UpdatedBeforeCreatedShape))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, ReviewSubtaskMissing, ReviewSubtaskInvalid, NonDecidedCarriesDecisionEvidence, DecidedMissingVerdictReject, DecidedMissingFindingsReject, FindingsDigestInvalid, UpdatedBeforeCreatedReject))

  val acceptedHasReviewSubtask =
    not(s.accepted) or s.reviewSubtaskShape == ValidReviewSubtask

  val acceptedTimestampsAreMonotonic =
    not(s.accepted) or s.timestampShape == Monotonic

  val decidedReviewsRequireCompleteValidDecisionEvidence =
    not(s.accepted and s.lifecycle == Decided)
    or s.decisionShape == CompleteDecision

  val nonDecidedReviewsRejectDecisionEvidence =
    not(s.accepted and s.lifecycle != Decided)
    or s.decisionShape == NoDecision

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor(
      s.lifecycle,
      s.decisionShape,
      s.reviewSubtaskShape,
      s.timestampShape,
    )
}

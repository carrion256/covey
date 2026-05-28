module CoveyApplyProofRowLifecycle {
  type Case =
    ReviewRequestedNoDecision
    | ReviewInProgressNoDecision
    | ReviewSupersededNoDecision
    | ReviewDecidedApproveFindings
    | ReviewDecidedBlockedFindings
    | ReviewDecidedMissingVerdict
    | ReviewDecidedMissingFindings
    | ReviewRequestedWithVerdict
    | ReviewInProgressWithFindings
    | ReviewSupersededWithDecision
    | QueueQueuedNoClaim
    | QueueQueuedLastFence
    | QueueQueuedActiveSession
    | QueueQueuedLeaseOnly
    | QueueQueuedInvalidLastFence
    | QueueInFlightComplete
    | QueueInFlightMissingSession
    | QueueInFlightMissingFence
    | QueueInFlightInvalidFence
    | QueueInFlightMissingLease
    | QueueAppliedFence
    | QueueAppliedMissingFence
    | QueueAppliedInvalidFence
    | QueueAppliedActiveSession
    | QueueSupersededNoClaim
    | QueueSupersededLastFence
    | QueueSupersededActiveClaim
    | QueueCancelledNoClaim
    | QueueCancelledLastFence
    | QueueCancelledLeaseOnly

  type RowKind = ReviewProofRow | ReadyQueueProofRow
  type ReviewState = Requested | InProgress | Decided | Superseded
  type ReadyQueueState = Queued | QueueInFlight | Applied | QueueSuperseded | Cancelled
  type Fence = NoFence | ZeroFence | PositiveFence
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | DecidedMissingVerdict
    | DecidedMissingFindingsDigest
    | NonDecidedReviewCarriesDecisionEvidence
    | TerminalQueueCarriesActiveClaimFields
    | InFlightMissingClaimedSession
    | InFlightMissingClaimFence
    | InFlightMissingLeaseDeadline
    | AppliedMissingClaimFence
    | AppliedCarriesActiveClaimFields
    | InvalidClaimFence

  type Model = {
    caseIndex: int,
    case: Case,
    rowKind: RowKind,
    reviewState: ReviewState,
    readyQueueState: ReadyQueueState,
    verdictPresent: bool,
    findingsDigestPresent: bool,
    claimedBySessionPresent: bool,
    claimFence: Fence,
    claimLeaseDeadlinePresent: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def isReviewCase(case) =
    case.in(Set(
      ReviewRequestedNoDecision,
      ReviewInProgressNoDecision,
      ReviewSupersededNoDecision,
      ReviewDecidedApproveFindings,
      ReviewDecidedBlockedFindings,
      ReviewDecidedMissingVerdict,
      ReviewDecidedMissingFindings,
      ReviewRequestedWithVerdict,
      ReviewInProgressWithFindings,
      ReviewSupersededWithDecision,
    ))

  def reviewStateFor(case) =
    if (case.in(Set(ReviewRequestedNoDecision, ReviewRequestedWithVerdict))) Requested
    else if (case.in(Set(ReviewInProgressNoDecision, ReviewInProgressWithFindings))) InProgress
    else if (case.in(Set(ReviewSupersededNoDecision, ReviewSupersededWithDecision))) Superseded
    else Decided

  def readyQueueStateFor(case) =
    if (case.in(Set(
      QueueQueuedNoClaim,
      QueueQueuedLastFence,
      QueueQueuedActiveSession,
      QueueQueuedLeaseOnly,
      QueueQueuedInvalidLastFence,
    ))) Queued
    else if (case.in(Set(
      QueueInFlightComplete,
      QueueInFlightMissingSession,
      QueueInFlightMissingFence,
      QueueInFlightInvalidFence,
      QueueInFlightMissingLease,
    ))) QueueInFlight
    else if (case.in(Set(
      QueueAppliedFence,
      QueueAppliedMissingFence,
      QueueAppliedInvalidFence,
      QueueAppliedActiveSession,
    ))) Applied
    else if (case.in(Set(
      QueueSupersededNoClaim,
      QueueSupersededLastFence,
      QueueSupersededActiveClaim,
    ))) QueueSuperseded
    else Cancelled

  def verdictPresentFor(case) =
    case.in(Set(
      ReviewDecidedApproveFindings,
      ReviewDecidedBlockedFindings,
      ReviewDecidedMissingFindings,
      ReviewRequestedWithVerdict,
      ReviewSupersededWithDecision,
    ))

  def findingsDigestPresentFor(case) =
    case.in(Set(
      ReviewDecidedApproveFindings,
      ReviewDecidedBlockedFindings,
      ReviewDecidedMissingVerdict,
      ReviewInProgressWithFindings,
      ReviewSupersededWithDecision,
    ))

  def claimedBySessionPresentFor(case) =
    case.in(Set(
      QueueQueuedActiveSession,
      QueueInFlightComplete,
      QueueInFlightMissingFence,
      QueueInFlightInvalidFence,
      QueueInFlightMissingLease,
      QueueAppliedActiveSession,
      QueueSupersededActiveClaim,
    ))

  def claimFenceFor(case) =
    if (case.in(Set(
      QueueQueuedLastFence,
      QueueInFlightComplete,
      QueueInFlightMissingLease,
      QueueAppliedFence,
      QueueAppliedActiveSession,
      QueueSupersededLastFence,
      QueueSupersededActiveClaim,
      QueueCancelledLastFence,
      QueueCancelledLeaseOnly,
    ))) PositiveFence
    else if (case.in(Set(
      QueueQueuedInvalidLastFence,
      QueueInFlightInvalidFence,
      QueueAppliedInvalidFence,
    ))) ZeroFence
    else NoFence

  def claimLeaseDeadlinePresentFor(case) =
    case.in(Set(
      QueueQueuedLeaseOnly,
      QueueInFlightComplete,
      QueueInFlightMissingSession,
      QueueInFlightMissingFence,
      QueueInFlightInvalidFence,
      QueueSupersededActiveClaim,
      QueueCancelledLeaseOnly,
    ))

  def reviewRejectFor(state, verdictPresent, findingsDigestPresent) =
    if (state == Decided and not(verdictPresent)) DecidedMissingVerdict
    else if (state == Decided and not(findingsDigestPresent)) DecidedMissingFindingsDigest
    else if (state != Decided and (verdictPresent or findingsDigestPresent)) NonDecidedReviewCarriesDecisionEvidence
    else NoReject

  def terminalQueueState(state) =
    state.in(Set(Queued, QueueSuperseded, Cancelled))

  def queueRejectFor(state, claimedBySessionPresent, claimFence, claimLeaseDeadlinePresent) =
    if (terminalQueueState(state) and (claimedBySessionPresent or claimLeaseDeadlinePresent)) TerminalQueueCarriesActiveClaimFields
    else if (terminalQueueState(state) and claimFence == ZeroFence) InvalidClaimFence
    else if (state == QueueInFlight and not(claimedBySessionPresent)) InFlightMissingClaimedSession
    else if (state == QueueInFlight and claimFence == NoFence) InFlightMissingClaimFence
    else if (state == QueueInFlight and claimFence == ZeroFence) InvalidClaimFence
    else if (state == QueueInFlight and not(claimLeaseDeadlinePresent)) InFlightMissingLeaseDeadline
    else if (state == Applied and (claimedBySessionPresent or claimLeaseDeadlinePresent)) AppliedCarriesActiveClaimFields
    else if (state == Applied and claimFence == NoFence) AppliedMissingClaimFence
    else if (state == Applied and claimFence == ZeroFence) InvalidClaimFence
    else NoReject

  def rejectFor(rowKind, reviewState, readyQueueState, verdictPresent, findingsDigestPresent, claimedBySessionPresent, claimFence, claimLeaseDeadlinePresent) =
    if (rowKind == ReviewProofRow) reviewRejectFor(reviewState, verdictPresent, findingsDigestPresent)
    else queueRejectFor(readyQueueState, claimedBySessionPresent, claimFence, claimLeaseDeadlinePresent)

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ReviewRequestedNoDecision,
      rowKind: ReviewProofRow,
      reviewState: Requested,
      readyQueueState: Queued,
      verdictPresent: false,
      findingsDigestPresent: false,
      claimedBySessionPresent: false,
      claimFence: NoFence,
      claimLeaseDeadlinePresent: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val rowKind = if (isReviewCase(case)) ReviewProofRow else ReadyQueueProofRow
    val reviewState = reviewStateFor(case)
    val readyQueueState = readyQueueStateFor(case)
    val verdictPresent = verdictPresentFor(case)
    val findingsDigestPresent = findingsDigestPresentFor(case)
    val claimedBySessionPresent = claimedBySessionPresentFor(case)
    val claimFence = claimFenceFor(case)
    val claimLeaseDeadlinePresent = claimLeaseDeadlinePresentFor(case)
    val rejectReason = rejectFor(
      rowKind,
      reviewState,
      readyQueueState,
      verdictPresent,
      findingsDigestPresent,
      claimedBySessionPresent,
      claimFence,
      claimLeaseDeadlinePresent,
    )
    val accepted = rejectReason == NoReject
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      rowKind: rowKind,
      reviewState: reviewState,
      readyQueueState: readyQueueState,
      verdictPresent: verdictPresent,
      findingsDigestPresent: findingsDigestPresent,
      claimedBySessionPresent: claimedBySessionPresent,
      claimFence: claimFence,
      claimLeaseDeadlinePresent: claimLeaseDeadlinePresent,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ReviewRequestedNoDecision) },
    all { s.caseIndex == 1, evaluate(ReviewInProgressNoDecision) },
    all { s.caseIndex == 2, evaluate(ReviewSupersededNoDecision) },
    all { s.caseIndex == 3, evaluate(ReviewDecidedApproveFindings) },
    all { s.caseIndex == 4, evaluate(ReviewDecidedBlockedFindings) },
    all { s.caseIndex == 5, evaluate(ReviewDecidedMissingVerdict) },
    all { s.caseIndex == 6, evaluate(ReviewDecidedMissingFindings) },
    all { s.caseIndex == 7, evaluate(ReviewRequestedWithVerdict) },
    all { s.caseIndex == 8, evaluate(ReviewInProgressWithFindings) },
    all { s.caseIndex == 9, evaluate(ReviewSupersededWithDecision) },
    all { s.caseIndex == 10, evaluate(QueueQueuedNoClaim) },
    all { s.caseIndex == 11, evaluate(QueueQueuedLastFence) },
    all { s.caseIndex == 12, evaluate(QueueQueuedActiveSession) },
    all { s.caseIndex == 13, evaluate(QueueQueuedLeaseOnly) },
    all { s.caseIndex == 14, evaluate(QueueQueuedInvalidLastFence) },
    all { s.caseIndex == 15, evaluate(QueueInFlightComplete) },
    all { s.caseIndex == 16, evaluate(QueueInFlightMissingSession) },
    all { s.caseIndex == 17, evaluate(QueueInFlightMissingFence) },
    all { s.caseIndex == 18, evaluate(QueueInFlightInvalidFence) },
    all { s.caseIndex == 19, evaluate(QueueInFlightMissingLease) },
    all { s.caseIndex == 20, evaluate(QueueAppliedFence) },
    all { s.caseIndex == 21, evaluate(QueueAppliedMissingFence) },
    all { s.caseIndex == 22, evaluate(QueueAppliedInvalidFence) },
    all { s.caseIndex == 23, evaluate(QueueAppliedActiveSession) },
    all { s.caseIndex == 24, evaluate(QueueSupersededNoClaim) },
    all { s.caseIndex == 25, evaluate(QueueSupersededLastFence) },
    all { s.caseIndex == 26, evaluate(QueueSupersededActiveClaim) },
    all { s.caseIndex == 27, evaluate(QueueCancelledNoClaim) },
    all { s.caseIndex == 28, evaluate(QueueCancelledLastFence) },
    all { s.caseIndex == 29, evaluate(QueueCancelledLeaseOnly) },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.rowKind.in(Set(ReviewProofRow, ReadyQueueProofRow))
    and s.reviewState.in(Set(Requested, InProgress, Decided, Superseded))
    and s.readyQueueState.in(Set(Queued, QueueInFlight, Applied, QueueSuperseded, Cancelled))
    and s.claimFence.in(Set(NoFence, ZeroFence, PositiveFence))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(
      NoReject,
      DecidedMissingVerdict,
      DecidedMissingFindingsDigest,
      NonDecidedReviewCarriesDecisionEvidence,
      TerminalQueueCarriesActiveClaimFields,
      InFlightMissingClaimedSession,
      InFlightMissingClaimFence,
      InFlightMissingLeaseDeadline,
      AppliedMissingClaimFence,
      AppliedCarriesActiveClaimFields,
      InvalidClaimFence,
    ))

  val decidedReviewCarriesCompleteDecisionEvidence =
    not(s.accepted)
    or s.rowKind != ReviewProofRow
    or s.reviewState != Decided
    or (s.verdictPresent and s.findingsDigestPresent)

  val nonDecidedReviewCarriesNoDecisionEvidence =
    not(s.accepted)
    or s.rowKind != ReviewProofRow
    or s.reviewState == Decided
    or (not(s.verdictPresent) and not(s.findingsDigestPresent))

  val inFlightQueueCarriesCompleteActiveClaim =
    not(s.accepted)
    or s.rowKind != ReadyQueueProofRow
    or s.readyQueueState != QueueInFlight
    or (
      s.claimedBySessionPresent
      and s.claimFence == PositiveFence
      and s.claimLeaseDeadlinePresent
    )

  val appliedQueueCarriesFenceOnly =
    not(s.accepted)
    or s.rowKind != ReadyQueueProofRow
    or s.readyQueueState != Applied
    or (
      not(s.claimedBySessionPresent)
      and s.claimFence == PositiveFence
      and not(s.claimLeaseDeadlinePresent)
    )

  val terminalQueueCarriesNoActiveClaimFields =
    not(s.accepted)
    or s.rowKind != ReadyQueueProofRow
    or not(terminalQueueState(s.readyQueueState))
    or (
      not(s.claimedBySessionPresent)
      and not(s.claimLeaseDeadlinePresent)
      and s.claimFence != ZeroFence
    )

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated)
    or s.rejectReason == rejectFor(
      s.rowKind,
      s.reviewState,
      s.readyQueueState,
      s.verdictPresent,
      s.findingsDigestPresent,
      s.claimedBySessionPresent,
      s.claimFence,
      s.claimLeaseDeadlinePresent,
    )
}

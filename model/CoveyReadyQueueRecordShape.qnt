module CoveyReadyQueueRecordShape {
  type Case =
    ValidQueuedNoFence
    | ValidQueuedWithFence
    | ValidInFlight
    | ValidApplied
    | ValidSupersededNoFence
    | ValidSupersededWithFence
    | ValidCancelledNoFence
    | ValidCancelledWithFence
    | QueuedWithActiveSession
    | QueuedWithLease
    | InFlightMissingSession
    | InFlightBlankSession
    | InFlightInvalidSession
    | InFlightMissingFence
    | InFlightMissingLease
    | AppliedMissingFence
    | AppliedWithActiveSession
    | AppliedWithLease
    | SupersededWithActiveSession
    | CancelledWithLease
    | UpdatedBeforeEnqueued
    | InvalidQueueId
    | InvalidArtifactDigest
    | InvalidSubtaskId
    | NonPositiveFence
    | NegativeLease

  type ReadyQueueLifecycle = Queued | InFlight | Applied | Superseded | Cancelled
  type IdentityShape = ValidIdentity | InvalidQueueIdShape | InvalidArtifactDigestShape | InvalidSubtaskIdShape
  type ClaimShape =
    NoClaim
    | FenceOnly
    | ActiveClaim
    | ActiveSessionOnly
    | ActiveSessionAndFence
    | ActiveSessionAndLease
    | MissingSession
    | BlankSession
    | InvalidSession
    | MissingFence
    | MissingLease
    | NonPositiveFenceShape
    | NegativeLeaseShape
  type TimestampShape = Monotonic | UpdatedBeforeEnqueuedShape
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | QueueIdentityInvalid
    | ArtifactDigestInvalid
    | SubtaskIdInvalid
    | UpdatedBeforeEnqueuedReject
    | ClaimFenceInvalid
    | ClaimLeaseInvalid
    | ActiveClaimNotAllowed
    | InFlightMissingSessionReject
    | InFlightInvalidSessionReject
    | InFlightMissingFenceReject
    | InFlightMissingLeaseReject
    | AppliedMissingFenceReject

  type Model = {
    caseIndex: int,
    case: Case,
    lifecycle: ReadyQueueLifecycle,
    identityShape: IdentityShape,
    claimShape: ClaimShape,
    timestampShape: TimestampShape,
    claimedBySessionPresent: bool,
    claimFencePresent: bool,
    claimLeaseDeadlinePresent: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def lifecycleFor(case) =
    if (case.in(Set(ValidInFlight, InFlightMissingSession, InFlightBlankSession, InFlightInvalidSession, InFlightMissingFence, InFlightMissingLease, NegativeLease))) InFlight
    else if (case.in(Set(ValidApplied, AppliedMissingFence, AppliedWithActiveSession, AppliedWithLease, NonPositiveFence))) Applied
    else if (case.in(Set(ValidSupersededNoFence, ValidSupersededWithFence, SupersededWithActiveSession))) Superseded
    else if (case.in(Set(ValidCancelledNoFence, ValidCancelledWithFence, CancelledWithLease))) Cancelled
    else Queued

  def identityShapeFor(case) =
    if (case == InvalidQueueId) InvalidQueueIdShape
    else if (case == InvalidArtifactDigest) InvalidArtifactDigestShape
    else if (case == InvalidSubtaskId) InvalidSubtaskIdShape
    else ValidIdentity

  def claimShapeFor(case) =
    if (case.in(Set(ValidQueuedWithFence, ValidApplied, ValidSupersededWithFence, ValidCancelledWithFence))) FenceOnly
    else if (case == ValidInFlight) ActiveClaim
    else if (case.in(Set(QueuedWithActiveSession, AppliedWithActiveSession, SupersededWithActiveSession))) ActiveSessionOnly
    else if (case.in(Set(QueuedWithLease, AppliedWithLease, CancelledWithLease))) ActiveSessionAndLease
    else if (case == InFlightMissingSession) MissingSession
    else if (case == InFlightBlankSession) BlankSession
    else if (case == InFlightInvalidSession) InvalidSession
    else if (case == InFlightMissingFence) MissingFence
    else if (case == InFlightMissingLease) MissingLease
    else if (case == NonPositiveFence) NonPositiveFenceShape
    else if (case == NegativeLease) NegativeLeaseShape
    else NoClaim

  def timestampShapeFor(case) =
    if (case == UpdatedBeforeEnqueued) UpdatedBeforeEnqueuedShape else Monotonic

  def claimedBySessionPresentFor(shape) =
    shape.in(Set(ActiveClaim, ActiveSessionOnly, ActiveSessionAndFence, ActiveSessionAndLease, BlankSession, InvalidSession, MissingFence, MissingLease, NegativeLeaseShape))

  def claimFencePresentFor(shape) =
    shape.in(Set(FenceOnly, ActiveClaim, ActiveSessionAndFence, ActiveSessionAndLease, MissingSession, BlankSession, InvalidSession, MissingLease, NonPositiveFenceShape, NegativeLeaseShape))

  def claimLeaseDeadlinePresentFor(shape) =
    shape.in(Set(ActiveClaim, ActiveSessionAndLease, MissingSession, BlankSession, InvalidSession, MissingFence, NonPositiveFenceShape, NegativeLeaseShape))

  def hasActiveClaimField(shape) =
    claimedBySessionPresentFor(shape) or claimLeaseDeadlinePresentFor(shape)

  def activeClaimForbidden(lifecycle) =
    lifecycle.in(Set(Queued, Applied, Superseded, Cancelled))

  def rejectFor(lifecycle, identityShape, claimShape, timestampShape) =
    if (identityShape == InvalidQueueIdShape) QueueIdentityInvalid
    else if (identityShape == InvalidArtifactDigestShape) ArtifactDigestInvalid
    else if (identityShape == InvalidSubtaskIdShape) SubtaskIdInvalid
    else if (timestampShape == UpdatedBeforeEnqueuedShape) UpdatedBeforeEnqueuedReject
    else if (claimShape == NonPositiveFenceShape) ClaimFenceInvalid
    else if (claimShape == NegativeLeaseShape) ClaimLeaseInvalid
    else if (activeClaimForbidden(lifecycle) and hasActiveClaimField(claimShape)) ActiveClaimNotAllowed
    else if (lifecycle == InFlight and claimShape.in(Set(MissingSession, BlankSession))) InFlightMissingSessionReject
    else if (lifecycle == InFlight and claimShape == MissingFence) InFlightMissingFenceReject
    else if (lifecycle == InFlight and claimShape == MissingLease) InFlightMissingLeaseReject
    else if (lifecycle == InFlight and claimShape == InvalidSession) InFlightInvalidSessionReject
    else if (lifecycle == Applied and not(claimFencePresentFor(claimShape))) AppliedMissingFenceReject
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidQueuedNoFence,
      lifecycle: Queued,
      identityShape: ValidIdentity,
      claimShape: NoClaim,
      timestampShape: Monotonic,
      claimedBySessionPresent: false,
      claimFencePresent: false,
      claimLeaseDeadlinePresent: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val lifecycle = lifecycleFor(case)
    val identityShape = identityShapeFor(case)
    val claimShape = claimShapeFor(case)
    val timestampShape = timestampShapeFor(case)
    val rejectReason = rejectFor(lifecycle, identityShape, claimShape, timestampShape)
    val accepted = rejectReason == NoReject
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      lifecycle: lifecycle,
      identityShape: identityShape,
      claimShape: claimShape,
      timestampShape: timestampShape,
      claimedBySessionPresent: claimedBySessionPresentFor(claimShape),
      claimFencePresent: claimFencePresentFor(claimShape),
      claimLeaseDeadlinePresent: claimLeaseDeadlinePresentFor(claimShape),
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidQueuedNoFence) },
    all { s.caseIndex == 1, evaluate(ValidQueuedWithFence) },
    all { s.caseIndex == 2, evaluate(ValidInFlight) },
    all { s.caseIndex == 3, evaluate(ValidApplied) },
    all { s.caseIndex == 4, evaluate(ValidSupersededNoFence) },
    all { s.caseIndex == 5, evaluate(ValidSupersededWithFence) },
    all { s.caseIndex == 6, evaluate(ValidCancelledNoFence) },
    all { s.caseIndex == 7, evaluate(ValidCancelledWithFence) },
    all { s.caseIndex == 8, evaluate(QueuedWithActiveSession) },
    all { s.caseIndex == 9, evaluate(QueuedWithLease) },
    all { s.caseIndex == 10, evaluate(InFlightMissingSession) },
    all { s.caseIndex == 11, evaluate(InFlightBlankSession) },
    all { s.caseIndex == 12, evaluate(InFlightInvalidSession) },
    all { s.caseIndex == 13, evaluate(InFlightMissingFence) },
    all { s.caseIndex == 14, evaluate(InFlightMissingLease) },
    all { s.caseIndex == 15, evaluate(AppliedMissingFence) },
    all { s.caseIndex == 16, evaluate(AppliedWithActiveSession) },
    all { s.caseIndex == 17, evaluate(AppliedWithLease) },
    all { s.caseIndex == 18, evaluate(SupersededWithActiveSession) },
    all { s.caseIndex == 19, evaluate(CancelledWithLease) },
    all { s.caseIndex == 20, evaluate(UpdatedBeforeEnqueued) },
    all { s.caseIndex == 21, evaluate(InvalidQueueId) },
    all { s.caseIndex == 22, evaluate(InvalidArtifactDigest) },
    all { s.caseIndex == 23, evaluate(InvalidSubtaskId) },
    all { s.caseIndex == 24, evaluate(NonPositiveFence) },
    all { s.caseIndex == 25, evaluate(NegativeLease) },
    all { s.caseIndex >= 26, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 26
    and s.case.in(Set(ValidQueuedNoFence, ValidQueuedWithFence, ValidInFlight, ValidApplied, ValidSupersededNoFence, ValidSupersededWithFence, ValidCancelledNoFence, ValidCancelledWithFence, QueuedWithActiveSession, QueuedWithLease, InFlightMissingSession, InFlightBlankSession, InFlightInvalidSession, InFlightMissingFence, InFlightMissingLease, AppliedMissingFence, AppliedWithActiveSession, AppliedWithLease, SupersededWithActiveSession, CancelledWithLease, UpdatedBeforeEnqueued, InvalidQueueId, InvalidArtifactDigest, InvalidSubtaskId, NonPositiveFence, NegativeLease))
    and s.lifecycle.in(Set(Queued, InFlight, Applied, Superseded, Cancelled))
    and s.identityShape.in(Set(ValidIdentity, InvalidQueueIdShape, InvalidArtifactDigestShape, InvalidSubtaskIdShape))
    and s.claimShape.in(Set(NoClaim, FenceOnly, ActiveClaim, ActiveSessionOnly, ActiveSessionAndFence, ActiveSessionAndLease, MissingSession, BlankSession, InvalidSession, MissingFence, MissingLease, NonPositiveFenceShape, NegativeLeaseShape))
    and s.timestampShape.in(Set(Monotonic, UpdatedBeforeEnqueuedShape))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, QueueIdentityInvalid, ArtifactDigestInvalid, SubtaskIdInvalid, UpdatedBeforeEnqueuedReject, ClaimFenceInvalid, ClaimLeaseInvalid, ActiveClaimNotAllowed, InFlightMissingSessionReject, InFlightInvalidSessionReject, InFlightMissingFenceReject, InFlightMissingLeaseReject, AppliedMissingFenceReject))

  val acceptedTimestampsAreMonotonic =
    not(s.accepted) or s.timestampShape == Monotonic

  val acceptedInFlightHasCompleteActiveClaim =
    not(s.accepted and s.lifecycle == InFlight)
    or (s.claimedBySessionPresent and s.claimFencePresent and s.claimLeaseDeadlinePresent)

  val acceptedNonInFlightHasNoActiveClaimFields =
    not(s.accepted and s.lifecycle != InFlight)
    or (not(s.claimedBySessionPresent) and not(s.claimLeaseDeadlinePresent))

  val acceptedAppliedHasClaimFence =
    not(s.accepted and s.lifecycle == Applied) or s.claimFencePresent

  val acceptedQueuedSupersededCancelledFenceIsOnlyHistorical =
    not(s.accepted and s.lifecycle.in(Set(Queued, Superseded, Cancelled)))
    or not(s.claimLeaseDeadlinePresent)

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor(
      s.lifecycle,
      s.identityShape,
      s.claimShape,
      s.timestampShape,
    )
}

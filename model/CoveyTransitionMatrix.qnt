module CoveyTransitionMatrix {
  type Case =
    WorkAvailableClaimed
    | WorkClaimedInProgress
    | WorkInProgressArtifactPublished
    | WorkRepublishArtifact
    | WorkReviewPendingArtifactPublished
    | WorkArtifactPublishedReviewPending
    | WorkReviewPendingChangesRequested
    | WorkReviewPendingBlocked
    | WorkReviewPendingApproved
    | WorkApprovedReadyForApply
    | WorkReadyForApplyApplied
    | ReviewAvailableClaimed
    | ReviewClaimedInProgress
    | ReviewInProgressDecided
    | ReviewRequestedInProgress
    | ReviewRequestedSuperseded
    | ReviewInProgressReviewDecided
    | ReviewInProgressSuperseded
    | QueueQueuedInFlight
    | QueueInFlightApplied
    | QueueQueuedSuperseded
    | QueueInFlightSuperseded
    | QueueQueuedCancelled
    | QueueInFlightCancelled
    | ConflictOpenAcknowledged
    | ConflictOpenResolved
    | ConflictAcknowledgedAcknowledged
    | ConflictAcknowledgedResolved
    | ConflictResolvedResolved
    | ReservationActiveReleased
    | ReservationActiveExpired
    | IllegalWorkApprovedApplied
    | IllegalWorkChangesRequestedClaimed
    | IllegalWorkBlockedClaimed
    | IllegalReviewAvailableInProgress
    | IllegalReviewDecidedInProgress
    | IllegalQueueAppliedQueued
    | IllegalQueueCancelledInFlight
    | IllegalConflictResolvedAcknowledged
    | IllegalReservationReleasedExpired

  type Object = WorkSubtask | ReviewSubtask | ReviewObject | ReadyQueue | Conflict | Reservation
  type StateValue =
    Available
    | Claimed
    | InProgress
    | ArtifactPublished
    | ReviewPending
    | ChangesRequested
    | Blocked
    | Approved
    | ReadyForApply
    | Applied
    | Decided
    | ReviewRequested
    | ReviewInProgress
    | ReviewDecided
    | ReviewSuperseded
    | QueueQueued
    | QueueInFlight
    | QueueApplied
    | QueueSuperseded
    | QueueCancelled
    | ConflictOpen
    | ConflictAcknowledged
    | ConflictResolved
    | ReservationActive
    | ReservationReleased
    | ReservationExpired
  type Outcome = NotEvaluated | Allowed | Rejected

  type Model = {
    caseIndex: int,
    case: Case,
    object: Object,
    from: StateValue,
    to: StateValue,
    allowedByMatrix: bool,
    outcome: Outcome,
    evaluated: bool,
  }

  var s: Model

  def factsForCase(case) =
    if (case == WorkAvailableClaimed) { object: WorkSubtask, from: Available, to: Claimed, allowed: true }
    else if (case == WorkClaimedInProgress) { object: WorkSubtask, from: Claimed, to: InProgress, allowed: true }
    else if (case == WorkInProgressArtifactPublished) { object: WorkSubtask, from: InProgress, to: ArtifactPublished, allowed: true }
    else if (case == WorkRepublishArtifact) { object: WorkSubtask, from: ArtifactPublished, to: ArtifactPublished, allowed: true }
    else if (case == WorkReviewPendingArtifactPublished) { object: WorkSubtask, from: ReviewPending, to: ArtifactPublished, allowed: true }
    else if (case == WorkArtifactPublishedReviewPending) { object: WorkSubtask, from: ArtifactPublished, to: ReviewPending, allowed: true }
    else if (case == WorkReviewPendingChangesRequested) { object: WorkSubtask, from: ReviewPending, to: ChangesRequested, allowed: true }
    else if (case == WorkReviewPendingBlocked) { object: WorkSubtask, from: ReviewPending, to: Blocked, allowed: true }
    else if (case == WorkReviewPendingApproved) { object: WorkSubtask, from: ReviewPending, to: Approved, allowed: true }
    else if (case == WorkApprovedReadyForApply) { object: WorkSubtask, from: Approved, to: ReadyForApply, allowed: true }
    else if (case == WorkReadyForApplyApplied) { object: WorkSubtask, from: ReadyForApply, to: Applied, allowed: true }
    else if (case == ReviewAvailableClaimed) { object: ReviewSubtask, from: Available, to: Claimed, allowed: true }
    else if (case == ReviewClaimedInProgress) { object: ReviewSubtask, from: Claimed, to: InProgress, allowed: true }
    else if (case == ReviewInProgressDecided) { object: ReviewSubtask, from: InProgress, to: Decided, allowed: true }
    else if (case == ReviewRequestedInProgress) { object: ReviewObject, from: ReviewRequested, to: ReviewInProgress, allowed: true }
    else if (case == ReviewRequestedSuperseded) { object: ReviewObject, from: ReviewRequested, to: ReviewSuperseded, allowed: true }
    else if (case == ReviewInProgressReviewDecided) { object: ReviewObject, from: ReviewInProgress, to: ReviewDecided, allowed: true }
    else if (case == ReviewInProgressSuperseded) { object: ReviewObject, from: ReviewInProgress, to: ReviewSuperseded, allowed: true }
    else if (case == QueueQueuedInFlight) { object: ReadyQueue, from: QueueQueued, to: QueueInFlight, allowed: true }
    else if (case == QueueInFlightApplied) { object: ReadyQueue, from: QueueInFlight, to: QueueApplied, allowed: true }
    else if (case == QueueQueuedSuperseded) { object: ReadyQueue, from: QueueQueued, to: QueueSuperseded, allowed: true }
    else if (case == QueueInFlightSuperseded) { object: ReadyQueue, from: QueueInFlight, to: QueueSuperseded, allowed: true }
    else if (case == QueueQueuedCancelled) { object: ReadyQueue, from: QueueQueued, to: QueueCancelled, allowed: true }
    else if (case == QueueInFlightCancelled) { object: ReadyQueue, from: QueueInFlight, to: QueueCancelled, allowed: true }
    else if (case == ConflictOpenAcknowledged) { object: Conflict, from: ConflictOpen, to: ConflictAcknowledged, allowed: true }
    else if (case == ConflictOpenResolved) { object: Conflict, from: ConflictOpen, to: ConflictResolved, allowed: true }
    else if (case == ConflictAcknowledgedAcknowledged) { object: Conflict, from: ConflictAcknowledged, to: ConflictAcknowledged, allowed: true }
    else if (case == ConflictAcknowledgedResolved) { object: Conflict, from: ConflictAcknowledged, to: ConflictResolved, allowed: true }
    else if (case == ConflictResolvedResolved) { object: Conflict, from: ConflictResolved, to: ConflictResolved, allowed: true }
    else if (case == ReservationActiveReleased) { object: Reservation, from: ReservationActive, to: ReservationReleased, allowed: true }
    else if (case == ReservationActiveExpired) { object: Reservation, from: ReservationActive, to: ReservationExpired, allowed: true }
    else if (case == IllegalWorkApprovedApplied) { object: WorkSubtask, from: Approved, to: Applied, allowed: false }
    else if (case == IllegalWorkChangesRequestedClaimed) { object: WorkSubtask, from: ChangesRequested, to: Claimed, allowed: false }
    else if (case == IllegalWorkBlockedClaimed) { object: WorkSubtask, from: Blocked, to: Claimed, allowed: false }
    else if (case == IllegalReviewAvailableInProgress) { object: ReviewSubtask, from: Available, to: InProgress, allowed: false }
    else if (case == IllegalReviewDecidedInProgress) { object: ReviewObject, from: ReviewDecided, to: ReviewInProgress, allowed: false }
    else if (case == IllegalQueueAppliedQueued) { object: ReadyQueue, from: QueueApplied, to: QueueQueued, allowed: false }
    else if (case == IllegalQueueCancelledInFlight) { object: ReadyQueue, from: QueueCancelled, to: QueueInFlight, allowed: false }
    else if (case == IllegalConflictResolvedAcknowledged) { object: Conflict, from: ConflictResolved, to: ConflictAcknowledged, allowed: false }
    else { object: Reservation, from: ReservationReleased, to: ReservationExpired, allowed: false }

  def expectedAllowed(state) =
    if (state.object == WorkSubtask) {
      (state.from == Available and state.to == Claimed)
      or (state.from == Claimed and state.to == InProgress)
      or (state.from == InProgress and state.to == ArtifactPublished)
      or (state.from == ArtifactPublished and state.to == ArtifactPublished)
      or (state.from == ReviewPending and state.to == ArtifactPublished)
      or (state.from == ArtifactPublished and state.to == ReviewPending)
      or (state.from == ReviewPending and state.to == ChangesRequested)
      or (state.from == ReviewPending and state.to == Blocked)
      or (state.from == ReviewPending and state.to == Approved)
      or (state.from == Approved and state.to == ReadyForApply)
      or (state.from == ReadyForApply and state.to == Applied)
    } else if (state.object == ReviewSubtask) {
      (state.from == Available and state.to == Claimed)
      or (state.from == Claimed and state.to == InProgress)
      or (state.from == InProgress and state.to == Decided)
    } else if (state.object == ReviewObject) {
      (state.from == ReviewRequested and state.to == ReviewInProgress)
      or (state.from == ReviewRequested and state.to == ReviewSuperseded)
      or (state.from == ReviewInProgress and state.to == ReviewDecided)
      or (state.from == ReviewInProgress and state.to == ReviewSuperseded)
    } else if (state.object == ReadyQueue) {
      (state.from == QueueQueued and state.to == QueueInFlight)
      or (state.from == QueueInFlight and state.to == QueueApplied)
      or (state.from == QueueQueued and state.to == QueueSuperseded)
      or (state.from == QueueInFlight and state.to == QueueSuperseded)
      or (state.from == QueueQueued and state.to == QueueCancelled)
      or (state.from == QueueInFlight and state.to == QueueCancelled)
    } else if (state.object == Conflict) {
      (state.from == ConflictOpen and state.to == ConflictAcknowledged)
      or (state.from == ConflictOpen and state.to == ConflictResolved)
      or (state.from == ConflictAcknowledged and state.to == ConflictAcknowledged)
      or (state.from == ConflictAcknowledged and state.to == ConflictResolved)
      or (state.from == ConflictResolved and state.to == ConflictResolved)
    } else {
      (state.from == ReservationActive and state.to == ReservationReleased)
      or (state.from == ReservationActive and state.to == ReservationExpired)
    }

  action init = {
    s' = {
      caseIndex: 0,
      case: WorkAvailableClaimed,
      object: WorkSubtask,
      from: Available,
      to: Available,
      allowedByMatrix: false,
      outcome: NotEvaluated,
      evaluated: false,
    }
  }

  action evaluate(caseIndex, case) = all {
    val facts = factsForCase(case)
    s' = {
      caseIndex: caseIndex,
      case: case,
      object: facts.object,
      from: facts.from,
      to: facts.to,
      allowedByMatrix: facts.allowed,
      outcome: if (facts.allowed) Allowed else Rejected,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(1, WorkAvailableClaimed) },
    all { s.caseIndex == 1, evaluate(2, WorkClaimedInProgress) },
    all { s.caseIndex == 2, evaluate(3, WorkInProgressArtifactPublished) },
    all { s.caseIndex == 3, evaluate(4, WorkRepublishArtifact) },
    all { s.caseIndex == 4, evaluate(5, WorkReviewPendingArtifactPublished) },
    all { s.caseIndex == 5, evaluate(6, WorkArtifactPublishedReviewPending) },
    all { s.caseIndex == 6, evaluate(7, WorkReviewPendingChangesRequested) },
    all { s.caseIndex == 7, evaluate(8, WorkReviewPendingBlocked) },
    all { s.caseIndex == 8, evaluate(9, WorkReviewPendingApproved) },
    all { s.caseIndex == 9, evaluate(10, WorkApprovedReadyForApply) },
    all { s.caseIndex == 10, evaluate(11, WorkReadyForApplyApplied) },
    all { s.caseIndex == 11, evaluate(12, ReviewAvailableClaimed) },
    all { s.caseIndex == 12, evaluate(13, ReviewClaimedInProgress) },
    all { s.caseIndex == 13, evaluate(14, ReviewInProgressDecided) },
    all { s.caseIndex == 14, evaluate(15, ReviewRequestedInProgress) },
    all { s.caseIndex == 15, evaluate(16, ReviewRequestedSuperseded) },
    all { s.caseIndex == 16, evaluate(17, ReviewInProgressReviewDecided) },
    all { s.caseIndex == 17, evaluate(18, ReviewInProgressSuperseded) },
    all { s.caseIndex == 18, evaluate(19, QueueQueuedInFlight) },
    all { s.caseIndex == 19, evaluate(20, QueueInFlightApplied) },
    all { s.caseIndex == 20, evaluate(21, QueueQueuedSuperseded) },
    all { s.caseIndex == 21, evaluate(22, QueueInFlightSuperseded) },
    all { s.caseIndex == 22, evaluate(23, QueueQueuedCancelled) },
    all { s.caseIndex == 23, evaluate(24, QueueInFlightCancelled) },
    all { s.caseIndex == 24, evaluate(25, ConflictOpenAcknowledged) },
    all { s.caseIndex == 25, evaluate(26, ConflictOpenResolved) },
    all { s.caseIndex == 26, evaluate(27, ConflictAcknowledgedAcknowledged) },
    all { s.caseIndex == 27, evaluate(28, ConflictAcknowledgedResolved) },
    all { s.caseIndex == 28, evaluate(29, ConflictResolvedResolved) },
    all { s.caseIndex == 29, evaluate(30, ReservationActiveReleased) },
    all { s.caseIndex == 30, evaluate(31, ReservationActiveExpired) },
    all { s.caseIndex == 31, evaluate(32, IllegalWorkApprovedApplied) },
    all { s.caseIndex == 32, evaluate(33, IllegalWorkChangesRequestedClaimed) },
    all { s.caseIndex == 33, evaluate(34, IllegalWorkBlockedClaimed) },
    all { s.caseIndex == 34, evaluate(35, IllegalReviewAvailableInProgress) },
    all { s.caseIndex == 35, evaluate(36, IllegalReviewDecidedInProgress) },
    all { s.caseIndex == 36, evaluate(37, IllegalQueueAppliedQueued) },
    all { s.caseIndex == 37, evaluate(38, IllegalQueueCancelledInFlight) },
    all { s.caseIndex == 38, evaluate(39, IllegalConflictResolvedAcknowledged) },
    all { s.caseIndex == 39, evaluate(40, IllegalReservationReleasedExpired) },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.outcome.in(Set(NotEvaluated, Allowed, Rejected))

  val outcomeMatchesTransitionMatrix =
    not(s.evaluated)
    or (s.allowedByMatrix == expectedAllowed(s) and s.outcome == (if (expectedAllowed(s)) Allowed else Rejected))

  val workCannotBypassReadyForApply =
    not(s.allowedByMatrix and s.object == WorkSubtask and s.to == Applied) or s.from == ReadyForApply

  val failedWorkOutcomesAreTerminal =
    not(s.object == WorkSubtask and s.from.in(Set(ChangesRequested, Blocked))) or not(s.allowedByMatrix)

  val reviewSubtaskCannotBypassClaimed =
    not(s.allowedByMatrix and s.object == ReviewSubtask and s.to == InProgress) or s.from == Claimed

  val decidedReviewIsTerminal =
    not(s.object == ReviewObject and s.from == ReviewDecided) or not(s.allowedByMatrix)

  val appliedOrCancelledQueueIsTerminal =
    not(s.object == ReadyQueue and s.from.in(Set(QueueApplied, QueueCancelled))) or not(s.allowedByMatrix)

  val conflictResolutionNeverDowngrades =
    not(s.allowedByMatrix and s.object == Conflict and s.from == ConflictResolved) or s.to == ConflictResolved

  val reservationTerminalStatesDoNotTransition =
    not(s.object == Reservation and s.from.in(Set(ReservationReleased, ReservationExpired))) or not(s.allowedByMatrix)
}

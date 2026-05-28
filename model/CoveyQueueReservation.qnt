module CoveyQueueReservation {
  type QueueState = NoQueue | Queued | InFlight | Applied | Superseded | Cancelled
  type ReservationState = NoReservation | Active | Released | Expired
  type ConflictState = NoConflict | Open | Acknowledged | Resolved
  type Fence = F0 | F1 | F2 | F3

  type Model = {
    queue: QueueState,
    reservation: ReservationState,
    conflict: ConflictState,
    queueFence: Fence,
    queueClaimLive: bool,
    queueLeaseLive: bool,
    applyVerified: bool,
    subtaskReady: bool,
    artifactMatches: bool,
    metaSchedulable: bool,
    reservationLeaseLive: bool,
    overlapDetected: bool,
    conflictBound: bool,
    conflictRankFloor: int,
  }

  var s: Model

  def terminalQueue(state) = state.queue.in(Set(Applied, Superseded, Cancelled))
  def activeQueueClaim(state) = state.queue == InFlight
  def nextFence(f) = if (f == F0) F1 else if (f == F1) F2 else F3
  def conflictRank(c) =
    if (c == NoConflict) 0
    else if (c == Open) 0
    else if (c == Acknowledged) 1
    else 2

  action init = {
    s' = {
      queue: NoQueue,
      reservation: NoReservation,
      conflict: NoConflict,
      queueFence: F0,
      queueClaimLive: false,
      queueLeaseLive: false,
      applyVerified: false,
      subtaskReady: false,
      artifactMatches: false,
      metaSchedulable: true,
      reservationLeaseLive: false,
      overlapDetected: false,
      conflictBound: false,
      conflictRankFloor: 0,
    }
  }

  action enqueueForApply = all {
    s.queue.in(Set(NoQueue, Superseded, Applied, Cancelled)),
    s' = s
      .with("queue", Queued)
      .with("queueClaimLive", false)
      .with("queueLeaseLive", false)
      .with("applyVerified", false)
      .with("subtaskReady", true)
      .with("artifactMatches", true)
      .with("metaSchedulable", true),
  }

  action invalidateQueuedArtifact = all {
    s.queue == Queued,
    s' = s
      .with("queue", Superseded)
      .with("artifactMatches", false)
      .with("queueClaimLive", false)
      .with("queueLeaseLive", false),
  }

  action cancelQueuedForUnavailableMetaTask = all {
    s.queue == Queued,
    s' = s
      .with("queue", Cancelled)
      .with("metaSchedulable", false)
      .with("queueClaimLive", false)
      .with("queueLeaseLive", false),
  }

  action claimQueued = all {
    s.queue == Queued,
    s.subtaskReady,
    s.artifactMatches,
    s.metaSchedulable,
    s' = s
      .with("queue", InFlight)
      .with("queueFence", nextFence(s.queueFence))
      .with("queueClaimLive", true)
      .with("queueLeaseLive", true),
  }

  action requeueExpiredClaim = all {
    s.queue == InFlight,
    s.queueClaimLive,
    s' = s
      .with("queue", Queued)
      .with("queueClaimLive", false)
      .with("queueLeaseLive", false)
      .with("applyVerified", false),
  }

  action supersedeOpenQueue = all {
    s.queue.in(Set(Queued, InFlight)),
    s' = s
      .with("queue", Superseded)
      .with("queueClaimLive", false)
      .with("queueLeaseLive", false),
  }

  action recordApplyVerification = all {
    s.queue == InFlight,
    s.queueClaimLive,
    s.queueLeaseLive,
    s.subtaskReady,
    s.artifactMatches,
    s' = s.with("applyVerified", true),
  }

  action markApplied = all {
    s.queue == InFlight,
    s.queueClaimLive,
    s.queueLeaseLive,
    s.applyVerified,
    s.subtaskReady,
    s.artifactMatches,
    s' = s
      .with("queue", Applied)
      .with("queueClaimLive", false)
      .with("queueLeaseLive", false),
  }

  action requestReservation = all {
    s.reservation.in(Set(NoReservation, Released, Expired)),
    s' = s
      .with("reservation", Active)
      .with("reservationLeaseLive", true)
      .with("overlapDetected", false)
      .with("conflictBound", false)
      .with("conflict", NoConflict)
      .with("conflictRankFloor", 0),
  }

  action detectReservationOverlap = all {
    s.reservation == Active,
    s.reservationLeaseLive,
    s.conflict == NoConflict,
    s' = s
      .with("overlapDetected", true)
      .with("conflictBound", true)
      .with("conflict", Open),
  }

  action acknowledgeConflict = all {
    s.conflict == Open,
    s.conflictBound,
    s' = s
      .with("conflict", Acknowledged)
      .with("conflictRankFloor", 1),
  }

  action resolveConflict = all {
    s.conflict.in(Set(Open, Acknowledged)),
    s.conflictBound,
    s' = s
      .with("conflict", Resolved)
      .with("conflictRankFloor", 2),
  }

  action releaseReservation = all {
    s.reservation == Active,
    s' = s
      .with("reservation", Released)
      .with("reservationLeaseLive", false)
      .with("conflict", if (s.conflict.in(Set(Open, Acknowledged))) Resolved else s.conflict)
      .with("conflictRankFloor", if (s.conflict.in(Set(Open, Acknowledged))) 2 else s.conflictRankFloor),
  }

  action expireReservation = all {
    s.reservation == Active,
    s' = s
      .with("reservation", Expired)
      .with("reservationLeaseLive", false)
      .with("conflict", if (s.conflict.in(Set(Open, Acknowledged))) Resolved else s.conflict)
      .with("conflictRankFloor", if (s.conflict.in(Set(Open, Acknowledged))) 2 else s.conflictRankFloor),
  }

  action step = any {
    enqueueForApply,
    invalidateQueuedArtifact,
    cancelQueuedForUnavailableMetaTask,
    claimQueued,
    requeueExpiredClaim,
    supersedeOpenQueue,
    recordApplyVerification,
    markApplied,
    requestReservation,
    detectReservationOverlap,
    acknowledgeConflict,
    resolveConflict,
    releaseReservation,
    expireReservation,
  }

  val typeOk = true
  val queueClaimOnlyInFlight = s.queueClaimLive iff activeQueueClaim(s)
  val inFlightQueueHasLeaseAndFence = s.queue != InFlight or (s.queueLeaseLive and s.queueFence != F0)
  val terminalQueueHasNoLiveClaim = not(terminalQueue(s)) or (not(s.queueClaimLive) and not(s.queueLeaseLive))
  val appliedQueueRequiresVerification = s.queue != Applied or s.applyVerified
  val cancelledQueueIsTerminal = s.queue != Cancelled or (not(s.queueClaimLive) and not(s.queueLeaseLive))
  val supersededQueueIsTerminal = s.queue != Superseded or (not(s.queueClaimLive) and not(s.queueLeaseLive))
  val activeReservationHasLiveLease = s.reservation != Active or s.reservationLeaseLive
  val terminalReservationHasNoLiveLease = not(s.reservation.in(Set(Released, Expired))) or not(s.reservationLeaseLive)
  val unresolvedConflictRequiresActiveReservation = not(s.conflict.in(Set(Open, Acknowledged))) or (s.reservation == Active and s.overlapDetected)
  val conflictRequiresRecordedOverlap = s.conflict == NoConflict or (s.overlapDetected and s.conflictBound)
  val conflictResolutionIsMonotonic = conflictRank(s.conflict) >= s.conflictRankFloor
  val resolvedConflictCannotDowngrade = s.conflictRankFloor < 2 or s.conflict == Resolved
}

module CoveyLandingReceipt {
  type QueueState = NoQueue | Queued | InFlight | Applied | Superseded
  type ReceiptState = NoReceipt | ReceiptRecorded
  type ReceiptAttempt = NoAttempt | Accepted | ReplayedSame | Rejected
  type TargetRef = NoTarget | TargetMain | TargetRelease
  type CommitOid = NoCommit | CommitA | CommitB

  type Model = {
    queue: QueueState,
    receipt: ReceiptState,
    lastAttempt: ReceiptAttempt,
    actorAuthorized: bool,
    artifactMatches: bool,
    fenceMatches: bool,
    receiptActorAuthorized: bool,
    receiptArtifactMatches: bool,
    receiptFenceMatches: bool,
    receiptTarget: TargetRef,
    receiptCommit: CommitOid,
    attemptedTarget: TargetRef,
    attemptedCommit: CommitOid,
    receiptCreatedByLastAttempt: bool,
    divergentAttemptRejected: bool,
  }

  var s: Model

  def terminalQueue(state) =
    state.queue.in(Set(Applied, Superseded))

  def sameReceipt(state) =
    state.receipt == ReceiptRecorded
    and state.receiptTarget == state.attemptedTarget
    and state.receiptCommit == state.attemptedCommit

  def divergentReceipt(state) =
    state.receipt == ReceiptRecorded
    and state.attemptedTarget != NoTarget
    and state.attemptedCommit != NoCommit
    and (state.receiptTarget != state.attemptedTarget or state.receiptCommit != state.attemptedCommit)

  def receiptPreconditions(state) =
    state.queue == Applied
    and state.actorAuthorized
    and state.artifactMatches
    and state.fenceMatches
    and state.attemptedTarget != NoTarget
    and state.attemptedCommit != NoCommit

  action init = {
    s' = {
      queue: NoQueue,
      receipt: NoReceipt,
      lastAttempt: NoAttempt,
      actorAuthorized: false,
      artifactMatches: false,
      fenceMatches: false,
      receiptActorAuthorized: false,
      receiptArtifactMatches: false,
      receiptFenceMatches: false,
      receiptTarget: NoTarget,
      receiptCommit: NoCommit,
      attemptedTarget: NoTarget,
      attemptedCommit: NoCommit,
      receiptCreatedByLastAttempt: false,
      divergentAttemptRejected: false,
    }
  }

  action enqueue = all {
    s.queue == NoQueue,
    s' = s.with("queue", Queued),
  }

  action markInFlight = all {
    s.queue == Queued,
    s' = s.with("queue", InFlight),
  }

  action markApplied = all {
    s.queue == InFlight,
    s' = s
      .with("queue", Applied)
      .with("artifactMatches", true)
      .with("fenceMatches", true),
  }

  action supersedeOpenQueue = all {
    s.queue.in(Set(Queued, InFlight)),
    s' = s.with("queue", Superseded),
  }

  action attemptReceipt(target, commit, actorAuthorized, artifactMatches, fenceMatches) = all {
    val attemptedState = s
      .with("attemptedTarget", target)
      .with("attemptedCommit", commit)
      .with("actorAuthorized", actorAuthorized)
      .with("artifactMatches", artifactMatches)
      .with("fenceMatches", fenceMatches)
    s' =
      if (attemptedState.receipt == NoReceipt and receiptPreconditions(attemptedState)) {
        attemptedState
          .with("receipt", ReceiptRecorded)
          .with("receiptActorAuthorized", actorAuthorized)
          .with("receiptArtifactMatches", artifactMatches)
          .with("receiptFenceMatches", fenceMatches)
          .with("receiptTarget", target)
          .with("receiptCommit", commit)
          .with("lastAttempt", Accepted)
          .with("receiptCreatedByLastAttempt", true)
          .with("divergentAttemptRejected", false)
      } else if (receiptPreconditions(attemptedState) and sameReceipt(attemptedState)) {
        attemptedState
          .with("lastAttempt", ReplayedSame)
          .with("receiptCreatedByLastAttempt", false)
          .with("divergentAttemptRejected", false)
      } else {
        attemptedState
          .with("lastAttempt", Rejected)
          .with("receiptCreatedByLastAttempt", false)
          .with("divergentAttemptRejected", divergentReceipt(attemptedState))
      },
  }

  action step = any {
    enqueue,
    markInFlight,
    markApplied,
    supersedeOpenQueue,
    attemptReceipt(TargetMain, CommitA, true, true, true),
    attemptReceipt(TargetMain, CommitB, true, true, true),
    attemptReceipt(TargetRelease, CommitA, true, true, true),
    attemptReceipt(TargetMain, CommitA, false, true, true),
    attemptReceipt(TargetMain, CommitA, true, false, true),
    attemptReceipt(TargetMain, CommitA, true, true, false),
  }

  val typeOk = true

  val receiptOnlyAfterAppliedQueue =
    s.receipt != ReceiptRecorded or s.queue == Applied

  val receiptRequiresAuthorizedActor =
    s.receipt != ReceiptRecorded or s.receiptActorAuthorized

  val receiptRequiresArtifactAndFenceMatch =
    s.receipt != ReceiptRecorded or (s.receiptArtifactMatches and s.receiptFenceMatches)

  val recordedReceiptHasTargetAndCommit =
    s.receipt != ReceiptRecorded or (s.receiptTarget != NoTarget and s.receiptCommit != NoCommit)

  val terminalQueueDoesNotCreateReceipt =
    not(s.queue == Superseded) or s.receipt == NoReceipt

  val acceptedAttemptRecordsReceipt =
    s.lastAttempt != Accepted or s.receipt == ReceiptRecorded

  val replaySameDoesNotChangeReceipt =
    s.lastAttempt != ReplayedSame
    or (s.receipt == ReceiptRecorded and sameReceipt(s))

  val divergentReceiptAttemptRejected =
    not(divergentReceipt(s)) or (s.lastAttempt == Rejected and s.divergentAttemptRejected)

  val rejectedAttemptDoesNotRecordNewReceipt =
    s.lastAttempt != Rejected or not(s.receiptCreatedByLastAttempt)
}

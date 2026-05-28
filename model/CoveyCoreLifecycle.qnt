module CoveyCoreLifecycle {
  type SubtaskState = Available | Claimed | InProgress | ArtifactPublished | ReviewPending | ChangesRequested | Approved | ReadyForApply | Applied | Abandoned
  type ClaimState = NoClaim | Held | Released | Expired
  type SessionState = ActiveSession | ExitedSession
  type ReviewState = NoReview | Requested | ReviewInProgress | Decided | Superseded
  type QueueState = NoQueue | Queued | QueueInFlight | QueueApplied | QueueSuperseded
  type Fence = F0 | F1 | F2

  type Model = {
    subtask: SubtaskState,
    claim: ClaimState,
    session: SessionState,
    review: ReviewState,
    queue: QueueState,
    fence: Fence,
    activeSubtask: bool,
    artifactPresent: bool,
    reviewApproved: bool,
    applyVerified: bool,
    terminal: bool,
  }

  var s: Model

  def terminalSubtask(state) = state.subtask.in(Set(Applied, Abandoned))
  def claimBound(state) = state.claim == Held iff state.activeSubtask
  def activeClaimNeedsLiveSubtask(state) =
    state.claim != Held
    or state.subtask.in(Set(Claimed, InProgress, ArtifactPublished, ReviewPending))
  def reviewOpen(state) = state.review.in(Set(Requested, ReviewInProgress))
  def queueOpen(state) = state.queue.in(Set(Queued, QueueInFlight))
  def reviewRequiresArtifact(state) = state.review == NoReview or state.artifactPresent
  def applyRequiresReadyAndVerification(state) = state.queue != QueueApplied or (state.subtask == Applied and state.applyVerified)
  def terminalHasNoClaim(state) = not(terminalSubtask(state)) or state.claim != Held
  def readyQueueRequiresApprovedReview(state) = state.queue == NoQueue or state.reviewApproved

  def nextFence(f) = if (f == F0) F1 else F2

  action init = {
    s' = {
      subtask: Available,
      claim: NoClaim,
      session: ActiveSession,
      review: NoReview,
      queue: NoQueue,
      fence: F0,
      activeSubtask: false,
      artifactPresent: false,
      reviewApproved: false,
      applyVerified: false,
      terminal: false,
    }
  }

  action claimAvailable = all {
    s.session == ActiveSession,
    s.subtask == Available,
    s.claim == NoClaim,
    not(s.activeSubtask),
    s' = s
      .with("subtask", Claimed)
      .with("claim", Held)
      .with("activeSubtask", true)
      .with("fence", nextFence(s.fence))
      .with("artifactPresent", false),
  }

  action startClaim = all {
    s.session == ActiveSession,
    s.claim == Held,
    s.subtask == Claimed,
    s' = s.with("subtask", InProgress),
  }

  action publishArtifact = all {
    s.session == ActiveSession,
    s.claim == Held,
    s.subtask.in(Set(InProgress, ReviewPending, ArtifactPublished)),
    s' = s.with("subtask", ArtifactPublished).with("artifactPresent", true),
  }

  action requestReview = all {
    s.session == ActiveSession,
    s.claim == Held,
    s.subtask == ArtifactPublished,
    s.artifactPresent,
    not(reviewOpen(s)),
    s' = s.with("subtask", ReviewPending).with("review", Requested),
  }

  action startReview = all {
    s.review == Requested,
    s.subtask == ReviewPending,
    s' = s.with("review", ReviewInProgress),
  }

  action requestChanges = all {
    s.review == ReviewInProgress,
    s.subtask == ReviewPending,
    s' = s
      .with("review", Decided)
      .with("subtask", ChangesRequested)
      .with("claim", Released)
      .with("activeSubtask", false),
  }

  action approveReview = all {
    s.review == ReviewInProgress,
    s.subtask == ReviewPending,
    s' = s
      .with("review", Decided)
      .with("subtask", Approved)
      .with("claim", Released)
      .with("activeSubtask", false)
      .with("reviewApproved", true),
  }

  action enqueueReady = all {
    s.subtask == Approved,
    s.reviewApproved,
    s.queue == NoQueue,
    s.claim != Held,
    s' = s.with("subtask", ReadyForApply).with("queue", Queued),
  }

  action claimQueue = all {
    s.queue == Queued,
    s.subtask == ReadyForApply,
    s' = s.with("queue", QueueInFlight),
  }

  action verifyApply = all {
    s.queue == QueueInFlight,
    s.subtask == ReadyForApply,
    s' = s.with("applyVerified", true),
  }

  action markApplied = all {
    s.queue == QueueInFlight,
    s.subtask == ReadyForApply,
    s.applyVerified,
    s' = s.with("queue", QueueApplied).with("subtask", Applied).with("terminal", true),
  }

  action supersedeQueue = all {
    s.queue.in(Set(Queued, QueueInFlight)),
    s' = s.with("queue", QueueSuperseded),
  }

  def releasedSubtaskState(state) =
    if (state.subtask.in(Set(Claimed, InProgress))) Available else state.subtask

  action releaseClaim = all {
    s.claim == Held,
    not(terminalSubtask(s)),
    s' = s
      .with("claim", Released)
      .with("subtask", releasedSubtaskState(s))
      .with("activeSubtask", false),
  }

  action expireClaim = all {
    s.claim == Held,
    s.subtask.in(Set(Claimed, InProgress)),
    s' = s.with("claim", Expired).with("subtask", Available).with("activeSubtask", false),
  }

  action abandon = all {
    s.claim == Held,
    not(terminalSubtask(s)),
    s' = s.with("claim", Released).with("subtask", Abandoned).with("activeSubtask", false).with("terminal", true),
  }

  action exitSession = all {
    s.session == ActiveSession,
    s.claim != Held,
    not(s.activeSubtask),
    s' = s.with("session", ExitedSession),
  }

  action step = any {
    claimAvailable,
    startClaim,
    publishArtifact,
    requestReview,
    startReview,
    requestChanges,
    approveReview,
    enqueueReady,
    claimQueue,
    verifyApply,
    markApplied,
    supersedeQueue,
    releaseClaim,
    expireClaim,
    abandon,
    exitSession,
  }

  val typeOk = true
  val heldClaimBindsActiveSessionAndSubtask = s.claim != Held or (s.session == ActiveSession and s.activeSubtask and activeClaimNeedsLiveSubtask(s))
  val activeSubtaskRequiresHeldClaim = not(s.activeSubtask) or s.claim == Held
  val terminalSubtaskHasNoHeldClaim = terminalHasNoClaim(s)
  val reviewStateRequiresArtifact = reviewRequiresArtifact(s)
  val decidedReviewIsOnlyTerminalReview = s.review != Decided or s.subtask.in(Set(ChangesRequested, Approved, ReadyForApply, Applied))
  val readyQueueRequiresApprovedArtifactReview = readyQueueRequiresApprovedReview(s)
  val queuedApplyRequiresReadyForApply = not(queueOpen(s)) or s.subtask == ReadyForApply
  val appliedQueueRequiresApplyVerification = applyRequiresReadyAndVerification(s)
  val appliedSubtaskRequiresAppliedQueue = s.subtask != Applied or s.queue == QueueApplied
}

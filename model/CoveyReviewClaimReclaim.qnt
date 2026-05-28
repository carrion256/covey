module CoveyReviewClaimReclaim {
  type ReviewState = NoReview | Requested | InProgress | Decided | Superseded
  type ReviewSubtaskState = NoReviewSubtask | Available | Claimed | SubtaskInProgress | SubtaskDecided
  type ClaimState = NoClaim | Held | Expired | Released
  type Reviewer = NoReviewer | ReviewerA | ReviewerB
  type Verdict = NoVerdict | Approve | ChangesRequested | Blocked

  type Model = {
    review: ReviewState,
    reviewSubtask: ReviewSubtaskState,
    claim: ClaimState,
    owner: Reviewer,
    currentFence: int,
    expiredFence: int,
    verdict: Verdict,
    artifactCurrent: bool,
    followupAvailable: bool,
    followupCount: int,
    followupReviewBound: bool,
    followupSourceSubtaskBound: bool,
    followupSourceArtifactBound: bool,
    followupFindingsBound: bool,
    followupCreatedByReviewer: bool,
    followupWorkAvailable: bool,
    executorClaimedFollowup: bool,
    staleDecisionRejected: bool,
    duplicateDecisionRejected: bool,
  }

  var s: Model

  def openReview(state) = state.review == Requested or state.review == InProgress
  def terminalReview(state) = state.review == Decided or state.review == Superseded
  def hasHeldClaim(state) = state.claim == Held
  def currentClaimCanDecide(state) =
    state.review == InProgress
    and state.reviewSubtask == SubtaskInProgress
    and state.claim == Held
    and state.owner != NoReviewer
    and state.artifactCurrent

  action init = {
    s' = {
      review: NoReview,
      reviewSubtask: NoReviewSubtask,
      claim: NoClaim,
      owner: NoReviewer,
      currentFence: 0,
      expiredFence: 0,
      verdict: NoVerdict,
      artifactCurrent: true,
      followupAvailable: false,
      followupCount: 0,
      followupReviewBound: false,
      followupSourceSubtaskBound: false,
      followupSourceArtifactBound: false,
      followupFindingsBound: false,
      followupCreatedByReviewer: false,
      followupWorkAvailable: false,
      executorClaimedFollowup: false,
      staleDecisionRejected: false,
      duplicateDecisionRejected: false,
    }
  }

  action requestReview = all {
    s.review == NoReview,
    s.reviewSubtask == NoReviewSubtask,
    s.artifactCurrent,
    s' = s.with("review", Requested).with("reviewSubtask", Available),
  }

  action claimReviewBy(reviewer) = all {
    s.review == Requested,
    s.reviewSubtask == Available,
    s.claim != Held,
    s.currentFence < 3,
    s' = s
      .with("reviewSubtask", Claimed)
      .with("claim", Held)
      .with("owner", reviewer)
      .with("currentFence", s.currentFence + 1)
      .with("staleDecisionRejected", false)
      .with("duplicateDecisionRejected", false),
  }

  action startReview = all {
    s.review == Requested,
    s.reviewSubtask == Claimed,
    s.claim == Held,
    s.owner != NoReviewer,
    s' = s.with("review", InProgress).with("reviewSubtask", SubtaskInProgress),
  }

  action expireInProgressReviewClaim = all {
    s.review == InProgress,
    s.reviewSubtask == SubtaskInProgress,
    s.claim == Held,
    s.owner != NoReviewer,
    s' = s
      .with("review", Requested)
      .with("reviewSubtask", Available)
      .with("claim", Expired)
      .with("owner", NoReviewer)
      .with("expiredFence", s.currentFence),
  }

  action rejectStaleDecisionAttempt = all {
    s.claim == Expired,
    s.expiredFence > 0,
    s.review == Requested,
    s.reviewSubtask == Available,
    s' = s.with("staleDecisionRejected", true),
  }

  action decideApprove = all {
    currentClaimCanDecide(s),
    s' = s
      .with("review", Decided)
      .with("reviewSubtask", SubtaskDecided)
      .with("claim", Released)
      .with("owner", NoReviewer)
      .with("verdict", Approve),
  }

  action decideChangesRequested = all {
    currentClaimCanDecide(s),
    s' = s
      .with("review", Decided)
      .with("reviewSubtask", SubtaskDecided)
      .with("claim", Released)
      .with("owner", NoReviewer)
      .with("verdict", ChangesRequested)
      .with("followupAvailable", true)
      .with("followupCount", 1)
      .with("followupReviewBound", true)
      .with("followupSourceSubtaskBound", true)
      .with("followupSourceArtifactBound", true)
      .with("followupFindingsBound", true)
      .with("followupCreatedByReviewer", true)
      .with("followupWorkAvailable", true)
      .with("executorClaimedFollowup", false),
  }

  action decideBlocked = all {
    currentClaimCanDecide(s),
    s' = s
      .with("review", Decided)
      .with("reviewSubtask", SubtaskDecided)
      .with("claim", Released)
      .with("owner", NoReviewer)
      .with("verdict", Blocked)
      .with("followupAvailable", true)
      .with("followupCount", 1)
      .with("followupReviewBound", true)
      .with("followupSourceSubtaskBound", true)
      .with("followupSourceArtifactBound", true)
      .with("followupFindingsBound", true)
      .with("followupCreatedByReviewer", true)
      .with("followupWorkAvailable", true)
      .with("executorClaimedFollowup", false),
  }

  action rejectDuplicateDecisionAttempt = all {
    s.review == Decided,
    s.verdict != NoVerdict,
    not(s.duplicateDecisionRejected),
    s' = s.with("duplicateDecisionRejected", true),
  }

  action claimExecutorFollowup = all {
    s.review == Decided,
    s.followupAvailable,
    s.followupWorkAvailable,
    s.followupCount == 1,
    not(s.executorClaimedFollowup),
    s' = s
      .with("followupWorkAvailable", false)
      .with("executorClaimedFollowup", true),
  }

  action artifactSuperseded = all {
    openReview(s),
    s' = s
      .with("review", Superseded)
      .with("claim", if (s.claim == Held) Released else s.claim)
      .with("owner", NoReviewer)
      .with("artifactCurrent", false)
      .with("staleDecisionRejected", false),
  }

  action step = any {
    requestReview,
    claimReviewBy(ReviewerA),
    claimReviewBy(ReviewerB),
    startReview,
    expireInProgressReviewClaim,
    rejectStaleDecisionAttempt,
    decideApprove,
    decideChangesRequested,
    decideBlocked,
    rejectDuplicateDecisionAttempt,
    claimExecutorFollowup,
    artifactSuperseded,
  }

  val typeOk =
    s.currentFence >= 0
    and s.currentFence <= 3
    and s.expiredFence >= 0
    and s.expiredFence <= s.currentFence
    and s.followupCount >= 0
    and s.followupCount <= 1

  val heldClaimRequiresReviewerOwner =
    s.claim != Held or s.owner != NoReviewer

  val nonHeldClaimHasNoOwner =
    s.claim == Held or s.owner == NoReviewer

  val inProgressReviewRequiresHeldStartedSubtask =
    s.review != InProgress or (s.reviewSubtask == SubtaskInProgress and s.claim == Held)

  val requestedReviewIsClaimableAfterExpiry =
    not(s.claim == Expired and s.review == Requested) or s.reviewSubtask == Available

  val decidedReviewRequiresReleasedClaim =
    s.review != Decided or (s.reviewSubtask == SubtaskDecided and s.claim == Released and s.owner == NoReviewer)

  val staleExpiredClaimCannotDecide =
    not(s.claim == Expired) or s.review != Decided

  val staleDecisionAttemptDoesNotMutateReview =
    not(s.staleDecisionRejected) or (s.review == Requested and s.reviewSubtask == Available)

  val nonApprovalDecisionCreatesFollowup =
    not(s.verdict.in(Set(ChangesRequested, Blocked)))
    or (s.followupAvailable and s.followupCount == 1 and (s.followupWorkAvailable or s.executorClaimedFollowup))

  val approveDoesNotCreateFollowup =
    s.verdict != Approve or (not(s.followupAvailable) and s.followupCount == 0)

  val noFollowupWithoutFailedReviewDecision =
    s.followupCount == 0 or s.verdict.in(Set(ChangesRequested, Blocked))

  val followupRecordBindsReviewSourceArtifactFindingsAndReviewer =
    s.followupCount == 0
    or (
      s.followupReviewBound
      and s.followupSourceSubtaskBound
      and s.followupSourceArtifactBound
      and s.followupFindingsBound
      and s.followupCreatedByReviewer
    )

  val changesRequestedFollowupIsExecutorClaimable =
    s.verdict != ChangesRequested or s.followupWorkAvailable or s.executorClaimedFollowup

  val blockedFollowupIsExecutorClaimable =
    s.verdict != Blocked or s.followupWorkAvailable or s.executorClaimedFollowup

  val executorClaimedFollowupCameFromFailedReview =
    not(s.executorClaimedFollowup) or (s.followupCount == 1 and s.verdict.in(Set(ChangesRequested, Blocked)))

  val executorClaimConsumesAvailability =
    not(s.executorClaimedFollowup) or not(s.followupWorkAvailable)

  val duplicateDecisionDoesNotDuplicateFollowup =
    not(s.duplicateDecisionRejected) or s.followupCount <= 1

  val decidedReviewHasVerdict =
    s.review != Decided or s.verdict != NoVerdict

  val supersededNeverDecidesOrCreatesFollowup =
    s.review != Superseded or (s.verdict == NoVerdict and not(s.followupAvailable) and s.followupCount == 0)

  val artifactDriftCannotBeDecided =
    s.artifactCurrent or s.review != Decided

  val staleDecisionAttemptDoesNotCreateFollowup =
    not(s.staleDecisionRejected) or s.followupCount == 0

  val fenceMonotonicAfterReclaim =
    s.claim != Held or s.currentFence > s.expiredFence
}

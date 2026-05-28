module CoveyViewAttachmentShape {
  type Case =
    ValidSessionNoActiveDetached
    | ValidSessionActiveClaimArtifact
    | SessionMissingActiveView
    | SessionMismatchedActiveView
    | SessionUnexpectedActiveView
    | SubtaskMissingClaim
    | SubtaskMismatchedClaim
    | SubtaskForeignClaim
    | SubtaskUnexpectedClaimCase
    | SubtaskMissingArtifact
    | SubtaskMismatchedArtifact
    | SubtaskForeignArtifact
    | SubtaskUnexpectedArtifactCase
    | SubtaskForeignReview
    | SubtaskForeignReadyQueue

  type Outcome = NotEvaluated | Accepted | Rejected

  type RejectReason =
    NoReject
    | SessionRequiresActiveSubtask
    | SessionActiveSubtaskMismatch
    | SessionUnexpectedActiveSubtask
    | SubtaskRequiresActiveClaim
    | SubtaskClaimMismatch
    | SubtaskClaimForeign
    | SubtaskUnexpectedClaimReject
    | SubtaskRequiresArtifact
    | SubtaskArtifactMismatch
    | SubtaskArtifactForeign
    | SubtaskUnexpectedArtifactReject
    | SubtaskReviewForeign
    | SubtaskReadyQueueForeign

  type Model = {
    caseIndex: int,
    case: Case,
    sessionHasActiveSubtaskId: bool,
    activeSubtaskViewPresent: bool,
    activeSubtaskMatchesSession: bool,
    subtaskHasActiveClaimId: bool,
    claimRowPresent: bool,
    claimIdMatchesActive: bool,
    claimBelongsToSubtask: bool,
    subtaskHasArtifactDigest: bool,
    artifactRowPresent: bool,
    artifactDigestMatches: bool,
    artifactBelongsToSubtask: bool,
    reviewsBelongToSubtask: bool,
    readyQueueBelongsToSubtask: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    sessionHasActiveSubtaskId:
      case != SessionUnexpectedActiveView and case != ValidSessionNoActiveDetached,
    activeSubtaskViewPresent:
      case != SessionMissingActiveView and case != ValidSessionNoActiveDetached,
    activeSubtaskMatchesSession:
      case != SessionMismatchedActiveView,
    subtaskHasActiveClaimId:
      case.in(Set(ValidSessionActiveClaimArtifact, SubtaskMissingClaim, SubtaskMismatchedClaim, SubtaskForeignClaim)),
    claimRowPresent:
      case.in(Set(ValidSessionActiveClaimArtifact, SubtaskMismatchedClaim, SubtaskForeignClaim, SubtaskUnexpectedClaimCase)),
    claimIdMatchesActive:
      case != SubtaskMismatchedClaim,
    claimBelongsToSubtask:
      case != SubtaskForeignClaim,
    subtaskHasArtifactDigest:
      case.in(Set(ValidSessionActiveClaimArtifact, SubtaskMissingArtifact, SubtaskMismatchedArtifact, SubtaskForeignArtifact)),
    artifactRowPresent:
      case.in(Set(ValidSessionActiveClaimArtifact, SubtaskMismatchedArtifact, SubtaskForeignArtifact, SubtaskUnexpectedArtifactCase)),
    artifactDigestMatches:
      case != SubtaskMismatchedArtifact,
    artifactBelongsToSubtask:
      case != SubtaskForeignArtifact,
    reviewsBelongToSubtask:
      case != SubtaskForeignReview,
    readyQueueBelongsToSubtask:
      case != SubtaskForeignReadyQueue,
  }

  def rejectFor(facts) =
    if (facts.sessionHasActiveSubtaskId and not(facts.activeSubtaskViewPresent)) SessionRequiresActiveSubtask
    else if (facts.sessionHasActiveSubtaskId and facts.activeSubtaskViewPresent and not(facts.activeSubtaskMatchesSession)) SessionActiveSubtaskMismatch
    else if (not(facts.sessionHasActiveSubtaskId) and facts.activeSubtaskViewPresent) SessionUnexpectedActiveSubtask
    else if (facts.subtaskHasActiveClaimId and not(facts.claimRowPresent)) SubtaskRequiresActiveClaim
    else if (facts.subtaskHasActiveClaimId and facts.claimRowPresent and not(facts.claimIdMatchesActive)) SubtaskClaimMismatch
    else if (facts.subtaskHasActiveClaimId and facts.claimRowPresent and facts.claimIdMatchesActive and not(facts.claimBelongsToSubtask)) SubtaskClaimForeign
    else if (not(facts.subtaskHasActiveClaimId) and facts.claimRowPresent) SubtaskUnexpectedClaimReject
    else if (facts.subtaskHasArtifactDigest and not(facts.artifactRowPresent)) SubtaskRequiresArtifact
    else if (facts.subtaskHasArtifactDigest and facts.artifactRowPresent and not(facts.artifactDigestMatches)) SubtaskArtifactMismatch
    else if (facts.subtaskHasArtifactDigest and facts.artifactRowPresent and facts.artifactDigestMatches and not(facts.artifactBelongsToSubtask)) SubtaskArtifactForeign
    else if (not(facts.subtaskHasArtifactDigest) and facts.artifactRowPresent) SubtaskUnexpectedArtifactReject
    else if (not(facts.reviewsBelongToSubtask)) SubtaskReviewForeign
    else if (not(facts.readyQueueBelongsToSubtask)) SubtaskReadyQueueForeign
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidSessionNoActiveDetached,
      sessionHasActiveSubtaskId: false,
      activeSubtaskViewPresent: false,
      activeSubtaskMatchesSession: false,
      subtaskHasActiveClaimId: false,
      claimRowPresent: false,
      claimIdMatchesActive: false,
      claimBelongsToSubtask: false,
      subtaskHasArtifactDigest: false,
      artifactRowPresent: false,
      artifactDigestMatches: false,
      artifactBelongsToSubtask: false,
      reviewsBelongToSubtask: false,
      readyQueueBelongsToSubtask: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val facts = factsFor(case)
    val rejectReason = rejectFor(facts)
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      sessionHasActiveSubtaskId: facts.sessionHasActiveSubtaskId,
      activeSubtaskViewPresent: facts.activeSubtaskViewPresent,
      activeSubtaskMatchesSession: facts.activeSubtaskMatchesSession,
      subtaskHasActiveClaimId: facts.subtaskHasActiveClaimId,
      claimRowPresent: facts.claimRowPresent,
      claimIdMatchesActive: facts.claimIdMatchesActive,
      claimBelongsToSubtask: facts.claimBelongsToSubtask,
      subtaskHasArtifactDigest: facts.subtaskHasArtifactDigest,
      artifactRowPresent: facts.artifactRowPresent,
      artifactDigestMatches: facts.artifactDigestMatches,
      artifactBelongsToSubtask: facts.artifactBelongsToSubtask,
      reviewsBelongToSubtask: facts.reviewsBelongToSubtask,
      readyQueueBelongsToSubtask: facts.readyQueueBelongsToSubtask,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidSessionNoActiveDetached) },
    all { s.caseIndex == 1, evaluate(ValidSessionActiveClaimArtifact) },
    all { s.caseIndex == 2, evaluate(SessionMissingActiveView) },
    all { s.caseIndex == 3, evaluate(SessionMismatchedActiveView) },
    all { s.caseIndex == 4, evaluate(SessionUnexpectedActiveView) },
    all { s.caseIndex == 5, evaluate(SubtaskMissingClaim) },
    all { s.caseIndex == 6, evaluate(SubtaskMismatchedClaim) },
    all { s.caseIndex == 7, evaluate(SubtaskForeignClaim) },
    all { s.caseIndex == 8, evaluate(SubtaskUnexpectedClaimCase) },
    all { s.caseIndex == 9, evaluate(SubtaskMissingArtifact) },
    all { s.caseIndex == 10, evaluate(SubtaskMismatchedArtifact) },
    all { s.caseIndex == 11, evaluate(SubtaskForeignArtifact) },
    all { s.caseIndex == 12, evaluate(SubtaskUnexpectedArtifactCase) },
    all { s.caseIndex == 13, evaluate(SubtaskForeignReview) },
    all { s.caseIndex == 14, evaluate(SubtaskForeignReadyQueue) },
    all { s.caseIndex >= 15, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 15
    and s.case.in(Set(ValidSessionNoActiveDetached, ValidSessionActiveClaimArtifact, SessionMissingActiveView, SessionMismatchedActiveView, SessionUnexpectedActiveView, SubtaskMissingClaim, SubtaskMismatchedClaim, SubtaskForeignClaim, SubtaskUnexpectedClaimCase, SubtaskMissingArtifact, SubtaskMismatchedArtifact, SubtaskForeignArtifact, SubtaskUnexpectedArtifactCase, SubtaskForeignReview, SubtaskForeignReadyQueue))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionRequiresActiveSubtask, SessionActiveSubtaskMismatch, SessionUnexpectedActiveSubtask, SubtaskRequiresActiveClaim, SubtaskClaimMismatch, SubtaskClaimForeign, SubtaskUnexpectedClaimReject, SubtaskRequiresArtifact, SubtaskArtifactMismatch, SubtaskArtifactForeign, SubtaskUnexpectedArtifactReject, SubtaskReviewForeign, SubtaskReadyQueueForeign))

  val sessionActiveBindingOk =
    s.outcome != Accepted
    or (
      (s.sessionHasActiveSubtaskId and s.activeSubtaskViewPresent and s.activeSubtaskMatchesSession)
      or (not(s.sessionHasActiveSubtaskId) and not(s.activeSubtaskViewPresent))
    )

  val activeClaimBindingOk =
    s.outcome != Accepted
    or (
      (s.subtaskHasActiveClaimId and s.claimRowPresent and s.claimIdMatchesActive and s.claimBelongsToSubtask)
      or (not(s.subtaskHasActiveClaimId) and not(s.claimRowPresent))
    )

  val artifactBindingOk =
    s.outcome != Accepted
    or (
      (s.subtaskHasArtifactDigest and s.artifactRowPresent and s.artifactDigestMatches and s.artifactBelongsToSubtask)
      or (not(s.subtaskHasArtifactDigest) and not(s.artifactRowPresent))
    )

  val collectionAttachmentsBelong =
    s.outcome != Accepted
    or (s.reviewsBelongToSubtask and s.readyQueueBelongsToSubtask)

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject)
}

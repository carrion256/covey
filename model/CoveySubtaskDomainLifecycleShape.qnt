module CoveySubtaskDomainLifecycleShape {
  type Case =
    WorkAvailable
    | WorkBlocked
    | WorkArtifactPublished
    | WorkReviewPending
    | WorkChangesRequested
    | WorkApproved
    | WorkReadyForApply
    | WorkApplied
    | WorkAbandoned
    | WorkDecided
    | ReviewAvailable
    | ReviewClaimed
    | ReviewInProgress
    | ReviewDecided
    | ReviewAbandoned
    | ReviewBlocked
    | ReviewArtifactPublished
    | ReviewReviewPending
    | ReviewChangesRequested
    | ReviewApproved
    | ReviewReadyForApply
    | ReviewApplied

  type SubtaskKind = Work | Review
  type Lifecycle =
    Available
    | Blocked
    | Claimed
    | InProgress
    | ArtifactPublished
    | ReviewPending
    | ChangesRequested
    | Approved
    | Decided
    | ReadyForApply
    | Applied
    | Abandoned

  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | WorkCannotUseDecided
    | ReviewCannotUseBlocked
    | ReviewCannotUseWorkArtifactState

  type Model = {
    caseIndex: int,
    case: Case,
    kind: SubtaskKind,
    lifecycle: Lifecycle,
    carriesArtifact: bool,
    carriesActiveClaim: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def kindFor(case) =
    if (case.in(Set(
      ReviewAvailable,
      ReviewClaimed,
      ReviewInProgress,
      ReviewDecided,
      ReviewAbandoned,
      ReviewBlocked,
      ReviewArtifactPublished,
      ReviewReviewPending,
      ReviewChangesRequested,
      ReviewApproved,
      ReviewReadyForApply,
      ReviewApplied,
    ))) Review
    else Work

  def lifecycleFor(case) =
    if (case.in(Set(WorkBlocked, ReviewBlocked))) Blocked
    else if (case == ReviewClaimed) Claimed
    else if (case == ReviewInProgress) InProgress
    else if (case.in(Set(WorkArtifactPublished, ReviewArtifactPublished))) ArtifactPublished
    else if (case.in(Set(WorkReviewPending, ReviewReviewPending))) ReviewPending
    else if (case.in(Set(WorkChangesRequested, ReviewChangesRequested))) ChangesRequested
    else if (case.in(Set(WorkApproved, ReviewApproved))) Approved
    else if (case.in(Set(WorkDecided, ReviewDecided))) Decided
    else if (case.in(Set(WorkReadyForApply, ReviewReadyForApply))) ReadyForApply
    else if (case.in(Set(WorkApplied, ReviewApplied))) Applied
    else if (case.in(Set(WorkAbandoned, ReviewAbandoned))) Abandoned
    else Available

  def carriesArtifact(lifecycle) =
    lifecycle.in(Set(
      Blocked,
      ArtifactPublished,
      ReviewPending,
      ChangesRequested,
      Approved,
      ReadyForApply,
      Applied,
    ))

  def carriesActiveClaim(lifecycle) =
    lifecycle.in(Set(Claimed, InProgress, ArtifactPublished, ReviewPending))

  // Mirrors SubtaskLifecycle::ensure_allowed_for_kind.
  def rejectFor(kind, lifecycle) =
    if (kind == Work and lifecycle == Decided) WorkCannotUseDecided
    else if (kind == Review and lifecycle == Blocked) ReviewCannotUseBlocked
    else if (kind == Review and lifecycle.in(Set(
      ArtifactPublished,
      ReviewPending,
      ChangesRequested,
      Approved,
      ReadyForApply,
      Applied,
    ))) ReviewCannotUseWorkArtifactState
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: WorkAvailable,
      kind: Work,
      lifecycle: Available,
      carriesArtifact: false,
      carriesActiveClaim: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val kind = kindFor(case)
    val lifecycle = lifecycleFor(case)
    val rejectReason = rejectFor(kind, lifecycle)
    val accepted = rejectReason == NoReject
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      kind: kind,
      lifecycle: lifecycle,
      carriesArtifact: carriesArtifact(lifecycle),
      carriesActiveClaim: carriesActiveClaim(lifecycle),
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(WorkAvailable) },
    all { s.caseIndex == 1, evaluate(WorkBlocked) },
    all { s.caseIndex == 2, evaluate(WorkArtifactPublished) },
    all { s.caseIndex == 3, evaluate(WorkReviewPending) },
    all { s.caseIndex == 4, evaluate(WorkChangesRequested) },
    all { s.caseIndex == 5, evaluate(WorkApproved) },
    all { s.caseIndex == 6, evaluate(WorkReadyForApply) },
    all { s.caseIndex == 7, evaluate(WorkApplied) },
    all { s.caseIndex == 8, evaluate(WorkAbandoned) },
    all { s.caseIndex == 9, evaluate(WorkDecided) },
    all { s.caseIndex == 10, evaluate(ReviewAvailable) },
    all { s.caseIndex == 11, evaluate(ReviewClaimed) },
    all { s.caseIndex == 12, evaluate(ReviewInProgress) },
    all { s.caseIndex == 13, evaluate(ReviewDecided) },
    all { s.caseIndex == 14, evaluate(ReviewAbandoned) },
    all { s.caseIndex == 15, evaluate(ReviewBlocked) },
    all { s.caseIndex == 16, evaluate(ReviewArtifactPublished) },
    all { s.caseIndex == 17, evaluate(ReviewReviewPending) },
    all { s.caseIndex == 18, evaluate(ReviewChangesRequested) },
    all { s.caseIndex == 19, evaluate(ReviewApproved) },
    all { s.caseIndex == 20, evaluate(ReviewReadyForApply) },
    all { s.caseIndex == 21, evaluate(ReviewApplied) },
    all { s.caseIndex >= 22, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 22
    and s.case.in(Set(
      WorkAvailable,
      WorkBlocked,
      WorkArtifactPublished,
      WorkReviewPending,
      WorkChangesRequested,
      WorkApproved,
      WorkReadyForApply,
      WorkApplied,
      WorkAbandoned,
      WorkDecided,
      ReviewAvailable,
      ReviewClaimed,
      ReviewInProgress,
      ReviewDecided,
      ReviewAbandoned,
      ReviewBlocked,
      ReviewArtifactPublished,
      ReviewReviewPending,
      ReviewChangesRequested,
      ReviewApproved,
      ReviewReadyForApply,
      ReviewApplied,
    ))
    and s.kind.in(Set(Work, Review))
    and s.lifecycle.in(Set(
      Available,
      Blocked,
      Claimed,
      InProgress,
      ArtifactPublished,
      ReviewPending,
      ChangesRequested,
      Approved,
      Decided,
      ReadyForApply,
      Applied,
      Abandoned,
    ))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(
      NoReject,
      WorkCannotUseDecided,
      ReviewCannotUseBlocked,
      ReviewCannotUseWorkArtifactState,
    ))

  val acceptedWorkNeverUsesReviewOnlyLifecycle =
    not(s.accepted) or s.kind != Work or s.lifecycle != Decided

  val acceptedReviewNeverUsesWorkOnlyLifecycle =
    not(s.accepted)
    or s.kind != Review
    or not(s.lifecycle.in(Set(
      Blocked,
      ArtifactPublished,
      ReviewPending,
      ChangesRequested,
      Approved,
      ReadyForApply,
      Applied,
    )))

  val acceptedArtifactCarrierMatchesLifecycle =
    not(s.accepted) or s.carriesArtifact == carriesArtifact(s.lifecycle)

  val acceptedActiveClaimCarrierMatchesLifecycle =
    not(s.accepted) or s.carriesActiveClaim == carriesActiveClaim(s.lifecycle)

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor(s.kind, s.lifecycle)
}

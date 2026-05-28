module CoveyClaimDependencyGate {
  type ClaimPath = ClaimNext | TargetedClaim
  type Role = Executor | Reviewer | Orchestrator
  type Session = SessionFree | SessionOccupied | SessionInactive
  type Meta = MetaActive | MetaPlanning | MetaCompleted | MetaCancelled
  type Kind = Work | Review
  type Lineage = PlainCandidate | ChangesRequestedSourceCandidate | ReviewFollowupCandidate
  type Candidate = CandidateAvailable | CandidateClaimed | CandidateInProgress | NoCandidate
  type Dependency =
    NoDependency
    | DepOpen
    | DepApproved
    | DepReadyForApply
    | DepApplied
    | DepDecided
    | DepChangesRequestedNoFollowup
    | DepChangesRequestedFollowupAvailable
    | DepChangesRequestedFollowupApproved
    | DepChangesRequestedFollowupReadyForApply
    | DepChangesRequestedFollowupApplied
    | DepChangesRequestedFollowupDecided

  type Decision = NotEvaluated | ClaimCreated | NoClaimableCandidate | Rejected
  type RejectReason = NoReject | WrongRole | SessionUnavailable | SessionAlreadyOccupied | MetaUnavailable | IllegalTransition | DependencyUnsatisfied

  type Model = {
    caseIndex: int,
    path: ClaimPath,
    role: Role,
    session: Session,
    meta: Meta,
    kind: Kind,
    lineage: Lineage,
    candidate: Candidate,
    dependency: Dependency,
    decision: Decision,
    rejectReason: RejectReason,
    dependencySatisfied: bool,
    candidateSelected: bool,
    claimCreated: bool,
    subtaskClaimed: bool,
    sessionActiveSubtaskSet: bool,
    fenceIssued: bool,
    evaluated: bool,
  }

  var s: Model

  def roleCanClaim(role, kind) =
    (role == Executor and kind == Work) or (role == Reviewer and kind == Review)

  def metaClaimable(meta) =
    meta.in(Set(MetaActive, MetaPlanning))

  def dependencySatisfiedForWork(dependency) =
    dependency.in(Set(
      NoDependency,
      DepApproved,
      DepReadyForApply,
      DepApplied,
      DepDecided,
      DepChangesRequestedFollowupApproved,
      DepChangesRequestedFollowupReadyForApply,
      DepChangesRequestedFollowupApplied,
      DepChangesRequestedFollowupDecided
    ))

  def expectedDependencySatisfied(kind, dependency) =
    kind == Review or dependencySatisfiedForWork(dependency)

  def expectedDependencySatisfiedForCandidate(kind, lineage, dependency) =
    kind == Review or lineage == ReviewFollowupCandidate or dependencySatisfiedForWork(dependency)

  def legalCandidate(kind, lineage, candidate) =
    candidate == CandidateAvailable and (kind == Work or lineage == PlainCandidate)

  def expectedReject(role, session, meta, kind, lineage, candidate, dependency) =
    if (session == SessionInactive) SessionUnavailable
    else if (session == SessionOccupied) SessionAlreadyOccupied
    else if (not(roleCanClaim(role, kind))) WrongRole
    else if (not(metaClaimable(meta))) MetaUnavailable
    else if (not(legalCandidate(kind, lineage, candidate))) IllegalTransition
    else if (not(expectedDependencySatisfiedForCandidate(kind, lineage, dependency))) DependencyUnsatisfied
    else NoReject

  def expectedDecision(path, role, session, meta, kind, lineage, candidate, dependency) =
    val reject = expectedReject(role, session, meta, kind, lineage, candidate, dependency)
    if (reject == NoReject) ClaimCreated
    else if (path == ClaimNext and reject.in(Set(MetaUnavailable, IllegalTransition, DependencyUnsatisfied))) NoClaimableCandidate
    else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      path: ClaimNext,
      role: Executor,
      session: SessionFree,
      meta: MetaActive,
      kind: Work,
      lineage: PlainCandidate,
      candidate: NoCandidate,
      dependency: NoDependency,
      decision: NotEvaluated,
      rejectReason: NoReject,
      dependencySatisfied: false,
      candidateSelected: false,
      claimCreated: false,
      subtaskClaimed: false,
      sessionActiveSubtaskSet: false,
      fenceIssued: false,
      evaluated: false,
    }
  }

  action evaluate(path, role, session, meta, kind, lineage, candidate, dependency) = all {
    val dependencyOk = expectedDependencySatisfiedForCandidate(kind, lineage, dependency)
    val reject = expectedReject(role, session, meta, kind, lineage, candidate, dependency)
    val decision = expectedDecision(path, role, session, meta, kind, lineage, candidate, dependency)
    val claim = decision == ClaimCreated
    s' = {
      caseIndex: s.caseIndex + 1,
      path: path,
      role: role,
      session: session,
      meta: meta,
      kind: kind,
      lineage: lineage,
      candidate: candidate,
      dependency: dependency,
      decision: decision,
      rejectReason: if (decision == ClaimCreated or decision == NoClaimableCandidate) NoReject else reject,
      dependencySatisfied: dependencyOk,
      candidateSelected: claim or (path == TargetedClaim and candidate != NoCandidate),
      claimCreated: claim,
      subtaskClaimed: claim,
      sessionActiveSubtaskSet: claim,
      fenceIssued: claim,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ClaimNext, Executor, SessionFree, MetaActive, Work, PlainCandidate, CandidateAvailable, NoDependency) },
    all { s.caseIndex == 1, evaluate(ClaimNext, Executor, SessionFree, MetaActive, Work, PlainCandidate, CandidateAvailable, DepOpen) },
    all { s.caseIndex == 2, evaluate(ClaimNext, Executor, SessionFree, MetaActive, Work, PlainCandidate, CandidateAvailable, DepApplied) },
    all { s.caseIndex == 3, evaluate(ClaimNext, Executor, SessionFree, MetaActive, Work, ChangesRequestedSourceCandidate, CandidateAvailable, DepChangesRequestedNoFollowup) },
    all { s.caseIndex == 4, evaluate(ClaimNext, Executor, SessionFree, MetaActive, Work, ChangesRequestedSourceCandidate, CandidateAvailable, DepChangesRequestedFollowupAvailable) },
    all { s.caseIndex == 5, evaluate(ClaimNext, Executor, SessionFree, MetaActive, Work, ReviewFollowupCandidate, CandidateAvailable, DepChangesRequestedFollowupAvailable) },
    all { s.caseIndex == 6, evaluate(ClaimNext, Executor, SessionFree, MetaActive, Work, PlainCandidate, CandidateAvailable, DepChangesRequestedFollowupApplied) },
    all { s.caseIndex == 7, evaluate(TargetedClaim, Executor, SessionFree, MetaActive, Work, PlainCandidate, CandidateAvailable, DepOpen) },
    all { s.caseIndex == 8, evaluate(TargetedClaim, Executor, SessionFree, MetaActive, Work, PlainCandidate, CandidateAvailable, DepChangesRequestedFollowupDecided) },
    all { s.caseIndex == 9, evaluate(ClaimNext, Executor, SessionOccupied, MetaActive, Work, PlainCandidate, CandidateAvailable, NoDependency) },
    all { s.caseIndex == 10, evaluate(ClaimNext, Orchestrator, SessionFree, MetaActive, Work, PlainCandidate, CandidateAvailable, NoDependency) },
    all { s.caseIndex == 11, evaluate(ClaimNext, Executor, SessionFree, MetaCompleted, Work, PlainCandidate, CandidateAvailable, NoDependency) },
    all { s.caseIndex == 12, evaluate(ClaimNext, Executor, SessionFree, MetaActive, Work, PlainCandidate, CandidateClaimed, NoDependency) },
    all { s.caseIndex == 13, evaluate(ClaimNext, Reviewer, SessionFree, MetaActive, Review, PlainCandidate, CandidateAvailable, DepOpen) },
    all { s.caseIndex == 14, evaluate(TargetedClaim, Reviewer, SessionFree, MetaActive, Work, PlainCandidate, CandidateAvailable, NoDependency) },
    all { s.caseIndex >= 15, s' = s },
  }

  val typeOk = true

  val decisionMatchesInputs =
    not(s.evaluated)
    or s.decision == expectedDecision(s.path, s.role, s.session, s.meta, s.kind, s.lineage, s.candidate, s.dependency)

  val dependencyMarkerMatchesState =
    not(s.evaluated)
    or s.dependencySatisfied == expectedDependencySatisfiedForCandidate(s.kind, s.lineage, s.dependency)

  val workClaimsRequireSatisfiedDependencies =
    not(s.claimCreated and s.kind == Work)
    or s.dependencySatisfied

  val openDependencyBlocksWorkClaim =
    not(s.evaluated and s.kind == Work and s.dependency == DepOpen)
    or not(s.claimCreated)

  val changesRequestedSourceWithoutTerminalFollowupBlocks =
    not(s.evaluated and s.kind == Work and s.lineage == ChangesRequestedSourceCandidate and s.dependency.in(Set(DepChangesRequestedNoFollowup, DepChangesRequestedFollowupAvailable)))
    or not(s.claimCreated)

  val reviewFollowupCandidateForChangesRequestedSourceClaims =
    not(s.evaluated and s.kind == Work and s.lineage == ReviewFollowupCandidate and s.dependency == DepChangesRequestedFollowupAvailable and s.path == ClaimNext)
    or s.claimCreated

  val terminalFollowupSatisfiesSourceDependency =
    not(s.evaluated and s.kind == Work and s.dependency.in(Set(DepChangesRequestedFollowupApproved, DepChangesRequestedFollowupReadyForApply, DepChangesRequestedFollowupApplied, DepChangesRequestedFollowupDecided)))
    or s.dependencySatisfied

  val terminalDependencyStatesSatisfyDependency =
    not(s.evaluated and s.kind == Work and s.dependency.in(Set(DepApproved, DepReadyForApply, DepApplied, DepDecided)))
    or s.dependencySatisfied

  val blockedClaimDoesNotMutateClaimState =
    s.claimCreated
    or (not(s.subtaskClaimed) and not(s.sessionActiveSubtaskSet) and not(s.fenceIssued))

  val createdClaimBindsSessionSubtaskAndFence =
    not(s.claimCreated)
    or (s.subtaskClaimed and s.sessionActiveSubtaskSet and s.fenceIssued)

  val claimNextBlockedCandidateReturnsNoCandidate =
    not(s.evaluated and s.path == ClaimNext and s.decision == NoClaimableCandidate)
    or not(s.claimCreated)

  val targetedBlockedCandidateRejects =
    not(s.evaluated and s.path == TargetedClaim and expectedReject(s.role, s.session, s.meta, s.kind, s.lineage, s.candidate, s.dependency) != NoReject)
    or s.decision == Rejected

  val wrongRoleNeverClaims =
    not(s.evaluated and not(roleCanClaim(s.role, s.kind)))
    or not(s.claimCreated)

  val occupiedSessionNeverClaims =
    not(s.evaluated and s.session == SessionOccupied)
    or not(s.claimCreated)

  val terminalMetaNeverClaims =
    not(s.evaluated and not(metaClaimable(s.meta)))
    or not(s.claimCreated)
}

module CoveyReservationOverlap {
  type Case =
    RepoCandidateOverlapsExact
    | ExactCandidateOverlapsRepo
    | ExactSamePath
    | ExactUnderExistingSubtree
    | ExactMatchesGeneratedMember
    | ExactDifferentPath
    | ExactVsDescendantSubtree
    | SubtreeCandidateOverlapsRepo
    | SubtreeContainsExact
    | SubtreeContainsSubtree
    | SubtreeInsideExistingSubtree
    | SubtreeContainsGeneratedMember
    | SubtreeDifferentPath
    | GeneratedCandidateOverlapsRepo
    | GeneratedMatchesExact
    | GeneratedUnderExistingSubtree
    | GeneratedMatchesGeneratedMember
    | GeneratedDifferentMember
    | ExistingReservationExpired
    | GeneratedCandidateHasNoMembers
    | CandidatePathInvalid

  type Scope = RepoGlobal | ExactPath | Subtree | GeneratedSet
  type Relation =
    SamePath
    | CandidateUnderExisting
    | ExistingUnderCandidate
    | SharedGeneratedMember
    | DifferentPath
    | NoRelation

  type RejectReason = NoReject | EmptyGeneratedMembers | InvalidPath

  type Model = {
    caseIndex: int,
    case: Case,
    candidateScope: Scope,
    existingScope: Scope,
    relation: Relation,
    existingActive: bool,
    candidatePathValid: bool,
    candidateMembersPresent: bool,
    overlapReturned: bool,
    conflictRecorded: bool,
    rejectReason: RejectReason,
    evaluated: bool,
  }

  var s: Model

  def factsForCase(case) =
    if (case == RepoCandidateOverlapsExact) { candidateScope: RepoGlobal, existingScope: ExactPath, relation: NoRelation, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == ExactCandidateOverlapsRepo) { candidateScope: ExactPath, existingScope: RepoGlobal, relation: NoRelation, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == ExactSamePath) { candidateScope: ExactPath, existingScope: ExactPath, relation: SamePath, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == ExactUnderExistingSubtree) { candidateScope: ExactPath, existingScope: Subtree, relation: CandidateUnderExisting, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == ExactMatchesGeneratedMember) { candidateScope: ExactPath, existingScope: GeneratedSet, relation: SharedGeneratedMember, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == ExactDifferentPath) { candidateScope: ExactPath, existingScope: ExactPath, relation: DifferentPath, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == ExactVsDescendantSubtree) { candidateScope: ExactPath, existingScope: Subtree, relation: ExistingUnderCandidate, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == SubtreeCandidateOverlapsRepo) { candidateScope: Subtree, existingScope: RepoGlobal, relation: NoRelation, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == SubtreeContainsExact) { candidateScope: Subtree, existingScope: ExactPath, relation: ExistingUnderCandidate, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == SubtreeContainsSubtree) { candidateScope: Subtree, existingScope: Subtree, relation: ExistingUnderCandidate, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == SubtreeInsideExistingSubtree) { candidateScope: Subtree, existingScope: Subtree, relation: CandidateUnderExisting, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == SubtreeContainsGeneratedMember) { candidateScope: Subtree, existingScope: GeneratedSet, relation: ExistingUnderCandidate, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == SubtreeDifferentPath) { candidateScope: Subtree, existingScope: Subtree, relation: DifferentPath, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == GeneratedCandidateOverlapsRepo) { candidateScope: GeneratedSet, existingScope: RepoGlobal, relation: NoRelation, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == GeneratedMatchesExact) { candidateScope: GeneratedSet, existingScope: ExactPath, relation: SharedGeneratedMember, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == GeneratedUnderExistingSubtree) { candidateScope: GeneratedSet, existingScope: Subtree, relation: CandidateUnderExisting, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == GeneratedMatchesGeneratedMember) { candidateScope: GeneratedSet, existingScope: GeneratedSet, relation: SharedGeneratedMember, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == GeneratedDifferentMember) { candidateScope: GeneratedSet, existingScope: GeneratedSet, relation: DifferentPath, existingActive: true, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == ExistingReservationExpired) { candidateScope: ExactPath, existingScope: ExactPath, relation: SamePath, existingActive: false, candidatePathValid: true, candidateMembersPresent: true }
    else if (case == GeneratedCandidateHasNoMembers) { candidateScope: GeneratedSet, existingScope: ExactPath, relation: SharedGeneratedMember, existingActive: true, candidatePathValid: true, candidateMembersPresent: false }
    else { candidateScope: ExactPath, existingScope: ExactPath, relation: SamePath, existingActive: true, candidatePathValid: false, candidateMembersPresent: true }

  def candidateValid(facts) =
    facts.candidatePathValid
    and (facts.candidateScope != GeneratedSet or facts.candidateMembersPresent)

  def expectedReject(facts) =
    if (not(facts.candidatePathValid)) InvalidPath
    else if (facts.candidateScope == GeneratedSet and not(facts.candidateMembersPresent)) EmptyGeneratedMembers
    else NoReject

  def scopesIntersect(facts) =
    facts.candidateScope == RepoGlobal
    or facts.existingScope == RepoGlobal
    or (
      facts.candidateScope == ExactPath
      and (
        (facts.existingScope == ExactPath and facts.relation == SamePath)
        or (facts.existingScope == Subtree and facts.relation == CandidateUnderExisting)
        or (facts.existingScope == GeneratedSet and facts.relation == SharedGeneratedMember)
      )
    )
    or (
      facts.candidateScope == Subtree
      and (
        facts.relation == SamePath
        or facts.relation == CandidateUnderExisting
        or facts.relation == ExistingUnderCandidate
      )
    )
    or (
      facts.candidateScope == GeneratedSet
      and (
        (facts.existingScope == ExactPath and facts.relation == SharedGeneratedMember)
        or (facts.existingScope == Subtree and facts.relation == CandidateUnderExisting)
        or (facts.existingScope == GeneratedSet and facts.relation == SharedGeneratedMember)
      )
    )

  def expectedOverlap(facts) =
    candidateValid(facts)
    and facts.existingActive
    and scopesIntersect(facts)

  action init = {
    s' = {
      caseIndex: 0,
      case: RepoCandidateOverlapsExact,
      candidateScope: RepoGlobal,
      existingScope: ExactPath,
      relation: NoRelation,
      existingActive: true,
      candidatePathValid: true,
      candidateMembersPresent: true,
      overlapReturned: false,
      conflictRecorded: false,
      rejectReason: NoReject,
      evaluated: false,
    }
  }

  action evaluate(caseIndex, case) = all {
    val facts = factsForCase(case)
    val overlaps = expectedOverlap(facts)
    s' = {
      caseIndex: caseIndex,
      case: case,
      candidateScope: facts.candidateScope,
      existingScope: facts.existingScope,
      relation: facts.relation,
      existingActive: facts.existingActive,
      candidatePathValid: facts.candidatePathValid,
      candidateMembersPresent: facts.candidateMembersPresent,
      overlapReturned: overlaps,
      conflictRecorded: overlaps,
      rejectReason: expectedReject(facts),
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(1, RepoCandidateOverlapsExact) },
    all { s.caseIndex == 1, evaluate(2, ExactCandidateOverlapsRepo) },
    all { s.caseIndex == 2, evaluate(3, ExactSamePath) },
    all { s.caseIndex == 3, evaluate(4, ExactUnderExistingSubtree) },
    all { s.caseIndex == 4, evaluate(5, ExactMatchesGeneratedMember) },
    all { s.caseIndex == 5, evaluate(6, ExactDifferentPath) },
    all { s.caseIndex == 6, evaluate(7, ExactVsDescendantSubtree) },
    all { s.caseIndex == 7, evaluate(8, SubtreeCandidateOverlapsRepo) },
    all { s.caseIndex == 8, evaluate(9, SubtreeContainsExact) },
    all { s.caseIndex == 9, evaluate(10, SubtreeContainsSubtree) },
    all { s.caseIndex == 10, evaluate(11, SubtreeInsideExistingSubtree) },
    all { s.caseIndex == 11, evaluate(12, SubtreeContainsGeneratedMember) },
    all { s.caseIndex == 12, evaluate(13, SubtreeDifferentPath) },
    all { s.caseIndex == 13, evaluate(14, GeneratedCandidateOverlapsRepo) },
    all { s.caseIndex == 14, evaluate(15, GeneratedMatchesExact) },
    all { s.caseIndex == 15, evaluate(16, GeneratedUnderExistingSubtree) },
    all { s.caseIndex == 16, evaluate(17, GeneratedMatchesGeneratedMember) },
    all { s.caseIndex == 17, evaluate(18, GeneratedDifferentMember) },
    all { s.caseIndex == 18, evaluate(19, ExistingReservationExpired) },
    all { s.caseIndex == 19, evaluate(20, GeneratedCandidateHasNoMembers) },
    all { s.caseIndex == 20, evaluate(21, CandidatePathInvalid) },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 21
    and s.case.in(Set(RepoCandidateOverlapsExact, ExactCandidateOverlapsRepo, ExactSamePath, ExactUnderExistingSubtree, ExactMatchesGeneratedMember, ExactDifferentPath, ExactVsDescendantSubtree, SubtreeCandidateOverlapsRepo, SubtreeContainsExact, SubtreeContainsSubtree, SubtreeInsideExistingSubtree, SubtreeContainsGeneratedMember, SubtreeDifferentPath, GeneratedCandidateOverlapsRepo, GeneratedMatchesExact, GeneratedUnderExistingSubtree, GeneratedMatchesGeneratedMember, GeneratedDifferentMember, ExistingReservationExpired, GeneratedCandidateHasNoMembers, CandidatePathInvalid))
    and s.candidateScope.in(Set(RepoGlobal, ExactPath, Subtree, GeneratedSet))
    and s.existingScope.in(Set(RepoGlobal, ExactPath, Subtree, GeneratedSet))
    and s.relation.in(Set(SamePath, CandidateUnderExisting, ExistingUnderCandidate, SharedGeneratedMember, DifferentPath, NoRelation))
    and s.rejectReason.in(Set(NoReject, EmptyGeneratedMembers, InvalidPath))

  val overlapMatchesScopeSemantics =
    not(s.evaluated)
    or s.overlapReturned == expectedOverlap(s)

  val repoGlobalOverlapsEveryActiveReservation =
    not(s.evaluated and s.rejectReason == NoReject and s.existingActive and (s.candidateScope == RepoGlobal or s.existingScope == RepoGlobal))
    or s.overlapReturned

  val expiredReservationsNeverOverlap =
    not(s.evaluated and not(s.existingActive))
    or not(s.overlapReturned)

  val invalidCandidateNeverOverlaps =
    not(s.evaluated and s.rejectReason != NoReject)
    or (not(s.overlapReturned) and not(s.conflictRecorded))

  val exactPathOverlapRequiresSameAncestorOrGeneratedMember =
    not(s.evaluated and s.candidateScope == ExactPath and s.existingScope != RepoGlobal and s.overlapReturned)
    or (
      (s.existingScope == ExactPath and s.relation == SamePath)
      or (s.existingScope == Subtree and s.relation == CandidateUnderExisting)
      or (s.existingScope == GeneratedSet and s.relation == SharedGeneratedMember)
    )

  val subtreeOverlapRequiresContainmentOrRepoGlobal =
    not(s.evaluated and s.candidateScope == Subtree and s.existingScope != RepoGlobal and s.overlapReturned)
    or s.relation.in(Set(SamePath, CandidateUnderExisting, ExistingUnderCandidate))

  val generatedOverlapRequiresMemberIntersectionOrRepoGlobal =
    not(s.evaluated and s.candidateScope == GeneratedSet and s.existingScope != RepoGlobal and s.overlapReturned)
    or (
      (s.existingScope == ExactPath and s.relation == SharedGeneratedMember)
      or (s.existingScope == Subtree and s.relation == CandidateUnderExisting)
      or (s.existingScope == GeneratedSet and s.relation == SharedGeneratedMember)
    )

  val conflictsRecordedOnlyForReturnedOverlaps =
    not(s.conflictRecorded) or s.overlapReturned
}

module CoveyReservationRecordShape {
  type Case =
    ValidExactPathActive
    | ValidSubtreeReleased
    | ValidRepoGlobalExpired
    | ValidGeneratedSetActive
    | ExactPathWithGeneratedMembers
    | SubtreeWithGeneratedMembers
    | RepoGlobalWrongKey
    | RepoGlobalWithGeneratedMembers
    | GeneratedSetWithoutMembers
    | GeneratedSetBlankMember
    | GeneratedSetPaddedMember
    | GeneratedSetDuplicateMembers
    | BlankScopeKey
    | PaddedScopeKey
    | UpdatedBeforeCreated

  type Scope = ExactPath | Subtree | RepoGlobal | GeneratedSet
  type Lifecycle = Active | Released | Expired
  type TimestampShape = Monotonic | UpdatedBeforeCreatedShape
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | ScopeKeyMissing
    | ScopeKeyNotNormalized
    | GeneratedMembersForbidden
    | RepoGlobalKeyInvalid
    | GeneratedMembersMissing
    | GeneratedMemberInvalid
    | GeneratedMemberDuplicate
    | UpdatedBeforeCreatedReject

  type Model = {
    caseIndex: int,
    case: Case,
    scope: Scope,
    lifecycle: Lifecycle,
    scopeKeyPresent: bool,
    scopeKeyNormalized: bool,
    repoGlobalKeyCanonical: bool,
    generatedMembersPresent: bool,
    generatedMembersAllowed: bool,
    generatedMembersNormalized: bool,
    generatedMembersUnique: bool,
    timestampShape: TimestampShape,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    scope:
      if (case.in(Set(ValidSubtreeReleased, SubtreeWithGeneratedMembers))) Subtree
      else if (case.in(Set(ValidRepoGlobalExpired, RepoGlobalWrongKey, RepoGlobalWithGeneratedMembers))) RepoGlobal
      else if (case.in(Set(ValidGeneratedSetActive, GeneratedSetWithoutMembers, GeneratedSetBlankMember, GeneratedSetPaddedMember, GeneratedSetDuplicateMembers))) GeneratedSet
      else ExactPath,
    lifecycle:
      if (case == ValidSubtreeReleased) Released
      else if (case == ValidRepoGlobalExpired) Expired
      else Active,
    scopeKeyPresent: case != BlankScopeKey,
    scopeKeyNormalized: case != PaddedScopeKey,
    repoGlobalKeyCanonical: case != RepoGlobalWrongKey,
    generatedMembersPresent:
      case.in(Set(ValidGeneratedSetActive, ExactPathWithGeneratedMembers, SubtreeWithGeneratedMembers, RepoGlobalWithGeneratedMembers, GeneratedSetBlankMember, GeneratedSetPaddedMember, GeneratedSetDuplicateMembers)),
    generatedMembersAllowed:
      case.in(Set(ValidGeneratedSetActive, GeneratedSetWithoutMembers, GeneratedSetBlankMember, GeneratedSetPaddedMember, GeneratedSetDuplicateMembers)),
    generatedMembersNormalized:
      not(case.in(Set(GeneratedSetBlankMember, GeneratedSetPaddedMember))),
    generatedMembersUnique:
      case != GeneratedSetDuplicateMembers,
    timestampShape:
      if (case == UpdatedBeforeCreated) UpdatedBeforeCreatedShape else Monotonic,
  }

  def scopeRejectFor(facts) =
    if (not(facts.scopeKeyPresent)) ScopeKeyMissing
    else if (not(facts.scopeKeyNormalized)) ScopeKeyNotNormalized
    else if (facts.scope != GeneratedSet and facts.generatedMembersPresent) GeneratedMembersForbidden
    else if (facts.scope == RepoGlobal and not(facts.repoGlobalKeyCanonical)) RepoGlobalKeyInvalid
    else if (facts.scope == GeneratedSet and not(facts.generatedMembersPresent)) GeneratedMembersMissing
    else if (facts.scope == GeneratedSet and not(facts.generatedMembersNormalized)) GeneratedMemberInvalid
    else if (facts.scope == GeneratedSet and not(facts.generatedMembersUnique)) GeneratedMemberDuplicate
    else NoReject

  def rejectFor(facts) = {
    val scopeReject = scopeRejectFor(facts)
    if (scopeReject != NoReject) scopeReject
    else if (facts.timestampShape == UpdatedBeforeCreatedShape) UpdatedBeforeCreatedReject
    else NoReject
  }

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidExactPathActive,
      scope: ExactPath,
      lifecycle: Active,
      scopeKeyPresent: false,
      scopeKeyNormalized: false,
      repoGlobalKeyCanonical: false,
      generatedMembersPresent: false,
      generatedMembersAllowed: false,
      generatedMembersNormalized: false,
      generatedMembersUnique: false,
      timestampShape: Monotonic,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val facts = factsFor(case)
    val rejectReason = rejectFor(facts)
    val accepted = rejectReason == NoReject
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      scope: facts.scope,
      lifecycle: facts.lifecycle,
      scopeKeyPresent: facts.scopeKeyPresent,
      scopeKeyNormalized: facts.scopeKeyNormalized,
      repoGlobalKeyCanonical: facts.repoGlobalKeyCanonical,
      generatedMembersPresent: facts.generatedMembersPresent,
      generatedMembersAllowed: facts.generatedMembersAllowed,
      generatedMembersNormalized: facts.generatedMembersNormalized,
      generatedMembersUnique: facts.generatedMembersUnique,
      timestampShape: facts.timestampShape,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidExactPathActive) },
    all { s.caseIndex == 1, evaluate(ValidSubtreeReleased) },
    all { s.caseIndex == 2, evaluate(ValidRepoGlobalExpired) },
    all { s.caseIndex == 3, evaluate(ValidGeneratedSetActive) },
    all { s.caseIndex == 4, evaluate(ExactPathWithGeneratedMembers) },
    all { s.caseIndex == 5, evaluate(SubtreeWithGeneratedMembers) },
    all { s.caseIndex == 6, evaluate(RepoGlobalWrongKey) },
    all { s.caseIndex == 7, evaluate(RepoGlobalWithGeneratedMembers) },
    all { s.caseIndex == 8, evaluate(GeneratedSetWithoutMembers) },
    all { s.caseIndex == 9, evaluate(GeneratedSetBlankMember) },
    all { s.caseIndex == 10, evaluate(GeneratedSetPaddedMember) },
    all { s.caseIndex == 11, evaluate(GeneratedSetDuplicateMembers) },
    all { s.caseIndex == 12, evaluate(BlankScopeKey) },
    all { s.caseIndex == 13, evaluate(PaddedScopeKey) },
    all { s.caseIndex == 14, evaluate(UpdatedBeforeCreated) },
    all { s.caseIndex >= 15, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 15
    and s.case.in(Set(ValidExactPathActive, ValidSubtreeReleased, ValidRepoGlobalExpired, ValidGeneratedSetActive, ExactPathWithGeneratedMembers, SubtreeWithGeneratedMembers, RepoGlobalWrongKey, RepoGlobalWithGeneratedMembers, GeneratedSetWithoutMembers, GeneratedSetBlankMember, GeneratedSetPaddedMember, GeneratedSetDuplicateMembers, BlankScopeKey, PaddedScopeKey, UpdatedBeforeCreated))
    and s.scope.in(Set(ExactPath, Subtree, RepoGlobal, GeneratedSet))
    and s.lifecycle.in(Set(Active, Released, Expired))
    and s.timestampShape.in(Set(Monotonic, UpdatedBeforeCreatedShape))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, ScopeKeyMissing, ScopeKeyNotNormalized, GeneratedMembersForbidden, RepoGlobalKeyInvalid, GeneratedMembersMissing, GeneratedMemberInvalid, GeneratedMemberDuplicate, UpdatedBeforeCreatedReject))

  val acceptedRequiresNormalizedScopeKey =
    not(s.accepted) or (s.scopeKeyPresent and s.scopeKeyNormalized)

  val nonGeneratedScopesForbidGeneratedMembers =
    not(s.accepted and s.scope != GeneratedSet) or not(s.generatedMembersPresent)

  val repoGlobalUsesCanonicalScopeKey =
    not(s.accepted and s.scope == RepoGlobal) or s.repoGlobalKeyCanonical

  val generatedSetRequiresValidUniqueMembers =
    not(s.accepted and s.scope == GeneratedSet)
    or (
      s.generatedMembersPresent
      and s.generatedMembersAllowed
      and s.generatedMembersNormalized
      and s.generatedMembersUnique
    )

  val acceptedTimestampsAreMonotonic =
    not(s.accepted) or s.timestampShape == Monotonic

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      scope: s.scope,
      scopeKeyPresent: s.scopeKeyPresent,
      scopeKeyNormalized: s.scopeKeyNormalized,
      repoGlobalKeyCanonical: s.repoGlobalKeyCanonical,
      generatedMembersPresent: s.generatedMembersPresent,
      generatedMembersAllowed: s.generatedMembersAllowed,
      generatedMembersNormalized: s.generatedMembersNormalized,
      generatedMembersUnique: s.generatedMembersUnique,
      timestampShape: s.timestampShape,
    })
}

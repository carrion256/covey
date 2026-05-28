module CoveyReservationRequestShape {
  type Case =
    ValidExactPathRequest
    | ValidSubtreeQuery
    | ValidRepoGlobalRequest
    | ValidGeneratedSetQuery
    | InvalidSessionToken
    | InvalidOwnerSubtaskId
    | NonPositiveLeaseDuration
    | BlankIdempotencyKey
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

  type Operation = ReservationRequest | OverlapQuery
  type Scope = ExactPath | Subtree | RepoGlobal | GeneratedSet
  type Outcome = NotEvaluated | Accepted | Rejected

  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | OwnerSubtaskIdInvalid
    | LeaseDurationInvalid
    | IdempotencyKeyInvalid
    | ScopeKeyMissing
    | ScopeKeyNotNormalized
    | GeneratedMembersForbidden
    | RepoGlobalKeyInvalid
    | GeneratedMembersMissing
    | GeneratedMemberInvalid
    | GeneratedMemberDuplicate

  type Model = {
    caseIndex: int,
    case: Case,
    operation: Operation,
    sessionTokenValid: bool,
    ownerSubtaskIdValid: bool,
    leaseDurationPositive: bool,
    idempotencyKeyValid: bool,
    scope: Scope,
    scopeKeyPresent: bool,
    scopeKeyNormalized: bool,
    repoGlobalKeyCanonical: bool,
    generatedMembersPresent: bool,
    generatedMembersAllowed: bool,
    generatedMembersNormalized: bool,
    generatedMembersUnique: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    operation:
      if (case.in(Set(ValidSubtreeQuery, ValidGeneratedSetQuery, RepoGlobalWrongKey, GeneratedSetWithoutMembers, GeneratedSetBlankMember))) OverlapQuery else ReservationRequest,
    sessionTokenValid: case != InvalidSessionToken,
    ownerSubtaskIdValid: case != InvalidOwnerSubtaskId,
    leaseDurationPositive: case != NonPositiveLeaseDuration,
    idempotencyKeyValid: case != BlankIdempotencyKey,
    scope:
      if (case.in(Set(ValidSubtreeQuery, SubtreeWithGeneratedMembers))) Subtree
      else if (case.in(Set(ValidRepoGlobalRequest, RepoGlobalWrongKey, RepoGlobalWithGeneratedMembers))) RepoGlobal
      else if (case.in(Set(ValidGeneratedSetQuery, GeneratedSetWithoutMembers, GeneratedSetBlankMember, GeneratedSetPaddedMember, GeneratedSetDuplicateMembers))) GeneratedSet
      else ExactPath,
    scopeKeyPresent: case != BlankScopeKey,
    scopeKeyNormalized: case != PaddedScopeKey,
    repoGlobalKeyCanonical: case != RepoGlobalWrongKey,
    generatedMembersPresent:
      case.in(Set(ValidGeneratedSetQuery, ExactPathWithGeneratedMembers, SubtreeWithGeneratedMembers, RepoGlobalWithGeneratedMembers, GeneratedSetBlankMember, GeneratedSetPaddedMember, GeneratedSetDuplicateMembers)),
    generatedMembersAllowed:
      case.in(Set(ValidGeneratedSetQuery, GeneratedSetWithoutMembers, GeneratedSetBlankMember, GeneratedSetPaddedMember, GeneratedSetDuplicateMembers)),
    generatedMembersNormalized:
      not(case.in(Set(GeneratedSetBlankMember, GeneratedSetPaddedMember))),
    generatedMembersUnique:
      case != GeneratedSetDuplicateMembers,
  }

  def operationRequestFieldsValid(facts) =
    facts.operation == OverlapQuery
    or (
      facts.sessionTokenValid
      and facts.ownerSubtaskIdValid
      and facts.leaseDurationPositive
      and facts.idempotencyKeyValid
    )

  def scopeRejectFor(facts) =
    if (not(facts.scopeKeyPresent)) ScopeKeyMissing
    else if (not(facts.scopeKeyNormalized)) ScopeKeyNotNormalized
    else if (facts.scope != GeneratedSet and facts.generatedMembersPresent) GeneratedMembersForbidden
    else if (facts.scope == RepoGlobal and not(facts.repoGlobalKeyCanonical)) RepoGlobalKeyInvalid
    else if (facts.scope == GeneratedSet and not(facts.generatedMembersPresent)) GeneratedMembersMissing
    else if (facts.scope == GeneratedSet and not(facts.generatedMembersNormalized)) GeneratedMemberInvalid
    else if (facts.scope == GeneratedSet and not(facts.generatedMembersUnique)) GeneratedMemberDuplicate
    else NoReject

  def rejectFor(facts) =
    if (facts.operation == ReservationRequest and not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (facts.operation == ReservationRequest and not(facts.ownerSubtaskIdValid)) OwnerSubtaskIdInvalid
    else if (facts.operation == ReservationRequest and not(facts.leaseDurationPositive)) LeaseDurationInvalid
    else {
      val scopeReject = scopeRejectFor(facts)
      if (scopeReject != NoReject) scopeReject
      else if (facts.operation == ReservationRequest and not(facts.idempotencyKeyValid)) IdempotencyKeyInvalid
      else NoReject
    }

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidExactPathRequest,
      operation: ReservationRequest,
      sessionTokenValid: false,
      ownerSubtaskIdValid: false,
      leaseDurationPositive: false,
      idempotencyKeyValid: false,
      scope: ExactPath,
      scopeKeyPresent: false,
      scopeKeyNormalized: false,
      repoGlobalKeyCanonical: false,
      generatedMembersPresent: false,
      generatedMembersAllowed: false,
      generatedMembersNormalized: false,
      generatedMembersUnique: false,
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
      operation: facts.operation,
      sessionTokenValid: facts.sessionTokenValid,
      ownerSubtaskIdValid: facts.ownerSubtaskIdValid,
      leaseDurationPositive: facts.leaseDurationPositive,
      idempotencyKeyValid: facts.idempotencyKeyValid,
      scope: facts.scope,
      scopeKeyPresent: facts.scopeKeyPresent,
      scopeKeyNormalized: facts.scopeKeyNormalized,
      repoGlobalKeyCanonical: facts.repoGlobalKeyCanonical,
      generatedMembersPresent: facts.generatedMembersPresent,
      generatedMembersAllowed: facts.generatedMembersAllowed,
      generatedMembersNormalized: facts.generatedMembersNormalized,
      generatedMembersUnique: facts.generatedMembersUnique,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidExactPathRequest) },
    all { s.caseIndex == 1, evaluate(ValidSubtreeQuery) },
    all { s.caseIndex == 2, evaluate(ValidRepoGlobalRequest) },
    all { s.caseIndex == 3, evaluate(ValidGeneratedSetQuery) },
    all { s.caseIndex == 4, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 5, evaluate(InvalidOwnerSubtaskId) },
    all { s.caseIndex == 6, evaluate(NonPositiveLeaseDuration) },
    all { s.caseIndex == 7, evaluate(BlankIdempotencyKey) },
    all { s.caseIndex == 8, evaluate(ExactPathWithGeneratedMembers) },
    all { s.caseIndex == 9, evaluate(SubtreeWithGeneratedMembers) },
    all { s.caseIndex == 10, evaluate(RepoGlobalWrongKey) },
    all { s.caseIndex == 11, evaluate(RepoGlobalWithGeneratedMembers) },
    all { s.caseIndex == 12, evaluate(GeneratedSetWithoutMembers) },
    all { s.caseIndex == 13, evaluate(GeneratedSetBlankMember) },
    all { s.caseIndex == 14, evaluate(GeneratedSetPaddedMember) },
    all { s.caseIndex == 15, evaluate(GeneratedSetDuplicateMembers) },
    all { s.caseIndex == 16, evaluate(BlankScopeKey) },
    all { s.caseIndex == 17, evaluate(PaddedScopeKey) },
    all { s.caseIndex >= 18, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 18
    and s.case.in(Set(ValidExactPathRequest, ValidSubtreeQuery, ValidRepoGlobalRequest, ValidGeneratedSetQuery, InvalidSessionToken, InvalidOwnerSubtaskId, NonPositiveLeaseDuration, BlankIdempotencyKey, ExactPathWithGeneratedMembers, SubtreeWithGeneratedMembers, RepoGlobalWrongKey, RepoGlobalWithGeneratedMembers, GeneratedSetWithoutMembers, GeneratedSetBlankMember, GeneratedSetPaddedMember, GeneratedSetDuplicateMembers, BlankScopeKey, PaddedScopeKey))
    and s.operation.in(Set(ReservationRequest, OverlapQuery))
    and s.scope.in(Set(ExactPath, Subtree, RepoGlobal, GeneratedSet))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, OwnerSubtaskIdInvalid, LeaseDurationInvalid, IdempotencyKeyInvalid, ScopeKeyMissing, ScopeKeyNotNormalized, GeneratedMembersForbidden, RepoGlobalKeyInvalid, GeneratedMembersMissing, GeneratedMemberInvalid, GeneratedMemberDuplicate))

  val acceptedRequiresRequestFields =
    not(s.accepted) or operationRequestFieldsValid(s)

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

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)
}

module CoveyConflictRecordShape {
  type Case =
    ValidOpenExactPathSubtree
    | ValidAcknowledgedRepoGlobal
    | ValidResolvedGeneratedSet
    | InvalidConflictId
    | UnknownConflictKind
    | WrongObjectType
    | WrongObjectId
    | MissingTypedPayload
    | BlankReservationId
    | InvalidOverlappingReservationId
    | BlankOwnerSubtaskId
    | RepoGlobalWrongKey
    | ScopeKeyMissing
    | ScopeKeyNotNormalized
    | OverlappingScopeKeyNotNormalized

  type ObjectShape = ReservationObject | SubtaskObject
  type ConflictKindShape = ReservationOverlapKind | UnknownKind
  type PayloadShape =
    ValidPayload
    | MissingPayload
    | BlankReservation
    | InvalidOverlappingReservation
    | BlankOwnerSubtask
    | RepoGlobalInvalidKey
    | MissingScopeKey
    | PaddedScopeKey
    | PaddedOverlappingScopeKey
  type ResolutionState = Open | Acknowledged | Resolved
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | ConflictIdInvalid
    | ConflictKindUnknown
    | ObjectTypeMismatch
    | TypedPayloadInvalid
    | ObjectIdMismatch

  type Model = {
    caseIndex: int,
    case: Case,
    objectShape: ObjectShape,
    conflictKindShape: ConflictKindShape,
    payloadShape: PayloadShape,
    resolutionState: ResolutionState,
    conflictIdValid: bool,
    typedPayloadValid: bool,
    objectIdMatchesPayload: bool,
    scopeKeyNormalized: bool,
    overlappingScopeKeyNormalized: bool,
    repoGlobalKeyCanonical: bool,
    payloadJsonPreserved: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    objectShape:
      if (case == WrongObjectType) SubtaskObject else ReservationObject,
    conflictKindShape:
      if (case == UnknownConflictKind) UnknownKind else ReservationOverlapKind,
    payloadShape:
      if (case == MissingTypedPayload) MissingPayload
      else if (case == BlankReservationId) BlankReservation
      else if (case == InvalidOverlappingReservationId) InvalidOverlappingReservation
      else if (case == BlankOwnerSubtaskId) BlankOwnerSubtask
      else if (case == RepoGlobalWrongKey) RepoGlobalInvalidKey
      else if (case == ScopeKeyMissing) MissingScopeKey
      else if (case == ScopeKeyNotNormalized) PaddedScopeKey
      else if (case == OverlappingScopeKeyNotNormalized) PaddedOverlappingScopeKey
      else ValidPayload,
    resolutionState:
      if (case == ValidAcknowledgedRepoGlobal) Acknowledged
      else if (case == ValidResolvedGeneratedSet) Resolved
      else Open,
    conflictIdValid: case != InvalidConflictId,
    objectIdMatchesPayload: case != WrongObjectId,
    scopeKeyNormalized: not(case.in(Set(ScopeKeyMissing, ScopeKeyNotNormalized))),
    overlappingScopeKeyNormalized: case != OverlappingScopeKeyNotNormalized,
    repoGlobalKeyCanonical: case != RepoGlobalWrongKey,
  }

  def payloadValid(facts) =
    facts.payloadShape == ValidPayload

  def rejectFor(facts) =
    if (not(facts.conflictIdValid)) ConflictIdInvalid
    else if (facts.conflictKindShape == UnknownKind) ConflictKindUnknown
    else if (facts.objectShape != ReservationObject) ObjectTypeMismatch
    else if (not(payloadValid(facts))) TypedPayloadInvalid
    else if (not(facts.objectIdMatchesPayload)) ObjectIdMismatch
    else NoReject

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidOpenExactPathSubtree,
      objectShape: ReservationObject,
      conflictKindShape: ReservationOverlapKind,
      payloadShape: ValidPayload,
      resolutionState: Open,
      conflictIdValid: false,
      typedPayloadValid: false,
      objectIdMatchesPayload: false,
      scopeKeyNormalized: false,
      overlappingScopeKeyNormalized: false,
      repoGlobalKeyCanonical: false,
      payloadJsonPreserved: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val facts = factsFor(case)
    val typedPayloadValid = payloadValid(facts)
    val rejectReason = rejectFor(facts)
    val accepted = rejectReason == NoReject
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      objectShape: facts.objectShape,
      conflictKindShape: facts.conflictKindShape,
      payloadShape: facts.payloadShape,
      resolutionState: facts.resolutionState,
      conflictIdValid: facts.conflictIdValid,
      typedPayloadValid: typedPayloadValid,
      objectIdMatchesPayload: facts.objectIdMatchesPayload,
      scopeKeyNormalized: facts.scopeKeyNormalized,
      overlappingScopeKeyNormalized: facts.overlappingScopeKeyNormalized,
      repoGlobalKeyCanonical: facts.repoGlobalKeyCanonical,
      payloadJsonPreserved: accepted,
      outcome: if (accepted) Accepted else Rejected,
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidOpenExactPathSubtree) },
    all { s.caseIndex == 1, evaluate(ValidAcknowledgedRepoGlobal) },
    all { s.caseIndex == 2, evaluate(ValidResolvedGeneratedSet) },
    all { s.caseIndex == 3, evaluate(InvalidConflictId) },
    all { s.caseIndex == 4, evaluate(UnknownConflictKind) },
    all { s.caseIndex == 5, evaluate(WrongObjectType) },
    all { s.caseIndex == 6, evaluate(WrongObjectId) },
    all { s.caseIndex == 7, evaluate(MissingTypedPayload) },
    all { s.caseIndex == 8, evaluate(BlankReservationId) },
    all { s.caseIndex == 9, evaluate(InvalidOverlappingReservationId) },
    all { s.caseIndex == 10, evaluate(BlankOwnerSubtaskId) },
    all { s.caseIndex == 11, evaluate(RepoGlobalWrongKey) },
    all { s.caseIndex == 12, evaluate(ScopeKeyMissing) },
    all { s.caseIndex == 13, evaluate(ScopeKeyNotNormalized) },
    all { s.caseIndex == 14, evaluate(OverlappingScopeKeyNotNormalized) },
    all { s.caseIndex >= 15, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 15
    and s.case.in(Set(ValidOpenExactPathSubtree, ValidAcknowledgedRepoGlobal, ValidResolvedGeneratedSet, InvalidConflictId, UnknownConflictKind, WrongObjectType, WrongObjectId, MissingTypedPayload, BlankReservationId, InvalidOverlappingReservationId, BlankOwnerSubtaskId, RepoGlobalWrongKey, ScopeKeyMissing, ScopeKeyNotNormalized, OverlappingScopeKeyNotNormalized))
    and s.objectShape.in(Set(ReservationObject, SubtaskObject))
    and s.conflictKindShape.in(Set(ReservationOverlapKind, UnknownKind))
    and s.payloadShape.in(Set(ValidPayload, MissingPayload, BlankReservation, InvalidOverlappingReservation, BlankOwnerSubtask, RepoGlobalInvalidKey, MissingScopeKey, PaddedScopeKey, PaddedOverlappingScopeKey))
    and s.resolutionState.in(Set(Open, Acknowledged, Resolved))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, ConflictIdInvalid, ConflictKindUnknown, ObjectTypeMismatch, TypedPayloadInvalid, ObjectIdMismatch))

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedRequiresReservationOverlapBinding =
    not(s.accepted)
    or (
      s.conflictIdValid
      and s.objectShape == ReservationObject
      and s.conflictKindShape == ReservationOverlapKind
      and s.typedPayloadValid
      and s.objectIdMatchesPayload
    )

  val acceptedPayloadKeepsTypedJson =
    not(s.accepted) or s.payloadJsonPreserved

  val acceptedScopesAreNormalized =
    not(s.accepted)
    or (
      s.scopeKeyNormalized
      and s.overlappingScopeKeyNormalized
      and s.repoGlobalKeyCanonical
    )

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      objectShape: s.objectShape,
      conflictKindShape: s.conflictKindShape,
      payloadShape: s.payloadShape,
      conflictIdValid: s.conflictIdValid,
      objectIdMatchesPayload: s.objectIdMatchesPayload,
    })
}

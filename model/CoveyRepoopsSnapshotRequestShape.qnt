module CoveyRepoopsSnapshotRequestShape {
  type Case =
    ValidSinglePath
    | ValidNormalizedPath
    | ValidTwoPaths
    | InvalidSessionToken
    | InvalidClaimId
    | NonPositiveFence
    | EmptyPathSet
    | EmptyPath
    | EscapingPath
    | ControlCharacterPath
    | DuplicateAfterNormalization

  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | ClaimIdInvalid
    | FenceSeqInvalid
    | EmptyPathSetInvalid
    | PathInvalid
    | DuplicatePathInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    sessionTokenValid: bool,
    claimIdValid: bool,
    fenceSeqPositive: bool,
    pathSetNonEmpty: bool,
    everyPathValid: bool,
    duplicateAfterNormalization: bool,
    normalizesPathSyntax: bool,
    serializedPathsNormalized: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    sessionTokenValid: case != InvalidSessionToken,
    claimIdValid: case != InvalidClaimId,
    fenceSeqPositive: case != NonPositiveFence,
    pathSetNonEmpty: case != EmptyPathSet,
    everyPathValid: not(case.in(Set(EmptyPath, EscapingPath, ControlCharacterPath))),
    duplicateAfterNormalization: case == DuplicateAfterNormalization,
    normalizesPathSyntax: case.in(Set(ValidNormalizedPath, DuplicateAfterNormalization)),
  }

  def rejectFor(facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.claimIdValid)) ClaimIdInvalid
    else if (not(facts.fenceSeqPositive)) FenceSeqInvalid
    else if (not(facts.pathSetNonEmpty)) EmptyPathSetInvalid
    else if (not(facts.everyPathValid)) PathInvalid
    else if (facts.duplicateAfterNormalization) DuplicatePathInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidSinglePath,
      sessionTokenValid: false,
      claimIdValid: false,
      fenceSeqPositive: false,
      pathSetNonEmpty: false,
      everyPathValid: false,
      duplicateAfterNormalization: false,
      normalizesPathSyntax: false,
      serializedPathsNormalized: false,
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
      sessionTokenValid: facts.sessionTokenValid,
      claimIdValid: facts.claimIdValid,
      fenceSeqPositive: facts.fenceSeqPositive,
      pathSetNonEmpty: facts.pathSetNonEmpty,
      everyPathValid: facts.everyPathValid,
      duplicateAfterNormalization: facts.duplicateAfterNormalization,
      normalizesPathSyntax: facts.normalizesPathSyntax,
      serializedPathsNormalized: accepted,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidSinglePath) },
    all { s.caseIndex == 1, evaluate(ValidNormalizedPath) },
    all { s.caseIndex == 2, evaluate(ValidTwoPaths) },
    all { s.caseIndex == 3, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 4, evaluate(InvalidClaimId) },
    all { s.caseIndex == 5, evaluate(NonPositiveFence) },
    all { s.caseIndex == 6, evaluate(EmptyPathSet) },
    all { s.caseIndex == 7, evaluate(EmptyPath) },
    all { s.caseIndex == 8, evaluate(EscapingPath) },
    all { s.caseIndex == 9, evaluate(ControlCharacterPath) },
    all { s.caseIndex == 10, evaluate(DuplicateAfterNormalization) },
    all { s.caseIndex >= 11, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 11
    and s.case.in(Set(ValidSinglePath, ValidNormalizedPath, ValidTwoPaths, InvalidSessionToken, InvalidClaimId, NonPositiveFence, EmptyPathSet, EmptyPath, EscapingPath, ControlCharacterPath, DuplicateAfterNormalization))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, ClaimIdInvalid, FenceSeqInvalid, EmptyPathSetInvalid, PathInvalid, DuplicatePathInvalid))

  val acceptedRequiresIdentityAndFence =
    not(s.accepted) or (s.sessionTokenValid and s.claimIdValid and s.fenceSeqPositive)

  val acceptedRequiresNonEmptyValidUniquePaths =
    not(s.accepted)
    or (s.pathSetNonEmpty and s.everyPathValid and not(s.duplicateAfterNormalization))

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val serializedPathsAreNormalizedOnSuccess =
    not(s.accepted) or s.serializedPathsNormalized
}

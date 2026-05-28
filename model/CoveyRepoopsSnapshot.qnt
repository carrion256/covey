module CoveyRepoopsSnapshot {
  type Case =
    ValidInProgressOwnedLock
    | ValidOpenClaim
    | NoMatchingReservation
    | ForeignReservationLock
    | InvalidCurrentClaim
    | InvalidRequestedPath
    | OwnerReservationNotInScope
    | RawOwnershipTokenLeak
    | OpenClaimWithActiveToken
    | InProgressClaimMissingToken

  type SubtaskState =
    Available
    | Claimed
    | InProgress
    | ArtifactPublished
    | ReviewPending
    | ChangesRequested
    | Approved
    | Decided
    | ReadyForApply
    | Applied
    | Blocked
    | Abandoned

  type ClaimFactStatus = NoClaimFact | ClaimOpen | ClaimInProgress
  type LockKind = NoLock | OwnedLock | ForeignLock
  type TokenShape = NoToken | TokenRef | RawToken
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | CurrentClaimInvalid
    | PathInvalid
    | ClaimFactInvalid
    | LockFactInvalid
    | ScopeFactInvalid
    | OwnershipTokenInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    currentClaimValid: bool,
    requestedPathValid: bool,
    subtask: SubtaskState,
    ownerReservationActive: bool,
    foreignReservationActive: bool,
    reservationCoversRequestedPath: bool,
    ownerReservationInScope: bool,
    claimFactStatus: ClaimFactStatus,
    activeOwnershipToken: TokenShape,
    callerOwnershipToken: TokenShape,
    scopeIncludesOwnerReservation: bool,
    lockKind: LockKind,
    lockOwnerMatchesSession: bool,
    lockClaimRefMatchesClaim: bool,
    factSourcesUseTokenRefs: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
  }

  var s: Model

  def expectedClaimStatus(subtask) =
    if (subtask == InProgress) ClaimInProgress else ClaimOpen

  def factsForCase(case) = {
    currentClaimValid: case != InvalidCurrentClaim,
    requestedPathValid: case != InvalidRequestedPath,
    subtask:
      if (case.in(Set(ValidInProgressOwnedLock, RawOwnershipTokenLeak, InProgressClaimMissingToken))) InProgress
      else ReadyForApply,
    ownerReservationActive:
      case.in(Set(ValidInProgressOwnedLock, ValidOpenClaim, OwnerReservationNotInScope, RawOwnershipTokenLeak, OpenClaimWithActiveToken, InProgressClaimMissingToken)),
    foreignReservationActive: case == ForeignReservationLock,
    reservationCoversRequestedPath: case != NoMatchingReservation,
    ownerReservationInScope: case != OwnerReservationNotInScope,
    claimFactStatus:
      if (case == InvalidCurrentClaim) NoClaimFact
      else if (case == InProgressClaimMissingToken) ClaimInProgress
      else if (case == OpenClaimWithActiveToken) ClaimOpen
      else if (case == RawOwnershipTokenLeak) ClaimInProgress
      else expectedClaimStatus(if (case == ValidInProgressOwnedLock) InProgress else ReadyForApply),
    activeOwnershipToken:
      if (case == InProgressClaimMissingToken) NoToken
      else if (case == RawOwnershipTokenLeak) RawToken
      else if (case == OpenClaimWithActiveToken) TokenRef
      else if (case == ValidInProgressOwnedLock) TokenRef
      else NoToken,
    callerOwnershipToken: if (case == RawOwnershipTokenLeak) RawToken else TokenRef,
    scopeIncludesOwnerReservation:
      case.in(Set(ValidInProgressOwnedLock, ValidOpenClaim, RawOwnershipTokenLeak, OpenClaimWithActiveToken, InProgressClaimMissingToken)),
    lockKind:
      if (case == ForeignReservationLock) ForeignLock
      else if (case.in(Set(ValidInProgressOwnedLock, ValidOpenClaim, OwnerReservationNotInScope, RawOwnershipTokenLeak, OpenClaimWithActiveToken, InProgressClaimMissingToken))) OwnedLock
      else NoLock,
    lockOwnerMatchesSession: case != ForeignReservationLock,
    lockClaimRefMatchesClaim: case != ForeignReservationLock,
    factSourcesUseTokenRefs: case != RawOwnershipTokenLeak,
  }

  def tokenRefOnly(token) = token != RawToken

  def currentClaimOk(facts) =
    facts.currentClaimValid

  def pathOk(facts) =
    facts.requestedPathValid

  def claimFactOk(facts) =
    facts.claimFactStatus == expectedClaimStatus(facts.subtask)
    and (
      (facts.claimFactStatus == ClaimInProgress and facts.activeOwnershipToken != NoToken)
      or (facts.claimFactStatus == ClaimOpen and facts.activeOwnershipToken == NoToken)
    )

  def scopeFactOk(facts) =
    not(facts.ownerReservationActive)
    or (facts.ownerReservationInScope and facts.scopeIncludesOwnerReservation)

  def lockFactOk(facts) =
    if (not(facts.reservationCoversRequestedPath)) {
      facts.lockKind == NoLock
    } else if (facts.ownerReservationActive) {
      facts.lockKind == OwnedLock
      and facts.lockOwnerMatchesSession
      and facts.lockClaimRefMatchesClaim
    } else if (facts.foreignReservationActive) {
      facts.lockKind == ForeignLock
      and not(facts.lockOwnerMatchesSession)
      and not(facts.lockClaimRefMatchesClaim)
    } else {
      facts.lockKind == NoLock
    }

  def ownershipTokenOk(facts) =
    tokenRefOnly(facts.callerOwnershipToken)
    and tokenRefOnly(facts.activeOwnershipToken)
    and facts.factSourcesUseTokenRefs

  def expectedReject(facts) =
    if (not(currentClaimOk(facts))) CurrentClaimInvalid
    else if (not(pathOk(facts))) PathInvalid
    else if (not(claimFactOk(facts))) ClaimFactInvalid
    else if (not(scopeFactOk(facts))) ScopeFactInvalid
    else if (not(lockFactOk(facts))) LockFactInvalid
    else if (not(ownershipTokenOk(facts))) OwnershipTokenInvalid
    else NoReject

  action evaluate(caseIndex, case) = all {
    val facts = factsForCase(case)
    val reason = expectedReject(facts)
    s' = {
      caseIndex: caseIndex,
      case: case,
      currentClaimValid: facts.currentClaimValid,
      requestedPathValid: facts.requestedPathValid,
      subtask: facts.subtask,
      ownerReservationActive: facts.ownerReservationActive,
      foreignReservationActive: facts.foreignReservationActive,
      reservationCoversRequestedPath: facts.reservationCoversRequestedPath,
      ownerReservationInScope: facts.ownerReservationInScope,
      claimFactStatus: facts.claimFactStatus,
      activeOwnershipToken: facts.activeOwnershipToken,
      callerOwnershipToken: facts.callerOwnershipToken,
      scopeIncludesOwnerReservation: facts.scopeIncludesOwnerReservation,
      lockKind: facts.lockKind,
      lockOwnerMatchesSession: facts.lockOwnerMatchesSession,
      lockClaimRefMatchesClaim: facts.lockClaimRefMatchesClaim,
      factSourcesUseTokenRefs: facts.factSourcesUseTokenRefs,
      outcome: if (reason == NoReject) Accepted else Rejected,
      rejectReason: reason,
      accepted: reason == NoReject,
    }
  }

  action init = all {
    s' = {
      caseIndex: 0,
      case: ValidInProgressOwnedLock,
      currentClaimValid: false,
      requestedPathValid: false,
      subtask: Available,
      ownerReservationActive: false,
      foreignReservationActive: false,
      reservationCoversRequestedPath: false,
      ownerReservationInScope: false,
      claimFactStatus: NoClaimFact,
      activeOwnershipToken: NoToken,
      callerOwnershipToken: NoToken,
      scopeIncludesOwnerReservation: false,
      lockKind: NoLock,
      lockOwnerMatchesSession: false,
      lockClaimRefMatchesClaim: false,
      factSourcesUseTokenRefs: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(1, ValidInProgressOwnedLock) },
    all { s.caseIndex == 1, evaluate(2, ValidOpenClaim) },
    all { s.caseIndex == 2, evaluate(3, NoMatchingReservation) },
    all { s.caseIndex == 3, evaluate(4, ForeignReservationLock) },
    all { s.caseIndex == 4, evaluate(5, InvalidCurrentClaim) },
    all { s.caseIndex == 5, evaluate(6, InvalidRequestedPath) },
    all { s.caseIndex == 6, evaluate(7, OwnerReservationNotInScope) },
    all { s.caseIndex == 7, evaluate(8, RawOwnershipTokenLeak) },
    all { s.caseIndex == 8, evaluate(9, OpenClaimWithActiveToken) },
    all { s.caseIndex == 9, evaluate(10, InProgressClaimMissingToken) },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.case.in(Set(ValidInProgressOwnedLock, ValidOpenClaim, NoMatchingReservation, ForeignReservationLock, InvalidCurrentClaim, InvalidRequestedPath, OwnerReservationNotInScope, RawOwnershipTokenLeak, OpenClaimWithActiveToken, InProgressClaimMissingToken))
    and s.subtask.in(Set(Available, Claimed, InProgress, ArtifactPublished, ReviewPending, ChangesRequested, Approved, Decided, ReadyForApply, Applied, Blocked, Abandoned))
    and s.claimFactStatus.in(Set(NoClaimFact, ClaimOpen, ClaimInProgress))
    and s.lockKind.in(Set(NoLock, OwnedLock, ForeignLock))
    and s.activeOwnershipToken.in(Set(NoToken, TokenRef, RawToken))
    and s.callerOwnershipToken.in(Set(NoToken, TokenRef, RawToken))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, CurrentClaimInvalid, PathInvalid, ClaimFactInvalid, LockFactInvalid, ScopeFactInvalid, OwnershipTokenInvalid))

  val rejectReasonMatchesFirstFailedGate =
    s.outcome == NotEvaluated
    or s.rejectReason == expectedReject(s)

  val acceptedOnlyWithValidCurrentClaimAndPath =
    not(s.accepted) or (s.currentClaimValid and s.requestedPathValid)

  val acceptedInProgressSubtaskProducesInProgressClaimFact =
    not(s.accepted) or s.subtask != InProgress or s.claimFactStatus == ClaimInProgress

  val acceptedNonInProgressSubtaskProducesOpenClaimFact =
    not(s.accepted) or s.subtask == InProgress or s.claimFactStatus == ClaimOpen

  val acceptedInProgressClaimCarriesTokenRef =
    not(s.accepted) or s.claimFactStatus != ClaimInProgress or s.activeOwnershipToken == TokenRef

  val acceptedOpenClaimCarriesNoActiveToken =
    not(s.accepted) or s.claimFactStatus != ClaimOpen or s.activeOwnershipToken == NoToken

  val ownedLockRequiresOwnerReservationCoverage =
    s.lockKind != OwnedLock
    or (s.ownerReservationActive and s.reservationCoversRequestedPath and s.lockOwnerMatchesSession and s.lockClaimRefMatchesClaim)

  val foreignLockRequiresForeignReservationCoverage =
    s.lockKind != ForeignLock
    or (s.foreignReservationActive and s.reservationCoversRequestedPath and not(s.lockOwnerMatchesSession) and not(s.lockClaimRefMatchesClaim))

  val missingMatchingReservationProducesNoLock =
    s.reservationCoversRequestedPath or s.lockKind == NoLock

  val acceptedScopeContainsOwnerReservations =
    not(s.accepted) or not(s.ownerReservationActive) or (s.ownerReservationInScope and s.scopeIncludesOwnerReservation)

  val acceptedSnapshotsNeverExposeRawSessionToken =
    not(s.accepted)
    or s.activeOwnershipToken != RawToken
    and s.callerOwnershipToken != RawToken
    and s.factSourcesUseTokenRefs
}

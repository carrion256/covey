module CoveyEnqueueForApplyRequestShape {
  type Case =
    ValidCanonical
    | InvalidSessionToken
    | InvalidArtifactDigest
    | InvalidSubtaskId
    | BlankIdempotencyKey

  type SettlementTarget = Canonical | UnsupportedInternal
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | ArtifactDigestInvalid
    | SubtaskIdInvalid
    | IdempotencyKeyInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    sessionTokenValid: bool,
    artifactDigestValid: bool,
    subtaskIdValid: bool,
    settlementTarget: SettlementTarget,
    idempotencyKeyValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    sessionTokenValid: case != InvalidSessionToken,
    artifactDigestValid: case != InvalidArtifactDigest,
    subtaskIdValid: case != InvalidSubtaskId,
    settlementTarget: Canonical,
    idempotencyKeyValid: case != BlankIdempotencyKey,
  }

  // Mirrors EnqueueForApplyReq::try_from_raw_parts validation order.
  def rejectFor(facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.artifactDigestValid)) ArtifactDigestInvalid
    else if (not(facts.subtaskIdValid)) SubtaskIdInvalid
    else if (not(facts.idempotencyKeyValid)) IdempotencyKeyInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidCanonical,
      sessionTokenValid: false,
      artifactDigestValid: false,
      subtaskIdValid: false,
      settlementTarget: Canonical,
      idempotencyKeyValid: false,
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
      artifactDigestValid: facts.artifactDigestValid,
      subtaskIdValid: facts.subtaskIdValid,
      settlementTarget: facts.settlementTarget,
      idempotencyKeyValid: facts.idempotencyKeyValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidCanonical) },
    all { s.caseIndex == 1, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 2, evaluate(InvalidArtifactDigest) },
    all { s.caseIndex == 3, evaluate(InvalidSubtaskId) },
    all { s.caseIndex == 4, evaluate(BlankIdempotencyKey) },
    all { s.caseIndex >= 5, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 5
    and s.case.in(Set(ValidCanonical, InvalidSessionToken, InvalidArtifactDigest, InvalidSubtaskId, BlankIdempotencyKey))
    and s.settlementTarget.in(Set(Canonical, UnsupportedInternal))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, ArtifactDigestInvalid, SubtaskIdInvalid, IdempotencyKeyInvalid))

  val acceptedRequiresSessionArtifactAndSubtask =
    not(s.accepted)
    or (s.sessionTokenValid and s.artifactDigestValid and s.subtaskIdValid)

  val acceptedRequiresCanonicalSettlementTarget =
    not(s.accepted) or s.settlementTarget == Canonical

  val acceptedRequiresIdempotencyKey =
    not(s.accepted) or s.idempotencyKeyValid

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      sessionTokenValid: s.sessionTokenValid,
      artifactDigestValid: s.artifactDigestValid,
      subtaskIdValid: s.subtaskIdValid,
      settlementTarget: s.settlementTarget,
      idempotencyKeyValid: s.idempotencyKeyValid,
    })
}

module CoveyRequestReviewRequestShape {
  type Case =
    ValidAutoReviewSubtask
    | ValidExplicitReviewSubtask
    | InvalidSessionToken
    | InvalidSubtaskId
    | InvalidArtifactDigest
    | InvalidReviewSubtaskId
    | NegativePriority
    | TooHighPriority
    | BlankIdempotencyKey

  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | SubtaskIdInvalid
    | ArtifactDigestInvalid
    | ReviewSubtaskIdInvalid
    | PriorityInvalid
    | IdempotencyKeyInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    sessionTokenValid: bool,
    subtaskIdValid: bool,
    artifactDigestValid: bool,
    explicitReviewSubtaskPresent: bool,
    reviewSubtaskIdValid: bool,
    priorityValid: bool,
    idempotencyKeyValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    sessionTokenValid: case != InvalidSessionToken,
    subtaskIdValid: case != InvalidSubtaskId,
    artifactDigestValid: case != InvalidArtifactDigest,
    explicitReviewSubtaskPresent: not(case == ValidAutoReviewSubtask),
    reviewSubtaskIdValid: case != InvalidReviewSubtaskId,
    priorityValid: not(case.in(Set(NegativePriority, TooHighPriority))),
    idempotencyKeyValid: case != BlankIdempotencyKey,
  }

  // Mirrors RequestReviewReq::try_from_raw_parts validation order.
  def rejectFor(facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.subtaskIdValid)) SubtaskIdInvalid
    else if (not(facts.artifactDigestValid)) ArtifactDigestInvalid
    else if (facts.explicitReviewSubtaskPresent and not(facts.reviewSubtaskIdValid)) ReviewSubtaskIdInvalid
    else if (not(facts.priorityValid)) PriorityInvalid
    else if (not(facts.idempotencyKeyValid)) IdempotencyKeyInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidAutoReviewSubtask,
      sessionTokenValid: false,
      subtaskIdValid: false,
      artifactDigestValid: false,
      explicitReviewSubtaskPresent: false,
      reviewSubtaskIdValid: false,
      priorityValid: false,
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
      subtaskIdValid: facts.subtaskIdValid,
      artifactDigestValid: facts.artifactDigestValid,
      explicitReviewSubtaskPresent: facts.explicitReviewSubtaskPresent,
      reviewSubtaskIdValid: facts.reviewSubtaskIdValid,
      priorityValid: facts.priorityValid,
      idempotencyKeyValid: facts.idempotencyKeyValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidAutoReviewSubtask) },
    all { s.caseIndex == 1, evaluate(ValidExplicitReviewSubtask) },
    all { s.caseIndex == 2, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 3, evaluate(InvalidSubtaskId) },
    all { s.caseIndex == 4, evaluate(InvalidArtifactDigest) },
    all { s.caseIndex == 5, evaluate(InvalidReviewSubtaskId) },
    all { s.caseIndex == 6, evaluate(NegativePriority) },
    all { s.caseIndex == 7, evaluate(TooHighPriority) },
    all { s.caseIndex == 8, evaluate(BlankIdempotencyKey) },
    all { s.caseIndex >= 9, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 9
    and s.case.in(Set(ValidAutoReviewSubtask, ValidExplicitReviewSubtask, InvalidSessionToken, InvalidSubtaskId, InvalidArtifactDigest, InvalidReviewSubtaskId, NegativePriority, TooHighPriority, BlankIdempotencyKey))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, SubtaskIdInvalid, ArtifactDigestInvalid, ReviewSubtaskIdInvalid, PriorityInvalid, IdempotencyKeyInvalid))

  val acceptedRequiresSessionTargetAndArtifact =
    not(s.accepted)
    or (s.sessionTokenValid and s.subtaskIdValid and s.artifactDigestValid)

  val acceptedExplicitReviewSubtaskIsValid =
    not(s.accepted and s.explicitReviewSubtaskPresent)
    or s.reviewSubtaskIdValid

  val acceptedRequiresPriorityAndIdempotency =
    not(s.accepted) or (s.priorityValid and s.idempotencyKeyValid)

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      sessionTokenValid: s.sessionTokenValid,
      subtaskIdValid: s.subtaskIdValid,
      artifactDigestValid: s.artifactDigestValid,
      explicitReviewSubtaskPresent: s.explicitReviewSubtaskPresent,
      reviewSubtaskIdValid: s.reviewSubtaskIdValid,
      priorityValid: s.priorityValid,
      idempotencyKeyValid: s.idempotencyKeyValid,
    })
}

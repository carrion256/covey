module CoveyReadyQueueMetrics {
  type Case =
    EmptyBoth
    | QueuedNonEmpty
    | InFlightNonEmpty
    | BothNonEmpty
    | EmptyQueuedWithAge
    | QueuedMissingAge
    | QueuedNegativeAge
    | EmptyInFlightWithAge
    | InFlightMissingAge
    | InFlightNegativeAge

  type BucketShape = EmptyBucket | NonEmptyBucket | InvalidBucket
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | EmptyQueuedHasAge
    | NonEmptyQueuedMissingAge
    | NegativeQueuedAge
    | EmptyInFlightHasAge
    | NonEmptyInFlightMissingAge
    | NegativeInFlightAge

  type Model = {
    caseIndex: int,
    case: Case,
    queuedCount: int,
    inFlightCount: int,
    queuedAgePresent: bool,
    inFlightAgePresent: bool,
    queuedAgeNonNegative: bool,
    inFlightAgeNonNegative: bool,
    queuedShape: BucketShape,
    inFlightShape: BucketShape,
    outcome: Outcome,
    rejectReason: RejectReason,
    evaluated: bool,
  }

  var s: Model

  def factsForCase(case) = {
    queuedCount:
      if (case.in(Set(QueuedNonEmpty, BothNonEmpty, QueuedMissingAge, QueuedNegativeAge))) 1
      else 0,
    inFlightCount:
      if (case.in(Set(InFlightNonEmpty, BothNonEmpty, InFlightMissingAge, InFlightNegativeAge))) 1
      else 0,
    queuedAgePresent:
      case.in(Set(QueuedNonEmpty, BothNonEmpty, EmptyQueuedWithAge, QueuedNegativeAge)),
    inFlightAgePresent:
      case.in(Set(InFlightNonEmpty, BothNonEmpty, EmptyInFlightWithAge, InFlightNegativeAge)),
    queuedAgeNonNegative:
      case != QueuedNegativeAge,
    inFlightAgeNonNegative:
      case != InFlightNegativeAge,
  }

  def bucketShape(count, agePresent, ageNonNegative) =
    if (count == 0 and not(agePresent)) EmptyBucket
    else if (count > 0 and agePresent and ageNonNegative) NonEmptyBucket
    else InvalidBucket

  def expectedReject(facts) =
    if (facts.queuedCount == 0 and facts.queuedAgePresent) EmptyQueuedHasAge
    else if (facts.queuedCount > 0 and not(facts.queuedAgePresent)) NonEmptyQueuedMissingAge
    else if (facts.queuedAgePresent and not(facts.queuedAgeNonNegative)) NegativeQueuedAge
    else if (facts.inFlightCount == 0 and facts.inFlightAgePresent) EmptyInFlightHasAge
    else if (facts.inFlightCount > 0 and not(facts.inFlightAgePresent)) NonEmptyInFlightMissingAge
    else if (facts.inFlightAgePresent and not(facts.inFlightAgeNonNegative)) NegativeInFlightAge
    else NoReject

  action init = {
    s' = {
      caseIndex: 0,
      case: EmptyBoth,
      queuedCount: 0,
      inFlightCount: 0,
      queuedAgePresent: false,
      inFlightAgePresent: false,
      queuedAgeNonNegative: true,
      inFlightAgeNonNegative: true,
      queuedShape: EmptyBucket,
      inFlightShape: EmptyBucket,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val facts = factsForCase(case)
    val reject = expectedReject(facts)
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      queuedCount: facts.queuedCount,
      inFlightCount: facts.inFlightCount,
      queuedAgePresent: facts.queuedAgePresent,
      inFlightAgePresent: facts.inFlightAgePresent,
      queuedAgeNonNegative: facts.queuedAgeNonNegative,
      inFlightAgeNonNegative: facts.inFlightAgeNonNegative,
      queuedShape: bucketShape(facts.queuedCount, facts.queuedAgePresent, facts.queuedAgeNonNegative),
      inFlightShape: bucketShape(facts.inFlightCount, facts.inFlightAgePresent, facts.inFlightAgeNonNegative),
      outcome: if (reject == NoReject) Accepted else Rejected,
      rejectReason: reject,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(EmptyBoth) },
    all { s.caseIndex == 1, evaluate(QueuedNonEmpty) },
    all { s.caseIndex == 2, evaluate(InFlightNonEmpty) },
    all { s.caseIndex == 3, evaluate(BothNonEmpty) },
    all { s.caseIndex == 4, evaluate(EmptyQueuedWithAge) },
    all { s.caseIndex == 5, evaluate(QueuedMissingAge) },
    all { s.caseIndex == 6, evaluate(QueuedNegativeAge) },
    all { s.caseIndex == 7, evaluate(EmptyInFlightWithAge) },
    all { s.caseIndex == 8, evaluate(InFlightMissingAge) },
    all { s.caseIndex == 9, evaluate(InFlightNegativeAge) },
    all { s.caseIndex >= 10, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 10
    and s.queuedCount >= 0
    and s.inFlightCount >= 0
    and s.case.in(Set(EmptyBoth, QueuedNonEmpty, InFlightNonEmpty, BothNonEmpty, EmptyQueuedWithAge, QueuedMissingAge, QueuedNegativeAge, EmptyInFlightWithAge, InFlightMissingAge, InFlightNegativeAge))
    and s.queuedShape.in(Set(EmptyBucket, NonEmptyBucket, InvalidBucket))
    and s.inFlightShape.in(Set(EmptyBucket, NonEmptyBucket, InvalidBucket))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, EmptyQueuedHasAge, NonEmptyQueuedMissingAge, NegativeQueuedAge, EmptyInFlightHasAge, NonEmptyInFlightMissingAge, NegativeInFlightAge))

  val rejectReasonMatchesBucketShape =
    not(s.evaluated) or s.rejectReason == expectedReject(s)

  val acceptedIffBothBucketsValid =
    not(s.evaluated)
    or (s.outcome == Accepted) == (s.queuedShape != InvalidBucket and s.inFlightShape != InvalidBucket)

  val emptyBucketsHaveNoOldestAge =
    s.outcome != Accepted
    or ((s.queuedCount != 0 or not(s.queuedAgePresent)) and (s.inFlightCount != 0 or not(s.inFlightAgePresent)))

  val nonEmptyBucketsHaveOldestAge =
    s.outcome != Accepted
    or ((s.queuedCount == 0 or s.queuedAgePresent) and (s.inFlightCount == 0 or s.inFlightAgePresent))

  val oldestAgesAreNonNegative =
    s.outcome != Accepted
    or ((not(s.queuedAgePresent) or s.queuedAgeNonNegative) and (not(s.inFlightAgePresent) or s.inFlightAgeNonNegative))
}

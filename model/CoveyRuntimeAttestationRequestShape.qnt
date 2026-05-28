module CoveyRuntimeAttestationRequestShape {
  type Case =
    ValidProcessOnly
    | ValidContainerOnly
    | ValidProcessAndContainer
    | MissingIdentity
    | BlankProcess
    | PaddedContainer
    | NegativeStartedAt
    | NegativeEndedAt
    | InvertedTimestamps
    | BlankProviderRunId
    | PaddedProviderRunIdIssuer
    | InvalidSessionToken
    | InvalidProviderToken
    | InvalidModelToken
    | InvalidTranscriptDigest
    | BlankIdempotencyKey

  type Outcome = NotEvaluated | Accepted | Rejected

  type RejectReason =
    NoReject
    | MissingRuntimeIdentity
    | InvalidProcessId
    | InvalidContainerId
    | InvalidStartedAt
    | InvalidEndedAt
    | InvertedTimeRange
    | InvalidProviderRunId
    | InvalidProviderRunIdIssuer
    | SessionTokenInvalid
    | ProviderInvalid
    | ModelInvalid
    | CommandTranscriptDigestInvalid
    | IdempotencyKeyInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    sessionTokenValid: bool,
    providerValid: bool,
    modelValid: bool,
    providerRunIdValid: bool,
    providerRunIdIssuerValid: bool,
    processIdPresent: bool,
    processIdValid: bool,
    containerIdPresent: bool,
    containerIdValid: bool,
    commandTranscriptDigestValid: bool,
    startedAtNonNegative: bool,
    endedAtNonNegative: bool,
    timestampsOrdered: bool,
    idempotencyKeyValid: bool,
    flatSerializationPreservesIdentity: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    sessionTokenValid: case != InvalidSessionToken,
    providerValid: case != InvalidProviderToken,
    modelValid: case != InvalidModelToken,
    providerRunIdValid: case != BlankProviderRunId,
    providerRunIdIssuerValid: case != PaddedProviderRunIdIssuer,
    processIdPresent:
      case.in(Set(ValidProcessOnly, ValidProcessAndContainer, BlankProcess, PaddedContainer, NegativeStartedAt, NegativeEndedAt, InvertedTimestamps, BlankProviderRunId, PaddedProviderRunIdIssuer, InvalidSessionToken, InvalidProviderToken, InvalidModelToken, InvalidTranscriptDigest, BlankIdempotencyKey)),
    processIdValid: case != BlankProcess,
    containerIdPresent:
      case.in(Set(ValidContainerOnly, ValidProcessAndContainer, PaddedContainer)),
    containerIdValid: case != PaddedContainer,
    commandTranscriptDigestValid: case != InvalidTranscriptDigest,
    startedAtNonNegative: case != NegativeStartedAt,
    endedAtNonNegative: case != NegativeEndedAt,
    timestampsOrdered: case != InvertedTimestamps,
    idempotencyKeyValid: case != BlankIdempotencyKey,
  }

  def identityPresent(facts) =
    facts.processIdPresent or facts.containerIdPresent

  def identitiesValid(facts) =
    (not(facts.processIdPresent) or facts.processIdValid)
    and (not(facts.containerIdPresent) or facts.containerIdValid)

  // Mirrors RecordRuntimeAttestationReq::try_from_parts validation order.
  def rejectFor(facts) =
    if (facts.processIdPresent and not(facts.processIdValid)) InvalidProcessId
    else if (facts.containerIdPresent and not(facts.containerIdValid)) InvalidContainerId
    else if (not(identityPresent(facts))) MissingRuntimeIdentity
    else if (not(facts.startedAtNonNegative)) InvalidStartedAt
    else if (not(facts.endedAtNonNegative)) InvalidEndedAt
    else if (not(facts.timestampsOrdered)) InvertedTimeRange
    else if (not(facts.providerRunIdValid)) InvalidProviderRunId
    else if (not(facts.providerRunIdIssuerValid)) InvalidProviderRunIdIssuer
    else if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.providerValid)) ProviderInvalid
    else if (not(facts.modelValid)) ModelInvalid
    else if (not(facts.commandTranscriptDigestValid)) CommandTranscriptDigestInvalid
    else if (not(facts.idempotencyKeyValid)) IdempotencyKeyInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidProcessOnly,
      sessionTokenValid: false,
      providerValid: false,
      modelValid: false,
      providerRunIdValid: false,
      providerRunIdIssuerValid: false,
      processIdPresent: false,
      processIdValid: false,
      containerIdPresent: false,
      containerIdValid: false,
      commandTranscriptDigestValid: false,
      startedAtNonNegative: false,
      endedAtNonNegative: false,
      timestampsOrdered: false,
      idempotencyKeyValid: false,
      flatSerializationPreservesIdentity: false,
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
      providerValid: facts.providerValid,
      modelValid: facts.modelValid,
      providerRunIdValid: facts.providerRunIdValid,
      providerRunIdIssuerValid: facts.providerRunIdIssuerValid,
      processIdPresent: facts.processIdPresent,
      processIdValid: facts.processIdValid,
      containerIdPresent: facts.containerIdPresent,
      containerIdValid: facts.containerIdValid,
      commandTranscriptDigestValid: facts.commandTranscriptDigestValid,
      startedAtNonNegative: facts.startedAtNonNegative,
      endedAtNonNegative: facts.endedAtNonNegative,
      timestampsOrdered: facts.timestampsOrdered,
      idempotencyKeyValid: facts.idempotencyKeyValid,
      flatSerializationPreservesIdentity: accepted and identityPresent(facts),
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidProcessOnly) },
    all { s.caseIndex == 1, evaluate(ValidContainerOnly) },
    all { s.caseIndex == 2, evaluate(ValidProcessAndContainer) },
    all { s.caseIndex == 3, evaluate(MissingIdentity) },
    all { s.caseIndex == 4, evaluate(BlankProcess) },
    all { s.caseIndex == 5, evaluate(PaddedContainer) },
    all { s.caseIndex == 6, evaluate(NegativeStartedAt) },
    all { s.caseIndex == 7, evaluate(NegativeEndedAt) },
    all { s.caseIndex == 8, evaluate(InvertedTimestamps) },
    all { s.caseIndex == 9, evaluate(BlankProviderRunId) },
    all { s.caseIndex == 10, evaluate(PaddedProviderRunIdIssuer) },
    all { s.caseIndex == 11, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 12, evaluate(InvalidProviderToken) },
    all { s.caseIndex == 13, evaluate(InvalidModelToken) },
    all { s.caseIndex == 14, evaluate(InvalidTranscriptDigest) },
    all { s.caseIndex == 15, evaluate(BlankIdempotencyKey) },
    all { s.caseIndex >= 16, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 16
    and s.case.in(Set(ValidProcessOnly, ValidContainerOnly, ValidProcessAndContainer, MissingIdentity, BlankProcess, PaddedContainer, NegativeStartedAt, NegativeEndedAt, InvertedTimestamps, BlankProviderRunId, PaddedProviderRunIdIssuer, InvalidSessionToken, InvalidProviderToken, InvalidModelToken, InvalidTranscriptDigest, BlankIdempotencyKey))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, MissingRuntimeIdentity, InvalidProcessId, InvalidContainerId, InvalidStartedAt, InvalidEndedAt, InvertedTimeRange, InvalidProviderRunId, InvalidProviderRunIdIssuer, SessionTokenInvalid, ProviderInvalid, ModelInvalid, CommandTranscriptDigestInvalid, IdempotencyKeyInvalid))

  val acceptedRequiresValidatedFields =
    not(s.accepted)
    or (
      s.sessionTokenValid
      and s.providerValid
      and s.modelValid
      and s.providerRunIdValid
      and s.providerRunIdIssuerValid
      and identitiesValid(s)
      and s.commandTranscriptDigestValid
      and s.idempotencyKeyValid
    )

  val acceptedRequiresRuntimeIdentity =
    not(s.accepted) or identityPresent(s)

  val acceptedRequiresOrderedNonNegativeTimestamps =
    not(s.accepted)
    or (s.startedAtNonNegative and s.endedAtNonNegative and s.timestampsOrdered)

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val serializationPreservesFlatIdentityForAccepted =
    not(s.accepted) or s.flatSerializationPreservesIdentity
}

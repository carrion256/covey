module CoveyRuntimeAttestationProofRowShape {
  type Case =
    ValidProcessObservedProviderRun
    | ValidContainerLegacyMissingProviderRun
    | ValidProcessAndContainerObservedProviderRun
    | MissingRuntimeIdentity
    | InvalidProcessId
    | InvalidContainerId
    | PartialProviderRunId
    | PartialProviderRunIssuer
    | InvalidProviderRunId
    | InvalidProviderRunIssuer
    | NegativeStartedAt
    | NegativeEndedAt
    | NegativeRecordedAt
    | EndedBeforeStarted
    | RecordedBeforeEnded
    | InvalidSessionToken
    | InvalidAgentPrincipal
    | InvalidAgentInstance
    | InvalidRole
    | InvalidProvider
    | InvalidModel
    | InvalidTranscript

  type RuntimeIdentityShape =
    ProcessOnly
    | ContainerOnly
    | ProcessAndContainer
    | MissingRuntimeIdentityShape
    | InvalidProcessIdShape
    | InvalidContainerIdShape

  type ProviderRunShape =
    ObservedProviderRun
    | LegacyMissingProviderRun
    | PartialProviderRunIdShape
    | PartialProviderRunIssuerShape
    | InvalidProviderRunIdShape
    | InvalidProviderRunIssuerShape

  type TimestampShape =
    MonotonicRuntimeSpan
    | NegativeStartedAtShape
    | NegativeEndedAtShape
    | NegativeRecordedAtShape
    | EndedBeforeStartedShape
    | RecordedBeforeEndedShape

  type Outcome = NotEvaluated | Accepted | Rejected

  type RejectReason =
    NoReject
    | InvalidProcessIdReject
    | InvalidContainerIdReject
    | MissingRuntimeIdentityReject
    | InvalidProviderRunIdReject
    | InvalidProviderRunIssuerReject
    | PartialProviderRunIdentityReject
    | InvalidStartedAtReject
    | InvalidEndedAtReject
    | InvalidRecordedAtReject
    | EndedBeforeStartedReject
    | RecordedBeforeEndedReject
    | SessionTokenInvalid
    | AgentPrincipalInvalid
    | AgentInstanceInvalid
    | InvalidRoleReject
    | ProviderInvalid
    | ModelInvalid
    | TranscriptInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    runtimeIdentityShape: RuntimeIdentityShape,
    providerRunShape: ProviderRunShape,
    timestampShape: TimestampShape,
    sessionTokenValid: bool,
    agentPrincipalValid: bool,
    agentInstanceValid: bool,
    roleValid: bool,
    providerValid: bool,
    modelValid: bool,
    transcriptValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    scheduledWork: bool,
    mutationAuthorityGranted: bool,
    evaluated: bool,
  }

  var s: Model

  def runtimeIdentityShapeFor(case) =
    if (case == ValidContainerLegacyMissingProviderRun) ContainerOnly
    else if (case == ValidProcessAndContainerObservedProviderRun) ProcessAndContainer
    else if (case == MissingRuntimeIdentity) MissingRuntimeIdentityShape
    else if (case == InvalidProcessId) InvalidProcessIdShape
    else if (case == InvalidContainerId) InvalidContainerIdShape
    else ProcessOnly

  def providerRunShapeFor(case) =
    if (case == ValidContainerLegacyMissingProviderRun) LegacyMissingProviderRun
    else if (case == PartialProviderRunId) PartialProviderRunIdShape
    else if (case == PartialProviderRunIssuer) PartialProviderRunIssuerShape
    else if (case == InvalidProviderRunId) InvalidProviderRunIdShape
    else if (case == InvalidProviderRunIssuer) InvalidProviderRunIssuerShape
    else ObservedProviderRun

  def timestampShapeFor(case) =
    if (case == NegativeStartedAt) NegativeStartedAtShape
    else if (case == NegativeEndedAt) NegativeEndedAtShape
    else if (case == NegativeRecordedAt) NegativeRecordedAtShape
    else if (case == EndedBeforeStarted) EndedBeforeStartedShape
    else if (case == RecordedBeforeEnded) RecordedBeforeEndedShape
    else MonotonicRuntimeSpan

  def factsFor(case) = {
    runtimeIdentityShape: runtimeIdentityShapeFor(case),
    providerRunShape: providerRunShapeFor(case),
    timestampShape: timestampShapeFor(case),
    sessionTokenValid: case != InvalidSessionToken,
    agentPrincipalValid: case != InvalidAgentPrincipal,
    agentInstanceValid: case != InvalidAgentInstance,
    roleValid: case != InvalidRole,
    providerValid: case != InvalidProvider,
    modelValid: case != InvalidModel,
    transcriptValid: case != InvalidTranscript,
  }

  // Mirrors RuntimeAttestationRow::from_db_parts validation order:
  // runtime identity, provider-run identity, timestamps, then typed row fields.
  def rejectFor(facts) =
    if (facts.runtimeIdentityShape == InvalidProcessIdShape) InvalidProcessIdReject
    else if (facts.runtimeIdentityShape == InvalidContainerIdShape) InvalidContainerIdReject
    else if (facts.runtimeIdentityShape == MissingRuntimeIdentityShape) MissingRuntimeIdentityReject
    else if (facts.providerRunShape == InvalidProviderRunIdShape) InvalidProviderRunIdReject
    else if (facts.providerRunShape == InvalidProviderRunIssuerShape) InvalidProviderRunIssuerReject
    else if (facts.providerRunShape.in(Set(PartialProviderRunIdShape, PartialProviderRunIssuerShape))) PartialProviderRunIdentityReject
    else if (facts.timestampShape == NegativeStartedAtShape) InvalidStartedAtReject
    else if (facts.timestampShape == NegativeEndedAtShape) InvalidEndedAtReject
    else if (facts.timestampShape == NegativeRecordedAtShape) InvalidRecordedAtReject
    else if (facts.timestampShape == EndedBeforeStartedShape) EndedBeforeStartedReject
    else if (facts.timestampShape == RecordedBeforeEndedShape) RecordedBeforeEndedReject
    else if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.agentPrincipalValid)) AgentPrincipalInvalid
    else if (not(facts.agentInstanceValid)) AgentInstanceInvalid
    else if (not(facts.roleValid)) InvalidRoleReject
    else if (not(facts.providerValid)) ProviderInvalid
    else if (not(facts.modelValid)) ModelInvalid
    else if (not(facts.transcriptValid)) TranscriptInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidProcessObservedProviderRun,
      runtimeIdentityShape: ProcessOnly,
      providerRunShape: ObservedProviderRun,
      timestampShape: MonotonicRuntimeSpan,
      sessionTokenValid: false,
      agentPrincipalValid: false,
      agentInstanceValid: false,
      roleValid: false,
      providerValid: false,
      modelValid: false,
      transcriptValid: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
      scheduledWork: false,
      mutationAuthorityGranted: false,
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
      runtimeIdentityShape: facts.runtimeIdentityShape,
      providerRunShape: facts.providerRunShape,
      timestampShape: facts.timestampShape,
      sessionTokenValid: facts.sessionTokenValid,
      agentPrincipalValid: facts.agentPrincipalValid,
      agentInstanceValid: facts.agentInstanceValid,
      roleValid: facts.roleValid,
      providerValid: facts.providerValid,
      modelValid: facts.modelValid,
      transcriptValid: facts.transcriptValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      scheduledWork: false,
      mutationAuthorityGranted: false,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidProcessObservedProviderRun) },
    all { s.caseIndex == 1, evaluate(ValidContainerLegacyMissingProviderRun) },
    all { s.caseIndex == 2, evaluate(ValidProcessAndContainerObservedProviderRun) },
    all { s.caseIndex == 3, evaluate(MissingRuntimeIdentity) },
    all { s.caseIndex == 4, evaluate(InvalidProcessId) },
    all { s.caseIndex == 5, evaluate(InvalidContainerId) },
    all { s.caseIndex == 6, evaluate(PartialProviderRunId) },
    all { s.caseIndex == 7, evaluate(PartialProviderRunIssuer) },
    all { s.caseIndex == 8, evaluate(InvalidProviderRunId) },
    all { s.caseIndex == 9, evaluate(InvalidProviderRunIssuer) },
    all { s.caseIndex == 10, evaluate(NegativeStartedAt) },
    all { s.caseIndex == 11, evaluate(NegativeEndedAt) },
    all { s.caseIndex == 12, evaluate(NegativeRecordedAt) },
    all { s.caseIndex == 13, evaluate(EndedBeforeStarted) },
    all { s.caseIndex == 14, evaluate(RecordedBeforeEnded) },
    all { s.caseIndex == 15, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 16, evaluate(InvalidAgentPrincipal) },
    all { s.caseIndex == 17, evaluate(InvalidAgentInstance) },
    all { s.caseIndex == 18, evaluate(InvalidRole) },
    all { s.caseIndex == 19, evaluate(InvalidProvider) },
    all { s.caseIndex == 20, evaluate(InvalidModel) },
    all { s.caseIndex == 21, evaluate(InvalidTranscript) },
    all { s.caseIndex >= 22, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 22
    and s.runtimeIdentityShape.in(Set(ProcessOnly, ContainerOnly, ProcessAndContainer, MissingRuntimeIdentityShape, InvalidProcessIdShape, InvalidContainerIdShape))
    and s.providerRunShape.in(Set(ObservedProviderRun, LegacyMissingProviderRun, PartialProviderRunIdShape, PartialProviderRunIssuerShape, InvalidProviderRunIdShape, InvalidProviderRunIssuerShape))
    and s.timestampShape.in(Set(MonotonicRuntimeSpan, NegativeStartedAtShape, NegativeEndedAtShape, NegativeRecordedAtShape, EndedBeforeStartedShape, RecordedBeforeEndedShape))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, InvalidProcessIdReject, InvalidContainerIdReject, MissingRuntimeIdentityReject, InvalidProviderRunIdReject, InvalidProviderRunIssuerReject, PartialProviderRunIdentityReject, InvalidStartedAtReject, InvalidEndedAtReject, InvalidRecordedAtReject, EndedBeforeStartedReject, RecordedBeforeEndedReject, SessionTokenInvalid, AgentPrincipalInvalid, AgentInstanceInvalid, InvalidRoleReject, ProviderInvalid, ModelInvalid, TranscriptInvalid))

  val acceptedHasRuntimeIdentity =
    not(s.accepted)
    or s.runtimeIdentityShape.in(Set(ProcessOnly, ContainerOnly, ProcessAndContainer))

  val acceptedHasProviderRunPairOrLegacyMissing =
    not(s.accepted)
    or s.providerRunShape.in(Set(ObservedProviderRun, LegacyMissingProviderRun))

  val acceptedTimestampsAreMonotonic =
    not(s.accepted) or s.timestampShape == MonotonicRuntimeSpan

  val acceptedTypedFieldsAreValid =
    not(s.accepted)
    or (
      s.sessionTokenValid
      and s.agentPrincipalValid
      and s.agentInstanceValid
      and s.roleValid
      and s.providerValid
      and s.modelValid
      and s.transcriptValid
    )

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      runtimeIdentityShape: s.runtimeIdentityShape,
      providerRunShape: s.providerRunShape,
      timestampShape: s.timestampShape,
      sessionTokenValid: s.sessionTokenValid,
      agentPrincipalValid: s.agentPrincipalValid,
      agentInstanceValid: s.agentInstanceValid,
      roleValid: s.roleValid,
      providerValid: s.providerValid,
      modelValid: s.modelValid,
      transcriptValid: s.transcriptValid,
    })

  val proofRowParsingDoesNotScheduleWork =
    not(s.scheduledWork)

  val proofRowParsingDoesNotGrantMutationAuthority =
    not(s.mutationAuthorityGranted)
}

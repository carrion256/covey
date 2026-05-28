module CoveyRuntimeAttestationRecordShape {
  type Case =
    ValidProcessObservedProviderRun
    | ValidContainerObservedProviderRun
    | ValidProcessAndContainerObservedProviderRun
    | ValidLegacyMissingProviderRun
    | MissingRuntimeIdentity
    | PaddedProcessId
    | PaddedContainerId
    | PartialLegacyMissingProviderRunId
    | PartialLegacyMissingProviderRunIssuer
    | PaddedProviderRunId
    | PaddedProviderRunIssuer
    | EndedBeforeStarted
    | RecordedBeforeEnded
    | InvalidAgentPrincipal
    | InvalidAgentInstance

  type ProviderRunShape = ObservedProviderRun | LegacyMissingProviderRun | PartialLegacyId | PartialLegacyIssuer | InvalidProviderRunId | InvalidProviderRunIssuer
  type RuntimeIdentityShape = ProcessOnly | ContainerOnly | ProcessAndContainer | MissingRuntimeIdentityShape | InvalidProcessId | InvalidContainerId
  type TimestampShape = MonotonicRuntimeSpan | EndedBeforeStartedShape | RecordedBeforeEndedShape
  type AgentShape = ValidAgent | InvalidPrincipal | InvalidInstance
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | InvalidProviderRunIdReject
    | InvalidProviderRunIssuerReject
    | PartialProviderRunIdentityReject
    | InvalidProcessIdReject
    | InvalidContainerIdReject
    | MissingRuntimeIdentityReject
    | EndedBeforeStartedReject
    | RecordedBeforeEndedReject
    | InvalidPrincipalReject
    | InvalidInstanceReject

  type Model = {
    caseIndex: int,
    case: Case,
    providerRunShape: ProviderRunShape,
    runtimeIdentityShape: RuntimeIdentityShape,
    timestampShape: TimestampShape,
    agentShape: AgentShape,
    processPresent: bool,
    containerPresent: bool,
    providerRunRefPresent: bool,
    legacyProviderRunMissing: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def providerRunShapeFor(case) =
    if (case == ValidLegacyMissingProviderRun) LegacyMissingProviderRun
    else if (case == PartialLegacyMissingProviderRunId) PartialLegacyId
    else if (case == PartialLegacyMissingProviderRunIssuer) PartialLegacyIssuer
    else if (case == PaddedProviderRunId) InvalidProviderRunId
    else if (case == PaddedProviderRunIssuer) InvalidProviderRunIssuer
    else ObservedProviderRun

  def runtimeIdentityShapeFor(case) =
    if (case == ValidContainerObservedProviderRun) ContainerOnly
    else if (case == ValidProcessAndContainerObservedProviderRun) ProcessAndContainer
    else if (case == MissingRuntimeIdentity) MissingRuntimeIdentityShape
    else if (case == PaddedProcessId) InvalidProcessId
    else if (case == PaddedContainerId) InvalidContainerId
    else ProcessOnly

  def timestampShapeFor(case) =
    if (case == EndedBeforeStarted) EndedBeforeStartedShape
    else if (case == RecordedBeforeEnded) RecordedBeforeEndedShape
    else MonotonicRuntimeSpan

  def agentShapeFor(case) =
    if (case == InvalidAgentPrincipal) InvalidPrincipal
    else if (case == InvalidAgentInstance) InvalidInstance
    else ValidAgent

  def processPresentFor(runtimeIdentityShape) =
    runtimeIdentityShape.in(Set(ProcessOnly, ProcessAndContainer, InvalidProcessId))

  def containerPresentFor(runtimeIdentityShape) =
    runtimeIdentityShape.in(Set(ContainerOnly, ProcessAndContainer, InvalidContainerId))

  def providerRunRefPresentFor(providerRunShape) =
    providerRunShape == ObservedProviderRun

  def legacyProviderRunMissingFor(providerRunShape) =
    providerRunShape == LegacyMissingProviderRun

  // Mirrors RuntimeAttestation::try_from_parts: provider-run identity is parsed
  // first, then runtime identity, then timestamps, then agent text fields.
  def rejectFor(providerRunShape, runtimeIdentityShape, timestampShape, agentShape) =
    if (providerRunShape == InvalidProviderRunId) InvalidProviderRunIdReject
    else if (providerRunShape == InvalidProviderRunIssuer) InvalidProviderRunIssuerReject
    else if (providerRunShape.in(Set(PartialLegacyId, PartialLegacyIssuer))) PartialProviderRunIdentityReject
    else if (runtimeIdentityShape == InvalidProcessId) InvalidProcessIdReject
    else if (runtimeIdentityShape == InvalidContainerId) InvalidContainerIdReject
    else if (runtimeIdentityShape == MissingRuntimeIdentityShape) MissingRuntimeIdentityReject
    else if (timestampShape == EndedBeforeStartedShape) EndedBeforeStartedReject
    else if (timestampShape == RecordedBeforeEndedShape) RecordedBeforeEndedReject
    else if (agentShape == InvalidPrincipal) InvalidPrincipalReject
    else if (agentShape == InvalidInstance) InvalidInstanceReject
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidProcessObservedProviderRun,
      providerRunShape: ObservedProviderRun,
      runtimeIdentityShape: ProcessOnly,
      timestampShape: MonotonicRuntimeSpan,
      agentShape: ValidAgent,
      processPresent: false,
      containerPresent: false,
      providerRunRefPresent: false,
      legacyProviderRunMissing: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val providerRunShape = providerRunShapeFor(case)
    val runtimeIdentityShape = runtimeIdentityShapeFor(case)
    val timestampShape = timestampShapeFor(case)
    val agentShape = agentShapeFor(case)
    val rejectReason = rejectFor(providerRunShape, runtimeIdentityShape, timestampShape, agentShape)
    val accepted = rejectReason == NoReject
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      providerRunShape: providerRunShape,
      runtimeIdentityShape: runtimeIdentityShape,
      timestampShape: timestampShape,
      agentShape: agentShape,
      processPresent: processPresentFor(runtimeIdentityShape),
      containerPresent: containerPresentFor(runtimeIdentityShape),
      providerRunRefPresent: providerRunRefPresentFor(providerRunShape),
      legacyProviderRunMissing: legacyProviderRunMissingFor(providerRunShape),
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidProcessObservedProviderRun) },
    all { s.caseIndex == 1, evaluate(ValidContainerObservedProviderRun) },
    all { s.caseIndex == 2, evaluate(ValidProcessAndContainerObservedProviderRun) },
    all { s.caseIndex == 3, evaluate(ValidLegacyMissingProviderRun) },
    all { s.caseIndex == 4, evaluate(MissingRuntimeIdentity) },
    all { s.caseIndex == 5, evaluate(PaddedProcessId) },
    all { s.caseIndex == 6, evaluate(PaddedContainerId) },
    all { s.caseIndex == 7, evaluate(PartialLegacyMissingProviderRunId) },
    all { s.caseIndex == 8, evaluate(PartialLegacyMissingProviderRunIssuer) },
    all { s.caseIndex == 9, evaluate(PaddedProviderRunId) },
    all { s.caseIndex == 10, evaluate(PaddedProviderRunIssuer) },
    all { s.caseIndex == 11, evaluate(EndedBeforeStarted) },
    all { s.caseIndex == 12, evaluate(RecordedBeforeEnded) },
    all { s.caseIndex == 13, evaluate(InvalidAgentPrincipal) },
    all { s.caseIndex == 14, evaluate(InvalidAgentInstance) },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.providerRunShape.in(Set(
      ObservedProviderRun,
      LegacyMissingProviderRun,
      PartialLegacyId,
      PartialLegacyIssuer,
      InvalidProviderRunId,
      InvalidProviderRunIssuer,
    ))
    and s.runtimeIdentityShape.in(Set(
      ProcessOnly,
      ContainerOnly,
      ProcessAndContainer,
      MissingRuntimeIdentityShape,
      InvalidProcessId,
      InvalidContainerId,
    ))
    and s.timestampShape.in(Set(MonotonicRuntimeSpan, EndedBeforeStartedShape, RecordedBeforeEndedShape))
    and s.agentShape.in(Set(ValidAgent, InvalidPrincipal, InvalidInstance))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(
      NoReject,
      InvalidProviderRunIdReject,
      InvalidProviderRunIssuerReject,
      PartialProviderRunIdentityReject,
      InvalidProcessIdReject,
      InvalidContainerIdReject,
      MissingRuntimeIdentityReject,
      EndedBeforeStartedReject,
      RecordedBeforeEndedReject,
      InvalidPrincipalReject,
      InvalidInstanceReject,
    ))

  val acceptedHasRuntimeIdentity =
    not(s.accepted) or s.processPresent or s.containerPresent

  val acceptedHasProviderRunPairOrLegacyMissing =
    not(s.accepted)
    or s.providerRunRefPresent
    or s.legacyProviderRunMissing

  val providerRunPairAndLegacyMissingAreExclusive =
    not(s.providerRunRefPresent and s.legacyProviderRunMissing)

  val acceptedTimestampsAreMonotonic =
    not(s.accepted) or s.timestampShape == MonotonicRuntimeSpan

  val acceptedAgentIdentityIsValid =
    not(s.accepted) or s.agentShape == ValidAgent

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated)
    or s.rejectReason == rejectFor(
      s.providerRunShape,
      s.runtimeIdentityShape,
      s.timestampShape,
      s.agentShape,
    )
}

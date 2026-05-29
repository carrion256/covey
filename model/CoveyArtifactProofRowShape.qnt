module CoveyArtifactProofRowShape {
  type Case =
    ValidArtifactRow
    | InvalidArtifactDigest
    | InvalidProducedBySubtask
    | InvalidProducedBySession
    | EmptyManifestPath
    | ControlManifestPath
    | InvalidChangedPathsDigest

  type ManifestPathShape = ValidManifestPath | EmptyPath | ControlPath
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | ArtifactDigestInvalid
    | ProducedBySubtaskInvalid
    | ProducedBySessionInvalid
    | ManifestPathInvalid
    | ChangedPathsDigestInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    artifactDigestValid: bool,
    producedBySubtaskValid: bool,
    producedBySessionValid: bool,
    manifestPathShape: ManifestPathShape,
    changedPathsDigestValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    scheduledWork: bool,
    mutationAuthorityGranted: bool,
    evaluated: bool,
  }

  var s: Model

  def manifestPathShapeFor(case) =
    if (case == EmptyManifestPath) EmptyPath
    else if (case == ControlManifestPath) ControlPath
    else ValidManifestPath

  def factsFor(case) = {
    artifactDigestValid: case != InvalidArtifactDigest,
    producedBySubtaskValid: case != InvalidProducedBySubtask,
    producedBySessionValid: case != InvalidProducedBySession,
    manifestPathShape: manifestPathShapeFor(case),
    changedPathsDigestValid: case != InvalidChangedPathsDigest,
  }

  def manifestPathValid(shape) =
    shape == ValidManifestPath

  // Mirrors ArtifactRow::from_db_parts validation order.
  def rejectFor(facts) =
    if (not(facts.artifactDigestValid)) ArtifactDigestInvalid
    else if (not(facts.producedBySubtaskValid)) ProducedBySubtaskInvalid
    else if (not(facts.producedBySessionValid)) ProducedBySessionInvalid
    else if (not(manifestPathValid(facts.manifestPathShape))) ManifestPathInvalid
    else if (not(facts.changedPathsDigestValid)) ChangedPathsDigestInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidArtifactRow,
      artifactDigestValid: false,
      producedBySubtaskValid: false,
      producedBySessionValid: false,
      manifestPathShape: EmptyPath,
      changedPathsDigestValid: false,
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
      artifactDigestValid: facts.artifactDigestValid,
      producedBySubtaskValid: facts.producedBySubtaskValid,
      producedBySessionValid: facts.producedBySessionValid,
      manifestPathShape: facts.manifestPathShape,
      changedPathsDigestValid: facts.changedPathsDigestValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      scheduledWork: false,
      mutationAuthorityGranted: false,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidArtifactRow) },
    all { s.caseIndex == 1, evaluate(InvalidArtifactDigest) },
    all { s.caseIndex == 2, evaluate(InvalidProducedBySubtask) },
    all { s.caseIndex == 3, evaluate(InvalidProducedBySession) },
    all { s.caseIndex == 4, evaluate(EmptyManifestPath) },
    all { s.caseIndex == 5, evaluate(ControlManifestPath) },
    all { s.caseIndex == 6, evaluate(InvalidChangedPathsDigest) },
    all { s.caseIndex >= 7, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 7
    and s.case.in(Set(ValidArtifactRow, InvalidArtifactDigest, InvalidProducedBySubtask, InvalidProducedBySession, EmptyManifestPath, ControlManifestPath, InvalidChangedPathsDigest))
    and s.manifestPathShape.in(Set(ValidManifestPath, EmptyPath, ControlPath))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, ArtifactDigestInvalid, ProducedBySubtaskInvalid, ProducedBySessionInvalid, ManifestPathInvalid, ChangedPathsDigestInvalid))

  val acceptedRequiresTypedArtifactAndProducer =
    not(s.accepted)
    or (
      s.artifactDigestValid
      and s.producedBySubtaskValid
      and s.producedBySessionValid
    )

  val acceptedRequiresValidManifestAndChangedPaths =
    not(s.accepted)
    or (manifestPathValid(s.manifestPathShape) and s.changedPathsDigestValid)

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      artifactDigestValid: s.artifactDigestValid,
      producedBySubtaskValid: s.producedBySubtaskValid,
      producedBySessionValid: s.producedBySessionValid,
      manifestPathShape: s.manifestPathShape,
      changedPathsDigestValid: s.changedPathsDigestValid,
    })

  val proofRowParsingDoesNotScheduleWork =
    not(s.scheduledWork)

  val proofRowParsingDoesNotGrantMutationAuthority =
    not(s.mutationAuthorityGranted)
}

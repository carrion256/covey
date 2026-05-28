module CoveyLandingAuthorizationStatusShape {
  type Case =
    ValidAccepted
    | MissingAcceptedFlag
    | RejectedFlag
    | InvalidQueueId
    | InvalidArtifactDigest
    | InvalidArtifactKind
    | MissingArtifactKind
    | InvalidBaseRev
    | InvalidManifestPath
    | InvalidChangedPathsDigest
    | InvalidReviewId
    | InvalidFindingsDigest
    | NonPositiveFence
    | InvalidVerifier
    | InvalidVerdictDigest
    | InvalidSealDigest
    | InvalidRecordedSession

  type FieldShape = ValidField | InvalidField | MissingField
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | AcceptedFlagMissing
    | AcceptedFlagFalse
    | QueueIdInvalid
    | ArtifactDigestInvalid
    | ArtifactKindInvalid
    | ArtifactKindMissing
    | BaseRevInvalid
    | ManifestPathInvalid
    | ChangedPathsDigestInvalid
    | ReviewIdInvalid
    | FindingsDigestInvalid
    | ClaimFenceInvalid
    | VerifierInvalid
    | VerdictDigestInvalid
    | SealDigestInvalid
    | RecordedSessionInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    acceptedFlagPresent: bool,
    acceptedFlagTrue: bool,
    queueIdShape: FieldShape,
    artifactDigestShape: FieldShape,
    artifactKindShape: FieldShape,
    baseRevShape: FieldShape,
    manifestPathShape: FieldShape,
    changedPathsDigestShape: FieldShape,
    reviewIdShape: FieldShape,
    findingsDigestShape: FieldShape,
    claimFencePositive: bool,
    verifierShape: FieldShape,
    verdictDigestShape: FieldShape,
    sealDigestShape: FieldShape,
    recordedSessionShape: FieldShape,
    flatFieldsPreserved: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    acceptedFlagPresent: case != MissingAcceptedFlag,
    acceptedFlagTrue: case != RejectedFlag,
    queueIdShape: if (case == InvalidQueueId) InvalidField else ValidField,
    artifactDigestShape: if (case == InvalidArtifactDigest) InvalidField else ValidField,
    artifactKindShape:
      if (case == MissingArtifactKind) MissingField
      else if (case == InvalidArtifactKind) InvalidField
      else ValidField,
    baseRevShape: if (case == InvalidBaseRev) InvalidField else ValidField,
    manifestPathShape: if (case == InvalidManifestPath) InvalidField else ValidField,
    changedPathsDigestShape: if (case == InvalidChangedPathsDigest) InvalidField else ValidField,
    reviewIdShape: if (case == InvalidReviewId) InvalidField else ValidField,
    findingsDigestShape: if (case == InvalidFindingsDigest) InvalidField else ValidField,
    claimFencePositive: case != NonPositiveFence,
    verifierShape: if (case == InvalidVerifier) InvalidField else ValidField,
    verdictDigestShape: if (case == InvalidVerdictDigest) InvalidField else ValidField,
    sealDigestShape: if (case == InvalidSealDigest) InvalidField else ValidField,
    recordedSessionShape: if (case == InvalidRecordedSession) InvalidField else ValidField,
  }

  def rejectFor(facts) =
    if (not(facts.acceptedFlagPresent)) AcceptedFlagMissing
    else if (not(facts.acceptedFlagTrue)) AcceptedFlagFalse
    else if (facts.queueIdShape == InvalidField) QueueIdInvalid
    else if (facts.artifactDigestShape == InvalidField) ArtifactDigestInvalid
    else if (facts.artifactKindShape == MissingField) ArtifactKindMissing
    else if (facts.artifactKindShape == InvalidField) ArtifactKindInvalid
    else if (facts.baseRevShape == InvalidField) BaseRevInvalid
    else if (facts.manifestPathShape == InvalidField) ManifestPathInvalid
    else if (facts.changedPathsDigestShape == InvalidField) ChangedPathsDigestInvalid
    else if (facts.reviewIdShape == InvalidField) ReviewIdInvalid
    else if (facts.findingsDigestShape == InvalidField) FindingsDigestInvalid
    else if (not(facts.claimFencePositive)) ClaimFenceInvalid
    else if (facts.verifierShape == InvalidField) VerifierInvalid
    else if (facts.verdictDigestShape == InvalidField) VerdictDigestInvalid
    else if (facts.sealDigestShape == InvalidField) SealDigestInvalid
    else if (facts.recordedSessionShape == InvalidField) RecordedSessionInvalid
    else NoReject

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidAccepted,
      acceptedFlagPresent: false,
      acceptedFlagTrue: false,
      queueIdShape: ValidField,
      artifactDigestShape: ValidField,
      artifactKindShape: ValidField,
      baseRevShape: ValidField,
      manifestPathShape: ValidField,
      changedPathsDigestShape: ValidField,
      reviewIdShape: ValidField,
      findingsDigestShape: ValidField,
      claimFencePositive: false,
      verifierShape: ValidField,
      verdictDigestShape: ValidField,
      sealDigestShape: ValidField,
      recordedSessionShape: ValidField,
      flatFieldsPreserved: false,
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
      acceptedFlagPresent: facts.acceptedFlagPresent,
      acceptedFlagTrue: facts.acceptedFlagTrue,
      queueIdShape: facts.queueIdShape,
      artifactDigestShape: facts.artifactDigestShape,
      artifactKindShape: facts.artifactKindShape,
      baseRevShape: facts.baseRevShape,
      manifestPathShape: facts.manifestPathShape,
      changedPathsDigestShape: facts.changedPathsDigestShape,
      reviewIdShape: facts.reviewIdShape,
      findingsDigestShape: facts.findingsDigestShape,
      claimFencePositive: facts.claimFencePositive,
      verifierShape: facts.verifierShape,
      verdictDigestShape: facts.verdictDigestShape,
      sealDigestShape: facts.sealDigestShape,
      recordedSessionShape: facts.recordedSessionShape,
      flatFieldsPreserved: accepted,
      outcome: if (accepted) Accepted else Rejected,
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidAccepted) },
    all { s.caseIndex == 1, evaluate(MissingAcceptedFlag) },
    all { s.caseIndex == 2, evaluate(RejectedFlag) },
    all { s.caseIndex == 3, evaluate(InvalidQueueId) },
    all { s.caseIndex == 4, evaluate(InvalidArtifactDigest) },
    all { s.caseIndex == 5, evaluate(InvalidArtifactKind) },
    all { s.caseIndex == 6, evaluate(MissingArtifactKind) },
    all { s.caseIndex == 7, evaluate(InvalidBaseRev) },
    all { s.caseIndex == 8, evaluate(InvalidManifestPath) },
    all { s.caseIndex == 9, evaluate(InvalidChangedPathsDigest) },
    all { s.caseIndex == 10, evaluate(InvalidReviewId) },
    all { s.caseIndex == 11, evaluate(InvalidFindingsDigest) },
    all { s.caseIndex == 12, evaluate(NonPositiveFence) },
    all { s.caseIndex == 13, evaluate(InvalidVerifier) },
    all { s.caseIndex == 14, evaluate(InvalidVerdictDigest) },
    all { s.caseIndex == 15, evaluate(InvalidSealDigest) },
    all { s.caseIndex == 16, evaluate(InvalidRecordedSession) },
    all { s.caseIndex >= 17, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 17
    and s.case.in(Set(ValidAccepted, MissingAcceptedFlag, RejectedFlag, InvalidQueueId, InvalidArtifactDigest, InvalidArtifactKind, MissingArtifactKind, InvalidBaseRev, InvalidManifestPath, InvalidChangedPathsDigest, InvalidReviewId, InvalidFindingsDigest, NonPositiveFence, InvalidVerifier, InvalidVerdictDigest, InvalidSealDigest, InvalidRecordedSession))
    and s.queueIdShape.in(Set(ValidField, InvalidField, MissingField))
    and s.artifactDigestShape.in(Set(ValidField, InvalidField, MissingField))
    and s.artifactKindShape.in(Set(ValidField, InvalidField, MissingField))
    and s.baseRevShape.in(Set(ValidField, InvalidField, MissingField))
    and s.manifestPathShape.in(Set(ValidField, InvalidField, MissingField))
    and s.changedPathsDigestShape.in(Set(ValidField, InvalidField, MissingField))
    and s.reviewIdShape.in(Set(ValidField, InvalidField, MissingField))
    and s.findingsDigestShape.in(Set(ValidField, InvalidField, MissingField))
    and s.verifierShape.in(Set(ValidField, InvalidField, MissingField))
    and s.verdictDigestShape.in(Set(ValidField, InvalidField, MissingField))
    and s.sealDigestShape.in(Set(ValidField, InvalidField, MissingField))
    and s.recordedSessionShape.in(Set(ValidField, InvalidField, MissingField))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, AcceptedFlagMissing, AcceptedFlagFalse, QueueIdInvalid, ArtifactDigestInvalid, ArtifactKindInvalid, ArtifactKindMissing, BaseRevInvalid, ManifestPathInvalid, ChangedPathsDigestInvalid, ReviewIdInvalid, FindingsDigestInvalid, ClaimFenceInvalid, VerifierInvalid, VerdictDigestInvalid, SealDigestInvalid, RecordedSessionInvalid))

  val acceptedRequiresAcceptedFlag =
    not(s.accepted) or (s.acceptedFlagPresent and s.acceptedFlagTrue)

  val acceptedRequiresAllEvidenceFields =
    not(s.accepted)
    or (
      s.queueIdShape == ValidField
      and s.artifactDigestShape == ValidField
      and s.artifactKindShape == ValidField
      and s.baseRevShape == ValidField
      and s.manifestPathShape == ValidField
      and s.changedPathsDigestShape == ValidField
      and s.reviewIdShape == ValidField
      and s.findingsDigestShape == ValidField
      and s.claimFencePositive
      and s.verifierShape == ValidField
      and s.verdictDigestShape == ValidField
      and s.sealDigestShape == ValidField
      and s.recordedSessionShape == ValidField
    )

  val acceptedPreservesFlatFields =
    not(s.accepted) or s.flatFieldsPreserved

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      acceptedFlagPresent: s.acceptedFlagPresent,
      acceptedFlagTrue: s.acceptedFlagTrue,
      queueIdShape: s.queueIdShape,
      artifactDigestShape: s.artifactDigestShape,
      artifactKindShape: s.artifactKindShape,
      baseRevShape: s.baseRevShape,
      manifestPathShape: s.manifestPathShape,
      changedPathsDigestShape: s.changedPathsDigestShape,
      reviewIdShape: s.reviewIdShape,
      findingsDigestShape: s.findingsDigestShape,
      claimFencePositive: s.claimFencePositive,
      verifierShape: s.verifierShape,
      verdictDigestShape: s.verdictDigestShape,
      sealDigestShape: s.sealDigestShape,
      recordedSessionShape: s.recordedSessionShape,
    })
}

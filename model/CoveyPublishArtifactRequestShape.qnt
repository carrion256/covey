module CoveyPublishArtifactRequestShape {
  type Case =
    ValidPatchBundle
    | ValidTreeBundle
    | InvalidSessionToken
    | InvalidClaimId
    | NonPositiveFence
    | InvalidArtifactDigest
    | InvalidBaseRev
    | EmptyManifestPath
    | ControlManifestPath
    | InvalidChangedPathsDigest
    | BlankIdempotencyKey

  type ArtifactKind = PatchBundle | TreeBundle
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | ClaimIdInvalid
    | FenceSeqInvalid
    | ArtifactDigestInvalid
    | BaseRevInvalid
    | ManifestPathInvalid
    | ChangedPathsDigestInvalid
    | IdempotencyKeyInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    artifactKind: ArtifactKind,
    sessionTokenValid: bool,
    claimIdValid: bool,
    fenceSeqPositive: bool,
    artifactDigestValid: bool,
    baseRevValid: bool,
    manifestPathPresent: bool,
    manifestPathNoControl: bool,
    changedPathsDigestValid: bool,
    idempotencyKeyValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def factsFor(case) = {
    artifactKind:
      if (case == ValidTreeBundle) TreeBundle else PatchBundle,
    sessionTokenValid: case != InvalidSessionToken,
    claimIdValid: case != InvalidClaimId,
    fenceSeqPositive: case != NonPositiveFence,
    artifactDigestValid: case != InvalidArtifactDigest,
    baseRevValid: case != InvalidBaseRev,
    manifestPathPresent: case != EmptyManifestPath,
    manifestPathNoControl: case != ControlManifestPath,
    changedPathsDigestValid: case != InvalidChangedPathsDigest,
    idempotencyKeyValid: case != BlankIdempotencyKey,
  }

  // Mirrors PublishArtifactReq::try_from_raw_parts validation order.
  def rejectFor(facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (not(facts.claimIdValid)) ClaimIdInvalid
    else if (not(facts.fenceSeqPositive)) FenceSeqInvalid
    else if (not(facts.artifactDigestValid)) ArtifactDigestInvalid
    else if (not(facts.baseRevValid)) BaseRevInvalid
    else if (not(facts.manifestPathPresent and facts.manifestPathNoControl)) ManifestPathInvalid
    else if (not(facts.changedPathsDigestValid)) ChangedPathsDigestInvalid
    else if (not(facts.idempotencyKeyValid)) IdempotencyKeyInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidPatchBundle,
      artifactKind: PatchBundle,
      sessionTokenValid: false,
      claimIdValid: false,
      fenceSeqPositive: false,
      artifactDigestValid: false,
      baseRevValid: false,
      manifestPathPresent: false,
      manifestPathNoControl: false,
      changedPathsDigestValid: false,
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
      artifactKind: facts.artifactKind,
      sessionTokenValid: facts.sessionTokenValid,
      claimIdValid: facts.claimIdValid,
      fenceSeqPositive: facts.fenceSeqPositive,
      artifactDigestValid: facts.artifactDigestValid,
      baseRevValid: facts.baseRevValid,
      manifestPathPresent: facts.manifestPathPresent,
      manifestPathNoControl: facts.manifestPathNoControl,
      changedPathsDigestValid: facts.changedPathsDigestValid,
      idempotencyKeyValid: facts.idempotencyKeyValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidPatchBundle) },
    all { s.caseIndex == 1, evaluate(ValidTreeBundle) },
    all { s.caseIndex == 2, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 3, evaluate(InvalidClaimId) },
    all { s.caseIndex == 4, evaluate(NonPositiveFence) },
    all { s.caseIndex == 5, evaluate(InvalidArtifactDigest) },
    all { s.caseIndex == 6, evaluate(InvalidBaseRev) },
    all { s.caseIndex == 7, evaluate(EmptyManifestPath) },
    all { s.caseIndex == 8, evaluate(ControlManifestPath) },
    all { s.caseIndex == 9, evaluate(InvalidChangedPathsDigest) },
    all { s.caseIndex == 10, evaluate(BlankIdempotencyKey) },
    all { s.caseIndex >= 11, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 11
    and s.case.in(Set(ValidPatchBundle, ValidTreeBundle, InvalidSessionToken, InvalidClaimId, NonPositiveFence, InvalidArtifactDigest, InvalidBaseRev, EmptyManifestPath, ControlManifestPath, InvalidChangedPathsDigest, BlankIdempotencyKey))
    and s.artifactKind.in(Set(PatchBundle, TreeBundle))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, ClaimIdInvalid, FenceSeqInvalid, ArtifactDigestInvalid, BaseRevInvalid, ManifestPathInvalid, ChangedPathsDigestInvalid, IdempotencyKeyInvalid))

  val acceptedRequiresClaimFenceAndArtifactIdentity =
    not(s.accepted)
    or (
      s.sessionTokenValid
      and s.claimIdValid
      and s.fenceSeqPositive
      and s.artifactDigestValid
      and s.baseRevValid
    )

  val acceptedRequiresManifestAndChangedPaths =
    not(s.accepted)
    or (
      s.manifestPathPresent
      and s.manifestPathNoControl
      and s.changedPathsDigestValid
    )

  val acceptedRequiresIdempotencyKey =
    not(s.accepted) or s.idempotencyKeyValid

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor({
      artifactKind: s.artifactKind,
      sessionTokenValid: s.sessionTokenValid,
      claimIdValid: s.claimIdValid,
      fenceSeqPositive: s.fenceSeqPositive,
      artifactDigestValid: s.artifactDigestValid,
      baseRevValid: s.baseRevValid,
      manifestPathPresent: s.manifestPathPresent,
      manifestPathNoControl: s.manifestPathNoControl,
      changedPathsDigestValid: s.changedPathsDigestValid,
      idempotencyKeyValid: s.idempotencyKeyValid,
    })
}

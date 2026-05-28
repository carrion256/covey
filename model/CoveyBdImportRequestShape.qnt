module CoveyBdImportRequestShape {
  type Case =
    ExistingMetaValid
    | NewMetaValid
    | BothSelectors
    | NeitherSelector
    | InvalidExistingMetaId
    | BlankNewPrompt
    | InvalidSessionToken
    | EmptyBeadsDbPath
    | WhitespaceBeadsDbPath
    | InvalidIdempotencyKey

  type DestinationShape = ExistingMeta | NewMeta | Both | Neither
  type PathShape = ValidPath | EmptyPath | WhitespacePath
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | DestinationInvalid
    | MetaTaskIdInvalid
    | PromptTextInvalid
    | SessionTokenInvalid
    | BeadsDbPathInvalid
    | IdempotencyKeyInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    destinationShape: DestinationShape,
    sessionTokenValid: bool,
    beadsDbPathShape: PathShape,
    metaTaskIdValid: bool,
    promptTextValid: bool,
    idempotencyKeyValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    existingMetaSelectorPreserved: bool,
    newPromptSelectorPreserved: bool,
    beadsDbPathPreserved: bool,
    coveyStateMutated: bool,
    schedulerAction: bool,
    evaluated: bool,
  }

  var s: Model

  def destinationFor(case) =
    if (case.in(Set(ExistingMetaValid, InvalidExistingMetaId, InvalidSessionToken, EmptyBeadsDbPath, WhitespaceBeadsDbPath, InvalidIdempotencyKey))) ExistingMeta
    else if (case.in(Set(NewMetaValid, BlankNewPrompt))) NewMeta
    else if (case == BothSelectors) Both
    else Neither

  def pathFor(case) =
    if (case == EmptyBeadsDbPath) EmptyPath
    else if (case == WhitespaceBeadsDbPath) WhitespacePath
    else ValidPath

  def rejectFor(case) =
    val destination = destinationFor(case)
    val path = pathFor(case)
    if (destination.in(Set(Both, Neither))) DestinationInvalid
    else if (case == InvalidExistingMetaId) MetaTaskIdInvalid
    else if (case == BlankNewPrompt) PromptTextInvalid
    else if (case == InvalidSessionToken) SessionTokenInvalid
    else if (path != ValidPath) BeadsDbPathInvalid
    else if (case == InvalidIdempotencyKey) IdempotencyKeyInvalid
    else NoReject

  action init = {
    s' = {
      caseIndex: 0,
      case: ExistingMetaValid,
      destinationShape: ExistingMeta,
      sessionTokenValid: true,
      beadsDbPathShape: ValidPath,
      metaTaskIdValid: true,
      promptTextValid: true,
      idempotencyKeyValid: true,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      existingMetaSelectorPreserved: false,
      newPromptSelectorPreserved: false,
      beadsDbPathPreserved: false,
      coveyStateMutated: false,
      schedulerAction: false,
      evaluated: false,
    }
  }

  action evaluate(case) = {
    val destination = destinationFor(case)
    val rejectReason = rejectFor(case)
    val accepted = rejectReason == NoReject
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      destinationShape: destination,
      sessionTokenValid: case != InvalidSessionToken,
      beadsDbPathShape: pathFor(case),
      metaTaskIdValid: case != InvalidExistingMetaId,
      promptTextValid: case != BlankNewPrompt,
      idempotencyKeyValid: case != InvalidIdempotencyKey,
      outcome: if (accepted) Accepted else Rejected,
      rejectReason: rejectReason,
      existingMetaSelectorPreserved: accepted and destination == ExistingMeta,
      newPromptSelectorPreserved: accepted and destination == NewMeta,
      beadsDbPathPreserved: accepted,
      coveyStateMutated: false,
      schedulerAction: false,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ExistingMetaValid) },
    all { s.caseIndex == 1, evaluate(NewMetaValid) },
    all { s.caseIndex == 2, evaluate(BothSelectors) },
    all { s.caseIndex == 3, evaluate(NeitherSelector) },
    all { s.caseIndex == 4, evaluate(InvalidExistingMetaId) },
    all { s.caseIndex == 5, evaluate(BlankNewPrompt) },
    all { s.caseIndex == 6, evaluate(InvalidSessionToken) },
    all { s.caseIndex == 7, evaluate(EmptyBeadsDbPath) },
    all { s.caseIndex == 8, evaluate(WhitespaceBeadsDbPath) },
    all { s.caseIndex == 9, evaluate(InvalidIdempotencyKey) },
    all { s.caseIndex >= 10, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 10

  val outcomeMatchesReject =
    not(s.evaluated)
    or s.outcome == (if (s.rejectReason == NoReject) Accepted else Rejected)

  val rejectReasonMatchesCase =
    not(s.evaluated) or s.rejectReason == rejectFor(s.case)

  val exactlyOneDestinationSelectorAccepted =
    not(s.evaluated and s.outcome == Accepted)
    or s.destinationShape.in(Set(ExistingMeta, NewMeta))

  val destinationRejectsBeforeBoundaryFields =
    not(s.destinationShape.in(Set(Both, Neither)))
    or s.rejectReason == DestinationInvalid

  val acceptedPreservesChosenDestination =
    not(s.evaluated and s.outcome == Accepted)
    or (
      (s.destinationShape == ExistingMeta and s.existingMetaSelectorPreserved and not(s.newPromptSelectorPreserved))
      or (s.destinationShape == NewMeta and s.newPromptSelectorPreserved and not(s.existingMetaSelectorPreserved))
    )

  val acceptedPreservesBeadsDbPath =
    not(s.evaluated and s.outcome == Accepted) or s.beadsDbPathPreserved

  val rejectedPreservesNoSelectors =
    not(s.evaluated and s.outcome == Rejected)
    or (not(s.existingMetaSelectorPreserved) and not(s.newPromptSelectorPreserved) and not(s.beadsDbPathPreserved))

  val requestShapeDoesNotMutateCoveyState =
    not(s.coveyStateMutated)

  val requestShapeDoesNotScheduleWork =
    not(s.schedulerAction)
}

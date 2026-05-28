module CoveyBdImportItemOutcomeShape {
  type Case =
    ImportedWithSubtask
    | DuplicateWithSubtask
    | InvalidRowWithoutSubtask
    | DuplicateMissingSubtask
    | InvalidRowWithSubtask
    | MissingSubtaskAndSkipReason

  type SkipReason = NoSkipReason | DeterministicDuplicate | InvalidRow
  type Outcome = NotEvaluated | Imported | SkippedDuplicate | SkippedInvalidRow | Rejected
  type RejectReason = NoReject | DuplicateMissingSubtaskReject | InvalidRowHasSubtaskReject | MissingSubtaskAndSkipReasonReject

  type Model = {
    caseIndex: int,
    case: Case,
    subtaskIdPresent: bool,
    skipReason: SkipReason,
    outcome: Outcome,
    rejectReason: RejectReason,
    evaluated: bool,
  }

  var s: Model

  def subtaskPresentFor(case) =
    case.in(Set(ImportedWithSubtask, DuplicateWithSubtask, InvalidRowWithSubtask))

  def skipReasonFor(case) =
    if (case.in(Set(DuplicateWithSubtask, DuplicateMissingSubtask))) DeterministicDuplicate
    else if (case.in(Set(InvalidRowWithoutSubtask, InvalidRowWithSubtask))) InvalidRow
    else NoSkipReason

  def rejectFor(subtaskIdPresent, skipReason) =
    if (skipReason == DeterministicDuplicate and not(subtaskIdPresent)) DuplicateMissingSubtaskReject
    else if (skipReason == InvalidRow and subtaskIdPresent) InvalidRowHasSubtaskReject
    else if (skipReason == NoSkipReason and not(subtaskIdPresent)) MissingSubtaskAndSkipReasonReject
    else NoReject

  def outcomeFor(subtaskIdPresent, skipReason, rejectReason) =
    if (rejectReason != NoReject) Rejected
    else if (skipReason == NoSkipReason and subtaskIdPresent) Imported
    else if (skipReason == DeterministicDuplicate and subtaskIdPresent) SkippedDuplicate
    else SkippedInvalidRow

  action init = {
    s' = {
      caseIndex: 0,
      case: ImportedWithSubtask,
      subtaskIdPresent: false,
      skipReason: NoSkipReason,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val subtaskIdPresent = subtaskPresentFor(case)
    val skipReason = skipReasonFor(case)
    val rejectReason = rejectFor(subtaskIdPresent, skipReason)
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      subtaskIdPresent: subtaskIdPresent,
      skipReason: skipReason,
      outcome: outcomeFor(subtaskIdPresent, skipReason, rejectReason),
      rejectReason: rejectReason,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ImportedWithSubtask) },
    all { s.caseIndex == 1, evaluate(DuplicateWithSubtask) },
    all { s.caseIndex == 2, evaluate(InvalidRowWithoutSubtask) },
    all { s.caseIndex == 3, evaluate(DuplicateMissingSubtask) },
    all { s.caseIndex == 4, evaluate(InvalidRowWithSubtask) },
    all { s.caseIndex == 5, evaluate(MissingSubtaskAndSkipReason) },
    all { s.caseIndex >= 6, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 6
    and s.case.in(Set(ImportedWithSubtask, DuplicateWithSubtask, InvalidRowWithoutSubtask, DuplicateMissingSubtask, InvalidRowWithSubtask, MissingSubtaskAndSkipReason))
    and s.skipReason.in(Set(NoSkipReason, DeterministicDuplicate, InvalidRow))
    and s.outcome.in(Set(NotEvaluated, Imported, SkippedDuplicate, SkippedInvalidRow, Rejected))
    and s.rejectReason.in(Set(NoReject, DuplicateMissingSubtaskReject, InvalidRowHasSubtaskReject, MissingSubtaskAndSkipReasonReject))

  val importedRequiresSubtaskAndNoSkipReason =
    s.outcome != Imported or (s.subtaskIdPresent and s.skipReason == NoSkipReason)

  val duplicateRequiresSubtaskAndDuplicateReason =
    s.outcome != SkippedDuplicate or (s.subtaskIdPresent and s.skipReason == DeterministicDuplicate)

  val invalidRowRequiresNoSubtaskAndInvalidReason =
    s.outcome != SkippedInvalidRow or (not(s.subtaskIdPresent) and s.skipReason == InvalidRow)

  val malformedShapesReject =
    s.outcome != Rejected or s.rejectReason != NoReject

  val acceptedIffNoReject =
    s.outcome.in(Set(Imported, SkippedDuplicate, SkippedInvalidRow))
    == (s.evaluated and s.rejectReason == NoReject)
}

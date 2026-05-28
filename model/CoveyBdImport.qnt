module CoveyBdImport {
  type Destination = NoDestination | BothDestinations | ExistingActiveMeta | ExistingTerminalMeta | NewMeta
  type Role = NoRole | Executor | Orchestrator
  type Source =
    MissingDb
    | BadSchema
    | EmptySource
    | OneImportable
    | MixedValidInvalid
    | DuplicateSameMeta
    | DuplicateDifferentMeta
    | OrderedByPriorityDependencyId

  type ConflictReason =
    NoConflict
    | InvalidDestination
    | MetaUnavailable
    | SourceUnavailable
    | SourceSchemaInvalid
    | RoleNotOrchestrator
    | DuplicateDifferentMetaConflict

  type Outcome = NotEvaluated | Applied | Rejected
  type SubtaskShape = NoSubtasks | AvailableWorkOnly | DuplicateSkipOnly | InvalidSkipOnly
  type ItemOrder = NoItems | SingleItem | ValidBeforeInvalid | DuplicateOnly | PriorityDependencyId

  type Model = {
    caseIndex: int,
    destination: Destination,
    role: Role,
    source: Source,
    conflictReason: ConflictReason,
    outcome: Outcome,
    metaWritten: bool,
    subtaskWritten: bool,
    eventWritten: bool,
    importedCount: int,
    skippedCount: int,
    subtaskShape: SubtaskShape,
    itemOrder: ItemOrder,
    duplicateSkipHasSubtask: bool,
    invalidSkipHasSubtask: bool,
    claimCreated: bool,
    sessionActiveSubtaskSet: bool,
    evaluated: bool,
  }

  var s: Model

  def validDestination(destination) =
    destination.in(Set(ExistingActiveMeta, NewMeta))

  def expectedConflict(destination, role, source) =
    if (destination.in(Set(NoDestination, BothDestinations))) InvalidDestination
    else if (destination == ExistingTerminalMeta) MetaUnavailable
    else if (source == MissingDb) SourceUnavailable
    else if (source == BadSchema) SourceSchemaInvalid
    else if (role != Orchestrator) RoleNotOrchestrator
    else if (source == DuplicateDifferentMeta) DuplicateDifferentMetaConflict
    else NoConflict

  def expectedImportedCount(source) =
    if (source == OneImportable) 1
    else if (source == MixedValidInvalid) 2
    else if (source == OrderedByPriorityDependencyId) 3
    else 0

  def expectedSkippedCount(source) =
    if (source == MixedValidInvalid) 3
    else if (source == DuplicateSameMeta) 1
    else 0

  def expectedSubtaskShape(source) =
    if (source.in(Set(OneImportable, MixedValidInvalid, OrderedByPriorityDependencyId))) AvailableWorkOnly
    else if (source == DuplicateSameMeta) DuplicateSkipOnly
    else if (source == BadSchema) InvalidSkipOnly
    else NoSubtasks

  def expectedItemOrder(source) =
    if (source == OneImportable) SingleItem
    else if (source == MixedValidInvalid) ValidBeforeInvalid
    else if (source == DuplicateSameMeta) DuplicateOnly
    else if (source == OrderedByPriorityDependencyId) PriorityDependencyId
    else NoItems

  def shouldApply(destination, role, source) =
    expectedConflict(destination, role, source) == NoConflict

  action init = {
    s' = {
      caseIndex: 0,
      destination: NoDestination,
      role: NoRole,
      source: MissingDb,
      conflictReason: NoConflict,
      outcome: NotEvaluated,
      metaWritten: false,
      subtaskWritten: false,
      eventWritten: false,
      importedCount: 0,
      skippedCount: 0,
      subtaskShape: NoSubtasks,
      itemOrder: NoItems,
      duplicateSkipHasSubtask: false,
      invalidSkipHasSubtask: false,
      claimCreated: false,
      sessionActiveSubtaskSet: false,
      evaluated: false,
    }
  }

  action evaluate(destination, role, source) = all {
    val conflict = expectedConflict(destination, role, source)
    val applied = conflict == NoConflict
    val imported = if (applied) expectedImportedCount(source) else 0
    val skipped = if (applied) expectedSkippedCount(source) else 0
    val shape = if (applied) expectedSubtaskShape(source) else NoSubtasks
    s' = {
      caseIndex: s.caseIndex + 1,
      destination: destination,
      role: role,
      source: source,
      conflictReason: conflict,
      outcome: if (applied) Applied else Rejected,
      metaWritten: applied and destination == NewMeta,
      subtaskWritten: imported > 0,
      eventWritten: applied and (destination == NewMeta or imported > 0),
      importedCount: imported,
      skippedCount: skipped,
      subtaskShape: shape,
      itemOrder: if (applied) expectedItemOrder(source) else NoItems,
      duplicateSkipHasSubtask: applied and source == DuplicateSameMeta,
      invalidSkipHasSubtask: false,
      claimCreated: false,
      sessionActiveSubtaskSet: false,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(NoDestination, Orchestrator, OneImportable) },
    all { s.caseIndex == 1, evaluate(BothDestinations, Orchestrator, OneImportable) },
    all { s.caseIndex == 2, evaluate(ExistingTerminalMeta, Orchestrator, OneImportable) },
    all { s.caseIndex == 3, evaluate(NewMeta, Executor, OneImportable) },
    all { s.caseIndex == 4, evaluate(NewMeta, Orchestrator, MissingDb) },
    all { s.caseIndex == 5, evaluate(NewMeta, Orchestrator, BadSchema) },
    all { s.caseIndex == 6, evaluate(NewMeta, Orchestrator, EmptySource) },
    all { s.caseIndex == 7, evaluate(NewMeta, Orchestrator, OneImportable) },
    all { s.caseIndex == 8, evaluate(NewMeta, Orchestrator, MixedValidInvalid) },
    all { s.caseIndex == 9, evaluate(NewMeta, Orchestrator, DuplicateDifferentMeta) },
    all { s.caseIndex == 10, evaluate(ExistingActiveMeta, Orchestrator, DuplicateSameMeta) },
    all { s.caseIndex == 11, evaluate(ExistingActiveMeta, Orchestrator, OrderedByPriorityDependencyId) },
    all { s.caseIndex >= 12, s' = s },
  }

  val typeOk = true

  val decisionMatchesInputs =
    not(s.evaluated)
    or s.conflictReason == expectedConflict(s.destination, s.role, s.source)

  val invalidDestinationRejectsWithoutWrites =
    not(s.evaluated and s.destination.in(Set(NoDestination, BothDestinations)))
    or (s.outcome == Rejected and not(s.metaWritten or s.subtaskWritten or s.eventWritten))

  val terminalDestinationRejectsWithoutWrites =
    not(s.evaluated and s.destination == ExistingTerminalMeta)
    or (s.conflictReason == MetaUnavailable and s.outcome == Rejected and not(s.metaWritten or s.subtaskWritten or s.eventWritten))

  val sourceFailureRejectsBeforeWrites =
    not(s.evaluated and s.source.in(Set(MissingDb, BadSchema)))
    or (s.outcome == Rejected and not(s.metaWritten or s.subtaskWritten or s.eventWritten))

  val roleRequiredBeforeMutation =
    not(s.evaluated and s.role != Orchestrator)
    or (s.outcome == Rejected and s.conflictReason == RoleNotOrchestrator and not(s.metaWritten or s.subtaskWritten or s.eventWritten))

  val duplicateDifferentMetaIsAtomicConflict =
    not(s.evaluated and s.source == DuplicateDifferentMeta)
    or (s.outcome == Rejected and s.conflictReason == DuplicateDifferentMetaConflict and not(s.metaWritten or s.subtaskWritten or s.eventWritten))

  val duplicateSameMetaIsDeterministicSkip =
    not(s.evaluated and s.source == DuplicateSameMeta)
    or (s.outcome == Applied and s.importedCount == 0 and s.skippedCount == 1 and s.duplicateSkipHasSubtask)

  val invalidRowsNeverCarrySubtaskIds =
    not(s.invalidSkipHasSubtask)

  val importedRowsBecomeAvailableWorkOnly =
    s.importedCount == 0
    or (s.subtaskWritten and s.subtaskShape == AvailableWorkOnly)

  val emptySourceCreatesOnlyDestinationMeta =
    not(s.evaluated and s.source == EmptySource)
    or (s.outcome == Applied and s.metaWritten == (s.destination == NewMeta) and not(s.subtaskWritten) and s.importedCount == 0 and s.skippedCount == 0)

  val orderedImportUsesPriorityDependencyId =
    not(s.evaluated and s.source == OrderedByPriorityDependencyId)
    or (s.outcome == Applied and s.itemOrder == PriorityDependencyId and s.importedCount == 3)

  val importNeverCreatesClaims =
    not(s.claimCreated) and not(s.sessionActiveSubtaskSet)

  val countsMatchSourceEligibility =
    not(s.evaluated and s.outcome == Applied)
    or (s.importedCount == expectedImportedCount(s.source) and s.skippedCount == expectedSkippedCount(s.source))

  val rejectedImportHasNoItems =
    s.outcome != Rejected
    or (s.importedCount == 0 and s.skippedCount == 0 and s.itemOrder == NoItems)
}

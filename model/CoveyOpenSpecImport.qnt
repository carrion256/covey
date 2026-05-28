module CoveyOpenSpecImport {
  type Store = Empty | ImportedV1 | ImportedV2 | ActiveClaimV1 | DifferentMetaTask | TerminalSameTitle
  type Source = SourceV1 | SourceV2
  type Mode = NoMode | DryRun | Write
  type Role = NoRole | Executor | Orchestrator
  type Diff = NoDiff | CreateDiff | UnchangedDiff | UpdateDiff | ConflictDiff
  type ConflictReason = NoConflict | ActiveClaimChangedSource | ExistingSubtaskDifferentMetaTask
  type ApplyResult = NotAttempted | Applied | Rejected

  type Model = {
    store: Store,
    source: Source,
    mode: Mode,
    role: Role,
    diff: Diff,
    conflictReason: ConflictReason,
    applyResult: ApplyResult,
    metaWritten: bool,
    subtaskWritten: bool,
    provenanceWritten: bool,
    importEventWritten: bool,
    dependenciesWritten: bool,
    claimLive: bool,
    claimCreatedByImport: bool,
    evaluated: bool,
  }

  var s: Model

  def sourceVersion(source) =
    if (source == SourceV1) 1 else 2

  def storeVersion(store) =
    if (store.in(Set(ImportedV1, ActiveClaimV1, TerminalSameTitle))) 1
    else if (store == ImportedV2) 2
    else 0

  def storeHasImportedSubtask(store) =
    store.in(Set(ImportedV1, ImportedV2, ActiveClaimV1, TerminalSameTitle))

  def expectedDiff(state) =
    if (state.store == Empty) CreateDiff
    else if (state.store == DifferentMetaTask) ConflictDiff
    else if (state.store == ActiveClaimV1 and state.source == SourceV2) ConflictDiff
    else if (storeVersion(state.store) == sourceVersion(state.source)) UnchangedDiff
    else UpdateDiff

  def expectedConflict(state) =
    if (state.store == DifferentMetaTask) ExistingSubtaskDifferentMetaTask
    else if (state.store == ActiveClaimV1 and state.source == SourceV2) ActiveClaimChangedSource
    else NoConflict

  def shouldApply(state) =
    state.mode == Write
    and state.role == Orchestrator
    and expectedConflict(state) == NoConflict
    and expectedDiff(state).in(Set(CreateDiff, UpdateDiff))

  action init = {
    s' = {
      store: Empty,
      source: SourceV1,
      mode: NoMode,
      role: NoRole,
      diff: NoDiff,
      conflictReason: NoConflict,
      applyResult: NotAttempted,
      metaWritten: false,
      subtaskWritten: false,
      provenanceWritten: false,
      importEventWritten: false,
      dependenciesWritten: false,
      claimLive: false,
      claimCreatedByImport: false,
      evaluated: false,
    }
  }

  action chooseInput(store, source, mode, role) = all {
    not(s.evaluated),
    s' = s
      .with("store", store)
      .with("source", source)
      .with("mode", mode)
      .with("role", role)
      .with("claimLive", store == ActiveClaimV1),
  }

  action evaluate = all {
    not(s.evaluated),
    val evaluatedState = s.with("evaluated", true)
    val diff = expectedDiff(evaluatedState)
    val conflict = expectedConflict(evaluatedState)
    val applied = shouldApply(evaluatedState)
    s' = evaluatedState
      .with("diff", diff)
      .with("conflictReason", conflict)
      .with("applyResult", if (applied) Applied else if (evaluatedState.mode == Write and evaluatedState.role == Orchestrator and conflict != NoConflict) Rejected else NotAttempted)
      .with("metaWritten", applied)
      .with("subtaskWritten", applied and diff.in(Set(CreateDiff, UpdateDiff)))
      .with("provenanceWritten", applied)
      .with("importEventWritten", applied)
      .with("dependenciesWritten", applied)
      .with("claimCreatedByImport", false),
  }

  action step = any {
    chooseInput(Empty, SourceV1, DryRun, NoRole),
    chooseInput(Empty, SourceV1, Write, Executor),
    chooseInput(Empty, SourceV1, Write, Orchestrator),
    chooseInput(ImportedV1, SourceV1, DryRun, NoRole),
    chooseInput(ImportedV1, SourceV1, Write, Orchestrator),
    chooseInput(ImportedV1, SourceV2, Write, Orchestrator),
    chooseInput(ImportedV2, SourceV2, Write, Orchestrator),
    chooseInput(ActiveClaimV1, SourceV2, DryRun, NoRole),
    chooseInput(ActiveClaimV1, SourceV2, Write, Orchestrator),
    chooseInput(DifferentMetaTask, SourceV1, Write, Orchestrator),
    chooseInput(TerminalSameTitle, SourceV1, Write, Orchestrator),
    evaluate,
  }

  val typeOk = true

  val dryRunNeverWrites =
    not(s.evaluated and s.mode == DryRun)
    or not(s.metaWritten or s.subtaskWritten or s.provenanceWritten or s.importEventWritten or s.dependenciesWritten)

  val writeApplyRequiresOrchestrator =
    s.applyResult != Applied or (s.mode == Write and s.role == Orchestrator)

  val conflictPreventsPartialApply =
    s.conflictReason == NoConflict
    or s.mode != Write
    or s.role != Orchestrator
    or (s.applyResult == Rejected and not(s.metaWritten or s.subtaskWritten or s.provenanceWritten or s.importEventWritten or s.dependenciesWritten))

  val activeClaimChangedSourceConflicts =
    not(s.evaluated and s.store == ActiveClaimV1 and s.source == SourceV2)
    or (s.diff == ConflictDiff and s.conflictReason == ActiveClaimChangedSource and s.claimLive)

  val differentMetaTaskConflicts =
    not(s.evaluated and s.store == DifferentMetaTask)
    or (s.diff == ConflictDiff and s.conflictReason == ExistingSubtaskDifferentMetaTask)

  val provenanceOnlyWithAppliedDiff =
    not(s.provenanceWritten or s.importEventWritten)
    or (s.applyResult == Applied and s.diff.in(Set(CreateDiff, UpdateDiff)))

  val dependenciesOnlyAfterAppliedDiff =
    not(s.dependenciesWritten)
    or (s.applyResult == Applied and s.conflictReason == NoConflict)

  val unchangedImportDoesNotWrite =
    not(s.evaluated and s.diff == UnchangedDiff)
    or not(s.metaWritten or s.subtaskWritten or s.provenanceWritten or s.importEventWritten or s.dependenciesWritten)

  val createOnlyWhenNoImportedSubtask =
    s.diff != CreateDiff or not(storeHasImportedSubtask(s.store))

  val updateOnlyWithoutLiveClaim =
    s.diff != UpdateDiff or not(s.claimLive)

  val importNeverCreatesClaims =
    not(s.claimCreatedByImport)

  val dryRunReportsSameDiffAsWrite =
    not(s.evaluated) or s.diff == expectedDiff(s)
}

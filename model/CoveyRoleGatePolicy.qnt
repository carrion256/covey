module CoveyRoleGatePolicy {
  type Case =
    ClaimWorkExecutor
    | ClaimWorkReviewer
    | ClaimReviewReviewer
    | ClaimReviewExecutor
    | RequestReviewExecutor
    | RequestReviewOrchestrator
    | RequestReviewReviewer
    | DecideReviewReviewer
    | DecideReviewExecutor
    | EnqueueOrchestrator
    | EnqueueApplyGate
    | EnqueueExecutor
    | ApplyQueueApplyGate
    | ApplyQueueOrchestrator
    | VerifyLandingOrchestrator
    | VerifyLandingApplyGate
    | RecordReceiptOrchestrator
    | RecordReceiptApplyGate
    | SupersedeOrchestrator
    | SupersedeApplyGate
    | ReservationOrchestrator
    | ReservationExecutor
    | ImportOrchestrator
    | ImportApplyGate

  type Operation =
    ClaimWork
    | ClaimReview
    | RequestReview
    | DecideReview
    | EnqueueForApply
    | ApplyQueueMutation
    | VerifyLandingAuthorization
    | RecordLandingReceipt
    | SupersedeQueueItem
    | ReservationMutation
    | ImportPlanning

  type Role = Executor | Reviewer | Orchestrator | ApplyGate
  type Decision = NotEvaluated | Allow | DenyWrongRole

  type Model = {
    caseIndex: int,
    case: Case,
    operation: Operation,
    role: Role,
    executorAllowed: bool,
    reviewerAllowed: bool,
    orchestratorAllowed: bool,
    applyGateAllowed: bool,
    decision: Decision,
    claimCreated: bool,
    queueMutated: bool,
    reservationMutated: bool,
    importWritten: bool,
    evaluated: bool,
  }

  var s: Model

  def roleFor(case) =
    if (case.in(Set(
      ClaimWorkExecutor,
      ClaimReviewExecutor,
      RequestReviewExecutor,
      DecideReviewExecutor,
      EnqueueExecutor,
      ReservationExecutor
    ))) Executor
    else if (case.in(Set(ClaimWorkReviewer, ClaimReviewReviewer, RequestReviewReviewer, DecideReviewReviewer))) Reviewer
    else if (case.in(Set(
      RequestReviewOrchestrator,
      EnqueueOrchestrator,
      ApplyQueueOrchestrator,
      VerifyLandingOrchestrator,
      RecordReceiptOrchestrator,
      SupersedeOrchestrator,
      ReservationOrchestrator,
      ImportOrchestrator
    ))) Orchestrator
    else ApplyGate

  def operationFor(case) =
    if (case.in(Set(ClaimWorkExecutor, ClaimWorkReviewer))) ClaimWork
    else if (case.in(Set(ClaimReviewReviewer, ClaimReviewExecutor))) ClaimReview
    else if (case.in(Set(RequestReviewExecutor, RequestReviewOrchestrator, RequestReviewReviewer))) RequestReview
    else if (case.in(Set(DecideReviewReviewer, DecideReviewExecutor))) DecideReview
    else if (case.in(Set(EnqueueOrchestrator, EnqueueApplyGate, EnqueueExecutor))) EnqueueForApply
    else if (case.in(Set(ApplyQueueApplyGate, ApplyQueueOrchestrator))) ApplyQueueMutation
    else if (case.in(Set(VerifyLandingOrchestrator, VerifyLandingApplyGate))) VerifyLandingAuthorization
    else if (case.in(Set(RecordReceiptOrchestrator, RecordReceiptApplyGate))) RecordLandingReceipt
    else if (case.in(Set(SupersedeOrchestrator, SupersedeApplyGate))) SupersedeQueueItem
    else if (case.in(Set(ReservationOrchestrator, ReservationExecutor))) ReservationMutation
    else ImportPlanning

  def roleAllowed(operation, role) =
    if (operation == ClaimWork) role == Executor
    else if (operation == ClaimReview) role == Reviewer
    else if (operation == RequestReview) role.in(Set(Executor, Orchestrator))
    else if (operation == DecideReview) role == Reviewer
    else if (operation == EnqueueForApply) role.in(Set(Orchestrator, ApplyGate))
    else if (operation == ApplyQueueMutation) role == ApplyGate
    else if (operation == VerifyLandingAuthorization) role.in(Set(Orchestrator, ApplyGate))
    else if (operation == RecordLandingReceipt) role.in(Set(Orchestrator, ApplyGate))
    else if (operation == SupersedeQueueItem) role.in(Set(Orchestrator, ApplyGate))
    else if (operation == ReservationMutation) role == Orchestrator
    else role == Orchestrator

  def expectedRoles(operation) = {
    executorAllowed: roleAllowed(operation, Executor),
    reviewerAllowed: roleAllowed(operation, Reviewer),
    orchestratorAllowed: roleAllowed(operation, Orchestrator),
    applyGateAllowed: roleAllowed(operation, ApplyGate),
  }

  def decisionFor(operation, role) =
    if (roleAllowed(operation, role)) Allow else DenyWrongRole

  action init = {
    s' = {
      caseIndex: 0,
      case: ClaimWorkExecutor,
      operation: ClaimWork,
      role: Executor,
      executorAllowed: false,
      reviewerAllowed: false,
      orchestratorAllowed: false,
      applyGateAllowed: false,
      decision: NotEvaluated,
      claimCreated: false,
      queueMutated: false,
      reservationMutated: false,
      importWritten: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val operation = operationFor(case)
    val role = roleFor(case)
    val roles = expectedRoles(operation)
    val allowed = roleAllowed(operation, role)
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      operation: operation,
      role: role,
      executorAllowed: roles.executorAllowed,
      reviewerAllowed: roles.reviewerAllowed,
      orchestratorAllowed: roles.orchestratorAllowed,
      applyGateAllowed: roles.applyGateAllowed,
      decision: decisionFor(operation, role),
      claimCreated: allowed and operation.in(Set(ClaimWork, ClaimReview)),
      queueMutated: allowed and operation.in(Set(EnqueueForApply, ApplyQueueMutation, SupersedeQueueItem)),
      reservationMutated: allowed and operation == ReservationMutation,
      importWritten: allowed and operation == ImportPlanning,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ClaimWorkExecutor) },
    all { s.caseIndex == 1, evaluate(ClaimWorkReviewer) },
    all { s.caseIndex == 2, evaluate(ClaimReviewReviewer) },
    all { s.caseIndex == 3, evaluate(ClaimReviewExecutor) },
    all { s.caseIndex == 4, evaluate(RequestReviewExecutor) },
    all { s.caseIndex == 5, evaluate(RequestReviewOrchestrator) },
    all { s.caseIndex == 6, evaluate(RequestReviewReviewer) },
    all { s.caseIndex == 7, evaluate(DecideReviewReviewer) },
    all { s.caseIndex == 8, evaluate(DecideReviewExecutor) },
    all { s.caseIndex == 9, evaluate(EnqueueOrchestrator) },
    all { s.caseIndex == 10, evaluate(EnqueueApplyGate) },
    all { s.caseIndex == 11, evaluate(EnqueueExecutor) },
    all { s.caseIndex == 12, evaluate(ApplyQueueApplyGate) },
    all { s.caseIndex == 13, evaluate(ApplyQueueOrchestrator) },
    all { s.caseIndex == 14, evaluate(VerifyLandingOrchestrator) },
    all { s.caseIndex == 15, evaluate(VerifyLandingApplyGate) },
    all { s.caseIndex == 16, evaluate(RecordReceiptOrchestrator) },
    all { s.caseIndex == 17, evaluate(RecordReceiptApplyGate) },
    all { s.caseIndex == 18, evaluate(SupersedeOrchestrator) },
    all { s.caseIndex == 19, evaluate(SupersedeApplyGate) },
    all { s.caseIndex == 20, evaluate(ReservationOrchestrator) },
    all { s.caseIndex == 21, evaluate(ReservationExecutor) },
    all { s.caseIndex == 22, evaluate(ImportOrchestrator) },
    all { s.caseIndex == 23, evaluate(ImportApplyGate) },
    all { s.caseIndex >= 24, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 24
    and s.case.in(Set(ClaimWorkExecutor, ClaimWorkReviewer, ClaimReviewReviewer, ClaimReviewExecutor, RequestReviewExecutor, RequestReviewOrchestrator, RequestReviewReviewer, DecideReviewReviewer, DecideReviewExecutor, EnqueueOrchestrator, EnqueueApplyGate, EnqueueExecutor, ApplyQueueApplyGate, ApplyQueueOrchestrator, VerifyLandingOrchestrator, VerifyLandingApplyGate, RecordReceiptOrchestrator, RecordReceiptApplyGate, SupersedeOrchestrator, SupersedeApplyGate, ReservationOrchestrator, ReservationExecutor, ImportOrchestrator, ImportApplyGate))
    and s.operation.in(Set(ClaimWork, ClaimReview, RequestReview, DecideReview, EnqueueForApply, ApplyQueueMutation, VerifyLandingAuthorization, RecordLandingReceipt, SupersedeQueueItem, ReservationMutation, ImportPlanning))
    and s.role.in(Set(Executor, Reviewer, Orchestrator, ApplyGate))
    and s.decision.in(Set(NotEvaluated, Allow, DenyWrongRole))

  val decisionMatchesRolePolicy =
    not(s.evaluated) or s.decision == decisionFor(s.operation, s.role)

  val allowedRoleSetMatchesOperation =
    not(s.evaluated)
    or (
      s.executorAllowed == roleAllowed(s.operation, Executor)
      and s.reviewerAllowed == roleAllowed(s.operation, Reviewer)
      and s.orchestratorAllowed == roleAllowed(s.operation, Orchestrator)
      and s.applyGateAllowed == roleAllowed(s.operation, ApplyGate)
    )

  val executorOnlyClaimsWork =
    not(s.evaluated and s.operation == ClaimWork) or (s.decision == Allow == (s.role == Executor))

  val reviewerOnlyClaimsAndDecidesReviews =
    not(s.evaluated and s.operation.in(Set(ClaimReview, DecideReview))) or (s.decision == Allow == (s.role == Reviewer))

  val requestReviewAllowsExecutorOrOrchestratorOnly =
    not(s.evaluated and s.operation == RequestReview) or (s.decision == Allow == s.role.in(Set(Executor, Orchestrator)))

  val enqueueAllowsOrchestratorOrApplyGateOnly =
    not(s.evaluated and s.operation == EnqueueForApply) or (s.decision == Allow == s.role.in(Set(Orchestrator, ApplyGate)))

  val applyQueueMutationAllowsApplyGateOnly =
    not(s.evaluated and s.operation == ApplyQueueMutation) or (s.decision == Allow == (s.role == ApplyGate))

  val landingProofReadsAndReceiptsAllowOrchestratorOrApplyGateOnly =
    not(s.evaluated and s.operation.in(Set(VerifyLandingAuthorization, RecordLandingReceipt, SupersedeQueueItem)))
    or (s.decision == Allow == s.role.in(Set(Orchestrator, ApplyGate)))

  val reservationAndImportAllowOrchestratorOnly =
    not(s.evaluated and s.operation.in(Set(ReservationMutation, ImportPlanning)))
    or (s.decision == Allow == (s.role == Orchestrator))

  val deniedRoleHasNoSideEffects =
    not(s.evaluated and s.decision == DenyWrongRole)
    or (
      not(s.claimCreated)
      and not(s.queueMutated)
      and not(s.reservationMutated)
      and not(s.importWritten)
    )
}

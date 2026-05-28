module CoveyMetaTaskRequestShape {
  type Operation = SubmitMetaTask | CancelMetaTask
  type Case =
    ValidSubmit
    | ValidCancel
    | SubmitInvalidSessionToken
    | SubmitBlankPrompt
    | SubmitBlankIdempotency
    | CancelInvalidSessionToken
    | CancelInvalidMetaTaskId
    | CancelBlankIdempotency

  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason =
    NoReject
    | SessionTokenInvalid
    | PromptTextInvalid
    | MetaTaskIdInvalid
    | IdempotencyKeyInvalid

  type Model = {
    caseIndex: int,
    case: Case,
    operation: Operation,
    sessionTokenValid: bool,
    promptTextValid: bool,
    metaTaskIdValid: bool,
    idempotencyKeyValid: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    accepted: bool,
    evaluated: bool,
  }

  var s: Model

  def operationFor(case) =
    if (case.in(Set(ValidSubmit, SubmitInvalidSessionToken, SubmitBlankPrompt, SubmitBlankIdempotency))) SubmitMetaTask
    else CancelMetaTask

  def factsFor(case) = {
    sessionTokenValid: not(case.in(Set(SubmitInvalidSessionToken, CancelInvalidSessionToken))),
    promptTextValid: case != SubmitBlankPrompt,
    metaTaskIdValid: case != CancelInvalidMetaTaskId,
    idempotencyKeyValid: not(case.in(Set(SubmitBlankIdempotency, CancelBlankIdempotency))),
  }

  // Mirrors SubmitMetaTaskReq and CancelMetaTaskReq validation order.
  def rejectFor(operation, facts) =
    if (not(facts.sessionTokenValid)) SessionTokenInvalid
    else if (operation == SubmitMetaTask and not(facts.promptTextValid)) PromptTextInvalid
    else if (operation == CancelMetaTask and not(facts.metaTaskIdValid)) MetaTaskIdInvalid
    else if (not(facts.idempotencyKeyValid)) IdempotencyKeyInvalid
    else NoReject

  def outcomeFor(rejectReason) =
    if (rejectReason == NoReject) Accepted else Rejected

  action init = {
    s' = {
      caseIndex: 0,
      case: ValidSubmit,
      operation: SubmitMetaTask,
      sessionTokenValid: false,
      promptTextValid: false,
      metaTaskIdValid: false,
      idempotencyKeyValid: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      accepted: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val operation = operationFor(case)
    val facts = factsFor(case)
    val rejectReason = rejectFor(operation, facts)
    val accepted = rejectReason == NoReject
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      operation: operation,
      sessionTokenValid: facts.sessionTokenValid,
      promptTextValid: facts.promptTextValid,
      metaTaskIdValid: facts.metaTaskIdValid,
      idempotencyKeyValid: facts.idempotencyKeyValid,
      outcome: outcomeFor(rejectReason),
      rejectReason: rejectReason,
      accepted: accepted,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ValidSubmit) },
    all { s.caseIndex == 1, evaluate(ValidCancel) },
    all { s.caseIndex == 2, evaluate(SubmitInvalidSessionToken) },
    all { s.caseIndex == 3, evaluate(SubmitBlankPrompt) },
    all { s.caseIndex == 4, evaluate(SubmitBlankIdempotency) },
    all { s.caseIndex == 5, evaluate(CancelInvalidSessionToken) },
    all { s.caseIndex == 6, evaluate(CancelInvalidMetaTaskId) },
    all { s.caseIndex == 7, evaluate(CancelBlankIdempotency) },
    all { s.caseIndex >= 8, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 8
    and s.case.in(Set(ValidSubmit, ValidCancel, SubmitInvalidSessionToken, SubmitBlankPrompt, SubmitBlankIdempotency, CancelInvalidSessionToken, CancelInvalidMetaTaskId, CancelBlankIdempotency))
    and s.operation.in(Set(SubmitMetaTask, CancelMetaTask))
    and s.outcome.in(Set(NotEvaluated, Accepted, Rejected))
    and s.rejectReason.in(Set(NoReject, SessionTokenInvalid, PromptTextInvalid, MetaTaskIdInvalid, IdempotencyKeyInvalid))

  val acceptedRequiresSessionAndIdempotency =
    not(s.accepted) or (s.sessionTokenValid and s.idempotencyKeyValid)

  val submitAcceptedRequiresPrompt =
    not(s.accepted and s.operation == SubmitMetaTask) or s.promptTextValid

  val cancelAcceptedRequiresMetaTask =
    not(s.accepted and s.operation == CancelMetaTask) or s.metaTaskIdValid

  val rejectedIffRejectReasonPresent =
    s.outcome == Rejected == (s.evaluated and s.rejectReason != NoReject)

  val acceptedIffNoReject =
    s.outcome == Accepted == (s.evaluated and s.rejectReason == NoReject and s.accepted)

  val validationOrderMatchesFirstFailedGate =
    not(s.evaluated) or s.rejectReason == rejectFor(
      s.operation,
      {
        sessionTokenValid: s.sessionTokenValid,
        promptTextValid: s.promptTextValid,
        metaTaskIdValid: s.metaTaskIdValid,
        idempotencyKeyValid: s.idempotencyKeyValid,
      }
    )
}

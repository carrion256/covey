module CoveyMetaTaskStatusViewShape {
  type Case =
    NoSubtasksValid
    | OneMatchingSubtask
    | TwoMatchingSubtasks
    | OneForeignSubtask
    | FirstMatchingSecondForeign
    | FirstForeignSecondMatching

  type SubtaskSet = Empty | OneMatching | TwoMatching | OneForeign | MatchingThenForeign | ForeignThenMatching
  type Outcome = NotEvaluated | Accepted | Rejected
  type RejectReason = NoReject | ForeignSubtaskReject

  type Model = {
    caseIndex: int,
    case: Case,
    subtaskSet: SubtaskSet,
    subtaskCount: int,
    allSubtasksBelong: bool,
    acceptedMetaTaskPreserved: bool,
    acceptedSubtasksPreserved: bool,
    scheduledWork: bool,
    mutationAuthorityGranted: bool,
    outcome: Outcome,
    rejectReason: RejectReason,
    evaluated: bool,
  }

  var s: Model

  def subtaskSetFor(case) =
    if (case == NoSubtasksValid) Empty
    else if (case == OneMatchingSubtask) OneMatching
    else if (case == TwoMatchingSubtasks) TwoMatching
    else if (case == OneForeignSubtask) OneForeign
    else if (case == FirstMatchingSecondForeign) MatchingThenForeign
    else ForeignThenMatching

  def subtaskCountFor(subtaskSet) =
    if (subtaskSet == Empty) 0
    else if (subtaskSet.in(Set(OneMatching, OneForeign))) 1
    else 2

  def allSubtasksBelongFor(subtaskSet) =
    subtaskSet.in(Set(Empty, OneMatching, TwoMatching))

  def rejectFor(subtaskSet) =
    if (allSubtasksBelongFor(subtaskSet)) NoReject else ForeignSubtaskReject

  action init = {
    s' = {
      caseIndex: 0,
      case: NoSubtasksValid,
      subtaskSet: Empty,
      subtaskCount: 0,
      allSubtasksBelong: true,
      acceptedMetaTaskPreserved: false,
      acceptedSubtasksPreserved: false,
      scheduledWork: false,
      mutationAuthorityGranted: false,
      outcome: NotEvaluated,
      rejectReason: NoReject,
      evaluated: false,
    }
  }

  action evaluate(case) = {
    val subtaskSet = subtaskSetFor(case)
    val rejectReason = rejectFor(subtaskSet)
    val accepted = rejectReason == NoReject
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      subtaskSet: subtaskSet,
      subtaskCount: subtaskCountFor(subtaskSet),
      allSubtasksBelong: allSubtasksBelongFor(subtaskSet),
      acceptedMetaTaskPreserved: accepted,
      acceptedSubtasksPreserved: accepted,
      scheduledWork: false,
      mutationAuthorityGranted: false,
      outcome: if (accepted) Accepted else Rejected,
      rejectReason: rejectReason,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(NoSubtasksValid) },
    all { s.caseIndex == 1, evaluate(OneMatchingSubtask) },
    all { s.caseIndex == 2, evaluate(TwoMatchingSubtasks) },
    all { s.caseIndex == 3, evaluate(OneForeignSubtask) },
    all { s.caseIndex == 4, evaluate(FirstMatchingSecondForeign) },
    all { s.caseIndex == 5, evaluate(FirstForeignSecondMatching) },
    all { s.caseIndex >= 6, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 6
    and s.subtaskCount >= 0
    and s.subtaskCount <= 2
    and s.subtaskSet.in(Set(Empty, OneMatching, TwoMatching, OneForeign, MatchingThenForeign, ForeignThenMatching))

  val outcomeMatchesReject =
    not(s.evaluated)
    or s.outcome == (if (s.rejectReason == NoReject) Accepted else Rejected)

  val rejectReasonMatchesSubtaskOwnership =
    not(s.evaluated)
    or s.rejectReason == rejectFor(s.subtaskSet)

  val acceptedRequiresAllSubtasksBelong =
    s.outcome != Accepted or s.allSubtasksBelong

  val rejectedIffForeignSubtask =
    s.outcome == Rejected == (s.evaluated and not(s.allSubtasksBelong))

  val acceptedPreservesMetaAndSubtasks =
    s.outcome != Accepted
    or (s.acceptedMetaTaskPreserved and s.acceptedSubtasksPreserved)

  val viewParsingDoesNotScheduleWork =
    not(s.scheduledWork)

  val viewParsingDoesNotGrantMutationAuthority =
    not(s.mutationAuthorityGranted)
}

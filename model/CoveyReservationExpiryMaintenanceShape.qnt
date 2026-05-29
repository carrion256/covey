module CoveyReservationExpiryMaintenanceShape {
  type Case =
    ActiveExpiredNoConflict
    | ActiveExpiredOpenConflict
    | ActiveExpiredAcknowledgedConflict
    | ActiveExpiredResolvedConflict
    | ActiveLiveNoExpire
    | ReleasedExpiredLease
    | AlreadyExpiredTerminal
    | MissingReservation

  type ReservationState = NoReservation | Active | Released | Expired
  type ConflictState = NoConflict | Open | Acknowledged | Resolved

  type Model = {
    caseIndex: int,
    case: Case,
    beforeReservation: ReservationState,
    afterReservation: ReservationState,
    leaseExpired: bool,
    beforeConflict: ConflictState,
    afterConflict: ConflictState,
    conflictBoundToReservation: bool,
    selectedForExpiry: bool,
    expiredCount: int,
    eventEmitted: bool,
    executorClaimCreated: bool,
    mutationAuthorityGranted: bool,
    evaluated: bool,
  }

  var s: Model

  def reservationFor(case) =
    if (case == ReleasedExpiredLease) Released
    else if (case == AlreadyExpiredTerminal) Expired
    else if (case == MissingReservation) NoReservation
    else Active

  def leaseExpiredFor(case) =
    not(case.in(Set(ActiveLiveNoExpire, MissingReservation)))

  def conflictFor(case) =
    if (case == ActiveExpiredOpenConflict) Open
    else if (case == ActiveExpiredAcknowledgedConflict) Acknowledged
    else if (case == ActiveExpiredResolvedConflict) Resolved
    else NoConflict

  def boundFor(conflict) =
    conflict != NoConflict

  def selectedFor(reservation, leaseExpired) =
    reservation == Active and leaseExpired

  def reservationAfter(reservation, selected) =
    if (selected) Expired else reservation

  def conflictAfter(conflict, selected, bound) =
    if (selected and bound and conflict.in(Set(Open, Acknowledged))) Resolved else conflict

  action init = {
    s' = {
      caseIndex: 0,
      case: ActiveExpiredNoConflict,
      beforeReservation: NoReservation,
      afterReservation: NoReservation,
      leaseExpired: false,
      beforeConflict: NoConflict,
      afterConflict: NoConflict,
      conflictBoundToReservation: false,
      selectedForExpiry: false,
      expiredCount: 0,
      eventEmitted: false,
      executorClaimCreated: false,
      mutationAuthorityGranted: false,
      evaluated: false,
    }
  }

  action evaluate(case) = all {
    val reservation = reservationFor(case)
    val leaseExpired = leaseExpiredFor(case)
    val conflict = conflictFor(case)
    val bound = boundFor(conflict)
    val selected = selectedFor(reservation, leaseExpired)
    s' = {
      caseIndex: s.caseIndex + 1,
      case: case,
      beforeReservation: reservation,
      afterReservation: reservationAfter(reservation, selected),
      leaseExpired: leaseExpired,
      beforeConflict: conflict,
      afterConflict: conflictAfter(conflict, selected, bound),
      conflictBoundToReservation: bound,
      selectedForExpiry: selected,
      expiredCount: if (selected) 1 else 0,
      eventEmitted: selected,
      executorClaimCreated: false,
      mutationAuthorityGranted: false,
      evaluated: true,
    }
  }

  action step = any {
    all { s.caseIndex == 0, evaluate(ActiveExpiredNoConflict) },
    all { s.caseIndex == 1, evaluate(ActiveExpiredOpenConflict) },
    all { s.caseIndex == 2, evaluate(ActiveExpiredAcknowledgedConflict) },
    all { s.caseIndex == 3, evaluate(ActiveExpiredResolvedConflict) },
    all { s.caseIndex == 4, evaluate(ActiveLiveNoExpire) },
    all { s.caseIndex == 5, evaluate(ReleasedExpiredLease) },
    all { s.caseIndex == 6, evaluate(AlreadyExpiredTerminal) },
    all { s.caseIndex == 7, evaluate(MissingReservation) },
    all { s.caseIndex >= 8, s' = s },
  }

  val typeOk =
    s.caseIndex >= 0
    and s.caseIndex <= 8
    and s.case.in(Set(ActiveExpiredNoConflict, ActiveExpiredOpenConflict, ActiveExpiredAcknowledgedConflict, ActiveExpiredResolvedConflict, ActiveLiveNoExpire, ReleasedExpiredLease, AlreadyExpiredTerminal, MissingReservation))
    and s.beforeReservation.in(Set(NoReservation, Active, Released, Expired))
    and s.afterReservation.in(Set(NoReservation, Active, Released, Expired))
    and s.beforeConflict.in(Set(NoConflict, Open, Acknowledged, Resolved))
    and s.afterConflict.in(Set(NoConflict, Open, Acknowledged, Resolved))
    and s.expiredCount >= 0
    and s.expiredCount <= 1

  val selectedIffActiveExpired =
    not(s.evaluated) or s.selectedForExpiry == selectedFor(s.beforeReservation, s.leaseExpired)

  val expiredCountMatchesSelection =
    not(s.evaluated) or s.expiredCount == (if (s.selectedForExpiry) 1 else 0)

  val selectedActiveExpiredBecomesExpired =
    not(s.evaluated and s.selectedForExpiry) or s.afterReservation == Expired

  val nonSelectedReservationUnchanged =
    not(s.evaluated and not(s.selectedForExpiry)) or s.afterReservation == s.beforeReservation

  val unresolvedBoundConflictResolvesOnExpiry =
    not(s.evaluated and s.selectedForExpiry and s.conflictBoundToReservation and s.beforeConflict.in(Set(Open, Acknowledged)))
    or s.afterConflict == Resolved

  val resolvedConflictStaysResolved =
    not(s.evaluated and s.beforeConflict == Resolved) or s.afterConflict == Resolved

  val noSyntheticConflictCreated =
    not(s.evaluated and s.beforeConflict == NoConflict) or s.afterConflict == NoConflict

  val eventEmittedIffExpired =
    not(s.evaluated) or s.eventEmitted == (s.expiredCount > 0)

  val maintenanceDoesNotClaimOrGrantMutationAuthority =
    not(s.executorClaimCreated) and not(s.mutationAuthorityGranted)
}

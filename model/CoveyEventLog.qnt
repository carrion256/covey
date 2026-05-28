module CoveyEventLog {
  type EventType = NoEvent | SessionHeartbeat | ClaimsExpired
  type ObjectType = NoObject | Session | Claim
  type Actor = NoActor | SessionActor | SystemActor
  type Token = NoToken | ValidSessionToken | InvalidSessionToken | SystemSentinel
  type Payload = NoPayload | HeartbeatPayload | ExpiredCountPayload | MalformedPayload
  type Outcome = NoAttempt | Committed | Rejected | RolledBack | SerializeFailed | CapacityFull

  type EventRecord = {
    present: bool,
    seq: int,
    eventType: EventType,
    objectType: ObjectType,
    actor: Actor,
    visibleSessionToken: bool,
    payload: Payload,
    readable: bool,
  }

  type Model = {
    e0: EventRecord,
    e1: EventRecord,
    nextSeq: int,
    lastOutcome: Outcome,
    lastEventType: EventType,
    lastObjectType: ObjectType,
    lastActor: Actor,
    lastToken: Token,
    lastPayload: Payload,
    lastMutationOk: bool,
    lastPayloadMatches: bool,
    lastObjectMatches: bool,
    lastActorValid: bool,
    lastReadable: bool,
    lastSeqAssigned: int,
    lastCountDelta: int,
  }

  var s: Model

  def emptyEvent = {
    present: false,
    seq: 0,
    eventType: NoEvent,
    objectType: NoObject,
    actor: NoActor,
    visibleSessionToken: false,
    payload: NoPayload,
    readable: true,
  }

  def expectedEventType(payload) =
    if (payload == HeartbeatPayload) SessionHeartbeat
    else if (payload == ExpiredCountPayload) ClaimsExpired
    else NoEvent

  def expectedObjectType(payload) =
    if (payload == HeartbeatPayload) Session
    else if (payload == ExpiredCountPayload) Claim
    else NoObject

  def payloadMatches(eventType, payload) =
    payload != MalformedPayload
    and payload != NoPayload
    and expectedEventType(payload) == eventType

  def objectMatches(objectType, payload) =
    payload != MalformedPayload
    and payload != NoPayload
    and expectedObjectType(payload) == objectType

  def actorValid(actor, token) =
    if (actor == SessionActor) token == ValidSessionToken
    else if (actor == SystemActor) token == SystemSentinel
    else false

  def visibleSessionToken(actor, token) =
    actor == SessionActor and token == ValidSessionToken

  def readable(eventType, objectType, actor, token, payload) =
    payloadMatches(eventType, payload)
    and objectMatches(objectType, payload)
    and actorValid(actor, token)

  def eventCount(state) =
    (if (state.e0.present) 1 else 0) + (if (state.e1.present) 1 else 0)

  def putRecord(seq, eventType, objectType, actor, token, payload) = {
    present: true,
    seq: seq,
    eventType: eventType,
    objectType: objectType,
    actor: actor,
    visibleSessionToken: visibleSessionToken(actor, token),
    payload: payload,
    readable: readable(eventType, objectType, actor, token, payload),
  }

  action init = {
    s' = {
      e0: emptyEvent,
      e1: emptyEvent,
      nextSeq: 1,
      lastOutcome: NoAttempt,
      lastEventType: NoEvent,
      lastObjectType: NoObject,
      lastActor: NoActor,
      lastToken: NoToken,
      lastPayload: NoPayload,
      lastMutationOk: false,
      lastPayloadMatches: false,
      lastObjectMatches: false,
      lastActorValid: false,
      lastReadable: false,
      lastSeqAssigned: 0,
      lastCountDelta: 0,
    }
  }

  action appendAttempt(eventType, objectType, actor, token, payload, serializeOk, mutationOk) = all {
    val payloadOk = payloadMatches(eventType, payload)
    val objectOk = objectMatches(objectType, payload)
    val actorOk = actorValid(actor, token)
    val readableOk = readable(eventType, objectType, actor, token, payload)
    val base = s
      .with("lastEventType", eventType)
      .with("lastObjectType", objectType)
      .with("lastActor", actor)
      .with("lastToken", token)
      .with("lastPayload", payload)
      .with("lastMutationOk", mutationOk)
      .with("lastPayloadMatches", payloadOk)
      .with("lastObjectMatches", objectOk)
      .with("lastActorValid", actorOk)
      .with("lastReadable", readableOk)
      .with("lastSeqAssigned", 0)
      .with("lastCountDelta", 0)

    s' =
      if (not(serializeOk)) {
        base.with("lastOutcome", SerializeFailed)
      } else if (not(readableOk)) {
        base.with("lastOutcome", Rejected)
      } else if (not(mutationOk)) {
        base.with("lastOutcome", RolledBack)
      } else if (s.e0.present and s.e1.present) {
        base.with("lastOutcome", CapacityFull)
      } else if (not(s.e0.present)) {
        base
          .with("e0", putRecord(s.nextSeq, eventType, objectType, actor, token, payload))
          .with("nextSeq", s.nextSeq + 1)
          .with("lastOutcome", Committed)
          .with("lastSeqAssigned", s.nextSeq)
          .with("lastCountDelta", 1)
      } else {
        base
          .with("e1", putRecord(s.nextSeq, eventType, objectType, actor, token, payload))
          .with("nextSeq", s.nextSeq + 1)
          .with("lastOutcome", Committed)
          .with("lastSeqAssigned", s.nextSeq)
          .with("lastCountDelta", 1)
      },
  }

  action step = any {
    appendAttempt(SessionHeartbeat, Session, SessionActor, ValidSessionToken, HeartbeatPayload, true, true),
    appendAttempt(SessionHeartbeat, Claim, SessionActor, ValidSessionToken, HeartbeatPayload, true, true),
    appendAttempt(SessionHeartbeat, Session, SessionActor, NoToken, HeartbeatPayload, true, true),
    appendAttempt(SessionHeartbeat, Session, SessionActor, ValidSessionToken, ExpiredCountPayload, true, true),
    appendAttempt(SessionHeartbeat, Session, SessionActor, ValidSessionToken, MalformedPayload, true, true),
    appendAttempt(SessionHeartbeat, Session, SessionActor, ValidSessionToken, HeartbeatPayload, false, true),
    appendAttempt(SessionHeartbeat, Session, SessionActor, ValidSessionToken, HeartbeatPayload, true, false),
    appendAttempt(ClaimsExpired, Claim, SystemActor, SystemSentinel, ExpiredCountPayload, true, true),
    appendAttempt(ClaimsExpired, Claim, SystemActor, ValidSessionToken, ExpiredCountPayload, true, true),
  }

  action scenarioStep = any {
    all {
      s.lastOutcome == NoAttempt,
      appendAttempt(SessionHeartbeat, Session, SessionActor, ValidSessionToken, ExpiredCountPayload, true, true),
    },
    all {
      s.lastOutcome == Rejected,
      appendAttempt(SessionHeartbeat, Claim, SessionActor, ValidSessionToken, HeartbeatPayload, true, true),
    },
    all {
      s.lastOutcome == Rejected,
      appendAttempt(SessionHeartbeat, Session, SessionActor, NoToken, HeartbeatPayload, true, true),
    },
    all {
      s.lastOutcome == Rejected,
      appendAttempt(SessionHeartbeat, Session, SessionActor, ValidSessionToken, HeartbeatPayload, false, true),
    },
    all {
      s.lastOutcome == SerializeFailed,
      appendAttempt(SessionHeartbeat, Session, SessionActor, ValidSessionToken, HeartbeatPayload, true, false),
    },
    all {
      s.lastOutcome == RolledBack,
      appendAttempt(SessionHeartbeat, Session, SessionActor, ValidSessionToken, HeartbeatPayload, true, true),
    },
    all {
      s.lastOutcome == Committed,
      eventCount(s) == 1,
      appendAttempt(ClaimsExpired, Claim, SystemActor, SystemSentinel, ExpiredCountPayload, true, true),
    },
    all {
      s.lastOutcome == Committed,
      eventCount(s) == 2,
      appendAttempt(ClaimsExpired, Claim, SystemActor, SystemSentinel, ExpiredCountPayload, true, true),
    },
  }

  val typeOk = true

  val committedRowsAreReadable =
    (not(s.e0.present) or s.e0.readable)
    and (not(s.e1.present) or s.e1.readable)

  val committedRowsHaveValidActorShape =
    (not(s.e0.present) or ((s.e0.actor == SessionActor) == s.e0.visibleSessionToken))
    and (not(s.e1.present) or ((s.e1.actor == SessionActor) == s.e1.visibleSessionToken))

  val appendOnlySequence =
    s.nextSeq == eventCount(s) + 1
    and (not(s.e0.present) or s.e0.seq == 1)
    and (not(s.e1.present) or (s.e0.present and s.e1.seq == 2))

  val rejectedAttemptsDoNotAppend =
    not(s.lastOutcome.in(Set(Rejected, SerializeFailed, RolledBack, CapacityFull)))
    or (s.lastCountDelta == 0 and s.lastSeqAssigned == 0)

  val committedAttemptAppendsExactlyOne =
    s.lastOutcome != Committed
    or (s.lastReadable and s.lastMutationOk and s.lastCountDelta == 1 and s.lastSeqAssigned > 0)

  val validationRejectsUnreadableRows =
    s.lastReadable
    or s.lastOutcome.in(Set(NoAttempt, Rejected, SerializeFailed))

  val rollbackDoesNotConsumeSequence =
    s.lastOutcome != RolledBack
    or s.lastSeqAssigned == 0

  val systemEventsHaveNoVisibleSessionToken =
    (not(s.e0.present) or s.e0.actor != SystemActor or not(s.e0.visibleSessionToken))
    and (not(s.e1.present) or s.e1.actor != SystemActor or not(s.e1.visibleSessionToken))
}

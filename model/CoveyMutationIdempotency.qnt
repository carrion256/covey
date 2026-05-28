module CoveyMutationIdempotency {
  type Actor = NoActor | ActorA | ActorB
  type Operation = NoOperation | RegisterSession | CreateSubtask
  type Key = NoKey | Key1 | Key2 | BlankKey
  type RequestHash = NoRequest | ReqA | ReqB
  type ResponseValue = NoResponse | RespA | RespB
  type Outcome =
    NoAttempt | Created | Replayed | Conflict | InvalidKey | ClosureFailed | SerializeFailed | DeserializeFailed | CapacityFull

  type Record = {
    present: bool,
    actor: Actor,
    operation: Operation,
    key: Key,
    requestHash: RequestHash,
    response: ResponseValue,
    responseJsonValid: bool,
  }

  type Model = {
    r0: Record,
    r1: Record,
    lastActor: Actor,
    lastOperation: Operation,
    lastKey: Key,
    lastRequestHash: RequestHash,
    lastResponse: ResponseValue,
    lastClosureOk: bool,
    lastSerializeOk: bool,
    lastOutcome: Outcome,
    sideEffects: int,
    recordWrites: int,
    lastSideEffectDelta: int,
    lastRecordWriteDelta: int,
    identityMatchedBeforeAttempt: bool,
  }

  var s: Model

  def emptyRecord = {
    present: false,
    actor: NoActor,
    operation: NoOperation,
    key: NoKey,
    requestHash: NoRequest,
    response: NoResponse,
    responseJsonValid: true,
  }

  def validKey(key) =
    key.in(Set(Key1, Key2))

  def sameIdentity(record, actor, operation, key) =
    record.present
    and record.actor == actor
    and record.operation == operation
    and record.key == key

  def matchingRecordExists(state, actor, operation, key) =
    sameIdentity(state.r0, actor, operation, key)
    or sameIdentity(state.r1, actor, operation, key)

  def matchingRecord(state, actor, operation, key) =
    if (sameIdentity(state.r0, actor, operation, key)) state.r0 else state.r1

  def emptySlotExists(state) =
    not(state.r0.present) or not(state.r1.present)

  def putRecord(state, actor, operation, key, requestHash, response, responseJsonValid) = {
    present: true,
    actor: actor,
    operation: operation,
    key: key,
    requestHash: requestHash,
    response: response,
    responseJsonValid: responseJsonValid,
  }

  action init = {
    s' = {
      r0: emptyRecord,
      r1: emptyRecord,
      lastActor: NoActor,
      lastOperation: NoOperation,
      lastKey: NoKey,
      lastRequestHash: NoRequest,
      lastResponse: NoResponse,
      lastClosureOk: false,
      lastSerializeOk: false,
      lastOutcome: NoAttempt,
      sideEffects: 0,
      recordWrites: 0,
      lastSideEffectDelta: 0,
      lastRecordWriteDelta: 0,
      identityMatchedBeforeAttempt: false,
    }
  }

  action attempt(actor, operation, key, requestHash, response, closureOk, serializeOk, responseJsonValid) = all {
    val matched = matchingRecordExists(s, actor, operation, key)
    val base = s
      .with("lastActor", actor)
      .with("lastOperation", operation)
      .with("lastKey", key)
      .with("lastRequestHash", requestHash)
      .with("lastResponse", NoResponse)
      .with("lastClosureOk", closureOk)
      .with("lastSerializeOk", serializeOk)
      .with("lastSideEffectDelta", 0)
      .with("lastRecordWriteDelta", 0)
      .with("identityMatchedBeforeAttempt", matched)

    s' =
      if (not(validKey(key))) {
        base.with("lastOutcome", InvalidKey)
      } else if (matched) {
        val record = matchingRecord(s, actor, operation, key)
        if (record.requestHash != requestHash) {
          base.with("lastOutcome", Conflict)
        } else if (not(record.responseJsonValid)) {
          base.with("lastOutcome", DeserializeFailed)
        } else {
          base
            .with("lastOutcome", Replayed)
            .with("lastResponse", record.response)
        }
      } else if (not(closureOk)) {
        base.with("lastOutcome", ClosureFailed)
      } else if (not(serializeOk)) {
        base.with("lastOutcome", SerializeFailed)
      } else if (not(emptySlotExists(s))) {
        base.with("lastOutcome", CapacityFull)
      } else if (not(s.r0.present)) {
        base
          .with("r0", putRecord(s, actor, operation, key, requestHash, response, responseJsonValid))
          .with("lastOutcome", Created)
          .with("lastResponse", response)
          .with("sideEffects", s.sideEffects + 1)
          .with("recordWrites", s.recordWrites + 1)
          .with("lastSideEffectDelta", 1)
          .with("lastRecordWriteDelta", 1)
      } else {
        base
          .with("r1", putRecord(s, actor, operation, key, requestHash, response, responseJsonValid))
          .with("lastOutcome", Created)
          .with("lastResponse", response)
          .with("sideEffects", s.sideEffects + 1)
          .with("recordWrites", s.recordWrites + 1)
          .with("lastSideEffectDelta", 1)
          .with("lastRecordWriteDelta", 1)
      },
  }

  action step = any {
    attempt(ActorA, RegisterSession, Key1, ReqA, RespA, true, true, true),
    attempt(ActorA, RegisterSession, Key1, ReqB, RespB, true, true, true),
    attempt(ActorA, RegisterSession, Key2, ReqA, RespB, true, true, true),
    attempt(ActorB, RegisterSession, Key1, ReqA, RespB, true, true, true),
    attempt(ActorA, CreateSubtask, Key1, ReqA, RespB, true, true, true),
    attempt(ActorA, RegisterSession, BlankKey, ReqA, RespA, true, true, true),
    attempt(ActorA, RegisterSession, Key1, ReqA, RespA, false, true, true),
    attempt(ActorA, RegisterSession, Key1, ReqA, RespA, true, false, true),
    attempt(ActorA, RegisterSession, Key1, ReqA, RespA, true, true, false),
  }

  action scenarioStep = any {
    all {
      s.lastOutcome == NoAttempt,
      attempt(ActorA, RegisterSession, Key1, ReqA, RespA, false, true, true),
    },
    all {
      s.lastOutcome == ClosureFailed,
      attempt(ActorA, RegisterSession, Key1, ReqA, RespA, true, false, true),
    },
    all {
      s.lastOutcome == SerializeFailed,
      attempt(ActorA, RegisterSession, BlankKey, ReqA, RespA, true, true, true),
    },
    all {
      s.lastOutcome == InvalidKey,
      s.recordWrites == 0,
      attempt(ActorA, RegisterSession, Key1, ReqA, RespA, true, true, false),
    },
    all {
      s.lastOutcome == Created,
      s.recordWrites == 1,
      not(s.r0.responseJsonValid),
      attempt(ActorA, RegisterSession, Key1, ReqA, RespA, true, true, true),
    },
    all {
      s.lastOutcome == DeserializeFailed,
      attempt(ActorA, RegisterSession, Key1, ReqB, RespB, true, true, true),
    },
    all {
      s.lastOutcome == Conflict,
      s.recordWrites == 1,
      attempt(ActorB, RegisterSession, Key1, ReqA, RespB, true, true, true),
    },
    all {
      s.lastOutcome == Created,
      s.recordWrites == 2,
      attempt(ActorB, RegisterSession, Key1, ReqA, RespB, true, true, true),
    },
    all {
      s.lastOutcome == Replayed,
      attempt(ActorA, CreateSubtask, Key1, ReqA, RespB, true, true, true),
    },
  }

  val typeOk = true

  val invalidKeyNeverRunsClosure =
    s.lastOutcome != InvalidKey
    or (s.lastSideEffectDelta == 0 and s.lastRecordWriteDelta == 0)

  val replayRequiresSameRequestAndStoredResponse =
    s.lastOutcome != Replayed
    or (
      s.identityMatchedBeforeAttempt
      and matchingRecord(s, s.lastActor, s.lastOperation, s.lastKey).requestHash == s.lastRequestHash
      and matchingRecord(s, s.lastActor, s.lastOperation, s.lastKey).response == s.lastResponse
    )

  val replayDoesNotRerunSideEffect =
    s.lastOutcome != Replayed
    or (s.lastSideEffectDelta == 0 and s.lastRecordWriteDelta == 0)

  val driftConflictsWithoutMutation =
    s.lastOutcome != Conflict
    or (s.identityMatchedBeforeAttempt and s.lastSideEffectDelta == 0 and s.lastRecordWriteDelta == 0)

  val failedMutationDoesNotRecord =
    not(s.lastOutcome.in(Set(ClosureFailed, SerializeFailed, InvalidKey, Conflict, CapacityFull)))
    or (s.lastSideEffectDelta == 0 and s.lastRecordWriteDelta == 0)

  val newRecordOnlyAfterSuccessfulMutation =
    s.lastRecordWriteDelta == 0
    or (s.lastOutcome == Created and s.lastSideEffectDelta == 1 and s.lastClosureOk and s.lastSerializeOk)

  val conflictOnlyWithinActorOperationKeyNamespace =
    s.lastOutcome != Conflict
    or matchingRecordExists(s, s.lastActor, s.lastOperation, s.lastKey)

  val actorOperationKeyNamespacesAreIndependent =
    s.lastOutcome == Conflict
    or not(s.identityMatchedBeforeAttempt)
    or s.lastOutcome.in(Set(Replayed, DeserializeFailed))

  val recordsHaveUniqueIdentities =
    not(s.r0.present and s.r1.present)
    or not(sameIdentity(s.r0, s.r1.actor, s.r1.operation, s.r1.key))

  val deserializeFailureDoesNotRerunSideEffect =
    s.lastOutcome != DeserializeFailed
    or (s.identityMatchedBeforeAttempt and s.lastSideEffectDelta == 0 and s.lastRecordWriteDelta == 0)
}

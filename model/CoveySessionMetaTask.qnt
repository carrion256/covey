module CoveySessionMetaTask {
  type SessionState = NoSession | ActiveIdle | ActiveWithSubtask | Stale | Exited
  type MetaTaskState = NoMeta | Planning | Active | Completed | MetaCancelled
  type SubtaskSummary = NoSubtasks | OpenSubtasks | TerminalSubtasks
  type ClaimState = NoClaim | Held | Released | Expired | Revoked
  type QueueState = NoQueue | Queued | InFlight | Applied | QueueCancelled

  type Model = {
    session: SessionState,
    meta: MetaTaskState,
    subtasks: SubtaskSummary,
    claim: ClaimState,
    queue: QueueState,
    heartbeatFresh: bool,
  }

  var s: Model

  def activeSession(state) = state.session.in(Set(ActiveIdle, ActiveWithSubtask))
  def activeSubtask(state) = state.session == ActiveWithSubtask
  def terminalMeta(state) = state.meta.in(Set(Completed, MetaCancelled))
  def openQueue(state) = state.queue.in(Set(Queued, InFlight))
  def claimableMeta(state) = state.meta.in(Set(Planning, Active))
  def metaFromSubtasks(subtasks) =
    if (subtasks == NoSubtasks) Planning
    else if (subtasks == TerminalSubtasks) Completed
    else Active

  action init = {
    s' = {
      session: NoSession,
      meta: NoMeta,
      subtasks: NoSubtasks,
      claim: NoClaim,
      queue: NoQueue,
      heartbeatFresh: false,
    }
  }

  action registerSession = all {
    not(activeSession(s)),
    s' = s.with("session", ActiveIdle).with("heartbeatFresh", true),
  }

  action heartbeat = all {
    activeSession(s),
    s' = s.with("heartbeatFresh", true),
  }

  action submitMetaTask = all {
    s.session == ActiveIdle,
    s.meta == NoMeta,
    s' = s.with("meta", Planning).with("subtasks", NoSubtasks),
  }

  action addOpenSubtask = all {
    s.meta == Planning,
    s.subtasks == NoSubtasks,
    s' = s.with("meta", Active).with("subtasks", OpenSubtasks),
  }

  action claimSubtask = all {
    s.session == ActiveIdle,
    claimableMeta(s),
    s.subtasks == OpenSubtasks,
    s.claim != Held,
    s' = s.with("session", ActiveWithSubtask).with("claim", Held),
  }

  action releaseClaim = all {
    s.session == ActiveWithSubtask,
    s.claim == Held,
    s' = s.with("session", ActiveIdle).with("claim", Released),
  }

  action completeOpenSubtasks = all {
    s.meta == Active,
    s.subtasks == OpenSubtasks,
    s.claim != Held,
    not(openQueue(s)),
    s' = s.with("meta", Completed).with("subtasks", TerminalSubtasks),
  }

  action enqueueApply = all {
    s.meta == Active,
    s.subtasks == OpenSubtasks,
    s.queue == NoQueue,
    s.claim != Held,
    s' = s.with("queue", Queued),
  }

  action claimApply = all {
    s.meta == Active,
    s.queue == Queued,
    s' = s.with("queue", InFlight),
  }

  action markApplied = all {
    s.meta == Active,
    s.queue == InFlight,
    s' = s.with("queue", Applied).with("subtasks", TerminalSubtasks).with("meta", Completed),
  }

  action cancelMetaTask = all {
    s.meta.in(Set(Planning, Active)),
    s' = s
      .with("meta", MetaCancelled)
      .with("subtasks", TerminalSubtasks)
      .with("claim", if (s.claim == Held) Revoked else s.claim)
      .with("queue", if (openQueue(s)) QueueCancelled else s.queue)
      .with("session", if (s.session == ActiveWithSubtask) ActiveIdle else s.session),
  }

  action reapStaleIdleSession = all {
    s.session == ActiveIdle,
    not(s.heartbeatFresh),
    s' = s.with("session", Stale),
  }

  action reapStaleHeldSession = all {
    s.session == ActiveWithSubtask,
    s.claim == Held,
    not(s.heartbeatFresh),
    s' = s.with("session", Stale).with("claim", Expired),
  }

  action ageHeartbeat = all {
    activeSession(s),
    s.heartbeatFresh,
    s' = s.with("heartbeatFresh", false),
  }

  action exitIdleSession = all {
    s.session.in(Set(ActiveIdle, Stale)),
    s.claim != Held,
    s' = s.with("session", Exited).with("heartbeatFresh", false),
  }

  action exitHeldSession = all {
    s.session == ActiveWithSubtask,
    s.claim == Held,
    s' = s.with("session", Exited).with("claim", Expired).with("heartbeatFresh", false),
  }

  action step = any {
    registerSession,
    heartbeat,
    submitMetaTask,
    addOpenSubtask,
    claimSubtask,
    releaseClaim,
    completeOpenSubtasks,
    enqueueApply,
    claimApply,
    markApplied,
    cancelMetaTask,
    reapStaleIdleSession,
    reapStaleHeldSession,
    ageHeartbeat,
    exitIdleSession,
    exitHeldSession,
  }

  val typeOk = true
  val heldClaimRequiresActiveSessionWithSubtask = s.claim != Held or s.session == ActiveWithSubtask
  val activeSubtaskRequiresHeldClaim = not(activeSubtask(s)) or s.claim == Held
  val inactiveSessionHasNoHeldClaim = not(s.session.in(Set(Stale, Exited))) or s.claim != Held
  val inactiveSessionHasNoFreshHeartbeat = not(s.session.in(Set(Stale, Exited))) or not(s.heartbeatFresh)
  val terminalMetaHasNoHeldClaim = not(terminalMeta(s)) or s.claim != Held
  val terminalMetaHasNoOpenQueue = not(terminalMeta(s)) or not(openQueue(s))
  val completedRequiresTerminalSubtasks = s.meta != Completed or s.subtasks == TerminalSubtasks
  val cancelledHasTerminalSubtaskSummary = s.meta != MetaCancelled or s.subtasks == TerminalSubtasks
  val planningRequiresNoSubtasks = s.meta != Planning or s.subtasks == NoSubtasks
  val activeRequiresOpenSubtasks = s.meta != Active or s.subtasks == OpenSubtasks
  val noMetaHasNoWork = s.meta != NoMeta or (s.subtasks == NoSubtasks and s.claim == NoClaim and s.queue == NoQueue)
}

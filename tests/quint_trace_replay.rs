use rstest::{fixture, rstest};
use serde::Deserialize;

const COVEY_REVIEW_FOLLOWUP_ITF: &str = include_str!("fixtures/quint/CoveyReviewFollowup.itf.json");
const COVEY_CORE_LIFECYCLE_ITF: &str = include_str!("fixtures/quint/CoveyCoreLifecycle.itf.json");
const COVEY_QUEUE_RESERVATION_ITF: &str =
    include_str!("fixtures/quint/CoveyQueueReservation.itf.json");

#[derive(Debug, Deserialize)]
struct ItfTrace {
    states: Vec<ItfState>,
}

#[derive(Debug, Deserialize)]
struct ItfState {
    m: ReviewFollowupState,
}

#[derive(Debug, Deserialize)]
struct CoreItfTrace {
    states: Vec<CoreItfState>,
}

#[derive(Debug, Deserialize)]
struct CoreItfState {
    s: CoreLifecycleState,
}

#[derive(Debug, Deserialize)]
struct QueueReservationItfTrace {
    states: Vec<QueueReservationItfState>,
}

#[derive(Debug, Deserialize)]
struct QueueReservationItfState {
    s: QueueReservationState,
}

#[derive(Debug, Deserialize)]
struct CoreLifecycleState {
    #[serde(deserialize_with = "deserialize_itf_variant")]
    subtask: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    claim: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    session: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    review: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    queue: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    fence: String,
    #[serde(rename = "activeSubtask")]
    active_subtask: bool,
    #[serde(rename = "artifactPresent")]
    artifact_present: bool,
    #[serde(rename = "reviewApproved")]
    review_approved: bool,
    #[serde(rename = "applyVerified")]
    apply_verified: bool,
    terminal: bool,
}

#[derive(Debug, Deserialize)]
struct QueueReservationState {
    #[serde(deserialize_with = "deserialize_itf_variant")]
    queue: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    reservation: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    conflict: String,
    #[serde(rename = "queueFence", deserialize_with = "deserialize_itf_variant")]
    queue_fence: String,
    #[serde(rename = "queueClaimLive")]
    queue_claim_live: bool,
    #[serde(rename = "queueLeaseLive")]
    queue_lease_live: bool,
    #[serde(rename = "applyVerified")]
    apply_verified: bool,
    #[serde(rename = "subtaskReady")]
    subtask_ready: bool,
    #[serde(rename = "artifactMatches")]
    artifact_matches: bool,
    #[serde(rename = "metaSchedulable")]
    meta_schedulable: bool,
    #[serde(rename = "reservationLeaseLive")]
    reservation_lease_live: bool,
    #[serde(rename = "overlapDetected")]
    overlap_detected: bool,
    #[serde(rename = "conflictBound")]
    conflict_bound: bool,
    #[serde(
        rename = "conflictRankFloor",
        deserialize_with = "deserialize_itf_bigint"
    )]
    conflict_rank_floor: i64,
}

#[derive(Debug, Deserialize)]
struct ReviewFollowupState {
    #[serde(deserialize_with = "deserialize_itf_variant")]
    b0: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    b1: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    b2: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    b3: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    p0: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    p1: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    p2: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    p3: String,
    #[serde(deserialize_with = "deserialize_itf_variant")]
    active: String,
    #[serde(rename = "nextBlock", deserialize_with = "deserialize_itf_bigint")]
    next_block: i64,
    #[serde(rename = "idleObserved")]
    idle_observed: bool,
    r0: bool,
    r1: bool,
    r2: bool,
    r3: bool,
}

#[derive(Debug, Deserialize)]
struct ItfVariant {
    tag: String,
}

#[derive(Debug, Deserialize)]
struct ItfBigInt {
    #[serde(rename = "#bigint")]
    value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    B0,
    B1,
    B2,
    B3,
}

const BLOCKS: [Block; 4] = [Block::B0, Block::B1, Block::B2, Block::B3];

fn deserialize_itf_variant<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(ItfVariant::deserialize(deserializer)?.tag)
}

fn deserialize_itf_bigint<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    ItfBigInt::deserialize(deserializer)?
        .value
        .parse::<i64>()
        .map_err(serde::de::Error::custom)
}

impl Block {
    fn as_str(self) -> &'static str {
        match self {
            Block::B0 => "B0",
            Block::B1 => "B1",
            Block::B2 => "B2",
            Block::B3 => "B3",
        }
    }

    fn index(self) -> usize {
        match self {
            Block::B0 => 0,
            Block::B1 => 1,
            Block::B2 => 2,
            Block::B3 => 3,
        }
    }
}

impl ReviewFollowupState {
    fn status(&self, block: Block) -> &str {
        match block {
            Block::B0 => self.b0.as_str(),
            Block::B1 => self.b1.as_str(),
            Block::B2 => self.b2.as_str(),
            Block::B3 => self.b3.as_str(),
        }
    }

    fn parent(&self, block: Block) -> &str {
        match block {
            Block::B0 => self.p0.as_str(),
            Block::B1 => self.p1.as_str(),
            Block::B2 => self.p2.as_str(),
            Block::B3 => self.p3.as_str(),
        }
    }

    fn rejected(&self, block: Block) -> bool {
        match block {
            Block::B0 => self.r0,
            Block::B1 => self.r1,
            Block::B2 => self.r2,
            Block::B3 => self.r3,
        }
    }
}

fn block_from_str(value: &str) -> Option<Block> {
    match value {
        "B0" => Some(Block::B0),
        "B1" => Some(Block::B1),
        "B2" => Some(Block::B2),
        "B3" => Some(Block::B3),
        _ => None,
    }
}

fn children_of(state: &ReviewFollowupState, block: Block) -> Vec<Block> {
    BLOCKS
        .into_iter()
        .filter(|child| state.parent(*child) == block.as_str() && state.status(*child) != "Absent")
        .collect()
}

fn available_block_exists(state: &ReviewFollowupState) -> bool {
    BLOCKS
        .into_iter()
        .any(|block| state.status(block) == "Available")
}

fn repairable_missing_followup(state: &ReviewFollowupState) -> bool {
    let Ok(next_index) = usize::try_from(state.next_block) else {
        return false;
    };
    let Some(candidate) = BLOCKS.get(next_index).copied() else {
        return false;
    };
    if state.status(candidate) != "Absent" {
        return false;
    }
    BLOCKS.into_iter().any(|block| {
        state.status(block) == "ChangesRequested" && children_of(state, block).is_empty()
    })
}

fn replay_review_followup_trace(trace: &ItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.m;
        for block in BLOCKS {
            let status = state.status(block);
            let parent = state.parent(block);
            let children = children_of(state, block);
            if status == "Absent" && parent != "NoBlock" {
                violations.push(format!(
                    "state[{index}]: absent block {} has parent {parent}",
                    block.as_str()
                ));
            }
            if state.rejected(block) {
                if status != "ChangesRequested" {
                    violations.push(format!(
                        "state[{index}]: rejected block {} is {status}",
                        block.as_str()
                    ));
                }
                if children.len() != 1 {
                    violations.push(format!(
                        "state[{index}]: rejected block {} has {} followups",
                        block.as_str(),
                        children.len()
                    ));
                }
            }
            if children.len() > 1 {
                violations.push(format!(
                    "state[{index}]: block {} has forked followups",
                    block.as_str()
                ));
            }
            if let Some(parent_block) = block_from_str(parent)
                && parent_block.index() >= block.index()
            {
                violations.push(format!(
                    "state[{index}]: followup {} does not point backward",
                    block.as_str()
                ));
            }
        }
        if state.idle_observed
            && (available_block_exists(state) || repairable_missing_followup(state))
        {
            violations.push(format!(
                "state[{index}]: idle observed while work or repair exists"
            ));
        }
        if let Some(active) = block_from_str(&state.active)
            && !matches!(state.status(active), "Claimed" | "InProgress")
        {
            violations.push(format!(
                "state[{index}]: active block {} is not claimed or in progress",
                active.as_str()
            ));
        }
    }
    violations
}

fn core_terminal_subtask(state: &CoreLifecycleState) -> bool {
    matches!(state.subtask.as_str(), "Applied" | "Abandoned")
}

fn core_claim_live_subtask(state: &CoreLifecycleState) -> bool {
    matches!(
        state.subtask.as_str(),
        "Claimed" | "InProgress" | "ArtifactPublished" | "ReviewPending"
    )
}

fn core_queue_open(state: &CoreLifecycleState) -> bool {
    matches!(state.queue.as_str(), "Queued" | "QueueInFlight")
}

fn queue_reservation_terminal_queue(state: &QueueReservationState) -> bool {
    matches!(state.queue.as_str(), "Applied" | "Superseded" | "Cancelled")
}

fn queue_reservation_conflict_rank(state: &QueueReservationState) -> i64 {
    match state.conflict.as_str() {
        "Acknowledged" => 1,
        "Resolved" => 2,
        _ => 0,
    }
}

fn replay_core_lifecycle_trace(trace: &CoreItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if state.claim == "Held"
            && !(state.session == "ActiveSession"
                && state.active_subtask
                && core_claim_live_subtask(state))
        {
            violations.push(format!(
                "state[{index}]: held claim is not bound to active session/subtask"
            ));
        }
        if state.active_subtask && state.claim != "Held" {
            violations.push(format!("state[{index}]: active subtask without held claim"));
        }
        if core_terminal_subtask(state) && state.claim == "Held" {
            violations.push(format!("state[{index}]: terminal subtask has held claim"));
        }
        if state.review != "NoReview" && !state.artifact_present {
            violations.push(format!("state[{index}]: review exists without artifact"));
        }
        if state.review == "Decided"
            && !matches!(
                state.subtask.as_str(),
                "ChangesRequested" | "Approved" | "ReadyForApply" | "Applied"
            )
        {
            violations.push(format!(
                "state[{index}]: decided review is not reflected in subtask state"
            ));
        }
        if state.queue != "NoQueue" && !state.review_approved {
            violations.push(format!(
                "state[{index}]: ready queue exists without approved review"
            ));
        }
        if core_queue_open(state) && state.subtask != "ReadyForApply" {
            violations.push(format!(
                "state[{index}]: open ready queue is not bound to ready_for_apply subtask"
            ));
        }
        if state.queue == "QueueApplied" && !(state.subtask == "Applied" && state.apply_verified) {
            violations.push(format!(
                "state[{index}]: applied queue lacks apply verification"
            ));
        }
        if state.subtask == "Applied" && state.queue != "QueueApplied" {
            violations.push(format!(
                "state[{index}]: applied subtask lacks applied queue"
            ));
        }
        if state.terminal != core_terminal_subtask(state) {
            violations.push(format!(
                "state[{index}]: terminal marker disagrees with subtask state"
            ));
        }
        if matches!(state.fence.as_str(), "F1" | "F2") && state.claim == "NoClaim" {
            violations.push(format!(
                "state[{index}]: issued fence exists before any claim lifecycle"
            ));
        }
    }
    violations
}

fn replay_queue_reservation_trace(trace: &QueueReservationItfTrace) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, wrapped_state) in trace.states.iter().enumerate() {
        let state = &wrapped_state.s;
        if state.queue_claim_live != (state.queue == "InFlight") {
            violations.push(format!(
                "state[{index}]: queue claim liveness disagrees with queue state"
            ));
        }
        if state.queue == "InFlight" && (!state.queue_lease_live || state.queue_fence == "F0") {
            violations.push(format!(
                "state[{index}]: in-flight queue lacks live lease or issued fence"
            ));
        }
        if queue_reservation_terminal_queue(state)
            && (state.queue_claim_live || state.queue_lease_live)
        {
            violations.push(format!("state[{index}]: terminal queue has live claim"));
        }
        if state.queue == "Applied" && !state.apply_verified {
            violations.push(format!(
                "state[{index}]: applied queue lacks apply verification"
            ));
        }
        if state.queue == "Queued" && (!state.subtask_ready || !state.artifact_matches) {
            violations.push(format!(
                "state[{index}]: queued item is not bound to a ready matching artifact"
            ));
        }
        if state.queue == "InFlight"
            && (!state.subtask_ready || !state.artifact_matches || !state.meta_schedulable)
        {
            violations.push(format!(
                "state[{index}]: in-flight queue lacks ready artifact or schedulable meta-task"
            ));
        }
        if state.reservation == "Active" && !state.reservation_lease_live {
            violations.push(format!(
                "state[{index}]: active reservation lacks live lease"
            ));
        }
        if matches!(state.reservation.as_str(), "Released" | "Expired")
            && state.reservation_lease_live
        {
            violations.push(format!(
                "state[{index}]: terminal reservation has live lease"
            ));
        }
        if matches!(state.conflict.as_str(), "Open" | "Acknowledged")
            && !(state.reservation == "Active" && state.overlap_detected)
        {
            violations.push(format!(
                "state[{index}]: unresolved conflict is not bound to active overlap"
            ));
        }
        if state.conflict != "NoConflict" && !(state.overlap_detected && state.conflict_bound) {
            violations.push(format!(
                "state[{index}]: conflict exists without recorded overlap binding"
            ));
        }
        if queue_reservation_conflict_rank(state) < state.conflict_rank_floor {
            violations.push(format!(
                "state[{index}]: conflict resolution moved below recorded floor"
            ));
        }
        if state.conflict_rank_floor >= 2 && state.conflict != "Resolved" {
            violations.push(format!(
                "state[{index}]: resolved conflict floor was downgraded"
            ));
        }
    }
    violations
}

#[fixture]
fn review_followup_trace() -> ItfTrace {
    serde_json::from_str(COVEY_REVIEW_FOLLOWUP_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn core_lifecycle_trace() -> CoreItfTrace {
    serde_json::from_str(COVEY_CORE_LIFECYCLE_ITF).expect("fixture must be valid ITF JSON")
}

#[fixture]
fn queue_reservation_trace() -> QueueReservationItfTrace {
    serde_json::from_str(COVEY_QUEUE_RESERVATION_ITF).expect("fixture must be valid ITF JSON")
}

#[rstest]
fn covey_replays_quint_review_followup_itf_trace(review_followup_trace: ItfTrace) {
    assert!(
        !review_followup_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    assert_eq!(
        replay_review_followup_trace(&review_followup_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_core_lifecycle_itf_trace(core_lifecycle_trace: CoreItfTrace) {
    assert!(
        !core_lifecycle_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    assert_eq!(
        replay_core_lifecycle_trace(&core_lifecycle_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replays_quint_queue_reservation_itf_trace(
    queue_reservation_trace: QueueReservationItfTrace,
) {
    assert!(
        !queue_reservation_trace.states.is_empty(),
        "fixture should contain at least one state"
    );
    assert_eq!(
        replay_queue_reservation_trace(&queue_reservation_trace),
        Vec::<String>::new()
    );
}

#[rstest]
fn covey_replay_reports_quint_counterexample_shape() {
    let state = ReviewFollowupState {
        b0: "Available".to_owned(),
        b1: "Available".to_owned(),
        b2: "Absent".to_owned(),
        b3: "Absent".to_owned(),
        p0: "NoBlock".to_owned(),
        p1: "B0".to_owned(),
        p2: "NoBlock".to_owned(),
        p3: "NoBlock".to_owned(),
        active: "NoBlock".to_owned(),
        next_block: 2,
        idle_observed: true,
        r0: false,
        r1: false,
        r2: false,
        r3: false,
    };
    let trace = ItfTrace {
        states: vec![ItfState { m: state }],
    };

    assert_eq!(
        replay_review_followup_trace(&trace),
        vec!["state[0]: idle observed while work or repair exists"]
    );
}

#[rstest]
fn covey_core_lifecycle_replay_reports_counterexample_shape() {
    let state = CoreLifecycleState {
        subtask: "Applied".to_owned(),
        claim: "Held".to_owned(),
        session: "ExitedSession".to_owned(),
        review: "Requested".to_owned(),
        queue: "QueueApplied".to_owned(),
        fence: "F1".to_owned(),
        active_subtask: true,
        artifact_present: false,
        review_approved: false,
        apply_verified: false,
        terminal: false,
    };
    let trace = CoreItfTrace {
        states: vec![CoreItfState { s: state }],
    };

    assert_eq!(
        replay_core_lifecycle_trace(&trace),
        vec![
            "state[0]: held claim is not bound to active session/subtask",
            "state[0]: terminal subtask has held claim",
            "state[0]: review exists without artifact",
            "state[0]: ready queue exists without approved review",
            "state[0]: applied queue lacks apply verification",
            "state[0]: terminal marker disagrees with subtask state",
        ]
    );
}

#[rstest]
fn covey_queue_reservation_replay_reports_counterexample_shape() {
    let state = QueueReservationState {
        queue: "Applied".to_owned(),
        reservation: "Released".to_owned(),
        conflict: "Acknowledged".to_owned(),
        queue_fence: "F0".to_owned(),
        queue_claim_live: true,
        queue_lease_live: true,
        apply_verified: false,
        subtask_ready: false,
        artifact_matches: false,
        meta_schedulable: false,
        reservation_lease_live: true,
        overlap_detected: false,
        conflict_bound: false,
        conflict_rank_floor: 2,
    };
    let trace = QueueReservationItfTrace {
        states: vec![QueueReservationItfState { s: state }],
    };

    assert_eq!(
        replay_queue_reservation_trace(&trace),
        vec![
            "state[0]: queue claim liveness disagrees with queue state",
            "state[0]: terminal queue has live claim",
            "state[0]: applied queue lacks apply verification",
            "state[0]: terminal reservation has live lease",
            "state[0]: unresolved conflict is not bound to active overlap",
            "state[0]: conflict exists without recorded overlap binding",
            "state[0]: conflict resolution moved below recorded floor",
            "state[0]: resolved conflict floor was downgraded",
        ]
    );
}

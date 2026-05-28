use rstest::{fixture, rstest};
use serde::Deserialize;

const COVEY_REVIEW_FOLLOWUP_ITF: &str = include_str!("fixtures/quint/CoveyReviewFollowup.itf.json");

#[derive(Debug, Deserialize)]
struct ItfTrace {
    states: Vec<ItfState>,
}

#[derive(Debug, Deserialize)]
struct ItfState {
    m: ReviewFollowupState,
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

#[fixture]
fn review_followup_trace() -> ItfTrace {
    serde_json::from_str(COVEY_REVIEW_FOLLOWUP_ITF).expect("fixture must be valid ITF JSON")
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

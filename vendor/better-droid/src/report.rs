use crate::model::{PlanningClass, SourceSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Machine-readable lint/compile report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionReport {
    pub schema: String,
    pub change_id: String,
    pub planning_class: PlanningClass,
    #[serde(default)]
    pub packet_kind: PacketKind,
    pub status: ReportStatus,
    pub readiness: ReadinessGates,
    pub product_impact: ProductImpactAudit,
    pub requires_implementation_packet: bool,
    pub import_ready: bool,
    pub checked_artifact_paths: Vec<String>,
    pub created_artifacts: Vec<String>,
    pub source_digests: BTreeMap<String, String>,
    pub artifact_digests: BTreeMap<String, String>,
    pub task_counts: TaskCounts,
    pub blockers: Vec<Blocker>,
    pub warnings: Vec<Warning>,
    pub task_classifications: Vec<TaskClassification>,
    pub non_canonical_run_metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    PlanningReady,
    PlanningBlocked,
    CoveyImportReady,
    Blocked,
}

impl std::fmt::Display for ReportStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PlanningReady => formatter.write_str("planning_ready"),
            Self::PlanningBlocked => formatter.write_str("planning_ready_blocked"),
            Self::CoveyImportReady => formatter.write_str("covey_import_ready"),
            Self::Blocked => formatter.write_str("blocked"),
        }
    }
}

impl ReportStatus {
    pub const fn planning_ready(self) -> bool {
        matches!(self, Self::PlanningReady | Self::CoveyImportReady)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PacketKind {
    #[default]
    PlanningOnly,
    Implementation,
    Migration,
    ApplyOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ReadinessGates {
    pub planning_ready: bool,
    pub planning_ready_blocked: bool,
    pub covey_import_ready: bool,
    pub covey_imported: bool,
    pub implementation_ready: bool,
    pub execution_ready: bool,
    pub review_approved: bool,
    pub apply_queued: bool,
    pub apply_authorized: bool,
    pub landed: bool,
    pub shipped_verified: bool,
    pub not_imported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ProductImpactAudit {
    pub product_files_changed: bool,
    pub product_tests_run: bool,
    pub covey_imported: bool,
    pub apply_receipt: bool,
    pub shipped_evidence: bool,
    pub product_write_paths: Vec<String>,
    pub validation_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaskCounts {
    pub total: usize,
    pub importable: usize,
    pub blocked: usize,
    pub rejected: usize,
    pub deferred: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Blocker {
    pub id: String,
    pub source_path: String,
    pub task_id: Option<String>,
    pub scenario_id: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Warning {
    pub id: String,
    pub source_path: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskClassification {
    pub task_id: String,
    pub title: String,
    pub task_type: String,
    pub import_status: ImportStatus,
    pub task_digest: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImportStatus {
    Importable,
    Blocked,
    Rejected,
    Deferred,
}

#[derive(Debug, Clone)]
pub(crate) struct LintState {
    pub(crate) blockers: Vec<Blocker>,
    pub(crate) warnings: Vec<Warning>,
    pub(crate) classifications: Vec<TaskClassification>,
}

impl LintState {
    pub(crate) fn has_blockers(&self) -> bool {
        !self.blockers.is_empty()
    }
}

pub(crate) fn report_from_lint(
    source: &SourceSnapshot,
    lint: LintState,
    created_artifacts: Vec<String>,
    artifact_digests: BTreeMap<String, String>,
) -> MissionReport {
    let task_counts = task_counts(&lint.classifications);
    let packet_kind = classify_packet(source);
    let product_impact = product_impact_audit(source);
    let human_gate_blocked = is_human_gate_only_blocked(&lint.blockers);
    let implementation_ready = lint.blockers.is_empty()
        && source.planning_class == PlanningClass::WorkPacket
        && packet_kind == PacketKind::Implementation
        && !product_impact.product_write_paths.is_empty();
    let planning_ready = lint.blockers.is_empty() || human_gate_blocked;
    let covey_import_ready = lint.blockers.is_empty()
        && source.planning_class == PlanningClass::WorkPacket
        && matches!(
            packet_kind,
            PacketKind::Implementation | PacketKind::Migration | PacketKind::ApplyOnly
        );
    let status = if covey_import_ready {
        ReportStatus::CoveyImportReady
    } else if planning_ready && human_gate_blocked {
        ReportStatus::PlanningBlocked
    } else if planning_ready {
        ReportStatus::PlanningReady
    } else {
        ReportStatus::Blocked
    };
    let requires_implementation_packet = packet_requires_implementation(source, packet_kind);
    MissionReport {
        schema: "better-droid.compile-report.v1".to_owned(),
        change_id: source.change_id.clone(),
        planning_class: source.planning_class,
        packet_kind,
        status,
        readiness: ReadinessGates {
            planning_ready,
            planning_ready_blocked: human_gate_blocked,
            covey_import_ready,
            covey_imported: false,
            implementation_ready,
            execution_ready: false,
            review_approved: false,
            apply_queued: false,
            apply_authorized: false,
            landed: false,
            shipped_verified: false,
            not_imported: true,
        },
        product_impact,
        requires_implementation_packet,
        import_ready: covey_import_ready,
        checked_artifact_paths: source_path_list(source),
        created_artifacts,
        source_digests: source_digest_map(source),
        artifact_digests,
        task_counts,
        blockers: lint.blockers,
        warnings: lint.warnings,
        task_classifications: lint.classifications,
        non_canonical_run_metadata: BTreeMap::new(),
    }
}

pub(crate) fn classify_packet(source: &SourceSnapshot) -> PacketKind {
    let tasks = source.tasks_parsed.iter().collect::<Vec<_>>();
    let write_paths = tasks
        .iter()
        .flat_map(|task| {
            task.fields
                .get("Allowed Write Paths")
                .into_iter()
                .flat_map(|value| value.split([',', '\n']))
                .map(str::trim)
                .map(|path| path.trim_start_matches("- ").trim_matches('`'))
                .filter(|path| !path.is_empty())
        })
        .collect::<Vec<_>>();

    if write_paths.iter().all(|path| is_planning_path(path)) {
        return PacketKind::PlanningOnly;
    }

    let text = format!(
        "{}\n{}\n{}",
        source.change_id, source.proposal.text, source.tasks.text
    )
    .to_ascii_lowercase();
    let has_product_implementation_task = tasks.iter().any(|task| {
        task.fields.get("Type").is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "implementation" | "test" | "data-movement" | "schema-change" | "refactor"
            )
        }) && task
            .fields
            .get("Allowed Write Paths")
            .into_iter()
            .flat_map(|value| value.split([',', '\n']))
            .map(str::trim)
            .map(|path| path.trim_start_matches("- ").trim_matches('`'))
            .any(|path| !path.is_empty() && !is_planning_path(path))
    });
    let explicitly_apply_only = text.contains("apply-only") || text.contains("apply only");
    let has_apply_task = tasks.iter().any(|task| {
        task.fields
            .get("Type")
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("apply"))
    });
    let explicitly_migration = source
        .change_id
        .to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|token| matches!(token, "migrate" | "migration" | "cutover"))
        || text.contains("migration packet")
        || text.contains("data migration")
        || tasks.iter().any(|task| {
            task.fields.get("Type").is_some_and(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "data-movement" | "schema-change"
                )
            })
        });
    if has_product_implementation_task && !explicitly_migration {
        PacketKind::Implementation
    } else if (explicitly_apply_only || has_apply_task) && !has_product_implementation_task {
        PacketKind::ApplyOnly
    } else if explicitly_migration {
        PacketKind::Migration
    } else {
        PacketKind::Implementation
    }
}

fn is_human_gate_only_blocked(blockers: &[Blocker]) -> bool {
    !blockers.is_empty() && blockers.iter().all(is_human_gate_blocker)
}

fn is_human_gate_blocker(blocker: &Blocker) -> bool {
    blocker.id == "human_gate_open"
        || blocker
            .detail
            .to_ascii_lowercase()
            .contains("blocked-needs-human")
        || blocker
            .detail
            .to_ascii_lowercase()
            .contains("human approval")
}

fn product_impact_audit(source: &SourceSnapshot) -> ProductImpactAudit {
    let mut product_write_paths = BTreeSet::new();
    let mut validation_commands = BTreeSet::new();

    for task in &source.tasks_parsed {
        if let Some(paths) = task.fields.get("Allowed Write Paths") {
            for path in paths
                .split([',', '\n'])
                .map(str::trim)
                .map(|path| path.trim_start_matches("- ").trim_matches('`'))
                .filter(|path| !path.is_empty())
            {
                if !is_planning_path(path) {
                    product_write_paths.insert(path.to_owned());
                }
            }
        }
        if let Some(command) = task.fields.get("Command / Action")
            && !command.trim().is_empty()
        {
            validation_commands.insert(command.trim().to_owned());
        }
    }

    ProductImpactAudit {
        product_files_changed: false,
        product_tests_run: false,
        covey_imported: false,
        apply_receipt: false,
        shipped_evidence: false,
        product_write_paths: product_write_paths.into_iter().collect(),
        validation_commands: validation_commands.into_iter().collect(),
    }
}

fn is_planning_path(path: &str) -> bool {
    let normalized = path.trim().trim_start_matches("./");
    normalized.is_empty()
        || normalized.eq_ignore_ascii_case("none")
        || normalized.starts_with("openspec/")
        || normalized.starts_with("docs/")
        || normalized.starts_with("better-droid/docs-site/")
        || normalized.starts_with("better-droid/openspec/")
}

fn packet_requires_implementation(source: &SourceSnapshot, packet_kind: PacketKind) -> bool {
    if packet_kind != PacketKind::PlanningOnly {
        return false;
    }
    let text = format!("{}\n{}", source.proposal.text, source.tasks.text).to_ascii_lowercase();
    text.contains("follow-up implementation")
        || text.contains("follow-up code")
        || text.contains("does not implement code directly")
        || text.contains("planning packet")
        || text.contains("blocks code execution")
}

pub(crate) fn task_counts(classifications: &[TaskClassification]) -> TaskCounts {
    let mut counts = TaskCounts {
        total: classifications.len(),
        ..TaskCounts::default()
    };
    for classification in classifications {
        match classification.import_status {
            ImportStatus::Importable => counts.importable += 1,
            ImportStatus::Blocked => counts.blocked += 1,
            ImportStatus::Rejected => counts.rejected += 1,
            ImportStatus::Deferred => counts.deferred += 1,
        }
    }
    counts
}

pub(crate) fn source_path_list(source: &SourceSnapshot) -> Vec<String> {
    let mut paths = vec![
        source.openspec_yaml.relative_path.clone(),
        source.proposal.relative_path.clone(),
        source.design.relative_path.clone(),
        source.tasks.relative_path.clone(),
    ];
    paths.extend(source.specs.iter().map(|spec| spec.relative_path.clone()));
    paths.sort();
    paths
}

pub(crate) fn source_digest_map(source: &SourceSnapshot) -> BTreeMap<String, String> {
    let mut digests = BTreeMap::new();
    digests.insert(
        source.openspec_yaml.relative_path.clone(),
        source.openspec_yaml.digest.clone(),
    );
    digests.insert(
        source.proposal.relative_path.clone(),
        source.proposal.digest.clone(),
    );
    digests.insert(
        source.design.relative_path.clone(),
        source.design.digest.clone(),
    );
    digests.insert(
        source.tasks.relative_path.clone(),
        source.tasks.digest.clone(),
    );
    for spec in &source.specs {
        digests.insert(spec.relative_path.clone(), spec.digest.clone());
    }
    digests
}

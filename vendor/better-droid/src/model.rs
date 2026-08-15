use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::report::ImportStatus;

pub(crate) const CANONICAL_PROTECTED_FORBIDDEN_PATHS: &[&str] =
    &["authority/**", "contracts/imported/**", ".git/**"];
pub(crate) const PROTECTED_FORBIDDEN_PATH_GROUPS: &[&[&str]] =
    &[&["authority/**"], &["contracts/imported/**"], &[".git/**"]];
pub(crate) const ARTIFACT_NAMES: &[&str] = &[
    "mission.json",
    "mission-packet.json",
    "traceability.json",
    "validation.json",
    "path-policy.json",
    "review-rubric.json",
    "assumptions.json",
    "compile-report.json",
];

/// Options for Better Droid mission lint.
#[derive(Debug, Clone)]
pub struct LintOptions {
    pub project_root: PathBuf,
    pub change_id: String,
}

/// Options for Better Droid mission compile.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    pub project_root: PathBuf,
    pub change_id: String,
    pub output_dir: Option<PathBuf>,
}

/// Typed view of a compiled Better Droid mission bundle that Covey can import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledMission {
    pub tasks: Vec<CompiledTask>,
    pub planning_class: PlanningClass,
    pub source_digests: BTreeMap<String, String>,
    pub artifact_digests: BTreeMap<String, String>,
    pub artifact_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct MissionArtifact {
    pub schema: String,
    pub change_id: String,
    pub tasks: Vec<CompiledTask>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PathPolicyArtifact {
    pub schema: String,
    pub change_id: String,
    #[serde(default)]
    pub allowed_write_paths: Vec<String>,
    #[serde(default)]
    pub generated_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AssumptionsArtifact {
    pub schema: String,
    pub change_id: String,
    #[serde(default)]
    pub approval_summary: AssumptionApprovalSummary,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct AssumptionApprovalSummary {
    #[serde(default)]
    pub high_or_critical_pending: i64,
}

#[derive(Debug)]
pub(crate) struct SourceSnapshot {
    pub(crate) change_id: String,
    pub(crate) relative_change_path: String,
    pub(crate) schema_name: String,
    pub(crate) change_status: Option<String>,
    pub(crate) planning_class: PlanningClass,
    pub(crate) planning_class_raw: Option<String>,
    pub(crate) openspec_yaml: SourceFile,
    pub(crate) proposal: SourceFile,
    pub(crate) design: SourceFile,
    pub(crate) tasks: SourceFile,
    pub(crate) specs: Vec<SourceFile>,
    pub(crate) tasks_parsed: Vec<SourceTask>,
    pub(crate) requirements: Vec<Requirement>,
    pub(crate) scenarios: Vec<Scenario>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanningClass {
    WorkPacket,
    Invalid,
}

impl PlanningClass {
    pub(crate) fn parse_yaml(value: Option<&str>) -> Self {
        let Some(value) = value else {
            return Self::Invalid;
        };
        Self::parse_value(value)
    }

    fn parse_value(value: &str) -> Self {
        match value.trim().replace('-', "_").to_ascii_lowercase().as_str() {
            "work_packet" => Self::WorkPacket,
            "invalid" => Self::Invalid,
            _ => Self::Invalid,
        }
    }
}

impl<'de> Deserialize<'de> for PlanningClass {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::parse_value(&value))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SourceFile {
    pub(crate) relative_path: String,
    pub(crate) text: String,
    pub(crate) digest: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SourceTask {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) source_path: String,
    pub(crate) fields: BTreeMap<String, String>,
    pub(crate) field_syntax_errors: Vec<String>,
    pub(crate) raw_block: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompiledTask {
    pub task_id: String,
    pub title: String,
    pub task_type: String,
    pub import_status: ImportStatus,
    pub purpose: String,
    pub acceptance_criteria: Vec<String>,
    pub requirement_ids: Vec<String>,
    pub scenario_ids: Vec<String>,
    pub dependencies: Vec<String>,
    pub allowed_read_paths: Vec<String>,
    pub allowed_write_paths: Vec<String>,
    pub forbidden_write_paths: Vec<String>,
    pub validation_refs: Vec<String>,
    pub expected_evidence: Vec<String>,
    pub expected_artifact_kind: String,
    pub review_rubric_refs: Vec<String>,
    pub stale_if: Vec<String>,
    pub task_digest: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Requirement {
    pub(crate) id: String,
    pub(crate) title: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Scenario {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) requirement_id: String,
}

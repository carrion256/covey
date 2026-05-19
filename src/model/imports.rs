//! Import boundary DTOs for historical Beads and OpenSpec planning input.
//!
//! Raw IDs and tokens here represent external source or CLI payload shape.
//! Imported records are converted into Covey's validated domain records before
//! becoming live lifecycle state.

use derive_new::new;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

use super::{ObjectType, TimestampMs};

/// Request to import eligible bd issues from a beads database into Covey as work subtasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportBdV1Req {
    pub session_token: String,
    pub beads_db_path: String,
    pub meta_task_id: Option<String>,
    pub prompt_text: Option<String>,
    pub idempotency_key: String,
}

/// Result of a V1 bd batch import operation.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct ImportBdV1Result {
    pub meta_task_id: String,
    pub imported_count: usize,
    pub skipped_count: usize,
    pub items: Vec<ImportBdV1ItemResult>,
}

impl ImportBdV1Result {
    /// Returns a concise, deterministic human-readable summary of the import.
    pub fn human_summary(&self) -> String {
        let mut summary = format!(
            "imported {} subtask(s) into {} (skipped {})",
            self.imported_count, self.meta_task_id, self.skipped_count
        );
        if self.skipped_count > 0 {
            let mut duplicate_count = 0usize;
            let mut invalid_count = 0usize;
            for item in &self.items {
                match item.skip_reason {
                    Some(ImportBdV1SkipReason::DeterministicDuplicate) => duplicate_count += 1,
                    Some(ImportBdV1SkipReason::InvalidRow { .. }) => invalid_count += 1,
                    None => {}
                }
            }
            let mut reasons = Vec::new();
            if duplicate_count > 0 {
                reasons.push(format!("{} duplicate", duplicate_count));
            }
            if invalid_count > 0 {
                reasons.push(format!("{} invalid", invalid_count));
            }
            if !reasons.is_empty() {
                summary.push_str(&format!(": {}", reasons.join(", ")));
            }
        }
        summary
    }
}

/// Per-item outcome for a V1 bd import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportBdV1ItemResult {
    pub source_issue_id: String,
    pub subtask_id: Option<String>,
    pub skip_reason: Option<ImportBdV1SkipReason>,
}

/// Reasons a bd issue may be skipped during V1 import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportBdV1SkipReason {
    DeterministicDuplicate,
    InvalidRow { detail: String },
}

/// Request to import one OpenSpec change into Covey task state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportOpenSpecReq {
    pub session_token: Option<String>,
    pub change_id: String,
    pub project_root: String,
    pub dry_run: bool,
}

/// Result of an OpenSpec import or dry-run import plan.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportOpenSpecResult {
    pub change_id: String,
    pub meta_task_id: String,
    pub dry_run: bool,
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub conflicts: Vec<ImportOpenSpecConflict>,
    pub items: Vec<ImportOpenSpecItemResult>,
}

impl ImportOpenSpecResult {
    /// Returns a concise, deterministic human-readable summary of the import.
    pub fn human_summary(&self) -> String {
        format!(
            "openspec import {} meta_task={} created={} updated={} unchanged={} conflicts={} dry_run={}",
            self.change_id,
            self.meta_task_id,
            self.created,
            self.updated,
            self.unchanged,
            self.conflicts.len(),
            self.dry_run
        )
    }
}

/// Per-logical-record outcome for an OpenSpec import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportOpenSpecItemResult {
    pub object_type: ObjectType,
    pub object_id: String,
    pub openspec_task_id: Option<String>,
    pub title: Option<String>,
    pub task_type: Option<String>,
    pub task_digest: Option<String>,
    pub source_path: Option<String>,
    pub action: ImportOpenSpecAction,
}

/// Import action assigned to one logical OpenSpec/Covey record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ImportOpenSpecAction {
    Created,
    Updated,
    Unchanged,
    Conflict,
}

/// Conflict reported by the OpenSpec importer without mutating unsafe records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportOpenSpecConflict {
    pub object_type: ObjectType,
    pub object_id: String,
    pub openspec_task_id: Option<String>,
    pub reason: String,
    pub source_path: String,
    pub task_digest: Option<String>,
}

/// Persisted provenance for records imported from OpenSpec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenSpecImportProvenance {
    pub object_type: ObjectType,
    pub object_id: String,
    pub planning_format: String,
    pub openspec_change_id: String,
    pub openspec_change_path: String,
    pub openspec_task_id: Option<String>,
    pub proposal_digest: Option<String>,
    pub design_digest: Option<String>,
    pub tasks_digest: String,
    pub spec_digests: Vec<OpenSpecSourceDigest>,
    pub source_digests: Vec<OpenSpecSourceDigest>,
    pub mission_artifact_digests: Vec<OpenSpecSourceDigest>,
    pub mission_artifacts: Vec<String>,
    pub task_digest: Option<String>,
    pub updated_at: TimestampMs,
}

/// Digest for one OpenSpec source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct OpenSpecSourceDigest {
    pub path: String,
    pub digest: String,
}

/// Event payload emitted for OpenSpec import creates and updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportOpenSpecEvent {
    pub change_id: String,
    pub object_type: ObjectType,
    pub object_id: String,
    pub openspec_task_id: Option<String>,
    pub action: ImportOpenSpecAction,
    pub provenance: OpenSpecImportProvenance,
}

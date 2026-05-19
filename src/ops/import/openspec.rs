use std::time::Instant;

mod apply;
mod diff;
mod ids;
mod mission;
#[cfg(test)]
mod parse;
mod provenance;
mod source;
#[cfg(test)]
mod tests;
mod util;

use crate::{
    Covey, SessionRole,
    error::{CoveyError, Result},
    model::{
        ImportOpenSpecAction, ImportOpenSpecReq, ImportOpenSpecResult, ObjectType,
        OpenSpecImportProvenance, OpenSpecSourceDigest,
    },
    queries::load_import_provenance_tx,
    validators::{MAX_OBJECT_ID_LEN, MAX_PATH_LEN, ensure_length, require_role},
};

const OPENSPEC_PLANNING_FORMAT: &str = "openspec";
const OPENSPEC_ACTIVE_CLAIM_CONFLICT: &str = "active_claim_changed_source";

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenSpecSourceSnapshot {
    change_id: String,
    change_path: String,
    tasks: Vec<OpenSpecSourceTask>,
    proposal_digest: String,
    design_digest: String,
    tasks_digest: String,
    spec_digests: Vec<OpenSpecSourceDigest>,
    source_digests: Vec<OpenSpecSourceDigest>,
    mission_artifact_digests: Vec<OpenSpecSourceDigest>,
    mission_artifacts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenSpecSourceTask {
    task_id: String,
    title: String,
    source_path: String,
    task_digest: String,
    task_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenSpecImportDiff {
    result: ImportOpenSpecResult,
    records: Vec<OpenSpecImportRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenSpecImportRecord {
    object_type: ObjectType,
    object_id: String,
    openspec_task_id: Option<String>,
    title: Option<String>,
    action: ImportOpenSpecAction,
    provenance: OpenSpecImportProvenance,
}

impl Covey {
    /// Returns persisted OpenSpec import provenance for one imported record when present.
    pub fn import_provenance(
        &self,
        object_type: ObjectType,
        object_id: &str,
    ) -> Result<Option<OpenSpecImportProvenance>> {
        let started_at = Instant::now();
        ensure_length("object_id", object_id, MAX_OBJECT_ID_LEN)?;
        let result = self.with_read_tx(|tx| load_import_provenance_tx(tx, object_type, object_id));
        self.log_operation("import_provenance", "system", started_at, &result, |_| {
            vec![format!("{}:{object_id}", object_type.to_string())]
        });
        result
    }

    /// Imports an OpenSpec change into deterministic Covey meta-task and work-subtask records.
    ///
    /// This importer is intentionally non-orchestrating: it creates or updates available work
    /// records and provenance only. It never claims work, schedules workers, enqueues apply work,
    /// or mutates the OpenSpec source files.
    pub fn import_openspec(&self, req: ImportOpenSpecReq) -> Result<ImportOpenSpecResult> {
        let started_at = Instant::now();
        ensure_length("project_root", &req.project_root, MAX_PATH_LEN)?;
        ensure_length("change_id", &req.change_id, MAX_OBJECT_ID_LEN)?;
        ids::validate_openspec_change_id(&req.change_id)?;
        let source = source::load_openspec_source_snapshot(&req.project_root, &req.change_id)?;

        let result = if req.dry_run {
            self.with_read_tx(|tx| {
                diff::build_openspec_import_diff_tx(tx, &source, true).map(|d| d.result)
            })
        } else {
            let session_token = req.session_token.as_deref().ok_or_else(|| {
                CoveyError::InvalidImportDestination {
                    reason: "write mode requires --session-token".to_owned(),
                }
            })?;
            self.with_write_tx(|tx, now| {
                require_role(tx, session_token, &[SessionRole::Orchestrator])?;
                let diff = diff::build_openspec_import_diff_tx(tx, &source, false)?;
                if diff.result.conflicts.is_empty() {
                    apply::apply_openspec_import_diff_tx(tx, session_token, &diff.records, now)?;
                }
                Ok(diff.result)
            })
        };

        self.log_operation(
            "import_openspec",
            req.session_token.as_deref().unwrap_or("dry-run"),
            started_at,
            &result,
            |result| {
                let mut affected = vec![format!("meta_task:{}", result.meta_task_id)];
                affected.extend(
                    result
                        .items
                        .iter()
                        .filter(|item| item.object_type() == ObjectType::Subtask)
                        .map(|item| format!("subtask:{}", item.object_id)),
                );
                affected
            },
        );
        result
    }
}

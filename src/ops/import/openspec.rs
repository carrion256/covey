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
        ImportOpenSpecAction, ImportOpenSpecReq, ImportOpenSpecResult, MetaTaskId, ObjectType,
        OpenSpecChangeId, OpenSpecDigest, OpenSpecImportProvenance, OpenSpecPath,
        OpenSpecSourceDigest, OpenSpecTaskId, PromptText, SubtaskId, SubtaskTitle,
        object_type_name,
    },
    queries::load_import_provenance_tx,
    validators::{MAX_OBJECT_ID_LEN, MAX_PATH_LEN, ensure_length, require_role},
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenSpecSourceSnapshot {
    change_id: OpenSpecChangeId,
    change_path: OpenSpecPath,
    tasks: Vec<OpenSpecSourceTask>,
    proposal_digest: OpenSpecDigest,
    design_digest: OpenSpecDigest,
    tasks_digest: OpenSpecDigest,
    spec_digests: Vec<OpenSpecSourceDigest>,
    source_digests: Vec<OpenSpecSourceDigest>,
    mission_artifact_digests: Vec<OpenSpecSourceDigest>,
    mission_artifacts: Vec<OpenSpecPath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenSpecSourceTask {
    task_id: OpenSpecTaskId,
    title: SubtaskTitle,
    source_path: OpenSpecPath,
    task_digest: OpenSpecDigest,
    task_type: Option<OpenSpecTaskType>,
    scenario_refs: Vec<OpenSpecScenarioRef>,
    dependencies: Vec<OpenSpecDependencyRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenSpecImportDiff {
    result: ImportOpenSpecResult,
    records: Vec<OpenSpecImportRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenSpecImportRecord {
    object: OpenSpecImportRecordObject,
    action: ImportOpenSpecAction,
    provenance: OpenSpecImportProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OpenSpecImportRecordObject {
    MetaTask {
        object_id: MetaTaskId,
        title: PromptText,
    },
    Subtask {
        object_id: SubtaskId,
        openspec_task_id: OpenSpecTaskId,
        title: SubtaskTitle,
        source_path: OpenSpecPath,
        scenario_refs: Vec<OpenSpecScenarioRef>,
        dependencies: Vec<OpenSpecDependencyRef>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenSpecTaskType(String);

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenSpecDependencyRef(String);

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenSpecScenarioRef(String);

impl OpenSpecSourceSnapshot {
    #[allow(clippy::too_many_arguments)]
    fn try_from_raw_parts(
        change_id: String,
        change_path: String,
        tasks: Vec<OpenSpecSourceTask>,
        proposal_digest: String,
        design_digest: String,
        tasks_digest: String,
        spec_digests: Vec<OpenSpecSourceDigest>,
        source_digests: Vec<OpenSpecSourceDigest>,
        mission_artifact_digests: Vec<OpenSpecSourceDigest>,
        mission_artifacts: Vec<OpenSpecPath>,
    ) -> Result<Self> {
        Ok(Self {
            change_id: OpenSpecChangeId::parse(change_id).map_err(|err| {
                CoveyError::InvalidSourceSchema {
                    path: "OpenSpec source".to_owned(),
                    detail: err.to_string(),
                }
            })?,
            change_path: OpenSpecPath::parse(change_path).map_err(|detail| {
                CoveyError::InvalidSourceSchema {
                    path: "OpenSpec source".to_owned(),
                    detail,
                }
            })?,
            tasks,
            proposal_digest: OpenSpecDigest::parse(proposal_digest).map_err(|err| {
                CoveyError::InvalidSourceSchema {
                    path: "OpenSpec proposal".to_owned(),
                    detail: err.to_string(),
                }
            })?,
            design_digest: OpenSpecDigest::parse(design_digest).map_err(|err| {
                CoveyError::InvalidSourceSchema {
                    path: "OpenSpec design".to_owned(),
                    detail: err.to_string(),
                }
            })?,
            tasks_digest: OpenSpecDigest::parse(tasks_digest).map_err(|err| {
                CoveyError::InvalidSourceSchema {
                    path: "OpenSpec tasks".to_owned(),
                    detail: err.to_string(),
                }
            })?,
            spec_digests,
            source_digests,
            mission_artifact_digests,
            mission_artifacts,
        })
    }
}

impl OpenSpecSourceTask {
    fn try_from_raw_parts(
        task_id: String,
        title: String,
        source_path: String,
        task_digest: String,
        task_type: Option<String>,
        scenario_refs: Vec<String>,
        dependencies: Vec<String>,
    ) -> Result<Self> {
        Ok(Self {
            task_id: OpenSpecTaskId::parse(task_id).map_err(|detail| {
                CoveyError::InvalidSourceSchema {
                    path: source_path.clone(),
                    detail,
                }
            })?,
            title: SubtaskTitle::parse(title).map_err(|err| CoveyError::InvalidSourceSchema {
                path: source_path.clone(),
                detail: err.to_string(),
            })?,
            source_path: OpenSpecPath::parse(source_path.clone()).map_err(|detail| {
                CoveyError::InvalidSourceSchema {
                    path: source_path.clone(),
                    detail,
                }
            })?,
            task_digest: OpenSpecDigest::parse(task_digest).map_err(|err| {
                CoveyError::InvalidSourceSchema {
                    path: source_path.clone(),
                    detail: err.to_string(),
                }
            })?,
            task_type: task_type.map(OpenSpecTaskType::parse).transpose()?,
            scenario_refs: scenario_refs
                .into_iter()
                .map(OpenSpecScenarioRef::parse)
                .collect::<Result<Vec<_>>>()?,
            dependencies: dependencies
                .into_iter()
                .map(OpenSpecDependencyRef::parse)
                .collect::<Result<Vec<_>>>()?,
        })
    }
}

impl OpenSpecImportRecord {
    fn meta_task(
        object_id: MetaTaskId,
        title: PromptText,
        action: ImportOpenSpecAction,
        provenance: OpenSpecImportProvenance,
    ) -> Self {
        Self {
            object: OpenSpecImportRecordObject::MetaTask { object_id, title },
            action,
            provenance,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn subtask(
        object_id: SubtaskId,
        openspec_task_id: OpenSpecTaskId,
        title: SubtaskTitle,
        source_path: OpenSpecPath,
        scenario_refs: Vec<OpenSpecScenarioRef>,
        dependencies: Vec<OpenSpecDependencyRef>,
        action: ImportOpenSpecAction,
        provenance: OpenSpecImportProvenance,
    ) -> Self {
        Self {
            object: OpenSpecImportRecordObject::Subtask {
                object_id,
                openspec_task_id,
                title,
                source_path,
                scenario_refs,
                dependencies,
            },
            action,
            provenance,
        }
    }

    fn object_type(&self) -> ObjectType {
        self.object.object_type()
    }

    fn object_id(&self) -> &str {
        self.object.object_id()
    }

    fn title(&self) -> &str {
        self.object.title()
    }

    fn openspec_task_id(&self) -> Option<&OpenSpecTaskId> {
        self.object.openspec_task_id()
    }

    fn dependencies(&self) -> &[OpenSpecDependencyRef] {
        self.object.dependencies()
    }

    fn source_path(&self) -> Option<&OpenSpecPath> {
        self.object.source_path()
    }

    fn scenario_refs(&self) -> &[OpenSpecScenarioRef] {
        self.object.scenario_refs()
    }
}

impl OpenSpecImportRecordObject {
    fn object_type(&self) -> ObjectType {
        match self {
            Self::MetaTask { .. } => ObjectType::MetaTask,
            Self::Subtask { .. } => ObjectType::Subtask,
        }
    }

    fn object_id(&self) -> &str {
        match self {
            Self::MetaTask { object_id, .. } => object_id.as_str(),
            Self::Subtask { object_id, .. } => object_id.as_str(),
        }
    }

    fn title(&self) -> &str {
        match self {
            Self::MetaTask { title, .. } => title.as_str(),
            Self::Subtask { title, .. } => title.as_str(),
        }
    }

    fn openspec_task_id(&self) -> Option<&OpenSpecTaskId> {
        match self {
            Self::MetaTask { .. } => None,
            Self::Subtask {
                openspec_task_id, ..
            } => Some(openspec_task_id),
        }
    }

    fn dependencies(&self) -> &[OpenSpecDependencyRef] {
        match self {
            Self::MetaTask { .. } => &[],
            Self::Subtask { dependencies, .. } => dependencies,
        }
    }

    fn source_path(&self) -> Option<&OpenSpecPath> {
        match self {
            Self::MetaTask { .. } => None,
            Self::Subtask { source_path, .. } => Some(source_path),
        }
    }

    fn scenario_refs(&self) -> &[OpenSpecScenarioRef] {
        match self {
            Self::MetaTask { .. } => &[],
            Self::Subtask { scenario_refs, .. } => scenario_refs,
        }
    }
}

impl OpenSpecTaskType {
    fn parse(value: String) -> Result<Self> {
        validate_normalized_import_text("task_type", value).map(Self)
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl OpenSpecDependencyRef {
    fn parse(value: String) -> Result<Self> {
        validate_normalized_import_text("dependency", value).map(Self)
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl OpenSpecScenarioRef {
    fn parse(value: String) -> Result<Self> {
        validate_normalized_import_text("scenario_ref", value).map(Self)
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_normalized_import_text(field: &str, value: String) -> Result<String> {
    if value.trim().is_empty() {
        return Err(CoveyError::InvalidSourceSchema {
            path: "compiled Better Droid mission".to_owned(),
            detail: format!("{field} must not be empty"),
        });
    }
    if value.trim() != value {
        return Err(CoveyError::InvalidSourceSchema {
            path: "compiled Better Droid mission".to_owned(),
            detail: format!("{field} must not include leading or trailing whitespace"),
        });
    }
    if value.chars().any(char::is_control) {
        return Err(CoveyError::InvalidSourceSchema {
            path: "compiled Better Droid mission".to_owned(),
            detail: format!("{field} must not contain control characters"),
        });
    }
    Ok(value)
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
            vec![format!("{}:{object_id}", object_type_name(object_type))]
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
        ensure_length("project_root", req.project_root.as_str(), MAX_PATH_LEN)?;
        ensure_length("change_id", &req.change_id, MAX_OBJECT_ID_LEN)?;
        ids::validate_openspec_change_id(&req.change_id)?;
        let source =
            source::load_openspec_source_snapshot(req.project_root.as_str(), &req.change_id)?;

        let result = if req.is_dry_run() {
            self.with_read_tx(|tx| {
                diff::build_openspec_import_diff_tx(tx, &source, true).map(|d| d.result)
            })
        } else {
            let session_token =
                req.write_session_token()
                    .ok_or_else(|| CoveyError::InvalidImportDestination {
                        reason: "write mode requires --session-token".to_owned(),
                    })?;
            self.with_write_tx(|tx, now| {
                require_role(tx, session_token, &[SessionRole::Orchestrator])?;
                let diff = diff::build_openspec_import_diff_tx(tx, &source, false)?;
                if diff.result.conflicts().is_empty() {
                    apply::apply_openspec_import_diff_tx(tx, session_token, &diff.records, now)?;
                }
                Ok(diff.result)
            })
        };

        self.log_operation(
            "import_openspec",
            req.session_token().unwrap_or("dry-run"),
            started_at,
            &result,
            |result| {
                let mut affected = vec![format!("meta_task:{}", result.meta_task_id)];
                affected.extend(
                    result
                        .items()
                        .iter()
                        .filter(|item| item.object_type() == ObjectType::Subtask)
                        .map(|item| format!("subtask:{}", item.object_id())),
                );
                affected
            },
        );
        result
    }
}

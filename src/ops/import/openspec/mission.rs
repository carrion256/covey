use std::path::Path;

use better_droid::{BetterDroidError, load_compiled_mission};

use crate::{
    error::{CoveyError, Result},
    model::{OpenSpecPath, OpenSpecSourceDigest},
};

use super::{OpenSpecSourceTask, util::normalize_relative_path};

pub(super) struct MissionPacket {
    pub(super) tasks: Vec<OpenSpecSourceTask>,
    pub(super) source_digests: Vec<OpenSpecSourceDigest>,
    pub(super) artifact_digests: Vec<OpenSpecSourceDigest>,
    pub(super) artifact_paths: Vec<OpenSpecPath>,
}

pub(super) fn load_mission_packet(
    root: &Path,
    change_id: &str,
    change_path: &Path,
) -> Result<MissionPacket> {
    let compiled = load_compiled_mission(root, change_id, change_path)
        .map_err(compiled_mission_error_to_covey)?;
    let task_source_path = normalize_relative_path(&change_path.join("tasks.md"));

    Ok(MissionPacket {
        tasks: compiled
            .tasks
            .into_iter()
            .map(|task| {
                OpenSpecSourceTask::try_from_raw_parts(
                    task.task_id,
                    task.title,
                    task_source_path.clone(),
                    task.task_digest,
                    Some(task.task_type),
                    task.scenario_ids,
                    task.dependencies,
                )
            })
            .collect::<Result<Vec<_>>>()?,
        source_digests: digest_map(compiled.source_digests)?,
        artifact_digests: digest_map(compiled.artifact_digests)?,
        artifact_paths: artifact_paths(compiled.artifact_paths)?,
    })
}

fn artifact_paths(paths: Vec<String>) -> Result<Vec<OpenSpecPath>> {
    paths
        .into_iter()
        .map(|path| {
            OpenSpecPath::parse(path).map_err(|detail| CoveyError::InvalidSourceSchema {
                path: "compiled Better Droid mission".to_owned(),
                detail,
            })
        })
        .collect()
}

fn digest_map(
    digests: std::collections::BTreeMap<String, String>,
) -> Result<Vec<OpenSpecSourceDigest>> {
    digests
        .into_iter()
        .map(|(path, digest)| {
            OpenSpecSourceDigest::new(path, digest).map_err(|detail| {
                CoveyError::InvalidSourceSchema {
                    path: "compiled Better Droid mission".to_owned(),
                    detail,
                }
            })
        })
        .collect()
}

fn compiled_mission_error_to_covey(error: BetterDroidError) -> CoveyError {
    match error {
        BetterDroidError::InvalidSource { path, detail } => {
            CoveyError::InvalidSourceSchema { path, detail }
        }
        BetterDroidError::Io { path, source } => CoveyError::InvalidSourceSchema {
            path,
            detail: format!("failed to read Better Droid mission artifact: {source}"),
        },
        BetterDroidError::OutputPathEscape { path } => CoveyError::InvalidSourceSchema {
            path,
            detail: "output path escapes mission directory".to_owned(),
        },
        BetterDroidError::Json(source) => CoveyError::InvalidSourceSchema {
            path: "compiled Better Droid mission".to_owned(),
            detail: source.to_string(),
        },
    }
}

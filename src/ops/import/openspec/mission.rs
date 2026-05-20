use std::path::Path;

use better_droid::{BetterDroidError, load_compiled_mission};

use crate::{
    error::{CoveyError, Result},
    model::OpenSpecSourceDigest,
};

use super::{OpenSpecSourceTask, util::normalize_relative_path};

pub(super) struct MissionPacket {
    pub(super) tasks: Vec<OpenSpecSourceTask>,
    pub(super) source_digests: Vec<OpenSpecSourceDigest>,
    pub(super) artifact_digests: Vec<OpenSpecSourceDigest>,
    pub(super) artifact_paths: Vec<String>,
}

pub(super) fn load_mission_packet(
    root: &Path,
    change_id: &str,
    change_path: &Path,
) -> Result<MissionPacket> {
    let compiled = load_compiled_mission(root, change_id, change_path)
        .map_err(compiled_mission_error_to_covey)?;
    let mission_source_path =
        normalize_relative_path(&change_path.join("mission").join("mission.json"));

    Ok(MissionPacket {
        tasks: compiled
            .tasks
            .into_iter()
            .map(|task| OpenSpecSourceTask {
                task_id: task.task_id,
                title: task.title,
                source_path: mission_source_path.clone(),
                task_digest: task.task_digest,
                task_type: Some(task.task_type),
                dependencies: task.dependencies,
            })
            .collect(),
        source_digests: digest_map(compiled.source_digests)?,
        artifact_digests: digest_map(compiled.artifact_digests)?,
        artifact_paths: compiled.artifact_paths,
    })
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

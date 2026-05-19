#![cfg_attr(coverage_nightly, coverage(off))]

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    error::{CoveyError, Result},
    model::OpenSpecSourceDigest,
};

use super::{
    OpenSpecSourceSnapshot,
    mission::load_mission_packet,
    util::{blake3_digest, normalize_relative_path},
};

pub(super) fn load_openspec_source_snapshot(
    project_root: &str,
    change_id: &str,
) -> Result<OpenSpecSourceSnapshot> {
    let root = Path::new(project_root);
    if !root.is_dir() {
        return Err(CoveyError::ImportSourceNotFound {
            path: project_root.to_owned(),
        });
    }

    let change_path = PathBuf::from("openspec").join("changes").join(change_id);
    let absolute_change_path = root.join(&change_path);
    if !absolute_change_path.is_dir() {
        return Err(CoveyError::InvalidSourceSchema {
            path: absolute_change_path.to_string_lossy().to_string(),
            detail: "missing OpenSpec change directory".to_owned(),
        });
    }

    let proposal_path = change_path.join("proposal.md");
    let design_path = change_path.join("design.md");
    let tasks_path = change_path.join("tasks.md");
    let proposal_text = read_required_openspec_file(root, &proposal_path)?;
    let design_text = read_required_openspec_file(root, &design_path)?;
    let tasks_text = read_required_openspec_file(root, &tasks_path)?;
    let spec_digests = load_openspec_spec_digests(root, &change_path)?;
    let proposal_digest = blake3_digest(proposal_text.as_bytes());
    let design_digest = blake3_digest(design_text.as_bytes());
    let tasks_digest = blake3_digest(tasks_text.as_bytes());
    let mission_packet = load_mission_packet(root, change_id, &change_path)?;
    validate_source_digest_binding(
        &mission_packet.source_digests,
        &[
            source_digest(
                normalize_relative_path(&proposal_path),
                proposal_digest.clone(),
            )?,
            source_digest(normalize_relative_path(&design_path), design_digest.clone())?,
            source_digest(normalize_relative_path(&tasks_path), tasks_digest.clone())?,
        ],
        &spec_digests,
    )?;

    Ok(OpenSpecSourceSnapshot {
        change_id: change_id.to_owned(),
        change_path: normalize_relative_path(&change_path),
        proposal_digest,
        design_digest,
        tasks_digest,
        tasks: mission_packet.tasks,
        spec_digests,
        source_digests: mission_packet.source_digests,
        mission_artifact_digests: mission_packet.artifact_digests,
        mission_artifacts: mission_packet.artifact_paths,
    })
}

fn validate_source_digest_binding(
    compiled: &[OpenSpecSourceDigest],
    required: &[OpenSpecSourceDigest],
    spec_digests: &[OpenSpecSourceDigest],
) -> Result<()> {
    for expected in required.iter().chain(spec_digests.iter()) {
        let Some(actual) = compiled
            .iter()
            .find(|digest| digest.path() == expected.path())
        else {
            return Err(CoveyError::InvalidSourceSchema {
                path: expected.path().to_owned(),
                detail: "compiled mission missing source digest binding".to_owned(),
            });
        };
        if actual.digest() != expected.digest() {
            return Err(CoveyError::InvalidSourceSchema {
                path: expected.path().to_owned(),
                detail: "compiled mission source digest is stale".to_owned(),
            });
        }
    }
    Ok(())
}

fn read_required_openspec_file(root: &Path, relative_path: &Path) -> Result<String> {
    let path = root.join(relative_path);
    if !path.is_file() {
        return Err(CoveyError::InvalidSourceSchema {
            path: path.to_string_lossy().to_string(),
            detail: "missing required OpenSpec file".to_owned(),
        });
    }
    fs::read_to_string(&path).map_err(|err| CoveyError::InvalidSourceSchema {
        path: path.to_string_lossy().to_string(),
        detail: format!("failed to read OpenSpec file: {err}"),
    })
}

fn load_openspec_spec_digests(
    root: &Path,
    change_path: &Path,
) -> Result<Vec<OpenSpecSourceDigest>> {
    let specs_root = root.join(change_path).join("specs");
    if !specs_root.is_dir() {
        return Err(CoveyError::InvalidSourceSchema {
            path: specs_root.to_string_lossy().to_string(),
            detail: "missing specs directory".to_owned(),
        });
    }

    let mut spec_paths = Vec::new();
    for entry in fs::read_dir(&specs_root).map_err(|err| CoveyError::InvalidSourceSchema {
        path: specs_root.to_string_lossy().to_string(),
        detail: format!("failed to read specs directory: {err}"),
    })? {
        let entry = entry.map_err(|err| CoveyError::InvalidSourceSchema {
            path: specs_root.to_string_lossy().to_string(),
            detail: format!("failed to read specs directory entry: {err}"),
        })?;
        let candidate = entry.path().join("spec.md");
        if candidate.is_file() {
            spec_paths.push(candidate);
        }
    }
    spec_paths.sort();

    if spec_paths.is_empty() {
        return Err(CoveyError::InvalidSourceSchema {
            path: specs_root.to_string_lossy().to_string(),
            detail: "missing specs/*/spec.md files".to_owned(),
        });
    }

    let mut digests = Vec::with_capacity(spec_paths.len());
    for spec_path in spec_paths {
        let bytes = fs::read(&spec_path).map_err(|err| CoveyError::InvalidSourceSchema {
            path: spec_path.to_string_lossy().to_string(),
            detail: format!("failed to read OpenSpec spec file: {err}"),
        })?;
        let relative = spec_path.strip_prefix(root).unwrap_or(&spec_path);
        digests.push(source_digest(
            normalize_relative_path(relative),
            blake3_digest(&bytes),
        )?);
    }
    Ok(digests)
}

fn source_digest(path: String, digest: String) -> Result<OpenSpecSourceDigest> {
    OpenSpecSourceDigest::new(path.clone(), digest)
        .map_err(|detail| CoveyError::InvalidSourceSchema { path, detail })
}

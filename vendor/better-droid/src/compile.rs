use crate::error::{BetterDroidError, Result};
use crate::lint::lint_source;
use crate::mission::build_artifacts;
use crate::model::{ARTIFACT_NAMES, CompileOptions};
use crate::report::{MissionReport, report_from_lint};
use crate::source::{
    artifact_relative_path, canonical_json_digest, load_source, resolve_output_dir,
};
use std::collections::BTreeMap;
use std::fs;

/// Compile a Better Droid OpenSpec change into canonical JSON artifacts.
///
/// # Errors
///
/// Returns an error when source files cannot be read, output confinement fails, or JSON cannot be
/// serialized.
pub fn compile_change(options: &CompileOptions) -> Result<MissionReport> {
    let source = load_source(&options.project_root, &options.change_id)?;
    let lint = lint_source(&source);

    if lint.has_blockers() {
        return Ok(report_from_lint(&source, lint, Vec::new(), BTreeMap::new()));
    }

    let output_dir =
        resolve_output_dir(&options.project_root, &source, options.output_dir.as_ref())?;
    fs::create_dir_all(&output_dir).map_err(|source_error| BetterDroidError::Io {
        path: output_dir.display().to_string(),
        source: source_error,
    })?;

    let artifacts = build_artifacts(&source, &lint);
    let mut created = Vec::with_capacity(ARTIFACT_NAMES.len());
    let mut artifact_digests = BTreeMap::new();

    for name in ARTIFACT_NAMES {
        if *name == "compile-report.json" {
            continue;
        }
        let value = artifacts
            .get(*name)
            .expect("artifact name must exist in compiled artifact map");
        let canonical_digest = canonical_json_digest(value)?;
        let artifact_path = output_dir.join(name);
        let bytes = serde_json::to_vec_pretty(value)?;
        fs::write(&artifact_path, bytes).map_err(|source_error| BetterDroidError::Io {
            path: artifact_path.display().to_string(),
            source: source_error,
        })?;
        let relative_path = artifact_relative_path(&options.project_root, &artifact_path);
        artifact_digests.insert(relative_path.clone(), canonical_digest);
        created.push(relative_path);
    }

    let report_path = output_dir.join("compile-report.json");
    let report_relative_path = artifact_relative_path(&options.project_root, &report_path);
    let report_without_self_digest = report_from_lint(
        &source,
        lint.clone(),
        {
            let mut paths = created.clone();
            paths.push(report_relative_path.clone());
            paths
        },
        artifact_digests.clone(),
    );
    let report_value_without_self_digest = serde_json::to_value(&report_without_self_digest)?;
    artifact_digests.insert(
        report_relative_path.clone(),
        canonical_json_digest(&report_value_without_self_digest)?,
    );
    let report = report_from_lint(
        &source,
        lint,
        {
            let mut paths = created.clone();
            paths.push(report_relative_path);
            paths
        },
        artifact_digests,
    );
    fs::write(&report_path, serde_json::to_vec_pretty(&report)?).map_err(|source_error| {
        BetterDroidError::Io {
            path: report_path.display().to_string(),
            source: source_error,
        }
    })?;
    Ok(report)
}

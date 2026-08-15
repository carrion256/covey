use std::{
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use serde::Serialize;

use crate::{LintOptions, MissionReport, ReportStatus, Result, lint_change};

/// Inputs for Better Droid/OpenSpec readiness comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorOptions {
    pub project_root: PathBuf,
    pub change_id: String,
    pub openspec_bin: PathBuf,
}

/// Better Droid doctor report comparing OpenSpec structural readiness with mission readiness.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DoctorReport {
    pub schema: &'static str,
    pub change_id: String,
    pub status: DoctorStatus,
    pub import_ready: bool,
    pub checks: Vec<DoctorCheck>,
    pub better_droid: MissionReport,
}

/// Coarse doctor result status.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    PlanningReady,
    PlanningBlocked,
    CoveyImportReady,
    Blocked,
}

impl std::fmt::Display for DoctorStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PlanningReady => formatter.write_str("planning_ready"),
            Self::PlanningBlocked => formatter.write_str("planning_ready_blocked"),
            Self::CoveyImportReady => formatter.write_str("covey_import_ready"),
            Self::Blocked => formatter.write_str("blocked"),
        }
    }
}

impl DoctorStatus {
    #[must_use]
    pub const fn from_report_status(status: ReportStatus, openspec_ready: bool) -> Self {
        if !openspec_ready {
            return Self::Blocked;
        }
        match status {
            ReportStatus::PlanningReady => Self::PlanningReady,
            ReportStatus::PlanningBlocked => Self::PlanningBlocked,
            ReportStatus::CoveyImportReady => Self::CoveyImportReady,
            ReportStatus::Blocked => Self::Blocked,
        }
    }
}

/// One doctor check result.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DoctorCheck {
    pub id: String,
    pub status: DoctorCheckStatus,
    pub command: Option<Vec<String>>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub detail: String,
}

/// Pass/fail status for a doctor check.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorCheckStatus {
    Passed,
    Failed,
}

/// Compare OpenSpec structural readiness with Better Droid mission readiness.
///
/// # Errors
///
/// Returns a Better Droid error when mission source cannot be loaded or linted.
pub fn doctor_change(options: DoctorOptions) -> Result<DoctorReport> {
    let DoctorOptions {
        project_root,
        change_id,
        openspec_bin,
    } = options;
    let mut checks = Vec::new();
    let schema_path = project_root
        .join("openspec")
        .join("schemas")
        .join("better-droid")
        .join("schema.yaml");
    checks.push(DoctorCheck {
        id: "root_better_droid_schema".to_owned(),
        status: if schema_path.is_file() {
            DoctorCheckStatus::Passed
        } else {
            DoctorCheckStatus::Failed
        },
        command: None,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        detail: format!(
            "{} {}",
            schema_path.display(),
            if schema_path.is_file() {
                "exists"
            } else {
                "is missing"
            }
        ),
    });

    checks.push(run_openspec_check(
        &project_root,
        &openspec_bin,
        "openspec_schema_which_better_droid",
        &["schema", "which", "better-droid"],
    ));
    checks.push(run_openspec_check(
        &project_root,
        &openspec_bin,
        "openspec_validate_strict",
        &[
            "validate", &change_id, "--type", "change", "--strict", "--json",
        ],
    ));
    checks.push(run_openspec_check(
        &project_root,
        &openspec_bin,
        "openspec_status",
        &["status", "--change", &change_id, "--json"],
    ));

    let better_droid = lint_change(&LintOptions {
        project_root,
        change_id: change_id.clone(),
    })?;
    checks.push(DoctorCheck {
        id: "better_droid_lint".to_owned(),
        status: if better_droid.import_ready {
            DoctorCheckStatus::Passed
        } else {
            DoctorCheckStatus::Failed
        },
        command: None,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        detail: format!("lint status: {}", better_droid.status),
    });

    let openspec_ready = checks
        .iter()
        .filter(|check| check.id.starts_with("openspec_") || check.id == "root_better_droid_schema")
        .all(|check| check.status == DoctorCheckStatus::Passed);
    checks.push(DoctorCheck {
        id: "readiness_alignment".to_owned(),
        status: if openspec_ready == better_droid.import_ready {
            DoctorCheckStatus::Passed
        } else {
            DoctorCheckStatus::Failed
        },
        command: None,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        detail: format!(
            "openspec_ready={openspec_ready}; better_droid_import_ready={}",
            better_droid.import_ready
        ),
    });

    let import_ready = checks
        .iter()
        .all(|check| check.status == DoctorCheckStatus::Passed)
        && better_droid.import_ready;
    Ok(DoctorReport {
        schema: "better-droid.doctor-report.v1",
        change_id,
        status: DoctorStatus::from_report_status(better_droid.status, openspec_ready),
        import_ready,
        checks,
        better_droid,
    })
}

#[must_use]
pub fn run_openspec_check(
    project_root: &Path,
    openspec_bin: &Path,
    id: &str,
    args: &[&str],
) -> DoctorCheck {
    let command = std::iter::once(openspec_bin.display().to_string())
        .chain(args.iter().map(|arg| (*arg).to_owned()))
        .collect::<Vec<_>>();
    match ProcessCommand::new(openspec_bin)
        .args(args)
        .current_dir(project_root)
        .output()
    {
        Ok(output) => DoctorCheck {
            id: id.to_owned(),
            status: if output.status.success() {
                DoctorCheckStatus::Passed
            } else {
                DoctorCheckStatus::Failed
            },
            command: Some(command),
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            detail: if output.status.success() {
                "openspec command succeeded".to_owned()
            } else {
                "openspec command failed".to_owned()
            },
        },
        Err(error) => DoctorCheck {
            id: id.to_owned(),
            status: DoctorCheckStatus::Failed,
            command: Some(command),
            exit_code: None,
            stdout: String::new(),
            stderr: error.to_string(),
            detail: "failed to execute openspec command".to_owned(),
        },
    }
}

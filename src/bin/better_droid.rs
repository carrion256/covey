#![allow(unexpected_cfgs)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![cfg_attr(coverage_nightly, coverage(off))]

use std::{
    ffi::OsString,
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::{Command as ProcessCommand, ExitCode},
};

use clap::{Parser, Subcommand};
use covey::better_droid::{
    BetterDroidError, CompileOptions, LintOptions, MissionReport, compile_change, lint_change,
};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "better-droid",
    about = "Better Droid mission lint and compile tools",
    disable_help_subcommand = true
)]
struct Cli {
    #[arg(long, global = true, default_value = ".")]
    project_root: PathBuf,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate a Better Droid OpenSpec mission without writing mission artifacts.
    Lint {
        /// OpenSpec change id under openspec/changes/.
        change_id: String,
    },
    /// Compile a Better Droid OpenSpec mission into canonical JSON artifacts.
    Compile {
        /// OpenSpec change id under openspec/changes/.
        change_id: String,
        /// Optional output directory. It must stay inside openspec/changes/CHANGE_ID/mission.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Compare root OpenSpec readiness with Better Droid mission readiness.
    Doctor {
        /// OpenSpec change id under openspec/changes/.
        change_id: String,
        /// OpenSpec executable to use. Defaults to PATH lookup for `openspec`.
        #[arg(long, default_value = "openspec")]
        openspec_bin: PathBuf,
    },
}

fn main() -> ExitCode {
    match run(std::env::args_os().collect()) {
        Ok(code) => code,
        Err(error) => error.render(),
    }
}

fn run(raw_args: Vec<OsString>) -> Result<ExitCode, CliError> {
    let mode = OutputMode::resolve(&raw_args);
    let cli = match Cli::try_parse_from(raw_args) {
        Ok(cli) => cli,
        Err(error) => {
            if error.kind() == clap::error::ErrorKind::DisplayHelp && mode == OutputMode::Human {
                let _ = error.print();
                return Ok(ExitCode::SUCCESS);
            }
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                render_success(
                    mode,
                    &HelpPayload {
                        help: error.to_string(),
                    },
                    error.to_string(),
                );
                return Ok(ExitCode::SUCCESS);
            }

            return Err(CliError {
                mode,
                exit_code: clap_exit_code(error.kind()),
                code: "invalid_args",
                message: error.to_string(),
            });
        }
    };

    match cli.command {
        Command::Lint { change_id } => {
            let report = lint_change(&LintOptions {
                project_root: cli.project_root,
                change_id,
            })
            .map_err(|error| CliError::from_better_droid(mode, error))?;
            render_success(mode, &report, format!("lint status: {}", report.status));
            Ok(ExitCode::SUCCESS)
        }
        Command::Compile { change_id, output } => {
            let report = compile_change(&CompileOptions {
                project_root: cli.project_root,
                change_id,
                output_dir: output,
            })
            .map_err(|error| CliError::from_better_droid(mode, error))?;
            let exit = if report.import_ready {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(4)
            };
            render_success(mode, &report, format!("compile status: {}", report.status));
            Ok(exit)
        }
        Command::Doctor {
            change_id,
            openspec_bin,
        } => {
            let report = doctor_change(cli.project_root, change_id, openspec_bin)
                .map_err(|error| CliError::from_better_droid(mode, error))?;
            let exit = if report.import_ready {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(4)
            };
            render_success(mode, &report, format!("doctor status: {}", report.status));
            Ok(exit)
        }
    }
}

fn doctor_change(
    project_root: PathBuf,
    change_id: String,
    openspec_bin: PathBuf,
) -> Result<DoctorReport, BetterDroidError> {
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
        &["validate", &change_id, "--strict", "--json"],
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
        status: if import_ready {
            DoctorStatus::Ready
        } else {
            DoctorStatus::Blocked
        },
        import_ready,
        checks,
        better_droid,
    })
}

fn run_openspec_check(
    project_root: &std::path::Path,
    openspec_bin: &std::path::Path,
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

#[derive(Debug, Serialize, PartialEq, Eq)]
struct DoctorReport {
    schema: &'static str,
    change_id: String,
    status: DoctorStatus,
    import_ready: bool,
    checks: Vec<DoctorCheck>,
    better_droid: MissionReport,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DoctorStatus {
    Ready,
    Blocked,
}

impl std::fmt::Display for DoctorStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready => formatter.write_str("ready"),
            Self::Blocked => formatter.write_str("blocked"),
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct DoctorCheck {
    id: String,
    status: DoctorCheckStatus,
    command: Option<Vec<String>>,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DoctorCheckStatus {
    Passed,
    Failed,
}

fn clap_exit_code(kind: clap::error::ErrorKind) -> u8 {
    match kind {
        clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => 0,
        _ => 2,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputMode {
    Human,
    Json,
}

impl OutputMode {
    fn resolve(raw_args: &[OsString]) -> Self {
        if raw_args.iter().any(|arg| arg == "--json") || !io::stdout().is_terminal() {
            Self::Json
        } else {
            Self::Human
        }
    }
}

#[derive(Debug)]
struct CliError {
    mode: OutputMode,
    exit_code: u8,
    code: &'static str,
    message: String,
}

impl CliError {
    fn from_better_droid(mode: OutputMode, error: BetterDroidError) -> Self {
        let (exit_code, code) = match error {
            BetterDroidError::Io { .. } | BetterDroidError::Json(_) => (5, "internal_error"),
            BetterDroidError::InvalidSource { .. } => (2, "invalid_args"),
            BetterDroidError::OutputPathEscape { .. } => (2, "output_path_escape"),
        };
        Self {
            mode,
            exit_code,
            code,
            message: error.to_string(),
        }
    }

    fn render(&self) -> ExitCode {
        match self.mode {
            OutputMode::Json => {
                write_json_stderr(&ErrorEnvelope {
                    ok: false,
                    code: self.code,
                    message: self.message.clone(),
                });
            }
            OutputMode::Human => {
                let _ = writeln!(io::stderr(), "{}: {}", self.code, self.message);
            }
        }
        ExitCode::from(self.exit_code)
    }
}

#[derive(Debug, Serialize)]
struct SuccessEnvelope<'a, T> {
    ok: bool,
    data: &'a T,
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    ok: bool,
    code: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
struct HelpPayload {
    help: String,
}

fn render_success<T: Serialize>(mode: OutputMode, value: &T, human: String) {
    match mode {
        OutputMode::Json => write_json_stdout(&SuccessEnvelope {
            ok: true,
            data: value,
        }),
        OutputMode::Human => {
            let _ = writeln!(io::stdout(), "{human}");
        }
    }
}

fn write_json_stdout<T: Serialize>(value: &T) {
    let mut stdout = io::stdout();
    let _ = serde_json::to_writer(&mut stdout, value);
    let _ = stdout.write_all(b"\n");
}

fn write_json_stderr<T: Serialize>(value: &T) {
    let mut stderr = io::stderr();
    let _ = serde_json::to_writer(&mut stderr, value);
    let _ = stderr.write_all(b"\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(clap::error::ErrorKind::DisplayHelp, 0)]
    #[case(clap::error::ErrorKind::DisplayVersion, 0)]
    #[case(clap::error::ErrorKind::InvalidSubcommand, 2)]
    #[case(clap::error::ErrorKind::UnknownArgument, 2)]
    fn clap_exit_code_maps_help_version_and_errors(
        #[case] kind: clap::error::ErrorKind,
        #[case] expected: u8,
    ) {
        assert_eq!(clap_exit_code(kind), expected);
    }

    #[test]
    fn output_mode_resolve_prefers_json_for_explicit_flag_or_piped_stdout() {
        assert_eq!(
            OutputMode::resolve(&[OsString::from("better-droid"), OsString::from("--json")]),
            OutputMode::Json
        );
        let expected_default = if io::stdout().is_terminal() {
            OutputMode::Human
        } else {
            OutputMode::Json
        };
        assert_eq!(
            OutputMode::resolve(&[OsString::from("better-droid")]),
            expected_default
        );
    }

    #[test]
    fn doctor_status_display_and_failed_openspec_execution_are_stable() {
        assert_eq!(DoctorStatus::Ready.to_string(), "ready");
        assert_eq!(DoctorStatus::Blocked.to_string(), "blocked");

        let check = run_openspec_check(
            std::path::Path::new("."),
            std::path::Path::new("/definitely/missing/openspec"),
            "unit_openspec",
            &["--version"],
        );
        assert_eq!(check.id, "unit_openspec");
        assert_eq!(check.status, DoctorCheckStatus::Failed);
        assert!(check.exit_code.is_none());
        assert!(check.command.is_some());
        assert!(!check.stderr.is_empty());
    }

    #[test]
    fn run_maps_parse_errors_help_and_command_failures() {
        let help = run(vec![
            OsString::from("better-droid"),
            OsString::from("--json"),
            OsString::from("--help"),
        ])
        .expect("help should render successfully");
        assert_eq!(help, ExitCode::SUCCESS);

        let invalid = run(vec![
            OsString::from("better-droid"),
            OsString::from("--json"),
            OsString::from("--bogus"),
        ])
        .expect_err("unknown argument should become CLI error");
        assert_eq!(invalid.mode, OutputMode::Json);
        assert_eq!(invalid.exit_code, 2);
        assert_eq!(invalid.code, "invalid_args");

        let missing_lint = run(vec![
            OsString::from("better-droid"),
            OsString::from("--json"),
            OsString::from("--project-root"),
            OsString::from("/definitely/missing/mutai/project"),
            OsString::from("lint"),
            OsString::from("missing-change"),
        ])
        .expect_err("missing lint source should become CLI error");
        assert_eq!(missing_lint.mode, OutputMode::Json);
        assert_eq!(missing_lint.exit_code, 2);
        assert_eq!(missing_lint.code, "invalid_args");

        let missing_compile = run(vec![
            OsString::from("better-droid"),
            OsString::from("--json"),
            OsString::from("--project-root"),
            OsString::from("/definitely/missing/mutai/project"),
            OsString::from("compile"),
            OsString::from("missing-change"),
        ])
        .expect_err("missing compile source should become CLI error");
        assert_eq!(missing_compile.mode, OutputMode::Json);
        assert_eq!(missing_compile.exit_code, 2);
        assert_eq!(missing_compile.code, "invalid_args");
    }

    #[rstest]
    #[case(
        BetterDroidError::InvalidSource {
            path: "openspec/changes/demo".into(),
            detail: "missing tasks".into(),
        },
        2,
        "invalid_args"
    )]
    #[case(
        BetterDroidError::OutputPathEscape {
            path: "../escape".into(),
        },
        2,
        "output_path_escape"
    )]
    #[case(
        BetterDroidError::Io {
            path: "source.md".into(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        },
        5,
        "internal_error"
    )]
    fn better_droid_errors_map_to_cli_reports(
        #[case] error: BetterDroidError,
        #[case] exit_code: u8,
        #[case] code: &'static str,
    ) {
        let cli_error = CliError::from_better_droid(OutputMode::Json, error);

        assert_eq!(cli_error.mode, OutputMode::Json);
        assert_eq!(cli_error.exit_code, exit_code);
        assert_eq!(cli_error.code, code);
        assert!(!cli_error.message.is_empty());
    }

    #[test]
    fn render_helpers_cover_human_and_json_paths() {
        render_success(
            OutputMode::Human,
            &HelpPayload {
                help: "usage".into(),
            },
            "human usage".into(),
        );
        render_success(
            OutputMode::Json,
            &HelpPayload {
                help: "usage".into(),
            },
            "json usage".into(),
        );

        let human_error = CliError {
            mode: OutputMode::Human,
            exit_code: 2,
            code: "invalid_args",
            message: "bad args".into(),
        };
        let json_error = CliError {
            mode: OutputMode::Json,
            exit_code: 5,
            code: "internal_error",
            message: "boom".into(),
        };
        assert_eq!(human_error.render(), ExitCode::from(2));
        assert_eq!(json_error.render(), ExitCode::from(5));
    }
}

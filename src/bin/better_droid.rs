#![allow(unexpected_cfgs)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![cfg_attr(coverage_nightly, coverage(off))]

use std::{
    ffi::OsString,
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use covey::better_droid::{
    BetterDroidError, CompileOptions, LintOptions, compile_change, lint_change,
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
    }
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

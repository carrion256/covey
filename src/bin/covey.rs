#![allow(unexpected_cfgs)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![cfg_attr(coverage_nightly, coverage(off))]

use std::{
    ffi::OsString,
    io::{self, IsTerminal, Write},
    process::ExitCode,
};

use clap::Parser;
use covey::{Covey, CoveyError};
use serde::Serialize;
use serde_json::Value;

mod cli;
mod dispatch_support;
mod render_support;

use cli::Cli;
use dispatch_support::dispatch;
use render_support::{
    OutputError, OutputMode, Rendered, ReportableError, exit_code_for_clap_kind, help_payload,
    print_line, render_error, render_success, resolve_output_mode, short_help_payload,
    version_payload,
};

const DEFAULT_DB_PATH: &str = "./covey.db";
const COMMAND_GROUPS: &[&str] = &[
    "session",
    "meta",
    "subtask",
    "claim",
    "artifact",
    "review",
    "queue",
    "reservation",
    "repoops",
    "events",
    "conflict",
    "maint",
    "import",
];
const EXAMPLES: &[&str] = &[
    "covey session register --agent-principal-id agent-a --agent-instance-id run-1 --role executor",
    "covey subtask claim-next --session-token session_123 --lease-duration-ms 30000",
    "covey subtask claim --session-token session_123 --subtask-id work_1 --lease-duration-ms 30000",
    "covey events list --after-seq 0 --limit 50",
];
const SHORT_HELP: &str = "\
covey <group> <command> [flags]
groups: session meta subtask claim artifact review queue reservation repoops events conflict maint import
global: --db PATH --json
examples:
  covey session register --agent-principal-id agent-a --agent-instance-id run-1 --role executor
  covey subtask claim-next --session-token session_123 --lease-duration-ms 30000
  covey subtask claim --session-token session_123 --subtask-id work_1 --lease-duration-ms 30000
  covey events list --after-seq 0 --limit 50";

fn main() -> ExitCode {
    match run(std::env::args_os().collect()) {
        Ok(code) => code,
        Err(err) => {
            render_error(err.mode, &err.report);
            ExitCode::from(err.report.exit_code)
        }
    }
}

fn run(raw_args: Vec<OsString>) -> Result<ExitCode, OutputError> {
    let explicit_json = raw_args.iter().any(|arg| arg == "--json");
    let mode = resolve_output_mode(&raw_args);
    if raw_args.iter().any(|arg| arg == "--version" || arg == "-V") {
        if mode == OutputMode::Human && !explicit_json {
            print_line(
                &format!("covey {}", env!("CARGO_PKG_VERSION")),
                &mut io::stdout(),
            );
        } else {
            render_success(
                mode,
                &Rendered::summary(version_payload(), env!("CARGO_PKG_VERSION").into()),
            );
        }
        return Ok(ExitCode::SUCCESS);
    }
    if raw_args.len() <= 1 {
        render_success(
            mode,
            &Rendered::summary(short_help_payload(), SHORT_HELP.into()),
        );
        return Ok(ExitCode::SUCCESS);
    }

    let cli = match Cli::try_parse_from(raw_args) {
        Ok(cli) => cli,
        Err(err) => {
            if matches!(
                err.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) && mode == OutputMode::Human
                && !explicit_json
            {
                let _ = err.print();
                return Ok(ExitCode::SUCCESS);
            }
            if matches!(
                err.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                let rendered = match err.kind() {
                    clap::error::ErrorKind::DisplayVersion => {
                        Rendered::summary(version_payload(), env!("CARGO_PKG_VERSION").into())
                    }
                    _ => Rendered::summary(help_payload(err.to_string().trim()), SHORT_HELP.into()),
                };
                render_success(mode, &rendered);
                return Ok(ExitCode::SUCCESS);
            }
            if mode == OutputMode::Human {
                let _ = err.print();
                return Ok(ExitCode::from(exit_code_for_clap_kind(err.kind())));
            }
            return Err(OutputError::new(
                mode,
                ReportableError::invalid_args(
                    err.to_string().trim().to_owned(),
                    vec!["Run `covey --help` for usage.".into()],
                ),
            ));
        }
    };

    let store = Covey::open(&cli.db).map_err(|err| OutputError::from_covey(mode, err))?;

    let rendered =
        dispatch(&store, cli.command).map_err(|err| OutputError::from_covey(mode, err))?;
    render_success(mode, &rendered);
    Ok(ExitCode::SUCCESS)
}

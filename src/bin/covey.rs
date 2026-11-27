#![allow(unexpected_cfgs)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![cfg_attr(coverage_nightly, coverage(off))]

use std::{
    ffi::OsString,
    io::{self, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::Parser;
use covey::{
    Covey, CoveyError,
    proof_apply::{
        apply_proof_contract, emit_apply_proof_error, verify_apply_proof, verify_apply_proof_batch,
    },
};
use serde::Serialize;
use serde_json::Value;
use tracing::level_filters::LevelFilter;

mod cli;
mod dispatch_support;
mod render_support;

use cli::{Cli, Commands, DigestBlake3Args, DigestCommand, ProofApplyCommand, ProofCommand};
use dispatch_support::dispatch;
use render_support::{
    OutputError, OutputMode, Rendered, ReportableError, exit_code_for_clap_kind, help_payload,
    print_line, render_error, render_success, resolve_output_mode, short_help_payload,
    version_line, version_payload,
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
    "digest",
    "proof",
];
const EXAMPLES: &[&str] = &[
    "covey session register --agent-principal-id agent-a --agent-instance-id run-1 --role executor",
    "covey subtask claim-next --session-token session_123 --lease-duration-ms 30000",
    "covey subtask claim --session-token session_123 --subtask-id work_1 --lease-duration-ms 30000",
    "covey events list --after-seq 0 --limit 50",
];
const SHORT_HELP: &str = "\
covey <group> <command> [flags]
groups: session meta subtask claim artifact review queue reservation repoops events conflict maint import digest
digest: covey digest blake3 --file PATH | --text TEXT | --stdin
proof: covey proof apply verify | verify-batch | print-contract
global: --db PATH --json --config PATH --profile NAME --read-only
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
    install_logging(mode)?;
    if raw_args.iter().any(|arg| arg == "--version" || arg == "-V") {
        if mode == OutputMode::Human && !explicit_json {
            print_line(&version_line(), &mut io::stdout());
        } else {
            render_success(mode, &Rendered::summary(version_payload(), version_line()));
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
                        Rendered::summary(version_payload(), version_line())
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

    if let Commands::Proof { command } = cli.command {
        return Ok(run_proof_command(command));
    }
    if let Commands::Digest { command } = cli.command {
        return run_digest_command(command, mode);
    }

    let workflow = covey::config::resolve_workflow(cli.config.as_deref(), cli.profile.as_deref())
        .map_err(|err| {
        OutputError::new(
            mode,
            ReportableError::invalid_args(
                err.to_string(),
                vec!["Check --config/--profile values.".into()],
            ),
        )
    })?;
    let (db_path, named_store) = match cli.db {
        Some(path) => (path, true),
        None => (PathBuf::from(DEFAULT_DB_PATH), false),
    };
    let plan = store_open_plan(
        &db_path,
        named_store,
        cli.read_only,
        cli.command.is_read_only_command(),
    );
    let store = match plan {
        StoreOpenPlan::ReadOnly => Covey::open_read_only_with_workflow(&db_path, workflow),
        StoreOpenPlan::RefuseMissing => Err(CoveyError::StoreNotFound {
            path: db_path.display().to_string(),
        }),
        StoreOpenPlan::OpenOrCreate => Covey::open_with_workflow(&db_path, workflow),
    }
    .map_err(|err| OutputError::from_covey(mode, err))?;

    let rendered =
        dispatch(&store, cli.command).map_err(|err| OutputError::from_covey(mode, err))?;
    render_success(mode, &rendered);
    Ok(ExitCode::SUCCESS)
}

enum StoreOpenPlan {
    OpenOrCreate,
    ReadOnly,
    RefuseMissing,
}

/// Decides how the store is opened: `--read-only` opens an existing store
/// without migrating it; a missing store is refused for a read-only command
/// and for any command when the path is the default rather than one named by
/// `--db` or `COVEY_DB_PATH`; everything else opens or creates the store.
fn store_open_plan(
    db_path: &Path,
    named_store: bool,
    read_only: bool,
    read_only_command: bool,
) -> StoreOpenPlan {
    if read_only {
        StoreOpenPlan::ReadOnly
    } else if (read_only_command || !named_store) && !store_path_exists(db_path) {
        StoreOpenPlan::RefuseMissing
    } else {
        StoreOpenPlan::OpenOrCreate
    }
}

/// A path of exactly `:memory:` counts as an existing store: SQLite creates it
/// in memory.
fn store_path_exists(db_path: &Path) -> bool {
    db_path == Path::new(":memory:") || db_path.exists()
}

fn install_logging(mode: OutputMode) -> Result<(), OutputError> {
    let Ok(raw) = std::env::var("COVEY_LOG") else {
        return Ok(());
    };
    let level = match raw.as_str() {
        "off" => LevelFilter::OFF,
        "error" => LevelFilter::ERROR,
        "warn" => LevelFilter::WARN,
        "info" => LevelFilter::INFO,
        "debug" => LevelFilter::DEBUG,
        "trace" => LevelFilter::TRACE,
        other => {
            return Err(OutputError::new(
                mode,
                ReportableError::invalid_args(
                    format!("invalid COVEY_LOG value `{other}`"),
                    vec!["Set COVEY_LOG to off, error, warn, info, debug, or trace.".into()],
                ),
            ));
        }
    };
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(io::stderr)
        .with_max_level(level)
        .try_init();
    Ok(())
}

fn run_proof_command(command: ProofCommand) -> ExitCode {
    let result = match command {
        ProofCommand::Apply { command } => match command {
            ProofApplyCommand::Verify(args) => verify_apply_proof(args),
            ProofApplyCommand::VerifyBatch(args) => verify_apply_proof_batch(args),
            ProofApplyCommand::PrintContract => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&apply_proof_contract())
                        .expect("proof contract json")
                );
                Ok(0)
            }
        },
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            let output = error.output_path();
            ExitCode::from(emit_apply_proof_error(error, output.as_deref()))
        }
    }
}

fn run_digest_command(command: DigestCommand, mode: OutputMode) -> Result<ExitCode, OutputError> {
    match command {
        DigestCommand::Blake3(args) => {
            let bytes = digest_input(args).map_err(|message| {
                OutputError::new(
                    mode,
                    ReportableError::invalid_args(
                        message,
                        vec!["Pass exactly one of --file, --text, or --stdin.".into()],
                    ),
                )
            })?;
            let digest = format!("blake3:{}", blake3::hash(&bytes).to_hex());
            render_success(
                mode,
                &Rendered::summary(
                    serde_json::json!({ "algorithm": "blake3", "digest": digest }),
                    digest,
                ),
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn digest_input(args: DigestBlake3Args) -> Result<Vec<u8>, String> {
    match (args.file, args.text, args.stdin) {
        (Some(path), None, false) => std::fs::read(&path)
            .map_err(|err| format!("failed to read `{}`: {err}", path.display())),
        (None, Some(text), false) => Ok(text.into_bytes()),
        (None, None, true) => {
            let mut bytes = Vec::new();
            io::stdin()
                .read_to_end(&mut bytes)
                .map_err(|err| format!("failed to read stdin: {err}"))?;
            Ok(bytes)
        }
        _ => Err("digest input is ambiguous or missing".into()),
    }
}

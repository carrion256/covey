use super::*;

pub(crate) fn render_success(render_mode: OutputMode, rendered: &Rendered) {
    match render_mode {
        OutputMode::Json => {
            write_json_stdout(&SuccessEnvelope {
                ok: true,
                data: rendered.data.clone(),
                warnings: Vec::new(),
            });
        }
        OutputMode::Human => {
            print_line(&rendered.human, &mut io::stdout());
        }
    }
}

pub(crate) fn render_error(render_mode: OutputMode, report: &ReportableError) {
    match render_mode {
        OutputMode::Json => {
            write_json_stderr(&ErrorEnvelope {
                ok: false,
                code: report.code,
                message: report.message.clone(),
                suggestions: report.suggestions.clone(),
            });
        }
        OutputMode::Human => {
            let mut stderr = io::stderr();
            let _ = writeln!(stderr, "{}: {}", report.code, report.message);
            for suggestion in &report.suggestions {
                let _ = writeln!(stderr, "hint: {}", suggestion);
            }
        }
    }
}

pub(crate) fn resolve_output_mode(raw_args: &[OsString]) -> OutputMode {
    if raw_args.iter().any(|arg| arg == "--json") || !io::stdout().is_terminal() {
        OutputMode::Json
    } else {
        OutputMode::Human
    }
}

pub(crate) fn exit_code_for_clap_kind(kind: clap::error::ErrorKind) -> u8 {
    match kind {
        clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => 0,
        _ => 2,
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

pub(crate) fn print_line(message: &str, writer: &mut impl Write) {
    let _ = writeln!(writer, "{message}");
}

pub(crate) fn short_help_payload() -> Value {
    serde_json::to_value(ShortHelpPayload::new()).expect("short help payload")
}

pub(crate) fn help_payload(help_text: &str) -> Value {
    serde_json::to_value(HelpPayload::new(help_text.to_owned())).expect("help payload")
}

pub(crate) fn version_payload() -> Value {
    serde_json::to_value(VersionPayload::new()).expect("version payload")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputMode {
    Human,
    Json,
}

#[derive(Debug)]
pub(crate) struct OutputError {
    pub(crate) mode: OutputMode,
    pub(crate) report: ReportableError,
}

impl OutputError {
    pub(crate) fn new(mode: OutputMode, report: ReportableError) -> Self {
        Self { mode, report }
    }

    pub(crate) fn from_covey(mode: OutputMode, error: CoveyError) -> Self {
        Self {
            mode,
            report: ReportableError::from(error),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ReportableError {
    pub(crate) exit_code: u8,
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) suggestions: Vec<String>,
}

impl ReportableError {
    pub(crate) fn invalid_args(message: impl Into<String>, suggestions: Vec<String>) -> Self {
        Self {
            exit_code: 2,
            code: "invalid_args",
            message: message.into(),
            suggestions,
        }
    }
}

impl From<CoveyError> for ReportableError {
    fn from(error: CoveyError) -> Self {
        use CoveyError::{
            ApplyGateEvidenceMissing, ApplyGateSeparationOfDutiesViolation,
            ArtifactDigestCollision, ArtifactNotFound, ClaimNotFound, ClaimNotHeld,
            ConflictNotFound, DatabaseError, DuplicateSubtaskId, FenceTokenMismatch,
            IdempotencyConflict, IllegalTransition, ImportDuplicate, ImportSourceNotFound,
            InputTooLarge, InvalidIdempotencyKey, InvalidImportDestination, InvalidImportRow,
            InvalidLeaseDuration, InvalidPath, InvalidSessionToken, InvalidSourceSchema,
            LeaseExpired, MetaTaskNotFound, MetaTaskUnavailable, MigrationError, NotClaimOwner,
            NotQueueClaimOwner, QueueItemNotFound, ReservationNotFound, ReviewAlreadyOpen,
            ReviewKindMismatch, ReviewNotFound, SeparationOfDutiesViolation, SerializationError,
            SessionAlreadyActive, SessionAlreadyHasActiveSubtask, SessionNotActive,
            SessionNotFound, StaleFenceToken, SubtaskAlreadyClaimed, SubtaskNotFound,
            UnknownArtifactDigest, WrongRole,
        };

        match error {
            SessionNotFound
            | SubtaskNotFound
            | ArtifactNotFound
            | ReviewNotFound
            | MetaTaskNotFound
            | ReservationNotFound
            | ClaimNotFound
            | QueueItemNotFound
            | ConflictNotFound
            | UnknownArtifactDigest { .. }
            | ImportSourceNotFound { .. } => Self {
                exit_code: 1,
                code: "not_found",
                message: error.to_string(),
                suggestions: vec!["Check the referenced id or digest.".into()],
            },
            InvalidLeaseDuration { .. }
            | InvalidPath { .. }
            | InputTooLarge { .. }
            | InvalidIdempotencyKey { .. }
            | InvalidSourceSchema { .. }
            | InvalidImportDestination { .. }
            | InvalidImportRow { .. } => Self {
                exit_code: 2,
                code: "invalid_args",
                message: error.to_string(),
                suggestions: vec!["Fix the request shape and retry.".into()],
            },
            WrongRole { .. }
            | NotClaimOwner { .. }
            | NotQueueClaimOwner { .. }
            | SeparationOfDutiesViolation { .. }
            | ApplyGateSeparationOfDutiesViolation { .. } => Self {
                exit_code: 3,
                code: "permission_denied",
                message: error.to_string(),
                suggestions: vec!["Use a session with the required role or ownership.".into()],
            },
            SessionAlreadyActive { .. }
            | SessionNotActive { .. }
            | SessionAlreadyHasActiveSubtask { .. }
            | InvalidSessionToken { .. }
            | IllegalTransition { .. }
            | SubtaskAlreadyClaimed { .. }
            | StaleFenceToken { .. }
            | FenceTokenMismatch
            | ClaimNotHeld { .. }
            | LeaseExpired { .. }
            | ReviewKindMismatch
            | ReviewAlreadyOpen { .. }
            | MetaTaskUnavailable { .. }
            | ArtifactDigestCollision { .. }
            | DuplicateSubtaskId { .. }
            | IdempotencyConflict { .. }
            | ImportDuplicate { .. }
            | ApplyGateEvidenceMissing { .. } => Self {
                exit_code: 4,
                code: "conflict",
                message: error.to_string(),
                suggestions: vec![
                    "Refresh state and retry with current ids, leases, and fence tokens.".into(),
                ],
            },
            DatabaseError(_) | MigrationError(_) | SerializationError(_) => Self {
                exit_code: 5,
                code: "internal_error",
                message: error.to_string(),
                suggestions: vec!["Inspect the database path and runtime logs.".into()],
            },
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Rendered {
    pub(crate) data: Value,
    pub(crate) human: String,
}

impl Rendered {
    pub(crate) fn summary<T: Serialize>(value: T, human: String) -> Self {
        Self {
            data: serde_json::to_value(value).expect("serializable result"),
            human,
        }
    }

    pub(crate) fn pretty<T: Serialize>(value: T) -> Self {
        let data = serde_json::to_value(value).expect("serializable result");
        let human = serde_json::to_string_pretty(&data).expect("pretty json");
        Self { data, human }
    }
}

#[derive(Serialize)]
pub(crate) struct SuccessEnvelope {
    pub(crate) ok: bool,
    pub(crate) data: Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct ErrorEnvelope {
    pub(crate) ok: bool,
    pub(crate) code: &'static str,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) suggestions: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct ShortHelpPayload {
    pub(crate) command: &'static str,
    pub(crate) version: &'static str,
    pub(crate) usage: &'static str,
    pub(crate) groups: &'static [&'static str],
    pub(crate) global_flags: &'static [&'static str],
    pub(crate) examples: &'static [&'static str],
}

impl ShortHelpPayload {
    fn new() -> Self {
        Self {
            command: "covey",
            version: env!("CARGO_PKG_VERSION"),
            usage: "covey <group> <command> [flags]",
            groups: COMMAND_GROUPS,
            global_flags: &["--db PATH", "--json"],
            examples: EXAMPLES,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct HelpPayload {
    pub(crate) command: &'static str,
    pub(crate) version: &'static str,
    pub(crate) help_text: String,
    pub(crate) summary: ShortHelpPayload,
}

impl HelpPayload {
    fn new(help_text: String) -> Self {
        Self {
            command: "covey",
            version: env!("CARGO_PKG_VERSION"),
            help_text,
            summary: ShortHelpPayload::new(),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct VersionPayload {
    pub(crate) command: &'static str,
    pub(crate) version: &'static str,
}

impl VersionPayload {
    fn new() -> Self {
        Self {
            command: "covey",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct SessionTokenAck {
    pub(crate) operation: &'static str,
    pub(crate) session_token: String,
}

#[derive(Serialize)]
pub(crate) struct MetaTaskRef {
    pub(crate) meta_task_id: String,
}

#[derive(Serialize)]
pub(crate) struct MetaTaskAck {
    pub(crate) operation: &'static str,
    pub(crate) meta_task_id: String,
}

#[derive(Serialize)]
pub(crate) struct SubtaskRef {
    pub(crate) subtask_id: String,
}

#[derive(Serialize)]
pub(crate) struct ClaimFenceAck {
    pub(crate) operation: &'static str,
    pub(crate) claim_id: String,
    pub(crate) fence_seq: i64,
}

#[derive(Serialize)]
pub(crate) struct ArtifactPublishAck {
    pub(crate) operation: &'static str,
    pub(crate) artifact_digest: String,
    pub(crate) artifact_kind: String,
    pub(crate) claim_id: String,
    pub(crate) fence_seq: i64,
}

#[derive(Serialize)]
pub(crate) struct ReviewRef {
    pub(crate) review_id: String,
}

#[derive(Serialize)]
pub(crate) struct ReviewDecisionAck {
    pub(crate) operation: &'static str,
    pub(crate) review_id: String,
    pub(crate) claim_id: String,
    pub(crate) fence_seq: i64,
    pub(crate) verdict: String,
}

#[derive(Serialize)]
pub(crate) struct QueueRef {
    pub(crate) queue_id: String,
}

#[derive(Serialize)]
pub(crate) struct QueueClaimAck {
    pub(crate) operation: &'static str,
    pub(crate) queue_id: String,
    pub(crate) claim_fence_seq: i64,
}

#[derive(Serialize)]
pub(crate) struct QueueOpAck {
    pub(crate) operation: &'static str,
    pub(crate) queue_id: String,
}

#[derive(Serialize)]
pub(crate) struct ReservationRef {
    pub(crate) reservation_id: String,
}

#[derive(Serialize)]
pub(crate) struct ReservationAck {
    pub(crate) operation: &'static str,
    pub(crate) reservation_id: String,
}

#[derive(Serialize)]
pub(crate) struct ConflictResolutionAck {
    pub(crate) operation: &'static str,
    pub(crate) conflict_id: String,
    pub(crate) resolution_state: String,
}

/// Render payload for a successful V1 bd batch import.
///
/// Mirrors `covey::ImportBdV1Result` with an added `operation` field so the
/// JSON envelope stays consistent with other Covey mutation acks.
#[derive(Serialize)]
pub(crate) struct ImportBdV1Ack {
    pub(crate) operation: &'static str,
    pub(crate) meta_task_id: String,
    pub(crate) imported_count: usize,
    pub(crate) skipped_count: usize,
    pub(crate) items: Vec<covey::ImportBdV1ItemResult>,
}

/// Render payload for a successful OpenSpec import or dry-run plan.
#[derive(Serialize)]
pub(crate) struct ImportOpenSpecAck {
    pub(crate) operation: &'static str,
    pub(crate) change_id: String,
    pub(crate) meta_task_id: String,
    pub(crate) dry_run: bool,
    pub(crate) created: usize,
    pub(crate) updated: usize,
    pub(crate) unchanged: usize,
    pub(crate) conflicts: Vec<covey::ImportOpenSpecConflict>,
    pub(crate) items: Vec<covey::ImportOpenSpecItemResult>,
}

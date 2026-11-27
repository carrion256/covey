use std::io::Read;

use clap::{Args, Subcommand, ValueEnum};
use covey::{
    MetaTaskAnnotationKey, MetaTaskClosure, MetaTaskListState, MetaTaskSort, MetaTaskUrgency,
    SessionRole,
};

#[derive(Subcommand, Debug)]
pub(crate) enum SessionCommand {
    Register(RegisterSessionArgs),
    Ensure(EnsureSessionArgs),
    ActiveForPrincipal(ActiveSessionForPrincipalArgs),
    Attest(RecordRuntimeAttestationArgs),
    Heartbeat(HeartbeatArgs),
    Exit(ExitSessionArgs),
    Status(SessionStatusArgs),
}

#[derive(Subcommand, Debug)]
pub(crate) enum MetaCommand {
    Submit(SubmitMetaArgs),
    Cancel(CancelMetaArgs),
    Complete(CompleteMetaArgs),
    Annotate(AnnotateMetaArgs),
    /// Set or clear the title, urgency, or due date of a meta task.
    Update(UpdateMetaArgs),
    LinkSource(LinkSourceMetaArgs),
    Status(MetaStatusArgs),
    List(MetaListArgs),
}
#[derive(Args, Debug)]
pub(crate) struct RegisterSessionArgs {
    #[arg(long)]
    pub(crate) agent_principal_id: String,
    #[arg(long)]
    pub(crate) agent_instance_id: String,
    #[arg(long)]
    pub(crate) role: SessionRoleArg,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct EnsureSessionArgs {
    #[arg(long)]
    pub(crate) agent_principal_id: String,
    #[arg(long)]
    pub(crate) agent_instance_id: String,
    #[arg(long)]
    pub(crate) role: SessionRoleArg,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ActiveSessionForPrincipalArgs {
    #[arg(long)]
    pub(crate) agent_principal_id: String,
    #[arg(long)]
    pub(crate) role: SessionRoleArg,
}

#[derive(Args, Debug)]
pub(crate) struct RecordRuntimeAttestationArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) provider: String,
    #[arg(long)]
    pub(crate) model: String,
    #[arg(long)]
    pub(crate) provider_run_id: String,
    #[arg(long)]
    pub(crate) provider_run_id_issuer: String,
    #[arg(long)]
    pub(crate) process_id: Option<String>,
    #[arg(long)]
    pub(crate) container_id: Option<String>,
    #[arg(long)]
    pub(crate) command_transcript_digest: String,
    #[arg(long)]
    pub(crate) started_at: i64,
    #[arg(long)]
    pub(crate) ended_at: i64,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct HeartbeatArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ExitSessionArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct SessionStatusArgs {
    #[arg(long)]
    pub(crate) session_token: String,
}

#[derive(Args, Debug)]
pub(crate) struct SubmitMetaArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    /// Prompt text supplied on the command line.
    #[arg(
        long,
        conflicts_with_all = ["prompt_file"],
        required_unless_present_any = ["prompt_file"]
    )]
    pub(crate) prompt_text: Option<String>,
    /// Read the prompt text from this file, or from stdin when the path is `-`.
    ///
    /// One trailing newline is stripped before the text is validated.
    #[arg(
        long,
        value_name = "PATH",
        value_parser = read_prompt_file,
        conflicts_with_all = ["prompt_text"],
        required_unless_present_any = ["prompt_text"]
    )]
    pub(crate) prompt_file: Option<String>,
    /// One-line title recorded next to the prompt text.
    #[arg(long)]
    pub(crate) title: Option<String>,
    /// Declared urgency recorded next to the prompt text.
    #[arg(long, value_enum)]
    pub(crate) urgency: Option<MetaTaskUrgencyArg>,
    /// Calendar due date recorded next to the prompt text.
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub(crate) due: Option<String>,
    /// Dedupe key scoping this task to the submitting principal.
    #[arg(long)]
    pub(crate) dedupe_key: Option<String>,
    /// Existing meta-task this submission replaces. Repeat for several.
    #[arg(long = "supersedes", value_name = "META_TASK_ID", action = clap::ArgAction::Append)]
    pub(crate) supersedes: Vec<String>,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
    #[arg(long, requires_all = ["source_received_at", "source_path"])]
    pub(crate) source_thread_id: Option<String>,
    #[arg(long, requires_all = ["source_thread_id", "source_path"])]
    pub(crate) source_received_at: Option<String>,
    #[arg(long, requires_all = ["source_thread_id", "source_received_at"])]
    pub(crate) source_path: Option<String>,
    #[arg(
        long,
        requires_all = ["source_thread_id", "source_received_at", "source_path"]
    )]
    pub(crate) source_url: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct LinkSourceMetaArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) meta_task_id: String,
    #[arg(long)]
    pub(crate) thread_id: String,
    #[arg(long)]
    pub(crate) received_at: String,
    #[arg(long)]
    pub(crate) path: String,
    #[arg(long)]
    pub(crate) url: Option<String>,
    #[arg(long)]
    pub(crate) method: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct CancelMetaArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    /// Meta-task id to cancel.
    #[arg(long, conflicts_with_all = ["handle"], required_unless_present_any = ["handle"])]
    pub(crate) meta_task_id: Option<String>,
    /// Store-issued handle to cancel, as `12` or `#12`.
    #[arg(
        long,
        value_name = "N",
        conflicts_with_all = ["meta_task_id"],
        required_unless_present_any = ["meta_task_id"]
    )]
    pub(crate) handle: Option<String>,
    /// Typed reason the task ended.
    #[arg(long, value_enum, default_value = "dropped")]
    pub(crate) closure: MetaTaskClosureArg,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct CompleteMetaArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    /// Meta-task id to complete.
    #[arg(long, conflicts_with_all = ["handle"], required_unless_present_any = ["handle"])]
    pub(crate) meta_task_id: Option<String>,
    /// Store-issued handle to complete, as `12` or `#12`.
    #[arg(
        long,
        value_name = "N",
        conflicts_with_all = ["meta_task_id"],
        required_unless_present_any = ["meta_task_id"]
    )]
    pub(crate) handle: Option<String>,
    /// Typed reason the task ended.
    #[arg(long, value_enum, default_value = "done")]
    pub(crate) closure: MetaTaskClosureArg,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct AnnotateMetaArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    /// Meta-task id to annotate.
    #[arg(long, conflicts_with_all = ["handle"], required_unless_present_any = ["handle"])]
    pub(crate) meta_task_id: Option<String>,
    /// Store-issued handle to annotate, as `12` or `#12`.
    #[arg(
        long,
        value_name = "N",
        conflicts_with_all = ["meta_task_id"],
        required_unless_present_any = ["meta_task_id"]
    )]
    pub(crate) handle: Option<String>,
    /// Set the `verdict` annotation.
    #[arg(
        long,
        conflicts_with_all = ["acked", "tier", "entity", "clear"],
        required_unless_present_any = ["acked", "tier", "entity", "clear"]
    )]
    pub(crate) verdict: Option<String>,
    /// Set the `acked` annotation.
    #[arg(
        long,
        value_name = "BOOL",
        action = clap::ArgAction::Set,
        num_args = 1,
        conflicts_with_all = ["verdict", "tier", "entity", "clear"],
        required_unless_present_any = ["verdict", "tier", "entity", "clear"]
    )]
    pub(crate) acked: Option<bool>,
    /// Set the `tier` annotation.
    #[arg(
        long,
        conflicts_with_all = ["verdict", "acked", "entity", "clear"],
        required_unless_present_any = ["verdict", "acked", "entity", "clear"]
    )]
    pub(crate) tier: Option<String>,
    /// Set the `entity` annotation.
    #[arg(
        long,
        conflicts_with_all = ["verdict", "acked", "tier", "clear"],
        required_unless_present_any = ["verdict", "acked", "tier", "clear"]
    )]
    pub(crate) entity: Option<String>,
    /// Remove one annotation key.
    #[arg(
        long,
        value_name = "KEY",
        value_enum,
        conflicts_with_all = ["verdict", "acked", "tier", "entity"],
        required_unless_present_any = ["verdict", "acked", "tier", "entity"]
    )]
    pub(crate) clear: Option<MetaTaskAnnotationKeyArg>,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
#[command(group(
    clap::ArgGroup::new("field_change")
        .required(true)
        .multiple(true)
        .args(["title", "clear_title", "urgency", "clear_urgency", "due", "clear_due"])
))]
pub(crate) struct UpdateMetaArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    /// Meta-task id to update.
    #[arg(long, conflicts_with_all = ["handle"], required_unless_present_any = ["handle"])]
    pub(crate) meta_task_id: Option<String>,
    /// Store-issued handle to update, as `12` or `#12`.
    #[arg(
        long,
        value_name = "N",
        conflicts_with_all = ["meta_task_id"],
        required_unless_present_any = ["meta_task_id"]
    )]
    pub(crate) handle: Option<String>,
    /// Set the one-line title.
    #[arg(long, conflicts_with = "clear_title")]
    pub(crate) title: Option<String>,
    /// Remove the title.
    #[arg(long)]
    pub(crate) clear_title: bool,
    /// Set the declared urgency.
    #[arg(long, value_enum, conflicts_with = "clear_urgency")]
    pub(crate) urgency: Option<MetaTaskUrgencyArg>,
    /// Remove the urgency.
    #[arg(long)]
    pub(crate) clear_urgency: bool,
    /// Set the calendar due date.
    #[arg(long, value_name = "YYYY-MM-DD", conflicts_with = "clear_due")]
    pub(crate) due: Option<String>,
    /// Remove the due date.
    #[arg(long)]
    pub(crate) clear_due: bool,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct MetaStatusArgs {
    /// Meta-task id to read.
    #[arg(long, conflicts_with_all = ["handle"], required_unless_present_any = ["handle"])]
    pub(crate) meta_task_id: Option<String>,
    /// Store-issued handle to read, as `12` or `#12`.
    #[arg(
        long,
        value_name = "N",
        conflicts_with_all = ["meta_task_id"],
        required_unless_present_any = ["meta_task_id"]
    )]
    pub(crate) handle: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct MetaListArgs {
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u64).range(1..=500))]
    pub(crate) limit: u64,
    #[arg(long, default_value_t = 0)]
    pub(crate) offset: u64,
    /// Restrict the page to one state filter.
    #[arg(long, value_enum)]
    pub(crate) state: Option<MetaTaskListStateArg>,
    /// Restrict the page to tasks due before this date.
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub(crate) due_before: Option<String>,
    /// Restrict the page to tasks linked to one source thread.
    #[arg(long)]
    pub(crate) source_thread_id: Option<String>,
    /// Restrict the page to the task filed under one dedupe key.
    #[arg(long)]
    pub(crate) dedupe_key: Option<String>,
    /// Ordering of the page.
    #[arg(long, value_enum, default_value = "created_at")]
    pub(crate) sort: MetaTaskSortArg,
    /// Omit `prompt_text` from every row.
    #[arg(long)]
    pub(crate) compact: bool,
}

/// Reads the prompt text behind `--prompt-file`, with `-` naming stdin.
///
/// Exactly one trailing newline, written either as `\n` or as `\r\n`, is
/// stripped so a file and the same text passed to `--prompt-text` validate
/// identically.
fn read_prompt_file(raw: &str) -> Result<String, String> {
    let text = if raw == "-" {
        let mut buffer = String::new();
        std::io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|error| format!("failed to read stdin: {error}"))?;
        buffer
    } else {
        std::fs::read_to_string(raw).map_err(|error| format!("failed to read `{raw}`: {error}"))?
    };
    Ok(strip_trailing_newline(text))
}

fn strip_trailing_newline(mut text: String) -> String {
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    text
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum SessionRoleArg {
    Executor,
    Orchestrator,
    #[value(alias = "apply_gate")]
    ApplyGate,
    Reviewer,
}

impl From<SessionRoleArg> for SessionRole {
    fn from(value: SessionRoleArg) -> Self {
        match value {
            SessionRoleArg::Executor => SessionRole::Executor,
            SessionRoleArg::Orchestrator => SessionRole::Orchestrator,
            SessionRoleArg::ApplyGate => SessionRole::ApplyGate,
            SessionRoleArg::Reviewer => SessionRole::Reviewer,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
pub(crate) enum MetaTaskUrgencyArg {
    Low,
    Medium,
    High,
}

impl From<MetaTaskUrgencyArg> for MetaTaskUrgency {
    fn from(value: MetaTaskUrgencyArg) -> Self {
        match value {
            MetaTaskUrgencyArg::Low => MetaTaskUrgency::Low,
            MetaTaskUrgencyArg::Medium => MetaTaskUrgency::Medium,
            MetaTaskUrgencyArg::High => MetaTaskUrgency::High,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
pub(crate) enum MetaTaskClosureArg {
    Done,
    UpstreamClosed,
    Superseded,
    Dropped,
}

impl From<MetaTaskClosureArg> for MetaTaskClosure {
    fn from(value: MetaTaskClosureArg) -> Self {
        match value {
            MetaTaskClosureArg::Done => MetaTaskClosure::Done,
            MetaTaskClosureArg::UpstreamClosed => MetaTaskClosure::UpstreamClosed,
            MetaTaskClosureArg::Superseded => MetaTaskClosure::Superseded,
            MetaTaskClosureArg::Dropped => MetaTaskClosure::Dropped,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
pub(crate) enum MetaTaskAnnotationKeyArg {
    Verdict,
    Acked,
    Tier,
    Entity,
}

impl From<MetaTaskAnnotationKeyArg> for MetaTaskAnnotationKey {
    fn from(value: MetaTaskAnnotationKeyArg) -> Self {
        match value {
            MetaTaskAnnotationKeyArg::Verdict => MetaTaskAnnotationKey::Verdict,
            MetaTaskAnnotationKeyArg::Acked => MetaTaskAnnotationKey::Acked,
            MetaTaskAnnotationKeyArg::Tier => MetaTaskAnnotationKey::Tier,
            MetaTaskAnnotationKeyArg::Entity => MetaTaskAnnotationKey::Entity,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
pub(crate) enum MetaTaskListStateArg {
    Open,
    Planning,
    Active,
    Completed,
    Cancelled,
}

impl From<MetaTaskListStateArg> for MetaTaskListState {
    fn from(value: MetaTaskListStateArg) -> Self {
        match value {
            MetaTaskListStateArg::Open => MetaTaskListState::Open,
            MetaTaskListStateArg::Planning => MetaTaskListState::Planning,
            MetaTaskListStateArg::Active => MetaTaskListState::Active,
            MetaTaskListStateArg::Completed => MetaTaskListState::Completed,
            MetaTaskListStateArg::Cancelled => MetaTaskListState::Cancelled,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
pub(crate) enum MetaTaskSortArg {
    CreatedAt,
    Due,
    Urgency,
}

impl From<MetaTaskSortArg> for MetaTaskSort {
    fn from(value: MetaTaskSortArg) -> Self {
        match value {
            MetaTaskSortArg::CreatedAt => MetaTaskSort::CreatedAt,
            MetaTaskSortArg::Due => MetaTaskSort::Due,
            MetaTaskSortArg::Urgency => MetaTaskSort::Urgency,
        }
    }
}

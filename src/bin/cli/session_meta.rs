use clap::{Args, Subcommand, ValueEnum};
use covey::SessionRole;

#[derive(Subcommand, Debug)]
pub(crate) enum SessionCommand {
    Register(RegisterSessionArgs),
    Heartbeat(HeartbeatArgs),
    Exit(ExitSessionArgs),
    Status(SessionStatusArgs),
}

#[derive(Subcommand, Debug)]
pub(crate) enum MetaCommand {
    Submit(SubmitMetaArgs),
    Cancel(CancelMetaArgs),
    Status(MetaStatusArgs),
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
    #[arg(long)]
    pub(crate) prompt_text: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct CancelMetaArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) meta_task_id: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct MetaStatusArgs {
    #[arg(long)]
    pub(crate) meta_task_id: String,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum SessionRoleArg {
    Executor,
    Orchestrator,
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

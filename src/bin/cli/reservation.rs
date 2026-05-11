use clap::{Args, Subcommand, ValueEnum};
use covey::ScopeClass;

#[derive(Subcommand, Debug)]
pub(crate) enum ReservationCommand {
    Request(RequestReservationArgs),
    Release(ReleaseReservationArgs),
    Renew(RenewReservationArgs),
    Overlaps(OverlapArgs),
}
#[derive(Args, Debug)]
pub(crate) struct RequestReservationArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) owner_subtask_id: String,
    #[arg(long)]
    pub(crate) scope_class: ScopeClassArg,
    #[arg(long)]
    pub(crate) scope_key: String,
    #[arg(long = "member")]
    pub(crate) generated_members: Vec<String>,
    #[arg(long)]
    pub(crate) lease_duration_ms: i64,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ReleaseReservationArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) reservation_id: String,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct RenewReservationArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) reservation_id: String,
    #[arg(long)]
    pub(crate) extend_by_ms: i64,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct OverlapArgs {
    #[arg(long)]
    pub(crate) scope_class: ScopeClassArg,
    #[arg(long)]
    pub(crate) scope_key: String,
    #[arg(long = "member")]
    pub(crate) generated_members: Vec<String>,
}
#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum ScopeClassArg {
    ExactPath,
    Subtree,
    RepoGlobal,
    GeneratedSet,
}

impl From<ScopeClassArg> for ScopeClass {
    fn from(value: ScopeClassArg) -> Self {
        match value {
            ScopeClassArg::ExactPath => ScopeClass::ExactPath,
            ScopeClassArg::Subtree => ScopeClass::Subtree,
            ScopeClassArg::RepoGlobal => ScopeClass::RepoGlobal,
            ScopeClassArg::GeneratedSet => ScopeClass::GeneratedSet,
        }
    }
}

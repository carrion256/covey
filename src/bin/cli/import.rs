use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use covey::ObjectType;

#[derive(Subcommand, Debug)]
pub(crate) enum ImportCommand {
    Bd(ImportBdArgs),
    Openspec(ImportOpenSpecArgs),
    Provenance(ImportProvenanceArgs),
}

#[derive(Args, Debug)]
pub(crate) struct ImportBdArgs {
    #[arg(long)]
    pub(crate) session_token: String,
    #[arg(long)]
    pub(crate) beads_db: String,
    #[arg(long)]
    pub(crate) meta_task_id: Option<String>,
    #[arg(long)]
    pub(crate) prompt_text: Option<String>,
    #[arg(long)]
    pub(crate) idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ImportOpenSpecArgs {
    #[arg(long)]
    pub(crate) change: String,
    #[arg(long)]
    pub(crate) project_root: PathBuf,
    #[arg(long)]
    pub(crate) session_token: Option<String>,
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ImportProvenanceArgs {
    #[arg(long)]
    pub(crate) object_type: ImportObjectTypeArg,
    #[arg(long)]
    pub(crate) object_id: String,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum ImportObjectTypeArg {
    MetaTask,
    Subtask,
}

impl From<ImportObjectTypeArg> for ObjectType {
    fn from(value: ImportObjectTypeArg) -> Self {
        match value {
            ImportObjectTypeArg::MetaTask => ObjectType::MetaTask,
            ImportObjectTypeArg::Subtask => ObjectType::Subtask,
        }
    }
}

mod compile;
mod doctor;
mod error;
mod lint;
mod mission;
mod model;
mod report;
mod source;
mod validation;

#[cfg(test)]
mod tests;

pub use compile::compile_change;
pub use doctor::{
    DoctorCheck, DoctorCheckStatus, DoctorOptions, DoctorReport, DoctorStatus, doctor_change,
    run_openspec_check,
};
pub use error::{BetterDroidError, Result};
pub use lint::lint_change;
pub use model::{
    AssumptionApprovalSummary, AssumptionsArtifact, CompileOptions, CompiledMission, CompiledTask,
    LintOptions, MissionArtifact, PathPolicyArtifact, PlanningClass,
};
pub use report::{
    Blocker, ImportStatus, MissionReport, PacketKind, ProductImpactAudit, ReadinessGates,
    ReportStatus, TaskClassification, TaskCounts, Warning,
};
pub use validation::load_compiled_mission;

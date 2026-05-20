//! Import boundary DTOs for historical Beads and OpenSpec planning input.
//!
//! Raw IDs and tokens here represent external source or CLI payload shape.
//! Imported records are converted into Covey's validated domain records before
//! becoming live lifecycle state.

use std::{fmt, ops::Deref};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use strum::{Display, EnumString};

use super::{
    CoveyTypeValidationError, IdempotencyKey, MetaTaskId, ObjectType, OpenSpecChangeId,
    OpenSpecDigest, SessionToken, SourceIssueId, SubtaskId, TimestampMs,
};

/// Request to import eligible bd issues from a beads database into Covey as work subtasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBdV1Req {
    pub session_token: SessionToken,
    pub beads_db_path: BeadsDbPath,
    destination: ImportBdV1Destination,
    pub idempotency_key: IdempotencyKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ImportBdV1Destination {
    ExistingMetaTask { meta_task_id: MetaTaskId },
    NewMetaTask { prompt_text: String },
}

/// Filesystem path to a source beads database.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BeadsDbPath(String);

/// Filesystem path to the project root that owns an OpenSpec change.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectRootPath(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawImportBdV1Req {
    session_token: String,
    beads_db_path: String,
    meta_task_id: Option<String>,
    prompt_text: Option<String>,
    idempotency_key: String,
}

impl ImportBdV1Req {
    /// Builds a bd import request that attaches to an existing meta-task.
    #[must_use]
    pub fn existing_meta_task(
        session_token: impl Into<String>,
        beads_db_path: impl Into<String>,
        meta_task_id: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Self {
        Self {
            session_token: SessionToken::parse(session_token.into())
                .expect("import bd request session_token must be a valid Covey token"),
            beads_db_path: BeadsDbPath::parse(beads_db_path.into())
                .expect("import bd request beads_db_path must be valid"),
            destination: ImportBdV1Destination::ExistingMetaTask {
                meta_task_id: MetaTaskId::parse(meta_task_id.into())
                    .expect("import bd request meta_task_id must be a valid Covey id"),
            },
            idempotency_key: IdempotencyKey::parse(idempotency_key.into())
                .expect("import bd request idempotency_key must be valid"),
        }
    }

    /// Builds a bd import request that creates a new meta-task.
    #[must_use]
    pub fn new_meta_task(
        session_token: impl Into<String>,
        beads_db_path: impl Into<String>,
        prompt_text: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Self {
        Self {
            session_token: SessionToken::parse(session_token.into())
                .expect("import bd request session_token must be a valid Covey token"),
            beads_db_path: BeadsDbPath::parse(beads_db_path.into())
                .expect("import bd request beads_db_path must be valid"),
            destination: ImportBdV1Destination::NewMetaTask {
                prompt_text: prompt_text.into(),
            },
            idempotency_key: IdempotencyKey::parse(idempotency_key.into())
                .expect("import bd request idempotency_key must be valid"),
        }
    }

    /// Builds a bd import request from the legacy flat selector fields.
    ///
    /// # Errors
    ///
    /// Returns an error unless exactly one of `meta_task_id` or `prompt_text`
    /// is present.
    pub fn from_flat_selectors(
        session_token: impl Into<String>,
        beads_db_path: impl Into<String>,
        meta_task_id: Option<String>,
        prompt_text: Option<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        let destination = ImportBdV1Destination::from_flat_selectors(meta_task_id, prompt_text)?;
        Ok(Self {
            session_token: SessionToken::parse(session_token.into())?,
            beads_db_path: BeadsDbPath::parse(beads_db_path.into())?,
            destination,
            idempotency_key: IdempotencyKey::parse(idempotency_key.into())?,
        })
    }

    /// Returns the session token that authorizes this import.
    #[must_use]
    pub fn session_token(&self) -> &SessionToken {
        &self.session_token
    }

    /// Returns the existing meta-task selector, when this request uses one.
    #[must_use]
    pub fn meta_task_id(&self) -> Option<&str> {
        self.destination.meta_task_id()
    }

    /// Returns the new meta-task prompt selector, when this request uses one.
    #[must_use]
    pub fn prompt_text(&self) -> Option<&str> {
        self.destination.prompt_text()
    }
}

impl ImportBdV1Destination {
    fn from_flat_selectors(
        meta_task_id: Option<String>,
        prompt_text: Option<String>,
    ) -> Result<Self, CoveyTypeValidationError> {
        match (meta_task_id, prompt_text) {
            (Some(meta_task_id), None) => Ok(Self::ExistingMetaTask {
                meta_task_id: MetaTaskId::parse(meta_task_id)?,
            }),
            (None, Some(prompt_text)) => Ok(Self::NewMetaTask { prompt_text }),
            _ => Err(CoveyTypeValidationError::new(
                "bd_import_destination",
                "requires exactly one destination selector",
            )),
        }
    }

    fn meta_task_id(&self) -> Option<&str> {
        match self {
            Self::ExistingMetaTask { meta_task_id } => Some(meta_task_id.as_str()),
            Self::NewMetaTask { .. } => None,
        }
    }

    fn prompt_text(&self) -> Option<&str> {
        match self {
            Self::ExistingMetaTask { .. } => None,
            Self::NewMetaTask { prompt_text } => Some(prompt_text),
        }
    }
}

fn validate_import_filesystem_path(
    field: &'static str,
    value: &str,
) -> Result<(), CoveyTypeValidationError> {
    if value.trim().is_empty() {
        return Err(CoveyTypeValidationError::new(field, "must not be empty"));
    }
    if value.trim() != value {
        return Err(CoveyTypeValidationError::new(
            field,
            "must not include leading or trailing whitespace",
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(CoveyTypeValidationError::new(
            field,
            "must not contain control characters",
        ));
    }
    Ok(())
}

impl BeadsDbPath {
    /// Parses and validates a source beads database path.
    pub fn parse(value: impl Into<String>) -> Result<Self, CoveyTypeValidationError> {
        let value = value.into();
        validate_import_filesystem_path("beads_db_path", &value)?;
        Ok(Self(value))
    }

    /// Returns the validated path string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ProjectRootPath {
    /// Parses and validates an OpenSpec project root path.
    pub fn parse(value: impl Into<String>) -> Result<Self, CoveyTypeValidationError> {
        let value = value.into();
        validate_import_filesystem_path("project_root", &value)?;
        Ok(Self(value))
    }

    /// Returns the validated path string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BeadsDbPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for ProjectRootPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Deref for BeadsDbPath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl Deref for ProjectRootPath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl Serialize for BeadsDbPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl Serialize for ProjectRootPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for BeadsDbPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(raw).map_err(serde::de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for ProjectRootPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(raw).map_err(serde::de::Error::custom)
    }
}

impl From<&ImportBdV1Req> for RawImportBdV1Req {
    fn from(req: &ImportBdV1Req) -> Self {
        Self {
            session_token: req.session_token.to_string(),
            beads_db_path: req.beads_db_path.to_string(),
            meta_task_id: req.meta_task_id().map(str::to_owned),
            prompt_text: req.prompt_text().map(str::to_owned),
            idempotency_key: req.idempotency_key.to_string(),
        }
    }
}

impl Serialize for ImportBdV1Req {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawImportBdV1Req::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ImportBdV1Req {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawImportBdV1Req::deserialize(deserializer)?;
        Self::from_flat_selectors(
            raw.session_token,
            raw.beads_db_path,
            raw.meta_task_id,
            raw.prompt_text,
            raw.idempotency_key,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Result of a V1 bd batch import operation.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBdV1Result {
    pub meta_task_id: MetaTaskId,
    tally: ImportBdV1ResultTally,
    items: Vec<ImportBdV1ItemResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ImportBdV1ResultTally {
    imported_count: usize,
    skipped_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawImportBdV1Result {
    meta_task_id: String,
    imported_count: usize,
    skipped_count: usize,
    items: Vec<ImportBdV1ItemResult>,
}

impl ImportBdV1Result {
    /// Builds a bd import result with aggregate counts derived from item outcomes.
    ///
    /// # Errors
    ///
    /// Returns an error when provided flat counts do not match the item outcomes.
    pub fn new(
        meta_task_id: impl Into<String>,
        items: Vec<ImportBdV1ItemResult>,
    ) -> Result<Self, String> {
        Self::from_raw_parts(meta_task_id, None, items)
    }

    fn from_raw_parts(
        meta_task_id: impl Into<String>,
        expected_tally: Option<ImportBdV1ResultTally>,
        items: Vec<ImportBdV1ItemResult>,
    ) -> Result<Self, String> {
        let tally = ImportBdV1ResultTally::from_items(&items);
        if let Some(expected_tally) = expected_tally {
            expected_tally.matches(tally)?;
        }
        Ok(Self {
            meta_task_id: MetaTaskId::parse(meta_task_id.into()).map_err(|err| err.to_string())?,
            tally,
            items,
        })
    }

    /// Returns the number of imported records, derived from item outcomes.
    #[must_use]
    pub const fn imported_count(&self) -> usize {
        self.tally.imported_count
    }

    /// Returns the number of skipped records, derived from item outcomes.
    #[must_use]
    pub const fn skipped_count(&self) -> usize {
        self.tally.skipped_count
    }

    /// Returns per-item import outcomes.
    #[must_use]
    pub fn items(&self) -> &[ImportBdV1ItemResult] {
        &self.items
    }

    /// Splits this result into the legacy flat response parts.
    #[must_use]
    pub fn into_flat_parts(self) -> (String, usize, usize, Vec<ImportBdV1ItemResult>) {
        (
            self.meta_task_id.to_string(),
            self.tally.imported_count,
            self.tally.skipped_count,
            self.items,
        )
    }

    /// Returns a concise, deterministic human-readable summary of the import.
    pub fn human_summary(&self) -> String {
        let mut summary = format!(
            "imported {} subtask(s) into {} (skipped {})",
            self.imported_count(),
            self.meta_task_id,
            self.skipped_count()
        );
        if self.skipped_count() > 0 {
            let mut duplicate_count = 0usize;
            let mut invalid_count = 0usize;
            for item in &self.items {
                match item.skip_reason() {
                    Some(ImportBdV1SkipReason::DeterministicDuplicate) => duplicate_count += 1,
                    Some(ImportBdV1SkipReason::InvalidRow { .. }) => invalid_count += 1,
                    None => {}
                }
            }
            let mut reasons = Vec::new();
            if duplicate_count > 0 {
                reasons.push(format!("{} duplicate", duplicate_count));
            }
            if invalid_count > 0 {
                reasons.push(format!("{} invalid", invalid_count));
            }
            if !reasons.is_empty() {
                summary.push_str(&format!(": {}", reasons.join(", ")));
            }
        }
        summary
    }
}

impl ImportBdV1ResultTally {
    fn from_items(items: &[ImportBdV1ItemResult]) -> Self {
        let mut tally = Self {
            imported_count: 0,
            skipped_count: 0,
        };
        for item in items {
            if item.skip_reason().is_some() {
                tally.skipped_count += 1;
            } else {
                tally.imported_count += 1;
            }
        }
        tally
    }

    fn matches(self, actual: Self) -> Result<(), String> {
        if self.imported_count != actual.imported_count {
            return Err(format!(
                "bd import result imported_count mismatch: expected {}, derived {}",
                self.imported_count, actual.imported_count
            ));
        }
        if self.skipped_count != actual.skipped_count {
            return Err(format!(
                "bd import result skipped_count mismatch: expected {}, derived {}",
                self.skipped_count, actual.skipped_count
            ));
        }
        Ok(())
    }
}

impl From<&ImportBdV1Result> for RawImportBdV1Result {
    fn from(result: &ImportBdV1Result) -> Self {
        Self {
            meta_task_id: result.meta_task_id.to_string(),
            imported_count: result.tally.imported_count,
            skipped_count: result.tally.skipped_count,
            items: result.items.clone(),
        }
    }
}

impl Serialize for ImportBdV1Result {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawImportBdV1Result::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ImportBdV1Result {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawImportBdV1Result::deserialize(deserializer)?;
        Self::from_raw_parts(
            raw.meta_task_id,
            Some(ImportBdV1ResultTally {
                imported_count: raw.imported_count,
                skipped_count: raw.skipped_count,
            }),
            raw.items,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Per-item outcome for a V1 bd import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBdV1ItemResult {
    pub source_issue_id: SourceIssueId,
    outcome: ImportBdV1ItemOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ImportBdV1ItemOutcome {
    Imported {
        subtask_id: SubtaskId,
    },
    Skipped {
        subtask_id: Option<SubtaskId>,
        skip_reason: ImportBdV1SkipReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawImportBdV1ItemResult {
    source_issue_id: String,
    subtask_id: Option<String>,
    skip_reason: Option<ImportBdV1SkipReason>,
}

impl ImportBdV1ItemResult {
    /// Builds an imported bd item result.
    #[must_use]
    pub fn imported(source_issue_id: impl Into<String>, subtask_id: impl Into<String>) -> Self {
        Self {
            source_issue_id: SourceIssueId::parse(source_issue_id.into())
                .expect("imported bd item source_issue_id must be valid"),
            outcome: ImportBdV1ItemOutcome::Imported {
                subtask_id: SubtaskId::parse(subtask_id.into())
                    .expect("imported bd item subtask_id must be a valid Covey id"),
            },
        }
    }

    /// Builds a skipped bd item result.
    #[must_use]
    pub fn skipped(
        source_issue_id: impl Into<String>,
        subtask_id: Option<String>,
        skip_reason: ImportBdV1SkipReason,
    ) -> Self {
        let outcome = ImportBdV1ItemOutcome::try_from_parts(subtask_id, Some(skip_reason))
            .expect("skipped bd item outcome must be internally consistent");
        Self {
            source_issue_id: SourceIssueId::parse(source_issue_id.into())
                .expect("skipped bd item source_issue_id must be valid"),
            outcome,
        }
    }

    /// Returns the source issue id that produced this import item.
    #[must_use]
    pub fn source_issue_id(&self) -> &str {
        self.source_issue_id.as_str()
    }

    /// Returns the imported or duplicate-existing subtask id, when present.
    #[must_use]
    pub fn subtask_id(&self) -> Option<&str> {
        match &self.outcome {
            ImportBdV1ItemOutcome::Imported { subtask_id } => Some(subtask_id.as_str()),
            ImportBdV1ItemOutcome::Skipped { subtask_id, .. } => {
                subtask_id.as_ref().map(SubtaskId::as_str)
            }
        }
    }

    /// Returns the skip reason for skipped items.
    #[must_use]
    pub const fn skip_reason(&self) -> Option<&ImportBdV1SkipReason> {
        match &self.outcome {
            ImportBdV1ItemOutcome::Imported { .. } => None,
            ImportBdV1ItemOutcome::Skipped { skip_reason, .. } => Some(skip_reason),
        }
    }
}

impl ImportBdV1ItemOutcome {
    fn try_from_parts(
        subtask_id: Option<String>,
        skip_reason: Option<ImportBdV1SkipReason>,
    ) -> Result<Self, String> {
        match (subtask_id, skip_reason) {
            (Some(subtask_id), None) => Ok(Self::Imported {
                subtask_id: SubtaskId::parse(subtask_id).map_err(|err| err.to_string())?,
            }),
            (Some(subtask_id), Some(ImportBdV1SkipReason::DeterministicDuplicate)) => {
                Ok(Self::Skipped {
                    subtask_id: Some(SubtaskId::parse(subtask_id).map_err(|err| err.to_string())?),
                    skip_reason: ImportBdV1SkipReason::DeterministicDuplicate,
                })
            }
            (None, Some(ImportBdV1SkipReason::DeterministicDuplicate)) => {
                Err("deterministic duplicate bd import items require subtask_id".into())
            }
            (None, Some(skip_reason @ ImportBdV1SkipReason::InvalidRow { .. })) => {
                Ok(Self::Skipped {
                    subtask_id: None,
                    skip_reason,
                })
            }
            (Some(_), Some(ImportBdV1SkipReason::InvalidRow { .. })) => {
                Err("invalid bd import rows must not include subtask_id".into())
            }
            (None, None) => {
                Err("bd import item must include either subtask_id or skip_reason".into())
            }
        }
    }

    fn subtask_id(&self) -> Option<&str> {
        match self {
            Self::Imported { subtask_id } => Some(subtask_id.as_str()),
            Self::Skipped { subtask_id, .. } => subtask_id.as_ref().map(SubtaskId::as_str),
        }
    }

    const fn skip_reason(&self) -> Option<&ImportBdV1SkipReason> {
        match self {
            Self::Imported { .. } => None,
            Self::Skipped { skip_reason, .. } => Some(skip_reason),
        }
    }
}

impl Serialize for ImportBdV1ItemResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawImportBdV1ItemResult {
            source_issue_id: self.source_issue_id.to_string(),
            subtask_id: self.outcome.subtask_id().map(str::to_owned),
            skip_reason: self.outcome.skip_reason().cloned(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ImportBdV1ItemResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawImportBdV1ItemResult::deserialize(deserializer)?;
        let outcome = ImportBdV1ItemOutcome::try_from_parts(raw.subtask_id, raw.skip_reason)
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            source_issue_id: SourceIssueId::parse(raw.source_issue_id)
                .map_err(serde::de::Error::custom)?,
            outcome,
        })
    }
}

/// Reasons a bd issue may be skipped during V1 import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportBdV1SkipReason {
    DeterministicDuplicate,
    InvalidRow { detail: String },
}

/// Request to import one OpenSpec change into Covey task state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOpenSpecReq {
    pub change_id: OpenSpecChangeId,
    pub project_root: ProjectRootPath,
    mode: ImportOpenSpecMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ImportOpenSpecMode {
    DryRun { session_token: Option<SessionToken> },
    Write { session_token: SessionToken },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawImportOpenSpecReq {
    session_token: Option<String>,
    change_id: String,
    project_root: String,
    dry_run: bool,
}

impl ImportOpenSpecReq {
    /// Builds an anonymous OpenSpec dry-run import request.
    #[must_use]
    pub fn dry_run(change_id: impl Into<String>, project_root: impl Into<String>) -> Self {
        Self {
            change_id: OpenSpecChangeId::parse(change_id.into())
                .expect("OpenSpec import change_id must be kebab-case ASCII"),
            project_root: ProjectRootPath::parse(project_root.into())
                .expect("OpenSpec import project_root must be valid"),
            mode: ImportOpenSpecMode::DryRun {
                session_token: None,
            },
        }
    }

    /// Builds a session-attributed OpenSpec dry-run import request.
    #[must_use]
    pub fn dry_run_for_session(
        session_token: impl Into<String>,
        change_id: impl Into<String>,
        project_root: impl Into<String>,
    ) -> Self {
        Self {
            change_id: OpenSpecChangeId::parse(change_id.into())
                .expect("OpenSpec import change_id must be kebab-case ASCII"),
            project_root: ProjectRootPath::parse(project_root.into())
                .expect("OpenSpec import project_root must be valid"),
            mode: ImportOpenSpecMode::DryRun {
                session_token: Some(
                    SessionToken::parse(session_token.into())
                        .expect("OpenSpec dry-run session_token must be a valid Covey token"),
                ),
            },
        }
    }

    /// Builds an OpenSpec write-mode import request.
    #[must_use]
    pub fn write(
        session_token: impl Into<String>,
        change_id: impl Into<String>,
        project_root: impl Into<String>,
    ) -> Self {
        Self {
            change_id: OpenSpecChangeId::parse(change_id.into())
                .expect("OpenSpec import change_id must be kebab-case ASCII"),
            project_root: ProjectRootPath::parse(project_root.into())
                .expect("OpenSpec import project_root must be valid"),
            mode: ImportOpenSpecMode::Write {
                session_token: SessionToken::parse(session_token.into())
                    .expect("OpenSpec write session_token must be a valid Covey token"),
            },
        }
    }

    /// Builds an OpenSpec import request from the legacy flat mode fields.
    ///
    /// # Errors
    ///
    /// Returns an error when write mode is requested without a session token.
    pub fn from_flat_mode(
        session_token: Option<String>,
        change_id: impl Into<String>,
        project_root: impl Into<String>,
        dry_run: bool,
    ) -> Result<Self, CoveyTypeValidationError> {
        let mode = ImportOpenSpecMode::from_flat_mode(session_token, dry_run)?;
        Ok(Self {
            change_id: OpenSpecChangeId::parse(change_id.into())?,
            project_root: ProjectRootPath::parse(project_root.into())?,
            mode,
        })
    }

    /// Returns the session token used to attribute this request, when present.
    #[must_use]
    pub fn session_token(&self) -> Option<&str> {
        self.mode.session_token()
    }

    /// Returns the required write-mode session token, when this request mutates state.
    #[must_use]
    pub fn write_session_token(&self) -> Option<&str> {
        self.mode.write_session_token()
    }

    /// Returns whether this import should only plan the OpenSpec change.
    #[must_use]
    pub const fn is_dry_run(&self) -> bool {
        self.mode.dry_run()
    }
}

impl ImportOpenSpecMode {
    fn from_flat_mode(
        session_token: Option<String>,
        dry_run: bool,
    ) -> Result<Self, CoveyTypeValidationError> {
        if dry_run {
            Ok(Self::DryRun {
                session_token: session_token.map(SessionToken::parse).transpose()?,
            })
        } else {
            session_token
                .map(|session_token| {
                    Ok(Self::Write {
                        session_token: SessionToken::parse(session_token)?,
                    })
                })
                .ok_or_else(|| {
                    CoveyTypeValidationError::new(
                        "session_token",
                        "write mode requires --session-token",
                    )
                })?
        }
    }

    fn session_token(&self) -> Option<&str> {
        match self {
            Self::DryRun { session_token } => session_token.as_ref().map(SessionToken::as_str),
            Self::Write { session_token } => Some(session_token.as_str()),
        }
    }

    fn write_session_token(&self) -> Option<&str> {
        match self {
            Self::DryRun { .. } => None,
            Self::Write { session_token } => Some(session_token.as_str()),
        }
    }

    const fn dry_run(&self) -> bool {
        match self {
            Self::DryRun { .. } => true,
            Self::Write { .. } => false,
        }
    }
}

impl From<&ImportOpenSpecReq> for RawImportOpenSpecReq {
    fn from(req: &ImportOpenSpecReq) -> Self {
        Self {
            session_token: req.session_token().map(str::to_owned),
            change_id: req.change_id.to_string(),
            project_root: req.project_root.to_string(),
            dry_run: req.is_dry_run(),
        }
    }
}

impl Serialize for ImportOpenSpecReq {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawImportOpenSpecReq::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ImportOpenSpecReq {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawImportOpenSpecReq::deserialize(deserializer)?;
        Self::from_flat_mode(
            raw.session_token,
            raw.change_id,
            raw.project_root,
            raw.dry_run,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Result of an OpenSpec import or dry-run import plan.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOpenSpecResult {
    pub change_id: OpenSpecChangeId,
    pub meta_task_id: MetaTaskId,
    dry_run: bool,
    tally: ImportOpenSpecResultTally,
    conflicts: Vec<ImportOpenSpecConflict>,
    items: Vec<ImportOpenSpecItemResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ImportOpenSpecResultTally {
    created: usize,
    updated: usize,
    unchanged: usize,
    conflicts: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawImportOpenSpecResult {
    change_id: String,
    meta_task_id: String,
    dry_run: bool,
    created: usize,
    updated: usize,
    unchanged: usize,
    conflicts: Vec<ImportOpenSpecConflict>,
    items: Vec<ImportOpenSpecItemResult>,
}

impl ImportOpenSpecResult {
    /// Builds an OpenSpec import result with aggregate counts derived from items.
    ///
    /// # Errors
    ///
    /// Returns an error when the conflict vector does not match the number of
    /// conflict item outcomes.
    pub fn new(
        change_id: impl Into<String>,
        meta_task_id: impl Into<String>,
        dry_run: bool,
        conflicts: Vec<ImportOpenSpecConflict>,
        items: Vec<ImportOpenSpecItemResult>,
    ) -> Result<Self, String> {
        Self::from_raw_parts(change_id, meta_task_id, dry_run, None, conflicts, items)
    }

    fn from_raw_parts(
        change_id: impl Into<String>,
        meta_task_id: impl Into<String>,
        dry_run: bool,
        expected_tally: Option<ImportOpenSpecResultTally>,
        conflicts: Vec<ImportOpenSpecConflict>,
        items: Vec<ImportOpenSpecItemResult>,
    ) -> Result<Self, String> {
        let tally = ImportOpenSpecResultTally::from_items(&items);
        if tally.conflicts != conflicts.len() {
            return Err(format!(
                "OpenSpec import result has {} conflict item(s) but {} conflict record(s)",
                tally.conflicts,
                conflicts.len()
            ));
        }
        if let Some(expected_tally) = expected_tally {
            expected_tally.matches(tally)?;
        }
        Ok(Self {
            change_id: OpenSpecChangeId::parse(change_id.into()).map_err(|err| err.to_string())?,
            meta_task_id: MetaTaskId::parse(meta_task_id.into()).map_err(|err| err.to_string())?,
            dry_run,
            tally,
            conflicts,
            items,
        })
    }

    /// Returns whether this result came from a dry-run import plan.
    #[must_use]
    pub const fn dry_run(&self) -> bool {
        self.dry_run
    }

    /// Returns the number of created records, derived from item actions.
    #[must_use]
    pub const fn created(&self) -> usize {
        self.tally.created
    }

    /// Returns the number of updated records, derived from item actions.
    #[must_use]
    pub const fn updated(&self) -> usize {
        self.tally.updated
    }

    /// Returns the number of unchanged records, derived from item actions.
    #[must_use]
    pub const fn unchanged(&self) -> usize {
        self.tally.unchanged
    }

    /// Returns conflict records for item outcomes that could not be imported.
    #[must_use]
    pub fn conflicts(&self) -> &[ImportOpenSpecConflict] {
        &self.conflicts
    }

    /// Returns per-record import outcomes.
    #[must_use]
    pub fn items(&self) -> &[ImportOpenSpecItemResult] {
        &self.items
    }

    /// Splits this result into the legacy flat response parts.
    #[must_use]
    pub fn into_flat_parts(
        self,
    ) -> (
        String,
        String,
        bool,
        usize,
        usize,
        usize,
        Vec<ImportOpenSpecConflict>,
        Vec<ImportOpenSpecItemResult>,
    ) {
        (
            self.change_id.to_string(),
            self.meta_task_id.to_string(),
            self.dry_run,
            self.tally.created,
            self.tally.updated,
            self.tally.unchanged,
            self.conflicts,
            self.items,
        )
    }

    /// Returns a concise, deterministic human-readable summary of the import.
    pub fn human_summary(&self) -> String {
        format!(
            "openspec import {} meta_task={} created={} updated={} unchanged={} conflicts={} dry_run={}",
            self.change_id,
            self.meta_task_id,
            self.created(),
            self.updated(),
            self.unchanged(),
            self.conflicts.len(),
            self.dry_run
        )
    }
}

impl ImportOpenSpecResultTally {
    fn from_items(items: &[ImportOpenSpecItemResult]) -> Self {
        let mut tally = Self {
            created: 0,
            updated: 0,
            unchanged: 0,
            conflicts: 0,
        };
        for item in items {
            match item.action() {
                ImportOpenSpecAction::Created => tally.created += 1,
                ImportOpenSpecAction::Updated => tally.updated += 1,
                ImportOpenSpecAction::Unchanged => tally.unchanged += 1,
                ImportOpenSpecAction::Conflict => tally.conflicts += 1,
            }
        }
        tally
    }

    fn matches(self, actual: Self) -> Result<(), String> {
        if self.created != actual.created {
            return Err(format!(
                "OpenSpec import result created count mismatch: expected {}, derived {}",
                self.created, actual.created
            ));
        }
        if self.updated != actual.updated {
            return Err(format!(
                "OpenSpec import result updated count mismatch: expected {}, derived {}",
                self.updated, actual.updated
            ));
        }
        if self.unchanged != actual.unchanged {
            return Err(format!(
                "OpenSpec import result unchanged count mismatch: expected {}, derived {}",
                self.unchanged, actual.unchanged
            ));
        }
        Ok(())
    }
}

impl From<&ImportOpenSpecResult> for RawImportOpenSpecResult {
    fn from(result: &ImportOpenSpecResult) -> Self {
        Self {
            change_id: result.change_id.to_string(),
            meta_task_id: result.meta_task_id.to_string(),
            dry_run: result.dry_run,
            created: result.tally.created,
            updated: result.tally.updated,
            unchanged: result.tally.unchanged,
            conflicts: result.conflicts.clone(),
            items: result.items.clone(),
        }
    }
}

impl Serialize for ImportOpenSpecResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawImportOpenSpecResult::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ImportOpenSpecResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawImportOpenSpecResult::deserialize(deserializer)?;
        Self::from_raw_parts(
            raw.change_id,
            raw.meta_task_id,
            raw.dry_run,
            Some(ImportOpenSpecResultTally {
                created: raw.created,
                updated: raw.updated,
                unchanged: raw.unchanged,
                conflicts: raw.conflicts.len(),
            }),
            raw.conflicts,
            raw.items,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Normalized OpenSpec-relative path stored in import DTOs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpenSpecPath(String);

impl OpenSpecPath {
    /// Parses a raw path into a validated OpenSpec-relative path.
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err("OpenSpec path must not be empty".to_owned());
        }
        if value.trim() != value {
            return Err("OpenSpec path must not include leading or trailing whitespace".to_owned());
        }
        if value.starts_with('/') || value.starts_with('\\') {
            return Err("OpenSpec path must be relative".to_owned());
        }
        if value.chars().any(char::is_control) {
            return Err("OpenSpec path must not contain control characters".to_owned());
        }
        if value.split(['/', '\\']).any(|component| component == "..") {
            return Err("OpenSpec path must not escape upward".to_owned());
        }
        Ok(Self(value))
    }

    /// Returns the validated path string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OpenSpecPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Deref for OpenSpecPath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl Serialize for OpenSpecPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for OpenSpecPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(raw).map_err(serde::de::Error::custom)
    }
}

/// Stable OpenSpec task identifier such as `1.1` or `2.3.4`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpenSpecTaskId(String);

impl OpenSpecTaskId {
    /// Parses a raw OpenSpec task id into hierarchical numeric form.
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if !value.contains('.') {
            return Err("OpenSpec task id must be hierarchical numeric form".to_owned());
        }
        if value
            .split('.')
            .any(|segment| segment.is_empty() || !segment.chars().all(|ch| ch.is_ascii_digit()))
        {
            return Err(
                "OpenSpec task id must contain only numeric dot-separated segments".to_owned(),
            );
        }
        Ok(Self(value))
    }

    /// Returns the validated task id string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OpenSpecTaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Deref for OpenSpecTaskId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl Serialize for OpenSpecTaskId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for OpenSpecTaskId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(raw).map_err(serde::de::Error::custom)
    }
}

/// Per-logical-record outcome for an OpenSpec import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOpenSpecItemResult {
    action: ImportOpenSpecAction,
    object: ImportOpenSpecItemObject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ImportOpenSpecItemObject {
    MetaTask {
        object_id: MetaTaskId,
        title: String,
    },
    Subtask {
        object_id: SubtaskId,
        openspec_task_id: OpenSpecTaskId,
        title: String,
        task_type: Option<String>,
        task_digest: OpenSpecDigest,
        source_path: OpenSpecPath,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawImportOpenSpecItemResult {
    object_type: ObjectType,
    object_id: String,
    openspec_task_id: Option<String>,
    title: Option<String>,
    task_type: Option<String>,
    task_digest: Option<String>,
    source_path: Option<String>,
    action: ImportOpenSpecAction,
}

impl ImportOpenSpecItemResult {
    /// Builds an OpenSpec import item for the destination meta-task.
    #[must_use]
    pub fn meta_task(
        object_id: impl Into<String>,
        title: impl Into<String>,
        action: ImportOpenSpecAction,
    ) -> Result<Self, String> {
        if action == ImportOpenSpecAction::Conflict {
            return Err("metatask OpenSpec import items cannot use conflict action".into());
        }
        Ok(Self {
            action,
            object: ImportOpenSpecItemObject::MetaTask {
                object_id: MetaTaskId::parse(object_id.into()).map_err(|err| err.to_string())?,
                title: title.into(),
            },
        })
    }

    /// Builds an OpenSpec import item for one work subtask.
    #[must_use]
    pub fn subtask(
        object_id: impl Into<String>,
        openspec_task_id: impl Into<String>,
        title: impl Into<String>,
        task_type: Option<String>,
        task_digest: impl Into<String>,
        source_path: impl Into<String>,
        action: ImportOpenSpecAction,
    ) -> Result<Self, String> {
        Ok(Self {
            action,
            object: ImportOpenSpecItemObject::Subtask {
                object_id: SubtaskId::parse(object_id.into()).map_err(|err| err.to_string())?,
                openspec_task_id: OpenSpecTaskId::parse(openspec_task_id.into())?,
                title: title.into(),
                task_type,
                task_digest: OpenSpecDigest::parse(task_digest.into())
                    .map_err(|err| err.to_string())?,
                source_path: OpenSpecPath::parse(source_path.into())?,
            },
        })
    }

    /// Returns the imported Covey object type.
    #[must_use]
    pub const fn object_type(&self) -> ObjectType {
        self.object.object_type()
    }

    /// Returns the imported Covey object id.
    #[must_use]
    pub fn object_id(&self) -> &str {
        self.object.object_id()
    }

    /// Returns the import action for this item.
    #[must_use]
    pub const fn action(&self) -> ImportOpenSpecAction {
        self.action
    }

    /// Returns the OpenSpec task id for subtask import items.
    #[must_use]
    pub fn openspec_task_id(&self) -> Option<&str> {
        match &self.object {
            ImportOpenSpecItemObject::MetaTask { .. } => None,
            ImportOpenSpecItemObject::Subtask {
                openspec_task_id, ..
            } => Some(openspec_task_id.as_str()),
        }
    }

    /// Returns the item title.
    #[must_use]
    pub fn title(&self) -> &str {
        match &self.object {
            ImportOpenSpecItemObject::MetaTask { title, .. } => title,
            ImportOpenSpecItemObject::Subtask { title, .. } => title,
        }
    }

    /// Returns the task type for subtask import items when OpenSpec supplied one.
    #[must_use]
    pub fn task_type(&self) -> Option<&str> {
        match &self.object {
            ImportOpenSpecItemObject::MetaTask { .. } => None,
            ImportOpenSpecItemObject::Subtask { task_type, .. } => task_type.as_deref(),
        }
    }

    /// Returns the task digest for subtask import items.
    #[must_use]
    pub fn task_digest(&self) -> Option<&str> {
        match &self.object {
            ImportOpenSpecItemObject::MetaTask { .. } => None,
            ImportOpenSpecItemObject::Subtask { task_digest, .. } => Some(task_digest.as_str()),
        }
    }

    /// Returns the source path for subtask import items.
    #[must_use]
    pub fn source_path(&self) -> Option<&str> {
        match &self.object {
            ImportOpenSpecItemObject::MetaTask { .. } => None,
            ImportOpenSpecItemObject::Subtask { source_path, .. } => Some(source_path.as_str()),
        }
    }
}

impl ImportOpenSpecItemObject {
    fn try_from_raw_parts(
        object_type: ObjectType,
        object_id: String,
        openspec_task_id: Option<String>,
        title: Option<String>,
        task_type: Option<String>,
        task_digest: Option<String>,
        source_path: Option<String>,
    ) -> Result<Self, String> {
        match object_type {
            ObjectType::MetaTask => {
                if openspec_task_id.is_some()
                    || task_type.is_some()
                    || task_digest.is_some()
                    || source_path.is_some()
                {
                    return Err(
                        "metatask OpenSpec import items must not include task fields".into(),
                    );
                }
                let title = title
                    .ok_or_else(|| "metatask OpenSpec import items require title".to_owned())?;
                Ok(Self::MetaTask {
                    object_id: MetaTaskId::parse(object_id).map_err(|err| err.to_string())?,
                    title,
                })
            }
            ObjectType::Subtask => {
                let openspec_task_id = openspec_task_id.ok_or_else(|| {
                    "subtask OpenSpec import items require openspec_task_id".to_owned()
                })?;
                let title = title
                    .ok_or_else(|| "subtask OpenSpec import items require title".to_owned())?;
                let task_digest = task_digest.ok_or_else(|| {
                    "subtask OpenSpec import items require task_digest".to_owned()
                })?;
                let source_path = source_path.ok_or_else(|| {
                    "subtask OpenSpec import items require source_path".to_owned()
                })?;
                Ok(Self::Subtask {
                    object_id: SubtaskId::parse(object_id).map_err(|err| err.to_string())?,
                    openspec_task_id: OpenSpecTaskId::parse(openspec_task_id)?,
                    title,
                    task_type,
                    task_digest: OpenSpecDigest::parse(task_digest)
                        .map_err(|err| err.to_string())?,
                    source_path: OpenSpecPath::parse(source_path)?,
                })
            }
            _ => Err("OpenSpec import items only support metatask and subtask objects".into()),
        }
    }

    const fn object_type(&self) -> ObjectType {
        match self {
            Self::MetaTask { .. } => ObjectType::MetaTask,
            Self::Subtask { .. } => ObjectType::Subtask,
        }
    }

    fn object_id(&self) -> &str {
        match self {
            Self::MetaTask { object_id, .. } => object_id.as_str(),
            Self::Subtask { object_id, .. } => object_id.as_str(),
        }
    }

    fn validate_action(&self, action: ImportOpenSpecAction) -> Result<(), String> {
        match (self, action) {
            (Self::MetaTask { .. }, ImportOpenSpecAction::Conflict) => {
                Err("metatask OpenSpec import items cannot use conflict action".into())
            }
            _ => Ok(()),
        }
    }
}

impl Serialize for ImportOpenSpecItemResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let (openspec_task_id, title, task_type, task_digest, source_path) = match &self.object {
            ImportOpenSpecItemObject::MetaTask { title, .. } => {
                (None, Some(title.clone()), None, None, None)
            }
            ImportOpenSpecItemObject::Subtask {
                openspec_task_id,
                title,
                task_type,
                task_digest,
                source_path,
                ..
            } => (
                Some(openspec_task_id.to_string()),
                Some(title.clone()),
                task_type.clone(),
                Some(task_digest.to_string()),
                Some(source_path.to_string()),
            ),
        };
        RawImportOpenSpecItemResult {
            object_type: self.object_type(),
            object_id: self.object_id().to_owned(),
            openspec_task_id,
            title,
            task_type,
            task_digest,
            source_path,
            action: self.action,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ImportOpenSpecItemResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawImportOpenSpecItemResult::deserialize(deserializer)?;
        let object = ImportOpenSpecItemObject::try_from_raw_parts(
            raw.object_type,
            raw.object_id,
            raw.openspec_task_id,
            raw.title,
            raw.task_type,
            raw.task_digest,
            raw.source_path,
        )
        .map_err(serde::de::Error::custom)?;
        object
            .validate_action(raw.action)
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            action: raw.action,
            object,
        })
    }
}

/// Import action assigned to one logical OpenSpec/Covey record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ImportOpenSpecAction {
    Created,
    Updated,
    Unchanged,
    Conflict,
}

/// Conflict reported by the OpenSpec importer without mutating unsafe records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOpenSpecConflict {
    pub reason: ImportOpenSpecConflictReason,
    pub source_path: OpenSpecPath,
    object: ImportOpenSpecConflictObject,
}

/// Machine-readable reason for an OpenSpec import conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ImportOpenSpecConflictReason {
    ActiveClaimChangedSource,
    ExistingSubtaskDifferentMetaTask,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ImportOpenSpecConflictObject {
    Subtask {
        object_id: SubtaskId,
        openspec_task_id: OpenSpecTaskId,
        task_digest: OpenSpecDigest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawImportOpenSpecConflict {
    object_type: ObjectType,
    object_id: String,
    openspec_task_id: Option<String>,
    reason: ImportOpenSpecConflictReason,
    source_path: String,
    task_digest: Option<String>,
}

impl ImportOpenSpecConflict {
    /// Builds a subtask-scoped OpenSpec import conflict.
    #[must_use]
    pub fn subtask(
        object_id: impl Into<String>,
        openspec_task_id: impl Into<String>,
        reason: ImportOpenSpecConflictReason,
        source_path: impl Into<String>,
        task_digest: impl Into<String>,
    ) -> Result<Self, String> {
        Ok(Self {
            reason,
            source_path: OpenSpecPath::parse(source_path.into())?,
            object: ImportOpenSpecConflictObject::Subtask {
                object_id: SubtaskId::parse(object_id.into()).map_err(|err| err.to_string())?,
                openspec_task_id: OpenSpecTaskId::parse(openspec_task_id.into())?,
                task_digest: OpenSpecDigest::parse(task_digest.into())
                    .map_err(|err| err.to_string())?,
            },
        })
    }

    /// Returns the conflicting Covey object type.
    #[must_use]
    pub const fn object_type(&self) -> ObjectType {
        self.object.object_type()
    }

    /// Returns the conflicting Covey object id.
    #[must_use]
    pub fn object_id(&self) -> &str {
        self.object.object_id()
    }

    /// Returns the OpenSpec task id for this subtask conflict.
    #[must_use]
    pub fn openspec_task_id(&self) -> &str {
        match &self.object {
            ImportOpenSpecConflictObject::Subtask {
                openspec_task_id, ..
            } => openspec_task_id.as_str(),
        }
    }

    /// Returns the source task digest for this subtask conflict.
    #[must_use]
    pub fn task_digest(&self) -> &str {
        match &self.object {
            ImportOpenSpecConflictObject::Subtask { task_digest, .. } => task_digest.as_str(),
        }
    }

    /// Returns the typed conflict reason.
    #[must_use]
    pub const fn reason(&self) -> ImportOpenSpecConflictReason {
        self.reason
    }
}

impl ImportOpenSpecConflictObject {
    fn try_from_raw_parts(
        object_type: ObjectType,
        object_id: String,
        openspec_task_id: Option<String>,
        task_digest: Option<String>,
    ) -> Result<Self, String> {
        match object_type {
            ObjectType::Subtask => {
                let openspec_task_id = openspec_task_id.ok_or_else(|| {
                    "subtask OpenSpec import conflicts require openspec_task_id".to_owned()
                })?;
                let task_digest = task_digest.ok_or_else(|| {
                    "subtask OpenSpec import conflicts require task_digest".to_owned()
                })?;
                Ok(Self::Subtask {
                    object_id: SubtaskId::parse(object_id).map_err(|err| err.to_string())?,
                    openspec_task_id: OpenSpecTaskId::parse(openspec_task_id)?,
                    task_digest: OpenSpecDigest::parse(task_digest)
                        .map_err(|err| err.to_string())?,
                })
            }
            _ => Err("OpenSpec import conflicts only support subtask objects".into()),
        }
    }

    const fn object_type(&self) -> ObjectType {
        match self {
            Self::Subtask { .. } => ObjectType::Subtask,
        }
    }

    fn object_id(&self) -> &str {
        match self {
            Self::Subtask { object_id, .. } => object_id.as_str(),
        }
    }
}

impl Serialize for ImportOpenSpecConflict {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawImportOpenSpecConflict {
            object_type: self.object_type(),
            object_id: self.object_id().to_owned(),
            openspec_task_id: Some(self.openspec_task_id().to_owned()),
            reason: self.reason,
            source_path: self.source_path.to_string(),
            task_digest: Some(self.task_digest().to_owned()),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ImportOpenSpecConflict {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawImportOpenSpecConflict::deserialize(deserializer)?;
        let object = ImportOpenSpecConflictObject::try_from_raw_parts(
            raw.object_type,
            raw.object_id,
            raw.openspec_task_id,
            raw.task_digest,
        )
        .map_err(serde::de::Error::custom)?;
        Ok(Self {
            reason: raw.reason,
            source_path: OpenSpecPath::parse(raw.source_path).map_err(serde::de::Error::custom)?,
            object,
        })
    }
}

/// Persisted provenance for records imported from OpenSpec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSpecImportProvenance {
    common: OpenSpecImportProvenanceCommon,
    object: OpenSpecImportProvenanceObject,
}

/// Object-independent OpenSpec provenance fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSpecImportProvenanceCommon {
    openspec_change_id: OpenSpecChangeId,
    openspec_change_path: OpenSpecPath,
    tasks_digest: OpenSpecDigest,
    source_digests: Vec<OpenSpecSourceDigest>,
    mission_artifact_digests: Vec<OpenSpecSourceDigest>,
    mission_artifacts: Vec<OpenSpecPath>,
    updated_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OpenSpecImportProvenanceObject {
    MetaTask {
        object_id: MetaTaskId,
        proposal_digest: OpenSpecDigest,
        design_digest: OpenSpecDigest,
        spec_digests: Vec<OpenSpecSourceDigest>,
    },
    Subtask {
        object_id: SubtaskId,
        openspec_task_id: OpenSpecTaskId,
        task_digest: OpenSpecDigest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RawOpenSpecImportProvenance {
    pub(crate) object_type: ObjectType,
    pub(crate) object_id: String,
    pub(crate) planning_format: String,
    pub(crate) openspec_change_id: String,
    pub(crate) openspec_change_path: String,
    pub(crate) openspec_task_id: Option<String>,
    pub(crate) proposal_digest: Option<String>,
    pub(crate) design_digest: Option<String>,
    pub(crate) tasks_digest: String,
    pub(crate) spec_digests: Vec<OpenSpecSourceDigest>,
    pub(crate) source_digests: Vec<OpenSpecSourceDigest>,
    pub(crate) mission_artifact_digests: Vec<OpenSpecSourceDigest>,
    pub(crate) mission_artifacts: Vec<String>,
    pub(crate) task_digest: Option<String>,
    pub(crate) updated_at: TimestampMs,
}

impl OpenSpecImportProvenanceCommon {
    const PLANNING_FORMAT: &'static str = "openspec";

    /// Builds common OpenSpec provenance fields.
    pub fn new(
        openspec_change_id: impl Into<String>,
        openspec_change_path: impl Into<String>,
        tasks_digest: impl Into<String>,
        source_digests: Vec<OpenSpecSourceDigest>,
        mission_artifact_digests: Vec<OpenSpecSourceDigest>,
        mission_artifacts: Vec<String>,
        updated_at: TimestampMs,
    ) -> Result<Self, String> {
        let mission_artifacts = mission_artifacts
            .into_iter()
            .map(OpenSpecPath::parse)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            openspec_change_id: OpenSpecChangeId::parse(openspec_change_id.into())
                .map_err(|err| err.to_string())?,
            openspec_change_path: OpenSpecPath::parse(openspec_change_path.into())?,
            tasks_digest: OpenSpecDigest::parse(tasks_digest.into())
                .map_err(|err| err.to_string())?,
            source_digests,
            mission_artifact_digests,
            mission_artifacts,
            updated_at,
        })
    }

    fn from_raw_parts(
        planning_format: String,
        openspec_change_id: String,
        openspec_change_path: String,
        tasks_digest: String,
        source_digests: Vec<OpenSpecSourceDigest>,
        mission_artifact_digests: Vec<OpenSpecSourceDigest>,
        mission_artifacts: Vec<String>,
        updated_at: TimestampMs,
    ) -> Result<Self, String> {
        if planning_format != Self::PLANNING_FORMAT {
            return Err("OpenSpec provenance planning_format must be openspec".to_owned());
        }
        Self::new(
            openspec_change_id,
            openspec_change_path,
            tasks_digest,
            source_digests,
            mission_artifact_digests,
            mission_artifacts,
            updated_at,
        )
    }

    #[must_use]
    pub const fn planning_format(&self) -> &'static str {
        Self::PLANNING_FORMAT
    }

    #[must_use]
    pub fn openspec_change_id(&self) -> &str {
        self.openspec_change_id.as_str()
    }

    #[must_use]
    pub fn openspec_change_path(&self) -> &str {
        self.openspec_change_path.as_str()
    }

    #[must_use]
    pub fn tasks_digest(&self) -> &str {
        self.tasks_digest.as_str()
    }

    #[must_use]
    pub fn source_digests(&self) -> &[OpenSpecSourceDigest] {
        &self.source_digests
    }

    #[must_use]
    pub fn mission_artifact_digests(&self) -> &[OpenSpecSourceDigest] {
        &self.mission_artifact_digests
    }

    #[must_use]
    pub fn mission_artifacts(&self) -> &[OpenSpecPath] {
        &self.mission_artifacts
    }

    #[must_use]
    pub const fn updated_at(&self) -> TimestampMs {
        self.updated_at
    }

    fn set_updated_at(&mut self, updated_at: TimestampMs) {
        self.updated_at = updated_at;
    }
}

impl OpenSpecImportProvenance {
    /// Builds MetaTask provenance from validated object-specific evidence.
    #[must_use]
    pub fn meta_task(
        common: OpenSpecImportProvenanceCommon,
        object_id: impl Into<String>,
        proposal_digest: String,
        design_digest: String,
        spec_digests: Vec<OpenSpecSourceDigest>,
    ) -> Result<Self, String> {
        Ok(Self {
            common,
            object: OpenSpecImportProvenanceObject::MetaTask {
                object_id: MetaTaskId::parse(object_id.into()).map_err(|err| err.to_string())?,
                proposal_digest: OpenSpecDigest::parse(proposal_digest)
                    .map_err(|err| err.to_string())?,
                design_digest: OpenSpecDigest::parse(design_digest)
                    .map_err(|err| err.to_string())?,
                spec_digests,
            },
        })
    }

    /// Builds Subtask provenance from validated task evidence.
    #[must_use]
    pub fn subtask(
        common: OpenSpecImportProvenanceCommon,
        object_id: impl Into<String>,
        openspec_task_id: String,
        task_digest: String,
    ) -> Result<Self, String> {
        Ok(Self {
            common,
            object: OpenSpecImportProvenanceObject::Subtask {
                object_id: SubtaskId::parse(object_id.into()).map_err(|err| err.to_string())?,
                openspec_task_id: OpenSpecTaskId::parse(openspec_task_id)?,
                task_digest: OpenSpecDigest::parse(task_digest).map_err(|err| err.to_string())?,
            },
        })
    }

    /// Parses the flat persisted/wire shape into object-kind-specific provenance.
    pub(crate) fn from_raw(raw: RawOpenSpecImportProvenance) -> Result<Self, String> {
        let object = OpenSpecImportProvenanceObject::try_from_raw_parts(
            raw.object_type,
            raw.object_id,
            raw.openspec_task_id,
            raw.proposal_digest,
            raw.design_digest,
            raw.spec_digests,
            raw.task_digest,
        )?;
        Ok(Self {
            common: OpenSpecImportProvenanceCommon::from_raw_parts(
                raw.planning_format,
                raw.openspec_change_id,
                raw.openspec_change_path,
                raw.tasks_digest,
                raw.source_digests,
                raw.mission_artifact_digests,
                raw.mission_artifacts,
                raw.updated_at,
            )?,
            object,
        })
    }

    /// Returns a flat persisted/wire representation.
    #[must_use]
    pub(crate) fn to_raw(&self) -> RawOpenSpecImportProvenance {
        RawOpenSpecImportProvenance {
            object_type: self.object_type(),
            object_id: self.object_id().to_owned(),
            planning_format: self.planning_format().to_owned(),
            openspec_change_id: self.openspec_change_id().to_owned(),
            openspec_change_path: self.openspec_change_path().to_owned(),
            openspec_task_id: self.openspec_task_id().map(str::to_owned),
            proposal_digest: self.proposal_digest().map(str::to_owned),
            design_digest: self.design_digest().map(str::to_owned),
            tasks_digest: self.tasks_digest().to_owned(),
            spec_digests: self.spec_digests().to_vec(),
            source_digests: self.source_digests().to_vec(),
            mission_artifact_digests: self.mission_artifact_digests().to_vec(),
            mission_artifacts: self
                .mission_artifacts()
                .iter()
                .map(ToString::to_string)
                .collect(),
            task_digest: self.task_digest().map(str::to_owned),
            updated_at: self.updated_at(),
        }
    }

    /// Returns the imported Covey object type.
    #[must_use]
    pub const fn object_type(&self) -> ObjectType {
        self.object.object_type()
    }

    #[must_use]
    pub fn object_id(&self) -> &str {
        self.object.object_id()
    }

    #[must_use]
    pub fn planning_format(&self) -> &str {
        self.common.planning_format()
    }

    #[must_use]
    pub fn openspec_change_id(&self) -> &str {
        self.common.openspec_change_id()
    }

    #[must_use]
    pub fn openspec_change_path(&self) -> &str {
        self.common.openspec_change_path()
    }

    #[must_use]
    pub fn openspec_task_id(&self) -> Option<&str> {
        match &self.object {
            OpenSpecImportProvenanceObject::MetaTask { .. } => None,
            OpenSpecImportProvenanceObject::Subtask {
                openspec_task_id, ..
            } => Some(openspec_task_id.as_str()),
        }
    }

    #[must_use]
    pub fn proposal_digest(&self) -> Option<&str> {
        match &self.object {
            OpenSpecImportProvenanceObject::MetaTask {
                proposal_digest, ..
            } => Some(proposal_digest.as_str()),
            OpenSpecImportProvenanceObject::Subtask { .. } => None,
        }
    }

    #[must_use]
    pub fn design_digest(&self) -> Option<&str> {
        match &self.object {
            OpenSpecImportProvenanceObject::MetaTask { design_digest, .. } => {
                Some(design_digest.as_str())
            }
            OpenSpecImportProvenanceObject::Subtask { .. } => None,
        }
    }

    #[must_use]
    pub fn tasks_digest(&self) -> &str {
        self.common.tasks_digest()
    }

    #[must_use]
    pub fn spec_digests(&self) -> &[OpenSpecSourceDigest] {
        match &self.object {
            OpenSpecImportProvenanceObject::MetaTask { spec_digests, .. } => spec_digests,
            OpenSpecImportProvenanceObject::Subtask { .. } => &[],
        }
    }

    #[must_use]
    pub fn source_digests(&self) -> &[OpenSpecSourceDigest] {
        self.common.source_digests()
    }

    #[must_use]
    pub fn mission_artifact_digests(&self) -> &[OpenSpecSourceDigest] {
        self.common.mission_artifact_digests()
    }

    #[must_use]
    pub fn mission_artifacts(&self) -> &[OpenSpecPath] {
        self.common.mission_artifacts()
    }

    #[must_use]
    pub fn task_digest(&self) -> Option<&str> {
        match &self.object {
            OpenSpecImportProvenanceObject::MetaTask { .. } => None,
            OpenSpecImportProvenanceObject::Subtask { task_digest, .. } => {
                Some(task_digest.as_str())
            }
        }
    }

    #[must_use]
    pub const fn updated_at(&self) -> TimestampMs {
        self.common.updated_at()
    }

    pub fn set_updated_at(&mut self, updated_at: TimestampMs) {
        self.common.set_updated_at(updated_at);
    }
}

impl OpenSpecImportProvenanceObject {
    fn try_from_raw_parts(
        object_type: ObjectType,
        object_id: String,
        openspec_task_id: Option<String>,
        proposal_digest: Option<String>,
        design_digest: Option<String>,
        spec_digests: Vec<OpenSpecSourceDigest>,
        task_digest: Option<String>,
    ) -> Result<Self, String> {
        match object_type {
            ObjectType::MetaTask => {
                if openspec_task_id.is_some() || task_digest.is_some() {
                    return Err("metatask OpenSpec provenance must not include task fields".into());
                }
                let proposal_digest = proposal_digest.ok_or_else(|| {
                    "metatask OpenSpec provenance requires proposal_digest".to_owned()
                })?;
                let design_digest = design_digest.ok_or_else(|| {
                    "metatask OpenSpec provenance requires design_digest".to_owned()
                })?;
                Ok(Self::MetaTask {
                    object_id: MetaTaskId::parse(object_id).map_err(|err| err.to_string())?,
                    proposal_digest: OpenSpecDigest::parse(proposal_digest)
                        .map_err(|err| err.to_string())?,
                    design_digest: OpenSpecDigest::parse(design_digest)
                        .map_err(|err| err.to_string())?,
                    spec_digests,
                })
            }
            ObjectType::Subtask => {
                if proposal_digest.is_some() || design_digest.is_some() || !spec_digests.is_empty()
                {
                    return Err(
                        "subtask OpenSpec provenance must not include metatask fields".into(),
                    );
                }
                let openspec_task_id = openspec_task_id.ok_or_else(|| {
                    "subtask OpenSpec provenance requires openspec_task_id".to_owned()
                })?;
                let task_digest = task_digest
                    .ok_or_else(|| "subtask OpenSpec provenance requires task_digest".to_owned())?;
                Ok(Self::Subtask {
                    object_id: SubtaskId::parse(object_id).map_err(|err| err.to_string())?,
                    openspec_task_id: OpenSpecTaskId::parse(openspec_task_id)?,
                    task_digest: OpenSpecDigest::parse(task_digest)
                        .map_err(|err| err.to_string())?,
                })
            }
            _ => Err("OpenSpec provenance only supports metatask and subtask objects".into()),
        }
    }

    const fn object_type(&self) -> ObjectType {
        match self {
            Self::MetaTask { .. } => ObjectType::MetaTask,
            Self::Subtask { .. } => ObjectType::Subtask,
        }
    }

    fn object_id(&self) -> &str {
        match self {
            Self::MetaTask { object_id, .. } => object_id.as_str(),
            Self::Subtask { object_id, .. } => object_id.as_str(),
        }
    }
}

impl Serialize for OpenSpecImportProvenance {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_raw().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OpenSpecImportProvenance {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawOpenSpecImportProvenance::deserialize(deserializer)?;
        Self::from_raw(raw).map_err(serde::de::Error::custom)
    }
}

/// Digest for one OpenSpec source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSpecSourceDigest {
    path: OpenSpecPath,
    digest: OpenSpecDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawOpenSpecSourceDigest {
    path: String,
    digest: String,
}

impl OpenSpecSourceDigest {
    /// Builds a source digest from a normalized relative path and blake3 digest.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is empty, absolute, escapes upward, or
    /// when the digest is not a `blake3:` value.
    pub fn new(path: impl Into<String>, digest: impl Into<String>) -> Result<Self, String> {
        let path = path.into();
        let digest = digest.into();
        Ok(Self {
            path: OpenSpecPath::parse(path)?,
            digest: OpenSpecDigest::parse(digest).map_err(|err| err.to_string())?,
        })
    }

    /// Returns the normalized relative source path.
    #[must_use]
    pub fn path(&self) -> &str {
        self.path.as_str()
    }

    /// Returns the source digest.
    #[must_use]
    pub fn digest(&self) -> &str {
        self.digest.as_str()
    }
}

impl From<&OpenSpecSourceDigest> for RawOpenSpecSourceDigest {
    fn from(digest: &OpenSpecSourceDigest) -> Self {
        Self {
            path: digest.path.to_string(),
            digest: digest.digest.to_string(),
        }
    }
}

impl Serialize for OpenSpecSourceDigest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawOpenSpecSourceDigest::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OpenSpecSourceDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawOpenSpecSourceDigest::deserialize(deserializer)?;
        Self::new(raw.path, raw.digest).map_err(serde::de::Error::custom)
    }
}

/// Event payload emitted for OpenSpec import creates and updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOpenSpecEvent {
    action: ImportOpenSpecAction,
    provenance: OpenSpecImportProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawImportOpenSpecEvent {
    change_id: String,
    object_type: ObjectType,
    object_id: String,
    openspec_task_id: Option<String>,
    action: ImportOpenSpecAction,
    provenance: OpenSpecImportProvenance,
}

impl ImportOpenSpecEvent {
    /// Builds an OpenSpec import event from its canonical provenance.
    #[must_use]
    pub const fn new(action: ImportOpenSpecAction, provenance: OpenSpecImportProvenance) -> Self {
        Self { action, provenance }
    }

    /// Returns the imported OpenSpec change id.
    #[must_use]
    pub fn change_id(&self) -> &str {
        self.provenance.openspec_change_id()
    }

    /// Returns the imported Covey object type.
    #[must_use]
    pub const fn object_type(&self) -> ObjectType {
        self.provenance.object_type()
    }

    /// Returns the imported Covey object id.
    #[must_use]
    pub fn object_id(&self) -> &str {
        self.provenance.object_id()
    }

    /// Returns the OpenSpec task id for subtask import events.
    #[must_use]
    pub fn openspec_task_id(&self) -> Option<&str> {
        self.provenance.openspec_task_id()
    }

    /// Returns the import action that produced the event.
    #[must_use]
    pub const fn action(&self) -> ImportOpenSpecAction {
        self.action
    }

    /// Returns canonical provenance for the imported object.
    #[must_use]
    pub const fn provenance(&self) -> &OpenSpecImportProvenance {
        &self.provenance
    }
}

impl Serialize for ImportOpenSpecEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawImportOpenSpecEvent {
            change_id: self.change_id().to_owned(),
            object_type: self.object_type(),
            object_id: self.object_id().to_owned(),
            openspec_task_id: self.openspec_task_id().map(str::to_owned),
            action: self.action(),
            provenance: self.provenance.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ImportOpenSpecEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawImportOpenSpecEvent::deserialize(deserializer)?;
        if raw.change_id != raw.provenance.openspec_change_id() {
            return Err(serde::de::Error::custom(
                "OpenSpec import event change_id must match provenance",
            ));
        }
        if raw.object_type != raw.provenance.object_type() {
            return Err(serde::de::Error::custom(
                "OpenSpec import event object_type must match provenance",
            ));
        }
        if raw.object_id != raw.provenance.object_id() {
            return Err(serde::de::Error::custom(
                "OpenSpec import event object_id must match provenance",
            ));
        }
        if raw.openspec_task_id.as_deref() != raw.provenance.openspec_task_id() {
            return Err(serde::de::Error::custom(
                "OpenSpec import event openspec_task_id must match provenance",
            ));
        }
        Ok(Self {
            action: raw.action,
            provenance: raw.provenance,
        })
    }
}

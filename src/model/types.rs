use std::{
    borrow::{Borrow, Cow},
    fmt,
    ops::{Add, Deref, Sub},
    str::FromStr,
};

use rusqlite::{
    ToSql,
    types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Validation failure for strongly typed Covey scalar values.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid {field}: {reason}")]
pub struct CoveyTypeValidationError {
    field: &'static str,
    reason: &'static str,
}

impl CoveyTypeValidationError {
    pub(crate) const fn new(field: &'static str, reason: &'static str) -> Self {
        Self { field, reason }
    }

    pub(crate) const fn reason(&self) -> &'static str {
        self.reason
    }
}

fn validate_tokenish(field: &'static str, value: &str) -> Result<(), CoveyTypeValidationError> {
    if value.trim().is_empty() {
        return Err(CoveyTypeValidationError::new(field, "must not be empty"));
    }
    if value.len() > 256 {
        return Err(CoveyTypeValidationError::new(field, "exceeds 256 bytes"));
    }
    if value
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(CoveyTypeValidationError::new(
            field,
            "must not contain whitespace or control characters",
        ));
    }
    Ok(())
}

fn validate_digest(field: &'static str, value: &str) -> Result<(), CoveyTypeValidationError> {
    validate_tokenish(field, value)?;
    let Some((algorithm, digest)) = value.split_once(':') else {
        return Err(CoveyTypeValidationError::new(
            field,
            "must include an algorithm prefix",
        ));
    };
    if algorithm.is_empty() || digest.is_empty() {
        return Err(CoveyTypeValidationError::new(
            field,
            "algorithm and digest must be non-empty",
        ));
    }
    if !algorithm
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
    {
        return Err(CoveyTypeValidationError::new(
            field,
            "algorithm prefix contains invalid characters",
        ));
    }
    if !digest
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(CoveyTypeValidationError::new(
            field,
            "digest contains invalid characters",
        ));
    }
    Ok(())
}

fn validate_blake3_digest(
    field: &'static str,
    value: &str,
) -> Result<(), CoveyTypeValidationError> {
    validate_digest(field, value)?;
    let Some((algorithm, _)) = value.split_once(':') else {
        return Err(CoveyTypeValidationError::new(
            field,
            "must include an algorithm prefix",
        ));
    };
    if algorithm != "blake3" {
        return Err(CoveyTypeValidationError::new(
            field,
            "must use blake3: prefix",
        ));
    }
    Ok(())
}

fn validate_manifest_path(
    field: &'static str,
    value: &str,
) -> Result<(), CoveyTypeValidationError> {
    const MAX_MANIFEST_PATH_LEN: usize = 4 * 1024;

    if value.trim().is_empty() {
        return Err(CoveyTypeValidationError::new(field, "must not be empty"));
    }
    if value.len() > MAX_MANIFEST_PATH_LEN {
        return Err(CoveyTypeValidationError::new(field, "exceeds 4096 bytes"));
    }
    if value.chars().any(char::is_control) {
        return Err(CoveyTypeValidationError::new(
            field,
            "must not contain control characters",
        ));
    }
    Ok(())
}

fn validate_normalized_text(
    field: &'static str,
    value: &str,
) -> Result<(), CoveyTypeValidationError> {
    if value.trim().is_empty() {
        return Err(CoveyTypeValidationError::new(field, "must not be empty"));
    }
    if value.len() > 1024 {
        return Err(CoveyTypeValidationError::new(field, "exceeds 1024 bytes"));
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

fn validate_commit_oid(field: &'static str, value: &str) -> Result<(), CoveyTypeValidationError> {
    if !(7..=64).contains(&value.len()) {
        return Err(CoveyTypeValidationError::new(
            field,
            "must be 7 to 64 hexadecimal characters",
        ));
    }
    if !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(CoveyTypeValidationError::new(
            field,
            "must contain only hexadecimal characters",
        ));
    }
    Ok(())
}

fn validate_prompt_text(field: &'static str, value: &str) -> Result<(), CoveyTypeValidationError> {
    const MAX_PROMPT_TEXT_LEN: usize = 32 * 1024;

    if value.trim().is_empty() {
        return Err(CoveyTypeValidationError::new(field, "must not be empty"));
    }
    if value.len() > MAX_PROMPT_TEXT_LEN {
        return Err(CoveyTypeValidationError::new(field, "exceeds 32768 bytes"));
    }
    Ok(())
}

fn validate_subtask_title(
    field: &'static str,
    value: &str,
) -> Result<(), CoveyTypeValidationError> {
    const MAX_SUBTASK_TITLE_LEN: usize = 512;

    if value.trim().is_empty() {
        return Err(CoveyTypeValidationError::new(field, "must not be empty"));
    }
    if value.len() > MAX_SUBTASK_TITLE_LEN {
        return Err(CoveyTypeValidationError::new(field, "exceeds 512 bytes"));
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

fn validate_idempotency_key(
    field: &'static str,
    value: &str,
) -> Result<(), CoveyTypeValidationError> {
    const MAX_IDEMPOTENCY_KEY_LEN: usize = 256;

    if value.trim().is_empty() {
        return Err(CoveyTypeValidationError::new(field, "must not be empty"));
    }
    if value.len() > MAX_IDEMPOTENCY_KEY_LEN {
        return Err(CoveyTypeValidationError::new(field, "exceeds 256 bytes"));
    }
    Ok(())
}

fn validate_openspec_change_id(
    field: &'static str,
    value: &str,
) -> Result<(), CoveyTypeValidationError> {
    if value.is_empty()
        || value.starts_with('-')
        || value.ends_with('-')
        || !value
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(CoveyTypeValidationError::new(
            field,
            "must be kebab-case ASCII",
        ));
    }
    Ok(())
}

macro_rules! string_newtype {
    ($name:ident, $field:literal, $validator:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Parses and validates a raw wire/storage value into this domain type.
            pub fn parse(value: impl Into<String>) -> Result<Self, CoveyTypeValidationError> {
                Self::try_from(value.into())
            }

            /// Returns the validated string value.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = CoveyTypeValidationError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                $validator($field, &value)?;
                Ok(Self(value))
            }
        }

        impl FromStr for $name {
            type Err = CoveyTypeValidationError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value.to_owned())
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.as_str()
            }
        }

        impl Deref for $name {
            type Target = str;

            fn deref(&self) -> &Self::Target {
                self.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                self.0.to_sql()
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                let raw = String::column_result(value)?;
                Self::try_from(raw).map_err(|err| FromSqlError::Other(Box::new(err)))
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                self.as_str() == other
            }
        }

        impl PartialEq<$name> for String {
            fn eq(&self, other: &$name) -> bool {
                self == other.as_str()
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.as_str() == *other
            }
        }

        impl PartialEq<$name> for &str {
            fn eq(&self, other: &$name) -> bool {
                *self == other.as_str()
            }
        }
    };
}

macro_rules! i64_newtype {
    ($name:ident, $field:literal, $validator:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(try_from = "i64", into = "i64")]
        pub struct $name(i64);

        impl $name {
            /// Parses and validates a raw wire/storage value into this domain type.
            pub fn parse(value: i64) -> Result<Self, CoveyTypeValidationError> {
                Self::try_from(value)
            }

            /// Returns the validated integer value.
            #[must_use]
            pub const fn get(self) -> i64 {
                self.0
            }
        }

        impl TryFrom<i64> for $name {
            type Error = CoveyTypeValidationError;

            fn try_from(value: i64) -> Result<Self, Self::Error> {
                $validator($field, value)?;
                Ok(Self(value))
            }
        }

        impl From<$name> for i64 {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                self.0.to_sql()
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                let raw = i64::column_result(value)?;
                Self::try_from(raw).map_err(|err| FromSqlError::Other(Box::new(err)))
            }
        }

        impl PartialEq<i64> for $name {
            fn eq(&self, other: &i64) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<$name> for i64 {
            fn eq(&self, other: &$name) -> bool {
                *self == other.0
            }
        }

        impl PartialOrd<i64> for $name {
            fn partial_cmp(&self, other: &i64) -> Option<std::cmp::Ordering> {
                self.0.partial_cmp(other)
            }
        }

        impl PartialOrd<$name> for i64 {
            fn partial_cmp(&self, other: &$name) -> Option<std::cmp::Ordering> {
                self.partial_cmp(&other.0)
            }
        }

        impl Sub<i64> for $name {
            type Output = i64;

            fn sub(self, rhs: i64) -> Self::Output {
                self.0 - rhs
            }
        }

        impl Sub<$name> for i64 {
            type Output = i64;

            fn sub(self, rhs: $name) -> Self::Output {
                self - rhs.0
            }
        }

        impl Add<i64> for $name {
            type Output = Self;

            fn add(self, rhs: i64) -> Self::Output {
                Self::try_from(self.0 + rhs)
                    .expect("validated integer newtype addition must preserve invariants")
            }
        }
    };
}

fn validate_positive_i64(field: &'static str, value: i64) -> Result<(), CoveyTypeValidationError> {
    if value <= 0 {
        return Err(CoveyTypeValidationError::new(field, "must be positive"));
    }
    Ok(())
}

fn validate_non_negative_i64(
    field: &'static str,
    value: i64,
) -> Result<(), CoveyTypeValidationError> {
    if value < 0 {
        return Err(CoveyTypeValidationError::new(field, "must be non-negative"));
    }
    Ok(())
}

fn validate_subtask_priority(
    field: &'static str,
    value: i64,
) -> Result<(), CoveyTypeValidationError> {
    if !(0..=1000).contains(&value) {
        return Err(CoveyTypeValidationError::new(
            field,
            "must be between 0 and 1000",
        ));
    }
    Ok(())
}

/// Normalized repo-relative path used by repoops snapshot requests.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RepoopsPath(String);

impl RepoopsPath {
    /// Parses and normalizes a raw path into a repo-relative path.
    pub fn parse(value: impl Into<String>) -> Result<Self, CoveyTypeValidationError> {
        Self::try_from(value.into())
    }

    /// Returns the normalized repo-relative path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RepoopsPath {
    type Error = CoveyTypeValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.chars().any(char::is_control) {
            return Err(CoveyTypeValidationError::new(
                "path",
                "must not contain control characters",
            ));
        }
        let mut normalized = if value.as_bytes().contains(&b'\\') {
            Cow::Owned(value.trim().replace('\\', "/"))
        } else {
            Cow::Borrowed(value.trim())
        };
        while let Some(rest) = normalized.strip_prefix("./") {
            normalized = Cow::Owned(rest.to_owned());
        }
        while let Some(rest) = normalized.strip_prefix('/') {
            normalized = Cow::Owned(rest.to_owned());
        }
        let mut parts =
            Vec::with_capacity(normalized.bytes().filter(|byte| *byte == b'/').count() + 1);
        for part in normalized.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    return Err(CoveyTypeValidationError::new(
                        "path",
                        "must not traverse outside the repository",
                    ));
                }
                _ => parts.push(part),
            }
        }
        if parts.is_empty() {
            return Err(CoveyTypeValidationError::new("path", "must not be empty"));
        }
        Ok(Self(parts.join("/")))
    }
}

impl From<RepoopsPath> for String {
    fn from(value: RepoopsPath) -> Self {
        value.0
    }
}

impl AsRef<str> for RepoopsPath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for RepoopsPath {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl Deref for RepoopsPath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for RepoopsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<String> for RepoopsPath {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<RepoopsPath> for String {
    fn eq(&self, other: &RepoopsPath) -> bool {
        self == other.as_str()
    }
}

impl PartialEq<&str> for RepoopsPath {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<RepoopsPath> for &str {
    fn eq(&self, other: &RepoopsPath) -> bool {
        *self == other.as_str()
    }
}

string_newtype!(SessionToken, "session_token", validate_tokenish);
string_newtype!(AgentPrincipalId, "agent_principal_id", validate_tokenish);
string_newtype!(AgentInstanceId, "agent_instance_id", validate_tokenish);
string_newtype!(MetaTaskId, "meta_task_id", validate_tokenish);
string_newtype!(PromptText, "prompt_text", validate_prompt_text);
string_newtype!(SubtaskId, "subtask_id", validate_tokenish);
string_newtype!(SubtaskTitle, "title", validate_subtask_title);
string_newtype!(ClaimId, "claim_id", validate_tokenish);
string_newtype!(RepoopsClaimRef, "repoops_claim_ref", validate_tokenish);
string_newtype!(QueueId, "queue_id", validate_tokenish);
string_newtype!(ReviewId, "review_id", validate_tokenish);
string_newtype!(ReservationId, "reservation_id", validate_tokenish);
string_newtype!(ConflictId, "conflict_id", validate_tokenish);
string_newtype!(EventObjectId, "event_object_id", validate_tokenish);
string_newtype!(ArtifactDigest, "artifact_digest", validate_digest);
string_newtype!(
    ArtifactManifestPath,
    "manifest_path",
    validate_manifest_path
);
string_newtype!(ChangedPathsDigest, "changed_paths_digest", validate_digest);
string_newtype!(FindingsDigest, "findings_digest", validate_digest);
string_newtype!(OpenSpecDigest, "openspec_digest", validate_blake3_digest);
string_newtype!(BaseRev, "base_rev", validate_tokenish);
string_newtype!(LandedCommitOid, "landed_commit_oid", validate_commit_oid);
string_newtype!(ProviderId, "provider", validate_tokenish);
string_newtype!(ModelId, "model", validate_tokenish);
string_newtype!(ProviderRunId, "provider_run_id", validate_normalized_text);
string_newtype!(
    ProviderRunIdIssuer,
    "provider_run_id_issuer",
    validate_normalized_text
);
string_newtype!(RuntimeProcessId, "process_id", validate_normalized_text);
string_newtype!(RuntimeContainerId, "container_id", validate_normalized_text);
string_newtype!(VerifierId, "verifier", validate_tokenish);
string_newtype!(IdempotencyKey, "idempotency_key", validate_idempotency_key);
string_newtype!(
    OpenSpecChangeId,
    "openspec_change_id",
    validate_openspec_change_id
);
string_newtype!(SourceIssueId, "source_issue_id", validate_tokenish);
string_newtype!(
    CommandTranscriptDigest,
    "command_transcript_digest",
    validate_digest
);

i64_newtype!(FenceSeq, "fence_seq", validate_positive_i64);
i64_newtype!(EventSeq, "event_seq", validate_positive_i64);
i64_newtype!(
    SessionHeartbeatTick,
    "last_heartbeat_tick",
    validate_non_negative_i64
);
i64_newtype!(LeaseDurationMs, "lease_duration_ms", validate_positive_i64);
i64_newtype!(LeaseDeadlineMs, "lease_deadline", validate_non_negative_i64);
i64_newtype!(SubtaskPriority, "priority", validate_subtask_priority);
i64_newtype!(TimestampMs, "timestamp_ms", validate_non_negative_i64);

#[cfg(test)]
mod tests {
    use super::{
        ArtifactDigest, ArtifactManifestPath, ClaimId, FenceSeq, IdempotencyKey, OpenSpecChangeId,
        OpenSpecDigest, PromptText, SubtaskTitle, TimestampMs,
    };

    #[test]
    fn claim_id_rejects_empty_values() {
        assert!(ClaimId::try_from(String::new()).is_err());
    }

    #[test]
    fn artifact_digest_requires_algorithm_prefix() {
        assert!(ArtifactDigest::try_from("digestonly".to_owned()).is_err());
        assert!(ArtifactDigest::try_from("blake3:abc123".to_owned()).is_ok());
    }

    #[test]
    fn artifact_manifest_path_rejects_empty_and_control_characters() {
        assert!(ArtifactManifestPath::try_from(String::new()).is_err());
        assert!(ArtifactManifestPath::try_from("manifest\n.json".to_owned()).is_err());
        assert!(ArtifactManifestPath::try_from("artifact bundle/manifest.json".to_owned()).is_ok());
    }

    #[test]
    fn fence_seq_is_positive() {
        assert!(FenceSeq::try_from(0).is_err());
        assert!(FenceSeq::try_from(1).is_ok());
    }

    #[test]
    fn timestamps_are_non_negative() {
        assert!(TimestampMs::try_from(-1).is_err());
        assert!(TimestampMs::try_from(0).is_ok());
    }

    #[test]
    fn idempotency_keys_reject_blank_and_oversized_values() {
        assert!(IdempotencyKey::try_from("idem-1".to_owned()).is_ok());
        assert!(IdempotencyKey::try_from(" ".to_owned()).is_err());
        assert!(IdempotencyKey::try_from("x".repeat(257)).is_err());
    }

    #[test]
    fn prompt_text_rejects_blank_and_oversized_values() {
        assert!(PromptText::try_from("do work".to_owned()).is_ok());
        assert!(PromptText::try_from(" ".to_owned()).is_err());
        assert!(PromptText::try_from("x".repeat(32 * 1024 + 1)).is_err());
    }

    #[test]
    fn subtask_titles_reject_blank_padded_and_oversized_values() {
        assert!(SubtaskTitle::try_from("implement work".to_owned()).is_ok());
        assert!(SubtaskTitle::try_from(" ".to_owned()).is_err());
        assert!(SubtaskTitle::try_from(" padded".to_owned()).is_err());
        assert!(SubtaskTitle::try_from("x".repeat(513)).is_err());
    }

    #[test]
    fn openspec_change_ids_require_kebab_case_ascii() {
        assert!(OpenSpecChangeId::try_from("change-1".to_owned()).is_ok());
        assert!(OpenSpecChangeId::try_from("Change-1".to_owned()).is_err());
        assert!(OpenSpecChangeId::try_from("-change".to_owned()).is_err());
        assert!(OpenSpecChangeId::try_from("change_1".to_owned()).is_err());
    }

    #[test]
    fn openspec_digests_require_blake3_prefix() {
        assert!(OpenSpecDigest::try_from("blake3:abc123".to_owned()).is_ok());
        assert!(OpenSpecDigest::try_from("sha256:abc123".to_owned()).is_err());
        assert!(OpenSpecDigest::try_from("blake3:".to_owned()).is_err());
    }
}

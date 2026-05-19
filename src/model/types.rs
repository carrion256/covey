use std::{
    borrow::Borrow,
    fmt,
    ops::{Add, Deref, Sub},
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
    const fn new(field: &'static str, reason: &'static str) -> Self {
        Self { field, reason }
    }
}

fn validate_tokenish(field: &'static str, value: &str) -> Result<(), CoveyTypeValidationError> {
    if value.is_empty() {
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

string_newtype!(SessionToken, "session_token", validate_tokenish);
string_newtype!(MetaTaskId, "meta_task_id", validate_tokenish);
string_newtype!(SubtaskId, "subtask_id", validate_tokenish);
string_newtype!(ClaimId, "claim_id", validate_tokenish);
string_newtype!(RepoopsClaimRef, "repoops_claim_ref", validate_tokenish);
string_newtype!(QueueId, "queue_id", validate_tokenish);
string_newtype!(ReviewId, "review_id", validate_tokenish);
string_newtype!(ReservationId, "reservation_id", validate_tokenish);
string_newtype!(ArtifactDigest, "artifact_digest", validate_digest);
string_newtype!(FindingsDigest, "findings_digest", validate_digest);
string_newtype!(BaseRev, "base_rev", validate_tokenish);
string_newtype!(ProviderId, "provider", validate_tokenish);
string_newtype!(ModelId, "model", validate_tokenish);
string_newtype!(
    CommandTranscriptDigest,
    "command_transcript_digest",
    validate_digest
);

i64_newtype!(FenceSeq, "fence_seq", validate_positive_i64);
i64_newtype!(LeaseDurationMs, "lease_duration_ms", validate_positive_i64);
i64_newtype!(LeaseDeadlineMs, "lease_deadline", validate_non_negative_i64);
i64_newtype!(TimestampMs, "timestamp_ms", validate_non_negative_i64);

#[cfg(test)]
mod tests {
    use super::{ArtifactDigest, ClaimId, FenceSeq, TimestampMs};

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
    fn fence_seq_is_positive() {
        assert!(FenceSeq::try_from(0).is_err());
        assert!(FenceSeq::try_from(1).is_ok());
    }

    #[test]
    fn timestamps_are_non_negative() {
        assert!(TimestampMs::try_from(-1).is_err());
        assert!(TimestampMs::try_from(0).is_ok());
    }
}

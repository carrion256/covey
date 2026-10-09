//! Pluggable workflow profiles.
//!
//! A profile composes three already-implemented, immutable-per-task choices: the
//! completion policy, the routing lane, and whether direct attempt evidence is
//! optional or required. The built-in default is the simple flow (`direct`,
//! `default` route, optional evidence). Profiles never weaken reviewed or
//! canonical-apply governance; they only select which mode a new task uses.

use std::{
    collections::BTreeMap,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use serde::{
    Deserialize, Serialize,
    de::{self, MapAccess, Visitor},
};
use strum::{AsRefStr, Display, EnumString};

use crate::{CompletionPolicy, RoutingKey};

const DEFAULT_ROUTING_KEY: &str = "default";

/// Whether direct attempt outcomes must carry an evidence digest.
///
/// Persisted per task at creation; supplied evidence is always validated.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    EnumString,
    Display,
    AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum EvidencePolicy {
    #[default]
    Optional,
    Required,
}

/// Completion mode, routing lane, and evidence policy for newly created work.
///
/// Omitted fields mean the simple flow (`direct`, `default` route,
/// optional evidence), so `{}` is a valid simple-workflow profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkflowProfile {
    pub completion_policy: CompletionPolicy,
    pub routing_key: RoutingKey,
    pub evidence_policy: EvidencePolicy,
}

impl Default for WorkflowProfile {
    fn default() -> Self {
        Self {
            completion_policy: CompletionPolicy::Direct,
            routing_key: RoutingKey::parse(DEFAULT_ROUTING_KEY)
                .expect("the default routing key is a valid routing key"),
            evidence_policy: EvidencePolicy::Optional,
        }
    }
}

/// Errors raised while loading or resolving a workflow config file.
#[derive(Debug, thiserror::Error)]
pub enum WorkflowConfigError {
    /// The explicitly selected file could not be read.
    #[error("cannot read workflow config {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    /// JSON syntax, field types, names, or duplicate keys are invalid.
    #[error("invalid workflow config: {0}")]
    Parse(#[from] serde_json::Error),
    /// The file names a default profile that it does not define.
    #[error("default_profile `{0}` is not a defined workflow profile")]
    UnknownDefaultProfile(String),
    /// The requested profile is not defined in the file.
    #[error("unknown workflow profile `{0}`")]
    UnknownProfile(String),
    /// Neither the caller nor the file selected a profile.
    #[error("workflow config selects no profile; pass --profile or set default_profile")]
    NoProfileSelected,
    /// A profile was selected without an explicit config file.
    #[error("--profile requires --config")]
    ProfileWithoutConfig,
}

/// Strict, named workflow profiles parsed from one explicit JSON file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowConfig {
    default_profile: Option<String>,
    profiles: BTreeMap<String, WorkflowProfile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkflowConfig {
    #[serde(default)]
    default_profile: Option<String>,
    #[serde(default)]
    profiles: Profiles,
}

/// Deserializes the profiles map directly, rejecting duplicate keys in-map.
#[derive(Default)]
struct Profiles(BTreeMap<String, WorkflowProfile>);

impl<'de> Deserialize<'de> for Profiles {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MapVisitor;
        impl<'de> Visitor<'de> for MapVisitor {
            type Value = Profiles;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map of profile name to workflow profile")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                use std::collections::btree_map::Entry;
                let mut profiles = BTreeMap::new();
                while let Some((name, profile)) = map.next_entry::<String, WorkflowProfile>()? {
                    if name.is_empty() {
                        return Err(de::Error::custom(
                            "workflow profile names must not be empty",
                        ));
                    }
                    match profiles.entry(name) {
                        Entry::Occupied(duplicate) => {
                            return Err(de::Error::custom(format_args!(
                                "duplicate workflow profile `{}`",
                                duplicate.key()
                            )));
                        }
                        Entry::Vacant(vacant) => {
                            vacant.insert(profile);
                        }
                    }
                }
                Ok(Profiles(profiles))
            }
        }
        deserializer.deserialize_map(MapVisitor)
    }
}

impl WorkflowConfig {
    /// Parses and validates a config document.
    ///
    /// # Errors
    ///
    /// Rejects unknown fields, bad types, malformed routing keys, empty or
    /// duplicate profile names (rejected in-map by the profiles visitor), and
    /// a `default_profile` that is not defined.
    ///
    pub fn from_json_str(json: &str) -> Result<Self, WorkflowConfigError> {
        let raw: RawWorkflowConfig = serde_json::from_str(json)?;
        let profiles = raw.profiles.0;
        if let Some(default) = &raw.default_profile
            && !profiles.contains_key(default)
        {
            return Err(WorkflowConfigError::UnknownDefaultProfile(default.clone()));
        }
        Ok(Self {
            default_profile: raw.default_profile,
            profiles,
        })
    }

    /// Reads and parses the config file at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`WorkflowConfigError::Read`] when the file is unreadable, or any
    /// parse error from [`WorkflowConfig::from_json_str`].
    pub fn load(path: &Path) -> Result<Self, WorkflowConfigError> {
        let json = fs::read_to_string(path).map_err(|source| WorkflowConfigError::Read {
            path: path.to_owned(),
            source,
        })?;
        Self::from_json_str(&json)
    }

    /// Resolves `selected` (or the file's `default_profile`) to a profile.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected profile is undefined or neither a
    /// selection nor a `default_profile` exists.
    pub fn resolve(&self, selected: Option<&str>) -> Result<WorkflowProfile, WorkflowConfigError> {
        let name = selected
            .or(self.default_profile.as_deref())
            .ok_or(WorkflowConfigError::NoProfileSelected)?;
        self.profiles
            .get(name)
            .cloned()
            .ok_or_else(|| WorkflowConfigError::UnknownProfile(name.to_owned()))
    }
}

/// Resolves the effective workflow profile from the explicit CLI inputs.
///
/// No config and no profile is the built-in simple flow. There is no implicit
/// config discovery, and `profile` without `config` is rejected.
///
/// # Errors
///
/// Returns an error when the config is unreadable or invalid, `profile` is
/// given without `config`, or the profile cannot be resolved.
pub fn resolve_workflow(
    config: Option<&Path>,
    profile: Option<&str>,
) -> Result<WorkflowProfile, WorkflowConfigError> {
    match (config, profile) {
        (None, None) => Ok(WorkflowProfile::default()),
        (None, Some(_)) => Err(WorkflowConfigError::ProfileWithoutConfig),
        (Some(path), profile) => WorkflowConfig::load(path)?.resolve(profile),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_defaults_are_the_simple_flow() {
        let config =
            WorkflowConfig::from_json_str(r#"{"profiles":{"p":{"completion_policy":"direct"}}}"#)
                .unwrap();
        assert_eq!(
            config.resolve(Some("p")).unwrap(),
            WorkflowProfile::default()
        );
    }

    #[rstest::rstest]
    #[case(r#"{"extra":1}"#)]
    #[case(r#"{"profiles":{"p":{"unknown":1}}}"#)]
    #[case(r#"{"profiles":{"p":{"completion_policy":"bypass"}}}"#)]
    #[case(r#"{"profiles":{"p":{"evidence_policy":true}}}"#)]
    #[case(r#"{"profiles":{"p":{"routing_key":"has space"}}}"#)]
    #[case(r#"{"default_profile":"missing","profiles":{}}"#)]
    #[case(r#"{"profiles":{"p":{},"p":{"evidence_policy":"required"}}}"#)]
    #[case(r#"{"profiles":{"p":{"evidence_policy":"required","evidence_policy":"optional"}}}"#)]
    #[case(r#"{"default_profile":"strict","default_profile":"simple","profiles":{}}"#)]
    #[case(r#"{"profiles":{"":{}}}"#)]
    fn strict_parsing_fails_closed(#[case] bad: &str) {
        assert!(WorkflowConfig::from_json_str(bad).is_err(), "{bad}");
    }

    #[test]
    fn resolution_requires_an_existing_selection() {
        let config = WorkflowConfig::from_json_str(
            r#"{"default_profile":"gov","profiles":{"gov":{"completion_policy":"reviewed","evidence_policy":"required"},"fast":{"completion_policy":"direct"}}}"#,
        )
        .unwrap();
        assert_eq!(
            config.resolve(None).unwrap().completion_policy,
            CompletionPolicy::Reviewed
        );
        assert_eq!(
            config.resolve(Some("fast")).unwrap(),
            WorkflowProfile::default()
        );
        assert!(matches!(
            config.resolve(Some("nope")),
            Err(WorkflowConfigError::UnknownProfile(_))
        ));
        let no_default = WorkflowConfig::from_json_str(r#"{"profiles":{"p":{}}}"#).unwrap();
        assert!(matches!(
            no_default.resolve(None),
            Err(WorkflowConfigError::NoProfileSelected)
        ));
        assert!(matches!(
            resolve_workflow(None, Some("p")),
            Err(WorkflowConfigError::ProfileWithoutConfig)
        ));
        assert_eq!(
            resolve_workflow(None, None).unwrap(),
            WorkflowProfile::default()
        );
    }
}

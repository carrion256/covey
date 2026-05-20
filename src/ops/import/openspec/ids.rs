use crate::{
    error::{CoveyError, Result},
    validators::{MAX_OBJECT_ID_LEN, ensure_length},
};

pub(super) fn validate_openspec_change_id(change_id: &str) -> Result<()> {
    if change_id.is_empty()
        || change_id.starts_with('-')
        || change_id.ends_with('-')
        || !change_id
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(CoveyError::InvalidImportDestination {
            reason: "change id must be kebab-case ASCII".to_owned(),
        });
    }
    Ok(())
}

pub(super) fn validate_deterministic_covey_id(field: &str, id: &str) -> Result<()> {
    ensure_length(field, id, MAX_OBJECT_ID_LEN)?;
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ':' | '-' | '_' | '.'))
    {
        return Err(CoveyError::InvalidImportDestination {
            reason: format!("{field} contains unsupported characters"),
        });
    }
    Ok(())
}

pub(super) fn openspec_meta_task_id(change_id: &str) -> String {
    format!("openspec:{change_id}")
}

pub(super) fn openspec_subtask_id(change_id: &str, task_id: &str) -> String {
    format!("openspec:{change_id}:{task_id}")
}

pub(super) fn openspec_meta_prompt(source: &super::OpenSpecSourceSnapshot) -> String {
    format!(
        "OpenSpec change {}\n\nImported from {}",
        source.change_id.as_str(),
        source.change_path.as_str()
    )
}

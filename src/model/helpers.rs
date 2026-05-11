use blake3::hash as blake3_hash;
use uuid::Uuid;

pub(crate) fn make_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

/// Derives the deterministic V1 import subtask id for a bd issue id.
///
/// V1 duplicate handling is provenance-based and deterministic:
/// the same bd issue id always maps to the same Covey work-subtask id.
/// Re-importing the same source issue into the same destination therefore
/// resolves to the existing subtask instead of creating a second work item.
#[must_use]
pub(crate) fn bd_import_v1_subtask_id(source_issue_id: &str) -> String {
    let digest = blake3_hash(source_issue_id.as_bytes()).to_hex();
    let hint = bd_import_source_hint(source_issue_id);
    format!("bdwork_{hint}_{}", &digest[..16])
}

#[must_use]
fn bd_import_source_hint(source_issue_id: &str) -> String {
    let mut hint = String::with_capacity(source_issue_id.len().min(24));
    let mut last_was_separator = false;

    for ch in source_issue_id.chars() {
        if hint.len() >= 24 {
            break;
        }

        if ch.is_ascii_alphanumeric() {
            hint.push(ch.to_ascii_lowercase());
            last_was_separator = false;
            continue;
        }

        if !hint.is_empty() && !last_was_separator {
            hint.push('_');
            last_was_separator = true;
        }
    }

    while hint.ends_with('_') {
        hint.pop();
    }

    if hint.is_empty() {
        "issue".to_owned()
    } else {
        hint
    }
}

pub(crate) fn parse_generated_members(
    raw: &str,
) -> std::result::Result<Vec<String>, serde_json::Error> {
    serde_json::from_str(raw)
}

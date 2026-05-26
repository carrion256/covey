#![cfg_attr(coverage_nightly, coverage(off))]

use std::collections::HashSet;

use crate::error::{CoveyError, Result};

use super::{OpenSpecSourceTask, util::blake3_prefixed_digest};

pub(super) fn parse_openspec_tasks(
    tasks_text: &str,
    source_path: &str,
) -> Result<Vec<OpenSpecSourceTask>> {
    let mut seen = HashSet::new();
    let mut tasks = Vec::new();

    for (line_index, line) in tasks_text.lines().enumerate() {
        let trimmed = line.trim();
        let Some(rest) = task_line_rest(trimmed) else {
            if trimmed.starts_with("- [") {
                return Err(invalid_openspec_task(
                    source_path,
                    line_index + 1,
                    "malformed task checkbox",
                ));
            }
            continue;
        };

        let (task_id, title) = rest.split_once(' ').ok_or_else(|| {
            invalid_openspec_task(source_path, line_index + 1, "missing task title")
        })?;
        validate_openspec_task_id(task_id)
            .map_err(|reason| invalid_openspec_task(source_path, line_index + 1, reason))?;
        let title = title.trim();
        if title.is_empty() {
            return Err(invalid_openspec_task(
                source_path,
                line_index + 1,
                "missing task title",
            ));
        }
        if !seen.insert(task_id) {
            return Err(invalid_openspec_task(
                source_path,
                line_index + 1,
                "duplicate task id",
            ));
        }

        tasks.push(OpenSpecSourceTask::try_from_raw_parts(
            task_id.to_owned(),
            title.to_owned(),
            source_path.to_owned(),
            task_digest(task_id, title),
            None,
            Vec::new(),
        )?);
    }

    if tasks.is_empty() {
        return Err(CoveyError::InvalidSourceSchema {
            path: source_path.to_owned(),
            detail: "no stable OpenSpec task checklist entries found".to_owned(),
        });
    }

    Ok(tasks)
}

fn task_line_rest(trimmed: &str) -> Option<&str> {
    trimmed
        .strip_prefix("- [ ] ")
        .or_else(|| trimmed.strip_prefix("- [x] "))
        .or_else(|| trimmed.strip_prefix("- [X] "))
}

fn invalid_openspec_task(source_path: &str, line: usize, reason: &str) -> CoveyError {
    CoveyError::InvalidSourceSchema {
        path: source_path.to_owned(),
        detail: format!("{reason} at line {line}"),
    }
}

fn task_digest(task_id: &str, title: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(task_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(title.as_bytes());
    blake3_prefixed_digest(hasher.finalize())
}

fn validate_openspec_task_id(task_id: &str) -> std::result::Result<(), &'static str> {
    if !task_id.contains('.') {
        return Err("task id must be hierarchical numeric form");
    }
    if task_id
        .split('.')
        .any(|segment| segment.is_empty() || !segment.chars().all(|ch| ch.is_ascii_digit()))
    {
        return Err("task id must contain only numeric dot-separated segments");
    }
    Ok(())
}

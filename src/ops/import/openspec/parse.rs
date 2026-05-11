use std::collections::HashSet;

use crate::error::{CoveyError, Result};

use super::{OpenSpecSourceTask, util::sha256_digest};

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
        if !seen.insert(task_id.to_owned()) {
            return Err(invalid_openspec_task(
                source_path,
                line_index + 1,
                "duplicate task id",
            ));
        }

        tasks.push(OpenSpecSourceTask {
            task_id: task_id.to_owned(),
            title: title.to_owned(),
            source_path: source_path.to_owned(),
            task_digest: sha256_digest(format!("{task_id}\n{title}").as_bytes()),
            task_type: None,
        });
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

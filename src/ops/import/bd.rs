use std::time::Instant;

use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};
use serde::Serialize;

use crate::{
    Covey, SessionRole,
    error::{CoveyError, Result},
    model::{
        CreateSubtaskRequest, ImportBdV1ItemResult, ImportBdV1Req, ImportBdV1Result,
        ImportBdV1SkipReason, SessionToken, SourceIssueId, bd_import_v1_subtask_id,
    },
    ops::meta_task::submit_meta_task_tx,
    ops::workflow::create_subtask_tx,
    queries::load_subtask_tx,
    validators::{
        MAX_OBJECT_ID_LEN, MAX_PATH_LEN, MAX_PROMPT_LEN, MAX_TITLE_LEN, ensure_length,
        ensure_meta_task_is_schedulable, require_role, subtask_exists,
    },
};

const SOURCE_TABLE_ISSUES: &str = "issues";
const SOURCE_TABLE_DEPENDENCIES: &str = "dependencies";
const SOURCE_TABLE_LABELS: &str = "labels";
const REQUIRED_ISSUES_COLUMNS: &[&str] = &["id", "title", "status", "priority", "issue_type"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceIssue {
    id: String,
    title: String,
    status: String,
    priority: i64,
    issue_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceDependency {
    issue_id: String,
    depends_on_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceLabel {
    issue_id: String,
    label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceSnapshot {
    issues: Vec<SourceIssue>,
    dependencies: Vec<SourceDependency>,
    labels: Vec<SourceLabel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SourceIssueEligibility {
    Importable,
    Skip(ImportBdV1SkipReason),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ImportBdV1WorkSubtaskReq {
    session_token: SessionToken,
    meta_task_id: Option<String>,
    prompt_text: Option<String>,
    source_issue_id: String,
    title: String,
    priority: i64,
    idempotency_key: String,
}

#[derive(Clone, Copy)]
enum ImportBdV1Destination<'a> {
    ExistingMetaTask(&'a str),
    NewMetaTask(&'a str),
}

impl Covey {
    /// Imports one bd issue under the V1 semantics contract.
    ///
    /// V1 is intentionally truthful and minimal:
    /// - exactly one destination selector is allowed
    /// - `meta_task_id` attaches only to an existing non-terminal meta-task
    /// - `prompt_text` creates a new meta-task via the existing submit-meta-task flow
    /// - imported bd issues become `work` subtasks in `available` state only
    /// - no claims, review subtasks, or dependency enforcement are created here
    /// - duplicate handling is deterministic via `bd_import_v1_subtask_id`
    #[allow(clippy::too_many_arguments)]
    pub fn import_bd_v1_work_subtask(
        &self,
        session_token: &str,
        meta_task_id: Option<&str>,
        prompt_text: Option<&str>,
        source_issue_id: &str,
        title: &str,
        priority: i64,
        idempotency_key: &str,
    ) -> Result<String> {
        let started_at = Instant::now();
        let req = ImportBdV1WorkSubtaskReq {
            session_token: SessionToken::parse(session_token)?,
            meta_task_id: meta_task_id.map(str::to_owned),
            prompt_text: prompt_text.map(str::to_owned),
            source_issue_id: source_issue_id.to_owned(),
            title: title.to_owned(),
            priority,
            idempotency_key: idempotency_key.to_owned(),
        };

        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                req.session_token.as_str(),
                "import_bd_v1_work_subtask",
                idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || import_bd_v1_work_subtask_tx(tx, &req, now),
            )
        });

        self.log_operation(
            "import_bd_v1_work_subtask",
            req.session_token.as_str(),
            started_at,
            &result,
            |subtask_id| vec![format!("subtask:{subtask_id}")],
        );
        result
    }

    /// Batch-import eligible bd issues from a beads database into Covey as work subtasks.
    ///
    /// V2 BOUNDARY: `claim_subtask` now exists as a separate primitive, but import-held-claim
    /// composition is intentionally deferred. This method creates only `available` work
    /// subtasks. Any future import-and-claim mode must compose through `claim_subtask`
    /// after import; the importer must not become a second claim engine.
    pub fn import_bd_v1(&self, req: ImportBdV1Req) -> Result<ImportBdV1Result> {
        let started_at = Instant::now();

        let destination = parse_import_bd_v1_destination(req.meta_task_id(), req.prompt_text())?;
        ensure_length("beads_db_path", req.beads_db_path.as_str(), MAX_PATH_LEN)?;

        let source_snapshot = load_source_snapshot(req.beads_db_path.as_str())?;

        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "import_bd_v1",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(tx, &req.session_token, &[SessionRole::Orchestrator])?;
                    let destination_meta_task_id = resolve_import_bd_v1_destination(
                        tx,
                        &req.session_token,
                        destination,
                        &req,
                        now,
                    )?;

                    import_bd_v1_source_rows_tx(
                        tx,
                        &req,
                        &destination_meta_task_id,
                        &source_snapshot,
                        now,
                    )
                },
            )
        });

        self.log_operation(
            "import_bd_v1",
            &req.session_token,
            started_at,
            &result,
            |_| Vec::new(),
        );
        result
    }
}

fn import_bd_v1_work_subtask_tx(
    tx: &Transaction<'_>,
    req: &ImportBdV1WorkSubtaskReq,
    now: i64,
) -> Result<String> {
    require_role(tx, req.session_token.as_str(), &[SessionRole::Orchestrator])?;
    ensure_length("source_issue_id", &req.source_issue_id, MAX_OBJECT_ID_LEN)?;
    ensure_length("title", &req.title, MAX_TITLE_LEN)?;
    let destination =
        parse_import_bd_v1_destination(req.meta_task_id.as_deref(), req.prompt_text.as_deref())?;
    let destination_meta_task_id =
        resolve_import_bd_v1_destination(tx, req.session_token.as_str(), destination, req, now)?;
    let subtask_id = bd_import_v1_subtask_id(&req.source_issue_id);

    if subtask_exists(tx, &subtask_id)? {
        let existing = load_subtask_tx(tx, &subtask_id)?;
        if existing.meta_task_id != destination_meta_task_id {
            return Err(CoveyError::DuplicateSubtaskId {
                subtask_id: crate::model::SubtaskId::parse(&subtask_id)?,
            });
        }

        return Ok(subtask_id);
    }

    let create_req = CreateSubtaskRequest::try_from_raw_parts(
        req.session_token.to_string(),
        destination_meta_task_id.clone(),
        Some(subtask_id.clone()),
        req.title.clone(),
        req.priority,
        req.idempotency_key.clone(),
    )?;

    create_subtask_tx(tx, &create_req, now)
}

fn import_bd_v1_source_rows_tx(
    tx: &Transaction<'_>,
    req: &ImportBdV1Req,
    destination_meta_task_id: &str,
    source_snapshot: &SourceSnapshot,
    now: i64,
) -> Result<ImportBdV1Result> {
    let mut ordered_issues = source_snapshot.issues.clone();
    ordered_issues.sort_by(|left, right| {
        let left_dep_count = source_snapshot
            .dependencies
            .iter()
            .filter(|dep| dep.issue_id == left.id)
            .count();
        let right_dep_count = source_snapshot
            .dependencies
            .iter()
            .filter(|dep| dep.issue_id == right.id)
            .count();

        left.priority
            .cmp(&right.priority)
            .then_with(|| left_dep_count.cmp(&right_dep_count))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut items = Vec::with_capacity(ordered_issues.len());

    for issue in &ordered_issues {
        match assess_source_issue(issue, source_snapshot) {
            SourceIssueEligibility::Importable => {
                let existed_before =
                    existing_subtask_matches_destination(tx, &issue.id, destination_meta_task_id)?;
                let subtask_id = import_bd_v1_work_subtask_tx(
                    tx,
                    &ImportBdV1WorkSubtaskReq {
                        session_token: SessionToken::parse(req.session_token.as_str())?,
                        meta_task_id: Some(destination_meta_task_id.to_owned()),
                        prompt_text: None,
                        source_issue_id: issue.id.clone(),
                        title: issue.title.clone(),
                        priority: issue.priority,
                        idempotency_key: format!("{}:{}", req.idempotency_key, issue.id),
                    },
                    now,
                )?;

                if existed_before {
                    items.push(ImportBdV1ItemResult::skipped(
                        issue.id.clone(),
                        Some(subtask_id),
                        ImportBdV1SkipReason::DeterministicDuplicate,
                    ));
                } else {
                    items.push(ImportBdV1ItemResult::imported(issue.id.clone(), subtask_id));
                }
            }
            SourceIssueEligibility::Skip(skip_reason) => {
                let source_issue_id = if SourceIssueId::parse(issue.id.clone()).is_ok() {
                    issue.id.clone()
                } else {
                    format!("invalid-source-issue-{}", items.len() + 1)
                };
                items.push(ImportBdV1ItemResult::skipped(
                    source_issue_id,
                    None,
                    skip_reason,
                ));
            }
        }
    }

    ImportBdV1Result::new(destination_meta_task_id.to_owned(), items)
        .map_err(|reason| CoveyError::InvalidImportDestination { reason })
}

fn existing_subtask_matches_destination(
    tx: &Transaction<'_>,
    source_issue_id: &str,
    destination_meta_task_id: &str,
) -> Result<bool> {
    let subtask_id = bd_import_v1_subtask_id(source_issue_id);
    if !subtask_exists(tx, &subtask_id)? {
        return Ok(false);
    }

    let existing = load_subtask_tx(tx, &subtask_id)?;
    if existing.meta_task_id != destination_meta_task_id {
        return Err(CoveyError::ImportDuplicate {
            source_issue_id: source_issue_id.to_owned(),
            subtask_id: crate::model::SubtaskId::parse(&subtask_id)?,
        });
    }

    Ok(true)
}

fn assess_source_issue(
    issue: &SourceIssue,
    source_snapshot: &SourceSnapshot,
) -> SourceIssueEligibility {
    if issue.id.trim().is_empty() {
        return SourceIssueEligibility::Skip(ImportBdV1SkipReason::InvalidRow {
            detail: "missing issue id".to_owned(),
        });
    }

    if issue.id.len() > MAX_OBJECT_ID_LEN {
        return SourceIssueEligibility::Skip(ImportBdV1SkipReason::InvalidRow {
            detail: format!("issue id exceeds max length {MAX_OBJECT_ID_LEN}"),
        });
    }

    if issue.title.trim().is_empty() {
        return SourceIssueEligibility::Skip(ImportBdV1SkipReason::InvalidRow {
            detail: "missing title".to_owned(),
        });
    }

    if issue.title.len() > MAX_TITLE_LEN {
        return SourceIssueEligibility::Skip(ImportBdV1SkipReason::InvalidRow {
            detail: format!("title exceeds max length {MAX_TITLE_LEN}"),
        });
    }

    if !issue.status.eq_ignore_ascii_case("open") {
        return SourceIssueEligibility::Skip(ImportBdV1SkipReason::InvalidRow {
            detail: format!("unsupported status {}", issue.status),
        });
    }

    if !is_importable_issue_type(&issue.issue_type) {
        return SourceIssueEligibility::Skip(ImportBdV1SkipReason::InvalidRow {
            detail: format!("unsupported issue_type {}", issue.issue_type),
        });
    }

    if source_snapshot.labels.iter().any(|label| {
        label.issue_id == issue.id
            && (label.label.eq_ignore_ascii_case("review")
                || label.label.eq_ignore_ascii_case("import:skip"))
    }) {
        return SourceIssueEligibility::Skip(ImportBdV1SkipReason::InvalidRow {
            detail: "unsupported labeled issue".to_owned(),
        });
    }

    SourceIssueEligibility::Importable
}

fn is_importable_issue_type(issue_type: &str) -> bool {
    issue_type.eq_ignore_ascii_case("task")
        || issue_type.eq_ignore_ascii_case("feature")
        || issue_type.eq_ignore_ascii_case("epic")
}

fn load_source_snapshot(path: &str) -> Result<SourceSnapshot> {
    let source_conn = match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(conn) => conn,
        Err(_) => {
            return Err(CoveyError::ImportSourceNotFound {
                path: path.to_owned(),
            });
        }
    };

    validate_source_schema(&source_conn, path)?;
    let issues = load_source_issues(&source_conn, path)?;
    let dependencies = load_source_dependencies(&source_conn, path)?;
    let labels = load_source_labels(&source_conn, path)?;

    Ok(SourceSnapshot {
        issues,
        dependencies,
        labels,
    })
}

fn validate_source_schema(source_conn: &Connection, path: &str) -> Result<()> {
    ensure_table_exists(source_conn, path, SOURCE_TABLE_ISSUES)?;
    ensure_issue_columns(source_conn, path)?;
    Ok(())
}

fn ensure_table_exists(source_conn: &Connection, path: &str, table_name: &str) -> Result<()> {
    let present = source_conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name = ?1",
            params![table_name],
            |_| Ok(()),
        )
        .optional()?;

    if present.is_none() {
        return Err(CoveyError::InvalidSourceSchema {
            path: path.to_owned(),
            detail: format!("missing {table_name} table"),
        });
    }

    Ok(())
}

fn ensure_issue_columns(source_conn: &Connection, path: &str) -> Result<()> {
    let mut stmt = source_conn.prepare("PRAGMA table_info(issues)")?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut present_columns = Vec::new();
    for column in columns {
        present_columns.push(column?);
    }

    for required in REQUIRED_ISSUES_COLUMNS {
        if !present_columns.iter().any(|column| column == required) {
            return Err(CoveyError::InvalidSourceSchema {
                path: path.to_owned(),
                detail: format!("issues table missing required column {required}"),
            });
        }
    }

    Ok(())
}

fn load_source_issues(source_conn: &Connection, path: &str) -> Result<Vec<SourceIssue>> {
    let deleted_at_predicate = if issue_table_has_column(source_conn, "deleted_at")? {
        "WHERE deleted_at IS NULL OR deleted_at = ''"
    } else {
        ""
    };
    let mut stmt = source_conn.prepare(&format!(
        r#"
            SELECT id, title, status, priority, issue_type
            FROM issues
            {deleted_at_predicate}
            ORDER BY priority ASC, id ASC
            "#
    ))?;
    let rows = stmt.query_map([], |row| {
        Ok(SourceIssue {
            id: row.get(0)?,
            title: row.get(1)?,
            status: row.get(2)?,
            priority: row.get(3)?,
            issue_type: row.get(4)?,
        })
    })?;

    collect_source_rows(rows, path, "issues")
}

fn load_source_dependencies(source_conn: &Connection, path: &str) -> Result<Vec<SourceDependency>> {
    if !table_exists(source_conn, SOURCE_TABLE_DEPENDENCIES)? {
        return Ok(Vec::new());
    }
    let mut stmt = source_conn.prepare(
        "SELECT issue_id, depends_on_id FROM dependencies ORDER BY issue_id ASC, depends_on_id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(SourceDependency {
            issue_id: row.get(0)?,
            depends_on_id: row.get(1)?,
        })
    })?;
    collect_source_rows(rows, path, SOURCE_TABLE_DEPENDENCIES)
}

fn load_source_labels(source_conn: &Connection, path: &str) -> Result<Vec<SourceLabel>> {
    if !table_exists(source_conn, SOURCE_TABLE_LABELS)? {
        return Ok(Vec::new());
    }
    let mut stmt = source_conn
        .prepare("SELECT issue_id, label FROM labels ORDER BY issue_id ASC, label ASC")?;
    let rows = stmt.query_map([], |row| {
        Ok(SourceLabel {
            issue_id: row.get(0)?,
            label: row.get(1)?,
        })
    })?;
    collect_source_rows(rows, path, SOURCE_TABLE_LABELS)
}

fn collect_source_rows<T, F>(
    rows: rusqlite::MappedRows<'_, F>,
    path: &str,
    table_name: &str,
) -> Result<Vec<T>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    let mut collected = Vec::new();
    for row in rows {
        match row {
            Ok(value) => collected.push(value),
            Err(err) => {
                return Err(CoveyError::InvalidSourceSchema {
                    path: path.to_owned(),
                    detail: format!("invalid row in {table_name}: {err}"),
                });
            }
        }
    }
    Ok(collected)
}

fn issue_table_has_column(source_conn: &Connection, column_name: &str) -> Result<bool> {
    let mut stmt = source_conn.prepare("PRAGMA table_info(issues)")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let present_name: String = row.get(1)?;
        if present_name == column_name {
            return Ok(true);
        }
    }

    Ok(false)
}

fn table_exists(source_conn: &Connection, table_name: &str) -> Result<bool> {
    source_conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name = ?1",
            params![table_name],
            |_| Ok(()),
        )
        .optional()
        .map(|present| present.is_some())
        .map_err(Into::into)
}

fn parse_import_bd_v1_destination<'a>(
    meta_task_id: Option<&'a str>,
    prompt_text: Option<&'a str>,
) -> Result<ImportBdV1Destination<'a>> {
    match (meta_task_id, prompt_text) {
        (Some(meta_task_id), None) => Ok(ImportBdV1Destination::ExistingMetaTask(meta_task_id)),
        (None, Some(prompt_text)) => Ok(ImportBdV1Destination::NewMetaTask(prompt_text)),
        _ => Err(CoveyError::InvalidPath {
            path: "bd import destination selector".to_owned(),
        }),
    }
}

fn resolve_import_bd_v1_destination<T: Serialize>(
    tx: &Transaction<'_>,
    session_token: &str,
    destination: ImportBdV1Destination<'_>,
    event_payload: &T,
    now: i64,
) -> Result<String> {
    match destination {
        ImportBdV1Destination::ExistingMetaTask(meta_task_id) => {
            ensure_length("meta_task_id", meta_task_id, MAX_OBJECT_ID_LEN)?;
            ensure_meta_task_is_schedulable(tx, meta_task_id)?;
            Ok(meta_task_id.to_owned())
        }
        ImportBdV1Destination::NewMetaTask(prompt_text) => {
            ensure_length("prompt_text", prompt_text, MAX_PROMPT_LEN)?;
            submit_meta_task_tx(tx, session_token, prompt_text, event_payload, now)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_import_bd_v1_destination;
    use crate::error::CoveyError;

    #[test]
    fn import_destination_requires_exactly_one_selector() {
        assert!(matches!(
            parse_import_bd_v1_destination(Some("meta_1"), Some("prompt")),
            Err(CoveyError::InvalidPath { path }) if path == "bd import destination selector"
        ));
        assert!(matches!(
            parse_import_bd_v1_destination(None, None),
            Err(CoveyError::InvalidPath { path }) if path == "bd import destination selector"
        ));
    }
}

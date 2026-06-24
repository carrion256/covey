#![cfg_attr(coverage_nightly, coverage(off))]

use std::time::Instant;

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        EventType, ObjectType, ObserveVcsWorkspaceReq, RecordVcsWorkspaceReq, SessionRole,
        VcsWorkspace, VcsWorkspaceId, vcs_workspace_cleanliness_name, vcs_workspace_kind_name,
        vcs_workspace_state_name,
    },
    queries::{collect_rows, deserialize_row},
    store::append_session_event,
    validators::{MAX_OBJECT_ID_LEN, ensure_length, require_role},
};

impl Covey {
    /// Records or refreshes a scheduler-created VCS workspace binding.
    pub fn record_vcs_workspace(&self, req: RecordVcsWorkspaceReq) -> Result<VcsWorkspace> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "record_vcs_workspace",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(
                        tx,
                        &req.session_token,
                        &[SessionRole::Orchestrator, SessionRole::ApplyGate],
                    )?;
                    ensure_length("workspace_id", &req.workspace_id, MAX_OBJECT_ID_LEN)?;
                    ensure_vcs_workspace_target_shape(&req)?;

                    tx.execute(
                        r#"
                        INSERT INTO vcs_workspaces (
                            workspace_id, kind, path, jj_workspace_name, claim_id, subtask_id,
                            openspec_change_id, queue_id, artifact_digest, current_bookmark,
                            current_change_id, current_commit_id, state, last_cleanliness,
                            last_observed_reason, recorded_by_session, created_at, updated_at,
                            last_observed_at
                        ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                            ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?17, ?17
                        )
                        ON CONFLICT(workspace_id) DO UPDATE SET
                            kind = excluded.kind,
                            path = excluded.path,
                            jj_workspace_name = excluded.jj_workspace_name,
                            claim_id = excluded.claim_id,
                            subtask_id = excluded.subtask_id,
                            openspec_change_id = excluded.openspec_change_id,
                            queue_id = excluded.queue_id,
                            artifact_digest = excluded.artifact_digest,
                            current_bookmark = excluded.current_bookmark,
                            current_change_id = excluded.current_change_id,
                            current_commit_id = excluded.current_commit_id,
                            state = excluded.state,
                            last_cleanliness = excluded.last_cleanliness,
                            last_observed_reason = excluded.last_observed_reason,
                            updated_at = excluded.updated_at,
                            last_observed_at = excluded.last_observed_at
                        "#,
                        params![
                            req.workspace_id.as_str(),
                            vcs_workspace_kind_name(req.kind),
                            req.path.as_str(),
                            req.jj_workspace_name.as_ref().map(AsRef::as_ref),
                            req.claim_id.as_ref().map(AsRef::as_ref),
                            req.subtask_id.as_ref().map(AsRef::as_ref),
                            req.openspec_change_id.as_ref().map(AsRef::as_ref),
                            req.queue_id.as_ref().map(AsRef::as_ref),
                            req.artifact_digest.as_ref().map(AsRef::as_ref),
                            req.current_bookmark.as_ref().map(AsRef::as_ref),
                            req.current_change_id.as_ref().map(AsRef::as_ref),
                            req.current_commit_id.as_ref().map(AsRef::as_ref),
                            vcs_workspace_state_name(req.state),
                            vcs_workspace_cleanliness_name(req.last_cleanliness),
                            req.last_observed_reason.as_ref().map(AsRef::as_ref),
                            req.session_token.as_str(),
                            now,
                        ],
                    )?;
                    append_session_event(
                        tx,
                        EventType::VcsWorkspaceRecorded,
                        ObjectType::VcsWorkspace,
                        req.workspace_id.as_str(),
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    load_vcs_workspace_tx(tx, &req.workspace_id)
                },
            )
        });
        self.log_operation(
            "record_vcs_workspace",
            &req.session_token,
            started_at,
            &result,
            |workspace| vec![format!("vcs_workspace:{}", workspace.workspace_id)],
        );
        result
    }

    /// Updates the last observed state for a registered VCS workspace.
    pub fn observe_vcs_workspace(&self, req: ObserveVcsWorkspaceReq) -> Result<VcsWorkspace> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "observe_vcs_workspace",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(
                        tx,
                        &req.session_token,
                        &[SessionRole::Orchestrator, SessionRole::ApplyGate],
                    )?;
                    load_vcs_workspace_tx(tx, &req.workspace_id)?;
                    tx.execute(
                        r#"
                        UPDATE vcs_workspaces
                        SET state = ?2,
                            last_cleanliness = ?3,
                            last_observed_reason = ?4,
                            current_bookmark = COALESCE(?5, current_bookmark),
                            current_change_id = COALESCE(?6, current_change_id),
                            current_commit_id = COALESCE(?7, current_commit_id),
                            updated_at = ?8,
                            last_observed_at = ?8
                        WHERE workspace_id = ?1
                        "#,
                        params![
                            req.workspace_id.as_str(),
                            vcs_workspace_state_name(req.state),
                            vcs_workspace_cleanliness_name(req.last_cleanliness),
                            req.last_observed_reason.as_ref().map(AsRef::as_ref),
                            req.current_bookmark.as_ref().map(AsRef::as_ref),
                            req.current_change_id.as_ref().map(AsRef::as_ref),
                            req.current_commit_id.as_ref().map(AsRef::as_ref),
                            now,
                        ],
                    )?;
                    append_session_event(
                        tx,
                        EventType::VcsWorkspaceObserved,
                        ObjectType::VcsWorkspace,
                        req.workspace_id.as_str(),
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    load_vcs_workspace_tx(tx, &req.workspace_id)
                },
            )
        });
        self.log_operation(
            "observe_vcs_workspace",
            &req.session_token,
            started_at,
            &result,
            |workspace| vec![format!("vcs_workspace:{}", workspace.workspace_id)],
        );
        result
    }

    /// Loads a registered VCS workspace by id.
    pub fn vcs_workspace(&self, workspace_id: &str) -> Result<VcsWorkspace> {
        let workspace_id = VcsWorkspaceId::parse(workspace_id)?;
        self.with_read_tx(|tx| load_vcs_workspace_tx(tx, &workspace_id))
    }

    /// Loads all registered VCS workspaces for one OpenSpec change id.
    pub fn vcs_workspaces_for_openspec(
        &self,
        openspec_change_id: &str,
    ) -> Result<Vec<VcsWorkspace>> {
        self.with_read_tx(|tx| {
            let mut stmt = tx.prepare(
                r#"
                SELECT workspace_id, kind, path, jj_workspace_name, claim_id, subtask_id,
                       openspec_change_id, queue_id, artifact_digest, current_bookmark,
                       current_change_id, current_commit_id, state, last_cleanliness,
                       last_observed_reason, recorded_by_session, created_at, updated_at,
                       last_observed_at
                FROM vcs_workspaces
                WHERE openspec_change_id = ?1
                ORDER BY kind, workspace_id
                "#,
            )?;
            let rows =
                stmt.query_map(params![openspec_change_id], deserialize_row::<VcsWorkspace>)?;
            collect_rows(rows)
        })
    }

    /// Loads all registered VCS workspaces for one claim id.
    pub fn vcs_workspaces_for_claim(&self, claim_id: &str) -> Result<Vec<VcsWorkspace>> {
        self.with_read_tx(|tx| {
            let mut stmt = tx.prepare(
                r#"
                SELECT workspace_id, kind, path, jj_workspace_name, claim_id, subtask_id,
                       openspec_change_id, queue_id, artifact_digest, current_bookmark,
                       current_change_id, current_commit_id, state, last_cleanliness,
                       last_observed_reason, recorded_by_session, created_at, updated_at,
                       last_observed_at
                FROM vcs_workspaces
                WHERE claim_id = ?1
                ORDER BY kind, workspace_id
                "#,
            )?;
            let rows = stmt.query_map(params![claim_id], deserialize_row::<VcsWorkspace>)?;
            collect_rows(rows)
        })
    }

    /// Loads all registered VCS workspaces for one apply queue item.
    pub fn vcs_workspaces_for_queue(&self, queue_id: &str) -> Result<Vec<VcsWorkspace>> {
        self.with_read_tx(|tx| {
            let mut stmt = tx.prepare(
                r#"
                SELECT workspace_id, kind, path, jj_workspace_name, claim_id, subtask_id,
                       openspec_change_id, queue_id, artifact_digest, current_bookmark,
                       current_change_id, current_commit_id, state, last_cleanliness,
                       last_observed_reason, recorded_by_session, created_at, updated_at,
                       last_observed_at
                FROM vcs_workspaces
                WHERE queue_id = ?1
                ORDER BY kind, workspace_id
                "#,
            )?;
            let rows = stmt.query_map(params![queue_id], deserialize_row::<VcsWorkspace>)?;
            collect_rows(rows)
        })
    }

    /// Returns registered VCS workspace/cache paths eligible for janitor inspection.
    pub fn vcs_workspace_cleanup_candidates(
        &self,
        older_than_updated_at_ms: i64,
        limit: usize,
    ) -> Result<Vec<VcsWorkspace>> {
        self.with_read_tx(|tx| {
            let mut stmt = tx.prepare(
                r#"
                SELECT workspace_id, kind, path, jj_workspace_name, claim_id, subtask_id,
                       openspec_change_id, queue_id, artifact_digest, current_bookmark,
                       current_change_id, current_commit_id, state, last_cleanliness,
                       last_observed_reason, recorded_by_session, created_at, updated_at,
                       last_observed_at
                FROM vcs_workspaces
                WHERE state IN (?1, ?2) AND updated_at <= ?3
                ORDER BY updated_at, workspace_id
                LIMIT ?4
                "#,
            )?;
            let rows = stmt.query_map(
                params![
                    vcs_workspace_state_name(crate::model::VcsWorkspaceState::CleanupAllowed),
                    vcs_workspace_state_name(crate::model::VcsWorkspaceState::Retained),
                    older_than_updated_at_ms.max(0),
                    limit as i64,
                ],
                deserialize_row::<VcsWorkspace>,
            )?;
            collect_rows(rows)
        })
    }
}

fn ensure_vcs_workspace_target_shape(req: &RecordVcsWorkspaceReq) -> Result<()> {
    match req.kind {
        crate::model::VcsWorkspaceKind::Claim => {
            if req.claim_id.is_none() || req.subtask_id.is_none() {
                return Err(CoveyError::InvalidImportDestination {
                    reason: "claim VCS workspace requires claim_id and subtask_id".to_owned(),
                });
            }
        }
        crate::model::VcsWorkspaceKind::Packet => {
            if req.openspec_change_id.is_none() {
                return Err(CoveyError::InvalidImportDestination {
                    reason: "packet VCS workspace requires openspec_change_id".to_owned(),
                });
            }
        }
        crate::model::VcsWorkspaceKind::Apply => {
            if req.queue_id.is_none() || req.artifact_digest.is_none() {
                return Err(CoveyError::InvalidImportDestination {
                    reason: "apply VCS workspace requires queue_id and artifact_digest".to_owned(),
                });
            }
        }
        crate::model::VcsWorkspaceKind::Execution => {
            if req.openspec_change_id.is_none() {
                return Err(CoveyError::InvalidImportDestination {
                    reason: "execution VCS workspace requires openspec_change_id".to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn load_vcs_workspace_tx(
    tx: &Transaction<'_>,
    workspace_id: &VcsWorkspaceId,
) -> Result<VcsWorkspace> {
    tx.query_row(
        r#"
        SELECT workspace_id, kind, path, jj_workspace_name, claim_id, subtask_id,
               openspec_change_id, queue_id, artifact_digest, current_bookmark,
               current_change_id, current_commit_id, state, last_cleanliness,
               last_observed_reason, recorded_by_session, created_at, updated_at,
               last_observed_at
        FROM vcs_workspaces
        WHERE workspace_id = ?1
        "#,
        params![workspace_id.as_str()],
        deserialize_row::<VcsWorkspace>,
    )
    .optional()?
    .ok_or_else(|| CoveyError::InvalidImportDestination {
        reason: format!("VCS workspace {} is not registered", workspace_id.as_str()),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        Covey, ManualClock,
        model::{
            RecordVcsWorkspaceReq, SessionRole, VcsWorkspaceCleanliness, VcsWorkspaceKind,
            VcsWorkspaceState,
        },
    };

    const TEST_NOW_MS: i64 = 1_700_000_000_000;

    #[test]
    fn record_vcs_workspace_persists_execution_binding() {
        let clock = Arc::new(ManualClock::new(TEST_NOW_MS));
        let covey = Covey::open_in_memory_with_clock(clock).expect("open covey");
        let session = covey
            .register_session(
                crate::RegisterSessionReq::try_from_raw_parts(
                    "orch-vcs-workspace",
                    "orch-vcs-workspace-1",
                    SessionRole::Orchestrator,
                    "register-orch-vcs-workspace",
                )
                .expect("valid session"),
            )
            .expect("register session");

        let workspace = covey
            .record_vcs_workspace(
                RecordVcsWorkspaceReq::try_from_raw_parts(
                    session.session_token().to_owned(),
                    "workspace-change-a-execution",
                    VcsWorkspaceKind::Execution,
                    "/data/tmp/mutai-worktrees/repo/change-a",
                    None,
                    None,
                    None,
                    Some("change-a".to_owned()),
                    None,
                    None,
                    None,
                    None,
                    None,
                    VcsWorkspaceState::Active,
                    VcsWorkspaceCleanliness::Clean,
                    Some("admitted clean execution root".to_owned()),
                    "record-workspace-change-a",
                )
                .expect("valid workspace request"),
            )
            .expect("record workspace");

        assert_eq!(
            workspace.workspace_id.as_str(),
            "workspace-change-a-execution"
        );
        assert_eq!(workspace.openspec_change_id.as_deref(), Some("change-a"));
        assert_eq!(workspace.last_cleanliness, VcsWorkspaceCleanliness::Clean);
        assert_eq!(
            covey
                .vcs_workspaces_for_openspec("change-a")
                .expect("workspace lookup")
                .len(),
            1
        );
    }
}

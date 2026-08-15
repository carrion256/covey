#![cfg_attr(coverage_nightly, coverage(off))]

use std::time::Instant;

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        EventType, ObjectType, RecordVcsPacketStackEntryReq, RecordVcsPrPublicationReq,
        ReviewState, ReviewVerdict, SessionRole, VcsPacketStackEntry, VcsPacketStackEntryId,
        VcsPrPublication, VcsPrPublicationId, VcsPrPublicationKind, VcsPrPublicationStatus,
        vcs_packet_stack_entry_state_name, vcs_pr_publication_kind_name,
        vcs_pr_publication_status_name,
    },
    queries::{collect_rows, deserialize_row, load_artifact_tx, load_claim_tx, load_review_tx},
    store::append_session_event,
    validators::{MAX_DIGEST_LEN, MAX_OBJECT_ID_LEN, ensure_length, require_role},
};

impl Covey {
    /// Records one reviewed claim change as part of an OpenSpec packet stack.
    pub fn record_vcs_packet_stack_entry(
        &self,
        req: RecordVcsPacketStackEntryReq,
    ) -> Result<VcsPacketStackEntry> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "record_vcs_packet_stack_entry",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(
                        tx,
                        &req.session_token,
                        &[SessionRole::Orchestrator, SessionRole::ApplyGate],
                    )?;
                    validate_packet_stack_entry_tx(tx, &req)?;
                    tx.execute(
                        r#"
                        INSERT INTO vcs_packet_stack_entries (
                            stack_entry_id, openspec_change_id, packet_bookmark, claim_id,
                            subtask_id, artifact_digest, review_id, findings_digest,
                            claim_bookmark, claim_change_id, claim_commit_id, stack_position,
                            tree_equivalence_digest, state, recorded_by_session, created_at,
                            updated_at
                        ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                            ?11, ?12, ?13, ?14, ?15, ?16, ?16
                        )
                        ON CONFLICT(stack_entry_id) DO UPDATE SET
                            openspec_change_id = excluded.openspec_change_id,
                            packet_bookmark = excluded.packet_bookmark,
                            claim_id = excluded.claim_id,
                            subtask_id = excluded.subtask_id,
                            artifact_digest = excluded.artifact_digest,
                            review_id = excluded.review_id,
                            findings_digest = excluded.findings_digest,
                            claim_bookmark = excluded.claim_bookmark,
                            claim_change_id = excluded.claim_change_id,
                            claim_commit_id = excluded.claim_commit_id,
                            stack_position = excluded.stack_position,
                            tree_equivalence_digest = excluded.tree_equivalence_digest,
                            state = excluded.state,
                            updated_at = excluded.updated_at
                        "#,
                        params![
                            req.stack_entry_id.as_str(),
                            req.openspec_change_id.as_str(),
                            req.packet_bookmark.as_str(),
                            req.claim_id.as_str(),
                            req.subtask_id.as_str(),
                            req.artifact_digest.as_str(),
                            req.review_id.as_str(),
                            req.findings_digest.as_str(),
                            req.claim_bookmark.as_str(),
                            req.claim_change_id.as_str(),
                            req.claim_commit_id.as_str(),
                            req.stack_position,
                            req.tree_equivalence_digest.as_ref().map(AsRef::as_ref),
                            vcs_packet_stack_entry_state_name(req.state),
                            req.session_token.as_str(),
                            now,
                        ],
                    )?;
                    append_session_event(
                        tx,
                        EventType::VcsPacketStackEntryRecorded,
                        ObjectType::VcsPacketStackEntry,
                        req.stack_entry_id.as_str(),
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    load_vcs_packet_stack_entry_tx(tx, &req.stack_entry_id)
                },
            )
        });
        self.log_operation(
            "record_vcs_packet_stack_entry",
            &req.session_token,
            started_at,
            &result,
            |entry| vec![format!("vcs_packet_stack_entry:{}", entry.stack_entry_id)],
        );
        result
    }

    /// Loads all packet stack entries for one OpenSpec change in stack order.
    pub fn vcs_packet_stack_entries_for_openspec(
        &self,
        openspec_change_id: &str,
    ) -> Result<Vec<VcsPacketStackEntry>> {
        self.with_read_tx(|tx| {
            let mut stmt = tx.prepare(
                r#"
                SELECT stack_entry_id, openspec_change_id, packet_bookmark, claim_id,
                       subtask_id, artifact_digest, review_id, findings_digest,
                       claim_bookmark, claim_change_id, claim_commit_id, stack_position,
                       tree_equivalence_digest, state, recorded_by_session, created_at,
                       updated_at
                FROM vcs_packet_stack_entries
                WHERE openspec_change_id = ?1
                ORDER BY stack_position, stack_entry_id
                "#,
            )?;
            let rows = stmt.query_map(params![openspec_change_id], deserialize_row)?;
            collect_rows(rows)
        })
    }

    /// Records a GitHub PR publication projection for a packet or standalone claim.
    pub fn record_vcs_pr_publication(
        &self,
        req: RecordVcsPrPublicationReq,
    ) -> Result<VcsPrPublication> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "record_vcs_pr_publication",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    require_role(
                        tx,
                        &req.session_token,
                        &[SessionRole::Orchestrator, SessionRole::ApplyGate],
                    )?;
                    validate_pr_publication_tx(tx, &req)?;
                    tx.execute(
                        r#"
                        INSERT INTO vcs_pr_publications (
                            publication_id, kind, openspec_change_id, claim_id, bookmark,
                            head_commit_id, base_ref, pr_url, status, blocker_reason,
                            recorded_by_session, created_at, updated_at
                        ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12
                        )
                        ON CONFLICT(publication_id) DO UPDATE SET
                            kind = excluded.kind,
                            openspec_change_id = excluded.openspec_change_id,
                            claim_id = excluded.claim_id,
                            bookmark = excluded.bookmark,
                            head_commit_id = excluded.head_commit_id,
                            base_ref = excluded.base_ref,
                            pr_url = excluded.pr_url,
                            status = excluded.status,
                            blocker_reason = excluded.blocker_reason,
                            updated_at = excluded.updated_at
                        "#,
                        params![
                            req.publication_id.as_str(),
                            vcs_pr_publication_kind_name(req.kind),
                            req.openspec_change_id.as_ref().map(AsRef::as_ref),
                            req.claim_id.as_ref().map(AsRef::as_ref),
                            req.bookmark.as_str(),
                            req.head_commit_id.as_str(),
                            req.base_ref.as_str(),
                            req.pr_url.as_ref().map(AsRef::as_ref),
                            vcs_pr_publication_status_name(req.status),
                            req.blocker_reason.as_ref().map(AsRef::as_ref),
                            req.session_token.as_str(),
                            now,
                        ],
                    )?;
                    append_session_event(
                        tx,
                        EventType::VcsPrPublicationRecorded,
                        ObjectType::VcsPrPublication,
                        req.publication_id.as_str(),
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    load_vcs_pr_publication_tx(tx, &req.publication_id)
                },
            )
        });
        self.log_operation(
            "record_vcs_pr_publication",
            &req.session_token,
            started_at,
            &result,
            |publication| vec![format!("vcs_pr_publication:{}", publication.publication_id)],
        );
        result
    }

    /// Loads all PR publication projections for one OpenSpec packet.
    pub fn vcs_pr_publications_for_openspec(
        &self,
        openspec_change_id: &str,
    ) -> Result<Vec<VcsPrPublication>> {
        self.with_read_tx(|tx| {
            let mut stmt = tx.prepare(
                r#"
                SELECT publication_id, kind, openspec_change_id, claim_id, bookmark,
                       head_commit_id, base_ref, pr_url, status, blocker_reason,
                       recorded_by_session, created_at, updated_at
                FROM vcs_pr_publications
                WHERE openspec_change_id = ?1
                ORDER BY updated_at DESC, publication_id
                "#,
            )?;
            let rows = stmt.query_map(params![openspec_change_id], deserialize_row)?;
            collect_rows(rows)
        })
    }
}

fn validate_packet_stack_entry_tx(
    tx: &Transaction<'_>,
    req: &RecordVcsPacketStackEntryReq,
) -> Result<()> {
    ensure_length("stack_entry_id", &req.stack_entry_id, MAX_OBJECT_ID_LEN)?;
    ensure_length(
        "openspec_change_id",
        &req.openspec_change_id,
        MAX_OBJECT_ID_LEN,
    )?;
    ensure_length("claim_id", &req.claim_id, MAX_OBJECT_ID_LEN)?;
    ensure_length("subtask_id", &req.subtask_id, MAX_OBJECT_ID_LEN)?;
    ensure_length("artifact_digest", &req.artifact_digest, MAX_DIGEST_LEN)?;
    ensure_length("findings_digest", &req.findings_digest, MAX_DIGEST_LEN)?;
    if req.stack_position < 0 {
        return Err(invalid_projection(
            "packet stack position must be greater than or equal to zero",
        ));
    }

    let expected_packet_bookmark = format!("packet/{}", req.openspec_change_id);
    if req.packet_bookmark.as_str() != expected_packet_bookmark {
        return Err(invalid_projection(
            "packet stack entry packet_bookmark must be packet/<openspec_change_id>",
        ));
    }
    let expected_claim_bookmark = format!("claim/{}", req.claim_id);
    if req.claim_bookmark.as_str() != expected_claim_bookmark {
        return Err(invalid_projection(
            "packet stack entry claim_bookmark must be claim/<claim_id>",
        ));
    }

    let claim = load_claim_tx(tx, &req.claim_id)?;
    if claim.subtask_id != req.subtask_id {
        return Err(invalid_projection(
            "packet stack entry claim_id does not belong to subtask_id",
        ));
    }

    let scoped_change_id = tx
        .query_row(
            "SELECT openspec_change_id FROM openspec_subtask_scope WHERE subtask_id = ?1",
            params![req.subtask_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| invalid_projection("packet stack entry subtask is not OpenSpec-scoped"))?;
    if scoped_change_id != req.openspec_change_id.as_str() {
        return Err(invalid_projection(
            "packet stack entry openspec_change_id does not match subtask scope",
        ));
    }

    let artifact = load_artifact_tx(tx, &req.artifact_digest)?;
    if artifact.produced_by_subtask_id != req.subtask_id {
        return Err(invalid_projection(
            "packet stack entry artifact was not produced by subtask_id",
        ));
    }

    let review = load_review_tx(tx, &req.review_id)?;
    if review.subtask_id() != req.subtask_id.as_str()
        || review.artifact_digest() != req.artifact_digest.as_str()
        || review.state() != ReviewState::Decided
        || review.verdict() != Some(ReviewVerdict::Approve)
        || review.findings_digest() != Some(req.findings_digest.as_str())
    {
        return Err(invalid_projection(
            "packet stack entry review evidence must approve the exact artifact and findings digest",
        ));
    }

    Ok(())
}

fn validate_pr_publication_tx(tx: &Transaction<'_>, req: &RecordVcsPrPublicationReq) -> Result<()> {
    ensure_length("publication_id", &req.publication_id, MAX_OBJECT_ID_LEN)?;
    match req.kind {
        VcsPrPublicationKind::Packet => {
            let Some(openspec_change_id) = &req.openspec_change_id else {
                return Err(invalid_projection(
                    "packet PR publication requires openspec_change_id",
                ));
            };
            if req.claim_id.is_some() {
                return Err(invalid_projection(
                    "packet PR publication must not include claim_id",
                ));
            }
            let expected_bookmark = format!("packet/{openspec_change_id}");
            if req.bookmark.as_str() != expected_bookmark {
                return Err(invalid_projection(
                    "packet PR publication bookmark must be packet/<openspec_change_id>",
                ));
            }
            if matches!(
                req.status,
                VcsPrPublicationStatus::Prepared | VcsPrPublicationStatus::Published
            ) {
                ensure_packet_stack_has_entries(tx, openspec_change_id.as_str())?;
            }
        }
        VcsPrPublicationKind::Claim => {
            let Some(claim_id) = &req.claim_id else {
                return Err(invalid_projection("claim PR publication requires claim_id"));
            };
            load_claim_tx(tx, claim_id)?;
            let expected_bookmark = format!("claim/{claim_id}");
            if req.bookmark.as_str() != expected_bookmark {
                return Err(invalid_projection(
                    "claim PR publication bookmark must be claim/<claim_id>",
                ));
            }
        }
    }

    match req.status {
        VcsPrPublicationStatus::Published if req.pr_url.is_none() => Err(invalid_projection(
            "published PR publication requires pr_url",
        )),
        VcsPrPublicationStatus::Blocked if req.blocker_reason.is_none() => Err(invalid_projection(
            "blocked PR publication requires blocker_reason",
        )),
        VcsPrPublicationStatus::Prepared | VcsPrPublicationStatus::Published
            if req.blocker_reason.is_some() =>
        {
            Err(invalid_projection(
                "non-blocked PR publication must not include blocker_reason",
            ))
        }
        _ => Ok(()),
    }
}

fn ensure_packet_stack_has_entries(tx: &Transaction<'_>, openspec_change_id: &str) -> Result<()> {
    let has_entries = tx
        .query_row(
            "SELECT 1 FROM vcs_packet_stack_entries WHERE openspec_change_id = ?1 AND state != 'superseded' LIMIT 1",
            params![openspec_change_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if has_entries {
        Ok(())
    } else {
        Err(invalid_projection(
            "packet PR publication requires at least one active packet stack entry",
        ))
    }
}

fn load_vcs_packet_stack_entry_tx(
    tx: &Transaction<'_>,
    stack_entry_id: &VcsPacketStackEntryId,
) -> Result<VcsPacketStackEntry> {
    tx.query_row(
        r#"
        SELECT stack_entry_id, openspec_change_id, packet_bookmark, claim_id,
               subtask_id, artifact_digest, review_id, findings_digest,
               claim_bookmark, claim_change_id, claim_commit_id, stack_position,
               tree_equivalence_digest, state, recorded_by_session, created_at,
               updated_at
        FROM vcs_packet_stack_entries
        WHERE stack_entry_id = ?1
        "#,
        params![stack_entry_id.as_str()],
        deserialize_row,
    )
    .optional()?
    .ok_or_else(|| invalid_projection("packet stack entry is not recorded"))
}

fn load_vcs_pr_publication_tx(
    tx: &Transaction<'_>,
    publication_id: &VcsPrPublicationId,
) -> Result<VcsPrPublication> {
    tx.query_row(
        r#"
        SELECT publication_id, kind, openspec_change_id, claim_id, bookmark,
               head_commit_id, base_ref, pr_url, status, blocker_reason,
               recorded_by_session, created_at, updated_at
        FROM vcs_pr_publications
        WHERE publication_id = ?1
        "#,
        params![publication_id.as_str()],
        deserialize_row,
    )
    .optional()?
    .ok_or_else(|| invalid_projection("PR publication is not recorded"))
}

fn invalid_projection(reason: impl Into<String>) -> CoveyError {
    CoveyError::InvalidImportDestination {
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rusqlite::params;

    use crate::{
        Covey, ManualClock,
        model::{
            RecordVcsPacketStackEntryReq, RecordVcsPrPublicationReq, RegisterSessionReq,
            SessionRole, VcsPacketStackEntryState, VcsPrPublicationKind, VcsPrPublicationStatus,
        },
    };

    const TEST_NOW_MS: i64 = 1_700_000_000_000;

    fn seed_fixture(covey: &Covey) -> String {
        let session = covey
            .register_session(
                RegisterSessionReq::try_from_raw_parts(
                    "orch-vcs-projection",
                    "orch-vcs-projection-1",
                    SessionRole::Orchestrator,
                    "register-orch-vcs-projection",
                )
                .expect("valid orchestrator session"),
            )
            .expect("register orchestrator");

        let conn = covey.conn.lock().expect("covey connection mutex");
        conn.execute(
            "INSERT INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
             VALUES ('session-exec-vcs-projection', 'exec-vcs-projection', 'exec-vcs-projection-1', 'executor', 'active', NULL, 1, 1, 1, 1)",
            [],
        )
        .expect("insert executor session");
        conn.execute(
            "INSERT INTO sessions (session_token, agent_principal_id, agent_instance_id, role, state, active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at)
             VALUES ('session-reviewer-vcs-projection', 'reviewer-vcs-projection', 'reviewer-vcs-projection-1', 'reviewer', 'active', NULL, 1, 1, 1, 1)",
            [],
        )
        .expect("insert reviewer session");
        conn.execute(
            "INSERT INTO meta_tasks (meta_task_id, prompt_text, state, created_by, created_at, updated_at)
             VALUES ('openspec:vcs-projection-change', 'vcs projection fixture', 'active', ?1, 1, 1)",
            params![session.session_token()],
        )
        .expect("insert meta task");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, completion_policy, routing_key, created_at, updated_at)
             VALUES ('subtask-vcs-projection-1', 'openspec:vcs-projection-change', 'vcs projection work', 'work', NULL, NULL, 'available', NULL, NULL, 1, 'canonical_apply', 'mutai', 2, 2)",
            [],
        )
        .expect("insert work subtask");
        conn.execute(
            "INSERT INTO openspec_subtask_scope (subtask_id, openspec_change_id, openspec_task_id, source_path, scenario_refs_json, updated_at)
             VALUES ('subtask-vcs-projection-1', 'vcs-projection-change', '1.1', 'openspec/changes/vcs-projection-change/tasks.md', '[]', 2)",
            [],
        )
        .expect("insert openspec scope");
        conn.execute(
            "INSERT INTO claims (claim_id, subtask_id, owner_session_token, fence_seq, lease_deadline, state, created_at, updated_at)
             VALUES ('claim-vcs-projection-1', 'subtask-vcs-projection-1', 'session-exec-vcs-projection', 1, 9999999999999, 'held', 3, 3)",
            [],
        )
        .expect("insert claim");
        conn.execute(
            "INSERT INTO artifacts (artifact_digest, artifact_kind, base_rev, produced_by_subtask_id, produced_by_session, manifest_path, changed_paths_digest, created_at)
             VALUES ('blake3:vcsprojectionartifact', 'patch_bundle', 'base', 'subtask-vcs-projection-1', 'session-exec-vcs-projection', 'artifact.json', 'blake3:vcsprojectionpaths', 4)",
            [],
        )
        .expect("insert artifact");
        conn.execute(
            "UPDATE subtasks SET state = 'approved', artifact_digest = 'blake3:vcsprojectionartifact', updated_at = 5 WHERE subtask_id = 'subtask-vcs-projection-1'",
            [],
        )
        .expect("attach artifact to work subtask");
        conn.execute(
            "INSERT INTO subtasks (subtask_id, meta_task_id, title, kind, review_target_subtask_id, review_target_artifact_digest, state, current_claim_id, artifact_digest, priority, completion_policy, routing_key, created_at, updated_at)
             VALUES ('review-vcs-projection-1', 'openspec:vcs-projection-change', 'vcs projection review', 'review', 'subtask-vcs-projection-1', 'blake3:vcsprojectionartifact', 'decided', NULL, NULL, 1, 'canonical_apply', 'mutai', 5, 5)",
            [],
        )
        .expect("insert review subtask");
        conn.execute(
            "INSERT INTO reviews (review_id, subtask_id, artifact_digest, reviewer_session, review_subtask_id, verdict, findings_digest, state, created_at, updated_at)
             VALUES ('review-vcs-projection-1', 'subtask-vcs-projection-1', 'blake3:vcsprojectionartifact', 'session-reviewer-vcs-projection', 'review-vcs-projection-1', 'approve', 'blake3:vcsprojectionfindings', 'decided', 5, 5)",
            [],
        )
        .expect("insert review");

        session.session_token().to_owned()
    }

    fn packet_stack_req(session_token: &str) -> RecordVcsPacketStackEntryReq {
        RecordVcsPacketStackEntryReq::try_from_raw_parts(
            session_token,
            "stack-entry-vcs-projection-1",
            "vcs-projection-change",
            "packet/vcs-projection-change",
            "claim-vcs-projection-1",
            "subtask-vcs-projection-1",
            "blake3:vcsprojectionartifact",
            "review-vcs-projection-1",
            "blake3:vcsprojectionfindings",
            "claim/claim-vcs-projection-1",
            "change-vcs-projection-1",
            "commit-vcs-projection-1",
            0,
            Some("blake3:vcsprojectiontree".to_owned()),
            VcsPacketStackEntryState::TreeEquivalent,
            "record-stack-entry-vcs-projection-1",
        )
        .expect("valid packet stack request")
    }

    #[test]
    fn records_packet_stack_entry_with_approved_review_evidence() {
        let clock = Arc::new(ManualClock::new(TEST_NOW_MS));
        let covey = Covey::open_in_memory_with_clock(clock).expect("open covey");
        let session_token = seed_fixture(&covey);

        let entry = covey
            .record_vcs_packet_stack_entry(packet_stack_req(&session_token))
            .expect("record stack entry");

        assert_eq!(entry.openspec_change_id.as_str(), "vcs-projection-change");
        assert_eq!(entry.stack_position, 0);
        let entries = covey
            .vcs_packet_stack_entries_for_openspec("vcs-projection-change")
            .expect("lookup stack entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].claim_id.as_str(), "claim-vcs-projection-1");
    }

    #[test]
    fn rejects_packet_stack_entry_without_matching_approved_review() {
        let clock = Arc::new(ManualClock::new(TEST_NOW_MS));
        let covey = Covey::open_in_memory_with_clock(clock).expect("open covey");
        let session_token = seed_fixture(&covey);
        let mut req = packet_stack_req(&session_token);
        req.findings_digest = "blake3:wrongfindings"
            .parse()
            .expect("valid wrong findings digest");
        req.idempotency_key = "record-stack-entry-vcs-projection-wrong"
            .parse()
            .expect("valid idempotency key");

        let err = covey
            .record_vcs_packet_stack_entry(req)
            .expect_err("mismatched review evidence rejected");

        assert!(
            err.to_string()
                .contains("review evidence must approve the exact artifact")
        );
    }

    #[test]
    fn records_packet_pr_publication_after_stack_entry() {
        let clock = Arc::new(ManualClock::new(TEST_NOW_MS));
        let covey = Covey::open_in_memory_with_clock(clock).expect("open covey");
        let session_token = seed_fixture(&covey);
        covey
            .record_vcs_packet_stack_entry(packet_stack_req(&session_token))
            .expect("record stack entry");

        let publication = covey
            .record_vcs_pr_publication(
                RecordVcsPrPublicationReq::try_from_raw_parts(
                    &session_token,
                    "publication-vcs-projection-1",
                    VcsPrPublicationKind::Packet,
                    Some("vcs-projection-change".to_owned()),
                    None,
                    "packet/vcs-projection-change",
                    "commit-vcs-projection-packet",
                    "main",
                    Some("https://github.com/example/repo/pull/1".to_owned()),
                    VcsPrPublicationStatus::Published,
                    None,
                    "record-publication-vcs-projection-1",
                )
                .expect("valid publication request"),
            )
            .expect("record publication");

        assert_eq!(
            publication.bookmark.as_str(),
            "packet/vcs-projection-change"
        );
        let publications = covey
            .vcs_pr_publications_for_openspec("vcs-projection-change")
            .expect("lookup publications");
        assert_eq!(publications.len(), 1);
        assert_eq!(publications[0].status, VcsPrPublicationStatus::Published);
    }
}

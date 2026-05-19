use std::time::Instant;

use rusqlite::params;

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        CommandTranscriptDigest, EventType, ExitSessionReq, HeartbeatReq, ModelId, ObjectType,
        ProviderId, RecordRuntimeAttestationReq, RegisterSessionReq, RuntimeAttestation,
        SessionHandle, SessionState, TimestampMs,
    },
    queries::{load_session_tx, load_subtask_tx},
    schema::advance_lease_clock,
    store::append_session_event,
    validators::{
        MAX_AGENT_ID_LEN, MAX_RUNTIME_FIELD_LEN, ensure_length, ensure_no_other_active_session,
        ensure_non_empty, ensure_transition, require_active_session, require_session,
    },
};

impl Covey {
    /// Registers a new session or refreshes an already-active session with the same identity.
    pub fn register_session(&self, req: RegisterSessionReq) -> Result<SessionHandle> {
        ensure_length(
            "agent_principal_id",
            &req.agent_principal_id,
            MAX_AGENT_ID_LEN,
        )?;
        ensure_length(
            "agent_instance_id",
            &req.agent_instance_id,
            MAX_AGENT_ID_LEN,
        )?;
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.agent_principal_id,
                "register_session",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    ensure_no_other_active_session(tx, &req.agent_principal_id)?;
                    let session_token = crate::model::make_id("session");
                    tx.execute(
                        r#"
                        INSERT INTO sessions (
                            session_token, agent_principal_id, agent_instance_id, role, state,
                            active_subtask_id, last_heartbeat_at, last_heartbeat_tick, created_at, updated_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?6, ?6)
                        "#,
                        params![
                            session_token,
                            req.agent_principal_id,
                            req.agent_instance_id,
                            req.role.to_string(),
                            SessionState::Active.to_string(),
                            now,
                            lease_now,
                        ],
                    )?;
                    let handle = SessionHandle::new(
                        session_token.clone(),
                        req.agent_principal_id.clone(),
                        req.agent_instance_id.clone(),
                        req.role,
                    );
                    append_session_event(
                        tx,
                        EventType::SessionRegistered,
                        ObjectType::Session,
                        &session_token,
                        &session_token,
                        &handle,
                        now,
                    )?;
                    Ok(handle)
                },
            )
        });
        self.log_operation(
            "register_session",
            &req.agent_principal_id,
            started_at,
            &result,
            |handle| vec![format!("session:{}", handle.session_token)],
        );
        result
    }

    /// Records runtime identity evidence for an active session.
    pub fn record_runtime_attestation(
        &self,
        req: RecordRuntimeAttestationReq,
    ) -> Result<RuntimeAttestation> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "record_runtime_attestation",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    let session = require_active_session(tx, &req.session_token)?;
                    let session_token = session.session_token.clone();
                    validate_runtime_attestation_req(&req, &session_token)?;
                    let attestation =
                        RuntimeAttestation {
                            session_token: session.session_token.clone(),
                            agent_principal_id: session.agent_principal_id.clone(),
                            agent_instance_id: session.agent_instance_id.clone(),
                            role: session.role,
                            provider: ProviderId::parse(req.provider.clone()).map_err(|err| {
                                CoveyError::InvalidRuntimeAttestation {
                                    session_token: session_token.clone(),
                                    reason: err.to_string(),
                                }
                            })?,
                            model: ModelId::parse(req.model.clone()).map_err(|err| {
                                CoveyError::InvalidRuntimeAttestation {
                                    session_token: session_token.clone(),
                                    reason: err.to_string(),
                                }
                            })?,
                            provider_run_id: req.provider_run_id.clone(),
                            provider_run_id_issuer: req.provider_run_id_issuer.clone(),
                            process_id: req.process_id.clone(),
                            container_id: req.container_id.clone(),
                            command_transcript_digest: CommandTranscriptDigest::parse(
                                req.command_transcript_digest.clone(),
                            )
                            .map_err(|err| CoveyError::InvalidRuntimeAttestation {
                                session_token: session_token.clone(),
                                reason: err.to_string(),
                            })?,
                            started_at: TimestampMs::parse(req.started_at).map_err(|err| {
                                CoveyError::InvalidRuntimeAttestation {
                                    session_token: session_token.clone(),
                                    reason: err.to_string(),
                                }
                            })?,
                            ended_at: TimestampMs::parse(req.ended_at).map_err(|err| {
                                CoveyError::InvalidRuntimeAttestation {
                                    session_token: session_token.clone(),
                                    reason: err.to_string(),
                                }
                            })?,
                            recorded_at: TimestampMs::parse(now)
                                .expect("wall clock timestamps are non-negative"),
                        };
                    tx.execute(
                        r#"
                        INSERT INTO runtime_attestations (
                            session_token, agent_principal_id, agent_instance_id, role,
                            provider, model, provider_run_id, provider_run_id_issuer,
                            process_id, container_id,
                            command_transcript_digest, started_at, ended_at, recorded_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                        "#,
                        params![
                            attestation.session_token.as_str(),
                            attestation.agent_principal_id.as_str(),
                            attestation.agent_instance_id.as_str(),
                            attestation.role.to_string(),
                            attestation.provider.as_str(),
                            attestation.model.as_str(),
                            attestation.provider_run_id.as_str(),
                            attestation.provider_run_id_issuer.as_str(),
                            attestation.process_id.as_deref(),
                            attestation.container_id.as_deref(),
                            attestation.command_transcript_digest.as_str(),
                            attestation.started_at,
                            attestation.ended_at,
                            attestation.recorded_at,
                        ],
                    )?;
                    append_session_event(
                        tx,
                        EventType::RuntimeAttestationRecorded,
                        ObjectType::RuntimeAttestation,
                        &req.session_token,
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(attestation)
                },
            )
        });
        self.log_operation(
            "record_runtime_attestation",
            &req.session_token,
            started_at,
            &result,
            |attestation| vec![format!("session:{}", attestation.session_token)],
        );
        result
    }

    /// Records a heartbeat for an already-active session.
    pub fn heartbeat(&self, req: HeartbeatReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            let lease_now = advance_lease_clock(tx, now)?;
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "heartbeat",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    let session = require_session(tx, &req.session_token)?;
                    ensure_transition(
                        session.state(),
                        SessionState::Active,
                        ObjectType::Session,
                        session.state() == SessionState::Active,
                    )?;
                    tx.execute(
                        "UPDATE sessions SET last_heartbeat_at = ?2, last_heartbeat_tick = ?3, updated_at = ?2 WHERE session_token = ?1",
                        params![req.session_token, now, lease_now],
                    )?;
                    append_session_event(
                        tx,
                        EventType::SessionHeartbeat,
                        ObjectType::Session,
                        &req.session_token,
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(())
                },
            )
        });
        self.log_operation("heartbeat", &req.session_token, started_at, &result, |_| {
            vec![format!("session:{}", req.session_token)]
        });
        result
    }

    /// Marks a session as exited without deleting its historical record.
    pub fn exit_session(&self, req: ExitSessionReq) -> Result<()> {
        let started_at = Instant::now();
        let result = self.with_write_tx(|tx, now| {
            crate::store::with_idempotent_mutation(
                tx,
                &req.session_token,
                "exit_session",
                &req.idempotency_key,
                &req,
                crate::model::TimestampMs::parse(now)?,
                || {
                    let session = require_session(tx, &req.session_token)?;
                    ensure_transition(
                        session.state(),
                        SessionState::Exited,
                        ObjectType::Session,
                        matches!(session.state(), SessionState::Active | SessionState::Stale),
                    )?;
                    let updated = tx.execute(
                        "UPDATE sessions SET state = ?2, active_subtask_id = NULL, updated_at = ?3 WHERE session_token = ?1 AND state IN (?4, ?5)",
                        params![
                            req.session_token,
                            SessionState::Exited.to_string(),
                            now,
                            SessionState::Active.to_string(),
                            SessionState::Stale.to_string()
                        ],
                    )?;
                    if updated != 1 {
                        return Err(CoveyError::IllegalTransition {
                            from: session.state().into(),
                            to: SessionState::Exited.into(),
                            object: ObjectType::Session,
                        });
                    }
                    append_session_event(
                        tx,
                        EventType::SessionExited,
                        ObjectType::Session,
                        &req.session_token,
                        &req.session_token,
                        &req,
                        now,
                    )?;
                    Ok(())
                },
            )
        });
        self.log_operation(
            "exit_session",
            &req.session_token,
            started_at,
            &result,
            |_| vec![format!("session:{}", req.session_token)],
        );
        result
    }

    /// Returns the current persisted status for a session.
    pub fn session_status(&self, session_token: &str) -> Result<crate::model::SessionStatus> {
        let started_at = Instant::now();
        let result = self.with_read_tx(|tx| {
            let session = load_session_tx(tx, session_token)?;
            let active_subtask = session
                .active_subtask_id()
                .map(|subtask_id| load_subtask_tx(tx, subtask_id))
                .transpose()?
                .map(crate::model::SubtaskView::try_from)
                .transpose()?;
            Ok(crate::model::SessionStatus::new(session, active_subtask))
        });
        self.log_operation(
            "session_status",
            session_token,
            started_at,
            &result,
            |status| vec![format!("session:{}", status.session.session_token)],
        );
        result
    }
}

fn validate_runtime_attestation_req(
    req: &RecordRuntimeAttestationReq,
    session_token: &crate::model::SessionToken,
) -> Result<()> {
    ensure_length("provider", &req.provider, MAX_RUNTIME_FIELD_LEN)?;
    ensure_length("model", &req.model, MAX_RUNTIME_FIELD_LEN)?;
    ensure_length(
        "provider_run_id",
        &req.provider_run_id,
        MAX_RUNTIME_FIELD_LEN,
    )?;
    ensure_length(
        "provider_run_id_issuer",
        &req.provider_run_id_issuer,
        MAX_RUNTIME_FIELD_LEN,
    )?;
    ensure_length(
        "command_transcript_digest",
        &req.command_transcript_digest,
        MAX_RUNTIME_FIELD_LEN,
    )?;
    ensure_non_empty("provider", &req.provider, session_token)?;
    ensure_non_empty("model", &req.model, session_token)?;
    ensure_non_empty("provider_run_id", &req.provider_run_id, session_token)?;
    ensure_non_empty(
        "provider_run_id_issuer",
        &req.provider_run_id_issuer,
        session_token,
    )?;
    ensure_non_empty(
        "command_transcript_digest",
        &req.command_transcript_digest,
        session_token,
    )?;
    if let Some(process_id) = req.process_id.as_deref() {
        ensure_length("process_id", process_id, MAX_RUNTIME_FIELD_LEN)?;
        ensure_non_empty("process_id", process_id, session_token)?;
    }
    if let Some(container_id) = req.container_id.as_deref() {
        ensure_length("container_id", container_id, MAX_RUNTIME_FIELD_LEN)?;
        ensure_non_empty("container_id", container_id, session_token)?;
    }
    if req.process_id.is_none() && req.container_id.is_none() {
        return Err(CoveyError::InvalidRuntimeAttestation {
            session_token: session_token.clone(),
            reason: "process_id or container_id is required".to_owned(),
        });
    }
    if req.ended_at < req.started_at {
        return Err(CoveyError::InvalidRuntimeAttestation {
            session_token: session_token.clone(),
            reason: "ended_at must be greater than or equal to started_at".to_owned(),
        });
    }
    Ok(())
}

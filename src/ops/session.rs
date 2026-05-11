use std::time::Instant;

use rusqlite::params;

use crate::{
    Covey,
    error::{CoveyError, Result},
    model::{
        EventType, ExitSessionReq, HeartbeatReq, ObjectType, RegisterSessionReq, SessionHandle,
        SessionState,
    },
    queries::{load_session_tx, load_subtask_tx},
    schema::advance_lease_clock,
    store::append_session_event,
    validators::{
        MAX_AGENT_ID_LEN, ensure_length, ensure_no_other_active_session, ensure_transition,
        require_session,
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
                now,
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
                now,
                || {
                    let session = require_session(tx, &req.session_token)?;
                    ensure_transition(
                        session.state,
                        SessionState::Active,
                        ObjectType::Session,
                        session.state == SessionState::Active,
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
                now,
                || {
                    let session = require_session(tx, &req.session_token)?;
                    ensure_transition(
                        session.state,
                        SessionState::Exited,
                        ObjectType::Session,
                        matches!(session.state, SessionState::Active | SessionState::Stale),
                    )?;
                    let updated = tx.execute(
                        "UPDATE sessions SET state = ?2, updated_at = ?3 WHERE session_token = ?1 AND state IN (?4, ?5)",
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
                            from: session.state.into(),
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
                .active_subtask_id
                .as_deref()
                .map(|subtask_id| load_subtask_tx(tx, subtask_id))
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

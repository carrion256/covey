use super::*;

pub(super) fn dispatch_session(store: &Covey, command: SessionCommand) -> covey::Result<Rendered> {
    match command {
        SessionCommand::Register(args) => {
            let handle = store.register_session(RegisterSessionReq {
                agent_principal_id: args.agent_principal_id,
                agent_instance_id: args.agent_instance_id,
                role: args.role.into(),
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("register-session")),
            })?;
            Ok(Rendered::summary(
                &handle,
                format!(
                    "session {} role={} principal={}",
                    handle.session_token, handle.role, handle.agent_principal_id
                ),
            ))
        }
        SessionCommand::Attest(args) => {
            let session_token_for_error = args.session_token.clone();
            let attestation = store.record_runtime_attestation(
                RecordRuntimeAttestationReq::try_from_parts(
                    args.session_token.clone(),
                    args.provider,
                    args.model,
                    args.provider_run_id,
                    args.provider_run_id_issuer,
                    args.process_id,
                    args.container_id,
                    args.command_transcript_digest,
                    args.started_at,
                    args.ended_at,
                    args.idempotency_key
                        .unwrap_or_else(|| new_idempotency_key("record-runtime-attestation")),
                )
                .map_err(|reason| {
                    match covey::SessionToken::parse(session_token_for_error.clone()) {
                        Ok(session_token) => covey::CoveyError::InvalidRuntimeAttestation {
                            session_token,
                            reason,
                        },
                        Err(_) => covey::CoveyError::InvalidSessionToken {
                            session_token: session_token_for_error,
                        },
                    }
                })?,
            )?;
            Ok(Rendered::summary(
                &attestation,
                format!(
                    "runtime attestation {} role={} principal={}",
                    attestation.session_token, attestation.role, attestation.agent_principal_id
                ),
            ))
        }
        SessionCommand::Heartbeat(args) => {
            store.heartbeat(HeartbeatReq {
                session_token: args.session_token.clone(),
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("heartbeat")),
            })?;
            Ok(Rendered::summary(
                SessionTokenAck {
                    operation: "heartbeat",
                    session_token: args.session_token.clone(),
                },
                format!("heartbeat {}", args.session_token),
            ))
        }
        SessionCommand::Exit(args) => {
            store.exit_session(ExitSessionReq {
                session_token: args.session_token.clone(),
                idempotency_key: args
                    .idempotency_key
                    .unwrap_or_else(|| new_idempotency_key("exit-session")),
            })?;
            Ok(Rendered::summary(
                SessionTokenAck {
                    operation: "exit",
                    session_token: args.session_token.clone(),
                },
                format!("session exited {}", args.session_token),
            ))
        }
        SessionCommand::Status(args) => {
            let status = store.session_status(&args.session_token)?;
            Ok(Rendered::pretty(&status))
        }
    }
}

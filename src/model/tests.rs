use super::{
    ActorKind, Event, EventPayload, EventType, HeartbeatReq, ObjectType, SessionHandle,
    SessionRole, bd_import_v1_subtask_id, make_id, parse_generated_members,
};
use crate::CoveyError;

#[test]
fn make_id_uses_prefix_and_uuid_suffix() {
    let id = make_id("claim");

    assert!(id.starts_with("claim_"));
    assert_eq!(id.len(), "claim_".len() + 32);
    assert!(
        id["claim_".len()..]
            .chars()
            .all(|ch| ch.is_ascii_hexdigit())
    );
}

#[test]
fn bd_import_v1_subtask_id_is_deterministic_and_traceable() {
    let first = bd_import_v1_subtask_id("ISSUE-42/fix import semantics");
    let second = bd_import_v1_subtask_id("ISSUE-42/fix import semantics");
    let different = bd_import_v1_subtask_id("ISSUE-43/fix import semantics");

    assert_eq!(first, second);
    assert_ne!(first, different);
    assert!(first.starts_with("bdwork_issue_42_fix_import_"));
}

#[test]
fn parse_generated_members_decodes_string_arrays() {
    let raw = serde_json::to_string(&vec!["src/lib.rs", "src/main.rs"])
        .expect("vector serialization must succeed");

    let parsed = parse_generated_members(&raw).expect("array payload must parse");

    assert_eq!(parsed, vec!["src/lib.rs", "src/main.rs"]);
}

#[test]
fn parse_generated_members_rejects_non_array_payloads() {
    let raw = serde_json::to_string("src/lib.rs").expect("string serialization must succeed");

    let err = parse_generated_members(&raw).expect_err("scalar payload must be rejected");

    assert!(
        err.to_string().contains("sequence"),
        "unexpected error message: {err}"
    );
}

#[test]
fn event_payload_from_json_decodes_typed_payloads() {
    let payload = SessionHandle::new(
        "session-1".to_owned(),
        "principal-1".to_owned(),
        "instance-1".to_owned(),
        SessionRole::Executor,
    );
    let payload_json =
        serde_json::to_string(&payload).expect("session handle serialization must succeed");

    let decoded = EventPayload::from_json(EventType::SessionRegistered, &payload_json)
        .expect("matching payload must decode");

    assert_eq!(decoded, EventPayload::SessionRegistered(payload));
}

#[test]
fn event_payload_from_json_rejects_mismatched_payloads() {
    let payload = SessionHandle::new(
        "session-1".to_owned(),
        "principal-1".to_owned(),
        "instance-1".to_owned(),
        SessionRole::Executor,
    );
    let payload_json =
        serde_json::to_string(&payload).expect("session handle serialization must succeed");

    let err = EventPayload::from_json(EventType::SessionHeartbeat, &payload_json)
        .expect_err("wrong event type must fail to decode");

    assert!(matches!(err, CoveyError::SerializationError(_)));
}

#[test]
fn event_typed_decodes_payload_and_preserves_event_metadata() {
    let payload = HeartbeatReq {
        session_token: "session-1".to_owned(),
        idempotency_key: "idem-1".to_owned(),
    };
    let event = Event {
        seq: 42,
        event_type: EventType::SessionHeartbeat,
        object_type: ObjectType::Session,
        object_id: "session-1".to_owned(),
        actor_kind: ActorKind::Session,
        session_token: Some("session-1".to_owned()),
        payload_json: serde_json::to_string(&payload)
            .expect("heartbeat serialization must succeed"),
        created_at: 1_234,
    };

    let typed = event.typed().expect("event payload must decode");

    assert_eq!(typed.seq, 42);
    assert_eq!(typed.object_id, "session-1");
    assert_eq!(typed.payload, EventPayload::SessionHeartbeat(payload));
}

#[test]
fn event_typed_propagates_payload_decode_failures() {
    let event = Event {
        seq: 1,
        event_type: EventType::SessionHeartbeat,
        object_type: ObjectType::Session,
        object_id: "session-1".to_owned(),
        actor_kind: ActorKind::Session,
        session_token: Some("session-1".to_owned()),
        payload_json: "{".to_owned(),
        created_at: 99,
    };

    let err = event
        .typed()
        .expect_err("malformed payload must fail to decode");

    assert!(matches!(err, CoveyError::SerializationError(_)));
}

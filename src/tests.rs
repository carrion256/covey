use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::{
    Clock, Covey, ManualClock, RegisterSessionReq, Result, SessionRole, SubmitMetaTaskReq,
    schema::{apply_migrations, apply_pragmas},
};

impl Covey {
    /// Opens an in-memory Covey database with an injected clock for tests.
    pub fn open_in_memory_with_clock(clock: Arc<dyn Clock>) -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        apply_pragmas(&conn)?;
        apply_migrations(&mut conn)?;
        Ok(Self {
            db_path: None,
            conn: Mutex::new(conn),
            clock,
        })
    }
}

#[test]
fn in_memory_covey_exercises_shared_connection_read_paths() {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_in_memory_with_clock(clock).expect("open in-memory covey");
    let session = covey
        .register_session(RegisterSessionReq {
            agent_principal_id: "in-memory-orchestrator".into(),
            agent_instance_id: "in-memory-orchestrator-1".into(),
            role: SessionRole::Orchestrator,
            idempotency_key: "register-in-memory".into(),
        })
        .expect("register session");
    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: session.session_token.clone(),
            prompt_text: "exercise in-memory read paths".into(),
            idempotency_key: "submit-in-memory".into(),
        })
        .expect("submit meta task");

    assert_eq!(
        covey
            .meta_task_status(&meta_task_id)
            .expect("meta status")
            .meta_task
            .meta_task_id,
        meta_task_id
    );
    assert!(!covey.fetch_events(0, 10).expect("fetch events").is_empty());
}

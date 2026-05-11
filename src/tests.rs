use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::{
    Clock, Covey, Result,
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

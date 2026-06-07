//! Orchestration coordination store, ported from
//! `src/main/runtime/orchestration/db.ts`. Schema is faithful to the source;
//! this first cut implements the message + task operations (dispatch contexts,
//! decision gates, and coordinator runs share the schema and are added next).

use orca_store::{Database, OpenOptions, StoreError};
use rusqlite::{params, OptionalExtension};

/// The full orchestration schema (verbatim from `db.ts createTables`).
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS messages (
  id            TEXT NOT NULL,
  from_handle   TEXT NOT NULL,
  to_handle     TEXT NOT NULL,
  subject       TEXT NOT NULL,
  body          TEXT NOT NULL DEFAULT '',
  type          TEXT NOT NULL DEFAULT 'status'
    CHECK(type IN ('status','dispatch','worker_done','merge_ready','escalation','handoff','decision_gate','heartbeat')),
  priority      TEXT NOT NULL DEFAULT 'normal'
    CHECK(priority IN ('normal','high','urgent')),
  thread_id     TEXT,
  payload       TEXT,
  read          INTEGER NOT NULL DEFAULT 0,
  sequence      INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at    TEXT NOT NULL DEFAULT (datetime('now')),
  delivered_at  TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_id ON messages(id);
CREATE INDEX IF NOT EXISTS idx_inbox ON messages(to_handle, read);
CREATE INDEX IF NOT EXISTS idx_thread ON messages(thread_id);

CREATE TABLE IF NOT EXISTS tasks (
  id            TEXT PRIMARY KEY,
  parent_id     TEXT,
  created_by_terminal_handle TEXT,
  spec          TEXT NOT NULL,
  status        TEXT NOT NULL DEFAULT 'pending'
    CHECK(status IN ('pending','ready','dispatched','completed','failed','blocked')),
  deps          TEXT NOT NULL DEFAULT '[]',
  result        TEXT,
  created_at    TEXT NOT NULL DEFAULT (datetime('now')),
  completed_at  TEXT
);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_parent ON tasks(parent_id);

CREATE TABLE IF NOT EXISTS dispatch_contexts (
  id                  TEXT PRIMARY KEY,
  task_id             TEXT NOT NULL,
  assignee_handle     TEXT,
  status              TEXT NOT NULL DEFAULT 'pending'
    CHECK(status IN ('pending','dispatched','completed','failed','circuit_broken')),
  failure_count       INTEGER NOT NULL DEFAULT 0,
  last_failure        TEXT,
  dispatched_at       TEXT,
  completed_at        TEXT,
  created_at          TEXT NOT NULL DEFAULT (datetime('now')),
  last_heartbeat_at   TEXT
);
CREATE INDEX IF NOT EXISTS idx_dispatch_task ON dispatch_contexts(task_id);
CREATE INDEX IF NOT EXISTS idx_dispatch_status ON dispatch_contexts(status);

CREATE TABLE IF NOT EXISTS decision_gates (
  id            TEXT PRIMARY KEY,
  task_id       TEXT NOT NULL,
  question      TEXT NOT NULL,
  options       TEXT NOT NULL DEFAULT '[]',
  status        TEXT NOT NULL DEFAULT 'pending'
    CHECK(status IN ('pending','resolved','timeout')),
  resolution    TEXT,
  created_at    TEXT NOT NULL DEFAULT (datetime('now')),
  resolved_at   TEXT
);
CREATE INDEX IF NOT EXISTS idx_gates_task ON decision_gates(task_id);
CREATE INDEX IF NOT EXISTS idx_gates_status ON decision_gates(status);

CREATE TABLE IF NOT EXISTS coordinator_runs (
  id                  TEXT PRIMARY KEY,
  spec                TEXT NOT NULL,
  status              TEXT NOT NULL DEFAULT 'idle'
    CHECK(status IN ('idle','running','completed','failed')),
  coordinator_handle  TEXT NOT NULL,
  poll_interval_ms    INTEGER NOT NULL DEFAULT 2000,
  created_at          TEXT NOT NULL DEFAULT (datetime('now')),
  completed_at        TEXT
);
"#;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewMessage {
    pub id: String,
    pub from_handle: String,
    pub to_handle: String,
    pub subject: String,
    pub body: String,
    pub message_type: String,
    pub priority: String,
    pub thread_id: Option<String>,
    pub payload: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub from_handle: String,
    pub to_handle: String,
    pub subject: String,
    pub body: String,
    pub message_type: String,
    pub priority: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub parent_id: Option<String>,
    pub spec: String,
    pub status: String,
    pub deps: String,
    pub result: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchContext {
    pub id: String,
    pub task_id: String,
    pub assignee_handle: Option<String>,
    pub status: String,
    pub failure_count: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionGate {
    pub id: String,
    pub task_id: String,
    pub question: String,
    pub options: String,
    pub status: String,
    pub resolution: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoordinatorRun {
    pub id: String,
    pub spec: String,
    pub status: String,
    pub coordinator_handle: String,
    pub poll_interval_ms: i64,
}

pub struct OrchestrationDb {
    db: Database,
}

impl OrchestrationDb {
    pub fn open(path: &str) -> Result<Self, StoreError> {
        let db = Database::open(path, OpenOptions::default())?;
        Self::init(db)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(Database::open_in_memory()?)
    }

    fn init(db: Database) -> Result<Self, StoreError> {
        // WAL is a no-op for :memory:; harmless and matches the TS pragmas.
        db.set_pragma_i64("busy_timeout", 5000)?;
        db.exec("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        db.exec(SCHEMA)?;
        Ok(Self { db })
    }

    pub fn send_message(&self, message: &NewMessage) -> Result<(), StoreError> {
        self.db.connection().execute(
            "INSERT INTO messages (id, from_handle, to_handle, subject, body, type, priority, thread_id, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                message.id, message.from_handle, message.to_handle, message.subject, message.body,
                message.message_type, message.priority, message.thread_id, message.payload,
            ],
        )?;
        Ok(())
    }

    /// Unread messages addressed to `handle`, oldest first.
    pub fn inbox(&self, handle: &str) -> Result<Vec<Message>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(
            "SELECT id, from_handle, to_handle, subject, body, type, priority
             FROM messages WHERE to_handle = ?1 AND read = 0 ORDER BY sequence",
        )?;
        let mut rows = stmt.query([handle])?;
        let mut messages = Vec::new();
        while let Some(row) = rows.next()? {
            messages.push(Message {
                id: row.get(0)?,
                from_handle: row.get(1)?,
                to_handle: row.get(2)?,
                subject: row.get(3)?,
                body: row.get(4)?,
                message_type: row.get(5)?,
                priority: row.get(6)?,
            });
        }
        Ok(messages)
    }

    /// Mark a message read by id; returns the number of rows updated.
    pub fn mark_read(&self, id: &str) -> Result<usize, StoreError> {
        Ok(self
            .db
            .connection()
            .execute("UPDATE messages SET read = 1 WHERE id = ?1", params![id])?)
    }

    pub fn create_task(
        &self,
        id: &str,
        spec: &str,
        parent_id: Option<&str>,
        deps_json: &str,
        created_by: Option<&str>,
    ) -> Result<(), StoreError> {
        self.db.connection().execute(
            "INSERT INTO tasks (id, parent_id, created_by_terminal_handle, spec, deps)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, parent_id, created_by, spec, deps_json],
        )?;
        Ok(())
    }

    /// Tasks, optionally filtered by status, oldest first.
    pub fn list_tasks(&self, status: Option<&str>) -> Result<Vec<Task>, StoreError> {
        let conn = self.db.connection();
        let select = "SELECT id, parent_id, spec, status, deps, result FROM tasks";
        let mut tasks = Vec::new();
        if let Some(status) = status {
            let mut stmt = conn.prepare(&format!("{select} WHERE status = ?1 ORDER BY created_at"))?;
            let mut rows = stmt.query([status])?;
            while let Some(row) = rows.next()? {
                tasks.push(row_to_task(row)?);
            }
        } else {
            let mut stmt = conn.prepare(&format!("{select} ORDER BY created_at"))?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                tasks.push(row_to_task(row)?);
            }
        }
        Ok(tasks)
    }

    pub fn update_task_status(&self, id: &str, status: &str, result: Option<&str>) -> Result<usize, StoreError> {
        Ok(self.db.connection().execute(
            "UPDATE tasks SET status = ?2, result = ?3 WHERE id = ?1",
            params![id, status, result],
        )?)
    }

    pub fn get_task(&self, id: &str) -> Result<Option<Task>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare("SELECT id, parent_id, spec, status, deps, result FROM tasks WHERE id = ?1")?;
        let mut rows = stmt.query([id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_task(row)?)),
            None => Ok(None),
        }
    }

    // ---- dispatch contexts ----

    /// Dispatch `task_id` (which must be `ready`) to `assignee_handle`. Refuses
    /// if the assignee already has an active dispatch; carries the failure count
    /// forward (circuit breaker); marks the task `dispatched`.
    pub fn create_dispatch_context(
        &self,
        task_id: &str,
        assignee_handle: &str,
        id: &str,
    ) -> Result<DispatchContext, StoreError> {
        let task = self
            .get_task(task_id)?
            .ok_or_else(|| StoreError::Message(format!("Task not found: {task_id}")))?;
        if task.status != "ready" {
            return Err(StoreError::Message(format!(
                "Task {task_id} is {}; only ready tasks can be dispatched",
                task.status
            )));
        }
        let conn = self.db.connection();
        let active: Option<String> = conn
            .query_row(
                "SELECT id FROM dispatch_contexts WHERE assignee_handle = ?1 AND status IN ('pending','dispatched')",
                [assignee_handle],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = active {
            return Err(StoreError::Message(format!(
                "Terminal {assignee_handle} already has an active dispatch ({existing})"
            )));
        }
        let prior_failures: i64 = conn.query_row(
            "SELECT COALESCE(MAX(failure_count), 0) FROM dispatch_contexts WHERE task_id = ?1",
            [task_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO dispatch_contexts (id, task_id, assignee_handle, status, failure_count, dispatched_at)
             VALUES (?1, ?2, ?3, 'dispatched', ?4, datetime('now'))",
            params![id, task_id, assignee_handle, prior_failures],
        )?;
        conn.execute("UPDATE tasks SET status = 'dispatched' WHERE id = ?1", params![task_id])?;
        self.dispatch_context_by_id(id)?
            .ok_or_else(|| StoreError::Message("dispatch context vanished after insert".into()))
    }

    pub fn dispatch_context_by_id(&self, id: &str) -> Result<Option<DispatchContext>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, assignee_handle, status, failure_count FROM dispatch_contexts WHERE id = ?1",
        )?;
        let mut rows = stmt.query([id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_dispatch(row)?)),
            None => Ok(None),
        }
    }

    pub fn complete_dispatch(&self, id: &str) -> Result<usize, StoreError> {
        Ok(self.db.connection().execute(
            "UPDATE dispatch_contexts SET status = 'completed', completed_at = datetime('now') WHERE id = ?1",
            params![id],
        )?)
    }

    // ---- decision gates ----

    pub fn create_gate(
        &self,
        id: &str,
        task_id: &str,
        question: &str,
        options: &[&str],
    ) -> Result<DecisionGate, StoreError> {
        self.db.connection().execute(
            "INSERT INTO decision_gates (id, task_id, question, options) VALUES (?1, ?2, ?3, ?4)",
            params![id, task_id, question, json_string_array(options)],
        )?;
        self.gate_by_id(id)?
            .ok_or_else(|| StoreError::Message("gate vanished after insert".into()))
    }

    pub fn resolve_gate(&self, id: &str, resolution: &str) -> Result<Option<DecisionGate>, StoreError> {
        self.db.connection().execute(
            "UPDATE decision_gates SET status = 'resolved', resolution = ?2, resolved_at = datetime('now') WHERE id = ?1",
            params![id, resolution],
        )?;
        self.gate_by_id(id)
    }

    pub fn gate_by_id(&self, id: &str) -> Result<Option<DecisionGate>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, question, options, status, resolution FROM decision_gates WHERE id = ?1",
        )?;
        let mut rows = stmt.query([id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_gate(row)?)),
            None => Ok(None),
        }
    }

    /// Gates for `task_id`, optionally filtered by status, oldest first.
    pub fn list_gates(&self, task_id: &str, status: Option<&str>) -> Result<Vec<DecisionGate>, StoreError> {
        let conn = self.db.connection();
        let select = "SELECT id, task_id, question, options, status, resolution FROM decision_gates";
        let mut gates = Vec::new();
        if let Some(status) = status {
            let mut stmt = conn.prepare(&format!("{select} WHERE task_id = ?1 AND status = ?2 ORDER BY created_at"))?;
            let mut rows = stmt.query(params![task_id, status])?;
            while let Some(row) = rows.next()? {
                gates.push(row_to_gate(row)?);
            }
        } else {
            let mut stmt = conn.prepare(&format!("{select} WHERE task_id = ?1 ORDER BY created_at"))?;
            let mut rows = stmt.query([task_id])?;
            while let Some(row) = rows.next()? {
                gates.push(row_to_gate(row)?);
            }
        }
        Ok(gates)
    }

    // ---- coordinator runs ----

    pub fn create_coordinator_run(
        &self,
        id: &str,
        spec: &str,
        coordinator_handle: &str,
        poll_interval_ms: Option<i64>,
    ) -> Result<CoordinatorRun, StoreError> {
        self.db.connection().execute(
            "INSERT INTO coordinator_runs (id, spec, status, coordinator_handle, poll_interval_ms)
             VALUES (?1, ?2, 'running', ?3, ?4)",
            params![id, spec, coordinator_handle, poll_interval_ms.unwrap_or(2000)],
        )?;
        self.coordinator_run_by_id(id)?
            .ok_or_else(|| StoreError::Message("coordinator run vanished after insert".into()))
    }

    pub fn coordinator_run_by_id(&self, id: &str) -> Result<Option<CoordinatorRun>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(
            "SELECT id, spec, status, coordinator_handle, poll_interval_ms FROM coordinator_runs WHERE id = ?1",
        )?;
        let mut rows = stmt.query([id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_coordinator(row)?)),
            None => Ok(None),
        }
    }

    /// Update status; terminal states (`completed`/`failed`) stamp `completed_at`.
    pub fn update_coordinator_run(&self, id: &str, status: &str) -> Result<Option<CoordinatorRun>, StoreError> {
        let conn = self.db.connection();
        if status == "completed" || status == "failed" {
            conn.execute(
                "UPDATE coordinator_runs SET status = ?2, completed_at = datetime('now') WHERE id = ?1",
                params![id, status],
            )?;
        } else {
            conn.execute("UPDATE coordinator_runs SET status = ?2 WHERE id = ?1", params![id, status])?;
        }
        self.coordinator_run_by_id(id)
    }

    /// The most recent still-running coordinator, if any.
    pub fn active_coordinator_run(&self) -> Result<Option<CoordinatorRun>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(
            "SELECT id, spec, status, coordinator_handle, poll_interval_ms FROM coordinator_runs
             WHERE status = 'running' ORDER BY created_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_coordinator(row)?)),
            None => Ok(None),
        }
    }
}

fn row_to_coordinator(row: &rusqlite::Row<'_>) -> rusqlite::Result<CoordinatorRun> {
    Ok(CoordinatorRun {
        id: row.get(0)?,
        spec: row.get(1)?,
        status: row.get(2)?,
        coordinator_handle: row.get(3)?,
        poll_interval_ms: row.get(4)?,
    })
}

fn row_to_dispatch(row: &rusqlite::Row<'_>) -> rusqlite::Result<DispatchContext> {
    Ok(DispatchContext {
        id: row.get(0)?,
        task_id: row.get(1)?,
        assignee_handle: row.get(2)?,
        status: row.get(3)?,
        failure_count: row.get(4)?,
    })
}

fn row_to_gate(row: &rusqlite::Row<'_>) -> rusqlite::Result<DecisionGate> {
    Ok(DecisionGate {
        id: row.get(0)?,
        task_id: row.get(1)?,
        question: row.get(2)?,
        options: row.get(3)?,
        status: row.get(4)?,
        resolution: row.get(5)?,
    })
}

/// Encode `["a","b"]` for the gate `options` column (no serde dependency).
fn json_string_array(items: &[&str]) -> String {
    let mut out = String::from("[");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        for ch in item.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                _ => out.push(ch),
            }
        }
        out.push('"');
    }
    out.push(']');
    out
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        parent_id: row.get(1)?,
        spec: row.get(2)?,
        status: row.get(3)?,
        deps: row.get(4)?,
        result: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, to: &str, subject: &str) -> NewMessage {
        NewMessage {
            id: id.to_string(),
            from_handle: "coordinator".to_string(),
            to_handle: to.to_string(),
            subject: subject.to_string(),
            body: String::new(),
            message_type: "status".to_string(),
            priority: "normal".to_string(),
            thread_id: None,
            payload: None,
        }
    }

    #[test]
    fn creates_schema_on_open() {
        // No panic / error means the full schema + indexes applied cleanly.
        let db = OrchestrationDb::open_in_memory().unwrap();
        assert!(db.inbox("nobody").unwrap().is_empty());
    }

    #[test]
    fn sends_and_reads_inbox_then_marks_read() {
        let db = OrchestrationDb::open_in_memory().unwrap();
        db.send_message(&msg("m1", "worker-a", "do the thing")).unwrap();
        db.send_message(&msg("m2", "worker-b", "other thing")).unwrap();

        let inbox = db.inbox("worker-a").unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].id, "m1");
        assert_eq!(inbox[0].subject, "do the thing");
        assert_eq!(inbox[0].from_handle, "coordinator");

        assert_eq!(db.mark_read("m1").unwrap(), 1);
        assert!(db.inbox("worker-a").unwrap().is_empty());
        assert_eq!(db.inbox("worker-b").unwrap().len(), 1);
    }

    #[test]
    fn message_type_check_constraint_rejects_invalid_type() {
        let db = OrchestrationDb::open_in_memory().unwrap();
        let mut bad = msg("m1", "worker-a", "x");
        bad.message_type = "not-a-real-type".to_string();
        assert!(db.send_message(&bad).is_err());
    }

    #[test]
    fn creates_lists_and_updates_tasks() {
        let db = OrchestrationDb::open_in_memory().unwrap();
        db.create_task("t1", "build the parser", None, "[]", Some("term-1")).unwrap();
        db.create_task("t2", "write tests", Some("t1"), "[\"t1\"]", None).unwrap();

        let all = db.list_tasks(None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, "t1");
        assert_eq!(all[0].status, "pending");
        assert_eq!(all[1].parent_id.as_deref(), Some("t1"));
        assert_eq!(all[1].deps, "[\"t1\"]");

        assert_eq!(db.update_task_status("t1", "completed", Some("done")).unwrap(), 1);
        let completed = db.list_tasks(Some("completed")).unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].id, "t1");
        assert_eq!(completed[0].result.as_deref(), Some("done"));
        assert_eq!(db.list_tasks(Some("pending")).unwrap().len(), 1);
    }

    #[test]
    fn decision_gates_create_list_resolve() {
        let db = OrchestrationDb::open_in_memory().unwrap();
        let gate = db.create_gate("g1", "t1", "Proceed with deploy?", &["yes", "no"]).unwrap();
        assert_eq!(gate.status, "pending");
        assert_eq!(gate.options, "[\"yes\",\"no\"]");
        assert_eq!(gate.resolution, None);

        assert_eq!(db.list_gates("t1", None).unwrap().len(), 1);
        assert_eq!(db.list_gates("t1", Some("pending")).unwrap().len(), 1);

        let resolved = db.resolve_gate("g1", "yes").unwrap().unwrap();
        assert_eq!(resolved.status, "resolved");
        assert_eq!(resolved.resolution.as_deref(), Some("yes"));
        assert!(db.list_gates("t1", Some("pending")).unwrap().is_empty());
        assert_eq!(db.list_gates("t1", Some("resolved")).unwrap().len(), 1);
    }

    #[test]
    fn dispatch_requires_ready_task_and_one_active_per_assignee() {
        let db = OrchestrationDb::open_in_memory().unwrap();
        db.create_task("t1", "spec1", None, "[]", None).unwrap();
        db.create_task("t2", "spec2", None, "[]", None).unwrap();

        // Not-ready task cannot be dispatched.
        assert!(db.create_dispatch_context("t1", "worker-1", "ctx0").is_err());
        // Unknown task.
        assert!(db.create_dispatch_context("nope", "worker-1", "ctxX").is_err());

        db.update_task_status("t1", "ready", None).unwrap();
        db.update_task_status("t2", "ready", None).unwrap();

        let ctx = db.create_dispatch_context("t1", "worker-1", "ctx1").unwrap();
        assert_eq!(ctx.status, "dispatched");
        assert_eq!(ctx.failure_count, 0);
        // Task is now dispatched.
        assert_eq!(db.get_task("t1").unwrap().unwrap().status, "dispatched");

        // worker-1 already has an active dispatch → second (on ready t2) refused.
        let err = db.create_dispatch_context("t2", "worker-1", "ctx2").unwrap_err();
        assert!(err.to_string().contains("already has an active dispatch"), "{err}");

        // After completing ctx1, worker-1 is free again.
        assert_eq!(db.complete_dispatch("ctx1").unwrap(), 1);
        let ctx3 = db.create_dispatch_context("t2", "worker-1", "ctx3").unwrap();
        assert_eq!(ctx3.task_id, "t2");
        assert_eq!(db.dispatch_context_by_id("ctx3").unwrap().unwrap().status, "dispatched");
    }

    #[test]
    fn coordinator_run_lifecycle() {
        let db = OrchestrationDb::open_in_memory().unwrap();
        let run = db.create_coordinator_run("run1", "ship it", "coordinator-a", None).unwrap();
        assert_eq!(run.status, "running");
        assert_eq!(run.poll_interval_ms, 2000); // default

        assert_eq!(db.active_coordinator_run().unwrap().unwrap().id, "run1");

        let done = db.update_coordinator_run("run1", "completed").unwrap().unwrap();
        assert_eq!(done.status, "completed");
        assert!(db.active_coordinator_run().unwrap().is_none()); // no longer running

        let custom = db.create_coordinator_run("run2", "spec", "coordinator-b", Some(500)).unwrap();
        assert_eq!(custom.poll_interval_ms, 500);
    }
}

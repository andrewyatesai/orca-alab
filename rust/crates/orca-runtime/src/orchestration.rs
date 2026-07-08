//! Orchestration coordination store, ported from
//! `src/main/runtime/orchestration/db.ts`. Schema creation and the
//! `user_version` migration ladder live in `orchestration_schema`; this module
//! implements the message + task operations (dispatch contexts, decision
//! gates, and coordinator runs share the schema and are added next).

use crate::orchestration_schema;
use orca_store::{Database, OpenOptions, StoreError};
use rusqlite::{params, OptionalExtension};

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

    /// Borrow the underlying SQLite connection for raw introspection — used by
    /// tests and the parity state-dump harness, not by production callers.
    pub fn connection(&self) -> &rusqlite::Connection {
        self.db.connection()
    }

    fn init(db: Database) -> Result<Self, StoreError> {
        // Why: same pragmas in the same order as the TS constructor. WAL is a
        // no-op for :memory:; harmless on both sides.
        db.exec("PRAGMA journal_mode = WAL")?;
        db.exec("PRAGMA synchronous = NORMAL")?;
        db.exec("PRAGMA busy_timeout = 5000")?;
        orchestration_schema::create_tables(&db)?;
        orchestration_schema::migrate(&db)?;
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

    /// Stamp `delivered_at` (db.ts `markAsDelivered`): a push-on-idle delivery
    /// marker, distinct from the `read` bit, so a queued row is auto-pushed at
    /// most once. Uses `datetime('now')` for format-consistency with the other
    /// SQL-default timestamps.
    pub fn mark_delivered(&self, id: &str) -> Result<usize, StoreError> {
        Ok(self.db.connection().execute(
            "UPDATE messages SET delivered_at = datetime('now') WHERE id = ?1",
            params![id],
        )?)
    }

    /// Unread AND undelivered messages for a handle, oldest first — the
    /// push-on-idle replay guard (db.ts `getUndeliveredUnreadMessages`): a
    /// delivered-but-unread row is filtered out so it is not re-injected.
    pub fn undelivered_inbox(&self, handle: &str) -> Result<Vec<Message>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(
            "SELECT id, from_handle, to_handle, subject, body, type, priority
             FROM messages WHERE to_handle = ?1 AND read = 0 AND delivered_at IS NULL ORDER BY sequence",
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

    /// Insert a task. Mirrors db.ts `createTask`: an empty dep set makes the
    /// task immediately `ready`; any deps hold it `pending` until they all
    /// complete (see `promote_ready_tasks`). `deps` is serialized to a JSON
    /// string array byte-identical to the TS `JSON.stringify(deps)`.
    pub fn create_task(
        &self,
        id: &str,
        spec: &str,
        parent_id: Option<&str>,
        deps: &[&str],
        created_by: Option<&str>,
    ) -> Result<(), StoreError> {
        let deps_json = serde_json::to_string(deps).unwrap_or_else(|_| "[]".to_string());
        let status = if deps.is_empty() { "ready" } else { "pending" };
        self.db.connection().execute(
            "INSERT INTO tasks (id, parent_id, created_by_terminal_handle, spec, status, deps)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, parent_id, created_by, spec, status, deps_json],
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

    /// Update a task's status. Mirrors db.ts `updateTaskStatus`: terminal states
    /// (`completed`/`failed`) stamp `completed_at`; `result` is COALESCE'd so a
    /// `None` preserves any prior result. Completing a task runs the DAG
    /// promotion (`promote_ready_tasks`) and closes its active dispatch, both in
    /// this writer so there is no half-resolved window.
    pub fn update_task_status(
        &self,
        id: &str,
        status: &str,
        result: Option<&str>,
    ) -> Result<Option<Task>, StoreError> {
        let conn = self.db.connection();
        if status == "completed" || status == "failed" {
            conn.execute(
                "UPDATE tasks SET status = ?2, result = COALESCE(?3, result), completed_at = datetime('now') WHERE id = ?1",
                params![id, status, result],
            )?;
        } else {
            conn.execute(
                "UPDATE tasks SET status = ?2, result = COALESCE(?3, result) WHERE id = ?1",
                params![id, status, result],
            )?;
        }
        if status == "completed" {
            self.promote_ready_tasks(id)?;
            self.complete_active_dispatch_for_task(id)?;
        }
        self.get_task(id)
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

    fn task_status(&self, id: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .db
            .connection()
            .query_row("SELECT status FROM tasks WHERE id = ?1", params![id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?)
    }

    // Why: when a task completes, promote any `pending` task whose full dep set
    // is now satisfied to `ready`. Runs in the same writer as the status update
    // (db.ts `promoteReadyTasks`) so there is no window where a task is
    // completable but its children have not been promoted.
    fn promote_ready_tasks(&self, completed_task_id: &str) -> Result<(), StoreError> {
        let candidates: Vec<(String, String)> = {
            let conn = self.db.connection();
            let mut stmt = conn.prepare("SELECT id, deps FROM tasks WHERE status = 'pending'")?;
            let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (task_id, deps_json) in candidates {
            let deps: Vec<String> = serde_json::from_str(&deps_json).unwrap_or_default();
            if !deps.iter().any(|dep| dep == completed_task_id) {
                continue;
            }
            let mut all_completed = true;
            for dep_id in &deps {
                if self.task_status(dep_id)?.as_deref() != Some("completed") {
                    all_completed = false;
                    break;
                }
            }
            if all_completed {
                self.db
                    .connection()
                    .execute("UPDATE tasks SET status = 'ready' WHERE id = ?1", params![task_id])?;
            }
        }
        Ok(())
    }

    // db.ts `completeActiveDispatchForTask`: close the newest still-open dispatch
    // for a task (used when the task completes or is gated).
    fn complete_active_dispatch_for_task(&self, task_id: &str) -> Result<(), StoreError> {
        let active: Option<String> = self
            .db
            .connection()
            .query_row(
                "SELECT id FROM dispatch_contexts WHERE task_id = ?1 AND status IN ('pending','dispatched') ORDER BY rowid DESC LIMIT 1",
                params![task_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = active {
            self.complete_dispatch(&id)?;
        }
        Ok(())
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

    /// Record a dispatch failure. Mirrors db.ts `failDispatch`: bumps
    /// `failure_count`; the third failure trips the circuit breaker
    /// (`circuit_broken` + task `failed`), otherwise the dispatch is `failed`
    /// and the task returns to `ready` so the coordinator can re-dispatch it
    /// (its deps are already satisfied, so `ready` — not `pending`).
    pub fn fail_dispatch(&self, id: &str, error: &str) -> Result<Option<DispatchContext>, StoreError> {
        let conn = self.db.connection();
        let existing: Option<(String, i64)> = conn
            .query_row(
                "SELECT task_id, failure_count FROM dispatch_contexts WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((task_id, failure_count)) = existing else {
            return Ok(None);
        };
        let new_failure_count = failure_count + 1;
        let new_status = if new_failure_count >= 3 { "circuit_broken" } else { "failed" };
        conn.execute(
            "UPDATE dispatch_contexts SET status = ?2, failure_count = ?3, last_failure = ?4 WHERE id = ?1",
            params![id, new_status, new_failure_count, error],
        )?;
        let task_status = if new_status == "circuit_broken" { "failed" } else { "ready" };
        conn.execute("UPDATE tasks SET status = ?2 WHERE id = ?1", params![task_id, task_status])?;
        self.dispatch_context_by_id(id)
    }

    /// Stamp a liveness heartbeat, but only on a still-`dispatched` context.
    /// Mirrors db.ts `recordHeartbeat`: a straggler heartbeat from an already
    /// completed/failed/circuit_broken dispatch must not revive it, or the
    /// stale-dispatch detector would miss a hung retry. Returns rows updated.
    pub fn record_heartbeat(&self, id: &str, at: &str) -> Result<usize, StoreError> {
        Ok(self.db.connection().execute(
            "UPDATE dispatch_contexts SET last_heartbeat_at = ?2 WHERE id = ?1 AND status = 'dispatched'",
            params![id, at],
        )?)
    }

    /// Dispatched contexts past the heartbeat/dispatch-age threshold. Mirrors
    /// db.ts `getStaleDispatches`; the caller passes an ISO threshold so
    /// SQLite's lexicographic string compare orders correctly in time.
    pub fn get_stale_dispatches(&self, threshold_iso: &str) -> Result<Vec<DispatchContext>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, assignee_handle, status, failure_count FROM dispatch_contexts
             WHERE status = 'dispatched'
               AND dispatched_at IS NOT NULL
               AND dispatched_at < ?1
               AND (last_heartbeat_at IS NULL OR last_heartbeat_at < ?1)",
        )?;
        let mut rows = stmt.query([threshold_iso])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row_to_dispatch(row)?);
        }
        Ok(out)
    }

    // ---- decision gates ----

    /// Open a decision gate on a task. Mirrors db.ts `createGate`: closes the
    /// task's active dispatch and moves the task to `blocked` until the gate is
    /// resolved (both side-effects inside this writer).
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
        self.complete_active_dispatch_for_task(task_id)?;
        self.db
            .connection()
            .execute("UPDATE tasks SET status = 'blocked' WHERE id = ?1", params![task_id])?;
        self.gate_by_id(id)?
            .ok_or_else(|| StoreError::Message("gate vanished after insert".into()))
    }

    /// Resolve a gate and unblock its task. Mirrors db.ts `resolveGate`: a
    /// missing gate is a no-op (returns `None`); otherwise the gate is
    /// `resolved` and the task returns to `ready` so the coordinator re-engages
    /// the worker with the decision outcome.
    pub fn resolve_gate(&self, id: &str, resolution: &str) -> Result<Option<DecisionGate>, StoreError> {
        let Some(gate) = self.gate_by_id(id)? else {
            return Ok(None);
        };
        let conn = self.db.connection();
        conn.execute(
            "UPDATE decision_gates SET status = 'resolved', resolution = ?2, resolved_at = datetime('now') WHERE id = ?1",
            params![id, resolution],
        )?;
        conn.execute("UPDATE tasks SET status = 'ready' WHERE id = ?1", params![gate.task_id])?;
        self.gate_by_id(id)
    }

    /// Time a gate out (no resolution). Mirrors db.ts `timeoutGate`: marks the
    /// gate `timeout` and stamps `resolved_at`, leaving the task blocked.
    pub fn timeout_gate(&self, id: &str) -> Result<Option<DecisionGate>, StoreError> {
        self.db.connection().execute(
            "UPDATE decision_gates SET status = 'timeout', resolved_at = datetime('now') WHERE id = ?1",
            params![id],
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

    fn status_of(db: &OrchestrationDb, id: &str) -> String {
        db.get_task(id).unwrap().unwrap().status
    }

    #[test]
    fn create_task_deps_drive_initial_status() {
        let db = OrchestrationDb::open_in_memory().unwrap();
        db.create_task("t1", "build the parser", None, &[], Some("term-1")).unwrap();
        db.create_task("t2", "write tests", Some("t1"), &["t1"], None).unwrap();

        let all = db.list_tasks(None).unwrap();
        assert_eq!(all.len(), 2);
        // No deps → immediately ready; a dep holds the task pending.
        assert_eq!(all[0].id, "t1");
        assert_eq!(all[0].status, "ready");
        assert_eq!(all[1].parent_id.as_deref(), Some("t1"));
        assert_eq!(all[1].status, "pending");
        assert_eq!(all[1].deps, "[\"t1\"]");
    }

    #[test]
    fn completing_a_task_promotes_ready_dependents_and_stamps_result() {
        let db = OrchestrationDb::open_in_memory().unwrap();
        db.create_task("t1", "a", None, &[], None).unwrap();
        db.create_task("t2", "b", None, &["t1"], None).unwrap();
        db.create_task("t3", "c", None, &["t1", "t2"], None).unwrap();

        // Completing t1 promotes t2 (its only dep), but not t3 (t2 still open).
        db.update_task_status("t1", "completed", Some("done")).unwrap();
        assert_eq!(status_of(&db, "t2"), "ready");
        assert_eq!(status_of(&db, "t3"), "pending");
        let t1 = db.get_task("t1").unwrap().unwrap();
        assert_eq!(t1.result.as_deref(), Some("done"));

        // A later status update without a result preserves it (COALESCE) — keep
        // t1 completed so it still satisfies t3's dep below.
        db.update_task_status("t1", "completed", None).unwrap();
        assert_eq!(db.get_task("t1").unwrap().unwrap().result.as_deref(), Some("done"));

        // Completing t2 now satisfies all of t3's deps → t3 becomes ready.
        db.update_task_status("t2", "completed", None).unwrap();
        assert_eq!(status_of(&db, "t3"), "ready");
    }

    #[test]
    fn decision_gate_blocks_task_and_resolution_unblocks_it() {
        let db = OrchestrationDb::open_in_memory().unwrap();
        db.create_task("t1", "spec", None, &[], None).unwrap();
        let ctx = db.create_dispatch_context("t1", "worker-1", "ctx1").unwrap();
        assert_eq!(ctx.status, "dispatched");

        // Opening a gate closes the active dispatch and blocks the task.
        let gate = db.create_gate("g1", "t1", "Proceed?", &["yes", "no"]).unwrap();
        assert_eq!(gate.status, "pending");
        assert_eq!(gate.options, "[\"yes\",\"no\"]");
        assert_eq!(status_of(&db, "t1"), "blocked");
        assert_eq!(db.dispatch_context_by_id("ctx1").unwrap().unwrap().status, "completed");

        // Resolving the gate unblocks the task back to ready.
        let resolved = db.resolve_gate("g1", "yes").unwrap().unwrap();
        assert_eq!(resolved.status, "resolved");
        assert_eq!(resolved.resolution.as_deref(), Some("yes"));
        assert_eq!(status_of(&db, "t1"), "ready");
        assert!(db.list_gates("t1", Some("pending")).unwrap().is_empty());

        // Timing out a fresh gate marks it timeout and leaves the task as-is.
        db.create_gate("g2", "t1", "Again?", &["ok"]).unwrap();
        assert_eq!(status_of(&db, "t1"), "blocked");
        let timed = db.timeout_gate("g2").unwrap().unwrap();
        assert_eq!(timed.status, "timeout");
        assert_eq!(status_of(&db, "t1"), "blocked");
    }

    #[test]
    fn dispatch_requires_ready_task_and_one_active_per_assignee() {
        let db = OrchestrationDb::open_in_memory().unwrap();
        db.create_task("dep", "dep", None, &[], None).unwrap();
        db.create_task("t1", "spec1", None, &["dep"], None).unwrap(); // pending (dep open)
        db.create_task("t2", "spec2", None, &[], None).unwrap(); // ready

        // Pending task cannot be dispatched.
        assert!(db.create_dispatch_context("t1", "worker-1", "ctx0").is_err());
        // Unknown task.
        assert!(db.create_dispatch_context("nope", "worker-1", "ctxX").is_err());

        // Completing the dep promotes t1 to ready.
        db.update_task_status("dep", "completed", None).unwrap();
        assert_eq!(status_of(&db, "t1"), "ready");

        let ctx = db.create_dispatch_context("t1", "worker-1", "ctx1").unwrap();
        assert_eq!(ctx.status, "dispatched");
        assert_eq!(ctx.failure_count, 0);
        assert_eq!(status_of(&db, "t1"), "dispatched");

        // worker-1 already has an active dispatch → second (on ready t2) refused.
        let err = db.create_dispatch_context("t2", "worker-1", "ctx2").unwrap_err();
        assert!(err.to_string().contains("already has an active dispatch"), "{err}");

        // After completing ctx1, worker-1 is free again.
        assert_eq!(db.complete_dispatch("ctx1").unwrap(), 1);
        let ctx3 = db.create_dispatch_context("t2", "worker-1", "ctx3").unwrap();
        assert_eq!(ctx3.task_id, "t2");
    }

    #[test]
    fn fail_dispatch_carries_failures_and_trips_circuit_breaker_at_three() {
        let db = OrchestrationDb::open_in_memory().unwrap();
        db.create_task("t1", "spec", None, &[], None).unwrap();

        // Failure 1 and 2: dispatch fails, task returns to ready, count carries.
        for (attempt, ctx_id, expected_count) in [("ctx1", "ctx1", 1), ("ctx2", "ctx2", 2)] {
            let _ = attempt;
            let ctx = db.create_dispatch_context("t1", "worker-1", ctx_id).unwrap();
            assert_eq!(ctx.failure_count, expected_count - 1); // carried forward
            let failed = db.fail_dispatch(ctx_id, "boom").unwrap().unwrap();
            assert_eq!(failed.status, "failed");
            assert_eq!(failed.failure_count, expected_count);
            assert_eq!(status_of(&db, "t1"), "ready");
        }

        // Failure 3 trips the breaker: dispatch circuit_broken, task failed.
        let ctx3 = db.create_dispatch_context("t1", "worker-1", "ctx3").unwrap();
        assert_eq!(ctx3.failure_count, 2);
        let broken = db.fail_dispatch("ctx3", "boom").unwrap().unwrap();
        assert_eq!(broken.status, "circuit_broken");
        assert_eq!(broken.failure_count, 3);
        assert_eq!(status_of(&db, "t1"), "failed");

        // Failing an unknown context is a no-op (None).
        assert!(db.fail_dispatch("nope", "boom").unwrap().is_none());
    }

    #[test]
    fn heartbeat_only_touches_dispatched_and_stale_detector_respects_threshold() {
        let db = OrchestrationDb::open_in_memory().unwrap();
        db.create_task("t1", "spec", None, &[], None).unwrap();
        db.create_dispatch_context("t1", "worker-1", "ctx1").unwrap();

        // Fresh dispatch with no heartbeat is stale against a future threshold.
        let future = "2999-01-01 00:00:00";
        assert_eq!(db.get_stale_dispatches(future).unwrap().len(), 1);

        // A heartbeat newer than the threshold clears staleness.
        assert_eq!(db.record_heartbeat("ctx1", "2999-06-01 00:00:00").unwrap(), 1);
        assert!(db.get_stale_dispatches(future).unwrap().is_empty());

        // Nothing is stale against a past threshold (dispatched_at grace).
        assert!(db.get_stale_dispatches("2000-01-01 00:00:00").unwrap().is_empty());

        // Zombie-heartbeat guard: once completed, a heartbeat updates 0 rows.
        db.complete_dispatch("ctx1").unwrap();
        assert_eq!(db.record_heartbeat("ctx1", "2999-06-02 00:00:00").unwrap(), 0);
    }

    #[test]
    fn delivered_marker_is_distinct_from_read_replay_guard() {
        let db = OrchestrationDb::open_in_memory().unwrap();
        db.send_message(&msg("m1", "worker-a", "hello")).unwrap();

        // Undelivered + unread → eligible for auto-push.
        assert_eq!(db.undelivered_inbox("worker-a").unwrap().len(), 1);

        // Marking delivered removes it from the push queue but not the inbox.
        assert_eq!(db.mark_delivered("m1").unwrap(), 1);
        assert!(db.undelivered_inbox("worker-a").unwrap().is_empty());
        assert_eq!(db.inbox("worker-a").unwrap().len(), 1); // still unread
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

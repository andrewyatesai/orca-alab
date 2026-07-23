//! Orchestration coordination store, ported from
//! `src/main/runtime/orchestration/db.ts`. Schema creation and the
//! `user_version` migration ladder live in `orchestration_schema`; this module
//! is the full store API (messages, tasks, dispatch contexts, decision gates,
//! coordinator runs) the main-process `OrchestrationStore` napi class exposes.
//!
//! Fidelity contract with the deleted TS twin (see the swap audit): row structs
//! serialize to the exact TS Row JSON; JS-side nondeterminism (generated ids,
//! `new Date().toISOString()` completion stamps, display strings) is computed by
//! the caller and passed in, while all other timestamps use SQLite
//! `datetime('now')` — byte-identical to what the TS store wrote.

use crate::orchestration_schema;
use orca_store::{Database, OpenOptions, StoreError};
use rusqlite::{params, params_from_iter, OptionalExtension, Row as SqlRow, ToSql};
use serde::Serialize;

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
    pub sender_pane_key: Option<String>,
    // Why: recorded at send time so delivery can re-resolve the pane's current
    // handle after the addressed handle goes stale (#9163).
    pub recipient_pane_key: Option<String>,
}

// Row structs are FULL rows (every column) with field names + `Serialize` output
// byte-matching the TS `MessageRow`/`TaskRow`/… in orchestration/types.ts, so the
// napi shim marshals them straight to the shapes production consumers read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Message {
    pub id: String,
    pub from_handle: String,
    pub to_handle: String,
    pub subject: String,
    pub body: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub priority: String,
    pub thread_id: Option<String>,
    pub payload: Option<String>,
    pub read: i64,
    pub sequence: i64,
    pub created_at: String,
    pub delivered_at: Option<String>,
    pub sender_pane_key: Option<String>,
    pub recipient_pane_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Task {
    pub id: String,
    pub parent_id: Option<String>,
    pub created_by_terminal_handle: Option<String>,
    pub task_title: Option<String>,
    pub display_name: Option<String>,
    pub spec: String,
    pub status: String,
    pub deps: String,
    pub result: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

/// A task row plus its active dispatch (LEFT JOIN), for `list_tasks_with_dispatch`.
/// `#[serde(flatten)]` inlines the task columns, then adds the two join columns —
/// matching the TS `TaskRow & { assignee_handle; dispatch_id }`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TaskWithDispatch {
    #[serde(flatten)]
    pub task: Task,
    pub assignee_handle: Option<String>,
    pub dispatch_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DispatchContext {
    pub id: String,
    pub task_id: String,
    pub assignee_handle: Option<String>,
    pub assignee_pane_key: Option<String>,
    pub status: String,
    pub failure_count: i64,
    pub last_failure: Option<String>,
    pub dispatched_at: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub last_heartbeat_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DecisionGate {
    pub id: String,
    pub task_id: String,
    pub question: String,
    pub options: String,
    pub status: String,
    pub resolution: Option<String>,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CoordinatorRun {
    pub id: String,
    pub spec: String,
    pub status: String,
    pub coordinator_handle: String,
    pub poll_interval_ms: i64,
    pub created_at: String,
    pub completed_at: Option<String>,
}

// Column lists keep every SELECT and its row_to_* reader in lock-step order.
const MESSAGE_COLUMNS: &str =
    "id, from_handle, to_handle, subject, body, type, priority, thread_id, payload, read, sequence, created_at, delivered_at, sender_pane_key, recipient_pane_key";
const TASK_COLUMNS: &str =
    "id, parent_id, created_by_terminal_handle, task_title, display_name, spec, status, deps, result, created_at, completed_at";
const DISPATCH_COLUMNS: &str =
    "id, task_id, assignee_handle, assignee_pane_key, status, failure_count, last_failure, dispatched_at, completed_at, created_at, last_heartbeat_at";
const GATE_COLUMNS: &str =
    "id, task_id, question, options, status, resolution, created_at, resolved_at";
const RUN_COLUMNS: &str =
    "id, spec, status, coordinator_handle, poll_interval_ms, created_at, completed_at";

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

    // ---- messages ----

    /// Insert a message, returning the persisted row (TS `insertMessage`). The
    /// caller supplies `message.id` (the TS shim's `generateId('msg')`), keeping
    /// the `msg_<hex>` id shape stable.
    pub fn send_message(&self, message: &NewMessage) -> Result<Message, StoreError> {
        self.db.connection().execute(
            "INSERT INTO messages (id, from_handle, to_handle, subject, body, type, priority, thread_id, payload, sender_pane_key, recipient_pane_key)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                message.id, message.from_handle, message.to_handle, message.subject, message.body,
                message.message_type, message.priority, message.thread_id, message.payload,
                message.sender_pane_key, message.recipient_pane_key,
            ],
        )?;
        self.get_message_by_id(&message.id)?
            .ok_or_else(|| StoreError::Message("message vanished after insert".into()))
    }

    pub fn get_message_by_id(&self, id: &str) -> Result<Option<Message>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(&format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE id = ?1"))?;
        Ok(stmt.query_row([id], row_to_message).optional()?)
    }

    /// Unread messages for `handle`, oldest first (TS `getUnreadMessages`);
    /// `types` optionally restricts by message type.
    pub fn get_unread_messages(&self, handle: &str, types: Option<&[String]>) -> Result<Vec<Message>, StoreError> {
        self.query_messages("read = 0", "ORDER BY sequence", handle, types, None)
    }

    /// Unread AND undelivered messages for `handle`, oldest first — the
    /// push-on-idle replay guard (TS `getUndeliveredUnreadMessages`).
    pub fn get_undelivered_unread_messages(
        &self,
        handle: &str,
        types: Option<&[String]>,
    ) -> Result<Vec<Message>, StoreError> {
        self.query_messages("read = 0 AND delivered_at IS NULL", "ORDER BY sequence", handle, types, None)
    }

    /// Most-recent messages for `handle` (TS `getAllMessages`), newest first.
    pub fn get_all_messages(&self, handle: &str, limit: i64) -> Result<Vec<Message>, StoreError> {
        self.query_messages("1 = 1", "ORDER BY sequence DESC", handle, None, Some(limit))
    }

    /// Every message for `handle`, newest first, never touching the read bit
    /// (TS `getAllMessagesForHandle`); optional type filter.
    pub fn get_all_messages_for_handle(
        &self,
        handle: &str,
        limit: i64,
        types: Option<&[String]>,
    ) -> Result<Vec<Message>, StoreError> {
        self.query_messages("1 = 1", "ORDER BY sequence DESC", handle, types, Some(limit))
    }

    /// All messages regardless of recipient, newest first (TS `getInbox`).
    pub fn get_inbox(&self, limit: i64) -> Result<Vec<Message>, StoreError> {
        let conn = self.db.connection();
        let mut stmt =
            conn.prepare(&format!("SELECT {MESSAGE_COLUMNS} FROM messages ORDER BY sequence DESC LIMIT ?1"))?;
        let rows = stmt.query_map([limit], row_to_message)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Thread-scoped replies addressed to `to_handle`, oldest first (TS
    /// `getThreadMessagesFor`); `after_sequence` resumes past an already-seen marker.
    pub fn get_thread_messages_for(
        &self,
        thread_id: &str,
        to_handle: &str,
        after_sequence: Option<i64>,
    ) -> Result<Vec<Message>, StoreError> {
        let conn = self.db.connection();
        match after_sequence {
            Some(seq) => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {MESSAGE_COLUMNS} FROM messages WHERE thread_id = ?1 AND to_handle = ?2 AND sequence > ?3 ORDER BY sequence ASC"
                ))?;
                let rows = stmt.query_map(params![thread_id, to_handle, seq], row_to_message)?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            }
            None => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {MESSAGE_COLUMNS} FROM messages WHERE thread_id = ?1 AND to_handle = ?2 ORDER BY sequence ASC"
                ))?;
                let rows = stmt.query_map(params![thread_id, to_handle], row_to_message)?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            }
        }
    }

    /// Mark messages read by id (TS `markAsRead`). Empty `ids` is a no-op.
    pub fn mark_as_read(&self, ids: &[&str]) -> Result<(), StoreError> {
        self.update_messages_by_ids("read = 1", ids)
    }

    /// Stamp `delivered_at = datetime('now')` on messages by id (TS
    /// `markAsDelivered`) — the push-on-idle delivery marker.
    pub fn mark_as_delivered(&self, ids: &[&str]) -> Result<(), StoreError> {
        self.update_messages_by_ids("delivered_at = datetime('now')", ids)
    }

    /// Mark messages both read and delivered (TS `markAsReadAndDelivered`) —
    /// superseded lifecycle messages stay queryable but must not be re-consumed
    /// or re-injected. `delivered_at` is only stamped if not already set.
    pub fn mark_as_read_and_delivered(&self, ids: &[&str]) -> Result<(), StoreError> {
        self.update_messages_by_ids(
            "read = 1, delivered_at = COALESCE(delivered_at, datetime('now'))",
            ids,
        )
    }

    /// Rewrite a `worker_done`/`heartbeat` message into an audited rejection (TS
    /// `convertLifecycleMessageToRejection`): keeps the row queryable but stops it
    /// reaching later read paths as an actionable completion/liveness event. A
    /// non-lifecycle or missing message is returned unchanged.
    pub fn convert_lifecycle_message_to_rejection(
        &self,
        message_id: &str,
        reason: &str,
    ) -> Result<Option<Message>, StoreError> {
        let Some(message) = self.get_message_by_id(message_id)? else {
            return Ok(None);
        };
        if message.message_type != "worker_done" && message.message_type != "heartbeat" {
            return Ok(Some(message));
        }
        let original_body = if message.body.is_empty() {
            String::new()
        } else {
            format!("\n\nOriginal body:\n{}", message.body)
        };
        let body = format!(
            "Orca rejected this {}: {reason}{original_body}",
            message.message_type
        );
        let payload = add_lifecycle_rejection_marker(message.payload.as_deref(), reason);
        let subject = format!("Rejected {}: {}", message.message_type, message.subject);
        self.db.connection().execute(
            "UPDATE messages SET priority = 'high', subject = ?1, body = ?2, payload = ?3 WHERE id = ?4",
            params![subject, body, payload, message_id],
        )?;
        self.get_message_by_id(message_id)
    }

    fn update_messages_by_ids(&self, set_clause: &str, ids: &[&str]) -> Result<(), StoreError> {
        if ids.is_empty() {
            return Ok(());
        }
        let placeholders = placeholders(ids.len());
        let sql = format!("UPDATE messages SET {set_clause} WHERE id IN ({placeholders})");
        let params: Vec<&dyn ToSql> = ids.iter().map(|id| id as &dyn ToSql).collect();
        self.db.connection().execute(&sql, params_from_iter(params))?;
        Ok(())
    }

    fn query_messages(
        &self,
        base_where: &str,
        order: &str,
        handle: &str,
        types: Option<&[String]>,
        limit: Option<i64>,
    ) -> Result<Vec<Message>, StoreError> {
        let conn = self.db.connection();
        let mut sql = format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE to_handle = ? AND {base_where}");
        let mut binds: Vec<&dyn ToSql> = vec![&handle];
        let types = types.filter(|t| !t.is_empty());
        if let Some(types) = types {
            sql.push_str(&format!(" AND type IN ({})", placeholders(types.len())));
            for t in types {
                binds.push(t as &dyn ToSql);
            }
        }
        sql.push(' ');
        sql.push_str(order);
        if let Some(limit) = &limit {
            sql.push_str(" LIMIT ?");
            binds.push(limit as &dyn ToSql);
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(binds), row_to_message)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- tasks ----

    /// Insert a task (TS `createTask`): empty deps → `ready`, else `pending`.
    /// `deps` is serialized to a JSON string array byte-identical to
    /// `JSON.stringify(deps)`; `task_title`/`display_name` are the shim's
    /// pre-resolved display strings (empty → NULL, done caller-side).
    #[allow(clippy::too_many_arguments)]
    pub fn create_task(
        &self,
        id: &str,
        spec: &str,
        parent_id: Option<&str>,
        deps: &[&str],
        created_by: Option<&str>,
        task_title: Option<&str>,
        display_name: Option<&str>,
    ) -> Result<Task, StoreError> {
        let deps_json = serde_json::to_string(deps).unwrap_or_else(|_| "[]".to_string());
        let status = if deps.is_empty() { "ready" } else { "pending" };
        self.db.connection().execute(
            "INSERT INTO tasks (id, parent_id, created_by_terminal_handle, task_title, display_name, spec, status, deps)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, parent_id, created_by, task_title, display_name, spec, status, deps_json],
        )?;
        self.get_task(id)?
            .ok_or_else(|| StoreError::Message("task vanished after insert".into()))
    }

    pub fn get_task(&self, id: &str) -> Result<Option<Task>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(&format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"))?;
        Ok(stmt.query_row([id], row_to_task).optional()?)
    }

    /// Tasks, optionally filtered by status, oldest first (TS `listTasks`; the
    /// shim maps its `ready` filter to `status = 'ready'`).
    pub fn list_tasks(&self, status: Option<&str>) -> Result<Vec<Task>, StoreError> {
        let conn = self.db.connection();
        match status {
            Some(status) => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {TASK_COLUMNS} FROM tasks WHERE status = ?1 ORDER BY created_at"
                ))?;
                let rows = stmt.query_map([status], row_to_task)?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            }
            None => {
                let mut stmt = conn.prepare(&format!("SELECT {TASK_COLUMNS} FROM tasks ORDER BY created_at"))?;
                let rows = stmt.query_map([], row_to_task)?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            }
        }
    }

    /// Tasks with their active dispatch's assignee + id (TS `listTasksWithDispatch`).
    /// The inner subquery picks the newest active dispatch per task; non-dispatched
    /// tasks keep NULL join columns.
    pub fn list_tasks_with_dispatch(&self, status: Option<&str>) -> Result<Vec<TaskWithDispatch>, StoreError> {
        let conn = self.db.connection();
        let where_clause = if status.is_some() { "WHERE t.status = ?1" } else { "" };
        let sql = format!(
            "SELECT {}, d.assignee_handle AS j_assignee, d.id AS j_dispatch
             FROM tasks t
             LEFT JOIN (
               SELECT dc.* FROM dispatch_contexts dc
               INNER JOIN (
                 SELECT task_id, MAX(rowid) AS max_rowid FROM dispatch_contexts
                 WHERE status IN ('pending', 'dispatched') GROUP BY task_id
               ) latest ON latest.task_id = dc.task_id AND latest.max_rowid = dc.rowid
             ) d ON d.task_id = t.id
             {where_clause}
             ORDER BY t.created_at",
            TASK_COLUMNS.split(", ").map(|c| format!("t.{c}")).collect::<Vec<_>>().join(", ")
        );
        let mut stmt = conn.prepare(&sql)?;
        let map = |row: &SqlRow<'_>| {
            Ok(TaskWithDispatch {
                task: row_to_task(row)?,
                assignee_handle: row.get("j_assignee")?,
                dispatch_id: row.get("j_dispatch")?,
            })
        };
        let rows = if let Some(status) = status {
            stmt.query_map([status], map)?.collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map([], map)?.collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(rows)
    }

    /// Update a task's status (TS `updateTaskStatus`). `result` and `completed_at`
    /// are COALESCE'd (a `None` preserves the prior value); the caller passes the
    /// `new Date().toISOString()` stamp for terminal states. Completing a task runs
    /// DAG promotion + closes its active dispatch in this writer.
    pub fn update_task_status(
        &self,
        id: &str,
        status: &str,
        result: Option<&str>,
        completed_at: Option<&str>,
    ) -> Result<Option<Task>, StoreError> {
        self.db.connection().execute(
            "UPDATE tasks SET status = ?2, result = COALESCE(?3, result), completed_at = COALESCE(?4, completed_at) WHERE id = ?1",
            params![id, status, result, completed_at],
        )?;
        if status == "completed" {
            self.promote_ready_tasks(id)?;
            self.complete_active_dispatch_for_task(id)?;
        }
        self.get_task(id)
    }

    fn task_status(&self, id: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .db
            .connection()
            .query_row("SELECT status FROM tasks WHERE id = ?1", params![id], |r| r.get::<_, String>(0))
            .optional()?)
    }

    // Why: when a task completes, promote any `pending` task whose full dep set
    // is now satisfied to `ready`, in the same writer as the status update (TS
    // `promoteReadyTasks`) so there is no half-resolved window.
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

    // ---- dispatch contexts ----

    /// Dispatch `task_id` (which must be `ready`) to `assignee_handle` (TS
    /// `createDispatchContext`). Refuses if the assignee already has an active
    /// dispatch; carries the failure count forward; marks the task `dispatched`.
    pub fn create_dispatch_context(
        &self,
        task_id: &str,
        assignee_handle: &str,
        id: &str,
        assignee_pane_key: Option<&str>,
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
        // Handle match covers legacy rows without pane keys.
        let mut conflict: Option<(String, String)> = conn
            .query_row(
                "SELECT id, task_id FROM dispatch_contexts WHERE assignee_handle = ?1 AND status IN ('pending','dispatched') LIMIT 1",
                [assignee_handle],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        // Pane-identity match: when both the new assignee and an active row carry
        // usable pane keys, also lock on equivalent pane identity (remint-stable
        // leaf) so a reminted handle can't open a second dispatch on the same pane.
        if conflict.is_none() {
            if let Some(pane_key) = assignee_pane_key {
                let mut stmt = conn.prepare(
                    "SELECT id, task_id, assignee_pane_key FROM dispatch_contexts WHERE assignee_pane_key IS NOT NULL AND status IN ('pending','dispatched')",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?))
                })?;
                for row in rows {
                    let (id, tid, existing_key) = row?;
                    if existing_key.as_deref().is_some_and(|k| is_equivalent_pane_key(k, pane_key)) {
                        conflict = Some((id, tid));
                        break;
                    }
                }
            }
        }
        if let Some((existing_id, existing_task)) = conflict {
            return Err(StoreError::Message(format!(
                "Terminal {assignee_handle} already has an active dispatch ({existing_id} for task {existing_task})"
            )));
        }
        let prior_failures: i64 = conn.query_row(
            "SELECT COALESCE(MAX(failure_count), 0) FROM dispatch_contexts WHERE task_id = ?1",
            [task_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO dispatch_contexts (id, task_id, assignee_handle, assignee_pane_key, status, failure_count, dispatched_at)
             VALUES (?1, ?2, ?3, ?4, 'dispatched', ?5, datetime('now'))",
            params![id, task_id, assignee_handle, assignee_pane_key, prior_failures],
        )?;
        conn.execute("UPDATE tasks SET status = 'dispatched' WHERE id = ?1", params![task_id])?;
        self.dispatch_context_by_id(id)?
            .ok_or_else(|| StoreError::Message("dispatch context vanished after insert".into()))
    }

    pub fn dispatch_context_by_id(&self, id: &str) -> Result<Option<DispatchContext>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(&format!("SELECT {DISPATCH_COLUMNS} FROM dispatch_contexts WHERE id = ?1"))?;
        Ok(stmt.query_row([id], row_to_dispatch).optional()?)
    }

    /// The newest dispatch for a task (TS `getDispatchContext`, rowid DESC).
    pub fn get_dispatch_context(&self, task_id: &str) -> Result<Option<DispatchContext>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(&format!(
            "SELECT {DISPATCH_COLUMNS} FROM dispatch_contexts WHERE task_id = ?1 ORDER BY rowid DESC LIMIT 1"
        ))?;
        Ok(stmt.query_row([task_id], row_to_dispatch).optional()?)
    }

    /// The active (pending/dispatched) dispatch for a terminal (TS `getActiveDispatchForTerminal`).
    pub fn get_active_dispatch_for_terminal(&self, handle: &str) -> Result<Option<DispatchContext>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(&format!(
            "SELECT {DISPATCH_COLUMNS} FROM dispatch_contexts WHERE assignee_handle = ?1 AND status IN ('pending','dispatched') LIMIT 1"
        ))?;
        Ok(stmt.query_row([handle], row_to_dispatch).optional()?)
    }

    /// The newest dispatch for a terminal regardless of status (TS `getLatestDispatchForTerminal`).
    pub fn get_latest_dispatch_for_terminal(&self, handle: &str) -> Result<Option<DispatchContext>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(&format!(
            "SELECT {DISPATCH_COLUMNS} FROM dispatch_contexts WHERE assignee_handle = ?1 ORDER BY rowid DESC LIMIT 1"
        ))?;
        Ok(stmt.query_row([handle], row_to_dispatch).optional()?)
    }

    pub fn complete_dispatch(&self, id: &str) -> Result<usize, StoreError> {
        Ok(self.db.connection().execute(
            "UPDATE dispatch_contexts SET status = 'completed', completed_at = datetime('now') WHERE id = ?1",
            params![id],
        )?)
    }

    // db.ts `completeActiveDispatchForTask`: close the newest still-open dispatch
    // for a task (used when the task completes or is gated).
    pub fn complete_active_dispatch_for_task(&self, task_id: &str) -> Result<(), StoreError> {
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

    /// Fail the newest active dispatch for a task, if any (TS `failActiveDispatchForTask`).
    pub fn fail_active_dispatch_for_task(&self, task_id: &str, error: &str) -> Result<Option<DispatchContext>, StoreError> {
        let active: Option<String> = self
            .db
            .connection()
            .query_row(
                "SELECT id FROM dispatch_contexts WHERE task_id = ?1 AND status IN ('pending','dispatched') ORDER BY rowid DESC LIMIT 1",
                params![task_id],
                |r| r.get(0),
            )
            .optional()?;
        match active {
            Some(id) => self.fail_dispatch(&id, error),
            None => Ok(None),
        }
    }

    /// Record a dispatch failure (TS `failDispatch`): bumps `failure_count`; the
    /// third failure trips the circuit breaker (`circuit_broken` + task `failed`),
    /// otherwise the dispatch is `failed` and the task returns to `ready`.
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

    /// Stamp a liveness heartbeat, but only on a still-`dispatched` context (TS
    /// `recordHeartbeat`): a straggler heartbeat from a completed dispatch must
    /// not revive it. `at` is stored verbatim. Returns rows updated.
    pub fn record_heartbeat(&self, id: &str, at: &str) -> Result<usize, StoreError> {
        Ok(self.db.connection().execute(
            "UPDATE dispatch_contexts SET last_heartbeat_at = ?2 WHERE id = ?1 AND status = 'dispatched'",
            params![id, at],
        )?)
    }

    /// Dispatched contexts past the heartbeat/dispatch-age threshold (TS
    /// `getStaleDispatches`). The stored columns are written by `datetime('now')`
    /// (space-separated, `'2026-07-13 16:59:00'`) while the caller passes an ISO
    /// `T` threshold (`'…T16:50:00.000Z'`). A raw string `<` is byte-lexicographic
    /// and space (0x20) < `T` (0x54), so every same-day timestamp would sort
    /// before any threshold regardless of real time — flagging healthy workers as
    /// stale. Canonicalize BOTH operands with `datetime()` so the compare is by
    /// actual time across either format.
    pub fn get_stale_dispatches(&self, threshold_iso: &str) -> Result<Vec<DispatchContext>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(&format!(
            "SELECT {DISPATCH_COLUMNS} FROM dispatch_contexts
             WHERE status = 'dispatched'
               AND dispatched_at IS NOT NULL
               AND datetime(dispatched_at) < datetime(?1)
               AND (last_heartbeat_at IS NULL OR datetime(last_heartbeat_at) < datetime(?1))"
        ))?;
        let rows = stmt.query_map([threshold_iso], row_to_dispatch)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Set a dispatch's `dispatched_at` / `last_heartbeat_at` directly (COALESCE:
    /// a `None` leaves the column unchanged). A low-level seam used by tests to
    /// backdate timestamps deterministically; not on the production path.
    pub fn set_dispatch_timestamps(
        &self,
        id: &str,
        dispatched_at: Option<&str>,
        last_heartbeat_at: Option<&str>,
    ) -> Result<usize, StoreError> {
        Ok(self.db.connection().execute(
            "UPDATE dispatch_contexts SET dispatched_at = COALESCE(?2, dispatched_at), last_heartbeat_at = COALESCE(?3, last_heartbeat_at) WHERE id = ?1",
            params![id, dispatched_at, last_heartbeat_at],
        )?)
    }

    // ---- decision gates ----

    /// Open a decision gate on a task (TS `createGate`): closes the task's active
    /// dispatch and moves the task to `blocked`.
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

    /// Resolve a gate and unblock its task (TS `resolveGate`): a missing gate is a
    /// no-op (`None`); otherwise the gate is `resolved` and the task returns to `ready`.
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

    /// Time a gate out (TS `timeoutGate`): marks it `timeout` + stamps `resolved_at`,
    /// leaving the task blocked.
    pub fn timeout_gate(&self, id: &str) -> Result<Option<DecisionGate>, StoreError> {
        self.db.connection().execute(
            "UPDATE decision_gates SET status = 'timeout', resolved_at = datetime('now') WHERE id = ?1",
            params![id],
        )?;
        self.gate_by_id(id)
    }

    pub fn gate_by_id(&self, id: &str) -> Result<Option<DecisionGate>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(&format!("SELECT {GATE_COLUMNS} FROM decision_gates WHERE id = ?1"))?;
        Ok(stmt.query_row([id], row_to_gate).optional()?)
    }

    /// Gates filtered by task and/or status, oldest first (TS `listGates`).
    pub fn list_gates(&self, task_id: Option<&str>, status: Option<&str>) -> Result<Vec<DecisionGate>, StoreError> {
        let conn = self.db.connection();
        let mut sql = format!("SELECT {GATE_COLUMNS} FROM decision_gates");
        let mut binds: Vec<&dyn ToSql> = Vec::new();
        let mut clauses: Vec<&str> = Vec::new();
        if let Some(task_id) = &task_id {
            clauses.push("task_id = ?");
            binds.push(task_id as &dyn ToSql);
        }
        if let Some(status) = &status {
            clauses.push("status = ?");
            binds.push(status as &dyn ToSql);
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY created_at");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(binds), row_to_gate)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
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
        let mut stmt = conn.prepare(&format!("SELECT {RUN_COLUMNS} FROM coordinator_runs WHERE id = ?1"))?;
        Ok(stmt.query_row([id], row_to_coordinator).optional()?)
    }

    /// Update status (TS `updateCoordinatorRun`); the caller passes the
    /// `new Date().toISOString()` stamp for terminal states, COALESCE'd so a
    /// non-terminal transition preserves any prior `completed_at`.
    pub fn update_coordinator_run(
        &self,
        id: &str,
        status: &str,
        completed_at: Option<&str>,
    ) -> Result<Option<CoordinatorRun>, StoreError> {
        self.db.connection().execute(
            "UPDATE coordinator_runs SET status = ?2, completed_at = COALESCE(?3, completed_at) WHERE id = ?1",
            params![id, status, completed_at],
        )?;
        self.coordinator_run_by_id(id)
    }

    /// The most recent still-running coordinator, if any (TS `getActiveCoordinatorRun`).
    pub fn active_coordinator_run(&self) -> Result<Option<CoordinatorRun>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM coordinator_runs WHERE status = 'running' ORDER BY created_at DESC LIMIT 1"
        ))?;
        Ok(stmt.query_row([], row_to_coordinator).optional()?)
    }

    /// Every still-running coordinator, newest first (TS `getActiveCoordinatorRuns`).
    /// Multiple orchestrators may run concurrently in one workspace (issue #4389),
    /// so lifecycle gating must see all of them, not just the latest.
    pub fn active_coordinator_runs(&self) -> Result<Vec<CoordinatorRun>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM coordinator_runs WHERE status = 'running' ORDER BY created_at DESC, rowid DESC"
        ))?;
        let rows = stmt.query_map([], row_to_coordinator)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- queries + lifecycle ----

    /// Terminal handles seen in message history that have no active dispatch (TS
    /// `getIdleTerminals`), excluding `exclude_handles`.
    pub fn get_idle_terminals(&self, exclude_handles: &[&str]) -> Result<Vec<String>, StoreError> {
        let conn = self.db.connection();
        let mut busy: std::collections::HashSet<String> = {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT assignee_handle FROM dispatch_contexts WHERE status IN ('pending','dispatched') AND assignee_handle IS NOT NULL",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            let collected: rusqlite::Result<std::collections::HashSet<String>> = rows.collect();
            collected?
        };
        for h in exclude_handles {
            busy.insert((*h).to_string());
        }
        let mut stmt = conn
            .prepare("SELECT DISTINCT to_handle FROM messages UNION SELECT DISTINCT from_handle FROM messages")?;
        let all: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<_>>()?;
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut out = Vec::new();
        for handle in all {
            if !busy.contains(&handle) && seen.insert(handle.clone()) {
                out.push(handle);
            }
        }
        Ok(out)
    }

    pub fn reset_all(&self) -> Result<(), StoreError> {
        self.db.exec(
            "DELETE FROM coordinator_runs; DELETE FROM decision_gates; DELETE FROM dispatch_contexts; DELETE FROM tasks; DELETE FROM messages;",
        )
    }

    pub fn reset_tasks(&self) -> Result<(), StoreError> {
        self.db.exec(
            "DELETE FROM coordinator_runs; DELETE FROM decision_gates; DELETE FROM dispatch_contexts; DELETE FROM tasks;",
        )
    }

    pub fn reset_messages(&self) -> Result<(), StoreError> {
        self.db.exec("DELETE FROM messages")
    }

    // ---- introspection (tests + parity state dump) ----

    fn all<T>(&self, sql: &str, f: fn(&SqlRow<'_>) -> rusqlite::Result<T>) -> Result<Vec<T>, StoreError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], f)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Every row of every table (raw full rows, insertion order) as JSON — the
    /// state-dump seam the parity harness canonicalizes. Not a production path.
    pub fn dump_all_rows(&self) -> Result<serde_json::Value, StoreError> {
        let messages = self.all(&format!("SELECT {MESSAGE_COLUMNS} FROM messages ORDER BY rowid"), row_to_message)?;
        let tasks = self.all(&format!("SELECT {TASK_COLUMNS} FROM tasks ORDER BY rowid"), row_to_task)?;
        let dispatch_contexts =
            self.all(&format!("SELECT {DISPATCH_COLUMNS} FROM dispatch_contexts ORDER BY rowid"), row_to_dispatch)?;
        let decision_gates =
            self.all(&format!("SELECT {GATE_COLUMNS} FROM decision_gates ORDER BY rowid"), row_to_gate)?;
        let coordinator_runs =
            self.all(&format!("SELECT {RUN_COLUMNS} FROM coordinator_runs ORDER BY rowid"), row_to_coordinator)?;
        Ok(serde_json::json!({
            "messages": messages,
            "tasks": tasks,
            "dispatch_contexts": dispatch_contexts,
            "decision_gates": decision_gates,
            "coordinator_runs": coordinator_runs,
        }))
    }
}

fn placeholders(n: usize) -> String {
    std::iter::repeat_n("?", n).collect::<Vec<_>>().join(",")
}

/// Port of `addLifecycleRejectionMarker`: merge the audit marker into the
/// message payload object (or a fresh object when the payload is absent or not a
/// JSON object), mirroring `JSON.stringify({ ...parsed, _orcaLifecycleRejection })`.
fn add_lifecycle_rejection_marker(payload: Option<&str>, reason: &str) -> String {
    let mut obj = match payload.and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok()) {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    obj.insert(
        "_orcaLifecycleRejection".to_string(),
        serde_json::json!({ "code": "sender_not_assignee", "reason": reason }),
    );
    serde_json::Value::Object(obj).to_string()
}

/// Port of `parsePaneKey().leafId`: a pane key is `<tabId>:<leafId>` with a
/// single `:` and a stable-pane-id (v1-5 UUID) leaf; returns the leaf or None.
fn pane_key_leaf(key: &str) -> Option<&str> {
    let idx = key.find(':')?;
    if idx == 0 || key.rfind(':') != Some(idx) || idx + 1 >= key.len() {
        return None;
    }
    let leaf = &key[idx + 1..];
    is_stable_pane_id(leaf).then_some(leaf)
}

/// Port of `isStablePaneId` UUID_RE:
/// `[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}`.
fn is_stable_pane_id(v: &str) -> bool {
    let b = v.as_bytes();
    if b.len() != 36 {
        return false;
    }
    b.iter().enumerate().all(|(i, &c)| match i {
        8 | 13 | 18 | 23 => c == b'-',
        14 => c.is_ascii_digit() && (b'1'..=b'5').contains(&c),
        19 => matches!(c, b'8' | b'9' | b'a' | b'b'),
        _ => matches!(c, b'0'..=b'9' | b'a'..=b'f'),
    })
}

/// Port of `isEquivalentPaneKey`: identical keys, or the same stable leaf.
fn is_equivalent_pane_key(a: &str, b: &str) -> bool {
    a == b || matches!((pane_key_leaf(a), pane_key_leaf(b)), (Some(la), Some(lb)) if la == lb)
}

fn row_to_message(row: &SqlRow<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        from_handle: row.get(1)?,
        to_handle: row.get(2)?,
        subject: row.get(3)?,
        body: row.get(4)?,
        message_type: row.get(5)?,
        priority: row.get(6)?,
        thread_id: row.get(7)?,
        payload: row.get(8)?,
        read: row.get(9)?,
        sequence: row.get(10)?,
        created_at: row.get(11)?,
        delivered_at: row.get(12)?,
        sender_pane_key: row.get(13)?,
        recipient_pane_key: row.get(14)?,
    })
}

fn row_to_task(row: &SqlRow<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        parent_id: row.get(1)?,
        created_by_terminal_handle: row.get(2)?,
        task_title: row.get(3)?,
        display_name: row.get(4)?,
        spec: row.get(5)?,
        status: row.get(6)?,
        deps: row.get(7)?,
        result: row.get(8)?,
        created_at: row.get(9)?,
        completed_at: row.get(10)?,
    })
}

fn row_to_dispatch(row: &SqlRow<'_>) -> rusqlite::Result<DispatchContext> {
    Ok(DispatchContext {
        id: row.get(0)?,
        task_id: row.get(1)?,
        assignee_handle: row.get(2)?,
        assignee_pane_key: row.get(3)?,
        status: row.get(4)?,
        failure_count: row.get(5)?,
        last_failure: row.get(6)?,
        dispatched_at: row.get(7)?,
        completed_at: row.get(8)?,
        created_at: row.get(9)?,
        last_heartbeat_at: row.get(10)?,
    })
}

fn row_to_gate(row: &SqlRow<'_>) -> rusqlite::Result<DecisionGate> {
    Ok(DecisionGate {
        id: row.get(0)?,
        task_id: row.get(1)?,
        question: row.get(2)?,
        options: row.get(3)?,
        status: row.get(4)?,
        resolution: row.get(5)?,
        created_at: row.get(6)?,
        resolved_at: row.get(7)?,
    })
}

fn row_to_coordinator(row: &SqlRow<'_>) -> rusqlite::Result<CoordinatorRun> {
    Ok(CoordinatorRun {
        id: row.get(0)?,
        spec: row.get(1)?,
        status: row.get(2)?,
        coordinator_handle: row.get(3)?,
        poll_interval_ms: row.get(4)?,
        created_at: row.get(5)?,
        completed_at: row.get(6)?,
    })
}

/// Encode `["a","b"]` for the gate `options` column (byte-identical to the TS
/// `JSON.stringify(options)`).
fn json_string_array(items: &[&str]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(test)]
mod tests;

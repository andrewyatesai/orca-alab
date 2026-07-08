//! Parity module for the orca-runtime orchestration store — the state-replay
//! half of the store cutover. Mirrors `tools/parity/dispatch/orchestration-store.ts`:
//! replays an operation sequence on a fresh in-memory `OrchestrationDb`, then
//! dumps the canonical final state + read-op results.
//!
//! Determinism (same contract as the TS adapter): generated ids are remapped to
//! creation-ordinal placeholders (task_0, ctx_1…) and timestamps normalized to
//! null / "SET", so the DAG/circuit-breaker/gate invariants are compared without
//! the random-id + wall-clock noise. Ops reference prior-created entities by
//! "@<opIndex>"; on this side ids are generated deterministically per op index
//! (the TS side uses the store's random ids), and both agree after canonicalization
//! because inserts happen in the same order → same rowids → same placeholders.

use orca_runtime::orchestration::{
    CoordinatorRun, DecisionGate, DispatchContext, Message, NewMessage, OrchestrationDb, Task,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::HashMap;

/// table → canonical id prefix, in the fixed order the id map is built.
const TABLE_PREFIX: [(&str, &str); 5] = [
    ("messages", "msg"),
    ("tasks", "task"),
    ("dispatch_contexts", "ctx"),
    ("decision_gates", "gate"),
    ("coordinator_runs", "run"),
];

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "runOpSequence" => run_op_sequence(input),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn str_field(op: &Value, key: &str) -> String {
    op.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn opt_str_field(op: &Value, key: &str) -> Option<String> {
    op.get(key).and_then(Value::as_str).map(str::to_string)
}

/// The completion timestamp the TS shim stamps for terminal states (its
/// `new Date().toISOString()`). A fixed value here suffices because the harness
/// normalizes timestamps to "SET" — only null-vs-set matters.
fn terminal_stamp(status: &str) -> Option<&'static str> {
    if status == "completed" || status == "failed" {
        Some("2026-01-01T00:00:00.000Z")
    } else {
        None
    }
}

fn resolve_ref(value: Option<&Value>, created: &[Option<String>]) -> String {
    let Some(text) = value.and_then(Value::as_str) else {
        return String::new();
    };
    if let Some(index) = text.strip_prefix('@') {
        let idx: usize = index.parse().unwrap_or(usize::MAX);
        return created.get(idx).and_then(Clone::clone).unwrap_or_default();
    }
    text.to_string()
}

/// Read-op payloads capture typed rows now and canonicalize after replay, once
/// the final id map exists.
enum Pending {
    Ok(bool),
    Messages(Vec<Message>),
    Tasks(Vec<Task>),
    Task(Option<Task>),
    Gates(Vec<DecisionGate>),
    Dispatches(Vec<DispatchContext>),
    Run(Option<CoordinatorRun>),
}

#[allow(clippy::too_many_lines)]
fn run_op_sequence(input: &Value) -> Value {
    let db = match OrchestrationDb::open_in_memory() {
        Ok(db) => db,
        Err(err) => return json!({ "__parity_error__": err.to_string() }),
    };
    let ops = input.get("ops").and_then(Value::as_array).cloned().unwrap_or_default();

    let mut created: Vec<Option<String>> = Vec::with_capacity(ops.len());
    let mut pending: Vec<Pending> = Vec::with_capacity(ops.len());

    for (index, op) in ops.iter().enumerate() {
        // Deterministic unique id for anything this op creates (canonicalized away).
        let gen = format!("e{index}");
        let mut created_id: Option<String> = None;
        let result = match op.get("op").and_then(Value::as_str).unwrap_or_default() {
            "sendMessage" => {
                let message = NewMessage {
                    id: gen.clone(),
                    from_handle: str_field(op, "from"),
                    to_handle: str_field(op, "to"),
                    subject: str_field(op, "subject"),
                    body: opt_str_field(op, "body").unwrap_or_default(),
                    message_type: opt_str_field(op, "type").unwrap_or_else(|| "status".into()),
                    priority: opt_str_field(op, "priority").unwrap_or_else(|| "normal".into()),
                    thread_id: opt_str_field(op, "threadId"),
                    payload: opt_str_field(op, "payload"),
                };
                let ok = db.send_message(&message).is_ok();
                if ok {
                    created_id = Some(gen);
                }
                Pending::Ok(ok)
            }
            "markRead" => {
                let id = resolve_ref(op.get("message"), &created);
                Pending::Ok(db.mark_as_read(&[&id]).is_ok())
            }
            "markDelivered" => {
                let id = resolve_ref(op.get("message"), &created);
                Pending::Ok(db.mark_as_delivered(&[&id]).is_ok())
            }
            "createTask" => {
                let deps: Vec<String> = op
                    .get("deps")
                    .and_then(Value::as_array)
                    .map(|arr| arr.iter().map(|v| resolve_ref(Some(v), &created)).collect())
                    .unwrap_or_default();
                let deps_ref: Vec<&str> = deps.iter().map(String::as_str).collect();
                let parent = op.get("parentId").map(|v| resolve_ref(Some(v), &created));
                let created_by = opt_str_field(op, "createdBy");
                let ok = db
                    .create_task(&gen, &str_field(op, "spec"), parent.as_deref(), &deps_ref, created_by.as_deref(), None, None)
                    .is_ok();
                if ok {
                    created_id = Some(gen);
                }
                Pending::Ok(ok)
            }
            "updateTaskStatus" => {
                let task = resolve_ref(op.get("task"), &created);
                let status = str_field(op, "status");
                let result = opt_str_field(op, "result");
                let updated = db
                    .update_task_status(&task, &status, result.as_deref(), terminal_stamp(&status))
                    .unwrap_or(None);
                Pending::Ok(updated.is_some())
            }
            "createDispatch" => {
                let task = resolve_ref(op.get("task"), &created);
                match db.create_dispatch_context(&task, &str_field(op, "assignee"), &gen) {
                    Ok(_) => {
                        created_id = Some(gen);
                        Pending::Ok(true)
                    }
                    Err(_) => Pending::Ok(false),
                }
            }
            "completeDispatch" => {
                Pending::Ok(db.complete_dispatch(&resolve_ref(op.get("dispatch"), &created)).is_ok())
            }
            "failDispatch" => {
                let dispatch = resolve_ref(op.get("dispatch"), &created);
                let failed = db.fail_dispatch(&dispatch, &str_field(op, "error")).unwrap_or(None);
                Pending::Ok(failed.is_some())
            }
            "recordHeartbeat" => {
                let dispatch = resolve_ref(op.get("dispatch"), &created);
                Pending::Ok(db.record_heartbeat(&dispatch, &str_field(op, "at")).is_ok())
            }
            "createGate" => {
                let task = resolve_ref(op.get("task"), &created);
                let options: Vec<String> = op
                    .get("options")
                    .and_then(Value::as_array)
                    .map(|arr| arr.iter().filter_map(Value::as_str).map(str::to_string).collect())
                    .unwrap_or_default();
                let options_ref: Vec<&str> = options.iter().map(String::as_str).collect();
                let ok = db.create_gate(&gen, &task, &str_field(op, "question"), &options_ref).is_ok();
                if ok {
                    created_id = Some(gen);
                }
                Pending::Ok(ok)
            }
            "resolveGate" => {
                let gate = resolve_ref(op.get("gate"), &created);
                let resolved = db.resolve_gate(&gate, &str_field(op, "resolution")).unwrap_or(None);
                Pending::Ok(resolved.is_some())
            }
            "timeoutGate" => {
                let gate = resolve_ref(op.get("gate"), &created);
                Pending::Ok(db.timeout_gate(&gate).unwrap_or(None).is_some())
            }
            "createCoordinatorRun" => {
                let poll = op.get("pollIntervalMs").and_then(Value::as_i64);
                let ok = db
                    .create_coordinator_run(&gen, &str_field(op, "spec"), &str_field(op, "coordinator"), poll)
                    .is_ok();
                if ok {
                    created_id = Some(gen);
                }
                Pending::Ok(ok)
            }
            "updateCoordinatorRun" => {
                let run = resolve_ref(op.get("run"), &created);
                let status = str_field(op, "status");
                let updated =
                    db.update_coordinator_run(&run, &status, terminal_stamp(&status)).unwrap_or(None);
                Pending::Ok(updated.is_some())
            }
            "getUnreadMessages" => {
                Pending::Messages(db.get_unread_messages(&str_field(op, "handle"), None).unwrap_or_default())
            }
            "getUndeliveredUnread" => Pending::Messages(
                db.get_undelivered_unread_messages(&str_field(op, "handle"), None).unwrap_or_default(),
            ),
            "listTasks" => {
                let status = opt_str_field(op, "status");
                Pending::Tasks(db.list_tasks(status.as_deref()).unwrap_or_default())
            }
            "getTask" => {
                let task = resolve_ref(op.get("task"), &created);
                Pending::Task(db.get_task(&task).unwrap_or(None))
            }
            "listGates" => {
                let task = resolve_ref(op.get("task"), &created);
                let status = opt_str_field(op, "status");
                Pending::Gates(db.list_gates(Some(&task), status.as_deref()).unwrap_or_default())
            }
            "getStaleDispatches" => {
                Pending::Dispatches(db.get_stale_dispatches(&str_field(op, "threshold")).unwrap_or_default())
            }
            "getActiveCoordinatorRun" => Pending::Run(db.active_coordinator_run().unwrap_or(None)),
            other => return json!({ "__parity_error__": format!("unknown op {other}") }),
        };
        created.push(created_id);
        pending.push(result);
    }

    let id_map = build_id_map(db.connection());
    let ops_out: Vec<Value> = pending.into_iter().map(|entry| finalize(entry, &id_map)).collect();
    json!({ "ops": ops_out, "state": dump_state(db.connection(), &id_map) })
}

fn build_id_map(conn: &Connection) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for (table, prefix) in TABLE_PREFIX {
        let mut stmt = conn.prepare(&format!("SELECT id FROM {table} ORDER BY rowid")).expect("prepare id scan");
        let ids: Vec<String> =
            stmt.query_map([], |row| row.get::<_, String>(0)).expect("id scan").flatten().collect();
        for (index, id) in ids.into_iter().enumerate() {
            map.insert(id, format!("{prefix}_{index}"));
        }
    }
    map
}

fn canon(map: &HashMap<String, String>, id: Option<String>) -> Value {
    match id {
        Some(value) => Value::String(map.get(&value).cloned().unwrap_or(value)),
        None => Value::Null,
    }
}

fn norm_ts(value: Option<String>) -> Value {
    match value {
        Some(_) => json!("SET"),
        None => Value::Null,
    }
}

fn canon_deps(map: &HashMap<String, String>, deps_json: &str) -> Value {
    let deps: Vec<String> = serde_json::from_str(deps_json).unwrap_or_default();
    Value::Array(deps.into_iter().map(|d| canon(map, Some(d))).collect())
}

fn opt(value: Option<String>) -> Value {
    match value {
        Some(text) => Value::String(text),
        None => Value::Null,
    }
}

/// Sort a canonicalized read list by creation ordinal (the integer in the
/// canonical id) so ordering is deterministic regardless of SQLite tie-break.
fn by_ordinal(mut rows: Vec<Value>) -> Value {
    rows.sort_by_key(|row| {
        row.get("id")
            .and_then(Value::as_str)
            .and_then(|s| s.rsplit('_').next())
            .and_then(|n| n.parse::<i64>().ok())
            .unwrap_or(i64::MAX)
    });
    Value::Array(rows)
}

fn finalize(entry: Pending, map: &HashMap<String, String>) -> Value {
    match entry {
        Pending::Ok(ok) => json!({ "ok": ok }),
        Pending::Messages(rows) => by_ordinal(rows.iter().map(|m| read_message(map, m)).collect()),
        Pending::Tasks(rows) => by_ordinal(rows.iter().map(|t| read_task(map, t)).collect()),
        Pending::Task(row) => row.map_or(Value::Null, |t| read_task(map, &t)),
        Pending::Gates(rows) => by_ordinal(rows.iter().map(|g| read_gate(map, g)).collect()),
        Pending::Dispatches(rows) => by_ordinal(rows.iter().map(|d| read_dispatch(map, d)).collect()),
        Pending::Run(row) => row.map_or(Value::Null, |r| read_run(map, &r)),
    }
}

fn read_message(map: &HashMap<String, String>, m: &Message) -> Value {
    json!({
        "id": canon(map, Some(m.id.clone())),
        "from_handle": m.from_handle,
        "to_handle": m.to_handle,
        "subject": m.subject,
        "body": m.body,
        "type": m.message_type,
        "priority": m.priority,
    })
}

fn read_task(map: &HashMap<String, String>, t: &Task) -> Value {
    json!({
        "id": canon(map, Some(t.id.clone())),
        "parent_id": canon(map, t.parent_id.clone()),
        "spec": t.spec,
        "status": t.status,
        "deps": canon_deps(map, &t.deps),
        "result": opt(t.result.clone()),
    })
}

fn read_gate(map: &HashMap<String, String>, g: &DecisionGate) -> Value {
    json!({
        "id": canon(map, Some(g.id.clone())),
        "task_id": canon(map, Some(g.task_id.clone())),
        "question": g.question,
        "options": g.options,
        "status": g.status,
        "resolution": opt(g.resolution.clone()),
    })
}

fn read_dispatch(map: &HashMap<String, String>, d: &DispatchContext) -> Value {
    json!({
        "id": canon(map, Some(d.id.clone())),
        "task_id": canon(map, Some(d.task_id.clone())),
        "assignee_handle": opt(d.assignee_handle.clone()),
        "status": d.status,
        "failure_count": d.failure_count,
    })
}

fn read_run(map: &HashMap<String, String>, r: &CoordinatorRun) -> Value {
    json!({
        "id": canon(map, Some(r.id.clone())),
        "spec": r.spec,
        "status": r.status,
        "coordinator_handle": r.coordinator_handle,
        "poll_interval_ms": r.poll_interval_ms,
    })
}

// ── canonical state dump (raw SQL; task_title/display_name excluded — see the
// TS adapter: presentation strings, ported at swap time, not a store invariant) ──

fn dump_state(conn: &Connection, map: &HashMap<String, String>) -> Value {
    json!({
        "messages": dump_messages(conn, map),
        "tasks": dump_tasks(conn, map),
        "dispatch_contexts": dump_dispatches(conn, map),
        "decision_gates": dump_gates(conn, map),
        "coordinator_runs": dump_runs(conn, map),
    })
}

fn dump_messages(conn: &Connection, map: &HashMap<String, String>) -> Value {
    let mut stmt = conn
        .prepare(
            "SELECT id, from_handle, to_handle, subject, body, type, priority, thread_id, payload, read, sequence, delivered_at, created_at
             FROM messages ORDER BY rowid",
        )
        .expect("prepare messages dump");
    let rows = stmt
        .query_map([], |row| {
            Ok(json!({
                "id": canon(map, Some(row.get::<_, String>(0)?)),
                "from_handle": row.get::<_, String>(1)?,
                "to_handle": row.get::<_, String>(2)?,
                "subject": row.get::<_, String>(3)?,
                "body": row.get::<_, String>(4)?,
                "type": row.get::<_, String>(5)?,
                "priority": row.get::<_, String>(6)?,
                "thread_id": opt(row.get::<_, Option<String>>(7)?),
                "payload": opt(row.get::<_, Option<String>>(8)?),
                "read": row.get::<_, i64>(9)?,
                "sequence": row.get::<_, i64>(10)?,
                "delivered_at": norm_ts(row.get::<_, Option<String>>(11)?),
                "created_at": norm_ts(row.get::<_, Option<String>>(12)?),
            }))
        })
        .expect("messages dump");
    Value::Array(rows.flatten().collect())
}

fn dump_tasks(conn: &Connection, map: &HashMap<String, String>) -> Value {
    let mut stmt = conn
        .prepare(
            "SELECT id, parent_id, created_by_terminal_handle, spec, status, deps, result, completed_at, created_at
             FROM tasks ORDER BY rowid",
        )
        .expect("prepare tasks dump");
    let rows = stmt
        .query_map([], |row| {
            Ok(json!({
                "id": canon(map, Some(row.get::<_, String>(0)?)),
                "parent_id": canon(map, row.get::<_, Option<String>>(1)?),
                "created_by_terminal_handle": opt(row.get::<_, Option<String>>(2)?),
                "spec": row.get::<_, String>(3)?,
                "status": row.get::<_, String>(4)?,
                "deps": canon_deps(map, &row.get::<_, String>(5)?),
                "result": opt(row.get::<_, Option<String>>(6)?),
                "completed_at": norm_ts(row.get::<_, Option<String>>(7)?),
                "created_at": norm_ts(row.get::<_, Option<String>>(8)?),
            }))
        })
        .expect("tasks dump");
    Value::Array(rows.flatten().collect())
}

fn dump_dispatches(conn: &Connection, map: &HashMap<String, String>) -> Value {
    let mut stmt = conn
        .prepare(
            "SELECT id, task_id, assignee_handle, status, failure_count, last_failure, dispatched_at, completed_at, created_at, last_heartbeat_at
             FROM dispatch_contexts ORDER BY rowid",
        )
        .expect("prepare dispatch dump");
    let rows = stmt
        .query_map([], |row| {
            Ok(json!({
                "id": canon(map, Some(row.get::<_, String>(0)?)),
                "task_id": canon(map, Some(row.get::<_, String>(1)?)),
                "assignee_handle": opt(row.get::<_, Option<String>>(2)?),
                "status": row.get::<_, String>(3)?,
                "failure_count": row.get::<_, i64>(4)?,
                "last_failure": opt(row.get::<_, Option<String>>(5)?),
                "dispatched_at": norm_ts(row.get::<_, Option<String>>(6)?),
                "completed_at": norm_ts(row.get::<_, Option<String>>(7)?),
                "created_at": norm_ts(row.get::<_, Option<String>>(8)?),
                "last_heartbeat_at": norm_ts(row.get::<_, Option<String>>(9)?),
            }))
        })
        .expect("dispatch dump");
    Value::Array(rows.flatten().collect())
}

fn dump_gates(conn: &Connection, map: &HashMap<String, String>) -> Value {
    let mut stmt = conn
        .prepare(
            "SELECT id, task_id, question, options, status, resolution, resolved_at, created_at
             FROM decision_gates ORDER BY rowid",
        )
        .expect("prepare gates dump");
    let rows = stmt
        .query_map([], |row| {
            Ok(json!({
                "id": canon(map, Some(row.get::<_, String>(0)?)),
                "task_id": canon(map, Some(row.get::<_, String>(1)?)),
                "question": row.get::<_, String>(2)?,
                "options": row.get::<_, String>(3)?,
                "status": row.get::<_, String>(4)?,
                "resolution": opt(row.get::<_, Option<String>>(5)?),
                "resolved_at": norm_ts(row.get::<_, Option<String>>(6)?),
                "created_at": norm_ts(row.get::<_, Option<String>>(7)?),
            }))
        })
        .expect("gates dump");
    Value::Array(rows.flatten().collect())
}

fn dump_runs(conn: &Connection, map: &HashMap<String, String>) -> Value {
    let mut stmt = conn
        .prepare(
            "SELECT id, spec, status, coordinator_handle, poll_interval_ms, completed_at, created_at
             FROM coordinator_runs ORDER BY rowid",
        )
        .expect("prepare runs dump");
    let rows = stmt
        .query_map([], |row| {
            Ok(json!({
                "id": canon(map, Some(row.get::<_, String>(0)?)),
                "spec": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
                "coordinator_handle": row.get::<_, String>(3)?,
                "poll_interval_ms": row.get::<_, i64>(4)?,
                "completed_at": norm_ts(row.get::<_, Option<String>>(5)?),
                "created_at": norm_ts(row.get::<_, Option<String>>(6)?),
            }))
        })
        .expect("runs dump");
    Value::Array(rows.flatten().collect())
}

//! Schema creation + `user_version` migrations for the orchestration DB,
//! ported from `src/main/runtime/orchestration/db.ts` (`createTables` /
//! `migrate`). SQL strings are byte-copies of the TS template literals
//! (indentation and trailing whitespace included) so the `sqlite_master.sql`
//! text of a Rust-created database matches a TS-created one exactly.

use orca_store::{Database, StoreError};
use rusqlite::OptionalExtension;

// Why: v1 → v2 added 'heartbeat' to messages.type CHECK + last_heartbeat_at;
// v2 → v3 added messages.delivered_at; v3 → v4 tasks.created_by_terminal_handle;
// v4 → v5 tasks.task_title/display_name; v5 → v6 pane-identity columns
// (dispatch_contexts.assignee_pane_key, messages.sender_pane_key) so
// worker_done ownership survives a terminal handle remint. Mirrors db.ts.
pub(crate) const SCHEMA_VERSION: i64 = 6;

/// Byte-copy of the db.ts `createTables` exec template.
const CREATE_TABLES_SQL: &str = r#"
      CREATE TABLE IF NOT EXISTS messages (
        id            TEXT NOT NULL,
        from_handle   TEXT NOT NULL,
        to_handle     TEXT NOT NULL,
        subject       TEXT NOT NULL,
        body          TEXT NOT NULL DEFAULT '',
        type          TEXT NOT NULL DEFAULT 'status'
          CHECK(type IN (
            'status', 'dispatch', 'worker_done', 'merge_ready',
            'escalation', 'handoff', 'decision_gate', 'heartbeat'
          )),
        priority      TEXT NOT NULL DEFAULT 'normal'
          CHECK(priority IN ('normal', 'high', 'urgent')),
        thread_id     TEXT,
        payload       TEXT,
        read          INTEGER NOT NULL DEFAULT 0,
        sequence      INTEGER PRIMARY KEY AUTOINCREMENT,
        created_at    TEXT NOT NULL DEFAULT (datetime('now')),
        delivered_at  TEXT,
        sender_pane_key TEXT
      );

      CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_id ON messages(id);
      CREATE INDEX IF NOT EXISTS idx_inbox ON messages(to_handle, read);
      CREATE INDEX IF NOT EXISTS idx_thread ON messages(thread_id);

      CREATE TABLE IF NOT EXISTS tasks (
        id            TEXT PRIMARY KEY,
        parent_id     TEXT,
        created_by_terminal_handle TEXT,
        task_title    TEXT,
        display_name  TEXT,
        spec          TEXT NOT NULL,
        status        TEXT NOT NULL DEFAULT 'pending'
          CHECK(status IN (
            'pending', 'ready', 'dispatched',
            'completed', 'failed', 'blocked'
          )),
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
        assignee_pane_key   TEXT,
        status              TEXT NOT NULL DEFAULT 'pending'
          CHECK(status IN ('pending', 'dispatched', 'completed', 'failed', 'circuit_broken')),
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
          CHECK(status IN ('pending', 'resolved', 'timeout')),
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
          CHECK(status IN ('idle', 'running', 'completed', 'failed')),
        coordinator_handle  TEXT NOT NULL,
        poll_interval_ms    INTEGER NOT NULL DEFAULT 2000,
        created_at          TEXT NOT NULL DEFAULT (datetime('now')),
        completed_at        TEXT
      );
    "#;

/// Byte-copy of the db.ts v1 → v2 messages-table rebuild exec template
/// (widens the type CHECK to include 'heartbeat' and adds `delivered_at` in
/// the same rewrite; recreates the indexes DROP TABLE removed).
const MESSAGES_HEARTBEAT_REBUILD_SQL: &str = r#"
            CREATE TABLE messages_new (
              id            TEXT NOT NULL,
              from_handle   TEXT NOT NULL,
              to_handle     TEXT NOT NULL,
              subject       TEXT NOT NULL,
              body          TEXT NOT NULL DEFAULT '',
              type          TEXT NOT NULL DEFAULT 'status'
                CHECK(type IN (
                  'status', 'dispatch', 'worker_done', 'merge_ready',
                  'escalation', 'handoff', 'decision_gate', 'heartbeat'
                )),
              priority      TEXT NOT NULL DEFAULT 'normal'
                CHECK(priority IN ('normal', 'high', 'urgent')),
              thread_id     TEXT,
              payload       TEXT,
              read          INTEGER NOT NULL DEFAULT 0,
              sequence      INTEGER PRIMARY KEY AUTOINCREMENT,
              created_at    TEXT NOT NULL DEFAULT (datetime('now')),
              delivered_at  TEXT
            );
            INSERT INTO messages_new (
              id, from_handle, to_handle, subject, body, type, priority,
              thread_id, payload, read, sequence, created_at
            )
            SELECT
              id, from_handle, to_handle, subject, body, type, priority,
              thread_id, payload, read, sequence, created_at
            FROM messages;
            DROP TABLE messages;
            ALTER TABLE messages_new RENAME TO messages;

            CREATE UNIQUE INDEX idx_messages_id ON messages(id);
            CREATE INDEX idx_inbox ON messages(to_handle, read);
            CREATE INDEX idx_messages_undelivered_inbox
              ON messages(to_handle, read, delivered_at, sequence);
            CREATE INDEX idx_thread ON messages(thread_id);
          "#;

// Why: written with \n escapes (not a raw string) because the statement has no
// terminating `;`, so SQLite stores the trailing "\n    " into sqlite_master.sql
// — literal trailing whitespace in source would be fragile.
const UNDELIVERED_INBOX_INDEX_SQL: &str =
    "\n      CREATE INDEX IF NOT EXISTS idx_messages_undelivered_inbox\n        ON messages(to_handle, read, delivered_at, sequence)\n    ";

/// TS `createTables`: idempotent full-schema creation, then the
/// delivered_at-gated inbox index.
pub(crate) fn create_tables(db: &Database) -> Result<(), StoreError> {
    db.exec(CREATE_TABLES_SQL)?;
    create_undelivered_inbox_index_if_possible(db)
}

/// TS `migrate`: incremental `user_version` ladder inside one transaction.
/// `user_version` is bumped only on success; a current-or-future version
/// (>= 5) returns immediately and is left untouched (mirrors TS).
pub(crate) fn migrate(db: &Database) -> Result<(), StoreError> {
    let current = db.pragma_i64("user_version")?;
    if current >= SCHEMA_VERSION {
        return Ok(());
    }
    db.exec("BEGIN")?;
    // Why: COMMIT sits inside the fallible path (as in the TS try block), so
    // any failure — including a failed COMMIT — rolls back and the DB stays at
    // the prior version.
    let applied = apply_version_ladder(db, current).and_then(|()| db.exec("COMMIT"));
    if let Err(err) = applied {
        db.exec("ROLLBACK")?;
        return Err(err);
    }
    Ok(())
}

fn apply_version_ladder(db: &Database, current: i64) -> Result<(), StoreError> {
    // v1 → v2: add last_heartbeat_at; rebuild messages to widen the type CHECK
    // (SQLite cannot ALTER a CHECK constraint). The rebuild also carries the
    // v3 delivered_at column so v1 DBs need only one table rewrite.
    if current < 2 {
        if !has_column(db, "dispatch_contexts", "last_heartbeat_at")? {
            db.exec("ALTER TABLE dispatch_contexts ADD COLUMN last_heartbeat_at TEXT")?;
        }
        if !messages_type_check_allows_heartbeat(db)? {
            db.exec(MESSAGES_HEARTBEAT_REBUILD_SQL)?;
        }
    }
    // v2 → v3: DBs that reached v2 via the rebuild above already have the
    // column; this covers DBs that were at v2 before v3 shipped.
    if current < 3 {
        if !has_column(db, "messages", "delivered_at")? {
            db.exec("ALTER TABLE messages ADD COLUMN delivered_at TEXT")?;
        }
    }
    if current < 4 {
        if !has_column(db, "tasks", "created_by_terminal_handle")? {
            db.exec("ALTER TABLE tasks ADD COLUMN created_by_terminal_handle TEXT")?;
        }
    }
    if current < 5 {
        if !has_column(db, "tasks", "task_title")? {
            db.exec("ALTER TABLE tasks ADD COLUMN task_title TEXT")?;
        }
        if !has_column(db, "tasks", "display_name")? {
            db.exec("ALTER TABLE tasks ADD COLUMN display_name TEXT")?;
        }
    }
    // v5 → v6: pane-identity columns for remint-stable ownership.
    if current < 6 {
        if !has_column(db, "dispatch_contexts", "assignee_pane_key")? {
            db.exec("ALTER TABLE dispatch_contexts ADD COLUMN assignee_pane_key TEXT")?;
        }
        if !has_column(db, "messages", "sender_pane_key")? {
            db.exec("ALTER TABLE messages ADD COLUMN sender_pane_key TEXT")?;
        }
    }
    create_undelivered_inbox_index_if_possible(db)?;
    db.exec(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))
}

fn create_undelivered_inbox_index_if_possible(db: &Database) -> Result<(), StoreError> {
    if !has_column(db, "messages", "delivered_at")? {
        return Ok(());
    }
    db.exec(UNDELIVERED_INBOX_INDEX_SQL)
}

fn has_column(db: &Database, table: &str, column: &str) -> Result<bool, StoreError> {
    let conn = db.connection();
    // Why: table name interpolated, not bound — PRAGMA takes no parameters
    // (same as the TS hasColumn); callers only pass fixed schema names.
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get("name")?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

// Why: sqlite_master keeps the original CREATE TABLE text including the CHECK
// clause; inspecting it is the cheapest reliable pre-rebuild probe (same as TS).
fn messages_type_check_allows_heartbeat(db: &Database) -> Result<bool, StoreError> {
    let sql: Option<Option<String>> = db
        .connection()
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'messages'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(sql.flatten().is_some_and(|s| s.contains("'heartbeat'")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_column_probes_existing_and_missing_columns() {
        let db = Database::open_in_memory().unwrap();
        db.exec("CREATE TABLE probe (alpha TEXT, beta INTEGER)").unwrap();
        assert!(has_column(&db, "probe", "alpha").unwrap());
        assert!(has_column(&db, "probe", "beta").unwrap());
        assert!(!has_column(&db, "probe", "gamma").unwrap());
        // Missing table → empty table_info → false (mirrors TS).
        assert!(!has_column(&db, "no_such_table", "alpha").unwrap());
    }

    #[test]
    fn heartbeat_probe_reads_check_text_from_sqlite_master() {
        let db = Database::open_in_memory().unwrap();
        // No messages table at all → false.
        assert!(!messages_type_check_allows_heartbeat(&db).unwrap());
        db.exec("CREATE TABLE messages (type TEXT CHECK(type IN ('status')))").unwrap();
        assert!(!messages_type_check_allows_heartbeat(&db).unwrap());
        db.exec("DROP TABLE messages").unwrap();
        db.exec("CREATE TABLE messages (type TEXT CHECK(type IN ('status', 'heartbeat')))")
            .unwrap();
        assert!(messages_type_check_allows_heartbeat(&db).unwrap());
    }

    #[test]
    fn fresh_database_lands_on_current_schema_version() {
        let db = Database::open_in_memory().unwrap();
        create_tables(&db).unwrap();
        migrate(&db).unwrap();
        assert_eq!(db.pragma_i64("user_version").unwrap(), SCHEMA_VERSION);
        // Idempotent: a second migrate is a no-op.
        migrate(&db).unwrap();
        assert_eq!(db.pragma_i64("user_version").unwrap(), SCHEMA_VERSION);
    }
}

//! Synchronous SQLite adapter, ported from `src/main/sqlite/sync-database.ts`.
//!
//! The TS adapter exposes `exec`/`prepare`/`pragma`/`close` over
//! `node:sqlite`'s `DatabaseSync`, with a `fileMustExist` guard. This is the
//! idiomatic Rust equivalent over `rusqlite::Connection`.

use rusqlite::{Connection, OpenFlags};
use std::path::Path;

#[derive(Debug)]
pub enum StoreError {
    /// `file_must_exist` was set but the database file is absent.
    Missing(String),
    /// A logical/domain error (e.g. an invalid state transition).
    Message(String),
    Sqlite(rusqlite::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Missing(path) => write!(f, "SQLite database does not exist: {path}"),
            StoreError::Message(message) => write!(f, "{message}"),
            StoreError::Sqlite(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        StoreError::Sqlite(error)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OpenOptions {
    pub read_only: bool,
    pub file_must_exist: bool,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Ok(Self { conn: Connection::open_in_memory()? })
    }

    pub fn open(path: &str, options: OpenOptions) -> Result<Self, StoreError> {
        if options.file_must_exist && path != ":memory:" && !Path::new(path).exists() {
            return Err(StoreError::Missing(path.to_string()));
        }
        let flags = if options.read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
        };
        Ok(Self { conn: Connection::open_with_flags(path, flags)? })
    }

    /// Run one or more SQL statements (TS `exec`).
    pub fn exec(&self, sql: &str) -> Result<(), StoreError> {
        self.conn.execute_batch(sql)?;
        Ok(())
    }

    /// Borrow the underlying connection for prepared statements / queries.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Read a single integer-valued `PRAGMA` (e.g. `user_version`).
    pub fn pragma_i64(&self, pragma: &str) -> Result<i64, StoreError> {
        Ok(self.conn.query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))?)
    }

    /// Set an integer-valued `PRAGMA`.
    pub fn set_pragma_i64(&self, pragma: &str, value: i64) -> Result<(), StoreError> {
        self.conn.execute_batch(&format!("PRAGMA {pragma} = {value}"))?;
        Ok(())
    }

    /// Consume and close the database (mirrors TS `close`; `Connection` also
    /// closes on drop).
    pub fn close(self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_exec_and_query() {
        let db = Database::open_in_memory().unwrap();
        db.exec(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             INSERT INTO items (name) VALUES ('a'), ('b'), ('c');",
        )
        .unwrap();
        let count: i64 =
            db.connection().query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn pragma_round_trips() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.pragma_i64("user_version").unwrap(), 0);
        db.set_pragma_i64("user_version", 7).unwrap();
        assert_eq!(db.pragma_i64("user_version").unwrap(), 7);
    }

    #[test]
    fn file_must_exist_guard() {
        let result = Database::open(
            "/definitely/not/a/real/orca-store-path.db",
            OpenOptions { read_only: false, file_must_exist: true },
        );
        assert!(matches!(result, Err(StoreError::Missing(_))));
    }

    #[test]
    fn memory_path_bypasses_file_must_exist() {
        let db = Database::open(":memory:", OpenOptions { read_only: false, file_must_exist: true });
        assert!(db.is_ok());
    }
}

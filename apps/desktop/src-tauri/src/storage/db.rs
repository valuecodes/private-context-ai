//! Opening the SQLCipher database.

use std::path::Path;

use rusqlite::{Connection, ErrorCode};
use zeroize::Zeroizing;

use super::migrations;
use crate::error::AppError;

/// Opens (or creates) the database with a raw 256-bit key, migrates it, and
/// marks replies a previous run left `pending` as `error`. That last step is
/// safe only because the single-instance lock is already held.
pub fn open(path: &Path, hex_key: &str) -> Result<Connection, AppError> {
    let mut conn = Connection::open(path)?;
    {
        // `x'…'` hands SQLCipher the key itself, so its PBKDF2 step is skipped.
        let pragma = Zeroizing::new(format!("PRAGMA key = \"x'{hex_key}'\";"));
        conn.execute_batch(&pragma)?;
    }
    // The first read is where SQLCipher notices a wrong key.
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|err| match err.sqlite_error_code() {
        Some(ErrorCode::NotADatabase) => {
            AppError::WrongKey("the key does not open the database".into())
        }
        _ => AppError::from(err),
    })?;
    conn.pragma_update(None, "cipher_memory_security", "ON")?;
    conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrations::apply(&mut conn)?;
    conn.execute(
        "UPDATE messages SET status = 'error' WHERE status = 'pending'",
        [],
    )?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::key::new_key;

    #[test]
    fn open_creates_and_migrates() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(&dir.path().join("t.db"), &new_key()).unwrap();

        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);
        let mut tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        tables.sort();
        assert_eq!(
            tables,
            ["audit_events", "conversations", "messages", "settings"]
        );
        let foreign_keys: i64 = conn
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1);
    }

    #[test]
    fn reopen_with_wrong_key_is_wrong_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        drop(open(&path, &new_key()).unwrap());

        assert!(matches!(
            open(&path, &new_key()),
            Err(AppError::WrongKey(_))
        ));
    }

    #[test]
    fn reopen_with_same_key_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let key = new_key();
        let conn = open(&path, &key).unwrap();
        conn.execute("INSERT INTO settings (key, value) VALUES ('a', 'b')", [])
            .unwrap();
        drop(conn);

        let conn = open(&path, &key).unwrap();
        let value: String = conn
            .query_row("SELECT value FROM settings WHERE key = 'a'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(value, "b");
    }

    #[test]
    fn plain_sqlite_cannot_read_db() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        drop(open(&path, &new_key()).unwrap());

        let plain = Connection::open(&path).unwrap();
        let err = plain
            .query_row("SELECT count(*) FROM sqlite_master", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap_err();
        assert_eq!(err.sqlite_error_code(), Some(ErrorCode::NotADatabase));
    }

    #[test]
    fn pending_rows_become_error_on_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let key = new_key();
        let conn = open(&path, &key).unwrap();
        conn.execute_batch(
            "INSERT INTO conversations VALUES ('c', 't', 'm', 'x', 'x');
             INSERT INTO messages (id, conversation_id, role, content, created_at, status)
               VALUES ('a', 'c', 'assistant', 'half', 'x', 'pending'),
                      ('b', 'c', 'assistant', 'done', 'x', 'complete');",
        )
        .unwrap();
        drop(conn);

        let conn = open(&path, &key).unwrap();
        let statuses: Vec<(String, String)> = conn
            .prepare("SELECT id, status FROM messages ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            statuses,
            [
                ("a".to_owned(), "error".to_owned()),
                ("b".to_owned(), "complete".to_owned())
            ]
        );
    }

    #[test]
    fn newer_schema_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let key = new_key();
        let conn = open(&path, &key).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        drop(conn);

        assert!(matches!(open(&path, &key), Err(AppError::Io(_))));
    }
}

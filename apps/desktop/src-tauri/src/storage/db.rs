//! Opening the SQLCipher database.

use std::path::Path;

use rusqlite::{Connection, ErrorCode};
use zeroize::Zeroizing;

use super::{messages, migrations};
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
    recover_pending(&mut conn)?;
    Ok(conn)
}

/// A reply the last run left `pending` failed: a regenerate or edit is
/// undone, as if it had failed while running, and any other reply becomes
/// `error`.
fn recover_pending(conn: &mut Connection) -> Result<(), AppError> {
    let tx = conn.transaction()?;
    let pending: Vec<String> = tx
        .prepare("SELECT id FROM messages WHERE status = 'pending'")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for id in &pending {
        messages::restore_superseded(&tx, id)?;
    }
    tx.execute(
        "UPDATE messages SET status = 'error' WHERE status = 'pending'",
        [],
    )?;
    tx.commit()?;
    Ok(())
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
        assert_eq!(version, 7);
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
            "INSERT INTO conversations (id, title, model, created_at, updated_at)
               VALUES ('c', 't', 'm', 'x', 'x');
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
    fn version_one_db_gains_thinking_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let key = new_key();
        let conn = open(&path, &key).unwrap();
        // Back to what the first release wrote.
        conn.execute_batch(
            "ALTER TABLE conversations DROP COLUMN system_prompt;
             ALTER TABLE conversations DROP COLUMN temperature;
             ALTER TABLE conversations DROP COLUMN num_ctx;
             ALTER TABLE conversations DROP COLUMN think;
             ALTER TABLE conversations DROP COLUMN charts;
             ALTER TABLE messages DROP COLUMN charts;
             ALTER TABLE messages DROP COLUMN superseded_by;
             ALTER TABLE messages DROP COLUMN thinking;
             ALTER TABLE messages DROP COLUMN thinking_ms;
             ALTER TABLE messages DROP COLUMN eval_ms;
             ALTER TABLE messages DROP COLUMN model;
             ALTER TABLE messages DROP COLUMN compare_group;
             PRAGMA user_version = 1;
             INSERT INTO conversations VALUES ('c', 't', 'm', 'x', 'x');
             INSERT INTO messages (id, conversation_id, role, content, created_at, status)
               VALUES ('a', 'c', 'assistant', 'old', 'x', 'complete');",
        )
        .unwrap();
        drop(conn);

        let conn = open(&path, &key).unwrap();
        let row: (String, Option<String>, Option<i64>) = conn
            .query_row(
                "SELECT content, thinking, thinking_ms FROM messages WHERE id = 'a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, ("old".to_owned(), None, None));
    }

    #[test]
    fn version_three_db_gains_settings_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let key = new_key();
        let conn = open(&path, &key).unwrap();
        // Back to what the previous release wrote.
        conn.execute_batch(
            "ALTER TABLE conversations DROP COLUMN system_prompt;
             ALTER TABLE conversations DROP COLUMN temperature;
             ALTER TABLE conversations DROP COLUMN num_ctx;
             ALTER TABLE conversations DROP COLUMN think;
             ALTER TABLE conversations DROP COLUMN charts;
             ALTER TABLE messages DROP COLUMN charts;
             ALTER TABLE messages DROP COLUMN eval_ms;
             ALTER TABLE messages DROP COLUMN model;
             ALTER TABLE messages DROP COLUMN compare_group;
             PRAGMA user_version = 3;
             INSERT INTO conversations VALUES ('c', 't', 'm', 'x', 'x');",
        )
        .unwrap();
        drop(conn);

        let conn = open(&path, &key).unwrap();
        let row: (String, Option<f64>, Option<i64>, String) = conn
            .query_row(
                "SELECT system_prompt, temperature, num_ctx, think FROM conversations
                 WHERE id = 'c'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(row, (String::new(), None, None, "off".to_owned()));
    }

    #[test]
    fn version_four_db_gains_eval_ms() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let key = new_key();
        let conn = open(&path, &key).unwrap();
        // Back to what the previous release wrote.
        conn.execute_batch(
            "ALTER TABLE conversations DROP COLUMN charts;
             ALTER TABLE messages DROP COLUMN charts;
             ALTER TABLE messages DROP COLUMN eval_ms;
             ALTER TABLE messages DROP COLUMN model;
             ALTER TABLE messages DROP COLUMN compare_group;
             PRAGMA user_version = 4;
             INSERT INTO conversations (id, title, model, created_at, updated_at)
               VALUES ('c', 't', 'm', 'x', 'x');
             INSERT INTO messages (id, conversation_id, role, content, created_at, status,
                                   duration_ms)
               VALUES ('a', 'c', 'assistant', 'old', 'x', 'complete', 900);",
        )
        .unwrap();
        drop(conn);

        let conn = open(&path, &key).unwrap();
        let row: (Option<i64>, Option<i64>) = conn
            .query_row(
                "SELECT duration_ms, eval_ms FROM messages WHERE id = 'a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(row, (Some(900), None));
    }

    #[test]
    fn version_five_db_leaves_model_null() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let key = new_key();
        let conn = open(&path, &key).unwrap();
        // Back to what the previous release wrote.
        conn.execute_batch(
            "ALTER TABLE conversations DROP COLUMN charts;
             ALTER TABLE messages DROP COLUMN charts;
             ALTER TABLE messages DROP COLUMN model;
             ALTER TABLE messages DROP COLUMN compare_group;
             PRAGMA user_version = 5;
             INSERT INTO conversations (id, title, model, created_at, updated_at)
               VALUES ('c', 't', 'm', 'x', 'x');
             INSERT INTO messages (id, conversation_id, role, content, created_at, status)
               VALUES ('a', 'c', 'assistant', 'old', 'x', 'complete');",
        )
        .unwrap();
        drop(conn);

        let conn = open(&path, &key).unwrap();
        let row: (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT content, model, compare_group FROM messages WHERE id = 'a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, ("old".to_owned(), None, None));
    }

    #[test]
    fn version_six_db_leaves_charts_off() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let key = new_key();
        let conn = open(&path, &key).unwrap();
        // Back to what the previous release wrote.
        conn.execute_batch(
            "ALTER TABLE conversations DROP COLUMN charts;
             ALTER TABLE messages DROP COLUMN charts;
             PRAGMA user_version = 6;
             INSERT INTO conversations (id, title, model, created_at, updated_at)
               VALUES ('c', 't', 'm', 'x', 'x');
             INSERT INTO messages (id, conversation_id, role, content, created_at, status)
               VALUES ('a', 'c', 'assistant', 'old', 'x', 'complete');",
        )
        .unwrap();
        drop(conn);

        let conn = open(&path, &key).unwrap();
        let row: (i64, Option<String>) = conn
            .query_row(
                "SELECT c.charts, m.charts FROM conversations c JOIN messages m
                 ON m.conversation_id = c.id",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(row, (0, None));
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

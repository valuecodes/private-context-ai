//! Conversation CRUD.

use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;
use uuid::Uuid;

use super::Storage;
use super::messages::{StoredMessage, load_messages};
use crate::error::AppError;
use crate::ollama::types::validate_model;
use crate::redacted::Redacted;

/// Longest title accepted, in characters.
pub const MAX_TITLE_CHARS: usize = 200;
pub const DEFAULT_TITLE: &str = "New chat";

/// One conversation without its messages. The title is often taken from the
/// first prompt, so it is redacted in logs like message text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: Uuid,
    pub title: Redacted,
    pub model: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationWithMessages {
    pub conversation: Conversation,
    pub messages: Vec<StoredMessage>,
}

const COLUMNS: &str = "id, title, model, created_at, updated_at";

/// ISO 8601 UTC with milliseconds, computed by SQLite.
pub(super) const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

pub(super) fn uuid_column(row: &Row<'_>, index: usize) -> rusqlite::Result<Uuid> {
    let text: String = row.get(index)?;
    Uuid::parse_str(&text).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, err.into())
    })
}

fn conversation_row(row: &Row<'_>) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: uuid_column(row, 0)?,
        title: Redacted::new(row.get::<_, String>(1)?),
        model: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

/// Trimmed, 1 to [`MAX_TITLE_CHARS`] characters, no control characters.
fn clean_title(title: &str) -> Result<String, AppError> {
    let title = title.trim();
    if title.is_empty()
        || title.chars().count() > MAX_TITLE_CHARS
        || title.chars().any(char::is_control)
    {
        return Err(AppError::InvalidInput(format!(
            "a title is 1 to {MAX_TITLE_CHARS} characters on one line"
        )));
    }
    Ok(title.to_owned())
}

pub(super) fn not_found() -> AppError {
    AppError::NotFound("no such conversation".into())
}

impl Storage {
    /// `model` may be empty when no model is installed yet; the first send
    /// sets it.
    pub fn create_conversation(
        &self,
        title: Option<&str>,
        model: &str,
    ) -> Result<Conversation, AppError> {
        let title = clean_title(title.unwrap_or(DEFAULT_TITLE))?;
        if !model.is_empty() {
            validate_model(model)?;
        }
        let conn = self.conn();
        let conversation = conn.query_row(
            &format!(
                "INSERT INTO conversations ({COLUMNS}) VALUES (?1, ?2, ?3, {NOW}, {NOW})
                 RETURNING {COLUMNS}"
            ),
            params![Uuid::new_v4().to_string(), title, model],
            conversation_row,
        )?;
        Ok(conversation)
    }

    /// Most recently used first.
    pub fn list_conversations(&self) -> Result<Vec<Conversation>, AppError> {
        let conn = self.conn();
        let mut statement = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM conversations ORDER BY updated_at DESC, rowid DESC"
        ))?;
        let conversations = statement
            .query_map([], conversation_row)?
            .collect::<Result<_, _>>()?;
        Ok(conversations)
    }

    pub fn get_conversation(&self, id: Uuid) -> Result<ConversationWithMessages, AppError> {
        let conn = self.conn();
        let conversation = conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM conversations WHERE id = ?1"),
                [id.to_string()],
                conversation_row,
            )
            .optional()?
            .ok_or_else(not_found)?;
        let messages = load_messages(&conn, id)?;
        Ok(ConversationWithMessages {
            conversation,
            messages,
        })
    }

    /// Renaming does not count as use, so the list order stays put.
    pub fn rename_conversation(&self, id: Uuid, title: &str) -> Result<Conversation, AppError> {
        let title = clean_title(title)?;
        let conn = self.conn();
        conn.query_row(
            &format!("UPDATE conversations SET title = ?2 WHERE id = ?1 RETURNING {COLUMNS}"),
            params![id.to_string(), title],
            conversation_row,
        )
        .optional()?
        .ok_or_else(not_found)
    }

    /// Deletes the conversation and, by cascade, its messages. Deleting one
    /// that is already gone is not an error.
    pub fn delete_conversation(&self, id: Uuid) -> Result<(), AppError> {
        self.conn()
            .execute("DELETE FROM conversations WHERE id = ?1", [id.to_string()])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::testing::temp_storage;

    #[test]
    fn crud_round_trip() {
        let (storage, _dir) = temp_storage();

        let created = storage
            .create_conversation(Some("  First  "), "llama3.2")
            .unwrap();
        assert_eq!(created.title.expose(), "First");
        assert_eq!(created.model, "llama3.2");
        assert_eq!(created.created_at, created.updated_at);
        assert!(created.created_at.ends_with('Z'));

        assert_eq!(storage.list_conversations().unwrap(), vec![created.clone()]);
        let fetched = storage.get_conversation(created.id).unwrap();
        assert_eq!(fetched.conversation, created);
        assert!(fetched.messages.is_empty());

        let renamed = storage.rename_conversation(created.id, "Second").unwrap();
        assert_eq!(renamed.title.expose(), "Second");
        assert_eq!(renamed.updated_at, created.updated_at);

        storage.delete_conversation(created.id).unwrap();
        assert!(storage.list_conversations().unwrap().is_empty());
        assert!(matches!(
            storage.get_conversation(created.id),
            Err(AppError::NotFound(_))
        ));
        storage.delete_conversation(created.id).unwrap();
    }

    #[test]
    fn create_defaults_title_and_allows_empty_model() {
        let (storage, _dir) = temp_storage();
        let created = storage.create_conversation(None, "").unwrap();
        assert_eq!(created.title.expose(), DEFAULT_TITLE);
        assert_eq!(created.model, "");
        assert!(matches!(
            storage.create_conversation(None, "bad\nmodel"),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn rename_rejects_bad_titles() {
        let (storage, _dir) = temp_storage();
        let id = storage.create_conversation(None, "m").unwrap().id;
        let long = "x".repeat(MAX_TITLE_CHARS + 1);
        for title in ["", "   ", "two\nlines", long.as_str()] {
            assert!(matches!(
                storage.rename_conversation(id, title),
                Err(AppError::InvalidInput(_))
            ));
        }
        assert!(
            storage
                .rename_conversation(id, &"x".repeat(MAX_TITLE_CHARS))
                .is_ok()
        );
    }

    #[test]
    fn unknown_id_is_not_found() {
        let (storage, _dir) = temp_storage();
        let id = Uuid::new_v4();
        assert!(matches!(
            storage.get_conversation(id),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            storage.rename_conversation(id, "x"),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn list_orders_by_updated_at() {
        let (storage, _dir) = temp_storage();
        let first = storage.create_conversation(Some("a"), "m").unwrap();
        let second = storage.create_conversation(Some("b"), "m").unwrap();
        let ids = |list: Vec<Conversation>| list.into_iter().map(|c| c.id).collect::<Vec<_>>();

        assert_eq!(
            ids(storage.list_conversations().unwrap()),
            [second.id, first.id]
        );

        storage
            .conn()
            .execute(
                "UPDATE conversations SET updated_at = '9999' WHERE id = ?1",
                [first.id.to_string()],
            )
            .unwrap();
        assert_eq!(
            ids(storage.list_conversations().unwrap()),
            [first.id, second.id]
        );
    }
}

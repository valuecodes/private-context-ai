//! Conversation CRUD.

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::Storage;
use super::messages::{StoredMessage, bad_text, load_messages};
use crate::error::AppError;
use crate::ollama::types::{ChatOptions, ThinkChoice, validate_model};
use crate::redacted::Redacted;

/// Longest title accepted, in characters.
pub const MAX_TITLE_CHARS: usize = 200;
pub const DEFAULT_TITLE: &str = "New chat";
/// Longest system prompt accepted, in bytes.
pub const MAX_SYSTEM_PROMPT_BYTES: usize = 32 * 1024;
/// Longest search query accepted, in characters.
pub const MAX_SEARCH_CHARS: usize = 200;
/// Most conversations one search returns.
const SEARCH_LIMIT: usize = 100;

/// How a conversation is sent, applied from its next send. Saved chats keep
/// it in their row, temporary ones in memory.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationSettings {
    /// Sent first as a `system` message when not blank; never a message row.
    pub system_prompt: Redacted,
    /// Only `temperature` and `num_ctx` are kept.
    pub options: ChatOptions,
    pub think: ThinkChoice,
    /// Offers the display-only `render_chart` tool to models that list
    /// `tools`. Off unless the user turns it on.
    #[serde(default)]
    pub charts: bool,
}

/// At most [`MAX_SYSTEM_PROMPT_BYTES`] of text: a system prompt or a
/// project's instructions, named `what` in the error.
pub(super) fn check_prompt_text(text: &Redacted, what: &str) -> Result<(), AppError> {
    if text.len_bytes() > MAX_SYSTEM_PROMPT_BYTES || text.expose().contains('\0') {
        return Err(AppError::InvalidInput(format!(
            "{what} is at most {MAX_SYSTEM_PROMPT_BYTES} bytes of text"
        )));
    }
    Ok(())
}

impl ConversationSettings {
    pub fn validate(&self) -> Result<(), AppError> {
        check_prompt_text(&self.system_prompt, "a system prompt")?;
        if self.options.num_predict.is_some() || self.options.seed.is_some() {
            return Err(AppError::InvalidInput(
                "only temperature and num_ctx can be saved".into(),
            ));
        }
        self.options.validate()?;
        self.think.validate()
    }

    /// The options Ollama is sent; `None` leaves the body without them.
    pub fn options(&self) -> Option<ChatOptions> {
        (self.options != ChatOptions::default()).then_some(self.options)
    }
}

/// One conversation without its messages. The title is often taken from the
/// first prompt, so it is redacted in logs like message text.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: Uuid,
    pub title: Redacted,
    pub model: String,
    pub created_at: String,
    pub updated_at: String,
    pub settings: ConversationSettings,
    /// The project it belongs to; `None` outside any project.
    pub project_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationWithMessages {
    pub conversation: Conversation,
    pub messages: Vec<StoredMessage>,
}

const COLUMNS: &str = "id, title, model, created_at, updated_at, system_prompt, temperature, \
                       num_ctx, think, charts, project_id";
const SETTINGS_COLUMNS: &str = "system_prompt, temperature, num_ctx, think, charts";

/// The `think` column: `off`, `on` or `level:<name>`.
fn think_column(think: &ThinkChoice) -> String {
    match think {
        ThinkChoice::Off => "off".into(),
        ThinkChoice::On => "on".into(),
        ThinkChoice::Level(level) => format!("level:{level}"),
    }
}

fn parse_think(value: &str) -> Option<ThinkChoice> {
    match value {
        "off" => Some(ThinkChoice::Off),
        "on" => Some(ThinkChoice::On),
        _ => value
            .strip_prefix("level:")
            .map(|level| ThinkChoice::Level(level.to_owned())),
    }
}

/// The five settings columns, starting at `first`.
fn settings_row(row: &Row<'_>, first: usize) -> rusqlite::Result<ConversationSettings> {
    let think: String = row.get(first + 3)?;
    // Saved from an `f32`, so it converts back exactly.
    let temperature = row.get::<_, Option<f64>>(first + 1)?.map(|t| t as f32);
    Ok(ConversationSettings {
        system_prompt: Redacted::new(row.get::<_, String>(first)?),
        options: ChatOptions {
            temperature,
            num_ctx: row
                .get::<_, Option<i64>>(first + 2)?
                .and_then(|n| u32::try_from(n).ok()),
            ..ChatOptions::default()
        },
        think: parse_think(&think).ok_or_else(|| bad_text(first + 3, "think"))?,
        charts: row.get::<_, i64>(first + 4)? != 0,
    })
}

/// The settings of conversation `id`, read on `conn` so a transaction sees
/// its own snapshot.
pub(super) fn load_settings(conn: &Connection, id: &str) -> Result<ConversationSettings, AppError> {
    conn.query_row(
        &format!("SELECT {SETTINGS_COLUMNS} FROM conversations WHERE id = ?1"),
        [id],
        |row| settings_row(row, 0),
    )
    .optional()?
    .ok_or_else(not_found)
}

/// The instructions of conversation `id`'s project, empty outside any
/// project. Read on `conn` so a transaction sees its own snapshot.
pub(super) fn load_project_instructions(conn: &Connection, id: &str) -> Result<Redacted, AppError> {
    conn.query_row(
        "SELECT coalesce(p.instructions, '') FROM conversations c
         LEFT JOIN projects p ON p.id = c.project_id WHERE c.id = ?1",
        [id],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(Redacted::new)
    .ok_or_else(not_found)
}

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
        settings: settings_row(row, 5)?,
        project_id: row
            .get::<_, Option<String>>(10)?
            .map(|_| uuid_column(row, 10))
            .transpose()?,
    })
}

/// Trimmed, 1 to [`MAX_TITLE_CHARS`] characters, no control characters: a
/// title or a project name, named `what` in the error.
pub(super) fn clean_line(text: &str, what: &str) -> Result<String, AppError> {
    let text = text.trim();
    if text.is_empty()
        || text.chars().count() > MAX_TITLE_CHARS
        || text.chars().any(char::is_control)
    {
        return Err(AppError::InvalidInput(format!(
            "{what} is 1 to {MAX_TITLE_CHARS} characters on one line"
        )));
    }
    Ok(text.to_owned())
}

fn clean_title(title: &str) -> Result<String, AppError> {
    clean_line(title, "a title")
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
        self.create_conversation_with(title, model, &ConversationSettings::default(), None)
    }

    /// Like [`Self::create_conversation`], saved with `settings` at once so
    /// no later update can race the first send, and in project `project_id`
    /// when given (`NotFound` if it does not exist).
    pub fn create_conversation_with(
        &self,
        title: Option<&str>,
        model: &str,
        settings: &ConversationSettings,
        project_id: Option<Uuid>,
    ) -> Result<Conversation, AppError> {
        let title = clean_title(title.unwrap_or(DEFAULT_TITLE))?;
        if !model.is_empty() {
            validate_model(model)?;
        }
        settings.validate()?;
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let project_id = project_id.map(|id| id.to_string());
        if let Some(project) = &project_id {
            super::projects::check_exists(&tx, project)?;
        }
        let conversation = tx.query_row(
            &format!(
                "INSERT INTO conversations ({COLUMNS})
                 VALUES (?1, ?2, ?3, {NOW}, {NOW}, ?4, ?5, ?6, ?7, ?8, ?9)
                 RETURNING {COLUMNS}"
            ),
            params![
                Uuid::new_v4().to_string(),
                title,
                model,
                settings.system_prompt.expose(),
                settings.options.temperature.map(f64::from),
                settings.options.num_ctx,
                think_column(&settings.think),
                settings.charts,
                project_id
            ],
            conversation_row,
        )?;
        tx.commit()?;
        Ok(conversation)
    }

    pub fn conversation_settings(&self, id: Uuid) -> Result<ConversationSettings, AppError> {
        load_settings(&self.conn(), &id.to_string())
    }

    /// Applies from the next send. Like renaming, not counted as use.
    pub fn update_settings(
        &self,
        id: Uuid,
        settings: &ConversationSettings,
    ) -> Result<Conversation, AppError> {
        settings.validate()?;
        let conn = self.conn();
        conn.query_row(
            &format!(
                "UPDATE conversations
                 SET system_prompt = ?2, temperature = ?3, num_ctx = ?4, think = ?5,
                     charts = ?6
                 WHERE id = ?1 RETURNING {COLUMNS}"
            ),
            params![
                id.to_string(),
                settings.system_prompt.expose(),
                settings.options.temperature.map(f64::from),
                settings.options.num_ctx,
                think_column(&settings.think),
                settings.charts
            ],
            conversation_row,
        )
        .optional()?
        .ok_or_else(not_found)
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

    /// The conversations whose shown messages contain `query`, ignoring
    /// case, most recently used first, each once. Titles are matched by the
    /// sidebar, and thinking is never searched. Only ids come back, never
    /// message text.
    ///
    /// Matched in Rust: SQLite's `LIKE` folds ASCII only, so `äiti` would
    /// miss `Äiti`. Like a `LIKE` scan, this reads every shown message of
    /// the conversations it looks at.
    pub fn search_conversations(&self, query: &str) -> Result<Vec<Uuid>, AppError> {
        let query = query.trim();
        if query.chars().count() > MAX_SEARCH_CHARS {
            return Err(AppError::InvalidInput(format!(
                "a search is at most {MAX_SEARCH_CHARS} characters"
            )));
        }
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let needle = query.to_lowercase();
        let conn = self.conn();
        let ids: Vec<Uuid> = conn
            .prepare("SELECT id FROM conversations ORDER BY updated_at DESC, rowid DESC")?
            .query_map([], |row| uuid_column(row, 0))?
            .collect::<Result<_, _>>()?;
        let mut contents = conn.prepare(
            "SELECT content FROM messages WHERE conversation_id = ?1 AND superseded_by IS NULL",
        )?;
        let mut found = Vec::new();
        for id in ids {
            let mut rows = contents.query([id.to_string()])?;
            while let Some(row) = rows.next()? {
                let content: String = row.get(0)?;
                if content.to_lowercase().contains(&needle) {
                    found.push(id);
                    break;
                }
            }
            if found.len() == SEARCH_LIMIT {
                break;
            }
        }
        Ok(found)
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

    fn settings(
        prompt: &str,
        temperature: Option<f32>,
        think: ThinkChoice,
    ) -> ConversationSettings {
        ConversationSettings {
            system_prompt: Redacted::new(prompt),
            options: ChatOptions {
                temperature,
                num_ctx: Some(8192),
                ..ChatOptions::default()
            },
            think,
            charts: false,
        }
    }

    #[test]
    fn settings_round_trip() {
        let (storage, _dir) = temp_storage();
        let plain = storage.create_conversation(None, "m").unwrap();
        assert_eq!(plain.settings, ConversationSettings::default());
        assert_eq!(plain.settings.options(), None);

        let first = settings("Answer only in Finnish", Some(0.7), ThinkChoice::On);
        let created = storage
            .create_conversation_with(Some("t"), "m", &first, None)
            .unwrap();
        assert_eq!(created.settings, first);
        assert_eq!(
            storage.get_conversation(created.id).unwrap().conversation,
            created
        );

        let second = ConversationSettings {
            charts: true,
            ..settings("", None, ThinkChoice::Level("high".into()))
        };
        let updated = storage.update_settings(created.id, &second).unwrap();
        assert_eq!(updated.settings, second);
        assert_eq!(updated.updated_at, created.updated_at);
        assert_eq!(storage.conversation_settings(created.id).unwrap(), second);
        assert_eq!(
            storage.list_conversations().unwrap()[0].settings,
            second,
            "the list carries the settings too"
        );
    }

    #[test]
    fn settings_validation_rejects_out_of_range() {
        let (storage, _dir) = temp_storage();
        let id = storage.create_conversation(None, "m").unwrap().id;
        let with_options = |options: ChatOptions| ConversationSettings {
            options,
            ..ConversationSettings::default()
        };
        let bad = [
            settings("", Some(2.5), ThinkChoice::Off),
            settings("", Some(-0.1), ThinkChoice::Off),
            with_options(ChatOptions {
                num_ctx: Some(100),
                ..ChatOptions::default()
            }),
            with_options(ChatOptions {
                num_ctx: Some(200_000),
                ..ChatOptions::default()
            }),
            with_options(ChatOptions {
                num_predict: Some(10),
                ..ChatOptions::default()
            }),
            settings(
                &"x".repeat(MAX_SYSTEM_PROMPT_BYTES + 1),
                None,
                ThinkChoice::Off,
            ),
            settings("a\0b", None, ThinkChoice::Off),
            settings("", None, ThinkChoice::Level(String::new())),
        ];
        for settings in &bad {
            assert!(matches!(
                storage.update_settings(id, settings),
                Err(AppError::InvalidInput(_))
            ));
            assert!(matches!(
                storage.create_conversation_with(None, "m", settings, None),
                Err(AppError::InvalidInput(_))
            ));
        }
        assert_eq!(
            storage.conversation_settings(id).unwrap(),
            ConversationSettings::default()
        );
        assert_eq!(storage.list_conversations().unwrap().len(), 1);
        let longest = settings(
            &"x".repeat(MAX_SYSTEM_PROMPT_BYTES),
            Some(2.0),
            ThinkChoice::Off,
        );
        assert!(storage.update_settings(id, &longest).is_ok());
    }

    #[test]
    fn update_settings_unknown_is_not_found() {
        let (storage, _dir) = temp_storage();
        let id = Uuid::new_v4();
        assert!(matches!(
            storage.update_settings(id, &ConversationSettings::default()),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            storage.conversation_settings(id),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn migrated_conversation_sends_think_false_to_boolean_model() {
        use crate::ollama::types::{ModelCapabilities, Think, ThinkStyle, resolve_think};

        let (storage, _dir) = temp_storage();
        // What a row written before the settings columns existed holds.
        storage
            .conn()
            .execute(
                "INSERT INTO conversations (id, title, model, created_at, updated_at)
                 VALUES (?1, 'old', 'qwen3', 'x', 'x')",
                [Uuid::nil().to_string()],
            )
            .unwrap();
        let saved = storage.conversation_settings(Uuid::nil()).unwrap();
        // Charts too start off.
        assert_eq!(saved, ConversationSettings::default());
        assert!(!saved.charts);

        let qwen3 = ModelCapabilities {
            thinking: true,
            vision: false,
            tools: true,
            embedding_only: false,
            think_style: ThinkStyle::Bool,
        };
        assert_eq!(
            resolve_think(&qwen3, saved.think.requested()),
            Ok(Some(Think::Enabled(false)))
        );
    }

    /// A message row with `content`; `extra` sets other columns.
    fn insert_message(storage: &Storage, conversation: Uuid, content: &str, extra: &str) {
        storage
            .conn()
            .execute(
                "INSERT INTO messages (id, conversation_id, role, content, created_at, status)
                 VALUES (?1, ?2, 'user', ?3, 'x', 'complete')",
                params![
                    Uuid::new_v4().to_string(),
                    conversation.to_string(),
                    content
                ],
            )
            .unwrap();
        if !extra.is_empty() {
            storage
                .conn()
                .execute(
                    &format!("UPDATE messages SET {extra} WHERE rowid = last_insert_rowid()"),
                    [],
                )
                .unwrap();
        }
    }

    fn chat_with(storage: &Storage, title: &str, contents: &[&str]) -> Uuid {
        let id = storage.create_conversation(Some(title), "m").unwrap().id;
        for content in contents {
            insert_message(storage, id, content, "");
        }
        id
    }

    #[test]
    fn search_ignores_case_beyond_ascii() {
        let (storage, _dir) = temp_storage();
        let finnish = chat_with(&storage, "a", &["Äiti soitti"]);
        let greek = chat_with(&storage, "b", &["ΣΟΦΙΑ"]);
        assert_eq!(storage.search_conversations("äITI").unwrap(), [finnish]);
        assert_eq!(storage.search_conversations("σοφια").unwrap(), [greek]);
    }

    #[test]
    fn search_matches_wildcard_characters_literally() {
        let (storage, _dir) = temp_storage();
        let percent = chat_with(&storage, "a", &["a 50% discount"]);
        let plain = chat_with(&storage, "b", &["a 500 discount"]);
        let underscore = chat_with(&storage, "c", &["snake_case"]);
        let backslash = chat_with(&storage, "d", &[r"C:\temp"]);
        chat_with(&storage, "e", &["snakeXcase", "C:temp"]);

        assert_eq!(storage.search_conversations("50%").unwrap(), [percent]);
        assert_eq!(storage.search_conversations("e_c").unwrap(), [underscore]);
        assert_eq!(storage.search_conversations(r":\t").unwrap(), [backslash]);
        assert_eq!(
            storage.search_conversations("  DISCOUNT ").unwrap(),
            [plain, percent],
            "trimmed, ASCII case-insensitive"
        );
    }

    #[test]
    fn search_matches_content_once() {
        let (storage, _dir) = temp_storage();
        let many = chat_with(&storage, "x", &["kiwi"; 5]);
        assert_eq!(storage.search_conversations("kiwi").unwrap(), [many]);
    }

    #[test]
    fn search_matches_content_not_title() {
        let (storage, _dir) = temp_storage();
        chat_with(&storage, "Banana bread", &["flour"]);
        assert!(storage.search_conversations("banana").unwrap().is_empty());
    }

    #[test]
    fn search_orders_newest_first_and_limits() {
        let (storage, _dir) = temp_storage();
        let ids: Vec<Uuid> = (0..=SEARCH_LIMIT)
            .map(|_| chat_with(&storage, "t", &["needle"]))
            .collect();
        let oldest = ids[0];
        storage
            .conn()
            .execute(
                "UPDATE conversations SET updated_at = '9999' WHERE id = ?1",
                [oldest.to_string()],
            )
            .unwrap();

        let found = storage.search_conversations("needle").unwrap();
        assert_eq!(found.len(), SEARCH_LIMIT);
        assert_eq!(found[0], oldest);
        // Same `updated_at` otherwise: newest insert first, the next-oldest cut.
        assert_eq!(found[1], ids[SEARCH_LIMIT]);
        assert!(!found.contains(&ids[1]));
    }

    #[test]
    fn search_skips_superseded_and_thinking() {
        let (storage, _dir) = temp_storage();
        let id = chat_with(&storage, "t", &[]);
        insert_message(&storage, id, "replaced text", "superseded_by = 'other'");
        insert_message(&storage, id, "shown", "thinking = 'secret reasoning'");

        assert!(storage.search_conversations("replaced").unwrap().is_empty());
        assert!(
            storage
                .search_conversations("reasoning")
                .unwrap()
                .is_empty()
        );
        assert_eq!(storage.search_conversations("shown").unwrap(), [id]);
    }

    #[test]
    fn search_empty_and_long_queries() {
        let (storage, _dir) = temp_storage();
        chat_with(&storage, "t", &["anything"]);
        assert!(storage.search_conversations("  ").unwrap().is_empty());
        assert!(
            storage
                .search_conversations(&"a".repeat(MAX_SEARCH_CHARS))
                .is_ok()
        );
        assert!(matches!(
            storage.search_conversations(&"a".repeat(MAX_SEARCH_CHARS + 1)),
            Err(AppError::InvalidInput(_))
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

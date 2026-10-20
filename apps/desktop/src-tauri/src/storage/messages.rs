//! Messages: starting a turn, and saving a streamed reply as it arrives.

use std::sync::Arc;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use uuid::Uuid;

use super::Storage;
use super::conversations::{NOW, not_found, uuid_column};
use crate::error::AppError;
use crate::ollama::types::{ChatMessage, ChatRole};
use crate::redacted::Redacted;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    /// The reply is still streaming, or the app died while it was.
    Pending,
    Complete,
    Cancelled,
    Error,
}

impl MessageStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Complete => "complete",
            Self::Cancelled => "cancelled",
            Self::Error => "error",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "complete" => Some(Self::Complete),
            "cancelled" => Some(Self::Cancelled),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredMessage {
    pub id: Uuid,
    pub role: ChatRole,
    pub content: Redacted,
    pub status: MessageStatus,
    pub created_at: String,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub duration_ms: Option<u64>,
}

fn bad_text(index: usize, what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        format!("unknown {what}").into(),
    )
}

fn count_column(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    Ok(row
        .get::<_, Option<i64>>(index)?
        .and_then(|value| u64::try_from(value).ok()))
}

fn to_sql_count(value: Option<u64>) -> Option<i64> {
    value.and_then(|value| i64::try_from(value).ok())
}

fn message_row(row: &Row<'_>) -> rusqlite::Result<StoredMessage> {
    let role: String = row.get(1)?;
    let status: String = row.get(3)?;
    Ok(StoredMessage {
        id: uuid_column(row, 0)?,
        role: ChatRole::parse(&role).ok_or_else(|| bad_text(1, "role"))?,
        content: Redacted::new(row.get::<_, String>(2)?),
        status: MessageStatus::parse(&status).ok_or_else(|| bad_text(3, "status"))?,
        created_at: row.get(4)?,
        prompt_tokens: count_column(row, 5)?,
        completion_tokens: count_column(row, 6)?,
        duration_ms: count_column(row, 7)?,
    })
}

/// Oldest first. Rows inserted in the same millisecond keep insert order.
pub(super) fn load_messages(
    conn: &Connection,
    conversation_id: Uuid,
) -> Result<Vec<StoredMessage>, AppError> {
    let mut statement = conn.prepare(
        "SELECT id, role, content, status, created_at, prompt_tokens, completion_tokens,
                duration_ms
         FROM messages WHERE conversation_id = ?1 ORDER BY created_at, rowid",
    )?;
    let messages = statement
        .query_map([conversation_id.to_string()], message_row)?
        .collect::<Result<_, _>>()?;
    Ok(messages)
}

/// A turn that was saved and is ready to stream.
#[derive(Debug)]
pub struct Turn {
    /// The `pending` assistant row the reply goes into.
    pub assistant_id: Uuid,
    /// Everything Ollama gets: earlier turns plus the new prompt.
    pub history: Vec<ChatMessage>,
}

impl Storage {
    /// In one transaction: reads the history, lets `check` refuse it (size
    /// limits) before anything is written, then inserts the user message and
    /// an empty `pending` assistant row, and marks the conversation used.
    ///
    /// History is every `complete` message plus the visible part of a
    /// `cancelled` reply; failed and unfinished replies are left out.
    pub fn begin_turn(
        &self,
        conversation_id: Uuid,
        model: &str,
        prompt: &Redacted,
        check: impl FnOnce(&[ChatMessage]) -> Result<(), AppError>,
    ) -> Result<Turn, AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let id = conversation_id.to_string();
        tx.query_row("SELECT 1 FROM conversations WHERE id = ?1", [&id], |_| {
            Ok(())
        })
        .optional()?
        .ok_or_else(not_found)?;

        let mut history: Vec<ChatMessage> = {
            let mut statement = tx.prepare(
                "SELECT role, content FROM messages
                 WHERE conversation_id = ?1
                   AND (status = 'complete' OR (status = 'cancelled' AND content <> ''))
                 ORDER BY created_at, rowid",
            )?;
            statement
                .query_map([&id], |row| {
                    let role: String = row.get(0)?;
                    Ok(ChatMessage {
                        role: ChatRole::parse(&role).ok_or_else(|| bad_text(0, "role"))?,
                        content: Redacted::new(row.get::<_, String>(1)?),
                    })
                })?
                .collect::<Result<_, _>>()?
        };
        history.push(ChatMessage {
            role: ChatRole::User,
            content: prompt.clone(),
        });
        check(&history)?;

        let assistant_id = Uuid::new_v4();
        let insert = format!(
            "INSERT INTO messages (id, conversation_id, role, content, created_at, status)
             VALUES (?1, ?2, ?3, ?4, {NOW}, ?5)"
        );
        tx.execute(
            &insert,
            params![
                Uuid::new_v4().to_string(),
                id,
                ChatRole::User.as_str(),
                prompt.expose(),
                MessageStatus::Complete.as_str()
            ],
        )?;
        tx.execute(
            &insert,
            params![
                assistant_id.to_string(),
                id,
                ChatRole::Assistant.as_str(),
                "",
                MessageStatus::Pending.as_str()
            ],
        )?;
        tx.execute(
            &format!("UPDATE conversations SET model = ?2, updated_at = {NOW} WHERE id = ?1"),
            params![id, model],
        )?;
        tx.commit()?;
        Ok(Turn {
            assistant_id,
            history,
        })
    }

    /// Only a `pending` row changes, so a late write can never undo a final
    /// status. A deleted row is silently skipped.
    fn save_pending(&self, id: Uuid, content: &str) -> Result<(), AppError> {
        self.conn().execute(
            "UPDATE messages SET content = ?2 WHERE id = ?1 AND status = 'pending'",
            params![id.to_string(), content],
        )?;
        Ok(())
    }

    fn save_final(&self, id: Uuid, content: &str, end: &FinalState) -> Result<(), AppError> {
        self.conn().execute(
            "UPDATE messages
             SET content = ?2, status = ?3, prompt_tokens = ?4, completion_tokens = ?5,
                 duration_ms = ?6
             WHERE id = ?1 AND status = 'pending'",
            params![
                id.to_string(),
                content,
                end.status.as_str(),
                to_sql_count(end.prompt_tokens),
                to_sql_count(end.completion_tokens),
                to_sql_count(end.duration_ms)
            ],
        )?;
        Ok(())
    }
}

/// When a streamed reply is written to disk while it streams.
#[derive(Debug, Clone, Copy)]
pub struct FlushPolicy {
    /// The ticker's interval: buffered tokens are on disk within this time.
    pub every: Duration,
    /// Write as soon as this many tokens are buffered.
    pub tokens: usize,
}

impl Default for FlushPolicy {
    fn default() -> Self {
        Self {
            every: Duration::from_millis(500),
            tokens: 64,
        }
    }
}

#[derive(Debug)]
struct FinalState {
    status: MessageStatus,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    duration_ms: Option<u64>,
}

impl FinalState {
    fn new(status: MessageStatus) -> Self {
        Self {
            status,
            prompt_tokens: None,
            completion_tokens: None,
            duration_ms: None,
        }
    }
}

/// Saves one streamed reply into its `pending` row, so a hard kill keeps what
/// had arrived. No `Debug`: it holds the reply text.
pub struct TurnRecorder {
    storage: Arc<Storage>,
    message_id: Uuid,
    content: String,
    unflushed: usize,
    policy: FlushPolicy,
    /// A write failed: the turn ends as `error` whatever the stream does.
    failed: bool,
    finished: bool,
}

impl TurnRecorder {
    pub fn new(storage: Arc<Storage>, message_id: Uuid, policy: FlushPolicy) -> Self {
        Self {
            storage,
            message_id,
            content: String::new(),
            unflushed: 0,
            policy,
            failed: false,
            finished: false,
        }
    }

    pub fn policy(&self) -> FlushPolicy {
        self.policy
    }

    /// Buffers a token and writes once [`FlushPolicy::tokens`] are waiting.
    /// After a failed write every call fails, so the stream stops.
    pub fn push(&mut self, token: &Redacted) -> Result<(), AppError> {
        self.check_failed()?;
        if self.finished {
            return Ok(());
        }
        self.content.push_str(token.expose());
        self.unflushed += 1;
        if self.unflushed >= self.policy.tokens {
            self.flush()?;
        }
        Ok(())
    }

    /// Writes buffered tokens, if any. Called by the ticker.
    pub fn flush_if_dirty(&mut self) -> Result<(), AppError> {
        self.check_failed()?;
        if self.finished || self.unflushed == 0 {
            return Ok(());
        }
        self.flush()
    }

    /// Ends the turn as `complete`, or as `error` if an earlier write failed.
    pub fn complete(
        &mut self,
        prompt_tokens: Option<u64>,
        completion_tokens: Option<u64>,
        duration_ms: Option<u64>,
    ) -> Result<(), AppError> {
        self.finish_with(FinalState {
            status: MessageStatus::Complete,
            prompt_tokens,
            completion_tokens,
            duration_ms,
        })
    }

    /// Ends the turn as `cancelled` or `error`, keeping the partial reply.
    pub fn finish(&mut self, status: MessageStatus) -> Result<(), AppError> {
        self.finish_with(FinalState::new(status))
    }

    /// After an earlier failed write the row becomes `error` and this still
    /// fails, so the caller reports the failure instead of success.
    fn finish_with(&mut self, end: FinalState) -> Result<(), AppError> {
        if self.finished {
            return Ok(());
        }
        let end = if self.failed {
            FinalState::new(MessageStatus::Error)
        } else {
            end
        };
        if let Err(err) = self
            .storage
            .save_final(self.message_id, &self.content, &end)
        {
            self.failed = true;
            return Err(err);
        }
        self.finished = true;
        self.check_failed()
    }

    fn flush(&mut self) -> Result<(), AppError> {
        match self.storage.save_pending(self.message_id, &self.content) {
            Ok(()) => {
                self.unflushed = 0;
                Ok(())
            }
            Err(err) => {
                self.failed = true;
                Err(err)
            }
        }
    }

    /// Fails once any write for this turn has failed.
    pub fn ensure_saved(&self) -> Result<(), AppError> {
        self.check_failed()
    }

    fn check_failed(&self) -> Result<(), AppError> {
        if self.failed {
            Err(AppError::Database("the reply could not be saved".into()))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::testing::temp_storage;

    fn conversation(storage: &Storage) -> Uuid {
        storage.create_conversation(None, "m").unwrap().id
    }

    fn turn(storage: &Storage, conversation_id: Uuid, prompt: &str) -> Turn {
        storage
            .begin_turn(conversation_id, "llama3.2", &Redacted::new(prompt), |_| {
                Ok(())
            })
            .unwrap()
    }

    fn message(storage: &Storage, conversation_id: Uuid, id: Uuid) -> StoredMessage {
        storage
            .get_conversation(conversation_id)
            .unwrap()
            .messages
            .into_iter()
            .find(|message| message.id == id)
            .unwrap()
    }

    fn token(text: &str) -> Redacted {
        Redacted::new(text)
    }

    fn texts(history: &[ChatMessage]) -> Vec<(ChatRole, &str)> {
        history
            .iter()
            .map(|message| (message.role, message.content.expose()))
            .collect()
    }

    #[test]
    fn begin_turn_inserts_user_and_pending_rows() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        let before = storage.get_conversation(id).unwrap().conversation;

        let turn = turn(&storage, id, "Hi");
        assert_eq!(texts(&turn.history), [(ChatRole::User, "Hi")]);

        let saved = storage.get_conversation(id).unwrap();
        let rows: Vec<(ChatRole, &str, MessageStatus)> = saved
            .messages
            .iter()
            .map(|m| (m.role, m.content.expose(), m.status))
            .collect();
        assert_eq!(
            rows,
            [
                (ChatRole::User, "Hi", MessageStatus::Complete),
                (ChatRole::Assistant, "", MessageStatus::Pending)
            ]
        );
        assert_eq!(saved.messages[1].id, turn.assistant_id);
        assert_eq!(saved.conversation.model, "llama3.2");
        assert!(saved.conversation.updated_at >= before.updated_at);
    }

    #[test]
    fn history_skips_error_pending_and_empty_cancelled() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        let policy = FlushPolicy::default();

        let done = turn(&storage, id, "One");
        let mut recorder = TurnRecorder::new(Arc::clone(&storage), done.assistant_id, policy);
        recorder.push(&token("Reply")).unwrap();
        recorder.complete(None, None, None).unwrap();

        let stopped = turn(&storage, id, "Two");
        let mut recorder = TurnRecorder::new(Arc::clone(&storage), stopped.assistant_id, policy);
        recorder.push(&token("Half")).unwrap();
        recorder.finish(MessageStatus::Cancelled).unwrap();

        let empty = turn(&storage, id, "Three");
        let mut recorder = TurnRecorder::new(Arc::clone(&storage), empty.assistant_id, policy);
        recorder.finish(MessageStatus::Cancelled).unwrap();

        let failed = turn(&storage, id, "Four");
        let mut recorder = TurnRecorder::new(Arc::clone(&storage), failed.assistant_id, policy);
        recorder.push(&token("oops")).unwrap();
        recorder.finish(MessageStatus::Error).unwrap();

        let _pending = turn(&storage, id, "Five");
        let last = turn(&storage, id, "Six");
        assert_eq!(
            texts(&last.history),
            [
                (ChatRole::User, "One"),
                (ChatRole::Assistant, "Reply"),
                (ChatRole::User, "Two"),
                (ChatRole::Assistant, "Half"),
                (ChatRole::User, "Three"),
                (ChatRole::User, "Four"),
                (ChatRole::User, "Five"),
                (ChatRole::User, "Six"),
            ]
        );
    }

    #[test]
    fn begin_turn_check_failure_inserts_nothing() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        let before = storage.get_conversation(id).unwrap();

        let err = storage
            .begin_turn(id, "other", &Redacted::new("Hi"), |history| {
                assert_eq!(history.len(), 1);
                Err(AppError::TooLarge("too big".into()))
            })
            .unwrap_err();
        assert!(matches!(err, AppError::TooLarge(_)));
        assert_eq!(storage.get_conversation(id).unwrap(), before);
    }

    #[test]
    fn begin_turn_unknown_conversation_is_not_found() {
        let (storage, _dir) = temp_storage();
        let err = storage
            .begin_turn(Uuid::new_v4(), "m", &Redacted::new("Hi"), |_| Ok(()))
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn recorder_done_marks_complete_with_counts() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        let turn = turn(&storage, id, "Hi");
        let mut recorder = TurnRecorder::new(
            Arc::clone(&storage),
            turn.assistant_id,
            FlushPolicy::default(),
        );

        recorder.push(&token("Hel")).unwrap();
        recorder.push(&token("lo")).unwrap();
        recorder.complete(Some(3), Some(2), Some(120)).unwrap();

        let saved = message(&storage, id, turn.assistant_id);
        assert_eq!(saved.content.expose(), "Hello");
        assert_eq!(saved.status, MessageStatus::Complete);
        assert_eq!(saved.prompt_tokens, Some(3));
        assert_eq!(saved.completion_tokens, Some(2));
        assert_eq!(saved.duration_ms, Some(120));
    }

    #[test]
    fn cancel_stores_partial_content_as_cancelled() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        let turn = turn(&storage, id, "Hi");
        let mut recorder = TurnRecorder::new(
            Arc::clone(&storage),
            turn.assistant_id,
            FlushPolicy::default(),
        );

        recorder.push(&token("Par")).unwrap();
        recorder.finish(MessageStatus::Cancelled).unwrap();

        let saved = message(&storage, id, turn.assistant_id);
        assert_eq!(saved.content.expose(), "Par");
        assert_eq!(saved.status, MessageStatus::Cancelled);
    }

    #[test]
    fn recorder_error_keeps_partial_content() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        let turn = turn(&storage, id, "Hi");
        let mut recorder = TurnRecorder::new(
            Arc::clone(&storage),
            turn.assistant_id,
            FlushPolicy::default(),
        );

        recorder.push(&token("Some")).unwrap();
        recorder.finish(MessageStatus::Error).unwrap();

        let saved = message(&storage, id, turn.assistant_id);
        assert_eq!(saved.content.expose(), "Some");
        assert_eq!(saved.status, MessageStatus::Error);
    }

    #[test]
    fn dropped_stream_keeps_flushed_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let keys = crate::storage::key::testing::MemoryKeySource::default();
        let storage = Arc::new(super::super::open_storage(dir.path(), &keys).unwrap());
        let id = conversation(&storage);
        let turn = turn(&storage, id, "Hi");
        let policy = FlushPolicy {
            every: Duration::from_secs(60),
            tokens: 3,
        };
        let mut recorder = TurnRecorder::new(Arc::clone(&storage), turn.assistant_id, policy);
        for text in ["a", "b", "c", "d"] {
            recorder.push(&token(text)).unwrap();
        }
        // A hard kill: no finish, nothing else written.
        drop(recorder);
        drop(storage);

        let storage = super::super::open_storage(dir.path(), &keys).unwrap();
        let saved = message(&storage, id, turn.assistant_id);
        assert_eq!(saved.content.expose(), "abc");
        assert_eq!(saved.status, MessageStatus::Error);
    }

    #[test]
    fn flush_if_dirty_persists_short_reply() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        let turn = turn(&storage, id, "Hi");
        let mut recorder = TurnRecorder::new(
            Arc::clone(&storage),
            turn.assistant_id,
            FlushPolicy::default(),
        );

        recorder.push(&token("Hi")).unwrap();
        recorder.push(&token("!")).unwrap();
        assert_eq!(
            message(&storage, id, turn.assistant_id).content.expose(),
            ""
        );

        recorder.flush_if_dirty().unwrap();
        let saved = message(&storage, id, turn.assistant_id);
        assert_eq!(saved.content.expose(), "Hi!");
        assert_eq!(saved.status, MessageStatus::Pending);
    }

    #[test]
    fn finished_recorder_ignores_later_calls() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        let turn = turn(&storage, id, "Hi");
        let mut recorder = TurnRecorder::new(
            Arc::clone(&storage),
            turn.assistant_id,
            FlushPolicy::default(),
        );

        recorder.push(&token("Done")).unwrap();
        recorder.complete(None, Some(1), None).unwrap();
        recorder.push(&token("late")).unwrap();
        recorder.flush_if_dirty().unwrap();
        recorder.finish(MessageStatus::Cancelled).unwrap();

        let saved = message(&storage, id, turn.assistant_id);
        assert_eq!(saved.content.expose(), "Done");
        assert_eq!(saved.status, MessageStatus::Complete);
    }

    #[test]
    fn update_after_delete_is_ignored() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        let turn = turn(&storage, id, "Hi");
        let mut recorder = TurnRecorder::new(
            Arc::clone(&storage),
            turn.assistant_id,
            FlushPolicy::default(),
        );

        storage.delete_conversation(id).unwrap();
        recorder.push(&token("x")).unwrap();
        recorder.flush_if_dirty().unwrap();
        recorder.complete(None, None, None).unwrap();
        assert!(storage.list_conversations().unwrap().is_empty());
    }

    #[test]
    fn delete_cascades_messages() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        turn(&storage, id, "Hi");

        storage.delete_conversation(id).unwrap();
        let count: i64 = storage
            .conn()
            .query_row("SELECT count(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}

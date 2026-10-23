//! Messages: starting a turn, and saving a streamed reply as it arrives.

use std::sync::Arc;
use std::time::Duration;

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use serde::Serialize;
use uuid::Uuid;

use super::Storage;
use super::conversations::{ConversationSettings, NOW, load_settings, uuid_column};
use crate::error::AppError;
use crate::ollama::chart_tool::{ChartEntry, replay};
use crate::ollama::types::{ChatMessage, ChatOptions, ChatRole};
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

#[derive(Debug, Clone, PartialEq, Serialize)]
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
    /// The model's reasoning; shown collapsed, never part of the history.
    pub thinking: Option<Redacted>,
    pub thinking_ms: Option<u64>,
    /// Generating the completion tokens only; `None` for replies saved
    /// before it was recorded.
    pub eval_ms: Option<u64>,
    /// The model that wrote a reply; `None` for prompts and for replies
    /// saved before it was recorded.
    pub model: Option<String>,
    /// A compared prompt and its candidate replies, until one is picked.
    pub compare_group: Option<Uuid>,
    /// A reply's `render_chart` calls; empty for most.
    pub charts: Vec<ChartEntry>,
}

/// The `charts` column: JSON, `NULL` when there are none.
pub(super) fn charts_column(row: &Row<'_>, index: usize) -> rusqlite::Result<Vec<ChartEntry>> {
    row.get::<_, Option<String>>(index)?
        .map(|json| serde_json::from_str(&json).map_err(|_| bad_text(index, "charts")))
        .transpose()
        .map(Option::unwrap_or_default)
}

fn charts_json(charts: &[ChartEntry]) -> Result<Option<String>, AppError> {
    if charts.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(charts)
        .map(Some)
        .map_err(|err| AppError::Internal(format!("charts: {err}")))
}

pub(super) fn bad_text(index: usize, what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        format!("unknown {what}").into(),
    )
}

pub(super) fn count_column(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    Ok(row
        .get::<_, Option<i64>>(index)?
        .and_then(|value| u64::try_from(value).ok()))
}

pub(super) fn to_sql_count(value: Option<u64>) -> Option<i64> {
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
        thinking: row
            .get::<_, Option<String>>(8)?
            .filter(|text| !text.is_empty())
            .map(Redacted::new),
        thinking_ms: count_column(row, 9)?,
        eval_ms: count_column(row, 10)?,
        model: row.get(11)?,
        compare_group: row
            .get::<_, Option<String>>(12)?
            .map(|group| Uuid::parse_str(&group).map_err(|_| bad_text(12, "compare group")))
            .transpose()?,
        charts: charts_column(row, 13)?,
    })
}

/// `NULL` rather than an empty string for a reply with no thinking.
fn thinking_column(thinking: &str) -> Option<&str> {
    (!thinking.is_empty()).then_some(thinking)
}

/// The rows a conversation shows: not replaced by a running turn.
const LIVE: &str = "conversation_id = ?1 AND superseded_by IS NULL";

/// The rows the conversation's turns are made of: shown, and not part of a
/// comparison still waiting for a pick.
pub(super) const SETTLED: &str =
    "conversation_id = ?1 AND superseded_by IS NULL AND compare_group IS NULL";

/// Oldest first. Rows inserted in the same millisecond keep insert order.
/// Rows a running turn replaces are left out; an unpicked comparison is
/// included, marked with its group.
pub(super) fn load_messages(
    conn: &Connection,
    conversation_id: Uuid,
) -> Result<Vec<StoredMessage>, AppError> {
    let mut statement = conn.prepare(&format!(
        "SELECT id, role, content, status, created_at, prompt_tokens, completion_tokens,
                duration_ms, thinking, thinking_ms, eval_ms, model, compare_group, charts
         FROM messages WHERE {LIVE} ORDER BY created_at, rowid"
    ))?;
    let messages = statement
        .query_map([conversation_id.to_string()], message_row)?
        .collect::<Result<_, _>>()?;
    Ok(messages)
}

/// Undoes a turn that replaced rows: deletes what it added (every live row
/// after the first one it replaced) and shows the replaced rows again. A
/// turn that replaced nothing is left as it is.
pub(super) fn restore_superseded(conn: &Connection, assistant_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM messages
         WHERE superseded_by IS NULL
           AND conversation_id =
               (SELECT conversation_id FROM messages WHERE superseded_by = ?1 LIMIT 1)
           AND (created_at, rowid) >
               (SELECT created_at, rowid FROM messages WHERE superseded_by = ?1
                ORDER BY created_at, rowid LIMIT 1)",
        [assistant_id],
    )?;
    conn.execute(
        "UPDATE messages SET superseded_by = NULL WHERE superseded_by = ?1",
        [assistant_id],
    )?;
    Ok(())
}

/// What a turn sends.
#[derive(Debug, Clone)]
pub enum TurnInput {
    /// A new prompt after the last message.
    New(Redacted),
    /// A new reply to the last prompt, replacing everything after it.
    Regenerate,
    /// The user message `message_id` and everything after it, replaced by
    /// `prompt`.
    Edit { message_id: Uuid, prompt: Redacted },
}

impl TurnInput {
    /// The new prompt, for the inputs that have one.
    pub fn prompt(&self) -> Option<&Redacted> {
        match self {
            Self::New(prompt) | Self::Edit { prompt, .. } => Some(prompt),
            Self::Regenerate => None,
        }
    }
}

/// Refuses a new turn while the conversation has a comparison waiting for a
/// pick: its next history depends on which answer is kept.
pub(super) fn check_no_comparison(
    conn: &Connection,
    conversation_id: &str,
) -> Result<(), AppError> {
    let open: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM messages
                        WHERE conversation_id = ?1 AND compare_group IS NOT NULL)",
        [conversation_id],
        |row| row.get(0),
    )?;
    if open {
        return Err(AppError::InvalidInput(
            "pick an answer or discard the comparison first".into(),
        ));
    }
    Ok(())
}

/// A row's place in its conversation, in `(created_at, rowid)` order.
pub(super) struct Position {
    id: Uuid,
    created_at: String,
    rowid: i64,
}

fn position_row(row: &Row<'_>) -> rusqlite::Result<Position> {
    Ok(Position {
        id: uuid_column(row, 0)?,
        created_at: row.get(1)?,
        rowid: row.get(2)?,
    })
}

/// Every `complete` message plus the visible part of a `cancelled` reply;
/// failed and unfinished replies are left out, and so is an unpicked
/// comparison. Role and content only, with a charted reply replayed as its
/// calls and their results: thinking is never sent back to the model. With
/// `until`, only the rows before that position (`<`) or up to it (`<=`).
pub(super) fn load_history(
    conn: &Connection,
    conversation_id: &str,
    until: Option<(&Position, &str)>,
) -> rusqlite::Result<Vec<ChatMessage>> {
    let mut sql = format!(
        "SELECT role, content, charts FROM messages
         WHERE {SETTLED} AND (status = 'complete' OR (status = 'cancelled' AND content <> ''))"
    );
    let mut args: Vec<Value> = vec![conversation_id.to_owned().into()];
    if let Some((position, op)) = until {
        sql.push_str(&format!(" AND (created_at, rowid) {op} (?2, ?3)"));
        args.push(position.created_at.clone().into());
        args.push(position.rowid.into());
    }
    sql.push_str(" ORDER BY created_at, rowid");
    let mut statement = conn.prepare(&sql)?;
    let rows = statement
        .query_map(params_from_iter(args), |row| {
            let role: String = row.get(0)?;
            let role = ChatRole::parse(&role).ok_or_else(|| bad_text(0, "role"))?;
            let content: String = row.get(1)?;
            let charts = charts_column(row, 2)?;
            Ok(if role == ChatRole::Assistant && !charts.is_empty() {
                replay(&content, &charts)
            } else {
                vec![ChatMessage::new(role, Redacted::new(content))]
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows.into_iter().flatten().collect())
}

/// A turn that was saved and is ready to stream.
#[derive(Debug)]
pub struct Turn {
    /// The `pending` assistant row the reply goes into.
    pub assistant_id: Uuid,
    /// The prompt the reply answers: new, or the existing one when
    /// regenerating.
    pub user_id: Uuid,
    /// Everything Ollama gets: the system prompt, earlier turns and the
    /// prompt.
    pub history: Vec<ChatMessage>,
    /// From the conversation's settings.
    pub options: Option<ChatOptions>,
    /// The Charts setting, read with the history.
    pub charts: bool,
}

/// The conversation's system prompt as the first message, when not blank.
pub(crate) fn with_system_prompt(
    settings: &ConversationSettings,
    history: Vec<ChatMessage>,
) -> Vec<ChatMessage> {
    if settings.system_prompt.expose().trim().is_empty() {
        return history;
    }
    let system = ChatMessage::new(ChatRole::System, settings.system_prompt.clone());
    std::iter::once(system).chain(history).collect()
}

/// When a turn's new rows are created: never before an existing row, even
/// if the clock went back. The order is what a revision's undo relies on.
pub(super) fn next_created_at(
    conn: &Connection,
    conversation_id: &str,
) -> rusqlite::Result<String> {
    conn.query_row(
        &format!(
            "SELECT max({NOW}, coalesce(
                 (SELECT max(created_at) FROM messages WHERE conversation_id = ?1), ''))"
        ),
        [conversation_id],
        |row| row.get(0),
    )
}

pub(super) fn insert_prompt(
    conn: &Connection,
    conversation_id: &str,
    id: Uuid,
    prompt: &Redacted,
    created_at: &str,
    group: Option<Uuid>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO messages (id, conversation_id, role, content, created_at, status,
                               compare_group)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            id.to_string(),
            conversation_id,
            ChatRole::User.as_str(),
            prompt.expose(),
            created_at,
            MessageStatus::Complete.as_str(),
            group.map(|group| group.to_string())
        ],
    )?;
    Ok(())
}

/// The empty `pending` row a reply by `model` streams into.
pub(super) fn insert_pending_reply(
    conn: &Connection,
    conversation_id: &str,
    id: Uuid,
    model: &str,
    created_at: &str,
    group: Option<Uuid>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO messages (id, conversation_id, role, content, created_at, status, model,
                               compare_group)
         VALUES (?1, ?2, ?3, '', ?4, ?5, ?6, ?7)",
        params![
            id.to_string(),
            conversation_id,
            ChatRole::Assistant.as_str(),
            created_at,
            MessageStatus::Pending.as_str(),
            model,
            group.map(|group| group.to_string())
        ],
    )?;
    Ok(())
}

impl Storage {
    /// In one transaction: reads the settings and the history, lets `check`
    /// refuse them (size limits, settings changed since the send was
    /// prepared) before anything is written, then saves the turn and marks
    /// the conversation used. A non-blank system prompt goes first in the
    /// history and is never saved as a row. A new prompt is added after the
    /// last message. Regenerate and Edit mark the rows they replace
    /// `superseded_by` the new reply: they disappear from the conversation
    /// now and are deleted only once that reply completes (see
    /// [`TurnRecorder`]). The reply row records `model`. Refused while a
    /// comparison waits for a pick.
    pub fn begin_turn(
        &self,
        conversation_id: Uuid,
        model: &str,
        input: &TurnInput,
        check: impl FnOnce(&[ChatMessage], &ConversationSettings) -> Result<(), AppError>,
    ) -> Result<Turn, AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let id = conversation_id.to_string();
        let settings = load_settings(&tx, &id)?;
        check_no_comparison(&tx, &id)?;

        // `replaced`: the first row this turn replaces is after (`>`) or at
        // (`>=`) this position.
        let (mut history, replaced, user_id) = match input {
            TurnInput::New(_) => (load_history(&tx, &id, None)?, None, Uuid::new_v4()),
            TurnInput::Regenerate => {
                let last = tx
                    .query_row(
                        &format!(
                            "SELECT id, created_at, rowid FROM messages
                             WHERE {LIVE} AND role = 'user'
                             ORDER BY created_at DESC, rowid DESC LIMIT 1"
                        ),
                        [&id],
                        position_row,
                    )
                    .optional()?
                    .ok_or_else(|| {
                        AppError::InvalidInput("there is no message to regenerate".into())
                    })?;
                let history = load_history(&tx, &id, Some((&last, "<=")))?;
                let user_id = last.id;
                (history, Some((last, ">")), user_id)
            }
            TurnInput::Edit { message_id, .. } => {
                let edited = tx
                    .query_row(
                        &format!(
                            "SELECT id, created_at, rowid FROM messages
                             WHERE {LIVE} AND role = 'user' AND id = ?2"
                        ),
                        params![id, message_id.to_string()],
                        position_row,
                    )
                    .optional()?
                    .ok_or_else(|| AppError::NotFound("no such message".into()))?;
                let history = load_history(&tx, &id, Some((&edited, "<")))?;
                (history, Some((edited, ">=")), Uuid::new_v4())
            }
        };
        if let Some(prompt) = input.prompt() {
            history.push(ChatMessage::new(ChatRole::User, prompt.clone()));
        }
        let history = with_system_prompt(&settings, history);
        check(&history, &settings)?;

        let assistant_id = Uuid::new_v4();
        if let Some((position, op)) = &replaced {
            tx.execute(
                &format!(
                    "UPDATE messages SET superseded_by = ?4
                     WHERE {LIVE} AND (created_at, rowid) {op} (?2, ?3)"
                ),
                params![
                    id,
                    position.created_at,
                    position.rowid,
                    assistant_id.to_string()
                ],
            )?;
        }
        let created_at = next_created_at(&tx, &id)?;
        if let Some(prompt) = input.prompt() {
            insert_prompt(&tx, &id, user_id, prompt, &created_at, None)?;
        }
        insert_pending_reply(&tx, &id, assistant_id, model, &created_at, None)?;
        tx.execute(
            &format!("UPDATE conversations SET model = ?2, updated_at = {NOW} WHERE id = ?1"),
            params![id, model],
        )?;
        tx.commit()?;
        Ok(Turn {
            assistant_id,
            user_id,
            history,
            options: settings.options(),
            charts: settings.charts,
        })
    }

    /// Only a `pending` row changes, so a late write can never undo a final
    /// status. A deleted row is silently skipped.
    fn save_pending(&self, id: Uuid, reply: &ReplyText<'_>) -> Result<(), AppError> {
        self.conn().execute(
            "UPDATE messages SET content = ?2, thinking = ?3, charts = ?4
             WHERE id = ?1 AND status = 'pending'",
            params![
                id.to_string(),
                reply.content,
                thinking_column(reply.thinking),
                charts_json(reply.charts)?
            ],
        )?;
        Ok(())
    }

    /// In one transaction with the final status: a complete reply deletes
    /// the rows its turn replaced; a failed or stopped one restores them.
    fn save_final(
        &self,
        id: Uuid,
        reply: &ReplyText<'_>,
        end: &FinalState,
    ) -> Result<(), AppError> {
        let charts = charts_json(reply.charts)?;
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let id = id.to_string();
        let changed = tx.execute(
            "UPDATE messages
             SET content = ?2, status = ?3, prompt_tokens = ?4, completion_tokens = ?5,
                 duration_ms = ?6, thinking = ?7, thinking_ms = ?8, eval_ms = ?9, charts = ?10
             WHERE id = ?1 AND status = 'pending'",
            params![
                id,
                reply.content,
                end.status.as_str(),
                to_sql_count(end.prompt_tokens),
                to_sql_count(end.completion_tokens),
                to_sql_count(end.duration_ms),
                thinking_column(reply.thinking),
                to_sql_count(end.thinking_ms),
                to_sql_count(end.eval_ms),
                charts
            ],
        )?;
        if changed == 1 {
            if end.status == MessageStatus::Complete {
                tx.execute("DELETE FROM messages WHERE superseded_by = ?1", [&id])?;
            } else {
                restore_superseded(&tx, &id)?;
            }
        }
        tx.commit()?;
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

/// What a reply row holds so far.
struct ReplyText<'a> {
    content: &'a str,
    thinking: &'a str,
    charts: &'a [ChartEntry],
}

#[derive(Debug)]
struct FinalState {
    status: MessageStatus,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    duration_ms: Option<u64>,
    thinking_ms: Option<u64>,
    eval_ms: Option<u64>,
}

impl FinalState {
    fn new(status: MessageStatus) -> Self {
        Self {
            status,
            prompt_tokens: None,
            completion_tokens: None,
            duration_ms: None,
            thinking_ms: None,
            eval_ms: None,
        }
    }
}

/// The counts a finished reply is saved with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReplyStats {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub duration_ms: Option<u64>,
    pub thinking_ms: Option<u64>,
    pub eval_ms: Option<u64>,
}

/// Saves one streamed reply into its `pending` row, so a hard kill keeps what
/// had arrived. No `Debug`: it holds the reply text.
pub struct TurnRecorder {
    storage: Arc<Storage>,
    message_id: Uuid,
    content: String,
    thinking: String,
    charts: Vec<ChartEntry>,
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
            thinking: String::new(),
            charts: Vec::new(),
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
        self.buffer(token, false)
    }

    /// Buffers a thinking chunk, flushed like [`Self::push`].
    pub fn push_thinking(&mut self, chunk: &Redacted) -> Result<(), AppError> {
        self.buffer(chunk, true)
    }

    /// Buffers a chart call, flushed like [`Self::push`].
    pub fn push_chart(&mut self, entry: &ChartEntry) -> Result<(), AppError> {
        self.check_failed()?;
        if self.finished {
            return Ok(());
        }
        self.charts.push(entry.clone());
        self.unflushed += 1;
        if self.unflushed >= self.policy.tokens {
            self.flush()?;
        }
        Ok(())
    }

    /// The answer so far.
    pub fn text(&self) -> String {
        self.content.clone()
    }

    fn reply(&self) -> ReplyText<'_> {
        ReplyText {
            content: &self.content,
            thinking: &self.thinking,
            charts: &self.charts,
        }
    }

    fn buffer(&mut self, text: &Redacted, thinking: bool) -> Result<(), AppError> {
        self.check_failed()?;
        if self.finished {
            return Ok(());
        }
        let target = if thinking {
            &mut self.thinking
        } else {
            &mut self.content
        };
        target.push_str(text.expose());
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
    pub fn complete(&mut self, stats: ReplyStats) -> Result<(), AppError> {
        self.finish_with(FinalState {
            status: MessageStatus::Complete,
            prompt_tokens: stats.prompt_tokens,
            completion_tokens: stats.completion_tokens,
            duration_ms: stats.duration_ms,
            thinking_ms: stats.thinking_ms,
            eval_ms: stats.eval_ms,
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
            .save_final(self.message_id, &self.reply(), &end)
        {
            self.failed = true;
            return Err(err);
        }
        self.finished = true;
        self.check_failed()
    }

    fn flush(&mut self) -> Result<(), AppError> {
        match self.storage.save_pending(self.message_id, &self.reply()) {
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
    use crate::storage::testing::{count_rows, temp_storage};

    fn conversation(storage: &Storage) -> Uuid {
        storage.create_conversation(None, "m").unwrap().id
    }

    fn begin(storage: &Storage, conversation_id: Uuid, input: TurnInput) -> Result<Turn, AppError> {
        storage.begin_turn(conversation_id, "llama3.2", &input, |_, _| Ok(()))
    }

    fn turn(storage: &Storage, conversation_id: Uuid, prompt: &str) -> Turn {
        begin(
            storage,
            conversation_id,
            TurnInput::New(Redacted::new(prompt)),
        )
        .unwrap()
    }

    fn edit(message_id: Uuid, prompt: &str) -> TurnInput {
        TurnInput::Edit {
            message_id,
            prompt: Redacted::new(prompt),
        }
    }

    fn recorder(storage: &Arc<Storage>, turn: &Turn) -> TurnRecorder {
        TurnRecorder::new(
            Arc::clone(storage),
            turn.assistant_id,
            FlushPolicy::default(),
        )
    }

    /// Streams `text` into the turn's reply and completes it.
    fn reply(storage: &Arc<Storage>, turn: &Turn, text: &str) {
        let mut recorder = recorder(storage, turn);
        recorder.push(&token(text)).unwrap();
        recorder.complete(ReplyStats::default()).unwrap();
    }

    /// A conversation of `prompts`, each answered `A<prompt>`.
    fn answered(storage: &Arc<Storage>, prompts: &[&str]) -> Uuid {
        let id = conversation(storage);
        for prompt in prompts {
            let turn = turn(storage, id, prompt);
            reply(storage, &turn, &format!("A{prompt}"));
        }
        id
    }

    fn messages(storage: &Storage, conversation_id: Uuid) -> Vec<StoredMessage> {
        storage.get_conversation(conversation_id).unwrap().messages
    }

    fn shown(storage: &Storage, conversation_id: Uuid) -> Vec<(ChatRole, String)> {
        messages(storage, conversation_id)
            .into_iter()
            .map(|message| (message.role, message.content.expose().to_owned()))
            .collect()
    }

    fn user_ids(storage: &Storage, conversation_id: Uuid) -> Vec<Uuid> {
        messages(storage, conversation_id)
            .into_iter()
            .filter(|message| message.role == ChatRole::User)
            .map(|message| message.id)
            .collect()
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
    fn begin_turn_prepends_system_prompt() {
        use crate::storage::conversations::ConversationSettings;

        let (storage, _dir) = temp_storage();
        let id = answered(&storage, &["One"]);
        let settings = ConversationSettings {
            system_prompt: Redacted::new("Answer only in Finnish"),
            ..ConversationSettings::default()
        };
        storage.update_settings(id, &settings).unwrap();

        let mut seen = None;
        let turn = storage
            .begin_turn(id, "m", &TurnInput::New(Redacted::new("Two")), |_, s| {
                seen = Some(s.clone());
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, Some(settings));
        assert_eq!(
            texts(&turn.history),
            [
                (ChatRole::System, "Answer only in Finnish"),
                (ChatRole::User, "One"),
                (ChatRole::Assistant, "AOne"),
                (ChatRole::User, "Two"),
            ]
        );
        assert_eq!(turn.options, None);
        // Never saved as a row.
        assert!(
            messages(&storage, id)
                .iter()
                .all(|message| message.role != ChatRole::System)
        );
        assert_eq!(count_rows(&storage, "messages"), 4);

        // A blank prompt sends nothing; options come along.
        let blank = ConversationSettings {
            system_prompt: Redacted::new("  \n"),
            options: ChatOptions {
                temperature: Some(0.2),
                ..ChatOptions::default()
            },
            ..ConversationSettings::default()
        };
        storage.update_settings(id, &blank).unwrap();
        let turn = begin(&storage, id, TurnInput::Regenerate).unwrap();
        assert_eq!(turn.history[0].role, ChatRole::User);
        assert_eq!(turn.options, blank.options());
    }

    #[test]
    fn history_skips_error_pending_and_empty_cancelled() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        let policy = FlushPolicy::default();

        let done = turn(&storage, id, "One");
        let mut recorder = TurnRecorder::new(Arc::clone(&storage), done.assistant_id, policy);
        recorder.push_thinking(&token("Private reasoning")).unwrap();
        recorder.push(&token("Reply")).unwrap();
        recorder.complete(ReplyStats::default()).unwrap();

        let stopped = turn(&storage, id, "Two");
        let mut recorder = TurnRecorder::new(Arc::clone(&storage), stopped.assistant_id, policy);
        recorder.push_thinking(&token("More reasoning")).unwrap();
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
        // Thinking is saved, but never part of what the model is sent.
        assert!(
            last.history
                .iter()
                .all(|message| !message.content.expose().contains("reasoning"))
        );
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
            .begin_turn(
                id,
                "other",
                &TurnInput::New(Redacted::new("Hi")),
                |history, _| {
                    assert_eq!(history.len(), 1);
                    Err(AppError::TooLarge("too big".into()))
                },
            )
            .unwrap_err();
        assert!(matches!(err, AppError::TooLarge(_)));
        assert_eq!(storage.get_conversation(id).unwrap(), before);
    }

    #[test]
    fn begin_turn_unknown_conversation_is_not_found() {
        let (storage, _dir) = temp_storage();
        for input in [TurnInput::New(Redacted::new("Hi")), TurnInput::Regenerate] {
            assert!(matches!(
                begin(&storage, Uuid::new_v4(), input),
                Err(AppError::NotFound(_))
            ));
        }
    }

    #[test]
    fn regenerate_replaces_last_reply() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage, &["One", "Two"]);
        let two = user_ids(&storage, id)[1];

        let turn = begin(&storage, id, TurnInput::Regenerate).unwrap();
        assert_eq!(turn.user_id, two);
        assert_eq!(
            texts(&turn.history),
            [
                (ChatRole::User, "One"),
                (ChatRole::Assistant, "AOne"),
                (ChatRole::User, "Two"),
            ]
        );
        reply(&storage, &turn, "Again");

        assert_eq!(
            shown(&storage, id),
            [
                (ChatRole::User, "One".to_owned()),
                (ChatRole::Assistant, "AOne".to_owned()),
                (ChatRole::User, "Two".to_owned()),
                (ChatRole::Assistant, "Again".to_owned()),
            ]
        );
        assert_eq!(count_rows(&storage, "messages"), 4);
    }

    #[test]
    fn regenerate_without_user_is_invalid() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        assert!(matches!(
            begin(&storage, id, TurnInput::Regenerate),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn edit_truncates_following_rows() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage, &["One", "Two", "Three", "Four"]);
        let second = user_ids(&storage, id)[1];

        let turn = begin(&storage, id, edit(second, "Changed")).unwrap();
        assert_ne!(turn.user_id, second);
        assert_eq!(
            texts(&turn.history),
            [
                (ChatRole::User, "One"),
                (ChatRole::Assistant, "AOne"),
                (ChatRole::User, "Changed"),
            ]
        );
        reply(&storage, &turn, "New");

        assert_eq!(
            shown(&storage, id),
            [
                (ChatRole::User, "One".to_owned()),
                (ChatRole::Assistant, "AOne".to_owned()),
                (ChatRole::User, "Changed".to_owned()),
                (ChatRole::Assistant, "New".to_owned()),
            ]
        );
        assert_eq!(user_ids(&storage, id)[1], turn.user_id);
        // The replaced rows are gone from the table, not just hidden.
        assert_eq!(count_rows(&storage, "messages"), 4);
    }

    #[test]
    fn edit_check_failure_changes_nothing() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage, &["One", "Two"]);
        let before = storage.get_conversation(id).unwrap();

        let err = storage
            .begin_turn(
                id,
                "other",
                &edit(user_ids(&storage, id)[0], "x"),
                |_, _| Err(AppError::TooLarge("too big".into())),
            )
            .unwrap_err();

        assert!(matches!(err, AppError::TooLarge(_)));
        assert_eq!(storage.get_conversation(id).unwrap(), before);
        assert_eq!(count_rows(&storage, "messages"), 4);
    }

    #[test]
    fn edit_foreign_or_assistant_message_not_found() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage, &["One"]);
        let other = answered(&storage, &["Elsewhere"]);
        let assistant = messages(&storage, id)[1].id;

        for message_id in [assistant, user_ids(&storage, other)[0], Uuid::new_v4()] {
            assert!(matches!(
                begin(&storage, id, edit(message_id, "x")),
                Err(AppError::NotFound(_))
            ));
        }
        assert_eq!(count_rows(&storage, "messages"), 4);
    }

    #[test]
    fn superseded_rows_restored_on_error_and_cancel() {
        // Error, a Stop before any token, and a Stop after some.
        let ends: [(Option<&str>, MessageStatus); 3] = [
            (None, MessageStatus::Error),
            (None, MessageStatus::Cancelled),
            (Some("Half"), MessageStatus::Cancelled),
        ];
        for (partial, status) in ends {
            for editing in [false, true] {
                let (storage, _dir) = temp_storage();
                let id = answered(&storage, &["One", "Two", "Three"]);
                let before = messages(&storage, id);
                let input = if editing {
                    edit(user_ids(&storage, id)[1], "Changed")
                } else {
                    TurnInput::Regenerate
                };

                let turn = begin(&storage, id, input).unwrap();
                let mut recorder = recorder(&storage, &turn);
                if let Some(text) = partial {
                    recorder.push(&token(text)).unwrap();
                }
                recorder.finish(status).unwrap();

                assert_eq!(messages(&storage, id), before, "{status:?} {partial:?}");
                assert_eq!(count_rows(&storage, "messages"), 6);
            }
        }
    }

    #[test]
    fn superseded_rows_excluded_from_history_and_get() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage, &["One", "Two"]);

        let pending = begin(&storage, id, edit(user_ids(&storage, id)[1], "Changed")).unwrap();

        assert_eq!(
            shown(&storage, id),
            [
                (ChatRole::User, "One".to_owned()),
                (ChatRole::Assistant, "AOne".to_owned()),
                (ChatRole::User, "Changed".to_owned()),
                (ChatRole::Assistant, String::new()),
            ]
        );
        assert_eq!(messages(&storage, id)[3].id, pending.assistant_id);
        let next = turn(&storage, id, "Next");
        assert_eq!(
            texts(&next.history),
            [
                (ChatRole::User, "One"),
                (ChatRole::Assistant, "AOne"),
                (ChatRole::User, "Changed"),
                (ChatRole::User, "Next"),
            ]
        );
    }

    #[test]
    fn regenerate_history_skips_failed_replies() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        let failed = turn(&storage, id, "One");
        let mut recorder = recorder(&storage, &failed);
        recorder.push(&token("bad")).unwrap();
        recorder.finish(MessageStatus::Error).unwrap();
        let stopped = turn(&storage, id, "Two");
        self::recorder(&storage, &stopped)
            .finish(MessageStatus::Cancelled)
            .unwrap();
        let last = turn(&storage, id, "Three");
        reply(&storage, &last, "ok");

        let regenerated = begin(&storage, id, TurnInput::Regenerate).unwrap();
        assert_eq!(
            texts(&regenerated.history),
            [
                (ChatRole::User, "One"),
                (ChatRole::User, "Two"),
                (ChatRole::User, "Three"),
            ]
        );
        self::recorder(&storage, &regenerated)
            .finish(MessageStatus::Error)
            .unwrap();

        let edited = begin(&storage, id, edit(user_ids(&storage, id)[2], "3")).unwrap();
        assert_eq!(
            texts(&edited.history),
            [
                (ChatRole::User, "One"),
                (ChatRole::User, "Two"),
                (ChatRole::User, "3"),
            ]
        );
    }

    #[test]
    fn rows_sort_after_existing_when_clock_is_behind() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage, &["One", "Two"]);
        let future = "9999-01-01T00:00:00.000Z";
        let last = messages(&storage, id)[3].id.to_string();
        storage
            .conn()
            .execute(
                "UPDATE messages SET created_at = ?1 WHERE id = ?2",
                params![future, last],
            )
            .unwrap();
        let before = messages(&storage, id);

        let turn = begin(&storage, id, edit(user_ids(&storage, id)[0], "Changed")).unwrap();
        assert!(
            messages(&storage, id)
                .iter()
                .all(|message| message.created_at == future)
        );
        recorder(&storage, &turn)
            .finish(MessageStatus::Error)
            .unwrap();

        assert_eq!(messages(&storage, id), before);
    }

    #[test]
    fn startup_recovery_restores_superseded() {
        let dir = tempfile::tempdir().unwrap();
        let keys = crate::storage::key::testing::MemoryKeySource::default();
        let storage = Arc::new(super::super::open_storage(dir.path(), &keys).unwrap());
        let edited = answered(&storage, &["One", "Two"]);
        let before = messages(&storage, edited);
        let plain = conversation(&storage);
        let interrupted = turn(&storage, plain, "Hi");
        let revision = begin(&storage, edited, edit(user_ids(&storage, edited)[1], "x")).unwrap();
        let mut recorder = recorder(&storage, &revision);
        recorder.push(&token("partial")).unwrap();
        recorder.flush_if_dirty().unwrap();
        // A hard kill while both replies stream.
        drop(recorder);
        drop(storage);

        let storage = super::super::open_storage(dir.path(), &keys).unwrap();
        assert_eq!(messages(&storage, edited), before);
        let rows = messages(&storage, plain);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].id, interrupted.assistant_id);
        assert_eq!(rows[1].status, MessageStatus::Error);
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
        recorder
            .complete(ReplyStats {
                prompt_tokens: Some(3),
                completion_tokens: Some(2),
                duration_ms: Some(120),
                thinking_ms: None,
                eval_ms: Some(80),
            })
            .unwrap();

        let saved = message(&storage, id, turn.assistant_id);
        assert_eq!(saved.content.expose(), "Hello");
        assert_eq!(saved.status, MessageStatus::Complete);
        assert_eq!(saved.prompt_tokens, Some(3));
        assert_eq!(saved.completion_tokens, Some(2));
        assert_eq!(saved.duration_ms, Some(120));
        assert_eq!(saved.eval_ms, Some(80));
        assert_eq!(saved.thinking, None);
        assert_eq!(saved.thinking_ms, None);
    }

    #[test]
    fn recorder_saves_thinking() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        let turn = turn(&storage, id, "Hi");
        let policy = FlushPolicy {
            every: Duration::from_secs(60),
            tokens: 2,
        };
        let mut recorder = TurnRecorder::new(Arc::clone(&storage), turn.assistant_id, policy);

        recorder.push_thinking(&token("Let me")).unwrap();
        recorder.push_thinking(&token(" see")).unwrap();
        // Two chunks reach the flush threshold: the thinking is on disk.
        let pending = message(&storage, id, turn.assistant_id);
        assert_eq!(pending.thinking.unwrap().expose(), "Let me see");
        assert_eq!(pending.content.expose(), "");
        assert_eq!(pending.status, MessageStatus::Pending);

        recorder.push(&token("Hi")).unwrap();
        recorder
            .complete(ReplyStats {
                thinking_ms: Some(1500),
                ..ReplyStats::default()
            })
            .unwrap();
        let saved = message(&storage, id, turn.assistant_id);
        assert_eq!(saved.thinking.unwrap().expose(), "Let me see");
        assert_eq!(saved.thinking_ms, Some(1500));
        assert_eq!(saved.content.expose(), "Hi");
    }

    fn bar_chart(offset: u64) -> ChartEntry {
        use crate::ollama::chart_tool::{ChartKind, ChartSpec};
        ChartEntry {
            offset,
            spec: Some(ChartSpec {
                title: "Population".into(),
                kind: ChartKind::Bar,
                labels: vec!["1950".into()],
                values: vec![4.0],
            }),
            error: None,
        }
    }

    #[test]
    fn reply_charts_persist() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        let turn = turn(&storage, id, "Chart it");
        let policy = FlushPolicy {
            every: Duration::from_secs(60),
            tokens: 2,
        };
        let mut recorder = TurnRecorder::new(Arc::clone(&storage), turn.assistant_id, policy);
        assert!(!turn.charts);

        recorder.push(&token("Here:")).unwrap();
        recorder.push_chart(&bar_chart(5)).unwrap();
        // Two buffered: on disk while it streams.
        let pending = message(&storage, id, turn.assistant_id);
        assert_eq!(pending.charts, [bar_chart(5)]);
        assert_eq!(recorder.text(), "Here:");

        recorder.push(&token(" Grew.")).unwrap();
        recorder.complete(ReplyStats::default()).unwrap();
        let saved = message(&storage, id, turn.assistant_id);
        assert_eq!(saved.content.expose(), "Here: Grew.");
        assert_eq!(saved.charts, [bar_chart(5)]);
        // A reply without charts keeps the column NULL.
        assert!(
            message(&storage, id, user_ids(&storage, id)[0])
                .charts
                .is_empty()
        );
        let nulls: i64 = storage
            .conn()
            .query_row(
                "SELECT count(*) FROM messages WHERE charts IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(nulls, 1);
    }

    #[test]
    fn history_replays_tool_call_and_result() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        let first = turn(&storage, id, "Chart it");
        let mut recorder = recorder(&storage, &first);
        recorder.push(&token("Here:")).unwrap();
        recorder.push_chart(&bar_chart(5)).unwrap();
        recorder.push(&token(" Grew.")).unwrap();
        recorder.complete(ReplyStats::default()).unwrap();

        let next = turn(&storage, id, "Thanks");
        assert_eq!(
            texts(&next.history),
            [
                (ChatRole::User, "Chart it"),
                (ChatRole::Assistant, "Here:"),
                (ChatRole::Tool, "The chart was shown to the user."),
                (ChatRole::Assistant, " Grew."),
                (ChatRole::User, "Thanks"),
            ]
        );
        let calls = next.history[1].tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(next.history[2].tool_name, Some("render_chart"));
    }

    #[test]
    fn begin_turn_reads_charts_setting() {
        use crate::storage::conversations::ConversationSettings;

        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        storage
            .update_settings(
                id,
                &ConversationSettings {
                    charts: true,
                    ..ConversationSettings::default()
                },
            )
            .unwrap();
        assert!(turn(&storage, id, "Hi").charts);
    }

    #[test]
    fn cancelled_reply_keeps_thinking() {
        let (storage, _dir) = temp_storage();
        let id = conversation(&storage);
        let turn = turn(&storage, id, "Hi");
        let mut recorder = TurnRecorder::new(
            Arc::clone(&storage),
            turn.assistant_id,
            FlushPolicy::default(),
        );

        recorder.push_thinking(&token("Hmm")).unwrap();
        recorder.finish(MessageStatus::Cancelled).unwrap();

        let saved = message(&storage, id, turn.assistant_id);
        assert_eq!(saved.thinking.unwrap().expose(), "Hmm");
        assert_eq!(saved.thinking_ms, None);
        assert_eq!(saved.content.expose(), "");
        assert_eq!(saved.status, MessageStatus::Cancelled);
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
        recorder
            .complete(ReplyStats {
                completion_tokens: Some(1),
                ..ReplyStats::default()
            })
            .unwrap();
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
        recorder.complete(ReplyStats::default()).unwrap();
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

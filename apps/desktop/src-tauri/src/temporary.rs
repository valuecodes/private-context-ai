//! Temporary chats: history kept only in process memory, never written to
//! the database. Managed by Tauri as `Arc<TempChats>`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use uuid::Uuid;

use crate::commands::ollama::ReplyRecorder;
use crate::error::AppError;
use crate::ollama::types::{ChatMessage, ChatOptions, ChatRole};
use crate::redacted::Redacted;
use crate::storage::conversations::ConversationSettings;
use crate::storage::messages::{MessageStatus, ReplyStats, TurnInput, with_system_prompt};

/// How many temporary chats may exist at once. The UI keeps one; the rest
/// bound what a reloaded webview can leave behind.
const MAX_SESSIONS: usize = 8;

/// One message, with the id the UI knows it by.
struct TempMessage {
    id: Uuid,
    message: ChatMessage,
}

/// What a running regenerate or edit replaced: the messages from `base` on,
/// put back if its reply fails or is stopped.
struct Revision {
    base: usize,
    tail: Vec<TempMessage>,
}

#[derive(Default)]
struct Session {
    messages: Vec<TempMessage>,
    revision: Option<Revision>,
    settings: ConversationSettings,
}

/// What [`TempChats::begin`] hands the send.
#[derive(Debug)]
pub struct TempTurn {
    /// The prompt's id: new, or the existing one when regenerating.
    pub user_id: Uuid,
    /// The system prompt, earlier messages and the prompt.
    pub history: Vec<ChatMessage>,
    pub options: Option<ChatOptions>,
}

impl Session {
    /// Undoes a running regenerate or edit. `false` when none runs.
    fn restore(&mut self) -> bool {
        let Some(revision) = self.revision.take() else {
            return false;
        };
        self.messages.truncate(revision.base);
        self.messages.extend(revision.tail);
        true
    }
}

type Sessions = HashMap<Uuid, Session>;

fn not_found() -> AppError {
    AppError::NotFound("no such temporary chat".into())
}

/// Every temporary chat's history, by session id.
pub struct TempChats {
    sessions: Mutex<Sessions>,
    max_sessions: usize,
}

impl Default for TempChats {
    fn default() -> Self {
        Self::with_max(MAX_SESSIONS)
    }
}

impl TempChats {
    pub fn with_max(max_sessions: usize) -> Self {
        Self {
            sessions: Mutex::default(),
            max_sessions,
        }
    }

    fn sessions(&self) -> MutexGuard<'_, Sessions> {
        // Every critical section is one map operation or one push or pop, so
        // a poisoned lock is still consistent.
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A new, empty chat. `Busy` once `max_sessions` exist.
    pub fn start(&self) -> Result<Uuid, AppError> {
        self.start_with(ConversationSettings::default())
    }

    /// Like [`Self::start`], with `settings` from the first send on.
    pub fn start_with(&self, settings: ConversationSettings) -> Result<Uuid, AppError> {
        settings.validate()?;
        let mut sessions = self.sessions();
        if sessions.len() >= self.max_sessions {
            return Err(AppError::Busy(format!(
                "{} temporary chats are already open",
                self.max_sessions
            )));
        }
        let id = Uuid::new_v4();
        sessions.insert(
            id,
            Session {
                settings,
                ..Session::default()
            },
        );
        Ok(id)
    }

    pub fn settings(&self, id: Uuid) -> Result<ConversationSettings, AppError> {
        self.sessions()
            .get(&id)
            .map(|session| session.settings.clone())
            .ok_or_else(not_found)
    }

    /// Applies from the chat's next send.
    pub fn update_settings(
        &self,
        id: Uuid,
        settings: ConversationSettings,
    ) -> Result<(), AppError> {
        settings.validate()?;
        let mut sessions = self.sessions();
        let session = sessions.get_mut(&id).ok_or_else(not_found)?;
        session.settings = settings;
        Ok(())
    }

    /// Forgets a chat. `false` when there was no such chat.
    pub fn remove(&self, id: Uuid) -> bool {
        self.sessions().remove(&id).is_some()
    }

    /// Under one lock, so a discard or settings change cannot land halfway:
    /// the history `input` sends with the chat's settings, which `check` may
    /// refuse (size limits, settings changed) before anything changes. Then
    /// the prompt is kept, and a regenerate or edit sets aside the messages
    /// it replaces until its reply ends ([`MemoryRecorder`]).
    pub fn begin(
        &self,
        id: Uuid,
        input: &TurnInput,
        check: impl FnOnce(&[ChatMessage], &ConversationSettings) -> Result<(), AppError>,
    ) -> Result<TempTurn, AppError> {
        let mut sessions = self.sessions();
        let session = sessions.get_mut(&id).ok_or_else(not_found)?;
        let is_user = |message: &TempMessage| message.message.role == ChatRole::User;
        // `cut`: where the replaced messages start.
        let (cut, user_id) = match input {
            TurnInput::New(_) => (None, Uuid::new_v4()),
            TurnInput::Regenerate => {
                let last = session.messages.iter().rposition(is_user).ok_or_else(|| {
                    AppError::InvalidInput("there is no message to regenerate".into())
                })?;
                (Some(last + 1), session.messages[last].id)
            }
            TurnInput::Edit { message_id, .. } => {
                let edited = session
                    .messages
                    .iter()
                    .position(|message| message.id == *message_id && is_user(message))
                    .ok_or_else(|| AppError::NotFound("no such message".into()))?;
                (Some(edited), Uuid::new_v4())
            }
        };
        let prompt = input.prompt().map(|prompt| TempMessage {
            id: user_id,
            message: ChatMessage {
                role: ChatRole::User,
                content: prompt.clone(),
            },
        });
        let history: Vec<ChatMessage> = session.messages[..cut.unwrap_or(session.messages.len())]
            .iter()
            .chain(&prompt)
            .map(|message| message.message.clone())
            .collect();
        let history = with_system_prompt(&session.settings, history);
        check(&history, &session.settings)?;
        let options = session.settings.options();
        if let Some(base) = cut {
            let tail = session.messages.split_off(base);
            session.revision = Some(Revision { base, tail });
        }
        session.messages.extend(prompt);
        Ok(TempTurn {
            user_id,
            history,
            options,
        })
    }

    /// Takes back the prompt [`Self::begin`] kept for a new message when the
    /// request then did not start. Only one request runs per chat, so it is
    /// still the last message. (A regenerate or edit that did not start is
    /// undone by its recorder.)
    pub fn rollback(&self, id: Uuid) {
        if let Some(session) = self.sessions().get_mut(&id)
            && session
                .messages
                .last()
                .is_some_and(|message| message.message.role == ChatRole::User)
        {
            session.messages.pop();
        }
    }

    /// Ends the running reply: `Some` adds it and drops what a regenerate or
    /// edit replaced; `None` puts that back. A discarded chat is a no-op.
    fn end_reply(&self, id: Uuid, reply: Option<TempMessage>) {
        if let Some(session) = self.sessions().get_mut(&id) {
            match reply {
                Some(reply) => {
                    session.revision = None;
                    session.messages.push(reply);
                }
                None => {
                    session.restore();
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn history(&self, id: Uuid) -> Option<Vec<ChatMessage>> {
        self.sessions().get(&id).map(|session| {
            session
                .messages
                .iter()
                .map(|message| message.message.clone())
                .collect()
        })
    }

    #[cfg(test)]
    pub(crate) fn ids(&self, id: Uuid) -> Vec<Uuid> {
        self.sessions()
            .get(&id)
            .map(|session| session.messages.iter().map(|message| message.id).collect())
            .unwrap_or_default()
    }
}

/// Collects one streamed reply and adds it to its chat when it ends, the way
/// a saved chat keeps it: a complete reply always, a cancelled one only with
/// visible content and only for a new message, a failed one never. A reply
/// that is not kept undoes its regenerate or edit. Thinking is dropped,
/// since it is never sent back. Nothing here can fail. No `Debug`: it holds
/// the reply text.
pub struct MemoryRecorder {
    chats: Weak<TempChats>,
    session_id: Uuid,
    message_id: Uuid,
    /// A regenerate or edit: only a complete reply replaces what it
    /// replaced.
    revision: bool,
    content: String,
    finished: bool,
}

impl MemoryRecorder {
    pub fn new(chats: &Arc<TempChats>, session_id: Uuid, message_id: Uuid, revision: bool) -> Self {
        Self {
            chats: Arc::downgrade(chats),
            session_id,
            message_id,
            revision,
            content: String::new(),
            finished: false,
        }
    }

    fn end(&mut self, keep: bool) {
        if self.finished {
            return;
        }
        self.finished = true;
        if let Some(chats) = self.chats.upgrade() {
            let reply = keep.then(|| TempMessage {
                id: self.message_id,
                message: ChatMessage {
                    role: ChatRole::Assistant,
                    content: Redacted::new(std::mem::take(&mut self.content)),
                },
            });
            chats.end_reply(self.session_id, reply);
        }
    }
}

impl ReplyRecorder for MemoryRecorder {
    fn push(&mut self, token: &Redacted) -> Result<(), AppError> {
        if !self.finished {
            self.content.push_str(token.expose());
        }
        Ok(())
    }

    fn push_thinking(&mut self, _chunk: &Redacted) -> Result<(), AppError> {
        Ok(())
    }

    fn complete(&mut self, _stats: ReplyStats) -> Result<(), AppError> {
        self.end(true);
        Ok(())
    }

    fn finish(&mut self, status: MessageStatus) -> Result<(), AppError> {
        self.end(status == MessageStatus::Cancelled && !self.revision && !self.content.is_empty());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(history: &[ChatMessage]) -> Vec<(ChatRole, &str)> {
        history
            .iter()
            .map(|message| (message.role, message.content.expose()))
            .collect()
    }

    fn new(prompt: &str) -> TurnInput {
        TurnInput::New(Redacted::new(prompt))
    }

    /// Runs one turn of `input` that ends with `reply`, or fails when `None`.
    fn run(chats: &Arc<TempChats>, id: Uuid, input: &TurnInput, reply: Option<&str>) {
        chats.begin(id, input, |_, _| Ok(())).unwrap();
        let revision = !matches!(input, TurnInput::New(_));
        let mut recorder = MemoryRecorder::new(chats, id, Uuid::new_v4(), revision);
        match reply {
            Some(text) => {
                recorder.push(&Redacted::new(text)).unwrap();
                recorder.complete(ReplyStats::default()).unwrap();
            }
            None => recorder.finish(MessageStatus::Error).unwrap(),
        }
    }

    /// A chat of `prompts`, each answered `A<prompt>`.
    fn answered(prompts: &[&str]) -> (Arc<TempChats>, Uuid) {
        let chats = Arc::new(TempChats::default());
        let id = chats.start().unwrap();
        for prompt in prompts {
            run(&chats, id, &new(prompt), Some(&format!("A{prompt}")));
        }
        (chats, id)
    }

    #[test]
    fn temp_regenerate_replaces_last_reply() {
        let (chats, id) = answered(&["One", "Two"]);
        let ids = chats.ids(id);

        let turn = chats
            .begin(id, &TurnInput::Regenerate, |_, _| Ok(()))
            .unwrap();
        assert_eq!(turn.user_id, ids[2]);
        assert_eq!(
            texts(&turn.history),
            [
                (ChatRole::User, "One"),
                (ChatRole::Assistant, "AOne"),
                (ChatRole::User, "Two"),
            ]
        );
        let mut recorder = MemoryRecorder::new(&chats, id, Uuid::new_v4(), true);
        recorder.push(&Redacted::new("Again")).unwrap();
        recorder.complete(ReplyStats::default()).unwrap();

        assert_eq!(
            texts(&chats.history(id).unwrap()),
            [
                (ChatRole::User, "One"),
                (ChatRole::Assistant, "AOne"),
                (ChatRole::User, "Two"),
                (ChatRole::Assistant, "Again"),
            ]
        );
    }

    #[test]
    fn temp_edit_truncates_and_restores_on_error() {
        let (chats, id) = answered(&["One", "Two", "Three"]);
        let before = chats.history(id).unwrap();
        let ids = chats.ids(id);
        let edit = TurnInput::Edit {
            message_id: ids[2],
            prompt: Redacted::new("Changed"),
        };

        // Failed, then stopped with some content: both put everything back.
        run(&chats, id, &edit, None);
        assert_eq!(chats.history(id).unwrap(), before);
        assert_eq!(chats.ids(id), ids);
        chats.begin(id, &edit, |_, _| Ok(())).unwrap();
        let mut stopped = MemoryRecorder::new(&chats, id, Uuid::new_v4(), true);
        stopped.push(&Redacted::new("Half")).unwrap();
        stopped.finish(MessageStatus::Cancelled).unwrap();
        assert_eq!(chats.history(id).unwrap(), before);

        run(&chats, id, &edit, Some("New"));
        assert_eq!(
            texts(&chats.history(id).unwrap()),
            [
                (ChatRole::User, "One"),
                (ChatRole::Assistant, "AOne"),
                (ChatRole::User, "Changed"),
                (ChatRole::Assistant, "New"),
            ]
        );
    }

    #[test]
    fn temp_edit_unknown_or_assistant_message_not_found() {
        let (chats, id) = answered(&["One"]);
        let assistant = chats.ids(id)[1];
        for message_id in [assistant, Uuid::new_v4()] {
            let edit = TurnInput::Edit {
                message_id,
                prompt: Redacted::new("x"),
            };
            assert!(matches!(
                chats.begin(id, &edit, |_, _| Ok(())),
                Err(AppError::NotFound(_))
            ));
        }
        let empty = chats.start().unwrap();
        assert!(matches!(
            chats.begin(empty, &TurnInput::Regenerate, |_, _| Ok(())),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn temp_settings_apply_to_session() {
        let chats = TempChats::default();
        let finnish = ConversationSettings {
            system_prompt: Redacted::new("Answer only in Finnish"),
            ..ConversationSettings::default()
        };
        let id = chats.start_with(finnish.clone()).unwrap();
        assert_eq!(chats.settings(id).unwrap(), finnish);

        let mut seen = None;
        let turn = chats
            .begin(id, &new("Hi"), |_, settings| {
                seen = Some(settings.clone());
                Ok(())
            })
            .unwrap();
        assert_eq!(seen.as_ref(), Some(&finnish));
        assert_eq!(
            texts(&turn.history),
            [
                (ChatRole::System, "Answer only in Finnish"),
                (ChatRole::User, "Hi")
            ]
        );
        assert_eq!(turn.options, None);
        // The system prompt is sent, never kept as a message.
        assert_eq!(texts(&chats.history(id).unwrap()), [(ChatRole::User, "Hi")]);

        let warm = ConversationSettings {
            options: ChatOptions {
                temperature: Some(1.5),
                ..ChatOptions::default()
            },
            ..ConversationSettings::default()
        };
        chats.update_settings(id, warm.clone()).unwrap();
        chats.rollback(id);
        let turn = chats.begin(id, &new("Again"), |_, _| Ok(())).unwrap();
        assert_eq!(texts(&turn.history), [(ChatRole::User, "Again")]);
        assert_eq!(turn.options, warm.options());

        let bad = ConversationSettings {
            options: ChatOptions {
                temperature: Some(3.0),
                ..ChatOptions::default()
            },
            ..ConversationSettings::default()
        };
        assert!(matches!(
            chats.update_settings(id, bad.clone()),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            chats.start_with(bad),
            Err(AppError::InvalidInput(_))
        ));
        assert_eq!(chats.settings(id).unwrap(), warm);
        let unknown = Uuid::new_v4();
        assert!(matches!(
            chats.update_settings(unknown, warm),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            chats.settings(unknown),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn max_sessions_enforced() {
        let chats = TempChats::with_max(2);
        let first = chats.start().unwrap();
        chats.start().unwrap();

        assert!(matches!(chats.start(), Err(AppError::Busy(_))));
        chats.remove(first);
        chats.start().unwrap();
    }

    #[test]
    fn refused_begin_keeps_history() {
        let chats = TempChats::default();
        let id = chats.start().unwrap();

        let err = chats
            .begin(id, &new("Hi"), |_, _| Err(AppError::TooLarge("big".into())))
            .unwrap_err();

        assert!(matches!(err, AppError::TooLarge(_)));
        assert_eq!(chats.history(id).unwrap(), []);

        // A refused edit sets nothing aside either.
        let (chats, id) = answered(&["One"]);
        let before = chats.history(id).unwrap();
        let edit = TurnInput::Edit {
            message_id: chats.ids(id)[0],
            prompt: Redacted::new("x"),
        };
        assert!(
            chats
                .begin(id, &edit, |_, _| Err(AppError::TooLarge("big".into())))
                .is_err()
        );
        assert_eq!(chats.history(id).unwrap(), before);
    }

    #[test]
    fn begin_unknown_chat_is_not_found() {
        let chats = TempChats::default();
        assert!(matches!(
            chats.begin(Uuid::new_v4(), &new("Hi"), |_, _| Ok(())),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn rollback_takes_back_the_prompt() {
        let chats = TempChats::default();
        let id = chats.start().unwrap();
        chats.begin(id, &new("Hi"), |_, _| Ok(())).unwrap();

        chats.rollback(id);

        assert_eq!(chats.history(id).unwrap(), []);
    }

    #[test]
    fn temp_history_accumulates_complete_and_cancelled() {
        let chats = Arc::new(TempChats::default());
        let id = chats.start().unwrap();
        let turn = |prompt: &str, reply: &str, status: Option<MessageStatus>| {
            chats.begin(id, &new(prompt), |_, _| Ok(())).unwrap();
            let mut recorder = MemoryRecorder::new(&chats, id, Uuid::new_v4(), false);
            recorder.push_thinking(&Redacted::new("hmm")).unwrap();
            recorder.push(&Redacted::new(reply)).unwrap();
            match status {
                None => recorder.complete(ReplyStats::default()).unwrap(),
                Some(status) => recorder.finish(status).unwrap(),
            }
            // `on_end` runs after the sink saw `Done`.
            recorder.finish(MessageStatus::Cancelled).unwrap();
        };

        turn("One", "Done", None);
        turn("Two", "Par", Some(MessageStatus::Cancelled));
        turn("Three", "", Some(MessageStatus::Cancelled));
        turn("Four", "Bad", Some(MessageStatus::Error));

        assert_eq!(
            texts(&chats.history(id).unwrap()),
            [
                (ChatRole::User, "One"),
                (ChatRole::Assistant, "Done"),
                (ChatRole::User, "Two"),
                (ChatRole::Assistant, "Par"),
                (ChatRole::User, "Three"),
                (ChatRole::User, "Four"),
            ]
        );
    }

    #[test]
    fn discard_then_late_finish_is_noop() {
        let chats = Arc::new(TempChats::default());
        let id = chats.start().unwrap();
        chats.begin(id, &new("Hi"), |_, _| Ok(())).unwrap();
        let mut recorder = MemoryRecorder::new(&chats, id, Uuid::new_v4(), false);
        recorder.push(&Redacted::new("Late")).unwrap();

        chats.remove(id);
        recorder.complete(ReplyStats::default()).unwrap();

        assert!(chats.history(id).is_none());
    }
}

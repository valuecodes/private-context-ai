//! Temporary chats: history kept only in process memory, never written to
//! the database. Managed by Tauri as `Arc<TempChats>`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use uuid::Uuid;

use crate::commands::ollama::ReplyRecorder;
use crate::error::AppError;
use crate::ollama::types::{ChatMessage, ChatRole};
use crate::redacted::Redacted;
use crate::storage::messages::{MessageStatus, ReplyStats};

/// How many temporary chats may exist at once. The UI keeps one; the rest
/// bound what a reloaded webview can leave behind.
const MAX_SESSIONS: usize = 8;

type Sessions = HashMap<Uuid, Vec<ChatMessage>>;

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
        let mut sessions = self.sessions();
        if sessions.len() >= self.max_sessions {
            return Err(AppError::Busy(format!(
                "{} temporary chats are already open",
                self.max_sessions
            )));
        }
        let id = Uuid::new_v4();
        sessions.insert(id, Vec::new());
        Ok(id)
    }

    /// Forgets a chat; an unknown id is a no-op.
    pub fn remove(&self, id: Uuid) {
        self.sessions().remove(&id);
    }

    /// Under one lock, so a discard cannot land halfway: the chat's history
    /// plus `prompt`, which `check` may refuse (size limits) before anything
    /// changes. On success the prompt is kept and the history returned.
    pub fn begin(
        &self,
        id: Uuid,
        prompt: &Redacted,
        check: impl FnOnce(&[ChatMessage]) -> Result<(), AppError>,
    ) -> Result<Vec<ChatMessage>, AppError> {
        let mut sessions = self.sessions();
        let session = sessions.get_mut(&id).ok_or_else(not_found)?;
        let prompt = ChatMessage {
            role: ChatRole::User,
            content: prompt.clone(),
        };
        let mut history = session.clone();
        history.push(prompt.clone());
        check(&history)?;
        session.push(prompt);
        Ok(history)
    }

    /// Takes back the prompt [`Self::begin`] kept when the request then did
    /// not start. Only one request runs per chat, so it is still the last
    /// message.
    pub fn rollback(&self, id: Uuid) {
        if let Some(session) = self.sessions().get_mut(&id)
            && session
                .last()
                .is_some_and(|message| message.role == ChatRole::User)
        {
            session.pop();
        }
    }

    /// A discarded chat silently drops the message.
    fn append(&self, id: Uuid, message: ChatMessage) {
        if let Some(session) = self.sessions().get_mut(&id) {
            session.push(message);
        }
    }

    #[cfg(test)]
    pub(crate) fn history(&self, id: Uuid) -> Option<Vec<ChatMessage>> {
        self.sessions().get(&id).cloned()
    }
}

/// Collects one streamed reply and adds it to its chat when it ends, the way
/// `begin_turn` would include it in the history: a complete reply always, a
/// cancelled one only with visible content, a failed one never. Thinking is
/// dropped, since it is never sent back. Nothing here can fail. No `Debug`:
/// it holds the reply text.
pub struct MemoryRecorder {
    chats: Weak<TempChats>,
    session_id: Uuid,
    content: String,
    finished: bool,
}

impl MemoryRecorder {
    pub fn new(chats: &Arc<TempChats>, session_id: Uuid) -> Self {
        Self {
            chats: Arc::downgrade(chats),
            session_id,
            content: String::new(),
            finished: false,
        }
    }

    fn end(&mut self, keep: bool) {
        if self.finished {
            return;
        }
        self.finished = true;
        if keep && let Some(chats) = self.chats.upgrade() {
            chats.append(
                self.session_id,
                ChatMessage {
                    role: ChatRole::Assistant,
                    content: Redacted::new(std::mem::take(&mut self.content)),
                },
            );
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
        self.end(status == MessageStatus::Cancelled && !self.content.is_empty());
        Ok(())
    }

    fn ensure_saved(&self) -> Result<(), AppError> {
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
            .begin(id, &Redacted::new("Hi"), |_| {
                Err(AppError::TooLarge("big".into()))
            })
            .unwrap_err();

        assert!(matches!(err, AppError::TooLarge(_)));
        assert_eq!(chats.history(id).unwrap(), []);
    }

    #[test]
    fn begin_unknown_chat_is_not_found() {
        let chats = TempChats::default();
        assert!(matches!(
            chats.begin(Uuid::new_v4(), &Redacted::new("Hi"), |_| Ok(())),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn rollback_takes_back_the_prompt() {
        let chats = TempChats::default();
        let id = chats.start().unwrap();
        chats.begin(id, &Redacted::new("Hi"), |_| Ok(())).unwrap();

        chats.rollback(id);

        assert_eq!(chats.history(id).unwrap(), []);
    }

    #[test]
    fn temp_history_accumulates_complete_and_cancelled() {
        let chats = Arc::new(TempChats::default());
        let id = chats.start().unwrap();
        let turn = |prompt: &str, reply: &str, status: Option<MessageStatus>| {
            chats.begin(id, &Redacted::new(prompt), |_| Ok(())).unwrap();
            let mut recorder = MemoryRecorder::new(&chats, id);
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
        chats.begin(id, &Redacted::new("Hi"), |_| Ok(())).unwrap();
        let mut recorder = MemoryRecorder::new(&chats, id);
        recorder.push(&Redacted::new("Late")).unwrap();

        chats.remove(id);
        recorder.complete(ReplyStats::default()).unwrap();

        assert!(chats.history(id).is_none());
    }
}

//! Temporary chats: history kept only in process memory, never written to
//! the database. Managed by Tauri as `Arc<TempChats>`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use uuid::Uuid;

use crate::commands::ollama::ReplyRecorder;
use crate::error::AppError;
use crate::ollama::chart_tool::{ChartEntry, replay};
use crate::ollama::types::{ChatMessage, ChatOptions, ChatRole};
use crate::redacted::Redacted;
use crate::storage::conversations::ConversationSettings;
use crate::storage::memory::ProjectContext;
use crate::storage::messages::{MessageStatus, ReplyStats, TurnInput, with_system_message};

/// How many temporary chats may exist at once. The UI keeps one; the rest
/// bound what a reloaded webview can leave behind.
const MAX_SESSIONS: usize = 8;

/// One message, with the id the UI knows it by.
struct TempMessage {
    id: Uuid,
    message: ChatMessage,
    /// A reply's `render_chart` calls.
    charts: Vec<ChartEntry>,
}

impl TempMessage {
    /// What the model is sent for this message: a charted reply as its
    /// calls and their results.
    fn history(&self) -> Vec<ChatMessage> {
        if self.message.role == ChatRole::Assistant && !self.charts.is_empty() {
            replay(self.message.content.expose(), &self.charts)
        } else {
            vec![self.message.clone()]
        }
    }
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
    /// The project it was started in. Only the id is kept: its instructions
    /// are read from storage at every turn, so edits apply at once.
    project_id: Option<Uuid>,
}

/// What [`TempChats::begin`] hands the send.
#[derive(Debug)]
pub struct TempTurn {
    /// The prompt's id: new, or the existing one when regenerating.
    pub user_id: Uuid,
    /// The system prompt, earlier messages and the prompt.
    pub history: Vec<ChatMessage>,
    pub options: Option<ChatOptions>,
    /// The Charts setting, read with the history.
    pub charts: bool,
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
        self.start_with(ConversationSettings::default(), None)
    }

    /// Like [`Self::start`], with `settings` from the first send on, in
    /// project `project_id` when given.
    pub fn start_with(
        &self,
        settings: ConversationSettings,
        project_id: Option<Uuid>,
    ) -> Result<Uuid, AppError> {
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
                project_id,
                ..Session::default()
            },
        );
        Ok(id)
    }

    /// The project the chat was started in, which never changes.
    pub fn project(&self, id: Uuid) -> Result<Option<Uuid>, AppError> {
        self.sessions()
            .get(&id)
            .map(|session| session.project_id)
            .ok_or_else(not_found)
    }

    /// Forgets every chat started in project `project_id` and returns their
    /// ids, so their replies can be cancelled.
    pub fn discard_project(&self, project_id: Uuid) -> Vec<Uuid> {
        let mut sessions = self.sessions();
        let ids: Vec<Uuid> = sessions
            .iter()
            .filter(|(_, session)| session.project_id == Some(project_id))
            .map(|(id, _)| *id)
            .collect();
        for id in &ids {
            sessions.remove(id);
        }
        ids
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

    /// The prompt `input` sends: its own, or for a regenerate the chat's
    /// last user message; `None` when there is none to regenerate.
    pub fn turn_prompt(&self, id: Uuid, input: &TurnInput) -> Result<Option<Redacted>, AppError> {
        if let Some(prompt) = input.prompt() {
            return Ok(Some(prompt.clone()));
        }
        let sessions = self.sessions();
        let session = sessions.get(&id).ok_or_else(not_found)?;
        Ok(session
            .messages
            .iter()
            .rev()
            .find(|message| message.message.role == ChatRole::User)
            .map(|message| message.message.content.clone()))
    }

    /// Forgets a chat. `false` when there was no such chat.
    pub fn remove(&self, id: Uuid) -> bool {
        self.sessions().remove(&id).is_some()
    }

    /// Under one lock, so a discard or settings change cannot land halfway:
    /// the history `input` sends with the chat's settings and its project's
    /// `context` (instructions and memory, read by the caller just before;
    /// memory fills only what `max_prompt_bytes` leaves), which `check` may
    /// refuse (size limits, settings changed) before anything changes. Then
    /// the prompt is kept, and a regenerate or edit sets aside the messages
    /// it replaces until its reply ends ([`MemoryRecorder`]).
    pub fn begin(
        &self,
        id: Uuid,
        context: &ProjectContext,
        max_prompt_bytes: usize,
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
            message: ChatMessage::new(ChatRole::User, prompt.clone()),
            charts: Vec::new(),
        });
        let history: Vec<ChatMessage> = session.messages[..cut.unwrap_or(session.messages.len())]
            .iter()
            .chain(&prompt)
            .flat_map(TempMessage::history)
            .collect();
        let history = with_system_message(context, &session.settings, history, max_prompt_bytes);
        check(&history, &session.settings)?;
        let options = session.settings.options();
        let charts = session.settings.charts;
        if let Some(base) = cut {
            let tail = session.messages.split_off(base);
            session.revision = Some(Revision { base, tail });
        }
        session.messages.extend(prompt);
        Ok(TempTurn {
            user_id,
            history,
            options,
            charts,
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
    charts: Vec<ChartEntry>,
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
            charts: Vec::new(),
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
                message: ChatMessage::new(
                    ChatRole::Assistant,
                    Redacted::new(std::mem::take(&mut self.content)),
                ),
                charts: std::mem::take(&mut self.charts),
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

    fn push_chart(&mut self, entry: &ChartEntry) -> Result<(), AppError> {
        if !self.finished {
            self.charts.push(entry.clone());
        }
        Ok(())
    }

    fn text(&self) -> String {
        self.content.clone()
    }

    fn complete(&mut self, _stats: ReplyStats) -> Result<(), AppError> {
        self.end(true);
        Ok(())
    }

    fn finish(&mut self, status: MessageStatus) -> Result<(), AppError> {
        // A chart the user can still see counts as visible content.
        let visible = !self.content.is_empty() || !self.charts.is_empty();
        self.end(status == MessageStatus::Cancelled && !self.revision && visible);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::testing::PROMPT_LIMIT;

    fn texts(history: &[ChatMessage]) -> Vec<(ChatRole, &str)> {
        history
            .iter()
            .map(|message| (message.role, message.content.expose()))
            .collect()
    }

    fn new(prompt: &str) -> TurnInput {
        TurnInput::New(Redacted::new(prompt))
    }

    /// The context of a chat outside any project.
    fn no_project() -> ProjectContext {
        ProjectContext::default()
    }

    /// Runs one turn of `input` that ends with `reply`, or fails when `None`.
    fn run(chats: &Arc<TempChats>, id: Uuid, input: &TurnInput, reply: Option<&str>) {
        chats
            .begin(id, &no_project(), PROMPT_LIMIT, input, |_, _| Ok(()))
            .unwrap();
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
            .begin(
                id,
                &no_project(),
                PROMPT_LIMIT,
                &TurnInput::Regenerate,
                |_, _| Ok(()),
            )
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
        chats
            .begin(id, &no_project(), PROMPT_LIMIT, &edit, |_, _| Ok(()))
            .unwrap();
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
                chats.begin(id, &no_project(), PROMPT_LIMIT, &edit, |_, _| Ok(())),
                Err(AppError::NotFound(_))
            ));
        }
        let empty = chats.start().unwrap();
        assert!(matches!(
            chats.begin(
                empty,
                &no_project(),
                PROMPT_LIMIT,
                &TurnInput::Regenerate,
                |_, _| Ok(())
            ),
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
        let id = chats.start_with(finnish.clone(), None).unwrap();
        assert_eq!(chats.settings(id).unwrap(), finnish);

        let mut seen = None;
        let turn = chats
            .begin(
                id,
                &no_project(),
                PROMPT_LIMIT,
                &new("Hi"),
                |_, settings| {
                    seen = Some(settings.clone());
                    Ok(())
                },
            )
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
        let turn = chats
            .begin(
                id,
                &no_project(),
                PROMPT_LIMIT,
                &new("Again"),
                |_, _| Ok(()),
            )
            .unwrap();
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
            chats.start_with(bad, None),
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
    fn temp_begin_puts_project_instructions_first() {
        let chats = TempChats::default();
        let project = Uuid::new_v4();
        let finnish = ConversationSettings {
            system_prompt: Redacted::new("Answer only in Finnish"),
            ..ConversationSettings::default()
        };
        let id = chats.start_with(finnish, Some(project)).unwrap();
        assert_eq!(chats.project(id).unwrap(), Some(project));

        let context = ProjectContext {
            instructions: Redacted::new("Cite sources."),
            ..ProjectContext::default()
        };
        let turn = chats
            .begin(id, &context, PROMPT_LIMIT, &new("Hi"), |_, _| Ok(()))
            .unwrap();
        assert_eq!(
            texts(&turn.history),
            [
                (
                    ChatRole::System,
                    "## Project instructions\n\nCite sources.\n\n## This chat\n\nAnswer only in Finnish"
                ),
                (ChatRole::User, "Hi")
            ]
        );
        assert_eq!(texts(&chats.history(id).unwrap()), [(ChatRole::User, "Hi")]);
        assert!(matches!(
            chats.project(Uuid::new_v4()),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn temp_turn_prompt_is_the_prompt_sent() {
        let (chats, id) = answered(&["One", "Two"]);
        assert!(chats.settings(id).unwrap().files);
        assert_eq!(
            chats.turn_prompt(id, &new("Three")).unwrap(),
            Some(Redacted::new("Three"))
        );
        assert_eq!(
            chats.turn_prompt(id, &TurnInput::Regenerate).unwrap(),
            Some(Redacted::new("Two"))
        );
        let empty = chats.start().unwrap();
        assert_eq!(
            chats.turn_prompt(empty, &TurnInput::Regenerate).unwrap(),
            None
        );
        assert!(matches!(
            chats.turn_prompt(Uuid::new_v4(), &TurnInput::Regenerate),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn temp_begin_adds_passages_for_its_prompt_only() {
        use crate::storage::files::SearchHit;

        let chats = TempChats::default();
        let project = Uuid::new_v4();
        let id = chats
            .start_with(ConversationSettings::default(), Some(project))
            .unwrap();
        let context = |query: &str| ProjectContext {
            files: vec![SearchHit {
                name: Redacted::new("a.txt"),
                start_line: 1,
                end_line: 1,
                snippet: Redacted::default(),
                text: Redacted::new("plover"),
            }],
            files_query: Redacted::new(query),
            ..ProjectContext::default()
        };
        let system = |context: &ProjectContext, prompt: &str| {
            let turn = chats
                .begin(id, context, PROMPT_LIMIT, &new(prompt), |_, _| Ok(()))
                .unwrap();
            chats.rollback(id);
            turn.history[0].content.expose().to_owned()
        };

        assert!(system(&context("Hi"), "Hi").contains("<<<file \"a.txt\" line 1>>>\nplover"));
        // Searched for another prompt than the one sent: left out.
        assert_eq!(system(&context("Other"), "Hi"), "Hi");

        chats
            .update_settings(
                id,
                ConversationSettings {
                    files: false,
                    ..ConversationSettings::default()
                },
            )
            .unwrap();
        assert_eq!(system(&context("Hi"), "Hi"), "Hi");
    }

    #[test]
    fn discard_project_removes_only_its_sessions() {
        let chats = TempChats::default();
        let project = Uuid::new_v4();
        let first = chats
            .start_with(ConversationSettings::default(), Some(project))
            .unwrap();
        let second = chats
            .start_with(ConversationSettings::default(), Some(project))
            .unwrap();
        let other = chats
            .start_with(ConversationSettings::default(), Some(Uuid::new_v4()))
            .unwrap();
        let loose = chats.start().unwrap();

        let mut discarded = chats.discard_project(project);
        discarded.sort();
        let mut expected = vec![first, second];
        expected.sort();
        assert_eq!(discarded, expected);
        assert!(chats.history(first).is_none());
        assert!(chats.history(other).is_some());
        assert!(chats.history(loose).is_some());
        assert!(chats.discard_project(project).is_empty());
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
            .begin(id, &no_project(), PROMPT_LIMIT, &new("Hi"), |_, _| {
                Err(AppError::TooLarge("big".into()))
            })
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
                .begin(id, &no_project(), PROMPT_LIMIT, &edit, |_, _| Err(
                    AppError::TooLarge("big".into())
                ))
                .is_err()
        );
        assert_eq!(chats.history(id).unwrap(), before);
    }

    #[test]
    fn begin_unknown_chat_is_not_found() {
        let chats = TempChats::default();
        assert!(matches!(
            chats.begin(
                Uuid::new_v4(),
                &no_project(),
                PROMPT_LIMIT,
                &new("Hi"),
                |_, _| Ok(())
            ),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn rollback_takes_back_the_prompt() {
        let chats = TempChats::default();
        let id = chats.start().unwrap();
        chats
            .begin(id, &no_project(), PROMPT_LIMIT, &new("Hi"), |_, _| Ok(()))
            .unwrap();

        chats.rollback(id);

        assert_eq!(chats.history(id).unwrap(), []);
    }

    #[test]
    fn temp_history_accumulates_complete_and_cancelled() {
        let chats = Arc::new(TempChats::default());
        let id = chats.start().unwrap();
        let turn = |prompt: &str, reply: &str, status: Option<MessageStatus>| {
            chats
                .begin(id, &no_project(), PROMPT_LIMIT, &new(prompt), |_, _| Ok(()))
                .unwrap();
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
    fn temp_history_replays_chart() {
        use crate::ollama::chart_tool::{ChartKind, ChartSpec};

        let chats = Arc::new(TempChats::default());
        let id = chats
            .start_with(
                ConversationSettings {
                    charts: true,
                    ..ConversationSettings::default()
                },
                None,
            )
            .unwrap();
        let turn = chats
            .begin(id, &no_project(), PROMPT_LIMIT, &new("Chart it"), |_, _| {
                Ok(())
            })
            .unwrap();
        assert!(turn.charts);
        let mut recorder = MemoryRecorder::new(&chats, id, Uuid::new_v4(), false);
        recorder.push(&Redacted::new("Here:")).unwrap();
        recorder
            .push_chart(&ChartEntry {
                offset: 5,
                spec: Some(ChartSpec {
                    title: "t".into(),
                    kind: ChartKind::Pie,
                    labels: vec!["a".into()],
                    values: vec![1.0],
                }),
                error: None,
            })
            .unwrap();
        assert_eq!(recorder.text(), "Here:");
        recorder.push(&Redacted::new(" Done.")).unwrap();
        recorder.complete(ReplyStats::default()).unwrap();

        let next = chats
            .begin(id, &no_project(), PROMPT_LIMIT, &new("Thanks"), |_, _| {
                Ok(())
            })
            .unwrap();
        assert_eq!(
            texts(&next.history),
            [
                (ChatRole::User, "Chart it"),
                (ChatRole::Assistant, "Here:"),
                (ChatRole::Tool, "The chart was shown to the user."),
                (ChatRole::Assistant, " Done."),
                (ChatRole::User, "Thanks"),
            ]
        );
        assert!(next.history[1].tool_calls.is_some());

        // Stopped after a chart but before any text: the chart is kept.
        chats.rollback(id);
        let mut stopped = MemoryRecorder::new(&chats, id, Uuid::new_v4(), false);
        stopped
            .push_chart(&ChartEntry {
                offset: 0,
                spec: None,
                error: Some("why".into()),
            })
            .unwrap();
        stopped.finish(MessageStatus::Cancelled).unwrap();
        let roles: Vec<ChatRole> = chats
            .begin(
                id,
                &no_project(),
                PROMPT_LIMIT,
                &new("Once more"),
                |_, _| Ok(()),
            )
            .unwrap()
            .history
            .iter()
            .map(|message| message.role)
            .collect();
        assert_eq!(
            roles[roles.len() - 3..],
            [ChatRole::Assistant, ChatRole::Tool, ChatRole::User]
        );
    }

    #[test]
    fn discard_then_late_finish_is_noop() {
        let chats = Arc::new(TempChats::default());
        let id = chats.start().unwrap();
        chats
            .begin(id, &no_project(), PROMPT_LIMIT, &new("Hi"), |_, _| Ok(()))
            .unwrap();
        let mut recorder = MemoryRecorder::new(&chats, id, Uuid::new_v4(), false);
        recorder.push(&Redacted::new("Late")).unwrap();

        chats.remove(id);
        recorder.complete(ReplyStats::default()).unwrap();

        assert!(chats.history(id).is_none());
    }
}

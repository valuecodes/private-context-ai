//! Ollama status, model list and streaming chat.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::AppError;
use crate::memory::MemoryScheduler;
use crate::ollama::chart_tool::{ChartEntry, without_tools};
use crate::ollama::file_tools::{
    FileCall, FileLookup, FileRunner, LIST_PAGE, SEARCH_HITS, list_result, read_result,
    search_result,
};
use crate::ollama::stream::ChatEnd;
use crate::ollama::types::{
    ChatEvent, ChatMessage, ModelCapabilities, ModelInfo, OllamaStatus, ThinkChoice, validate_model,
};
use crate::ollama::{ChatCall, ChatThink, FollowUp, Ollama, ToolSet};
use crate::redacted::Redacted;
use crate::storage::conversations::ConversationSettings;
use crate::storage::messages::{FlushPolicy, MessageStatus, ReplyStats, TurnInput, TurnRecorder};
use crate::storage::{Storage, StorageState};

/// A reachable Ollama also resumes background memory.
#[tauri::command]
pub async fn ollama_status(
    ollama: State<'_, Arc<Ollama>>,
    scheduler: State<'_, Arc<MemoryScheduler>>,
) -> Result<OllamaStatus, AppError> {
    let status = ollama.status().await?;
    if status.reachable {
        scheduler.ollama_reachable();
    }
    Ok(status)
}

/// What a saved chat's completed reply tells the memory scheduler.
fn reply_hook(
    scheduler: &Arc<MemoryScheduler>,
    conversation_id: Uuid,
) -> impl FnOnce() + Send + 'static {
    let scheduler = Arc::clone(scheduler);
    move || scheduler.reply_completed(conversation_id)
}

#[tauri::command]
pub async fn ollama_list_models(
    ollama: State<'_, Arc<Ollama>>,
) -> Result<Vec<ModelInfo>, AppError> {
    ollama.list_models().await
}

#[tauri::command]
pub async fn ollama_model_capabilities(
    ollama: State<'_, Arc<Ollama>>,
    model: String,
) -> Result<ModelCapabilities, AppError> {
    ollama.model_capabilities(&model).await
}

/// What a send command returns once Rust admitted it; mirrored by
/// `SendStarted` in `src/lib/types.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendStarted {
    pub request_id: Uuid,
    /// The prompt being answered: new, or the existing one when
    /// regenerating.
    pub user_message_id: Uuid,
    pub assistant_message_id: Uuid,
}

/// One send, saved or temporary. For a temporary chat `conversation_id` is
/// its session id. Options and the system prompt come from the chat's
/// settings when the turn begins.
pub struct SendRequest {
    pub conversation_id: Uuid,
    pub model: String,
    pub input: TurnInput,
    /// The chat's Think setting that `think` was resolved from.
    pub think_choice: ThinkChoice,
    /// From [`Ollama::chat_think`].
    pub think: ChatThink,
}

/// Refuses a turn whose settings changed after its `think` was resolved,
/// then checks the history against the client's limits.
pub(crate) fn check_turn(
    ollama: &Ollama,
    model: &str,
    think_choice: &ThinkChoice,
    history: &[ChatMessage],
    settings: &ConversationSettings,
) -> Result<(), AppError> {
    if settings.think != *think_choice {
        return Err(AppError::InvalidInput(
            "the chat settings changed while sending; send again".into(),
        ));
    }
    ollama.validate(model, history, settings.options().as_ref())
}

/// The saved conversation's Think setting and what it resolves to for
/// `model`. A failed lookup fails the send: without it a thinking model
/// would get Ollama's default instead of an explicit `false`.
async fn saved_think(
    ollama: &Ollama,
    storage: &Storage,
    conversation_id: Uuid,
    model: &str,
) -> Result<(ThinkChoice, ChatThink), AppError> {
    let choice = storage.conversation_settings(conversation_id)?.think;
    let think = ollama.chat_think(model, choice.requested()).await?;
    Ok((choice, think))
}

/// A failed send means the webview is gone; the worker then cancels.
pub(crate) fn channel_sink(
    on_event: Channel<ChatEvent>,
) -> impl Fn(ChatEvent) -> bool + Send + Sync + 'static {
    move |event| on_event.send(event).is_ok()
}

/// An empty new prompt is refused before anything else.
pub(crate) fn check_prompt(input: &TurnInput) -> Result<(), AppError> {
    if input
        .prompt()
        .is_some_and(|prompt| prompt.expose().trim().is_empty())
    {
        return Err(AppError::InvalidInput("the message is empty".into()));
    }
    Ok(())
}

/// Saves `prompt` to the conversation, starts streaming the reply into
/// `on_event` and returns the new rows' ids at once. Rust builds the
/// history from the database and sends it with the conversation's
/// settings.
#[tauri::command]
pub async fn chat_send(
    ollama: State<'_, Arc<Ollama>>,
    storage: State<'_, StorageState>,
    scheduler: State<'_, Arc<MemoryScheduler>>,
    conversation_id: Uuid,
    model: String,
    prompt: Redacted,
    on_event: Channel<ChatEvent>,
) -> Result<SendStarted, AppError> {
    let storage = Arc::clone(storage.get()?);
    let (think_choice, think) = saved_think(&ollama, &storage, conversation_id, &model).await?;
    send_turn_then(
        &ollama,
        storage,
        SendRequest {
            conversation_id,
            model,
            input: TurnInput::New(prompt),
            think_choice,
            think,
        },
        FlushPolicy::default(),
        channel_sink(on_event),
        reply_hook(&scheduler, conversation_id),
    )
}

/// Like `chat_send`, but streams a new reply to the last prompt. The old
/// reply is deleted only once the new one completes.
#[tauri::command]
pub async fn chat_regenerate(
    ollama: State<'_, Arc<Ollama>>,
    storage: State<'_, StorageState>,
    scheduler: State<'_, Arc<MemoryScheduler>>,
    conversation_id: Uuid,
    model: String,
    on_event: Channel<ChatEvent>,
) -> Result<SendStarted, AppError> {
    let storage = Arc::clone(storage.get()?);
    let (think_choice, think) = saved_think(&ollama, &storage, conversation_id, &model).await?;
    send_turn_then(
        &ollama,
        storage,
        SendRequest {
            conversation_id,
            model,
            input: TurnInput::Regenerate,
            think_choice,
            think,
        },
        FlushPolicy::default(),
        channel_sink(on_event),
        reply_hook(&scheduler, conversation_id),
    )
}

/// Like `chat_send`, but replaces the user message `message_id` and
/// everything after it with `prompt` and its reply. Nothing is deleted
/// until that reply completes.
#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "Tauri injects the three states; the rest is the IPC payload"
)]
pub async fn chat_edit(
    ollama: State<'_, Arc<Ollama>>,
    storage: State<'_, StorageState>,
    scheduler: State<'_, Arc<MemoryScheduler>>,
    conversation_id: Uuid,
    message_id: Uuid,
    model: String,
    prompt: Redacted,
    on_event: Channel<ChatEvent>,
) -> Result<SendStarted, AppError> {
    let storage = Arc::clone(storage.get()?);
    let (think_choice, think) = saved_think(&ollama, &storage, conversation_id, &model).await?;
    send_turn_then(
        &ollama,
        storage,
        SendRequest {
            conversation_id,
            model,
            input: TurnInput::Edit { message_id, prompt },
            think_choice,
            think,
        },
        FlushPolicy::default(),
        channel_sink(on_event),
        reply_hook(&scheduler, conversation_id),
    )
}

#[tauri::command]
pub async fn chat_cancel(ollama: State<'_, Arc<Ollama>>, request_id: Uuid) -> Result<(), AppError> {
    ollama.chat_cancel(request_id).await
}

/// Where a streamed reply is kept: the database for a saved chat
/// ([`TurnRecorder`]), process memory for a temporary one.
pub trait ReplyRecorder: Send + 'static {
    fn push(&mut self, token: &Redacted) -> Result<(), AppError>;
    fn push_thinking(&mut self, chunk: &Redacted) -> Result<(), AppError>;
    fn push_chart(&mut self, entry: &ChartEntry) -> Result<(), AppError>;
    fn push_file_lookup(&mut self, lookup: &FileLookup) -> Result<(), AppError>;
    /// The answer so far, which a follow-up request sends back.
    fn text(&self) -> String;
    fn complete(&mut self, stats: ReplyStats) -> Result<(), AppError>;
    fn finish(&mut self, status: MessageStatus) -> Result<(), AppError>;
}

impl ReplyRecorder for TurnRecorder {
    fn push(&mut self, token: &Redacted) -> Result<(), AppError> {
        Self::push(self, token)
    }

    fn push_thinking(&mut self, chunk: &Redacted) -> Result<(), AppError> {
        Self::push_thinking(self, chunk)
    }

    fn push_chart(&mut self, entry: &ChartEntry) -> Result<(), AppError> {
        Self::push_chart(self, entry)
    }

    fn push_file_lookup(&mut self, lookup: &FileLookup) -> Result<(), AppError> {
        Self::push_file_lookup(self, lookup)
    }

    fn text(&self) -> String {
        Self::text(self)
    }

    fn complete(&mut self, stats: ReplyStats) -> Result<(), AppError> {
        Self::complete(self, stats)
    }

    fn finish(&mut self, status: MessageStatus) -> Result<(), AppError> {
        Self::finish(self, status)
    }
}

pub(crate) type Shared<R> = Arc<Mutex<R>>;
pub(crate) type SharedRecorder = Shared<TurnRecorder>;

pub(crate) fn lock<R>(recorder: &Shared<R>) -> MutexGuard<'_, R> {
    recorder.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The history as `model` is sent it: a model whose capabilities do not list
/// `tools` gets earlier charted replies as plain text, without tool calls or
/// tool messages its template may reject.
pub(crate) fn model_history(history: Vec<ChatMessage>, tools: bool) -> Vec<ChatMessage> {
    if tools {
        history
    } else {
        without_tools(&history)
    }
}

/// What a reply that called tools continues with: `history`, the answer so
/// far from `recorder`, and `files` to run file calls. `history` is only
/// cloned when tools are offered.
pub(crate) fn tool_follow_up<R: ReplyRecorder>(
    recorder: &Shared<R>,
    history: &[ChatMessage],
    tools: ToolSet,
    files: Option<FileRunner>,
) -> FollowUp {
    if !tools.charts && !tools.files {
        return FollowUp::none();
    }
    let recorder = Arc::clone(recorder);
    FollowUp {
        history: history.to_vec(),
        text: Box::new(move || lock(&recorder).text()),
        files: files.filter(|_| tools.files),
    }
}

/// Runs the file tools against project `project`'s library.
pub(crate) fn file_runner(storage: Arc<Storage>, project: Uuid) -> FileRunner {
    Arc::new(move |call| match call {
        FileCall::List { start } => Ok(list_result(
            &storage.list_files(project, start - 1, LIST_PAGE)?,
            *start,
        )),
        FileCall::Search { query } => Ok(search_result(
            query,
            &storage.search(project, query, SEARCH_HITS)?,
        )),
        FileCall::Read {
            name,
            start_line,
            line_count,
        } => Ok(read_result(
            name,
            &storage.read_lines(project, name, *start_line, *line_count)?,
        )),
    })
}

/// What a turn offers: charts with Charts on, the file tools with a
/// searchable project, both only to a model that lists `tools`.
pub(crate) fn turn_tools(think: &ChatThink, charts: bool, files: bool) -> ToolSet {
    ToolSet {
        charts: charts && think.tools,
        files: files && think.tools,
    }
}

/// The body of the send commands, without Tauri. Order matters: the slot is
/// taken before anything is written, and nothing is written when the
/// history is refused, so a refused send leaves no rows behind and a
/// refused regenerate or edit replaces nothing. A base URL switched, or a
/// Think setting changed, since `think` was resolved is refused before
/// anything is written.
pub fn send_turn<S>(
    ollama: &Arc<Ollama>,
    storage: Arc<Storage>,
    request: SendRequest,
    policy: FlushPolicy,
    send: S,
) -> Result<SendStarted, AppError>
where
    S: Fn(ChatEvent) -> bool + Send + Sync + 'static,
{
    send_turn_then(ollama, storage, request, policy, send, || {})
}

/// [`send_turn`], calling `on_complete` once the reply is saved complete.
pub fn send_turn_then<S, C>(
    ollama: &Arc<Ollama>,
    storage: Arc<Storage>,
    request: SendRequest,
    policy: FlushPolicy,
    send: S,
    on_complete: C,
) -> Result<SendStarted, AppError>
where
    S: Fn(ChatEvent) -> bool + Send + Sync + 'static,
    C: FnOnce() + Send + 'static,
{
    let SendRequest {
        conversation_id,
        model,
        input,
        think_choice,
        think,
    } = request;
    check_prompt(&input)?;
    validate_model(&model)?;
    let admission = ollama.admit(conversation_id)?;
    ollama.check_connection(think.connection)?;
    let turn = storage.begin_turn(
        conversation_id,
        &model,
        &input,
        ollama.max_prompt_bytes(),
        |history, settings| check_turn(ollama, &model, &think_choice, history, settings),
    )?;

    let tools = turn_tools(&think, turn.charts, turn.files_project.is_some());
    let files = turn
        .files_project
        .map(|project| file_runner(Arc::clone(&storage), project));
    let recorder: SharedRecorder = Arc::new(Mutex::new(TurnRecorder::new(
        storage,
        turn.assistant_id,
        policy,
    )));
    let stop_ticker = CancellationToken::new();
    spawn_ticker(Arc::clone(&recorder), policy.every, stop_ticker.clone());
    let sink = recording_sink(Arc::clone(&recorder), send);
    let history = model_history(turn.history, think.tools);
    let follow_up = tool_follow_up(&recorder, &history, tools, files);
    let request_id = ollama.start(
        admission,
        ChatCall {
            model: &model,
            messages: &history,
            options: turn.options.as_ref(),
            think,
            keep_alive: None,
            tools,
        },
        sink,
        follow_up,
        move |end| {
            stop_ticker.cancel();
            if finish_turn(&recorder, end) {
                on_complete();
            }
        },
    )?;
    Ok(SendStarted {
        request_id,
        user_message_id: turn.user_id,
        assistant_message_id: turn.assistant_id,
    })
}

/// Saves each event before the frontend sees it. A failed save becomes a
/// `database` error for the frontend and stops the stream; on `Done` that
/// error replaces the `Done`, so the UI never shows an unsaved reply as
/// complete. An `Error` ends the reply first, so a regenerate or edit is
/// already undone when the frontend reloads on it.
pub(crate) fn recording_sink<R, S>(
    recorder: Shared<R>,
    send: S,
) -> impl Fn(ChatEvent) -> bool + Send + Sync + 'static
where
    R: ReplyRecorder,
    S: Fn(ChatEvent) -> bool + Send + Sync + 'static,
{
    move |event| {
        let saved = match &event {
            ChatEvent::Token { content } => lock(&recorder).push(content),
            ChatEvent::Thinking { content } => lock(&recorder).push_thinking(content),
            ChatEvent::Chart { offset, spec } => lock(&recorder).push_chart(&ChartEntry {
                offset: *offset,
                spec: Some(spec.clone()),
                error: None,
            }),
            ChatEvent::ChartError { offset, reason } => lock(&recorder).push_chart(&ChartEntry {
                offset: *offset,
                spec: None,
                error: Some(reason.clone()),
            }),
            ChatEvent::FileLookup { lookup } => lock(&recorder).push_file_lookup(lookup),
            ChatEvent::Done {
                prompt_tokens,
                completion_tokens,
                duration_ms,
                thinking_ms,
                eval_ms,
                ..
            } => lock(&recorder).complete(ReplyStats {
                prompt_tokens: *prompt_tokens,
                completion_tokens: *completion_tokens,
                duration_ms: *duration_ms,
                thinking_ms: *thinking_ms,
                eval_ms: *eval_ms,
            }),
            // `on_end` then finds the reply finished. A save failure the
            // ticker already hit matters more than why Ollama stopped.
            ChatEvent::Error { .. } => lock(&recorder).finish(MessageStatus::Error),
        };
        match saved {
            Ok(()) => send(event),
            Err(error) => {
                tracing::warn!(kind = error.kind(), "could not save the reply");
                send(ChatEvent::Error { error });
                false
            }
        }
    }
}

/// Writes the final status. The sink has already saved a delivered `Done`,
/// so this matters for cancel, failure, and a `Done` nobody received.
/// `true` when the reply was saved complete.
pub(crate) fn finish_turn<R: ReplyRecorder>(recorder: &Shared<R>, end: &ChatEnd) -> bool {
    let mut recorder = lock(recorder);
    let result = match end {
        ChatEnd::Done {
            prompt_tokens,
            completion_tokens,
            duration_ms,
            thinking_ms,
            eval_ms,
            ..
        } => recorder.complete(ReplyStats {
            prompt_tokens: *prompt_tokens,
            completion_tokens: *completion_tokens,
            duration_ms: *duration_ms,
            thinking_ms: *thinking_ms,
            eval_ms: *eval_ms,
        }),
        ChatEnd::Cancelled => recorder.finish(MessageStatus::Cancelled),
        ChatEnd::Failed(_) => recorder.finish(MessageStatus::Error),
    };
    match result {
        Ok(()) => matches!(end, ChatEnd::Done { .. }),
        Err(error) => {
            // The row stays `pending` and becomes `error` at the next start.
            tracing::warn!(kind = error.kind(), "could not save the end of the reply");
            false
        }
    }
}

/// Flushes buffered tokens every `every`, so a short reply followed by
/// silence is on disk even though no further token triggers a write. A
/// failed flush marks the recorder failed; the next event then stops the
/// stream.
pub(crate) fn spawn_ticker(recorder: SharedRecorder, every: Duration, stop: CancellationToken) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick completes at once.
        interval.tick().await;
        loop {
            tokio::select! {
                () = stop.cancelled() => return,
                _ = interval.tick() => {}
            }
            if let Err(error) = lock(&recorder).flush_if_dirty() {
                tracing::warn!(kind = error.kind(), "could not save the reply");
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::Audit;
    use crate::commands::testing::{no_think, silent_ollama};
    use crate::ollama::Limits;
    use crate::ollama::client::OllamaBaseUrl;
    use crate::ollama::file_tools::{FILE_TOOLS, LookupKind};
    use crate::ollama::testing::{Reply, fake_ollama};
    use crate::ollama::types::Think;
    use crate::storage::memory::FILES_PREAMBLE;
    use crate::storage::messages::StoredMessage;
    use crate::storage::testing::{PROMPT_LIMIT, break_writes, count_rows, temp_storage};

    type Events = Arc<Mutex<Vec<ChatEvent>>>;

    fn collector(accept: bool) -> (Events, impl Fn(ChatEvent) -> bool + Send + Sync + 'static) {
        let events: Events = Arc::default();
        let store = Arc::clone(&events);
        (events, move |event| {
            store.lock().unwrap().push(event);
            accept
        })
    }

    fn started_turn(storage: &Arc<Storage>) -> (Uuid, SharedRecorder) {
        started_turn_with(storage, FlushPolicy::default())
    }

    fn started_turn_with(storage: &Arc<Storage>, policy: FlushPolicy) -> (Uuid, SharedRecorder) {
        let conversation = storage.create_conversation(None, "m").unwrap().id;
        let turn = storage
            .begin_turn(
                conversation,
                "m",
                &TurnInput::New(Redacted::new("Hi")),
                PROMPT_LIMIT,
                |_, _| Ok(()),
            )
            .unwrap();
        let recorder = Arc::new(Mutex::new(TurnRecorder::new(
            Arc::clone(storage),
            turn.assistant_id,
            policy,
        )));
        (conversation, recorder)
    }

    fn assistant(storage: &Storage, conversation: Uuid) -> StoredMessage {
        storage
            .get_conversation(conversation)
            .unwrap()
            .messages
            .pop()
            .unwrap()
    }

    fn token(text: &str) -> ChatEvent {
        ChatEvent::Token {
            content: Redacted::new(text),
        }
    }

    fn done() -> ChatEvent {
        ChatEvent::Done {
            prompt_tokens: Some(1),
            completion_tokens: Some(2),
            duration_ms: Some(3),
            done_reason: Some("stop".into()),
            thinking_ms: Some(4),
            eval_ms: Some(5),
        }
    }

    #[test]
    fn sink_saves_thinking() {
        let (storage, _dir) = temp_storage();
        let (conversation, recorder) = started_turn(&storage);
        let (events, send) = collector(true);
        let sink = recording_sink(recorder, send);

        assert!(sink(ChatEvent::Thinking {
            content: Redacted::new("Hmm")
        }));
        assert!(sink(token("Hi")));
        assert!(sink(done()));

        assert_eq!(events.lock().unwrap().len(), 3);
        let saved = assistant(&storage, conversation);
        assert_eq!(saved.thinking.unwrap().expose(), "Hmm");
        assert_eq!(saved.thinking_ms, Some(4));
        assert_eq!(saved.content.expose(), "Hi");
    }

    #[test]
    fn sink_saves_tokens_then_done() {
        let (storage, _dir) = temp_storage();
        let (conversation, recorder) = started_turn(&storage);
        let (events, send) = collector(true);
        let sink = recording_sink(recorder, send);

        assert!(sink(token("Hel")));
        assert!(sink(token("lo")));
        assert!(sink(done()));

        assert_eq!(events.lock().unwrap().last(), Some(&done()));
        let saved = assistant(&storage, conversation);
        assert_eq!(saved.content.expose(), "Hello");
        assert_eq!(saved.status, MessageStatus::Complete);
        assert_eq!(saved.duration_ms, Some(3));
        assert_eq!(saved.eval_ms, Some(5));
    }

    #[test]
    fn flush_failure_sends_database_error_and_stops() {
        let (storage, _dir) = temp_storage();
        let (_, recorder) = started_turn_with(
            &storage,
            FlushPolicy {
                tokens: 1,
                ..FlushPolicy::default()
            },
        );
        let (events, send) = collector(true);
        let sink = recording_sink(recorder, send);
        break_writes(&storage);

        assert!(!sink(token("x")));
        let events = events.lock().unwrap();
        assert!(matches!(
            events.as_slice(),
            [ChatEvent::Error {
                error: AppError::Database(_)
            }]
        ));
    }

    #[test]
    fn complete_failure_replaces_done() {
        let (storage, _dir) = temp_storage();
        let (_, recorder) = started_turn(&storage);
        let (events, send) = collector(true);
        let sink = recording_sink(Arc::clone(&recorder), send);

        assert!(sink(token("Hi")));
        break_writes(&storage);
        assert!(!sink(done()));

        let events = events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[1],
            ChatEvent::Error {
                error: AppError::Database(_)
            }
        ));
    }

    #[test]
    fn earlier_flush_failure_replaces_ollama_error() {
        let (storage, _dir) = temp_storage();
        let (_, recorder) = started_turn(&storage);
        let (events, send) = collector(true);
        let sink = recording_sink(Arc::clone(&recorder), send);

        assert!(sink(token("Hi")));
        break_writes(&storage);
        // What the ticker does between events.
        assert!(lock(&recorder).flush_if_dirty().is_err());
        assert!(!sink(ChatEvent::Error {
            error: AppError::Timeout("slow".into())
        }));

        let events = events.lock().unwrap();
        assert!(matches!(
            events.last(),
            Some(ChatEvent::Error {
                error: AppError::Database(_)
            })
        ));
    }

    #[test]
    fn finish_turn_records_cancel_and_failure() {
        let (storage, _dir) = temp_storage();
        let (conversation, recorder) = started_turn(&storage);
        lock(&recorder).push(&Redacted::new("Par")).unwrap();
        finish_turn(&recorder, &ChatEnd::Cancelled);
        let saved = assistant(&storage, conversation);
        assert_eq!(saved.content.expose(), "Par");
        assert_eq!(saved.status, MessageStatus::Cancelled);

        let (conversation, recorder) = started_turn(&storage);
        finish_turn(
            &recorder,
            &ChatEnd::Failed(AppError::Timeout("slow".into())),
        );
        assert_eq!(
            assistant(&storage, conversation).status,
            MessageStatus::Error
        );
    }

    #[tokio::test]
    async fn ticker_flushes_after_silence() {
        let (storage, _dir) = temp_storage();
        let (conversation, recorder) = started_turn(&storage);
        lock(&recorder).push(&Redacted::new("Hi")).unwrap();
        let stop = CancellationToken::new();

        spawn_ticker(
            Arc::clone(&recorder),
            Duration::from_millis(50),
            stop.clone(),
        );
        let mut saved = String::new();
        for _ in 0..100 {
            saved = assistant(&storage, conversation)
                .content
                .expose()
                .to_owned();
            if !saved.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        stop.cancel();
        assert_eq!(saved, "Hi");
    }

    fn request(ollama: &Ollama, conversation: Uuid, input: TurnInput) -> SendRequest {
        SendRequest {
            conversation_id: conversation,
            model: "llama3.2".into(),
            input,
            think_choice: ThinkChoice::Off,
            think: no_think(ollama),
        }
    }

    fn send_input(
        ollama: &Arc<Ollama>,
        storage: &Arc<Storage>,
        conversation: Uuid,
        input: TurnInput,
    ) -> Result<SendStarted, AppError> {
        send_turn(
            ollama,
            Arc::clone(storage),
            request(ollama, conversation, input),
            FlushPolicy::default(),
            |_| true,
        )
    }

    fn send(
        ollama: &Arc<Ollama>,
        storage: &Arc<Storage>,
        conversation: Uuid,
    ) -> Result<SendStarted, AppError> {
        send_input(
            ollama,
            storage,
            conversation,
            TurnInput::New(Redacted::new("Hi")),
        )
    }

    /// A conversation with one answered turn.
    fn answered(storage: &Arc<Storage>) -> (Uuid, Vec<StoredMessage>) {
        let (conversation, recorder) = started_turn(storage);
        lock(&recorder).push(&Redacted::new("Hello")).unwrap();
        lock(&recorder).complete(ReplyStats::default()).unwrap();
        let messages = storage.get_conversation(conversation).unwrap().messages;
        (conversation, messages)
    }

    fn messages(storage: &Storage, conversation: Uuid) -> Vec<StoredMessage> {
        storage.get_conversation(conversation).unwrap().messages
    }

    #[test]
    fn sink_error_restores_before_forwarding() {
        let (storage, _dir) = temp_storage();
        let (conversation, before) = answered(&storage);
        let turn = storage
            .begin_turn(
                conversation,
                "m",
                &TurnInput::Regenerate,
                PROMPT_LIMIT,
                |_, _| Ok(()),
            )
            .unwrap();
        let recorder = Arc::new(Mutex::new(TurnRecorder::new(
            Arc::clone(&storage),
            turn.assistant_id,
            FlushPolicy::default(),
        )));
        let seen: Arc<Mutex<Option<Vec<StoredMessage>>>> = Arc::default();
        let (reader, store) = (Arc::clone(&storage), Arc::clone(&seen));
        let sink = recording_sink(recorder, move |event| {
            if matches!(event, ChatEvent::Error { .. }) {
                *store.lock().unwrap() = Some(messages(&reader, conversation));
            }
            true
        });

        assert!(sink(token("Par")));
        assert!(sink(ChatEvent::Error {
            error: AppError::Timeout("slow".into())
        }));

        // What a reload on the `error` event would see.
        assert_eq!(seen.lock().unwrap().as_ref(), Some(&before));
    }

    #[tokio::test]
    async fn send_started_ids_match_rows() {
        let (ollama, _listener) = silent_ollama().await;
        let (storage, _dir) = temp_storage();
        let conversation = storage.create_conversation(None, "m").unwrap().id;

        let started = send(&ollama, &storage, conversation).unwrap();

        let rows = messages(&storage, conversation);
        assert_eq!(rows[0].id, started.user_message_id);
        assert_eq!(rows[1].id, started.assistant_message_id);
        ollama.cancel_conversation(conversation).await;
    }

    #[tokio::test]
    async fn regenerate_busy_while_streaming() {
        let (ollama, _listener) = silent_ollama().await;
        let (storage, _dir) = temp_storage();
        let (conversation, _) = answered(&storage);
        send(&ollama, &storage, conversation).unwrap();
        let before = messages(&storage, conversation);

        assert!(matches!(
            send_input(&ollama, &storage, conversation, TurnInput::Regenerate),
            Err(AppError::Busy(_))
        ));
        assert_eq!(messages(&storage, conversation), before);
        ollama.cancel_conversation(conversation).await;
    }

    #[tokio::test]
    async fn regenerate_connection_refused_leaves_conversation_unchanged() {
        let (ollama, listener) = silent_ollama().await;
        // Nothing listens any more, so the connect is refused.
        drop(listener);
        let (storage, _dir) = temp_storage();
        let (conversation, before) = answered(&storage);
        let (events, sink) = collector(true);

        let started = send_turn(
            &ollama,
            Arc::clone(&storage),
            request(&ollama, conversation, TurnInput::Regenerate),
            FlushPolicy::default(),
            sink,
        )
        .unwrap();
        assert_eq!(started.user_message_id, before[0].id);
        // Windows takes about 2 s to refuse a loopback connect.
        for _ in 0..1000 {
            if ollama.inflight_len() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(ollama.inflight_len(), 0);
        assert!(matches!(
            events.lock().unwrap().last(),
            Some(ChatEvent::Error { .. })
        ));
        assert_eq!(messages(&storage, conversation), before);
    }

    #[tokio::test]
    async fn refused_history_writes_nothing_and_frees_slot() {
        let (ollama, _listener) = silent_ollama().await;
        let (storage, _dir) = temp_storage();
        let conversation = storage.create_conversation(None, "m").unwrap().id;

        let err = send_input(
            &ollama,
            &storage,
            conversation,
            TurnInput::New(Redacted::new("x".repeat(2 * 1024 * 1024))),
        )
        .unwrap_err();

        assert!(matches!(err, AppError::TooLarge(_)));
        assert!(
            storage
                .get_conversation(conversation)
                .unwrap()
                .messages
                .is_empty()
        );
        assert_eq!(ollama.inflight_len(), 0);
    }

    #[tokio::test]
    async fn delete_then_send_is_not_found() {
        let (ollama, _listener) = silent_ollama().await;
        let (storage, _dir) = temp_storage();
        let conversation = storage.create_conversation(None, "m").unwrap().id;
        storage.delete_conversation(conversation).unwrap();

        assert!(matches!(
            send(&ollama, &storage, conversation),
            Err(AppError::NotFound(_))
        ));
        assert_eq!(ollama.inflight_len(), 0);
    }

    #[tokio::test]
    async fn send_admitted_before_delete_is_cancelled() {
        let (ollama, _listener) = silent_ollama().await;
        let (storage, _dir) = temp_storage();
        let conversation = storage.create_conversation(None, "m").unwrap().id;

        send(&ollama, &storage, conversation).unwrap();
        assert_eq!(ollama.inflight_len(), 1);
        storage.delete_conversation(conversation).unwrap();
        ollama.cancel_conversation(conversation).await;

        assert_eq!(ollama.inflight_len(), 0);
        assert!(storage.list_conversations().unwrap().is_empty());
    }

    #[tokio::test]
    async fn send_after_url_switch_writes_nothing() {
        let (ollama, listener) = silent_ollama().await;
        let (storage, _dir) = temp_storage();
        let conversation = storage.create_conversation(None, "m").unwrap().id;
        let think = ChatThink {
            think: Some(Think::Enabled(false)),
            connection: ollama.connection_id(),
            tools: false,
        };
        // Settings switch the URL between the lookup and the send.
        ollama.set_base_url(OllamaBaseUrl::parse(&format!(
            "http://{}",
            listener.local_addr().unwrap()
        )));

        let err = send_turn(
            &ollama,
            Arc::clone(&storage),
            SendRequest {
                think,
                ..request(&ollama, conversation, TurnInput::New(Redacted::new("Hi")))
            },
            FlushPolicy::default(),
            |_| true,
        )
        .unwrap_err();

        assert!(matches!(err, AppError::Ollama(_)));
        assert!(
            storage
                .get_conversation(conversation)
                .unwrap()
                .messages
                .is_empty()
        );
        assert_eq!(ollama.inflight_len(), 0);
    }

    #[tokio::test]
    async fn system_prompt_counts_toward_size_limit() {
        let (_, listener) = silent_ollama().await;
        let base = OllamaBaseUrl::parse(&format!("http://{}", listener.local_addr().unwrap()));
        let ollama = Arc::new(Ollama::with_limits(
            base,
            Arc::new(Audit::new()),
            Limits {
                max_prompt_bytes: 100,
                ..Limits::default()
            },
        ));
        let (storage, _dir) = temp_storage();
        let conversation = storage.create_conversation(None, "m").unwrap().id;
        let prompt = || TurnInput::New(Redacted::new("x".repeat(40)));
        storage
            .update_settings(
                conversation,
                &ConversationSettings {
                    system_prompt: Redacted::new("s".repeat(80)),
                    ..ConversationSettings::default()
                },
            )
            .unwrap();

        let err = send_input(&ollama, &storage, conversation, prompt()).unwrap_err();
        assert!(matches!(err, AppError::TooLarge(_)));
        assert_eq!(count_rows(&storage, "messages"), 0);
        assert_eq!(ollama.inflight_len(), 0);

        // The same prompt fits without the system prompt.
        storage
            .update_settings(conversation, &ConversationSettings::default())
            .unwrap();
        send_input(&ollama, &storage, conversation, prompt()).unwrap();
        ollama.cancel_conversation(conversation).await;
    }

    #[tokio::test]
    async fn settings_changed_while_sending_writes_nothing() {
        let (ollama, _listener) = silent_ollama().await;
        let (storage, _dir) = temp_storage();
        let conversation = storage.create_conversation(None, "m").unwrap().id;
        // `think` was resolved for Off; the user turned Think on meanwhile.
        storage
            .update_settings(
                conversation,
                &ConversationSettings {
                    think: ThinkChoice::On,
                    ..ConversationSettings::default()
                },
            )
            .unwrap();

        let err = send(&ollama, &storage, conversation).unwrap_err();

        assert!(matches!(err, AppError::InvalidInput(_)));
        assert_eq!(count_rows(&storage, "messages"), 0);
        assert_eq!(ollama.inflight_len(), 0);
    }

    const ANSWER: &[&str] = &[
        r#"{"message":{"role":"assistant","content":"Plover."},"done":false}"#,
        r#"{"message":{"role":"assistant","content":""},"done":true,"eval_count":1}"#,
    ];

    /// A conversation in a project whose one file says the word is plover.
    fn project_chat(storage: &Storage, settings: &ConversationSettings) -> (Uuid, Uuid) {
        let project = storage.create_project("p").unwrap().id;
        storage
            .add_file(
                project,
                &Redacted::new("a.txt"),
                &Redacted::new("the word is plover"),
            )
            .unwrap();
        let conversation = storage
            .create_conversation_with(None, "m", settings, Some(project))
            .unwrap()
            .id;
        (project, conversation)
    }

    /// Sends "word?" to `conversation` as a model that lists `tools`, and
    /// waits for the reply.
    async fn send_to_tool_model(ollama: &Arc<Ollama>, storage: &Arc<Storage>, conversation: Uuid) {
        send_turn(
            ollama,
            Arc::clone(storage),
            SendRequest {
                think: ChatThink {
                    tools: true,
                    ..no_think(ollama)
                },
                ..request(ollama, conversation, TurnInput::New(Redacted::new("word?")))
            },
            FlushPolicy::default(),
            |_| true,
        )
        .unwrap();
        ollama.wait_idle().await;
    }

    #[tokio::test]
    async fn automatic_section_sent_to_tool_model() {
        let fake = fake_ollama(Reply::Lines(ANSWER.to_vec(), Duration::ZERO)).await;
        let ollama = Arc::new(Ollama::new(
            OllamaBaseUrl::parse(&format!("http://{}", fake.addr)),
            Arc::new(Audit::new()),
        ));
        let (storage, _dir) = temp_storage();
        let (_, conversation) = project_chat(&storage, &ConversationSettings::default());

        send_to_tool_model(&ollama, &storage, conversation).await;

        let bodies = fake.bodies.lock().unwrap().clone();
        let [body] = bodies.as_slice() else {
            panic!("expected one request, got {bodies:?}");
        };
        assert_eq!(body["tools"], *FILE_TOOLS);
        let system = body["messages"][0]["content"].as_str().unwrap();
        assert!(system.contains(FILES_PREAMBLE));
        assert!(system.contains("the word is plover"));
        let saved = assistant(&storage, conversation);
        assert_eq!(saved.content.expose(), "Plover.");
        assert!(saved.file_lookups.is_empty());
    }

    #[tokio::test]
    async fn files_off_offers_no_file_tools() {
        let fake = fake_ollama(Reply::Lines(ANSWER.to_vec(), Duration::ZERO)).await;
        let ollama = Arc::new(Ollama::new(
            OllamaBaseUrl::parse(&format!("http://{}", fake.addr)),
            Arc::new(Audit::new()),
        ));
        let (storage, _dir) = temp_storage();
        let off = ConversationSettings {
            files: false,
            ..ConversationSettings::default()
        };
        let (_, conversation) = project_chat(&storage, &off);

        send_to_tool_model(&ollama, &storage, conversation).await;

        let bodies = fake.bodies.lock().unwrap().clone();
        assert!(bodies[0].get("tools").is_none());
    }

    #[test]
    fn file_lookups_are_saved() {
        let (storage, _dir) = temp_storage();
        let (conversation, recorder) = started_turn(&storage);
        let sink = recording_sink(recorder, |_| true);
        let lookup = FileLookup {
            tool: LookupKind::Search,
            target: Redacted::new("tax"),
            hits: 2,
            error: None,
        };

        assert!(sink(ChatEvent::FileLookup {
            lookup: lookup.clone()
        }));
        assert!(sink(done()));

        assert_eq!(assistant(&storage, conversation).file_lookups, [lookup]);
    }

    #[test]
    fn file_runner_formats_and_reports_missing_file() {
        let (storage, _dir) = temp_storage();
        let (project, _) = project_chat(&storage, &ConversationSettings::default());
        let run = file_runner(Arc::clone(&storage), project);

        let listed = run(&FileCall::List { start: 1 }).unwrap();
        assert_eq!(listed.hits, 1);
        assert!(listed.text.contains("<<<files 1-1 of 1>>>\na.txt\n"));
        let found = run(&FileCall::Search {
            query: "plover".into(),
        })
        .unwrap();
        assert_eq!(found.hits, 1);
        assert!(found.text.contains("\"a.txt\" line 1:"));
        let read = run(&FileCall::Read {
            name: "a.txt".into(),
            start_line: 1,
            line_count: 5,
        })
        .unwrap();
        assert!(read.text.contains("1\tthe word is plover"));
        assert!(matches!(
            run(&FileCall::Read {
                name: "b.txt".into(),
                start_line: 1,
                line_count: 5,
            }),
            Err(AppError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn empty_prompt_is_rejected() {
        let (ollama, _listener) = silent_ollama().await;
        let (storage, _dir) = temp_storage();
        let conversation = storage.create_conversation(None, "m").unwrap().id;

        for prompt in [
            TurnInput::New(Redacted::new("  \n")),
            TurnInput::Edit {
                message_id: Uuid::new_v4(),
                prompt: Redacted::new(""),
            },
        ] {
            let err = send_input(&ollama, &storage, conversation, prompt).unwrap_err();
            assert!(matches!(err, AppError::InvalidInput(_)));
        }
        assert_eq!(ollama.inflight_len(), 0);
    }
}

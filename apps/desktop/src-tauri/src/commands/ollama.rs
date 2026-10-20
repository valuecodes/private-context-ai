//! Ollama status, model list and streaming chat.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tauri::State;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::AppError;
use crate::ollama::Ollama;
use crate::ollama::stream::ChatEnd;
use crate::ollama::types::{ChatEvent, ChatOptions, ModelInfo, OllamaStatus, validate_model};
use crate::redacted::Redacted;
use crate::storage::messages::{FlushPolicy, MessageStatus, TurnRecorder};
use crate::storage::{Storage, StorageState};

#[tauri::command]
pub async fn ollama_status(ollama: State<'_, Arc<Ollama>>) -> Result<OllamaStatus, AppError> {
    ollama.status().await
}

#[tauri::command]
pub async fn ollama_list_models(
    ollama: State<'_, Arc<Ollama>>,
) -> Result<Vec<ModelInfo>, AppError> {
    ollama.list_models().await
}

/// Saves `prompt` to the conversation, starts streaming the reply into
/// `on_event` and returns the request id at once. Rust builds the history
/// from the database.
#[tauri::command]
pub async fn chat_send(
    ollama: State<'_, Arc<Ollama>>,
    storage: State<'_, StorageState>,
    conversation_id: Uuid,
    model: String,
    prompt: Redacted,
    options: Option<ChatOptions>,
    on_event: Channel<ChatEvent>,
) -> Result<Uuid, AppError> {
    let storage = Arc::clone(storage.get()?);
    // A failed send means the webview is gone; the worker then cancels.
    send_turn(
        &ollama,
        storage,
        conversation_id,
        &model,
        &prompt,
        options,
        FlushPolicy::default(),
        move |event| on_event.send(event).is_ok(),
    )
}

#[tauri::command]
pub async fn chat_cancel(ollama: State<'_, Arc<Ollama>>, request_id: Uuid) -> Result<(), AppError> {
    ollama.chat_cancel(request_id).await
}

type SharedRecorder = Arc<Mutex<TurnRecorder>>;

fn lock(recorder: &SharedRecorder) -> MutexGuard<'_, TurnRecorder> {
    recorder.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The body of `chat_send`, without Tauri. Order matters: the slot is taken
/// before anything is written, and nothing is written when the history is
/// refused, so a refused send leaves no rows behind.
#[allow(clippy::too_many_arguments)]
pub fn send_turn<S>(
    ollama: &Arc<Ollama>,
    storage: Arc<Storage>,
    conversation_id: Uuid,
    model: &str,
    prompt: &Redacted,
    options: Option<ChatOptions>,
    policy: FlushPolicy,
    send: S,
) -> Result<Uuid, AppError>
where
    S: Fn(ChatEvent) -> bool + Send + Sync + 'static,
{
    if prompt.expose().trim().is_empty() {
        return Err(AppError::InvalidInput("the message is empty".into()));
    }
    validate_model(model)?;
    if let Some(options) = &options {
        options.validate()?;
    }
    let admission = ollama.admit(conversation_id)?;
    let turn = storage.begin_turn(conversation_id, model, prompt, |history| {
        ollama.validate(model, history, options.as_ref())
    })?;

    let recorder: SharedRecorder = Arc::new(Mutex::new(TurnRecorder::new(
        storage,
        turn.assistant_id,
        policy,
    )));
    let stop_ticker = CancellationToken::new();
    spawn_ticker(Arc::clone(&recorder), policy.every, stop_ticker.clone());
    let sink = recording_sink(Arc::clone(&recorder), send);
    ollama.start(
        admission,
        model,
        &turn.history,
        options.as_ref(),
        sink,
        move |end| {
            stop_ticker.cancel();
            finish_turn(&recorder, end);
        },
    )
}

/// Saves each event before the frontend sees it. A failed save becomes a
/// `database` error for the frontend and stops the stream; on `Done` that
/// error replaces the `Done`, so the UI never shows an unsaved reply as
/// complete.
fn recording_sink<S>(
    recorder: SharedRecorder,
    send: S,
) -> impl Fn(ChatEvent) -> bool + Send + Sync + 'static
where
    S: Fn(ChatEvent) -> bool + Send + Sync + 'static,
{
    move |event| {
        let saved = match &event {
            ChatEvent::Token { content } => lock(&recorder).push(content),
            ChatEvent::Done {
                prompt_tokens,
                completion_tokens,
                duration_ms,
                ..
            } => lock(&recorder).complete(*prompt_tokens, *completion_tokens, *duration_ms),
            // `on_end` saves the error status.
            ChatEvent::Error { .. } => Ok(()),
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
fn finish_turn(recorder: &SharedRecorder, end: &ChatEnd) {
    let mut recorder = lock(recorder);
    let result = match end {
        ChatEnd::Done {
            prompt_tokens,
            completion_tokens,
            duration_ms,
        } => recorder.complete(*prompt_tokens, *completion_tokens, *duration_ms),
        ChatEnd::Cancelled => recorder.finish(MessageStatus::Cancelled),
        ChatEnd::Failed(_) => recorder.finish(MessageStatus::Error),
    };
    if let Err(error) = result {
        // The row stays `pending` and becomes `error` at the next start.
        tracing::warn!(kind = error.kind(), "could not save the end of the reply");
    }
}

/// Flushes buffered tokens every `every`, so a short reply followed by
/// silence is on disk even though no further token triggers a write. A
/// failed flush marks the recorder failed; the next event then stops the
/// stream.
fn spawn_ticker(recorder: SharedRecorder, every: Duration, stop: CancellationToken) {
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
    use tokio::net::TcpListener;

    use super::*;
    use crate::audit::MemorySink;
    use crate::ollama::client::OllamaBaseUrl;
    use crate::storage::messages::StoredMessage;
    use crate::storage::testing::{break_writes, temp_storage};

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
            .begin_turn(conversation, "m", &Redacted::new("Hi"), |_| Ok(()))
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
        }
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

    /// Accepts connections but never answers, so a request stays in flight.
    async fn silent_ollama() -> (Arc<Ollama>, TcpListener) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = OllamaBaseUrl::parse(&format!("http://{}", listener.local_addr().unwrap()));
        let ollama = Arc::new(Ollama::new(base, Arc::new(MemorySink::new())));
        (ollama, listener)
    }

    fn send(
        ollama: &Arc<Ollama>,
        storage: &Arc<Storage>,
        conversation: Uuid,
    ) -> Result<Uuid, AppError> {
        send_turn(
            ollama,
            Arc::clone(storage),
            conversation,
            "llama3.2",
            &Redacted::new("Hi"),
            None,
            FlushPolicy::default(),
            |_| true,
        )
    }

    #[tokio::test]
    async fn refused_history_writes_nothing_and_frees_slot() {
        let (ollama, _listener) = silent_ollama().await;
        let (storage, _dir) = temp_storage();
        let conversation = storage.create_conversation(None, "m").unwrap().id;

        let err = send_turn(
            &ollama,
            Arc::clone(&storage),
            conversation,
            "llama3.2",
            &Redacted::new("x".repeat(2 * 1024 * 1024)),
            None,
            FlushPolicy::default(),
            |_| true,
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
    async fn empty_prompt_is_rejected() {
        let (ollama, _listener) = silent_ollama().await;
        let (storage, _dir) = temp_storage();
        let conversation = storage.create_conversation(None, "m").unwrap().id;

        let err = send_turn(
            &ollama,
            Arc::clone(&storage),
            conversation,
            "llama3.2",
            &Redacted::new("  \n"),
            None,
            FlushPolicy::default(),
            |_| true,
        )
        .unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
        assert_eq!(ollama.inflight_len(), 0);
    }
}

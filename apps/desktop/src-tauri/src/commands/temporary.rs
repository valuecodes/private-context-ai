//! Temporary chats: streamed like saved ones, but the history stays in
//! [`TempChats`] and nothing is written to the database. Audit rows are still
//! written; they carry no chat id.

use std::sync::{Arc, Mutex};

use tauri::State;
use tauri::ipc::Channel;
use uuid::Uuid;

use crate::commands::ollama::{Shared, finish_turn, recording_sink};
use crate::error::AppError;
use crate::ollama::types::{ChatEvent, Think, validate_model};
use crate::ollama::{ChatThink, Ollama};
use crate::redacted::Redacted;
use crate::temporary::{MemoryRecorder, TempChats};

/// Starts an empty temporary chat and returns its session id.
#[tauri::command]
pub fn temp_chat_start(chats: State<'_, Arc<TempChats>>) -> Result<Uuid, AppError> {
    chats.start()
}

/// Like `chat_send`, but Rust builds the history from the session in memory.
#[tauri::command]
pub async fn temp_chat_send(
    ollama: State<'_, Arc<Ollama>>,
    chats: State<'_, Arc<TempChats>>,
    session_id: Uuid,
    model: String,
    prompt: Redacted,
    think: Option<Think>,
    on_event: Channel<ChatEvent>,
) -> Result<Uuid, AppError> {
    let think = ollama.chat_think(&model, think).await?;
    send_temp_turn(
        &ollama,
        &chats,
        session_id,
        &model,
        &prompt,
        think,
        move |event| on_event.send(event).is_ok(),
    )
}

#[tauri::command]
pub async fn temp_chat_discard(
    ollama: State<'_, Arc<Ollama>>,
    chats: State<'_, Arc<TempChats>>,
    session_id: Uuid,
) -> Result<(), AppError> {
    discard(&ollama, &chats, session_id).await;
    Ok(())
}

/// Forgets the chat, then cancels its reply if one runs, admitted or
/// streaming. An unknown id is a no-op.
pub async fn discard(ollama: &Ollama, chats: &TempChats, session_id: Uuid) {
    chats.remove(session_id);
    ollama.cancel_conversation(session_id).await;
}

/// The body of `temp_chat_send`, without Tauri. The slot is taken before the
/// session is read, so a discard either lands first (`NotFound`) or finds the
/// slot and cancels it. Nothing changes when the history is refused, and the
/// prompt is taken back when the request does not start.
pub fn send_temp_turn<S>(
    ollama: &Arc<Ollama>,
    chats: &Arc<TempChats>,
    session_id: Uuid,
    model: &str,
    prompt: &Redacted,
    think: ChatThink,
    send: S,
) -> Result<Uuid, AppError>
where
    S: Fn(ChatEvent) -> bool + Send + Sync + 'static,
{
    if prompt.expose().trim().is_empty() {
        return Err(AppError::InvalidInput("the message is empty".into()));
    }
    validate_model(model)?;
    let admission = ollama.admit(session_id)?;
    ollama.check_connection(think.connection)?;
    let history = chats.begin(session_id, prompt, |history| {
        ollama.validate(model, history, None)
    })?;

    let recorder: Shared<MemoryRecorder> =
        Arc::new(Mutex::new(MemoryRecorder::new(chats, session_id)));
    let sink = recording_sink(Arc::clone(&recorder), send);
    ollama
        .start(admission, model, &history, None, think, sink, move |end| {
            finish_turn(&recorder, end);
        })
        .inspect_err(|_| chats.rollback(session_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::Audit;
    use crate::commands::testing::{no_think, silent_ollama, silent_ollama_with};
    use crate::storage::testing::{count_rows, temp_storage};

    fn send(
        ollama: &Arc<Ollama>,
        chats: &Arc<TempChats>,
        session: Uuid,
        prompt: &str,
    ) -> Result<Uuid, AppError> {
        send_temp_turn(
            ollama,
            chats,
            session,
            "llama3.2",
            &Redacted::new(prompt),
            no_think(ollama),
            |_| true,
        )
    }

    fn prompts(chats: &TempChats, session: Uuid) -> Vec<String> {
        chats
            .history(session)
            .unwrap()
            .iter()
            .map(|message| message.content.expose().to_owned())
            .collect()
    }

    #[tokio::test]
    async fn temp_send_writes_no_rows() {
        let (storage, _dir) = temp_storage();
        let (ollama, _listener) =
            silent_ollama_with(Arc::new(Audit::over(Arc::clone(&storage)))).await;
        let chats = Arc::new(TempChats::default());
        let session = chats.start().unwrap();

        for prompt in ["One", "Two", "Three"] {
            send(&ollama, &chats, session, prompt).unwrap();
            ollama.cancel_conversation(session).await;
        }

        assert_eq!(count_rows(&storage, "conversations"), 0);
        assert_eq!(count_rows(&storage, "messages"), 0);
        let chats_audited = storage
            .list_audit(100, None)
            .unwrap()
            .into_iter()
            .filter(|event| event.kind == "ollama.chat")
            .count();
        assert_eq!(chats_audited, 3);
        assert_eq!(prompts(&chats, session), ["One", "Two", "Three"]);
    }

    #[tokio::test]
    async fn refused_temp_send_leaves_session_unchanged() {
        let (ollama, _listener) = silent_ollama().await;
        let chats = Arc::new(TempChats::default());
        let session = chats.start().unwrap();

        let err = send(&ollama, &chats, session, &"x".repeat(2 * 1024 * 1024)).unwrap_err();

        assert!(matches!(err, AppError::TooLarge(_)));
        assert!(prompts(&chats, session).is_empty());
        assert_eq!(ollama.inflight_len(), 0);
    }

    #[tokio::test]
    async fn failed_start_rolls_back_prompt() {
        let audit = Arc::new(Audit::new());
        audit.close();
        let (ollama, _listener) = silent_ollama_with(audit).await;
        let chats = Arc::new(TempChats::default());
        let session = chats.start().unwrap();

        let err = send(&ollama, &chats, session, "Hi").unwrap_err();

        assert!(matches!(err, AppError::StorageUnavailable(_)));
        assert!(prompts(&chats, session).is_empty());
        assert_eq!(ollama.inflight_len(), 0);
    }

    #[tokio::test]
    async fn discard_during_preflight_refuses_send() {
        let (ollama, _listener) = silent_ollama().await;
        let chats = Arc::new(TempChats::default());
        let session = chats.start().unwrap();
        // `temp_chat_send` awaits the capabilities lookup before the send;
        // the discard lands there.
        discard(&ollama, &chats, session).await;

        assert!(matches!(
            send(&ollama, &chats, session, "Hi"),
            Err(AppError::NotFound(_))
        ));
        assert_eq!(ollama.inflight_len(), 0);
    }

    #[tokio::test]
    async fn discard_after_admit_cancels_stream() {
        let (ollama, _listener) = silent_ollama().await;
        let chats = Arc::new(TempChats::default());
        let session = chats.start().unwrap();
        send(&ollama, &chats, session, "Hi").unwrap();
        assert_eq!(ollama.inflight_len(), 1);

        discard(&ollama, &chats, session).await;

        assert_eq!(ollama.inflight_len(), 0);
        assert!(chats.history(session).is_none());
    }

    #[tokio::test]
    async fn busy_per_session() {
        let (ollama, _listener) = silent_ollama().await;
        let chats = Arc::new(TempChats::default());
        let session = chats.start().unwrap();
        send(&ollama, &chats, session, "One").unwrap();

        assert!(matches!(
            send(&ollama, &chats, session, "Two"),
            Err(AppError::Busy(_))
        ));
        assert_eq!(prompts(&chats, session), ["One"]);
        // Another temporary chat is not blocked by this one.
        let other = chats.start().unwrap();
        send(&ollama, &chats, other, "Three").unwrap();
        ollama.cancel_conversation(session).await;
        ollama.cancel_conversation(other).await;
    }
}

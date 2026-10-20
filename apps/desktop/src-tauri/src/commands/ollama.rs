//! Ollama status, model list and streaming chat.

use std::sync::Arc;

use tauri::State;
use tauri::ipc::Channel;
use uuid::Uuid;

use crate::error::AppError;
use crate::ollama::Ollama;
use crate::ollama::types::{ChatEvent, ChatMessage, ChatOptions, ModelInfo, OllamaStatus};

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

/// Starts streaming a reply into `on_event` and returns the request id at
/// once. The frontend sends the full message list; `conversation_id` is its
/// own UUID for the conversation.
#[tauri::command]
pub async fn chat_send(
    ollama: State<'_, Arc<Ollama>>,
    conversation_id: Uuid,
    model: String,
    messages: Vec<ChatMessage>,
    options: Option<ChatOptions>,
    on_event: Channel<ChatEvent>,
) -> Result<Uuid, AppError> {
    // A failed send means the webview is gone; the worker then cancels.
    ollama.chat_send(conversation_id, model, messages, options, move |event| {
        on_event.send(event).is_ok()
    })
}

#[tauri::command]
pub async fn chat_cancel(ollama: State<'_, Arc<Ollama>>, request_id: Uuid) -> Result<(), AppError> {
    ollama.chat_cancel(request_id).await
}

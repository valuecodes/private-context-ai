//! Storage status and conversation CRUD. Each database call is one short
//! statement or transaction, run inline in an async command (never on the
//! main thread).

use std::sync::Arc;

use tauri::State;
use uuid::Uuid;

use crate::error::AppError;
use crate::memory::MemoryScheduler;
use crate::ollama::Ollama;
use crate::redacted::Redacted;
use crate::storage::conversations::{Conversation, ConversationSettings, ConversationWithMessages};
use crate::storage::{StorageState, StorageStatus};

#[tauri::command]
pub fn storage_status(storage: State<'_, StorageState>) -> StorageStatus {
    storage.status()
}

/// A new chat's settings come with its first send, so no update races it.
/// With `project_id`, the chat is created in that project.
#[tauri::command]
pub async fn conversation_create(
    storage: State<'_, StorageState>,
    title: Option<Redacted>,
    model: String,
    settings: Option<ConversationSettings>,
    project_id: Option<Uuid>,
) -> Result<Conversation, AppError> {
    storage.get()?.create_conversation_with(
        title.as_ref().map(Redacted::expose),
        &model,
        &settings.unwrap_or_default(),
        project_id,
    )
}

/// Applies from the conversation's next send.
#[tauri::command]
pub async fn conversation_update_settings(
    storage: State<'_, StorageState>,
    id: Uuid,
    settings: ConversationSettings,
) -> Result<Conversation, AppError> {
    storage.get()?.update_settings(id, &settings)
}

#[tauri::command]
pub async fn conversation_list(
    storage: State<'_, StorageState>,
) -> Result<Vec<Conversation>, AppError> {
    storage.get()?.list_conversations()
}

#[tauri::command]
pub async fn conversation_get(
    storage: State<'_, StorageState>,
    id: Uuid,
) -> Result<ConversationWithMessages, AppError> {
    storage.get()?.get_conversation(id)
}

/// The ids of conversations whose messages contain `query`, newest first.
#[tauri::command]
pub async fn conversation_search(
    storage: State<'_, StorageState>,
    query: Redacted,
) -> Result<Vec<Uuid>, AppError> {
    storage.get()?.search_conversations(query.expose())
}

/// The conversation as Markdown, for the clipboard; no file is written.
#[tauri::command]
pub async fn conversation_markdown(
    storage: State<'_, StorageState>,
    id: Uuid,
) -> Result<Redacted, AppError> {
    storage.get()?.conversation_markdown(id)
}

#[tauri::command]
pub async fn conversation_rename(
    storage: State<'_, StorageState>,
    id: Uuid,
    title: Redacted,
) -> Result<Conversation, AppError> {
    storage.get()?.rename_conversation(id, title.expose())
}

/// Deletes the rows first, then cancels the conversation's reply. A send
/// racing this either finds the conversation gone, or was admitted before
/// and is cancelled here; its late writes then match no row.
#[tauri::command]
pub async fn conversation_delete(
    storage: State<'_, StorageState>,
    ollama: State<'_, Arc<Ollama>>,
    scheduler: State<'_, Arc<MemoryScheduler>>,
    id: Uuid,
) -> Result<(), AppError> {
    storage.get()?.delete_conversation(id)?;
    ollama.cancel_conversation(id).await;
    scheduler.forget_chat(id);
    Ok(())
}

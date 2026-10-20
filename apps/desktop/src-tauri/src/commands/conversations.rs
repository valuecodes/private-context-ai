//! Storage status and conversation CRUD. Each database call is one short
//! statement or transaction, run inline in an async command (never on the
//! main thread).

use std::sync::Arc;

use tauri::State;
use uuid::Uuid;

use crate::error::AppError;
use crate::ollama::Ollama;
use crate::redacted::Redacted;
use crate::storage::conversations::{Conversation, ConversationWithMessages};
use crate::storage::{StorageState, StorageStatus};

#[tauri::command]
pub fn storage_status(storage: State<'_, StorageState>) -> StorageStatus {
    storage.status()
}

#[tauri::command]
pub async fn conversation_create(
    storage: State<'_, StorageState>,
    title: Option<Redacted>,
    model: String,
) -> Result<Conversation, AppError> {
    storage
        .get()?
        .create_conversation(title.as_ref().map(Redacted::expose), &model)
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
    id: Uuid,
) -> Result<(), AppError> {
    storage.get()?.delete_conversation(id)?;
    ollama.cancel_conversation(id).await;
    Ok(())
}

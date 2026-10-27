//! A project's file library. The webview reads the files the user picks or
//! drops and sends their text; no command here takes a disk path.

use tauri::State;
use uuid::Uuid;

use crate::error::AppError;
use crate::redacted::Redacted;
use crate::storage::StorageState;
use crate::storage::files::{FilePage, ProjectFile};

/// Stores one file, replacing a file of the same name in the project.
#[tauri::command]
pub async fn project_file_add(
    storage: State<'_, StorageState>,
    project_id: Uuid,
    name: Redacted,
    content: Redacted,
) -> Result<ProjectFile, AppError> {
    storage.get()?.add_file(project_id, &name, &content)
}

/// One page of the project's files, by name.
#[tauri::command]
pub async fn project_file_list(
    storage: State<'_, StorageState>,
    project_id: Uuid,
    offset: u32,
    limit: u32,
) -> Result<FilePage, AppError> {
    storage.get()?.list_files(project_id, offset, limit)
}

#[tauri::command]
pub async fn project_file_delete(
    storage: State<'_, StorageState>,
    project_id: Uuid,
    id: Uuid,
) -> Result<(), AppError> {
    storage.get()?.delete_file(project_id, id)
}

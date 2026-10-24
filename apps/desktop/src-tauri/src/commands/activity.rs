//! The Activity and Models panels: audit rows, the hosts they went to and
//! per-model speed.

use tauri::State;

use crate::audit::AuditEvent;
use crate::error::AppError;
use crate::storage::StorageState;
use crate::storage::audit::ModelStats;

/// Most rows one `activity_list` call returns.
pub const MAX_PAGE: u32 = 500;

/// Newest first, at most `limit` (1 to [`MAX_PAGE`]) rows with an id below
/// `before`.
#[tauri::command]
pub async fn activity_list(
    storage: State<'_, StorageState>,
    limit: u32,
    before: Option<i64>,
) -> Result<Vec<AuditEvent>, AppError> {
    let limit = usize::try_from(limit.clamp(1, MAX_PAGE)).unwrap_or(1);
    storage.get()?.list_audit(limit, before)
}

/// Every host in the retained audit rows, whichever page is shown.
#[tauri::command]
pub async fn activity_hosts(storage: State<'_, StorageState>) -> Result<Vec<String>, AppError> {
    storage.get()?.audit_hosts()
}

/// Per-model speed from the retained chat and memory audit rows.
#[tauri::command]
pub async fn activity_model_stats(
    storage: State<'_, StorageState>,
) -> Result<Vec<ModelStats>, AppError> {
    storage.get()?.model_stats()
}

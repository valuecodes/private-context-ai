//! The Activity panel: audit rows and the hosts they went to.

use tauri::State;

use crate::audit::AuditEvent;
use crate::error::AppError;
use crate::storage::StorageState;

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

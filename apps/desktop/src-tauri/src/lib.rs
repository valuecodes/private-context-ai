//! Private Context AI: Tauri entry point and IPC commands.
//!
//! Modules are `pub` so items only tests or later PRs use do not trip
//! `dead_code` under `clippy -D warnings`.
pub mod audit;
pub mod commands;
pub mod error;
pub mod observability;
pub mod ollama;
pub mod redacted;
pub mod settings;
pub mod storage;
pub mod temporary;

use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use tauri::{AppHandle, Manager, RunEvent};
use tracing_appender::non_blocking::WorkerGuard;

use crate::audit::{Audit, AuditStatus, NewAuditEvent};
use crate::commands::settings::SettingsLock;
use crate::error::AppError;
use crate::ollama::Ollama;
use crate::ollama::client::resolve_base_url;
use crate::settings::Settings;
use crate::storage::StorageState;
use crate::storage::key::KeychainKeySource;
use crate::temporary::TempChats;

/// What the `app_info` command returns; mirrored by `AppInfo` in
/// `src/lib/types.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: &'static str,
    /// Where the log files are, shown as text (no opener plugin).
    pub log_dir: String,
}

#[tauri::command]
fn app_info(app: AppHandle) -> Result<AppInfo, AppError> {
    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|err| AppError::Io(err.to_string()))?;
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        log_dir: log_dir.display().to_string(),
    })
}

/// Keeps the non-blocking log writer alive. Taken and dropped on exit so
/// buffered lines are flushed before the process ends.
struct LogGuard(Mutex<Option<WorkerGuard>>);

/// Opens the audit log on the database, or closes it when storage did not
/// open, and returns the stored settings.
fn open_audit(audit: &Audit, storage: &StorageState) -> Option<Settings> {
    let Ok(db) = storage.get() else {
        audit.record(NewAuditEvent::lifecycle(
            "db.open",
            AuditStatus::Error,
            Some(storage.status().kind().to_owned()),
        ));
        audit.close();
        return None;
    };
    audit.record(NewAuditEvent::lifecycle("db.open", AuditStatus::Ok, None));
    if let Err(error) = audit.attach(Arc::clone(db)) {
        // Ollama requests are refused from now on; the UI shows why.
        tracing::error!(kind = error.kind(), "audit log unavailable");
    }
    match settings::load(db) {
        Ok(settings) => Some(settings),
        Err(error) => {
            tracing::warn!(kind = error.kind(), "could not read the settings");
            None
        }
    }
}

/// Builds and runs the app. Panics only if Tauri itself cannot start.
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let log_dir = app.path().app_log_dir()?;
            let (guard, log_control) = observability::init(&log_dir)?;
            app.manage(LogGuard(Mutex::new(Some(guard))));

            let audit = Arc::new(Audit::new());
            audit.record(NewAuditEvent::lifecycle("app.start", AuditStatus::Ok, None));

            // A storage problem does not stop the app: the UI explains it and
            // every storage command refuses.
            let storage = StorageState::open(&app.path().app_data_dir()?, &KeychainKeySource);
            tracing::info!(status = storage.status().kind(), "storage opened");
            let stored = open_audit(&audit, &storage);
            app.manage(storage);

            if let Some(settings) = &stored
                && let Err(error) = log_control.set_level(settings.log_level)
            {
                tracing::warn!(kind = error.kind(), "could not change the log level");
            }
            app.manage(log_control);
            app.manage(SettingsLock::default());

            // A rejected base URL does not stop the app: every Ollama command
            // reports it, and the status line shows it.
            let base_url = resolve_base_url(
                stored
                    .as_ref()
                    .map(|settings| settings.ollama_base_url.as_str()),
            );
            if let Err(err) = &base_url {
                tracing::warn!(error = %err, "Ollama base URL rejected");
            }
            app.manage(Arc::new(Ollama::new(base_url, audit)));
            app.manage(Arc::new(TempChats::default()));

            tracing::info!(version = env!("CARGO_PKG_VERSION"), "app started");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            commands::ollama::ollama_status,
            commands::ollama::ollama_list_models,
            commands::ollama::ollama_model_capabilities,
            commands::ollama::chat_send,
            commands::ollama::chat_regenerate,
            commands::ollama::chat_edit,
            commands::ollama::chat_cancel,
            commands::compare::chat_compare,
            commands::compare::chat_compare_pick,
            commands::compare::chat_compare_discard,
            commands::temporary::temp_chat_start,
            commands::temporary::temp_chat_send,
            commands::temporary::temp_chat_regenerate,
            commands::temporary::temp_chat_edit,
            commands::temporary::temp_chat_discard,
            commands::temporary::temp_chat_update_settings,
            commands::conversations::storage_status,
            commands::conversations::conversation_create,
            commands::conversations::conversation_update_settings,
            commands::conversations::conversation_list,
            commands::conversations::conversation_get,
            commands::conversations::conversation_search,
            commands::conversations::conversation_markdown,
            commands::conversations::conversation_rename,
            commands::conversations::conversation_delete,
            commands::projects::project_create,
            commands::projects::project_list,
            commands::projects::project_get,
            commands::projects::project_rename,
            commands::projects::project_update_instructions,
            commands::projects::project_delete,
            commands::activity::activity_list,
            commands::activity::activity_hosts,
            commands::settings::settings_get,
            commands::settings::settings_set,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the Tauri application");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            tracing::info!("app exiting");
            if let Some(state) = handle.try_state::<LogGuard>() {
                drop(
                    state
                        .0
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .take(),
                );
            }
        }
    });
}

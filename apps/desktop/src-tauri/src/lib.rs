//! Private Context AI: Tauri entry point and IPC commands.
//!
//! Modules are `pub` so items that later PRs wire up (audit sink methods,
//! error variants) do not trip `dead_code` under `clippy -D warnings`.

pub mod audit;
pub mod error;
pub mod observability;

use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use tauri::{Manager, RunEvent};
use tracing_appender::non_blocking::WorkerGuard;

use crate::audit::{AuditSink, MemorySink};

/// What the `app_info` command returns; mirrored by `AppInfo` in
/// `src/lib/types.ts`.
#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    pub version: &'static str,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
    }
}

/// Keeps the non-blocking log writer alive. Taken and dropped on exit so
/// buffered lines are flushed before the process ends.
struct LogGuard(Mutex<Option<WorkerGuard>>);

/// Builds and runs the app. Panics only if Tauri itself cannot start.
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let log_dir = app.path().app_log_dir()?;
            let guard = observability::init(&log_dir)?;
            app.manage(LogGuard(Mutex::new(Some(guard))));

            let sink: Arc<dyn AuditSink> = Arc::new(MemorySink::new());
            app.manage(sink);

            tracing::info!(version = env!("CARGO_PKG_VERSION"), "app started");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![app_info])
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

//! Structured logging: JSON lines to a daily rolling file in the app log dir.
//!
//! Only metadata is logged (lengths, counts, durations, status), never prompt
//! or response text.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;

use crate::error::AppError;

/// Daily files kept before the oldest is deleted.
pub const MAX_LOG_FILES: usize = 14;

/// Installs the global subscriber. `RUST_LOG` overrides the `info` default.
/// The returned guard must stay alive for buffered lines to be written.
pub fn init(log_dir: &Path) -> Result<WorkerGuard, AppError> {
    std::fs::create_dir_all(log_dir)?;

    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("app")
        .filename_suffix("log")
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)
        .map_err(|err| AppError::Internal(format!("log file appender: {err}")))?;
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_writer(writer)
        .try_init()
        .map_err(|err| AppError::Internal(format!("tracing subscriber: {err}")))?;

    Ok(guard)
}

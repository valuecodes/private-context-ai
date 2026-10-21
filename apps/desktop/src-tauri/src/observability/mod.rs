//! Structured logging: JSON lines to a daily rolling file in the app log dir.
//!
//! Only metadata is logged (lengths, counts, durations, status), never prompt
//! or response text. The level follows the `log.level` setting at runtime.

use std::path::Path;

use tracing::Subscriber;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry, fmt, reload};

use crate::error::AppError;
use crate::settings::LogLevel;

/// Daily files kept before the oldest is deleted.
pub const MAX_LOG_FILES: usize = 14;

/// Changes the log level of a running subscriber.
pub struct LogControl {
    handle: reload::Handle<EnvFilter, Registry>,
    /// `RUST_LOG` was set at startup and wins over the setting.
    env_override: bool,
}

impl LogControl {
    pub fn set_level(&self, level: LogLevel) -> Result<(), AppError> {
        if self.env_override {
            return Ok(());
        }
        self.handle
            .reload(EnvFilter::new(level.as_str()))
            .map_err(|err| AppError::Internal(format!("log level: {err}")))
    }
}

/// A JSON subscriber writing to `writer`, filtered by `env_filter` when
/// given (a `RUST_LOG` override), else at `info` until
/// [`LogControl::set_level`] changes it.
pub fn subscriber<W>(
    writer: W,
    env_filter: Option<EnvFilter>,
) -> (impl Subscriber + Send + Sync + 'static, LogControl)
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let env_override = env_filter.is_some();
    let (filter, handle) =
        reload::Layer::new(env_filter.unwrap_or_else(|| EnvFilter::new(LogLevel::Info.as_str())));
    let subscriber = tracing_subscriber::registry().with(filter).with(
        fmt::layer()
            .json()
            // A file, not a terminal: no colour escape codes in the JSON lines.
            .with_ansi(false)
            .with_writer(writer),
    );
    (
        subscriber,
        LogControl {
            handle,
            env_override,
        },
    )
}

/// Installs the global subscriber. `RUST_LOG` overrides the level setting.
/// The returned guard must stay alive for buffered lines to be written.
pub fn init(log_dir: &Path) -> Result<(WorkerGuard, LogControl), AppError> {
    std::fs::create_dir_all(log_dir)?;

    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("app")
        .filename_suffix("log")
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)
        .map_err(|err| AppError::Internal(format!("log file appender: {err}")))?;
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let (subscriber, control) = subscriber(writer, EnvFilter::try_from_default_env().ok());
    subscriber
        .try_init()
        .map_err(|err| AppError::Internal(format!("tracing subscriber: {err}")))?;

    Ok((guard, control))
}

/// The request log test lives in `tests/request_log.rs`, its own process:
/// `tracing` caches callsite interest and the max level globally, so a
/// scoped subscriber can miss events that other test threads registered.
#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Captured {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    #[test]
    fn log_control_set_level_changes_filter() {
        let captured = Captured::default();
        let writer = captured.clone();
        let (subscriber, control) = subscriber(move || writer.clone(), None);

        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!("before");
            control.set_level(LogLevel::Debug).unwrap();
            tracing::debug!("after");
            control.set_level(LogLevel::Error).unwrap();
            tracing::warn!("quiet");
        });

        let text = captured.text();
        assert!(!text.contains("before"));
        assert!(text.contains("after"));
        assert!(!text.contains("quiet"));
    }

    #[test]
    fn env_override_ignores_set_level() {
        let captured = Captured::default();
        let writer = captured.clone();
        let (subscriber, control) =
            subscriber(move || writer.clone(), Some(EnvFilter::new("warn")));

        tracing::subscriber::with_default(subscriber, || {
            control.set_level(LogLevel::Trace).unwrap();
            tracing::info!("hidden");
            tracing::warn!("shown");
        });

        let text = captured.text();
        assert!(!text.contains("hidden"));
        assert!(text.contains("shown"));
    }
}

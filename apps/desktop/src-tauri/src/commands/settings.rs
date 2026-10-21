//! Reading and changing the user settings.

use std::sync::{Arc, Mutex, PoisonError};

use tauri::State;

use crate::error::AppError;
use crate::observability::LogControl;
use crate::ollama::Ollama;
use crate::ollama::client::{OllamaBaseUrl, env_override_active};
use crate::settings::{self, LogLevel, Settings, SettingsPatch};
use crate::storage::{Storage, StorageState};

/// Serializes save-and-apply, so concurrent saves take effect in the order
/// they were stored and the live base URL always matches the stored one.
#[derive(Default)]
pub struct SettingsLock(Mutex<()>);

#[tauri::command]
pub async fn settings_get(storage: State<'_, StorageState>) -> Result<Settings, AppError> {
    settings::load(storage.get()?)
}

#[tauri::command]
pub async fn settings_set(
    storage: State<'_, StorageState>,
    ollama: State<'_, Arc<Ollama>>,
    log: State<'_, LogControl>,
    lock: State<'_, SettingsLock>,
    patch: SettingsPatch,
) -> Result<Settings, AppError> {
    apply_patch(&lock, storage.get()?, &ollama, patch, |level| {
        log.set_level(level)
    })
}

/// The body of `settings_set`, without Tauri: saves `patch`, then points
/// Ollama at the stored base URL (unless the debug env override is active)
/// and applies the log level.
pub fn apply_patch(
    lock: &SettingsLock,
    storage: &Storage,
    ollama: &Ollama,
    patch: SettingsPatch,
    set_level: impl FnOnce(LogLevel) -> Result<(), AppError>,
) -> Result<Settings, AppError> {
    let _guard = lock.0.lock().unwrap_or_else(PoisonError::into_inner);
    let settings = settings::save(storage, patch)?;
    if !env_override_active() {
        ollama.set_base_url(OllamaBaseUrl::parse(&settings.ollama_base_url));
    }
    if let Err(error) = set_level(settings.log_level) {
        tracing::warn!(kind = error.kind(), "could not change the log level");
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::Audit;
    use crate::ollama::client::DEFAULT_BASE_URL;
    use crate::storage::testing::temp_storage;

    fn patch(url: &str) -> SettingsPatch {
        SettingsPatch {
            ollama_base_url: Some(url.into()),
            ..SettingsPatch::default()
        }
    }

    #[test]
    fn apply_switches_target_and_level() {
        let (storage, _dir) = temp_storage();
        let ollama = Ollama::new(
            OllamaBaseUrl::parse(DEFAULT_BASE_URL),
            Arc::new(Audit::over(Arc::clone(&storage))),
        );
        let mut applied = None;

        let settings = apply_patch(
            &SettingsLock::default(),
            &storage,
            &ollama,
            SettingsPatch {
                log_level: Some(LogLevel::Debug),
                ..patch("http://127.0.0.1:9999")
            },
            |level| {
                applied = Some(level);
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(settings.ollama_base_url, "http://127.0.0.1:9999");
        assert_eq!(
            ollama.current_base_url().as_deref(),
            Some("http://127.0.0.1:9999")
        );
        assert_eq!(applied, Some(LogLevel::Debug));
    }

    #[test]
    fn rejected_patch_keeps_target() {
        let (storage, _dir) = temp_storage();
        let ollama = Ollama::new(
            OllamaBaseUrl::parse(DEFAULT_BASE_URL),
            Arc::new(Audit::over(Arc::clone(&storage))),
        );

        let err = apply_patch(
            &SettingsLock::default(),
            &storage,
            &ollama,
            patch("http://example.com:11434"),
            |_| Ok(()),
        )
        .unwrap_err();

        assert!(matches!(err, AppError::InvalidBaseUrl(_)));
        assert_eq!(ollama.current_base_url().as_deref(), Some(DEFAULT_BASE_URL));
    }

    #[test]
    fn concurrent_saves_apply_in_commit_order() {
        let (storage, _dir) = temp_storage();
        let ollama = Arc::new(Ollama::new(
            OllamaBaseUrl::parse(DEFAULT_BASE_URL),
            Arc::new(Audit::over(Arc::clone(&storage))),
        ));
        let lock = Arc::new(SettingsLock::default());

        for _ in 0..20 {
            let threads: Vec<_> = (1..=4)
                .map(|n| {
                    let (lock, storage, ollama) =
                        (Arc::clone(&lock), Arc::clone(&storage), Arc::clone(&ollama));
                    std::thread::spawn(move || {
                        let url = format!("http://127.0.0.{n}:11434");
                        apply_patch(&lock, &storage, &ollama, patch(&url), |_| Ok(())).unwrap();
                    })
                })
                .collect();
            for thread in threads {
                thread.join().unwrap();
            }
            let stored = settings::load(&storage).unwrap().ollama_base_url;
            assert_eq!(ollama.current_base_url(), Some(stored));
        }
    }
}

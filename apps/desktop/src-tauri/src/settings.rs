//! User settings: the Ollama base URL, the default model and the log level,
//! kept in the encrypted `settings` table.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::ollama::client::{DEFAULT_BASE_URL, OllamaBaseUrl};
use crate::ollama::types::validate_model;
use crate::storage::Storage;

const KEY_BASE_URL: &str = "ollama.base_url";
const KEY_DEFAULT_MODEL: &str = "chat.default_model";
const KEY_LOG_LEVEL: &str = "log.level";

/// Level of the log file. Anything else fails to deserialize.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "error" => Some(Self::Error),
            "warn" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }
}

/// What `settings_get` returns; mirrored by `Settings` in `src/lib/types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Normalized, e.g. `http://127.0.0.1:11434`.
    pub ollama_base_url: String,
    /// Empty when none was picked.
    pub default_model: String,
    pub log_level: LogLevel,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ollama_base_url: DEFAULT_BASE_URL.to_owned(),
            default_model: String::new(),
            log_level: LogLevel::default(),
        }
    }
}

/// What `settings_set` takes: only the fields to change.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    pub ollama_base_url: Option<String>,
    pub default_model: Option<String>,
    pub log_level: Option<LogLevel>,
}

/// A stored value that no longer parses falls back to its default, so a bad
/// row can never lock the user out of the app.
fn from_values(values: &HashMap<String, String>) -> Settings {
    let mut settings = Settings::default();
    let invalid = |key: &str| tracing::warn!(key, "ignoring an invalid stored setting");
    if let Some(value) = values.get(KEY_BASE_URL) {
        match OllamaBaseUrl::parse(value) {
            Ok(base) => settings.ollama_base_url = base.to_string(),
            Err(_) => invalid(KEY_BASE_URL),
        }
    }
    if let Some(value) = values.get(KEY_DEFAULT_MODEL) {
        if value.is_empty() || validate_model(value).is_ok() {
            settings.default_model.clone_from(value);
        } else {
            invalid(KEY_DEFAULT_MODEL);
        }
    }
    if let Some(value) = values.get(KEY_LOG_LEVEL) {
        match LogLevel::parse(value) {
            Some(level) => settings.log_level = level,
            None => invalid(KEY_LOG_LEVEL),
        }
    }
    settings
}

pub fn load(storage: &Storage) -> Result<Settings, AppError> {
    Ok(from_values(&storage.setting_values()?))
}

/// Validates the whole patch, then writes it in one transaction; a rejected
/// patch writes nothing. Returns the settings after the change.
pub fn save(storage: &Storage, patch: SettingsPatch) -> Result<Settings, AppError> {
    let mut values = Vec::new();
    if let Some(url) = patch.ollama_base_url {
        values.push((KEY_BASE_URL, OllamaBaseUrl::parse(&url)?.to_string()));
    }
    if let Some(model) = patch.default_model {
        let model = model.trim().to_owned();
        if !model.is_empty() {
            validate_model(&model)?;
        }
        values.push((KEY_DEFAULT_MODEL, model));
    }
    if let Some(level) = patch.log_level {
        values.push((KEY_LOG_LEVEL, level.as_str().to_owned()));
    }
    storage.put_settings(&values)?;
    load(storage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::testing::temp_storage;

    fn base_url(url: &str) -> SettingsPatch {
        SettingsPatch {
            ollama_base_url: Some(url.into()),
            ..SettingsPatch::default()
        }
    }

    #[test]
    fn defaults_when_empty() {
        let (storage, _dir) = temp_storage();
        assert_eq!(load(&storage).unwrap(), Settings::default());
    }

    #[test]
    fn save_persists_and_reloads() {
        let (storage, _dir) = temp_storage();
        let saved = save(
            &storage,
            SettingsPatch {
                ollama_base_url: Some("http://127.0.0.1:8080".into()),
                default_model: Some(" llama3.2 ".into()),
                log_level: Some(LogLevel::Debug),
            },
        )
        .unwrap();

        let expected = Settings {
            ollama_base_url: "http://127.0.0.1:8080".into(),
            default_model: "llama3.2".into(),
            log_level: LogLevel::Debug,
        };
        assert_eq!(saved, expected);
        assert_eq!(load(&storage).unwrap(), expected);

        // A patch changes only what it names.
        let saved = save(
            &storage,
            SettingsPatch {
                log_level: Some(LogLevel::Warn),
                ..SettingsPatch::default()
            },
        )
        .unwrap();
        assert_eq!(saved.default_model, "llama3.2");
        assert_eq!(saved.log_level, LogLevel::Warn);
    }

    #[test]
    fn loopback_base_url_accepted_and_normalized() {
        let (storage, _dir) = temp_storage();
        for (input, stored) in [
            ("http://localhost:11434/", "http://localhost:11434"),
            ("  http://[::1]:11434 ", "http://[::1]:11434"),
            ("http://127.0.0.2", "http://127.0.0.2:80"),
        ] {
            let saved = save(&storage, base_url(input)).unwrap();
            assert_eq!(saved.ollama_base_url, stored, "{input}");
        }
    }

    #[test]
    fn remote_base_url_rejected_writes_nothing() {
        let (storage, _dir) = temp_storage();
        let err = save(
            &storage,
            SettingsPatch {
                ollama_base_url: Some("http://10.0.0.1:11434".into()),
                log_level: Some(LogLevel::Trace),
                ..SettingsPatch::default()
            },
        )
        .unwrap_err();

        assert!(matches!(err, AppError::InvalidBaseUrl(_)));
        assert_eq!(load(&storage).unwrap(), Settings::default());
    }

    #[test]
    fn invalid_default_model_rejected() {
        let (storage, _dir) = temp_storage();
        let err = save(
            &storage,
            SettingsPatch {
                default_model: Some("bad\nname".into()),
                ..SettingsPatch::default()
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));

        // Empty clears the default.
        let saved = save(
            &storage,
            SettingsPatch {
                default_model: Some(String::new()),
                ..SettingsPatch::default()
            },
        )
        .unwrap();
        assert_eq!(saved.default_model, "");
    }

    #[test]
    fn log_level_accepts_only_known_values() {
        for level in ["error", "warn", "info", "debug", "trace"] {
            let parsed: LogLevel = serde_json::from_value(serde_json::json!(level)).unwrap();
            assert_eq!(parsed.as_str(), level);
        }
        assert!(serde_json::from_value::<LogLevel>(serde_json::json!("verbose")).is_err());
        assert!(serde_json::from_value::<LogLevel>(serde_json::json!("INFO")).is_err());
        assert!(
            serde_json::from_value::<SettingsPatch>(serde_json::json!({ "logLevel": "verbose" }))
                .is_err()
        );
    }

    #[test]
    fn invalid_stored_value_falls_back_to_default() {
        let (storage, _dir) = temp_storage();
        storage
            .put_settings(&[
                (KEY_BASE_URL, "http://10.0.0.1:11434".into()),
                (KEY_DEFAULT_MODEL, "bad\nname".into()),
                (KEY_LOG_LEVEL, "loud".into()),
            ])
            .unwrap();

        assert_eq!(load(&storage).unwrap(), Settings::default());
    }

    #[test]
    fn settings_serialize_camel_case() {
        assert_eq!(
            serde_json::to_value(Settings::default()).unwrap(),
            serde_json::json!({
                "ollamaBaseUrl": "http://127.0.0.1:11434",
                "defaultModel": "",
                "logLevel": "info"
            })
        );
    }
}

//! The one error type every command returns.

use serde::Serialize;

/// Serialized as an adjacently tagged enum, `{ "kind": "io", "message": "…" }`,
/// so the frontend can switch on `kind`. Messages never carry prompt or
/// response text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("internal error: {0}")]
    Internal(String),
    /// The configured Ollama URL is not plain `http` on loopback. Raised
    /// before any socket is opened.
    #[error("Ollama URL rejected: {0}")]
    InvalidBaseUrl(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// The conversation already has a request running, or too many are.
    #[error("busy: {0}")]
    Busy(String),
    #[error("too large: {0}")]
    TooLarge(String),
    #[error("timed out: {0}")]
    Timeout(String),
    #[error("Ollama unreachable: {0}")]
    Unreachable(String),
    /// Ollama answered with an error status or an error line.
    #[error("Ollama error: {0}")]
    Ollama(String),
    /// The key from the keychain does not open the database.
    #[error("wrong database key: {0}")]
    WrongKey(String),
    #[error("not found: {0}")]
    NotFound(String),
    /// Secure storage did not open at startup; `storage_status` says why.
    #[error("secure storage unavailable: {0}")]
    StorageUnavailable(String),
    #[error("database error: {0}")]
    Database(String),
}

impl AppError {
    /// The serialized `kind`, for audit outcomes and log fields.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Io(_) => "io",
            Self::Internal(_) => "internal",
            Self::InvalidBaseUrl(_) => "invalidBaseUrl",
            Self::InvalidInput(_) => "invalidInput",
            Self::Busy(_) => "busy",
            Self::TooLarge(_) => "tooLarge",
            Self::Timeout(_) => "timeout",
            Self::Unreachable(_) => "unreachable",
            Self::Ollama(_) => "ollama",
            Self::WrongKey(_) => "wrongKey",
            Self::NotFound(_) => "notFound",
            Self::StorageUnavailable(_) => "storageUnavailable",
            Self::Database(_) => "database",
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

/// SQLite's messages name tables, columns and constraints, never row values.
impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Database(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_error_serializes_as_tagged_enum() {
        let internal = serde_json::to_value(AppError::Internal("boom".into())).unwrap();
        assert_eq!(
            internal,
            serde_json::json!({ "kind": "internal", "message": "boom" })
        );

        let io = serde_json::to_value(AppError::from(std::io::Error::other("disk full"))).unwrap();
        assert_eq!(
            io,
            serde_json::json!({ "kind": "io", "message": "disk full" })
        );

        let busy = serde_json::to_value(AppError::Busy("one at a time".into())).unwrap();
        assert_eq!(
            busy,
            serde_json::json!({ "kind": "busy", "message": "one at a time" })
        );

        let base = AppError::InvalidBaseUrl("host must be loopback".into());
        assert_eq!(serde_json::to_value(&base).unwrap()["kind"], base.kind());
        assert_eq!(base.kind(), "invalidBaseUrl");
    }

    #[test]
    fn storage_errors_carry_kind_and_message() {
        for (error, kind) in [
            (AppError::WrongKey("x".into()), "wrongKey"),
            (AppError::NotFound("x".into()), "notFound"),
            (
                AppError::StorageUnavailable("x".into()),
                "storageUnavailable",
            ),
            (AppError::Database("x".into()), "database"),
        ] {
            assert_eq!(error.kind(), kind);
            assert_eq!(
                serde_json::to_value(&error).unwrap(),
                serde_json::json!({ "kind": kind, "message": "x" })
            );
        }
    }
}

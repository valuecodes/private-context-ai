//! The one error type every command returns.

use serde::Serialize;

/// Serialized as an adjacently tagged enum, `{ "kind": "io", "message": "…" }`,
/// so the frontend can switch on `kind`. Messages never carry prompt or
/// response text.
#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
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
    }
}

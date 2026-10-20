//! Prompt and response text that must never reach a log line.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Message text. `Debug` and `Display` print only its length, so a stray
/// `{:?}` or tracing field can never leak content. It serializes as the plain
/// string, because Ollama and the frontend do need the text itself.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Redacted(String);

impl Redacted {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn len_bytes(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Redacted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[redacted, {} chars]", self.0.chars().count())
    }
}

impl fmt::Display for Redacted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_display_hide_content() {
        let text = Redacted::new("sécret");
        assert_eq!(format!("{text:?}"), "[redacted, 6 chars]");
        assert_eq!(text.to_string(), "[redacted, 6 chars]");
        assert_eq!(text.len_bytes(), 7);
    }

    #[test]
    fn serializes_transparently() {
        let text = Redacted::new("hello");
        assert_eq!(
            serde_json::to_value(&text).unwrap(),
            serde_json::json!("hello")
        );

        let back: Redacted = serde_json::from_str("\"hi\"").unwrap();
        assert_eq!(back.expose(), "hi");
    }
}

//! Wire types for Ollama's HTTP API and for the chat IPC.

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::redacted::Redacted;

/// Longest model name accepted, in characters.
pub const MAX_MODEL_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

/// One turn of the conversation, as the frontend sends it and as Ollama
/// receives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: Redacted,
}

/// Sampling options. Field names are Ollama's, on both sides of the IPC.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_ctx: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_predict: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u32>,
}

impl ChatOptions {
    pub fn validate(&self) -> Result<(), AppError> {
        if let Some(temperature) = self.temperature
            && !(0.0..=2.0).contains(&temperature)
        {
            return Err(invalid("temperature must be between 0 and 2"));
        }
        if let Some(num_ctx) = self.num_ctx
            && !(256..=131_072).contains(&num_ctx)
        {
            return Err(invalid("num_ctx must be between 256 and 131072"));
        }
        if let Some(num_predict) = self.num_predict
            && !(1..=131_072).contains(&num_predict)
        {
            return Err(invalid("num_predict must be between 1 and 131072"));
        }
        Ok(())
    }
}

/// The `/api/chat` request body. Deliberately has no `tools`, `format` or
/// `think` field: the model gets no tools and no structured-output hooks.
#[derive(Debug, Serialize)]
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub messages: &'a [ChatMessage],
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<&'a ChatOptions>,
}

/// One NDJSON line of a streamed `/api/chat` response. Unknown fields, such
/// as `message.thinking` from reasoning models, are ignored.
#[derive(Debug, Default, Deserialize)]
pub struct ChatLine {
    pub message: Option<ChatLineMessage>,
    #[serde(default)]
    pub done: bool,
    pub done_reason: Option<String>,
    pub prompt_eval_count: Option<u64>,
    pub eval_count: Option<u64>,
    /// Nanoseconds.
    pub total_duration: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ChatLineMessage {
    pub content: Option<Redacted>,
}

/// Streamed to the frontend over a Tauri channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChatEvent {
    Token {
        content: Redacted,
    },
    Done {
        prompt_tokens: Option<u64>,
        completion_tokens: Option<u64>,
        duration_ms: Option<u64>,
        /// `length` means the reply hit the token limit: a hint, not an error.
        done_reason: Option<String>,
    },
    Error {
        error: AppError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaStatus {
    pub reachable: bool,
    pub version: Option<String>,
    pub base_url: String,
}

/// `/api/version` response.
#[derive(Debug, Deserialize)]
pub struct VersionResponse {
    pub version: String,
}

/// `/api/tags` response. Only the fields the model picker shows are read.
#[derive(Debug, Deserialize)]
pub struct TagsResponse {
    #[serde(default)]
    pub models: Vec<TagModel>,
}

#[derive(Debug, Deserialize)]
pub struct TagModel {
    pub name: String,
    pub size: Option<u64>,
    pub details: Option<TagDetails>,
}

#[derive(Debug, Deserialize)]
pub struct TagDetails {
    pub parameter_size: Option<String>,
}

/// What `ollama_list_models` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub name: String,
    pub size_bytes: Option<u64>,
    pub parameter_size: Option<String>,
}

impl From<TagModel> for ModelInfo {
    fn from(model: TagModel) -> Self {
        Self {
            name: model.name,
            size_bytes: model.size,
            parameter_size: model.details.and_then(|details| details.parameter_size),
        }
    }
}

/// Checks a chat request before any slot is taken or socket opened.
pub fn validate_chat_input(
    model: &str,
    messages: &[ChatMessage],
    options: Option<&ChatOptions>,
    max_messages: usize,
    max_prompt_bytes: usize,
) -> Result<(), AppError> {
    if model.trim().is_empty()
        || model.chars().count() > MAX_MODEL_CHARS
        || model.chars().any(char::is_control)
    {
        return Err(invalid("model name is invalid"));
    }
    if messages.is_empty() {
        return Err(invalid("at least one message is required"));
    }
    if messages.len() > max_messages {
        return Err(invalid(&format!("at most {max_messages} messages")));
    }
    let prompt_bytes: usize = messages.iter().map(|m| m.content.len_bytes()).sum();
    if prompt_bytes > max_prompt_bytes {
        return Err(AppError::TooLarge(format!(
            "prompt is over {max_prompt_bytes} bytes"
        )));
    }
    if let Some(options) = options {
        options.validate()?;
    }
    Ok(())
}

fn invalid(message: &str) -> AppError {
    AppError::InvalidInput(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> ChatMessage {
        ChatMessage {
            role: ChatRole::User,
            content: Redacted::new(text),
        }
    }

    #[test]
    fn chat_request_serializes_without_tools() {
        let messages = [user("hi")];
        let options = ChatOptions {
            temperature: Some(0.5),
            ..ChatOptions::default()
        };
        for options in [None, Some(&options)] {
            let body = serde_json::to_value(ChatRequest {
                model: "llama3.2",
                messages: &messages,
                stream: true,
                options,
            })
            .unwrap();
            let object = body.as_object().unwrap();
            for forbidden in ["tools", "format", "think"] {
                assert!(!object.contains_key(forbidden), "{forbidden} present");
            }
            let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
            keys.sort_unstable();
            let expected: &[&str] = if options.is_some() {
                &["messages", "model", "options", "stream"]
            } else {
                &["messages", "model", "stream"]
            };
            assert_eq!(keys, expected);
            assert_eq!(
                body["messages"],
                serde_json::json!([{ "role": "user", "content": "hi" }])
            );
        }
    }

    #[test]
    fn options_out_of_range_rejected() {
        let bad = [
            ChatOptions {
                temperature: Some(2.5),
                ..ChatOptions::default()
            },
            ChatOptions {
                temperature: Some(f32::NAN),
                ..ChatOptions::default()
            },
            ChatOptions {
                num_ctx: Some(10),
                ..ChatOptions::default()
            },
            ChatOptions {
                num_predict: Some(0),
                ..ChatOptions::default()
            },
        ];
        for options in bad {
            assert!(matches!(options.validate(), Err(AppError::InvalidInput(_))));
        }
        let good = ChatOptions {
            temperature: Some(0.0),
            num_ctx: Some(4096),
            num_predict: Some(128),
            seed: Some(7),
        };
        assert_eq!(good.validate(), Ok(()));
    }

    #[test]
    fn chat_event_serializes_with_type_tag() {
        let token = ChatEvent::Token {
            content: Redacted::new("Hel"),
        };
        assert_eq!(
            serde_json::to_value(&token).unwrap(),
            serde_json::json!({ "type": "token", "content": "Hel" })
        );

        let done = ChatEvent::Done {
            prompt_tokens: Some(3),
            completion_tokens: Some(5),
            duration_ms: Some(120),
            done_reason: Some("stop".into()),
        };
        assert_eq!(
            serde_json::to_value(&done).unwrap(),
            serde_json::json!({
                "type": "done",
                "promptTokens": 3,
                "completionTokens": 5,
                "durationMs": 120,
                "doneReason": "stop"
            })
        );

        let error = ChatEvent::Error {
            error: AppError::Busy("one at a time".into()),
        };
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({
                "type": "error",
                "error": { "kind": "busy", "message": "one at a time" }
            })
        );
    }

    #[test]
    fn validate_rejects_bad_input() {
        let ok = [user("hello")];
        assert_eq!(validate_chat_input("llama3.2", &ok, None, 10, 100), Ok(()));

        for model in ["", "  ", "bad\nname"] {
            assert!(matches!(
                validate_chat_input(model, &ok, None, 10, 100),
                Err(AppError::InvalidInput(_))
            ));
        }
        assert!(matches!(
            validate_chat_input("m", &[], None, 10, 100),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            validate_chat_input("m", &[user("x"), user("y")], None, 1, 100),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            validate_chat_input("m", &[user("123456")], None, 10, 5),
            Err(AppError::TooLarge(_))
        ));
    }

    #[test]
    fn chat_message_rejects_unknown_role_and_fields() {
        assert!(serde_json::from_str::<ChatMessage>(r#"{"role":"tool","content":"x"}"#).is_err());
        assert!(
            serde_json::from_str::<ChatMessage>(r#"{"role":"user","content":"x","tools":[]}"#)
                .is_err()
        );
    }

    #[test]
    fn tags_map_to_model_info() {
        let tags: TagsResponse = serde_json::from_str(
            r#"{"models":[{"name":"llama3.2:latest","model":"llama3.2:latest",
            "modified_at":"2026-01-01T00:00:00Z","size":2019393189,"digest":"abc",
            "details":{"format":"gguf","family":"llama","parameter_size":"3.2B",
            "quantization_level":"Q4_K_M"}},{"name":"bare"}]}"#,
        )
        .unwrap();
        let models: Vec<ModelInfo> = tags.models.into_iter().map(ModelInfo::from).collect();
        assert_eq!(
            models,
            vec![
                ModelInfo {
                    name: "llama3.2:latest".into(),
                    size_bytes: Some(2_019_393_189),
                    parameter_size: Some("3.2B".into()),
                },
                ModelInfo {
                    name: "bare".into(),
                    size_bytes: None,
                    parameter_size: None,
                },
            ]
        );
    }
}

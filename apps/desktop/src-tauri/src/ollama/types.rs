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

impl ChatRole {
    /// The serialized name, also stored in the `messages.role` column.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "system" => Some(Self::System),
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            _ => None,
        }
    }
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

/// Longest thinking level name kept from `/api/show`, in characters.
const MAX_LEVEL_CHARS: usize = 32;

/// Ollama's `think` value: on/off, or a level. Levels are model-defined
/// strings (`low`, `high`, `max`, …) and are only ever sent exactly as the
/// model advertised them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Think {
    Enabled(bool),
    Level(String),
}

/// What a conversation saves as its Think setting. Off unless the user turns
/// it on; [`resolve_think`] decides what a model is actually sent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "level", rename_all = "camelCase")]
pub enum ThinkChoice {
    #[default]
    Off,
    On,
    Level(String),
}

impl ThinkChoice {
    /// What [`resolve_think`] is asked for. Off asks for nothing, so a model
    /// that always thinks gets its default level instead of a refusal.
    pub fn requested(&self) -> Option<Think> {
        match self {
            Self::Off => None,
            Self::On => Some(Think::Enabled(true)),
            Self::Level(level) => Some(Think::Level(level.clone())),
        }
    }

    /// A level follows the rule for advertised levels.
    pub fn validate(&self) -> Result<(), AppError> {
        match self {
            Self::Level(level) if !is_level_name(level) => {
                Err(invalid("thinking level is invalid"))
            }
            _ => Ok(()),
        }
    }
}

/// Non-empty, at most [`MAX_LEVEL_CHARS`], no control characters.
fn is_level_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_LEVEL_CHARS
        && !name.chars().any(char::is_control)
}

/// The `/api/chat` request body. Deliberately has no `tools` or `format`
/// field: the model gets no tools and no structured-output hooks. `think` is
/// only ever what [`resolve_think`] allowed for the model.
#[derive(Debug, Serialize)]
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub messages: &'a [ChatMessage],
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<&'a ChatOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub think: Option<Think>,
}

/// One NDJSON line of a streamed `/api/chat` response. Unknown fields are
/// ignored.
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
    /// Nanoseconds spent generating `eval_count` tokens.
    pub eval_duration: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ChatLineMessage {
    pub content: Option<Redacted>,
    /// Reasoning from thinking models; shown, stored, never sent back.
    pub thinking: Option<Redacted>,
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
    Thinking {
        content: Redacted,
    },
    Done {
        prompt_tokens: Option<u64>,
        completion_tokens: Option<u64>,
        duration_ms: Option<u64>,
        /// `length` means the reply hit the token limit: a hint, not an error.
        done_reason: Option<String>,
        /// From the first thinking chunk to the first answer chunk.
        thinking_ms: Option<u64>,
        /// Generating the completion tokens only, for tokens/s.
        eval_ms: Option<u64>,
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

/// `/api/show` response. Only the fields capabilities come from are read.
#[derive(Debug, Default, Deserialize)]
pub struct ShowResponse {
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub details: Option<ShowDetails>,
    /// Absent when Ollama has no thinking metadata for the model.
    pub thinking: Option<ShowThinking>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ShowDetails {
    pub family: Option<String>,
}

/// The `think` values a model takes, and the one used when none is sent.
#[derive(Debug, Default, Deserialize)]
pub struct ShowThinking {
    #[serde(default)]
    pub values: Vec<ThinkValue>,
    pub default: Option<ThinkValue>,
}

/// A boolean or a level name.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum ThinkValue {
    Bool(bool),
    Name(String),
}

impl ThinkValue {
    /// The level name, if it is one this app can show and send back: empty,
    /// overlong or control-character names are ignored.
    fn level(&self) -> Option<&str> {
        match self {
            Self::Name(name) if is_level_name(name) => Some(name),
            _ => None,
        }
    }
}

/// How a thinking model is switched: on/off, or by level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ThinkStyle {
    Bool,
    Levels {
        /// In the order `/api/show` lists them.
        levels: Vec<String>,
        default: String,
        /// `false` turns thinking off; models that always think refuse it.
        can_disable: bool,
    },
}

/// What `ollama_model_capabilities` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    pub thinking: bool,
    pub vision: bool,
    pub tools: bool,
    pub embedding_only: bool,
    pub think_style: ThinkStyle,
}

/// gpt-oss levels, for an Ollama that does not advertise them.
const GPT_OSS_LEVELS: [&str; 3] = ["low", "medium", "high"];

impl ModelCapabilities {
    /// The think style comes from the values `/api/show` advertises. Only
    /// when it advertises none, gpt-oss (by family or name) takes levels.
    pub fn from_show(model: &str, show: &ShowResponse) -> Self {
        let has = |name: &str| show.capabilities.iter().any(|c| c == name);
        let (thinking, think_style) = match &show.thinking {
            Some(advertised) => {
                let mut levels: Vec<String> = Vec::new();
                for level in advertised.values.iter().filter_map(ThinkValue::level) {
                    if !levels.iter().any(|known| known == level) {
                        levels.push(level.to_owned());
                    }
                }
                let can_enable =
                    !levels.is_empty() || advertised.values.contains(&ThinkValue::Bool(true));
                let can_disable = advertised.values.contains(&ThinkValue::Bool(false));
                let style = if levels.is_empty() {
                    ThinkStyle::Bool
                } else {
                    let default = advertised
                        .default
                        .as_ref()
                        .and_then(ThinkValue::level)
                        .filter(|level| levels.iter().any(|known| known == level))
                        .map_or_else(|| levels[levels.len() / 2].clone(), str::to_owned);
                    ThinkStyle::Levels {
                        levels,
                        default,
                        can_disable,
                    }
                };
                (has("thinking") && can_enable, style)
            }
            None => {
                let gpt_oss = show
                    .details
                    .as_ref()
                    .and_then(|details| details.family.as_deref())
                    == Some("gptoss")
                    || model.to_ascii_lowercase().starts_with("gpt-oss");
                let style = if gpt_oss {
                    ThinkStyle::Levels {
                        levels: GPT_OSS_LEVELS.map(str::to_owned).to_vec(),
                        default: "medium".to_owned(),
                        can_disable: false,
                    }
                } else {
                    ThinkStyle::Bool
                };
                (has("thinking"), style)
            }
        };
        Self {
            thinking,
            vision: has("vision"),
            tools: has("tools"),
            embedding_only: has("embedding") && !has("completion"),
            think_style,
        }
    }
}

/// The `think` value actually sent. A model that cannot think gets none,
/// whatever was asked; a thinking model always gets an explicit value, so
/// Ollama's version-dependent default never applies. A level the model does
/// not advertise is never sent.
pub fn resolve_think(
    caps: &ModelCapabilities,
    requested: Option<Think>,
) -> Result<Option<Think>, AppError> {
    if !caps.thinking {
        return Ok(None);
    }
    let think = match &caps.think_style {
        ThinkStyle::Bool => match requested {
            None => Think::Enabled(false),
            Some(Think::Enabled(on)) => Think::Enabled(on),
            Some(Think::Level(_)) => Think::Enabled(true),
        },
        ThinkStyle::Levels {
            levels,
            default,
            can_disable,
        } => match requested {
            Some(Think::Level(level)) if levels.contains(&level) => Think::Level(level),
            Some(Think::Level(_) | Think::Enabled(true)) => Think::Level(default.clone()),
            None | Some(Think::Enabled(false)) if *can_disable => Think::Enabled(false),
            None => Think::Level(default.clone()),
            Some(Think::Enabled(false)) => {
                return Err(invalid("this model always thinks"));
            }
        },
    };
    Ok(Some(think))
}

pub fn validate_model(model: &str) -> Result<(), AppError> {
    if model.trim().is_empty()
        || model.chars().count() > MAX_MODEL_CHARS
        || model.chars().any(char::is_control)
    {
        return Err(invalid("model name is invalid"));
    }
    Ok(())
}

/// Checks a chat request before any socket is opened.
pub fn validate_chat_input(
    model: &str,
    messages: &[ChatMessage],
    options: Option<&ChatOptions>,
    max_messages: usize,
    max_prompt_bytes: usize,
) -> Result<(), AppError> {
    validate_model(model)?;
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
                think: None,
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

        for (think, expected) in [
            (Think::Enabled(true), serde_json::json!(true)),
            (Think::Enabled(false), serde_json::json!(false)),
            (Think::Level("high".into()), serde_json::json!("high")),
            (Think::Level("max".into()), serde_json::json!("max")),
        ] {
            let body = serde_json::to_value(ChatRequest {
                model: "qwen3",
                messages: &messages,
                stream: true,
                options: None,
                think: Some(think),
            })
            .unwrap();
            let object = body.as_object().unwrap();
            assert!(!object.contains_key("tools"));
            assert!(!object.contains_key("format"));
            assert_eq!(body["think"], expected);
        }
    }

    fn show(json: &str) -> ShowResponse {
        serde_json::from_str(json).unwrap()
    }

    fn names(levels: &[&str]) -> Vec<String> {
        levels.iter().map(|&level| level.to_owned()).collect()
    }

    #[test]
    fn show_parses_advertised_levels() {
        let caps = ModelCapabilities::from_show(
            "gpt-oss:20b",
            &show(
                r#"{"license":"x","template":"y","capabilities":["completion","tools","thinking"],
                "details":{"family":"gptoss"},
                "thinking":{"values":["low","medium","high"],"default":"medium"}}"#,
            ),
        );
        assert!(caps.thinking);
        assert!(caps.tools);
        assert_eq!(
            caps.think_style,
            ThinkStyle::Levels {
                levels: names(&GPT_OSS_LEVELS),
                default: "medium".into(),
                can_disable: false,
            }
        );
    }

    #[test]
    fn show_keeps_model_defined_levels_and_reads_off_switch() {
        let caps = ModelCapabilities::from_show(
            "custom",
            &show(
                r#"{"capabilities":["completion","thinking"],
                "thinking":{"values":[false,"low","high","xhigh","max","low"],"default":"max"}}"#,
            ),
        );
        assert_eq!(
            caps.think_style,
            ThinkStyle::Levels {
                levels: names(&["low", "high", "xhigh", "max"]),
                default: "max".into(),
                can_disable: true,
            }
        );
    }

    #[test]
    fn show_ignores_unusable_level_names_and_bad_default() {
        let long = "x".repeat(MAX_LEVEL_CHARS + 1);
        let caps = ModelCapabilities::from_show(
            "custom",
            &show(&format!(
                r#"{{"capabilities":["completion","thinking"],
                "thinking":{{"values":["","bad\nname","{long}","low","medium","high"],"default":"turbo"}}}}"#
            )),
        );
        assert_eq!(
            caps.think_style,
            ThinkStyle::Levels {
                levels: names(&GPT_OSS_LEVELS),
                default: "medium".into(),
                can_disable: false,
            }
        );
    }

    #[test]
    fn show_parses_boolean_thinking() {
        let caps = ModelCapabilities::from_show(
            "qwen3:8b",
            &show(
                r#"{"capabilities":["completion","thinking"],"details":{"family":"qwen3"},
                "thinking":{"values":[true,false],"default":true}}"#,
            ),
        );
        assert!(caps.thinking);
        assert_eq!(caps.think_style, ThinkStyle::Bool);
    }

    #[test]
    fn show_falls_back_to_family_heuristic() {
        for (model, family) in [("gpt-oss:20b", "other"), ("renamed", "gptoss")] {
            let caps = ModelCapabilities::from_show(
                model,
                &show(&format!(
                    r#"{{"capabilities":["completion","thinking"],"details":{{"family":"{family}"}}}}"#
                )),
            );
            assert!(
                matches!(caps.think_style, ThinkStyle::Levels { .. }),
                "{model}"
            );
        }
        let caps = ModelCapabilities::from_show(
            "qwen3:8b",
            &show(r#"{"capabilities":["completion","thinking"],"details":{"family":"qwen3"}}"#),
        );
        assert!(caps.thinking);
        assert_eq!(caps.think_style, ThinkStyle::Bool);
    }

    #[test]
    fn show_values_false_means_not_thinking() {
        let caps = ModelCapabilities::from_show(
            "m",
            &show(r#"{"capabilities":["completion","thinking"],"thinking":{"values":[false]}}"#),
        );
        assert!(!caps.thinking);
        let caps = ModelCapabilities::from_show(
            "llama3.2",
            &show(r#"{"capabilities":["completion","tools"]}"#),
        );
        assert!(!caps.thinking);
    }

    #[test]
    fn show_embedding_only_and_vision() {
        let caps = ModelCapabilities::from_show(
            "nomic-embed-text",
            &show(r#"{"capabilities":["embedding"]}"#),
        );
        assert!(caps.embedding_only);
        let caps = ModelCapabilities::from_show(
            "llava",
            &show(r#"{"capabilities":["completion","vision"]}"#),
        );
        assert!(caps.vision);
        assert!(!caps.embedding_only);
    }

    fn caps(thinking: bool, think_style: ThinkStyle) -> ModelCapabilities {
        ModelCapabilities {
            thinking,
            vision: false,
            tools: false,
            embedding_only: false,
            think_style,
        }
    }

    #[test]
    fn resolve_think_table() {
        use Think::Enabled;
        let level = |name: &str| Think::Level(name.to_owned());
        let levels = |can_disable| ThinkStyle::Levels {
            levels: names(&["low", "medium"]),
            default: "medium".into(),
            can_disable,
        };
        let cases = [
            // A model that cannot think gets nothing, whatever was asked.
            (caps(false, ThinkStyle::Bool), Some(Enabled(true)), Ok(None)),
            (caps(false, levels(false)), Some(level("high")), Ok(None)),
            // Boolean models always get an explicit value.
            (caps(true, ThinkStyle::Bool), None, Ok(Some(Enabled(false)))),
            (
                caps(true, ThinkStyle::Bool),
                Some(Enabled(true)),
                Ok(Some(Enabled(true))),
            ),
            (
                caps(true, ThinkStyle::Bool),
                Some(level("low")),
                Ok(Some(Enabled(true))),
            ),
            // Levels: only advertised ones are sent.
            (
                caps(true, levels(false)),
                Some(level("low")),
                Ok(Some(level("low"))),
            ),
            (
                caps(true, levels(false)),
                Some(level("high")),
                Ok(Some(level("medium"))),
            ),
            (
                caps(true, levels(false)),
                Some(Enabled(true)),
                Ok(Some(level("medium"))),
            ),
            (caps(true, levels(false)), None, Ok(Some(level("medium")))),
            (caps(true, levels(true)), None, Ok(Some(Enabled(false)))),
            (
                caps(true, levels(true)),
                Some(Enabled(false)),
                Ok(Some(Enabled(false))),
            ),
        ];
        for (caps, requested, expected) in cases {
            let label = format!("{requested:?}");
            assert_eq!(resolve_think(&caps, requested), expected, "{label}");
        }
        assert!(matches!(
            resolve_think(&caps(true, levels(false)), Some(Enabled(false))),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn think_deserializes_from_ipc() {
        assert_eq!(
            serde_json::from_str::<Think>("false").unwrap(),
            Think::Enabled(false)
        );
        assert_eq!(
            serde_json::from_str::<Think>(r#""max""#).unwrap(),
            Think::Level("max".into())
        );
        assert!(serde_json::from_str::<Think>("3").is_err());
    }

    #[test]
    fn think_choice_ipc_and_request() {
        let cases = [
            (ThinkChoice::Off, r#"{"kind":"off"}"#, None),
            (
                ThinkChoice::On,
                r#"{"kind":"on"}"#,
                Some(Think::Enabled(true)),
            ),
            (
                ThinkChoice::Level("high".into()),
                r#"{"kind":"level","level":"high"}"#,
                Some(Think::Level("high".into())),
            ),
        ];
        for (choice, json, requested) in cases {
            assert_eq!(serde_json::to_string(&choice).unwrap(), json);
            assert_eq!(serde_json::from_str::<ThinkChoice>(json).unwrap(), choice);
            assert_eq!(choice.requested(), requested);
            assert!(choice.validate().is_ok());
        }
        // Off on a model that always thinks is its default level, not an error.
        let always = caps(
            true,
            ThinkStyle::Levels {
                levels: names(&["low", "medium"]),
                default: "medium".into(),
                can_disable: false,
            },
        );
        assert_eq!(
            resolve_think(&always, ThinkChoice::Off.requested()),
            Ok(Some(Think::Level("medium".into())))
        );
        for level in [
            String::new(),
            "x".repeat(MAX_LEVEL_CHARS + 1),
            "a\nb".into(),
        ] {
            assert!(matches!(
                ThinkChoice::Level(level).validate(),
                Err(AppError::InvalidInput(_))
            ));
        }
    }

    #[test]
    fn capabilities_serialize_camel_case() {
        let value = serde_json::to_value(caps(
            true,
            ThinkStyle::Levels {
                levels: names(&["low"]),
                default: "low".into(),
                can_disable: false,
            },
        ))
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "thinking": true,
                "vision": false,
                "tools": false,
                "embeddingOnly": false,
                "thinkStyle": {
                    "kind": "levels",
                    "levels": ["low"],
                    "default": "low",
                    "canDisable": false
                }
            })
        );
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

        let thinking = ChatEvent::Thinking {
            content: Redacted::new("hmm"),
        };
        assert_eq!(
            serde_json::to_value(&thinking).unwrap(),
            serde_json::json!({ "type": "thinking", "content": "hmm" })
        );

        let done = ChatEvent::Done {
            prompt_tokens: Some(3),
            completion_tokens: Some(5),
            duration_ms: Some(120),
            done_reason: Some("stop".into()),
            thinking_ms: Some(40),
            eval_ms: Some(100),
        };
        assert_eq!(
            serde_json::to_value(&done).unwrap(),
            serde_json::json!({
                "type": "done",
                "promptTokens": 3,
                "completionTokens": 5,
                "durationMs": 120,
                "doneReason": "stop",
                "thinkingMs": 40,
                "evalMs": 100
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

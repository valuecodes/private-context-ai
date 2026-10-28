//! The display-only tool a model can be offered: `render_chart`. The
//! read-only file tools are in [`super::file_tools`].
//!
//! Its definition is fixed here, never built from webview input. A call's
//! arguments are validated into a [`ChartSpec`] that the webview draws as
//! SVG; the model only ever gets a fixed result line back.

use std::borrow::Cow;
use std::fmt;
use std::sync::LazyLock;

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use super::types::{ChatMessage, ChatRole};
use crate::redacted::Redacted;

pub const TOOL_NAME: &str = "render_chart";
/// Calls accepted per reply; later ones are ignored.
pub const MAX_CHARTS: usize = 3;
const MAX_POINTS: usize = 100;
const MAX_PIE_SLICES: usize = 12;
/// Longest title or label, in characters.
const MAX_TEXT_CHARS: usize = 80;
/// Largest magnitude accepted, so no range can overflow.
const MAX_ABS_VALUE: f64 = 1e15;

/// The `tools` array of a request that offers charts.
pub static CHART_TOOLS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::json!([{
        "type": "function",
        "function": {
            "name": TOOL_NAME,
            "description": "Display an interactive chart in the chat",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "type": { "type": "string", "enum": ["bar", "line", "pie"] },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "values": { "type": "array", "items": { "type": "number" } }
                },
                "required": ["title", "type", "labels", "values"]
            }
        }
    }])
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartKind {
    Bar,
    Line,
    Pie,
}

/// A validated chart. Its text is the model's, so `Debug` prints only the
/// kind and the number of points.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartSpec {
    pub title: String,
    #[serde(rename = "type")]
    pub kind: ChartKind,
    pub labels: Vec<String>,
    pub values: Vec<f64>,
}

impl fmt::Debug for ChartSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChartSpec")
            .field("kind", &self.kind)
            .field("points", &self.values.len())
            .finish_non_exhaustive()
    }
}

/// One `render_chart` call of a reply, as saved with it and shown by the
/// webview: drawn when `spec` is set, otherwise a note saying why not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartEntry {
    /// Where the chart goes in the reply text, in UTF-16 code units: the
    /// length of the answer when the request that called it ended.
    pub offset: u64,
    pub spec: Option<ChartSpec>,
    /// Why no chart was shown; fixed text written here, never the model's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ChartEntry {
    /// Checks a call to `name` with `arguments`.
    pub fn checked(offset: u64, name: &str, arguments: &Value) -> Self {
        let result = if name == TOOL_NAME {
            validate(arguments)
        } else {
            Err("only render_chart can be called".to_owned())
        };
        match result {
            Ok(spec) => Self {
                offset,
                spec: Some(spec),
                error: None,
            },
            Err(error) => Self {
                offset,
                spec: None,
                error: Some(error),
            },
        }
    }
}

/// A call as sent back to Ollama in a later request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolCall {
    pub function: ToolFunction,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolFunction {
    /// The call's position, as Ollama's own responses number them.
    pub index: usize,
    pub name: Cow<'static, str>,
    pub arguments: ToolArgs,
}

/// A call's arguments as sent back: a chart's, or a file tool's validated
/// ones (`{}` for a refused call).
#[derive(Debug, Clone, PartialEq)]
pub enum ToolArgs {
    Chart(ChartArgs),
    File(Value),
}

impl Serialize for ToolArgs {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Chart(args) => args.serialize(serializer),
            Self::File(value) => value.serialize(serializer),
        }
    }
}

/// The calls of one assistant message: `charts`, then `files`, numbered in
/// that order.
pub fn tool_calls<'a>(
    charts: &[ChartEntry],
    files: impl IntoIterator<Item = (Cow<'static, str>, &'a Value)>,
) -> Vec<ToolCall> {
    charts
        .iter()
        .map(|entry| {
            (
                Cow::Borrowed(TOOL_NAME),
                ToolArgs::Chart(ChartArgs(entry.spec.clone())),
            )
        })
        .chain(
            files
                .into_iter()
                .map(|(name, arguments)| (name, ToolArgs::File(arguments.clone()))),
        )
        .enumerate()
        .map(|(index, (name, arguments))| ToolCall {
            function: ToolFunction {
                index,
                name,
                arguments,
            },
        })
        .collect()
}

/// The validated arguments; a rejected call is sent back as `{}`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartArgs(pub Option<ChartSpec>);

impl Serialize for ChartArgs {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            Some(spec) => spec.serialize(serializer),
            None => serializer.serialize_map(Some(0))?.end(),
        }
    }
}

fn clean_text(text: &str, what: &str) -> Result<String, String> {
    let text = text.trim();
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(format!("{what} is over {MAX_TEXT_CHARS} characters"));
    }
    Ok(text.to_owned())
}

/// Checks a `render_chart` call's arguments. Arguments sent as a JSON string
/// are parsed first. The reasons are fixed text, safe to show and to send
/// back to the model.
pub fn validate(arguments: &Value) -> Result<ChartSpec, String> {
    let parsed;
    let arguments = match arguments {
        Value::String(text) => {
            parsed = serde_json::from_str::<Value>(text)
                .map_err(|_| "the arguments are not JSON".to_owned())?;
            &parsed
        }
        other => other,
    };
    let Value::Object(fields) = arguments else {
        return Err("the arguments are not an object".into());
    };
    let title = fields
        .get("title")
        .and_then(Value::as_str)
        .ok_or("title must be a string")?;
    let title = clean_text(title, "the title")?;
    let kind = match fields.get("type").and_then(Value::as_str) {
        Some("bar") => ChartKind::Bar,
        Some("line") => ChartKind::Line,
        Some("pie") => ChartKind::Pie,
        _ => return Err("type must be bar, line or pie".into()),
    };
    let labels = fields
        .get("labels")
        .and_then(Value::as_array)
        .ok_or("labels must be an array")?
        .iter()
        .map(|label| {
            label
                .as_str()
                .ok_or_else(|| "every label must be a string".to_owned())
                .and_then(|label| clean_text(label, "a label"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let values = fields
        .get("values")
        .and_then(Value::as_array)
        .ok_or("values must be an array")?
        .iter()
        .map(|value| {
            value
                .as_f64()
                .filter(|value| value.is_finite() && value.abs() <= MAX_ABS_VALUE)
                .ok_or_else(|| "every value must be a number of at most 1e15".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if labels.len() != values.len() {
        return Err("labels and values differ in length".into());
    }
    if !(1..=MAX_POINTS).contains(&values.len()) {
        return Err(format!("a chart has 1 to {MAX_POINTS} values"));
    }
    if kind == ChartKind::Pie {
        if values.len() > MAX_PIE_SLICES {
            return Err(format!("a pie has at most {MAX_PIE_SLICES} slices"));
        }
        if values.iter().any(|value| *value < 0.0) {
            return Err("a pie cannot have negative values".into());
        }
        if values.iter().sum::<f64>() <= 0.0 {
            return Err("a pie needs a value above zero".into());
        }
    }
    Ok(ChartSpec {
        title,
        kind,
        labels,
        values,
    })
}

/// What the model is told about a call.
pub fn tool_result(entry: &ChartEntry) -> String {
    match (&entry.spec, &entry.error) {
        (Some(_), _) => "The chart was shown to the user.".to_owned(),
        (None, Some(error)) => format!("The chart could not be shown: {error}"),
        (None, None) => "The chart could not be shown.".to_owned(),
    }
}

/// `history`, then the reply so far with its calls, then one tool result
/// per call: what a follow-up request sends.
pub fn follow_up_history(
    mut history: Vec<ChatMessage>,
    text: &str,
    charts: &[ChartEntry],
) -> Vec<ChatMessage> {
    history.extend(super::round_messages(text, charts, &[]));
    history
}

/// A saved reply as the model saw it: the text before its charts with their
/// calls, the tool results, then the commentary.
pub fn replay(content: &str, charts: &[ChartEntry]) -> Vec<ChatMessage> {
    let Some(offset) = charts.iter().map(|entry| entry.offset).max() else {
        return vec![ChatMessage::new(
            ChatRole::Assistant,
            Redacted::new(content),
        )];
    };
    let (before, after) = content.split_at(byte_index(content, offset));
    let mut messages = follow_up_history(Vec::new(), before, charts);
    if !after.is_empty() {
        messages.push(ChatMessage::new(ChatRole::Assistant, Redacted::new(after)));
    }
    messages
}

/// `history` for a model that is not offered tools: each replayed chart turn
/// becomes one plain assistant message again (its text before the calls
/// plus the commentary), with no `tool_calls` and no `tool` messages, which
/// a tool-less template may reject.
pub fn without_tools(history: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut plain: Vec<ChatMessage> = Vec::with_capacity(history.len());
    // The text of a reply whose calls were just dropped, waiting for its
    // commentary.
    let mut open: Option<String> = None;
    for message in history {
        match message.role {
            ChatRole::Tool => {}
            ChatRole::Assistant if message.tool_calls.is_some() => {
                if let Some(text) = open.take().filter(|text| !text.is_empty()) {
                    plain.push(ChatMessage::new(ChatRole::Assistant, Redacted::new(text)));
                }
                open = Some(message.content.expose().to_owned());
            }
            ChatRole::Assistant if open.is_some() => {
                let mut text = open.take().unwrap_or_default();
                text.push_str(message.content.expose());
                plain.push(ChatMessage::new(ChatRole::Assistant, Redacted::new(text)));
            }
            _ => {
                if let Some(text) = open.take().filter(|text| !text.is_empty()) {
                    plain.push(ChatMessage::new(ChatRole::Assistant, Redacted::new(text)));
                }
                plain.push(message.clone());
            }
        }
    }
    if let Some(text) = open.filter(|text| !text.is_empty()) {
        plain.push(ChatMessage::new(ChatRole::Assistant, Redacted::new(text)));
    }
    plain
}

/// `text`'s length in UTF-16 code units, the unit offsets are in.
pub fn utf16_len(text: &str) -> u64 {
    text.chars().map(|c| c.len_utf16() as u64).sum()
}

/// The byte index of UTF-16 offset `offset`, moved forward to a character
/// boundary and clamped to the text.
pub fn byte_index(text: &str, offset: u64) -> usize {
    let mut units = 0u64;
    for (index, c) in text.char_indices() {
        if units >= offset {
            return index;
        }
        units += c.len_utf16() as u64;
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn valid_bar_line_and_pie() {
        for (kind, expected) in [
            ("bar", ChartKind::Bar),
            ("line", ChartKind::Line),
            ("pie", ChartKind::Pie),
        ] {
            let spec = validate(&args(&format!(
                r#"{{"title":"  Population ","type":"{kind}","labels":["1950"," 1960 "],"values":[4.0,4.4],"extra":1}}"#
            )))
            .unwrap();
            assert_eq!(spec.kind, expected);
            assert_eq!(spec.title, "Population");
            assert_eq!(spec.labels, ["1950", "1960"]);
            assert_eq!(spec.values, [4.0, 4.4]);
        }
        // Negative values are fine outside a pie.
        assert!(
            validate(&args(
                r#"{"title":"","type":"bar","labels":["a"],"values":[-3]}"#
            ))
            .is_ok()
        );
    }

    #[test]
    fn arguments_as_json_string_are_parsed() {
        let text = r#"{"title":"t","type":"line","labels":["a"],"values":[1]}"#;
        assert!(validate(&Value::String(text.into())).is_ok());
        assert!(validate(&Value::String("not json".into())).is_err());
    }

    #[test]
    fn invalid_arguments_rejected() {
        let long = "x".repeat(MAX_TEXT_CHARS + 1);
        let many_labels = serde_json::to_string(&vec!["a"; 101]).unwrap();
        let many_values = serde_json::to_string(&vec![1; 101]).unwrap();
        let slices = serde_json::to_string(&vec!["a"; 13]).unwrap();
        let slice_values = serde_json::to_string(&vec![1; 13]).unwrap();
        let cases = [
            "[]".to_owned(),
            r#"{"type":"bar","labels":["a"],"values":[1]}"#.to_owned(),
            r#"{"title":"t","type":"scatter","labels":["a"],"values":[1]}"#.to_owned(),
            r#"{"title":"t","type":"bar","labels":["a","b"],"values":[1]}"#.to_owned(),
            r#"{"title":"t","type":"bar","labels":[],"values":[]}"#.to_owned(),
            format!(
                r#"{{"title":"t","type":"bar","labels":{many_labels},"values":{many_values}}}"#
            ),
            r#"{"title":"t","type":"bar","labels":["a"],"values":["1"]}"#.to_owned(),
            r#"{"title":"t","type":"bar","labels":[1],"values":[1]}"#.to_owned(),
            r#"{"title":"t","type":"bar","labels":["a"],"values":[1e16]}"#.to_owned(),
            r#"{"title":"t","type":"bar","labels":["a"],"values":[1e999]}"#.to_owned(),
            format!(r#"{{"title":"{long}","type":"bar","labels":["a"],"values":[1]}}"#),
            format!(r#"{{"title":"t","type":"bar","labels":["{long}"],"values":[1]}}"#),
            format!(r#"{{"title":"t","type":"pie","labels":{slices},"values":{slice_values}}}"#),
            r#"{"title":"t","type":"pie","labels":["a","b"],"values":[3,-1]}"#.to_owned(),
            r#"{"title":"t","type":"pie","labels":["a","b"],"values":[0,0]}"#.to_owned(),
        ];
        for case in &cases {
            // `1e999` is not valid JSON for serde_json; treat a parse failure
            // as the string form, which must be refused too.
            let value = serde_json::from_str(case).unwrap_or_else(|_| Value::String(case.clone()));
            assert!(validate(&value).is_err(), "{case}");
        }
    }

    #[test]
    fn unknown_tool_name_is_an_error_entry() {
        let entry = ChartEntry::checked(
            5,
            "run_shell",
            &args(r#"{"title":"t","type":"bar","labels":["a"],"values":[1]}"#),
        );
        assert_eq!(entry.offset, 5);
        assert!(entry.spec.is_none());
        assert_eq!(
            tool_result(&entry),
            "The chart could not be shown: only render_chart can be called"
        );
    }

    fn bar() -> ChartSpec {
        ChartSpec {
            title: "t".into(),
            kind: ChartKind::Bar,
            labels: vec!["a".into()],
            values: vec![1.0],
        }
    }

    #[test]
    fn follow_up_history_shape() {
        let charts = [
            ChartEntry {
                offset: 4,
                spec: Some(bar()),
                error: None,
            },
            ChartEntry {
                offset: 4,
                spec: None,
                error: Some("labels and values differ in length".into()),
            },
        ];
        let history = follow_up_history(
            vec![ChatMessage::new(ChatRole::User, Redacted::new("Chart it"))],
            "Here",
            &charts,
        );
        assert_eq!(
            serde_json::to_value(&history).unwrap(),
            serde_json::json!([
                { "role": "user", "content": "Chart it" },
                {
                    "role": "assistant",
                    "content": "Here",
                    "tool_calls": [
                        { "function": { "index": 0, "name": "render_chart", "arguments":
                            { "title": "t", "type": "bar", "labels": ["a"], "values": [1.0] } } },
                        { "function": { "index": 1, "name": "render_chart", "arguments": {} } }
                    ]
                },
                { "role": "tool", "content": "The chart was shown to the user.",
                  "tool_name": "render_chart" },
                { "role": "tool",
                  "content": "The chart could not be shown: labels and values differ in length",
                  "tool_name": "render_chart" }
            ])
        );
    }

    #[test]
    fn replay_splits_at_offset() {
        let content = "Äiti 😀 says.\nCommentary";
        // "Äiti 😀 says." is 13 UTF-16 units: the emoji is two.
        let offset = utf16_len("Äiti 😀 says.");
        assert_eq!(offset, 13);
        let charts = [ChartEntry {
            offset,
            spec: Some(bar()),
            error: None,
        }];
        let messages = replay(content, &charts);
        let roles: Vec<(ChatRole, &str)> = messages
            .iter()
            .map(|message| (message.role, message.content.expose()))
            .collect();
        assert_eq!(
            roles,
            [
                (ChatRole::Assistant, "Äiti 😀 says."),
                (ChatRole::Tool, "The chart was shown to the user."),
                (ChatRole::Assistant, "\nCommentary"),
            ]
        );
        assert!(messages[0].tool_calls.is_some());

        // No commentary: no trailing assistant message.
        assert_eq!(
            replay(
                "Only",
                &[ChartEntry {
                    offset: 4,
                    ..charts[0].clone()
                }]
            )
            .len(),
            2
        );
        let plain = replay("Plain", &[]);
        assert_eq!(
            plain,
            [ChatMessage::new(
                ChatRole::Assistant,
                Redacted::new("Plain")
            )]
        );
    }

    #[test]
    fn without_tools_restores_plain_replies() {
        let chart = ChartEntry {
            offset: 5,
            spec: Some(bar()),
            error: None,
        };
        let user = |text: &str| ChatMessage::new(ChatRole::User, Redacted::new(text));
        let mut history = vec![user("Chart it")];
        history.extend(replay("Here: Grew.", std::slice::from_ref(&chart)));
        history.push(user("And?"));
        // A stopped chart-only reply: nothing left to send.
        history.extend(replay(
            "",
            &[ChartEntry {
                offset: 0,
                ..chart.clone()
            }],
        ));
        history.push(user("Last"));
        // Text before the calls, no commentary.
        history.extend(replay("Only", &[ChartEntry { offset: 4, ..chart }]));

        let plain = without_tools(&history);
        let texts: Vec<(ChatRole, &str)> = plain
            .iter()
            .map(|message| (message.role, message.content.expose()))
            .collect();
        assert_eq!(
            texts,
            [
                (ChatRole::User, "Chart it"),
                (ChatRole::Assistant, "Here: Grew."),
                (ChatRole::User, "And?"),
                (ChatRole::User, "Last"),
                (ChatRole::Assistant, "Only"),
            ]
        );
        assert!(
            plain
                .iter()
                .all(|message| message.tool_calls.is_none() && message.tool_name.is_none())
        );
        // A history without charts is unchanged.
        let simple = vec![
            user("a"),
            ChatMessage::new(ChatRole::Assistant, Redacted::new("b")),
        ];
        assert_eq!(without_tools(&simple), simple);
    }

    #[test]
    fn byte_index_clamps_and_keeps_boundaries() {
        assert_eq!(byte_index("abc", 0), 0);
        assert_eq!(byte_index("abc", 2), 2);
        assert_eq!(byte_index("abc", 99), 3);
        // Inside the emoji's surrogate pair: moved to the next boundary.
        assert_eq!(byte_index("😀x", 1), 4);
        assert_eq!(byte_index("Äx", 1), 2);
    }

    #[test]
    fn spec_debug_hides_text() {
        let debug = format!("{:?}", bar());
        assert!(!debug.contains('"'), "{debug}");
        assert!(debug.contains("points: 1"));
    }

    #[test]
    fn tool_definition_is_fixed() {
        let tools = CHART_TOOLS.as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], TOOL_NAME);
        assert_eq!(
            tools[0]["function"]["parameters"]["required"],
            serde_json::json!(["title", "type", "labels", "values"])
        );
    }
}

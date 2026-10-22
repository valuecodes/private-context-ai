//! Streaming `/api/chat`: NDJSON decoding, timeouts, size limits and cancel.

use std::future::Future;

use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::Limits;
use super::client::{
    MAX_ERROR_BODY, MAX_ERROR_CHARS, http_error, map_reqwest, read_limited, truncate,
};
use super::types::{ChatEvent, ChatLine};
use crate::error::AppError;

/// Splits a byte stream into NDJSON lines, keeping a partial line between
/// chunks and refusing any line longer than `max_line` bytes.
#[derive(Debug)]
pub struct NdjsonDecoder {
    buf: Vec<u8>,
    max_line: usize,
}

impl NdjsonDecoder {
    pub fn new(max_line: usize) -> Self {
        Self {
            buf: Vec::new(),
            max_line,
        }
    }

    /// Feeds one chunk and returns every line it completed.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<ChatLine>, AppError> {
        let mut lines = Vec::new();
        let mut rest = chunk;
        while let Some(pos) = rest.iter().position(|&byte| byte == b'\n') {
            self.extend(&rest[..pos])?;
            let line = std::mem::take(&mut self.buf);
            if let Some(parsed) = parse_line(&line)? {
                lines.push(parsed);
            }
            rest = &rest[pos + 1..];
        }
        self.extend(rest)?;
        Ok(lines)
    }

    /// Parses a last line that arrived without a trailing newline.
    pub fn finish(&mut self) -> Result<Option<ChatLine>, AppError> {
        let line = std::mem::take(&mut self.buf);
        parse_line(&line)
    }

    fn extend(&mut self, bytes: &[u8]) -> Result<(), AppError> {
        if self.buf.len() + bytes.len() > self.max_line {
            return Err(AppError::TooLarge(format!(
                "a stream line is over {} bytes",
                self.max_line
            )));
        }
        self.buf.extend_from_slice(bytes);
        Ok(())
    }
}

fn parse_line(bytes: &[u8]) -> Result<Option<ChatLine>, AppError> {
    let line = bytes.trim_ascii();
    if line.is_empty() {
        return Ok(None);
    }
    serde_json::from_slice(line)
        .map(Some)
        .map_err(|err| AppError::Ollama(format!("malformed stream line: {err}")))
}

/// How a chat stream ended.
#[derive(Debug, PartialEq, Eq)]
pub enum ChatEnd {
    Done {
        prompt_tokens: Option<u64>,
        completion_tokens: Option<u64>,
        duration_ms: Option<u64>,
        thinking_ms: Option<u64>,
        eval_ms: Option<u64>,
    },
    Cancelled,
    Failed(AppError),
}

#[derive(Debug)]
pub struct ChatRun {
    /// Absent when no response headers arrived.
    pub http_status: Option<u16>,
    pub end: ChatEnd,
}

/// Sends the request and streams the reply into `emit`. Never emits after
/// `cancel` fires; a `false` from `emit` (the receiver is gone) cancels.
pub async fn run_chat(
    http: &reqwest::Client,
    url: Url,
    body: Vec<u8>,
    limits: &Limits,
    cancel: &CancellationToken,
    emit: &(dyn Fn(ChatEvent) -> bool + Send + Sync),
) -> ChatRun {
    let mut http_status = None;
    let end = match stream_reply(http, url, body, limits, cancel, emit, &mut http_status).await {
        Ok(end) => end,
        Err(_) if cancel.is_cancelled() => ChatEnd::Cancelled,
        Err(err) => ChatEnd::Failed(err),
    };
    ChatRun { http_status, end }
}

/// How long the model thought: from its first thinking chunk to its first
/// answer chunk, or to the end when no answer came.
#[derive(Debug, Default)]
struct ThinkingClock {
    started: Option<Instant>,
    took: Option<std::time::Duration>,
}

impl ThinkingClock {
    fn thought(&mut self) {
        if self.started.is_none() {
            self.started = Some(Instant::now());
        }
    }

    fn answered(&mut self) {
        if let (Some(started), None) = (self.started, self.took) {
            self.took = Some(started.elapsed());
        }
    }

    fn ms(&self) -> Option<u64> {
        self.took
            .map(|took| u64::try_from(took.as_millis()).unwrap_or(u64::MAX))
    }
}

enum Step<T> {
    Ready(T),
    Cancelled,
}

/// Awaits `fut` unless `cancel` fires first or `deadline` passes.
async fn step<F: Future>(
    cancel: &CancellationToken,
    deadline: Instant,
    fut: F,
) -> Result<Step<F::Output>, AppError> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Ok(Step::Cancelled),
        result = tokio::time::timeout_at(deadline, fut) => result
            .map(Step::Ready)
            .map_err(|_| AppError::Timeout("Ollama stopped responding".into())),
    }
}

fn emit_checked(
    cancel: &CancellationToken,
    emit: &(dyn Fn(ChatEvent) -> bool + Send + Sync),
    event: ChatEvent,
) -> bool {
    if cancel.is_cancelled() {
        return false;
    }
    if emit(event) {
        true
    } else {
        cancel.cancel();
        false
    }
}

async fn stream_reply(
    http: &reqwest::Client,
    url: Url,
    body: Vec<u8>,
    limits: &Limits,
    cancel: &CancellationToken,
    emit: &(dyn Fn(ChatEvent) -> bool + Send + Sync),
    http_status: &mut Option<u16>,
) -> Result<ChatEnd, AppError> {
    // One deadline covers the response headers and the first body chunk,
    // since a cold model load can take a while before either arrives.
    let first_deadline = Instant::now() + limits.first_chunk;
    let request = http
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send();
    let mut resp = match step(cancel, first_deadline, request).await? {
        Step::Cancelled => return Ok(ChatEnd::Cancelled),
        Step::Ready(resp) => resp.map_err(|err| map_reqwest(&err))?,
    };
    let status = resp.status();
    *http_status = Some(status.as_u16());
    if !status.is_success() {
        return match step(
            cancel,
            first_deadline,
            read_limited(&mut resp, MAX_ERROR_BODY),
        )
        .await?
        {
            Step::Cancelled => Ok(ChatEnd::Cancelled),
            Step::Ready(body) => Err(http_error(status.as_u16(), &body?)),
        };
    }

    let mut decoder = NdjsonDecoder::new(limits.max_line_bytes);
    let mut received = 0usize;
    // Answer and thinking bytes together, against `max_content_bytes`.
    let mut content = 0usize;
    let mut thinking = ThinkingClock::default();
    let mut deadline = first_deadline;
    loop {
        let chunk = match step(cancel, deadline, resp.chunk()).await? {
            Step::Cancelled => return Ok(ChatEnd::Cancelled),
            Step::Ready(chunk) => chunk.map_err(|err| map_reqwest(&err))?,
        };
        deadline = Instant::now() + limits.idle;
        let ended = chunk.is_none();
        let lines = match chunk {
            Some(bytes) => {
                received += bytes.len();
                if received > limits.max_stream_bytes {
                    return Err(AppError::TooLarge(format!(
                        "the reply stream is over {} bytes",
                        limits.max_stream_bytes
                    )));
                }
                decoder.push(&bytes)?
            }
            None => decoder.finish()?.into_iter().collect(),
        };

        for line in lines {
            if let Some(error) = line.error {
                return Err(AppError::Ollama(truncate(&error, MAX_ERROR_CHARS)));
            }
            let message = line.message.unwrap_or_default();
            // Thinking comes first: within one line it precedes the answer.
            for (text, is_thinking) in [(message.thinking, true), (message.content, false)] {
                let Some(text) = text.filter(|text| !text.is_empty()) else {
                    continue;
                };
                content += text.len_bytes();
                if content > limits.max_content_bytes {
                    return Err(AppError::TooLarge(format!(
                        "the reply is over {} bytes",
                        limits.max_content_bytes
                    )));
                }
                let event = if is_thinking {
                    thinking.thought();
                    ChatEvent::Thinking { content: text }
                } else {
                    thinking.answered();
                    ChatEvent::Token { content: text }
                };
                if !emit_checked(cancel, emit, event) {
                    return Ok(ChatEnd::Cancelled);
                }
            }
            if line.done {
                let duration_ms = line.total_duration.map(|nanos| nanos / 1_000_000);
                let eval_ms = line.eval_duration.map(|nanos| nanos / 1_000_000);
                thinking.answered();
                let thinking_ms = thinking.ms();
                let done = ChatEvent::Done {
                    prompt_tokens: line.prompt_eval_count,
                    completion_tokens: line.eval_count,
                    duration_ms,
                    done_reason: line.done_reason,
                    thinking_ms,
                    eval_ms,
                };
                // The reply is complete either way; a closed receiver only
                // means nobody sees the summary.
                emit_checked(cancel, emit, done);
                return Ok(ChatEnd::Done {
                    prompt_tokens: line.prompt_eval_count,
                    completion_tokens: line.eval_count,
                    duration_ms,
                    thinking_ms,
                    eval_ms,
                });
            }
        }
        if ended {
            return Err(AppError::Ollama(
                "the stream ended before the reply was complete".into(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_joins_partial_chunks() {
        let mut decoder = NdjsonDecoder::new(1024);
        assert!(decoder.push(br#"{"message":{"con"#).unwrap().is_empty());
        let lines = decoder
            .push(b"tent\":\"Hel\"},\"done\":false}\n{\"message\":{\"content\":\"lo\"}}\n\n{\"mess")
            .unwrap();
        let texts: Vec<String> = lines
            .into_iter()
            .map(|line| line.message.unwrap().content.unwrap().expose().to_owned())
            .collect();
        assert_eq!(texts, ["Hel", "lo"]);
        let tail = decoder.push(b"age\":{\"content\":\"!\"}}\n").unwrap();
        assert_eq!(tail.len(), 1);
        assert!(decoder.finish().unwrap().is_none());
    }

    #[test]
    fn decoder_parses_done_line() {
        let mut decoder = NdjsonDecoder::new(1024);
        let lines = decoder
            .push(
                br#"{"model":"m","message":{"role":"assistant","content":""},"done":true,"done_reason":"length","total_duration":2500000000,"prompt_eval_count":12,"eval_count":34,"eval_duration":2000000000}
"#,
            )
            .unwrap();
        let [line] = lines.as_slice() else {
            panic!("expected one line");
        };
        assert!(line.done);
        assert_eq!(line.done_reason.as_deref(), Some("length"));
        assert_eq!(line.prompt_eval_count, Some(12));
        assert_eq!(line.eval_count, Some(34));
        assert_eq!(line.total_duration, Some(2_500_000_000));
        assert_eq!(line.eval_duration, Some(2_000_000_000));
    }

    #[test]
    fn decoder_reads_thinking() {
        let mut decoder = NdjsonDecoder::new(1024);
        let lines = decoder
            .push(b"{\"message\":{\"role\":\"assistant\",\"content\":\"\",\"thinking\":\"hmm\"},\"done\":false}\n")
            .unwrap();
        let [line] = lines.as_slice() else {
            panic!("expected one line");
        };
        let message = line.message.as_ref().unwrap();
        assert!(message.content.as_ref().unwrap().is_empty());
        assert_eq!(message.thinking.as_ref().unwrap().expose(), "hmm");
    }

    #[test]
    fn decoder_rejects_line_over_limit() {
        let mut decoder = NdjsonDecoder::new(16);
        assert!(decoder.push(b"{\"message\":").is_ok());
        assert!(matches!(
            decoder.push(b"{\"content\":\"far too long\"}}"),
            Err(AppError::TooLarge(_))
        ));

        let mut decoder = NdjsonDecoder::new(16);
        assert!(matches!(
            decoder.push(b"{\"error\":\"this line is long\"}\n"),
            Err(AppError::TooLarge(_))
        ));
    }

    #[test]
    fn decoder_finish_parses_tail() {
        let mut decoder = NdjsonDecoder::new(1024);
        assert!(decoder.push(br#"{"done":true}"#).unwrap().is_empty());
        assert!(decoder.finish().unwrap().unwrap().done);
    }

    #[test]
    fn decoder_rejects_malformed_line() {
        let mut decoder = NdjsonDecoder::new(1024);
        assert!(matches!(
            decoder.push(b"not json\n"),
            Err(AppError::Ollama(_))
        ));
    }
}

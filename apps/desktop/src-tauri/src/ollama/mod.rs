//! Chat with a local Ollama: admission, streaming workers and cancellation.
//!
//! Only this module opens sockets, and only to a base URL that passed the
//! loopback guard in [`client::OllamaBaseUrl`].

pub mod client;
pub mod stream;
pub mod types;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use self::client::{OllamaBaseUrl, build_http, get_json};
use self::stream::{ChatEnd, run_chat};
use self::types::{
    ChatEvent, ChatMessage, ChatOptions, ChatRequest, ModelInfo, OllamaStatus, TagsResponse,
    VersionResponse, validate_chat_input,
};
use crate::audit::{AuditEvent, AuditSink, now_ms};
use crate::error::AppError;

const MIB: usize = 1024 * 1024;
const VERSION_TIMEOUT: Duration = Duration::from_secs(3);
const TAGS_TIMEOUT: Duration = Duration::from_secs(10);

/// Timeouts and size caps for chat requests.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Deadline for the response headers plus the first body chunk.
    pub first_chunk: Duration,
    /// Longest gap between two body chunks after the first.
    pub idle: Duration,
    /// Total content bytes of the messages sent.
    pub max_prompt_bytes: usize,
    pub max_messages: usize,
    pub max_line_bytes: usize,
    /// Total assistant content bytes received.
    pub max_content_bytes: usize,
    /// Total raw bytes received, so a peer cannot hold a slot forever with
    /// empty chunks.
    pub max_stream_bytes: usize,
    pub max_inflight: usize,
    /// How long `chat_cancel` waits for the worker to release its slot.
    pub cancel_wait: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            first_chunk: Duration::from_secs(90),
            idle: Duration::from_secs(60),
            max_prompt_bytes: MIB,
            max_messages: 1000,
            max_line_bytes: MIB,
            max_content_bytes: 4 * MIB,
            max_stream_bytes: 8 * MIB,
            max_inflight: 4,
            cancel_wait: Duration::from_secs(2),
        }
    }
}

#[derive(Debug)]
struct InflightEntry {
    conversation_id: Uuid,
    cancel: CancellationToken,
    /// Cancelled by the worker's guard once the entry is gone.
    finished: CancellationToken,
}

type Inflight = Arc<Mutex<HashMap<Uuid, InflightEntry>>>;

fn lock(inflight: &Inflight) -> MutexGuard<'_, HashMap<Uuid, InflightEntry>> {
    // Every critical section is a single map operation, so a panic elsewhere
    // cannot leave the map half-updated.
    inflight.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Removes the worker's entry on every exit path, including a panic, then
/// wakes any `chat_cancel` waiting for it.
struct InflightGuard {
    inflight: Inflight,
    request_id: Uuid,
    finished: CancellationToken,
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        lock(&self.inflight).remove(&self.request_id);
        self.finished.cancel();
    }
}

/// A slot taken by [`Ollama::admit`], to be handed to [`Ollama::start`].
/// Dropping it instead frees the slot.
pub struct Admission {
    guard: InflightGuard,
    cancel: CancellationToken,
}

/// The Ollama side of the app state, managed by Tauri as `Arc<Ollama>`.
pub struct Ollama {
    /// An invalid URL is kept as the error so the app still starts and every
    /// command reports it before opening a socket.
    base_url: Result<OllamaBaseUrl, AppError>,
    http: Result<reqwest::Client, AppError>,
    inflight: Inflight,
    audit: Arc<dyn AuditSink>,
    limits: Limits,
}

impl Ollama {
    pub fn new(base_url: Result<OllamaBaseUrl, AppError>, audit: Arc<dyn AuditSink>) -> Self {
        Self::with_limits(base_url, audit, Limits::default())
    }

    pub fn with_limits(
        base_url: Result<OllamaBaseUrl, AppError>,
        audit: Arc<dyn AuditSink>,
        limits: Limits,
    ) -> Self {
        let http = match &base_url {
            Ok(base) => build_http(base),
            Err(err) => Err(err.clone()),
        };
        Self {
            base_url,
            http,
            inflight: Arc::default(),
            audit,
            limits,
        }
    }

    fn connection(&self) -> Result<(&OllamaBaseUrl, &reqwest::Client), AppError> {
        let base = self.base_url.as_ref().map_err(Clone::clone)?;
        let http = self.http.as_ref().map_err(Clone::clone)?;
        Ok((base, http))
    }

    /// `reachable: false` when nothing answers; an error only for a rejected
    /// base URL or a broken reply.
    pub async fn status(&self) -> Result<OllamaStatus, AppError> {
        let (base, http) = self.connection()?;
        let started = Instant::now();
        let (http_status, result) =
            get_json::<VersionResponse>(http, base.endpoint("/api/version"), VERSION_TIMEOUT).await;
        self.audit.record(audit_event(
            "ollama.version",
            base,
            started,
            http_status,
            outcome(&result),
        ));
        let base_url = base.to_string();
        match result {
            Ok(version) => Ok(OllamaStatus {
                reachable: true,
                version: Some(version.version),
                base_url,
            }),
            Err(AppError::Unreachable(_) | AppError::Timeout(_)) => Ok(OllamaStatus {
                reachable: false,
                version: None,
                base_url,
            }),
            Err(err) => Err(err),
        }
    }

    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, AppError> {
        let (base, http) = self.connection()?;
        let started = Instant::now();
        let (http_status, result) =
            get_json::<TagsResponse>(http, base.endpoint("/api/tags"), TAGS_TIMEOUT).await;
        self.audit.record(audit_event(
            "ollama.tags",
            base,
            started,
            http_status,
            outcome(&result),
        ));
        Ok(result?.models.into_iter().map(ModelInfo::from).collect())
    }

    /// Checks a request against this client's limits, before any socket is
    /// opened.
    pub fn validate(
        &self,
        model: &str,
        messages: &[ChatMessage],
        options: Option<&ChatOptions>,
    ) -> Result<(), AppError> {
        validate_chat_input(
            model,
            messages,
            options,
            self.limits.max_messages,
            self.limits.max_prompt_bytes,
        )
    }

    /// Takes a slot for `conversation_id` (one per conversation, `max_inflight`
    /// in total). The slot is visible to `chat_cancel` and
    /// `cancel_conversation` at once; dropping the admission frees it.
    pub fn admit(&self, conversation_id: Uuid) -> Result<Admission, AppError> {
        self.connection()?;
        let request_id = Uuid::new_v4();
        let cancel = CancellationToken::new();
        let finished = CancellationToken::new();
        {
            let mut inflight = lock(&self.inflight);
            if inflight
                .values()
                .any(|entry| entry.conversation_id == conversation_id)
            {
                return Err(AppError::Busy(
                    "this conversation is still answering".into(),
                ));
            }
            if inflight.len() >= self.limits.max_inflight {
                return Err(AppError::Busy(format!(
                    "{} requests are already running",
                    self.limits.max_inflight
                )));
            }
            inflight.insert(
                request_id,
                InflightEntry {
                    conversation_id,
                    cancel: cancel.clone(),
                    finished: finished.clone(),
                },
            );
        }
        Ok(Admission {
            guard: InflightGuard {
                inflight: Arc::clone(&self.inflight),
                request_id,
                finished,
            },
            cancel,
        })
    }

    /// Spawns the streaming worker for an admitted request and returns its
    /// id. `on_end` runs exactly once, on every path, before the audit record.
    /// A request cancelled while admitted ends at once as `Cancelled`. Must be
    /// called inside a tokio runtime.
    pub fn start<F, E>(
        self: &Arc<Self>,
        admission: Admission,
        model: &str,
        messages: &[ChatMessage],
        options: Option<&ChatOptions>,
        sink: F,
        on_end: E,
    ) -> Result<Uuid, AppError>
    where
        F: Fn(ChatEvent) -> bool + Send + Sync + 'static,
        E: FnOnce(&ChatEnd) + Send + 'static,
    {
        let (base, http) = match self.connection() {
            Ok(connection) => connection,
            Err(err) => {
                on_end(&ChatEnd::Failed(err.clone()));
                return Err(err);
            }
        };
        let body = match serde_json::to_vec(&ChatRequest {
            model,
            messages,
            stream: true,
            options,
        }) {
            Ok(body) => body,
            Err(err) => {
                let err = AppError::Internal(format!("request body: {err}"));
                on_end(&ChatEnd::Failed(err.clone()));
                return Err(err);
            }
        };
        let url = base.endpoint("/api/chat");
        let base = base.clone();
        let http = http.clone();
        let Admission { guard, cancel } = admission;
        let request_id = guard.request_id;

        let this = Arc::clone(self);
        tokio::spawn(async move {
            let _guard = guard;
            let started = Instant::now();
            let run = run_chat(&http, url, body, &this.limits, &cancel, &sink).await;
            if let ChatEnd::Failed(error) = &run.end
                && !cancel.is_cancelled()
            {
                sink(ChatEvent::Error {
                    error: error.clone(),
                });
            }
            on_end(&run.end);
            let (outcome, prompt_tokens, completion_tokens) = match run.end {
                ChatEnd::Done {
                    prompt_tokens,
                    completion_tokens,
                    ..
                } => ("ok".to_owned(), prompt_tokens, completion_tokens),
                ChatEnd::Cancelled => ("cancelled".to_owned(), None, None),
                ChatEnd::Failed(error) => (format!("error:{}", error.kind()), None, None),
            };
            let mut event = audit_event("ollama.chat", &base, started, run.http_status, outcome);
            event.prompt_tokens = prompt_tokens;
            event.completion_tokens = completion_tokens;
            this.audit.record(event);
        });
        Ok(request_id)
    }

    /// Cancels a running request and waits (up to `cancel_wait`) for its
    /// worker to release the slot, so an immediate resend is admitted. An
    /// unknown or finished id is a no-op.
    pub async fn chat_cancel(&self, request_id: Uuid) -> Result<(), AppError> {
        let finished = {
            let inflight = lock(&self.inflight);
            let Some(entry) = inflight.get(&request_id) else {
                return Ok(());
            };
            entry.cancel.cancel();
            entry.finished.clone()
        };
        let _ = tokio::time::timeout(self.limits.cancel_wait, finished.cancelled()).await;
        Ok(())
    }

    /// Cancels whatever runs for `conversation_id`, admitted or streaming,
    /// and waits like [`Self::chat_cancel`]. Nothing running is a no-op.
    pub async fn cancel_conversation(&self, conversation_id: Uuid) {
        let finished: Vec<CancellationToken> = lock(&self.inflight)
            .values()
            .filter(|entry| entry.conversation_id == conversation_id)
            .map(|entry| {
                entry.cancel.cancel();
                entry.finished.clone()
            })
            .collect();
        for finished in finished {
            let _ = tokio::time::timeout(self.limits.cancel_wait, finished.cancelled()).await;
        }
    }

    #[cfg(test)]
    pub(crate) fn inflight_len(&self) -> usize {
        lock(&self.inflight).len()
    }
}

fn outcome<T>(result: &Result<T, AppError>) -> String {
    match result {
        Ok(_) => "ok".to_owned(),
        Err(err) => format!("error:{}", err.kind()),
    }
}

fn audit_event(
    action: &str,
    base: &OllamaBaseUrl,
    started: Instant,
    http_status: Option<u16>,
    outcome: String,
) -> AuditEvent {
    AuditEvent {
        at_ms: now_ms(),
        action: action.to_owned(),
        target: base.host_port(),
        outcome,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        http_status,
        prompt_tokens: None,
        completion_tokens: None,
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use super::*;
    use crate::audit::MemorySink;
    use crate::ollama::types::ChatRole;
    use crate::redacted::Redacted;

    /// What the fake Ollama answers to every connection.
    #[derive(Clone)]
    enum Reply {
        /// `200` with these NDJSON lines, each sent after `delay`.
        Lines(Vec<&'static str>, Duration),
        /// `200` with a JSON body.
        Json(&'static str),
        /// An error status with a JSON body.
        Status(u16, &'static str),
        /// Accepts and reads the request, then never answers.
        Silent,
        /// `200`, then `chunk` repeated until the client hangs up.
        Endless(&'static str),
    }

    struct Fake {
        addr: SocketAddr,
        accepted: Arc<AtomicUsize>,
    }

    async fn fake_ollama(reply: Reply) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&accepted);
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(serve(socket, reply.clone()));
            }
        });
        Fake { addr, accepted }
    }

    /// Reads one request (headers plus `content-length` body), then answers.
    async fn serve(mut socket: TcpStream, reply: Reply) {
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        let body_start = loop {
            let Ok(n) = socket.read(&mut buf).await else {
                return;
            };
            if n == 0 {
                return;
            }
            request.extend_from_slice(&buf[..n]);
            if let Some(pos) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..body_start]).to_ascii_lowercase();
        let length: usize = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .map_or(0, |value| value.trim().parse().unwrap());
        while request.len() < body_start + length {
            match socket.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => request.extend_from_slice(&buf[..n]),
            }
        }

        let head = |status: u16| {
            format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n"
            )
        };
        match reply {
            Reply::Lines(lines, delay) => {
                let _ = socket.write_all(head(200).as_bytes()).await;
                for line in lines {
                    tokio::time::sleep(delay).await;
                    if socket
                        .write_all(format!("{line}\n").as_bytes())
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
            Reply::Json(body) => {
                let _ = socket.write_all(head(200).as_bytes()).await;
                let _ = socket.write_all(body.as_bytes()).await;
            }
            Reply::Status(status, body) => {
                let _ = socket.write_all(head(status).as_bytes()).await;
                let _ = socket.write_all(body.as_bytes()).await;
            }
            Reply::Silent => std::future::pending::<()>().await,
            Reply::Endless(chunk) => {
                let _ = socket.write_all(head(200).as_bytes()).await;
                while socket.write_all(chunk.as_bytes()).await.is_ok() {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        }
        let _ = socket.shutdown().await;
    }

    fn test_limits() -> Limits {
        Limits {
            first_chunk: Duration::from_secs(5),
            idle: Duration::from_secs(5),
            max_prompt_bytes: 1024,
            max_messages: 10,
            max_line_bytes: 256,
            max_content_bytes: 64,
            max_stream_bytes: 4096,
            max_inflight: 4,
            cancel_wait: Duration::from_secs(2),
        }
    }

    fn ollama_at(addr: SocketAddr, limits: Limits) -> (Arc<Ollama>, Arc<MemorySink>) {
        let sink = Arc::new(MemorySink::new());
        let base = OllamaBaseUrl::parse(&format!("http://{addr}"));
        let ollama = Arc::new(Ollama::with_limits(base, sink.clone(), limits));
        (ollama, sink)
    }

    type Events = Arc<Mutex<Vec<ChatEvent>>>;

    fn collector() -> (Events, impl Fn(ChatEvent) -> bool + Send + Sync + 'static) {
        let events: Events = Arc::default();
        let store = Arc::clone(&events);
        (events, move |event| {
            store.lock().unwrap().push(event);
            true
        })
    }

    fn prompt() -> Vec<ChatMessage> {
        vec![ChatMessage {
            role: ChatRole::User,
            content: Redacted::new("hi"),
        }]
    }

    type Ends = Arc<Mutex<Vec<ChatEnd>>>;

    fn send_with_end<F>(
        ollama: &Arc<Ollama>,
        conversation: Uuid,
        sink: F,
        ends: &Ends,
    ) -> Result<Uuid, AppError>
    where
        F: Fn(ChatEvent) -> bool + Send + Sync + 'static,
    {
        let messages = prompt();
        ollama.validate("llama3.2", &messages, None)?;
        let admission = ollama.admit(conversation)?;
        let ends = Arc::clone(ends);
        ollama.start(admission, "llama3.2", &messages, None, sink, move |end| {
            ends.lock().unwrap().push(clone_end(end));
        })
    }

    fn send<F>(ollama: &Arc<Ollama>, conversation: Uuid, sink: F) -> Result<Uuid, AppError>
    where
        F: Fn(ChatEvent) -> bool + Send + Sync + 'static,
    {
        send_with_end(ollama, conversation, sink, &Ends::default())
    }

    fn clone_end(end: &ChatEnd) -> ChatEnd {
        match end {
            ChatEnd::Done {
                prompt_tokens,
                completion_tokens,
                duration_ms,
            } => ChatEnd::Done {
                prompt_tokens: *prompt_tokens,
                completion_tokens: *completion_tokens,
                duration_ms: *duration_ms,
            },
            ChatEnd::Cancelled => ChatEnd::Cancelled,
            ChatEnd::Failed(error) => ChatEnd::Failed(error.clone()),
        }
    }

    async fn wait_idle(ollama: &Ollama) {
        for _ in 0..500 {
            if ollama.inflight_len() == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("requests still in flight");
    }

    async fn wait_events(events: &Events, count: usize) {
        for _ in 0..500 {
            if events.lock().unwrap().len() >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("expected {count} events");
    }

    fn snapshot(events: &Events) -> Vec<ChatEvent> {
        events.lock().unwrap().clone()
    }

    fn last_error(events: &Events) -> AppError {
        match snapshot(events).last() {
            Some(ChatEvent::Error { error }) => error.clone(),
            other => panic!("expected an error event, got {other:?}"),
        }
    }

    fn token(text: &str) -> ChatEvent {
        ChatEvent::Token {
            content: Redacted::new(text),
        }
    }

    const HEL: &str = r#"{"message":{"role":"assistant","content":"Hel"},"done":false}"#;
    const LO: &str = r#"{"message":{"role":"assistant","content":"lo"},"done":false}"#;
    const DONE: &str = r#"{"message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","total_duration":5000000,"prompt_eval_count":3,"eval_count":2}"#;

    #[tokio::test]
    async fn streams_tokens_then_done() {
        let fake = fake_ollama(Reply::Lines(vec![HEL, LO, DONE], Duration::ZERO)).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let (events, sink) = collector();

        send(&ollama, Uuid::new_v4(), sink).unwrap();
        wait_idle(&ollama).await;

        assert_eq!(
            snapshot(&events),
            vec![
                token("Hel"),
                token("lo"),
                ChatEvent::Done {
                    prompt_tokens: Some(3),
                    completion_tokens: Some(2),
                    duration_ms: Some(5),
                    done_reason: Some("stop".into()),
                },
            ]
        );
    }

    #[tokio::test]
    async fn invalid_base_url_rejected_without_socket() {
        let fake = fake_ollama(Reply::Lines(vec![DONE], Duration::ZERO)).await;
        let sink = Arc::new(MemorySink::new());
        let base = OllamaBaseUrl::parse(&format!("http://10.0.0.1:{}", fake.addr.port()));
        let ollama = Arc::new(Ollama::with_limits(base, sink.clone(), test_limits()));
        let (events, emit) = collector();

        let err = send(&ollama, Uuid::new_v4(), emit).unwrap_err();
        assert!(matches!(err, AppError::InvalidBaseUrl(_)));
        assert!(matches!(
            ollama.status().await,
            Err(AppError::InvalidBaseUrl(_))
        ));
        assert!(matches!(
            ollama.list_models().await,
            Err(AppError::InvalidBaseUrl(_))
        ));

        assert_eq!(ollama.inflight_len(), 0);
        assert!(snapshot(&events).is_empty());
        assert!(sink.recent(usize::MAX).is_empty());
        assert_eq!(fake.accepted.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn cancel_before_first_chunk_frees_slot() {
        let fake = fake_ollama(Reply::Silent).await;
        let (ollama, audit) = ollama_at(fake.addr, test_limits());
        let (events, sink) = collector();

        let id = send(&ollama, Uuid::new_v4(), sink).unwrap();
        ollama.chat_cancel(id).await.unwrap();

        assert_eq!(ollama.inflight_len(), 0);
        assert!(snapshot(&events).is_empty());
        assert_eq!(audit.recent(1)[0].outcome, "cancelled");
    }

    #[tokio::test]
    async fn cancel_while_streaming_stops_events() {
        let fake = fake_ollama(Reply::Lines(
            vec![HEL, LO, LO, LO, LO, DONE],
            Duration::from_millis(100),
        ))
        .await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let (events, sink) = collector();

        let id = send(&ollama, Uuid::new_v4(), sink).unwrap();
        wait_events(&events, 1).await;
        ollama.chat_cancel(id).await.unwrap();
        let seen = snapshot(&events).len();
        assert_eq!(ollama.inflight_len(), 0);

        tokio::time::sleep(Duration::from_millis(400)).await;
        let after = snapshot(&events);
        assert_eq!(after.len(), seen, "events arrived after cancel");
        assert!(
            after
                .iter()
                .all(|event| matches!(event, ChatEvent::Token { .. }))
        );
    }

    #[tokio::test]
    async fn cancel_after_done_is_noop() {
        let fake = fake_ollama(Reply::Lines(vec![HEL, DONE], Duration::ZERO)).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let (events, sink) = collector();

        let id = send(&ollama, Uuid::new_v4(), sink).unwrap();
        wait_idle(&ollama).await;
        let before = snapshot(&events);
        ollama.chat_cancel(id).await.unwrap();

        assert_eq!(snapshot(&events), before);
        assert!(matches!(before.last(), Some(ChatEvent::Done { .. })));
    }

    #[tokio::test]
    async fn cancel_unknown_id_is_ok() {
        let fake = fake_ollama(Reply::Silent).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        assert_eq!(ollama.chat_cancel(Uuid::new_v4()).await, Ok(()));
    }

    #[tokio::test]
    async fn cancel_then_resend_same_conversation_is_admitted() {
        let fake = fake_ollama(Reply::Silent).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let conversation = Uuid::new_v4();

        let first = send(&ollama, conversation, |_| true).unwrap();
        ollama.chat_cancel(first).await.unwrap();
        let second = send(&ollama, conversation, |_| true).unwrap();

        assert_ne!(first, second);
        ollama.chat_cancel(second).await.unwrap();
        assert_eq!(ollama.inflight_len(), 0);
    }

    #[tokio::test]
    async fn repeated_cancel_resend_never_exceeds_max_inflight() {
        let fake = fake_ollama(Reply::Silent).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let conversations: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();

        for _ in 0..5 {
            let mut ids = Vec::new();
            for &conversation in &conversations {
                ids.push(send(&ollama, conversation, |_| true).unwrap());
                assert!(ollama.inflight_len() <= 4);
            }
            assert!(matches!(
                send(&ollama, Uuid::new_v4(), |_| true),
                Err(AppError::Busy(_))
            ));
            for id in ids {
                ollama.chat_cancel(id).await.unwrap();
            }
            assert_eq!(ollama.inflight_len(), 0);
        }
    }

    #[tokio::test]
    async fn sink_failure_cancels_stream() {
        let fake = fake_ollama(Reply::Lines(
            vec![HEL, LO, LO, DONE],
            Duration::from_millis(20),
        ))
        .await;
        let (ollama, audit) = ollama_at(fake.addr, test_limits());
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);

        send(&ollama, Uuid::new_v4(), move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            false
        })
        .unwrap();
        wait_idle(&ollama).await;

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(audit.recent(1)[0].outcome, "cancelled");
    }

    #[tokio::test]
    async fn second_request_same_conversation_is_busy() {
        let fake = fake_ollama(Reply::Silent).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let conversation = Uuid::new_v4();

        let first = send(&ollama, conversation, |_| true).unwrap();
        assert!(matches!(
            send(&ollama, conversation, |_| true),
            Err(AppError::Busy(_))
        ));
        assert_eq!(ollama.inflight_len(), 1);
        ollama.chat_cancel(first).await.unwrap();
    }

    #[tokio::test]
    async fn fifth_request_overall_is_busy() {
        let fake = fake_ollama(Reply::Silent).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());

        let ids: Vec<Uuid> = (0..4)
            .map(|_| send(&ollama, Uuid::new_v4(), |_| true).unwrap())
            .collect();
        assert!(matches!(
            send(&ollama, Uuid::new_v4(), |_| true),
            Err(AppError::Busy(_))
        ));
        assert_eq!(ollama.inflight_len(), 4);
        for id in ids {
            ollama.chat_cancel(id).await.unwrap();
        }
    }

    #[tokio::test]
    async fn entry_removed_after_http_error() {
        let fake = fake_ollama(Reply::Status(404, r#"{"error":"model not found"}"#)).await;
        let (ollama, audit) = ollama_at(fake.addr, test_limits());
        let (events, sink) = collector();

        send(&ollama, Uuid::new_v4(), sink).unwrap();
        wait_idle(&ollama).await;

        assert_eq!(
            last_error(&events),
            AppError::Ollama("HTTP 404: model not found".into())
        );
        let event = &audit.recent(1)[0];
        assert_eq!(event.outcome, "error:ollama");
        assert_eq!(event.http_status, Some(404));
    }

    #[tokio::test]
    async fn first_chunk_deadline_frees_slot() {
        let fake = fake_ollama(Reply::Silent).await;
        let limits = Limits {
            first_chunk: Duration::from_millis(200),
            ..test_limits()
        };
        let (ollama, _) = ollama_at(fake.addr, limits);
        let (events, sink) = collector();

        send(&ollama, Uuid::new_v4(), sink).unwrap();
        wait_idle(&ollama).await;

        assert!(matches!(last_error(&events), AppError::Timeout(_)));
    }

    #[tokio::test]
    async fn endless_line_trips_size_limit() {
        let fake = fake_ollama(Reply::Endless("xxxxxxxxxxxxxxxx")).await;
        // A large stream cap, so only the line cap can trip however the
        // bytes are split into chunks.
        let limits = Limits {
            max_stream_bytes: MIB,
            ..test_limits()
        };
        let (ollama, _) = ollama_at(fake.addr, limits);
        let (events, sink) = collector();

        send(&ollama, Uuid::new_v4(), sink).unwrap();
        wait_idle(&ollama).await;

        assert!(matches!(last_error(&events), AppError::TooLarge(_)));
    }

    #[tokio::test]
    async fn endless_stream_trips_content_limit() {
        let fake = fake_ollama(Reply::Endless(
            "{\"message\":{\"content\":\"abcdefgh\"},\"done\":false}\n",
        ))
        .await;
        let limits = Limits {
            max_stream_bytes: MIB,
            ..test_limits()
        };
        let (ollama, _) = ollama_at(fake.addr, limits);
        let (events, sink) = collector();

        send(&ollama, Uuid::new_v4(), sink).unwrap();
        wait_idle(&ollama).await;

        assert!(matches!(last_error(&events), AppError::TooLarge(_)));
        let tokens = snapshot(&events)
            .iter()
            .filter(|event| matches!(event, ChatEvent::Token { .. }))
            .count();
        assert_eq!(tokens, 8, "64 content bytes is eight 8-byte tokens");
    }

    #[tokio::test]
    async fn endless_empty_chunks_trip_stream_byte_limit() {
        let fake = fake_ollama(Reply::Endless(
            "{\"message\":{\"content\":\"\",\"thinking\":\"hm\"},\"done\":false}\n",
        ))
        .await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let (events, sink) = collector();

        send(&ollama, Uuid::new_v4(), sink).unwrap();
        wait_idle(&ollama).await;

        assert_eq!(snapshot(&events).len(), 1);
        assert!(matches!(last_error(&events), AppError::TooLarge(_)));
    }

    #[tokio::test]
    async fn too_many_messages_rejected() {
        let fake = fake_ollama(Reply::Silent).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let messages = vec![prompt()[0].clone(); 11];

        let err = ollama.validate("m", &messages, None).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
        assert_eq!(ollama.inflight_len(), 0);
        assert_eq!(fake.accepted.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn dropped_admission_frees_slot() {
        let fake = fake_ollama(Reply::Silent).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let conversation = Uuid::new_v4();

        let admission = ollama.admit(conversation).unwrap();
        assert_eq!(ollama.inflight_len(), 1);
        assert!(matches!(ollama.admit(conversation), Err(AppError::Busy(_))));
        drop(admission);

        assert_eq!(ollama.inflight_len(), 0);
        assert!(ollama.admit(conversation).is_ok());
        assert_eq!(fake.accepted.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn on_end_called_for_done_cancel_and_error() {
        let done = fake_ollama(Reply::Lines(vec![HEL, DONE], Duration::ZERO)).await;
        let (ollama, _) = ollama_at(done.addr, test_limits());
        let ends = Ends::default();
        send_with_end(&ollama, Uuid::new_v4(), |_| true, &ends).unwrap();
        wait_idle(&ollama).await;
        assert!(matches!(
            ends.lock().unwrap().as_slice(),
            [ChatEnd::Done {
                prompt_tokens: Some(3),
                completion_tokens: Some(2),
                duration_ms: Some(5),
            }]
        ));

        let silent = fake_ollama(Reply::Silent).await;
        let (ollama, _) = ollama_at(silent.addr, test_limits());
        let ends = Ends::default();
        let id = send_with_end(&ollama, Uuid::new_v4(), |_| true, &ends).unwrap();
        ollama.chat_cancel(id).await.unwrap();
        assert!(matches!(
            ends.lock().unwrap().as_slice(),
            [ChatEnd::Cancelled]
        ));

        let failing = fake_ollama(Reply::Status(500, r#"{"error":"boom"}"#)).await;
        let (ollama, _) = ollama_at(failing.addr, test_limits());
        let ends = Ends::default();
        send_with_end(&ollama, Uuid::new_v4(), |_| true, &ends).unwrap();
        wait_idle(&ollama).await;
        assert!(matches!(
            ends.lock().unwrap().as_slice(),
            [ChatEnd::Failed(AppError::Ollama(_))]
        ));
    }

    #[tokio::test]
    async fn cancel_conversation_stops_its_stream() {
        let fake = fake_ollama(Reply::Silent).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let conversation = Uuid::new_v4();
        let other = Uuid::new_v4();

        send(&ollama, conversation, |_| true).unwrap();
        let other_id = send(&ollama, other, |_| true).unwrap();
        ollama.cancel_conversation(conversation).await;

        assert_eq!(ollama.inflight_len(), 1);
        assert!(send(&ollama, conversation, |_| true).is_ok());
        ollama.chat_cancel(other_id).await.unwrap();
        ollama.cancel_conversation(conversation).await;
        assert_eq!(ollama.inflight_len(), 0);
    }

    #[tokio::test]
    async fn cancel_while_admitted_ends_as_cancelled() {
        let fake = fake_ollama(Reply::Lines(vec![HEL, DONE], Duration::ZERO)).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let conversation = Uuid::new_v4();
        let ends = Ends::default();
        let (events, sink) = collector();

        let admission = ollama.admit(conversation).unwrap();
        // A delete racing a send: the cancel lands between admit and start.
        let canceller = {
            let ollama = Arc::clone(&ollama);
            tokio::spawn(async move { ollama.cancel_conversation(conversation).await })
        };
        for _ in 0..500 {
            if lock(&ollama.inflight)
                .values()
                .all(|entry| entry.cancel.is_cancelled())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let recorded = Arc::clone(&ends);
        ollama
            .start(admission, "m", &prompt(), None, sink, move |end| {
                recorded.lock().unwrap().push(clone_end(end));
            })
            .unwrap();
        canceller.await.unwrap();

        assert_eq!(ollama.inflight_len(), 0);
        assert!(snapshot(&events).is_empty());
        assert!(matches!(
            ends.lock().unwrap().as_slice(),
            [ChatEnd::Cancelled]
        ));
    }

    #[tokio::test]
    async fn stream_without_done_is_error() {
        let fake = fake_ollama(Reply::Lines(vec![HEL], Duration::ZERO)).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let (events, sink) = collector();

        send(&ollama, Uuid::new_v4(), sink).unwrap();
        wait_idle(&ollama).await;

        assert_eq!(snapshot(&events)[0], token("Hel"));
        assert!(matches!(last_error(&events), AppError::Ollama(_)));
    }

    #[tokio::test]
    async fn error_line_is_error() {
        let fake = fake_ollama(Reply::Lines(
            vec![HEL, r#"{"error":"out of memory"}"#],
            Duration::ZERO,
        ))
        .await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let (events, sink) = collector();

        send(&ollama, Uuid::new_v4(), sink).unwrap();
        wait_idle(&ollama).await;

        assert_eq!(
            last_error(&events),
            AppError::Ollama("out of memory".into())
        );
    }

    #[tokio::test]
    async fn audit_records_chat_outcome() {
        let fake = fake_ollama(Reply::Lines(vec![HEL, DONE], Duration::ZERO)).await;
        let (ollama, audit) = ollama_at(fake.addr, test_limits());

        send(&ollama, Uuid::new_v4(), |_| true).unwrap();
        wait_idle(&ollama).await;

        let events = audit.recent(usize::MAX);
        let [event] = events.as_slice() else {
            panic!("expected one audit event, got {events:?}");
        };
        assert_eq!(event.action, "ollama.chat");
        assert_eq!(event.target, fake.addr.to_string());
        assert_eq!(event.outcome, "ok");
        assert_eq!(event.http_status, Some(200));
        assert_eq!(event.prompt_tokens, Some(3));
        assert_eq!(event.completion_tokens, Some(2));
    }

    #[tokio::test]
    async fn status_unreachable_when_nothing_listens() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let (ollama, audit) = ollama_at(addr, test_limits());

        let status = ollama.status().await.unwrap();
        assert!(!status.reachable);
        assert_eq!(status.version, None);
        assert_eq!(status.base_url, format!("http://{addr}"));
        // Windows takes about 2 s to refuse a loopback connect, so on a busy
        // machine the 3 s request timeout can win; both mean "not running".
        let outcome = &audit.recent(1)[0].outcome;
        assert!(
            outcome == "error:unreachable" || outcome == "error:timeout",
            "{outcome}"
        );
    }

    #[tokio::test]
    async fn status_reports_version() {
        let fake = fake_ollama(Reply::Json(r#"{"version":"0.12.3"}"#)).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());

        let status = ollama.status().await.unwrap();
        assert!(status.reachable);
        assert_eq!(status.version.as_deref(), Some("0.12.3"));
    }

    #[tokio::test]
    async fn list_models_parses_tags() {
        let fake = fake_ollama(Reply::Json(
            r#"{"models":[{"name":"llama3.2:latest","model":"llama3.2:latest","size":2019393189,"digest":"abc","details":{"family":"llama","parameter_size":"3.2B","quantization_level":"Q4_K_M"}}]}"#,
        ))
        .await;
        let (ollama, audit) = ollama_at(fake.addr, test_limits());

        let models = ollama.list_models().await.unwrap();
        assert_eq!(
            models,
            vec![ModelInfo {
                name: "llama3.2:latest".into(),
                size_bytes: Some(2_019_393_189),
                parameter_size: Some("3.2B".into()),
            }]
        );
        assert_eq!(audit.recent(1)[0].action, "ollama.tags");
    }
}

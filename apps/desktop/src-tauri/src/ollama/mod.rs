//! Chat with a local Ollama: admission, streaming workers and cancellation.
//!
//! Only this module opens sockets, and only to a base URL that passed the
//! loopback guard in [`client::OllamaBaseUrl`].

pub mod chart_tool;
pub mod client;
pub mod stream;
#[cfg(test)]
pub(crate) mod testing;
pub mod types;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;

use self::chart_tool::{CHART_TOOLS, ChartEntry};
use self::client::{MAX_SHOW_BODY, OllamaBaseUrl, build_http, get_json, post_json};
use self::stream::{ChatEnd, run_chat};
use self::types::{
    ChatEvent, ChatMessage, ChatOptions, ChatRequest, ModelCapabilities, ModelInfo, OllamaStatus,
    ShowResponse, TagsResponse, Think, VersionResponse, resolve_think, validate_chat_input,
    validate_model,
};
use crate::audit::{Audit, AuditId, AuditOutcome, AuditStatus, NewAuditEvent};
use crate::error::AppError;

const MIB: usize = 1024 * 1024;
const VERSION_TIMEOUT: Duration = Duration::from_secs(3);
const TAGS_TIMEOUT: Duration = Duration::from_secs(10);
const SHOW_TIMEOUT: Duration = Duration::from_secs(10);
/// Audit kinds of `/api/chat` requests: a chat's turn, and a project memory
/// summary.
const CHAT_KIND: &str = "ollama.chat";
const MEMORY_KIND: &str = "ollama.memory";

/// Capabilities by (`host:port`, model), so a lookup is one audit row per
/// model and Ollama.
type CapabilityCache = Mutex<HashMap<(String, String), ModelCapabilities>>;

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
    /// The chat, or for a memory summary the project.
    conversation_id: Uuid,
    /// The chat a memory summary is written for; a send in that chat
    /// cancels it.
    summarizing: Option<Uuid>,
    /// A summary the scheduler started on its own; every other admission
    /// cancels it.
    background: bool,
    cancel: CancellationToken,
    /// Cancelled by the worker's guard once the entry is gone.
    finished: CancellationToken,
}

impl InflightEntry {
    /// False for a background summary already told to stop: it is winding
    /// down and no longer holds its slot.
    fn live(&self) -> bool {
        !(self.background && self.cancel.is_cancelled())
    }
}

#[derive(Debug, Default)]
struct InflightMap {
    entries: Mutex<HashMap<Uuid, InflightEntry>>,
    /// Woken when the last entry goes.
    emptied: Notify,
}

type Inflight = Arc<InflightMap>;

fn lock(inflight: &Inflight) -> MutexGuard<'_, HashMap<Uuid, InflightEntry>> {
    // Every critical section is a single map operation, so a panic elsewhere
    // cannot leave the map half-updated.
    inflight
        .entries
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
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
        let empty = {
            let mut entries = lock(&self.inflight);
            entries.remove(&self.request_id);
            entries.is_empty()
        };
        self.finished.cancel();
        if empty {
            self.inflight.emptied.notify_waiters();
        }
    }
}

/// A slot taken by [`Ollama::admit`], to be handed to [`Ollama::start`].
/// Dropping it instead frees the slot.
pub struct Admission {
    guard: InflightGuard,
    cancel: CancellationToken,
}

impl Admission {
    /// The id `chat_cancel` takes for this slot.
    pub fn request_id(&self) -> Uuid {
        self.guard.request_id
    }

    /// Cancelled by `chat_cancel` or `cancel_conversation`.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Completes once the slot's request is cancelled.
    pub async fn cancelled(&self) {
        self.cancel.cancelled().await;
    }
}

/// One `/api/chat` request for [`Ollama::run`].
pub struct ChatCall<'a> {
    pub model: &'a str,
    pub messages: &'a [ChatMessage],
    pub options: Option<&'a ChatOptions>,
    /// From [`Ollama::chat_think`].
    pub think: ChatThink,
    /// `Some(0)` unloads the model once it has answered.
    pub keep_alive: Option<u32>,
    /// Offers the display-only `render_chart` tool.
    pub tools: bool,
}

/// What the request continuing a charted reply reuses from the first one.
struct FollowUpCall {
    model: String,
    options: Option<ChatOptions>,
    think: ChatThink,
}

/// `a + b`, where a missing count is left out rather than unknown.
fn plus(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

/// A follow-up's `Done` with the first request's counts added, so the reply
/// shows what both requests took.
fn summed_event(first: &ChatEnd, event: ChatEvent) -> ChatEvent {
    let (
        ChatEnd::Done {
            prompt_tokens: p,
            completion_tokens: c,
            duration_ms: d,
            thinking_ms: t,
            eval_ms: e,
            ..
        },
        ChatEvent::Done {
            prompt_tokens,
            completion_tokens,
            duration_ms,
            done_reason,
            thinking_ms,
            eval_ms,
        },
    ) = (first, &event)
    else {
        return event;
    };
    ChatEvent::Done {
        prompt_tokens: plus(*p, *prompt_tokens),
        completion_tokens: plus(*c, *completion_tokens),
        duration_ms: plus(*d, *duration_ms),
        done_reason: done_reason.clone(),
        thinking_ms: plus(*t, *thinking_ms),
        eval_ms: plus(*e, *eval_ms),
    }
}

/// Like [`summed_event`], for the end; the charts are the first request's.
fn summed_end(first: &ChatEnd, end: ChatEnd) -> ChatEnd {
    let (
        ChatEnd::Done {
            prompt_tokens: p,
            completion_tokens: c,
            duration_ms: d,
            thinking_ms: t,
            eval_ms: e,
            prompt_eval_ms: pe,
            load_ms: l,
            charts,
        },
        ChatEnd::Done {
            prompt_tokens,
            completion_tokens,
            duration_ms,
            thinking_ms,
            eval_ms,
            prompt_eval_ms,
            load_ms,
            ..
        },
    ) = (first, &end)
    else {
        return end;
    };
    ChatEnd::Done {
        prompt_tokens: plus(*p, *prompt_tokens),
        completion_tokens: plus(*c, *completion_tokens),
        duration_ms: plus(*d, *duration_ms),
        thinking_ms: plus(*t, *thinking_ms),
        eval_ms: plus(*e, *eval_ms),
        prompt_eval_ms: plus(*pe, *prompt_eval_ms),
        load_ms: plus(*l, *load_ms),
        charts: charts.clone(),
    }
}

/// Which [`Connection`] something was decided against. Every
/// [`Ollama::set_base_url`] makes a new one, even for the same URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionId(u64);

/// The `think` value for one chat, tied to the connection whose `/api/show`
/// it was resolved from, so it is never sent to another Ollama.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatThink {
    pub think: Option<Think>,
    pub connection: ConnectionId,
    /// The model's audited capabilities list `tools`, so it may be offered
    /// `render_chart`.
    pub tools: bool,
}

/// A base URL and the client built for it. An invalid URL is kept as the
/// error so the app still starts and every command reports it before opening
/// a socket.
struct Connection {
    id: ConnectionId,
    base_url: Result<OllamaBaseUrl, AppError>,
    http: Result<reqwest::Client, AppError>,
}

impl Connection {
    fn new(id: ConnectionId, base_url: Result<OllamaBaseUrl, AppError>) -> Self {
        let http = match &base_url {
            Ok(base) => build_http(base),
            Err(err) => Err(err.clone()),
        };
        Self { id, base_url, http }
    }

    fn get(&self) -> Result<(&OllamaBaseUrl, &reqwest::Client), AppError> {
        let base = self.base_url.as_ref().map_err(Clone::clone)?;
        let http = self.http.as_ref().map_err(Clone::clone)?;
        Ok((base, http))
    }
}

/// One outbound request: its audit row, written before the socket opens, and
/// its log span.
struct Tracked {
    audit: Arc<Audit>,
    id: AuditId,
    started: Instant,
    span: tracing::Span,
}

impl Tracked {
    fn begin(
        audit: &Arc<Audit>,
        kind: &'static str,
        base: &OllamaBaseUrl,
        endpoint: &'static str,
        model: Option<&str>,
    ) -> Result<Self, AppError> {
        let host = base.host_port();
        let span = tracing::info_span!("ollama_request", kind, host = %host, endpoint);
        let id = audit.begin(NewAuditEvent::request(kind, host, endpoint, model))?;
        Ok(Self {
            audit: Arc::clone(audit),
            id,
            started: Instant::now(),
            span,
        })
    }

    /// Logs counts and duration (never content) and writes the outcome.
    fn finish(self, mut outcome: AuditOutcome) {
        let duration_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        outcome.duration_ms = Some(duration_ms);
        self.span.in_scope(|| {
            tracing::info!(
                status = outcome.status.as_str(),
                duration_ms,
                prompt_tokens = outcome.prompt_tokens,
                completion_tokens = outcome.completion_tokens,
                eval_ms = outcome.eval_ms,
                prompt_eval_ms = outcome.prompt_eval_ms,
                load_ms = outcome.load_ms,
                detail = outcome.detail.as_deref(),
                "request finished"
            );
        });
        self.audit.finish(self.id, &outcome);
    }
}

/// A chat request ready to send, its `attempt` row written.
struct Prepared {
    http: reqwest::Client,
    url: url::Url,
    body: Vec<u8>,
    tools: bool,
    tracked: Tracked,
}

fn connection_changed() -> AppError {
    AppError::Ollama("the Ollama address changed while sending; send again".into())
}

fn result_outcome<T>(result: &Result<T, AppError>, http_status: Option<u16>) -> AuditOutcome {
    match result {
        Ok(_) => AuditOutcome::new(AuditStatus::Ok),
        Err(err) => AuditOutcome::failed(err, http_status),
    }
}

/// The Ollama side of the app state, managed by Tauri as `Arc<Ollama>`.
pub struct Ollama {
    /// Swapped by [`Self::set_base_url`]; a request keeps the one it started
    /// with.
    connection: RwLock<Arc<Connection>>,
    inflight: Inflight,
    audit: Arc<Audit>,
    limits: Limits,
    capabilities: CapabilityCache,
    /// The id the next connection gets.
    next_connection: AtomicU64,
    /// Projects whose whole memory is being refreshed, chat by chat.
    refreshing: Mutex<HashSet<Uuid>>,
}

/// Holds a project's whole-memory refresh until dropped; from
/// [`Ollama::reserve_refresh`].
pub struct RefreshReservation<'a> {
    ollama: &'a Ollama,
    project_id: Uuid,
}

impl Drop for RefreshReservation<'_> {
    fn drop(&mut self) {
        self.ollama.refreshing().remove(&self.project_id);
    }
}

impl Ollama {
    pub fn new(base_url: Result<OllamaBaseUrl, AppError>, audit: Arc<Audit>) -> Self {
        Self::with_limits(base_url, audit, Limits::default())
    }

    pub fn with_limits(
        base_url: Result<OllamaBaseUrl, AppError>,
        audit: Arc<Audit>,
        limits: Limits,
    ) -> Self {
        Self {
            connection: RwLock::new(Arc::new(Connection::new(ConnectionId(0), base_url))),
            inflight: Arc::default(),
            audit,
            limits,
            capabilities: Mutex::default(),
            next_connection: AtomicU64::new(1),
            refreshing: Mutex::default(),
        }
    }

    fn refreshing(&self) -> MutexGuard<'_, HashSet<Uuid>> {
        // Every critical section is one set operation.
        self.refreshing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Reserves project `project_id` for one whole-memory refresh, so two
    /// cannot interleave chat by chat. `Busy` while another holds it.
    pub fn reserve_refresh(&self, project_id: Uuid) -> Result<RefreshReservation<'_>, AppError> {
        if !self.refreshing().insert(project_id) {
            return Err(AppError::Busy(
                "project memory is already being refreshed".into(),
            ));
        }
        Ok(RefreshReservation {
            ollama: self,
            project_id,
        })
    }

    fn capability_cache(&self) -> MutexGuard<'_, HashMap<(String, String), ModelCapabilities>> {
        // Every critical section is one map operation.
        self.capabilities
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn connection(&self) -> Arc<Connection> {
        // Every critical section is one assignment or one clone, so a
        // poisoned lock is still consistent.
        Arc::clone(
            &self
                .connection
                .read()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }

    /// Points later requests at `base_url`. Requests already running keep
    /// their target.
    pub fn set_base_url(&self, base_url: Result<OllamaBaseUrl, AppError>) {
        let id = ConnectionId(self.next_connection.fetch_add(1, Ordering::Relaxed));
        let connection = Arc::new(Connection::new(id, base_url));
        *self
            .connection
            .write()
            .unwrap_or_else(PoisonError::into_inner) = connection;
        // The same host may now be a different Ollama with other models.
        self.capability_cache().clear();
    }

    /// `reachable: false` when nothing answers; an error only for a rejected
    /// base URL, a broken reply or no audit log.
    pub async fn status(&self) -> Result<OllamaStatus, AppError> {
        let connection = self.connection();
        let (base, http) = connection.get()?;
        let tracked = Tracked::begin(&self.audit, "ollama.version", base, "/api/version", None)?;
        let (http_status, result) =
            get_json::<VersionResponse>(http, base.endpoint("/api/version"), VERSION_TIMEOUT)
                .instrument(tracked.span.clone())
                .await;
        tracked.finish(result_outcome(&result, http_status));
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
        let connection = self.connection();
        let (base, http) = connection.get()?;
        let tracked = Tracked::begin(&self.audit, "ollama.tags", base, "/api/tags", None)?;
        let (http_status, result) =
            get_json::<TagsResponse>(http, base.endpoint("/api/tags"), TAGS_TIMEOUT)
                .instrument(tracked.span.clone())
                .await;
        tracked.finish(result_outcome(&result, http_status));
        Ok(result?.models.into_iter().map(ModelInfo::from).collect())
    }

    /// The connection requests start on now.
    pub fn connection_id(&self) -> ConnectionId {
        self.connection().id
    }

    /// Fails when the base URL changed since `id` was current, so a decision
    /// made against one Ollama is never applied to another.
    pub fn check_connection(&self, id: ConnectionId) -> Result<(), AppError> {
        if self.connection_id() == id {
            Ok(())
        } else {
            Err(connection_changed())
        }
    }

    /// What `model` can do, from an audited `/api/show`. Successful lookups
    /// are cached per base URL, so only the first costs a request.
    pub async fn model_capabilities(&self, model: &str) -> Result<ModelCapabilities, AppError> {
        self.capabilities_on(&self.connection(), model).await
    }

    /// The `think` value for a chat with `model`, resolved from that model's
    /// capabilities on the current connection. The chat must start on the
    /// same connection: [`Self::start`] refuses it otherwise. A failed lookup
    /// fails, since without it a thinking model would get Ollama's default.
    pub async fn chat_think(
        &self,
        model: &str,
        requested: Option<Think>,
    ) -> Result<ChatThink, AppError> {
        let connection = self.connection();
        let caps = self.capabilities_on(&connection, model).await?;
        Ok(ChatThink {
            think: resolve_think(&caps, requested)?,
            connection: connection.id,
            tools: caps.tools,
        })
    }

    async fn capabilities_on(
        &self,
        connection: &Arc<Connection>,
        model: &str,
    ) -> Result<ModelCapabilities, AppError> {
        validate_model(model)?;
        let (base, http) = connection.get()?;
        let key = (base.host_port(), model.to_owned());
        if let Some(cached) = self.capability_cache().get(&key) {
            return Ok(cached.clone());
        }
        let tracked = Tracked::begin(&self.audit, "ollama.show", base, "/api/show", Some(model))?;
        let (http_status, result) = post_json::<ShowResponse>(
            http,
            base.endpoint("/api/show"),
            &serde_json::json!({ "model": model }),
            SHOW_TIMEOUT,
            MAX_SHOW_BODY,
        )
        .instrument(tracked.span.clone())
        .await;
        tracked.finish(result_outcome(&result, http_status));
        let caps = ModelCapabilities::from_show(model, &result?);
        // A base URL switched during the lookup must not get this answer.
        if connection.id == self.connection_id() {
            self.capability_cache().insert(key, caps.clone());
        }
        Ok(caps)
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

    /// What a request may send in total, in content bytes.
    pub fn max_prompt_bytes(&self) -> usize {
        self.limits.max_prompt_bytes
    }

    /// Takes a slot for `conversation_id` (one per conversation, `max_inflight`
    /// in total). The slot is visible to `chat_cancel` and
    /// `cancel_conversation` at once; dropping the admission frees it. Under
    /// the same lock, a memory summary of this chat is cancelled, since the
    /// turn changes what it summarizes, and so is a background summary of any
    /// chat.
    pub fn admit(&self, conversation_id: Uuid) -> Result<Admission, AppError> {
        self.admit_entry(conversation_id, None, false)
    }

    /// Takes the slot a memory summary of `conversation_id` runs on. It is
    /// keyed by `project_id`, so one summary runs per project and the chat
    /// itself never reports `Busy` because of it. `Busy` while the chat is
    /// answering.
    pub fn admit_summary(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<Admission, AppError> {
        self.admit_entry(project_id, Some(conversation_id), false)
    }

    /// Like [`Self::admit_summary`], for a summary the scheduler starts on
    /// its own: `Busy` unless nothing else is in flight, checked under the
    /// same lock as the insert. Any later admission cancels it, so a user's
    /// request never waits behind it.
    pub fn admit_background(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<Admission, AppError> {
        self.admit_entry(project_id, Some(conversation_id), true)
    }

    fn admit_entry(
        &self,
        conversation_id: Uuid,
        summarizing: Option<Uuid>,
        background: bool,
    ) -> Result<Admission, AppError> {
        self.connection().get()?;
        let request_id = Uuid::new_v4();
        let cancel = CancellationToken::new();
        let finished = CancellationToken::new();
        {
            let mut inflight = lock(&self.inflight);
            if background {
                if !inflight.is_empty() {
                    return Err(AppError::Busy("another request is running".into()));
                }
            } else {
                for entry in inflight.values().filter(|entry| entry.background) {
                    entry.cancel.cancel();
                }
            }
            if let Some(chat) = summarizing
                && inflight
                    .values()
                    .any(|entry| entry.live() && entry.conversation_id == chat)
            {
                return Err(AppError::Busy("this chat is answering".into()));
            }
            if inflight
                .values()
                .any(|entry| entry.live() && entry.conversation_id == conversation_id)
            {
                return Err(AppError::Busy(if summarizing.is_some() {
                    "project memory is already being refreshed".into()
                } else {
                    "this conversation is still answering".into()
                }));
            }
            if inflight.values().filter(|entry| entry.live()).count() >= self.limits.max_inflight {
                return Err(AppError::Busy(format!(
                    "{} requests are already running",
                    self.limits.max_inflight
                )));
            }
            if summarizing.is_none() {
                for entry in inflight.values() {
                    if entry.summarizing == Some(conversation_id) {
                        entry.cancel.cancel();
                    }
                }
            }
            inflight.insert(
                request_id,
                InflightEntry {
                    conversation_id,
                    summarizing,
                    background,
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

    /// Everything `start` needs before spawning; the audit row, of `kind`,
    /// is written last, so a request refused here leaves no `attempt` behind.
    fn prepare_chat(&self, call: ChatCall<'_>, kind: &'static str) -> Result<Prepared, AppError> {
        let connection = self.connection();
        if connection.id != call.think.connection {
            return Err(connection_changed());
        }
        let (base, http) = connection.get()?;
        let body = serde_json::to_vec(&ChatRequest {
            model: call.model,
            messages: call.messages,
            stream: true,
            options: call.options,
            think: call.think.think,
            keep_alive: call.keep_alive,
            tools: call.tools.then_some(&*CHART_TOOLS),
        })
        .map_err(|err| AppError::Internal(format!("request body: {err}")))?;
        let tracked = Tracked::begin(&self.audit, kind, base, "/api/chat", Some(call.model))?;
        Ok(Prepared {
            http: http.clone(),
            url: base.endpoint("/api/chat"),
            body,
            tools: call.tools,
            tracked,
        })
    }

    /// Streams one prepared request to its end. A failure that was not a
    /// cancel reaches `sink` as an `Error` event. Returns the end and the
    /// outcome to write once the caller has handled the end.
    async fn stream(
        &self,
        http: &reqwest::Client,
        url: url::Url,
        body: Vec<u8>,
        tools: bool,
        cancel: &CancellationToken,
        sink: &(dyn Fn(ChatEvent) -> bool + Send + Sync),
    ) -> (ChatEnd, AuditOutcome) {
        let run = run_chat(http, url, body, tools, &self.limits, cancel, sink).await;
        if let ChatEnd::Failed(error) = &run.end
            && !cancel.is_cancelled()
        {
            sink(ChatEvent::Error {
                error: error.clone(),
            });
        }
        let outcome = match &run.end {
            // This request's own timings: a follow-up has its own row.
            ChatEnd::Done {
                prompt_tokens,
                completion_tokens,
                eval_ms,
                prompt_eval_ms,
                load_ms,
                ..
            } => AuditOutcome {
                prompt_tokens: *prompt_tokens,
                completion_tokens: *completion_tokens,
                eval_ms: *eval_ms,
                prompt_eval_ms: *prompt_eval_ms,
                load_ms: *load_ms,
                ..AuditOutcome::new(AuditStatus::Ok)
            },
            ChatEnd::Cancelled => AuditOutcome::new(AuditStatus::Cancelled),
            ChatEnd::Failed(error) => AuditOutcome::failed(error, run.http_status),
        };
        (run.end, outcome)
    }

    /// Runs one chat on `admission`'s slot and waits for its end, so several
    /// chats can run one after another on one slot. Otherwise like
    /// [`Self::start`]: the `attempt` row is written before the socket
    /// opens, and `on_end` runs exactly once, before the outcome is written.
    /// A request refused before its socket opens also reaches `sink` as an
    /// `Error` event. Once the admission is cancelled, a run ends as
    /// `Cancelled` without a request.
    pub async fn run<E>(
        &self,
        admission: &Admission,
        call: ChatCall<'_>,
        sink: &(dyn Fn(ChatEvent) -> bool + Send + Sync),
        on_end: E,
    ) where
        E: FnOnce(&ChatEnd),
    {
        self.run_as(CHAT_KIND, admission, call, sink, on_end).await;
    }

    /// Like [`Self::run`], for a project memory summary: audited as
    /// `ollama.memory`.
    pub async fn run_memory<E>(
        &self,
        admission: &Admission,
        call: ChatCall<'_>,
        sink: &(dyn Fn(ChatEvent) -> bool + Send + Sync),
        on_end: E,
    ) where
        E: FnOnce(&ChatEnd),
    {
        self.run_as(MEMORY_KIND, admission, call, sink, on_end)
            .await;
    }

    /// Writes an `ollama.memory` row for a summary refused before any
    /// request, with why in `detail`. Nothing is sent.
    pub fn record_memory_skip(&self, model: &str, detail: &str) {
        let host = self
            .connection()
            .base_url
            .as_ref()
            .map(OllamaBaseUrl::host_port)
            .unwrap_or_default();
        let mut event = NewAuditEvent::request(MEMORY_KIND, host, "/api/chat", Some(model));
        event.outcome = AuditOutcome {
            detail: Some(detail.to_owned()),
            ..AuditOutcome::new(AuditStatus::Error)
        };
        self.audit.record(event);
    }

    async fn run_as<E>(
        &self,
        kind: &'static str,
        admission: &Admission,
        call: ChatCall<'_>,
        sink: &(dyn Fn(ChatEvent) -> bool + Send + Sync),
        on_end: E,
    ) where
        E: FnOnce(&ChatEnd),
    {
        if admission.is_cancelled() {
            on_end(&ChatEnd::Cancelled);
            return;
        }
        let Prepared {
            http,
            url,
            body,
            tools,
            tracked,
        } = match self.prepare_chat(call, kind) {
            Ok(prepared) => prepared,
            Err(error) => {
                sink(ChatEvent::Error {
                    error: error.clone(),
                });
                on_end(&ChatEnd::Failed(error));
                return;
            }
        };
        let span = tracked.span.clone();
        let (end, outcome) = self
            .stream(&http, url, body, tools, &admission.cancel, sink)
            .instrument(span)
            .await;
        on_end(&end);
        tracked.finish(outcome);
    }

    /// Writes the request's `attempt` row, spawns the streaming worker for an
    /// admitted request and returns its id. `on_end` runs exactly once, on
    /// every path, before the last outcome is written. A request cancelled
    /// while admitted ends at once as `Cancelled`. Must be called inside a
    /// tokio runtime. `think` comes from [`Self::chat_think`]; a request
    /// whose connection is no longer current is refused before its socket
    /// opens.
    ///
    /// When the reply calls `render_chart`, a second request on the same slot
    /// continues it: `follow_up` builds its history from the calls, and it
    /// goes with the same model, options and think but no `tools`, so the
    /// model cannot call again. It writes its own audit row, and its `Done`
    /// carries both requests' counts. A follow-up over the request limits is
    /// refused without a request, as an `Error`.
    pub fn start<F, U, E>(
        self: &Arc<Self>,
        admission: Admission,
        call: ChatCall<'_>,
        sink: F,
        follow_up: U,
        on_end: E,
    ) -> Result<Uuid, AppError>
    where
        F: Fn(ChatEvent) -> bool + Send + Sync + 'static,
        U: FnOnce(&[ChartEntry]) -> Vec<ChatMessage> + Send + 'static,
        E: FnOnce(&ChatEnd) + Send + 'static,
    {
        let next = FollowUpCall {
            model: call.model.to_owned(),
            options: call.options.copied(),
            think: call.think.clone(),
        };
        let Prepared {
            http,
            url,
            body,
            tools,
            tracked,
        } = match self.prepare_chat(call, CHAT_KIND) {
            Ok(prepared) => prepared,
            Err(err) => {
                on_end(&ChatEnd::Failed(err.clone()));
                return Err(err);
            }
        };
        let request_id = admission.request_id();

        let this = Arc::clone(self);
        let span = tracked.span.clone();
        tokio::spawn(
            async move {
                let (end, outcome) = this
                    .stream(&http, url, body, tools, &admission.cancel, &sink)
                    .await;
                let charts = match &end {
                    ChatEnd::Done { charts, .. } if !charts.is_empty() => charts.clone(),
                    _ => {
                        on_end(&end);
                        tracked.finish(outcome);
                        return;
                    }
                };
                tracked.finish(outcome);
                let messages = follow_up(&charts);
                let end = this
                    .continue_reply(&admission, &next, &messages, &end, &sink)
                    .await;
                on_end(&end);
            }
            .instrument(span),
        );
        Ok(request_id)
    }

    /// The follow-up request of a charted reply, ended by `first`.
    async fn continue_reply<F>(
        &self,
        admission: &Admission,
        next: &FollowUpCall,
        messages: &[ChatMessage],
        first: &ChatEnd,
        sink: &F,
    ) -> ChatEnd
    where
        F: Fn(ChatEvent) -> bool + Send + Sync,
    {
        if admission.is_cancelled() {
            return ChatEnd::Cancelled;
        }
        if let Err(error) = self.validate(&next.model, messages, next.options.as_ref()) {
            sink(ChatEvent::Error {
                error: error.clone(),
            });
            return ChatEnd::Failed(error);
        }
        let summing = |event| sink(summed_event(first, event));
        let call = ChatCall {
            model: &next.model,
            messages,
            options: next.options.as_ref(),
            think: next.think.clone(),
            keep_alive: None,
            tools: false,
        };
        let mut last = ChatEnd::Cancelled;
        self.run(admission, call, &summing, |end| last = end.clone())
            .await;
        summed_end(first, last)
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
    /// including a memory summary of it or, for a project id, of any of its
    /// chats, and waits like [`Self::chat_cancel`]. Nothing running is a
    /// no-op.
    pub async fn cancel_conversation(&self, conversation_id: Uuid) {
        self.cancel_where(|entry| {
            entry.conversation_id == conversation_id || entry.summarizing == Some(conversation_id)
        })
        .await;
    }

    /// Cancels only a background summary of `conversation_id`, never the
    /// chat's own reply, and waits like [`Self::chat_cancel`].
    pub async fn cancel_background(&self, conversation_id: Uuid) {
        self.cancel_where(|entry| entry.background && entry.summarizing == Some(conversation_id))
            .await;
    }

    async fn cancel_where(&self, matches: impl Fn(&InflightEntry) -> bool) {
        let finished: Vec<CancellationToken> = lock(&self.inflight)
            .values()
            .filter(|entry| matches(entry))
            .map(|entry| {
                entry.cancel.cancel();
                entry.finished.clone()
            })
            .collect();
        for finished in finished {
            let _ = tokio::time::timeout(self.limits.cancel_wait, finished.cancelled()).await;
        }
    }

    /// Nothing is admitted or streaming.
    pub fn is_idle(&self) -> bool {
        lock(&self.inflight).is_empty()
    }

    /// Completes once nothing is in flight; at once when nothing is.
    pub async fn wait_idle(&self) {
        let notified = self.inflight.emptied.notified();
        tokio::pin!(notified);
        // Registered before the check, so an entry leaving in between still
        // wakes it.
        notified.as_mut().enable();
        if self.is_idle() {
            return;
        }
        notified.await;
    }

    #[cfg(test)]
    pub(crate) fn inflight_len(&self) -> usize {
        lock(&self.inflight).len()
    }

    /// The current target, or `None` for a rejected URL.
    #[cfg(test)]
    pub(crate) fn current_base_url(&self) -> Option<String> {
        self.connection()
            .base_url
            .as_ref()
            .ok()
            .map(ToString::to_string)
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::net::TcpListener;

    use super::*;
    use crate::audit::AuditEvent;
    use crate::ollama::types::ChatRole;
    use crate::redacted::Redacted;
    use crate::storage::Storage;
    use crate::storage::testing::{break_writes, temp_storage};

    use super::testing::{Reply, fake_ollama};

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

    /// The audit rows of a test's database.
    struct Log {
        storage: Arc<Storage>,
        _dir: tempfile::TempDir,
    }

    impl Log {
        fn new() -> Self {
            let (storage, dir) = temp_storage();
            Self { storage, _dir: dir }
        }

        fn audit(&self) -> Arc<Audit> {
            Arc::new(Audit::over(Arc::clone(&self.storage)))
        }

        /// Newest first.
        fn all(&self) -> Vec<AuditEvent> {
            self.storage.list_audit(usize::MAX, None).unwrap()
        }

        fn last(&self) -> AuditEvent {
            self.all().into_iter().next().expect("no audit rows")
        }
    }

    fn ollama_with_base(
        base: Result<OllamaBaseUrl, AppError>,
        limits: Limits,
    ) -> (Arc<Ollama>, Log) {
        let log = Log::new();
        let ollama = Arc::new(Ollama::with_limits(base, log.audit(), limits));
        (ollama, log)
    }

    fn ollama_at(addr: SocketAddr, limits: Limits) -> (Arc<Ollama>, Log) {
        ollama_with_base(OllamaBaseUrl::parse(&format!("http://{addr}")), limits)
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
        vec![ChatMessage::new(ChatRole::User, Redacted::new("hi"))]
    }

    type Ends = Arc<Mutex<Vec<ChatEnd>>>;

    /// `think` as already resolved for the current connection.
    fn current(ollama: &Ollama, think: Option<Think>) -> ChatThink {
        ChatThink {
            think,
            connection: ollama.connection_id(),
            tools: false,
        }
    }

    /// A call of `messages` with `think`, no tools.
    fn call<'a>(model: &'a str, messages: &'a [ChatMessage], think: ChatThink) -> ChatCall<'a> {
        ChatCall {
            model,
            messages,
            options: None,
            think,
            keep_alive: None,
            tools: false,
        }
    }

    fn no_follow_up(_: &[ChartEntry]) -> Vec<ChatMessage> {
        Vec::new()
    }

    fn send_with_end<F>(
        ollama: &Arc<Ollama>,
        conversation: Uuid,
        sink: F,
        ends: &Ends,
    ) -> Result<Uuid, AppError>
    where
        F: Fn(ChatEvent) -> bool + Send + Sync + 'static,
    {
        send_thinking(ollama, conversation, None, sink, ends)
    }

    fn send_thinking<F>(
        ollama: &Arc<Ollama>,
        conversation: Uuid,
        think: Option<Think>,
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
        ollama.start(
            admission,
            call("llama3.2", &messages, current(ollama, think)),
            sink,
            no_follow_up,
            move |end| {
                ends.lock().unwrap().push(end.clone());
            },
        )
    }

    fn send<F>(ollama: &Arc<Ollama>, conversation: Uuid, sink: F) -> Result<Uuid, AppError>
    where
        F: Fn(ChatEvent) -> bool + Send + Sync + 'static,
    {
        send_with_end(ollama, conversation, sink, &Ends::default())
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
    const DONE: &str = r#"{"message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","total_duration":5000000,"load_duration":300000000,"prompt_eval_count":3,"prompt_eval_duration":1000000000,"eval_count":2,"eval_duration":2000000000}"#;

    #[tokio::test]
    async fn chat_audit_row_has_timings() {
        let fake = fake_ollama(Reply::Lines(vec![HEL, DONE], Duration::ZERO)).await;
        let (ollama, log) = ollama_at(fake.addr, test_limits());

        send(&ollama, Uuid::new_v4(), |_| true).unwrap();
        wait_idle(&ollama).await;

        let [row] = log.all().try_into().unwrap();
        assert_eq!(row.kind, "ollama.chat");
        assert_eq!(row.status, AuditStatus::Ok);
        assert_eq!(
            (row.eval_ms, row.prompt_eval_ms, row.load_ms),
            (Some(2000), Some(1000), Some(300))
        );
    }

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
                    thinking_ms: None,
                    eval_ms: Some(2000),
                },
            ]
        );
    }

    const THINK_A: &str =
        r#"{"message":{"role":"assistant","content":"","thinking":"Let me"},"done":false}"#;
    const THINK_B: &str =
        r#"{"message":{"role":"assistant","content":"","thinking":" see"},"done":false}"#;

    #[tokio::test]
    async fn streams_thinking_then_tokens() {
        let fake = fake_ollama(Reply::Lines(
            vec![THINK_A, THINK_B, HEL, DONE],
            Duration::from_millis(20),
        ))
        .await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let (events, sink) = collector();

        send(&ollama, Uuid::new_v4(), sink).unwrap();
        wait_idle(&ollama).await;

        let events = snapshot(&events);
        assert_eq!(
            events[..3],
            [
                ChatEvent::Thinking {
                    content: Redacted::new("Let me"),
                },
                ChatEvent::Thinking {
                    content: Redacted::new(" see"),
                },
                token("Hel"),
            ]
        );
        let Some(ChatEvent::Done {
            thinking_ms: Some(ms),
            ..
        }) = events.last()
        else {
            panic!("expected done with thinking_ms, got {events:?}");
        };
        // Two 20 ms gaps: the second thought and the first token.
        assert!(*ms >= 30, "{ms}");
    }

    #[tokio::test]
    async fn thinking_counts_toward_content_limit() {
        let fake = fake_ollama(Reply::Endless(
            "{\"message\":{\"content\":\"\",\"thinking\":\"abcdefgh\"},\"done\":false}\n",
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
        let thoughts = snapshot(&events)
            .iter()
            .filter(|event| matches!(event, ChatEvent::Thinking { .. }))
            .count();
        assert_eq!(thoughts, 8, "64 content bytes is eight 8-byte thoughts");
    }

    #[tokio::test]
    async fn chat_request_body_carries_think() {
        let fake = fake_ollama(Reply::Lines(vec![DONE], Duration::ZERO)).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let ends = Ends::default();

        send_thinking(
            &ollama,
            Uuid::new_v4(),
            Some(Think::Enabled(false)),
            |_| true,
            &ends,
        )
        .unwrap();
        wait_idle(&ollama).await;
        send_thinking(&ollama, Uuid::new_v4(), None, |_| true, &ends).unwrap();
        wait_idle(&ollama).await;

        let bodies = fake.bodies.lock().unwrap().clone();
        let [off, absent] = bodies.as_slice() else {
            panic!("expected two requests, got {bodies:?}");
        };
        assert_eq!(off["think"], serde_json::json!(false));
        assert!(absent.get("think").is_none());
        for body in &bodies {
            assert!(body.get("tools").is_none());
            assert!(body.get("format").is_none());
        }
    }

    const SHOW_QWEN3: &str = r#"{"license":"Apache","template":"{{ .Prompt }}","details":{"family":"qwen3"},"capabilities":["completion","tools","thinking"],"thinking":{"values":[true,false],"default":true}}"#;

    #[tokio::test]
    async fn model_capabilities_audited_once_and_cached() {
        let fake = fake_ollama(Reply::Json(SHOW_QWEN3)).await;
        let (ollama, log) = ollama_at(fake.addr, test_limits());

        let caps = ollama.model_capabilities("qwen3:8b").await.unwrap();
        assert!(caps.thinking);
        assert_eq!(caps.think_style, types::ThinkStyle::Bool);
        let again = ollama.model_capabilities("qwen3:8b").await.unwrap();
        assert_eq!(again, caps);

        let events = log.all();
        let [event] = events.as_slice() else {
            panic!("expected one audit event, got {events:?}");
        };
        assert_eq!(event.kind, "ollama.show");
        assert_eq!(event.endpoint.as_deref(), Some("/api/show"));
        assert_eq!(event.model.as_deref(), Some("qwen3:8b"));
        assert_eq!(event.status, AuditStatus::Ok);
        assert_eq!(fake.accepted.load(Ordering::SeqCst), 1);
        assert_eq!(
            fake.bodies.lock().unwrap().as_slice(),
            [serde_json::json!({ "model": "qwen3:8b" })]
        );

        // A new base URL, even the same one, forgets what was learned.
        ollama.set_base_url(OllamaBaseUrl::parse(&format!("http://{}", fake.addr)));
        ollama.model_capabilities("qwen3:8b").await.unwrap();
        assert_eq!(log.all().len(), 2);
    }

    #[tokio::test]
    async fn chat_think_is_bound_to_its_connection() {
        let first = fake_ollama(Reply::Json(SHOW_QWEN3)).await;
        let (ollama, log) = ollama_at(first.addr, test_limits());

        let think = ollama.chat_think("qwen3:8b", None).await.unwrap();
        assert_eq!(think.think, Some(Think::Enabled(false)));
        assert_eq!(ollama.check_connection(think.connection), Ok(()));

        // Settings switch the URL after the lookup, before the chat starts.
        let second = fake_ollama(Reply::Lines(vec![DONE], Duration::ZERO)).await;
        ollama.set_base_url(OllamaBaseUrl::parse(&format!("http://{}", second.addr)));
        assert!(matches!(
            ollama.check_connection(think.connection),
            Err(AppError::Ollama(_))
        ));
        let admission = ollama.admit(Uuid::new_v4()).unwrap();
        let ends = Ends::default();
        let recorded = Arc::clone(&ends);
        let messages = prompt();
        let err = ollama
            .start(
                admission,
                call("qwen3:8b", &messages, think),
                |_| true,
                no_follow_up,
                move |end| recorded.lock().unwrap().push(end.clone()),
            )
            .unwrap_err();

        assert!(matches!(err, AppError::Ollama(_)));
        assert!(matches!(
            ends.lock().unwrap().as_slice(),
            [ChatEnd::Failed(AppError::Ollama(_))]
        ));
        assert_eq!(ollama.inflight_len(), 0);
        assert_eq!(second.accepted.load(Ordering::SeqCst), 0);
        // Only the lookup was audited; the refused chat wrote nothing.
        assert_eq!(log.all().len(), 1);
    }

    #[tokio::test]
    async fn chat_think_fails_when_lookup_fails() {
        let fake = fake_ollama(Reply::Status(500, r#"{"error":"boom"}"#)).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        // Never a chat without `think` to a model that might think.
        for requested in [
            None,
            Some(Think::Enabled(false)),
            Some(Think::Enabled(true)),
        ] {
            assert!(matches!(
                ollama.chat_think("qwen3", requested).await,
                Err(AppError::Ollama(_))
            ));
        }
    }

    #[tokio::test]
    async fn model_capabilities_failure_is_not_cached() {
        let fake = fake_ollama(Reply::Status(404, r#"{"error":"model not found"}"#)).await;
        let (ollama, log) = ollama_at(fake.addr, test_limits());

        for _ in 0..2 {
            assert_eq!(
                ollama.model_capabilities("missing").await,
                Err(AppError::Ollama("HTTP 404: model not found".into()))
            );
        }
        assert_eq!(log.all().len(), 2);
        assert_eq!(log.last().status, AuditStatus::Error);
        assert!(matches!(
            ollama.model_capabilities("bad\nname").await,
            Err(AppError::InvalidInput(_))
        ));
        assert_eq!(log.all().len(), 2);
    }

    #[tokio::test]
    async fn invalid_base_url_rejected_without_socket() {
        let fake = fake_ollama(Reply::Lines(vec![DONE], Duration::ZERO)).await;
        let base = OllamaBaseUrl::parse(&format!("http://10.0.0.1:{}", fake.addr.port()));
        let (ollama, log) = ollama_with_base(base, test_limits());
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
        assert!(log.all().is_empty());
        assert_eq!(fake.accepted.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn cancel_before_first_chunk_frees_slot() {
        let fake = fake_ollama(Reply::Silent).await;
        let (ollama, log) = ollama_at(fake.addr, test_limits());
        let (events, sink) = collector();

        let id = send(&ollama, Uuid::new_v4(), sink).unwrap();
        ollama.chat_cancel(id).await.unwrap();

        assert_eq!(ollama.inflight_len(), 0);
        assert!(snapshot(&events).is_empty());
        assert_eq!(log.last().status, AuditStatus::Cancelled);
    }

    #[tokio::test]
    async fn attempt_row_written_before_reply() {
        let fake = fake_ollama(Reply::Silent).await;
        let (ollama, log) = ollama_at(fake.addr, test_limits());

        let id = send(&ollama, Uuid::new_v4(), |_| true).unwrap();
        // `start` returned, so the row exists whether or not the worker has
        // connected yet.
        let row = log.last();
        assert_eq!(row.kind, "ollama.chat");
        assert_eq!(row.status, AuditStatus::Attempt);
        assert_eq!(row.host, Some(fake.addr.to_string()));
        assert_eq!(row.endpoint.as_deref(), Some("/api/chat"));
        assert_eq!(row.model.as_deref(), Some("llama3.2"));
        assert_eq!(row.duration_ms, None);

        ollama.chat_cancel(id).await.unwrap();
        let row = log.last();
        assert_eq!(row.status, AuditStatus::Cancelled);
        assert!(row.duration_ms.is_some());
        assert_eq!(log.all().len(), 1);
    }

    #[tokio::test]
    async fn audit_write_failure_refuses_request() {
        let fake = fake_ollama(Reply::Json(r#"{"version":"0.12.3"}"#)).await;
        let (ollama, log) = ollama_at(fake.addr, test_limits());
        let ends = Ends::default();
        break_writes(&log.storage);

        let err = send_with_end(&ollama, Uuid::new_v4(), |_| true, &ends).unwrap_err();
        assert!(matches!(err, AppError::Database(_)));
        assert!(matches!(
            ends.lock().unwrap().as_slice(),
            [ChatEnd::Failed(AppError::Database(_))]
        ));
        assert!(matches!(ollama.status().await, Err(AppError::Database(_))));
        assert!(matches!(
            ollama.list_models().await,
            Err(AppError::Database(_))
        ));

        assert_eq!(ollama.inflight_len(), 0);
        assert_eq!(fake.accepted.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn set_base_url_switches_target() {
        let first = fake_ollama(Reply::Json(r#"{"version":"1"}"#)).await;
        let second = fake_ollama(Reply::Json(r#"{"version":"2"}"#)).await;
        let (ollama, log) = ollama_at(first.addr, test_limits());

        assert_eq!(ollama.status().await.unwrap().version.as_deref(), Some("1"));
        ollama.set_base_url(OllamaBaseUrl::parse(&format!("http://{}", second.addr)));
        let status = ollama.status().await.unwrap();
        assert_eq!(status.version.as_deref(), Some("2"));
        assert_eq!(status.base_url, format!("http://{}", second.addr));

        let mut hosts = log.storage.audit_hosts().unwrap();
        hosts.sort();
        let mut expected = vec![first.addr.to_string(), second.addr.to_string()];
        expected.sort();
        assert_eq!(hosts, expected);

        ollama.set_base_url(OllamaBaseUrl::parse("http://10.0.0.1:11434"));
        assert!(matches!(
            ollama.status().await,
            Err(AppError::InvalidBaseUrl(_))
        ));
        assert_eq!(first.accepted.load(Ordering::SeqCst), 1);
        assert_eq!(second.accepted.load(Ordering::SeqCst), 1);
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
        let (ollama, log) = ollama_at(fake.addr, test_limits());
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);

        send(&ollama, Uuid::new_v4(), move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            false
        })
        .unwrap();
        wait_idle(&ollama).await;

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(log.last().status, AuditStatus::Cancelled);
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
    async fn background_admission_needs_idle() {
        let fake = fake_ollama(Reply::Silent).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let (project, chat) = (Uuid::new_v4(), Uuid::new_v4());

        let id = send(&ollama, Uuid::new_v4(), |_| true).unwrap();
        assert!(matches!(
            ollama.admit_background(project, chat),
            Err(AppError::Busy(_))
        ));
        ollama.chat_cancel(id).await.unwrap();

        let admission = ollama.admit_background(project, chat).unwrap();
        // Only one background summary at a time.
        assert!(matches!(
            ollama.admit_background(Uuid::new_v4(), Uuid::new_v4()),
            Err(AppError::Busy(_))
        ));
        drop(admission);
        assert!(ollama.is_idle());
    }

    #[tokio::test]
    async fn user_admission_cancels_background() {
        let fake = fake_ollama(Reply::Silent).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let project = Uuid::new_v4();
        let background = ollama.admit_background(project, Uuid::new_v4()).unwrap();

        // Another chat, and a manual refresh of the same project, both go
        // first.
        let chat = ollama.admit(Uuid::new_v4()).unwrap();
        assert!(background.is_cancelled());
        let manual = ollama.admit_summary(project, Uuid::new_v4()).unwrap();
        assert!(!manual.is_cancelled());
        assert!(!chat.is_cancelled());

        // A winding-down background summary does not count toward the cap.
        let more: Vec<Admission> = (0..2)
            .map(|_| ollama.admit(Uuid::new_v4()).unwrap())
            .collect();
        assert_eq!(ollama.inflight_len(), 5);
        assert!(matches!(
            ollama.admit(Uuid::new_v4()),
            Err(AppError::Busy(_))
        ));
        drop(more);
    }

    #[tokio::test]
    async fn cancel_background_leaves_the_chat_reply() {
        let fake = fake_ollama(Reply::Silent).await;
        let limits = Limits {
            cancel_wait: Duration::from_millis(50),
            ..test_limits()
        };
        let (ollama, _) = ollama_at(fake.addr, limits);
        let chat = Uuid::new_v4();
        let background = ollama.admit_background(Uuid::new_v4(), chat).unwrap();
        // The chat's own turn, admitted while the summary winds down.
        let turn = ollama.admit(chat).unwrap();

        ollama.cancel_background(chat).await;

        assert!(background.is_cancelled());
        assert!(!turn.is_cancelled());
    }

    #[tokio::test]
    async fn wait_idle_wakes_when_emptied() {
        let fake = fake_ollama(Reply::Silent).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        ollama.wait_idle().await;

        let first = ollama.admit(Uuid::new_v4()).unwrap();
        let second = ollama.admit(Uuid::new_v4()).unwrap();
        let waiting = tokio::spawn({
            let ollama = Arc::clone(&ollama);
            async move { ollama.wait_idle().await }
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(first);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiting.is_finished());

        drop(second);
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn entry_removed_after_http_error() {
        let fake = fake_ollama(Reply::Status(404, r#"{"error":"model not found"}"#)).await;
        let (ollama, log) = ollama_at(fake.addr, test_limits());
        let (events, sink) = collector();

        send(&ollama, Uuid::new_v4(), sink).unwrap();
        wait_idle(&ollama).await;

        assert_eq!(
            last_error(&events),
            AppError::Ollama("HTTP 404: model not found".into())
        );
        let event = log.last();
        assert_eq!(event.status, AuditStatus::Error);
        assert_eq!(event.detail.as_deref(), Some("ollama, HTTP 404"));
        // The error text from Ollama is not copied into the audit row.
        assert_eq!(log.all().len(), 1);
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
            "{\"message\":{\"content\":\"\",\"thinking\":\"\"},\"done\":false}\n",
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
                thinking_ms: None,
                eval_ms: Some(2000),
                prompt_eval_ms: Some(1000),
                load_ms: Some(300),
                ..
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
        let (ollama, log) = ollama_at(fake.addr, test_limits());
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
        let messages = prompt();
        ollama
            .start(
                admission,
                call("m", &messages, current(&ollama, None)),
                sink,
                no_follow_up,
                move |end| {
                    recorded.lock().unwrap().push(end.clone());
                },
            )
            .unwrap();
        canceller.await.unwrap();

        assert_eq!(ollama.inflight_len(), 0);
        assert!(snapshot(&events).is_empty());
        assert!(matches!(
            ends.lock().unwrap().as_slice(),
            [ChatEnd::Cancelled]
        ));
        assert_eq!(log.last().status, AuditStatus::Cancelled);
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
        let (ollama, log) = ollama_at(fake.addr, test_limits());

        send(&ollama, Uuid::new_v4(), |_| true).unwrap();
        wait_idle(&ollama).await;

        let events = log.all();
        let [event] = events.as_slice() else {
            panic!("expected one audit event, got {events:?}");
        };
        assert_eq!(event.kind, "ollama.chat");
        assert_eq!(event.host, Some(fake.addr.to_string()));
        assert_eq!(event.status, AuditStatus::Ok);
        assert_eq!(event.detail, None);
        assert_eq!(event.prompt_tokens, Some(3));
        assert_eq!(event.completion_tokens, Some(2));
        assert!(event.duration_ms.is_some());
    }

    const BEFORE: &str = r#"{"message":{"role":"assistant","content":"Here:"},"done":false}"#;
    const CALL: &str = r#"{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"render_chart","arguments":{"title":"Pop","type":"bar","labels":["1950","1960"],"values":[4.0,4.4]}}}]},"done":false}"#;
    const CALL_BAD: &str = r#"{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"render_chart","arguments":{"title":"Pop","type":"bar","labels":["1950"],"values":[4.0,4.4]}}}]},"done":false}"#;
    const CALL_OTHER: &str = r#"{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"run_shell","arguments":{"cmd":"ls"}}}]},"done":false}"#;
    /// One NDJSON line.
    const CALL_FOUR: &str = concat!(
        r#"{"message":{"role":"assistant","content":"","tool_calls":["#,
        r#"{"function":{"name":"render_chart","arguments":{"title":"1","type":"bar","labels":["a"],"values":[1]}}},"#,
        r#"{"function":{"name":"render_chart","arguments":{"title":"2","type":"bar","labels":["a"],"values":[1]}}},"#,
        r#"{"function":{"name":"render_chart","arguments":{"title":"3","type":"bar","labels":["a"],"values":[1]}}},"#,
        r#"{"function":{"name":"render_chart","arguments":{"title":"4","type":"bar","labels":["a"],"values":[1]}}}"#,
        r#"]},"done":false}"#
    );
    const AFTER: &str = r#"{"message":{"role":"assistant","content":" Grew."},"done":false}"#;
    const AFTER_DONE: &str = r#"{"message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","total_duration":7000000,"prompt_eval_count":10,"eval_count":4,"eval_duration":1000000000}"#;

    /// Answers a request offering tools with `first`, any other with
    /// `follow`.
    async fn charted(first: Vec<&'static str>, follow: Reply) -> testing::Fake {
        fake_ollama(Reply::Route(Arc::new(move |_, body| {
            if body.get("tools").is_some() {
                Reply::Lines(first.clone(), Duration::ZERO)
            } else {
                follow.clone()
            }
        })))
        .await
    }

    fn commentary() -> Reply {
        Reply::Lines(vec![AFTER, AFTER_DONE], Duration::ZERO)
    }

    /// Sends with `render_chart` offered; the follow-up answers "Here:".
    fn send_charted<F>(
        ollama: &Arc<Ollama>,
        tools: bool,
        sink: F,
        ends: &Ends,
    ) -> Result<Uuid, AppError>
    where
        F: Fn(ChatEvent) -> bool + Send + Sync + 'static,
    {
        let messages = prompt();
        let admission = ollama.admit(Uuid::new_v4())?;
        let ends = Arc::clone(ends);
        ollama.start(
            admission,
            ChatCall {
                tools,
                ..call(
                    "qwen3",
                    &messages,
                    current(ollama, Some(Think::Enabled(false))),
                )
            },
            sink,
            |charts: &[ChartEntry]| chart_tool::follow_up_history(prompt(), "Here:", charts),
            move |end| ends.lock().unwrap().push(end.clone()),
        )
    }

    fn chart_events(events: &Events) -> Vec<ChatEvent> {
        snapshot(events)
            .into_iter()
            .filter(|event| {
                matches!(
                    event,
                    ChatEvent::Chart { .. } | ChatEvent::ChartError { .. }
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn tool_call_emits_chart_then_follow_up() {
        let fake = charted(vec![BEFORE, CALL, DONE], commentary()).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let (events, sink) = collector();
        let ends = Ends::default();

        send_charted(&ollama, true, sink, &ends).unwrap();
        wait_idle(&ollama).await;

        let events = snapshot(&events);
        assert_eq!(events.len(), 4, "{events:?}");
        assert_eq!(events[0], token("Here:"));
        let ChatEvent::Chart { offset, spec } = &events[1] else {
            panic!("expected a chart, got {:?}", events[1]);
        };
        assert_eq!(*offset, 5);
        assert_eq!(spec.labels, ["1950", "1960"]);
        assert_eq!(events[2], token(" Grew."));
        // One `Done`, with both requests' counts.
        assert_eq!(
            events[3],
            ChatEvent::Done {
                prompt_tokens: Some(13),
                completion_tokens: Some(6),
                duration_ms: Some(12),
                done_reason: Some("stop".into()),
                thinking_ms: None,
                eval_ms: Some(3000),
            }
        );
        let ends = ends.lock().unwrap();
        let [
            ChatEnd::Done {
                completion_tokens: Some(6),
                charts,
                ..
            },
        ] = ends.as_slice()
        else {
            panic!("expected one summed end, got {ends:?}");
        };
        assert_eq!(charts.len(), 1);
    }

    #[tokio::test]
    async fn follow_up_has_no_tools_and_is_audited_separately() {
        let fake = charted(vec![BEFORE, CALL, DONE], commentary()).await;
        let (ollama, log) = ollama_at(fake.addr, test_limits());

        send_charted(&ollama, true, |_| true, &Ends::default()).unwrap();
        wait_idle(&ollama).await;

        let bodies = fake.bodies.lock().unwrap().clone();
        let [first, second] = bodies.as_slice() else {
            panic!("expected two requests, got {bodies:?}");
        };
        assert_eq!(first["tools"][0]["function"]["name"], "render_chart");
        assert!(second.get("tools").is_none());
        for body in &bodies {
            assert!(body.get("format").is_none());
            assert_eq!(body["think"], serde_json::json!(false));
        }
        let messages = second["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["content"], "Here:");
        assert_eq!(
            messages[1]["tool_calls"][0]["function"]["arguments"]["values"],
            serde_json::json!([4.0, 4.4])
        );
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_name"], "render_chart");
        assert_eq!(messages[2]["content"], "The chart was shown to the user.");

        let rows: Vec<(String, AuditStatus)> = log
            .all()
            .into_iter()
            .map(|event| (event.kind, event.status))
            .collect();
        assert_eq!(
            rows,
            [
                ("ollama.chat".to_owned(), AuditStatus::Ok),
                ("ollama.chat".to_owned(), AuditStatus::Ok)
            ]
        );
    }

    #[tokio::test]
    async fn invalid_tool_call_sends_error_result() {
        for (line, reason) in [
            (CALL_BAD, "labels and values differ in length"),
            (CALL_OTHER, "only render_chart can be called"),
        ] {
            let fake = charted(vec![line, DONE], commentary()).await;
            let (ollama, _) = ollama_at(fake.addr, test_limits());
            let (events, sink) = collector();

            send_charted(&ollama, true, sink, &Ends::default()).unwrap();
            wait_idle(&ollama).await;

            assert_eq!(
                chart_events(&events),
                [ChatEvent::ChartError {
                    offset: 0,
                    reason: reason.into()
                }]
            );
            let bodies = fake.bodies.lock().unwrap().clone();
            assert_eq!(bodies.len(), 2);
            assert_eq!(
                bodies[1]["messages"][2]["content"],
                format!("The chart could not be shown: {reason}")
            );
            assert!(matches!(
                snapshot(&events).last(),
                Some(ChatEvent::Done { .. })
            ));
        }
    }

    #[tokio::test]
    async fn more_than_three_calls_ignored() {
        let fake = charted(vec![CALL_FOUR, DONE], commentary()).await;
        let limits = Limits {
            max_line_bytes: 4096,
            ..test_limits()
        };
        let (ollama, _) = ollama_at(fake.addr, limits);
        let (events, sink) = collector();

        send_charted(&ollama, true, sink, &Ends::default()).unwrap();
        wait_idle(&ollama).await;

        let titles: Vec<String> = chart_events(&events)
            .into_iter()
            .filter_map(|event| match event {
                ChatEvent::Chart { spec, .. } => Some(spec.title),
                _ => None,
            })
            .collect();
        assert_eq!(titles, ["1", "2", "3"]);
        let bodies = fake.bodies.lock().unwrap().clone();
        let tools = bodies[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "tool")
            .count();
        assert_eq!(tools, 3);
    }

    #[tokio::test]
    async fn cancel_during_follow_up() {
        let fake = charted(vec![CALL, DONE], Reply::Silent).await;
        let (ollama, log) = ollama_at(fake.addr, test_limits());
        let (events, sink) = collector();
        let ends = Ends::default();

        let id = send_charted(&ollama, true, sink, &ends).unwrap();
        for _ in 0..500 {
            if fake.bodies.lock().unwrap().len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        ollama.chat_cancel(id).await.unwrap();

        assert_eq!(ollama.inflight_len(), 0);
        assert!(matches!(
            ends.lock().unwrap().as_slice(),
            [ChatEnd::Cancelled]
        ));
        assert_eq!(chart_events(&events).len(), 1);
        assert!(
            !snapshot(&events)
                .iter()
                .any(|event| matches!(event, ChatEvent::Done { .. } | ChatEvent::Error { .. }))
        );
        assert_eq!(log.last().status, AuditStatus::Cancelled);
    }

    #[tokio::test]
    async fn tool_calls_ignored_when_not_offered() {
        let fake = fake_ollama(Reply::Lines(vec![BEFORE, CALL, DONE], Duration::ZERO)).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let (events, sink) = collector();

        send_charted(&ollama, false, sink, &Ends::default()).unwrap();
        wait_idle(&ollama).await;

        assert!(chart_events(&events).is_empty());
        assert!(matches!(
            snapshot(&events).last(),
            Some(ChatEvent::Done { .. })
        ));
        let bodies = fake.bodies.lock().unwrap().clone();
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].get("tools").is_none());
    }

    #[tokio::test]
    async fn follow_up_over_limit_ends_in_error() {
        let fake = charted(vec![CALL, DONE], commentary()).await;
        let (ollama, _) = ollama_at(fake.addr, test_limits());
        let (events, sink) = collector();
        let ends = Ends::default();
        let recorded = Arc::clone(&ends);
        let messages = prompt();

        ollama
            .start(
                ollama.admit(Uuid::new_v4()).unwrap(),
                ChatCall {
                    tools: true,
                    ..call("qwen3", &messages, current(&ollama, None))
                },
                sink,
                // Over `max_messages`.
                |charts: &[ChartEntry]| {
                    chart_tool::follow_up_history(vec![prompt()[0].clone(); 10], "", charts)
                },
                move |end| recorded.lock().unwrap().push(end.clone()),
            )
            .unwrap();
        wait_idle(&ollama).await;

        assert!(matches!(last_error(&events), AppError::InvalidInput(_)));
        assert_eq!(chart_events(&events).len(), 1);
        assert!(matches!(
            ends.lock().unwrap().as_slice(),
            [ChatEnd::Failed(AppError::InvalidInput(_))]
        ));
        assert_eq!(fake.bodies.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn status_unreachable_when_nothing_listens() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let (ollama, log) = ollama_at(addr, test_limits());

        let status = ollama.status().await.unwrap();
        assert!(!status.reachable);
        assert_eq!(status.version, None);
        assert_eq!(status.base_url, format!("http://{addr}"));
        // Windows takes about 2 s to refuse a loopback connect, so on a busy
        // machine the 3 s request timeout can win; both mean "not running".
        let event = log.last();
        assert_eq!(event.kind, "ollama.version");
        assert_eq!(event.status, AuditStatus::Error);
        let detail = event.detail.unwrap_or_default();
        assert!(detail == "unreachable" || detail == "timeout", "{detail}");
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
        let (ollama, log) = ollama_at(fake.addr, test_limits());

        let models = ollama.list_models().await.unwrap();
        assert_eq!(
            models,
            vec![ModelInfo {
                name: "llama3.2:latest".into(),
                size_bytes: Some(2_019_393_189),
                parameter_size: Some("3.2B".into()),
            }]
        );
        let event = log.last();
        assert_eq!(event.kind, "ollama.tags");
        assert_eq!(event.endpoint.as_deref(), Some("/api/tags"));
        assert_eq!(event.status, AuditStatus::Ok);
    }
}

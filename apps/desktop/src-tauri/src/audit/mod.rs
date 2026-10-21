//! Audit trail of outbound requests and app lifecycle events.
//!
//! Every outbound request gets an `attempt` row before its socket opens,
//! updated with the outcome when it ends, so a crash leaves a visible
//! `attempt`. Events wait in a [`MemorySink`] until the encrypted database
//! opens; [`Audit::attach`] then moves them into a [`DbSink`].

pub mod db;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use self::db::DbSink;
use crate::error::AppError;
use crate::storage::Storage;

/// Rows kept in `audit_events`; older ones are pruned when the database is
/// attached.
pub const KEEP_EVENTS: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditStatus {
    /// Started, no outcome yet. Left as is when the app dies mid-request.
    Attempt,
    Ok,
    Error,
    Cancelled,
}

impl AuditStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Attempt => "attempt",
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "attempt" => Some(Self::Attempt),
            "ok" => Some(Self::Ok),
            "error" => Some(Self::Error),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// A stored event, as `activity_list` returns it; mirrored by `AuditEvent` in
/// `src/lib/types.ts`. Metadata only, never message content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    pub id: i64,
    /// ISO-8601 UTC with milliseconds.
    pub at: String,
    /// `ollama.chat`, `ollama.tags`, `ollama.version`, `app.start`, `db.open`.
    pub kind: String,
    /// `host:port` for requests; absent for lifecycle events.
    pub host: Option<String>,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub status: AuditStatus,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub duration_ms: Option<u64>,
    /// The error kind (plus `HTTP <code>`) of a failed request, or the
    /// storage state of a failed `db.open`.
    pub detail: Option<String>,
}

/// How an event ended, or `attempt` while it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditOutcome {
    pub status: AuditStatus,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub duration_ms: Option<u64>,
    pub detail: Option<String>,
}

impl AuditOutcome {
    pub fn new(status: AuditStatus) -> Self {
        Self {
            status,
            prompt_tokens: None,
            completion_tokens: None,
            duration_ms: None,
            detail: None,
        }
    }

    /// `error` with the error kind, plus the HTTP status when a non-2xx one
    /// arrived.
    pub fn failed(error: &AppError, http_status: Option<u16>) -> Self {
        let detail = match http_status {
            Some(code) if !(200..300).contains(&code) => format!("{}, HTTP {code}", error.kind()),
            _ => error.kind().to_owned(),
        };
        Self {
            detail: Some(detail),
            ..Self::new(AuditStatus::Error)
        }
    }
}

/// An event to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAuditEvent {
    /// Unix time in milliseconds.
    pub at_ms: u64,
    pub kind: String,
    pub host: Option<String>,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub outcome: AuditOutcome,
}

impl NewAuditEvent {
    /// An outbound request about to open its socket.
    pub fn request(kind: &str, host: String, endpoint: &str, model: Option<&str>) -> Self {
        Self {
            at_ms: now_ms(),
            kind: kind.to_owned(),
            host: Some(host),
            endpoint: Some(endpoint.to_owned()),
            model: model.map(str::to_owned),
            outcome: AuditOutcome::new(AuditStatus::Attempt),
        }
    }

    /// Something the app did itself, finished at once.
    pub fn lifecycle(kind: &str, status: AuditStatus, detail: Option<String>) -> Self {
        Self {
            at_ms: now_ms(),
            kind: kind.to_owned(),
            host: None,
            endpoint: None,
            model: None,
            outcome: AuditOutcome {
                detail,
                ..AuditOutcome::new(status)
            },
        }
    }
}

/// Current Unix time in milliseconds; 0 if the clock is before 1970.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

pub trait AuditSink: Send + Sync {
    /// Stores `event` and returns its id.
    fn insert(&self, event: &NewAuditEvent) -> Result<i64, AppError>;

    /// Writes the outcome of an `attempt`. An unknown or finished id is left
    /// alone.
    fn finish(&self, id: i64, outcome: &AuditOutcome) -> Result<(), AppError>;
}

#[derive(Debug, Default)]
struct MemoryEvents {
    next_id: i64,
    events: VecDeque<(i64, NewAuditEvent)>,
}

/// Ring buffer of the last [`MemorySink::CAPACITY`] events.
#[derive(Debug, Default)]
pub struct MemorySink {
    events: Mutex<MemoryEvents>,
}

impl MemorySink {
    pub const CAPACITY: usize = 500;

    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, MemoryEvents> {
        // A panic while holding the lock cannot leave the deque half-updated,
        // so a poisoned lock is still safe to use.
        self.events.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes every buffered event with its id, oldest first.
    pub fn drain(&self) -> Vec<(i64, NewAuditEvent)> {
        self.lock().events.drain(..).collect()
    }
}

impl AuditSink for MemorySink {
    fn insert(&self, event: &NewAuditEvent) -> Result<i64, AppError> {
        let mut memory = self.lock();
        memory.next_id += 1;
        let id = memory.next_id;
        if memory.events.len() == Self::CAPACITY {
            memory.events.pop_front();
        }
        memory.events.push_back((id, event.clone()));
        Ok(id)
    }

    fn finish(&self, id: i64, outcome: &AuditOutcome) -> Result<(), AppError> {
        let mut memory = self.lock();
        if let Some((_, event)) = memory.events.iter_mut().find(|(event_id, event)| {
            *event_id == id && event.outcome.status == AuditStatus::Attempt
        }) {
            event.outcome = outcome.clone();
        }
        Ok(())
    }
}

/// Identifies an event for [`Audit::finish`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditId(Slot);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Memory(i64),
    Db(i64),
}

enum Backend {
    /// Setup, before the database opened. Nothing outbound runs yet.
    Buffering(MemorySink),
    Db {
        sink: DbSink,
        /// Buffered memory ids to their rows, for a late `finish`.
        remap: HashMap<i64, i64>,
    },
    /// No durable log: storage did not open, or the buffer could not be
    /// written. Every request is refused.
    Unavailable,
}

/// The app's audit log, shared by everything that makes requests.
pub struct Audit {
    backend: Mutex<Backend>,
}

impl Default for Audit {
    fn default() -> Self {
        Self::new()
    }
}

impl Audit {
    pub fn new() -> Self {
        Self {
            backend: Mutex::new(Backend::Buffering(MemorySink::new())),
        }
    }

    /// A log already writing to `storage`.
    pub fn over(storage: Arc<Storage>) -> Self {
        Self {
            backend: Mutex::new(Backend::Db {
                sink: DbSink::new(storage),
                remap: HashMap::new(),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Backend> {
        // Each critical section swaps the backend in one assignment or makes
        // one sink call, so a poisoned lock is still consistent.
        self.backend.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stores `event`. Refuses when there is no durable log, so a request
    /// never goes out without its row.
    pub fn begin(&self, event: NewAuditEvent) -> Result<AuditId, AppError> {
        match &*self.lock() {
            Backend::Buffering(memory) => memory.insert(&event).map(|id| AuditId(Slot::Memory(id))),
            Backend::Db { sink, .. } => sink.insert(&event).map(|id| AuditId(Slot::Db(id))),
            Backend::Unavailable => {
                Err(AppError::StorageUnavailable("audit log unavailable".into()))
            }
        }
    }

    /// Writes the outcome of [`Self::begin`]'s event. A failure is logged,
    /// never returned: the request already happened.
    pub fn finish(&self, id: AuditId, outcome: &AuditOutcome) {
        let result = match (&*self.lock(), id.0) {
            (Backend::Buffering(memory), Slot::Memory(id)) => memory.finish(id, outcome),
            (Backend::Db { sink, remap }, Slot::Memory(id)) => match remap.get(&id) {
                Some(&row) => sink.finish(row, outcome),
                None => Ok(()),
            },
            (Backend::Db { sink, .. }, Slot::Db(id)) => sink.finish(id, outcome),
            _ => Ok(()),
        };
        if let Err(error) = result {
            tracing::warn!(kind = error.kind(), "could not save an audit outcome");
        }
    }

    /// A finished event, such as `app.start`. A failure is logged.
    pub fn record(&self, event: NewAuditEvent) {
        if let Err(error) = self.begin(event) {
            tracing::warn!(kind = error.kind(), "could not save an audit event");
        }
    }

    /// Writes the buffered events to `storage` in one transaction and keeps
    /// writing there. On failure nothing is written and the log becomes
    /// unavailable. A log that is not buffering is left as it is.
    pub fn attach(&self, storage: Arc<Storage>) -> Result<(), AppError> {
        let mut backend = self.lock();
        let Backend::Buffering(memory) = &*backend else {
            return Ok(());
        };
        let buffered = memory.drain();
        let events: Vec<&NewAuditEvent> = buffered.iter().map(|(_, event)| event).collect();
        let rows = match storage.insert_audit_batch(&events) {
            Ok(rows) => rows,
            Err(error) => {
                *backend = Backend::Unavailable;
                return Err(error);
            }
        };
        if let Err(error) = storage.prune_audit(KEEP_EVENTS) {
            tracing::warn!(kind = error.kind(), "could not prune the audit log");
        }
        let remap = buffered.iter().map(|(id, _)| *id).zip(rows).collect();
        *backend = Backend::Db {
            sink: DbSink::new(storage),
            remap,
        };
        Ok(())
    }

    /// No durable log will come (storage did not open): refuse requests from
    /// now on.
    pub fn close(&self) {
        *self.lock() = Backend::Unavailable;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::testing::{break_writes, temp_storage};

    fn lifecycle(kind: &str) -> NewAuditEvent {
        NewAuditEvent::lifecycle(kind, AuditStatus::Ok, None)
    }

    fn request() -> NewAuditEvent {
        NewAuditEvent::request("ollama.tags", "127.0.0.1:11434".into(), "/api/tags", None)
    }

    #[test]
    fn memory_sink_drops_oldest_beyond_capacity() {
        let sink = MemorySink::new();
        let total = MemorySink::CAPACITY + 1;
        for _ in 0..total {
            sink.insert(&lifecycle("test")).unwrap();
        }

        let all = sink.drain();
        assert_eq!(all.len(), MemorySink::CAPACITY);
        assert_eq!(all.first().map(|(id, _)| *id), Some(2));
        assert_eq!(all.last().map(|(id, _)| *id), Some(total as i64));
    }

    #[test]
    fn memory_sink_finish_updates_in_place() {
        let sink = MemorySink::new();
        let id = sink.insert(&request()).unwrap();
        sink.finish(id, &AuditOutcome::new(AuditStatus::Ok))
            .unwrap();
        // A finished event is never rewritten.
        sink.finish(id, &AuditOutcome::new(AuditStatus::Cancelled))
            .unwrap();

        let [(_, event)] = sink.drain().try_into().unwrap();
        assert_eq!(event.outcome.status, AuditStatus::Ok);
    }

    #[test]
    fn attach_flushes_buffered_events_in_order() {
        let (storage, _dir) = temp_storage();
        let audit = Audit::new();
        audit.record(lifecycle("app.start"));
        audit.record(lifecycle("db.open"));

        audit.attach(Arc::clone(&storage)).unwrap();
        audit.record(lifecycle("later"));

        let kinds: Vec<String> = storage
            .list_audit(10, None)
            .unwrap()
            .into_iter()
            .map(|event| event.kind)
            .collect();
        assert_eq!(kinds, ["later", "db.open", "app.start"]);
    }

    #[test]
    fn finish_after_attach_updates_db_row() {
        let (storage, _dir) = temp_storage();
        let audit = Audit::new();
        let id = audit.begin(request()).unwrap();

        audit.attach(Arc::clone(&storage)).unwrap();
        assert_eq!(
            storage.list_audit(1, None).unwrap()[0].status,
            AuditStatus::Attempt
        );
        audit.finish(id, &AuditOutcome::new(AuditStatus::Ok));

        assert_eq!(
            storage.list_audit(1, None).unwrap()[0].status,
            AuditStatus::Ok
        );
    }

    #[test]
    fn finished_row_is_not_rewritten() {
        let (storage, _dir) = temp_storage();
        let audit = Audit::over(Arc::clone(&storage));
        let id = audit.begin(request()).unwrap();

        audit.finish(id, &AuditOutcome::new(AuditStatus::Cancelled));
        audit.finish(id, &AuditOutcome::new(AuditStatus::Ok));

        assert_eq!(
            storage.list_audit(1, None).unwrap()[0].status,
            AuditStatus::Cancelled
        );
    }

    #[test]
    fn failed_attach_commits_nothing_and_refuses_begin() {
        let (storage, _dir) = temp_storage();
        let audit = Audit::new();
        audit.record(lifecycle("app.start"));
        audit.record(lifecycle("db.open"));
        break_writes(&storage);

        assert!(matches!(
            audit.attach(Arc::clone(&storage)),
            Err(AppError::Database(_))
        ));
        assert!(storage.list_audit(10, None).unwrap().is_empty());
        assert!(matches!(
            audit.begin(request()),
            Err(AppError::StorageUnavailable(_))
        ));
    }

    #[test]
    fn closed_audit_refuses_begin() {
        let audit = Audit::new();
        assert!(audit.begin(request()).is_ok());

        audit.close();
        assert!(matches!(
            audit.begin(request()),
            Err(AppError::StorageUnavailable(_))
        ));
    }

    #[test]
    fn failed_outcome_names_kind_and_http_status() {
        let error = AppError::Ollama("HTTP 404: missing".into());
        assert_eq!(
            AuditOutcome::failed(&error, Some(404)).detail.as_deref(),
            Some("ollama, HTTP 404")
        );
        assert_eq!(
            AuditOutcome::failed(&error, Some(200)).detail.as_deref(),
            Some("ollama")
        );
        assert_eq!(
            AuditOutcome::failed(&AppError::Timeout("slow".into()), None)
                .detail
                .as_deref(),
            Some("timeout")
        );
    }
}

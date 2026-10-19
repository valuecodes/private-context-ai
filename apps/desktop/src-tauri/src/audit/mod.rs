//! Audit trail of outbound requests.
//!
//! `MemorySink` keeps the most recent events in memory; a durable database
//! sink implements the same trait later.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

use serde::Serialize;

/// One audited action. Carries metadata only, never message content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    /// Unix time in milliseconds.
    pub at_ms: u64,
    pub action: String,
    pub target: String,
    pub outcome: String,
}

pub trait AuditSink: Send + Sync {
    fn record(&self, event: AuditEvent);

    /// At most `limit` of the newest events, oldest first.
    fn recent(&self, limit: usize) -> Vec<AuditEvent>;
}

/// Ring buffer of the last [`MemorySink::CAPACITY`] events.
#[derive(Debug)]
pub struct MemorySink {
    events: Mutex<VecDeque<AuditEvent>>,
}

impl MemorySink {
    pub const CAPACITY: usize = 500;

    pub fn new() -> Self {
        Self {
            events: Mutex::new(VecDeque::with_capacity(Self::CAPACITY)),
        }
    }
}

impl Default for MemorySink {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditSink for MemorySink {
    fn record(&self, event: AuditEvent) {
        // A panic while holding the lock cannot leave the deque half-updated,
        // so a poisoned lock is still safe to use.
        let mut events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        if events.len() == Self::CAPACITY {
            events.pop_front();
        }
        events.push_back(event);
    }

    fn recent(&self, limit: usize) -> Vec<AuditEvent> {
        let events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        let skip = events.len().saturating_sub(limit);
        events.iter().skip(skip).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(n: u64) -> AuditEvent {
        AuditEvent {
            at_ms: n,
            action: "test".into(),
            target: format!("target-{n}"),
            outcome: "ok".into(),
        }
    }

    #[test]
    fn memory_sink_drops_oldest_beyond_capacity() {
        let sink = MemorySink::new();
        let total = MemorySink::CAPACITY as u64 + 1;
        for n in 0..total {
            sink.record(event(n));
        }

        let all = sink.recent(usize::MAX);
        assert_eq!(all.len(), MemorySink::CAPACITY);
        assert_eq!(all.first().map(|e| e.at_ms), Some(1));
        assert_eq!(all.last().map(|e| e.at_ms), Some(total - 1));
    }

    #[test]
    fn memory_sink_recent_returns_newest_in_order() {
        let sink = MemorySink::new();
        for n in 0..5 {
            sink.record(event(n));
        }

        let newest: Vec<u64> = sink.recent(3).iter().map(|e| e.at_ms).collect();
        assert_eq!(newest, vec![2, 3, 4]);
        assert!(sink.recent(0).is_empty());
    }
}

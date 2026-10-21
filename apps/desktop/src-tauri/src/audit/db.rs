//! The durable audit sink: rows in the encrypted `audit_events` table.

use std::sync::Arc;

use super::{AuditOutcome, AuditSink, KEEP_EVENTS, NewAuditEvent};
use crate::error::AppError;
use crate::storage::Storage;

/// Inserts between two prunes, so a long session stays near the bound too.
const PRUNE_EVERY: i64 = 500;

pub struct DbSink {
    storage: Arc<Storage>,
    keep: usize,
    prune_every: i64,
}

impl DbSink {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self::with_retention(storage, KEEP_EVENTS, PRUNE_EVERY)
    }

    /// Keeps the newest `keep` rows, pruning after every `prune_every`-th
    /// insert.
    pub fn with_retention(storage: Arc<Storage>, keep: usize, prune_every: i64) -> Self {
        Self {
            storage,
            keep,
            prune_every: prune_every.max(1),
        }
    }
}

impl AuditSink for DbSink {
    fn insert(&self, event: &NewAuditEvent) -> Result<i64, AppError> {
        let id = self.storage.insert_audit(event)?;
        if id % self.prune_every == 0
            && let Err(error) = self.storage.prune_audit(self.keep)
        {
            tracing::warn!(kind = error.kind(), "could not prune the audit log");
        }
        Ok(id)
    }

    fn finish(&self, id: i64, outcome: &AuditOutcome) -> Result<(), AppError> {
        self.storage.finish_audit(id, outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::AuditStatus;
    use crate::storage::testing::temp_storage;

    #[test]
    fn inserts_prune_during_a_session() {
        let (storage, _dir) = temp_storage();
        let sink = DbSink::with_retention(Arc::clone(&storage), 3, 2);
        for _ in 0..10 {
            sink.insert(&NewAuditEvent::lifecycle("k", AuditStatus::Ok, None))
                .unwrap();
        }

        let ids: Vec<i64> = storage
            .list_audit(100, None)
            .unwrap()
            .iter()
            .map(|event| event.id)
            .collect();
        assert_eq!(ids, [10, 9, 8]);
    }
}

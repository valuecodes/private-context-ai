//! The durable audit sink: rows in the encrypted `audit_events` table.

use std::sync::Arc;

use super::{AuditOutcome, AuditSink, NewAuditEvent};
use crate::error::AppError;
use crate::storage::Storage;

pub struct DbSink {
    storage: Arc<Storage>,
}

impl DbSink {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }
}

impl AuditSink for DbSink {
    fn insert(&self, event: &NewAuditEvent) -> Result<i64, AppError> {
        self.storage.insert_audit(event)
    }

    fn finish(&self, id: i64, outcome: &AuditOutcome) -> Result<(), AppError> {
        self.storage.finish_audit(id, outcome)
    }
}

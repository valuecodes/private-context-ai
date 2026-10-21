//! Audit rows: written by the durable audit sink, read by the Activity panel.

use rusqlite::{Connection, Row, params};

use super::Storage;
use super::messages::{bad_text, count_column, to_sql_count};
use crate::audit::{AuditEvent, AuditOutcome, AuditStatus, NewAuditEvent};
use crate::error::AppError;

/// `at` from Unix milliseconds in integer math, so `.123` never rounds to
/// `.122`.
const INSERT: &str = "INSERT INTO audit_events
       (at, kind, host, endpoint, model, status, prompt_tokens, completion_tokens,
        duration_ms, detail)
     VALUES (strftime('%Y-%m-%dT%H:%M:%S', ?1 / 1000, 'unixepoch')
               || printf('.%03dZ', ?1 % 1000),
             ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)";

fn insert(conn: &Connection, event: &NewAuditEvent) -> Result<i64, AppError> {
    let outcome = &event.outcome;
    conn.execute(
        INSERT,
        params![
            i64::try_from(event.at_ms).unwrap_or(i64::MAX),
            event.kind,
            event.host,
            event.endpoint,
            event.model,
            outcome.status.as_str(),
            to_sql_count(outcome.prompt_tokens),
            to_sql_count(outcome.completion_tokens),
            to_sql_count(outcome.duration_ms),
            outcome.detail
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn audit_row(row: &Row<'_>) -> rusqlite::Result<AuditEvent> {
    let status: String = row.get(6)?;
    Ok(AuditEvent {
        id: row.get(0)?,
        at: row.get(1)?,
        kind: row.get(2)?,
        host: row.get(3)?,
        endpoint: row.get(4)?,
        model: row.get(5)?,
        status: AuditStatus::parse(&status).ok_or_else(|| bad_text(6, "audit status"))?,
        prompt_tokens: count_column(row, 7)?,
        completion_tokens: count_column(row, 8)?,
        duration_ms: count_column(row, 9)?,
        detail: row.get(10)?,
    })
}

impl Storage {
    pub fn insert_audit(&self, event: &NewAuditEvent) -> Result<i64, AppError> {
        insert(&self.conn(), event)
    }

    /// All of `events` or none of them; returns their ids in order.
    pub fn insert_audit_batch(&self, events: &[&NewAuditEvent]) -> Result<Vec<i64>, AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let ids = events
            .iter()
            .map(|event| insert(&tx, event))
            .collect::<Result<_, _>>()?;
        tx.commit()?;
        Ok(ids)
    }

    /// Only an `attempt` changes, so a late write can never undo an outcome.
    pub fn finish_audit(&self, id: i64, outcome: &AuditOutcome) -> Result<(), AppError> {
        self.conn().execute(
            "UPDATE audit_events
             SET status = ?2, prompt_tokens = ?3, completion_tokens = ?4, duration_ms = ?5,
                 detail = ?6
             WHERE id = ?1 AND status = 'attempt'",
            params![
                id,
                outcome.status.as_str(),
                to_sql_count(outcome.prompt_tokens),
                to_sql_count(outcome.completion_tokens),
                to_sql_count(outcome.duration_ms),
                outcome.detail
            ],
        )?;
        Ok(())
    }

    /// Newest first, at most `limit` rows with an id below `before`.
    pub fn list_audit(
        &self,
        limit: usize,
        before: Option<i64>,
    ) -> Result<Vec<AuditEvent>, AppError> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT id, at, kind, host, endpoint, model, status, prompt_tokens,
                    completion_tokens, duration_ms, detail
             FROM audit_events WHERE ?2 IS NULL OR id < ?2
             ORDER BY id DESC LIMIT ?1",
        )?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let events = statement
            .query_map(params![limit, before], audit_row)?
            .collect::<Result<_, _>>()?;
        Ok(events)
    }

    /// Every distinct destination in the retained rows, sorted.
    pub fn audit_hosts(&self) -> Result<Vec<String>, AppError> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT DISTINCT host FROM audit_events WHERE host IS NOT NULL ORDER BY host",
        )?;
        let hosts = statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        Ok(hosts)
    }

    /// Deletes all but the newest `keep` rows.
    pub fn prune_audit(&self, keep: usize) -> Result<(), AppError> {
        self.conn().execute(
            "DELETE FROM audit_events
             WHERE id NOT IN (SELECT id FROM audit_events ORDER BY id DESC LIMIT ?1)",
            [i64::try_from(keep).unwrap_or(i64::MAX)],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::testing::temp_storage;

    fn event(kind: &str, host: Option<&str>) -> NewAuditEvent {
        NewAuditEvent {
            at_ms: 1_700_000_000_123,
            kind: kind.into(),
            host: host.map(str::to_owned),
            endpoint: None,
            model: None,
            outcome: AuditOutcome::new(AuditStatus::Ok),
        }
    }

    fn ids(events: &[AuditEvent]) -> Vec<i64> {
        events.iter().map(|event| event.id).collect()
    }

    #[test]
    fn list_is_newest_first_and_pages_with_before() {
        let (storage, _dir) = temp_storage();
        for n in 0..5 {
            storage
                .insert_audit(&event(&format!("k{n}"), None))
                .unwrap();
        }

        let first = storage.list_audit(2, None).unwrap();
        assert_eq!(ids(&first), [5, 4]);
        let next = storage.list_audit(2, Some(4)).unwrap();
        assert_eq!(ids(&next), [3, 2]);
        assert_eq!(ids(&storage.list_audit(10, Some(2)).unwrap()), [1]);
    }

    #[test]
    fn at_is_iso_utc_millis() {
        let (storage, _dir) = temp_storage();
        storage.insert_audit(&event("k", None)).unwrap();

        assert_eq!(
            storage.list_audit(1, None).unwrap()[0].at,
            "2023-11-14T22:13:20.123Z"
        );
    }

    #[test]
    fn row_round_trips() {
        let (storage, _dir) = temp_storage();
        let id = storage
            .insert_audit(&NewAuditEvent::request(
                "ollama.chat",
                "127.0.0.1:11434".into(),
                "/api/chat",
                Some("llama3.2"),
            ))
            .unwrap();
        storage
            .finish_audit(
                id,
                &AuditOutcome {
                    status: AuditStatus::Ok,
                    prompt_tokens: Some(3),
                    completion_tokens: Some(2),
                    duration_ms: Some(40),
                    detail: None,
                },
            )
            .unwrap();

        let saved = &storage.list_audit(1, None).unwrap()[0];
        assert_eq!(saved.kind, "ollama.chat");
        assert_eq!(saved.host.as_deref(), Some("127.0.0.1:11434"));
        assert_eq!(saved.endpoint.as_deref(), Some("/api/chat"));
        assert_eq!(saved.model.as_deref(), Some("llama3.2"));
        assert_eq!(saved.status, AuditStatus::Ok);
        assert_eq!(saved.prompt_tokens, Some(3));
        assert_eq!(saved.completion_tokens, Some(2));
        assert_eq!(saved.duration_ms, Some(40));
    }

    #[test]
    fn prune_keeps_newest() {
        let (storage, _dir) = temp_storage();
        for n in 0..5 {
            storage
                .insert_audit(&event(&format!("k{n}"), None))
                .unwrap();
        }

        storage.prune_audit(2).unwrap();
        assert_eq!(ids(&storage.list_audit(10, None).unwrap()), [5, 4]);
    }

    #[test]
    fn hosts_are_distinct_over_all_rows() {
        let (storage, _dir) = temp_storage();
        storage
            .insert_audit(&event("a", Some("127.0.0.2:11434")))
            .unwrap();
        storage.insert_audit(&event("b", None)).unwrap();
        for _ in 0..3 {
            storage
                .insert_audit(&event("c", Some("127.0.0.1:11434")))
                .unwrap();
        }

        assert_eq!(
            storage.audit_hosts().unwrap(),
            ["127.0.0.1:11434", "127.0.0.2:11434"]
        );
    }
}

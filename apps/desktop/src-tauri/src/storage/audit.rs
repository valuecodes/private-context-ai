//! Audit rows: written by the durable audit sink, read by the Activity and
//! Models panels.

use std::collections::BTreeMap;

use rusqlite::{Connection, Row, params};
use serde::Serialize;

use super::Storage;
use super::messages::{bad_text, count_column, to_sql_count};
use crate::audit::{AuditEvent, AuditOutcome, AuditStatus, NewAuditEvent};
use crate::error::AppError;

/// `at` from Unix milliseconds in integer math, so `.123` never rounds to
/// `.122`.
const INSERT: &str = "INSERT INTO audit_events
       (at, kind, host, endpoint, model, status, prompt_tokens, completion_tokens,
        duration_ms, eval_ms, prompt_eval_ms, load_ms, detail)
     VALUES (strftime('%Y-%m-%dT%H:%M:%S', ?1 / 1000, 'unixepoch')
               || printf('.%03dZ', ?1 % 1000),
             ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)";

/// One model's speed over its retained `ok` chat and memory requests that
/// have Ollama timings; returned by `activity_model_stats` and mirrored by
/// `ModelStats` in `src/lib/types.ts`. Speeds are tokens per second.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStats {
    pub model: String,
    /// Requests, not replies: a charted reply makes two.
    pub runs: u64,
    /// ISO-8601 UTC of the newest request.
    pub last_at: String,
    pub gen_tps_median: f64,
    pub gen_tps_p90: f64,
    /// Absent when no request has prompt timings.
    pub prompt_tps_median: Option<f64>,
    /// Absent when no request has a load time.
    pub load_ms_median: Option<f64>,
}

/// The nearest-rank `p` percentile (0 < `p` <= 1) of `sorted`, ascending and
/// not empty.
fn nearest_rank(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    let rank = (p * n as f64).ceil() as usize;
    sorted[rank.clamp(1, n) - 1]
}

/// Sorts `values` and takes its median, or `None` when it is empty.
fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some(nearest_rank(&values, 0.5))
}

/// `tokens` per second over `ms`, when both are known and `ms` is not 0.
fn per_second(tokens: Option<u64>, ms: Option<u64>) -> Option<f64> {
    match (tokens, ms) {
        (Some(tokens), Some(ms)) if ms > 0 => Some(tokens as f64 * 1000.0 / ms as f64),
        _ => None,
    }
}

/// One model's qualifying rows, before the percentiles.
#[derive(Default)]
struct Runs {
    last_at: String,
    generation: Vec<f64>,
    prompt: Vec<f64>,
    load: Vec<f64>,
}

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
            to_sql_count(outcome.eval_ms),
            to_sql_count(outcome.prompt_eval_ms),
            to_sql_count(outcome.load_ms),
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
        eval_ms: count_column(row, 10)?,
        prompt_eval_ms: count_column(row, 11)?,
        load_ms: count_column(row, 12)?,
        detail: row.get(13)?,
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
                 eval_ms = ?6, prompt_eval_ms = ?7, load_ms = ?8, detail = ?9
             WHERE id = ?1 AND status = 'attempt'",
            params![
                id,
                outcome.status.as_str(),
                to_sql_count(outcome.prompt_tokens),
                to_sql_count(outcome.completion_tokens),
                to_sql_count(outcome.duration_ms),
                to_sql_count(outcome.eval_ms),
                to_sql_count(outcome.prompt_eval_ms),
                to_sql_count(outcome.load_ms),
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
                    completion_tokens, duration_ms, eval_ms, prompt_eval_ms, load_ms, detail
             FROM audit_events WHERE ?2 IS NULL OR id < ?2
             ORDER BY id DESC LIMIT ?1",
        )?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let events = statement
            .query_map(params![limit, before], audit_row)?
            .collect::<Result<_, _>>()?;
        Ok(events)
    }

    /// Per model, sorted by name: its `ok` chat and memory requests with a
    /// generation time and a completion count. Errors, cancels, rows from
    /// before timings were recorded and other kinds are left out.
    pub fn model_stats(&self) -> Result<Vec<ModelStats>, AppError> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT model, at, completion_tokens, eval_ms, prompt_tokens, prompt_eval_ms,
                    load_ms
             FROM audit_events
             WHERE kind IN ('ollama.chat', 'ollama.memory') AND status = 'ok'
               AND model IS NOT NULL AND eval_ms > 0 AND completion_tokens IS NOT NULL",
        )?;
        let mut models: BTreeMap<String, Runs> = BTreeMap::new();
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let model: String = row.get(0)?;
            let at: String = row.get(1)?;
            let Some(gen_tps) = per_second(count_column(row, 2)?, count_column(row, 3)?) else {
                continue;
            };
            let runs = models.entry(model).or_default();
            if at > runs.last_at {
                runs.last_at = at;
            }
            runs.generation.push(gen_tps);
            if let Some(prompt_tps) = per_second(count_column(row, 4)?, count_column(row, 5)?) {
                runs.prompt.push(prompt_tps);
            }
            if let Some(load_ms) = count_column(row, 6)? {
                runs.load.push(load_ms as f64);
            }
        }
        Ok(models
            .into_iter()
            .map(|(model, mut runs)| {
                runs.generation.sort_by(f64::total_cmp);
                ModelStats {
                    model,
                    runs: u64::try_from(runs.generation.len()).unwrap_or(u64::MAX),
                    last_at: runs.last_at,
                    gen_tps_median: nearest_rank(&runs.generation, 0.5),
                    gen_tps_p90: nearest_rank(&runs.generation, 0.9),
                    prompt_tps_median: median(runs.prompt),
                    load_ms_median: median(runs.load),
                }
            })
            .collect())
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
                    eval_ms: Some(20),
                    prompt_eval_ms: Some(5),
                    load_ms: Some(10),
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
        assert_eq!(saved.eval_ms, Some(20));
        assert_eq!(saved.prompt_eval_ms, Some(5));
        assert_eq!(saved.load_ms, Some(10));
    }

    /// A finished request row with the given outcome.
    fn run(kind: &str, model: Option<&str>, at_ms: u64, outcome: AuditOutcome) -> NewAuditEvent {
        NewAuditEvent {
            at_ms,
            kind: kind.into(),
            host: Some("127.0.0.1:11434".into()),
            endpoint: Some("/api/chat".into()),
            model: model.map(str::to_owned),
            outcome,
        }
    }

    /// An `ok` outcome generating `tokens` in `eval_ms`.
    fn timed(tokens: u64, eval_ms: u64) -> AuditOutcome {
        AuditOutcome {
            completion_tokens: Some(tokens),
            eval_ms: Some(eval_ms),
            ..AuditOutcome::new(AuditStatus::Ok)
        }
    }

    #[test]
    fn nearest_rank_picks_ceil_rank() {
        assert_eq!(nearest_rank(&[7.0], 0.5), 7.0);
        assert_eq!(nearest_rank(&[7.0], 0.9), 7.0);
        let ten: Vec<f64> = (1..=10).map(f64::from).collect();
        assert_eq!(nearest_rank(&ten, 0.5), 5.0);
        assert_eq!(nearest_rank(&ten, 0.9), 9.0);
        assert_eq!(nearest_rank(&ten, 1.0), 10.0);
        assert_eq!(median(Vec::new()), None);
        assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
    }

    #[test]
    fn model_stats_median_and_p90() {
        let (storage, _dir) = temp_storage();
        // 10, 20, …, 100 tok/s, inserted out of order.
        for (n, tps) in [30, 10, 100, 50, 20, 90, 40, 70, 60, 80]
            .into_iter()
            .enumerate()
        {
            let outcome = AuditOutcome {
                prompt_tokens: Some(100),
                prompt_eval_ms: Some(500),
                load_ms: Some(if n == 0 { 900 } else { 0 }),
                ..timed(tps, 1000)
            };
            let kind = if n == 3 {
                "ollama.memory"
            } else {
                "ollama.chat"
            };
            storage
                .insert_audit(&run(
                    kind,
                    Some("fast"),
                    1_700_000_000_000 + n as u64,
                    outcome,
                ))
                .unwrap();
        }
        storage
            .insert_audit(&run(
                "ollama.chat",
                Some("slow"),
                1_600_000_000_000,
                timed(5, 1000),
            ))
            .unwrap();

        let stats = storage.model_stats().unwrap();
        assert_eq!(
            stats,
            [
                ModelStats {
                    model: "fast".into(),
                    runs: 10,
                    last_at: "2023-11-14T22:13:20.009Z".into(),
                    gen_tps_median: 50.0,
                    gen_tps_p90: 90.0,
                    prompt_tps_median: Some(200.0),
                    load_ms_median: Some(0.0),
                },
                ModelStats {
                    model: "slow".into(),
                    runs: 1,
                    last_at: "2020-09-13T12:26:40.000Z".into(),
                    gen_tps_median: 5.0,
                    gen_tps_p90: 5.0,
                    prompt_tps_median: None,
                    load_ms_median: None,
                },
            ]
        );
    }

    #[test]
    fn model_stats_excludes_unusable_rows() {
        let (storage, _dir) = temp_storage();
        let excluded = [
            run(
                "ollama.chat",
                Some("m"),
                1,
                AuditOutcome {
                    status: AuditStatus::Error,
                    ..timed(10, 1000)
                },
            ),
            run(
                "ollama.chat",
                Some("m"),
                2,
                AuditOutcome {
                    status: AuditStatus::Cancelled,
                    ..timed(10, 1000)
                },
            ),
            run(
                "ollama.chat",
                Some("m"),
                3,
                AuditOutcome {
                    status: AuditStatus::Attempt,
                    ..timed(10, 1000)
                },
            ),
            run(
                "ollama.chat",
                Some("m"),
                4,
                AuditOutcome {
                    eval_ms: None,
                    ..timed(10, 1000)
                },
            ),
            run("ollama.chat", Some("m"), 5, timed(10, 0)),
            run(
                "ollama.chat",
                Some("m"),
                6,
                AuditOutcome {
                    completion_tokens: None,
                    ..timed(10, 1000)
                },
            ),
            run("ollama.show", Some("m"), 7, timed(10, 1000)),
            run("ollama.tags", Some("m"), 8, timed(10, 1000)),
            run("ollama.chat", None, 9, timed(10, 1000)),
        ];
        for event in &excluded {
            storage.insert_audit(event).unwrap();
        }
        assert!(storage.model_stats().unwrap().is_empty());

        storage
            .insert_audit(&run("ollama.chat", Some("m"), 10, timed(20, 1000)))
            .unwrap();
        let [only] = storage.model_stats().unwrap().try_into().unwrap();
        assert_eq!(only.runs, 1);
        assert_eq!(only.gen_tps_median, 20.0);
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

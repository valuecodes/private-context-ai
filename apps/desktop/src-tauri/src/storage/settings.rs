//! Raw `settings` rows; parsing and validation live in `crate::settings`.

use std::collections::HashMap;

use rusqlite::params;

use super::Storage;
use crate::error::AppError;

impl Storage {
    pub fn setting_values(&self) -> Result<HashMap<String, String>, AppError> {
        let conn = self.conn();
        let mut statement = conn.prepare("SELECT key, value FROM settings")?;
        let values = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        Ok(values)
    }

    /// Writes every pair in one transaction.
    pub fn put_settings(&self, values: &[(&str, String)]) -> Result<(), AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        for (key, value) in values {
            tx.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

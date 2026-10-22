//! Schema migrations, tracked in SQLite's `user_version`.

use rusqlite::Connection;
use rusqlite_migration::{Error, M, MigrationDefinitionError, Migrations};

use crate::error::AppError;

/// Append-only: never edit a migration that has shipped.
const STEPS: &[M<'static>] = &[
    M::up(include_str!("schema.sql")),
    M::up(include_str!("migrations/0002_thinking.sql")),
    M::up(include_str!("migrations/0003_superseded.sql")),
    M::up(include_str!("migrations/0004_conversation_settings.sql")),
];
const MIGRATIONS: Migrations<'static> = Migrations::from_slice(STEPS);

/// Brings the schema up to date in one transaction. A database written by a
/// newer version of the app is refused rather than guessed at.
pub fn apply(conn: &mut Connection) -> Result<(), AppError> {
    MIGRATIONS.to_latest(conn).map_err(|err| match err {
        Error::MigrationDefinition(MigrationDefinitionError::DatabaseTooFarAhead) => {
            AppError::Io("the database is from a newer version of this app".into())
        }
        Error::RusqliteError { err, .. } => AppError::from(err),
        other => AppError::Database(other.to_string()),
    })
}

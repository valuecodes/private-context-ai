//! Project CRUD. A project's instructions are sent ahead of the system
//! prompt of every chat in it.

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use uuid::Uuid;

use super::Storage;
use super::conversations::{NOW, check_prompt_text, clean_line, uuid_column};
use crate::error::AppError;
use crate::redacted::Redacted;

/// One project. Its name and instructions are the user's text, so both are
/// redacted in logs like message text.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: Uuid,
    pub name: Redacted,
    pub instructions: Redacted,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str = "id, name, instructions, created_at, updated_at";

fn project_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: uuid_column(row, 0)?,
        name: Redacted::new(row.get::<_, String>(1)?),
        instructions: Redacted::new(row.get::<_, String>(2)?),
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

fn not_found() -> AppError {
    AppError::NotFound("no such project".into())
}

fn clean_name(name: &str) -> Result<String, AppError> {
    clean_line(name, "a project name")
}

/// `NotFound` unless project `id` exists, read on `conn` so a transaction
/// sees its own snapshot.
pub(super) fn check_exists(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.query_row("SELECT 1 FROM projects WHERE id = ?1", [id], |_| Ok(()))
        .optional()?
        .ok_or_else(not_found)
}

impl Storage {
    pub fn create_project(&self, name: &str) -> Result<Project, AppError> {
        let name = clean_name(name)?;
        let conn = self.conn();
        let project = conn.query_row(
            &format!(
                "INSERT INTO projects ({COLUMNS}) VALUES (?1, ?2, '', {NOW}, {NOW})
                 RETURNING {COLUMNS}"
            ),
            params![Uuid::new_v4().to_string(), name],
            project_row,
        )?;
        Ok(project)
    }

    /// Oldest first; the UI sorts them by name.
    pub fn list_projects(&self) -> Result<Vec<Project>, AppError> {
        let conn = self.conn();
        let mut statement = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM projects ORDER BY created_at, rowid"
        ))?;
        let projects = statement
            .query_map([], project_row)?
            .collect::<Result<_, _>>()?;
        Ok(projects)
    }

    pub fn get_project(&self, id: Uuid) -> Result<Project, AppError> {
        self.conn()
            .query_row(
                &format!("SELECT {COLUMNS} FROM projects WHERE id = ?1"),
                [id.to_string()],
                project_row,
            )
            .optional()?
            .ok_or_else(not_found)
    }

    /// What a temporary chat in project `id` is sent, read at each turn.
    pub fn project_instructions(&self, id: Uuid) -> Result<Redacted, AppError> {
        Ok(self.get_project(id)?.instructions)
    }

    pub fn rename_project(&self, id: Uuid, name: &str) -> Result<Project, AppError> {
        let name = clean_name(name)?;
        self.update_project(id, "name", &name)
    }

    /// Applies from the next send of every chat in the project.
    pub fn update_project_instructions(
        &self,
        id: Uuid,
        instructions: &Redacted,
    ) -> Result<Project, AppError> {
        check_prompt_text(instructions, "project instructions")?;
        self.update_project(id, "instructions", instructions.expose())
    }

    /// Sets the text column `column` (a constant, never user input).
    fn update_project(&self, id: Uuid, column: &str, value: &str) -> Result<Project, AppError> {
        self.conn()
            .query_row(
                &format!(
                    "UPDATE projects SET {column} = ?2, updated_at = {NOW}
                     WHERE id = ?1 RETURNING {COLUMNS}"
                ),
                params![id.to_string(), value],
                project_row,
            )
            .optional()?
            .ok_or_else(not_found)
    }

    /// Deletes the project and, by cascade, its conversations and their
    /// messages. Returns the deleted conversations' ids, so their replies can
    /// be cancelled. Deleting one that is already gone is not an error.
    pub fn delete_project(&self, id: Uuid) -> Result<Vec<Uuid>, AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let id = id.to_string();
        let chats: Vec<Uuid> = tx
            .prepare("SELECT id FROM conversations WHERE project_id = ?1")?
            .query_map([&id], |row| uuid_column(row, 0))?
            .collect::<Result<_, _>>()?;
        tx.execute("DELETE FROM projects WHERE id = ?1", [&id])?;
        tx.commit()?;
        Ok(chats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::conversations::{
        ConversationSettings, MAX_SYSTEM_PROMPT_BYTES, MAX_TITLE_CHARS,
    };
    use crate::storage::messages::TurnInput;
    use crate::storage::testing::{count_rows, temp_storage};

    #[test]
    fn project_crud() {
        let (storage, _dir) = temp_storage();

        let created = storage.create_project("  Thesis  ").unwrap();
        assert_eq!(created.name.expose(), "Thesis");
        assert_eq!(created.instructions.expose(), "");
        assert_eq!(created.created_at, created.updated_at);
        let second = storage.create_project("Taxes").unwrap();
        assert_eq!(
            storage.list_projects().unwrap(),
            [created.clone(), second.clone()]
        );
        assert_eq!(storage.get_project(created.id).unwrap(), created);

        let renamed = storage.rename_project(created.id, "Dissertation").unwrap();
        assert_eq!(renamed.name.expose(), "Dissertation");
        let updated = storage
            .update_project_instructions(created.id, &Redacted::new("Cite sources."))
            .unwrap();
        assert_eq!(updated.instructions.expose(), "Cite sources.");
        assert_eq!(updated.name.expose(), "Dissertation");
        assert_eq!(
            storage.project_instructions(created.id).unwrap().expose(),
            "Cite sources."
        );

        assert!(storage.delete_project(created.id).unwrap().is_empty());
        assert_eq!(storage.list_projects().unwrap(), [second]);
        assert!(storage.delete_project(created.id).unwrap().is_empty());
        for result in [
            storage.get_project(created.id),
            storage.rename_project(created.id, "x"),
            storage.update_project_instructions(created.id, &Redacted::new("x")),
        ] {
            assert!(matches!(result, Err(AppError::NotFound(_))));
        }
    }

    #[test]
    fn project_name_and_instructions_validated() {
        let (storage, _dir) = temp_storage();
        let id = storage.create_project("p").unwrap().id;
        let long = "x".repeat(MAX_TITLE_CHARS + 1);
        for name in ["", "  ", "two\nlines", long.as_str()] {
            assert!(matches!(
                storage.create_project(name),
                Err(AppError::InvalidInput(_))
            ));
            assert!(matches!(
                storage.rename_project(id, name),
                Err(AppError::InvalidInput(_))
            ));
        }
        for instructions in ["x".repeat(MAX_SYSTEM_PROMPT_BYTES + 1), "a\0b".to_owned()] {
            assert!(matches!(
                storage.update_project_instructions(id, &Redacted::new(instructions)),
                Err(AppError::InvalidInput(_))
            ));
        }
        assert!(
            storage
                .update_project_instructions(
                    id,
                    &Redacted::new("x".repeat(MAX_SYSTEM_PROMPT_BYTES))
                )
                .is_ok()
        );
        assert_eq!(storage.get_project(id).unwrap().name.expose(), "p");
    }

    #[test]
    fn project_name_and_instructions_debug_redacted() {
        let (storage, _dir) = temp_storage();
        let id = storage.create_project("Secret plans").unwrap().id;
        let project = storage
            .update_project_instructions(id, &Redacted::new("Never reveal this"))
            .unwrap();

        let debug = format!("{project:?}");
        assert!(!debug.contains("Secret"));
        assert!(!debug.contains("reveal"));
    }

    #[test]
    fn delete_project_cascades_chats_and_messages() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        let other = storage.create_project("q").unwrap().id;
        let in_project = |project| {
            let id = storage
                .create_conversation_with(
                    None,
                    "m",
                    &ConversationSettings::default(),
                    Some(project),
                )
                .unwrap()
                .id;
            storage
                .begin_turn(id, "m", &TurnInput::New(Redacted::new("Hi")), |_, _| Ok(()))
                .unwrap();
            id
        };
        let first = in_project(project);
        let second = in_project(project);
        let kept = in_project(other);
        let loose = storage.create_conversation(None, "m").unwrap().id;
        assert_eq!(
            storage
                .get_conversation(first)
                .unwrap()
                .conversation
                .project_id,
            Some(project)
        );

        let mut deleted = storage.delete_project(project).unwrap();
        deleted.sort();
        let mut expected = vec![first, second];
        expected.sort();
        assert_eq!(deleted, expected);

        let left: Vec<Uuid> = storage
            .list_conversations()
            .unwrap()
            .into_iter()
            .map(|conversation| conversation.id)
            .collect();
        assert_eq!(left.len(), 2);
        assert!(left.contains(&kept) && left.contains(&loose));
        // Two rows (prompt and pending reply) for the one chat left with a turn.
        assert_eq!(count_rows(&storage, "messages"), 2);
    }

    #[test]
    fn create_conversation_in_unknown_project_is_not_found() {
        let (storage, _dir) = temp_storage();
        assert!(matches!(
            storage.create_conversation_with(
                None,
                "m",
                &ConversationSettings::default(),
                Some(Uuid::new_v4())
            ),
            Err(AppError::NotFound(_))
        ));
        assert_eq!(count_rows(&storage, "conversations"), 0);
    }
}

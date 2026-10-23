//! Comparing models: one prompt answered by several models, one answer kept.

use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use super::Storage;
use super::conversations::{ConversationSettings, NOW, load_settings, not_found};
use super::messages::{
    check_no_comparison, insert_pending_reply, insert_prompt, load_history, next_created_at,
    with_system_prompt,
};
use crate::error::AppError;
use crate::ollama::types::{ChatMessage, ChatOptions, ChatRole};
use crate::redacted::Redacted;

/// A comparison that was saved and is ready to run.
#[derive(Debug)]
pub struct CompareTurn {
    pub group: Uuid,
    pub user_id: Uuid,
    /// One `pending` reply row per model, in run order.
    pub candidates: Vec<(Uuid, String)>,
    /// What every model gets: the system prompt, earlier turns and the
    /// prompt.
    pub history: Vec<ChatMessage>,
    pub options: Option<ChatOptions>,
}

fn still_running() -> AppError {
    AppError::InvalidInput("the comparison is still running".into())
}

impl Storage {
    /// Like [`Storage::begin_turn`] for a new prompt, but saves one `pending`
    /// reply per model. The prompt and its replies share a fresh compare
    /// group, which keeps them out of every history until one reply is
    /// picked. The conversation's model changes only at the pick.
    pub fn begin_compare(
        &self,
        conversation_id: Uuid,
        prompt: &Redacted,
        models: &[String],
        check: impl FnOnce(&[ChatMessage], &ConversationSettings) -> Result<(), AppError>,
    ) -> Result<CompareTurn, AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let id = conversation_id.to_string();
        let settings = load_settings(&tx, &id)?;
        check_no_comparison(&tx, &id)?;
        let mut history = load_history(&tx, &id, None)?;
        history.push(ChatMessage::new(ChatRole::User, prompt.clone()));
        let history = with_system_prompt(&settings, history);
        check(&history, &settings)?;

        let group = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let created_at = next_created_at(&tx, &id)?;
        insert_prompt(&tx, &id, user_id, prompt, &created_at, Some(group))?;
        let mut candidates = Vec::with_capacity(models.len());
        for model in models {
            let candidate = Uuid::new_v4();
            // Same time: rowid keeps the run order.
            insert_pending_reply(&tx, &id, candidate, model, &created_at, Some(group))?;
            candidates.push((candidate, model.clone()));
        }
        tx.execute(
            &format!("UPDATE conversations SET updated_at = {NOW} WHERE id = ?1"),
            [&id],
        )?;
        tx.commit()?;
        Ok(CompareTurn {
            group,
            user_id,
            candidates,
            history,
            options: settings.options(),
        })
    }

    /// Keeps the complete candidate `message_id`: the other candidates are
    /// deleted, the prompt and the pick become ordinary rows, and the
    /// conversation continues with the pick's model, which is returned.
    /// Refused while any candidate is still running.
    pub fn compare_pick(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<String, AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let id = conversation_id.to_string();
        let (group, status, model): (String, String, Option<String>) = tx
            .query_row(
                "SELECT compare_group, status, model FROM messages
                 WHERE id = ?1 AND conversation_id = ?2 AND role = 'assistant'
                   AND compare_group IS NOT NULL",
                params![message_id.to_string(), id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("no such answer".into()))?;
        if group_running(&tx, &id, &group)? {
            return Err(still_running());
        }
        if status != "complete" {
            return Err(AppError::InvalidInput(
                "only a complete answer can be kept".into(),
            ));
        }
        let model = model.unwrap_or_default();
        tx.execute(
            "DELETE FROM messages WHERE compare_group = ?1 AND role = 'assistant' AND id <> ?2",
            params![group, message_id.to_string()],
        )?;
        tx.execute(
            "UPDATE messages SET compare_group = NULL WHERE compare_group = ?1",
            [&group],
        )?;
        tx.execute(
            &format!("UPDATE conversations SET model = ?2, updated_at = {NOW} WHERE id = ?1"),
            params![id, model],
        )?;
        tx.commit()?;
        Ok(model)
    }

    /// Marks every candidate still `pending` as `error`, once the worker
    /// is done with the group. A candidate whose final write failed would
    /// otherwise keep the group running, refusing every pick, discard and
    /// turn until the next start.
    pub fn settle_compare(&self, group: Uuid) -> Result<(), AppError> {
        self.conn().execute(
            "UPDATE messages SET status = 'error' WHERE compare_group = ?1 AND status = 'pending'",
            [group.to_string()],
        )?;
        Ok(())
    }

    /// Deletes the compared prompt and all its candidates. Refused while any
    /// candidate is still running.
    pub fn compare_discard(&self, conversation_id: Uuid, group: Uuid) -> Result<(), AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let id = conversation_id.to_string();
        let group = group.to_string();
        if group_running(&tx, &id, &group)? {
            return Err(still_running());
        }
        let deleted = tx.execute(
            "DELETE FROM messages WHERE conversation_id = ?1 AND compare_group = ?2",
            params![id, group],
        )?;
        if deleted == 0 {
            return Err(not_found());
        }
        tx.commit()?;
        Ok(())
    }
}

fn group_running(
    conn: &rusqlite::Connection,
    conversation_id: &str,
    group: &str,
) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM messages
                        WHERE conversation_id = ?1 AND compare_group = ?2
                          AND status = 'pending')",
        [conversation_id, group],
        |row| row.get(0),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::storage::messages::{
        FlushPolicy, MessageStatus, ReplyStats, StoredMessage, TurnInput, TurnRecorder,
    };
    use crate::storage::testing::{count_rows, temp_storage};

    fn models(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn compare(storage: &Storage, id: Uuid, prompt: &str) -> CompareTurn {
        storage
            .begin_compare(id, &Redacted::new(prompt), &models(&["a", "b"]), |_, _| {
                Ok(())
            })
            .unwrap()
    }

    fn end(storage: &Arc<Storage>, message: Uuid, text: &str, status: MessageStatus) {
        let mut recorder = TurnRecorder::new(Arc::clone(storage), message, FlushPolicy::default());
        recorder.push(&Redacted::new(text)).unwrap();
        if status == MessageStatus::Complete {
            recorder.complete(ReplyStats::default()).unwrap();
        } else {
            recorder.finish(status).unwrap();
        }
    }

    /// A conversation with "One" answered "AOne".
    fn answered(storage: &Arc<Storage>) -> Uuid {
        let id = storage.create_conversation(None, "m").unwrap().id;
        let turn = storage
            .begin_turn(
                id,
                "m",
                &TurnInput::New(Redacted::new("One")),
                |_, _| Ok(()),
            )
            .unwrap();
        end(storage, turn.assistant_id, "AOne", MessageStatus::Complete);
        id
    }

    fn messages(storage: &Storage, id: Uuid) -> Vec<StoredMessage> {
        storage.get_conversation(id).unwrap().messages
    }

    fn texts(history: &[ChatMessage]) -> Vec<&str> {
        history
            .iter()
            .map(|message| message.content.expose())
            .collect()
    }

    #[test]
    fn begin_turn_records_message_model() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage);
        let rows = messages(&storage, id);
        assert_eq!(rows[0].model, None);
        assert_eq!(rows[1].model.as_deref(), Some("m"));
        assert_eq!(rows[1].compare_group, None);
    }

    #[test]
    fn begin_compare_saves_prompt_and_pending_candidates() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage);
        let before = storage.get_conversation(id).unwrap().conversation;

        let turn = compare(&storage, id, "Two");

        assert_eq!(texts(&turn.history), ["One", "AOne", "Two"]);
        let rows = messages(&storage, id);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[2].id, turn.user_id);
        assert_eq!(rows[2].compare_group, Some(turn.group));
        let candidates: Vec<(Uuid, Option<&str>, MessageStatus)> = rows[3..]
            .iter()
            .map(|row| (row.id, row.model.as_deref(), row.status))
            .collect();
        assert_eq!(
            candidates,
            [
                (turn.candidates[0].0, Some("a"), MessageStatus::Pending),
                (turn.candidates[1].0, Some("b"), MessageStatus::Pending),
            ]
        );
        assert!(
            rows[3..]
                .iter()
                .all(|row| row.compare_group == Some(turn.group))
        );
        // The model changes only at the pick.
        assert_eq!(
            storage.get_conversation(id).unwrap().conversation.model,
            before.model
        );
    }

    #[test]
    fn begin_compare_check_failure_inserts_nothing() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage);
        let err = storage
            .begin_compare(id, &Redacted::new("Two"), &models(&["a", "b"]), |_, _| {
                Err(AppError::TooLarge("too big".into()))
            })
            .unwrap_err();
        assert!(matches!(err, AppError::TooLarge(_)));
        assert_eq!(count_rows(&storage, "messages"), 2);
    }

    #[test]
    fn compare_candidates_excluded_from_history() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage);
        let turn = compare(&storage, id, "Two");
        end(
            &storage,
            turn.candidates[0].0,
            "From a",
            MessageStatus::Complete,
        );
        end(
            &storage,
            turn.candidates[1].0,
            "From b",
            MessageStatus::Complete,
        );

        storage.compare_pick(id, turn.candidates[1].0).unwrap();
        let next = storage
            .begin_turn(id, "b", &TurnInput::New(Redacted::new("Three")), |_, _| {
                Ok(())
            })
            .unwrap();
        assert_eq!(
            texts(&next.history),
            ["One", "AOne", "Two", "From b", "Three"]
        );
    }

    #[test]
    fn send_refused_while_compare_unresolved() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage);
        let turn = compare(&storage, id, "Two");
        end(&storage, turn.candidates[0].0, "x", MessageStatus::Complete);
        end(&storage, turn.candidates[1].0, "y", MessageStatus::Complete);
        let first = messages(&storage, id)[0].id;

        for input in [
            TurnInput::New(Redacted::new("Three")),
            TurnInput::Regenerate,
            TurnInput::Edit {
                message_id: first,
                prompt: Redacted::new("x"),
            },
        ] {
            assert!(matches!(
                storage.begin_turn(id, "m", &input, |_, _| Ok(())),
                Err(AppError::InvalidInput(_))
            ));
        }
        assert!(matches!(
            storage.begin_compare(
                id,
                &Redacted::new("Again"),
                &models(&["a", "b"]),
                |_, _| Ok(())
            ),
            Err(AppError::InvalidInput(_))
        ));
        assert_eq!(count_rows(&storage, "messages"), 5);
    }

    #[test]
    fn compare_pick_keeps_one_and_sets_model() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage);
        let turn = compare(&storage, id, "Two");
        end(
            &storage,
            turn.candidates[0].0,
            "From a",
            MessageStatus::Complete,
        );
        end(
            &storage,
            turn.candidates[1].0,
            "From b",
            MessageStatus::Error,
        );

        assert_eq!(storage.compare_pick(id, turn.candidates[0].0).unwrap(), "a");

        let rows = messages(&storage, id);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[3].id, turn.candidates[0].0);
        assert!(rows.iter().all(|row| row.compare_group.is_none()));
        assert_eq!(
            storage.get_conversation(id).unwrap().conversation.model,
            "a"
        );
        // Picked once: the group is gone.
        assert!(matches!(
            storage.compare_pick(id, turn.candidates[0].0),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn compare_pick_rejects_failed_candidate() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage);
        let turn = compare(&storage, id, "Two");
        end(
            &storage,
            turn.candidates[0].0,
            "half",
            MessageStatus::Cancelled,
        );
        end(&storage, turn.candidates[1].0, "bad", MessageStatus::Error);

        for (candidate, _) in &turn.candidates {
            assert!(matches!(
                storage.compare_pick(id, *candidate),
                Err(AppError::InvalidInput(_))
            ));
        }
        assert_eq!(count_rows(&storage, "messages"), 5);
    }

    #[test]
    fn compare_pick_and_discard_refused_while_pending() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage);
        let turn = compare(&storage, id, "Two");
        end(
            &storage,
            turn.candidates[0].0,
            "done",
            MessageStatus::Complete,
        );

        assert!(matches!(
            storage.compare_pick(id, turn.candidates[0].0),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            storage.compare_discard(id, turn.group),
            Err(AppError::InvalidInput(_))
        ));
        assert_eq!(count_rows(&storage, "messages"), 5);
    }

    #[test]
    fn compare_pick_wrong_conversation_or_message_not_found() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage);
        let other = answered(&storage);
        let turn = compare(&storage, id, "Two");
        let plain = messages(&storage, id)[1].id;

        assert!(matches!(
            storage.compare_pick(other, turn.candidates[0].0),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            storage.compare_pick(id, turn.user_id),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            storage.compare_pick(id, plain),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            storage.compare_discard(other, turn.group),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn settle_compare_unblocks_a_group_left_pending() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage);
        let turn = compare(&storage, id, "Two");
        end(
            &storage,
            turn.candidates[0].0,
            "From a",
            MessageStatus::Complete,
        );
        // b's final write failed: its row is still `pending`.

        storage.settle_compare(turn.group).unwrap();

        let rows = messages(&storage, id);
        assert_eq!(rows[3].status, MessageStatus::Complete);
        assert_eq!(rows[4].status, MessageStatus::Error);
        assert_eq!(storage.compare_pick(id, turn.candidates[0].0).unwrap(), "a");
    }

    #[test]
    fn compare_discard_removes_turn() {
        let (storage, _dir) = temp_storage();
        let id = answered(&storage);
        let before = messages(&storage, id);
        let turn = compare(&storage, id, "Two");
        end(&storage, turn.candidates[0].0, "x", MessageStatus::Complete);
        end(&storage, turn.candidates[1].0, "y", MessageStatus::Error);

        storage.compare_discard(id, turn.group).unwrap();

        assert_eq!(messages(&storage, id), before);
        assert_eq!(count_rows(&storage, "messages"), 2);
        // Sending works again.
        storage
            .begin_turn(id, "m", &TurnInput::New(Redacted::new("Next")), |_, _| {
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn startup_recovery_marks_pending_candidates_error() {
        let dir = tempfile::tempdir().unwrap();
        let keys = crate::storage::key::testing::MemoryKeySource::default();
        let storage = Arc::new(super::super::open_storage(dir.path(), &keys).unwrap());
        let id = answered(&storage);
        let turn = compare(&storage, id, "Two");
        end(
            &storage,
            turn.candidates[0].0,
            "From a",
            MessageStatus::Complete,
        );
        // A hard kill while b streams.
        drop(storage);

        let storage = super::super::open_storage(dir.path(), &keys).unwrap();
        let rows = messages(&storage, id);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[3].status, MessageStatus::Complete);
        assert_eq!(rows[4].status, MessageStatus::Error);
        assert!(
            rows[2..]
                .iter()
                .all(|row| row.compare_group == Some(turn.group))
        );
        assert_eq!(storage.compare_pick(id, turn.candidates[0].0).unwrap(), "a");
    }
}

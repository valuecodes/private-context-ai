//! Project CRUD. Each database call is one short statement or transaction,
//! run inline in an async command (never on the main thread).

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::error::AppError;
use crate::memory::{MemoryScheduler, RefreshResult, refresh};
use crate::ollama::Ollama;
use crate::redacted::Redacted;
use crate::storage::memory::{ContextPreview, MemoryEntry};
use crate::storage::projects::Project;
use crate::storage::{Storage, StorageState};
use crate::temporary::TempChats;

/// What `project_memory_list` returns; mirrored by `MemoryList` in
/// `src/lib/types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryList {
    /// Newest first.
    pub entries: Vec<MemoryEntry>,
    /// Chats with replies and no entry yet, the oldest first.
    pub unsaved: Vec<UnsavedChat>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsavedChat {
    pub conversation_id: Uuid,
    pub title: Redacted,
    pub skip_reason: Option<String>,
}

/// One project's pending count; mirrored by `PendingCount` in
/// `src/lib/types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingCount {
    pub project_id: Uuid,
    pub pending: usize,
}

#[tauri::command]
pub async fn project_create(
    storage: State<'_, StorageState>,
    name: Redacted,
) -> Result<Project, AppError> {
    storage.get()?.create_project(name.expose())
}

#[tauri::command]
pub async fn project_list(storage: State<'_, StorageState>) -> Result<Vec<Project>, AppError> {
    storage.get()?.list_projects()
}

#[tauri::command]
pub async fn project_get(storage: State<'_, StorageState>, id: Uuid) -> Result<Project, AppError> {
    storage.get()?.get_project(id)
}

#[tauri::command]
pub async fn project_rename(
    storage: State<'_, StorageState>,
    id: Uuid,
    name: Redacted,
) -> Result<Project, AppError> {
    storage.get()?.rename_project(id, name.expose())
}

/// Applies from the next send of every chat in the project, saved or
/// temporary.
#[tauri::command]
pub async fn project_update_instructions(
    storage: State<'_, StorageState>,
    id: Uuid,
    instructions: Redacted,
) -> Result<Project, AppError> {
    storage
        .get()?
        .update_project_instructions(id, &instructions)
}

/// Off drops the project's queued summaries and cancels a running one; on
/// queues its pending chats at once.
#[tauri::command]
pub async fn project_set_auto_memory(
    storage: State<'_, StorageState>,
    scheduler: State<'_, Arc<MemoryScheduler>>,
    id: Uuid,
    on: bool,
) -> Result<Project, AppError> {
    set_auto_memory(storage.get()?, &scheduler, id, on).await
}

/// The body of `project_set_auto_memory`, without Tauri.
pub async fn set_auto_memory(
    storage: &Storage,
    scheduler: &MemoryScheduler,
    id: Uuid,
    on: bool,
) -> Result<Project, AppError> {
    let project = storage.set_auto_memory(id, on)?;
    if on {
        scheduler.catch_up(id);
    } else {
        scheduler.forget_project(id).await;
    }
    Ok(project)
}

/// The project's memory entries, newest first, and its chats that have
/// none yet, each with why automatic memory skips it, if it does.
#[tauri::command]
pub async fn project_memory_list(
    storage: State<'_, StorageState>,
    scheduler: State<'_, Arc<MemoryScheduler>>,
    project_id: Uuid,
) -> Result<MemoryList, AppError> {
    memory_list(storage.get()?, &scheduler, project_id)
}

/// The body of `project_memory_list`, without Tauri.
pub fn memory_list(
    storage: &Storage,
    scheduler: &MemoryScheduler,
    project_id: Uuid,
) -> Result<MemoryList, AppError> {
    let entries = storage
        .list_memories(project_id)?
        .into_iter()
        .map(|entry| MemoryEntry {
            skip_reason: scheduler.skip_reason(entry.conversation_id),
            ..entry
        })
        .collect();
    let unsaved = storage
        .unsaved_chats(project_id)?
        .into_iter()
        .map(|chat| UnsavedChat {
            skip_reason: scheduler.skip_reason(chat.conversation_id),
            conversation_id: chat.conversation_id,
            title: chat.title,
        })
        .collect();
    Ok(MemoryList { entries, unsaved })
}

/// Moves the project's pending chats to the front of the background
/// queue; the project view calls it when it opens.
#[tauri::command]
pub async fn project_memory_catch_up(
    scheduler: State<'_, Arc<MemoryScheduler>>,
    project_id: Uuid,
) -> Result<(), AppError> {
    scheduler.catch_up(project_id);
    Ok(())
}

/// How many chats of each project with automatic memory on are pending;
/// projects with none are left out.
#[tauri::command]
pub async fn project_memory_pending(
    storage: State<'_, StorageState>,
) -> Result<Vec<PendingCount>, AppError> {
    pending_counts(storage.get()?)
}

/// The body of `project_memory_pending`, without Tauri.
pub fn pending_counts(storage: &Storage) -> Result<Vec<PendingCount>, AppError> {
    let mut counts: HashMap<Uuid, usize> = HashMap::new();
    for chat in storage.pending_chats(None)? {
        *counts.entry(chat.project_id).or_default() += 1;
    }
    Ok(counts
        .into_iter()
        .map(|(project_id, pending)| PendingCount {
            project_id,
            pending,
        })
        .collect())
}

/// Replaces a chat's entry with the user's text; its next summary starts
/// from it.
#[tauri::command]
pub async fn project_memory_update(
    storage: State<'_, StorageState>,
    conversation_id: Uuid,
    content: Redacted,
) -> Result<MemoryEntry, AppError> {
    storage.get()?.update_memory(conversation_id, &content)
}

#[tauri::command]
pub async fn project_memory_delete(
    storage: State<'_, StorageState>,
    conversation_id: Uuid,
) -> Result<(), AppError> {
    storage.get()?.delete_memory(conversation_id)
}

/// Summarizes one chat of the project, or every saved chat one after
/// another, each with its own model. Every request is audited as
/// `ollama.memory`.
#[tauri::command]
pub async fn project_memory_refresh(
    storage: State<'_, StorageState>,
    ollama: State<'_, Arc<Ollama>>,
    project_id: Uuid,
    conversation_id: Option<Uuid>,
) -> Result<Vec<RefreshResult>, AppError> {
    refresh(&ollama, storage.get()?, project_id, conversation_id).await
}

/// The exact `system` message the project's chats get now, or with
/// `conversation_id` what that chat gets, with `draft` the passages found
/// for it included. Sends nothing.
#[tauri::command]
pub async fn project_context_preview(
    storage: State<'_, StorageState>,
    ollama: State<'_, Arc<Ollama>>,
    project_id: Uuid,
    conversation_id: Option<Uuid>,
    draft: Option<Redacted>,
) -> Result<ContextPreview, AppError> {
    storage.get()?.context_preview(
        project_id,
        conversation_id,
        draft.as_ref(),
        ollama.max_prompt_bytes(),
    )
}

/// Deletes the project with its saved chats and temporary chats; returns how
/// many saved chats were deleted.
#[tauri::command]
pub async fn project_delete(
    storage: State<'_, StorageState>,
    ollama: State<'_, Arc<Ollama>>,
    chats: State<'_, Arc<TempChats>>,
    scheduler: State<'_, Arc<MemoryScheduler>>,
    id: Uuid,
) -> Result<usize, AppError> {
    let deleted = delete_project(storage.get()?, &ollama, &chats, id).await?;
    scheduler.forget_project(id).await;
    Ok(deleted)
}

/// The body of `project_delete`, without Tauri. Like `conversation_delete`
/// and a temporary chat's discard: the rows and sessions go first, then
/// their replies are cancelled, and so is a memory summary of the project.
/// A send racing this either finds its chat gone, or was admitted before
/// and is cancelled here.
pub async fn delete_project(
    storage: &Storage,
    ollama: &Ollama,
    chats: &TempChats,
    id: Uuid,
) -> Result<usize, AppError> {
    let saved = storage.delete_project(id)?;
    let temporary = chats.discard_project(id);
    for chat in saved.iter().chain(&temporary).chain([&id]) {
        ollama.cancel_conversation(*chat).await;
    }
    Ok(saved.len())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::audit::Audit;
    use crate::commands::ollama::{SendRequest, send_turn};
    use crate::commands::temporary::{confirm_project, send_temp_turn, start_temp};
    use crate::commands::testing::no_think;
    use crate::ollama::client::OllamaBaseUrl;
    use crate::ollama::testing::{Fake, Reply, fake_ollama};
    use crate::ollama::types::ThinkChoice;
    use crate::storage::conversations::ConversationSettings;
    use crate::storage::key::testing::MemoryKeySource;
    use crate::storage::memory::ProjectContext;
    use crate::storage::messages::{FlushPolicy, TurnInput};
    use crate::storage::testing::count_rows;

    const ANSWER: &str = r#"{"message":{"role":"assistant","content":"ok"},"done":false}"#;
    const DONE: &str = r#"{"message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":3,"eval_count":2}"#;

    struct Setup {
        fake: Fake,
        ollama: Arc<Ollama>,
        state: StorageState,
        storage: Arc<Storage>,
        chats: Arc<TempChats>,
        _dir: tempfile::TempDir,
    }

    /// Open storage, a fake Ollama answering every chat with `ok`, and its
    /// audit log on that storage.
    async fn setup() -> Setup {
        let fake = fake_ollama(Reply::Lines(vec![ANSWER, DONE], Duration::ZERO)).await;
        let dir = tempfile::tempdir().unwrap();
        let state = StorageState::open(dir.path(), &MemoryKeySource::default());
        let storage = Arc::clone(state.get().unwrap());
        let base = OllamaBaseUrl::parse(&format!("http://{}", fake.addr));
        let ollama = Arc::new(Ollama::new(
            base,
            Arc::new(Audit::over(Arc::clone(&storage))),
        ));
        Setup {
            fake,
            ollama,
            state,
            storage,
            chats: Arc::new(TempChats::default()),
            _dir: dir,
        }
    }

    fn request(ollama: &Ollama, conversation_id: Uuid, prompt: &str) -> SendRequest {
        SendRequest {
            conversation_id,
            model: "m".into(),
            input: TurnInput::New(Redacted::new(prompt)),
            think_choice: ThinkChoice::Off,
            think: no_think(ollama),
        }
    }

    /// Waits until the fake Ollama has received `count` request bodies and
    /// returns the last one's first message.
    async fn first_message(fake: &Fake, count: usize) -> serde_json::Value {
        for _ in 0..500 {
            if let Some(body) = fake.bodies.lock().unwrap().get(count - 1) {
                return body["messages"][0].clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("Ollama never got request {count}");
    }

    /// Waits until no reply is running.
    async fn wait_idle(ollama: &Ollama) {
        for _ in 0..500 {
            if ollama.inflight_len() == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the reply never ended");
    }

    fn project_with(storage: &Storage, instructions: &str) -> Uuid {
        let id = storage.create_project("p").unwrap().id;
        storage
            .update_project_instructions(id, &Redacted::new(instructions))
            .unwrap();
        id
    }

    fn send_temp(setup: &Setup, session: Uuid, prompt: &str) -> Result<(), AppError> {
        let context = match setup.chats.project(session)? {
            Some(project) => setup.storage.project_context(project)?,
            None => ProjectContext::default(),
        };
        send_temp_turn(
            &setup.ollama,
            &setup.chats,
            context,
            None,
            request(&setup.ollama, session, prompt),
            |_| true,
        )
        .map(|_| ())
    }

    #[tokio::test]
    async fn project_chat_request_has_instructions_first() {
        let setup = setup().await;
        let project = project_with(&setup.storage, "Cite sources.");
        let settings = ConversationSettings {
            system_prompt: Redacted::new("Be brief"),
            ..ConversationSettings::default()
        };
        let conversation = setup
            .storage
            .create_conversation_with(None, "m", &settings, Some(project))
            .unwrap()
            .id;

        send_turn(
            &setup.ollama,
            Arc::clone(&setup.storage),
            request(&setup.ollama, conversation, "Hi"),
            FlushPolicy::default(),
            |_| true,
        )
        .unwrap();

        assert_eq!(
            first_message(&setup.fake, 1).await,
            serde_json::json!({
                "role": "system",
                "content": "## Project instructions\n\nCite sources.\n\n## This chat\n\nBe brief"
            })
        );
    }

    #[tokio::test]
    async fn temp_project_chat_picks_up_edited_instructions() {
        let setup = setup().await;
        let project = project_with(&setup.storage, "First rules.");
        let session = start_temp(
            &setup.state,
            &setup.chats,
            ConversationSettings::default(),
            Some(project),
        )
        .unwrap();

        send_temp(&setup, session, "One").unwrap();
        assert_eq!(
            first_message(&setup.fake, 1).await["content"],
            "## Project instructions\n\nFirst rules."
        );
        wait_idle(&setup.ollama).await;
        setup
            .storage
            .update_project_instructions(project, &Redacted::new("Second rules."))
            .unwrap();
        send_temp(&setup, session, "Two").unwrap();

        assert_eq!(
            first_message(&setup.fake, 2).await["content"],
            "## Project instructions\n\nSecond rules."
        );
    }

    #[tokio::test]
    async fn temp_project_chat_writes_no_rows() {
        let setup = setup().await;
        let project = project_with(&setup.storage, "Rules.");
        let session = start_temp(
            &setup.state,
            &setup.chats,
            ConversationSettings::default(),
            Some(project),
        )
        .unwrap();

        send_temp(&setup, session, "One").unwrap();
        first_message(&setup.fake, 1).await;
        wait_idle(&setup.ollama).await;

        assert_eq!(count_rows(&setup.storage, "conversations"), 0);
        assert_eq!(count_rows(&setup.storage, "messages"), 0);
        let audited = setup
            .storage
            .list_audit(100, None)
            .unwrap()
            .into_iter()
            .filter(|event| event.kind == "ollama.chat")
            .count();
        assert_eq!(audited, 1);
    }

    #[tokio::test]
    async fn delete_project_discards_its_temp_sessions() {
        let setup = setup().await;
        let project = project_with(&setup.storage, "Rules.");
        let other = project_with(&setup.storage, "Other rules.");
        let start = |project| {
            start_temp(
                &setup.state,
                &setup.chats,
                ConversationSettings::default(),
                Some(project),
            )
            .unwrap()
        };
        let session = start(project);
        let kept = start(other);
        let saved = setup
            .storage
            .create_conversation_with(None, "m", &ConversationSettings::default(), Some(project))
            .unwrap()
            .id;

        let deleted = delete_project(&setup.storage, &setup.ollama, &setup.chats, project)
            .await
            .unwrap();

        assert_eq!(deleted, 1);
        assert!(matches!(
            setup.storage.get_conversation(saved),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            send_temp(&setup, session, "Hi"),
            Err(AppError::NotFound(_))
        ));
        assert_eq!(setup.chats.project(kept).unwrap(), Some(other));
    }

    #[tokio::test]
    async fn delete_project_cancels_running_replies() {
        let (storage, _dir) = crate::storage::testing::temp_storage();
        let (ollama, _listener) = crate::commands::testing::silent_ollama().await;
        let chats = Arc::new(TempChats::default());
        let project = storage.create_project("p").unwrap().id;
        let saved = storage
            .create_conversation_with(None, "m", &ConversationSettings::default(), Some(project))
            .unwrap()
            .id;
        send_turn(
            &ollama,
            Arc::clone(&storage),
            request(&ollama, saved, "Hi"),
            FlushPolicy::default(),
            |_| true,
        )
        .unwrap();
        let session = chats
            .start_with(ConversationSettings::default(), Some(project))
            .unwrap();
        send_temp_turn(
            &ollama,
            &chats,
            ProjectContext::default(),
            None,
            request(&ollama, session, "Hi"),
            |_| true,
        )
        .unwrap();
        assert_eq!(ollama.inflight_len(), 2);

        delete_project(&storage, &ollama, &chats, project)
            .await
            .unwrap();

        assert_eq!(ollama.inflight_len(), 0);
    }

    #[tokio::test]
    async fn project_temp_chat_refused_when_storage_closed() {
        let dir = tempfile::tempdir().unwrap();
        let closed = StorageState::open(dir.path(), &MemoryKeySource::unavailable());
        let chats = TempChats::default();

        assert!(matches!(
            start_temp(
                &closed,
                &chats,
                ConversationSettings::default(),
                Some(Uuid::new_v4())
            ),
            Err(AppError::StorageUnavailable(_))
        ));
        // Outside a project a temporary chat needs no storage.
        assert!(start_temp(&closed, &chats, ConversationSettings::default(), None).is_ok());
    }

    #[tokio::test]
    async fn temp_start_in_unknown_project_is_not_found() {
        let setup = setup().await;
        assert!(matches!(
            start_temp(
                &setup.state,
                &setup.chats,
                ConversationSettings::default(),
                Some(Uuid::new_v4())
            ),
            Err(AppError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn temp_start_racing_project_delete_leaves_no_session() {
        let setup = setup().await;
        let project = project_with(&setup.storage, "Rules.");
        // The start checked the project, then the delete ran (rows and
        // sessions) before the start inserted its session.
        let deleted = delete_project(&setup.storage, &setup.ollama, &setup.chats, project)
            .await
            .unwrap();
        assert_eq!(deleted, 0);
        let session = setup
            .chats
            .start_with(ConversationSettings::default(), Some(project))
            .unwrap();

        let result = confirm_project(&setup.storage, &setup.chats, session, project);

        assert!(matches!(result, Err(AppError::NotFound(_))));
        assert!(matches!(
            setup.chats.project(session),
            Err(AppError::NotFound(_))
        ));
    }
}

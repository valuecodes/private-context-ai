//! Project CRUD. Each database call is one short statement or transaction,
//! run inline in an async command (never on the main thread).

use std::sync::Arc;

use tauri::State;
use uuid::Uuid;

use crate::error::AppError;
use crate::ollama::Ollama;
use crate::redacted::Redacted;
use crate::storage::projects::Project;
use crate::storage::{Storage, StorageState};
use crate::temporary::TempChats;

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

/// Deletes the project with its saved chats and temporary chats; returns how
/// many saved chats were deleted.
#[tauri::command]
pub async fn project_delete(
    storage: State<'_, StorageState>,
    ollama: State<'_, Arc<Ollama>>,
    chats: State<'_, Arc<TempChats>>,
    id: Uuid,
) -> Result<usize, AppError> {
    delete_project(storage.get()?, &ollama, &chats, id).await
}

/// The body of `project_delete`, without Tauri. Like `conversation_delete`
/// and a temporary chat's discard: the rows and sessions go first, then
/// their replies are cancelled. A send racing this either finds its chat
/// gone, or was admitted before and is cancelled here.
pub async fn delete_project(
    storage: &Storage,
    ollama: &Ollama,
    chats: &TempChats,
    id: Uuid,
) -> Result<usize, AppError> {
    let saved = storage.delete_project(id)?;
    let temporary = chats.discard_project(id);
    for chat in saved.iter().chain(&temporary) {
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
        let instructions = match setup.chats.project(session)? {
            Some(project) => setup.storage.project_instructions(project)?,
            None => Redacted::default(),
        };
        send_temp_turn(
            &setup.ollama,
            &setup.chats,
            instructions,
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
            Redacted::default(),
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

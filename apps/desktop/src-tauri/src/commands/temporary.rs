//! Temporary chats: streamed like saved ones, but the history stays in
//! [`TempChats`] and nothing is written to the database. Audit rows are still
//! written; they carry no chat id.

use std::sync::{Arc, Mutex};

use tauri::State;
use tauri::ipc::Channel;
use uuid::Uuid;

use crate::commands::ollama::{
    SendRequest, SendStarted, Shared, channel_sink, check_prompt, check_turn, file_runner,
    finish_turn, model_history, recording_sink, tool_follow_up, turn_tools,
};
use crate::error::AppError;
use crate::ollama::file_tools::FileRunner;
use crate::ollama::types::{ChatEvent, ThinkChoice, validate_model};
use crate::ollama::{ChatCall, ChatThink, Ollama};
use crate::redacted::Redacted;
use crate::storage::conversations::ConversationSettings;
use crate::storage::files::TURN_PASSAGES;
use crate::storage::memory::ProjectContext;
use crate::storage::messages::TurnInput;
use crate::storage::{Storage, StorageState};
use crate::temporary::{MemoryRecorder, TempChats};

/// Starts an empty temporary chat and returns its session id. Its settings
/// come with its first send, like a new saved chat's. With `project_id` it
/// is started in that project.
#[tauri::command]
pub fn temp_chat_start(
    storage: State<'_, StorageState>,
    chats: State<'_, Arc<TempChats>>,
    settings: Option<ConversationSettings>,
    project_id: Option<Uuid>,
) -> Result<Uuid, AppError> {
    start_temp(&storage, &chats, settings.unwrap_or_default(), project_id)
}

/// The body of `temp_chat_start`. A chat in a project needs storage, which
/// holds the instructions, and an existing project.
pub fn start_temp(
    storage: &StorageState,
    chats: &TempChats,
    settings: ConversationSettings,
    project_id: Option<Uuid>,
) -> Result<Uuid, AppError> {
    let Some(project) = project_id else {
        return chats.start_with(settings, None);
    };
    let storage = storage.get()?;
    storage.get_project(project)?;
    let id = chats.start_with(settings, Some(project))?;
    confirm_project(storage, chats, id, project)
}

/// Checks project `project` again once session `id` is started in it. A
/// project deleted in between had its sessions discarded before this one
/// existed, so this one is removed here instead.
pub(crate) fn confirm_project(
    storage: &Storage,
    chats: &TempChats,
    id: Uuid,
    project: Uuid,
) -> Result<Uuid, AppError> {
    if let Err(error) = storage.get_project(project) {
        chats.remove(id);
        return Err(error);
    }
    Ok(id)
}

/// What the temporary chat's project currently instructs and remembers,
/// and with Files on the passages of its files that best match the prompt
/// `input` sends; empty outside a project, which needs no storage. Searched
/// before [`TempChats::begin`] takes the session, which sends the passages
/// only if that prompt is still the one it sends.
fn session_context(
    storage: &StorageState,
    chats: &TempChats,
    session_id: Uuid,
    input: &TurnInput,
) -> Result<ProjectContext, AppError> {
    let Some(project) = chats.project(session_id)? else {
        return Ok(ProjectContext::default());
    };
    let storage = storage.get()?;
    let mut context = storage.project_context(project)?;
    if chats.settings(session_id)?.files
        && let Some(prompt) = chats.turn_prompt(session_id, input)?
    {
        context.files = storage.search(project, prompt.expose(), TURN_PASSAGES)?;
        context.files_query = prompt;
    }
    Ok(context)
}

/// Runs the file tools for the temporary chat, when its project has files.
/// Whether they are offered is decided with the Files setting
/// [`TempChats::begin`] reads under its lock.
fn session_files(
    storage: &StorageState,
    chats: &TempChats,
    session_id: Uuid,
) -> Result<Option<FileRunner>, AppError> {
    let Some(project) = chats.project(session_id)? else {
        return Ok(None);
    };
    let storage = storage.get()?;
    Ok(storage
        .project_has_files(project)?
        .then(|| file_runner(Arc::clone(storage), project)))
}

/// Applies from the temporary chat's next send.
#[tauri::command]
pub fn temp_chat_update_settings(
    chats: State<'_, Arc<TempChats>>,
    session_id: Uuid,
    settings: ConversationSettings,
) -> Result<(), AppError> {
    chats.update_settings(session_id, settings)
}

/// The session's Think setting and what it resolves to for `model`.
async fn session_think(
    ollama: &Ollama,
    chats: &TempChats,
    session_id: Uuid,
    model: &str,
) -> Result<(ThinkChoice, ChatThink), AppError> {
    let choice = chats.settings(session_id)?.think;
    let think = ollama.chat_think(model, choice.requested()).await?;
    Ok((choice, think))
}

/// Like `chat_send`, but Rust builds the history from the session in memory.
#[tauri::command]
pub async fn temp_chat_send(
    ollama: State<'_, Arc<Ollama>>,
    storage: State<'_, StorageState>,
    chats: State<'_, Arc<TempChats>>,
    session_id: Uuid,
    model: String,
    prompt: Redacted,
    on_event: Channel<ChatEvent>,
) -> Result<SendStarted, AppError> {
    let (think_choice, think) = session_think(&ollama, &chats, session_id, &model).await?;
    let input = TurnInput::New(prompt);
    send_temp_turn(
        &ollama,
        &chats,
        session_context(&storage, &chats, session_id, &input)?,
        session_files(&storage, &chats, session_id)?,
        SendRequest {
            conversation_id: session_id,
            model,
            input,
            think_choice,
            think,
        },
        channel_sink(on_event),
    )
}

/// Like `chat_regenerate`, for a temporary chat.
#[tauri::command]
pub async fn temp_chat_regenerate(
    ollama: State<'_, Arc<Ollama>>,
    storage: State<'_, StorageState>,
    chats: State<'_, Arc<TempChats>>,
    session_id: Uuid,
    model: String,
    on_event: Channel<ChatEvent>,
) -> Result<SendStarted, AppError> {
    let (think_choice, think) = session_think(&ollama, &chats, session_id, &model).await?;
    send_temp_turn(
        &ollama,
        &chats,
        session_context(&storage, &chats, session_id, &TurnInput::Regenerate)?,
        session_files(&storage, &chats, session_id)?,
        SendRequest {
            conversation_id: session_id,
            model,
            input: TurnInput::Regenerate,
            think_choice,
            think,
        },
        channel_sink(on_event),
    )
}

/// Like `chat_edit`, for a temporary chat.
#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "Tauri injects the three states; the rest is the IPC payload"
)]
pub async fn temp_chat_edit(
    ollama: State<'_, Arc<Ollama>>,
    storage: State<'_, StorageState>,
    chats: State<'_, Arc<TempChats>>,
    session_id: Uuid,
    message_id: Uuid,
    model: String,
    prompt: Redacted,
    on_event: Channel<ChatEvent>,
) -> Result<SendStarted, AppError> {
    let (think_choice, think) = session_think(&ollama, &chats, session_id, &model).await?;
    let input = TurnInput::Edit { message_id, prompt };
    send_temp_turn(
        &ollama,
        &chats,
        session_context(&storage, &chats, session_id, &input)?,
        session_files(&storage, &chats, session_id)?,
        SendRequest {
            conversation_id: session_id,
            model,
            input,
            think_choice,
            think,
        },
        channel_sink(on_event),
    )
}

#[tauri::command]
pub async fn temp_chat_discard(
    ollama: State<'_, Arc<Ollama>>,
    chats: State<'_, Arc<TempChats>>,
    session_id: Uuid,
) -> Result<(), AppError> {
    discard(&ollama, &chats, session_id).await;
    Ok(())
}

/// Forgets the chat, then cancels its reply if one runs, admitted or
/// streaming. An unknown id is a no-op, so a saved conversation's id never
/// cancels that conversation's reply.
pub async fn discard(ollama: &Ollama, chats: &TempChats, session_id: Uuid) {
    if chats.remove(session_id) {
        ollama.cancel_conversation(session_id).await;
    }
}

/// The body of the temporary send commands, without Tauri; `conversation_id`
/// is the session id. The slot is taken before the session is read, so a
/// discard either lands first (`NotFound`) or finds the slot and cancels it.
/// Nothing changes when the history is refused, and a request that does not
/// start is taken back. `context` is the chat's project's instructions and
/// memory, read just before; the chat itself is never summarized. `files`
/// runs the file tools, offered only when the Files setting `begin` read is
/// on.
pub fn send_temp_turn<S>(
    ollama: &Arc<Ollama>,
    chats: &Arc<TempChats>,
    context: ProjectContext,
    files: Option<FileRunner>,
    request: SendRequest,
    send: S,
) -> Result<SendStarted, AppError>
where
    S: Fn(ChatEvent) -> bool + Send + Sync + 'static,
{
    let SendRequest {
        conversation_id: session_id,
        model,
        input,
        think_choice,
        think,
    } = request;
    check_prompt(&input)?;
    validate_model(&model)?;
    let admission = ollama.admit(session_id)?;
    ollama.check_connection(think.connection)?;
    let turn = chats.begin(
        session_id,
        &context,
        ollama.max_prompt_bytes(),
        &input,
        |history, settings| check_turn(ollama, &model, &think_choice, history, settings),
    )?;

    let assistant_id = Uuid::new_v4();
    let revision = !matches!(input, TurnInput::New(_));
    let recorder: Shared<MemoryRecorder> = Arc::new(Mutex::new(MemoryRecorder::new(
        chats,
        session_id,
        assistant_id,
        revision,
    )));
    let sink = recording_sink(Arc::clone(&recorder), send);
    let tools = turn_tools(&think, turn.charts, turn.files && files.is_some());
    let history = model_history(turn.history, think.tools);
    let follow_up = tool_follow_up(&recorder, &history, tools, files);
    let request_id = ollama
        .start(
            admission,
            ChatCall {
                model: &model,
                messages: &history,
                options: turn.options.as_ref(),
                think,
                keep_alive: None,
                tools,
            },
            sink,
            follow_up,
            move |end| {
                finish_turn(&recorder, end);
            },
        )
        .inspect_err(|_| {
            // A regenerate or edit was already undone by its recorder,
            // which `start` ends on every path.
            if !revision {
                chats.rollback(session_id);
            }
        })?;
    Ok(SendStarted {
        request_id,
        user_message_id: turn.user_id,
        assistant_message_id: assistant_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::Audit;
    use crate::commands::testing::{no_think, silent_ollama, silent_ollama_with};
    use crate::storage::testing::{count_rows, temp_storage};

    fn send_input(
        ollama: &Arc<Ollama>,
        chats: &Arc<TempChats>,
        session: Uuid,
        input: TurnInput,
    ) -> Result<SendStarted, AppError> {
        send_temp_turn(
            ollama,
            chats,
            ProjectContext::default(),
            None,
            SendRequest {
                conversation_id: session,
                model: "llama3.2".into(),
                input,
                think_choice: ThinkChoice::Off,
                think: no_think(ollama),
            },
            |_| true,
        )
    }

    fn send(
        ollama: &Arc<Ollama>,
        chats: &Arc<TempChats>,
        session: Uuid,
        prompt: &str,
    ) -> Result<SendStarted, AppError> {
        send_input(
            ollama,
            chats,
            session,
            TurnInput::New(Redacted::new(prompt)),
        )
    }

    fn prompts(chats: &TempChats, session: Uuid) -> Vec<String> {
        chats
            .history(session)
            .unwrap()
            .iter()
            .map(|message| message.content.expose().to_owned())
            .collect()
    }

    #[tokio::test]
    async fn temp_send_writes_no_rows() {
        let (storage, _dir) = temp_storage();
        let (ollama, _listener) =
            silent_ollama_with(Arc::new(Audit::over(Arc::clone(&storage)))).await;
        let chats = Arc::new(TempChats::default());
        let session = chats.start().unwrap();

        for prompt in ["One", "Two", "Three"] {
            send(&ollama, &chats, session, prompt).unwrap();
            ollama.cancel_conversation(session).await;
        }

        assert_eq!(count_rows(&storage, "conversations"), 0);
        assert_eq!(count_rows(&storage, "messages"), 0);
        let chats_audited = storage
            .list_audit(100, None)
            .unwrap()
            .into_iter()
            .filter(|event| event.kind == "ollama.chat")
            .count();
        assert_eq!(chats_audited, 3);
        assert_eq!(prompts(&chats, session), ["One", "Two", "Three"]);
    }

    #[test]
    fn session_context_searches_project_files() {
        use crate::storage::key::testing::MemoryKeySource;

        let dir = tempfile::tempdir().unwrap();
        let state = StorageState::open(dir.path(), &MemoryKeySource::default());
        let storage = state.get().unwrap();
        let project = storage.create_project("p").unwrap().id;
        storage
            .add_file(
                project,
                &Redacted::new("a.txt"),
                &Redacted::new("the word is plover"),
            )
            .unwrap();
        let chats = TempChats::default();
        let id = start_temp(
            &state,
            &chats,
            ConversationSettings::default(),
            Some(project),
        )
        .unwrap();
        let ask = TurnInput::New(Redacted::new("word?"));

        let context = session_context(&state, &chats, id, &ask).unwrap();
        assert_eq!(context.files.len(), 1);
        assert_eq!(context.files_query.expose(), "word?");
        // Nothing to regenerate yet: nothing searched.
        let regenerate = session_context(&state, &chats, id, &TurnInput::Regenerate).unwrap();
        assert!(regenerate.files.is_empty());

        chats
            .update_settings(
                id,
                ConversationSettings {
                    files: false,
                    ..ConversationSettings::default()
                },
            )
            .unwrap();
        assert!(
            session_context(&state, &chats, id, &ask)
                .unwrap()
                .files
                .is_empty()
        );
    }

    #[tokio::test]
    async fn temp_files_setting_read_in_begin_decides_file_tools() {
        use crate::ollama::client::OllamaBaseUrl;
        use crate::ollama::file_tools::FILE_TOOLS;
        use crate::ollama::testing::{Reply, fake_ollama};
        use crate::storage::key::testing::MemoryKeySource;

        let fake = fake_ollama(Reply::Lines(
            vec![r#"{"message":{"role":"assistant","content":"Hi"},"done":true}"#],
            std::time::Duration::ZERO,
        ))
        .await;
        let ollama = Arc::new(Ollama::new(
            OllamaBaseUrl::parse(&format!("http://{}", fake.addr)),
            Arc::new(Audit::new()),
        ));
        let dir = tempfile::tempdir().unwrap();
        let state = StorageState::open(dir.path(), &MemoryKeySource::default());
        let storage = state.get().unwrap();
        let project = storage.create_project("p").unwrap().id;
        storage
            .add_file(project, &Redacted::new("a.txt"), &Redacted::new("plover"))
            .unwrap();
        let chats = Arc::new(TempChats::default());
        let session = start_temp(
            &state,
            &chats,
            ConversationSettings::default(),
            Some(project),
        )
        .unwrap();
        let send = |files: Option<FileRunner>| {
            send_temp_turn(
                &ollama,
                &chats,
                ProjectContext::default(),
                files,
                SendRequest {
                    conversation_id: session,
                    model: "qwen3".into(),
                    input: TurnInput::New(Redacted::new("word?")),
                    think_choice: ThinkChoice::Off,
                    think: ChatThink {
                        tools: true,
                        ..no_think(&ollama)
                    },
                },
                |_| true,
            )
            .unwrap();
        };

        send(session_files(&state, &chats, session).unwrap());
        ollama.wait_idle().await;
        // Files turned off after the runner was built, before `begin`.
        let files = session_files(&state, &chats, session).unwrap();
        assert!(files.is_some());
        chats
            .update_settings(
                session,
                ConversationSettings {
                    files: false,
                    ..ConversationSettings::default()
                },
            )
            .unwrap();
        send(files);
        ollama.wait_idle().await;

        let bodies = fake.bodies.lock().unwrap().clone();
        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies[0]["tools"], *FILE_TOOLS);
        assert!(bodies[1].get("tools").is_none());
    }

    #[tokio::test]
    async fn refused_temp_send_leaves_session_unchanged() {
        let (ollama, _listener) = silent_ollama().await;
        let chats = Arc::new(TempChats::default());
        let session = chats.start().unwrap();

        let err = send(&ollama, &chats, session, &"x".repeat(2 * 1024 * 1024)).unwrap_err();

        assert!(matches!(err, AppError::TooLarge(_)));
        assert!(prompts(&chats, session).is_empty());
        assert_eq!(ollama.inflight_len(), 0);
    }

    #[tokio::test]
    async fn failed_start_rolls_back_prompt() {
        let audit = Arc::new(Audit::new());
        audit.close();
        let (ollama, _listener) = silent_ollama_with(audit).await;
        let chats = Arc::new(TempChats::default());
        let session = chats.start().unwrap();

        let err = send(&ollama, &chats, session, "Hi").unwrap_err();

        assert!(matches!(err, AppError::StorageUnavailable(_)));
        assert!(prompts(&chats, session).is_empty());
        assert_eq!(ollama.inflight_len(), 0);
    }

    #[tokio::test]
    async fn failed_start_restores_revision() {
        let (ollama, _listener) = silent_ollama().await;
        let chats = Arc::new(TempChats::default());
        let session = chats.start().unwrap();
        // Two prompts whose replies failed: the session holds only the
        // prompts.
        let first = send(&ollama, &chats, session, "One").unwrap();
        ollama.cancel_conversation(session).await;
        send(&ollama, &chats, session, "Two").unwrap();
        ollama.cancel_conversation(session).await;
        let audit = Arc::new(Audit::new());
        audit.close();
        let (closed, _listener) = silent_ollama_with(audit).await;

        let edit = TurnInput::Edit {
            message_id: first.user_message_id,
            prompt: Redacted::new("Changed"),
        };
        for input in [TurnInput::Regenerate, edit] {
            assert!(send_input(&closed, &chats, session, input).is_err());
            assert_eq!(prompts(&chats, session), ["One", "Two"]);
        }
    }

    #[tokio::test]
    async fn discard_during_preflight_refuses_send() {
        let (ollama, _listener) = silent_ollama().await;
        let chats = Arc::new(TempChats::default());
        let session = chats.start().unwrap();
        // `temp_chat_send` awaits the capabilities lookup before the send;
        // the discard lands there.
        discard(&ollama, &chats, session).await;

        assert!(matches!(
            send(&ollama, &chats, session, "Hi"),
            Err(AppError::NotFound(_))
        ));
        assert_eq!(ollama.inflight_len(), 0);
    }

    #[tokio::test]
    async fn discard_after_admit_cancels_stream() {
        let (ollama, _listener) = silent_ollama().await;
        let chats = Arc::new(TempChats::default());
        let session = chats.start().unwrap();
        send(&ollama, &chats, session, "Hi").unwrap();
        assert_eq!(ollama.inflight_len(), 1);

        discard(&ollama, &chats, session).await;

        assert_eq!(ollama.inflight_len(), 0);
        assert!(chats.history(session).is_none());
    }

    #[tokio::test]
    async fn discard_unknown_id_cancels_nothing() {
        let (ollama, _listener) = silent_ollama().await;
        let chats = Arc::new(TempChats::default());
        // A saved conversation's reply, running under its own id.
        let conversation = Uuid::new_v4();
        let _admission = ollama.admit(conversation).unwrap();

        discard(&ollama, &chats, conversation).await;

        assert_eq!(ollama.inflight_len(), 1);
    }

    #[tokio::test]
    async fn busy_per_session() {
        let (ollama, _listener) = silent_ollama().await;
        let chats = Arc::new(TempChats::default());
        let session = chats.start().unwrap();
        send(&ollama, &chats, session, "One").unwrap();

        assert!(matches!(
            send(&ollama, &chats, session, "Two"),
            Err(AppError::Busy(_))
        ));
        assert_eq!(prompts(&chats, session), ["One"]);
        // Another temporary chat is not blocked by this one.
        let other = chats.start().unwrap();
        send(&ollama, &chats, other, "Three").unwrap();
        ollama.cancel_conversation(session).await;
        ollama.cancel_conversation(other).await;
    }
}

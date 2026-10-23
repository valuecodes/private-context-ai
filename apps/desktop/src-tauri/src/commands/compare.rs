//! Comparing models: one prompt answered by two or three models, one after
//! another, and the answer the user picks kept.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::commands::ollama::{
    SharedRecorder, check_turn, finish_turn, lock, recording_sink, spawn_ticker,
};
use crate::error::AppError;
use crate::ollama::chart_tool::without_tools;
use crate::ollama::types::{ChatEvent, ThinkChoice, validate_model};
use crate::ollama::{Admission, ChatCall, ConnectionId, Ollama};
use crate::redacted::Redacted;
use crate::storage::compare::CompareTurn;
use crate::storage::messages::{FlushPolicy, MessageStatus, TurnRecorder};
use crate::storage::{Storage, StorageState};

const MIN_MODELS: usize = 2;
const MAX_MODELS: usize = 3;

/// One candidate reply; mirrored in `CompareStarted` in `src/lib/types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareCandidate {
    pub message_id: Uuid,
    pub model: String,
}

/// What `chat_compare` returns once Rust admitted it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareStarted {
    /// Cancels the whole comparison: the running model and the rest.
    pub request_id: Uuid,
    pub user_message_id: Uuid,
    pub compare_group: Uuid,
    /// In run order.
    pub candidates: Vec<CompareCandidate>,
}

/// Streamed by `chat_compare`; mirrored by `CompareEvent` in
/// `src/lib/types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CompareEvent {
    /// An event of one candidate's reply.
    Candidate { message_id: Uuid, event: ChatEvent },
    /// Every candidate has ended, and the slot is free again.
    Finished,
}

pub struct CompareRequest {
    pub conversation_id: Uuid,
    pub prompt: Redacted,
    pub models: Vec<String>,
}

fn check_request(request: &CompareRequest) -> Result<(), AppError> {
    if request.prompt.expose().trim().is_empty() {
        return Err(AppError::InvalidInput("the message is empty".into()));
    }
    let count = request.models.len();
    if !(MIN_MODELS..=MAX_MODELS).contains(&count) {
        return Err(AppError::InvalidInput(format!(
            "compare {MIN_MODELS} to {MAX_MODELS} models"
        )));
    }
    for model in &request.models {
        validate_model(model)?;
    }
    let distinct: HashSet<&String> = request.models.iter().collect();
    if distinct.len() != count {
        return Err(AppError::InvalidInput("compare different models".into()));
    }
    Ok(())
}

/// Saves `prompt` with one pending reply per model and starts answering it
/// with each model in turn, never two at once. Every model but the last is
/// unloaded after its answer. Returns the new rows' ids at once.
#[tauri::command]
pub async fn chat_compare(
    ollama: State<'_, Arc<Ollama>>,
    storage: State<'_, StorageState>,
    conversation_id: Uuid,
    prompt: Redacted,
    models: Vec<String>,
    on_event: Channel<CompareEvent>,
) -> Result<CompareStarted, AppError> {
    let storage = Arc::clone(storage.get()?);
    compare_turn(
        &ollama,
        storage,
        CompareRequest {
            conversation_id,
            prompt,
            models,
        },
        FlushPolicy::default(),
        move |event| on_event.send(event).is_ok(),
    )
}

/// Keeps the complete candidate `message_id` and drops the others; the chat
/// continues with its model.
#[tauri::command]
pub fn chat_compare_pick(
    storage: State<'_, StorageState>,
    conversation_id: Uuid,
    message_id: Uuid,
) -> Result<(), AppError> {
    storage.get()?.compare_pick(conversation_id, message_id)?;
    Ok(())
}

/// Deletes the compared prompt and every candidate.
#[tauri::command]
pub fn chat_compare_discard(
    storage: State<'_, StorageState>,
    conversation_id: Uuid,
    group: Uuid,
) -> Result<(), AppError> {
    storage.get()?.compare_discard(conversation_id, group)
}

/// The body of `chat_compare`, without Tauri. As for a send, the slot is
/// taken before anything is written and a refused history writes nothing.
/// The slot is held until every candidate has ended, so the conversation
/// stays busy for the whole comparison. Must be called inside a tokio
/// runtime.
pub fn compare_turn<S>(
    ollama: &Arc<Ollama>,
    storage: Arc<Storage>,
    request: CompareRequest,
    policy: FlushPolicy,
    send: S,
) -> Result<CompareStarted, AppError>
where
    S: Fn(CompareEvent) -> bool + Send + Sync + 'static,
{
    check_request(&request)?;
    let CompareRequest {
        conversation_id,
        prompt,
        models,
    } = request;
    let choice = storage.conversation_settings(conversation_id)?.think;
    let admission = ollama.admit(conversation_id)?;
    // Every candidate is answered by the Ollama current now.
    let connection = ollama.connection_id();
    let turn = storage.begin_compare(conversation_id, &prompt, &models, |history, settings| {
        for model in &models {
            check_turn(ollama, model, &choice, history, settings)?;
        }
        Ok(())
    })?;
    let started = CompareStarted {
        request_id: admission.request_id(),
        user_message_id: turn.user_id,
        compare_group: turn.group,
        candidates: turn
            .candidates
            .iter()
            .map(|(message_id, model)| CompareCandidate {
                message_id: *message_id,
                model: model.clone(),
            })
            .collect(),
    };
    tokio::spawn(run_candidates(
        Arc::clone(ollama),
        storage,
        Run {
            admission,
            turn,
            choice,
            connection,
            policy,
        },
        send,
    ));
    Ok(started)
}

/// What the comparison's worker owns.
struct Run {
    admission: Admission,
    turn: CompareTurn,
    choice: ThinkChoice,
    connection: ConnectionId,
    policy: FlushPolicy,
}

/// Answers the candidates in order. Think is resolved per model; a model
/// that refuses it, or a failed request, fails only its own candidate.
/// After a cancel, or once the webview is gone, the remaining candidates
/// end as `error` without a request.
async fn run_candidates<S>(ollama: Arc<Ollama>, storage: Arc<Storage>, run: Run, send: S)
where
    S: Fn(CompareEvent) -> bool + Send + Sync + 'static,
{
    let Run {
        admission,
        turn,
        choice,
        connection,
        policy,
    } = run;
    let send = Arc::new(send);
    let gone = Arc::new(AtomicBool::new(false));
    // Candidates are never offered tools: earlier charts go as plain text.
    let history = without_tools(&turn.history);
    let last = turn.candidates.len().saturating_sub(1);
    for (index, (message_id, model)) in turn.candidates.iter().enumerate() {
        let recorder: SharedRecorder = Arc::new(Mutex::new(TurnRecorder::new(
            Arc::clone(&storage),
            *message_id,
            policy,
        )));
        if admission.is_cancelled() || gone.load(Ordering::SeqCst) {
            if let Err(error) = lock(&recorder).finish(MessageStatus::Error) {
                tracing::warn!(kind = error.kind(), "could not save a skipped answer");
            }
            continue;
        }
        let candidate = {
            let (send, gone, message_id) = (Arc::clone(&send), Arc::clone(&gone), *message_id);
            move |event| {
                let delivered = send(CompareEvent::Candidate { message_id, event });
                if !delivered {
                    gone.store(true, Ordering::SeqCst);
                }
                delivered
            }
        };
        let sink = recording_sink(Arc::clone(&recorder), candidate);
        // A Stop during the lookup ends it at once, so the slot is free
        // within `chat_cancel`'s wait.
        let lookup = tokio::select! {
            () = admission.cancelled() => None,
            think = ollama.chat_think(model, choice.requested()) => Some(think),
        };
        let Some(lookup) = lookup else {
            if let Err(error) = lock(&recorder).finish(MessageStatus::Cancelled) {
                tracing::warn!(kind = error.kind(), "could not save a stopped answer");
            }
            continue;
        };
        // The connection is checked after the lookup: its ids only grow, so
        // a `think` resolved on any other connection fails here.
        let think = match lookup {
            Ok(think) => ollama.check_connection(connection).map(|()| think),
            Err(error) => Err(error),
        };
        let think = match think {
            Ok(think) => think,
            Err(error) => {
                // Saves the candidate as `error`, then shows why.
                sink(ChatEvent::Error { error });
                continue;
            }
        };
        let stop_ticker = CancellationToken::new();
        spawn_ticker(Arc::clone(&recorder), policy.every, stop_ticker.clone());
        let call = ChatCall {
            model,
            messages: &history,
            options: turn.options.as_ref(),
            think,
            // Only one model sits in memory: each but the last unloads.
            keep_alive: (index != last).then_some(0),
            // Compared answers stay tool-free.
            tools: false,
        };
        ollama
            .run(&admission, call, &sink, |end| {
                stop_ticker.cancel();
                finish_turn(&recorder, end);
            })
            .await;
    }
    // Every candidate ends with a final status, even one whose own final
    // write failed, so the group can be picked or discarded.
    if let Err(error) = storage.settle_compare(turn.group) {
        tracing::warn!(kind = error.kind(), "could not settle the comparison");
    }
    // Free the slot first, so a send right after `Finished` is admitted.
    drop(admission);
    send(CompareEvent::Finished);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::audit::{Audit, AuditStatus};
    use crate::ollama::client::OllamaBaseUrl;
    use crate::ollama::testing::{Fake, Reply, fake_ollama};
    use crate::storage::messages::StoredMessage;
    use crate::storage::testing::{count_rows, temp_storage};

    const PLAIN: &str = r#"{"capabilities":["completion"]}"#;
    /// A model that cannot turn thinking off.
    const ALWAYS_THINKS: &str = r#"{"capabilities":["completion","thinking"],
        "thinking":{"values":["low","medium","high"],"default":"medium"}}"#;
    const DONE: &str = r#"{"message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":3,"eval_count":2}"#;

    fn token_line(model: &str) -> &'static str {
        match model {
            "a" => r#"{"message":{"role":"assistant","content":"From a"},"done":false}"#,
            "b" => r#"{"message":{"role":"assistant","content":"From b"},"done":false}"#,
            _ => r#"{"message":{"role":"assistant","content":"From c"},"done":false}"#,
        }
    }

    /// `/api/show` answers by model; `/api/chat` streams `From <model>`
    /// after `delay` per line, or never answers when `silent`.
    fn ollama_reply(delay: Duration, silent: bool) -> Reply {
        Reply::Route(Arc::new(move |path, body| {
            let model = body["model"].as_str().unwrap_or_default();
            if path == "/api/show" {
                return match model {
                    "thinker" => Reply::Json(ALWAYS_THINKS),
                    "missing" => Reply::Status(404, r#"{"error":"model not found"}"#),
                    "hanging" => Reply::Silent,
                    _ => Reply::Json(PLAIN),
                };
            }
            if silent {
                return Reply::Silent;
            }
            Reply::Lines(vec![token_line(model), DONE], delay)
        }))
    }

    struct Setup {
        fake: Fake,
        ollama: Arc<Ollama>,
        storage: Arc<Storage>,
        _dir: tempfile::TempDir,
        conversation: Uuid,
    }

    async fn setup(reply: Reply) -> Setup {
        let fake = fake_ollama(reply).await;
        let (storage, dir) = temp_storage();
        let base = OllamaBaseUrl::parse(&format!("http://{}", fake.addr));
        let ollama = Arc::new(Ollama::new(
            base,
            Arc::new(Audit::over(Arc::clone(&storage))),
        ));
        let conversation = storage.create_conversation(None, "m").unwrap().id;
        Setup {
            fake,
            ollama,
            storage,
            _dir: dir,
            conversation,
        }
    }

    type Events = Arc<Mutex<Vec<CompareEvent>>>;

    fn collector() -> (
        Events,
        impl Fn(CompareEvent) -> bool + Send + Sync + 'static,
    ) {
        let events: Events = Arc::default();
        let store = Arc::clone(&events);
        (events, move |event| {
            store.lock().unwrap().push(event);
            true
        })
    }

    fn compare(
        setup: &Setup,
        models: &[&str],
        send: impl Fn(CompareEvent) -> bool + Send + Sync + 'static,
    ) -> Result<CompareStarted, AppError> {
        compare_turn(
            &setup.ollama,
            Arc::clone(&setup.storage),
            CompareRequest {
                conversation_id: setup.conversation,
                prompt: Redacted::new("Hi"),
                models: models.iter().map(|model| (*model).to_owned()).collect(),
            },
            FlushPolicy::default(),
            send,
        )
    }

    async fn wait_finished(events: &Events) {
        for _ in 0..500 {
            if events.lock().unwrap().last() == Some(&CompareEvent::Finished) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the comparison never finished");
    }

    fn rows(setup: &Setup) -> Vec<StoredMessage> {
        setup
            .storage
            .get_conversation(setup.conversation)
            .unwrap()
            .messages
    }

    fn chat_bodies(fake: &Fake) -> Vec<serde_json::Value> {
        fake.bodies
            .lock()
            .unwrap()
            .iter()
            .filter(|body| body.get("messages").is_some())
            .cloned()
            .collect()
    }

    #[tokio::test]
    async fn compare_runs_models_sequentially() {
        let setup = setup(ollama_reply(Duration::from_millis(50), false)).await;
        let (events, send) = collector();

        let started = compare(&setup, &["a", "b"], send).unwrap();
        wait_finished(&events).await;

        let chats: Vec<String> = setup
            .fake
            .log
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.contains("/api/chat"))
            .cloned()
            .collect();
        // The second request arrives only after the first answer's last line.
        assert_eq!(
            chats,
            [
                "> /api/chat a",
                "< /api/chat a",
                "> /api/chat b",
                "< /api/chat b"
            ]
        );
        let saved = rows(&setup);
        let candidates: Vec<(Uuid, &str, MessageStatus, Option<&str>)> = saved[1..]
            .iter()
            .map(|row| {
                (
                    row.id,
                    row.content.expose(),
                    row.status,
                    row.model.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            candidates,
            [
                (
                    started.candidates[0].message_id,
                    "From a",
                    MessageStatus::Complete,
                    Some("a")
                ),
                (
                    started.candidates[1].message_id,
                    "From b",
                    MessageStatus::Complete,
                    Some("b")
                ),
            ]
        );
        assert!(
            saved
                .iter()
                .all(|row| row.compare_group == Some(started.compare_group))
        );
        // Each event names its candidate.
        let events = events.lock().unwrap();
        assert!(events.contains(&CompareEvent::Candidate {
            message_id: started.candidates[1].message_id,
            event: ChatEvent::Token {
                content: Redacted::new("From b")
            },
        }));
        assert_eq!(setup.ollama.inflight_len(), 0);
    }

    #[tokio::test]
    async fn compare_keep_alive_zero_except_last() {
        let setup = setup(ollama_reply(Duration::ZERO, false)).await;
        let (events, send) = collector();

        compare(&setup, &["a", "b", "c"], send).unwrap();
        wait_finished(&events).await;

        let keep_alive: Vec<Option<serde_json::Value>> = chat_bodies(&setup.fake)
            .iter()
            .map(|body| body.get("keep_alive").cloned())
            .collect();
        assert_eq!(
            keep_alive,
            [Some(serde_json::json!(0)), Some(serde_json::json!(0)), None]
        );
        for body in chat_bodies(&setup.fake) {
            assert!(body.get("tools").is_none() && body.get("format").is_none());
            assert!(body.get("think").is_none(), "no thinking capability");
        }
    }

    #[tokio::test]
    async fn plain_send_has_no_keep_alive() {
        use crate::commands::ollama::{SendRequest, send_turn};
        use crate::storage::messages::TurnInput;

        let setup = setup(ollama_reply(Duration::ZERO, false)).await;
        let think = setup.ollama.chat_think("a", None).await.unwrap();
        send_turn(
            &setup.ollama,
            Arc::clone(&setup.storage),
            SendRequest {
                conversation_id: setup.conversation,
                model: "a".into(),
                input: TurnInput::New(Redacted::new("Hi")),
                think_choice: ThinkChoice::Off,
                think,
            },
            FlushPolicy::default(),
            |_| true,
        )
        .unwrap();
        for _ in 0..500 {
            if setup.ollama.inflight_len() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let bodies = chat_bodies(&setup.fake);
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].get("keep_alive").is_none());
        assert_eq!(rows(&setup)[1].model.as_deref(), Some("a"));
    }

    #[tokio::test]
    async fn send_offers_tools_only_with_charts_and_capable_model() {
        use crate::commands::ollama::{SendRequest, send_turn};
        use crate::storage::conversations::ConversationSettings;
        use crate::storage::messages::TurnInput;

        let route = Reply::Route(Arc::new(|path, body| {
            if path == "/api/show" {
                return if body["model"] == "tooled" {
                    Reply::Json(r#"{"capabilities":["completion","tools"]}"#)
                } else {
                    Reply::Json(PLAIN)
                };
            }
            Reply::Lines(vec![token_line("a"), DONE], Duration::ZERO)
        }));
        for (charts, model, offered) in [
            (true, "tooled", true),
            (false, "tooled", false),
            (true, "a", false),
        ] {
            let setup = setup(route.clone()).await;
            setup
                .storage
                .update_settings(
                    setup.conversation,
                    &ConversationSettings {
                        charts,
                        ..ConversationSettings::default()
                    },
                )
                .unwrap();
            let think = setup.ollama.chat_think(model, None).await.unwrap();
            send_turn(
                &setup.ollama,
                Arc::clone(&setup.storage),
                SendRequest {
                    conversation_id: setup.conversation,
                    model: model.into(),
                    input: TurnInput::New(Redacted::new("Hi")),
                    think_choice: ThinkChoice::Off,
                    think,
                },
                FlushPolicy::default(),
                |_| true,
            )
            .unwrap();
            for _ in 0..500 {
                if setup.ollama.inflight_len() == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let bodies = chat_bodies(&setup.fake);
            assert_eq!(bodies.len(), 1);
            assert_eq!(
                bodies[0].get("tools").is_some(),
                offered,
                "{charts} {model}"
            );
            assert!(bodies[0].get("format").is_none());
        }
    }

    #[tokio::test]
    async fn compare_think_resolved_per_model() {
        let setup = setup(ollama_reply(Duration::ZERO, false)).await;
        let (events, send) = collector();

        // The chat's Think is off. "missing" fails its `/api/show` lookup;
        // "thinker" always thinks, so it gets its default level.
        let started = compare(&setup, &["missing", "thinker", "a"], send).unwrap();
        wait_finished(&events).await;

        let statuses: Vec<MessageStatus> = rows(&setup)[1..].iter().map(|row| row.status).collect();
        assert_eq!(
            statuses,
            [
                MessageStatus::Error,
                MessageStatus::Complete,
                MessageStatus::Complete
            ]
        );
        let bodies = chat_bodies(&setup.fake);
        let thinks: Vec<(&str, Option<&serde_json::Value>)> = bodies
            .iter()
            .map(|body| (body["model"].as_str().unwrap(), body.get("think")))
            .collect();
        assert_eq!(
            thinks,
            [("thinker", Some(&serde_json::json!("medium"))), ("a", None)]
        );
        assert!(events.lock().unwrap().iter().any(|event| matches!(
            event,
            CompareEvent::Candidate {
                message_id,
                event: ChatEvent::Error { .. },
            } if *message_id == started.candidates[0].message_id
        )));
    }

    #[tokio::test]
    async fn compare_cancel_skips_remaining() {
        let setup = setup(ollama_reply(Duration::ZERO, true)).await;
        let (events, send) = collector();

        let started = compare(&setup, &["a", "b", "c"], send).unwrap();
        // Wait until the first request is on the wire.
        for _ in 0..500 {
            if !chat_bodies(&setup.fake).is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        setup.ollama.chat_cancel(started.request_id).await.unwrap();
        wait_finished(&events).await;

        let statuses: Vec<MessageStatus> = rows(&setup)[1..].iter().map(|row| row.status).collect();
        assert_eq!(
            statuses,
            [
                MessageStatus::Cancelled,
                MessageStatus::Error,
                MessageStatus::Error
            ]
        );
        assert_eq!(chat_bodies(&setup.fake).len(), 1);
        assert_eq!(setup.ollama.inflight_len(), 0);
    }

    #[tokio::test]
    async fn compare_cancel_during_lookup_frees_slot() {
        let setup = setup(ollama_reply(Duration::ZERO, false)).await;
        let (events, send) = collector();

        let started = compare(&setup, &["hanging", "a"], send).unwrap();
        for _ in 0..500 {
            if !setup.fake.log.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // Returns once the slot is free, well before the lookup's timeout.
        setup.ollama.chat_cancel(started.request_id).await.unwrap();
        assert_eq!(setup.ollama.inflight_len(), 0);
        wait_finished(&events).await;

        let statuses: Vec<MessageStatus> = rows(&setup)[1..].iter().map(|row| row.status).collect();
        assert_eq!(statuses, [MessageStatus::Cancelled, MessageStatus::Error]);
        assert!(chat_bodies(&setup.fake).is_empty());
    }

    #[tokio::test]
    async fn compare_holds_the_slot_until_finished() {
        let setup = setup(ollama_reply(Duration::from_millis(50), false)).await;
        let (events, send) = collector();

        compare(&setup, &["a", "b"], send).unwrap();
        // Busy in Ollama's slots as well as in storage.
        assert!(matches!(
            setup.ollama.admit(setup.conversation),
            Err(AppError::Busy(_))
        ));
        wait_finished(&events).await;
        assert!(setup.ollama.admit(setup.conversation).is_ok());
    }

    #[tokio::test]
    async fn compare_audits_each_request() {
        let setup = setup(ollama_reply(Duration::ZERO, false)).await;
        let (events, send) = collector();

        compare(&setup, &["a", "b"], send).unwrap();
        wait_finished(&events).await;

        let chats: Vec<(Option<String>, AuditStatus)> = setup
            .storage
            .list_audit(usize::MAX, None)
            .unwrap()
            .into_iter()
            .filter(|event| event.kind == "ollama.chat")
            .map(|event| (event.model, event.status))
            .collect();
        // Newest first.
        assert_eq!(
            chats,
            [
                (Some("b".to_owned()), AuditStatus::Ok),
                (Some("a".to_owned()), AuditStatus::Ok)
            ]
        );
    }

    #[tokio::test]
    async fn compare_rejects_bad_requests() {
        let setup = setup(ollama_reply(Duration::ZERO, true)).await;
        for models in [
            &["a"][..],
            &["a", "b", "c", "d"][..],
            &["a", "a"][..],
            &["a", " "][..],
        ] {
            assert!(matches!(
                compare(&setup, models, |_| true),
                Err(AppError::InvalidInput(_))
            ));
        }
        let err = compare_turn(
            &setup.ollama,
            Arc::clone(&setup.storage),
            CompareRequest {
                conversation_id: setup.conversation,
                prompt: Redacted::new("  "),
                models: vec!["a".into(), "b".into()],
            },
            FlushPolicy::default(),
            |_| true,
        )
        .unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
        assert_eq!(count_rows(&setup.storage, "messages"), 0);
        assert_eq!(setup.ollama.inflight_len(), 0);
    }

    #[tokio::test]
    async fn compare_too_large_writes_nothing() {
        let setup = setup(ollama_reply(Duration::ZERO, true)).await;
        let err = compare_turn(
            &setup.ollama,
            Arc::clone(&setup.storage),
            CompareRequest {
                conversation_id: setup.conversation,
                prompt: Redacted::new("x".repeat(2 * 1024 * 1024)),
                models: vec!["a".into(), "b".into()],
            },
            FlushPolicy::default(),
            |_| true,
        )
        .unwrap_err();
        assert!(matches!(err, AppError::TooLarge(_)));
        assert_eq!(count_rows(&setup.storage, "messages"), 0);
        assert_eq!(setup.ollama.inflight_len(), 0);
    }

    #[test]
    fn compare_event_serializes_with_candidate_id() {
        let id = Uuid::nil();
        let value = serde_json::to_value(CompareEvent::Candidate {
            message_id: id,
            event: ChatEvent::Token {
                content: Redacted::new("x"),
            },
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "type": "candidate",
                "messageId": id.to_string(),
                "event": { "type": "token", "content": "x" }
            })
        );
        assert_eq!(
            serde_json::to_value(CompareEvent::Finished).unwrap(),
            serde_json::json!({ "type": "finished" })
        );
    }
}

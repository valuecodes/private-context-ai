//! The request log carries counts and durations, never message content.
//!
//! A separate test binary, so this is the only test in its process: `tracing`
//! caches callsite interest and the max level globally, and a scoped
//! subscriber in a unit test can miss events other test threads registered.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use private_context_ai_lib::audit::Audit;
use private_context_ai_lib::observability;
use private_context_ai_lib::ollama::chart_tool::ChartEntry;
use private_context_ai_lib::ollama::client::OllamaBaseUrl;
use private_context_ai_lib::ollama::types::{ChatMessage, ChatRole};
use private_context_ai_lib::ollama::{ChatCall, ChatThink, Ollama};
use private_context_ai_lib::redacted::Redacted;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uuid::Uuid;

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

const REPLY: &str = concat!(
    "HTTP/1.1 200 OK\r\ncontent-type: application/x-ndjson\r\nconnection: close\r\n\r\n",
    r#"{"message":{"role":"assistant","content":"secret-reply"},"done":false}"#,
    "\n",
    r#"{"message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":3,"eval_count":2}"#,
    "\n",
);

/// Answers one chat request with a fixed stream, after reading it whole.
async fn fake_ollama() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = socket.read(&mut buf).await.unwrap();
            request.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
            if let Some(end) = text.find("\r\n\r\n") {
                let length: usize = text
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .map_or(0, |value| value.trim().parse().unwrap());
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        socket.write_all(REPLY.as_bytes()).await.unwrap();
        socket.shutdown().await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn request_log_has_counts_and_no_content() {
    let captured = Captured::default();
    let writer = captured.clone();
    let (subscriber, _control) = observability::subscriber(move || writer.clone(), None);
    // The default runtime runs every task on this thread, so the worker logs
    // through this subscriber too.
    let _default = tracing::subscriber::set_default(subscriber);

    let base = OllamaBaseUrl::parse(&fake_ollama().await);
    let ollama = Arc::new(Ollama::new(base, Arc::new(Audit::new())));
    let messages = vec![ChatMessage::new(
        ChatRole::User,
        Redacted::new("secret-prompt"),
    )];
    let admission = ollama.admit(Uuid::new_v4()).unwrap();
    // A model that cannot think, on the current connection.
    let think = ChatThink {
        think: None,
        connection: ollama.connection_id(),
        tools: false,
    };
    ollama
        .start(
            admission,
            ChatCall {
                model: "llama3.2",
                messages: &messages,
                options: None,
                think,
                keep_alive: None,
                tools: false,
            },
            |_| true,
            |_: &[ChartEntry]| Vec::new(),
            |_| {},
        )
        .unwrap();

    let mut text = String::new();
    for _ in 0..500 {
        text = captured.text();
        if text.contains("request finished") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let line = text
        .lines()
        .find(|line| line.contains("request finished"))
        .unwrap_or_else(|| panic!("no request line in {text}"));
    let json: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(json["span"]["name"], "ollama_request");
    assert_eq!(json["span"]["kind"], "ollama.chat");
    assert_eq!(json["fields"]["status"], "ok");
    assert_eq!(json["fields"]["prompt_tokens"], 3);
    assert_eq!(json["fields"]["completion_tokens"], 2);
    assert!(json["fields"]["duration_ms"].is_u64());
    assert!(!text.contains("secret-prompt"));
    assert!(!text.contains("secret-reply"));
}

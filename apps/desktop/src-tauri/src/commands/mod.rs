//! IPC commands, grouped by area. Each is a thin wrapper over a module that
//! can be tested without Tauri.

pub mod activity;
pub mod compare;
pub mod conversations;
pub mod files;
pub mod ollama;
pub mod projects;
pub mod settings;
pub mod temporary;

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::Arc;

    use tokio::net::TcpListener;

    use crate::audit::Audit;
    use crate::ollama::client::OllamaBaseUrl;
    use crate::ollama::{ChatThink, Ollama};

    /// Accepts connections but never answers, so a request stays in flight.
    pub async fn silent_ollama() -> (Arc<Ollama>, TcpListener) {
        silent_ollama_with(Arc::new(Audit::new())).await
    }

    /// [`silent_ollama`] writing its audit rows to `audit`.
    pub async fn silent_ollama_with(audit: Arc<Audit>) -> (Arc<Ollama>, TcpListener) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = OllamaBaseUrl::parse(&format!("http://{}", listener.local_addr().unwrap()));
        (Arc::new(Ollama::new(base, audit)), listener)
    }

    /// What `chat_think` gives a model that cannot think or use tools.
    pub fn no_think(ollama: &Ollama) -> ChatThink {
        ChatThink {
            think: None,
            connection: ollama.connection_id(),
            tools: false,
        }
    }
}

//! IPC commands, grouped by area. Each is a thin wrapper over a module that
//! can be tested without Tauri.

pub mod activity;
pub mod conversations;
pub mod ollama;
pub mod settings;

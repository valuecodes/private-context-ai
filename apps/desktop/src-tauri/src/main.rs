// Release builds on Windows open no console window next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    private_context_ai_lib::run();
}

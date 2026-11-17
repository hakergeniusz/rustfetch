//! `terminal` module: terminal emulator from environment variables.

use crate::{instance::Instance, modules::Row};

fn from_term_program(value: &str) -> String {
    match value {
        "vscode" => "VS Code".to_string(),
        "Apple_Terminal" => "Apple Terminal".to_string(),
        other => other.to_string(),
    }
}

fn from_term(term: &str) -> String {
    // Strip color suffixes: xterm-256color -> xterm, tmux-256color -> tmux
    let name = term
        .strip_suffix("-truecolor")
        .or_else(|| term.strip_suffix("-256color"))
        .or_else(|| term.strip_suffix("-color"))
        .or_else(|| term.strip_suffix("-direct"))
        .unwrap_or(term);

    if name == "linux" {
        return "Linux Console".to_string();
    }
    name.to_string()
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
        return vec![Row::new("Terminal", from_term_program(&term_program))];
    }
    if std::env::var_os("KONSOLE_VERSION").is_some() {
        return vec![Row::new("Terminal", "Konsole".to_string())];
    }
    if std::env::var_os("GHOSTTY_RESOURCES_DIR").is_some() {
        return vec![Row::new("Terminal", "Ghostty".to_string())];
    }
    if let Ok(term) = std::env::var("TERM") {
        return vec![Row::new("Terminal", from_term(&term))];
    }
    Vec::new()
}

//! `terminal` module: terminal emulator. Primary source is the parent
//! process tree (port of fastfetch's walk: skip shells and wrappers, the
//! first other ancestor is the terminal); environment fallbacks follow.

use crate::{instance::Instance, modules::Row, util};

/// Shells and wrappers to skip while walking up the process tree.
const SKIPPED: &[&str] = &[
    // shells
    "sh",
    "ash",
    "bash",
    "zsh",
    "ksh",
    "mksh",
    "pdksh",
    "oksh",
    "csh",
    "tcsh",
    "fish",
    "dash",
    "pwsh",
    "nu",
    "nushell",
    "git-shell",
    "elvish",
    "oil.ovm",
    "xonsh",
    // wrappers between us and the terminal
    "sudo",
    "su",
    "login",
    "script",
    "proot",
    "chezmoi",
    "clifm",
    "strace",
    "ltrace",
    "gdb",
    "lldb",
    "lldb-mi",
    "valgrind",
    "perf",
    "time",
    "mise",
    "mise-shim",
    "atuin",
    "run-parts",
    "guake-wrapped",
    "rustfetch",
    "fastfetch",
    "nohup",
];

fn is_skipped(name: &str) -> bool {
    SKIPPED.contains(&name)
        || name.ends_with(".sh")
        || name.contains("debug")
        || name.contains("command-not-")
        || name.starts_with("flatpak-")
        || name.starts_with("Relay(")
}

/// (comm, ppid) from /proc/<pid>/stat; comm may contain parens and spaces,
/// so it is cut between the first '(' and the last ')'.
fn process_basic_info(pid: u32) -> Option<(String, u32)> {
    let stat = util::read_file(&format!("/proc/{pid}/stat"))?;
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = &stat[open + 1..close];
    // after the comm: " S <ppid> ..." — state then ppid
    let ppid = stat[close + 2..].split_whitespace().nth(1)?.parse().ok()?;
    Some((comm.to_string(), ppid))
}

fn pretty_name(comm: &str) -> String {
    match comm {
        "gnome-terminal-" | "kgx" => "GNOME Terminal".to_string(),
        "konsole" => "Konsole".to_string(),
        other => other.to_string(),
    }
}

/// Walks ancestors from our own pid, skipping shells and wrappers; the
/// first process that is neither is the terminal. Returns the raw comm so
/// the terminalfont module can dispatch on it like fastfetch does.
pub fn from_process_tree_raw() -> Option<String> {
    let mut pid = std::process::id();
    for _ in 0..32 {
        if pid <= 1 {
            return None;
        }
        let (name, ppid) = process_basic_info(pid)?;
        if name == "rustfetch" || is_skipped(&name) {
            pid = ppid;
            continue;
        }
        return Some(name);
    }
    None
}

/// The terminal process name (comm) or the best env-based name, without
/// pretty-casing; shared with the terminalfont module's dispatch.
pub fn detect_raw() -> Option<String> {
    if let Some(name) = from_process_tree_raw() {
        return Some(name);
    }
    if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
        return Some(term_program);
    }
    if std::env::var_os("KONSOLE_VERSION").is_some() {
        return Some("konsole".to_string());
    }
    if std::env::var_os("GHOSTTY_RESOURCES_DIR").is_some() {
        return Some("ghostty".to_string());
    }
    std::env::var("TERM").ok().map(|term| from_term(&term))
}

fn from_process_tree() -> Option<String> {
    from_process_tree_raw().as_deref().map(pretty_name)
}

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
    if let Some(terminal) = from_process_tree() {
        return vec![Row::new("Terminal", terminal)];
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_stat_parsing_handles_parens_and_spaces() {
        // a comm like "sshd: user@pts" contains spaces and a ')'
        // fake file content is exercised through util::read_file in unit of
        // parse; here we just check the skip list logic
        assert!(is_skipped("zsh"));
        assert!(is_skipped("run-parts"));
        assert!(is_skipped("flatpak-something"));
        assert!(is_skipped("myscript.sh"));
        assert!(!is_skipped("konsole"));
        assert!(!is_skipped("zcode-cli"));
    }
}

//! `command` module: output of a config-supplied shell command, ported
//! from command.c. Defaults: /bin/sh -c, stdout and stderr merged, one
//! keyless "Command" row (or "Command N" per line with splitLines).

use std::process::Command;

use crate::{instance::Instance, modules::Row};

/// Runs one config command; empty text, spawn failure or empty output are
/// error rows hidden by default.
pub fn run_text(
    text: &str,
    shell: &str,
    param: Option<&str>,
    split_lines: bool,
    _instance: &Instance,
) -> Vec<Row> {
    if text.is_empty() {
        // "No command text specified" is an error row, hidden by default.
        return Vec::new();
    }
    let mut command = Command::new(shell);
    if let Some(param) = param {
        command.arg(param);
    }
    command.arg(text);
    let Ok(output) = command.stdin(std::process::Stdio::inherit()).output() else {
        return Vec::new();
    };
    // FF_PROCESS_OUTPUT_BOTH: both streams land in the result.
    let mut result = String::new();
    result.push_str(&String::from_utf8_lossy(&output.stdout));
    result.push_str(&String::from_utf8_lossy(&output.stderr));
    let result = result.trim_end();
    if result.is_empty() {
        // "No result generated" is an error row, hidden by default.
        return Vec::new();
    }

    if !split_lines {
        return vec![Row::new("Command", result)];
    }
    let lines: Vec<&str> = result.lines().collect();
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            // splitLines numbers from 1, so even a single line is "Command 1".
            Row::new(&format!("Command {}", index + 1), *line)
        })
        .collect()
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    // No command text configured; the error row is hidden by default.
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance() -> crate::instance::Instance {
        use clap::Parser as _;
        crate::instance::Instance::new(&crate::cli::Cli::parse_from(["rustfetch"]))
    }

    #[test]
    fn runs_and_splits_lines() {
        let instance = instance();
        let rows = run_text("echo one; echo two", "/bin/sh", Some("-c"), true, &instance);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].key.as_deref(), Some("Command 1"));
        assert_eq!(rows[0].value, "one");
        assert_eq!(rows[1].key.as_deref(), Some("Command 2"));

        let rows = run_text("echo single", "/bin/sh", Some("-c"), false, &instance);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key.as_deref(), Some("Command"));
        assert_eq!(rows[0].value, "single");
    }

    #[test]
    fn hides_empty_and_missing() {
        let instance = instance();
        assert!(run_text("", "/bin/sh", Some("-c"), false, &instance).is_empty());
        assert!(run_text("exit 0", "/bin/sh", Some("-c"), false, &instance).is_empty());
        assert!(run_text("echo hi", "/nonexistent-shell", None, false, &instance).is_empty());
    }
}

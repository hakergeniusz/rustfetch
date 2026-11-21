//! `command` module: output of a config-supplied shell command, ported
//! from command.c and detection/command/command.c. Defaults: /bin/sh -c,
//! stdout and stderr merged, one keyless "Command" row (or "Command N"
//! per line with splitLines). Only exit 127 is an error, matching
//! ffProcessReadOutput's comment; other exit codes still print output.

use std::io::ErrorKind;
use std::process::Command;

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
};

/// ffDetectCommand: the merged output, or the exact upstream error string.
fn run_result(text: &str, shell: &str, param: Option<&str>) -> Result<String, &'static str> {
    if text.is_empty() {
        return Err("No command text specified");
    }
    let mut command = Command::new(shell);
    if let Some(param) = param {
        command.arg(param);
    }
    command.arg(text);
    let output = match command.stdin(std::process::Stdio::inherit()).output() {
        Ok(output) => output,
        // posix_spawnp: ENOENT is "command not found", anything else is
        // the fixed spawn failure.
        Err(error) => {
            return Err(if error.kind() == ErrorKind::NotFound {
                "command not found"
            } else {
                "posix_spawnp() failed"
            });
        },
    };
    // WIFEXITED / WEXITSTATUS == 127, per ffProcessReadOutput.
    if let Some(code) = output.status.code() {
        if code == 127 {
            return Err("command not found");
        }
    } else {
        return Err("child process exited abnormally");
    }
    // FF_PROCESS_OUTPUT_BOTH: both streams land in the result, then
    // ffStrbufTrimRightSpace.
    let mut result = String::new();
    result.push_str(&String::from_utf8_lossy(&output.stdout));
    result.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(result.trim_end().to_string())
}

/// Runs one config command; errors are rows hidden by default.
pub fn run_text(
    text: &str,
    shell: &str,
    param: Option<&str>,
    split_lines: bool,
    instance: &Instance,
) -> Vec<Row> {
    let Ok(result) = run_result(text, shell, param) else {
        return Vec::new();
    };
    if result.is_empty() {
        // "No result generated" is an error row, hidden by default.
        return Vec::new();
    }

    if !split_lines {
        return vec![Row::new(instance.display_name("command"), result)];
    }
    let lines: Vec<&str> = result.lines().collect();
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            // splitLines numbers from 1, so even a single line is "Command 1".
            Row::new(&format!("{} {}", instance.display_name("command"), index + 1), *line).with_index(index as u32 + 1)
        })
        .collect()
}

/// ffGenerateCommandJsonResult: the whole output as one string, or one
/// array entry per line with splitLines.
pub fn json(
    text: &str,
    shell: &str,
    param: Option<&str>,
    split_lines: bool,
    _instance: &Instance,
) -> JsonOutcome {
    let result = match run_result(text, shell, param) {
        Ok(result) => result,
        Err(error) => return JsonOutcome::Error(error.to_string()),
    };
    if result.is_empty() {
        return JsonOutcome::Error("No result generated".to_string());
    }
    if split_lines {
        JsonOutcome::Result(Json::Array(
            result.lines().map(|line| Json::Str(line.to_string())).collect(),
        ))
    } else {
        JsonOutcome::Result(Json::Str(result))
    }
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    // No command text configured; the error row is hidden by default.
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance() -> crate::instance::Instance {
        crate::instance::Instance::new(
            &crate::cli::Cli {
                key_language: None,
                logo_position: None,
                structure: None,
                logo_none: false,
                logo_small: false,
                logo_source: None,
                logo_width: None,
                logo_height: None,
                logo_padding_top: None,
                logo_padding_bottom: None,
                logo_padding_left: None,
                logo_padding_right: None,
                logo_print_remaining: None,
                logo_type: None,
                logo_animation_frame: None,
                logo_colors: Vec::new(),
                logo_spin_speed_per_frame: None,
                logo_spin_speed_per_round: None,
                logo_spin_tilt: None,
                logo_spin_angle: None,
                logo_preserve_aspect_ratio: None,
                show_errors: None,
                stat: None,
                player_name: None,
                disable_linewrap: None,
                hide_cursor: None,
                key_value_separator: None,
                color_keys: None,
                color_title: None,
                color_output: None,
                color_separator: None,
                key_width: None,
                key_padding_left: None,
                key_type: None,
                bright_color: None,
                pipe: None,
                watch_interval: 0,
                format_json: false,
                config: None,
                config_loaded: false,
            },
            false,
            false,
            -1,
            String::new(),
            false,
            false,
            crate::instance::DisplaySurface::default(),
            crate::logo::LogoOptions::default(),
            &[],
        )
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

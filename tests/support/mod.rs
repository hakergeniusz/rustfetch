//! Shared helpers for rustfetch integration tests.
//!
//! Every child process is launched with `NO_CONFIG` set so an ambient
//! `~/.config/fastfetch/config.jsonc` cannot change the output shape, and
//! with the parity-version escape hatch removed so `--version` always
//! reports rustfetch's own identity.

const BIN: &str = env!("CARGO_BIN_EXE_rustfetch");

/// Captured stdout/stderr as raw bytes plus the decoded strings.
#[derive(Debug)]
pub struct CmdResult {
    pub status: i32,
    pub stdout_bytes: Vec<u8>,
    pub stderr_bytes: Vec<u8>,
}

impl CmdResult {
    /// Decoded stdout. Panics when the child emitted invalid UTF-8.
    pub fn stdout(&self) -> &str {
        utf8(&self.stdout_bytes)
    }

    /// Decoded stderr. Panics when the child emitted invalid UTF-8.
    pub fn stderr(&self) -> &str {
        utf8(&self.stderr_bytes)
    }
}

fn utf8(bytes: &[u8]) -> &str {
    match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => panic!("rustfetch emitted invalid UTF-8: {err}"),
    }
}

/// Run the binary with the given arguments, capturing both streams.
pub fn run(args: &[&str]) -> CmdResult {
    let child = std::process::Command::new(BIN)
        .args(args)
        .env("NO_CONFIG", "1")
        .env_remove("RUSTFETCH_PARITY_VERSION")
        .output();
    match child {
        Ok(output) => CmdResult {
            status: output.status.code().unwrap_or(-1),
            stdout_bytes: output.stdout,
            stderr_bytes: output.stderr,
        },
        Err(err) => panic!("failed to spawn {BIN}: {err}"),
    }
}

/// Run the binary and assert a successful (zero) exit status.
pub fn run_ok(args: &[&str]) -> CmdResult {
    let result = run(args);
    assert_eq!(result.status, 0, "rustfetch {args:?} failed: {}", result.stderr());
    result
}

/// Remove CSI and OSC control sequences (including their parameter bytes).
pub fn strip_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'[') {
            i += 2;
            while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
        } else if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b']') {
            i += 2;
            while i < bytes.len() {
                if bytes[i] == 0x07 {
                    i += 1;
                    break;
                }
                if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'\\') {
                    i += 2;
                    break;
                }
                i += 1;
            }
        } else if bytes[i] == 0x1b {
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    match String::from_utf8(out) {
        Ok(text) => text,
        Err(err) => panic!("strip_ansi produced invalid UTF-8: {err}"),
    }
}

fn parse_uint(bytes: &[u8]) -> Option<usize> {
    if bytes.is_empty() || !bytes.iter().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut value = 0usize;
    for &byte in bytes {
        value = value.checked_mul(10)?.checked_add(usize::from(byte - b'0'))?;
    }
    Some(value)
}

/// Expand `\x1b[<n>G` cursor moves into the equivalent padding spaces and
/// drop the other escapes. The result is the flat byte grid a terminal
/// renders, so the value column can be measured by byte offset.
pub fn expand_cursor_columns(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'[') {
            let start = i + 2;
            let mut end = start;
            while end < bytes.len() && !(0x40..=0x7e).contains(&bytes[end]) {
                end += 1;
            }
            let final_byte = bytes.get(end).copied();
            if final_byte == Some(b'G')
                && let Some(column) = parse_uint(&bytes[start..end])
            {
                while out.len() < column {
                    out.push(b' ');
                }
            }
            i = end.saturating_add(1);
        } else if bytes[i] == 0x1b {
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    match String::from_utf8(out) {
        Ok(text) => text,
        Err(err) => panic!("expand_cursor_columns produced invalid UTF-8: {err}"),
    }
}

/// Rows that carry a module key and value. Title, separator, blank and bare
/// color rows (which own their escapes) are intentionally excluded.
pub fn module_lines(output: &str) -> Vec<&str> {
    output
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || line.contains('\x1b') {
                return false;
            }
            if trimmed.bytes().all(|byte| byte == b'-') {
                return false;
            }
            line.contains(':')
        })
        .collect()
}

/// Split a `key: value` row. Returns `None` when the separator is not
/// exactly `": "` or the key is empty or colored.
pub fn split_module_line(line: &str) -> Option<(&str, &str)> {
    let colon = line.find(':')?;
    if line.as_bytes().get(colon + 1) != Some(&b' ') {
        return None;
    }
    let key = &line[..colon];
    if key.is_empty() || key.contains('\x1b') {
        return None;
    }
    Some((key, &line[colon + 2..]))
}

/// Keys of all module rows in output order.
pub fn module_keys(output: &str) -> Vec<String> {
    module_lines(output)
        .into_iter()
        .map(|line| match split_module_line(line) {
            Some((key, _)) => key.to_owned(),
            None => panic!("module row is not `key: value`: {line:?}"),
        })
        .collect()
}

fn is_semver(text: &str) -> bool {
    let mut parts = text.split('.');
    let components = [parts.next(), parts.next(), parts.next()];
    components.iter().all(|part| {
        part.is_some_and(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
    }) && parts.next().is_none()
}

/// Matches `^rustfetch \d+\.\d+\.\d+ \([a-z0-9_]+\)$` without a regex crate.
pub fn is_version_line(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("rustfetch ") else {
        return false;
    };
    let Some((version, arch)) = rest.split_once(" (") else {
        return false;
    };
    let Some(arch) = arch.strip_suffix(')') else {
        return false;
    };
    is_semver(version)
        && !arch.is_empty()
        && arch.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Matches `^\d+\.\d+\.\d+$`.
pub fn is_version_raw(text: &str) -> bool {
    is_semver(text)
}

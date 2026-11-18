//! `editor` module: $VISUAL or $EDITOR, ported from detection/editor/editor.c.
//! Only the first word of the variable names the program; it is resolved
//! through $PATH (absolute values verbatim), and the version comes from the
//! binary's embedded strings (nvim, vim, nano) or a `--version` spawn.

use crate::{instance::Instance, modules::Row, util};

fn extract_nvim_version(line: &str) -> Option<String> {
    let rest = line.strip_prefix("NVIM v")?;
    Some(rest.to_string())
}

fn extract_vim_version(line: &str) -> Option<String> {
    let rest = line.strip_prefix("VIM - Vi IMproved ")?;
    Some(rest.split(' ').next().unwrap_or(rest).to_string())
}

fn extract_nano_version(line: &str) -> Option<String> {
    let rest = line.strip_prefix("GNU nano ")?;
    Some(rest.to_string())
}

/// Whether the file exists and has any execute bit set (access(X_OK) for the
/// common case of a file owned by another uid).
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn find_in_path(name: &str) -> Option<std::path::PathBuf> {
    if name.starts_with('/') {
        let path = std::path::PathBuf::from(name);
        return is_executable(&path).then_some(path);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

/// Version via `--version` (or -version/-h) for the editors that print it;
/// the version token is the first run of digits up to whitespace.
fn version_spawn(path: &str, exe: &str) -> Option<String> {
    let param = match exe {
        "nano" | "vim" | "nvim" | "micro" | "emacs" | "hx" | "code" | "pluma" | "sublime_text"
        | "zeditor" => "--version",
        "kak" | "pico" => "-version",
        "ne" => "-h",
        _ if exe.starts_with("emacs-") || exe.starts_with("vim.") => "--version",
        _ => return None,
    };
    let output = std::process::Command::new(path).arg(param).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let first = util::first_line(&String::from_utf8_lossy(&output.stdout)).trim().to_string();
    let start = first.find(|c: char| c.is_ascii_digit())?;
    let end = first[start..]
        .find([' ', '\t', '\x0b', '\x0c', '\r'])
        .map_or(first.len(), |end| start + end);
    Some(first[start..end].to_string())
}

fn detect() -> Option<(String, Option<String>)> {
    let mut name = std::env::var("VISUAL").unwrap_or_default();
    if name.is_empty() {
        name = std::env::var("EDITOR").unwrap_or_default();
    }
    if name.is_empty() {
        return None;
    }

    // $EDITOR carries shell command lines sometimes ("code -w").
    let name_end = name.find([' ', '\t', '\n', '\x0b', '\x0c', '\r']).unwrap_or(name.len());
    name.truncate(name_end);
    if name.is_empty() {
        return None;
    }

    let path = find_in_path(&name)?;
    let path = match std::fs::canonicalize(&path) {
        // A resolved snap path is the read-only mount; keep the original.
        Ok(real) if real.to_string_lossy().ends_with("/snap") => path,
        Ok(real) => real,
        Err(_) => return None,
    };
    let exe = path.to_string_lossy().rsplit('/').next().unwrap_or("").to_string();
    if exe.is_empty() {
        return None;
    }

    let version = match exe.as_str() {
        "nvim" => util::binary_extract_strings(
            &path.to_string_lossy(),
            "NVIM v0.0.0".len(),
            extract_nvim_version,
        ),
        // vim.basic / vim.tiny variants
        name if name == "vim" || name.starts_with("vim.") => util::binary_extract_strings(
            &path.to_string_lossy(),
            "VIM - Vi IMproved 0.0".len(),
            extract_vim_version,
        ),
        "nano" => util::binary_extract_strings(
            &path.to_string_lossy(),
            "GNU nano 0.0".len(),
            extract_nano_version,
        ),
        _ => None,
    }
    .or_else(|| version_spawn(&path.to_string_lossy(), &exe));

    Some((exe, version))
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some((exe, version)) = detect() else {
        // ffPrintError is gated behind --show-errors, off by default.
        return Vec::new();
    };
    let value = match version {
        Some(version) => format!("{exe} {version}"),
        None => exe,
    };
    vec![Row::new("Editor", value)]
}

#[cfg(test)]
mod tests {
    #[test]
    fn extracts_embedded_versions() {
        assert_eq!(super::extract_nvim_version("NVIM v0.11.3"), Some("0.11.3".to_string()));
        assert_eq!(
            super::extract_vim_version("VIM - Vi IMproved 9.1 (2024 Jan 2)"),
            Some("9.1".to_string())
        );
        assert_eq!(super::extract_nano_version("GNU nano 8.4"), Some("8.4".to_string()));
        assert_eq!(super::extract_vim_version("no marker"), None);
    }

    #[test]
    fn first_word_only_names_the_editor() {
        let mut name = "code -w".to_string();
        let end = name.find([' ', '\t', '\n', '\x0b', '\x0c', '\r']).unwrap_or(name.len());
        name.truncate(end);
        assert_eq!(name, "code");
    }
}

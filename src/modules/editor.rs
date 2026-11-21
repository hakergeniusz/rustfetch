//! `editor` module: $VISUAL or $EDITOR, ported from detection/editor/editor.c.
//! Only the first word of the variable names the program; it is resolved
//! through $PATH (absolute values verbatim), and the version comes from the
//! binary's embedded strings (nvim, vim, nano) or a `--version` spawn.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

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

pub struct EditorResult {
    /// Which env var supplied the command ("Visual" or "Editor",
    /// matching upstream's result.type).
    pub editor_type: &'static str,
    pub name: String,
    pub path: String,
    pub exe: String,
    pub version: String,
}

fn detect() -> Result<EditorResult, &'static str> {
    let mut editor_type = "Visual";
    let mut name = std::env::var("VISUAL").unwrap_or_default();
    if name.is_empty() {
        editor_type = "Editor";
        name = std::env::var("EDITOR").unwrap_or_default();
    }
    if name.is_empty() {
        return Err("$VISUAL or $EDITOR not set");
    }

    // $EDITOR carries shell command lines sometimes ("code -w").
    let name_end = name.find([' ', '\t', '\n', '\x0b', '\x0c', '\r']).unwrap_or(name.len());
    name.truncate(name_end);
    if name.is_empty() {
        return Err("$VISUAL or $EDITOR not set");
    }

    let path = find_in_path(&name).ok_or("Failed to determine the executable name")?;
    let path = match std::fs::canonicalize(&path) {
        // A resolved snap path is the read-only mount; keep the original.
        Ok(real) if real.to_string_lossy().ends_with("/snap") => path,
        Ok(real) => real,
        Err(_) => return Err("Failed to determine the executable name"),
    };
    let exe = path.to_string_lossy().rsplit('/').next().unwrap_or("").to_string();
    if exe.is_empty() {
        return Err("Failed to determine the executable name");
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

    Ok(EditorResult {
        editor_type,
        name,
        path: path.to_string_lossy().into_owned(),
        exe,
        version: version.unwrap_or_default(),
    })
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(editor) = detect() else {
        // ffPrintError is gated behind --show-errors, off by default.
        return Vec::new();
    };
    let Some(format) = crate::modules::entry_format(instance.entry) else {
        let value = if editor.version.is_empty() {
            editor.exe.clone()
        } else {
            format!("{} {}", editor.exe, editor.version)
        };
        return vec![Row::new(instance.display_name("editor"), value)];
    };

    // FF_PRINT_FORMAT_CHECKED's arg list: type is a const char*, the
    // rest are FFstrbufs (all rendered as strings).
    let args = [
        crate::format_args::FormatArg { name: "type", value: crate::format_args::ArgValue::Str(editor.editor_type) },
        crate::format_args::FormatArg { name: "name", value: crate::format_args::ArgValue::Str(&editor.name) },
        crate::format_args::FormatArg { name: "exe-name", value: crate::format_args::ArgValue::Str(&editor.exe) },
        crate::format_args::FormatArg { name: "path", value: crate::format_args::ArgValue::Str(&editor.path) },
        crate::format_args::FormatArg { name: "version", value: crate::format_args::ArgValue::Str(&editor.version) },
    ];
    let rendered = crate::format_args::parse_format_string(format, &args, args.len() as u32, &crate::format_args::EngineEnv::new(instance));
    vec![Row::new(instance.display_name("editor"), rendered)]
}

/// ffGenerateEditorJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(editor) => {
            let mut obj = Json::obj();
            obj.push("type", editor.editor_type);
            obj.push("name", editor.name.clone());
            obj.push("path", editor.path.clone());
            obj.push("exe", editor.exe.clone());
            obj.push("version", editor.version.clone());
            JsonOutcome::Result(obj)
        },
    }
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

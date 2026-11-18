//! Small filesystem and formatting helpers shared by all modules.

use std::path::Path;

/// Reads a whole file into a String, returning None when it cannot be read.
pub fn read_file(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Reads a file and returns its trimmed contents, or None when unreadable or empty.
pub fn read_trimmed(path: &str) -> Option<String> {
    read_file(path)
        .map(|content| content.trim().to_string())
        .filter(|content| !content.is_empty())
}

/// Reads a file relative to a directory (used for /sys entries), trimmed.
pub fn read_trimmed_in(dir: &Path, file: &str) -> Option<String> {
    read_trimmed(dir.join(file).to_str()?)
}

/// Returns the first line of a string.
pub fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

/// Parses `KEY=VALUE` lines (os-release style) into pairs; strips quotes from values.
pub fn parse_kv(content: &str) -> Vec<(&str, &str)> {
    content
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            if key.trim().is_empty() {
                return None;
            }
            Some((key.trim(), value.trim().trim_matches('"')))
        })
        .collect()
}

/// Parses a `KEY: value` line (cpuinfo/meminfo style), stripping surrounding whitespace.
pub fn parse_colon_line(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    Some((key.trim(), value.trim()))
}

/// ffBinaryExtractStrings: scan a binary's NUL-separated strings and let
/// `extract` pull a value out of the first long-enough match.
pub fn binary_extract_strings(
    path: &str,
    min_len: usize,
    extract: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    data.split(|&b| b == 0)
        .filter_map(|chunk| std::str::from_utf8(chunk).ok())
        .find_map(|line| (line.len() >= min_len).then(|| extract(line)).flatten())
}

/// Names of all processes visible in /proc (comm values, truncated to 15
/// characters by the kernel). Shared by the wm and lm modules.
pub fn process_comms() -> std::collections::HashSet<String> {
    std::fs::read_dir("/proc")
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit())
                })
                .filter_map(|entry| read_trimmed_in(&entry.path(), "comm"))
                .filter(|comm| !comm.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// fastfetch's ffParsePropLinePointer for one line: matches `start` against
/// the line case-insensitively with flexible whitespace (whitespace in the
/// query matches any amount, even none), then returns the remainder
/// unquoted and space-trimmed.
pub fn parse_prop_line<'a>(line: &'a str, start: &str) -> Option<&'a str> {
    let mut rest = line.trim_start_matches([' ', '\t']);
    let mut start = start;
    let mut last_matched: Option<char> = None;

    while let Some(pattern) = start.chars().next() {
        if pattern == ' ' || pattern == '\t' {
            start = start.trim_start_matches([' ', '\t']);
            rest = rest.trim_start_matches([' ', '\t']);
            continue;
        }
        let actual = rest.chars().next()?;
        if !actual.eq_ignore_ascii_case(&pattern) {
            return None;
        }
        last_matched = Some(actual);
        rest = &rest[actual.len_utf8()..];
        start = &start[pattern.len_utf8()..];
    }

    // XML lines end their value at the next '<'; otherwise quoted values
    // end at the closing quote, plain ones at end of line.
    let mut value_end = '\n';
    if last_matched == Some('>') {
        value_end = '<';
    }
    rest = rest.trim_start_matches([' ', '\t']);
    if let Some(quote @ ('"' | '\'')) = rest.chars().next() {
        value_end = quote;
        rest = &rest[quote.len_utf8()..];
    }

    let value = match rest.find(value_end) {
        Some(end) => &rest[..end],
        None => rest,
    };
    Some(value.trim_end_matches(' '))
}

/// Reads a property file and returns the (last-occurrence-wins) value for
/// each query key, fastfetch's ffParsePropFileValues. Returns an empty map
/// when the file is unreadable; keys with no match are absent.
/// Reads a property file, filling unset slots for the given query keys,
/// fastfetch's ffParsePropFileValues: only unset-at-entry slots are filled
/// and the last matching occurrence in the file wins. Returns false when the
/// file is unreadable.
pub fn parse_prop_file(path: &Path, queries: &mut [(&'static str, &mut Option<String>)]) -> bool {
    let Some(content) = std::fs::read_to_string(path).ok() else {
        return false;
    };

    for (key, slot) in queries.iter_mut() {
        if slot.is_some() {
            continue;
        }
        for line in content.lines() {
            if let Some(value) = parse_prop_line(line, key) {
                **slot = Some(value.to_string());
            }
        }
    }
    true
}

/// fastfetch's ffParsePropFileConfigValues: read a file under each config
/// dir, stopping when all queries are set. Returns whether any file was
/// found alongside the values.
pub fn parse_props_in_config_dirs(
    relative: &str,
    queries: &[&'static str],
) -> (bool, Vec<Option<String>>) {
    let mut values: Vec<Option<String>> = vec![None; queries.len()];
    let mut found = false;
    for dir in config_dirs() {
        let mut pairs: Vec<(&'static str, &mut Option<String>)> = queries
            .iter()
            .zip(values.iter_mut())
            .map(|(query, value)| (*query, value))
            .collect();
        if parse_prop_file(&dir.join(relative), &mut pairs) {
            found = true;
        }
        if values.iter().all(Option::is_some) {
            break;
        }
    }
    (found, values)
}

/// XDG config dirs in fastfetch's order: XDG_CONFIG_HOME, ~/.config, $HOME,
/// XDG_CONFIG_DIRS, /etc/xdg and /etc.
pub fn config_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        dirs.push(std::path::PathBuf::from(dir));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::PathBuf::from(home);
        dirs.push(home.join(".config"));
        dirs.push(home.clone());
    }
    if let Some(list) = std::env::var_os("XDG_CONFIG_DIRS") {
        dirs.extend(std::env::split_paths(&list));
    }
    dirs.push(std::path::PathBuf::from("/etc/xdg"));
    dirs.push(std::path::PathBuf::from("/etc"));
    dirs
}

/// XDG data dirs in fastfetch's order: XDG_DATA_HOME, ~/.local/share, $HOME,
/// XDG_DATA_DIRS, /usr/local/share and /usr/share.
pub fn data_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(std::path::PathBuf::from(dir));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::PathBuf::from(home);
        dirs.push(home.join(".local/share"));
        dirs.push(home.clone());
    }
    if let Some(list) = std::env::var_os("XDG_DATA_DIRS") {
        dirs.extend(std::env::split_paths(&list));
    }
    dirs.push(std::path::PathBuf::from("/usr/local/share"));
    dirs.push(std::path::PathBuf::from("/usr/share"));
    dirs
}

/// ffSizeAppendNum with fastfetch's defaults (IEC prefixes, 2 digits with
/// trailing zeros kept, at most YiB): divide until below the base, then
/// format like glibc %.*f.
pub fn fmt_bytes(bytes: u64) -> String {
    const UNITS: [&str; 9] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"];

    let mut size = bytes as f64;
    let mut counter = 0usize;
    while size >= 1024.0 && counter < 8 {
        size /= 1024.0;
        counter += 1;
    }
    if counter == 0 {
        return format!("{bytes} B");
    }
    format!("{size:.2} {}", UNITS[counter])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kv_strips_quotes() {
        let os_release = "ID=arch\nNAME=\"Arch Linux\"\n\n# comment\nPRETTY_NAME=\"Arch Linux\"";
        let pairs = parse_kv(os_release);
        assert_eq!(pairs[0], ("ID", "arch"));
        assert_eq!(pairs[1], ("NAME", "Arch Linux"));
        assert_eq!(pairs.len(), 3);
    }

    #[test]
    fn parse_colon_line_trims() {
        assert_eq!(parse_colon_line("MemTotal :  1234 kB"), Some(("MemTotal", "1234 kB")));
        assert_eq!(parse_colon_line("no colon here"), None);
    }

    #[test]
    fn fmt_bytes_units() {
        assert_eq!(fmt_bytes(999), "999 B");
        assert_eq!(fmt_bytes(1024), "1.00 KiB");
        assert_eq!(fmt_bytes(1024 * 1024), "1.00 MiB");
        assert_eq!(fmt_bytes(1024 * 1024 * 1024), "1.00 GiB");
        assert_eq!(fmt_bytes(1024 * 1024 * 999), "999.00 MiB");
        assert_eq!(fmt_bytes(1024 * 1024 * 1024 * 58), "58.00 GiB");
        assert_eq!(fmt_bytes(1024 * 1024 * 1024 * 11 + 125 * 1024 * 1024), "11.12 GiB");
    }

    #[test]
    fn first_line_works() {
        assert_eq!(first_line("one\ntwo\n"), "one");
        assert_eq!(first_line(""), "");
    }

    #[test]
    fn parse_prop_line_matches_loosely() {
        assert_eq!(parse_prop_line("gtk-theme-name = Adwaita", "gtk-theme-name ="), Some("Adwaita"));
        assert_eq!(parse_prop_line("  GTK-Theme-Name=Adwaita-dark", "gtk-theme-name ="), Some("Adwaita-dark"));
        assert_eq!(parse_prop_line("gtk-font-name = \"Cantarell 11\"", "gtk-font-name ="), Some("Cantarell 11"));
        assert_eq!(parse_prop_line("gtk-font-name = 'Cantarell 11'", "gtk-font-name ="), Some("Cantarell 11"));
        assert_eq!(parse_prop_line("gtk-theme-name =  spaced   ", "gtk-theme-name ="), Some("spaced"));
        assert_eq!(parse_prop_line("Inherits=Breeze", "Inherits ="), Some("Breeze"));
        assert_eq!(parse_prop_line("Xcursor.theme:  Adwaita", "Xcursor.theme :"), Some("Adwaita"));
        assert_eq!(parse_prop_line("other-key = x", "gtk-theme-name ="), None);
    }

    #[test]
    fn parse_prop_file_last_occurrence_wins() {
        let dir = std::env::temp_dir().join(format!("rustfetch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_default();
        let path = dir.join("settings.ini");
        std::fs::write(
            &path,
            "gtk-theme-name = First\ngtk-icon-theme-name = Icons\n# gtk-theme-name = comment\ngtk-theme-name = Last\n",
        )
        .unwrap_or_default();

        let mut theme = None;
        let mut icons = None;
        parse_prop_file(&path, &mut [("gtk-theme-name =", &mut theme), ("gtk-icon-theme-name =", &mut icons)]);
        assert_eq!(theme.as_deref(), Some("Last"));
        assert_eq!(icons.as_deref(), Some("Icons"));

        std::fs::remove_dir_all(&dir).unwrap_or_default();
    }
}

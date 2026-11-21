//! Small filesystem and formatting helpers shared by all modules.

use std::io::Write as _;
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

/// Unquotes a gdbus-rendered string, undoing the \' and \\ escapes the
/// renderer emits; non-strings pass through.
pub fn unquote(text: &str) -> String {
    let Some(inner) = text.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) else {
        return text.to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(e @ ('\\' | '\'')) => out.push(e),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                },
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// ffDBusGetString-from-rendered-text: strings unquoted; arrays of strings
/// joined with ", "; anything else passes through.
pub fn string_value(text: &str) -> String {
    if let Some(inner) = text.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return split_top_level(inner)
            .iter()
            .map(|item| unquote(item))
            .collect::<Vec<_>>()
            .join(", ");
    }
    unquote(text)
}

/// Splits rendered container bodies on top-level ", " (outside quotes and
/// nested parens/brackets/braces), honoring backslash escapes.
pub fn split_top_level(text: &str) -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut start = 0usize;
    for (i, c) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '\'' {
                in_string = false;
            }
            continue;
        }
        match c {
            '\'' => in_string = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                items.push(text[start..i].trim().to_string());
                start = i + 1;
            },
            _ => {},
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() {
        items.push(last.to_string());
    }
    items
}

/// Splits a rendered dict entry `'key': value` at the first top-level ": "
/// (keys are quoted strings, so their colons sit inside quotes).
pub fn split_pair(item: &str) -> Option<(&str, &str)> {
    let pos = item.find("': ")?;
    Some((&item[..pos + 1], &item[pos + 3..]))
}

/// The (key, value) pairs of a rendered `{ 'k': v, ... }` dict body.
pub fn dict_entries(text: &str) -> Vec<(String, String)> {
    let Some(inner) = text.strip_prefix('{').and_then(|s| s.strip_suffix('}')) else {
        return Vec::new();
    };
    split_top_level(inner)
        .iter()
        .filter_map(|item| split_pair(item).map(|(k, v)| (unquote(k), v.to_string())))
        .collect()
}

/// ffTimeToFullStr: "<date>T<time>.<millis><±hhmm>" in the local timezone,
/// e.g. "2026-11-18T20:37:36.116+0100".
pub fn time_to_full_str(msec: u64) -> String {
    if msec == 0 {
        return String::new();
    }
    let seconds_since_epoch = (msec / 1000) as i64;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&seconds_since_epoch, &mut tm) };
    let offset = tm.tm_gmtoff.unsigned_abs();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}{}{:02}{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        msec % 1000,
        if tm.tm_gmtoff < 0 { '-' } else { '+' },
        offset / 3600,
        (offset % 3600) / 60,
    )
}

/// ffTimeToShortStr: "<date> <time>" in the local timezone, e.g.
/// "2026-11-18 20:37:36".
// Consumed by the uptime module's custom-format branch landing next.
#[expect(dead_code)]
pub fn time_to_short_str(msec: u64) -> String {
    if msec == 0 {
        return String::new();
    }
    let seconds_since_epoch = (msec / 1000) as i64;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&seconds_since_epoch, &mut tm) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
    )
}

/// FFTimeGetAgeResult: the calendar age between two timestamps — whole
/// years, the days into the current year, and the years with their
/// fraction.
// Consumed by the uptime module's custom-format branch landing next.
#[expect(dead_code)]
pub struct TimeAge {
    pub years: u32,
    pub days_of_year: u32,
    pub years_fraction: f64,
}

/// ffTimeGetAge: years count from the local calendar (decremented when the
/// anniversary hasn't passed yet), daysOfYear from the mktime-normalized
/// anniversary, and yearsFraction divides the elapsed seconds by the
/// anniversary-to-next-anniversary span (mktime's normalization carries
/// into the second call, matching leap years and DST like the C).
#[expect(dead_code)]
pub fn time_get_age(birth_ms: u64, now_ms: u64) -> TimeAge {
    if birth_ms == 0 || now_ms < birth_ms {
        return TimeAge { years: 0, days_of_year: 0, years_fraction: 0.0 };
    }
    // SAFETY: localtime_r and mktime write only their own tm struct;
    // birth_tm's mktime normalization carries into the second call like
    // the C.
    unsafe {
        let mut birth_s = (birth_ms / 1000) as libc::time_t;
        let mut birth_tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&birth_s, &mut birth_tm);
        let now_s = (now_ms / 1000) as libc::time_t;
        let mut now_tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now_s, &mut now_tm);

        let mut years = (now_tm.tm_year - birth_tm.tm_year) as u32;
        if now_tm.tm_yday < birth_tm.tm_yday {
            years -= 1;
        }

        birth_tm.tm_year += years as i32;
        birth_s = libc::mktime(&mut birth_tm);
        let diff_s = (now_s - birth_s) as u32;
        let days_of_year = diff_s / (24 * 60 * 60);

        birth_tm.tm_year += 1;
        let next_s = libc::mktime(&mut birth_tm);
        let years_fraction = diff_s as f64 / (next_s - birth_s) as f64 + years as f64;

        TimeAge { years, days_of_year, years_fraction }
    }
}

/// The kernel escapes spaces and control characters in /proc device names
/// as \NNN octal triples; getmntent decodes them the same way.
pub fn decode_octal_escapes(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let octal = std::str::from_utf8(&bytes[index + 1..index + 4]).unwrap_or("");
            if let Ok(value) = u8::from_str_radix(octal, 8) {
                out.push(value);
                index += 4;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// fnmatch without the FNM_PATHNAME flag: '*' and '?' wildcards only.
pub fn glob_match(pattern: &str, value: &str) -> bool {
    fn inner(p: &[u8], v: &[u8]) -> bool {
        match (p.first(), v.first()) {
            (None, None) => true,
            (Some(b'*'), _) => {
                for i in 0..=v.len() {
                    if inner(&p[1..], &v[i..]) {
                        return true;
                    }
                }
                false
            },
            (Some(b'?'), Some(_)) => inner(&p[1..], &v[1..]),
            (Some(c), Some(d)) if c == d => inner(&p[1..], &v[1..]),
            _ => false,
        }
    }
    inner(pattern.as_bytes(), value.as_bytes())
}

/// ffSuppressIO (io_unix.c): point stdout and stderr at /dev/null so noisy
/// third-party libraries (libva, Vulkan ICD loaders, EGL) cannot pollute
/// the output during detection. The original fds and the null fd are
/// captured once on the first suppress, like upstream's statics.
pub fn suppress_io(suppress: bool) -> bool {
    static ORIGINALS: std::sync::OnceLock<(i32, i32, i32)> = std::sync::OnceLock::new();
    let originals = if suppress {
        Some(ORIGINALS.get_or_init(|| unsafe {
            (
                libc::dup(libc::STDOUT_FILENO),
                libc::dup(libc::STDERR_FILENO),
                libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC),
            )
        }))
    } else {
        // Unsuppress before any suppress: nothing to restore, still fine.
        ORIGINALS.get()
    };
    let Some(&(orig_out, orig_err, null_fd)) = originals else {
        return true;
    };
    if null_fd == -1 {
        return false;
    }
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    unsafe {
        libc::dup2(
            if suppress { null_fd } else { orig_out },
            libc::STDOUT_FILENO,
        );
        libc::dup2(
            if suppress { null_fd } else { orig_err },
            libc::STDERR_FILENO,
        );
    }
    true
}

/// RAII wrapper around unsuppress, standing in for ffSuppressIO's
/// gnu::cleanup attribute: suppression ends when the guard drops.
pub struct SuppressIoGuard;

impl SuppressIoGuard {
    /// Returns None (instead of a guard) when the suppression failed, so
    /// callers keep ffSuppressIO's bool without needing a Drop restore.
    pub fn new() -> Option<Self> {
        if crate::util::suppress_io(true) {
            Some(Self)
        } else {
            None
        }
    }
}

impl Drop for SuppressIoGuard {
    fn drop(&mut self) {
        crate::util::suppress_io(false);
    }
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
    fn first_line_works() {
        assert_eq!(first_line("one\ntwo\n"), "one");
        assert_eq!(first_line(""), "");
    }

    #[test]
    fn parse_prop_line_matches_loosely() {
        assert_eq!(
            parse_prop_line("gtk-theme-name = Adwaita", "gtk-theme-name ="),
            Some("Adwaita")
        );
        assert_eq!(
            parse_prop_line("  GTK-Theme-Name=Adwaita-dark", "gtk-theme-name ="),
            Some("Adwaita-dark")
        );
        assert_eq!(
            parse_prop_line("gtk-font-name = \"Cantarell 11\"", "gtk-font-name ="),
            Some("Cantarell 11")
        );
        assert_eq!(
            parse_prop_line("gtk-font-name = 'Cantarell 11'", "gtk-font-name ="),
            Some("Cantarell 11")
        );
        assert_eq!(
            parse_prop_line("gtk-theme-name =  spaced   ", "gtk-theme-name ="),
            Some("spaced")
        );
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
        parse_prop_file(
            &path,
            &mut [
                ("gtk-theme-name =", &mut theme),
                ("gtk-icon-theme-name =", &mut icons),
            ],
        );
        assert_eq!(theme.as_deref(), Some("Last"));
        assert_eq!(icons.as_deref(), Some("Icons"));

        std::fs::remove_dir_all(&dir).unwrap_or_default();
    }

}

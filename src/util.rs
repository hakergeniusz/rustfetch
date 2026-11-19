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
/// format via yyjson's fixed precision, which rounds the exact decimal
/// expansion half away from zero (`mod >= p10 / 2`) — unlike Rust's
/// ties-to-even `{:.2}` (glibc %.*f matches Rust here, but sizes go
/// through ffStrbufAppendDouble).
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
    // scaled stays below 102400, so the multiply, truncation and the
    // half-way compare are exact for every dyadic byte count.
    let scaled = size * 100.0;
    let truncated = scaled.trunc();
    let units = truncated as u64 + if scaled - truncated >= 0.5 { 1 } else { 0 };
    format!("{}.{:02} {}", units / 100, units % 100, UNITS[counter])
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

/// ffDurationAppendNum: "<N days, [N hours,] N mins>" for a duration in
/// whole seconds. Seconds fold into minutes with a >= 30 bump, zero tails
/// are omitted, and 100+ days carry the "(!)" marker. Sub-minute
/// durations print "<N seconds>".
pub fn duration_parts(total_seconds: u64) -> Vec<String> {
    if total_seconds < 60 {
        return vec![plural(total_seconds, "second")];
    }

    let mut minutes_total = total_seconds / 60;
    if total_seconds % 60 >= 30 {
        minutes_total += 1;
    }
    let days = minutes_total / (60 * 24);
    let hours = (minutes_total / 60) % 24;
    let minutes = minutes_total % 60;

    let mut parts = Vec::new();
    if days > 0 {
        let mut text = plural(days, "day");
        if days >= 100 {
            text.push_str("(!)");
        }
        parts.push(text);
    }
    if hours > 0 {
        parts.push(plural(hours, "hour"));
    }
    if minutes > 0 {
        parts.push(plural(minutes, "min"));
    }
    parts
}

fn plural(count: u64, unit: &str) -> String {
    if count == 1 {
        format!("{count} {unit}")
    } else {
        format!("{count} {unit}s")
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

/// ffFreqAppendNum with the display defaults: up to 2 decimals, trailing
/// zeros trimmed, " GHz"; a zero frequency prints nothing.
pub fn fmt_freq(mhz: u32) -> String {
    if mhz == 0 {
        return String::new();
    }
    let ghz = mhz as f64 / 1000.0;
    let text = format!("{ghz:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.').to_string();
    format!("{text} GHz")
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
    fn fmt_bytes_ties_round_half_up_like_yyjson() {
        // 14.625 GiB exactly: Rust {:.2} would print 14.62 (ties-to-even),
        // yyjson rounds the exact tail up.
        assert_eq!(fmt_bytes(117 * (1 << 27)), "14.63 GiB");
        // 1.125 KiB exactly: even would be 1.12.
        assert_eq!(fmt_bytes(1152), "1.13 KiB");
        // 14.875: both rules agree on 14.88.
        assert_eq!(fmt_bytes(119 * (1 << 27)), "14.88 GiB");
        // 999.995 MiB carries over.
        assert_eq!(fmt_bytes(999 * (1 << 20) + 1023 * 1024), "1000.00 MiB");
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

    #[test]
    fn duration_rounds_like_ffdurationappendnum() {
        // 54 min 59 s still bumps to "55 mins"; 54 min 29 s does not.
        assert_eq!(duration_parts(3299).join(", "), "55 mins");
        assert_eq!(duration_parts(3269).join(", "), "54 mins");
        assert_eq!(duration_parts(3300).join(", "), "55 mins");
        // 55 min 30 s bumps into "56 mins".
        assert_eq!(duration_parts(3330).join(", "), "56 mins");
        assert_eq!(duration_parts(45).join(", "), "45 seconds");
        assert_eq!(duration_parts(1).join(", "), "1 second");
        assert_eq!(duration_parts(90_000).join(", "), "1 day, 1 hour");
        assert_eq!(duration_parts(95_400).join(", "), "1 day, 2 hours, 30 mins");
        assert_eq!(duration_parts(8_640_000).join(", "), "100 days(!)");
        assert_eq!(duration_parts(60).join(", "), "1 min");
    }
}

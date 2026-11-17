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

/// Floored percentage of `part` over `total`; None when total is zero.
pub fn percent(part: u64, total: u64) -> Option<u64> {
    (total > 0).then(|| part * 100 / total)
}

/// Formats a byte count the way fastfetch does: two decimals in GiB,
/// whole MiB/KiB otherwise.
pub fn fmt_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const KIB: f64 = 1024.0;

    let value = bytes as f64;
    if value >= GIB {
        format!("{:.2} GiB", value / GIB)
    } else if value >= MIB {
        format!("{:.0} MiB", value / MIB)
    } else if value >= KIB {
        format!("{:.0} KiB", value / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// Terminal column width via ioctl; 80 when unavailable (e.g. piped output).
pub fn term_width() -> u16 {
    let mut ws = libc::winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: `ws` is a plain POD struct and ioctl only writes into it on success.
    let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) } == 0;
    if ok && ws.ws_col > 0 {
        ws.ws_col
    } else {
        80
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
    fn percent_floors_and_guards() {
        assert_eq!(percent(1, 3), Some(33));
        assert_eq!(percent(0, 0), None);
        assert_eq!(percent(5, 0), None);
    }

    #[test]
    fn fmt_bytes_units() {
        assert_eq!(fmt_bytes(999), "999 B");
        assert_eq!(fmt_bytes(1024), "1 KiB");
        assert_eq!(fmt_bytes(1024 * 1024), "1 MiB");
        assert_eq!(fmt_bytes(1024 * 1024 * 1024), "1.00 GiB");
        assert_eq!(fmt_bytes(1024 * 1024 * 999), "999 MiB");
    }

    #[test]
    fn first_line_works() {
        assert_eq!(first_line("one\ntwo\n"), "one");
        assert_eq!(first_line(""), "");
    }
}

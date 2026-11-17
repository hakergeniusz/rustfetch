//! Logo selection and parsing of fastfetch-style `$N` color placeholders.

use crate::{cli::Cli, instance::Instance, logo_data, logo_data::LogoDef};

/// One piece of ascii art carrying the ANSI color parameter to print it with
/// (e.g. `"36"` or `"1;47"`).
pub struct Segment {
    pub text: String,
    pub color: &'static str,
}

/// A logo ready for rendering: parsed lines plus the width of the widest line.
pub struct Logo {
    pub def: &'static LogoDef,
    pub lines: Vec<Vec<Segment>>,
    pub width: usize,
}

/// Parses ascii art with `$0`..`$9` placeholders into styled segments.
///
/// `$0` resets to the default color, `$N` picks the Nth color of the palette.
/// The last used color carries over to following lines, matching fastfetch.
pub fn parse_art(art: &str, colors: &[&'static str]) -> (Vec<Vec<Segment>>, usize) {
    const DEFAULT: &str = "39";

    let mut lines = Vec::new();
    let mut carried = DEFAULT;
    let mut max_width = 0;

    for raw in art.lines() {
        let chars: Vec<char> = raw.chars().collect();
        let mut segments: Vec<Segment> = Vec::new();
        let mut text = String::new();
        let mut current = carried;

        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                if !text.is_empty() {
                    segments.push(Segment { text: std::mem::take(&mut text), color: current });
                }
                let n = chars[i + 1] as usize - b'0' as usize;
                current = if n == 0 { DEFAULT } else { colors.get(n - 1).copied().unwrap_or(DEFAULT) };
                carried = current;
                i += 2;
            } else {
                text.push(chars[i]);
                i += 1;
            }
        }
        if !text.is_empty() {
            segments.push(Segment { text, color: current });
        }

        max_width = max_width.max(segments.iter().map(|s| s.text.chars().count()).sum());
        lines.push(segments);
    }

    (lines, max_width)
}

/// Case-insensitive, punctuation-insensitive logo lookup: `EndeavourOS` and
/// `endeavouros` both normalize to `endeavouros`.
fn normalize(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

fn find_logo(id: &str) -> Option<&'static LogoDef> {
    let id = normalize(id);
    logo_data::LOGOS
        .iter()
        .find(|logo| logo.names.iter().any(|name| normalize(name) == id))
}

/// Builds a renderable logo from its definition, honoring `--small`.
fn build(def: &'static LogoDef, cli: &Cli) -> Logo {
    let art = if cli.small { def.small.unwrap_or(def.big) } else { def.big };
    let (lines, width) = parse_art(art, def.colors);
    Logo { def, lines, width }
}

/// Resolves the logo to display: explicit `--logo` source first, then the
/// detected distro (ID, then ID_LIKE), then fastfetch's unknown fallback.
pub fn load(cli: &Cli, instance: &Instance) -> Option<Logo> {
    if let Some(source) = &cli.logo {
        if source == "none" {
            return None;
        }
        if let Some(def) = find_logo(source) {
            return Some(build(def, cli));
        }
        if let Ok(art) = std::fs::read_to_string(source) {
            let (lines, width) = parse_art(&art, logo_data::UNKNOWN.colors);
            return Some(Logo { def: &logo_data::UNKNOWN, lines, width });
        }
        eprintln!("rustfetch: unknown logo or unreadable file: {source}");
        return None;
    }

    for id in instance.distro_id.iter().chain(&instance.distro_like) {
        if let Some(def) = find_logo(id) {
            return Some(build(def, cli));
        }
    }
    Some(build(&logo_data::UNKNOWN, cli))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg_text(line: &[Segment]) -> String {
        line.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn parse_art_placeholder_and_carry() {
        let colors: &[&'static str] = &["31", "34"];
        let (lines, width) = parse_art("  $1abc$2def\n  carried", colors);
        assert_eq!(lines.len(), 2);
        assert_eq!(seg_text(&lines[0]), "  abcdef");
        // text before the first placeholder keeps the default color
        assert_eq!(lines[0][0].color, "39");
        assert_eq!(lines[0][1].color, "31");
        assert_eq!(lines[0][2].color, "34");
        // color carries over to the next line
        assert_eq!(lines[1][0].color, "34");
        assert_eq!(width, 9);
    }

    #[test]
    fn parse_art_zero_is_default_and_missing_index_falls_back() {
        let colors: &[&'static str] = &["31"];
        let (lines, _) = parse_art("$1a$0b$2c", colors);
        assert_eq!(lines[0][0].color, "31");
        assert_eq!(lines[0][1].color, "39");
        assert_eq!(lines[0][2].color, "39");
    }

    #[test]
    fn parse_art_dollar_without_digit_is_literal() {
        let colors: &[&'static str] = &["31"];
        let (lines, _) = parse_art("a$ b$1", colors);
        assert_eq!(seg_text(&lines[0]), "a$ b");
        assert_eq!(lines[0][0].color, "39");
    }

    #[test]
    fn normalize_is_insesitive() {
        assert_eq!(normalize("EndeavourOS"), normalize("endeavouros"));
        assert_eq!(normalize("Linux Mint"), normalize("linuxmint"));
    }
}

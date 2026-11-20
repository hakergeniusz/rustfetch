//! Logo selection and parsing of fastfetch-style `$N` color placeholders.

use crate::{
    cli::Cli,
    instance::{Instance, LogoColors},
    logo_data,
    logo_data::LogoDef,
};

/// One piece of ascii art carrying the ANSI color parameter to print it with
/// (e.g. `"36"` or `"1;47"`).
pub struct Segment {
    pub text: String,
    pub color: &'static str,
}

/// A logo ready for rendering: parsed lines plus the width of the widest line.
pub struct Logo {
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
    // The first palette color carries into the leading text of every
    // logo, like fastfetch's carryColor seeded from colors[0].
    let mut carried = colors.first().copied().unwrap_or(DEFAULT);
    let mut max_width = 0;

    for raw in art.lines() {
        let chars: Vec<char> = raw.chars().collect();
        let mut segments: Vec<Segment> = Vec::new();
        let mut text = String::new();
        let mut current = carried;

        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                // Markers always emit the color, even when they sit at a
                // line start next to the carried color or back to back —
                // fastfetch prints one escape per `$N`.
                segments.push(Segment {
                    text: std::mem::take(&mut text),
                    color: current,
                });
                let n = chars[i + 1] as usize - b'0' as usize;
                current = if n == 0 {
                    DEFAULT
                } else {
                    colors.get(n - 1).copied().unwrap_or(DEFAULT)
                };
                carried = current;
                i += 2;
            } else {
                text.push(chars[i]);
                i += 1;
            }
        }
        if !text.is_empty() {
            segments.push(Segment {
                text,
                color: current,
            });
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

/// Builds a renderable logo from its definition; `small` picks the compact
/// variant, like fastfetch's `-l small`.
fn build(def: &'static LogoDef, small: bool) -> Logo {
    let art = if small {
        def.small.unwrap_or(def.big)
    } else {
        def.big
    };
    let (lines, width) = parse_art(art, def.colors);
    Logo { lines, width }
}

/// colorTitle = colors[0], colorKeys = colors[1]; logos may override both.
fn logo_colors(def: &LogoDef) -> LogoColors {
    LogoColors {
        title: def.color_title.or(def.colors.first().copied()),
        key: def.color_keys.or(def.colors.get(1).copied()),
    }
}

/// Resolves the logo to display, porting ffLogoPrint: the NONE type
/// prints nothing, an empty source uses the detected distro, and a
/// source tries the built-in logo table (SMALL variant under the SMALL
/// type), then `.raw` files verbatim, then text files with `$N`
/// replacement — falling back silently to the detected distro logo.
/// The color palette is always the detected distro's, even with no logo.
pub fn load(cli: &Cli, instance: &Instance) -> (Option<Logo>, LogoColors) {
    let detected = detected_def(instance);
    if cli.logo_none {
        return (None, logo_colors(detected));
    }
    let Some(source) = &cli.logo_source else {
        return (Some(build(detected, cli.logo_small)), logo_colors(detected));
    };

    // A miss on the builtin table falls back to the DETECTED big logo,
    // like ffLogoPrintDetected(FF_LOGO_SIZE_UNKNOWN).
    let fallback = || (Some(build(detected, false)), logo_colors(detected));
    if let Some(def) = find_logo(source) {
        return (Some(build(def, cli.logo_small)), logo_colors(def));
    }
    let Ok(art) = std::fs::read_to_string(source) else {
        return fallback();
    };
    if source.to_ascii_lowercase().ends_with(".raw") {
        // raw: the file's own escape sequences pass through, no `$N`
        // replacement (logoPrintFileIfExists's doColorReplacement=false).
        let lines = art.lines().map(|text| vec![Segment { text: text.to_string(), color: "" }]).collect();
        return (Some(Logo { lines, width: 0 }), logo_colors(detected));
    }
    let (lines, width) = parse_art(&art, logo_data::UNKNOWN.colors);
    (Some(Logo { lines, width }), logo_colors(&logo_data::UNKNOWN))
}

/// The distro's logo definition, falling back to fastfetch's unknown logo.
fn detected_def(instance: &Instance) -> &'static LogoDef {
    for id in instance.distro_id.iter().chain(&instance.distro_like) {
        if let Some(def) = find_logo(id) {
            return def;
        }
    }
    &logo_data::UNKNOWN
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
        // text before the first placeholder carries colors[0], like
        // fastfetch's carryColor seeded from the palette head
        assert_eq!(lines[0][0].color, "31");
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
        // each marker starts a segment, even with empty text before it
        assert_eq!(lines[0][0].color, "31");
        assert_eq!(lines[0][1].color, "31");
        assert_eq!(lines[0][2].color, "39");
        assert_eq!(lines[0][3].color, "39");
    }

    #[test]
    fn parse_art_dollar_without_digit_is_literal() {
        let colors: &[&'static str] = &["31"];
        let (lines, _) = parse_art("a$ b$1", colors);
        assert_eq!(seg_text(&lines[0]), "a$ b");
        assert_eq!(lines[0][0].color, "31");
    }

    #[test]
    fn normalize_is_insesitive() {
        assert_eq!(normalize("EndeavourOS"), normalize("endeavouros"));
        assert_eq!(normalize("Linux Mint"), normalize("linuxmint"));
    }
}

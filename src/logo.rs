//! Logo selection and parsing of fastfetch-style `$N` color placeholders.

use crate::{
    cli::Cli,
    instance::{Instance, LogoColors},
    logo_entries::{self, LogoEntry},
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

/// FFLogoSize reduced to the values the lookups can produce (Normal is
/// reachable via the empty source, Unknown has no line-type filter).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Size {
    Normal,
    Small,
    Unknown,
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

/// ffLogoGetBuiltin: only entries whose first name shares the requested
/// name's letter are examined (upstream's per-letter tables), filtered by
/// the size's line type, then matched case-insensitively; with small size
/// a name also matches a "_small"-suffixed entry name.
pub(crate) fn get_builtin(name: &str, size: Size) -> Option<&'static LogoEntry> {
    let first = name.chars().next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let first = first.to_ascii_lowercase();
    for entry in logo_entries::LOGO_ENTRIES {
        let Some(candidate) = entry.names.first() else { continue };
        let Some(bucket) = candidate.chars().next() else { continue };
        if bucket.to_ascii_lowercase() != first {
            continue;
        }
        let type_ok = match size {
            Size::Normal => entry.line_type == 0,
            Size::Small => entry.line_type == 1,
            Size::Unknown => true,
        };
        if !type_ok {
            continue;
        }
        if logo_has_name(entry, name, size == Size::Small) {
            return Some(entry);
        }
    }
    None
}

/// logoHasName.
fn logo_has_name(entry: &LogoEntry, name: &str, small: bool) -> bool {
    for logo_name in entry.names {
        if small
            && let Some(stripped) = logo_name.strip_suffix("_small")
            && stripped.len() == name.len()
            && stripped.eq_ignore_ascii_case(name)
        {
            return true;
        }
        if logo_name.eq_ignore_ascii_case(name) {
            return true;
        }
    }
    false
}

/// ffLogoGetBuiltinDetected: the os id, then the pretty name, then the
/// idLike tokens, then the kernel name, then the unknown logo.
pub(crate) fn get_builtin_detected(instance: &Instance, size: Size) -> &'static LogoEntry {
    if let Some(id) = &instance.distro_id
        && let Some(entry) = get_builtin(id, size)
    {
        return entry;
    }
    if let Some(name) = &instance.distro_name
        && let Some(entry) = get_builtin(name, size)
    {
        return entry;
    }
    // Upstream splits a multi-word ID_LIKE into single-word candidates.
    for like in &instance.distro_like {
        for token in like.split_whitespace() {
            if let Some(entry) = get_builtin(token, size) {
                return entry;
            }
        }
        if let Some(entry) = get_builtin(like, size) {
            return entry;
        }
    }
    if let Some(entry) = get_builtin(&crate::platform::platform().sysinfo.name, size) {
        return entry;
    }
    &logo_entries::UNKNOWN_ENTRY
}

/// The palette carried by an entry: colorTitle = colors[0],
/// colorKeys = colors[1]; entries may override both.
pub(crate) fn entry_colors(entry: &LogoEntry) -> LogoColors {
    LogoColors {
        title: entry.color_title.or(entry.colors.first().copied()),
        key: entry.color_keys.or(entry.colors.get(1).copied()),
    }
}

/// Builds a renderable logo from its entry's art.
fn build(entry: &'static LogoEntry) -> Logo {
    let (lines, width) = parse_art(entry.art, entry.colors);
    Logo { lines, width }
}

/// Builds the raw file logo: the file's own escape sequences pass through,
/// no `$N` replacement (logoPrintFileIfExists's doColorReplacement=false).
fn build_raw(art: &str) -> Logo {
    let lines = art.lines().map(|text| vec![Segment { text: text.to_string(), color: "" }]).collect();
    Logo { lines, width: 0 }
}

/// Resolves the logo to display, porting ffLogoPrint: the NONE type
/// prints nothing, an empty source uses the detected distro, and a
/// source tries the builtin logo table (SMALL variant under the SMALL
/// type), then `.raw` files verbatim, then text files with `$N`
/// replacement — falling back silently to the detected distro logo.
/// The color palette is always the detected distro's, even with no logo.
pub fn load(cli: &Cli, instance: &Instance) -> (Option<Logo>, LogoColors) {
    let detected = get_builtin_detected(instance, Size::Normal);
    if cli.logo_none {
        return (None, entry_colors(detected));
    }
    let Some(source) = &cli.logo_source else {
        let size = if cli.logo_small { Size::Small } else { Size::Normal };
        let entry = get_builtin_detected(instance, size);
        return (Some(build(entry)), entry_colors(entry));
    };

    // A miss on the builtin table falls back to the DETECTED logo with
    // no size filter, like ffLogoPrintDetected(FF_LOGO_SIZE_UNKNOWN).
    let fallback = || {
        let entry = get_builtin_detected(instance, Size::Unknown);
        (Some(build(entry)), entry_colors(entry))
    };
    if cli.logo_small {
        // A SMALL source matches a "_small"-suffixed entry name too.
        if let Some(entry) = get_builtin(source, Size::Small) {
            return (Some(build(entry)), entry_colors(entry));
        }
        return fallback();
    }
    if let Some(entry) = get_builtin(source, Size::Unknown) {
        return (Some(build(entry)), entry_colors(entry));
    }
    // "-" reads the logo from stdin, like ffLogoPrintFileIfExists.
    let Ok(art) = (if source == "-" {
        use std::io::Read;
        let mut buf = String::new();
        match std::io::stdin().read_to_string(&mut buf) {
            Ok(_) => Ok(buf),
            Err(_) => Err(std::io::Error::other("stdin")),
        }
    } else {
        std::fs::read_to_string(source)
    }) else {
        return fallback();
    };
    if source.to_ascii_lowercase().ends_with(".raw") {
        return (Some(build_raw(&art)), entry_colors(detected));
    }
    // A trailing newline terminates one more (empty) logo line, like
    // ffLogoPrintChars pushing the segment after the last '\n'.
    let mut art = art;
    if art.ends_with('\n') {
        art.push('\n');
    }
    // File logos apply the detected distro's palette to their `$N` markers
    // (logoPrintCustom's ffLogoApplyColors(logoGetBuiltinDetected(NORMAL))).
    let (lines, width) = parse_art(&art, detected.colors);
    (Some(Logo { lines, width }), entry_colors(detected))
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
    fn get_builtin_matches_names_and_small_suffix() {
        // Exact (case-insensitive) name.
        assert!(get_builtin("arch", Size::Unknown).is_some());
        assert!(get_builtin("ARCH", Size::Unknown).is_some());
        // A small size matches a "_small"-suffixed entry name with the
        // suffix stripped, and never the normal entry.
        let Some(small) = get_builtin("arch", Size::Small) else { panic!("arch_small") };
        assert_eq!(small.names.first(), Some(&"arch_small"));
        // Unknown size has no filter: the first entry named "arch".
        let Some(any) = get_builtin("arch", Size::Unknown) else { panic!("arch") };
        assert_eq!(any.names.first(), Some(&"arch"));
        // Non-alphabetic heads can not be bucketed.
        assert!(get_builtin("9front", Size::Unknown).is_none());
    }
}

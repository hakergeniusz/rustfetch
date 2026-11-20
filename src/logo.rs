//! Logo selection and parsing of fastfetch-style `$N` color placeholders.

use std::io::IsTerminal;

use crate::{
    cli::Cli,
    instance::{Instance, LogoColors},
    logo_entries::{self, LogoEntry},
};

/// The escape a segment prepends to its text: `Code` renders `\e[{code}m`
/// (an empty code renders `\e[m`, like an out-of-palette `$N`), `Esc` is a
/// verbatim escape sequence found in the logo data itself, and `Pass`
/// emits nothing (raw logo lines). Escapes are emitted regardless of the
/// color mode — logo.c has no pipe gate on the escape or carry paths —
/// while bold and reset are only for the terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SegmentColor {
    Code(&'static str),
    Esc(String),
    Pass,
}

/// One piece of ascii art carrying the ANSI escape to print before its
/// text.
pub struct Segment {
    pub text: String,
    pub color: SegmentColor,
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

/// Parses ascii art into styled segments, porting logoLineCacheBuild: tabs
/// widen to four spaces, `\e[` sequences pass through verbatim without
/// counting width (an `m`-terminated one becomes the carried color), and
/// `$1`-`$9` pick the palette — markers emit their escape at the marker
/// position, so a marker ending the line still flushes it before the
/// reset. `colors: None` turns the `$` handling off for raw files. In pipe
/// mode any `$X` (digit or not) is swallowed, the seed carry is skipped,
/// and no bold/reset wrap is emitted.
pub fn parse_art(art: &str, colors: Option<&'static [&'static str]>, pipe: bool) -> (Vec<Vec<Segment>>, usize) {
    let mut lines = Vec::new();
    // carryColor: seeded from colors[0] only for $-replacement logos outside
    // pipe mode; a raw `m`-terminated escape overwrites it in any mode.
    let mut carried = match (colors, pipe) {
        (Some(palette), false) => SegmentColor::Code(palette.first().copied().unwrap_or("")),
        _ => SegmentColor::Pass,
    };
    let mut max_width = 0;

    for raw in art.lines() {
        let chars: Vec<char> = raw.chars().collect();
        let mut segments: Vec<Segment> = Vec::new();
        let mut text = String::new();
        let mut current = carried.clone();
        // A color event ($N marker or escape) with no text since; it still
        // flushes its escape at the line end.
        let mut tail_is_event = false;
        // The char after a plain `$` bypasses the tab/escape/marker checks,
        // like the fall-through into the utf8 path.
        let mut plain_next = false;
        let mut width = 0;

        let mut i = 0;
        while i < chars.len() {
            if plain_next {
                text.push(chars[i]);
                width += 1;
                tail_is_event = false;
                plain_next = false;
                i += 1;
                continue;
            }
            match chars[i] {
                '\t' => {
                    text.push_str("    ");
                    width += 4;
                    tail_is_event = false;
                    i += 1;
                },
                '\u{1b}' if i + 1 < chars.len() && chars[i + 1] == '[' => {
                    let mut j = i + 2;
                    while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == ';') {
                        j += 1;
                    }
                    if j < chars.len() && chars[j].is_ascii() {
                        let esc: String = chars[i..=j].iter().collect();
                        // The escape passes through verbatim, counts no
                        // width, and an `m`-terminated one becomes the
                        // carried color. Its bytes are emitted by the next
                        // flush, exactly once, like the marker escapes.
                        let is_color = esc.ends_with('m');
                        segments.push(Segment { text: std::mem::take(&mut text), color: current.clone() });
                        current = SegmentColor::Esc(esc);
                        if is_color {
                            carried = current.clone();
                        }
                        tail_is_event = true;
                        i = j + 1;
                    } else {
                        // A non-ascii terminator: `\e[` plus the scanned
                        // digits drop from the output but keep their width.
                        width += j - i - 1;
                        i = j;
                        tail_is_event = false;
                    }
                },
                '$' if colors.is_some() && i + 1 < chars.len() => match chars[i + 1] {
                    // `$$` is an escaped dollar; a `$` ending the line or the
                    // logo is a plain one.
                    '$' => {
                        text.push('$');
                        width += 1;
                        tail_is_event = false;
                        i += 2;
                    },
                    _ if pipe => {
                        i += 2;
                    },
                    digit @ '1'..='9' => {
                        segments.push(Segment { text: std::mem::take(&mut text), color: current.clone() });
                        let n = digit as usize - b'0' as usize;
                        // Unfilled palette slots stay empty strings: `\e[m`.
                        current = SegmentColor::Code(colors.and_then(|palette| palette.get(n - 1)).copied().unwrap_or(""));
                        carried = current.clone();
                        tail_is_event = true;
                        i += 2;
                    },
                    // `$0` and `$X` are plain dollars; the next char is
                    // emitted without further interpretation.
                    _ => {
                        text.push('$');
                        width += 1;
                        tail_is_event = false;
                        plain_next = true;
                        i += 1;
                    },
                },
                c => {
                    text.push(c);
                    width += 1;
                    tail_is_event = false;
                    i += 1;
                },
            }
        }
        if !text.is_empty() || tail_is_event {
            segments.push(Segment { text, color: current });
        }

        max_width = max_width.max(width);
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
fn build(entry: &'static LogoEntry, pipe: bool) -> Logo {
    let (lines, width) = parse_art(entry.art, Some(entry.colors), pipe);
    Logo { lines, width }
}

/// Builds the raw file logo: the file's own escape sequences pass through,
/// no `$N` replacement (logoPrintFileIfExists's doColorReplacement=false).
fn build_raw(art: &str) -> Logo {
    let lines = art.lines().map(|text| vec![Segment { text: text.to_string(), color: SegmentColor::Pass }]).collect();
    Logo { lines, width: 0 }
}

/// Resolves the logo to display, porting ffLogoPrint: the NONE type
/// prints nothing, an empty source uses the detected distro, and a
/// source tries the builtin logo table (SMALL variant under the SMALL
/// type), then `.raw` files verbatim, then text files with `$N`
/// replacement — falling back silently to the detected distro logo.
/// The color palette is always the detected distro's, even with no logo.
pub fn load(cli: &Cli, instance: &Instance) -> (Option<Logo>, LogoColors) {
    let pipe = instance.pipe;
    let detected = get_builtin_detected(instance, Size::Normal);
    if cli.logo_none {
        return (None, entry_colors(detected));
    }
    let Some(source) = &cli.logo_source else {
        let size = if cli.logo_small { Size::Small } else { Size::Normal };
        let entry = get_builtin_detected(instance, size);
        return (Some(build(entry, pipe)), entry_colors(entry));
    };

    // A miss on the builtin table falls back to the DETECTED logo with
    // no size filter, like ffLogoPrintDetected(FF_LOGO_SIZE_UNKNOWN).
    let fallback = || {
        let entry = get_builtin_detected(instance, Size::Unknown);
        (Some(build(entry, pipe)), entry_colors(entry))
    };
    if cli.logo_small {
        // A SMALL source matches a "_small"-suffixed entry name too.
        if let Some(entry) = get_builtin(source, Size::Small) {
            return (Some(build(entry, pipe)), entry_colors(entry));
        }
        return fallback();
    }
    if let Some(entry) = get_builtin(source, Size::Unknown) {
        return (Some(build(entry, pipe)), entry_colors(entry));
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
    let (lines, width) = parse_art(&art, Some(detected.colors), pipe);
    (Some(Logo { lines, width }), entry_colors(detected))
}

/// listModules-style listing of the builtin logo table: numbered quoted
/// names (ffLogoBuiltinList) or one first-name per line for shell
/// completion (ffLogoBuiltinListAutocompletion).
pub(crate) fn list_logos_builtin(counter_start: usize, autocompletion: bool) {
    if autocompletion {
        for entry in logo_entries::LOGO_ENTRIES {
            println!("{}", entry.names[0]);
        }
        return;
    }
    for (index, entry) in logo_entries::LOGO_ENTRIES.iter().enumerate() {
        let counter = counter_start + index;
        print!("{counter}){} ", if counter < 10 { " " } else { "" });
        for name in entry.names {
            print!("\"{name}\" ");
        }
        putchar_newline();
    }
}

/// listAvailableLogos: the dataDirs' fastfetch/logos/ directories.
pub(crate) fn list_logos_custom() {
    for dir in crate::config::data_dirs() {
        let logos = format!("{dir}fastfetch/logos/");
        crate::config::list_files_recursively(&logos, &logos, true, 0, None);
    }
}

/// ffLogoBuiltinPrint: every builtin logo, name line first, then the art
/// printed position-top (each cache line plus a newline). The name carries
/// the logo's first color outside pipe mode.
pub(crate) fn print_logos() {
    let pipe = !std::io::stdout().is_terminal() || std::env::var_os("NO_COLOR").is_some();
    for entry in logo_entries::LOGO_ENTRIES {
        if pipe {
            println!("{}:", entry.names[0]);
        } else {
            println!("\x1b[{}m{}:\x1b[0m", entry.colors[0], entry.names[0]);
        }
        let logo = Logo {
            lines: parse_art(entry.art, Some(entry.colors), pipe).0,
            width: 0,
        };
        for (line, _) in crate::render::colorize(&logo, !pipe) {
            println!("{line}");
        }
    }
}

fn putchar_newline() {
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg_text(line: &[Segment]) -> String {
        line.iter().map(|s| s.text.as_str()).collect()
    }

    fn seg_codes(line: &[Segment]) -> Vec<&str> {
        line.iter().filter_map(|s| match &s.color {
            SegmentColor::Code(code) => Some(*code),
            _ => None,
        }).collect()
    }

    #[test]
    fn parse_art_placeholder_and_carry() {
        let colors: &'static [&'static str] = &["31", "34"];
        let (lines, width) = parse_art("  $1abc$2def\n  carried", Some(colors), false);
        assert_eq!(lines.len(), 2);
        assert_eq!(seg_text(&lines[0]), "  abcdef");
        // text before the first placeholder carries colors[0], like
        // fastfetch's carryColor seeded from the palette head
        assert_eq!(seg_codes(&lines[0]), ["31", "31", "34"]);
        // color carries over to the next line
        assert_eq!(seg_codes(&lines[1]), ["34"]);
        assert_eq!(width, 9);
    }

    #[test]
    fn parse_art_marker_ending_the_line_flushes_its_escape() {
        let colors: &'static [&'static str] = &["31", "34"];
        let (lines, width) = parse_art("ab$2\ncd", Some(colors), false);
        // the trailing `$2` still emits `\e[34m` before the reset, while
        // the next line re-emits it as the carried color
        assert_eq!(seg_codes(&lines[0]), ["31", "34"]);
        assert_eq!(seg_codes(&lines[1]), ["34"]);
        assert_eq!(seg_text(&lines[0]), "ab");
        assert_eq!(width, 2);
    }

    #[test]
    fn parse_art_zero_is_a_plain_dollar_and_missing_index_is_empty() {
        let colors: &'static [&'static str] = &["31"];
        let (lines, _) = parse_art("$1a$0b$2c", Some(colors), false);
        // each marker starts a segment, even with empty text before it
        assert_eq!(seg_codes(&lines[0]), ["31", "31", ""]);
        assert_eq!(seg_text(&lines[0]), "a$0bc");
    }

    #[test]
    fn parse_art_dollar_without_digit_is_literal() {
        let colors: &'static [&'static str] = &["31"];
        let (lines, _) = parse_art("a$ b$1", Some(colors), false);
        assert_eq!(seg_text(&lines[0]), "a$ b");
        assert_eq!(seg_codes(&lines[0]), ["31", "31"]);
    }

    #[test]
    fn parse_art_tab_widens_and_escape_carries_without_width() {
        let colors: &'static [&'static str] = &["36"];
        let (lines, width) = parse_art("A\tB\n\x1b[31mBBB\nCCC", Some(colors), false);
        assert_eq!(seg_text(&lines[0]), "A    B");
        // the escape passes through, counts no width and carries to the
        // next lines
        assert_eq!(seg_text(&lines[1]), "BBB");
        assert_eq!(lines[1][1].color, SegmentColor::Esc("\x1b[31m".to_string()));
        assert_eq!(lines[2][0].color, SegmentColor::Esc("\x1b[31m".to_string()));
        assert_eq!(width, 6);
    }

    #[test]
    fn parse_art_pipe_swallows_dollars_and_drops_the_seed() {
        let colors: &'static [&'static str] = &["31", "34"];
        let (lines, width) = parse_art("ab$2cd$1", Some(colors), true);
        assert_eq!(seg_text(&lines[0]), "abcd");
        assert_eq!(width, 4);
        // no carry seed in pipe mode: the line has no code segments
        assert!(lines[0].iter().all(|seg| seg.color == SegmentColor::Pass));
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

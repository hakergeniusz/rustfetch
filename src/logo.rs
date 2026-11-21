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
    pub text: Vec<u8>,
    pub color: SegmentColor,
}

/// FFLogoPosition before AUTO resolution; the default is LEFT.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PositionOpt {
    Auto,
    Left,
    Right,
    Top,
}

/// The resolved FFLogoPosition; AUTO becomes LEFT or TOP by
/// ffLogoSelectPosition and never reaches the renderer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Position {
    Left,
    Right,
    Top,
}

/// ffLogoSelectPosition: a non-AUTO position stands; without a terminal
/// width (not a tty, or the fastOnly ioctl failed) the logo stays LEFT;
/// otherwise it only keeps the two-column layout when the info columns
/// left of it number at least FF_LOGO_AUTO_MIN_INFO_WIDTH (32).
pub fn resolve_position(configured: PositionOpt, terminal_columns: u32, logo_width: u32) -> Position {
    match configured {
        PositionOpt::Left => Position::Left,
        PositionOpt::Right => Position::Right,
        PositionOpt::Top => Position::Top,
        PositionOpt::Auto => {
            const MIN_INFO_WIDTH: u32 = 32;
            if terminal_columns == 0
                || (logo_width <= terminal_columns && terminal_columns - logo_width >= MIN_INFO_WIDTH)
            {
                Position::Left
            } else {
                Position::Top
            }
        },
    }
}

/// A logo ready for rendering: parsed art lines plus the width of the
/// widest line for the cache renderer, or a `.raw` file's bytes dumped
/// verbatim through ffLogoPrintCharsRaw's cursor path.
pub enum Logo {
    Chars {
        /// The full cache: paddingTop blank lines, the parsed art, then the
        /// height-option fill — exactly what logoLineCacheBuild leaves.
        lines: Vec<Vec<Segment>>,
        /// maxLineWidth + the prefix pad: the widest cache line, floored by
        /// the width option (instance.state.logoWidth before position fixup).
        width: usize,
        /// The pad-left prefix baked into the parsed lines (0 when the
        /// configured position is RIGHT or the option is unset).
        pad_left: usize,
        /// (leading paddingTop entries, trailing height-fill entries) —
        /// the bare-empty bookends of the cache.
        bare: (usize, usize),
        position: Position,
    },
    Raw {
        content: Vec<u8>,
        position: Position,
        /// The width+height static path's pre/post bytes and the
        /// logoWidth/logoHeight the rows align against; None runs the
        /// cursor-query path instead.
        static_layout: Option<(Vec<u8>, Vec<u8>, usize, usize)>,
        /// The image printers' cursor-query differences: the failure
        /// message's name and printError gate (iterm's is ungated
        /// upstream), and the bytes written after a successful query.
        /// None is the .raw file logo's own behavior.
        cpr: Option<crate::image::ImageCpr>,
        /// The image printers flush ffStart's reset ahead of their dump
        /// (upstream's fflush); a .raw file keeps it buffered behind the
        /// dump — the wire order differs and is observable.
        flush_start: bool,
    },
}

/// FFOptionsLogo's text-rendering inputs: the paddings, the width/height
/// floors, whether the closing ffLogoPrintRemaining runs, and the spin
/// logo's turn inputs. Defaults match ffOptionsInitLogo (paddingRight 4,
/// printRemaining true, spin speeds 0 = unset).
#[derive(Clone, Copy)]
pub struct LogoOptions {
    pub padding_top: u32,
    pub padding_bottom: u32,
    pub padding_left: u32,
    pub padding_right: u32,
    pub width: u32,
    pub height: u32,
    /// --logo-preserve-aspect-ratio / config logo.preserveAspectRatio,
    /// read by the iterm image printer's payload.
    pub preserve_aspect_ratio: bool,
    pub print_remaining: bool,
    pub spin_speed_per_frame: f32,
    pub spin_speed_per_round: f32,
    pub spin_tilt: f32,
    pub spin_angle: f32,
    /// rustfetch-only HyFetch coloring: when a HyFetch palette was resolved,
    /// every logo line gets its own palette slot (a vertical flag gradient)
    /// instead of filling only the art's existing `$N` placeholders.
    pub hyfetch: bool,
}

/// FFLogoType: the --logo-type enum and the source-type setters' values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogoTypeOpt {
    Auto,
    Builtin,
    Small,
    File,
    FileRaw,
    Data,
    DataRaw,
    CommandRaw,
    ImageSixel,
    ImageKitty,
    ImageKittyDirect,
    ImageKittyIcat,
    ImageIterm,
    ImageChafa,
    ImageRaw,
    Spin,
    None,
}

impl Default for LogoOptions {
    fn default() -> Self {
        Self {
            padding_top: 0,
            padding_bottom: 0,
            padding_left: 0,
            padding_right: 4,
            width: 0,
            height: 0,
            preserve_aspect_ratio: false,
            print_remaining: true,
            spin_speed_per_frame: 0.0,
            spin_speed_per_round: 0.0,
            spin_tilt: 0.0,
            spin_angle: 0.0,
            hyfetch: false,
        }
    }
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
pub fn parse_art(art: &[u8], colors: Option<&'static [&'static str]>, pipe: bool) -> (Vec<Vec<Segment>>, usize) {
    // A NUL ends the parse entirely: upstream's walk stops there and the
    // outer loop breaks, so nothing after it is ever parsed.
    let art = match art.iter().position(|&byte| byte == 0) {
        Some(nul) => &art[..nul],
        None => art,
    };
    let mut lines = Vec::new();
    // carryColor: seeded from colors[0] only for $-replacement logos outside
    // pipe mode; a raw `m`-terminated escape overwrites it in any mode.
    let mut carried = match (colors, pipe) {
        (Some(palette), false) => SegmentColor::Code(palette.first().copied().unwrap_or("")),
        _ => SegmentColor::Pass,
    };
    let mut max_width = 0u32;

    // split('\n'), not lines(): a trailing newline parses one trailing
    // empty cache line, like upstream's logoLineCacheBuild (whose
    // parsedHeight only counts newline-terminated lines, so the cache
    // always ends parsedHeight + 1 entries long). A `\r` is content
    // unless the newline follows it.
    let ends_with_newline = art.ends_with(b"\n");
    let raw_lines: Vec<&[u8]> = art.split(|&byte| byte == b'\n').collect();
    for (index, raw) in raw_lines.iter().enumerate() {
        let strip_carriage = !(index + 1 == raw_lines.len() && !ends_with_newline);
        let line: &[u8] = if strip_carriage && raw.ends_with(b"\r") {
            &raw[..raw.len() - 1]
        } else {
            raw
        };
        let mut segments: Vec<Segment> = Vec::new();
        let mut text = Vec::new();
        let mut current = carried.clone();
        // A color event ($N marker or escape) with no text since; it still
        // flushes its escape at the line end.
        let mut tail_is_event = false;
        // The char after a plain `$` bypasses the tab/escape/marker checks,
        // like the fall-through into the utf8 path.
        let mut plain_next = false;
        let mut width = 0u32;

        let mut i = 0usize;
        while i < line.len() {
            if plain_next {
                // The char after a plain `$` still measures by wcwidth,
                // like the utf8 decode it falls into upstream.
                let (taken, char_width) = crate::wcwidth::decode_char(&line[i..]);
                text.extend_from_slice(&line[i..i + taken]);
                width += char_width;
                tail_is_event = false;
                plain_next = false;
                i += taken;
                continue;
            }
            match line[i] {
                b'\t' => {
                    text.extend_from_slice(b"    ");
                    width += 4;
                    tail_is_event = false;
                    i += 1;
                },
                0x1b if i + 1 < line.len() && line[i + 1] == b'[' => {
                    let mut j = i + 2;
                    while j < line.len() && (line[j].is_ascii_digit() || line[j] == b';') {
                        j += 1;
                    }
                    if j < line.len() && line[j].is_ascii() {
                        // is_ascii includes the NUL, which the truncation
                        // above already removed.
                        let esc: &[u8] = &line[i..=j];
                        // The escape passes through verbatim, counts no
                        // width, and an `m`-terminated one becomes the
                        // carried color. Its bytes are emitted by the next
                        // flush, exactly once, like the marker escapes.
                        let is_color = esc.last() == Some(&b'm');
                        segments.push(Segment { text: std::mem::take(&mut text), color: current.clone() });
                        current = SegmentColor::Esc(String::from_utf8(esc.to_vec()).unwrap_or_default());
                        if is_color {
                            carried = current.clone();
                        }
                        tail_is_event = true;
                        i = j + 1;
                    } else {
                        // A non-ascii terminator: `\e[` plus the scanned
                        // digits drop from the output but keep their width.
                        width += (j - i - 1) as u32;
                        i = j;
                        tail_is_event = false;
                    }
                },
                b'$' if colors.is_some() && i + 1 < line.len() => match line[i + 1] {
                    // `$$` is an escaped dollar; a `$` ending the line or the
                    // logo is a plain one.
                    b'$' => {
                        text.push(b'$');
                        width += 1;
                        tail_is_event = false;
                        i += 2;
                    },
                    _ if pipe => {
                        i += 2;
                    },
                    digit @ b'1'..=b'9' => {
                        segments.push(Segment { text: std::mem::take(&mut text), color: current.clone() });
                        let n = (digit - b'0') as usize;
                        // Unfilled palette slots stay empty strings: `\e[m`.
                        current = SegmentColor::Code(colors.and_then(|palette| palette.get(n - 1)).copied().unwrap_or(""));
                        carried = current.clone();
                        tail_is_event = true;
                        i += 2;
                    },
                    // `$0` and `$X` are plain dollars; the next char is
                    // emitted without further interpretation.
                    _ => {
                        text.push(b'$');
                        width += 1;
                        tail_is_event = false;
                        plain_next = true;
                        i += 1;
                    },
                },
                _ => {
                    // ffUtf8CharLenWidth: wide chars count 2, combining and
                    // nonprintable 0, and invalid bytes pass through at
                    // width 1 — the raw bytes always reach the output.
                    let (taken, char_width) = crate::wcwidth::decode_char(&line[i..]);
                    text.extend_from_slice(&line[i..i + taken]);
                    width += char_width;
                    tail_is_event = false;
                    i += taken;
                },
            }
        }
        if !text.is_empty() || tail_is_event {
            segments.push(Segment { text, color: current });
        }

        max_width = max_width.max(width);
        lines.push(segments);
    }

    (lines, max_width as usize)
}

/// HyFetch art rewrite. Strips every existing `$[0-9]` marker (leaving `$$`
/// escapes untouched) and prepends `$((line_index % 9) + 1)` to each line, so
/// consecutive lines cycle through all nine palette slots — a vertical flag
/// gradient like real `hyfetch`, which colors each line of the art instead
/// of only filling the placeholders fastfetch ships. Lines are exactly the
/// ones `parse_art` will see (`split('\n')`, including any trailing empty
/// line), and joining them back with `\n` preserves whitespace, tabs, CR,
/// line count and art width: markers count zero width, and a carried color
/// is simply replaced at the head of every line.
fn hyfetch_gradient_art(art: &[u8]) -> Vec<u8> {
    let lines: Vec<&[u8]> = art.split(|&byte| byte == b'\n').collect();
    let mut out = Vec::with_capacity(art.len() + lines.len() * 2);
    for (index, raw) in lines.iter().enumerate() {
        if index > 0 {
            out.push(b'\n');
        }
        out.push(b'$');
        out.push(b'1' + (index % 9) as u8);
        let mut i = 0;
        while i < raw.len() {
            if raw[i] == b'$' && i + 1 < raw.len() {
                match raw[i + 1] {
                    // `$$` is a literal dollar in parse_art; keep the pair
                    // so rewriting never changes what it renders.
                    b'$' => {
                        out.extend_from_slice(b"$$");
                        i += 2;
                        continue;
                    },
                    // `$0`-`$9` is the marker whose color we are replacing.
                    b'0'..=b'9' => {
                        i += 2;
                        continue;
                    },
                    _ => {},
                }
            }
            out.push(raw[i]);
            i += 1;
        }
    }
    out
}

/// `parse_art` with the optional HyFetch rewrite. Only palette-driven logos
/// (builtin, data, file) are rewritten; raw logos pass `None` deliberately —
/// their bytes carry their own escapes, so they ignore the gradient exactly
/// as they ignore `$N` replacement. Pipe mode drops all logo escapes, so the
/// rewrite is skipped there too. When the flag is off this is a plain
/// `parse_art`, byte-for-byte identical to fastfetch.
fn parse_logo_art(
    art: &[u8],
    palette: Option<&'static [&'static str]>,
    pipe: bool,
    hyfetch: bool,
) -> (Vec<Vec<Segment>>, usize) {
    if hyfetch && !pipe && palette.is_some() {
        let rewritten = hyfetch_gradient_art(art);
        parse_art(&rewritten, palette, pipe)
    } else {
        parse_art(art, palette, pipe)
    }
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

/// logoLineCacheBuild's cache assembly: the pad-left prefix (skipped when
/// the CONFIGURED position is RIGHT — the decision happens before
/// ffLogoResolveAutoPosition resolves AUTO), the width-option floor on
/// maxLineWidth, the height-option floor on parsedHeight, and the fill
/// that keeps the cache at logoHeight + 1 entries.
fn build_chars(
    lines: Vec<Vec<Segment>>,
    art_width: usize,
    configured: PositionOpt,
    opts: &LogoOptions,
) -> Logo {
    let pad_left = if configured == PositionOpt::Right {
        0
    } else {
        opts.padding_left as usize
    };
    // An empty art skips the whole parse loop upstream: no prefixes, and
    // maxLineWidth stays at the width-option floor.
    let max_line_width = if lines.is_empty() {
        opts.width as usize
    } else {
        (opts.width as usize).max(art_width + pad_left)
    };
    let position = resolve_position(
        configured,
        crate::modules::terminalsize::logo_terminal_columns(),
        max_line_width as u32 + opts.padding_right,
    );
    // parsedHeight counts newline-terminated lines (entries - 1), then the
    // height option floors it; the cache fills to logoHeight + 1 entries.
    let parsed_height = lines.len().saturating_sub(1).max(opts.height as usize);
    let logo_height = opts.padding_top as usize + parsed_height;
    let art_entries = lines.len();
    let mut cache = lines;
    for _ in 0..opts.padding_top as usize {
        cache.insert(0, Vec::new());
    }
    debug_assert!(cache.len() <= logo_height + 1);
    while cache.len() < logo_height + 1 {
        cache.push(Vec::new());
    }
    cache.truncate(logo_height + 1);
    let fill = cache.len() - opts.padding_top as usize - art_entries;
    Logo::Chars {
        lines: cache,
        width: max_line_width,
        pad_left,
        // The paddingTop bookend and the height-fill tail were pushed via
        // logoLineCachePush(nullptr, 0): always bare-empty, no prefix,
        // unlike blank art lines which gain the prefix's width.
        bare: (opts.padding_top as usize, fill),
        position,
    }
}

/// ffLogoApplyColors's palette merge: an explicit slot (config or
/// --logo-color-N) wins, the base entry fills the rest, and slots beyond
/// the base stay empty so their `$N` markers read as bare `\e[m`.
fn palette_with_overrides(
    base: &'static [&'static str],
    instance: &Instance,
) -> Option<&'static [&'static str]> {
    let overrides = instance.logo_color_overrides;
    if overrides.is_empty() {
        return Some(base);
    }
    let mut slots: Vec<&'static str> = (0..9)
        .map(|index| base.get(index).copied().unwrap_or(""))
        .collect();
    for (slot, color) in overrides {
        slots[*slot] = color;
    }
    Some(slots.leak())
}

/// Builds a renderable logo from its entry's art.
fn build(entry: &'static LogoEntry, pipe: bool, configured: PositionOpt, opts: &LogoOptions, instance: &Instance) -> Logo {
    let palette = palette_with_overrides(entry.colors, instance);
    let (lines, art_width) = parse_logo_art(builtin_art(entry).as_bytes(), palette, pipe, opts.hyfetch);
    build_chars(lines, art_width, configured, opts)
}

/// fastfetch_encode_c_string removes the trailing newline when CMake
/// embeds the ascii art (CMakeLists.txt:369), so builtin art never ends
/// with '\n' and its cache never gains the trailing empty line a raw
/// file logo would produce — one stray newline per builtin dump.
fn builtin_art(entry: &LogoEntry) -> &'static str {
    entry.art.strip_suffix('\n').unwrap_or(entry.art)
}

/// The image attempt ffLogoPrint makes for every non-.txt file source:
/// image rendering is not implemented, but the attempt still costs the
/// terminal a sizing probe (chafa's ffDetectTerminalSize slow path) whose
/// queries reach /dev/tty — reproduced here, result discarded, behind
/// ffLogoPrintImageIfExists' early gates (pipe mode, missing file, and
/// the screen multiplexer all skip it).
fn maybe_image_attempt(source: &str, pipe: bool) {
    if pipe
        || source.to_ascii_lowercase().ends_with(".txt")
        || !std::path::Path::new(source).exists()
        || std::env::var("TERM").as_deref() == Ok("screen")
    {
        return;
    }
    crate::modules::terminalsize::image_attempt_probe();
}

/// logoPrintData: the detected distro's palette applies to the `$N`
/// markers only when doColorReplacement (DataRaw and CommandRaw pass
/// None, so dollars stay literal and no carry color is seeded); the
/// title/key colors always come from the detected logo.
fn print_data(
    content: &str,
    palette: Option<&'static [&'static str]>,
    pipe: bool,
    configured: PositionOpt,
    opts: &LogoOptions,
    detected: &LogoEntry,
    instance: &Instance,
) -> (Option<Logo>, LogoColors) {
    let palette = palette.and_then(|base| palette_with_overrides(base, instance));
    let (lines, art_width) = parse_logo_art(content.as_bytes(), palette, pipe, opts.hyfetch);
    (Some(build_chars(lines, art_width, configured, opts)), entry_colors(detected))
}

/// updateLogoPath: resolve the source to readable file bytes — the direct
/// path, "-" (stdin), a wordexp expansion ($VAR and ~, command
/// substitution included: upstream passes 0 flags and picks a random
/// match; the port takes the first), or a dataDirs fastfetch/logos/
/// entry. "media-cover" is not resolvable (the media module's cover
/// detection is not wired into the logo path).
fn update_logo_path(source: &str, _instance: &Instance) -> Option<Vec<u8>> {
    if source == "-" {
        use std::io::Read;
        let mut buf = Vec::new();
        // A terminal stdin yields nothing readable (upstream's
        // ffAppendFDBuffer fails on it) and the caller falls back to the
        // detected logo; a real empty read is a failure too.
        return match std::io::stdin().read_to_end(&mut buf) {
            Ok(0) => None,
            Ok(_) => Some(buf),
            Err(_) => None,
        };
    }
    if source == "media-cover" {
        return None;
    }
    if std::path::Path::new(source).is_file() {
        return std::fs::read(source).ok();
    }
    if let Some(path) = wordexp_first(source)
        && std::path::Path::new(&path).is_file()
    {
        return std::fs::read(&path).ok();
    }
    for dir in crate::config::data_dirs() {
        let candidate = format!("{dir}fastfetch/logos/{source}");
        if std::path::Path::new(&candidate).is_file() {
            return std::fs::read(&candidate).ok();
        }
    }
    None
}

/// updateLogoPath's path resolution without the read: the direct file,
/// "-" (stdin, not a file — the image ladder rejects it), or the first
/// wordexp/dataDirs match. media-cover stays unresolvable.
fn resolve_logo_path(source: &str, _instance: &Instance) -> Option<String> {
    if source == "-" {
        return Some(source.to_string());
    }
    if source == "media-cover" {
        return None;
    }
    if std::path::Path::new(source).is_file() {
        return Some(source.to_string());
    }
    if let Some(path) = wordexp_first(source)
        && std::path::Path::new(&path).is_file()
    {
        return Some(path);
    }
    for dir in crate::config::data_dirs() {
        let candidate = format!("{dir}fastfetch/logos/{source}");
        if std::path::Path::new(&candidate).is_file() {
            return Some(candidate);
        }
    }
    None
}

/// The first wordexp expansion of the source, like ffPathExpandEnv's
/// wordexp(in, &exp, 0) without upstream's random pick among matches.
fn wordexp_first(source: &str) -> Option<String> {
    #[repr(C)]
    struct Wordexp {
        we_wordc: usize,
        we_wordv: *mut *mut libc::c_char,
        we_offs: usize,
    }
    unsafe extern "C" {
        fn wordexp(in_: *const libc::c_char, out: *mut Wordexp, flags: libc::c_int) -> libc::c_int;
        fn wordfree(out: *mut Wordexp);
    }
    let text = std::ffi::CString::new(source).ok()?;
    unsafe {
        let mut exp: Wordexp = std::mem::zeroed();
        if wordexp(text.as_ptr(), &mut exp, 0) != 0 {
            return None;
        }
        let result = if exp.we_wordc >= 1 {
            Some(std::ffi::CStr::from_ptr(*exp.we_wordv).to_string_lossy().into_owned())
        } else {
            None
        };
        wordfree(&mut exp);
        result
    }
}

/// Resolves the logo to display, porting ffLogoPrint: the NONE type
/// prints nothing, an empty source uses the detected distro, and a
/// source tries the builtin logo table (SMALL variant under the SMALL
/// type), then `.raw` files verbatim, then text files with `$N`
/// replacement — falling back silently to the detected distro logo.
/// The color palette is always the detected distro's, even with no logo.
/// The JSON config's logo keys are the base state; the command line runs
/// after it upstream, so its values win — NONE is sticky either way.
pub fn load(cli: &Cli, config: Option<&crate::config::Config>, instance: &Instance) -> (Option<Logo>, LogoColors) {
    let pipe = instance.pipe;
    let opts = &instance.logo_options;
    let configured = cli
        .logo_position
        .or(config.and_then(|config| config.logo_position))
        .unwrap_or(PositionOpt::Left);
    let logo_none = cli.logo_none || config.is_some_and(|config| config.logo_none);
    let logo_small = cli.logo_small || config.is_some_and(|config| config.logo_small);
    let logo_type = cli.logo_type.or(config.and_then(|config| config.logo_type));
    let detected = get_builtin_detected(instance, Size::Normal);
    if logo_none || logo_type == Some(LogoTypeOpt::None) {
        return (None, entry_colors(detected));
    }
    let source: Option<&str> = cli
        .logo_source
        .as_deref()
        .or(config.and_then(|config| config.logo_source.as_deref()));
    // Spin runs before the empty-source shortcut: it resolves an empty or
    // `auto` source to the detected logo itself (ffLogoPrint's order).
    if logo_type == Some(LogoTypeOpt::Spin) {
        return load_spin(source, configured, opts, instance, cli, detected);
    }
    let Some(source) = source else {
        let size = if logo_small || logo_type == Some(LogoTypeOpt::Small) {
            Size::Small
        } else {
            Size::Normal
        };
        let entry = get_builtin_detected(instance, size);
        return (Some(build(entry, pipe, configured, opts, instance)), entry_colors(entry));
    };

    // A miss on the builtin table falls back to the DETECTED logo with
    // no size filter, like ffLogoPrintDetected(FF_LOGO_SIZE_UNKNOWN).
    let fallback = || {
        let entry = get_builtin_detected(instance, Size::Unknown);
        (Some(build(entry, pipe, configured, opts, instance)), entry_colors(entry))
    };

    // An explicit type routes through logoTryKnownType: builtin and small
    // lookups (no file attempt), the data and command flavors verbatim,
    // files through updateLogoPath, and image types through their attempt
    // (this port's rendering is the chars fallback after the sizing probe).
    match logo_type {
        Some(LogoTypeOpt::Builtin) => {
            return match get_builtin(source, Size::Unknown) {
                Some(entry) => (Some(build(entry, pipe, configured, opts, instance)), entry_colors(entry)),
                None => fallback(),
            };
        },
        Some(LogoTypeOpt::Small) => {
            return match get_builtin(source, Size::Small) {
                Some(entry) => (Some(build(entry, pipe, configured, opts, instance)), entry_colors(entry)),
                None => fallback(),
            };
        },
        Some(LogoTypeOpt::Data) => {
            // logoPrintData's empty-source bail reports false, and the
            // caller falls back to the detected logo.
            if source.is_empty() {
                return fallback();
            }
            return print_data(source, Some(detected.colors), pipe, configured, opts, detected, instance);
        },
        Some(LogoTypeOpt::DataRaw) => {
            if source.is_empty() {
                return fallback();
            }
            return print_data(source, None, pipe, configured, opts, detected, instance);
        },
        Some(LogoTypeOpt::CommandRaw) => {
            // ffProcessAppendStdOut inherits stdin, captures stdout only,
            // ignores the exit status and trims trailing spaces; spawn
            // failures fall back silently (the error message is
            // display.showErrors-gated).
            let output = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(source)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::inherit())
                .output();
            let Ok(output) = output else {
                return fallback();
            };
            let text = String::from_utf8_lossy(&output.stdout);
            let text = text.trim_end_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']).to_string();
            if text.is_empty() {
                return fallback();
            }
            return print_data(&text, None, pipe, configured, opts, detected, instance);
        },
        Some(LogoTypeOpt::File) => {
            let Some(content) = update_logo_path(source, instance) else {
                if instance.show_errors {
                    eprintln!("Logo: Failed to resolve logo source: {source}");
                }
                return fallback();
            };
            let palette = palette_with_overrides(detected.colors, instance);
            let (lines, art_width) = parse_logo_art(&content, palette, pipe, opts.hyfetch);
            return (Some(build_chars(lines, art_width, configured, opts)), entry_colors(detected));
        },
        Some(LogoTypeOpt::FileRaw) => {
            // logoPrintFileIfExists(false, false): no `$N` replacement, but
            // the bytes still go through the chars pipeline — FILE_RAW's
            // "raw" means unprocessed palette, not unprocessed bytes.
            let Some(content) = update_logo_path(source, instance) else {
                if instance.show_errors {
                    eprintln!("Logo: Failed to resolve logo source: {source}");
                }
                return fallback();
            };
            let (lines, art_width) = parse_art(&content, None, pipe);
            return (Some(build_chars(lines, art_width, configured, opts)), entry_colors(detected));
        },
        Some(LogoTypeOpt::ImageRaw) => {
            let Some(content) = update_logo_path(source, instance) else {
                if instance.show_errors {
                    eprintln!("Logo: Failed to resolve logo source: {source}");
                }
                return fallback();
            };
            maybe_image_attempt(source, pipe);
            return (
                Some(Logo::Raw {
                    content,
                    position: resolve_position(
                        configured,
                        crate::modules::terminalsize::logo_terminal_columns(),
                        opts.width + opts.padding_left + opts.padding_right,
                    ),
                    static_layout: None,
                    cpr: Some(crate::image::ImageCpr {
                        who: "(image-raw)",
                        gated: true,
                        trailing: "\x1b[H",
                    }),
                    flush_start: false,
                }),
                entry_colors(detected),
            );
        },
        Some(
            kind @ (LogoTypeOpt::ImageSixel
            | LogoTypeOpt::ImageKitty
            | LogoTypeOpt::ImageKittyDirect
            | LogoTypeOpt::ImageKittyIcat
            | LogoTypeOpt::ImageIterm
            | LogoTypeOpt::ImageChafa),
        ) => {
            // logoTryKnownType resolves the path first; the image ladder
            // then fast-fails (pipe, missing source, TERM=screen) or runs
            // one of the pixel-free printers. Without the image payload
            // encoders, sixel/kitty/chafa report the upstream build errors.
            let Some(resolved) = resolve_logo_path(source, instance) else {
                if instance.show_errors {
                    eprintln!("Logo: Failed to resolve logo source: {source}");
                }
                return fallback();
            };
            let kind = match kind {
                LogoTypeOpt::ImageSixel => crate::image::ImageKind::Sixel,
                LogoTypeOpt::ImageKitty => crate::image::ImageKind::Kitty,
                LogoTypeOpt::ImageKittyDirect => crate::image::ImageKind::KittyDirect,
                LogoTypeOpt::ImageKittyIcat => crate::image::ImageKind::KittyIcat,
                LogoTypeOpt::ImageIterm => crate::image::ImageKind::Iterm,
                _ => crate::image::ImageKind::Chafa,
            };
            return match crate::image::print_image(kind, &resolved, configured, opts, instance) {
                Some(logo) => (Some(logo), entry_colors(detected)),
                None => fallback(),
            };
        },
        Some(LogoTypeOpt::Spin) => {
            return load_spin(Some(source), configured, opts, instance, cli, detected);
        },
        Some(LogoTypeOpt::None) | Some(LogoTypeOpt::Auto) | None => {},
    }

    if logo_small {
        // A SMALL source matches a "_small"-suffixed entry name too.
        if let Some(entry) = get_builtin(source, Size::Small) {
            return (Some(build(entry, pipe, configured, opts, instance)), entry_colors(entry));
        }
        return fallback();
    }
    if let Some(entry) = get_builtin(source, Size::Unknown) {
        return (Some(build(entry, pipe, configured, opts, instance)), entry_colors(entry));
    }
    let Some(content) = update_logo_path(source, instance) else {
        return fallback();
    };
    if source.to_ascii_lowercase().ends_with(".raw") {
        // ffLogoPrintCharsRaw resolves AUTO with the width option plus both
        // paddings before looking at the position. With width AND height
        // set the bytes dump through the static path (no clear, no cursor
        // query); otherwise RIGHT needs a width — without one the
        // printError-gated message stays hidden, the image attempt runs,
        // and the caller re-reads the file as a CHARS logo (escape
        // passthrough plus the $N palette), still position RIGHT.
        let position = resolve_position(
            configured,
            crate::modules::terminalsize::logo_terminal_columns(),
            opts.width + opts.padding_left + opts.padding_right,
        );
        if opts.width > 0 && opts.height > 0 {
            let mut prefix = Vec::new();
            prefix.extend(std::iter::repeat_n(b'\n', opts.padding_top as usize));
            if position == Position::Right {
                prefix.extend(format!("\x1b[9999999C\x1b[{}D", opts.padding_right + opts.width).into_bytes());
            } else if opts.padding_left > 0 {
                prefix.extend(format!("\x1b[{}C", opts.padding_left).into_bytes());
            }
            // The static path appends one newline of its own after the
            // bytes, before the suffix escapes.
            let mut suffix = b"\n".to_vec();
            let (logo_width, logo_height) = match position {
                Position::Left => {
                    let logo_width = (opts.width + opts.padding_left + opts.padding_right) as usize;
                    let logo_height = (opts.padding_top + opts.height) as usize;
                    suffix.extend(format!("\x1b[{logo_height}A").into_bytes());
                    (logo_width, logo_height)
                },
                Position::Top => {
                    suffix.extend(std::iter::repeat_n(b'\n', opts.padding_right as usize));
                    (0, 0)
                },
                Position::Right => {
                    suffix.extend(format!("\x1b[{}A", opts.height).into_bytes());
                    (0, 0)
                },
            };
            return (
                Some(Logo::Raw {
                    content,
                    position,
                    static_layout: Some((prefix, suffix, logo_width, logo_height)),
                    cpr: None,
                    flush_start: false,
                }),
                entry_colors(detected),
            );
        }
        if position == Position::Right && opts.width == 0 {
            maybe_image_attempt(source, pipe);
            let palette = palette_with_overrides(detected.colors, instance);
            let (lines, art_width) = parse_logo_art(&content, palette, pipe, opts.hyfetch);
            return (Some(build_chars(lines, art_width, configured, opts)), entry_colors(detected));
        }
        return (
            Some(Logo::Raw {
                content,
                position,
                static_layout: None,
                cpr: Some(crate::image::ImageCpr {
                    who: "(image-raw)",
                    gated: true,
                    trailing: "\x1b[H",
                }),
                flush_start: false,
            }),
            entry_colors(detected),
        );
    }
    maybe_image_attempt(source, pipe);
    // File logos apply the detected distro's palette to their `$N` markers
    // (logoPrintCustom's ffLogoApplyColors(logoGetBuiltinDetected(NORMAL)));
    // non-UTF-8 bytes ride through the segment parser untouched.
    let palette = palette_with_overrides(detected.colors, instance);
    let (lines, art_width) = parse_logo_art(&content, palette, pipe, opts.hyfetch);
    (Some(build_chars(lines, art_width, configured, opts)), entry_colors(detected))
}

/// spinLoadFile: resolve the source to readable file bytes through
/// ffPathExpandEnv's wordexp expansion first, then the literal path — no
/// stdin alias and no dataDirs search, unlike updateLogoPath. Empty reads
/// fail like ffAppendFileBuffer's zero length.
fn spin_load_file(source: &str) -> Option<Vec<u8>> {
    if let Some(path) = wordexp_first(source)
        && let Ok(bytes) = std::fs::read(&path)
    {
        return if bytes.is_empty() { None } else { Some(bytes) };
    }
    match std::fs::read(source) {
        Ok(bytes) if !bytes.is_empty() => Some(bytes),
        _ => None,
    }
}

/// ffSpinLogoPrint through the chars pipeline: the source is the detected
/// OS (`auto`), a builtin logo name, or a file in fastfetch's logo syntax;
/// a file borrows the detected logo's palette. Total failure reports
/// (showErrors-gated) and leaves the logo unset — there is no silent
/// fallback to the detected logo here, unlike the other source types.
/// The first frame draws immediately; the `--watch` loop redraws through
/// instance.spin.
fn load_spin(
    source: Option<&str>,
    configured: PositionOpt,
    opts: &LogoOptions,
    instance: &Instance,
    cli: &Cli,
    detected: &LogoEntry,
) -> (Option<Logo>, LogoColors) {
    let source = source.unwrap_or("");
    let pipe = instance.pipe;
    let is_auto = source.is_empty() || source.eq_ignore_ascii_case("auto");
    let entry = if is_auto {
        Some(get_builtin_detected(instance, Size::Normal))
    } else {
        get_builtin(source, Size::Normal).or_else(|| get_builtin(source, Size::Unknown))
    };
    let file_text = if entry.is_none() { spin_load_file(source) } else { None };
    if entry.is_none() && file_text.is_none() && instance.show_errors {
        eprintln!("Logo (spin): no builtin logo or readable file named {source}");
    }

    // `logo.color.N` on top of the logo's own colors; with no logo the
    // detected logo's colors apply (ffLogoApplyColors's replacement pass).
    let colors_entry = entry.unwrap_or(detected);
    let colors = entry_colors(colors_entry);
    let palette = palette_with_overrides(colors_entry.colors, instance);

    let text: Vec<u8> = match (&entry, file_text) {
        (Some(entry), _) => builtin_art(entry).as_bytes().to_vec(),
        (None, Some(text)) => text,
        (None, None) => Vec::new(), // the parse fails below, silently
    };
    let Some(art) = crate::spin::spin_art_parse(&text, palette.unwrap_or(&[])) else {
        return (None, colors);
    };

    let animated = cli.watch_interval > 0;
    let mut state = crate::spin::SpinState::new(art, configured, opts, animated, pipe, instance.display.bright_color);
    let Some(draw) = state.draw() else {
        return (None, colors);
    };
    *instance.spin.borrow_mut() = Some(state);

    // Hand the frame to the chars pipeline: it pads, positions and
    // measures the art, and its line cache is what puts the modules next
    // to the logo. `None` palette: the frame already carries its own SGR
    // sequences.
    let (lines, art_width) = parse_art(draw.ansi.as_bytes(), None, pipe);
    (Some(build_chars(lines, art_width, configured, opts)), colors)
}

/// Rebuilds the spin logo's chars cache from a fresh frame's ANSI text —
/// logoLineCacheBuild runs on every ffSpinLogoPrint, so the watch loop's
/// redraw reassembles the whole cache (paddings, width/height floors,
/// position resolution included).
pub(crate) fn rebuild_spin_chars(
    ansi: &str,
    configured: PositionOpt,
    opts: &LogoOptions,
    pipe: bool,
) -> Logo {
    let (lines, art_width) = parse_art(ansi.as_bytes(), None, pipe);
    build_chars(lines, art_width, configured, opts)
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
pub(crate) fn print_logos(opts: &LogoOptions) {
    let pipe = !std::io::stdout().is_terminal() || std::env::var_os("NO_COLOR").is_some();
    for entry in logo_entries::LOGO_ENTRIES {
        if pipe {
            println!("{}:", entry.names[0]);
        } else {
            println!("\x1b[{}m{}:\x1b[0m", entry.colors[0], entry.names[0]);
        }
        let (lines, art_width) = parse_art(builtin_art(entry).as_bytes(), Some(entry.colors), pipe);
        // ffLogoBuiltinPrint forces position top before each dump; the
        // parsed-so-far paddings and floors still apply to every cache.
        let logo = build_chars(lines, art_width, PositionOpt::Top, opts);
        let Logo::Chars { lines, pad_left, bare, .. } = &logo else { unreachable!() };
        use std::io::Write;
        let mut stdout = std::io::stdout().lock();
        for (line, _) in crate::render::colorize_cache(lines, !pipe, *pad_left, *bare) {
            let _ = stdout.write_all(&line);
            let _ = stdout.write_all(b"\n");
        }
        let _ = stdout.flush();
    }
}

fn putchar_newline() {
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg_text(line: &[Segment]) -> String {
        String::from_utf8_lossy(&line.iter().flat_map(|s| s.text.iter().copied()).collect::<Vec<u8>>()).into_owned()
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
        let (lines, width) = parse_art("  $1abc$2def\n  carried".as_bytes(), Some(colors), false);
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
        let (lines, width) = parse_art(b"ab$2\ncd", Some(colors), false);
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
        let (lines, _) = parse_art(b"$1a$0b$2c", Some(colors), false);
        // each marker starts a segment, even with empty text before it
        assert_eq!(seg_codes(&lines[0]), ["31", "31", ""]);
        assert_eq!(seg_text(&lines[0]), "a$0bc");
    }

    #[test]
    fn parse_art_dollar_without_digit_is_literal() {
        let colors: &'static [&'static str] = &["31"];
        let (lines, _) = parse_art(b"a$ b$1", Some(colors), false);
        assert_eq!(seg_text(&lines[0]), "a$ b");
        assert_eq!(seg_codes(&lines[0]), ["31", "31"]);
    }

    #[test]
    fn parse_art_tab_widens_and_escape_carries_without_width() {
        let colors: &'static [&'static str] = &["36"];
        let (lines, width) = parse_art(b"A\tB\n\x1b[31mBBB\nCCC", Some(colors), false);
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
        let (lines, width) = parse_art(b"ab$2cd$1", Some(colors), true);
        assert_eq!(seg_text(&lines[0]), "abcd");
        assert_eq!(width, 4);
        // no carry seed in pipe mode: the line has no code segments
        assert!(lines[0].iter().all(|seg| seg.color == SegmentColor::Pass));
    }

    #[test]
    fn hyfetch_gradient_art_strips_markers_and_keeps_escaped_dollars() {
        let art = b"  $1ab$2\tcd$$x\n$9\r\n$3";
        let rewritten = hyfetch_gradient_art(art);
        assert_eq!(rewritten, b"$1  ab\tcd$$x\n$2\r\n$3");
    }

    #[test]
    fn hyfetch_gradient_art_cycles_nine_slots() {
        let art = b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj";
        let rewritten = hyfetch_gradient_art(art);
        assert_eq!(rewritten, b"$1a\n$2b\n$3c\n$4d\n$5e\n$6f\n$7g\n$8h\n$9i\n$1j");
    }

    #[test]
    fn hyfetch_gradient_art_preserves_width_and_line_count() {
        let art = b"ab$1cd\n\t$2ef \r\n";
        let colors: &'static [&'static str] = &["31", "34", "36"];
        let (plain, plain_width) = parse_art(art, Some(colors), false);
        let (gradient, gradient_width) = parse_logo_art(art, Some(colors), false, true);
        assert_eq!(gradient.len(), plain.len());
        assert_eq!(gradient_width, plain_width);
        // The trailing newline is still a trailing empty line, now with a
        // marker of its own (width 0), so it stays empty.
        let last = gradient.len() - 1;
        assert!(seg_text(&gradient[last]).is_empty());
    }

    #[test]
    fn parse_logo_art_gradient_colors_every_line() {
        let colors: &'static [&'static str] = &["1", "2", "3", "4", "5", "6", "7", "8", "9"];
        let art = b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj";
        let (lines, _) = parse_logo_art(art, Some(colors), false, true);
        assert_eq!(lines.len(), 10);
        for (index, line) in lines.iter().enumerate() {
            // The leading marker flushes an empty carry segment first; the
            // last segment is the line's own palette color.
            assert_eq!(seg_codes(line).last().copied(), Some(colors[index % 9]));
            assert_eq!(seg_text(line), if index == 9 { "j" } else { &"abcdefghi"[index..index + 1] });
        }
    }

    #[test]
    fn parse_logo_art_flag_off_raw_and_pipe_are_inert() {
        let colors: &'static [&'static str] = &["31", "34"];
        let art = b"$1a$2b\n$1c";
        let (plain, plain_width) = parse_art(art, Some(colors), false);
        let (off, off_width) = parse_logo_art(art, Some(colors), false, false);
        assert_eq!(seg_text(&off[0]), seg_text(&plain[0]));
        assert_eq!(seg_codes(&off[0]), seg_codes(&plain[0]));
        assert_eq!(off_width, plain_width);

        // Pipe mode and raw logos keep their bytes untouched even when the
        // HyFetch flag is set.
        let (pipe_on, _) = parse_logo_art(art, Some(colors), true, true);
        let (pipe_off, _) = parse_art(art, Some(colors), true);
        assert_eq!(seg_text(&pipe_on[0]), seg_text(&pipe_off[0]));
        let (raw_on, raw_width) = parse_logo_art(art, None, false, true);
        let (raw_off, raw_off_width) = parse_art(art, None, false);
        assert_eq!(seg_text(&raw_on[0]), seg_text(&raw_off[0]));
        assert_eq!(raw_width, raw_off_width);
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

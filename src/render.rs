//! Two-column terminal renderer: logo on the left, info rows on the right.
//! Replicates fastfetch's exact escape sequence emission: one global reset
//! (ffStart), logo lines built as bold + per-segment colors + reset, keys
//! as reset + bold + key color + key + reset + ": " + reset, and a final
//! reset after the logo's remaining lines (ffLogoPrintRemaining). Watch
//! mode (`-w`) reruns the row collection in the alternate screen buffer
//! like fastfetch.c's run() loop.

use std::io::{BufWriter, Write};

use crate::json::{self, Json, JsonOutcome};
use crate::logo::{Position, Segment, SegmentColor};
use crate::{instance::Instance, logo::Logo, modules::JsonItem, modules::Kind, modules::Row};

/// The cache colorize: every parsed line starts bolt + running carry +
/// the paddingLeft prefix + segments, widening the line by the prefix —
/// except the cache's bare-empty bookends (paddingTop front, height-fill
/// back), pushed via logoLineCachePush(nullptr, 0) upstream and always
/// empty, and any width-zero line, whose bytes logoLineCachePush discards.
/// The carry at a line's start is its first segment's color when it has
/// segments (parse_art flushes the carried color there), else the color
/// flowing past the blank line — the last carry-updating segment's color,
/// or the seed (the first content line's carry) before any content.
pub(crate) fn colorize_cache(
    lines: &[Vec<Segment>],
    color: bool,
    pad_left: usize,
    bare: (usize, usize),
) -> Vec<(Vec<u8>, usize)> {
    let mut carry: Option<&SegmentColor> = None;
    let seed = lines
        .iter()
        .find(|line| !line.is_empty())
        .and_then(|line| line.first())
        .map(|seg| &seg.color);
    let updates_carry = |color: &SegmentColor| -> bool {
        match color {
            SegmentColor::Code(_) => true,
            SegmentColor::Esc(bytes) => bytes.ends_with('m'),
            SegmentColor::Pass => false,
        }
    };
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let bare_entry = index < bare.0 || index >= lines.len() - bare.1;
            // The display width: wcwidth per decoded sequence, not the
            // byte count (wide logo glyphs count 2, combining ones 0,
            // invalid bytes 1).
            let content_width = line
                .iter()
                .map(|seg| crate::wcwidth::utf8_bytes_width(&seg.text) as usize)
                .sum::<usize>();
            let width = if bare_entry { 0 } else { pad_left + content_width };
            if width == 0 {
                return (Vec::new(), 0);
            }
            // The carry escape: the line's own first segment carries it —
            // emit the spaces after that escape and skip the duplicate.
            let carry_here = if line.is_empty() {
                carry.or(seed)
            } else {
                line.first().map(|seg| &seg.color)
            };
            let mut out = Vec::new();
            if color {
                out.extend_from_slice(b"\x1b[1m");
            }
            let mut skip_first_escape = false;
            if let Some(carry_color) = carry_here {
                match carry_color {
                    SegmentColor::Code(code) => {
                        out.extend_from_slice(b"\x1b[");
                        out.extend_from_slice(code.as_bytes());
                        out.push(b'm');
                        skip_first_escape = true;
                    },
                    SegmentColor::Esc(bytes) => {
                        out.extend_from_slice(bytes.as_bytes());
                        skip_first_escape = true;
                    },
                    SegmentColor::Pass => {},
                }
            }
            out.extend(std::iter::repeat_n(b' ', pad_left));
            for (position, seg) in line.iter().enumerate() {
                if position == 0 && skip_first_escape {
                    out.extend_from_slice(&seg.text);
                    continue;
                }
                match &seg.color {
                    SegmentColor::Code(code) => {
                        out.extend_from_slice(b"\x1b[");
                        out.extend_from_slice(code.as_bytes());
                        out.push(b'm');
                    },
                    SegmentColor::Esc(bytes) => out.extend_from_slice(bytes.as_bytes()),
                    SegmentColor::Pass => {},
                }
                out.extend_from_slice(&seg.text);
            }
            if color {
                out.extend_from_slice(b"\x1b[m");
            }
            if let Some(last) = line.last()
                && updates_carry(&last.color)
            {
                carry = Some(&last.color);
            }
            (out, width)
        })
        .collect()
}

/// The key emission inputs: the resolved FFOptionsDisplay surface the
/// rows print against, shared by every branch of one render pass.
#[derive(Clone, Copy)]
struct KeyStyle<'a> {
    /// !pipe: the reset/color escapes only go out on a terminal.
    color: bool,
    bright_color: bool,
    /// The effective display.colorKeys / colorOutput / colorSeparator
    /// (ffLogoApplyColors-resolved), empty = print no escape.
    color_keys: &'a str,
    color_output: &'a str,
    color_separator: &'a str,
    /// display.keyValueSeparator, verbatim.
    separator: &'a str,
    key_width: u16,
    key_padding_left: u16,
    key_type: u8,
}

/// ffPrintLogoAndKey's body after the logo line, ported line for line:
/// the key block (gated on keyType != NONE — the title/colors/custom
/// modules hide it with the `" "` magic key) and the unconditional
/// reset+outputColor tail. `logo_width` is instance.state.logoWidth at
/// the call site; the `\e[{n}G` jump and the padding spaces run in pipe
/// mode too. The row's common module args override the display slots
/// per ffPrintLogoAndKey: keyColor over colorKeys, keyWidth over
/// display.keyWidth, keyIcon over the module default, outputColor over
/// colorOutput, and a custom `key` replaces the built-in key text (the
/// magic `" "` hides the whole block).
fn key_prefix(row: &Row, style: &KeyStyle<'_>, logo_width: usize) -> String {
    let common = row.common.as_ref();
    let raw_key = common.and_then(|args| args.key.as_deref());
    // The magic `" "` key hides the whole key block, tail included.
    let hidden = raw_key == Some(" ");
    // A set custom key replaces the text on rows that took the expansion
    // (collect stamps key_override; the NO_CUSTOM_KEY error sites didn't
    // and fall back to their built-in key).
    let custom = raw_key.is_some_and(|key| !key.is_empty() && key != " ");
    let key_text = if hidden {
        None
    } else if custom {
        row.key_override.as_deref().or(row.key.as_deref())
    } else {
        row.key.as_deref()
    };
    let mut out = String::new();
    if let Some(key) = key_text {
        if style.key_type != crate::instance::KEY_TYPE_NONE {
            for _ in 0..style.key_padding_left {
                out.push(' ');
            }
            if style.color {
                out.push_str("\x1b[m");
                if style.bright_color {
                    out.push_str("\x1b[1m");
                }
                let color_keys = common
                    .and_then(|args| args.key_color.as_deref())
                    .filter(|code| !code.is_empty())
                    .unwrap_or(style.color_keys);
                if !color_keys.is_empty() {
                    out.push_str(&format!("\x1b[{color_keys}m"));
                }
            }
            // The key-icon bits print moduleArgs->keyIcon: the entry's
            // keyIcon replaces the module default (an empty string clears
            // it, like upstream's ffStrbufSetJsonVal clearing the buffer).
            let key_icon = match common.and_then(|args| args.key_icon.as_deref()) {
                Some(icon) => icon,
                None => row.key_icon,
            };
            if style.key_type & crate::instance::KEY_TYPE_ICON != 0 && !key_icon.is_empty() {
                out.push_str(key_icon);
            }
            if style.key_type & crate::instance::KEY_TYPE_STRING != 0 {
                for _ in 0..(style.key_type >> crate::instance::KEY_TYPE_SPACE_SHIFT) {
                    out.push(' ');
                }
                out.push_str(key);
            }
            if style.color {
                out.push_str("\x1b[m");
                if !style.color_separator.is_empty() {
                    out.push_str(&format!("\x1b[{}m", style.color_separator));
                }
            }
            out.push_str(style.separator);
            if style.color && !style.color_separator.is_empty() {
                out.push_str("\x1b[m");
            }
            let key_width = common
                .map(|args| args.key_width)
                .filter(|width| *width > 0)
                .unwrap_or(style.key_width as u32);
            if key_width > 0 {
                // u32 arithmetic like upstream's printf("%u", keyWidth + logoWidth).
                out.push_str(&format!("\x1b[{}G", key_width.wrapping_add(logo_width as u32)));
            }
        }
    }
    if style.color {
        out.push_str("\x1b[m");
        let color_output = common
            .and_then(|args| args.output_color.as_deref())
            .filter(|code| !code.is_empty())
            .unwrap_or(style.color_output);
        if !color_output.is_empty() {
            out.push_str(&format!("\x1b[{color_output}m"));
        }
    }
    out
}

/// A late row's detection, run at print time; the placeholder carries the
/// entry's common args, which stamp onto the rows it expands into.
fn late_rows(row: &Row, instance: &Instance) -> Vec<Row> {
    let Kind::Late(late, module) = row.kind else {
        return Vec::new();
    };
    let mut produced = late(instance);
    if let Some(args) = row.common.as_ref() {
        crate::modules::stamp_common(&mut produced, args, module, instance);
    }
    produced
}

/// Renders a single info row; `Kind::Raw` values are printed verbatim because
/// modules embed their own escapes there.
fn row_line(row: &Row, style: &KeyStyle<'_>, logo_width: usize) -> String {
    match (row.key.as_deref(), row.kind) {
        (Some(_), Kind::Value) => {
            let mut out = key_prefix(row, style, logo_width);
            out.push_str(&row.value);
            out
        },
        // ffPrintError's message: bold red outside pipe mode, plain in it.
        (_, Kind::Error) => {
            let mut out = key_prefix(row, style, logo_width);
            if style.color {
                out.push_str("\x1b[1;31m");
            }
            out.push_str(&row.value);
            if style.color {
                out.push_str("\x1b[m");
            }
            out
        },
        // Keyless styled rows (title, colors): the module owns the value's
        // escapes; the renderer adds the tail reset (and outputColor) of
        // the key writer.
        (None, Kind::Value) => {
            let mut out = key_prefix(row, style, logo_width);
            out.push_str(&row.value);
            out
        },
        (_, Kind::Raw) => row.value.clone(),
        // Late rows expand into regular ones before reaching here.
        (_, Kind::Late(..)) => String::new(),
    }
}

/// The fixed two-column layout of one Chars render, shared by the row
/// emissions: the key column the rows start at, the key emission style,
/// and the resolved logo position.
struct CharsLayout<'a> {
    key_column: usize,
    /// instance.state.logoWidth: the key column for LEFT, zero for
    /// RIGHT/TOP (logoLineCacheBuild); the `\e[{n}G` jump adds it.
    logo_width: usize,
    style: KeyStyle<'a>,
    position: Position,
    /// RIGHT: how far the art sits from the right edge (maxLineWidth +
    /// paddingRight - 1, the cache's rightOffset).
    right_offset: usize,
}

/// Prints one info row against the next unconsumed logo line, standing in
/// for ffPrintLogoAndKey's ffLogoPrintLine + key sequence.
fn emit_chars_row(
    out: &mut BufWriter<std::io::StdoutLock<'_>>,
    logo_lines: &[(Vec<u8>, usize)],
    logo_index: &mut usize,
    layout: &CharsLayout<'_>,
    row: &Row,
    watch: bool,
) {
    if !logo_lines.is_empty() {
        match layout.position {
            Position::Right => {
                // ffLogoPrintLine's RIGHT branch: jump to the right edge, step
                // left onto the art, print it, and return to column 0 for the
                // key. Rows past the logo print nothing for the logo part.
                if let Some((text, _)) = logo_lines.get(*logo_index) {
                    let _ = write!(out, "\x1b[9999999C\x1b[{}D", layout.right_offset);
                    let _ = out.write_all(text);
                    let _ = write!(out, "\x1b[G");
                }
            },
            _ => {
                if let Some((text, width)) = logo_lines.get(*logo_index) {
                    let _ = out.write_all(text);
                    let pad = layout.key_column.saturating_sub(*width);
                    if pad > 0 {
                        let _ = out.write_all(&vec![b' '; pad]);
                    }
                } else if layout.position == Position::Left {
                    // Cache exhausted: the key column fills with spaces so
                    // the two-column alignment holds.
                    let pad = layout.key_column;
                    if pad > 0 {
                        let _ = out.write_all(&vec![b' '; pad]);
                    }
                }
            },
        }
    } else if layout.position == Position::Left && layout.key_column > 0 {
        // Cache cleared (watch rounds after the first): the cursor moves
        // right past the logo instead of reprinting spaces over it.
        let _ = write!(out, "\x1b[{}C", layout.key_column);
    }
    if watch && layout.position == Position::Left {
        // ffLogoPrintLine's dynamic-interval clear keeps short rows from
        // leaving old content past their new end. RIGHT skips it, as
        // clearing to end of line would erase the logo itself.
        let _ = write!(out, "\x1b[K");
    }
    *logo_index += 1;
    let _ = writeln!(out, "{}", row_line(row, &layout.style, layout.logo_width));
}

/// One watch round's close: clear from the cursor down, flush, sleep the
/// interval, home the cursor, and drop the netif default-route cache —
/// ffCacheInvalidateAll's only registrants — so the next round re-detects
/// it. The spin logo's per-round reprint runs right after this in the
/// callers (`if (ffSpinTick()) ffLogoPrint()`).
fn watch_pause(out: &mut BufWriter<std::io::StdoutLock<'_>>, interval_ms: u32) {
    let _ = write!(out, "\x1b[J");
    let _ = out.flush();
    std::thread::sleep(std::time::Duration::from_millis(interval_ms as u64));
    let _ = write!(out, "\x1b[H");
    crate::modules::netio::invalidate_route_cache();
}

/// ffPrintCommandOption's display.stat tail: save, up one line, right
/// edge, left len-1, print "N.NNNms" (green/yellow/red above a positive
/// threshold), restore. The left-count uses the plain string's length
/// even when the colored variant prints — upstream's quirk, kept.
fn stat_line(stat: i32, ms: f64) -> String {
    let plain = format!("{:.3}ms", ms);
    let body = if stat > 0 {
        let color = if ms <= stat as f64 {
            "32"
        } else if ms <= 2.0 * stat as f64 {
            "33"
        } else {
            "31"
        };
        format!("\x1b[{color}m{plain}\x1b[m")
    } else {
        plain.clone()
    };
    format!("\x1b7\x1b[1A\x1b[9999999C\x1b[{}D{}\x1b8", plain.len() - 1, body)
}

/// Walks the stat boundaries (row counts when each module finished),
/// emitting the timing line as the row printer catches up with them.
struct StatCursor<'a> {
    entries: &'a [(usize, f64)],
    next: usize,
    stat: i32,
}

impl StatCursor<'_> {
    fn flush(&mut self, out: &mut BufWriter<std::io::StdoutLock<'_>>, printed: usize) {
        while self.next < self.entries.len() && self.entries[self.next].0 <= printed {
            let (_, ms) = self.entries[self.next];
            let _ = write!(out, "{}", stat_line(self.stat, ms));
            self.next += 1;
        }
    }
}

pub fn print(
    logo: &Option<Logo>,
    instance: &Instance,
    watch: Option<u32>,
    next_round: &mut dyn FnMut() -> (Vec<Row>, Vec<(usize, f64)>),
) {
    // Pipe mode keeps the two-column layout; it only drops the colors,
    // which every writer below gates on instance.color.
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let color = instance.color;
    let style = KeyStyle {
        color,
        bright_color: instance.display.bright_color,
        color_keys: instance.key_color().unwrap_or(""),
        color_output: &instance.display.color_output,
        color_separator: &instance.display.color_separator,
        separator: &instance.display.key_value_separator,
        key_width: instance.display.key_width,
        key_padding_left: instance.display.key_padding_left,
        key_type: instance.display.key_type,
    };
    let opts = &instance.logo_options;

    // ffStart (init.c:111): the buffered reset, then the pipe-disabled
    // cursor/linewrap toggles; watch mode additionally enables the
    // alternate screen buffer, homes the cursor and flushes. Built once —
    // ffStart runs a single time per process. The image printers flush
    // all of it ahead of their raw dumps instead (upstream's fflush; a
    // .raw file keeps the reset buffered behind the dump — observable
    // wire order).
    let flushes_start = matches!(logo.as_ref(), Some(Logo::Raw { flush_start: true, .. }));
    let mut start_bytes = Vec::new();
    if color {
        start_bytes.extend_from_slice(b"\x1b[m");
    }
    if instance.hide_cursor {
        start_bytes.extend_from_slice(b"\x1b[?25l");
    }
    if instance.disable_linewrap {
        start_bytes.extend_from_slice(b"\x1b[?7l");
    }
    if watch.is_some() {
        start_bytes.extend_from_slice(b"\x1b[?1049h\x1b[H");
    }
    if flushes_start {
        instance.ff_started.set(false);
    } else if !instance.ff_started.get() {
        let _ = out.write_all(&start_bytes);
        if watch.is_some() {
            let _ = out.flush();
        }
        instance.ff_started.set(true);
    }

    match logo.as_ref() {
        Some(Logo::Raw { content, position, static_layout, cpr, flush_start }) => {
            let (mut logo_width, mut logo_height) = (0usize, 0usize);
            // ffStart's reset (+ the toggles and the watch alternate
            // screen) goes on the wire ahead of the dump for the image
            // printers only; a .raw file leaves it buffered behind
            // (upstream has no fflush).
            let mut ff_start = Vec::new();
            if *flush_start && !instance.ff_started.get() {
                ff_start = start_bytes.clone();
                instance.ff_started.set(true);
            }
            match static_layout {
                Some((prefix, suffix, width, height)) => {
                    // The width+height static path: no clear, no cursor
                    // query; ffStart + prefix + bytes + suffix go out as one
                    // write.
                    let mut dump = ff_start.clone();
                    dump.extend_from_slice(prefix);
                    dump.extend_from_slice(content);
                    dump.extend_from_slice(suffix);
                    unsafe {
                        libc::write(libc::STDOUT_FILENO, dump.as_ptr().cast(), dump.len());
                    }
                    logo_width = *width;
                    logo_height = *height;
                },
                None if *position == Position::Top => {
                    // ffLogoPrintCharsRaw's TOP branch: paddingTop newlines
                    // and paddingLeft spaces, then the bytes unprocessed;
                    // logoWidth/logoHeight stay zero and paddingRight
                    // newlines follow on the buffered stdout.
                    let mut dump = ff_start.clone();
                    dump.extend(std::iter::repeat_n(b'\n', opts.padding_top as usize));
                    dump.extend(std::iter::repeat_n(b' ', opts.padding_left as usize));
                    dump.extend_from_slice(content);
                    unsafe {
                        libc::write(libc::STDOUT_FILENO, dump.as_ptr().cast(), dump.len());
                    }
                    for _ in 0..opts.padding_right {
                        let _ = writeln!(out);
                    }
                },
                None => {
                    // ffLogoPrintCharsRaw, LEFT/RIGHT positions: clear the
                    // screen, home the cursor (RIGHT jumps to the right edge
                    // minus the width) and dump the file bytes unprocessed —
                    // regardless of the color mode, like upstream. Printed
                    // once; watch rounds only re-render the rows.
                    let mut dump = if *position == Position::Right {
                        format!(
                            "\x1b[2J\x1b[3J\x1b[{};9999999H\x1b[{}D",
                            opts.padding_top + 1,
                            opts.padding_right + opts.width
                        )
                    } else {
                        format!("\x1b[2J\x1b[3J\x1b[{};{}H", opts.padding_top + 1, opts.padding_left + 1)
                    }
                    .into_bytes();
                    dump.extend_from_slice(content);
                    // One raw write to fd 1: ffStart's reset goes ahead of
                    // it (upstream's image printers fflush first), and the
                    // query below writes to /dev/tty.
                    let mut with_start = ff_start.clone();
                    with_start.extend_from_slice(&dump);
                    unsafe {
                        libc::write(libc::STDOUT_FILENO, with_start.as_ptr().cast(), with_start.len());
                    }
                    // The cursor query learns where the dump ended: X (raised
                    // to width+paddingLeft, both unparsed) gives the key
                    // column, Y the logo height. On failure both stay at
                    // zero — no trailing move — and the query error prints
                    // per the printer's gate (the .raw message is gated;
                    // iterm's is not, upstream).
                    let Some(cpr) = cpr else {
                        return;
                    };
                    match crate::tty::response_with(b"\x1b[6n", crate::tty::parse_cpr) {
                        Ok((y, x)) => {
                            let mut x = x as usize;
                            // The width option raises X when the dump ends
                            // left of the configured key column; the key
                            // column is learned only for LEFT — RIGHT leaves
                            // logoWidth at zero so the rows print at
                            // column 0.
                            if *position == Position::Left {
                                let want = opts.width as usize + opts.padding_left as usize;
                                if want > x {
                                    x = want;
                                }
                                logo_width = x + opts.padding_right as usize - 1;
                            }
                            logo_height = y as usize;
                            let _ = write!(out, "{}", cpr.trailing);
                        },
                        Err(error) => {
                            // The message is gated per printer (iterm's is
                            // ungated upstream); the rows still print with
                            // the key column at zero.
                            if !cpr.gated || instance.show_errors {
                                let _ = out.flush();
                                eprintln!("\nLogo {}: fail to query cursor position: {error}", cpr.who);
                            }
                        },
                    }
                },
            }
            let mut keys_height;
            loop {
                let (rows, stats) = next_round();
                let mut cursor = StatCursor { entries: &stats, next: 0, stat: instance.stat };
                keys_height = 0;
                let mut printed = 0usize;
                cursor.flush(&mut out, printed);
                for row in &rows {
                    if let Kind::Late(..) = row.kind {
                        let _ = out.flush();
                        for produced in late_rows(row, instance) {
                            keys_height += 1;
                            if logo_width > 0 {
                                let _ = write!(out, "\x1b[{}C", logo_width);
                            }
                            if watch.is_some() && *position == Position::Left {
                                let _ = write!(out, "\x1b[K");
                            }
                            let _ = writeln!(out, "{}", row_line(&produced, &style, logo_width));
                        }
                        printed += 1;
                        cursor.flush(&mut out, printed);
                        continue;
                    }
                    keys_height += 1;
                    // ffLogoPrintLine with an empty cache: the cursor moves
                    // right by logoWidth instead of printing spaces.
                    if logo_width > 0 {
                        let _ = write!(out, "\x1b[{}C", logo_width);
                    }
                    if watch.is_some() && *position == Position::Left {
                        let _ = write!(out, "\x1b[K");
                    }
                    let _ = writeln!(out, "{}", row_line(row, &style, logo_width));
                    printed += 1;
                    cursor.flush(&mut out, printed);
                }
                cursor.flush(&mut out, printed);
                // A deferred color exit aborts here: upstream's structure
                // loop dies mid-print, before ffLogoPrintRemaining.
                if let Some((code, message)) = instance.pending_exit.borrow_mut().take() {
                    let _ = out.flush();
                    eprintln!("{message}");
                    std::process::exit(code);
                }
                // ffLogoPrintRemaining's empty-cache tail: one newline per
                // remaining logo height, re-run every watch round against
                // the reset keysHeight.
                if keys_height <= logo_height {
                    for _ in 0..logo_height - keys_height + 1 {
                        let _ = writeln!(out);
                    }
                }
                match watch {
                    None => break,
                    Some(interval_ms) => watch_pause(&mut out, interval_ms),
                }
            }
        },
        raw_or_chars => {
            let (lines, chars_width, pad_left, bare, position) = match raw_or_chars {
                Some(Logo::Chars { lines, width, pad_left, bare, position }) => {
                    (Some(lines), *width, *pad_left, *bare, *position)
                },
                _ => (None, 0, 0, (0, 0), Position::Left),
            };
            // logoLineCacheBuild's cache shape: the logo.rs build already
            // assembled the full cache (paddingTop front, art, height-fill
            // back) and floored the width, so both the TOP dump and the
            // LEFT/RIGHT line pairing draw from one cache.
            let mut logo_lines: Vec<(Vec<u8>, usize)> = match lines {
                Some(lines) => colorize_cache(lines, color, pad_left, bare),
                None => Vec::new(),
            };
            let has_logo = lines.is_some();
            if position == Position::Top {
                // ffLogoPrintChars' TOP path: the whole cache dumps through a
                // raw write(2) — bypassing the buffered ffStart reset, like
                // the raw-logo dump — then logoWidth/logoHeight zero out, the
                // cache clears, and every round's rows print at column 0.
                if has_logo {
                    // ffSpinLogoPrint flushes stdout before handing its frame
                    // to ffLogoPrintChars, so the pending ffStart reset lands
                    // ahead of the dump; the other logo types keep it
                    // buffered behind the dump.
                    if instance.spin.borrow().is_some() {
                        let _ = out.flush();
                    }
                    let mut dump = Vec::new();
                    for (text, _) in &logo_lines {
                        dump.extend_from_slice(text);
                        dump.push(b'\n');
                    }
                    // paddingBottom newlines close the dump, in the same
                    // buffer.
                    dump.extend(std::iter::repeat_n(b'\n', opts.padding_bottom as usize));
                    unsafe {
                        libc::write(libc::STDOUT_FILENO, dump.as_ptr().cast(), dump.len());
                    }
                }
                logo_lines.clear();
                let mut keys_height;
                let mut first_round = true;
                loop {
                    if first_round {
                        first_round = false;
                    } else if let Some((clear, logo)) = crate::spin::watch_redraw(instance) {
                        // fastfetch.c's watch loop: `if (ffSpinTick())
                        // ffLogoPrint()` — one rotation step per refresh, and
                        // the turned frame dumps over the previous one.
                        if clear {
                            let _ = write!(out, "\x1b[2J\x1b[H");
                        }
                        let _ = out.flush();
                        let Logo::Chars { lines: new_lines, pad_left, bare, .. } = logo else {
                            unreachable!()
                        };
                        let mut dump = Vec::new();
                        for (text, _) in colorize_cache(&new_lines, color, pad_left, bare) {
                            dump.extend_from_slice(&text);
                            dump.push(b'\n');
                        }
                        dump.extend(std::iter::repeat_n(b'\n', opts.padding_bottom as usize));
                        unsafe {
                            libc::write(libc::STDOUT_FILENO, dump.as_ptr().cast(), dump.len());
                        }
                    }
                    let (rows, stats) = next_round();
                    let mut cursor = StatCursor { entries: &stats, next: 0, stat: instance.stat };
                    keys_height = 0;
                    let mut printed = 0usize;
                    cursor.flush(&mut out, printed);
                    for row in &rows {
                        if let Kind::Late(..) = row.kind {
                            let _ = out.flush();
                            for produced in late_rows(row, instance) {
                                keys_height += 1;
                                let _ = writeln!(out, "{}", row_line(&produced, &style, 0));
                            }
                            printed += 1;
                            cursor.flush(&mut out, printed);
                            continue;
                        }
                        keys_height += 1;
                        let _ = writeln!(out, "{}", row_line(row, &style, 0));
                        printed += 1;
                        cursor.flush(&mut out, printed);
                    }
                    cursor.flush(&mut out, printed);
                    // The deferred color exit aborts before the tail.
                    if let Some((code, message)) = instance.pending_exit.borrow_mut().take() {
                        let _ = out.flush();
                        eprintln!("{message}");
                        std::process::exit(code);
                    }
                    // ffLogoPrintRemaining's cleared-cache tail: keysHeight <=
                    // logoHeight (0) closes with one newline when the round
                    // printed nothing.
                    if keys_height == 0 {
                        let _ = writeln!(out);
                    }
                    match watch {
                        None => break,
                        Some(interval_ms) => watch_pause(&mut out, interval_ms),
                    }
                }
            } else {
            let key_column = if has_logo {
                chars_width + opts.padding_right as usize
            } else {
                0
            };
            // instance.state.logoHeight for the tail: the cache's entry
            // count is exactly logoHeight + 1.
            let mut logo_height = if has_logo { logo_lines.len().saturating_sub(1) } else { 0 };
            let mut layout = CharsLayout {
                key_column,
                logo_width: if position == Position::Left { key_column } else { 0 },
                style,
                position,
                right_offset: chars_width + opts.padding_right as usize - 1,
            };

            let mut logo_index = 0usize;
            let mut keys_height;
            let mut first_round = true;
            loop {
                if first_round {
                    first_round = false;
                } else if let Some((clear, logo)) = crate::spin::watch_redraw(instance) {
                    // fastfetch.c's watch loop: `if (ffSpinTick())
                    // ffLogoPrint()` — logoLineCacheBuild rebuilt the cache
                    // from the turned frame, and the next round's rows print
                    // against it.
                    if clear {
                        let _ = write!(out, "\x1b[2J\x1b[H");
                    }
                    let Logo::Chars { lines: new_lines, width: new_width, pad_left, bare, .. } = logo
                    else {
                        unreachable!()
                    };
                    logo_lines = colorize_cache(&new_lines, color, pad_left, bare);
                    layout.key_column = new_width + opts.padding_right as usize;
                    // The spin frames flow through ffLogoPrintChars, so the
                    // LEFT logoWidth tracks the turned frame's width too.
                    layout.logo_width = if position == Position::Left { layout.key_column } else { 0 };
                    layout.right_offset = new_width + opts.padding_right as usize - 1;
                    logo_height = logo_lines.len().saturating_sub(1);
                    logo_index = 0;
                }
                let (rows, stats) = next_round();
                let mut cursor = StatCursor { entries: &stats, next: 0, stat: instance.stat };
                keys_height = 0;
                let mut printed = 0usize;
                cursor.flush(&mut out, printed);
                for row in &rows {
                    if let Kind::Late(..) = row.kind {
                        // Deferred detection runs at the row's position: flush
                        // the rows already rendered so the detection's /dev/tty
                        // queries land in the stream like upstream's print-time
                        // ffDetect calls.
                        let _ = out.flush();
                        for produced in late_rows(row, instance) {
                            keys_height += 1;
                            emit_chars_row(&mut out, &logo_lines, &mut logo_index, &layout, &produced, watch.is_some());
                        }
                        printed += 1;
                        cursor.flush(&mut out, printed);
                        continue;
                    }
                    keys_height += 1;
                    emit_chars_row(&mut out, &logo_lines, &mut logo_index, &layout, row, watch.is_some());
                    printed += 1;
                    cursor.flush(&mut out, printed);
                }
                cursor.flush(&mut out, printed);
                // The deferred color exit aborts before the remaining
                // logo lines: upstream exits inside the structure loop.
                if let Some((code, message)) = instance.pending_exit.borrow_mut().take() {
                    let _ = out.flush();
                    eprintln!("{message}");
                    std::process::exit(code);
                }
                // ffLogoPrintRemaining: lines past the last info row print
                // bare (LEFT: plain; RIGHT: from the right edge) and the
                // cache clears, so later watch rounds move the cursor past
                // the printed logo instead of reprinting it. An empty cache
                // closes with one newline per remaining logo height when
                // the round printed nothing. The --logo-print-remaining
                // option gates this closing call only — the watch rounds'
                // call above always runs.
                let mut remaining = |out: &mut BufWriter<std::io::StdoutLock<'_>>, keys_height: usize| {
                    if !logo_lines.is_empty() {
                        while logo_index < logo_lines.len() {
                            let (text, _width) = &logo_lines[logo_index];
                            if position == Position::Right {
                                let _ = write!(out, "\x1b[9999999C\x1b[{}D", layout.right_offset);
                            }
                            let _ = out.write_all(text);
                            let _ = writeln!(out);
                            logo_index += 1;
                        }
                        if color {
                            // ffLogoPrintRemaining ends the logo block with a
                            // reset even when every line was consumed by a row.
                            let _ = write!(out, "\x1b[m");
                        }
                        logo_lines.clear();
                    } else if keys_height <= logo_height {
                        for _ in 0..logo_height - keys_height + 1 {
                            let _ = writeln!(out);
                        }
                    }
                };
                match watch {
                    None => {
                        if opts.print_remaining {
                            remaining(&mut out, keys_height);
                        }
                        break;
                    },
                    Some(interval_ms) => {
                        remaining(&mut out, keys_height);
                        watch_pause(&mut out, interval_ms);
                    },
                }
            }
            }
        },
    }
    // resetConsole (ffFinish, init.c:81): the matching restores of the
    // toggles, linewrap first, then the watch alternate screen. JSON mode
    // never reaches it — fastfetch.c only calls ffFinish outside the
    // resultDoc branch.
    if instance.disable_linewrap {
        let _ = write!(out, "\x1b[?7h");
    }
    if instance.hide_cursor {
        let _ = write!(out, "\x1b[?25h");
    }
    if watch.is_some() {
        let _ = write!(out, "\x1b[?1049l");
    }
    let _ = out.flush();
}

/// --format json: the result document fastfetch serializes with
/// yyjson_mut_write_fp — one {type, result|error} object per module item,
/// pretty-printed with a trailing newline.
pub fn print_json(items: &[JsonItem]) {
    let mut root = Json::Array(Vec::new());
    for item in items {
        let mut object = Json::obj();
        object.push("type", item.type_name.as_str());
        match &item.outcome {
            JsonOutcome::Result(value) => object.push("result", value.clone()),
            JsonOutcome::Error(message) => object.push("error", message.as_str()),
            JsonOutcome::Unsupported => object.push("error", "Unsupported for JSON format"),
        }
        if let Some(ms) = item.stat {
            object.push("stat", ms);
        }
        root.push_value(object);
    }
    print!("{}", json::to_pretty(&root));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(color: bool) -> KeyStyle<'static> {
        KeyStyle {
            color,
            bright_color: true,
            color_keys: "",
            color_output: "",
            color_separator: "",
            separator: ": ",
            key_width: 0,
            key_padding_left: 0,
            key_type: crate::instance::KEY_TYPE_STRING,
        }
    }

    #[test]
    fn key_rows_match_fastfetch_escape_layout() {
        let row = Row::new("BTRFS", "1 GiB");
        let mut colored = style(true);
        colored.color_keys = "36";
        assert_eq!(
            row_line(&row, &colored, 0),
            "\x1b[m\x1b[1m\x1b[36mBTRFS\x1b[m: \x1b[m1 GiB"
        );
        assert_eq!(row_line(&row, &style(false), 0), "BTRFS: 1 GiB");
        // logos with a single palette color color no key
        assert_eq!(row_line(&row, &style(true), 0), "\x1b[m\x1b[1mBTRFS\x1b[m: \x1b[m1 GiB");
    }

    #[test]
    fn keyless_rows_only_add_the_trailing_reset_of_the_key_writer() {
        let row = Row::bare("\x1b[1m\x1b[36muser\x1b[m@\x1b[1m\x1b[36mhost\x1b[m");
        assert_eq!(
            row_line(&row, &style(true), 0),
            "\x1b[m\x1b[1m\x1b[36muser\x1b[m@\x1b[1m\x1b[36mhost\x1b[m"
        );
        assert_eq!(row_line(&row, &style(false), 0), row.value);
    }

    #[test]
    fn raw_rows_pass_through() {
        let row = Row::raw("---");
        assert_eq!(row_line(&row, &style(true), 0), "---");
        assert_eq!(row_line(&row, &style(false), 0), "---");
    }

    #[test]
    fn key_surface_matches_ffprintlogoandkey() {
        let mut colored = style(true);
        colored.color_keys = "34";
        colored.color_separator = "90";
        colored.color_output = "97";
        colored.separator = " => ";
        colored.key_padding_left = 2;
        colored.key_width = 20;
        // padding, bolt+keyColor, key, separator color+text+reset, the
        // absolute-column jump (keyWidth + logoWidth), then outputColor.
        let row = Row::new("OS", "Arch Linux");
        assert_eq!(
            row_line(&row, &colored, 5),
            "  \x1b[m\x1b[1m\x1b[34mOS\x1b[m\x1b[90m => \x1b[m\x1b[25G\x1b[m\x1b[97mArch Linux"
        );
        // keyType NONE drops the whole key block but keeps the tail.
        colored.key_type = crate::instance::KEY_TYPE_NONE;
        assert_eq!(row_line(&row, &colored, 0), "\x1b[m\x1b[97mArch Linux");
        // the STRING space shift pads between the key color and the text.
        colored.key_type = crate::instance::KEY_TYPE_STRING | 2 << crate::instance::KEY_TYPE_SPACE_SHIFT;
        assert_eq!(
            row_line(&row, &colored, 0),
            "  \x1b[m\x1b[1m\x1b[34m  OS\x1b[m\x1b[90m => \x1b[m\x1b[20G\x1b[m\x1b[97mArch Linux"
        );
        // pipe mode keeps the padding, separator and jump, drops all color.
        let mut pipe = style(false);
        pipe.key_padding_left = 2;
        pipe.key_width = 20;
        pipe.separator = " => ";
        assert_eq!(row_line(&row, &pipe, 5), "  OS => \x1b[25GArch Linux");
    }

    #[test]
    fn error_rows_wrap_the_value_in_the_error_modifier() {
        let mut colored = style(true);
        colored.color_keys = "34";
        let row = Row::error("CPU", "No CPU detected");
        assert_eq!(row_line(&row, &colored, 0), "\x1b[m\x1b[1m\x1b[34mCPU\x1b[m: \x1b[m\x1b[1;31mNo CPU detected\x1b[m");
        let mut bright_off = style(true);
        bright_off.color_keys = "34";
        bright_off.bright_color = false;
        assert_eq!(
            row_line(&row, &bright_off, 0),
            "\x1b[m\x1b[34mCPU\x1b[m: \x1b[m\x1b[1;31mNo CPU detected\x1b[m"
        );
    }
}

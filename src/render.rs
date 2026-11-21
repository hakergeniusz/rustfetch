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
) -> Vec<(String, usize)> {
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
            // The display width: wcwidth per char, not the char count
            // (wide logo glyphs count 2, combining ones 0).
            let content_width = line
                .iter()
                .map(|seg| seg.text.chars().map(|c| crate::wcwidth::wcwidth(c as u32) as usize).sum::<usize>())
                .sum::<usize>();
            let width = if bare_entry { 0 } else { pad_left + content_width };
            if width == 0 {
                return (String::new(), 0);
            }
            // The carry escape: the line's own first segment carries it —
            // emit the spaces after that escape and skip the duplicate.
            let carry_here = if line.is_empty() {
                carry.or(seed)
            } else {
                line.first().map(|seg| &seg.color)
            };
            let mut out = String::new();
            if color {
                out.push_str("\x1b[1m");
            }
            let mut skip_first_escape = false;
            if let Some(carry_color) = carry_here {
                match carry_color {
                    SegmentColor::Code(code) => {
                        out.push_str("\x1b[");
                        out.push_str(code);
                        out.push('m');
                        skip_first_escape = true;
                    },
                    SegmentColor::Esc(bytes) => {
                        out.push_str(bytes.as_str());
                        skip_first_escape = true;
                    },
                    SegmentColor::Pass => {},
                }
            }
            out.push_str(&" ".repeat(pad_left));
            for (position, seg) in line.iter().enumerate() {
                if position == 0 && skip_first_escape {
                    out.push_str(&seg.text);
                    continue;
                }
                match &seg.color {
                    SegmentColor::Code(code) => {
                        out.push_str("\x1b[");
                        out.push_str(code);
                        out.push('m');
                    },
                    SegmentColor::Esc(bytes) => out.push_str(bytes.as_str()),
                    SegmentColor::Pass => {},
                }
                out.push_str(&seg.text);
            }
            if color {
                out.push_str("\x1b[m");
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

/// Renders a single info row; `Kind::Raw` values are printed verbatim because
/// modules embed their own escapes there.
fn row_line(row: &Row, key_color: Option<&str>, color: bool) -> String {
    match (&row.key, row.kind) {
        (Some(key), Kind::Value) => {
            if color {
                let mut out = String::from("\x1b[m\x1b[1m");
                if let Some(code) = key_color {
                    out.push_str(&format!("\x1b[{code}m"));
                }
                out.push_str(key);
                out.push_str("\x1b[m: \x1b[m");
                out.push_str(&row.value);
                out
            } else {
                format!("{key}: {}", row.value)
            }
        },
        // ffPrintError's message: bold red outside pipe mode, plain in it.
        (_, Kind::Error) => {
            if color {
                format!("{}: \x1b[1;31m{}\x1b[m", {
                    let mut out = String::from("\x1b[m\x1b[1m");
                    if let Some(code) = key_color {
                        out.push_str(&format!("\x1b[{code}m"));
                    }
                    out.push_str(row.key.as_deref().unwrap_or(""));
                    out.push_str("\x1b[m: \x1b[m");
                    out
                }, row.value)
            } else {
                format!("{}: {}", row.key.as_deref().unwrap_or(""), row.value)
            }
        },
        // Keyless styled rows (title, colors): the module owns the value's
        // escapes; the renderer only adds ffPrintLogoAndKey's trailing reset.
        (None, Kind::Value) => {
            if color {
                format!("\x1b[m{}", row.value)
            } else {
                row.value.clone()
            }
        },
        (_, Kind::Raw) => row.value.clone(),
        // Late rows expand into regular ones before reaching here.
        (_, Kind::Late(_)) => String::new(),
    }
}

/// The fixed two-column layout of one Chars render, shared by the row
/// emissions: the key column the rows start at, the styling flags, and
/// the resolved logo position.
struct CharsLayout<'a> {
    key_column: usize,
    key_color: Option<&'a str>,
    color: bool,
    position: Position,
    /// RIGHT: how far the art sits from the right edge (maxLineWidth +
    /// paddingRight - 1, the cache's rightOffset).
    right_offset: usize,
}

/// Prints one info row against the next unconsumed logo line, standing in
/// for ffPrintLogoAndKey's ffLogoPrintLine + key sequence.
fn emit_chars_row(
    out: &mut BufWriter<std::io::StdoutLock<'_>>,
    logo_lines: &[(String, usize)],
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
                    let _ = write!(out, "{text}");
                    let _ = write!(out, "\x1b[G");
                }
            },
            _ => {
                if let Some((text, width)) = logo_lines.get(*logo_index) {
                    let _ = write!(out, "{text}");
                    let _ = write!(
                        out,
                        "{:width$}",
                        "",
                        width = layout.key_column.saturating_sub(*width)
                    );
                } else if layout.position == Position::Left {
                    // Cache exhausted: the key column fills with spaces so
                    // the two-column alignment holds.
                    let _ = write!(out, "{:width$}", "", width = layout.key_column);
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
    let _ = writeln!(out, "{}", row_line(row, layout.key_color, layout.color));
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
    let key_color = instance.logo_colors.get().and_then(|colors| colors.key);
    let opts = &instance.logo_options;

    // ffStart: the buffered reset; watch mode additionally enables the
    // alternate screen buffer, homes the cursor and flushes before any
    // logo output. The image printers flush these ahead of their raw
    // dumps instead (upstream's fflush; a .raw file keeps the reset
    // buffered behind the dump — observable wire order).
    let flushes_start = matches!(logo.as_ref(), Some(Logo::Raw { flush_start: true, .. }));
    if color && !instance.ff_started.get() && !flushes_start {
        let _ = write!(out, "\x1b[m");
    }
    if watch.is_some() && !instance.ff_started.get() && !flushes_start {
        let _ = write!(out, "\x1b[?1049h\x1b[H");
        let _ = out.flush();
    }

    match logo.as_ref() {
        Some(Logo::Raw { content, position, static_layout, cpr, flush_start }) => {
            let (mut logo_width, mut logo_height) = (0usize, 0usize);
            // ffStart's reset (+ the watch alternate screen) goes on the
            // wire ahead of the dump for the image printers only; a .raw
            // file leaves it buffered behind (upstream has no fflush).
            let mut ff_start = Vec::new();
            if *flush_start && !instance.ff_started.get() {
                if color {
                    ff_start.extend_from_slice(b"\x1b[m");
                }
                if watch.is_some() {
                    ff_start.extend_from_slice(b"\x1b[?1049h\x1b[H");
                }
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
                    if let Kind::Late(late) = row.kind {
                        let _ = out.flush();
                        for produced in late(instance) {
                            keys_height += 1;
                            if logo_width > 0 {
                                let _ = write!(out, "\x1b[{}C", logo_width);
                            }
                            if watch.is_some() && *position == Position::Left {
                                let _ = write!(out, "\x1b[K");
                            }
                            let _ = writeln!(out, "{}", row_line(&produced, key_color, color));
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
                    let _ = writeln!(out, "{}", row_line(row, key_color, color));
                    printed += 1;
                    cursor.flush(&mut out, printed);
                }
                cursor.flush(&mut out, printed);
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
            let mut logo_lines: Vec<(String, usize)> = match lines {
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
                        dump.extend_from_slice(text.as_bytes());
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
                            dump.extend_from_slice(text.as_bytes());
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
                        if let Kind::Late(late) = row.kind {
                            let _ = out.flush();
                            for produced in late(instance) {
                                keys_height += 1;
                                let _ = writeln!(out, "{}", row_line(&produced, key_color, color));
                            }
                            printed += 1;
                            cursor.flush(&mut out, printed);
                            continue;
                        }
                        keys_height += 1;
                        let _ = writeln!(out, "{}", row_line(row, key_color, color));
                        printed += 1;
                        cursor.flush(&mut out, printed);
                    }
                    cursor.flush(&mut out, printed);
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
                key_color,
                color,
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
                    if let Kind::Late(late) = row.kind {
                        // Deferred detection runs at the row's position: flush
                        // the rows already rendered so the detection's /dev/tty
                        // queries land in the stream like upstream's print-time
                        // ffDetect calls.
                        let _ = out.flush();
                        for produced in late(instance) {
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
                            let _ = write!(out, "{text}");
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

    #[test]
    fn key_rows_match_fastfetch_escape_layout() {
        let row = Row::new("BTRFS", "1 GiB");
        assert_eq!(
            row_line(&row, Some("36"), true),
            "\x1b[m\x1b[1m\x1b[36mBTRFS\x1b[m: \x1b[m1 GiB"
        );
        assert_eq!(row_line(&row, Some("36"), false), "BTRFS: 1 GiB");
        // logos with a single palette color color no key
        assert_eq!(row_line(&row, None, true), "\x1b[m\x1b[1mBTRFS\x1b[m: \x1b[m1 GiB");
    }

    #[test]
    fn keyless_rows_only_add_the_trailing_reset_of_the_key_writer() {
        let row = Row::bare("\x1b[1m\x1b[36muser\x1b[m@\x1b[1m\x1b[36mhost\x1b[m");
        assert_eq!(
            row_line(&row, Some("36"), true),
            "\x1b[m\x1b[1m\x1b[36muser\x1b[m@\x1b[1m\x1b[36mhost\x1b[m"
        );
        assert_eq!(row_line(&row, Some("36"), false), row.value);
    }

    #[test]
    fn raw_rows_pass_through() {
        let row = Row::raw("---");
        assert_eq!(row_line(&row, Some("36"), true), "---");
        assert_eq!(row_line(&row, Some("36"), false), "---");
    }
}

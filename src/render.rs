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

/// Serializes parsed logo lines to strings, tracking visible widths
/// separately. Every segment emits its escape — Code, raw Esc, including
/// empty ones from `$N` markers at line starts (fastfetch prints the
/// carried color and then the marker's own escape) — while the bold/reset
/// wrap only appears in color mode, like logoLineCacheBuild's BOLT and
/// logoLineCachePush's reset. Blank lines print nothing, width-zero like
/// upstream's logoLineCachePush(nullptr, 0).
pub(crate) fn colorize(lines: &[Vec<Segment>], color: bool) -> Vec<(String, usize)> {
    lines
        .iter()
        .map(|line| {
            let width = line.iter().map(|seg| seg.text.chars().count()).sum();
            if width == 0 {
                return (String::new(), 0);
            }
            let mut out = String::new();
            if color {
                out.push_str("\x1b[1m");
            }
            for seg in line {
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
/// emissions: fastfetch's padding model (0 left / 4 right — the
/// --logo-padding overrides are not parsed yet), the key column the rows
/// start at, the styling flags, and the resolved logo position.
struct CharsLayout<'a> {
    pad_left: usize,
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
                    let _ = write!(out, "{:width$}", "", width = layout.pad_left);
                    let _ = write!(out, "{text}");
                    let _ = write!(
                        out,
                        "{:width$}",
                        "",
                        width = layout
                            .key_column
                            .saturating_sub(layout.pad_left + width)
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
/// it. The spin logo's per-round reprint is not ported (still logos cost
/// ffSpinTick's single `false`).
fn watch_pause(out: &mut BufWriter<std::io::StdoutLock<'_>>, interval_ms: u32) {
    let _ = write!(out, "\x1b[J");
    let _ = out.flush();
    std::thread::sleep(std::time::Duration::from_millis(interval_ms as u64));
    let _ = write!(out, "\x1b[H");
    crate::modules::netio::invalidate_route_cache();
}

pub fn print(
    logo: &Option<Logo>,
    instance: &Instance,
    watch: Option<u32>,
    next_round: &mut dyn FnMut() -> Vec<Row>,
) {
    // Pipe mode keeps the two-column layout; it only drops the colors,
    // which every writer below gates on instance.color.
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let color = instance.color;
    let key_color = instance.logo_colors.get().and_then(|colors| colors.key);
    // fastfetch's padding model: 0 left / 4 right (the --logo-padding
    // overrides are not parsed yet). Without a logo the rows start at
    // column 0, like logoPrintNone leaving logoWidth at zero.
    let (pad_left, pad_right) = (0usize, 4usize);
    let padding_top = instance.padding_top.unwrap_or(0) as usize;

    // ffStart: the buffered reset; watch mode additionally enables the
    // alternate screen buffer, homes the cursor and flushes before any
    // logo output.
    if color {
        let _ = write!(out, "\x1b[m");
    }
    if watch.is_some() {
        let _ = write!(out, "\x1b[?1049h\x1b[H");
        let _ = out.flush();
    }

    match logo.as_ref() {
        Some(Logo::Raw { content, position }) => {
            if *position == Position::Top {
                // ffLogoPrintCharsRaw's TOP branch: paddingTop newlines and
                // paddingLeft spaces, then the bytes unprocessed; no
                // clear-screen, no cursor query. logoWidth/logoHeight stay
                // zero and paddingRight newlines follow on the buffered
                // stdout.
                let mut dump = Vec::new();
                dump.extend(std::iter::repeat_n(b'\n', padding_top));
                dump.extend(std::iter::repeat_n(b' ', pad_left));
                dump.extend_from_slice(content);
                unsafe {
                    libc::write(libc::STDOUT_FILENO, dump.as_ptr().cast(), dump.len());
                }
                for _ in 0..pad_right {
                    let _ = writeln!(out);
                }
                // ffLogoPrintRemaining: the cache is empty and logoHeight is
                // zero, so every round (and the non-watch ending) closes with
                // one newline iff the round printed no rows.
                let mut keys_height;
                loop {
                    let rows = next_round();
                    keys_height = 0;
                    for row in &rows {
                        if let Kind::Late(late) = row.kind {
                            // Deferred detection runs at the row's position:
                            // flush the rows already rendered so the
                            // detection's /dev/tty queries land in the stream
                            // like upstream's print-time ffDetect calls.
                            let _ = out.flush();
                            for produced in late(instance) {
                                keys_height += 1;
                                let _ = writeln!(out, "{}", row_line(&produced, key_color, color));
                            }
                            continue;
                        }
                        keys_height += 1;
                        let _ = writeln!(out, "{}", row_line(row, key_color, color));
                    }
                    if keys_height == 0 {
                        let _ = writeln!(out);
                    }
                    match watch {
                        None => break,
                        Some(interval_ms) => watch_pause(&mut out, interval_ms),
                    }
                }
            } else {
            // ffLogoPrintCharsRaw, LEFT position (logo.width/height unset):
            // clear the screen, home the cursor and dump the file bytes
            // unprocessed — regardless of the color mode, like upstream.
            // Printed once; watch rounds only re-render the rows.
            let mut dump = format!("\x1b[2J\x1b[3J\x1b[{};{}H", padding_top + 1, pad_left + 1).into_bytes();
            dump.extend_from_slice(content);
            // One raw write to fd 1: the buffered ffStart reset must not be
            // flushed ahead of it, and the query below writes to /dev/tty.
            unsafe {
                libc::write(libc::STDOUT_FILENO, dump.as_ptr().cast(), dump.len());
            }
            let mut logo_width = 0usize;
            let mut logo_height = 0usize;
            // The cursor query learns where the dump ended: X (raised to
            // width+paddingLeft, both unparsed) gives the key column, Y the
            // logo height. On failure upstream leaves both at zero — no
            // \e[H — and reports the query error whenever printError is
            // set, which it always is for .raw files.
            // On failure upstream reports "\nLogo (image-raw): fail to query
            // cursor position: <error>" only when display.showErrors is set,
            // which this port does not implement: the message stays hidden
            // and the width/height stay zero (no \e[H).
            if let Ok((y, x)) = crate::tty::response_with(b"\x1b[6n", crate::tty::parse_cpr) {
                logo_width = x as usize + pad_right - 1;
                logo_height = y as usize;
                let _ = write!(out, "\x1b[H");
            }
            let mut keys_height;
            loop {
                let rows = next_round();
                keys_height = 0;
                for row in &rows {
                    if let Kind::Late(late) = row.kind {
                        let _ = out.flush();
                        for produced in late(instance) {
                            keys_height += 1;
                            if logo_width > 0 {
                                let _ = write!(out, "\x1b[{}C", logo_width);
                            }
                            if watch.is_some() {
                                let _ = write!(out, "\x1b[K");
                            }
                            let _ = writeln!(out, "{}", row_line(&produced, key_color, color));
                        }
                        continue;
                    }
                    keys_height += 1;
                    // ffLogoPrintLine with an empty cache: the cursor moves
                    // right by logoWidth instead of printing spaces.
                    if logo_width > 0 {
                        let _ = write!(out, "\x1b[{}C", logo_width);
                    }
                    if watch.is_some() {
                        let _ = write!(out, "\x1b[K");
                    }
                    let _ = writeln!(out, "{}", row_line(row, key_color, color));
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
            }
        },
        raw_or_chars => {
            let (lines, chars_width, position) = match raw_or_chars {
                Some(Logo::Chars { lines, width, position }) => (Some(lines), *width, *position),
                _ => (None, 0, Position::Left),
            };
            // logoLineCacheBuild's cache shape: paddingTop empty lines in
            // front, then the parsed lines. No explicit fill: parsedHeight
            // counts newline-terminated lines only, so the cache always
            // holds parsedHeight + 1 entries on its own (art ending in a
            // newline parses one trailing empty line; art ending mid-line
            // leaves the final line uncounted). Both the TOP dump and the
            // LEFT/RIGHT line pairing draw from this cache.
            let mut logo_lines: Vec<(String, usize)> = match lines {
                Some(lines) => {
                    let mut cached = colorize(lines, color);
                    cached.splice(0..0, std::iter::repeat_n((String::new(), 0), padding_top));
                    // The totalLines = logoHeight + 1 fill loop only fires for
                    // an empty art (parsedHeight zero): one extra empty line.
                    if cached.len() == padding_top {
                        cached.push((String::new(), 0));
                    }
                    cached
                },
                None => Vec::new(),
            };
            let has_logo = lines.is_some();
            if position == Position::Top {
                // ffLogoPrintChars' TOP path: the whole cache dumps through a
                // raw write(2) — bypassing the buffered ffStart reset, like
                // the raw-logo dump — then logoWidth/logoHeight zero out, the
                // cache clears, and every round's rows print at column 0.
                if has_logo {
                    let mut dump = Vec::new();
                    for (text, _) in &logo_lines {
                        dump.extend_from_slice(text.as_bytes());
                        dump.push(b'\n');
                    }
                    // paddingBottom: 0 (the --logo-padding overrides are not parsed).
                    unsafe {
                        libc::write(libc::STDOUT_FILENO, dump.as_ptr().cast(), dump.len());
                    }
                }
                logo_lines.clear();
                let mut keys_height;
                loop {
                    let rows = next_round();
                    keys_height = 0;
                    for row in &rows {
                        if let Kind::Late(late) = row.kind {
                            let _ = out.flush();
                            for produced in late(instance) {
                                keys_height += 1;
                                let _ = writeln!(out, "{}", row_line(&produced, key_color, color));
                            }
                            continue;
                        }
                        keys_height += 1;
                        let _ = writeln!(out, "{}", row_line(row, key_color, color));
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
            let logo_width = chars_width;
            let key_column = if has_logo { logo_width + pad_right } else { 0 };
            // instance.state.logoHeight: paddingTop + parsedHeight, i.e.
            // exactly one less than the cache's entry count.
            let logo_height = match lines {
                Some(lines) if !lines.is_empty() => padding_top + lines.len() - 1,
                Some(_) => padding_top,
                None => 0,
            };
            let layout = CharsLayout {
                pad_left,
                key_column,
                key_color,
                color,
                position,
                right_offset: logo_width + pad_right - 1,
            };

            let mut logo_index = 0usize;
            let mut keys_height;
            loop {
                let rows = next_round();
                keys_height = 0;
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
                        continue;
                    }
                    keys_height += 1;
                    emit_chars_row(&mut out, &logo_lines, &mut logo_index, &layout, row, watch.is_some());
                }
                // ffLogoPrintRemaining: lines past the last info row print
                // bare (LEFT: plain; RIGHT: from the right edge) and the
                // cache clears, so later watch rounds move the cursor past
                // the printed logo instead of reprinting it. An empty cache
                // closes with one newline per remaining logo height when
                // the round printed nothing.
                if !logo_lines.is_empty() {
                    while logo_index < logo_lines.len() {
                        let (text, _width) = &logo_lines[logo_index];
                        if position == Position::Right {
                            let _ = write!(out, "\x1b[9999999C\x1b[{}D", layout.right_offset);
                        }
                        let _ = write!(out, "{:width$}", "", width = pad_left);
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
                match watch {
                    None => break,
                    Some(interval_ms) => watch_pause(&mut out, interval_ms),
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

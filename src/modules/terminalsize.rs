//! `terminalsize` module: port of terminalsize_linux.c — TIOCGWINSZ first
//! (stdout, falling back to /dev/tty), then the CSI 18t / 14t queries for
//! whatever the ioctl left at zero (skipped for the fastOnly logo-layout
//! probes), then fastfetch's "179 columns x 41 rows (1920x1440px)" format.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::IsTerminal;

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
};

// The renderer expands a late row with the top-level instance, whose
// `entry` is None — only collect()'s per-job clone carries the module
// entry. Capture the format at run() time into a FIFO: collect() and the
// renderer walk the structure in the same order, so the deferred print()
// pops exactly the format of its own row. Detection itself stays late.
thread_local! {
    static PENDING_FORMATS: RefCell<VecDeque<Option<String>>> = const { RefCell::new(VecDeque::new()) };
}

/// ffDetectTerminalSize: returns (columns, rows, width, height).
fn detect(fast_only: bool) -> Option<(u16, u16, u16, u16)> {
    let mut ws = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // upstream keeps one ttyfd: stdout while it is a tty, else /dev/tty.
    let mut ttyfd = libc::STDOUT_FILENO;
    unsafe {
        if libc::isatty(ttyfd) != 1 {
            ttyfd = libc::open(c"/dev/tty".as_ptr(), libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC);
        }
        // SAFETY: `ws` is a plain POD struct and ioctl only writes into it.
        libc::ioctl(ttyfd, libc::TIOCGWINSZ, &mut ws);
    }

    if !fast_only {
        if (ws.ws_row == 0 || ws.ws_col == 0)
            && let Some((rows, cols)) = query_size(b"\x1b[18t", b"\x1b[8;")
        {
            ws.ws_row = rows;
            ws.ws_col = cols;
        }
        if (ws.ws_ypixel == 0 || ws.ws_xpixel == 0)
            && let Some((height, width)) = query_size(b"\x1b[14t", b"\x1b[4;")
        {
            ws.ws_ypixel = height;
            ws.ws_xpixel = width;
        }
    }

    if ws.ws_row > 0 && ws.ws_col > 0 {
        Some((ws.ws_col, ws.ws_row, ws.ws_xpixel, ws.ws_ypixel))
    } else {
        None
    }
}

/// ffGetTerminalResponse for the CSI t responses: the request goes to
/// /dev/tty through the shared tty module and the two numbers after the
/// marker are parsed from the accumulated answer.
fn query_size(request: &[u8], marker: &[u8]) -> Option<(u16, u16)> {
    crate::tty::response(request, |buffer| parse_size(buffer, marker))
}

/// ffLogoResolveAutoPosition's terminal probe: the column count only when
/// stdout is a terminal (the fastOnly detection skips the CSI queries),
/// zero otherwise — AUTO stays LEFT without a terminal.
pub(crate) fn logo_terminal_columns() -> u32 {
    if !std::io::stdout().is_terminal() {
        return 0;
    }
    detect(true).map(|(columns, _, _, _)| columns as u32).unwrap_or(0)
}

/// ffLogoPrint's image attempt for non-.txt file logos: chafa's sizing
/// runs ffDetectTerminalSize (slow path) before the load fails on a
/// non-image file, so the CSI 18t/14t queries reach /dev/tty and their
/// answers are consumed even though nothing image-shaped is printed.
pub(crate) fn image_attempt_probe() {
    let _ = detect(false);
}

/// The `ESC[8;rows;colst` / `ESC[4;height;widtht` body after the marker.
fn parse_size(text: &[u8], marker: &[u8]) -> Option<(u16, u16)> {
    let start = text.windows(marker.len()).position(|window| window == marker)? + marker.len();
    let read_number = |mut position: usize, terminator: u8| -> Option<(u16, usize)> {
        let begin = position;
        while position < text.len() && text[position].is_ascii_digit() {
            position += 1;
        }
        if position == begin || text.get(position) != Some(&terminator) {
            return None;
        }
        let value: u16 = std::str::from_utf8(&text[begin..position]).ok()?.parse().ok()?;
        Some((value, position + 1))
    };
    let (first, position) = read_number(start, b';')?;
    let (second, _) = read_number(position, b't')?;
    Some((first, second))
}

/// The module detects at print time like ffPrintTerminalSize, so the CSI
/// query escapes reach /dev/tty at the row's position.
pub fn run(instance: &Instance) -> Vec<Row> {
    PENDING_FORMATS.with(|formats| {
        formats.borrow_mut().push_back(crate::modules::entry_format(instance.entry).map(str::to_owned));
    });
    vec![Row::late(print, "terminalsize")]
}

/// The deferred ffPrintTerminalSize: detection, then the row or the
/// hidden error (nothing at all, like ffPrintError with showErrors off).
fn print(instance: &Instance) -> Vec<Row> {
    // Consume this late row's captured format even when detection fails, so
    // one run() always pairs with one pop across watch rounds.
    let captured = PENDING_FORMATS.with(|formats| formats.borrow_mut().pop_front().flatten());
    let Some((cols, rows, width_px, height_px)) = detect(false) else {
        // The error row is hidden by default, like upstream's ffPrintError.
        return Vec::new();
    };

    // Prefer the instance's entry when the renderer carries one; otherwise
    // use the format captured by run() for this late row.
    let Some(format) = captured.as_deref().or_else(|| crate::modules::entry_format(instance.entry)) else {
        let mut value = format!("{cols} columns x {rows} rows");
        if width_px > 0 && height_px > 0 {
            value.push_str(&format!(" ({width_px}px x {height_px}px)"));
        }
        // collect()'s stamp_icons skips the keyless late placeholder, so
        // the expanded row stamps its own default key icon.
        return vec![Row::new(instance.display_name("terminalsize"), value).stamped(crate::module_icons::icon_for("terminalsize"))];
    };

    // Upstream FF_PRINT_FORMAT_CHECKED passes rows, columns, width and
    // height as FF_ARG_TYPE_UINT16 (the detector's uint16_t fields).
    let args = [
        crate::format_args::FormatArg { name: "rows", value: crate::format_args::ArgValue::UInt16(rows) },
        crate::format_args::FormatArg { name: "columns", value: crate::format_args::ArgValue::UInt16(cols) },
        crate::format_args::FormatArg { name: "width", value: crate::format_args::ArgValue::UInt16(width_px) },
        crate::format_args::FormatArg { name: "height", value: crate::format_args::ArgValue::UInt16(height_px) },
    ];
    let rendered = crate::format_args::parse_format_string(format, &args, args.len() as u32, &crate::format_args::EngineEnv::new(instance));
    vec![Row::new(instance.display_name("terminalsize"), rendered).stamped(crate::module_icons::icon_for("terminalsize"))]
}

/// ffGenerateTerminalSizeJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let Some((columns, rows, width, height)) = detect(false) else {
        return JsonOutcome::Error("Failed to detect terminal size".to_string());
    };
    let mut obj = Json::obj();
    obj.push("columns", columns as u64);
    obj.push("rows", rows as u64);
    obj.push("width", width as u64);
    obj.push("height", height as u64);
    JsonOutcome::Result(obj)
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_csi_t_responses() {
        assert_eq!(super::parse_size(b"\x1b[8;24;80t", b"\x1b[8;"), Some((24, 80)));
        assert_eq!(super::parse_size(b"\x1b[4;1440;1920t", b"\x1b[4;"), Some((1440, 1920)));
        // The marker must match, both numbers must terminate properly, and
        // a truncated body stays unparsed so the next chunk completes it.
        assert_eq!(super::parse_size(b"\x1b[8;24;8", b"\x1b[8;"), None);
        assert_eq!(super::parse_size(b"garbage\x1b[4;1080;1920t", b"\x1b[4;"), Some((1080, 1920)));
        assert_eq!(super::parse_size(b"\x1b[9;24;80t", b"\x1b[8;"), None);
    }
}

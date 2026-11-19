//! `terminalsize` module: terminal dimensions via ioctl, fastfetch format
//! ("179 columns x 41 rows (1920x1440px)").

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
};

fn winsize() -> Option<(u16, u16, u16, u16)> {
    let mut ws = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `ws` is a plain POD struct and ioctl only writes into it.
    let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) } == 0;
    (ok && ws.ws_col > 0 && ws.ws_row > 0).then_some((
        ws.ws_col,
        ws.ws_row,
        ws.ws_xpixel,
        ws.ws_ypixel,
    ))
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some((cols, rows, width_px, height_px)) = winsize() else {
        return Vec::new();
    };

    let mut value = format!("{cols} columns x {rows} rows");
    if width_px > 0 && height_px > 0 {
        value.push_str(&format!(" ({width_px}px x {height_px}px)"));
    }

    vec![Row::new("Terminal Size", value)]
}

/// ffGenerateTerminalSizeJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let Some((columns, rows, width, height)) = winsize() else {
        return JsonOutcome::Error("Failed to detect terminal size".to_string());
    };
    let mut obj = Json::obj();
    obj.push("columns", columns as u64);
    obj.push("rows", rows as u64);
    obj.push("width", width as u64);
    obj.push("height", height as u64);
    JsonOutcome::Result(obj)
}

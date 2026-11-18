//! `colors` module: two rows of color blocks, ported from colors.c with
//! its defaults: background blocks three spaces wide over colors 0-15.
//! The escape codes are not gated on pipe mode for backgrounds upstream,
//! and the bright row starts with the blink code on non-xterm $TERM.

use crate::{instance::Instance, modules::Row};

const BLOCK: &str = "   ";

pub fn run(_instance: &Instance) -> Vec<Row> {
    let mut rows = Vec::new();

    // Normal background colors 0-7. Unlike the foreground variant, the
    // background color codes are not gated on pipe mode upstream.
    let mut normal = String::new();
    for code in 0..=7u8 {
        normal.push_str(&format!("\x1b[4{code}m"));
        normal.push_str(BLOCK);
    }
    normal.push_str("\x1b[m");
    // The first row goes through ffPrintLogoAndKey (tail reset); the
    // second pops the logo line directly via ffLogoPrintLine.
    rows.push(Row::bare(normal));

    // Required by the Linux console for light backgrounds to work; not
    // gated on pipe mode upstream either.
    let mut bright = String::new();
    if let Some(term) = std::env::var_os("TERM")
        && !term.as_encoded_bytes().starts_with(b"xterm")
    {
        bright.push_str("\x1b[5m");
    }
    for code in 8..=15u8 {
        bright.push_str(&format!("\x1b[10{}m", code - 8));
        bright.push_str(BLOCK);
    }
    bright.push_str("\x1b[m");
    rows.push(Row::raw(bright));

    rows
}

#[cfg(test)]
mod tests {
    #[test]
    fn block_width_matches_upstream() {
        assert_eq!(super::BLOCK, "   ");
    }
}

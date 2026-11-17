//! Two-column terminal renderer: logo on the left, info rows on the right.

use std::io::{BufWriter, Write};

use crate::{cli::Cli, instance::Instance, logo::Logo, modules::Kind, modules::Row};

/// ANSI color parameter used for keys when the logo defines none.
const FALLBACK_ACCENT: &str = "34";

/// Keys are colored with the logo's key color if it defines one, else its
/// first art color, else bold blue.
fn accent_color(logo: &Option<Logo>) -> &'static str {
    logo.as_ref()
        .and_then(|logo| logo.def.color_keys.or(logo.def.colors.first().copied()))
        .filter(|color| *color != "39")
        .unwrap_or(FALLBACK_ACCENT)
}

/// The title row uses the logo's dedicated title color when set.
fn title_color(logo: &Option<Logo>) -> &'static str {
    logo.as_ref()
        .and_then(|logo| logo.def.color_title.or(logo.def.colors.first().copied()))
        .filter(|color| *color != "39")
        .unwrap_or(FALLBACK_ACCENT)
}

/// Serializes parsed logo lines to strings, tracking visible widths separately.
fn colorize(logo: &Logo, color: bool) -> Vec<(String, usize)> {
    logo.lines
        .iter()
        .map(|line| {
            let mut out = String::new();
            let mut current = "";
            for seg in line {
                if seg.text.is_empty() {
                    continue;
                }
                if color && seg.color != current {
                    out.push_str("\x1b[");
                    out.push_str(seg.color);
                    out.push('m');
                    current = seg.color;
                }
                out.push_str(&seg.text);
            }
            if color && !out.is_empty() {
                out.push_str("\x1b[0m");
            }
            let width = line.iter().map(|seg| seg.text.chars().count()).sum();
            (out, width)
        })
        .collect()
}

/// Renders a single info row; `Kind::Raw` values are printed verbatim because
/// modules embed their own escapes there.
fn row_line(row: &Row, accent: &str, title: &str, color: bool) -> String {
    match (&row.key, row.kind) {
        (Some(key), Kind::Value) => {
            if color {
                format!("\x1b[1;{accent}m{key}:\x1b[0m {}", row.value)
            } else {
                format!("{key}: {}", row.value)
            }
        },
        (None, Kind::Value) => {
            if color {
                format!("\x1b[1;{title}m{}\x1b[0m", row.value)
            } else {
                row.value.clone()
            }
        },
        (_, Kind::Raw) => row.value.clone(),
    }
}

pub fn print(logo: &Option<Logo>, rows: &[Row], cli: &Cli, instance: &Instance) {
    if cli.pipe {
        for row in rows {
            match &row.key {
                Some(key) => println!("{key}: {}", row.value),
                None => println!("{}", row.value),
            }
        }
        return;
    }

    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let accent = accent_color(logo);
    let title = title_color(logo);
    let logo_lines: Vec<(String, usize)> =
        logo.as_ref().map(|logo| colorize(logo, instance.color)).unwrap_or_default();

    // fastfetch's padding model: defaults are 0 left / 4 right; --padding N
    // sets both sides. The key column sits at logo width + left + right.
    let (pad_left, pad_right) = match cli.padding {
        Some(pad) => (pad as usize, pad as usize),
        None => (0, 4),
    };
    let key_column = pad_left + logo.as_ref().map_or(0, |logo| logo.width) + pad_right;

    let total = logo_lines.len().max(rows.len());
    for i in 0..total {
        if i < logo_lines.len() {
            let (text, width) = &logo_lines[i];
            let _ = write!(out, "{:width$}", "", width = pad_left);
            let _ = write!(out, "{text}");
            // lines past the last info row are printed bare, like fastfetch's
            // ffLogoPrintRemaining (no key-column fill)
            if i < rows.len() {
                let _ = write!(out, "{:width$}", "", width = key_column - pad_left - width);
            }
        } else {
            let _ = write!(out, "{:width$}", "", width = key_column);
        }

        if i < rows.len() {
            let _ = writeln!(out, "{}", row_line(&rows[i], accent, title, instance.color));
        } else {
            let _ = writeln!(out);
        }
    }
    let _ = out.flush();
}

/// JSON string escaping without a serde dependency; values are ASCII text.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// fastfetch-style JSON output: one compact object per module row.
pub fn print_json(rows: &[Row]) {
    let mut out = String::from("[");
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"key\":");
        match &row.key {
            Some(key) => out.push_str(&format!("\"{}\"", json_escape(key))),
            None => out.push_str("null"),
        }
        out.push_str(",\"value\":\"");
        out.push_str(&json_escape(&row.value));
        out.push_str("\"}");
    }
    out.push(']');
    println!("{out}");
}

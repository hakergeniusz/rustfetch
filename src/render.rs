//! Two-column terminal renderer: logo on the left, info rows on the right.
//! Replicates fastfetch's exact escape sequence emission: one global reset
//! (ffStart), logo lines built as bold + per-segment colors + reset, keys
//! as reset + bold + key color + key + reset + ": " + reset, and a final
//! reset after the logo's remaining lines (ffLogoPrintRemaining).

use std::io::{BufWriter, Write};

use crate::json::{self, Json, JsonOutcome};
use crate::logo::SegmentColor;
use crate::{instance::Instance, logo::Logo, modules::JsonItem, modules::Kind, modules::Row};

/// Serializes parsed logo lines to strings, tracking visible widths
/// separately. Every segment emits its escape — Code, raw Esc, including
/// empty ones from `$N` markers at line starts (fastfetch prints the
/// carried color and then the marker's own escape) — while the bold/reset
/// wrap only appears in color mode, like logoLineCacheBuild's BOLT and
/// logoLineCachePush's reset. Blank lines print nothing, width-zero like
/// upstream's logoLineCachePush(nullptr, 0).
pub(crate) fn colorize(logo: &Logo, color: bool) -> Vec<(String, usize)> {
    logo.lines
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
                    SegmentColor::Esc(bytes) => out.push_str(bytes),
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
    }
}

pub fn print(logo: &Option<Logo>, rows: &[Row], instance: &Instance) {
    // Pipe mode keeps the two-column layout; it only drops the colors,
    // which every writer below gates on instance.color.
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let color = instance.color;
    if color {
        // ffStart resets everything before printing starts.
        let _ = write!(out, "\x1b[m");
    }

    let key_color = instance.logo_colors.get().and_then(|colors| colors.key);
    let mut logo_lines: Vec<(String, usize)> =
        logo.as_ref().map(|logo| colorize(logo, color)).unwrap_or_default();
    // logo.padding.top pushes the whole logo block down: the first rows
    // print on key-column-width blank lines, like the reference's
    // `-c all` output.
    if let Some(top) = instance.padding_top {
        logo_lines.splice(0..0, std::iter::repeat_n((String::new(), 0), top as usize));
    }

    // fastfetch's padding model: 0 left / 4 right (the --logo-padding
    // overrides are not parsed yet). Without a logo the rows start at
    // column 0, like logoPrintNone leaving logoWidth at zero.
    let (pad_left, pad_right) = (0usize, 4usize);
    let logo_width = logo.as_ref().map_or(0, |logo| logo.width);
    let key_column = if logo.is_some() { logo_width + pad_right } else { 0 };

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
            let _ = writeln!(out, "{}", row_line(&rows[i], key_color, color));
        } else {
            let _ = writeln!(out);
        }
    }
    if color && logo.is_some() {
        // ffLogoPrintRemaining ends the logo block with a reset even when
        // every logo line was already consumed by a row.
        let _ = write!(out, "\x1b[m");
    }
    // ffLogoPrintRemaining's tail: an empty logo cache (logo none) still
    // closes with one newline when no rows were printed, from its
    // keysHeight <= logoHeight branch.
    if logo_lines.is_empty() && rows.is_empty() {
        let _ = writeln!(out);
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

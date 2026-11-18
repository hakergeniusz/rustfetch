//! `custom` module: config-supplied text, ported from custom.c. The text
//! comes from a JSON config object `{"type": "custom", "format": "..."}`
//! (collect() renders those inline); selecting the module without a
//! format prints the keyless empty row the format parser still emits.

use crate::{instance::Instance, modules::Row};

/// parseFormatString for the custom module: zero format args, so every
/// placeholder other than colors, resets, env vars and escapes renders
/// literally. Always ends with a reset outside pipe mode.
pub fn render_format(
    format: &str,
    color: bool,
    colors: Option<crate::instance::LogoColors>,
) -> String {
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '{' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= chars.len() {
            // unmatched trailing '{'
            out.push('{');
            break;
        }
        if chars[i] == '{' {
            out.push('{');
            i += 1;
            continue;
        }
        let (placeholder, closed): (String, bool) = match chars[i..].iter().position(|&c| c == '}')
        {
            Some(offset) => {
                let value = chars[i..i + offset].iter().collect();
                i += offset + 1;
                (value, true)
            },
            None => {
                let value = chars[i..].iter().collect();
                i = chars.len();
                (value, false)
            },
        };
        let mut parts = placeholder.chars();
        match parts.next() {
            // {-} stops rendering; the rest of the format is dropped.
            Some('-') if placeholder.len() == 1 => break,
            // {#} resets; {#color} emits the escape.
            Some('#') if placeholder.len() == 1 => {
                if color {
                    out.push_str("\x1b[m");
                }
            },
            Some('#') => {
                if color {
                    out.push_str("\x1b[");
                    append_color(&placeholder[1..], &mut out, colors);
                    out.push('m');
                }
            },
            // $NAME expands an environment variable; anything else has no
            // meaning with zero format args and prints literally.
            Some('$') => match std::env::var(&placeholder[1..]) {
                Ok(value) => out.push_str(&value),
                Err(_) => append_invalid(&mut out, &placeholder, closed),
            },
            _ => append_invalid(&mut out, &placeholder, closed),
        }
    }
    if color {
        out.push_str("\x1b[m");
    }
    out
}

/// appendInvalidPlaceholder: the raw placeholder, closing brace included
/// only when one was found.
fn append_invalid(out: &mut String, placeholder: &str, closed: bool) {
    out.push('{');
    out.push_str(placeholder);
    if closed {
        out.push('}');
    }
}

/// ffOptionParseColorNoClear: named colors, modifiers, xterm-256 (@N) and
/// RGB (#RGB / #RRGGBB); errors exit like upstream's exit(479).
fn append_color(value: &str, out: &mut String, colors: Option<crate::instance::LogoColors>) {
    if value.is_empty() {
        return;
    }
    // Already an ANSI escape sequence: splice it in without the final 'm'.
    if let Some(rest) = value.strip_prefix("\x1b[") {
        out.push_str(rest.trim_end_matches('m'));
        return;
    }

    const MODIFIERS: [(&str, &str); 10] = [
        ("reset_", "0;"),
        ("bold_", "1;"),
        ("bright_", "1;"),
        ("dim_", "2;"),
        ("italic_", "3;"),
        ("underline_", "4;"),
        ("blink_", "5;"),
        ("inverse_", "7;"),
        ("hidden_", "8;"),
        ("strike_", "9;"),
    ];
    const NAMED: [(&str, &str); 17] = [
        ("black", "30"),
        ("red", "31"),
        ("green", "32"),
        ("yellow", "33"),
        ("blue", "34"),
        ("magenta", "35"),
        ("cyan", "36"),
        ("white", "37"),
        ("default", "39"),
        ("light_black", "90"),
        ("light_red", "91"),
        ("light_green", "92"),
        ("light_yellow", "93"),
        ("light_blue", "94"),
        ("light_magenta", "95"),
        ("light_cyan", "96"),
        ("light_white", "97"),
    ];

    let rest = value.as_bytes();
    let mut read = 0usize;
    'scan: while read < rest.len() {
        let value_rest = &value[read..];
        if rest[read].is_ascii_alphabetic() {
            let lower = value_rest.to_lowercase();
            if let Some((name, code)) = MODIFIERS.iter().find(|(name, _)| lower.starts_with(name)) {
                out.push_str(code);
                read += name.len();
                continue;
            }
            if let Some((name, code)) = NAMED.iter().find(|(name, _)| lower.starts_with(name)) {
                out.push_str(code);
                read += name.len();
                continue;
            }
            // The key/title props resolve to the logo's colors, brightened;
            // output/separator props are unset by default.
            for (name, code) in [
                ("keys", colors.and_then(|c| c.key)),
                ("title", colors.and_then(|c| c.title)),
            ] {
                if let Some(code) = code
                    && lower.starts_with(name)
                {
                    out.push_str("1;");
                    out.push_str(code);
                    read += name.len();
                    continue 'scan;
                }
            }
            if lower.starts_with("output") {
                out.push_str("39");
                read += "output".len();
                continue;
            }
            if lower.starts_with("separator") {
                out.push_str("39");
                read += "separator".len();
                continue;
            }
            eprintln!("Error: invalid color code found: {value_rest}");
            std::process::exit(479);
        }
        match rest[read] {
            b'@' => {
                let digits: String =
                    value[read + 1..].chars().take_while(|c| c.is_ascii_digit()).collect();
                match digits.parse::<u32>() {
                    Ok(code) if !digits.is_empty() && code <= 255 => {
                        out.push_str("38;5;");
                        out.push_str(&digits);
                        read += 1 + digits.len();
                    },
                    _ => {
                        eprintln!("Error: invalid 256 color code found: {}", &value[read..]);
                        std::process::exit(479);
                    },
                }
            },
            b'#' => {
                let hex: String =
                    value[read + 1..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
                let rgb = match hex.len() {
                    6 => u32::from_str_radix(&hex, 16).unwrap_or(0),
                    3 => {
                        let v = u32::from_str_radix(&hex, 16).unwrap_or(0);
                        ((v & 0xF00) >> 8) * 0x110000
                            + ((v & 0x0F0) >> 4) * 0x001100
                            + (v & 0x00F) * 0x000011
                    },
                    0 => {
                        eprintln!("Error: invalid RGB color code found: {}", &value[read..]);
                        std::process::exit(479);
                    },
                    len if len > 6 => {
                        eprintln!("Error: RGB color code too long: {}", &value[read..]);
                        std::process::exit(479);
                    },
                    _ => {
                        eprintln!("Error: invalid RGB color code length: {}", &value[read..]);
                        std::process::exit(479);
                    },
                };
                out.push_str(&format!(
                    "38;2;{};{};{}",
                    (rgb >> 16) & 0xFF,
                    (rgb >> 8) & 0xFF,
                    rgb & 0xFF
                ));
                read += 1 + hex.len();
            },
            _ => {
                // Stray characters copy over; UTF-8 sequences reassemble.
                let ch_len = utf8_len(rest[read]);
                match std::str::from_utf8(&rest[read..read + ch_len]) {
                    Ok(text) => out.push_str(text),
                    Err(_) => out.push(rest[read] as char),
                }
                read += ch_len;
            },
        }
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// A rendered custom row; the keyless renderer adds the tail reset.
pub fn render_rows(format: &str, instance: &Instance) -> Vec<Row> {
    let colors = instance.logo_colors.get().copied();
    vec![Row::bare(render_format(format, instance.color, colors))]
}

pub fn run(instance: &Instance) -> Vec<Row> {
    render_rows("", instance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_text_and_escapes() {
        assert_eq!(render_format("hello", false, None), "hello");
        assert_eq!(render_format("{{}}", false, None), "{}}");
        assert_eq!(render_format("a{b}c", false, None), "a{b}c");
        // {-} stops rendering
        assert_eq!(render_format("one{-}two", false, None), "one");
        // unmatched trailing brace
        assert_eq!(render_format("x{red", false, None), "x{red");
    }

    #[test]
    fn colors_render_only_with_color() {
        assert_eq!(render_format("{#red}x", false, None), "x");
        assert_eq!(render_format("{#red}x", true, None), "\x1b[31mx\x1b[m");
        assert_eq!(render_format("{#}x", true, None), "\x1b[mx\x1b[m");
        // hex colors need the doubled ## (the first # is the placeholder
        // marker); a bare {#FF0000} is an invalid color name and exits
        assert_eq!(render_format("{##FF0000}x", true, None), "\x1b[38;2;255;0;0mx\x1b[m");
        assert_eq!(render_format("{##F00}x", true, None), "\x1b[38;2;255;0;0mx\x1b[m");
        assert_eq!(render_format("{#@5}x", true, None), "\x1b[38;5;5mx\x1b[m");
        // @-prefixed placeholders without the color marker are not colors
        assert_eq!(render_format("{@5}x", true, None), "{@5}x\x1b[m");
        assert_eq!(render_format("{#bold_red}x", true, None), "\x1b[1;31mx\x1b[m");
    }

    #[test]
    fn env_vars_expand() {
        unsafe { std::env::set_var("RUSTFETCH_CUSTOM_TEST", "value") };
        assert_eq!(render_format("{$RUSTFETCH_CUSTOM_TEST}", false, None), "value");
        assert_eq!(
            render_format("{$RUSTFETCH_MISSING_VAR}", false, None),
            "{$RUSTFETCH_MISSING_VAR}"
        );
        assert_eq!(render_format("{}", false, None), "{}");
        assert_eq!(render_format("{1}", false, None), "{1}");
    }
}

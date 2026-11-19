//! `terminaltheme` module: the terminal's foreground and background colors,
//! ported from terminaltheme.c. The OSC 10/11 queries go through /dev/tty
//! (non-canonical, 1s poll timeout); without a response the COLORFGBG
//! environment variable maps onto the classic 16-color palette.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
};

type Rgb = (u16, u16, u16);

struct Color {
    r: u16,
    g: u16,
    b: u16,
    dark: bool,
}

/// The dark threshold of ffDetectTerminalTheme.
fn set_dark(color: &mut Color) {
    color.dark =
        u32::from(color.r) * 299 + u32::from(color.g) * 587 + u32::from(color.b) * 114 < 128_000;
}

/// detectByEnv's fgbgToColor table.
fn color_from_palette(num: u32) -> Color {
    let (r, g, b) = match num {
        0 => (0, 0, 0),
        1 => (205, 0, 0),
        2 => (0, 205, 0),
        3 => (205, 205, 0),
        4 => (0, 0, 238),
        5 => (205, 0, 205),
        6 => (0, 205, 205),
        7 => (229, 229, 229),
        8 => (127, 127, 127),
        9 => (255, 0, 0),
        10 => (0, 255, 0),
        11 => (255, 255, 0),
        12 => (92, 92, 255),
        13 => (255, 0, 255),
        14 => (0, 255, 255),
        15 => (255, 255, 255),
        _ => (0, 0, 0),
    };
    let mut color = Color {
        r,
        g,
        b,
        dark: false,
    };
    set_dark(&mut color);
    color
}

/// detectByEnv: COLORFGBG is "fg;bg" palette indices.
fn detect_by_env() -> Option<(Color, Color)> {
    let color = std::env::var("COLORFGBG").ok()?;
    let (fg, bg) = color.split_once(';')?;
    let (Ok(fg), Ok(bg)) = (fg.trim().parse::<u32>(), bg.trim().parse::<u32>()) else {
        return None;
    };
    Some((color_from_palette(fg), color_from_palette(bg)))
}

fn read_hex(text: &[u8], start: usize) -> Option<(u32, usize)> {
    let mut end = start;
    while end < text.len() && text[end].is_ascii_hexdigit() {
        end += 1;
    }
    if end == start {
        return None;
    }
    let value = u32::from_str_radix(std::str::from_utf8(&text[start..end]).ok()?, 16).ok()?;
    // %hx keeps the low 16 bits.
    Some((value & 0xFFFF, end))
}

fn expect(text: &[u8], at: usize, byte: u8) -> Option<usize> {
    if text.get(at) == Some(&byte) {
        Some(at + 1)
    } else {
        None
    }
}

fn find_marker(text: &[u8], marker: &[u8]) -> Option<usize> {
    text.windows(marker.len()).position(|window| window == marker)
}

/// One `rgb:RRRR/GGGG/BBBB` triplet at `position`.
fn hex_color(text: &[u8], mut position: usize) -> Option<(Rgb, usize)> {
    let mut channels = [0u16; 3];
    for (index, channel) in channels.iter_mut().enumerate() {
        let (value, next) = read_hex(text, position)?;
        *channel = value as u16;
        position = next;
        if index < 2 {
            position = expect(text, position, b'/')?;
        }
    }
    Some((channels.into(), position))
}

/// detectByEscapeCode: query fg (10) and bg (11) through /dev/tty.
fn detect_by_escape() -> Option<(Color, Color)> {
    let request = b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\";
    let path = c"/dev/tty";
    let flags = libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC;
    let fd = unsafe { libc::open(path.as_ptr(), flags) };
    if fd < 0 {
        return None;
    }

    let mut result = None;
    unsafe {
        let mut old_term: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut old_term) == 0 {
            let mut new_term = old_term;
            new_term.c_lflag &= !(libc::ICANON | libc::ECHO);
            if libc::tcsetattr(fd, libc::TCSAFLUSH, &new_term) == 0 {
                let mut written = 0usize;
                while written < request.len() {
                    let n = libc::write(
                        fd,
                        request[written..].as_ptr().cast(),
                        request.len() - written,
                    );
                    if n <= 0 {
                        break;
                    }
                    written += n as usize;
                }
                if written == request.len() {
                    result = read_response(fd);
                }
                libc::tcsetattr(fd, libc::TCSAFLUSH, &old_term);
            }
        }
        libc::close(fd);
    }
    let (fg_raw, bg_raw) = result?;

    // Values above 0x100 are 16-bit; scale down like detectByEscapeCode.
    let scale = |color: &mut (u16, u16, u16)| {
        if color.0 > 0x100 || color.1 > 0x100 || color.2 > 0x100 {
            color.0 /= 0x100;
            color.1 /= 0x100;
            color.2 /= 0x100;
        }
    };
    let (mut fg, mut bg) = (fg_raw, bg_raw);
    scale(&mut fg);
    scale(&mut bg);
    let (mut fg, mut bg) = (
        Color {
            r: fg.0,
            g: fg.1,
            b: fg.2,
            dark: false,
        },
        Color {
            r: bg.0,
            g: bg.1,
            b: bg.2,
            dark: false,
        },
    );
    set_dark(&mut fg);
    set_dark(&mut bg);
    Some((fg, bg))
}

/// Read and parse with the chunking of ffGetTerminalResponse: poll up to
/// 1s, parse what arrived, wait again when the response was incomplete.
fn read_response(fd: i32) -> Option<(Rgb, Rgb)> {
    let mut buffer = Vec::with_capacity(1024);
    loop {
        let mut poll_fd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut poll_fd, 1, 1000) } <= 0 {
            return None;
        }
        let mut chunk = [0u8; 1024];
        let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
        if n <= 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..n as usize]);

        // Parse what arrived so far, like the vsscanf-per-chunk loop.
        if let Some(colors) = parse_response(&buffer) {
            return Some(colors);
        }
        if buffer.len() > 4096 {
            return None;
        }
    }
}

/// The OSC 10/11 response: two rgb triplets after their markers.
fn parse_response(text: &[u8]) -> Option<(Rgb, Rgb)> {
    let start = find_marker(text, b"10;rgb:")? + b"10;rgb:".len();
    let (fg, position) = hex_color(text, start)?;
    let marker = find_marker(&text[position..], b"11;rgb:")? + position + b"11;rgb:".len();
    let (bg, _) = hex_color(text, marker)?;
    Some((fg, bg))
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some((fg, bg)) = detect_by_escape().or_else(detect_by_env) else {
        // "Failed to detect terminal theme" is an error row, hidden by default.
        return Vec::new();
    };

    // The default print keys the [Dark]/[Light] tag off the background only.
    let value = format!(
        "#{:02X}{:02X}{:02X} (FG) - #{:02X}{:02X}{:02X} (BG) [{}]",
        fg.r,
        fg.g,
        fg.b,
        bg.r,
        bg.g,
        bg.b,
        if bg.dark { "Dark" } else { "Light" }
    );
    vec![Row::new("Terminal Theme", value)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_osc_10_11_responses() {
        let response = b"\x1b]10;rgb:1e1e/2e2e/3e3e\x1b\\\x1b]11;rgb:0c0c/0c0c/0c0c\x1b\\";
        let Some(((fr, fgc, fb), (br, bgc, bb))) = parse_response(response) else {
            panic!("response must parse");
        };
        assert_eq!((fr, fgc, fb), (0x1e1e, 0x2e2e, 0x3e3e));
        assert_eq!((br, bgc, bb), (0x0c0c, 0x0c0c, 0x0c0c));
    }

    #[test]
    fn scales_16bit_values_down() {
        // 0x1e1e > 0x100 -> /0x100 = 0x1e
        let mut color = (0x1e1eu16, 0x2e2eu16, 0x3e3eu16);
        if color.0 > 0x100 || color.1 > 0x100 || color.2 > 0x100 {
            color.0 /= 0x100;
            color.1 /= 0x100;
            color.2 /= 0x100;
        }
        assert_eq!(color, (0x1e, 0x2e, 0x3e));
    }

    #[test]
    fn maps_colorfgbg_palette() {
        let color = color_from_palette(4);
        assert_eq!((color.r, color.g, color.b), (0, 0, 238));
        assert!(color.dark);
        let color = color_from_palette(7);
        assert_eq!((color.r, color.g, color.b), (229, 229, 229));
        assert!(!color.dark);
        let color = color_from_palette(15);
        assert!(!color.dark);
    }
}

/// ffGenerateTerminalThemeJsonResult: {fg, bg} with r/g/b/dark each.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let Some((fg, bg)) = detect_by_escape().or_else(detect_by_env) else {
        return JsonOutcome::Error("Failed to detect terminal theme".to_string());
    };
    let mut obj = Json::obj();
    for (key, color) in [("fg", &fg), ("bg", &bg)] {
        let mut channel = Json::obj();
        channel.push("r", color.r as u64);
        channel.push("g", color.g as u64);
        channel.push("b", color.b as u64);
        channel.push("dark", color.dark);
        obj.push(key, channel);
    }
    JsonOutcome::Result(obj)
}

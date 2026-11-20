//! `terminalsize` module: port of terminalsize_linux.c — TIOCGWINSZ first
//! (stdout, falling back to /dev/tty), then the CSI 18t / 14t queries for
//! whatever the ioctl left at zero (skipped for the fastOnly logo-layout
//! probes), then fastfetch's "179 columns x 41 rows (1920x1440px)" format.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
};

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

/// ffGetTerminalResponse for the CSI t responses: write the request to
/// /dev/tty (canonical mode off, restored afterwards), then poll+read
/// chunks until the two numbers after the marker arrive — 1s per poll,
/// like FF_IO_TERM_RESP_WAIT_MS.
fn query_size(request: &[u8], marker: &[u8]) -> Option<(u16, u16)> {
    let fd = unsafe { libc::open(c"/dev/tty".as_ptr(), libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
    if fd < 0 {
        return None;
    }
    unsafe {
        let mut old_term: libc::termios = std::mem::zeroed();
        let mut result = None;
        if libc::tcgetattr(fd, &mut old_term) == 0 {
            let mut new_term = old_term;
            new_term.c_lflag &= !(libc::ICANON | libc::ECHO);
            if libc::tcsetattr(fd, libc::TCSAFLUSH, &new_term) == 0 {
                let mut written = 0usize;
                while written < request.len() {
                    let n = libc::write(fd, request[written..].as_ptr().cast(), request.len() - written);
                    if n <= 0 {
                        break;
                    }
                    written += n as usize;
                }
                if written == request.len() {
                    result = read_size(fd, marker);
                }
                libc::tcsetattr(fd, libc::TCSAFLUSH, &old_term);
            }
        }
        libc::close(fd);
        result
    }
}

fn read_size(fd: i32, marker: &[u8]) -> Option<(u16, u16)> {
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
        if let Some(size) = parse_size(&buffer, marker) {
            return Some(size);
        }
        if buffer.len() > 4096 {
            return None;
        }
    }
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

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some((cols, rows, width_px, height_px)) = detect(false) else {
        // The error row is hidden by default, like upstream's ffPrintError.
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

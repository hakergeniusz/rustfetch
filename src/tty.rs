//! Terminal queries through /dev/tty, ported from ffGetTerminalResponse
//! (io_unix.c): canonical mode off for the duration, request written to
//! the tty, then poll+read chunks until `parse` accepts the accumulated
//! bytes — 1s per poll, like FF_IO_TERM_RESP_WAIT_MS.

/// The `\e[<y>;<x>R` cursor position report, parsed like the
/// `%*[^0-9]%hu;%huR` scan: skip non-digits, then y, ';', x, 'R'.
pub(crate) fn parse_cpr(buffer: &[u8]) -> Option<(u16, u16)> {
    let read_number = |mut position: usize, terminator: u8| -> Option<(u16, usize)> {
        while position < buffer.len() && !buffer[position].is_ascii_digit() {
            position += 1;
        }
        let begin = position;
        while position < buffer.len() && buffer[position].is_ascii_digit() {
            position += 1;
        }
        if position == begin || buffer.get(position) != Some(&terminator) {
            return None;
        }
        let value: u16 = std::str::from_utf8(&buffer[begin..position]).ok()?.parse().ok()?;
        Some((value, position + 1))
    };
    let (y, position) = read_number(0, b';')?;
    let (x, _) = read_number(position, b'R')?;
    Some((y, x))
}

/// Writes `request` to /dev/tty and reads the answer, parsed per chunk
/// over the accumulated bytes; None when the tty is unavailable or the
/// answer never parses.
pub(crate) fn response<T>(request: &[u8], parse: impl Fn(&[u8]) -> Option<T>) -> Option<T> {
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
                    let mut buffer = Vec::with_capacity(1024);
                    loop {
                        let mut poll_fd = libc::pollfd {
                            fd,
                            events: libc::POLLIN,
                            revents: 0,
                        };
                        if libc::poll(&mut poll_fd, 1, 1000) <= 0 {
                            break;
                        }
                        let mut chunk = [0u8; 1024];
                        let n = libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len());
                        if n <= 0 {
                            break;
                        }
                        buffer.extend_from_slice(&chunk[..n as usize]);

                        if let Some(parsed) = parse(&buffer) {
                            result = Some(parsed);
                            break;
                        }
                        if buffer.len() > 4096 {
                            break;
                        }
                    }
                }
                libc::tcsetattr(fd, libc::TCSAFLUSH, &old_term);
            }
        }
        libc::close(fd);
        result
    }
}

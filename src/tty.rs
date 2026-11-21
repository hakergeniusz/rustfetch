//! Terminal queries through /dev/tty, ported from ffGetTerminalResponse
//! (io_unix.c): canonical mode off for the duration, request written to
//! the tty, then poll+read chunks until `parse` accepts the accumulated
//! bytes — 1s per poll, like FF_IO_TERM_RESP_WAIT_MS.

/// vsscanf's three outcomes over the accumulated buffer: every parameter
/// assigned (success), the first assigned but the scan incomplete (the
/// caller keeps polling for more chunks), or nothing assigned (`ret == 0`
/// — upstream aborts with the vsscanf error right away).
pub(crate) enum Scan<T> {
    Matched(T),
    Partial,
    Failed,
}

/// The `\e[<y>;<x>R` cursor position report, parsed with glibc sscanf's
/// exact `%*[^0-9]%hu;%huR` semantics: the suppressed class matches a
/// NON-empty non-digit run (a leading digit, or no digits at all, is a
/// matching failure), the literal `;` must sit right after the first
/// number, `%hu` skips whitespace and accepts an optional sign before the
/// second number, and the trailing `R` never needs to match — the scan
/// succeeds once both numbers are assigned. Numbers go through strtoul
/// and truncate to 16 bits.
pub(crate) fn parse_cpr(buffer: &[u8]) -> Scan<(u16, u16)> {
    let is_space = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    // %hu: optional sign, then base-10 digits (required), strtoul's
    // negation and ULONG_MAX-on-overflow behavior included.
    let scan_u16 = |mut position: usize, skip_space: bool| -> Scan<(u16, usize)> {
        if skip_space {
            while position < buffer.len() && is_space(buffer[position]) {
                position += 1;
            }
        }
        let negative = buffer.get(position) == Some(&b'-');
        if matches!(buffer.get(position), Some(b'-') | Some(b'+')) {
            position += 1;
        }
        let begin = position;
        while position < buffer.len() && buffer[position].is_ascii_digit() {
            position += 1;
        }
        if position == begin {
            // %hu unassigned: ret stays 0 for the first number (a hard
            // failure) and 1 for the second (keep reading).
            return Scan::Failed;
        }
        let magnitude = std::str::from_utf8(&buffer[begin..position])
            .ok()
            .and_then(|text| text.parse::<u32>().ok())
            .unwrap_or(u32::MAX);
        let signed = if negative {
            -(magnitude as i64)
        } else {
            magnitude as i64
        };
        Scan::Matched((signed as u32 as u16, position))
    };
    // %*[^0-9]: everything up to the first digit, at least one byte.
    if buffer.first().is_some_and(|b| b.is_ascii_digit()) {
        return Scan::Failed;
    }
    let mut position = 0usize;
    while position < buffer.len() && !buffer[position].is_ascii_digit() {
        position += 1;
    }
    if position == buffer.len() {
        // %hu#1 hits end of input: glibc returns EOF (-1).
        return Scan::Failed;
    }
    let (y, after) = match scan_u16(position, false) {
        Scan::Matched(value) => value,
        Scan::Partial => return Scan::Partial,
        Scan::Failed => return Scan::Failed,
    };
    if buffer.get(after) != Some(&b';') {
        // The literal ';' unmatched: y assigned, ret = 1, keep reading.
        return Scan::Partial;
    }
    match scan_u16(after + 1, true) {
        Scan::Matched((x, _)) => Scan::Matched((y, x)),
        // %hu#2 unassigned keeps ret at 1: wait for more chunks.
        _ => Scan::Partial,
    }
}

/// Writes `request` to /dev/tty and reads the answer, parsed per chunk
/// over the accumulated bytes; the error strings are ffGetTerminalResponse's.
pub(crate) fn response_with<T>(request: &[u8], parse: impl Fn(&[u8]) -> Scan<T>) -> Result<T, &'static str> {
    let fd = unsafe { libc::open(c"/dev/tty".as_ptr(), libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err("open(\"/dev/tty\", O_RDWR | O_NOCTTY | O_CLOEXEC) failed");
    }
    let result = unsafe {
        let mut old_term: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut old_term) != 0 {
            libc::close(fd);
            return Err("tcgetattr(STDIN_FILENO, &oldTerm) failed");
        }
        let mut new_term = old_term;
        new_term.c_lflag &= !(libc::ICANON | libc::ECHO);
        if libc::tcsetattr(fd, libc::TCSAFLUSH, &new_term) != 0 {
            libc::close(fd);
            return Err("tcsetattr(STDIN_FILENO, TCSAFLUSH, &newTerm)");
        }
        // ffWriteFDData's failure is unchecked upstream; a dead tty then
        // fails at the poll below, like this fallthrough does.
        let mut written = 0usize;
        while written < request.len() {
            let n = libc::write(fd, request[written..].as_ptr().cast(), request.len() - written);
            if n <= 0 {
                break;
            }
            written += n as usize;
        }
        let outcome = poll_response(fd, &parse);
        libc::tcsetattr(fd, libc::TCSAFLUSH, &old_term);
        outcome
    };
    unsafe { libc::close(fd) };
    result
}

/// The poll+read loop behind ffGetTerminalResponse: one poll before the
/// first read, another before every further chunk, and vsscanf's ret
/// mapped onto the Scan outcomes.
fn poll_response<T>(fd: i32, parse: impl Fn(&[u8]) -> Scan<T>) -> Result<T, &'static str> {
    let poll = |fd: i32| -> bool {
        let mut poll_fd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        unsafe { libc::poll(&mut poll_fd, 1, 1000) > 0 }
    };
    if !poll(fd) {
        return Err("poll(/dev/tty) timeout or failed");
    }
    let mut buffer = Vec::with_capacity(1024);
    loop {
        // The upstream read buffer is a 1024-byte stack array: once full
        // the read length hits zero and fails like an empty read.
        let room = 1023usize.saturating_sub(buffer.len());
        if room == 0 {
            return Err("read(STDIN_FILENO, buffer, sizeof(buffer) - 1) failed");
        }
        let mut chunk = [0u8; 1024];
        let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), room) };
        if n <= 0 {
            return Err("read(STDIN_FILENO, buffer, sizeof(buffer) - 1) failed");
        }
        buffer.extend_from_slice(&chunk[..n as usize]);
        match parse(&buffer) {
            Scan::Matched(value) => return Ok(value),
            Scan::Failed => return Err("vsscanf(buffer, format, args) failed"),
            Scan::Partial => {},
        }
        if !poll(fd) {
            return Err("poll(/dev/tty) timeout or failed");
        }
    }
}

/// Writes `request` to /dev/tty and reads the answer, parsed per chunk
/// over the accumulated bytes; None when the tty is unavailable or the
/// answer never parses.
pub(crate) fn response<T>(request: &[u8], parse: impl Fn(&[u8]) -> Option<T>) -> Option<T> {
    response_with(request, |buffer| match parse(buffer) {
        Some(value) => Scan::Matched(value),
        None => Scan::Partial,
    })
    .ok()
}

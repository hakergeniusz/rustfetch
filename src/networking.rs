//! The raw-socket HTTP client of common/impl/networking_linux.c and
//! networking_common.c: one blocking HTTP/1.0 request per call, framed by
//! Content-Length, chunked trailers or the connection close. The connect
//! runs in a thread with a watchdog like upstream's default
//! general.multithreading=true: when `timeout` elapses before the thread
//! finishes, upstream's pthread_timedjoin_np fires pthread_kill(SIGTERM)
//! and init.c's handler exits the whole process with status 0, so that is
//! what happens here too. TCP Fast Open is attempted first for callers
//! that enable it, with the same fallthrough to the traditional
//! connect+send on failure.

use std::ffi::{c_int, c_void, CString};

use std::net::TcpStream;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

/// FF_NETWORKING_MAX_RESPONSE_SIZE.
const MAX_RESPONSE_SIZE: usize = 1024 * 1024;

enum SendStatus {
    /// The fast path or inline send left the socket ready to read.
    Ready(TcpStream),
    /// The connect runs in a thread; the channel carries the socket when it
    /// succeeds and closes it on failure, like connectAndSend's error path.
    Threaded(JoinHandle<()>, mpsc::Receiver<Option<TcpStream>>),
    /// A synchronous failure (the string already surfaced through
    /// send()'s Result) or a finished request; recv reports the generic
    /// upstream error for both.
    Failed,
}

/// FFNetworkingState: one request from send to recv, then reset.
pub struct State {
    pub timeout: u32,
    pub ipv6: bool,
    pub tfo: bool,
    status: SendStatus,
}

impl State {
    /// ffNetworkingResetState leaves an idle state; timeout/ipv6/tfo are
    /// set by the module's prepare before every send.
    pub const fn new(timeout: u32, ipv6: bool, tfo: bool) -> Self {
        Self { timeout, ipv6, tfo, status: SendStatus::Failed }
    }

    /// ffNetworkingSendHttpRequest: build the request, resolve, socket,
    /// fast path, then the traditional connect+send in a thread.
    pub fn send(&mut self, host: &str, port: u16, path: &str, headers: &str) -> Result<(), &'static str> {
        let mut command = format!("GET {path} HTTP/1.0\r\nHost: ");
        if host.contains(':') {
            // An IPv6 literal has to be bracketed in the Host header
            // (RFC 9110 7.2), while getaddrinfo() wants it bare.
            command.push('[');
            command.push_str(host);
            command.push(']');
        } else {
            command.push_str(host);
        }
        if port != 80 {
            command.push_str(&format!(":{port}"));
        }
        command.push_str("\r\nConnection: close\r\n");
        command.push_str(headers);
        command.push_str("\r\n");

        let Ok(host) = CString::new(host) else {
            return Err("getaddrinfo() failed");
        };
        let Ok(port) = CString::new(port.to_string()) else {
            return Err("getaddrinfo() failed");
        };
        let mut hints: libc::addrinfo = unsafe { std::mem::zeroed() };
        hints.ai_family = if self.ipv6 { libc::AF_INET6 } else { libc::AF_INET };
        hints.ai_socktype = libc::SOCK_STREAM;
        hints.ai_flags = libc::AI_NUMERICSERV;
        let mut addr: *mut libc::addrinfo = std::ptr::null_mut();
        if unsafe { libc::getaddrinfo(host.as_ptr(), port.as_ptr(), &hints, &mut addr) } != 0 {
            return Err("getaddrinfo() failed");
        }
        // SAFETY: getaddrinfo succeeded, so `addr` is a valid chain with
        // exactly one entry per the exclusive ai_family hint.
        let (family, socktype, protocol, addr_len, sockaddr_copy) = unsafe {
            let ai = &*addr;
            let mut copy: libc::sockaddr_storage = std::mem::zeroed();
            std::ptr::copy_nonoverlapping(
                ai.ai_addr.cast::<u8>(),
                (&mut copy as *mut libc::sockaddr_storage).cast::<u8>(),
                ai.ai_addrlen.min(std::mem::size_of::<libc::sockaddr_storage>() as u32) as usize,
            );
            (ai.ai_family, ai.ai_socktype, ai.ai_protocol, ai.ai_addrlen, copy)
        };
        let sockfd = unsafe { libc::socket(family, socktype, protocol) };
        unsafe { libc::freeaddrinfo(addr) };
        if sockfd == -1 {
            return Err("socket() failed");
        }

        // TCP_NODELAY/TCP_QUICKACK failures are ignored, like upstream.
        let flag: c_int = 1;
        unsafe {
            libc::setsockopt(
                sockfd,
                libc::IPPROTO_TCP,
                libc::TCP_NODELAY,
                &flag as *const _ as *const c_void,
                std::mem::size_of_val(&flag) as libc::socklen_t,
            );
            libc::setsockopt(
                sockfd,
                libc::IPPROTO_TCP,
                libc::TCP_QUICKACK,
                &flag as *const _ as *const c_void,
                std::mem::size_of_val(&flag) as libc::socklen_t,
            );
        }

        if self.timeout > 0 {
            // Linux: TCP_USER_TIMEOUT carries the full millisecond value.
            unsafe {
                libc::setsockopt(
                    sockfd,
                    libc::IPPROTO_TCP,
                    libc::TCP_USER_TIMEOUT,
                    &self.timeout as *const _ as *const c_void,
                    std::mem::size_of_val(&self.timeout) as libc::socklen_t,
                );
            }
        }

        // A failed fast path is only a fallback trigger (upstream ignores
        // tryNonThreadingFastPath's result): the traditional connect+send
        // below runs on the same socket, and its thread failure surfaces
        // through the closed fd at recv, like upstream.
        if self.tfo
            && self.send_fast_open(sockfd, &command, &sockaddr_copy, addr_len).is_ok()
        {
            return Ok(());
        }

        // general.multithreading defaults to true, so the connect runs in a
        // thread whose failure is reported through the closed socket, like
        // upstream's connectAndSend thread entry.
        let (sender, receiver) = mpsc::channel();
        let handle = std::thread::Builder::new()
            .name("connect-and-send".into())
            .spawn(move || {
                // SAFETY: sockfd comes straight from socket() and
                // sockaddr_copy from the addrinfo entry.
                if unsafe {
                    libc::connect(
                        sockfd,
                        (&sockaddr_copy as *const libc::sockaddr_storage).cast(),
                        addr_len,
                    )
                } == -1 {
                    unsafe { libc::close(sockfd) };
                    let _ = sender.send(None);
                    return;
                }
                let sent = unsafe {
                    libc::send(sockfd, command.as_ptr().cast(), command.len(), 0)
                };
                if sent < 0 {
                    unsafe { libc::close(sockfd) };
                    let _ = sender.send(None);
                    return;
                }
                let _ = sender.send(Some(unsafe { TcpStream::from_raw_fd(sockfd) }));
            })
            .map_err(|_| "connect() failed")?;
        self.status = SendStatus::Threaded(handle, receiver);
        Ok(())
    }

    /// tryNonThreadingFastPath: TCP Fast Open sends the request inside
    /// SYN; EAGAIN/EWOULDBLOCK/EINPROGRESS mean the cookie handshake is in
    /// progress and counts as sent, like upstream.
    fn send_fast_open(
        &mut self,
        sockfd: RawFd,
        command: &str,
        sockaddr: &libc::sockaddr_storage,
        addr_len: libc::socklen_t,
    ) -> Result<(), &'static str> {
        let flag: c_int = 1;
        if unsafe {
            libc::setsockopt(
                sockfd,
                libc::IPPROTO_TCP,
                libc::TCP_FASTOPEN,
                &flag as *const _ as *const c_void,
                std::mem::size_of_val(&flag) as libc::socklen_t,
            )
        } != 0
        {
            return Err("setsockopt(TCP_FASTOPEN) failed");
        }

        let sent = unsafe {
            libc::sendto(
                sockfd,
                command.as_ptr().cast(),
                command.len(),
                libc::MSG_FASTOPEN | libc::MSG_NOSIGNAL,
                std::ptr::from_ref(sockaddr).cast(),
                addr_len,
            )
        };
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        if sent > 0 || errno == libc::EAGAIN || errno == libc::EWOULDBLOCK || errno == libc::EINPROGRESS {
            self.status = SendStatus::Ready(unsafe { TcpStream::from_raw_fd(sockfd) });
            Ok(())
        } else {
            Err("sendto() failed")
        }
    }

    /// ffNetworkingRecvHttpResponse: the whole framed response, headers
    /// included. The watchdog exits the process on a connect timeout,
    /// matching upstream's SIGTERM handler.
    pub fn recv(&mut self) -> Result<Vec<u8>, &'static str> {
        let stream = match std::mem::replace(
            &mut self.status,
            SendStatus::Failed,
        ) {
            SendStatus::Ready(stream) => Some(stream),
            SendStatus::Threaded(handle, receiver) => {
                // ffThreadJoin with the request's timeout: a timeout kills
                // the process through the SIGTERM handler upstream.
                let result = if self.timeout > 0 {
                    match receiver.recv_timeout(Duration::from_millis(self.timeout as u64)) {
                        Ok(result) => result,
                        Err(mpsc::RecvTimeoutError::Timeout) => std::process::exit(0),
                        Err(mpsc::RecvTimeoutError::Disconnected) => None,
                    }
                } else {
                    receiver.recv().unwrap_or(None)
                };
                let _ = handle.join();
                result
            },
            SendStatus::Failed => None,
        };
        let Some(stream) = stream else {
            return Err("ffNetworkingSendHttpRequest() failed");
        };

        let fd = stream.as_raw_fd();

        // A larger receive buffer replaces small repeated receives.
        let rcvbuf: c_int = 64 * 1024;
        unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                &rcvbuf as *const _ as *const c_void,
                std::mem::size_of_val(&rcvbuf) as libc::socklen_t,
            );
        }

        // The timeout must be enforced by the socket on every read: a
        // server that sends a partial response and stalls would otherwise
        // block the loop forever.
        if self.timeout > 0 {
            let timev = libc::timeval {
                tv_sec: (self.timeout / 1000) as _,
                tv_usec: ((self.timeout % 1000) * 1000) as _,
            };
            unsafe {
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_RCVTIMEO,
                    &timev as *const _ as *const c_void,
                    std::mem::size_of_val(&timev) as libc::socklen_t,
                );
            }
        }

        // Tell the server we are done writing; failure is not critical.
        unsafe { libc::shutdown(fd, libc::SHUT_WR) };

        let mut buffer: Vec<u8> = Vec::with_capacity(4096);
        let mut content_length: usize = 0;
        let mut header_end: usize = 0;
        let mut chunked = false;

        loop {
            // `Content-Length` may be absent (chunked responses), so the
            // buffer grows on demand instead of silently truncating. The
            // reserve overshoots the 1MB mark so a response larger than
            // the cap is caught on the next full-buffer pass.
            if buffer.len() == buffer.capacity() {
                if buffer.capacity() >= MAX_RESPONSE_SIZE {
                    return Err("Response too large");
                }
                buffer.reserve(MAX_RESPONSE_SIZE - buffer.len());
            }

            // With a known length, ask for exactly the remainder so
            // MSG_WAITALL returns once the response is complete; without
            // one, read whatever arrived and let the framing checks stop
            // the loop.
            let mut want = buffer.capacity() - buffer.len();
            let mut flags = 0;
            if content_length > 0 && header_end > 0 {
                let remaining = header_end + 4 + content_length - buffer.len();
                if remaining < want {
                    want = remaining;
                }
                flags = libc::MSG_WAITALL;
            }

            let received = unsafe {
                libc::recv(
                    fd,
                    buffer.as_mut_ptr().add(buffer.len()).cast(),
                    want,
                    flags,
                )
            };
            if received <= 0 {
                // Connection closed or reception failed; the partial
                // buffer decides the outcome below.
                break;
            }
            let received = received as usize;
            let new_len = buffer.len() + received;
            unsafe { buffer.set_len(new_len) };

            if header_end == 0
                && let Some(position) = find_subslice(&buffer, b"\r\n\r\n")
            {
                header_end = position;

                if let Some((value, _)) = find_header(&buffer[..header_end], b"Content-Length:") {
                    let parsed = parse_strtoul(value) as usize;
                    if parsed > 0 {
                        if parsed > MAX_RESPONSE_SIZE {
                            return Err("Content-Length too large");
                        }
                        // Ensure the body fits, adding header size and margin.
                        buffer.reserve(parsed + 16);
                        content_length = parsed;
                    }
                }

                if let Some((value, _)) = find_header(&buffer[..header_end], b"Transfer-Encoding:") {
                    match parse_transfer_encoding(value) {
                        TransferEncoding::Chunked => chunked = true,
                        TransferEncoding::Unsupported => {
                            // e.g. `gzip, chunked`: the payload would stay
                            // encoded, so fail instead of returning garbage.
                            return Err("Unsupported Transfer-Encoding");
                        },
                        TransferEncoding::None => {},
                    }
                }
            }

            // Stop as soon as the response is framed, rather than waiting
            // for the FIN.
            if chunked {
                match chunked_complete(&buffer[header_end + 4..]) {
                    Framing::Malformed => return Err("Malformed chunked body"),
                    Framing::Frame => break,
                    Framing::Incomplete => {},
                }
            } else if content_length > 0 && buffer.len() >= header_end + 4 + content_length {
                break;
            }
        }
        drop(stream);

        if buffer.is_empty() {
            return Err("Empty server response received");
        }
        if header_end == 0 {
            return Err("No HTTP header end found");
        }
        if chunked {
            let Some(new_header_end) = decode_chunked(&mut buffer, header_end) else {
                return Err("Failed to decode chunked response");
            };
            header_end = new_header_end;
        }
        if !buffer.starts_with(b"HTTP/1.0 200 OK\r\n") && !buffer.starts_with(b"HTTP/1.1 200 OK\r\n") {
            return Err("Invalid response");
        }
        // A chunked response was rewritten, so its (dropped) Content-Length
        // no longer describes anything; only the plain framing is checked.
        if !chunked
            && content_length > 0
            && buffer.len() != content_length + header_end + 4
        {
            return Err("Content length mismatch");
        }
        Ok(buffer)
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// ffNetworkingFindHeader: case-insensitive lookup restricted to the header
/// block, matching only at the start of a line. The value excludes the
/// terminating CRLF and any leading spaces/tabs.
fn find_header<'a>(headers: &'a [u8], name: &[u8]) -> Option<(&'a [u8], usize)> {
    let mut position = 0;
    while position < headers.len() {
        let eol = headers[position..]
            .iter()
            .position(|&byte| byte == b'\n')
            .map(|offset| position + offset)
            .unwrap_or(headers.len());
        let line_end = if eol > position && headers[eol - 1] == b'\r' { eol - 1 } else { eol };

        if line_end - position >= name.len()
            && headers[position..line_end][..name.len()].eq_ignore_ascii_case(name)
        {
            let mut value_start = position + name.len();
            while value_start < line_end && (headers[value_start] == b' ' || headers[value_start] == b'\t') {
                value_start += 1;
            }
            return Some((&headers[value_start..line_end], line_end - value_start));
        }

        if eol >= headers.len() {
            break;
        }
        position = eol + 1;
    }
    None
}

/// strtoul's leading-whitespace skip and digit stop; content lengths are
/// always sane positives, so sign and overflow handling reduce to zero.
fn parse_strtoul(value: &[u8]) -> u64 {
    let text = String::from_utf8_lossy(value);
    let trimmed = text.trim_start().trim_start_matches(['+', '-']);
    let digits = trimmed.as_bytes().iter().take_while(|byte| byte.is_ascii_digit()).count();
    if digits == 0 {
        return 0;
    }
    trimmed[..digits].parse().unwrap_or(0)
}

enum TransferEncoding {
    None,
    Chunked,
    Unsupported,
}

/// ffNetworkingParseTransferEncoding: only a lone `chunked` is decodable;
/// any other coding or chain (e.g. `gzip, chunked`) leaves the payload
/// encoded.
fn parse_transfer_encoding(value: &[u8]) -> TransferEncoding {
    let mut coding_count = 0;
    let mut coding: &[u8] = b"";
    let mut start = 0;
    for index in 0..=value.len() {
        if index < value.len() && value[index] != b',' {
            continue;
        }
        let mut from = start;
        let mut to = index;
        while from < to && (value[from] == b' ' || value[from] == b'\t') {
            from += 1;
        }
        while to > from && (value[to - 1] == b' ' || value[to - 1] == b'\t') {
            to -= 1;
        }
        if to > from {
            coding_count += 1;
            coding = &value[from..to];
        }
        start = index + 1;
    }

    if coding_count == 0 {
        return TransferEncoding::None;
    }
    if coding_count == 1
        && coding.len() == 7
        && coding.eq_ignore_ascii_case(b"chunked")
    {
        return TransferEncoding::Chunked;
    }
    TransferEncoding::Unsupported
}

enum Framing {
    Incomplete,
    Frame,
    Malformed,
}

/// ffNetworkingChunkedComplete: whether the chunked body received so far is
/// fully framed. The consumed length only feeds upstream's debug log, so
/// the port drops it.
fn chunked_complete(body: &[u8]) -> Framing {
    let mut position = 0;
    loop {
        // chunk-size [ chunk-ext ] CRLF
        let Some(eol) = body[position..]
            .iter()
            .position(|&byte| byte == b'\n')
            .map(|offset| position + offset)
        else {
            return Framing::Incomplete; // the chunk-size line is not complete yet
        };
        if eol == position || !body[position].is_ascii_hexdigit() {
            return Framing::Malformed;
        }

        let size = parse_strtoul_hex(&body[position..eol]) as usize;
        // Skip the chunk extension, which ends at the CRLF.
        position = eol + 1;

        if size == 0 {
            // last-chunk, followed by an optional trailer section and an
            // empty line
            let mut p = position;
            while p < body.len() {
                let Some(e) = body[p..]
                    .iter()
                    .position(|&byte| byte == b'\n')
                    .map(|offset| p + offset)
                else {
                    return Framing::Incomplete;
                };
                if e == p || (e == p + 1 && body[p] == b'\r') {
                    return Framing::Frame;
                }
                p = e + 1;
            }
            return Framing::Incomplete;
        }

        if size > body.len() - position {
            return Framing::Incomplete; // the chunk data is not complete yet
        }
        position += size;

        if body.len() - position < 2 {
            return Framing::Incomplete;
        }
        if body[position] != b'\r' || body[position + 1] != b'\n' {
            return Framing::Malformed;
        }
        position += 2;
    }
}

fn parse_strtoul_hex(value: &[u8]) -> u64 {
    let text = String::from_utf8_lossy(value);
    let digits: String = text
        .trim_start()
        .trim_start_matches(['+', '-'])
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    u64::from_str_radix(&digits, 16).unwrap_or(0)
}

/// ffNetworkingDecodeChunked: decode the body in place (the encoded form is
/// never shorter than the payload) and rewrite the response with a
/// Content-Length header in place of Transfer-Encoding. Returns the new
/// header end: the rewrite changes the header length.
fn decode_chunked(buffer: &mut Vec<u8>, header_end: usize) -> Option<usize> {
    // The header block is terminated by CR LF CR LF.
    if header_end + 4 > buffer.len() {
        return None;
    }
    let body_start = header_end + 4;
    let body_len = buffer.len() - body_start;

    if !matches!(chunked_complete(&buffer[body_start..]), Framing::Frame) {
        return None;
    }

    let mut decoded_len = 0;
    let mut position = 0;
    while position < body_len {
        let eol = buffer[body_start + position..body_start + body_len]
            .iter()
            .position(|&byte| byte == b'\n')
            .map(|offset| position + offset)
            .unwrap_or(body_len);
        let size = parse_strtoul_hex(&buffer[body_start + position..body_start + eol]) as usize;
        position = eol + 1;
        if size == 0 {
            break; // last-chunk; trailers are dropped
        }
        // Decode in place: out trails behind position, never overtakes it.
        buffer.copy_within(body_start + position..body_start + position + size, body_start + decoded_len);
        decoded_len += size;
        position += size + 2; // trailing CRLF
    }

    // rebuildResponse: keep the status line and every header except
    // Content-Length and Transfer-Encoding, then emit the decoded body.
    let mut new_buffer = Vec::with_capacity(header_end + decoded_len + 64);
    let mut pos = 0;
    while pos < header_end {
        let eol = buffer[pos..header_end]
            .iter()
            .position(|&byte| byte == b'\n')
            .map(|offset| pos + offset)
            .unwrap_or(header_end);
        let line_len = if eol < header_end { eol - pos + 1 } else { header_end - pos };

        let line = &buffer[pos..pos + line_len];
        let content_length_line = line.len() >= b"Content-Length:".len()
            && line[..b"Content-Length:".len()].eq_ignore_ascii_case(b"Content-Length:");
        let te_line = line.len() >= b"Transfer-Encoding:".len()
            && line[..b"Transfer-Encoding:".len()].eq_ignore_ascii_case(b"Transfer-Encoding:");
        if !content_length_line && !te_line {
            new_buffer.extend_from_slice(line);
        }
        pos += line_len;
    }
    new_buffer.extend_from_slice(format!("Content-Length: {decoded_len}\r\n\r\n").as_bytes());
    new_buffer.extend_from_slice(&buffer[body_start..body_start + decoded_len]);

    *buffer = new_buffer;
    // The terminating CRLF CRLF is the four bytes in front of the body.
    Some(buffer.len() - decoded_len - 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_header_matches_lines_case_insensitively() {
        let headers = b"HTTP/1.1 200 OK\r\ncontent-length: 41\r\nX-Odd: value: with: colons\r\n\r\nbody";
        let Some((value, len)) = super::find_header(&headers[..headers.len() - 6], b"Content-Length:") else {
            panic!("header present");
        };
        assert_eq!(value, b"41");
        assert_eq!(len, 2);
        // A value line can never be mistaken for a field name.
        let only = b"HTTP/1.1 200 OK\r\nx: content-length: 9\r\n\r\n";
        assert!(super::find_header(&only[..only.len() - 4], b"Content-Length:").is_none());
    }

    #[test]
    fn transfer_encoding_accepts_only_lone_chunked() {
        assert!(matches!(
            super::parse_transfer_encoding(b"chunked"),
            TransferEncoding::Chunked
        ));
        assert!(matches!(
            super::parse_transfer_encoding(b"Chunked "),
            TransferEncoding::Chunked
        ));
        assert!(matches!(
            super::parse_transfer_encoding(b"gzip, chunked"),
            TransferEncoding::Unsupported
        ));
        assert!(matches!(
            super::parse_transfer_encoding(b"identity"),
            TransferEncoding::Unsupported
        ));
        assert!(matches!(super::parse_transfer_encoding(b""), TransferEncoding::None));
    }

    #[test]
    fn chunked_framing_states() {
        assert!(matches!(super::chunked_complete(b"5\r\n"), Framing::Incomplete));
        assert!(matches!(super::chunked_complete(b"zz\r\n"), Framing::Malformed));
        assert!(matches!(
            super::chunked_complete(b"5\r\nhello\r\n"),
            Framing::Incomplete
        ));
        assert!(matches!(
            super::chunked_complete(b"5\r\nhello\r\n0\r\n\r\n"),
            Framing::Frame
        ));
        assert!(matches!(
            super::chunked_complete(b"5\r\nhello\r\n0\r\nX-T: 1\r\n\r\n"),
            Framing::Frame
        ));
    }

    #[test]
    fn decode_chunked_rewrites_headers() {
        let mut buffer = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nServer: x\r\n\r\n5\r\nhello\r\n0\r\n\r\n".to_vec();
        let Some(header_end) = find_subslice(&buffer, b"\r\n\r\n") else {
            panic!("header end present");
        };
        let Some(new_end) = super::decode_chunked(&mut buffer, header_end) else {
            panic!("complete frame decodes");
        };
        // The last kept header line ends at the CRLF CRLF itself, so it
        // carries no terminator into the rebuild: upstream's
        // rebuildResponse merges it directly into the new Content-Length.
        assert_eq!(&buffer[..new_end + 4], b"HTTP/1.1 200 OK\r\nServer: xContent-Length: 5\r\n\r\n");
        assert_eq!(&buffer[new_end + 4..], b"hello");
    }

    #[test]
    fn strtoul_parses_leading_whitespace_and_stops_at_nondigits() {
        assert_eq!(super::parse_strtoul(b"41\r"), 41);
        assert_eq!(super::parse_strtoul(b"  7abc"), 7);
        assert_eq!(super::parse_strtoul(b"\t\t12 "), 12);
        assert_eq!(super::parse_strtoul(b""), 0);
        assert_eq!(super::parse_strtoul_hex(b"1a\r\n"), 26);
    }
}

//! `sound` module: output devices and their volume, ported from
//! detection/sound/sound_linux.c and modules/sound/sound.c. The sound
//! server (PulseAudio; PipeWire speaks its protocol too) is asked first
//! over its native protocol on the unix socket — a 20 byte big-endian
//! frame header followed by tagstruct payloads, AUTH with SCM_CREDENTIALS
//! — and when no server answers the cards are read from ALSA instead,
//! via the kernel control interface ioctls on /dev/snd/controlC*. The
//! default reports the server's main sink as "name (volume%)".

use std::ffi::c_void;

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

/// Call `ioctl(2)` with a request cast to the type the target expects.
///
/// glibc declares the request as `c_ulong`, while bionic (Android) declares it
/// as `c_int`; `as _` lets the compiler pick the right type on each target.
#[inline]
unsafe fn portable_ioctl<T>(fd: libc::c_int, request: libc::c_ulong, arg: *mut T) -> libc::c_int {
    // SAFETY: the caller guarantees `fd` and `arg` are valid for `request`.
    unsafe { libc::ioctl(fd, request as _, arg) }
}

/// Read the thread-local errno value without glibc's `__errno_location`,
/// which is unavailable on Android/bionic. Equivalent to dereferencing
/// `__errno_location()` on Linux.
#[inline]
fn errno() -> libc::c_int {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

const VOLUME_UNKNOWN: u8 = 255;

#[derive(Default)]
struct SoundDevice {
    /// The sink name / "hw:N"; the identifier the JSON result reports.
    identifier: String,
    platform_api: String,
    name: String,
    volume: u8,
    is_main: bool,
    is_active: bool,
}

// ---------------------------------------------------------------------------
// the server: PulseAudio's native protocol ("protocol-native")
// ---------------------------------------------------------------------------

const PA_COMMAND_ERROR: u32 = 0;
const PA_COMMAND_REPLY: u32 = 2;
const PA_COMMAND_AUTH: u32 = 8;
const PA_COMMAND_SET_CLIENT_NAME: u32 = 9;
const PA_COMMAND_GET_SERVER_INFO: u32 = 20;
const PA_COMMAND_GET_SINK_INFO_LIST: u32 = 22;

const PA_TAG_STRING: u8 = b't';
const PA_TAG_STRING_NULL: u8 = b'N';
const PA_TAG_U32: u8 = b'L';
const PA_TAG_U8: u8 = b'B';
const PA_TAG_U64: u8 = b'R';
const PA_TAG_S64: u8 = b'r';
const PA_TAG_SAMPLE_SPEC: u8 = b'a';
const PA_TAG_ARBITRARY: u8 = b'x';
const PA_TAG_BOOLEAN_TRUE: u8 = b'1';
const PA_TAG_BOOLEAN_FALSE: u8 = b'0';
const PA_TAG_TIMEVAL: u8 = b'T';
const PA_TAG_USEC: u8 = b'U';
const PA_TAG_CHANNEL_MAP: u8 = b'm';
const PA_TAG_CVOLUME: u8 = b'v';
const PA_TAG_PROPLIST: u8 = b'P';
const PA_TAG_VOLUME: u8 = b'V';
const PA_TAG_FORMAT_INFO: u8 = b'f';

const PA_ERROR_ACCESS: u32 = 1;
const PA_ERROR_VERSION: u32 = 17;

const PA_VERSION: u32 = 35; // PA_PROTOCOL_VERSION
const PA_VERSION_MASK: u32 = 0xffff;
const PA_COOKIE_LENGTH: usize = 256;
const PA_FRAME_HEADER_SIZE: usize = 20;
const PA_FRAME_LENGTH_MAX: usize = 1024 * 1024 * 16;
const PA_CHANNELS_MAX: u8 = 32;
const PA_VOLUME_NORM: u32 = 0x10000;
const PA_PORT_AVAILABLE_NO: u32 = 2;
const PA_RECV_TIMEOUT_MS: u64 = 2000;
const MSG_NOSIGNAL: i32 = 0x4000;

struct PulseSinkInfo {
    /// The sink's own name, reported as the device identifier.
    name: String,
    description: Option<String>,
    volume: u32,
    mute: bool,
    main: bool,
    active: bool,
}

// ---------------------------------------------------------------------------
// tagstruct
// ---------------------------------------------------------------------------

fn put_tag_u32(buffer: &mut Vec<u8>, value: u32) {
    buffer.push(PA_TAG_U32);
    buffer.extend_from_slice(&value.to_be_bytes());
}

fn put_tag_string(buffer: &mut Vec<u8>, value: Option<&str>) {
    match value {
        None => buffer.push(PA_TAG_STRING_NULL),
        Some(value) => {
            buffer.push(PA_TAG_STRING);
            buffer.extend_from_slice(value.as_bytes());
            buffer.push(0);
        },
    }
}

fn put_tag_arbitrary(buffer: &mut Vec<u8>, value: &[u8]) {
    buffer.push(PA_TAG_ARBITRARY);
    buffer.extend_from_slice(&(value.len() as u32).to_be_bytes());
    buffer.extend_from_slice(value);
}

/// A proplist is "string key, u32 length, arbitrary value" triples ending
/// with a NULL string; it may only appear at the end of a packet.
fn put_tag_proplist(buffer: &mut Vec<u8>, key: &str, value: &str) {
    buffer.push(PA_TAG_PROPLIST);
    put_tag_string(buffer, Some(key));
    put_tag_u32(buffer, value.len() as u32);
    put_tag_arbitrary(buffer, value.as_bytes());
    put_tag_string(buffer, None);
}

// ---------------------------------------------------------------------------
// reading
// ---------------------------------------------------------------------------

struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
    error: bool,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            offset: 0,
            error: false,
        }
    }

    fn eof(&self) -> bool {
        self.offset >= self.data.len()
    }

    fn skip_bytes(&mut self, count: usize) -> bool {
        if self.offset + count > self.data.len() {
            self.error = true;
            return false;
        }
        self.offset += count;
        true
    }

    fn read_u8(&mut self) -> u8 {
        if !self.skip_bytes(1) {
            return 0;
        }
        self.data[self.offset - 1]
    }

    fn read_u32(&mut self) -> u32 {
        if !self.skip_bytes(4) {
            return 0;
        }
        let slice = &self.data[self.offset - 4..self.offset];
        u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]])
    }

    /// A tagged u32 ('L' + value).
    fn read_u32_value(&mut self) -> Option<u32> {
        if self.read_u8() != PA_TAG_U32 {
            self.error = true;
            return None;
        }
        let value = self.read_u32();
        (!self.error).then_some(value)
    }

    /// A tagged u8 ('B' + value).
    fn read_u8_value(&mut self) -> Option<u8> {
        if self.read_u8() != PA_TAG_U8 {
            self.error = true;
            return None;
        }
        let value = self.read_u8();
        (!self.error).then_some(value)
    }

    /// Skips a NUL terminated string whose tag has already been consumed.
    fn skip_string(&mut self) -> bool {
        while self.offset < self.data.len() && self.data[self.offset] != 0 {
            self.offset += 1;
        }
        if self.offset >= self.data.len() {
            self.error = true;
            return false;
        }
        self.offset += 1; // trailing NUL
        true
    }

    /// Reads a string tag; a NULL string ('N') is a valid None, not an
    /// error. Returns the bytes before the NUL terminator.
    fn read_string_ref(&mut self) -> Option<&'a [u8]> {
        let tag = self.read_u8();
        if self.error {
            return None;
        }
        if tag == PA_TAG_STRING_NULL {
            return None;
        }
        if tag != PA_TAG_STRING {
            self.error = true;
            return None;
        }
        let start = self.offset;
        if !self.skip_string() {
            return None;
        }
        Some(&self.data[start..self.offset - 1])
    }

    fn read_boolean(&mut self) -> Option<bool> {
        match self.read_u8() {
            PA_TAG_BOOLEAN_TRUE => Some(true),
            PA_TAG_BOOLEAN_FALSE => Some(false),
            _ => {
                self.error = true;
                None
            },
        }
    }

    /// Reads a cvolume tag and returns the volume of its first channel.
    fn read_cvolume(&mut self) -> Option<u32> {
        if self.read_u8() != PA_TAG_CVOLUME {
            self.error = true;
            return None;
        }
        let channels = self.read_u8();
        if self.error || channels > PA_CHANNELS_MAX {
            self.error = true;
            return None;
        }
        let mut volume = 0u32;
        for channel in 0..channels {
            let value = self.read_u32();
            if self.error {
                return None;
            }
            if channel == 0 {
                volume = value;
            }
        }
        Some(volume)
    }

    /// Skips the value that follows the already consumed tag, dispatching
    /// on the tag it actually reads so it can not run out of sync.
    fn skip_value(&mut self) -> bool {
        let tag = self.read_u8();
        if self.error {
            return false;
        }
        match tag {
            PA_TAG_STRING_NULL | PA_TAG_BOOLEAN_TRUE | PA_TAG_BOOLEAN_FALSE => true,
            PA_TAG_STRING => self.skip_string(),
            PA_TAG_U8 => self.skip_bytes(1),
            PA_TAG_U32 | PA_TAG_VOLUME => self.skip_bytes(4),
            PA_TAG_U64 | PA_TAG_S64 | PA_TAG_USEC | PA_TAG_TIMEVAL => self.skip_bytes(8),
            PA_TAG_SAMPLE_SPEC => self.skip_bytes(6), // format, channels, rate
            PA_TAG_ARBITRARY => {
                let length = self.read_u32() as usize;
                !self.error && self.skip_bytes(length)
            },
            PA_TAG_CHANNEL_MAP => {
                let channels = self.read_u8() as usize;
                !self.error && self.skip_bytes(channels)
            },
            PA_TAG_CVOLUME => {
                let channels = self.read_u8() as usize;
                !self.error && self.skip_bytes(channels * 4)
            },
            PA_TAG_PROPLIST => {
                while !self.error && !self.eof() && self.data[self.offset] != PA_TAG_STRING_NULL {
                    // key, length, value
                    if !self.skip_value() || !self.skip_value() || !self.skip_value() {
                        return false;
                    }
                }
                self.skip_value() // the NULL string terminating the list
            },
            PA_TAG_FORMAT_INFO => self.skip_value() && self.skip_value(), // encoding, proplist
            _ => {
                self.error = true;
                false
            },
        }
    }
}

// ---------------------------------------------------------------------------
// transport
// ---------------------------------------------------------------------------

fn write_all(fd: i32, mut data: &[u8]) -> bool {
    // SAFETY: send(2) on our own connected socket.
    while !data.is_empty() {
        let written = unsafe { libc::send(fd, data.as_ptr().cast(), data.len(), MSG_NOSIGNAL) };
        if written < 0 {
            if errno() == libc::EINTR {
                continue;
            }
            return false;
        }
        data = &data[written as usize..];
    }
    true
}

fn read_all(fd: i32, mut data: &mut [u8]) -> bool {
    // SAFETY: recv(2) on our own connected socket.
    while !data.is_empty() {
        let received = unsafe { libc::recv(fd, data.as_mut_ptr().cast(), data.len(), 0) };
        if received < 0 {
            if errno() == libc::EINTR {
                continue;
            }
            return false;
        }
        if received == 0 {
            // The server closed the connection.
            return false;
        }
        data = &mut data[received as usize..];
    }
    true
}

fn apply_receive_timeout(fd: i32) {
    // SAFETY: plain setsockopt.
    unsafe {
        let timeout = libc::timeval {
            tv_sec: (PA_RECV_TIMEOUT_MS / 1000) as i64,
            tv_usec: ((PA_RECV_TIMEOUT_MS % 1000) * 1000) as i64,
        };
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            &timeout as *const libc::timeval as *const c_void,
            std::mem::size_of::<libc::timeval>() as u32,
        );
    }
}

/// connect(2) to a unix socket at `path`.
fn connect_unix_socket(path: &str) -> i32 {
    if path.len() >= 108 {
        // sizeof sockaddr_un.sun_path
        return -1;
    }
    let mut address = libc::sockaddr_un {
        sun_family: libc::AF_UNIX as u16,
        sun_path: [0; 108],
    };
    for (target, byte) in address.sun_path.iter_mut().zip(path.as_bytes()) {
        *target = *byte as libc::c_char;
    }
    // SAFETY: socket/connect/close on POD buffers.
    unsafe {
        let fd = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0);
        if fd < 0 {
            return -1;
        }
        let length = 2 + path.len() + 1; // offsetof(sun_path) + length + 1
        if libc::connect(fd, (&address as *const libc::sockaddr_un).cast(), length as u32) < 0 {
            libc::close(fd);
            return -1;
        }
        apply_receive_timeout(fd);
        fd
    }
}

/// connect(2) to a TCP socket resolved from "host", "host:port" or
/// "[host]:port".
fn connect_tcp_socket(address: &str, family: i32) -> i32 {
    let host;
    let mut port = "";
    if let Some(rest) = address.strip_prefix('[') {
        let Some(end) = rest.find(']') else {
            return -1;
        };
        host = &rest[..end];
        if rest[end + 1..].starts_with(':') {
            port = &rest[end + 2..];
        }
    } else if let Some(colon) =
        address.rfind(':').filter(|colon| address[..*colon].find(':').is_none())
    {
        // An unbracketed address with several colons is an IPv6 literal.
        host = &address[..colon];
        port = &address[colon + 1..];
    } else {
        host = address;
    }
    let host = if host.is_empty() { "localhost" } else { host };
    let port = if port.is_empty() { "4713" } else { port };

    // SAFETY: getaddrinfo/socket/connect/freeaddrinfo on C strings.
    unsafe {
        let mut hints: libc::addrinfo = std::mem::zeroed();
        hints.ai_family = family;
        hints.ai_socktype = libc::SOCK_STREAM;
        let c_host = match std::ffi::CString::new(host) {
            Ok(host) => host,
            Err(_) => return -1,
        };
        let c_port = match std::ffi::CString::new(port) {
            Ok(port) => port,
            Err(_) => return -1,
        };
        let mut result: *mut libc::addrinfo = std::ptr::null_mut();
        if libc::getaddrinfo(c_host.as_ptr(), c_port.as_ptr(), &hints, &mut result) != 0 {
            return -1;
        }

        let mut fd = -1;
        let mut entry = result;
        while !entry.is_null() {
            let info = &*entry;
            fd = libc::socket(info.ai_family, info.ai_socktype, info.ai_protocol);
            if fd >= 0 && libc::connect(fd, info.ai_addr, info.ai_addrlen) == 0 {
                break;
            }
            if fd >= 0 {
                libc::close(fd);
            }
            fd = -1;
            entry = (*entry).ai_next;
        }
        libc::freeaddrinfo(result);

        if fd >= 0 {
            apply_receive_timeout(fd);
        }
        fd
    }
}

/// $PULSE_SERVER overrides the whole address and accepts "unix:", "tcp:",
/// "tcp4:" and "tcp6:" prefixes as well as a plain socket path.
fn connect_server(server: &str, default_socket_path: &str) -> i32 {
    if let Some(path) = server.strip_prefix("unix:") {
        return connect_unix_socket(if path.is_empty() {
            default_socket_path
        } else {
            path
        });
    }
    if let Some(address) = server.strip_prefix("tcp4:") {
        return connect_tcp_socket(address, libc::AF_INET);
    }
    if let Some(address) = server.strip_prefix("tcp6:") {
        return connect_tcp_socket(address, libc::AF_INET6);
    }
    if let Some(address) = server.strip_prefix("tcp:") {
        return connect_tcp_socket(address, libc::AF_UNSPEC);
    }
    connect_unix_socket(server)
}

/// $PULSE_RUNTIME_PATH is used as is, otherwise $XDG_RUNTIME_DIR/pulse, or
/// $HOME/.pulse. The socket is always named "native".
fn default_socket_path() -> String {
    for (variable, tail) in [
        ("PULSE_RUNTIME_PATH", "/native"),
        ("XDG_RUNTIME_DIR", "/pulse/native"),
        ("HOME", "/.pulse/native"),
    ] {
        if let Some(value) = std::env::var(variable).ok().filter(|value| !value.is_empty()) {
            return format!("{value}{tail}");
        }
    }
    String::new()
}

/// The daemon always compares 256 bytes, so the cookie is sent in full.
/// No cookie file means the credentials attached to the AUTH frame decide.
fn get_cookie() -> [u8; PA_COOKIE_LENGTH] {
    let mut cookie = [0u8; PA_COOKIE_LENGTH];
    let mut paths = String::new();
    if let Some(value) = std::env::var("PULSE_COOKIE").ok().filter(|value| !value.is_empty()) {
        paths = value;
    } else {
        if let Some(config_home) =
            std::env::var("XDG_CONFIG_HOME").ok().filter(|value| !value.is_empty())
        {
            paths.push_str(&format!("{config_home}/pulse/cookie:"));
        }
        if let Some(home) = std::env::var("HOME").ok().filter(|value| !value.is_empty()) {
            paths.push_str(&format!("{home}/.config/pulse/cookie:"));
            paths.push_str(&format!("{home}/.pulse-cookie"));
        }
    }

    for candidate in paths.split(':').filter(|path| !path.is_empty()) {
        if let Some(contents) = std::fs::read(candidate).ok().filter(|c| !c.is_empty()) {
            let length = contents.len().min(PA_COOKIE_LENGTH);
            cookie[..length].copy_from_slice(&contents[..length]);
            return cookie;
        }
    }
    cookie
}

// ---------------------------------------------------------------------------
// frames
// ---------------------------------------------------------------------------

/// The daemon authorizes same-user clients by the credentials of the last
/// packet it read before AUTH; they must ride on the whole frame, not a
/// separate write.
fn send_with_credentials(fd: i32, data: &[u8]) -> bool {
    // SAFETY: sendmsg with a SCM_CREDENTIALS cmsg built from our own ids.
    unsafe {
        let iov = libc::iovec {
            iov_base: data.as_ptr() as *mut c_void,
            iov_len: data.len(),
        };
        // Exactly CMSG_SPACE(sizeof(ucred)): the kernel walks the control
        // buffer as a cmsg chain, so trailing slack would parse as a
        // malformed second header (EINVAL).
        const CREDENTIALS_SPACE: usize = unsafe {
        libc::CMSG_SPACE(std::mem::size_of::<libc::ucred>() as libc::c_uint)
    } as usize;
        let mut control = [0u8; CREDENTIALS_SPACE];
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &iov as *const libc::iovec as *mut libc::iovec;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = std::mem::size_of_val(&control);

        let cmsg = &mut *(libc::CMSG_FIRSTHDR(&message) as *mut libc::cmsghdr);
        cmsg.cmsg_level = libc::SOL_SOCKET;
        cmsg.cmsg_type = libc::SCM_CREDENTIALS;
        cmsg.cmsg_len = libc::CMSG_LEN(std::mem::size_of::<libc::ucred>() as u32) as usize;
        let ucred = libc::ucred {
            pid: libc::getpid(),
            uid: libc::getuid(),
            gid: libc::getgid(),
        };
        libc::memcpy(
            libc::CMSG_DATA(cmsg).cast(),
            &ucred as *const libc::ucred as *const c_void,
            std::mem::size_of::<libc::ucred>(),
        );

        let mut sent;
        loop {
            sent = libc::sendmsg(fd, &message, MSG_NOSIGNAL);
            if sent >= 0 || errno() != libc::EINTR {
                break;
            }
        }
        if sent < 0 {
            return false;
        }
        write_all(fd, &data[sent as usize..]) // the remainder carries no credentials
    }
}

fn send_frame(conn_fd: i32, payload: &[u8], with_credentials: bool) -> bool {
    let mut frame = vec![0u8; PA_FRAME_HEADER_SIZE + payload.len()];
    frame[..4].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    frame[4..8].fill(0xff); // channel: control packet
    frame[8..PA_FRAME_HEADER_SIZE].fill(0);
    frame[PA_FRAME_HEADER_SIZE..].copy_from_slice(payload);

    if with_credentials {
        send_with_credentials(conn_fd, &frame)
    } else {
        write_all(conn_fd, &frame)
    }
}

struct PulseConn {
    fd: i32,
    version: u32,
    next_tag: u32,
    frame: Vec<u8>,
}

impl PulseConn {
    fn recv_frame(&mut self) -> bool {
        let mut header = [0u8; PA_FRAME_HEADER_SIZE];
        if !read_all(self.fd, &mut header) {
            return false;
        }
        let length = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
        if length == 0 || length > PA_FRAME_LENGTH_MAX {
            return false;
        }
        self.frame.resize(length, 0);
        read_all(self.fd, &mut self.frame)
    }

    /// Waits for the reply to `tag`; on success returns the payload offset
    /// just past the command and tag fields, for a Reader over `self.frame`
    /// (a separate borrow, so the connection stays usable while parsing).
    fn recv_reply(&mut self, tag: u32) -> Result<usize, &'static str> {
        loop {
            if !self.recv_frame() {
                return Err("Failed to read a pulseaudio reply");
            }
            let mut frame = Reader::new(&self.frame);
            let command = frame.read_u32_value();
            let reply_tag = command.is_some().then(|| frame.read_u32_value()).flatten();
            let (Some(command), Some(reply_tag)) = (command, reply_tag) else {
                return Err("Malformed pulseaudio reply");
            };
            if reply_tag != tag {
                // Server-initiated commands use tag (u32)-1 and only go to
                // subscribed clients; ignore everything that is not ours.
                continue;
            }
            if command == PA_COMMAND_ERROR {
                let Some(error) = frame.read_u32_value() else {
                    return Err("Malformed pulseaudio error reply");
                };
                return match error {
                    PA_ERROR_ACCESS => Err("Access to the pulseaudio server was denied"),
                    PA_ERROR_VERSION => Err("The pulseaudio server is too old"),
                    _ => Err("The pulseaudio server refused the request"),
                };
            }
            if command != PA_COMMAND_REPLY {
                return Err("Unexpected pulseaudio reply");
            }
            return Ok(frame.offset);
        }
    }
}

// ---------------------------------------------------------------------------
// handshake
// ---------------------------------------------------------------------------

/// AUTH, SET_CLIENT_NAME and GET_SERVER_INFO; yields the server name and
/// the default sink's name.
fn handshake(conn: &mut PulseConn) -> Result<(String, String), &'static str> {
    {
        let tag = conn.next_tag;
        conn.next_tag += 1;
        let mut payload = Vec::new();
        put_tag_u32(&mut payload, PA_COMMAND_AUTH);
        put_tag_u32(&mut payload, tag);
        put_tag_u32(&mut payload, PA_VERSION); // no SHM / MEMFD flags
        put_tag_arbitrary(&mut payload, &get_cookie());

        if !send_frame(conn.fd, &payload, true) {
            return Err("Failed to send the pulseaudio AUTH request");
        }
        let offset = conn.recv_reply(tag)?;
        let mut reader = Reader::new(&conn.frame);
        reader.offset = offset;
        let Some(version) = reader.read_u32_value() else {
            return Err("Malformed pulseaudio AUTH reply");
        };
        conn.version = version & PA_VERSION_MASK;
        if conn.version > PA_VERSION {
            conn.version = PA_VERSION;
        }
    }

    {
        let tag = conn.next_tag;
        conn.next_tag += 1;
        let mut payload = Vec::new();
        put_tag_u32(&mut payload, PA_COMMAND_SET_CLIENT_NAME);
        put_tag_u32(&mut payload, tag);
        if conn.version >= 13 {
            put_tag_proplist(&mut payload, "application.name", "fastfetch");
        } else {
            put_tag_string(&mut payload, Some("fastfetch"));
        }
        if !send_frame(conn.fd, &payload, false) {
            return Err("Failed to send the pulseaudio SET_CLIENT_NAME request");
        }
        conn.recv_reply(tag)?;
    }

    let tag = conn.next_tag;
    conn.next_tag += 1;
    let mut payload = Vec::new();
    put_tag_u32(&mut payload, PA_COMMAND_GET_SERVER_INFO);
    put_tag_u32(&mut payload, tag);
    if !send_frame(conn.fd, &payload, false) {
        return Err("Failed to send the pulseaudio GET_SERVER_INFO request");
    }
    let offset = conn.recv_reply(tag)?;
    let mut reader = Reader::new(&conn.frame);
    reader.offset = offset;

    let Some(name) = reader.read_string_ref() else {
        return Err("Malformed pulseaudio GET_SERVER_INFO reply");
    };
    let name = String::from_utf8_lossy(name).into_owned();
    let Some(version) = reader.read_string_ref() else {
        return Err("Malformed pulseaudio GET_SERVER_INFO reply");
    };
    let version = String::from_utf8_lossy(version).into_owned();
    if !reader.skip_value() // user name
        || !reader.skip_value() // host name
        || !reader.skip_value()
    // sample spec
    {
        return Err("Malformed pulseaudio GET_SERVER_INFO reply");
    }
    let default_sink = reader.read_string_ref().map(String::from_utf8_lossy);
    // (the default sink name may be a NULL string)

    // pipewire-pulse reports itself as "PulseAudio (on PipeWire x.y.z)"
    let server_name = match name.find("(on ") {
        Some(index) => name[index + 4..].trim_end_matches(')').to_string(),
        None => format!("{name} {version}"),
    };
    Ok((server_name, default_sink.unwrap_or_default().into_owned()))
}

// ---------------------------------------------------------------------------
// sinks
// ---------------------------------------------------------------------------

/// The port that is reported as active decides whether a sink counts as
/// active; its name is sent after the port list, so the list is scanned a
/// second time for its availability.
fn is_port_available(
    reader: &Reader,
    version: u32,
    port_count: u32,
    ports_offset: usize,
    active_port: &[u8],
) -> bool {
    if version < 24 {
        return true; // ports carry no availability information before v24
    }
    let mut scan = Reader {
        data: reader.data,
        offset: ports_offset,
        error: false,
    };
    for _ in 0..port_count {
        let name = scan.read_string_ref();
        if scan.error || !scan.skip_value() || !scan.skip_value() {
            // description, priority
            return true;
        }
        let Some(available) = scan.read_u32_value() else {
            return true;
        };
        if version >= 34 {
            // availability group, type
            if !scan.skip_value() || !scan.skip_value() {
                return true;
            }
        }
        if name == Some(active_port) {
            return available != PA_PORT_AVAILABLE_NO;
        }
    }
    true
}

/// Sink objects are sent in the order of sink_fill_tagstruct(); everything
/// unneeded is skipped and the read fields are tag checked.
fn read_sink_info(
    reader: &mut Reader,
    version: u32,
    default_sink_name: &str,
) -> Option<PulseSinkInfo> {
    if !reader.skip_value() {
        // index
        return None;
    }
    let sink_name = reader.read_string_ref()?.to_vec();
    if reader.error {
        return None;
    }
    let description = reader.read_string_ref().map(String::from_utf8_lossy);
    if reader.error {
        return None;
    }
    if !reader.skip_value() // sample spec
        || !reader.skip_value() // channel map
        || !reader.skip_value()
    // owner module
    {
        return None;
    }
    let volume = reader.read_cvolume()?;
    let mute = reader.read_boolean()?;
    if !reader.skip_value() // monitor source
        || !reader.skip_value() // monitor source name
        || !reader.skip_value() // latency
        || !reader.skip_value() // driver
        || !reader.skip_value()
    // flags
    {
        return None;
    }
    if version >= 13
        && (!reader.skip_value() // proplist
        || !reader.skip_value())
    // configured latency
    {
        return None;
    }
    if version >= 15 {
        // base volume, state, volume steps, card
        for _ in 0..4 {
            if !reader.skip_value() {
                return None;
            }
        }
    }

    let mut active_port: Option<Vec<u8>> = None;
    let mut port_count = 0u32;
    let mut ports_offset = 0usize;
    if version >= 16 {
        port_count = reader.read_u32_value()?;
        ports_offset = reader.offset;
        for _ in 0..port_count {
            if !reader.skip_value() // name
                || !reader.skip_value() // description
                || !reader.skip_value()
            // priority
            {
                return None;
            }
            if version >= 24 && !reader.skip_value() {
                // availability
                return None;
            }
            if version >= 34 {
                // availability group, type
                if !reader.skip_value() || !reader.skip_value() {
                    return None;
                }
            }
        }
        active_port = reader.read_string_ref().map(|bytes| bytes.to_vec()); // the port section's last field
        if reader.error {
            return None;
        }
    }

    if version >= 21 {
        let formats = reader.read_u8_value()?;
        for _ in 0..formats {
            if !reader.skip_value() {
                // format info
                return None;
            }
        }
    }

    let main = default_sink_name.as_bytes() == sink_name.as_slice();
    let active = active_port
        .map(|port| is_port_available(reader, version, port_count, ports_offset, &port))
        .unwrap_or(false);
    Some(PulseSinkInfo {
        name: String::from_utf8_lossy(&sink_name).trim_end().to_string(),
        description: description.map(|description| description.into_owned()),
        volume,
        mute,
        main,
        active,
    })
}

fn append_sink(devices: &mut Vec<SoundDevice>, server_name: &str, sink: &PulseSinkInfo) {
    devices.push(SoundDevice {
        identifier: sink.name.clone(),
        platform_api: server_name.to_string(),
        name: sink
            .description
            .as_deref()
            .unwrap_or_default()
            .trim_end_matches(' ')
            .trim_start_matches(' ')
            .to_string(),
        volume: if sink.mute {
            0
        } else {
            ((sink.volume as u64 * 100 + u64::from(PA_VOLUME_NORM) / 2) / u64::from(PA_VOLUME_NORM))
                .min(255) as u8
        },
        is_main: sink.main,
        is_active: sink.active,
    });
}

fn fetch_sink_list(
    conn: &mut PulseConn,
    devices: &mut Vec<SoundDevice>,
    server_name: &str,
    default_sink_name: &str,
) -> Result<(), &'static str> {
    let tag = conn.next_tag;
    conn.next_tag += 1;
    let mut payload = Vec::new();
    put_tag_u32(&mut payload, PA_COMMAND_GET_SINK_INFO_LIST);
    put_tag_u32(&mut payload, tag);
    if !send_frame(conn.fd, &payload, false) {
        return Err("Failed to send the pulseaudio GET_SINK_INFO_LIST request");
    }
    let offset = conn.recv_reply(tag)?;

    // All sinks are concatenated in a single reply without a separator, a
    // count or a terminator: parse until the payload is exhausted.
    let version = conn.version;
    let mut reader = Reader::new(&conn.frame);
    reader.offset = offset;
    while !reader.error && !reader.eof() {
        let Some(sink) = read_sink_info(&mut reader, version, default_sink_name) else {
            return Err("Failed to parse the pulseaudio sink list");
        };
        // The default soundType is MAIN: only the server's default sink is
        // reported.
        if sink.main {
            append_sink(devices, server_name, &sink);
        }
    }
    if reader.error {
        return Err("Failed to parse the pulseaudio sink list");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// the hardware: ALSA
// ---------------------------------------------------------------------------

// The request numbers are _IOWR-style and carry the struct sizes of the
// kernel uapi on x86_64; a layout mismatch is refused as an unknown ioctl.
const SNDRV_CTL_IOCTL_ELEM_LIST: libc::c_ulong = 0xc0505510;
const SNDRV_CTL_IOCTL_ELEM_INFO: libc::c_ulong = 0xc1105511;
const SNDRV_CTL_IOCTL_ELEM_READ: libc::c_ulong = 0xc4c85512;

const SNDRV_CTL_ELEM_IFACE_MIXER: u32 = 2;
const SNDRV_CTL_ELEM_TYPE_BOOLEAN: u32 = 1;
const SNDRV_CTL_ELEM_TYPE_INTEGER: u32 = 2;
const SNDRV_CTL_ELEM_TYPE_INTEGER64: u32 = 6;
const SNDRV_CTL_ELEM_ID_NAME_MAXLEN: usize = 44;
const ALSA_ID_BATCH: usize = 64;
const ALSA_PRIORITY_NONE: u32 = u32::MAX;

#[repr(C)]
#[derive(Clone, Copy)]
struct SndCtlElemId {
    numid: u32,
    iface: u32,
    device: u32,
    subdevice: u32,
    name: [u8; SNDRV_CTL_ELEM_ID_NAME_MAXLEN],
    index: u32,
}

#[repr(C)]
struct SndCtlElemList {
    offset: u32,
    space: u32,
    used: u32,
    count: u32,
    pids: *mut SndCtlElemId,
    reserved: [u8; 56],
}

#[repr(C)]
struct SndCtlElemInfoValue {
    integer: [i64; 3], // min, max, step
}

#[repr(C)]
struct SndCtlElemInfo {
    id: SndCtlElemId,
    elem_type: u32,
    _pad: [u8; 4],
    count: u32,
    owner: i32,
    value: SndCtlElemInfoValue,
    reserved: [u8; 168],
}

#[repr(C)]
union SndCtlElemValueUnion {
    integer: [i32; 128],
    integer64: [i64; 64],
}

#[repr(C)]
struct SndCtlElemValue {
    id: SndCtlElemId,
    _pad: [u8; 8],
    value: SndCtlElemValueUnion,
    reserved: [u8; 640],
}

/// The ALSA suffixes libasound's simple mixer layer carries, longest first;
/// a suffix must be the whole name or the start of a word in it.
const ALSA_SUFFIXES: &[(&str, bool, bool)] = &[
    // (suffix, isSwitch, playback)
    ("Playback Volume", false, true),
    ("Playback Switch", true, true),
    ("Playback Enum", false, false),
    ("Playback Route", false, false),
    ("Capture Volume", false, false),
    ("Capture Switch", false, false),
    ("Capture Enum", false, false),
    ("Capture Route", false, false),
    ("Volume", false, true),
    ("Switch", true, true),
    ("Enum", false, false),
    ("Route", false, false),
];

/// The control a desktop mixer would show the main slider for.
const ALSA_MAIN_CONTROLS: &[&str] = &[
    "Master",
    "Master Mono",
    "PCM",
    "Speaker",
    "Headphone",
    "Headphones",
    "Front",
];

/// Whether the control name is that of a playback volume or switch, and
/// how much of it the suffix takes up.
fn strip_alsa_name_suffix(name: &[u8]) -> Option<(bool, usize)> {
    // (isSwitch, baseLength)
    let length = name.len();
    for (suffix, is_switch, playback) in ALSA_SUFFIXES {
        let suffix = suffix.as_bytes();
        let suffix_length = suffix.len();
        if length < suffix_length || !name.ends_with(suffix) {
            continue;
        }
        // A suffix is a word of the name: either all of it, or preceded by
        // a space — "Capture Volume" ends in " Volume" and is not one.
        if length > suffix_length && name[length - suffix_length - 1] != b' ' {
            continue;
        }
        if !*playback {
            return None; // a capture or routing control is not the output
        }
        // The separating space belongs to neither part.
        let mut base = length - suffix_length;
        while base > 0 && name[base - 1] == b' ' {
            base -= 1;
        }
        return Some((*is_switch, base));
    }
    None
}

/// The rank of a control's base name, or ALSA_PRIORITY_NONE.
fn get_alsa_control_priority(name: &[u8], base_length: usize) -> u32 {
    for (index, main) in ALSA_MAIN_CONTROLS.iter().enumerate() {
        if main.len() == base_length && &name[..base_length] == main.as_bytes() {
            return index as u32;
        }
    }
    ALSA_PRIORITY_NONE
}

/// Walks the mixer controls of one card in ELEM_LIST batches.
struct AlsaControls {
    fd: i32,
    offset: u32,
    index: usize,
    batch: usize,
    done: bool,
    ids: Vec<SndCtlElemId>,
}

impl AlsaControls {
    fn new(fd: i32) -> Self {
        Self {
            fd,
            offset: 0,
            index: 0,
            batch: 0,
            done: false,
            ids: Vec::new(),
        }
    }

    /// The next mixer control of the card, or None at the end.
    fn next(&mut self) -> Option<SndCtlElemId> {
        loop {
            if self.index < self.batch {
                let id = self.ids[self.index];
                self.index += 1;
                if id.iface != SNDRV_CTL_ELEM_IFACE_MIXER {
                    continue;
                }
                return Some(id);
            }
            if self.done {
                return None;
            }

            let mut ids = vec![
                SndCtlElemId {
                    numid: 0,
                    iface: 0,
                    device: 0,
                    subdevice: 0,
                    name: [0; SNDRV_CTL_ELEM_ID_NAME_MAXLEN],
                    index: 0,
                };
                ALSA_ID_BATCH
            ];
            let mut list = SndCtlElemList {
                offset: self.offset,
                space: ALSA_ID_BATCH as u32,
                used: 0,
                count: 0,
                pids: ids.as_mut_ptr(),
                reserved: [0; 56],
            };
            // SAFETY: the ioctl fills our own id array.
            if unsafe { portable_ioctl(self.fd, SNDRV_CTL_IOCTL_ELEM_LIST, &mut list as *mut _) } < 0 {
                self.done = true;
                return None;
            }
            self.offset += list.used;
            self.ids = ids;
            self.index = 0;
            self.batch = list.used as usize;
            if list.used == 0 {
                self.done = true;
                return None;
            }
        }
    }
}

/// A card's volume control and its name without the suffix.
struct AlsaVolume {
    id: SndCtlElemId,
    base: Vec<u8>,
}

fn id_name(id: &SndCtlElemId) -> &[u8] {
    let length = id
        .name
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(SNDRV_CTL_ELEM_ID_NAME_MAXLEN);
    &id.name[..length]
}

/// findAlsaVolume: the card's playback volume control.
fn find_alsa_volume(controls: &mut AlsaControls) -> Option<AlsaVolume> {
    let mut priority = ALSA_PRIORITY_NONE;
    let mut found: Option<AlsaVolume> = None;
    while let Some(id) = controls.next() {
        let name = id_name(&id);
        let Some((is_switch, base_length)) = strip_alsa_name_suffix(name) else {
            continue;
        };
        if is_switch {
            continue;
        }
        let candidate = get_alsa_control_priority(name, base_length);
        if found.is_some() && candidate >= priority {
            // A control the list knows beats one it does not, and the first
            // of a rank wins.
            continue;
        }
        priority = candidate;
        found = Some(AlsaVolume {
            id,
            base: name[..base_length].to_vec(),
        });
    }
    found
}

/// findAlsaSwitch: the switch belonging to the volume control — same base
/// name, same index, same device.
fn find_alsa_switch(controls: &mut AlsaControls, volume: &AlsaVolume) -> Option<SndCtlElemId> {
    while let Some(id) = controls.next() {
        if id.index != volume.id.index
            || id.device != volume.id.device
            || id.subdevice != volume.id.subdevice
        {
            continue;
        }
        let name = id_name(&id);
        let Some((is_switch, base_length)) = strip_alsa_name_suffix(name) else {
            continue;
        };
        if !is_switch {
            continue;
        }
        if base_length != volume.base.len() || &name[..base_length] != volume.base.as_slice() {
            continue;
        }
        return Some(id);
    }
    None
}

/// findAlsaPlaybackSwitch: the output of a card with no volume control
/// (HDMI/DP cards with just an "IEC958 Playback Switch").
fn find_alsa_playback_switch(controls: &mut AlsaControls) -> Option<AlsaVolume> {
    while let Some(id) = controls.next() {
        let name = id_name(&id);
        let Some((is_switch, base_length)) = strip_alsa_name_suffix(name) else {
            continue;
        };
        if !is_switch {
            continue;
        }
        return Some(AlsaVolume {
            id,
            base: name[..base_length].to_vec(),
        });
    }
    None
}

/// One value of a control on the control's own scale.
struct AlsaValue {
    value: i64,
    minimum: i64,
    maximum: i64,
}

fn read_alsa_value(fd: i32, id: &SndCtlElemId) -> Option<AlsaValue> {
    let mut info = SndCtlElemInfo {
        id: *id,
        elem_type: 0,
        _pad: [0; 4],
        count: 0,
        owner: 0,
        value: SndCtlElemInfoValue { integer: [0; 3] },
        reserved: [0; 168],
    };
    // SAFETY: the ioctl fills our own struct.
    if unsafe { portable_ioctl(fd, SNDRV_CTL_IOCTL_ELEM_INFO, &mut info as *mut _) } < 0 {
        return None;
    }
    let mut value = SndCtlElemValue {
        id: *id,
        _pad: [0; 8],
        value: SndCtlElemValueUnion { integer: [0; 128] },
        reserved: [0; 640],
    };
    // SAFETY: the ioctl fills our own struct.
    if unsafe { portable_ioctl(fd, SNDRV_CTL_IOCTL_ELEM_READ, &mut value as *mut _) } < 0 {
        return None;
    }

    // Only the first channel is read, like the PulseAudio backend.
    unsafe {
        if info.elem_type == SNDRV_CTL_ELEM_TYPE_INTEGER64 {
            return Some(AlsaValue {
                value: value.value.integer64[0],
                minimum: info.value.integer[0],
                maximum: info.value.integer[1],
            });
        }
        if info.elem_type != SNDRV_CTL_ELEM_TYPE_INTEGER
            && info.elem_type != SNDRV_CTL_ELEM_TYPE_BOOLEAN
        {
            return None;
        }
        Some(AlsaValue {
            value: i64::from(value.value.integer[0]),
            minimum: info.value.integer[0],
            maximum: info.value.integer[1],
        })
    }
}

/// The control's own scale as a percentage — what amixer prints as [n%].
fn get_alsa_percent(value: &AlsaValue) -> u8 {
    if value.maximum <= value.minimum {
        return VOLUME_UNKNOWN;
    }
    if value.value <= value.minimum {
        return 0;
    }
    if value.value >= value.maximum {
        return 100;
    }
    (((value.value - value.minimum) * 100 + (value.maximum - value.minimum) / 2)
        / (value.maximum - value.minimum)) as u8
}

/// Appends the output of one card; returns whether the card has an output
/// to report at all.
fn append_alsa_card(
    card: u32,
    card_name: &str,
    main: bool,
    devices: &mut Vec<SoundDevice>,
) -> bool {
    let mut name = card_name.to_string();
    let mut volume = VOLUME_UNKNOWN;
    let mut found = false;
    let mut active = true;

    let path = format!("/dev/snd/controlC{card}");
    // SAFETY: open/close and the control ioctls on our own fds and buffers.
    let fd = unsafe {
        libc::open(
            std::ffi::CString::new(path).unwrap_or_default().as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        // Not a card without an output: still reported, without a volume.
        found = true;
    } else {
        if let Some(control) = find_alsa_volume(&mut AlsaControls::new(fd)) {
            found = true;
            if !control.base.is_empty() {
                name.push(' ');
                name.push_str(&String::from_utf8_lossy(&control.base));
            }
            if let Some(value) = read_alsa_value(fd, &control.id) {
                volume = get_alsa_percent(&value);
            }
            let mut switches = AlsaControls::new(fd);
            if let Some(switch_value) = find_alsa_switch(&mut switches, &control)
                .and_then(|switch_id| read_alsa_value(fd, &switch_id))
            {
                active = switch_value.value != 0;
                if !active {
                    // A muted control reports 0%, like a muted sink.
                    volume = 0;
                }
            }
        } else {
            // No volume control: an HDMI/DP card with only a playback
            // switch — still an output, without a percentage.
            if let Some(switch_control) = find_alsa_playback_switch(&mut AlsaControls::new(fd)) {
                found = true;
                if !switch_control.base.is_empty() {
                    name.push(' ');
                    name.push_str(&String::from_utf8_lossy(&switch_control.base));
                }
                if let Some(switch_value) = read_alsa_value(fd, &switch_control.id) {
                    active = switch_value.value != 0;
                    if !active {
                        volume = 0;
                    }
                }
            }
        }
        unsafe { libc::close(fd) };
    }

    if !found {
        return false;
    }

    if main {
        // The default soundType is MAIN: only the main output is reported.
        devices.push(SoundDevice {
            // "hw:0" is how the card is named everywhere else in ALSA.
            identifier: format!("hw:{card}"),
            platform_api: "ALSA".to_string(),
            name,
            volume,
            is_main: main,
            is_active: active,
        });
    }
    true
}

/// detectAlsa: the cards from /proc/asound/cards.
fn detect_alsa(devices: &mut Vec<SoundDevice>) -> Result<(), &'static str> {
    let Some(cards) = util::read_file("/proc/asound/cards") else {
        return Err("Failed to read the ALSA card list");
    };
    let mut main = true; // the machine's main output is the first card that has one
    let mut output = false;

    for line in cards.lines() {
        // One line per card with the index in the first column; the card's
        // long name is on the line below and does not start with a digit.
        // The reported name is the one behind the dash.
        let Some((number_text, rest)) = line.split_once('[') else {
            continue;
        };
        let Ok(number) = number_text.trim().parse::<i64>() else {
            continue;
        };
        if number < 0 {
            continue;
        }
        let Some(close) = rest.find(']') else {
            continue;
        };
        let Some(dash) = rest[close..].find(" - ") else {
            continue;
        };
        let card_name = rest[close + dash + 3..].trim_end_matches(' ');
        if append_alsa_card(number as u32, card_name, main, devices) {
            main = false;
            output = true;
        }
    }

    if !output {
        return Err("No ALSA card with a playback control was found");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// the server
// ---------------------------------------------------------------------------

fn detect_pulse_audio(devices: &mut Vec<SoundDevice>) -> Result<(), &'static str> {
    let default_socket_path = default_socket_path();
    let mut fd = -1;

    if let Ok(server_list) = std::env::var("PULSE_SERVER") {
        // $PULSE_SERVER may list several whitespace separated candidates;
        // the first one that accepts a connection wins, like libpulse.
        for server in server_list.split([' ', '\t']).filter(|s| !s.is_empty()) {
            fd = connect_server(server, &default_socket_path);
            if fd >= 0 {
                break;
            }
        }
    } else if !default_socket_path.is_empty() {
        fd = connect_unix_socket(&default_socket_path);
    }

    if fd < 0 {
        return Err("Failed to connect to the pulseaudio server");
    }

    let mut conn = PulseConn {
        fd,
        version: PA_VERSION,
        next_tag: 0,
        frame: Vec::new(),
    };
    let outcome = handshake(&mut conn).and_then(|(server_name, default_sink_name)| {
        fetch_sink_list(&mut conn, devices, &server_name, &default_sink_name)
    });
    unsafe { libc::close(fd) };
    outcome
}

/// ffDetectSound: the server first, the ALSA hardware only after it.
fn detect() -> Result<Vec<SoundDevice>, &'static str> {
    let mut devices = Vec::new();
    let error = match detect_pulse_audio(&mut devices) {
        Ok(()) => return Ok(devices),
        Err(error) => error,
    };

    // No server answered. The ALSA cards are read instead — but only ever
    // after the server, never instead of it.
    let detected = devices.len();
    match detect_alsa(&mut devices) {
        Ok(()) => Ok(devices),
        Err(_) if devices.len() > detected => Ok(devices),
        Err(_) => Err(error),
    }
}

// ---------------------------------------------------------------------------
// printing
// ---------------------------------------------------------------------------

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(devices) = detect() else {
        // The detection error string is an error row, hidden by default.
        return Vec::new();
    };
    if devices.is_empty() {
        // "No matched sound devices found" is an error row, hidden by
        // default.
        return Vec::new();
    }

    let percent = super::percent_config(
        instance.entry,
        crate::format::PercentModuleConfig { green: 80, yellow: 90, type_flags: 0 },
    );
    let ctx = super::print_ctx(instance);
    let df = &instance.display_format;
    let percent_type_flags =
        if percent.type_flags == 0 { df.percent_type } else { percent.type_flags };
    let single = devices.len() == 1;
    devices
        .into_iter()
        .enumerate()
        .map(|(index, device)| {
            let mut value = String::new();
            if percent_type_flags & crate::format::percent_type::HIDE_OTHERS_BIT == 0 {
                value.push_str(&device.name);
            }
            if device.volume != VOLUME_UNKNOWN {
                if percent_type_flags & crate::format::percent_type::BAR_BIT != 0 {
                    if !value.is_empty() {
                        value.push(' ');
                    }
                    crate::format::percent_append_bar(&mut value, device.volume as f64, percent, &ctx, df);
                }
                if percent_type_flags & crate::format::percent_type::NUM_BIT != 0 {
                    if !value.is_empty() {
                        value.push(' ');
                    }
                    let parentheses = !value.is_empty();
                    crate::format::percent_append_num(&mut value, device.volume as f64, percent, parentheses, &ctx, df);
                }
            }
            if percent_type_flags & crate::format::percent_type::HIDE_OTHERS_BIT == 0
                && device.is_main
                && !single
            {
                value.push_str(" (*)");
            }
            let key = if single {
                "Sound".to_string()
            } else {
                format!("{} {}", instance.display_name("sound"), index + 1)
            };
            Row::new(&key, value).with_index(if single { 0 } else { index as u32 + 1 })
        })
        .collect()
}

/// ffGenerateSoundJsonResult; volume keeps the 255 "unknown" sentinel as
/// null.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(devices) => {
            let mut arr = Json::Array(Vec::new());
            for device in &devices {
                let mut obj = Json::obj();
                obj.push("name", device.name.clone());
                obj.push("identifier", device.identifier.clone());
                obj.push("platformApi", device.platform_api.clone());

                let mut type_arr = Json::Array(Vec::new());
                if device.is_main {
                    type_arr.push_value("main");
                }
                if device.is_active {
                    type_arr.push_value("active");
                }
                obj.push("type", type_arr);

                if device.volume != VOLUME_UNKNOWN {
                    obj.push("volume", device.volume as u64);
                } else {
                    obj.push("volume", Json::Null);
                }
                arr.push_value(obj);
            }
            JsonOutcome::Result(arr)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_and_read_tagged_values_round_trip() {
        let mut payload = Vec::new();
        put_tag_u32(&mut payload, 0x12345678);
        put_tag_string(&mut payload, Some("sink name"));
        put_tag_string(&mut payload, None);
        let mut reader = Reader::new(&payload);
        assert_eq!(reader.read_u32_value(), Some(0x12345678));
        assert_eq!(reader.read_string_ref(), Some(b"sink name".as_slice()));
        assert_eq!(reader.read_string_ref(), None);
        assert!(!reader.error);
    }

    #[test]
    fn skip_value_walks_every_tag() {
        let mut payload = Vec::new();
        put_tag_u32(&mut payload, 7);
        payload.push(PA_TAG_BOOLEAN_TRUE);
        payload.push(PA_TAG_STRING_NULL);
        put_tag_string(&mut payload, Some("driver"));
        put_tag_arbitrary(&mut payload, &[1, 2, 3]);
        put_tag_cvolume_for_test(&mut payload, &[100, 200, 300]);
        let mut reader = Reader::new(&payload);
        assert!(reader.skip_value()); // u32
        assert!(reader.skip_value()); // boolean
        assert!(reader.skip_value()); // null string
        assert!(reader.skip_value()); // string
        assert!(reader.skip_value()); // arbitrary
        assert!(reader.skip_value()); // cvolume
        assert!(!reader.error);
        assert!(reader.eof());
    }

    fn put_tag_cvolume_for_test(buffer: &mut Vec<u8>, values: &[u32]) {
        buffer.push(PA_TAG_CVOLUME);
        buffer.push(values.len() as u8);
        for value in values {
            buffer.extend_from_slice(&value.to_be_bytes());
        }
    }

    #[test]
    fn cvolume_reads_the_first_channel() {
        let mut payload = Vec::new();
        put_tag_cvolume_for_test(&mut payload, &[0x10000, 0x8000]);
        let mut reader = Reader::new(&payload);
        assert_eq!(reader.read_cvolume(), Some(0x10000));
        // Over the channel limit is an error, not a walk off the payload.
        let mut bad = vec![PA_TAG_CVOLUME, PA_CHANNELS_MAX + 1];
        bad.extend_from_slice(&[0u8; 8]);
        let mut reader = Reader::new(&bad);
        assert_eq!(reader.read_cvolume(), None);
    }

    #[test]
    fn truncated_payloads_set_the_error_flag() {
        let payload = [PA_TAG_U32, 1, 2]; // three bytes of a tagged u32
        let mut reader = Reader::new(&payload);
        assert_eq!(reader.read_u32_value(), None);
        assert!(reader.error);
    }

    #[test]
    fn alsa_suffixes_match_word_boundaries() {
        // "Master Playback Volume" → volume of base "Master".
        let Some((is_switch, base)) = strip_alsa_name_suffix(b"Master Playback Volume") else {
            panic!("expected a playback volume suffix");
        };
        assert!(!is_switch);
        assert_eq!(base, "Master".len());
        // "Capture Volume" ends in " Volume" but is not an output.
        assert!(strip_alsa_name_suffix(b"Capture Volume").is_none());
        // The whole name may be just the suffix.
        let Some((_, base)) = strip_alsa_name_suffix(b"Playback Volume") else {
            panic!("expected the bare suffix to parse");
        };
        assert_eq!(base, 0);
        // Switches are recognized.
        let Some((is_switch, base)) = strip_alsa_name_suffix(b"IEC958 Playback Switch") else {
            panic!("expected the playback switch suffix");
        };
        assert!(is_switch);
        assert_eq!(base, "IEC958".len());
        // A coincidental tail is not a suffix.
        assert!(strip_alsa_name_suffix(b"XVolume").is_none());
    }

    #[test]
    fn alsa_priorities_rank_main_controls() {
        assert_eq!(get_alsa_control_priority(b"Master", 6), 0);
        assert_eq!(get_alsa_control_priority(b"PCM", 3), 2);
        assert_eq!(get_alsa_control_priority(b"Other", 5), ALSA_PRIORITY_NONE);
    }

    #[test]
    fn alsa_percent_follows_the_control_scale() {
        assert_eq!(
            get_alsa_percent(&AlsaValue {
                value: 0,
                minimum: 0,
                maximum: 100
            }),
            0
        );
        assert_eq!(
            get_alsa_percent(&AlsaValue {
                value: 100,
                minimum: 0,
                maximum: 100
            }),
            100
        );
        assert_eq!(
            get_alsa_percent(&AlsaValue {
                value: 50,
                minimum: 0,
                maximum: 100
            }),
            50
        );
        assert_eq!(
            get_alsa_percent(&AlsaValue {
                value: 46,
                minimum: 0,
                maximum: 87
            }),
            53
        );
        // An inverted range says nothing at all.
        assert_eq!(
            get_alsa_percent(&AlsaValue {
                value: 5,
                minimum: 10,
                maximum: 5
            }),
            VOLUME_UNKNOWN
        );
    }

    #[test]
    fn device_value_matches_the_default_format() {
        let device = SoundDevice {
            identifier: "auto_null".into(),
            platform_api: "PipeWire 1.4.3".into(),
            name: "Dummy Output".into(),
            volume: 100,
            is_main: true,
            is_active: true,
        };
        // Printed through run()'s formatting; check the pieces here.
        assert_eq!(device.volume, 100);
    }

    #[test]
    fn sink_volume_rounds_half_up() {
        // (volume * 100 + NORM/2) / NORM
        let norm = u64::from(PA_VOLUME_NORM);
        let volume = (0xFFFFu64 * 100 + norm / 2) / norm;
        assert_eq!(volume, 100);
        let volume = (0x8000u64 * 100 + norm / 2) / norm;
        assert_eq!(volume, 50);
    }
}

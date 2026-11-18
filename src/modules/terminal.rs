//! `terminal` module: terminal emulator name and version, ported from
//! detection/terminalshell/terminalshell.c, terminalshell_linux.c and
//! modules/terminal/terminal.c. The process tree is walked from our own
//! pid skipping shells and wrappers — the union of the upstream shell and
//! terminal skip lists, since upstream starts at the detected shell's
//! parent; environment fallbacks (SSH_TTY, KITTY_*, Windows Terminal,
//! ConEmu, Alacritty, konsole, gnome-terminal, TERM_PROGRAM,
//! LC_TERMINAL, TERM/ttyname) apply when the walk fails or lands on a
//! login/init process. Version detection dispatches per-terminal getters
//! (exe spawns, ELF .rodata scans, escape-sequence queries) with
//! TERM_PROGRAM_VERSION/LC_TERMINAL_VERSION as the generic fallback; the
//! value is the pretty name plus the version when one is found.

use std::ffi::CStr;

use crate::{instance::Instance, modules::Row, util};

/// Shells and wrappers skipped while walking up the process tree — the
/// upstream terminal walk list (getTerminalInfo) plus the shell walk's
/// extras (getShellInfo) and our own process, since this walk starts at
/// our own pid.
const SKIPPED: &[&str] = &[
    // shells (getTerminalInfo)
    "sh",
    "ash",
    "bash",
    "zsh",
    "ksh",
    "ksh93",
    "mksh",
    "pdksh",
    "oksh",
    "csh",
    "tcsh",
    "fish",
    "dash",
    "pwsh",
    "nu",
    "git-shell",
    "elvish",
    "oil.ovm",
    "xonsh",
    // wrappers between us and the terminal (both walks)
    "sudo",
    "su",
    "login",
    "clifm",
    "chezmoi",
    "proot",
    "script",
    "run-parts",
    // shell-walk extras (getShellInfo)
    "strace",
    "gdb",
    "lldb",
    "lldb-mi",
    "ltrace",
    "perf",
    "guake-wrapped",
    "time",
    "valgrind",
    "fastfetch",
    "flashfetch",
    "mise",
    "mise-shim",
    "atuin",
    // our own process, plus common invokers
    "rustfetch",
    "nohup",
];

fn is_skipped(name: &str) -> bool {
    SKIPPED.contains(&name)
        || name.starts_with("Relay(") // Unknown process in WSL2
        || name.starts_with("flatpak-")
        || name.ends_with(".sh")
}

/// (comm, ppid) from /proc/<pid>/stat; comm may contain parens and spaces,
/// so it is cut between the first '(' and the last ')'.
fn process_basic_info(pid: u32) -> Option<(String, u32)> {
    let stat = util::read_file(&format!("/proc/{pid}/stat"))?;
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = &stat[open + 1..close];
    // after the comm: " S <ppid> ..." — state then ppid
    let ppid = stat[close + 2..].split_whitespace().nth(1)?.parse().ok()?;
    Some((comm.to_string(), ppid))
}

/// access(path, X_OK).
fn x_ok(path: &str) -> bool {
    match std::ffi::CString::new(path) {
        Ok(c) => unsafe { libc::access(c.as_ptr(), libc::X_OK) == 0 },
        Err(_) => false,
    }
}

/// ffProcessGetInfoLinux: exe from the cmdline's arg0 (for interpreters,
/// the first non-option argument), minus login-shell dashes; exeName is
/// the basename; exePath is /proc/<pid>/exe while the binary still exists
/// (the literal /proc path otherwise, when accessible).
fn process_exe_info(pid: u32, comm: &str) -> (String, String, String) {
    let mut exe = String::new();
    if let Some(cmdline) = util::read_file(&format!("/proc/{pid}/cmdline")) {
        let args: Vec<&str> = cmdline.split('\0').collect();
        if let Some(&first) = args.first().filter(|arg| !arg.is_empty()) {
            exe = first.to_string();
            if args.len() > 2 {
                let name = exe.rsplit('/').next().unwrap_or(&exe);
                if name.starts_with("python") || name == "guile" {
                    for arg in &args[1..args.len() - 1] {
                        if arg.is_empty() {
                            break;
                        }
                        if arg.starts_with('-') {
                            continue;
                        }
                        exe = (*arg).to_string();
                        break;
                    }
                }
            }
            exe = exe.trim_start_matches('-').to_string();
        }
    }
    if exe.is_empty() {
        exe = comm.to_string();
    }
    let exe_name = exe.rsplit('/').next().unwrap_or(&exe).to_string();

    let mut exe_path = String::new();
    let link = format!("/proc/{pid}/exe");
    if let Some(target) = std::fs::read_link(&link).ok().filter(|target| target.exists()) {
        exe_path = target.to_string_lossy().into_owned();
    }
    if exe_path.is_empty() && x_ok(&link) {
        exe_path = link;
    }
    (exe, exe_name, exe_path)
}

/// The walked terminal process: comm and its pid, from our own pid upward.
fn walk() -> Option<(String, u32)> {
    let mut pid = std::process::id();
    for _ in 0..32 {
        if pid <= 1 {
            return None;
        }
        let (comm, ppid) = process_basic_info(pid)?;
        if is_skipped(&comm) {
            pid = ppid;
            continue;
        }
        return Some((comm, pid));
    }
    None
}

fn basename(exe: &str) -> String {
    exe.rsplit('/').next().unwrap_or(exe).to_string()
}

fn starts_ign(name: &str, prefix: &str) -> bool {
    name.len() >= prefix.len()
        && name.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

fn equals_ign(name: &str, other: &str) -> bool {
    name.eq_ignore_ascii_case(other)
}

// ---- ffStrbuf substring helpers ----

/// ffStrbufSubstrAfterFirstS: the part after `marker`, None when absent.
fn substr_after_first_s<'a>(s: &'a str, marker: &str) -> Option<&'a str> {
    s.find(marker).map(|i| &s[i + marker.len()..])
}

/// ffStrbufSubstrAfterFirstC: the part after the first `c`, None when absent.
fn substr_after_first_c(s: &str, c: char) -> Option<&str> {
    s.find(c).map(|i| &s[i + c.len_utf8()..])
}

/// ffStrbufSubstrAfterLastC: the part after the last `c`, None when absent.
fn substr_after_last_c(s: &str, c: char) -> Option<&str> {
    s.rfind(c).map(|i| &s[i + c.len_utf8()..])
}

/// ffStrbufSubstrBeforeFirstC: cut at the first `c`; unchanged when absent.
fn substr_before_first_c(s: &str, c: char) -> &str {
    match s.find(c) {
        Some(i) => &s[..i],
        None => s,
    }
}

/// ffStrbufSubstrBeforeLastC: cut at the last `c`; unchanged when absent.
fn substr_before_last_c(s: &str, c: char) -> &str {
    match s.rfind(c) {
        Some(i) => &s[..i],
        None => s,
    }
}

// ---- process spawning (ffProcessAppendStdOut/-StdErr) ----

/// Spawn and capture one stream; a nonzero exit is fine, exit 127 (command
/// not found) and abnormal exits are errors, like ffProcessReadOutput.
fn process_capture(argv: &[&str], stderr: bool) -> Option<String> {
    let mut command = std::process::Command::new(argv[0]);
    command.args(&argv[1..]).env("LANG", "C.UTF-8");
    let output = command.output().ok()?;
    match output.status.code() {
        Some(127) | None => None,
        Some(_) => {
            let stream = if stderr {
                &output.stderr
            } else {
                &output.stdout
            };
            Some(String::from_utf8_lossy(stream).into_owned())
        },
    }
}

/// getExeVersionRaw: `exe --version` stdout, non-empty.
fn exe_version_raw(exe: &str) -> Option<String> {
    let out = process_capture(&[exe, "--version"], false)?;
    (!out.is_empty()).then_some(out)
}

/// getExeVersionGeneral: the second space-separated token when present,
/// the whole output otherwise (both substring calls ignore absence).
fn exe_version_general(exe: &str) -> Option<String> {
    let out = exe_version_raw(exe)?;
    let out = substr_after_first_c(&out, ' ').unwrap_or(&out);
    let out = substr_before_first_c(out, ' ');
    (!out.is_empty()).then(|| out.to_string())
}

// ---- ELF .rodata string extraction (ffBinaryExtractStrings) ----

fn read_u16(data: &[u8], at: usize, le: bool) -> Option<u16> {
    let raw = data.get(at..at + 2)?;
    Some(if le {
        u16::from_le_bytes(raw.try_into().ok()?)
    } else {
        u16::from_be_bytes(raw.try_into().ok()?)
    })
}

fn read_u32(data: &[u8], at: usize, le: bool) -> Option<u32> {
    let raw = data.get(at..at + 4)?;
    Some(if le {
        u32::from_le_bytes(raw.try_into().ok()?)
    } else {
        u32::from_be_bytes(raw.try_into().ok()?)
    })
}

fn read_u64(data: &[u8], at: usize, le: bool) -> Option<u64> {
    let raw = data.get(at..at + 8)?;
    Some(if le {
        u64::from_le_bytes(raw.try_into().ok()?)
    } else {
        u64::from_be_bytes(raw.try_into().ok()?)
    })
}

/// Scan one .rodata section for NUL-terminated strings (length >= min_len,
/// printable first byte) and stop at the first the matcher accepts.
fn scan_rodata(section: &[u8], min_len: usize, matcher: &mut dyn FnMut(&str) -> bool) -> bool {
    let mut offset = 0;
    while offset < section.len() {
        if section[offset] == 0 {
            offset += 1;
            continue;
        }
        let len = section[offset..].iter().position(|&b| b == 0).unwrap_or(section.len() - offset);
        if len >= min_len && (b' '..=b'~').contains(&section[offset]) {
            let s = String::from_utf8_lossy(&section[offset..offset + len]);
            if !matcher(&s) {
                return true;
            }
        }
        offset += len + 1;
    }
    false
}

/// ffBinaryExtractStrings: scan the .rodata section of an ELF file. None
/// means the file or its section headers could not be parsed; either way
/// the caller only checks what the matcher captured.
fn binary_extract_strings(
    path: &str,
    min_len: usize,
    matcher: &mut dyn FnMut(&str) -> bool,
) -> Option<()> {
    let Ok(data) = std::fs::read(path) else {
        return None;
    };
    if !data.starts_with(b"\x7fELF") {
        return None;
    }
    let le = data.get(5) == Some(&1);
    let is64 = data.get(4) == Some(&2);
    let (shoff, shentsize, shnum, shstrndx) = if is64 {
        (
            read_u64(&data, 0x28, le)? as usize,
            read_u16(&data, 0x3A, le)? as usize,
            read_u16(&data, 0x3C, le)? as usize,
            read_u16(&data, 0x3E, le)? as usize,
        )
    } else {
        (
            read_u32(&data, 0x20, le)? as usize,
            read_u16(&data, 0x2E, le)? as usize,
            read_u16(&data, 0x30, le)? as usize,
            read_u16(&data, 0x32, le)? as usize,
        )
    };
    if shoff == 0 || shnum == 0 || shstrndx >= shnum {
        return None;
    }
    let section = |index: usize| -> Option<(&[u8], usize, usize)> {
        let base = shoff + index * shentsize;
        let (name, offset, size) = if is64 {
            (
                read_u32(&data, base, le)? as usize,
                read_u64(&data, base + 0x18, le)? as usize,
                read_u64(&data, base + 0x20, le)? as usize,
            )
        } else {
            (
                read_u32(&data, base, le)? as usize,
                read_u32(&data, base + 0x10, le)? as usize,
                read_u32(&data, base + 0x14, le)? as usize,
            )
        };
        Some((&data[offset..offset.checked_add(size)?], name, offset))
    };

    // The section header string table.
    let (_, _, strtab_offset) = section(shstrndx)?;
    let strtab = &data[strtab_offset..];
    let section_name = |name: usize| -> Option<&str> {
        let end = strtab[name..].iter().position(|&b| b == 0)? + name;
        std::str::from_utf8(&strtab[name..end]).ok()
    };

    for index in 0..shnum {
        let (section_data, name, _) = section(index)?;
        if section_name(name)? != ".rodata" {
            continue;
        }
        if scan_rodata(section_data, min_len, matcher) {
            return Some(());
        }
        return None; // only the first .rodata section, like upstream
    }
    None
}

// ---- scanf-style helpers for the escape responses ----

/// One sscanf %d: skip whitespace, optional sign, at least one digit.
fn scanf_int(data: &[u8], pos: &mut usize) -> bool {
    while *pos < data.len() && data[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
    if *pos < data.len() && (data[*pos] == b'+' || data[*pos] == b'-') {
        *pos += 1;
    }
    let start = *pos;
    while *pos < data.len() && data[*pos].is_ascii_digit() {
        *pos += 1;
    }
    *pos > start
}

/// One sscanf %u with a width cap: skip whitespace, then at most `width`
/// digits (scanf never wraps mid-width, so do not let the value overflow).
fn scanf_uint_capped(data: &[u8], pos: &mut usize, width: usize) -> Option<u32> {
    while *pos < data.len() && data[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
    let start = *pos;
    let mut value: u32 = 0;
    while *pos < data.len() && *pos - start < width && data[*pos].is_ascii_digit() {
        value = value.checked_mul(10)?.checked_add((data[*pos] - b'0') as u32)?;
        *pos += 1;
    }
    (*pos > start).then_some(value)
}

/// "%*d.%*d.%*d" — three dot-separated numbers, trailing junk allowed.
fn match_dotted_triple(s: &str) -> bool {
    let data = s.as_bytes();
    let mut pos = 0;
    for i in 0..3 {
        if !scanf_int(data, &mut pos) {
            return false;
        }
        if i < 2 {
            if data.get(pos) != Some(&b'.') {
                return false;
            }
            pos += 1;
        }
    }
    true
}

/// extractGeneralVersion: the whole string when it starts with a digit and
/// contains three dot-separated numbers.
fn general_version_string(s: &str) -> Option<String> {
    if !s.as_bytes().first().is_some_and(|b| b.is_ascii_digit()) {
        return None;
    }
    match_dotted_triple(s).then(|| s.to_string())
}

/// extractKmsconVersion: exactly the "vD.D.D" head of a "v"-string.
fn kmscon_version_string(s: &str) -> Option<String> {
    if !s.starts_with('v') {
        return None;
    }
    let data = s.as_bytes();
    let mut pos = 1;
    for i in 0..3 {
        if !scanf_int(data, &mut pos) {
            return None;
        }
        if i < 2 {
            if data.get(pos) != Some(&b'.') {
                return None;
            }
            pos += 1;
        }
    }
    // count includes the leading 'v'; (count - 1) bytes follow it.
    Some(s[1..pos].to_string())
}

/// extractSshdVersion: "OpenSSH_" + digit + "D.DpD" — whole remainder.
fn sshd_version_string(s: &str) -> Option<String> {
    let rest = s.strip_prefix("OpenSSH_")?;
    if !rest.as_bytes().first().is_some_and(|b| b.is_ascii_digit()) {
        return None;
    }
    let data = rest.as_bytes();
    let mut pos = 0;
    if !scanf_int(data, &mut pos) || data.get(pos) != Some(&b'.') {
        return None;
    }
    pos += 1;
    if !scanf_int(data, &mut pos) || data.get(pos) != Some(&b'p') {
        return None;
    }
    pos += 1;
    if !scanf_int(data, &mut pos) {
        return None;
    }
    Some(rest.to_string())
}

/// kitty constants.py: "version: Version = Version(M, m, p)".
fn kitty_constants_version(data: &[u8]) -> Option<String> {
    const MARKER: &[u8] = b"version: Version = Version(";
    let marker_end = data.windows(MARKER.len()).position(|w| w == MARKER)? + MARKER.len();
    let mut pos = marker_end;
    let mut numbers = [0i64; 3];
    for (i, number) in numbers.iter_mut().enumerate() {
        if i > 0 {
            if data.get(pos) != Some(&b',') {
                return None;
            }
            pos += 1;
        }
        let number_start = pos;
        if !scanf_int(data, &mut pos) {
            return None;
        }
        let text = std::str::from_utf8(&data[number_start..pos]).ok()?;
        *number = text.trim().parse().ok()?;
    }
    Some(format!("{}.{}.{}", numbers[0], numbers[1], numbers[2]))
}

// ---- terminal escape responses (ffGetTerminalResponse) ----

/// foot's DA1 response: "\e[>1;" + three %2u fields (trailing literal
/// mismatches still count as assigned, like vsscanf's return value).
fn parse_da1(buf: &[u8]) -> (usize, Option<(u32, u32, u32)>) {
    const PREFIX: &[u8] = b"\x1b[>1;";
    let Some(rest) = buf.strip_prefix(PREFIX) else {
        return (0, None);
    };
    let mut pos = 0;
    let mut values = [0u32; 3];
    for (i, value) in values.iter_mut().enumerate() {
        match scanf_uint_capped(rest, &mut pos, 2) {
            Some(number) => *value = number,
            None => return (i, None),
        }
    }
    (3, Some((values[0], values[1], values[2])))
}

/// kitty's XTGETTCAP reply: "\eP1+r" + anything up to '=' + up to 63
/// non-ESC payload chars (the trailing "\e\\" is not required; vsscanf
/// counts the one assigned conversion).
fn parse_kitty_response(buf: &[u8]) -> (usize, Option<String>) {
    const PREFIX: &[u8] = b"\x1bP1+r";
    let Some(rest) = buf.strip_prefix(PREFIX) else {
        return (0, None);
    };
    let Some(equals) = rest.iter().position(|&b| b == b'=') else {
        return (0, None);
    };
    let payload_start = equals + 1;
    let end = rest[payload_start..]
        .iter()
        .position(|&b| b == 0x1b)
        .map_or(rest.len(), |i| payload_start + i)
        .min(payload_start + 63);
    if end == payload_start {
        return (0, None);
    }
    (1, Some(hex_decode(&rest[payload_start..end])))
}

/// The kitty hex payload, decoded pair by pair like the upstream loop.
fn hex_decode(payload: &[u8]) -> String {
    let mut out = Vec::with_capacity(payload.len() / 2);
    for pair in payload.as_chunks::<2>().0 {
        let text = std::str::from_utf8(pair).unwrap_or_default();
        if let Ok(value) = u8::from_str_radix(text, 16) {
            out.push(value);
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// ffGetTerminalResponse on /dev/tty: raw mode, write the request, poll
/// (1s) and read chunks; the parser reports the assigned-parameter count,
/// so 0 fails immediately and a partial count keeps polling, like the
/// vsscanf loop.
/// The per-chunk parser: (assigned parameters, value when complete).
type ResponseParser<'a, T> = &'a mut dyn FnMut(&[u8]) -> (usize, Option<T>);

fn terminal_response<T>(
    request: &[u8],
    n_params: usize,
    parse: ResponseParser<'_, T>,
) -> Option<T> {
    let fd = unsafe {
        libc::open(c"/dev/tty".as_ptr(), libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC)
    };
    if fd < 0 {
        return None;
    }
    let mut result = None;
    unsafe {
        let mut old_term: libc::termios = std::mem::zeroed();
        'query: {
            if libc::tcgetattr(fd, &mut old_term) != 0 {
                break 'query;
            }
            let mut new_term = old_term;
            new_term.c_lflag &= !(libc::ICANON | libc::ECHO);
            if libc::tcsetattr(fd, libc::TCSAFLUSH, &new_term) != 0 {
                break 'query;
            }

            let mut written = 0usize;
            while written < request.len() {
                let n =
                    libc::write(fd, request[written..].as_ptr().cast(), request.len() - written);
                if n <= 0 {
                    break 'query;
                }
                written += n as usize;
            }

            let mut buffer: Vec<u8> = Vec::new();
            loop {
                let mut poll_fd = libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                if libc::poll(&mut poll_fd, 1, 1000) <= 0 {
                    break 'query;
                }
                // The 1023-byte cap matches the upstream fixed buffer; a
                // full buffer makes read() ask for zero bytes and fail.
                if buffer.len() >= 1023 {
                    break 'query;
                }
                let mut chunk = [0u8; 1024];
                let n = libc::read(
                    fd,
                    chunk.as_mut_ptr().cast(),
                    (1023 - buffer.len()).min(chunk.len()),
                );
                if n <= 0 {
                    break 'query;
                }
                buffer.extend_from_slice(&chunk[..n as usize]);

                let (matched, value) = parse(&buffer);
                if matched == 0 {
                    break 'query;
                }
                if matched >= n_params {
                    result = value;
                    break 'query;
                }
            }
        }
        libc::tcsetattr(fd, libc::TCSAFLUSH, &old_term);
        libc::close(fd);
    }
    result
}

// ---- per-terminal version getters ----

fn version_gnome(exe: &str) -> Option<String> {
    if exe.starts_with('/') {
        let mut found = None;
        binary_extract_strings(exe, "0.0.0".len(), &mut |s| {
            if let Some(v) = general_version_string(s) {
                found = Some(v);
                false
            } else {
                true
            }
        });
        if found.is_some() {
            return found;
        }
    }
    // GNOME Terminal 3.46.7 using VTE 0.70.2 +BIDI +GNUTLS +ICU +SYSTEMD
    let out = process_capture(&["gnome-terminal", "--version"], false)?;
    let out = substr_after_first_s(&out, "Terminal ")?;
    let out = substr_before_first_c(out, ' ');
    (!out.is_empty()).then(|| out.to_string())
}

/// konsole's KONSOLE_VERSION: YYYYMMDD → major.minor.patch.
fn konsole_env_version(value: &str) -> Option<String> {
    let value = value.trim_start();
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut major: i64 = value.parse().ok()?;
    if major < 0 {
        return None;
    }
    let patch = major % 100;
    major /= 100;
    let minor = major % 100;
    major /= 100;
    Some(format!("{major}.{minor}.{patch}"))
}

fn version_konsole(exe: &str) -> Option<String> {
    if let Some(version) =
        std::env::var("KONSOLE_VERSION").ok().and_then(|v| konsole_env_version(&v))
    {
        return Some(version);
    }
    // Likely TDE konsole. See #2319
    let out = exe_version_raw(exe)?;
    let out = substr_after_last_c(&out, ' ')?;
    (!out.is_empty()).then(|| out.to_string())
}

fn version_xfce4(exe: &str) -> Option<String> {
    if exe.starts_with('/') {
        let mut found = None;
        binary_extract_strings(exe, "0.0.0".len(), &mut |s| {
            if let Some(v) = general_version_string(s) {
                found = Some(v);
                false
            } else {
                true
            }
        });
        if found.is_some() {
            return found;
        }
    }
    // xfce4-terminal 1.0.4 (Xfce 4.18)...
    exe_version_general(exe)
}

fn version_foot(exe: &str) -> Option<String> {
    if let Some((major, minor, patch)) = terminal_response(b"\x1b[>c", 3, &mut parse_da1) {
        return Some(format!("{major}.{minor}.{patch}"));
    }
    // foot version: 1.13.1 -pgo +ime -graphemes -assertions
    let out = exe_version_raw(exe)?;
    let out = substr_after_first_s(&out, "version: ")?;
    let out = substr_before_first_c(out, ' ');
    (!out.is_empty()).then(|| out.to_string())
}

fn version_mate(exe: &str) -> Option<String> {
    let mut found = None;
    binary_extract_strings(exe, "0.0.0".len(), &mut |s| {
        if let Some(v) = general_version_string(s) {
            found = Some(v);
            false
        } else {
            true
        }
    });
    if found.is_some() {
        return found;
    }
    // MATE Terminal 1.26.1
    let out = exe_version_raw(exe)?;
    let out = substr_after_last_c(&out, ' ')?;
    (!out.is_empty()).then(|| out.to_string())
}

fn version_cockpit(exe: &str) -> Option<String> {
    // Version: 295\n...
    let out = exe_version_raw(exe)?;
    let out = substr_before_first_c(&out, '\n');
    let out = substr_after_first_c(out, ' ')?;
    (!out.is_empty()).then(|| out.to_string())
}

fn version_xterm(exe: &str) -> Option<String> {
    let out = match std::env::var("XTERM_VERSION") {
        Ok(v) if !v.is_empty() => v,
        _ => process_capture(&[exe, "-v"], false)?,
    };
    // xterm(273)
    let out = out.trim_end_matches(')');
    let out = substr_after_first_c(out, '(')?;
    (!out.is_empty()).then(|| out.to_string())
}

fn version_blackbox(exe: &str) -> Option<String> {
    // BlackBox version 0.14.0 (flatpak)
    let out = process_capture(&[exe, "--version"], false)?;
    let out = substr_after_first_s(&out, "version ")?;
    let out = substr_before_first_c(out, ' ');
    (!out.is_empty()).then(|| out.to_string())
}

fn version_urxvt() -> Option<String> {
    // Don't use exe because of urxvtd
    let out = process_capture(&["urxvt", "-invalid"], true)?;
    // rxvt-unicode (urxvt) v9.31 - released: 2023-01-02
    let out = substr_after_first_s(&out, "(urxvt) v")?;
    let out = substr_before_first_c(out, ' ');
    (!out.is_empty()).then(|| out.to_string())
}

fn version_st(exe: &str) -> Option<String> {
    // st 0.9
    let out = process_capture(&[exe, "-v"], true)?;
    let out = substr_after_first_c(&out, ' ')?;
    (!out.is_empty()).then(|| out.to_string())
}

fn version_lxterminal(exe: &str) -> Option<String> {
    // lxterminal 0.3.2
    let out = exe_version_raw(exe)?;
    let out = substr_after_first_c(&out, ' ')?;
    (!out.is_empty()).then(|| out.to_string())
}

fn version_weston() -> Option<String> {
    // weston-terminal doesn't report a version, use weston instead
    let out = process_capture(&["weston", "--version"], false)?;
    // weston 8.0.0
    let out = substr_after_first_c(&out, ' ')?;
    (!out.is_empty()).then(|| out.to_string())
}

fn version_kmscon(exe: &str) -> Option<String> {
    if exe.starts_with('/') {
        let mut found = None;
        binary_extract_strings(exe, "v0.0.0".len(), &mut |s| {
            if let Some(v) = kmscon_version_string(s) {
                found = Some(v);
                false
            } else {
                true
            }
        });
        if found.is_some() {
            return found;
        }
    }
    // kmscon version v10.0.0
    let out = exe_version_raw(exe)?;
    let out = substr_after_last_c(&out, ' ')?;
    (!out.is_empty()).then(|| out.to_string())
}

fn version_contour(exe: &str) -> Option<String> {
    if let Some(v) = std::env::var("TERMINAL_VERSION_STRING").ok().filter(|v| !v.is_empty()) {
        return Some(v);
    }
    // Contour Terminal Emulator 0.3.12.262
    let out = exe_version_raw(exe)?;
    let out = substr_after_last_c(&out, ' ')?;
    (!out.is_empty()).then(|| out.to_string())
}

fn version_screen(exe: &str) -> Option<String> {
    // Screen version 4.09.01 (GNU) 20-Aug-23
    let out = exe_version_raw(exe)?;
    let out = substr_after_first_s(&out, "Screen version ")?;
    let out = substr_before_first_c(out, ' ');
    (!out.is_empty()).then(|| out.to_string())
}

fn version_tmux(exe: &str) -> Option<String> {
    // tmux 3.4
    let out = process_capture(&[exe, "-V"], false)?;
    let out = substr_after_first_c(&out, ' ')?;
    (!out.is_empty()).then(|| out.to_string())
}

fn version_zellij(exe: &str) -> Option<String> {
    // zellij 0.39.2
    let out = exe_version_raw(exe)?;
    let out = substr_after_first_c(&out, ' ')?;
    (!out.is_empty()).then(|| out.to_string())
}

fn version_zed(exe: &str) -> Option<String> {
    let dir = match exe.rfind('/') {
        Some(i) => &exe[..i],
        None => exe,
    };
    let cli = format!("{dir}/cli");
    let out = process_capture(&[cli.as_str(), "--version"], false)?;
    // Zed 0.142.6 – /Applications/Zed.app
    let out = out.strip_prefix("Zed ")?;
    let out = substr_before_first_c(out, ' ');
    (!out.is_empty()).then(|| out.to_string())
}

/// ffFindExecutableInPath: the first PATH entry with an executable `name`.
fn find_in_path(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if x_ok(&candidate.to_string_lossy()) {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

fn version_sshd(exe: &str) -> Option<String> {
    let exe_path = if exe.starts_with('/') {
        exe.to_string()
    } else {
        find_in_path("sshd")?
    };

    let mut found = None;
    binary_extract_strings(&exe_path, "OpenSSH0.0".len(), &mut |s| {
        if let Some(v) = sshd_version_string(s) {
            found = Some(v);
            false
        } else {
            true
        }
    });
    if found.is_some() {
        return found;
    }

    let out = process_capture(&[exe_path.as_str(), "-V"], true)?;
    let out = match out.strip_prefix("unknown ") {
        // `unknown option -- V` (ancient OpenSSH versions)
        Some(rest) => substr_after_first_c(rest, '\n')?,
        None => out.as_str(),
    };
    let rest = out.strip_prefix("OpenSSH_")?;
    // OpenSSH_10.0p2 Ubuntu-5ubuntu5, OpenSSL 3.5.3 16 Sep 2025
    let rest = substr_before_first_c(rest, ',');
    (!rest.is_empty()).then(|| rest.to_string())
}

fn version_kitty(exe: &str) -> Option<String> {
    // Starts from version 0.17.0; only the first 1023 bytes are scanned,
    // like the upstream fixed buffer.
    for path in [
        "/usr/lib64/kitty/kitty/constants.py",
        "/usr/lib/kitty/kitty/constants.py",
    ] {
        if let Ok(data) = std::fs::read(path) {
            if let Some(version) = kitty_constants_version(&data[..data.len().min(1023)]) {
                return Some(version);
            }
            break;
        }
    }
    // kitty-query-version (discussion #1030)
    if let Some(hex) = terminal_response(
        b"\x1bP+q6b697474792d71756572792d76657273696f6e\x1b\\",
        1,
        &mut parse_kitty_response,
    )
    .filter(|hex| !hex.is_empty())
    {
        return Some(hex);
    }
    // kitty 0.21.2 created by Kovid Goyal
    exe_version_general(exe)
}

fn version_ptyxis() -> Option<String> {
    let out = process_capture(&["ptyxis", "--version"], false)?;
    let out = substr_before_first_c(&out, '\n');
    let out = substr_after_first_c(out, ' ')?;
    (!out.is_empty()).then(|| out.to_string())
}

fn version_tilix(exe: &str) -> Option<String> {
    if exe.starts_with('/') {
        let mut found = None;
        binary_extract_strings(exe, "0.0.0".len(), &mut |s| {
            if let Some(v) = general_version_string(s) {
                found = Some(v);
                false
            } else {
                true
            }
        });
        if found.is_some() {
            return found;
        }
    }
    let out = process_capture(&[exe, "--version"], false)?;
    let index = out.find("Tilix version: ")?;
    // Upstream advances by "Tilix version:" (no space) and then keeps
    // everything AFTER that position — the space is skipped there.
    let index = index + "Tilix version:".len();
    let end = out[index..].find('\n').map_or(out.len(), |i| index + i);
    let out = &out[index + 1..end];
    (!out.is_empty()).then(|| out.to_string())
}

fn version_sakura(exe: &str) -> Option<String> {
    // sakura version is 3.8.8
    let out = process_capture(&[exe, "--version"], true)?;
    let out = substr_after_last_c(&out, ' ')?;
    (!out.is_empty()).then(|| out.to_string())
}

fn version_termite(exe: &str) -> Option<String> {
    // termite v16.9\nvte 0.78.1 +BIDI +GNUTLS +ICU +SYSTEMD
    let out = process_capture(&[exe, "--version"], false)?;
    let out = substr_before_first_c(&out, '\n');
    let out = substr_after_last_c(out, 'v')?;
    (!out.is_empty()).then(|| out.to_string())
}

// ---- the version dispatcher (fftsGetTerminalVersion) ----

fn env_version_fallback(process_name: &str) -> Option<String> {
    let term_program_version =
        std::env::var("TERM_PROGRAM_VERSION").ok().filter(|v| !v.is_empty())?;
    if let Some(term_program) = std::env::var_os("TERM_PROGRAM") {
        let term_program = term_program.to_string_lossy();
        if starts_ign(process_name, &term_program) // processName ends with `.exe` on Windows
            || (term_program == "vscode" && starts_ign(process_name, "code"))
            || (term_program == "WarpTerminal" && process_name == "warp")
        {
            return Some(term_program_version);
        }
    }
    None
}

fn lc_version_fallback(process_name: &str) -> Option<String> {
    let term_program_version =
        std::env::var("LC_TERMINAL_VERSION").ok().filter(|v| !v.is_empty())?;
    if let Some(term_program) = std::env::var_os("LC_TERMINAL") {
        let term_program = term_program.to_string_lossy();
        if starts_ign(process_name, &term_program)
            || (term_program == "vscode" && starts_ign(process_name, "code"))
            || (term_program.starts_with("iTerm") && process_name.starts_with("iTermServer-"))
        {
            return Some(term_program_version);
        }
    }
    None
}

fn get_terminal_version(process_name: &str, exe: &str) -> String {
    if starts_ign(process_name, "gnome-terminal") {
        return version_gnome(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "konsole") || equals_ign(process_name, "yakuake") {
        // yakuake shares code with konsole
        return version_konsole(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "xfce4-terminal") {
        return version_xfce4(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "terminator") || equals_ign(process_name, "deepin-terminal") {
        return exe_version_general(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "foot") {
        return version_foot(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "qterminal") {
        return exe_version_raw(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "mate-terminal") {
        return version_mate(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "cockpit-bridge") {
        return version_cockpit(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "xterm") {
        return version_xterm(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "blackbox") {
        return version_blackbox(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "st") {
        return version_st(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "lxterminal") {
        return version_lxterminal(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "weston-terminal") {
        return version_weston().unwrap_or_default();
    }
    if equals_ign(process_name, "urxvt")
        || equals_ign(process_name, "urxvtd")
        || equals_ign(process_name, "rxvt")
        || equals_ign(process_name, "rxvt-unicode")
    {
        return version_urxvt().unwrap_or_default();
    }
    if equals_ign(process_name, "ptyxis-agent") {
        return version_ptyxis().unwrap_or_default();
    }
    if equals_ign(process_name, "tilix") {
        return version_tilix(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "sakura") {
        return version_sakura(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "termite") {
        return version_termite(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "cosmic-term") {
        return version_tmux(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "kmscon") {
        return version_kmscon(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "kitty") {
        return version_kitty(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "Tabby")
        && let Some(version) = exe_version_raw(exe)
    {
        return version;
    }
    if starts_ign(process_name, "alacritty") {
        return exe_version_general(exe).unwrap_or_default();
    }
    if starts_ign(process_name, "contour") {
        return version_contour(exe).unwrap_or_default();
    }
    if starts_ign(process_name, "screen") {
        return version_screen(exe).unwrap_or_default();
    }
    if starts_ign(process_name, "zellij") {
        return version_zellij(exe).unwrap_or_default();
    }
    if starts_ign(process_name, "zed") {
        return version_zed(exe).unwrap_or_default();
    }
    if let Some(version) =
        env_version_fallback(process_name).or_else(|| lc_version_fallback(process_name))
    {
        return version;
    }
    if starts_ign(process_name, "tmux") {
        return version_tmux(exe).unwrap_or_default();
    }
    if equals_ign(process_name, "sshd") || starts_ign(process_name, "sshd-") {
        return version_sshd(exe).unwrap_or_default();
    }
    String::new()
}

// ---- detection assembly ----

/// ttyname of stdout, for the sshd pretty name.
fn tty_name() -> String {
    unsafe {
        let ptr = libc::ttyname(1);
        if ptr.is_null() {
            return String::new();
        }
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

/// getTerminalFromEnv: the env fallbacks when the walk failed or landed on
/// a login/init process. Returns (processName, exe, exeName, exePath).
fn terminal_from_env() -> Option<(String, String, String, String)> {
    let term: String;
    if let Some(ssh_tty) = std::env::var_os("SSH_TTY") {
        term = ssh_tty.to_string_lossy().into_owned();
    } else if std::env::var_os("KITTY_PID").is_some()
        || std::env::var_os("KITTY_INSTALLATION_DIR").is_some()
    {
        if let Some(found) = terminal_info_by_pid_env("KITTY_PID") {
            return Some(found);
        }
        term = "kitty".to_string();
    } else if std::env::var_os("WT_SESSION").is_some()
        || std::env::var_os("WT_PROFILE_ID").is_some()
    {
        // Windows Terminal
        term = "Windows Terminal".to_string();
    } else if std::env::var_os("ConEmuPID").is_some() {
        term = "ConEmu".to_string();
    } else if std::env::var_os("ALACRITTY_SOCKET").is_some()
        || std::env::var_os("ALACRITTY_LOG").is_some()
        || std::env::var_os("ALACRITTY_WINDOW_ID").is_some()
    {
        term = "Alacritty".to_string();
    } else if std::env::var_os("KONSOLE_VERSION").is_some() {
        term = "konsole".to_string();
    } else if std::env::var_os("GNOME_TERMINAL_SCREEN").is_some()
        || std::env::var_os("GNOME_TERMINAL_SERVICE").is_some()
    {
        term = "gnome-terminal".to_string();
    } else if let Some(term_program) = std::env::var_os("TERM_PROGRAM") {
        term = term_program.to_string_lossy().into_owned();
    } else if let Some(lc_terminal) = std::env::var_os("LC_TERMINAL") {
        term = lc_terminal.to_string_lossy().into_owned();
    } else {
        let value = std::env::var_os("TERM").map(|t| t.to_string_lossy().into_owned());
        match value {
            Some(t) if !t.is_empty() && t != "linux" => term = t,
            // TTY
            _ => term = tty_name(),
        }
    }
    let exe_name = basename(&term);
    Some((term.clone(), term, exe_name, String::new()))
}

/// getTerminalInfoByPidEnv: fill all fields from a pid in the environment.
fn terminal_info_by_pid_env(var: &str) -> Option<(String, String, String, String)> {
    let pid_text = std::env::var(var).ok()?;
    // strtol(value, nullptr, 10): a leading number is enough.
    let end = pid_text
        .trim_start()
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(pid_text.trim_start().len());
    let pid: u32 = pid_text.trim_start()[..end].parse().ok()?;
    let (comm, _) = process_basic_info(pid)?;
    let (exe, exe_name, exe_path) = process_exe_info(pid, &comm);
    Some((comm, exe, exe_name, exe_path))
}

/// The raw terminal process name, shared with the terminalfont dispatch.
pub fn detect_raw() -> Option<String> {
    if let Some((comm, _)) = walk() {
        let gated = comm.starts_with("login")
            || comm == "(login)"
            || comm == "systemd"
            || comm == "init"
            || comm == "(init)"
            || comm == "SessionLeader"
            || comm == "0";
        if !gated {
            return Some(comm);
        }
    }
    terminal_from_env().map(|(name, _, _, _)| name)
}

/// setTerminalInfoDetails + fftsGetTerminalVersion: pretty name and
/// version for the walked-or-env terminal.
fn detect() -> Option<(String, String)> {
    let mut process_name = String::new();
    let mut exe = String::new();
    let mut exe_name = String::new();
    let mut exe_path = String::new();

    if let Some((comm, pid)) = walk() {
        let gated = comm.starts_with("login")
            || comm == "(login)"
            || comm == "systemd"
            || comm == "init"
            || comm == "(init)"
            || comm == "SessionLeader"
            || comm == "0";
        if !gated {
            (exe, exe_name, exe_path) = process_exe_info(pid, &comm);
            process_name = comm;
        }
    }
    if process_name.is_empty() {
        let (name, name_exe, name_exe_name, name_exe_path) = terminal_from_env()?;
        process_name = name;
        exe = name_exe;
        exe_name = name_exe_name;
        exe_path = name_exe_path;
    }

    // For Nixpkgs. Ref: #510 and nixpkgs PR #249428
    if process_name.starts_with('.') && exe_path.starts_with("/nix/store") {
        process_name = process_name[1..].to_string();
        if exe_name.len() < 15 {
            process_name = substr_before_last_c(&process_name, '-').to_string();
        }
    }

    let tty = tty_name();
    let pretty_name = if process_name == "wezterm-gui" {
        "WezTerm".to_string()
    } else if process_name.starts_with("tmux:") {
        "tmux".to_string()
    } else if process_name.starts_with("screen-") {
        "screen".to_string()
    } else if process_name == "sshd" || process_name.starts_with("sshd-") {
        if tty.is_empty() {
            "sshd".to_string()
        } else {
            tty
        }
    } else if process_name.starts_with("gnome-terminal") {
        "GNOME Terminal".to_string()
    } else if process_name.starts_with("kgx") {
        "GNOME Console".to_string()
    } else if process_name == "urxvt" || process_name == "urxvtd" || process_name == "rxvt" {
        "rxvt-unicode".to_string()
    } else if process_name.starts_with("ptyxis-agent") {
        "Ptyxis".to_string()
    } else if exe_name.starts_with(&process_name)
        || (exe_path.starts_with("/nix/store") && exe_name.len() > 15)
    {
        // if exeName starts with processName, print it. Otherwise print
        // processName. For nixpkgs, use exeName if processName can't be
        // unwrapped
        exe_name
    } else {
        process_name.clone()
    };

    let version =
        get_terminal_version(&process_name, if exe_path.is_empty() { &exe } else { &exe_path });
    Some((pretty_name, version))
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some((pretty_name, version)) = detect() else {
        // "Couldn't detect terminal" is an error row, hidden by default.
        return Vec::new();
    };
    let value = if version.is_empty() {
        pretty_name
    } else {
        format!("{pretty_name} {version}")
    };
    vec![Row::new("Terminal", value)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substring_combinators_match_ffstrbuf() {
        assert_eq!(substr_after_first_s("Terminal 3.4", "Terminal "), Some("3.4"));
        assert_eq!(substr_after_first_s("nothing", "Terminal "), None);
        assert_eq!(substr_after_first_c("st 0.9", ' '), Some("0.9"));
        assert_eq!(substr_after_last_c("a b c", ' '), Some("c"));
        assert_eq!(substr_after_last_c("abc", ' '), None);
        assert_eq!(substr_before_first_c("xterm(273)", '('), "xterm");
        // unchanged when absent
        assert_eq!(substr_before_first_c("abc", '('), "abc");
        assert_eq!(substr_before_last_c("nix-name-1", '-'), "nix-name");
        assert_eq!(substr_before_last_c("nodash", '-'), "nodash");
    }

    #[test]
    fn konsole_dates_decode_like_strtol() {
        assert_eq!(konsole_env_version("221201"), Some("22.12.1".to_string()));
        assert_eq!(konsole_env_version("240801"), Some("24.8.1".to_string()));
        assert_eq!(konsole_env_version("abc"), None);
        assert_eq!(konsole_env_version("12abc"), None);
        assert_eq!(konsole_env_version("-5"), None);
    }

    #[test]
    fn general_version_needs_three_numbers() {
        assert_eq!(general_version_string("3.46.7"), Some("3.46.7".to_string()));
        assert_eq!(general_version_string("3.4.5.6"), Some("3.4.5.6".to_string()));
        assert_eq!(general_version_string("3.46"), None);
        assert_eq!(general_version_string("v3.4.5"), None);
    }

    #[test]
    fn kmscon_versions_cut_at_the_head() {
        assert_eq!(kmscon_version_string("v10.0.0"), Some("10.0.0".to_string()));
        assert_eq!(kmscon_version_string("v1.2.3-patch"), Some("1.2.3".to_string()));
        assert_eq!(kmscon_version_string("10.0.0"), None);
        assert_eq!(kmscon_version_string("v1.2"), None);
    }

    #[test]
    fn sshd_versions_need_the_p_suffix() {
        assert_eq!(sshd_version_string("OpenSSH_10.0p2"), Some("10.0p2".to_string()));
        assert_eq!(sshd_version_string("OpenSSH_9.6p1"), Some("9.6p1".to_string()));
        assert_eq!(sshd_version_string("OpenSSH_10.0"), None);
        assert_eq!(sshd_version_string("other_1.2p3"), None);
    }

    #[test]
    fn da1_responses_parse_three_two_digit_fields() {
        assert_eq!(parse_da1(b"\x1b[>1;12003;0c"), (3, Some((12, 0, 3))));
        assert_eq!(parse_da1(b"\x1b[>0;1;0c"), (0, None));
        assert_eq!(parse_da1(b"\x1b[>1;12"), (1, None));
    }

    #[test]
    fn kitty_replies_decode_hex() {
        let reply = b"\x1bP1+r9=302e34322e32\x1b\\";
        let (matched, value) = parse_kitty_response(reply);
        assert_eq!(matched, 1);
        assert_eq!(value, Some("0.42.2".to_string()));
        let (matched, value) = parse_kitty_response(b"\x1bP1+r9=");
        assert_eq!(matched, 0);
        assert_eq!(value, None);
    }

    #[test]
    fn kitty_constants_parse_ints() {
        let data = b"kitty\ndef version: Version = Version(0, 42, 2):\n";
        assert_eq!(kitty_constants_version(data), Some("0.42.2".to_string()));
        assert_eq!(kitty_constants_version(b"nothing here"), None);
    }

    #[test]
    fn elf_rodata_scan_finds_planted_strings() {
        // The test binary itself is a real ELF. The linker merges adjacent
        // literals into one blob without NUL separators, which upstream's
        // libelf scan sees too — so only assert the marker is in there.
        let mut found = None;
        let Ok(exe) = std::env::current_exe() else {
            panic!("no test binary path");
        };
        binary_extract_strings(&exe.to_string_lossy(), "0.0.0".len(), &mut |s| {
            if s.contains("rustfetch-elf-scan") {
                found = Some(s.to_string());
                false
            } else {
                true
            }
        });
        assert!(found.is_some_and(|s| s.contains("rustfetch-elf-scan")));
        let mut any = false;
        binary_extract_strings("/nonexistent/file", 4, &mut |_| {
            any = true;
            false
        });
        assert!(!any);
    }

    #[test]
    fn walk_skip_list_covers_upstream_entries() {
        assert!(is_skipped("zsh"));
        assert!(is_skipped("ksh93"));
        assert!(is_skipped("Relay(auth)"));
        assert!(is_skipped("flatpak-run.sh"));
        assert!(is_skipped("boot.sh"));
        assert!(!is_skipped("ghostty"));
        assert!(!is_skipped("zcode-cli"));
        assert!(!is_skipped("konsole"));
    }
}

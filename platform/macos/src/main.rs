// rustfetch — native macOS build.
//
// A standalone, libc-only port of the Linux rustfetch for macOS. It ships its
// own binary also named `rustfetch` and deliberately has no dependency on the
// root crate (which is Linux-only). Every system fact is read through libc /
// small `extern "C"` declarations to libSystem; no subprocesses are spawned.
//
// Visual identity matches the Linux build: a colored ASCII logo on the left,
// `Key: value` rows on the right, fastfetch-style `user@host` title with a
// dashed underline, and hyfetch-style pride-flag gradients.

use std::ffi::{CStr, CString};
use std::path::Path;

const VERSION: &str = "0.2.5";

// ── ANSI colors ──────────────────────────────────────────────────────────────

const RESET: &str = "\x1b[0m";
const KEY_COLOR: &str = "\x1b[1;36m";
const TITLE_COLOR: &str = "\x1b[1;92m";
const LOGO_COLOR: &str = "\x1b[1;32m";

// ── Mach declarations not present in the `libc` crate ────────────────────────
// Both symbols live in libSystem and are available on every modern macOS.
// `host_statistics` with the HOST_VM_INFO flavour fills a `vm_statistics_data_t`
// — the exact 32-bit struct the `vm_stat(1)` command uses. Its layout (12
// natural_t/u32 fields) has been stable since 10.x.

/// Classic `vm_statistics_data_t`: 12 `natural_t` (= u32) counters.
#[repr(C)]
#[derive(Default, Clone, Copy)]
#[allow(dead_code)]
struct VmStatistics {
    free_count: u32,
    active_count: u32,
    inactive_count: u32,
    wire_count: u32,
    zero_fill_count: u32,
    reactivations: u32,
    pageins: u32,
    pageouts: u32,
    faults: u32,
    cow_faults: u32,
    lookups: u32,
    hits: u32,
}

const HOST_VM_INFO: libc::c_int = 2;
const HOST_VM_INFO_COUNT: libc::c_uint = 12;

/// `mach_port_t` is `unsigned int` on macOS; use the primitive so this file
/// does not depend on how `libc` re-exports Mach types at its crate root.
type MachPort = u32;

unsafe extern "C" {
    fn mach_host_self() -> MachPort;
    fn host_statistics(
        host: MachPort,
        flavor: libc::c_int,
        host_info_out: *mut libc::c_int,
        host_info_out_count: *mut libc::c_uint,
    ) -> libc::c_int;
}

// ── sysctl helpers ───────────────────────────────────────────────────────────

fn sysctl_bytes(name: &str) -> Option<Vec<u8>> {
    let name = CString::new(name).ok()?;
    let mut len: libc::size_t = 0;
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || len == 0
    {
        return None;
    }
    let mut buf = vec![0u8; len];
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    buf.truncate(len);
    Some(buf)
}

fn sysctl_str(name: &str) -> Option<String> {
    let bytes = sysctl_bytes(name)?;
    let text = String::from_utf8_lossy(&bytes);
    Some(text.trim_end_matches('\0').trim().to_owned())
}

fn sysctl_u64(name: &str) -> Option<u64> {
    let name = CString::new(name).ok()?;
    let mut value: u64 = 0;
    let mut len = std::mem::size_of::<u64>() as libc::size_t;
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut u64).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    Some(value)
}

fn sysctl_u32(name: &str) -> Option<u32> {
    let name = CString::new(name).ok()?;
    let mut value: u32 = 0;
    let mut len = std::mem::size_of::<u32>() as libc::size_t;
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut u32).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    Some(value)
}

fn boot_timeval() -> Option<libc::timeval> {
    let name = CString::new("kern.boottime").ok()?;
    let mut tv: libc::timeval = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::timeval>() as libc::size_t;
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut tv as *mut libc::timeval).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    Some(tv)
}

// ── module values ────────────────────────────────────────────────────────────

fn user_name() -> Option<String> {
    let uid = unsafe { libc::getuid() };
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0 as libc::c_char; 16384];
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let rc = unsafe {
        libc::getpwuid_r(
            uid,
            &mut pwd,
            buf.as_mut_ptr(),
            buf.len(),
            &mut result,
        )
    };
    if rc == 0 && !result.is_null() && !pwd.pw_name.is_null() {
        Some(unsafe { CStr::from_ptr(pwd.pw_name) }.to_string_lossy().into_owned())
    } else {
        std::env::var("USER").ok().filter(|s| !s.is_empty())
    }
}

fn host_name() -> Option<String> {
    let mut buf = [0 as libc::c_char; 256];
    if unsafe { libc::gethostname(buf.as_mut_ptr(), buf.len()) } != 0 {
        return None;
    }
    Some(unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned())
}

fn title_value() -> Option<String> {
    Some(format!("{}@{}", user_name()?, host_name()?))
}

fn os_value() -> Option<String> {
    let product = sysctl_str("kern.osproductversion").unwrap_or_default();
    let build = sysctl_str("kern.osversion").unwrap_or_default();
    match (product.is_empty(), build.is_empty()) {
        (true, true) => None,
        (false, true) => Some(format!("macOS {product}")),
        (true, false) => Some(format!("macOS ({build})")),
        (false, false) => Some(format!("macOS {product} ({build})")),
    }
}

fn kernel_value() -> Option<String> {
    sysctl_str("kern.osrelease").map(|release| format!("Darwin {release}"))
}

fn uptime_value() -> Option<String> {
    let boot = boot_timeval()?;
    let mut now: libc::timespec = unsafe { std::mem::zeroed() };
    if unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut now) } != 0 {
        return None;
    }
    let seconds = (now.tv_sec - boot.tv_sec as i64).max(0);
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let mins = (seconds % 3600) / 60;
    let mut parts: Vec<String> = Vec::new();
    if days > 0 {
        parts.push(format!("{days} day{}", if days == 1 { "" } else { "s" }));
    }
    if hours > 0 {
        parts.push(format!("{hours} hour{}", if hours == 1 { "" } else { "s" }));
    }
    if mins > 0 || parts.is_empty() {
        parts.push(format!("{mins} min{}", if mins == 1 { "" } else { "s" }));
    }
    Some(parts.join(", "))
}

fn shell_value() -> Option<String> {
    let shell = std::env::var("SHELL").ok()?;
    Path::new(&shell)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
}

fn terminal_value() -> Option<String> {
    std::env::var("TERM_PROGRAM")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("TERM")
                .ok()
                .filter(|s| !s.is_empty() && s != "dumb")
        })
}

fn cpu_value() -> Option<String> {
    let brand = sysctl_str("machdep.cpu.brand_string").or_else(|| sysctl_str("hw.model"));
    let cores = sysctl_u32("hw.ncpu").unwrap_or(0);
    match (brand, cores) {
        (Some(brand), cores) if !brand.is_empty() && cores > 0 => {
            Some(format!("{brand} ({cores})"))
        },
        (Some(brand), _) if !brand.is_empty() => Some(brand),
        (_, cores) if cores > 0 => Some(format!("{cores}-core CPU")),
        _ => None,
    }
}

/// Binary-prefix byte formatting, e.g. `6.23 GiB`.
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

fn percent(used: u64, total: u64) -> u64 {
    if total == 0 {
        0
    } else {
        ((used as f64 / total as f64) * 100.0).round() as u64
    }
}

fn memory_value() -> Option<String> {
    let total = sysctl_u64("hw.memsize")?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let page_size = if page_size > 0 { page_size as u64 } else { 4096 };

    // host_statistics(HOST_VM_INFO) is the same data `vm_stat(1)` prints.
    // We count active + inactive + wired pages as "used" and clamp to total.
    let mut used = 0u64;
    let host = unsafe { mach_host_self() };
    let mut stats: VmStatistics = VmStatistics::default();
    let mut count = HOST_VM_INFO_COUNT;
    if unsafe {
        host_statistics(
            host,
            HOST_VM_INFO,
            (&mut stats as *mut VmStatistics).cast(),
            &mut count,
        )
    } == 0
    {
        let used_pages = stats.active_count as u64
            + stats.inactive_count as u64
            + stats.wire_count as u64;
        used = used_pages.saturating_mul(page_size).min(total);
    }
    Some(format!(
        "{} / {} ({}%)",
        human_bytes(used),
        human_bytes(total),
        percent(used, total)
    ))
}

fn disk_value() -> Option<String> {
    let path = CString::new("/").ok()?;
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    let block = stat.f_bsize as u64;
    let total = stat.f_blocks.saturating_mul(block);
    let available = stat.f_bavail.saturating_mul(block);
    let used = total.saturating_sub(available);
    Some(format!(
        "{} / {} ({}%)",
        human_bytes(used),
        human_bytes(total),
        percent(used, total)
    ))
}

fn local_ip_value() -> Option<String> {
    let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut ifap) } != 0 || ifap.is_null() {
        return None;
    }
    let mut ips: Vec<String> = Vec::new();
    let mut cursor = ifap;
    while !cursor.is_null() {
        // SAFETY: cursor walks the getifaddrs list until its NULL terminator.
        let ifa: &libc::ifaddrs = unsafe { &*cursor };
        cursor = ifa.ifa_next;
        if ifa.ifa_addr.is_null() || ifa.ifa_name.is_null() {
            continue;
        }
        let family = unsafe { (*ifa.ifa_addr).sa_family } as i32;
        if family != libc::AF_INET {
            continue;
        }
        // Skip loopback interfaces (lo0 on macOS), like the Linux build's
        // default `showLoop = false`.
        if ifa.ifa_flags & libc::IFF_LOOPBACK as libc::c_uint != 0 {
            continue;
        }
        // SAFETY: ifa_addr is an AF_INET sockaddr for this entry.
        let sin = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in) };
        let octets = u32::from_be(sin.sin_addr.s_addr).to_be_bytes();
        let name = unsafe { CStr::from_ptr(ifa.ifa_name) }.to_string_lossy();
        ips.push(format!(
            "{}.{}.{}.{} ({})",
            octets[0], octets[1], octets[2], octets[3], name
        ));
    }
    unsafe { libc::freeifaddrs(ifap) };
    if ips.is_empty() {
        None
    } else {
        Some(ips.join(", "))
    }
}

// ── modules ──────────────────────────────────────────────────────────────────

/// (id, display label). `title` is rendered specially with a dashed underline.
const MODULES: &[(&str, &str)] = &[
    ("title", "User@Host"),
    ("os", "OS"),
    ("kernel", "Kernel"),
    ("uptime", "Uptime"),
    ("shell", "Shell"),
    ("terminal", "Terminal"),
    ("cpu", "CPU"),
    ("memory", "Memory"),
    ("disk", "Disk"),
    ("localip", "Local IP"),
];

fn normalize_module(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "title" | "user" => Some("title"),
        "os" => Some("os"),
        "kernel" | "kern" => Some("kernel"),
        "uptime" => Some("uptime"),
        "shell" => Some("shell"),
        "terminal" | "term" => Some("terminal"),
        "cpu" => Some("cpu"),
        "memory" | "mem" => Some("memory"),
        "disk" => Some("disk"),
        "localip" | "ip" => Some("localip"),
        _ => None,
    }
}

struct Row {
    id: &'static str,
    label: &'static str,
    value: String,
}

fn value_for(id: &str) -> String {
    let value = match id {
        "title" => title_value(),
        "os" => os_value(),
        "kernel" => kernel_value(),
        "uptime" => uptime_value(),
        "shell" => shell_value(),
        "terminal" => terminal_value(),
        "cpu" => cpu_value(),
        "memory" => memory_value(),
        "disk" => disk_value(),
        "localip" => local_ip_value(),
        _ => None,
    };
    value.unwrap_or_else(|| "Unknown".to_owned())
}

fn build_rows(ids: &[&'static str]) -> Vec<Row> {
    ids.iter()
        .filter_map(|id| {
            MODULES
                .iter()
                .find(|(module_id, _)| module_id == id)
                .map(|(_, label)| Row {
                    id,
                    label,
                    value: value_for(id),
                })
        })
        .collect()
}

// ── tiny JSON/JSONC scanner (no serde) ───────────────────────────────────────

#[derive(Clone, Debug)]
enum Json {
    Null,
    #[allow(dead_code)]
    Bool(bool),
    #[allow(dead_code)]
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    fn as_arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn new(text: &'a str) -> Self {
        Self { bytes: text.as_bytes(), pos: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        loop {
            while matches!(self.peek(), Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')) {
                self.pos += 1;
            }
            // // line comment
            if self.peek() == Some(b'/') && self.bytes.get(self.pos + 1) == Some(&b'/') {
                self.pos += 2;
                while let Some(c) = self.peek() {
                    self.pos += 1;
                    if c == b'\n' {
                        break;
                    }
                }
                continue;
            }
            // /* block comment */
            if self.peek() == Some(b'/') && self.bytes.get(self.pos + 1) == Some(&b'*') {
                self.pos += 2;
                while self.pos + 1 < self.bytes.len()
                    && !(self.bytes[self.pos] == b'*' && self.bytes[self.pos + 1] == b'/')
                {
                    self.pos += 1;
                }
                self.pos = (self.pos + 2).min(self.bytes.len());
                continue;
            }
            break;
        }
    }

    fn parse(mut self) -> Option<Json> {
        self.skip_ws();
        self.value()
    }

    fn value(&mut self) -> Option<Json> {
        self.skip_ws();
        match self.peek()? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string().map(Json::Str),
            b't' => self.literal(b"true", Json::Bool(true)),
            b'f' => self.literal(b"false", Json::Bool(false)),
            b'n' => self.literal(b"null", Json::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn literal(&mut self, word: &[u8], value: Json) -> Option<Json> {
        if self.bytes.get(self.pos..self.pos + word.len())? == word {
            self.pos += word.len();
            Some(value)
        } else {
            None
        }
    }

    fn string(&mut self) -> Option<String> {
        if self.peek()? != b'"' {
            return None;
        }
        self.pos += 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.pos += 1;
            match c {
                b'"' => return Some(out),
                b'\\' => {
                    let escaped = self.peek()?;
                    self.pos += 1;
                    match escaped {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = self.bytes.get(self.pos..self.pos + 4)?;
                            let hex = std::str::from_utf8(hex).ok()?;
                            let code = u32::from_str_radix(hex, 16).ok()?;
                            self.pos += 4;
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        },
                        _ => return None,
                    }
                },
                _ => out.push(c as char),
            }
        }
        None
    }

    fn number(&mut self) -> Option<Json> {
        let start = self.pos;
        while matches!(
            self.peek(),
            Some(b'0'..=b'9') | Some(b'-') | Some(b'+') | Some(b'.') | Some(b'e') | Some(b'E')
        ) {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
        text.parse::<f64>().ok().map(Json::Num)
    }

    fn array(&mut self) -> Option<Json> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Some(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    break;
                },
                _ => return None,
            }
        }
        Some(Json::Arr(items))
    }

    fn object(&mut self) -> Option<Json> {
        self.pos += 1;
        let mut entries = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Some(Json::Obj(entries));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            if self.peek()? != b':' {
                return None;
            }
            self.pos += 1;
            let value = self.value()?;
            entries.push((key, value));
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    break;
                },
                _ => return None,
            }
        }
        Some(Json::Obj(entries))
    }
}

#[derive(Default)]
struct Config {
    modules: Option<Vec<String>>,
    preset: Option<String>,
    logo: Option<String>,
}

fn load_config() -> Config {
    let Some(home) = std::env::var_os("HOME") else {
        return Config::default();
    };
    let path = Path::new(&home).join(".config/fastfetch/config.jsonc");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Config::default();
    };
    let Some(root) = JsonParser::new(&text).parse() else {
        return Config::default();
    };

    let modules = root.get("modules").and_then(Json::as_arr).map(|items| {
        items
            .iter()
            .filter_map(|item| match item {
                Json::Str(s) => Some(s.clone()),
                Json::Obj(_) => item.get("type").and_then(Json::as_str).map(str::to_owned),
                _ => None,
            })
            .collect()
    });
    let preset = root
        .get("logo")
        .and_then(|logo| logo.get("hyfetch"))
        .and_then(|hyfetch| hyfetch.get("preset"))
        .and_then(Json::as_str)
        .map(str::to_owned);
    let logo = root
        .get("logo")
        .and_then(|logo| logo.get("source"))
        .and_then(Json::as_str)
        .map(str::to_owned);

    Config { modules, preset, logo }
}

// ── hyfetch presets & gradient ───────────────────────────────────────────────

// Exact vendored hex values from hyfetch's presets.json (MIT, Dylan Araps).
const PRESETS: &[(&str, &[u32])] = &[
    (
        "rainbow",
        &[0xE5_0000, 0xFF_8D00, 0xFF_EE00, 0x02_8121, 0x00_4CFF, 0x77_0088],
    ),
    (
        "transgender",
        &[0x55_CDFD, 0xF6_AAB7, 0xFF_FFFF, 0xF6_AAB7, 0x55_CDFD],
    ),
    (
        "progress",
        &[
            0xFF_FFFF, 0xFF_AFC8, 0x74_D7EE, 0x61_3915, 0x00_0000, 0xE5_0000, 0xFF_8D00,
            0xFF_EE00, 0x02_8121, 0x00_4CFF, 0x77_0088,
        ],
    ),
];

const LIGHTNESS_FLOOR: f64 = 0.65;

fn rgb_to_hsl(rgb: u32) -> (f64, f64, f64) {
    let r = ((rgb >> 16) & 0xff) as f64 / 255.0;
    let g = ((rgb >> 8) & 0xff) as f64 / 255.0;
    let b = (rgb & 0xff) as f64 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) / 2.0;
    let delta = max - min;
    if delta == 0.0 {
        return (0.0, 0.0, lightness);
    }
    let saturation = if lightness > 0.5 {
        delta / (2.0 - max - min)
    } else {
        delta / (max + min)
    };
    let mut hue = if max == r {
        (g - b) / delta
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } * 60.0;
    if hue < 0.0 {
        hue += 360.0;
    }
    (hue, saturation, lightness)
}

fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> u32 {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let second = chroma * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let match_value = lightness - chroma / 2.0;
    let (r, g, b) = if hue < 60.0 {
        (chroma, second, 0.0)
    } else if hue < 120.0 {
        (second, chroma, 0.0)
    } else if hue < 180.0 {
        (0.0, chroma, second)
    } else if hue < 240.0 {
        (0.0, second, chroma)
    } else if hue < 300.0 {
        (second, 0.0, chroma)
    } else {
        (chroma, 0.0, second)
    };
    let byte = |value: f64| ((value + match_value) * 255.0).round().clamp(0.0, 255.0) as u32;
    (byte(r) << 16) | (byte(g) << 8) | byte(b)
}

/// Approximate hyfetch's OKHSL 0.65 lightness floor with plain HSL. This is
/// intentionally NOT byte-identical to the Linux build's exact OKHSL port; see
/// README ("HyFetch lightness") for the rationale.
fn lighten(rgb: u32) -> u32 {
    let (hue, saturation, lightness) = rgb_to_hsl(rgb);
    if lightness >= LIGHTNESS_FLOOR {
        rgb
    } else {
        hsl_to_rgb(hue, saturation, LIGHTNESS_FLOOR)
    }
}

/// Deterministic-enough pseudo-random index without another dependency.
fn random_index(len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let mut state = nanos ^ ((std::process::id() as u64) << 32) ^ 0x9e37_79b9_7f4a_7c15;
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    (state as usize) % len
}

fn preset_colors(name: &str) -> Option<Vec<u32>> {
    let name = if name.eq_ignore_ascii_case("random") {
        PRESETS[random_index(PRESETS.len())].0
    } else {
        PRESETS
            .iter()
            .find(|(preset, _)| preset.eq_ignore_ascii_case(name))
            .map(|(preset, _)| *preset)?
    };
    PRESETS
        .iter()
        .find(|(preset, _)| *preset == name)
        .map(|(_, colors)| colors.iter().copied().map(lighten).collect())
}

/// Spread `colors` over `lines` slots with hyfetch's `with_length` weighting:
/// each color gets `lines / len` copies plus a share of the remainder (center
/// first, then outermost pairs).
fn spread(colors: &[u32], lines: usize) -> Vec<u32> {
    if colors.is_empty() || lines == 0 {
        return Vec::new();
    }
    let len = colors.len();
    let center = len / 2;
    let repeats = lines / len;
    let mut extras = lines % len;
    let mut weights = vec![repeats; len];
    if extras % 2 == 1 {
        weights[center] += 1;
        extras -= 1;
    }
    for i in 0..extras / 2 {
        weights[i] += 1;
        weights[len - 1 - i] += 1;
    }
    let mut out = Vec::with_capacity(lines);
    for (index, times) in weights.iter().enumerate() {
        for _ in 0..*times {
            out.push(colors[index]);
        }
    }
    out
}

fn rgb_escape(color: u32) -> String {
    format!("\x1b[38;2;{};{};{}m", color >> 16 & 0xff, color >> 8 & 0xff, color & 0xff)
}

// ── logos ────────────────────────────────────────────────────────────────────

const APPLE_LOGO: &str = r#"             .:--==--.
          .:=+*######*+=-.
        :=+*############*+=.
       :+##################+:
      :+####################+:
     :+######################+:
     +########################+
     +########################+
     +#########::##############+
     +########################+
     +########################+
      +######################+
      :+####################+:
       :+##################+:
        .:=+*##########*+=-.
             .:--==--."#;

const ARCH_LOGO: &str = r#"                   -`
                  .o+`
                 `ooo/
                `+oooo:
               `+oooooo:
               -+oooooo+:
             `/:-:++oooo+:
            `/++++/+++++++:
           `/++++++++++++++:
          `/+++ooooooooooooo/`
         ./ooosssso++osssssso+`
        .oossssso-````/ossssss+`
       -osssssso.      :ssssssso.
      :osssssss/        osssso+++.
     /ossssssss/        +ssssooo/-
   `/ossssso+/:-        -:/+osssso+-
  `+sso+:-`                 `.-/+oso:
 `++:.                           `-/+/
 .`                                 `/"#;

const RUST_LOGO: &str = r#" #######
##     ##
##     ##
 #######
 ##  ##
 ##   ##
 ##    ##"#;

fn logo_lines(name: &str) -> Vec<String> {
    let source = match name.to_ascii_lowercase().as_str() {
        "none" | "off" | "" => return Vec::new(),
        "arch" | "neofetch" => ARCH_LOGO,
        "rust" => RUST_LOGO,
        "apple" | "mac" | "macos" | "osx" => APPLE_LOGO,
        _ => {
            eprintln!("rustfetch: unknown logo '{name}', using 'apple'");
            APPLE_LOGO
        },
    };
    source.lines().map(str::to_owned).collect()
}

// ── rendering ────────────────────────────────────────────────────────────────

fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn render_json(rows: &[Row]) {
    println!("{{");
    for (index, row) in rows.iter().enumerate() {
        let comma = if index + 1 == rows.len() { "" } else { "," };
        println!(
            "  \"{}\": \"{}\"{comma}",
            json_escape(row.id),
            json_escape(&row.value)
        );
    }
    println!("}}");
}

fn render_text(rows: &[Row], logo_name: &str, preset: Option<&str>, colors: bool) {
    let mut info: Vec<String> = Vec::new();
    for row in rows {
        if row.id == "title" {
            if colors {
                info.push(format!("{TITLE_COLOR}{}{RESET}", row.value));
                info.push(format!(
                    "{TITLE_COLOR}{}{RESET}",
                    "-".repeat(row.value.chars().count())
                ));
            } else {
                info.push(row.value.clone());
                info.push("-".repeat(row.value.chars().count()));
            }
        } else if colors {
            info.push(format!("{KEY_COLOR}{}:{RESET} {}", row.label, row.value));
        } else {
            info.push(format!("{}: {}", row.label, row.value));
        }
    }

    let mut logo = logo_lines(logo_name);
    if !logo.is_empty() && colors {
        let palette = preset.and_then(preset_colors);
        let escapes: Vec<String> = match palette {
            Some(colors) => spread(&colors, logo.len())
                .into_iter()
                .map(rgb_escape)
                .collect(),
            None => vec![LOGO_COLOR.to_owned(); logo.len()],
        };
        for (line, escape) in logo.iter_mut().zip(escapes) {
            *line = format!("{escape}{line}{RESET}");
        }
    }

    let logo_width = logo
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let gap = if logo.is_empty() { 0 } else { 2 };
    let lines = logo.len().max(info.len());
    for index in 0..lines {
        let left = match logo.get(index) {
            Some(line) => {
                let padding = logo_width.saturating_sub(line.chars().count()) + gap;
                format!("{}{}", " ".repeat(padding), line)
            },
            None => String::new(),
        };
        let right = info.get(index).cloned().unwrap_or_default();
        let combined = format!("{left}{right}");
        println!("{}", combined.trim_end());
    }
}

// ── CLI ──────────────────────────────────────────────────────────────────────

fn print_help() {
    println!(
        "rustfetch {VERSION} — native macOS build\n\n\
         Usage: rustfetch [options] [modules...]\n\n\
         Options:\n\
           --logo NAME|none      Built-in ASCII logo: apple (default), arch, rust, neofetch, none\n\
           --pipe                Disable colors\n\
           --format json         Print a simple JSON object (no logo, no colors)\n\
           --list-modules        List available module IDs\n\
           --hyfetch-preset NAME Pride-flag logo gradient: rainbow, transgender, progress, random\n\
           --help, -h            Show this help\n\
           --version, -V         Show version and architecture\n\n\
         With no module arguments, the default layout is:\n\
           title, os, kernel, uptime, shell, terminal, cpu, memory, disk, localip\n\n\
         Modules can also come from ~/.config/fastfetch/config.jsonc (`modules`\n\
         array and `logo.hyfetch.preset`). CLI flags override the config."
    );
}

fn print_modules() {
    for (id, label) in MODULES {
        println!("{id:<12} {label}");
    }
}

fn main() {
    let config = load_config();

    let mut colors = true;
    let mut format = String::from("text");
    let mut logo: Option<String> = None;
    let mut preset: Option<String> = None;
    let mut cli_modules: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                return;
            },
            "--version" | "-V" => {
                println!("rustfetch {VERSION} ({})", std::env::consts::ARCH);
                return;
            },
            "--list-modules" => {
                print_modules();
                return;
            },
            "--pipe" => colors = false,
            "--format" => match args.next() {
                Some(value) => format = value,
                None => eprintln!("rustfetch: --format requires a value"),
            },
            "--logo" | "-l" => match args.next() {
                Some(value) => logo = Some(value),
                None => eprintln!("rustfetch: --logo requires a value"),
            },
            "--hyfetch-preset" => match args.next() {
                Some(value) => preset = Some(value),
                None => eprintln!("rustfetch: --hyfetch-preset requires a value"),
            },
            _ => {
                if let Some(value) = arg.strip_prefix("--format=") {
                    format = value.to_owned();
                } else if let Some(value) = arg.strip_prefix("--logo=") {
                    logo = Some(value.to_owned());
                } else if let Some(value) = arg.strip_prefix("--hyfetch-preset=") {
                    preset = Some(value.to_owned());
                } else if arg.starts_with('-') {
                    eprintln!("rustfetch: unknown option '{arg}'");
                } else if normalize_module(&arg).is_some() {
                    cli_modules.push(arg);
                } else {
                    eprintln!("rustfetch: unknown module '{arg}'");
                }
            },
        }
    }

    let module_names: Vec<String> = if !cli_modules.is_empty() {
        cli_modules
    } else if let Some(modules) = config.modules {
        modules
    } else {
        MODULES.iter().map(|(id, _)| (*id).to_owned()).collect()
    };

    let ids: Vec<&'static str> = module_names
        .iter()
        .filter_map(|name| normalize_module(name))
        .collect();
    let rows = build_rows(&ids);

    if format.eq_ignore_ascii_case("json") {
        render_json(&rows);
        return;
    }
    if !format.eq_ignore_ascii_case("text") {
        eprintln!("rustfetch: unknown format '{format}', using text");
    }

    let logo_name = logo
        .or(config.logo)
        .unwrap_or_else(|| "apple".to_owned());
    let preset = preset.or(config.preset);
    render_text(&rows, &logo_name, preset.as_deref(), colors);
}

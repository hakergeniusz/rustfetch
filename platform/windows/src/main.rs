//! rustfetch 0.2.5 — native Windows build.
//!
//! Standalone crate: no crates.io dependencies. Every Win32 entry point used
//! here is declared below with its canonical ABI so the binary links only
//! against system libraries (kernel32, advapi32, ntdll, ws2_32).

use std::env;
use std::ffi::c_void;
use std::io::{self, Write};
use std::mem::{self, MaybeUninit};
use std::path::PathBuf;
use std::ptr;

// ---------------------------------------------------------------------------
// Win32 constants
// ---------------------------------------------------------------------------

const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5; // (DWORD)-11
const ENABLE_PROCESSED_OUTPUT: u32 = 0x0001;
const ENABLE_WRAP_AT_EOL_OUTPUT: u32 = 0x0002;
const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;

const HKEY_LOCAL_MACHINE: isize = 0x8000_0002usize as isize; // (HKEY)0x80000002
const KEY_READ: u32 = 0x0002_0019;
const REG_SZ: u32 = 1;
const REG_EXPAND_SZ: u32 = 2;
const REG_DWORD: u32 = 4;

const DRIVE_FIXED: u32 = 3;

const AF_INET: i32 = 2;
const SOCK_DGRAM: i32 = 2;
const IPPROTO_UDP: i32 = 17;
const INVALID_SOCKET: usize = usize::MAX;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[cfg(target_arch = "x86_64")]
const ARCH: &str = "x86_64";
#[cfg(target_arch = "x86")]
const ARCH: &str = "x86";
#[cfg(target_arch = "aarch64")]
const ARCH: &str = "aarch64";
#[cfg(not(any(
    target_arch = "x86_64",
    target_arch = "x86",
    target_arch = "aarch64"
)))]
const ARCH: &str = "unknown";

// ---------------------------------------------------------------------------
// Win32 structures (repr(C); pointer-sized fields keep x86/x64 layouts correct)
// ---------------------------------------------------------------------------

/// RTL_OSVERSIONINFOEXW / OSVERSIONINFOEXW — 284 bytes on both x86 and x64.
#[repr(C)]
struct OsVersionInfoExW {
    dw_os_version_info_size: u32,
    dw_major_version: u32,
    dw_minor_version: u32,
    dw_build_number: u32,
    dw_platform_id: u32,
    sz_csd_version: [u16; 128],
    w_service_pack_major: u16,
    w_service_pack_minor: u16,
    w_suite_mask: u16,
    w_product_type: u8,
    w_reserved: u8,
}

/// SYSTEM_INFO — the anonymous union is represented by its first two WORDs;
/// the compiler inserts the correct padding before the pointer fields.
#[repr(C)]
struct SystemInfo {
    w_processor_architecture: u16,
    w_reserved: u16,
    dw_page_size: u32,
    lp_minimum_application_address: *mut c_void,
    lp_maximum_application_address: *mut c_void,
    dw_active_processor_mask: usize,
    dw_number_of_processors: u32,
    dw_processor_type: u32,
    dw_allocation_granularity: u32,
    w_processor_level: u16,
    w_processor_revision: u16,
}

/// MEMORYSTATUSEX — 64 bytes.
#[repr(C)]
struct MemoryStatusEx {
    dw_length: u32,
    dw_memory_load: u32,
    ull_total_phys: u64,
    ull_avail_phys: u64,
    ull_total_page_file: u64,
    ull_avail_page_file: u64,
    ull_total_virtual: u64,
    ull_avail_virtual: u64,
    ull_avail_extended_virtual: u64,
}

/// sockaddr_in — 16 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
struct SockAddrIn {
    sin_family: i16,
    sin_port: u16,
    sin_addr: u32,
    sin_zero: [u8; 8],
}

/// WSADATA is ~408 bytes on x64 / ~396 on x86; 512 aligned bytes is a safe
/// opaque replacement since we never read any of its fields.
#[repr(C, align(8))]
struct WsaData {
    bytes: [u8; 512],
}

// ---------------------------------------------------------------------------
// Foreign functions
// ---------------------------------------------------------------------------

#[link(name = "kernel32")]
extern "system" {
    fn GetStdHandle(n_std_handle: u32) -> isize;
    fn GetConsoleMode(h_console_handle: isize, lp_mode: *mut u32) -> i32;
    fn SetConsoleMode(h_console_handle: isize, dw_mode: u32) -> i32;
    fn GetTickCount64() -> u64;
    fn GetSystemInfo(lp_system_info: *mut SystemInfo);
    fn GetVersionExW(lp_version_information: *mut OsVersionInfoExW) -> i32;
    fn GlobalMemoryStatusEx(lp_memory_status: *mut MemoryStatusEx) -> i32;
    fn GetLogicalDrives() -> u32;
    fn GetDriveTypeW(lp_root_path_name: *const u16) -> u32;
    fn GetDiskFreeSpaceExW(
        lp_directory_name: *const u16,
        lp_free_bytes_available: *mut u64,
        lp_total_number_of_bytes: *mut u64,
        lp_total_number_of_free_bytes: *mut u64,
    ) -> i32;
}

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(
        h_key: isize,
        lp_sub_key: *const u16,
        ul_options: u32,
        sam_desired: u32,
        phk_result: *mut isize,
    ) -> i32;
    fn RegQueryValueExW(
        h_key: isize,
        lp_value_name: *const u16,
        lp_reserved: *mut u32,
        lp_type: *mut u32,
        lp_data: *mut u8,
        lpcb_data: *mut u32,
    ) -> i32;
    fn RegCloseKey(h_key: isize) -> i32;
}

#[link(name = "ntdll")]
extern "system" {
    fn RtlGetVersion(lp_version_information: *mut OsVersionInfoExW) -> i32;
}

#[link(name = "ws2_32")]
extern "system" {
    fn WSAStartup(w_version_requested: u16, lp_wsa_data: *mut WsaData) -> i32;
    fn WSACleanup() -> i32;
    fn socket(af: i32, type_: i32, protocol: i32) -> usize;
    fn connect(s: usize, name: *const SockAddrIn, namelen: i32) -> i32;
    fn getsockname(s: usize, name: *mut SockAddrIn, namelen: *mut i32) -> i32;
    fn closesocket(s: usize) -> i32;
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn wide(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

fn basename(s: &str) -> String {
    s.rsplit(|c| c == '\\' || c == '/')
        .next()
        .unwrap_or(s)
        .trim()
        .to_string()
}

fn reg_read_string(subkey: &str, value: &str) -> Option<String> {
    unsafe {
        let sub = wide(subkey);
        let mut hkey: isize = 0;
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, sub.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return None;
        }
        let val = wide(value);
        let mut ty = 0u32;
        let mut len = 0u32;
        let rc = RegQueryValueExW(
            hkey,
            val.as_ptr(),
            ptr::null_mut(),
            &mut ty,
            ptr::null_mut(),
            &mut len,
        );
        let _ = RegCloseKey(hkey);
        if rc != 0 || (ty != REG_SZ && ty != REG_EXPAND_SZ) || len == 0 {
            return None;
        }
        let hkey2 = {
            let mut h2: isize = 0;
            if RegOpenKeyExW(HKEY_LOCAL_MACHINE, sub.as_ptr(), 0, KEY_READ, &mut h2) != 0 {
                return None;
            }
            h2
        };
        let mut buf = vec![0u16; (len as usize / 2) + 1];
        let mut out_len = len;
        let rc = RegQueryValueExW(
            hkey2,
            val.as_ptr(),
            ptr::null_mut(),
            &mut ty,
            buf.as_mut_ptr().cast::<u8>(),
            &mut out_len,
        );
        let _ = RegCloseKey(hkey2);
        if rc != 0 {
            return None;
        }
        Some(from_wide(&buf))
    }
}

fn reg_read_u32(subkey: &str, value: &str) -> Option<u32> {
    unsafe {
        let sub = wide(subkey);
        let mut hkey: isize = 0;
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, sub.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return None;
        }
        let val = wide(value);
        let mut ty = 0u32;
        let mut len = 4u32;
        let mut out = 0u32;
        let rc = RegQueryValueExW(
            hkey,
            val.as_ptr(),
            ptr::null_mut(),
            &mut ty,
            (&mut out as *mut u32).cast::<u8>(),
            &mut len,
        );
        let _ = RegCloseKey(hkey);
        if rc != 0 || ty != REG_DWORD || len < 4 {
            None
        } else {
            Some(out)
        }
    }
}

/// Ask the console for VT processing. Returns false on redirected output.
fn enable_ansi() -> bool {
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle == 0 || handle == -1 {
            return false;
        }
        let mut mode = 0u32;
        if GetConsoleMode(handle, &mut mode) == 0 {
            return false;
        }
        let flags = mode
            | ENABLE_PROCESSED_OUTPUT
            | ENABLE_WRAP_AT_EOL_OUTPUT
            | ENABLE_VIRTUAL_TERMINAL_PROCESSING;
        if SetConsoleMode(handle, flags) != 0 {
            return true;
        }
        SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}

// ---------------------------------------------------------------------------
// Info collection
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Rgb(u8, u8, u8);

struct DiskInfo {
    letter: String,
    total: u64,
    free: u64,
    used: u64,
    percent: u32,
}

struct Info {
    user: String,
    host: String,
    os: String,
    kernel: String,
    uptime: String,
    shell: String,
    terminal: String,
    cpu: String,
    gpu: Option<String>,
    memory: String,
    disks: Vec<DiskInfo>,
    local_ip: String,
}

fn windows_version_name(v: &OsVersionInfoExW) -> String {
    let major = v.dw_major_version;
    let minor = v.dw_minor_version;
    let build = v.dw_build_number;
    let workstation = v.w_product_type == 1; // VER_NT_WORKSTATION
    let name = match (major, minor) {
        (10, 0) if workstation && build >= 22000 => "Windows 11",
        (10, 0) if workstation => "Windows 10",
        (10, 0) if build >= 26100 => "Windows Server 2025",
        (10, 0) if build >= 17763 => "Windows Server 2019/2022",
        (10, 0) => "Windows Server 2016",
        (6, 3) if workstation => "Windows 8.1",
        (6, 3) => "Windows Server 2012 R2",
        (6, 2) if workstation => "Windows 8",
        (6, 2) => "Windows Server 2012",
        (6, 1) if workstation => "Windows 7",
        (6, 1) => "Windows Server 2008 R2",
        (6, 0) if workstation => "Windows Vista",
        (6, 0) => "Windows Server 2008",
        (5, 2) if workstation => "Windows XP x64",
        (5, 2) => "Windows Server 2003",
        (5, 1) => "Windows XP",
        _ => "Windows",
    };
    name.to_string()
}

fn version_rows() -> (String, String) {
    unsafe {
        let mut v: OsVersionInfoExW = mem::zeroed();
        v.dw_os_version_info_size = mem::size_of::<OsVersionInfoExW>() as u32;
        if RtlGetVersion(&mut v) != 0 {
            let mut v2: OsVersionInfoExW = mem::zeroed();
            v2.dw_os_version_info_size = v.dw_os_version_info_size;
            if GetVersionExW(&mut v2) != 0 {
                v = v2;
            }
        }
        let os = format!("{} ({})", windows_version_name(&v), ARCH);
        let kernel = format!(
            "Windows NT {}.{} (build {})",
            v.dw_major_version, v.dw_minor_version, v.dw_build_number
        );
        (os, kernel)
    }
}

fn format_uptime(ms: u64) -> String {
    let secs = ms / 1000;
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    let mut parts: Vec<String> = Vec::new();
    if days > 0 {
        parts.push(format!("{} day{}", days, if days == 1 { "" } else { "s" }));
    }
    if hours > 0 {
        parts.push(format!(
            "{} hour{}",
            hours,
            if hours == 1 { "" } else { "s" }
        ));
    }
    if mins > 0 || parts.is_empty() {
        parts.push(format!("{} min{}", mins, if mins == 1 { "" } else { "s" }));
    }
    parts.join(", ")
}

fn shell_name() -> String {
    if let Some(s) = env::var_os("SHELL") {
        let s = s.to_string_lossy().into_owned();
        if !s.trim().is_empty() {
            return basename(&s);
        }
    }
    if let Some(s) = env::var_os("COMSPEC") {
        let s = s.to_string_lossy().into_owned();
        if !s.trim().is_empty() {
            return basename(&s);
        }
    }
    "cmd.exe".to_string()
}

fn terminal_name() -> String {
    if let Some(p) = env::var_os("TERM_PROGRAM") {
        let p = p.to_string_lossy().into_owned();
        if !p.trim().is_empty() {
            if p.eq_ignore_ascii_case("Windows_Terminal") || p.eq_ignore_ascii_case("wt") {
                return "Windows Terminal".to_string();
            }
            return p;
        }
    }
    if env::var_os("WT_SESSION").is_some() {
        return "Windows Terminal".to_string();
    }
    if env::var_os("WEZTERM_PANE").is_some() || env::var_os("WEZTERM_EXECUTABLE").is_some() {
        return "WezTerm".to_string();
    }
    if env::var_os("ConEmuPID").is_some() || env::var_os("ConEmuTask").is_some() {
        return "ConEmu".to_string();
    }
    if let Some(t) = env::var_os("TERM") {
        let t = t.to_string_lossy().into_owned();
        if !t.trim().is_empty() {
            return t;
        }
    }
    "Windows Console".to_string()
}

fn cpu_name() -> String {
    unsafe {
        const CPU_KEY: &str = "HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0";
        let mut si: SystemInfo = mem::zeroed();
        GetSystemInfo(&mut si);
        let cores = si.dw_number_of_processors.max(1);
        let arch = match si.w_processor_architecture {
            0 => "x86",
            5 => "ARM",
            6 => "IA64",
            9 => "x64",
            12 => "ARM64",
            _ => "unknown",
        };
        let name = reg_read_string(CPU_KEY, "ProcessorNameString")
            .unwrap_or_default()
            .trim()
            .to_string();
        let name = if name.is_empty() {
            format!("Unknown CPU ({arch})")
        } else {
            name
        };
        let mut out = format!("{} ({})", name, cores);
        if let Some(mhz) = reg_read_u32(CPU_KEY, "~MHz") {
            if mhz > 0 {
                out.push_str(&format!(" @ {:.2} GHz", mhz as f64 / 1000.0));
            }
        }
        out
    }
}

fn gpu_names() -> Option<String> {
    const CLASS: &str =
        "SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e968-e325-11ce-bfc1-08002be10318}\\";
    let mut found: Vec<String> = Vec::new();
    for i in 0..4u32 {
        let sub = format!("{}{:04}", CLASS, i);
        if let Some(desc) = reg_read_string(&sub, "DriverDesc") {
            let desc = desc.trim().to_string();
            if !desc.is_empty() {
                found.push(desc);
            }
        }
    }
    if found.is_empty() {
        None
    } else {
        Some(found.join("; "))
    }
}

fn memory_info() -> String {
    unsafe {
        let mut m: MemoryStatusEx = mem::zeroed();
        m.dw_length = mem::size_of::<MemoryStatusEx>() as u32;
        if GlobalMemoryStatusEx(&mut m) == 0 {
            return "unavailable".to_string();
        }
        let used = m.ull_total_phys.saturating_sub(m.ull_avail_phys) as f64 / GIB;
        let total = m.ull_total_phys as f64 / GIB;
        format!("{:.2} GiB / {:.2} GiB ({}%)", used, total, m.dw_memory_load)
    }
}

fn disk_info() -> Vec<DiskInfo> {
    unsafe {
        let mask = GetLogicalDrives();
        let mut out = Vec::new();
        for i in 0..26u32 {
            if mask & (1u32 << i) == 0 {
                continue;
            }
            let letter = (b'A' + i as u8) as char;
            let root = wide(&format!("{}:\\", letter));
            if GetDriveTypeW(root.as_ptr()) != DRIVE_FIXED {
                continue;
            }
            let mut free_available = 0u64;
            let mut total = 0u64;
            let mut free_total = 0u64;
            if GetDiskFreeSpaceExW(
                root.as_ptr(),
                &mut free_available,
                &mut total,
                &mut free_total,
            ) == 0
                || total == 0
            {
                continue;
            }
            let used = total.saturating_sub(free_total);
            let percent = ((used as f64 / total as f64) * 100.0).round() as u32;
            out.push(DiskInfo {
                letter: format!("{}:", letter),
                total,
                free: free_total,
                used,
                percent,
            });
        }
        out
    }
}

fn local_ip() -> String {
    unsafe {
        let mut wsa = MaybeUninit::<WsaData>::uninit();
        if WSAStartup(0x0202, wsa.as_mut_ptr()) != 0 {
            return "127.0.0.1".to_string();
        }
        let mut result = "127.0.0.1".to_string();
        let s = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
        if s != INVALID_SOCKET {
            let mut target: SockAddrIn = mem::zeroed();
            target.sin_family = AF_INET as i16;
            target.sin_port = 0x3500u16; // htons(53) — bytes 00 35
            target.sin_addr = 0x0808_0808; // 8.8.8.8 in network byte order
            if connect(s, &target, mem::size_of::<SockAddrIn>() as i32) == 0 {
                let mut local: SockAddrIn = mem::zeroed();
                let mut len = mem::size_of::<SockAddrIn>() as i32;
                if getsockname(s, &mut local, &mut len) == 0 {
                    let b = local.sin_addr.to_ne_bytes();
                    result = format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]);
                }
            }
            let _ = closesocket(s);
        }
        let _ = WSACleanup();
        result
    }
}

fn collect_info() -> Info {
    let (os, kernel) = version_rows();
    Info {
        user: env::var("USERNAME").unwrap_or_else(|_| "user".to_string()),
        host: env::var("COMPUTERNAME").unwrap_or_else(|_| "host".to_string()),
        os,
        kernel,
        uptime: format_uptime(unsafe { GetTickCount64() }),
        shell: shell_name(),
        terminal: terminal_name(),
        cpu: cpu_name(),
        gpu: gpu_names(),
        memory: memory_info(),
        disks: disk_info(),
        local_ip: local_ip(),
    }
}

// ---------------------------------------------------------------------------
// HyFetch presets (vendored hex values)
// ---------------------------------------------------------------------------

type Preset = (&'static str, &'static [&'static str]);

/// The first three palettes are byte-for-byte the vendored rgb values from
/// src/hyfetch_presets.rs of the Linux crate. The others add "random" variety.
const PRESETS: &[Preset] = &[
    (
        "rainbow",
        &[
            "#E50000", "#FF8D00", "#FFEE00", "#028121", "#004CFF", "#770088",
        ],
    ),
    (
        "transgender",
        &["#55CDFD", "#F6AAB7", "#FFFFFF", "#F6AAB7", "#55CDFD"],
    ),
    (
        "progress",
        &[
            "#FFFFFF", "#FFAFC8", "#74D7EE", "#613915", "#000000", "#E50000",
            "#FF8D00", "#FFEE00", "#028121", "#004CFF", "#770088",
        ],
    ),
    (
        "nonbinary",
        &["#FCF431", "#FCFCFC", "#9D59D2", "#282828"],
    ),
    (
        "bisexual",
        &["#D60270", "#9B4F96", "#0038A8"],
    ),
    (
        "pansexual",
        &["#FF1C8D", "#FFD700", "#1AB3FF"],
    ),
    (
        "agender",
        &[
            "#000000", "#BABABA", "#FFFFFF", "#BAF484", "#FFFFFF", "#BABABA",
            "#000000",
        ],
    ),
    (
        "lesbian",
        &["#D62800", "#FF9B56", "#FFFFFF", "#D462A6", "#A40062"],
    ),
    (
        "asexual",
        &["#000000", "#A4A4A4", "#FFFFFF", "#810081"],
    ),
    (
        "aromantic",
        &["#3BA740", "#A8D47A", "#FFFFFF", "#ABABAB", "#000000"],
    ),
];

fn hex_rgb(s: &str) -> Option<Rgb> {
    let h = s.strip_prefix('#').unwrap_or(s);
    if h.len() != 6 {
        return None;
    }
    Some(Rgb(
        u8::from_str_radix(&h[0..2], 16).ok()?,
        u8::from_str_radix(&h[2..4], 16).ok()?,
        u8::from_str_radix(&h[4..6], 16).ok()?,
    ))
}

fn resolve_preset(name: &str) -> Option<&'static [Rgb]> {
    // Resolving into a 'static slice requires the converted palettes to live
    // forever; we use a small leaked cache (at most one entry per process run).
    use std::sync::OnceLock;
    static CACHE: OnceLock<Vec<(&'static str, Vec<Rgb>)>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        PRESETS
            .iter()
            .map(|(name, hexes)| {
                (
                    *name,
                    hexes.iter().filter_map(|h| hex_rgb(h)).collect::<Vec<_>>(),
                )
            })
            .collect()
    });

    let wanted = name.to_ascii_lowercase();
    let selected = if wanted == "random" {
        let tick = unsafe { GetTickCount64() };
        let idx = (tick.wrapping_mul(6364136223846793005).wrapping_add(1) >> 33) as usize;
        &cache[idx % cache.len()]
    } else {
        match cache.iter().find(|(n, _)| *n == wanted) {
            Some(entry) => entry,
            None => {
                eprintln!(
                    "rustfetch: unknown hyfetch preset '{}' (see --help); using raw colors",
                    name
                );
                return None;
            }
        }
    };
    if selected.1.is_empty() {
        None
    } else {
        Some(Box::leak(selected.1.clone().into_boxed_slice()))
    }
}

// ---------------------------------------------------------------------------
// Built-in logos
// ---------------------------------------------------------------------------

const WINDOWS_LOGO: &[&str] = &[
    "   .----------------.  ",
    "  /  ____________  \\  ",
    " |  |  ________  |  |  ",
    " |  | |        | |  |  ",
    " |  | |        | |  |  ",
    " |  | |________| |  |  ",
    " |  |____________|  |  ",
    "  \\________________/   ",
    "   '----------------'  ",
];

const GENERIC_LOGO: &[&str] = &[
    "      .--------.  ",
    "    .'          '. ",
    "   /   .----.     \\",
    "  |   /      \\     |",
    "  |  |  RF   |     |",
    "  |   \\______/     |",
    "   \\            .-' ",
    "    '.________.'    ",
];

/// Visible length (ANSI escape sequences do not count).
fn visible_len(s: &str) -> usize {
    let mut n = 0usize;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            n += 1;
        }
    }
    n
}

fn render_logo(name: &str, palette: Option<&[Rgb]>, color: bool) -> Vec<String> {
    if name.eq_ignore_ascii_case("none") || name.is_empty() {
        return Vec::new();
    }
    let art = if name.eq_ignore_ascii_case("windows") {
        WINDOWS_LOGO
    } else {
        if !name.eq_ignore_ascii_case("generic") {
            eprintln!(
                "rustfetch: unknown logo '{}', using 'generic' (try --logo none)",
                name
            );
        }
        GENERIC_LOGO
    };
    let count = palette.map_or(0, |p| p.len());
    art.iter()
        .enumerate()
        .map(|(i, line)| {
            if color && count > 0 {
                let idx = if art.len() <= 1 {
                    0
                } else {
                    i * (count - 1) / (art.len() - 1)
                };
                let c = &palette.unwrap()[idx];
                format!(
                    "\x1b[38;2;{};{};{}m{}\x1b[0m",
                    c.0, c.1, c.2, line
                )
            } else {
                (*line).to_string()
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tiny JSON(C) scanner for the fastfetch-compatible config
// ---------------------------------------------------------------------------

struct Scanner<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Scanner<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src: src.as_bytes(),
            pos: 0,
        }
    }

    fn ws(&mut self) {
        loop {
            while self.pos < self.src.len() && self.src[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
            if self.pos + 1 < self.src.len()
                && self.src[self.pos] == b'/'
                && self.src[self.pos + 1] == b'/'
            {
                self.pos += 2;
                while self.pos < self.src.len() && self.src[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            if self.pos + 1 < self.src.len()
                && self.src[self.pos] == b'/'
                && self.src[self.pos + 1] == b'*'
            {
                self.pos += 2;
                while self.pos + 1 < self.src.len()
                    && !(self.src[self.pos] == b'*' && self.src[self.pos + 1] == b'/')
                {
                    self.pos += 1;
                }
                self.pos = (self.pos + 2).min(self.src.len());
                continue;
            }
            break;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.src.get(self.pos).copied()
    }

    fn bump(&mut self) {
        self.pos += 1;
    }

    fn eat(&mut self, b: u8) -> bool {
        if self.peek() == Some(b) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let s = self.src.get(self.pos..self.pos + 4)?;
        let text = std::str::from_utf8(s).ok()?;
        self.pos += 4;
        u32::from_str_radix(text, 16).ok()
    }

    fn string(&mut self) -> Option<String> {
        if self.peek() != Some(b'"') {
            return None;
        }
        self.bump();
        let mut out = String::new();
        while let Some(&b) = self.src.get(self.pos) {
            self.pos += 1;
            match b {
                b'"' => return Some(out),
                b'\\' => {
                    let e = *self.src.get(self.pos)?;
                    self.pos += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let cp = self.hex4()?;
                            let ch = if (0xD800..=0xDBFF).contains(&cp) {
                                if self.src.get(self.pos..self.pos + 2)
                                    == Some(&[b'\\', b'u'][..])
                                {
                                    self.pos += 2;
                                    let lo = self.hex4()?;
                                    let full =
                                        0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                    char::from_u32(full).unwrap_or('\u{FFFD}')
                                } else {
                                    '\u{FFFD}'
                                }
                            } else {
                                char::from_u32(cp).unwrap_or('\u{FFFD}')
                            };
                            out.push(ch);
                        }
                        other => out.push(other as char),
                    }
                }
                b if b < 0x80 => out.push(b as char),
                _ => {
                    let start = self.pos - 1;
                    let rest = std::str::from_utf8(&self.src[start..]).ok()?;
                    let ch = rest.chars().next()?;
                    out.push(ch);
                    self.pos = start + ch.len_utf8();
                }
            }
        }
        None
    }

    fn skip_value(&mut self) {
        match self.peek() {
            Some(b'"') => {
                let _ = self.string();
            }
            Some(open @ (b'{' | b'[')) => {
                let close = if open == b'{' { b'}' } else { b']' };
                self.bump();
                let mut depth = 1usize;
                while depth > 0 {
                    match self.peek() {
                        None => break,
                        Some(c) if c == open => {
                            self.bump();
                            depth += 1;
                        }
                        Some(c) if c == close => {
                            self.bump();
                            depth -= 1;
                        }
                        Some(b'"') => {
                            let _ = self.string();
                        }
                        _ => self.bump(),
                    }
                }
            }
            _ => {
                while let Some(c) = self.peek() {
                    if c == b',' || c == b'}' || c == b']' || c.is_ascii_whitespace() {
                        break;
                    }
                    self.bump();
                }
            }
        }
    }
}

/// Consume an object and return its `"type"` string, if any.
fn object_type(s: &mut Scanner) -> Option<String> {
    if !s.eat(b'{') {
        return None;
    }
    let mut found = None;
    loop {
        let key = match s.peek() {
            Some(b'}') | None => {
                if s.peek().is_some() {
                    s.bump();
                }
                break;
            }
            _ => match s.string() {
                Some(k) => k,
                None => break,
            },
        };
        if !s.eat(b':') {
            break;
        }
        if key == "type" {
            found = s.string();
        } else {
            s.skip_value();
        }
        match s.peek() {
            Some(b',') => {
                s.bump();
            }
            Some(b'}') => {
                s.bump();
                break;
            }
            None => break,
            _ => s.bump(),
        }
    }
    found
}

fn parse_config(text: &str) -> (Vec<String>, Option<String>) {
    let mut s = Scanner::new(text.trim_start_matches('\u{feff}'));
    let mut modules = Vec::new();
    let mut preset = None;
    if !s.eat(b'{') {
        return (modules, preset);
    }
    loop {
        let key = match s.peek() {
            Some(b'}') | None => {
                if s.peek().is_some() {
                    s.bump();
                }
                break;
            }
            _ => match s.string() {
                Some(k) => k,
                None => break,
            },
        };
        if !s.eat(b':') {
            break;
        }
        match key.as_str() {
            "modules" => {
                if s.eat(b'[') {
                    loop {
                        match s.peek() {
                            Some(b']') | None => {
                                if s.peek().is_some() {
                                    s.bump();
                                }
                                break;
                            }
                            Some(b',') => {
                                s.bump();
                            }
                            Some(b'"') => {
                                if let Some(m) = s.string() {
                                    modules.push(m);
                                }
                            }
                            Some(b'{') => {
                                if let Some(m) = object_type(&mut s) {
                                    modules.push(m);
                                }
                            }
                            _ => s.skip_value(),
                        }
                    }
                } else {
                    s.skip_value();
                }
            }
            "logo" => {
                if s.eat(b'{') {
                    loop {
                        let logo_key = match s.peek() {
                            Some(b'}') | None => {
                                if s.peek().is_some() {
                                    s.bump();
                                }
                                break;
                            }
                            _ => match s.string() {
                                Some(k) => k,
                                None => break,
                            },
                        };
                        if !s.eat(b':') {
                            break;
                        }
                        if logo_key == "hyfetch" && s.peek() == Some(b'{') {
                            s.bump();
                            loop {
                                let hk = match s.peek() {
                                    Some(b'}') | None => {
                                        if s.peek().is_some() {
                                            s.bump();
                                        }
                                        break;
                                    }
                                    _ => match s.string() {
                                        Some(k) => k,
                                        None => break,
                                    },
                                };
                                if !s.eat(b':') {
                                    break;
                                }
                                if hk == "preset" {
                                    if s.peek() == Some(b'"') {
                                        preset = s.string();
                                    } else if s.eat(b'[') {
                                        loop {
                                            match s.peek() {
                                                Some(b']') | None => {
                                                    if s.peek().is_some() {
                                                        s.bump();
                                                    }
                                                    break;
                                                }
                                                Some(b',') => {
                                                    s.bump();
                                                }
                                                Some(b'"') => {
                                                    let v = s.string();
                                                    if preset.is_none() {
                                                        preset = v;
                                                    }
                                                }
                                                _ => s.skip_value(),
                                            }
                                        }
                                    } else {
                                        s.skip_value();
                                    }
                                } else {
                                    s.skip_value();
                                }
                            }
                        } else {
                            s.skip_value();
                        }
                    }
                } else {
                    s.skip_value();
                }
            }
            _ => s.skip_value(),
        }
        match s.peek() {
            Some(b',') => {
                s.bump();
            }
            Some(b'}') => {
                s.bump();
                break;
            }
            None => break,
            _ => s.bump(), // tolerate trailing commas / minor malformed input
        }
    }
    (modules, preset)
}

#[derive(Default)]
struct Config {
    modules: Vec<String>,
    preset: Option<String>,
}

fn load_config(explicit: Option<&str>) -> Config {
    let path = match explicit {
        Some(p) => PathBuf::from(p),
        None => match env::var_os("APPDATA") {
            Some(appdata) => PathBuf::from(appdata).join("fastfetch").join("config.jsonc"),
            None => return Config::default(),
        },
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Config::default(),
    };
    let (modules, preset) = parse_config(&text);
    Config {
        modules: normalize_modules(modules),
        preset,
    }
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

const ALL_MODULES: &[&str] = &[
    "title", "os", "kernel", "uptime", "shell", "terminal", "cpu", "gpu", "memory", "disk",
    "localip", "ip",
];

const DEFAULT_MODULES: &[&str] = &[
    "title", "os", "kernel", "uptime", "shell", "terminal", "cpu", "gpu", "memory", "disk",
    "localip",
];

fn normalize_modules(raw: Vec<String>) -> Vec<String> {
    raw.into_iter()
        .filter_map(|m| {
            let lower = m.to_ascii_lowercase();
            if ALL_MODULES.contains(&lower.as_str()) {
                Some(lower)
            } else {
                eprintln!("rustfetch: unknown module '{}' (see --list-modules)", m);
                None
            }
        })
        .collect()
}

#[derive(Default)]
struct Opts {
    pipe: bool,
    json: bool,
    list_modules: bool,
    help: bool,
    version: bool,
    preset: Option<String>,
    logo: Option<String>,
    config: Option<String>,
    modules: Vec<String>,
}

impl Opts {
    fn parse() -> Result<Self, String> {
        let mut o = Opts::default();
        let args: Vec<String> = env::args().skip(1).collect();
        let mut i = 0usize;
        while i < args.len() {
            let arg = &args[i];
            let (flag, inline) = match arg.split_once('=') {
                Some((f, v)) => (f, Some(v.to_string())),
                None => (arg.as_str(), None),
            };
            let mut take_value = |name: &str| -> Result<String, String> {
                if let Some(v) = inline.clone() {
                    if !v.is_empty() {
                        return Ok(v);
                    }
                }
                i += 1;
                args.get(i)
                    .cloned()
                    .ok_or_else(|| format!("{} requires a value", name))
            };
            match flag {
                "--pipe" => o.pipe = true,
                "--json" => o.json = true,
                "--format" => {
                    let v = take_value("--format")?;
                    if v.eq_ignore_ascii_case("json") {
                        o.json = true;
                    } else {
                        return Err(format!("unsupported --format '{}' (try json)", v));
                    }
                }
                "--list-modules" => o.list_modules = true,
                "--help" | "-h" => o.help = true,
                "--version" | "-V" => o.version = true,
                "--hyfetch-preset" => o.preset = Some(take_value("--hyfetch-preset")?),
                "--logo" => o.logo = Some(take_value("--logo")?),
                "--config" => o.config = Some(take_value("--config")?),
                _ if arg.starts_with('-') => return Err(format!("unknown flag '{}'", arg)),
                _ => o.modules.push(arg.clone()),
            }
            i += 1;
        }
        o.modules = normalize_modules(o.modules);
        Ok(o)
    }
}

fn short_help() {
    eprintln!("Try 'rustfetch --help' for more information.");
}

fn print_help() {
    println!(
        "rustfetch {} ({}) — system information for Windows\n\
         \n\
         USAGE:\n\
         \x20   rustfetch [OPTIONS] [MODULE ...]\n\
         \n\
         OPTIONS:\n\
         \x20   --pipe                    no ANSI colors or escapes\n\
         \x20   --format json             print one JSON object (same as --json)\n\
         \x20   --list-modules            list valid module names\n\
         \x20   --logo NAME|none          built-in logo: windows, generic, none\n\
         \x20   --hyfetch-preset NAME     rainbow, transgender, progress, random,\n\
         \x20                             nonbinary, bisexual, pansexual, agender,\n\
         \x20                             lesbian, asexual, aromantic\n\
         \x20   --config PATH             read a JSONC config (default:\n\
         \x20                             %APPDATA%\\fastfetch\\config.jsonc)\n\
         \x20   --help, -h                show this help\n\
         \x20   --version, -V             print version and architecture\n\
         \n\
         MODULES (when given, only these rows are printed, in this order):\n\
         \x20   title os kernel uptime shell terminal cpu gpu memory disk localip\n\
         \n\
         Flags accept both `--flag value` and `--flag=value`. Colors follow\n\
         NO_COLOR and are disabled when stdout is not a console.\n",
        "0.2.5", ARCH
    );
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_text_rows(modules: &[String], info: &Info) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    for m in modules {
        match m.as_str() {
            "title" => {
                let title = format!("{}@{}", info.user, info.host);
                rows.push(title.clone());
                rows.push("-".repeat(title.chars().count()));
            }
            "os" => rows.push(format!("OS: {}", info.os)),
            "kernel" => rows.push(format!("Kernel: {}", info.kernel)),
            "uptime" => rows.push(format!("Uptime: {}", info.uptime)),
            "shell" => rows.push(format!("Shell: {}", info.shell)),
            "terminal" => rows.push(format!("Terminal: {}", info.terminal)),
            "cpu" => rows.push(format!("CPU: {}", info.cpu)),
            "gpu" => {
                if let Some(gpu) = &info.gpu {
                    rows.push(format!("GPU: {}", gpu));
                }
            }
            "memory" => rows.push(format!("Memory: {}", info.memory)),
            "disk" => {
                for d in &info.disks {
                    rows.push(format!(
                        "Disk ({}): {:.2} GiB / {:.2} GiB ({}%)",
                        d.letter,
                        d.used as f64 / GIB,
                        d.total as f64 / GIB,
                        d.percent
                    ));
                }
            }
            "localip" | "ip" => rows.push(format!("Local IP: {}", info.local_ip)),
            _ => {}
        }
    }
    rows
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn jstr(s: &str) -> String {
    format!("\"{}\"", json_escape(s))
}

fn render_json(modules: &[String], info: &Info) -> String {
    let mut fields: Vec<String> = Vec::new();
    for m in modules {
        match m.as_str() {
            "title" => fields.push(format!(
                "\"title\": {}",
                jstr(&format!("{}@{}", info.user, info.host))
            )),
            "os" => fields.push(format!("\"os\": {}", jstr(&info.os))),
            "kernel" => fields.push(format!("\"kernel\": {}", jstr(&info.kernel))),
            "uptime" => fields.push(format!("\"uptime\": {}", jstr(&info.uptime))),
            "shell" => fields.push(format!("\"shell\": {}", jstr(&info.shell))),
            "terminal" => fields.push(format!("\"terminal\": {}", jstr(&info.terminal))),
            "cpu" => fields.push(format!("\"cpu\": {}", jstr(&info.cpu))),
            "gpu" => {
                if let Some(gpu) = &info.gpu {
                    fields.push(format!("\"gpu\": {}", jstr(gpu)));
                }
            }
            "memory" => fields.push(format!("\"memory\": {}", jstr(&info.memory))),
            "disk" => {
                let entries: Vec<String> = info
                    .disks
                    .iter()
                    .map(|d| {
                        format!(
                            "{{\"drive\": {}, \"total\": {}, \"free\": {}, \"used\": {}}}",
                            jstr(&d.letter),
                            d.total,
                            d.free,
                            d.used
                        )
                    })
                    .collect();
                fields.push(format!("\"disks\": [{}]", entries.join(", ")));
            }
            "localip" | "ip" => {
                fields.push(format!("\"localIp\": {}", jstr(&info.local_ip)))
            }
            _ => {}
        }
    }
    format!("{{\n  {}\n}}", fields.join(",\n  "))
}

fn print_columns(logo: Vec<String>, rows: Vec<String>) {
    let has_logo = !logo.is_empty();
    let logo_width = logo.iter().map(|l| visible_len(l)).max().unwrap_or(0);
    let count = logo.len().max(rows.len());
    let mut out = String::new();
    for i in 0..count {
        if has_logo {
            let left = logo.get(i).map(String::as_str).unwrap_or("");
            out.push_str(left);
            let pad = logo_width.saturating_sub(visible_len(left));
            for _ in 0..pad {
                out.push(' ');
            }
            out.push_str("    ");
        }
        if let Some(right) = rows.get(i) {
            out.push_str(right);
        }
        out.push('\n');
    }
    print!("{}", out);
    let _ = io::stdout().flush();
}

fn main() {
    let opts = match Opts::parse() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("rustfetch: {}", e);
            short_help();
            std::process::exit(2);
        }
    };

    if opts.help {
        print_help();
        return;
    }
    if opts.version {
        println!("rustfetch 0.2.5 ({})", ARCH);
        return;
    }
    if opts.list_modules {
        for module in ALL_MODULES {
            println!("{}", module);
        }
        return;
    }

    let config = load_config(opts.config.as_deref());
    let modules = if !opts.modules.is_empty() {
        opts.modules.clone()
    } else if !config.modules.is_empty() {
        config.modules.clone()
    } else {
        DEFAULT_MODULES.iter().map(|m| (*m).to_string()).collect()
    };

    let preset_name = opts.preset.clone().or(config.preset.clone());
    let palette = preset_name
        .as_deref()
        .and_then(|name| resolve_preset(name));
    let logo_name = opts.logo.clone().unwrap_or_else(|| "windows".to_string());

    let color = !opts.pipe
        && !opts.json
        && env::var_os("NO_COLOR").is_none()
        && enable_ansi();
    let info = collect_info();

    if opts.json {
        println!("{}", render_json(&modules, &info));
        return;
    }

    let logo = render_logo(&logo_name, palette, color);
    let rows = render_text_rows(&modules, &info);
    print_columns(logo, rows);
}

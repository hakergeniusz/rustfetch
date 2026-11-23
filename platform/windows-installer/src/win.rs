//! Windows-only implementation: WinHTTP download, pure-Rust SHA-256 check,
//! per-user PATH editing through advapi32, and WM_SETTINGCHANGE broadcast.
//!
//! Every external function is declared here directly with `extern "system"`;
//! there are no third-party crates. This whole module is cfg-gated out on
//! non-Windows targets so `cargo test` still works on Linux.

use std::env;
use std::ffi::c_void;
use std::fs;
use std::io;
use std::path::Path;
use std::ptr::{null, null_mut};

use crate::cli::Options;
use crate::{env as envutil, sha256, urls};

// ---------------------------------------------------------------------------
// WinHTTP
// ---------------------------------------------------------------------------

const WINHTTP_ACCESS_TYPE_DEFAULT_PROXY: u32 = 0;
const WINHTTP_FLAG_SECURE: u32 = 0x0080_0000;
const WINHTTP_QUERY_STATUS_CODE: u32 = 19;
const WINHTTP_QUERY_FLAG_NUMBER: u32 = 0x2000_0000;
const HTTP_OK: u32 = 200;

#[link(name = "winhttp")]
extern "system" {
    fn WinHttpOpen(
        psz_user_agent: *const u16,
        dw_access_type: u32,
        psz_proxy: *const u16,
        psz_proxy_bypass: *const u16,
        dw_flags: u32,
    ) -> *mut c_void;
    fn WinHttpConnect(
        session: *mut c_void,
        server_name: *const u16,
        server_port: u16,
        reserved: u32,
    ) -> *mut c_void;
    fn WinHttpOpenRequest(
        connect: *mut c_void,
        verb: *const u16,
        object_name: *const u16,
        version: *const u16,
        referrer: *const u16,
        accept_types: *const *const u16,
        flags: u32,
    ) -> *mut c_void;
    fn WinHttpSendRequest(
        request: *mut c_void,
        headers: *const u16,
        headers_length: u32,
        optional: *const c_void,
        optional_length: u32,
        total_length: u32,
        context: usize,
    ) -> i32;
    fn WinHttpReceiveResponse(request: *mut c_void, reserved: *mut c_void) -> i32;
    fn WinHttpQueryHeaders(
        request: *mut c_void,
        info_level: u32,
        name: *const u16,
        buffer: *mut c_void,
        buffer_length: *mut u32,
        index: *mut u32,
    ) -> i32;
    fn WinHttpQueryDataAvailable(request: *mut c_void, available: *mut u32) -> i32;
    fn WinHttpReadData(
        request: *mut c_void,
        buffer: *mut c_void,
        to_read: u32,
        read: *mut u32,
    ) -> i32;
    fn WinHttpCloseHandle(handle: *mut c_void) -> i32;
}

/// RAII wrapper so every handle is closed on success *and* on early return.
struct HttpHandle(*mut c_void);

impl Drop for HttpHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn os_error(context: &str) -> String {
    format!("{}: {}", context, io::Error::last_os_error())
}

/// Download `https://HOST/path` over HTTPS and return its bytes.
fn http_get(host: &str, path: &str) -> Result<Vec<u8>, String> {
    let agent = wide("rustfetch-setup/1.0");
    let host_w = wide(host);
    let path_w = wide(path);
    let verb = wide("GET");

    let session = HttpHandle(unsafe {
        WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            null(),
            null(),
            0,
        )
    });
    if session.0.is_null() {
        return Err(os_error("WinHttpOpen failed"));
    }

    let connection = HttpHandle(unsafe { WinHttpConnect(session.0, host_w.as_ptr(), 443, 0) });
    if connection.0.is_null() {
        return Err(os_error("WinHttpConnect failed"));
    }

    let request = HttpHandle(unsafe {
        WinHttpOpenRequest(
            connection.0,
            verb.as_ptr(),
            path_w.as_ptr(),
            null(),
            null(),
            null(),
            WINHTTP_FLAG_SECURE,
        )
    });
    if request.0.is_null() {
        return Err(os_error("WinHttpOpenRequest failed"));
    }

    if unsafe { WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0) } == 0 {
        return Err(os_error("WinHttpSendRequest failed"));
    }
    if unsafe { WinHttpReceiveResponse(request.0, null_mut()) } == 0 {
        return Err(os_error("WinHttpReceiveResponse failed"));
    }

    let mut status: u32 = 0;
    let mut status_len: u32 = std::mem::size_of::<u32>() as u32;
    if unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            &mut status as *mut u32 as *mut c_void,
            &mut status_len,
            null_mut(),
        )
    } == 0
    {
        return Err(os_error("Could not read the HTTP status code"));
    }
    if status != HTTP_OK {
        return Err(format!(
            "HTTP {} while downloading https://{}{}. Check the tag and your network connection.",
            status, host, path
        ));
    }

    let mut bytes: Vec<u8> = Vec::new();
    loop {
        let mut available: u32 = 0;
        if unsafe { WinHttpQueryDataAvailable(request.0, &mut available) } == 0 {
            return Err(os_error("WinHttpQueryDataAvailable failed"));
        }
        if available == 0 {
            break;
        }
        let mut chunk = vec![0u8; available as usize];
        let mut read: u32 = 0;
        if unsafe {
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr() as *mut c_void,
                available,
                &mut read,
            )
        } == 0
        {
            return Err(os_error("WinHttpReadData failed"));
        }
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read as usize]);
    }
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// Registry (HKCU\Environment only)
// ---------------------------------------------------------------------------

const KEY_READ: u32 = 0x0002_0019;
const KEY_WRITE: u32 = 0x0002_0006;
const REG_SZ: u32 = 1;
const ERROR_FILE_NOT_FOUND: i32 = 2;

/// Windows defines `HKEY_CURRENT_USER` as
/// `((HKEY)(ULONG_PTR)((LONG)0x80000001))` - the sign extension to pointer
/// width matters on x86_64, otherwise RegOpenKeyExW reports an invalid handle.
fn hkey_current_user() -> *mut c_void {
    (-2_147_483_647_i32 as isize) as *mut c_void
}

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(
        key: *mut c_void,
        sub_key: *const u16,
        options: u32,
        desired: u32,
        result: *mut *mut c_void,
    ) -> i32;
    fn RegQueryValueExW(
        key: *mut c_void,
        value_name: *const u16,
        reserved: *mut u32,
        value_type: *mut u32,
        data: *mut u8,
        data_len: *mut u32,
    ) -> i32;
    fn RegSetValueExW(
        key: *mut c_void,
        value_name: *const u16,
        reserved: u32,
        value_type: u32,
        data: *const u8,
        data_len: u32,
    ) -> i32;
    fn RegCloseKey(key: *mut c_void) -> i32;
}

struct RegHandle(*mut c_void);

impl Drop for RegHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }
}

fn open_environment_key() -> Result<RegHandle, String> {
    let subkey = wide("Environment");
    let mut key: *mut c_void = null_mut();
    let rc = unsafe {
        RegOpenKeyExW(
            hkey_current_user(),
            subkey.as_ptr(),
            0,
            KEY_READ | KEY_WRITE,
            &mut key,
        )
    };
    if rc != 0 {
        return Err(format!("Could not open HKCU\\Environment (error {}).", rc));
    }
    Ok(RegHandle(key))
}

/// Read the user `Path` value. Returns the value and its registry type so an
/// existing `REG_EXPAND_SZ` is preserved when we write it back.
fn read_user_path(key: *mut c_void) -> Result<(String, u32), String> {
    let name = wide("Path");
    let mut value_type: u32 = 0;
    let mut size: u32 = 0;
    let rc = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            null_mut(),
            &mut value_type,
            null_mut(),
            &mut size,
        )
    };
    if rc == ERROR_FILE_NOT_FOUND {
        return Ok((String::new(), REG_SZ));
    }
    if rc != 0 {
        return Err(format!("Could not query the user Path value (error {}).", rc));
    }
    if size == 0 {
        return Ok((String::new(), if value_type == 0 { REG_SZ } else { value_type }));
    }

    let words = (size as usize / 2) + 1;
    let mut buffer = vec![0u16; words];
    let mut actual = size;
    let rc = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            null_mut(),
            &mut value_type,
            buffer.as_mut_ptr() as *mut u8,
            &mut actual,
        )
    };
    if rc != 0 {
        return Err(format!("Could not read the user Path value (error {}).", rc));
    }
    let usable = (actual as usize / 2).min(buffer.len());
    let mut end = usable;
    while end > 0 && buffer[end - 1] == 0 {
        end -= 1;
    }
    Ok((
        String::from_utf16_lossy(&buffer[..end]),
        if value_type == 0 { REG_SZ } else { value_type },
    ))
}

fn write_user_path(key: *mut c_void, value: &str, value_type: u32) -> Result<(), String> {
    let name = wide("Path");
    let data = wide(value);
    let rc = unsafe {
        RegSetValueExW(
            key,
            name.as_ptr(),
            0,
            value_type,
            data.as_ptr() as *const u8,
            (data.len() * 2) as u32,
        )
    };
    if rc != 0 {
        return Err(format!("Could not write the user Path value (error {}).", rc));
    }
    Ok(())
}

/// Normalise a PATH entry for comparison: expand variables, drop quotes and
/// trailing separators, compare case-insensitively (Windows paths are
/// case-insensitive).
fn normalized_entry(entry: &str) -> String {
    let trimmed = entry.trim().trim_matches('"').trim();
    let expanded = envutil::expand(trimmed, |name| env::var(name).ok());
    expanded
        .trim()
        .trim_matches('"')
        .trim_end_matches('\\')
        .trim_end_matches('/')
        .to_lowercase()
}

fn add_to_user_path(dir: &str) -> Result<(), String> {
    let key = open_environment_key()?;
    let (path, value_type) = read_user_path(key.0)?;
    if path.split(';').any(|entry| normalized_entry(entry) == normalized_entry(dir)) {
        println!("[rustfetch-setup]   User PATH already contains the install directory.");
        return Ok(());
    }
    let updated = if path.trim().is_empty() {
        dir.to_string()
    } else {
        format!("{};{}", path.trim_end_matches(';'), dir)
    };
    write_user_path(key.0, &updated, value_type)?;
    broadcast_setting_change();
    Ok(())
}

fn remove_from_user_path(dir: &str) -> Result<(), String> {
    let key = open_environment_key()?;
    let (path, value_type) = read_user_path(key.0)?;
    if path.is_empty() {
        return Ok(());
    }
    let target = normalized_entry(dir);
    let kept: Vec<&str> = path
        .split(';')
        .filter(|entry| !entry.trim().is_empty() && normalized_entry(entry) != target)
        .collect();
    let updated = kept.join(";");
    write_user_path(key.0, &updated, value_type)?;
    broadcast_setting_change();
    Ok(())
}

// ---------------------------------------------------------------------------
// WM_SETTINGCHANGE broadcast (user32)
// ---------------------------------------------------------------------------

const HWND_BROADCAST_VALUE: usize = 0xffff;
const WM_SETTINGCHANGE: u32 = 0x001a;
const SMTO_ABORTIFHUNG: u32 = 0x0002;

#[link(name = "user32")]
extern "system" {
    fn SendMessageTimeoutW(
        window: *mut c_void,
        message: u32,
        w_param: usize,
        l_param: *const c_void,
        flags: u32,
        timeout: u32,
        result: *mut usize,
    ) -> isize;
}

/// Tell running applications to re-read the environment. Failure here is not
/// fatal: the registry value is already correct and new processes inherit it.
fn broadcast_setting_change() {
    let area = wide("Environment");
    let mut result: usize = 0;
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST_VALUE as *mut c_void,
            WM_SETTINGCHANGE,
            0,
            area.as_ptr() as *const c_void,
            SMTO_ABORTIFHUNG,
            2000,
            &mut result,
        );
    }
}

// ---------------------------------------------------------------------------
// Install / uninstall orchestration
// ---------------------------------------------------------------------------

fn resolve_install_dir(input: Option<&str>) -> Result<String, String> {
    let raw = match input {
        Some(value) if !value.trim().is_empty() => envutil::expand(value, |name| env::var(name).ok()),
        _ => {
            let local = env::var("LOCALAPPDATA")
                .ok()
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    env::var("USERPROFILE")
                        .ok()
                        .map(|profile| format!("{}\\AppData\\Local", profile))
                })
                .ok_or_else(|| {
                    "Could not determine LOCALAPPDATA; please pass --install-dir DIR.".to_string()
                })?;
            format!("{}\\Programs\\rustfetch", local.trim_end_matches('\\'))
        }
    };
    let trimmed = raw
        .trim()
        .trim_end_matches('\\')
        .trim_end_matches('/')
        .to_string();
    if trimmed.is_empty() {
        return Err("The install directory is empty.".to_string());
    }
    Ok(trimmed)
}

fn install(opts: &Options) -> Result<(), String> {
    println!("[rustfetch-setup] rustfetch installer");
    let install_dir = resolve_install_dir(opts.install_dir.as_deref())?;
    let exe_path = Path::new(&install_dir).join("rustfetch.exe");
    println!("[rustfetch-setup] Install directory: {}", install_dir);

    let base_path = urls::release_base_path(opts.version.as_deref());
    let asset_path = format!("{}/{}", base_path, urls::ASSET_NAME);
    let sidecar_path = format!("{}/{}.sha256", base_path, urls::ASSET_NAME);

    println!(
        "[rustfetch-setup] Downloading {}",
        urls::asset_url(opts.version.as_deref(), urls::ASSET_NAME)
    );
    let binary = http_get(urls::HOST, &asset_path)?;
    println!("[rustfetch-setup]   downloaded {} bytes", binary.len());

    println!("[rustfetch-setup] Downloading SHA-256 sidecar");
    let sidecar = http_get(urls::HOST, &sidecar_path)?;
    let sidecar_text = String::from_utf8_lossy(&sidecar);
    let expected = sha256::parse_sidecar_hash(&sidecar_text).ok_or_else(|| {
        format!(
            "The SHA-256 sidecar at https://{}{} is missing or malformed. Nothing was installed.",
            urls::HOST, sidecar_path
        )
    })?;
    let actual = sha256::to_hex(&sha256::sha256(&binary));
    if !expected.eq_ignore_ascii_case(&actual) {
        return Err(format!(
            "SHA-256 mismatch for {}: expected {}, got {}. Nothing was installed.",
            urls::ASSET_NAME, expected, actual
        ));
    }
    println!("[rustfetch-setup]   SHA-256 verified ({})", actual);

    fs::create_dir_all(&install_dir)
        .map_err(|err| format!("Could not create {}: {}", install_dir, err))?;
    fs::write(&exe_path, &binary).map_err(|err| {
        format!(
            "Could not write {} (close any running rustfetch instance and check permissions): {}",
            exe_path.display(),
            err
        )
    })?;
    println!("[rustfetch-setup] Installed {}", exe_path.display());

    if opts.no_path {
        println!("[rustfetch-setup] --no-path given: skipping user PATH update.");
    } else {
        println!("[rustfetch-setup] Adding the install directory to the user PATH");
        add_to_user_path(&install_dir)?;
    }

    println!("[rustfetch-setup] Done. Open a new terminal and run `rustfetch`.");
    Ok(())
}

fn uninstall(opts: &Options) -> Result<(), String> {
    println!("[rustfetch-setup] rustfetch uninstaller");
    let install_dir = resolve_install_dir(opts.install_dir.as_deref())?;
    let exe_path = Path::new(&install_dir).join("rustfetch.exe");

    match fs::remove_file(&exe_path) {
        Ok(()) => println!("[rustfetch-setup] Removed {}", exe_path.display()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            println!("[rustfetch-setup] {} was not present; nothing to remove.", exe_path.display());
        }
        Err(err) => {
            return Err(format!(
                "Could not remove {} (close any running rustfetch instance): {}",
                exe_path.display(),
                err
            ));
        }
    }

    if opts.no_path {
        println!("[rustfetch-setup] --no-path given: leaving user PATH untouched.");
    } else {
        println!("[rustfetch-setup] Removing the install directory from the user PATH");
        remove_from_user_path(&install_dir)?;
    }

    println!("[rustfetch-setup] Uninstall complete.");
    Ok(())
}

/// Entry point used by `main` on Windows.
pub fn run(opts: &Options) -> Result<(), String> {
    if opts.uninstall {
        uninstall(opts)
    } else {
        install(opts)
    }
}

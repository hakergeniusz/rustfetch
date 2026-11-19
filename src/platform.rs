//! Process and platform identity, ported from FFPlatform_unix.c: the passwd
//! fields and paths the title module serializes into JSON, and the uname
//! sysinfo the kernel/OS modules use. Computed once, read from all module
//! threads.

use std::ffi::CStr;
use std::sync::OnceLock;

pub struct Sysinfo {
    pub name: String,
    pub release: String,
    pub version: String,
    pub architecture: String,
    pub page_size: u64,
}

pub struct Platform {
    pub pid: u32,
    pub uid: u32,
    pub user_name: String,
    pub full_user_name: String,
    pub host_name: String,
    /// pw_dir (or $HOME), guaranteed to end with '/'.
    pub home_dir: String,
    /// /proc/self/exe resolved through realpath().
    pub exe_path: String,
    pub user_shell: String,
    /// getcwd(), guaranteed to end with '/'.
    pub cwd: String,
    pub sysinfo: Sysinfo,
}

/// read_strbuf of a fixed utsname/passwd char array field.
unsafe fn c_array_str(pointer: *const libc::c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(pointer).to_string_lossy().into_owned() }
}

/// ffStrbufTrimSpace: leading and trailing whitespace.
fn trim_space(text: &str) -> String {
    text.trim().to_string()
}

fn init() -> Platform {
    let pid = std::process::id();
    let uid = unsafe { libc::getuid() };
    // getpwuid returns a pointer into static storage; copy the fields now.
    let pwd = unsafe {
        let pointer = libc::getpwuid(uid);
        if pointer.is_null() {
            None
        } else {
            Some((
                c_array_str((*pointer).pw_name),
                trim_space(&c_array_str((*pointer).pw_gecos)),
                c_array_str((*pointer).pw_dir),
                c_array_str((*pointer).pw_shell),
            ))
        }
    };

    let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut uts) } != 0 {
        uts = unsafe { std::mem::zeroed() };
    }
    let (sysname, nodename, release, version, machine) = unsafe {
        (
            c_array_str(uts.sysname.as_ptr()),
            c_array_str(uts.nodename.as_ptr()),
            c_array_str(uts.release.as_ptr()),
            c_array_str(uts.version.as_ptr()),
            c_array_str(uts.machine.as_ptr()),
        )
    };

    // getExePath: /proc/self/exe through realpath(), falling back to the
    // raw link target when realpath fails (dangling symlink).
    let exe_path = std::fs::read_link("/proc/self/exe")
        .map(|path| {
            std::fs::canonicalize(&path)
                .map(|resolved| resolved.to_string_lossy().into_owned())
                .unwrap_or_else(|_| path.to_string_lossy().into_owned())
        })
        .unwrap_or_default();

    let mut home_dir = pwd
        .as_ref()
        .map(|(_, _, dir, _)| dir.clone())
        .filter(|dir| !dir.is_empty())
        .unwrap_or_else(|| std::env::var("HOME").unwrap_or_default());
    if !home_dir.ends_with('/') {
        home_dir.push('/');
    }

    let mut cwd = std::env::current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !cwd.is_empty() && !cwd.ends_with('/') {
        cwd.push('/');
    }

    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
    let sysinfo = Sysinfo {
        name: sysname,
        release,
        version,
        architecture: machine,
        // Upstream stores pageSizeShift = ctz(pagesize) and serializes
        // `1 << shift`; page sizes are powers of two, so that round-trips
        // to the page size itself.
        page_size,
    };

    Platform {
        pid,
        uid,
        user_name: pwd
            .as_ref()
            .map(|(name, _, _, _)| name.clone())
            .unwrap_or_else(|| std::env::var("USER").unwrap_or_default()),
        full_user_name: pwd.as_ref().map(|(_, gecos, _, _)| gecos.clone()).unwrap_or_default(),
        host_name: nodename,
        home_dir,
        exe_path,
        user_shell: pwd
            .as_ref()
            .map(|(_, _, _, shell)| shell.clone())
            .unwrap_or_else(|| std::env::var("SHELL").unwrap_or_default()),
        cwd,
        sysinfo,
    }
}

pub fn platform() -> &'static Platform {
    static PLATFORM: OnceLock<Platform> = OnceLock::new();
    PLATFORM.get_or_init(init)
}

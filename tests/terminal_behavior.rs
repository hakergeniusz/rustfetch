#![cfg(unix)]

//! End-to-end regression tests for the `--disable-linewrap` fix.
//!
//! These drive the real binary through a PTY so `isatty(stdout)` is true and
//! the renderer's `\e[?7l` / `\e[?7h` toggles are observable. A plain pipe run
//! is the control case. The base arguments only isolate the run from the
//! user's config and keep the output short; none of them set the linewrap
//! option, so the default resolution path is still what gets tested.

use std::ffi::{CStr, CString, c_char, c_int};
use std::process::Command;
use std::time::{Duration, Instant};

const DISABLE_WRAP: &[u8] = b"\x1b[?7l";
const RESTORE_WRAP: &[u8] = b"\x1b[?7h";

/// Two known module rows give us a first and a last line to bracket.
const BASE_ARGS: &[&str] = &["--config", "none", "--logo", "none", "--structure", "OS:Kernel"];

fn cstring(value: &str) -> Result<CString, String> {
    CString::new(value).map_err(|error| format!("CString::new({value:?}): {error}"))
}

fn position(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// An owned PTY master plus its child. Dropping it closes the master (which
/// hangs up the slave) and always reaps the child, even on a test panic.
struct PtyChild {
    master: c_int,
    child: libc::pid_t,
}

impl PtyChild {
    fn spawn(args: &[&str], rows: u16, cols: u16) -> Result<Self, String> {
        let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
        if master < 0 {
            return Err(format!("posix_openpt: {}", std::io::Error::last_os_error()));
        }
        match Self::spawn_on(master, args, rows, cols) {
            Ok(child) => Ok(child),
            Err(error) => {
                unsafe { libc::close(master) };
                Err(error)
            },
        }
    }

    fn spawn_on(master: c_int, args: &[&str], rows: u16, cols: u16) -> Result<Self, String> {
        unsafe {
            if libc::grantpt(master) != 0 {
                return Err(format!("grantpt: {}", std::io::Error::last_os_error()));
            }
            if libc::unlockpt(master) != 0 {
                return Err(format!("unlockpt: {}", std::io::Error::last_os_error()));
            }
            let name_ptr = libc::ptsname(master);
            if name_ptr.is_null() {
                return Err("ptsname returned null".to_string());
            }
            let name = CStr::from_ptr(name_ptr)
                .to_str()
                .map_err(|error| format!("ptsname is not UTF-8: {error}"))?;
            let slave = libc::open(name.as_ptr().cast(), libc::O_RDWR);
            if slave < 0 {
                return Err(format!("open(slave): {}", std::io::Error::last_os_error()));
            }

            let winsize = libc::winsize {
                ws_row: rows,
                ws_col: cols,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            if libc::ioctl(master, libc::TIOCSWINSZ, &winsize) != 0 {
                libc::close(slave);
                return Err(format!("ioctl(TIOCSWINSZ): {}", std::io::Error::last_os_error()));
            }

            let mut argv = Vec::with_capacity(args.len() + 1);
            argv.push(cstring(env!("CARGO_BIN_EXE_rustfetch"))?);
            for arg in args {
                argv.push(cstring(arg)?);
            }
            let mut argv_ptrs: Vec<*const c_char> = argv.iter().map(|text| text.as_ptr()).collect();
            argv_ptrs.push(std::ptr::null());

            let child = libc::fork();
            if child < 0 {
                libc::close(slave);
                return Err(format!("fork: {}", std::io::Error::last_os_error()));
            }
            if child == 0 {
                libc::close(master);
                if libc::setsid() < 0 || libc::ioctl(slave, libc::TIOCSCTTY, 0usize) != 0 {
                    libc::_exit(125);
                }
                if libc::dup2(slave, libc::STDIN_FILENO) < 0
                    || libc::dup2(slave, libc::STDOUT_FILENO) < 0
                    || libc::dup2(slave, libc::STDERR_FILENO) < 0
                {
                    libc::_exit(125);
                }
                if slave > libc::STDERR_FILENO {
                    libc::close(slave);
                }
                // A caller's NO_COLOR would force pipe mode even on a tty.
                libc::unsetenv(c"NO_COLOR".as_ptr());
                libc::execv(argv_ptrs[0], argv_ptrs.as_ptr());
                libc::_exit(127);
            }

            libc::close(slave);
            Ok(Self { master, child })
        }
    }

    /// Poll with a hard deadline, then drain until the slave closes. Linux
    /// reports a closed slave as EIO on the master, which is EOF here.
    fn read_all(&self, timeout: Duration) -> Result<Vec<u8>, String> {
        let flags = unsafe { libc::fcntl(self.master, libc::F_GETFL) };
        if flags < 0 {
            return Err(format!("fcntl(F_GETFL): {}", std::io::Error::last_os_error()));
        }
        if unsafe { libc::fcntl(self.master, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(format!("fcntl(F_SETFL): {}", std::io::Error::last_os_error()));
        }

        let deadline = Instant::now() + timeout;
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!("PTY read timed out after {} bytes", bytes.len()));
            }
            let mut pollfd = libc::pollfd {
                fd: self.master,
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut pollfd, 1, remaining.as_millis().min(200) as c_int) };
            if ready < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(format!("poll: {error}"));
            }
            if ready == 0 {
                continue;
            }
            loop {
                let count = unsafe { libc::read(self.master, chunk.as_mut_ptr().cast(), chunk.len()) };
                if count > 0 {
                    bytes.extend_from_slice(&chunk[..count as usize]);
                    continue;
                }
                if count == 0 {
                    return Ok(bytes);
                }
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    break;
                }
                if error.raw_os_error() == Some(libc::EIO) {
                    return Ok(bytes);
                }
                return Err(format!("read: {error}"));
            }
        }
    }

    fn reap_timeout(&mut self, timeout: Duration) -> Result<Option<c_int>, String> {
        let deadline = Instant::now() + timeout;
        loop {
            let mut status: c_int = 0;
            let result = unsafe { libc::waitpid(self.child, &mut status, libc::WNOHANG) };
            if result < 0 {
                return Err(format!("waitpid: {}", std::io::Error::last_os_error()));
            }
            if result == self.child {
                self.child = -1;
                return Ok(if libc::WIFEXITED(status) {
                    Some(libc::WEXITSTATUS(status))
                } else if libc::WIFSIGNALED(status) {
                    Some(128 + libc::WTERMSIG(status))
                } else {
                    None
                });
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for PtyChild {
    fn drop(&mut self) {
        unsafe {
            if self.master >= 0 {
                libc::close(self.master);
                self.master = -1;
            }
            if self.child > 0 {
                let mut status: c_int = 0;
                if libc::waitpid(self.child, &mut status, libc::WNOHANG) == 0 {
                    libc::kill(self.child, libc::SIGKILL);
                    let _ = libc::waitpid(self.child, &mut status, 0);
                }
                self.child = -1;
            }
        }
    }
}

fn run_pty(args: &[&str], rows: u16, cols: u16, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut pty = PtyChild::spawn(args, rows, cols)?;
    let bytes = pty.read_all(timeout)?;
    let status = pty.reap_timeout(Duration::from_secs(5))?;
    assert_eq!(status, Some(0), "rustfetch exited abnormally: {status:?}");
    Ok(bytes)
}

fn piped(args: &[&str]) -> Result<std::process::Output, String> {
    Command::new(env!("CARGO_BIN_EXE_rustfetch"))
        .args(args)
        .env_remove("NO_COLOR")
        .output()
        .map_err(|error| format!("spawn piped rustfetch: {error}"))
}

#[test]
fn pty_default_emits_both_wrap_toggles() -> Result<(), String> {
    let bytes = run_pty(BASE_ARGS, 24, 45, Duration::from_secs(15))?;
    let off = position(&bytes, DISABLE_WRAP).ok_or("default run is missing \\e[?7l")?;
    let on = position(&bytes, RESTORE_WRAP).ok_or("default run is missing \\e[?7h")?;
    assert!(off <= 16, "disable toggle is not near the start: {off}");
    assert!(bytes.ends_with(RESTORE_WRAP), "restore toggle is not at the end");
    assert!(off < on, "wrap toggles are out of order");
    Ok(())
}

#[test]
fn pty_explicit_false_emits_neither_toggle() -> Result<(), String> {
    let mut args = BASE_ARGS.to_vec();
    args.extend(["--disable-linewrap", "false"]);
    let bytes = run_pty(&args, 24, 45, Duration::from_secs(15))?;
    assert!(position(&bytes, DISABLE_WRAP).is_none(), "false should not emit \\e[?7l");
    assert!(position(&bytes, RESTORE_WRAP).is_none(), "false should not emit \\e[?7h");
    Ok(())
}

#[test]
fn pty_explicit_true_emits_both_toggles() -> Result<(), String> {
    let mut args = BASE_ARGS.to_vec();
    args.extend(["--disable-linewrap", "true"]);
    let bytes = run_pty(&args, 24, 45, Duration::from_secs(15))?;
    assert!(position(&bytes, DISABLE_WRAP).is_some(), "true should emit \\e[?7l");
    assert!(position(&bytes, RESTORE_WRAP).is_some(), "true should emit \\e[?7h");
    Ok(())
}

#[test]
fn piped_run_emits_neither_toggle_by_default() -> Result<(), String> {
    let output = piped(BASE_ARGS)?;
    assert!(output.status.success(), "piped rustfetch failed: {:?}", output.status);
    assert!(position(&output.stdout, DISABLE_WRAP).is_none(), "pipe should not emit \\e[?7l");
    assert!(position(&output.stdout, RESTORE_WRAP).is_none(), "pipe should not emit \\e[?7h");
    Ok(())
}

#[test]
fn wrap_toggles_bracket_the_module_output() -> Result<(), String> {
    let bytes = run_pty(BASE_ARGS, 24, 45, Duration::from_secs(15))?;
    let off = position(&bytes, DISABLE_WRAP).ok_or("missing \\e[?7l")?;
    let on = position(&bytes, RESTORE_WRAP).ok_or("missing \\e[?7h")?;
    let first_row = position(&bytes, b"OS").ok_or("first module row missing")?;
    let last_row = position(&bytes, b"Kernel").ok_or("last module row missing")?;
    assert!(off < first_row, "disable toggle must precede the first module row");
    assert!(first_row < last_row, "expected rows in structure order");
    assert!(last_row < on, "restore toggle must follow the last module row");
    Ok(())
}

#[test]
fn help_documents_linewrap_default_as_true() -> Result<(), String> {
    let full = Command::new(env!("CARGO_BIN_EXE_rustfetch"))
        .arg("--help")
        .output()
        .map_err(|error| format!("spawn --help: {error}"))?;
    assert!(full.status.success(), "--help failed: {:?}", full.status);
    assert!(
        String::from_utf8_lossy(&full.stdout).contains("--disable-linewrap"),
        "--help stopped documenting the flag"
    );

    // The verbatim help lists the flag; the specific-help entry carries its
    // default, which is what must stay in sync with the binary's behavior.
    let specific = Command::new(env!("CARGO_BIN_EXE_rustfetch"))
        .args(["-h", "disable-linewrap"])
        .output()
        .map_err(|error| format!("spawn -h disable-linewrap: {error}"))?;
    assert!(specific.status.success(), "specific help failed: {:?}", specific.status);
    assert!(
        String::from_utf8_lossy(&specific.stdout).contains("Default: true"),
        "disable-linewrap's documented default drifted from true"
    );
    Ok(())
}

#[test]
fn long_structure_under_narrow_pty_is_safe() -> Result<(), String> {
    let long_structure = vec!["UnknownModule"; 64].join(":");
    let args = [
        "--config",
        "none",
        "--logo",
        "none",
        "--structure",
        long_structure.as_str(),
    ];
    let bytes = run_pty(&args, 24, 20, Duration::from_secs(25))?;
    assert!(position(&bytes, DISABLE_WRAP).is_some(), "narrow run is missing \\e[?7l");
    assert!(position(&bytes, RESTORE_WRAP).is_some(), "narrow run is missing \\e[?7h");
    Ok(())
}

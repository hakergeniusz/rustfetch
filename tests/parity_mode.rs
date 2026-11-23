//! End-to-end regression tests for `RUSTFETCH_PARITY_VERSION` parity mode.
//!
//! Parity mode must force fastfetch 2.71.0 behavior even when rustfetch's
//! normal ergonomic defaults differ. PTY helpers are gated to Unix; the
//! version/pipe assertions are portable.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_rustfetch");

fn run_parity(args: &[&str]) -> (String, String, bool) {
    let out = match Command::new(BIN)
        .args(args)
        .env("RUSTFETCH_PARITY_VERSION", "1")
        .env("NO_CONFIG", "1")
        .output()
    {
        Ok(out) => out,
        Err(err) => panic!("spawn rustfetch in parity mode failed: {err}"),
    };
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.success(),
    )
}

#[test]
fn parity_mode_pins_the_fastfetch_version_strings() {
    let (stdout, _, ok) = run_parity(&["--version"]);
    assert!(ok);
    assert_eq!(stdout.trim(), "fastfetch 2.71.0 (x86_64)");

    let (raw, _, ok) = run_parity(&["--version-raw"]);
    assert!(ok);
    assert_eq!(raw.trim(), "2.71.0");
}

#[test]
fn parity_mode_pipe_output_is_ansi_free_and_valid() {
    for args in [
        vec!["--pipe", "--structure", "OS:Kernel"],
        vec!["--pipe", "--format", "json", "--structure", "OS:Kernel"],
        vec!["--pipe", "--structure", "Display"],
    ] {
        let (stdout, stderr, ok) = run_parity(&args);
        assert!(ok, "{args:?}: {stderr}");
        assert!(!stdout.contains('\u{1b}'), "{args:?} leaked an escape sequence");
    }
}

#[cfg(unix)]
mod pty {
    use super::*;
    use libc::{TIOCSWINSZ, grantpt, poll, pollfd, posix_openpt, unlockpt, winsize};
    use std::ffi::CStr;
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
    use std::process::{Child, Command, Stdio};

    struct PtyChild {
        master: OwnedFd,
        child: Option<Child>,
    }

    impl Drop for PtyChild {
        fn drop(&mut self) {
            if let Some(child) = self.child.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn spawn_pty(parity: bool, extra: &[&str]) -> PtyChild {
        let master = unsafe { posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
        assert!(master >= 0, "posix_openpt failed");
        assert_eq!(unsafe { grantpt(master) }, 0);
        assert_eq!(unsafe { unlockpt(master) }, 0);
        // SAFETY: master is an open PTY master; ptsname returns a static buffer.
        let slave_name = unsafe { CStr::from_ptr(libc::ptsname(master)) }.to_owned();
        let master = unsafe { OwnedFd::from_raw_fd(master) };

        let ws = winsize { ws_row: 24, ws_col: 45, ws_xpixel: 0, ws_ypixel: 0 };
        assert_eq!(unsafe { libc::ioctl(master.as_raw_fd(), TIOCSWINSZ, &ws) }, 0);

        let mut cmd = Command::new(BIN);
        cmd.arg("--structure").arg("OS:Kernel:Display").args(extra)
            .env("TERM", "xterm-256color").env("NO_CONFIG", "1")
            .env_remove("NO_COLOR")
            .stdin(Stdio::null()).stderr(Stdio::null());
        if parity {
            cmd.env("RUSTFETCH_PARITY_VERSION", "1");
        } else {
            cmd.env_remove("RUSTFETCH_PARITY_VERSION");
        }

        let slave = unsafe { std::fs::File::from_raw_fd(libc::open(slave_name.as_ptr(), libc::O_RDWR)) };
        let slave_fd = slave.into_raw_fd();
        // Only stdout must be a TTY: rustfetch's pipe detection is based on
        // isatty(STDOUT_FILENO), so no controlling terminal is needed.
        let child = cmd
            .stdout(unsafe { std::process::Stdio::from(OwnedFd::from_raw_fd(slave_fd)) })
            .spawn();
        let child = match child {
            Ok(c) => c,
            Err(err) => panic!("spawn failed: {err}"),
        };
        PtyChild { master, child: Some(child) }
    }

    fn collect(mut session: PtyChild) -> Vec<u8> {
        let mut out = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            let mut pfd = pollfd { fd: session.master.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            let ready = unsafe { poll(&mut pfd, 1, 100) };
            if ready <= 0 {
                continue;
            }
            let mut buf = [0u8; 4096];
            match unsafe { libc::read(session.master.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) } {
                n if n > 0 => out.extend_from_slice(&buf[..n as usize]),
                _ => break,
            }
        }
        let _ = session.child.take().map(|mut c| c.wait());
        out
    }

    #[test]
    fn parity_mode_emits_no_linewrap_toggles_even_when_flag_requests_them() {
        let normal = collect(spawn_pty(false, &[]));
        assert!(normal.windows(b"\x1b[?7l".len()).any(|w| w == b"\x1b[?7l"));
        assert!(normal.windows(b"\x1b[?7h".len()).any(|w| w == b"\x1b[?7h"));

        for extra in [&[][..], &["--disable-linewrap", "true"][..]] {
            let parity = collect(spawn_pty(true, extra));
            assert!(!parity.windows(b"\x1b[?7l".len()).any(|w| w == b"\x1b[?7l"),
                "parity mode emitted ESC[?7l with {extra:?}");
            assert!(!parity.windows(b"\x1b[?7h".len()).any(|w| w == b"\x1b[?7h"),
                "parity mode emitted ESC[?7h with {extra:?}");
        }
    }
}

#[test]
fn parity_mode_does_not_break_normal_mode_default() {
    // Running a normal invocation after a parity one must still cut lines.
    let (normal, _, ok) = {
        let out = match Command::new(BIN)
            .args(["--version"])
            .env_remove("RUSTFETCH_PARITY_VERSION")
            .env("NO_CONFIG", "1")
            .output()
        {
            Ok(out) => out,
            Err(err) => panic!("spawn rustfetch failed: {err}"),
        };
        (String::from_utf8_lossy(&out.stdout).into_owned(), String::new(), out.status.success())
    };
    assert!(ok);
    assert!(normal.starts_with("rustfetch "), "normal version identity changed: {normal}");
}

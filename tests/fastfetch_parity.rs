#![cfg(target_os = "linux")]
//! Byte-for-byte output parity against the reference fastfetch 2.71.0.
//!
//! The comparison is possible only on Linux and only when the exact reference
//! version is available. Set `RUSTFETCH_FASTFETCH` to the binary path, or place
//! it at `/usr/sbin/fastfetch` or `/usr/bin/fastfetch`. When no matching
//! reference is found the tests report a skip and pass: native ports and
//! systems with a different fastfetch version must not fail for that reason.
//!
//! Every case runs both binaries with the same arguments and environment,
//! with `NO_CONFIG=1` and rustfetch in `RUSTFETCH_PARITY_VERSION=1` mode. The
//! cases cover all built-in distro logos, a matrix of terminal/locale/session
//! environments, and raw PTY output at several widths.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

const REFERENCE_VERSION: &str = "2.71.0";
const BIN: &str = env!("CARGO_BIN_EXE_rustfetch");

struct Reference {
    path: PathBuf,
}

/// Locate a fastfetch binary whose `--version-raw` is exactly 2.71.0.
fn reference() -> Option<&'static Reference> {
    static FOUND: OnceLock<Option<Reference>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let mut candidates = Vec::new();
            if let Some(path) = std::env::var_os("RUSTFETCH_FASTFETCH") {
                // An explicit path is authoritative; never silently fall back
                // to a different (possibly mismatched) system fastfetch.
                candidates.push(PathBuf::from(path));
            } else {
                candidates.push(PathBuf::from("/usr/sbin/fastfetch"));
                candidates.push(PathBuf::from("/usr/bin/fastfetch"));
            }

            for path in candidates {
                let Ok(out) = Command::new(&path)
                    .arg("--version-raw")
                    .env("NO_CONFIG", "1")
                    .output()
                else {
                    continue;
                };
                if out.status.success()
                    && String::from_utf8_lossy(&out.stdout).trim() == REFERENCE_VERSION
                {
                    return Some(Reference { path });
                }
            }
            None
        })
        .as_ref()
}

macro_rules! reference_or_skip {
    () => {
        match reference() {
            Some(reference) => reference,
            None => {
                eprintln!(
                    "skipping fastfetch 2.71.0 parity test; set RUSTFETCH_FASTFETCH to a \
                     matching binary to enable it"
                );
                return;
            }
        }
    };
}

type Captured = (Vec<u8>, Vec<u8>, Option<i32>);

fn capture(bin: &Path, parity: bool, args: &[&str], scenario: &[(&str, &str)]) -> Captured {
    let mut command = Command::new(bin);
    command
        .args(args)
        .env("NO_CONFIG", "1")
        .env_remove("RUSTFETCH_PARITY_VERSION")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if parity {
        command.env("RUSTFETCH_PARITY_VERSION", "1");
    }
    for (key, value) in scenario {
        command.env(key, value);
    }
    let out = command.output().unwrap_or_else(|err| panic!("spawn {bin:?} failed: {err}"));
    (out.stdout, out.stderr, out.status.code())
}

fn preview(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(400)]);
    format!("{text:?}")
}

fn assert_pair(reference: &Reference, args: &[&str], scenario: &[(&str, &str)]) {
    let upstream = capture(&reference.path, false, args, scenario);
    let local = capture(Path::new(BIN), true, args, scenario);
    assert_eq!(
        upstream.2, local.2,
        "exit status differs: args={args:?} scenario={scenario:?}"
    );
    assert_eq!(
        upstream.1, local.1,
        "stderr differs: args={args:?} scenario={scenario:?}\nfastfetch: {}\nrustfetch: {}",
        preview(&upstream.1),
        preview(&local.1)
    );
    assert_eq!(
        upstream.0, local.0,
        "stdout differs: args={args:?} scenario={scenario:?}\nfastfetch: {}\nrustfetch: {}",
        preview(&upstream.0),
        preview(&local.0)
    );
}

/// First quoted alias on each `--list-logos` line, e.g. `"Amazon Linux" "amzn"`
/// yields `Amazon Linux`. Both binaries list the same 548 entries.
fn builtin_logo_names(reference: &Reference) -> Vec<String> {
    let out = Command::new(&reference.path)
        .args(["--list-logos", "--pipe"])
        .env("NO_CONFIG", "1")
        .output()
        .unwrap_or_else(|err| panic!("fastfetch --list-logos failed: {err}"));
    assert!(out.status.success(), "fastfetch --list-logos exited badly");
    let mut names = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some((_, rest)) = line.split_once(')') else {
            continue;
        };
        let Some(start) = rest.find('"') else {
            continue;
        };
        let tail = &rest[start + 1..];
        let Some(end) = tail.find('"') else {
            continue;
        };
        names.push(tail[..end].to_owned());
    }
    assert!(names.len() > 500, "only {} logos listed", names.len());
    names
}

#[test]
fn every_builtin_distro_logo_matches_in_pipe_mode() {
    let reference = reference_or_skip!();
    for logo in builtin_logo_names(reference) {
        assert_pair(reference, &["--pipe", "--structure", "Separator", "--logo", &logo], &[]);
    }
}

const ENVIRONMENTS: &[&[(&str, &str)]] = &[
    &[],
    &[("LC_ALL", "C"), ("LANG", "C")],
    &[("LC_ALL", "C.UTF-8"), ("LANG", "C.UTF-8")],
    &[("TERM", "xterm-256color"), ("COLORTERM", "truecolor")],
    &[("TERM", "xterm-256color"), ("NO_COLOR", "1")],
    &[("TERM", "dumb")],
    &[("TZ", "Pacific/Kiritimati")],
    &[("SSH_CONNECTION", "10.0.0.1 1234 10.0.0.2 22"), ("SSH_TTY", "/dev/pts/3")],
    &[("CI", "true"), ("GITHUB_ACTIONS", "true")],
    &[
        ("XDG_SESSION_TYPE", "wayland"),
        ("XDG_CURRENT_DESKTOP", "GNOME"),
        ("WAYLAND_DISPLAY", "wayland-0"),
    ],
    &[("XDG_CURRENT_DESKTOP", "GNOME"), ("DESKTOP_SESSION", "gnome")],
    &[("SHELL", "/bin/zsh")],
];

#[test]
fn environment_matrix_matches_in_pipe_and_json_modes() {
    let reference = reference_or_skip!();
    for scenario in ENVIRONMENTS {
        assert_pair(reference, &["--pipe", "--structure", "Separator", "--logo", "arch"], scenario);
        assert_pair(
            reference,
            &["--format", "json", "--structure", "Separator", "--logo", "arch"],
            scenario,
        );
    }
}

#[test]
fn distro_logo_color_overrides_match_across_environments() {
    let reference = reference_or_skip!();
    for (logo, scenario) in [
        ("ubuntu", &[][..]),
        ("debian", &[("LC_ALL", "C"), ("LANG", "C")][..]),
        ("fedora", &[("TERM", "xterm-256color"), ("NO_COLOR", "1")][..]),
        ("alpine", &[("TERM", "xterm-256color"), ("COLORTERM", "truecolor")][..]),
    ] {
        assert_pair(
            reference,
            &[
                "--pipe",
                "--structure",
                "Separator",
                "--logo",
                logo,
                "--logo-color-1",
                "red",
                "--logo-color-2",
                "#00FF00",
            ],
            scenario,
        );
    }
}

#[cfg(unix)]
mod pty {
    use super::*;
    use libc::{TIOCSWINSZ, grantpt, poll, pollfd, posix_openpt, unlockpt, winsize};
    use std::ffi::CStr;
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
    use std::time::{Duration, Instant};

    fn capture_pty(bin: &Path, parity: bool, scenario: &[(&str, &str)], cols: u16) -> Vec<u8> {
        let master = unsafe { posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
        assert!(master >= 0, "posix_openpt failed");
        assert_eq!(unsafe { grantpt(master) }, 0);
        assert_eq!(unsafe { unlockpt(master) }, 0);
        // SAFETY: master is an open PTY master; ptsname returns a static buffer.
        let slave_name = unsafe { CStr::from_ptr(libc::ptsname(master)) }.to_owned();
        let master = unsafe { OwnedFd::from_raw_fd(master) };

        let ws = winsize { ws_row: 24, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        assert_eq!(unsafe { libc::ioctl(master.as_raw_fd(), TIOCSWINSZ, &ws) }, 0);

        let mut command = Command::new(bin);
        command
            .args(["--structure", "Separator", "--logo", "arch"])
            .env("NO_CONFIG", "1")
            .env("TERM", "xterm-256color")
            .env_remove("NO_COLOR")
            .env_remove("RUSTFETCH_PARITY_VERSION")
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        if parity {
            command.env("RUSTFETCH_PARITY_VERSION", "1");
        }
        for (key, value) in scenario {
            if *key == "TERM" || *key == "NO_COLOR" {
                command.env(key, value);
            }
        }

        let fd = unsafe { libc::open(slave_name.as_ptr(), libc::O_RDWR) };
        assert!(fd >= 0, "open PTY slave failed");
        // SAFETY: fd is a freshly opened PTY slave; hand it to Stdio once.
        let slave = unsafe { OwnedFd::from_raw_fd(fd) };
        let mut child = command
            .stdout(unsafe { Stdio::from(OwnedFd::from_raw_fd(slave.into_raw_fd())) })
            .spawn()
            .unwrap_or_else(|err| panic!("spawn under PTY failed: {err}"));

        let mut out = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let mut pfd = pollfd { fd: master.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            if unsafe { poll(&mut pfd, 1, 100) } <= 0 {
                if child
                    .try_wait()
                    .unwrap_or_else(|err| panic!("try_wait on PTY child failed: {err}"))
                    .is_some()
                {
                    break;
                }
                continue;
            }
            let mut buf = [0u8; 4096];
            match unsafe { libc::read(master.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) } {
                n if n > 0 => out.extend_from_slice(&buf[..n as usize]),
                _ => break,
            }
        }
        let status = child.wait().unwrap_or_else(|err| panic!("wait on PTY child failed: {err}"));
        assert!(status.success());
        out
    }

    #[test]
    fn pty_output_matches_at_multiple_widths_and_terminals() {
        let reference = reference_or_skip!();
        for cols in [20, 40, 80, 120, 200] {
            for scenario in [
                &[][..],
                &[("TERM", "dumb")][..],
                &[("NO_COLOR", "1")][..],
                &[("COLORTERM", "truecolor")][..],
            ] {
                let upstream = capture_pty(&reference.path, false, scenario, cols);
                let local = capture_pty(Path::new(BIN), true, scenario, cols);
                assert_eq!(
                    upstream, local,
                    "PTY output differs at {cols} columns with {scenario:?}\nfastfetch: {}\nrustfetch: {}",
                    preview(&upstream),
                    preview(&local)
                );
            }
        }
    }
}

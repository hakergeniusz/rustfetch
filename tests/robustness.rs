//! "Fuzz-lite" robustness tests for CLI argument handling.
//!
//! Deterministic case tables plus a small LCG drive the real binary with
//! adversarial argv. Every run must terminate promptly, must not die from
//! a signal (an aborting Rust panic is SIGABRT/134 in release builds),
//! must not print `panicked at`, and must produce valid UTF-8 on stdout.

use std::{
    ffi::OsStr,
    os::unix::process::ExitStatusExt,
    process::{Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const BIN: &str = env!("CARGO_BIN_EXE_rustfetch");
const DEADLINE: Duration = Duration::from_secs(3);

struct Run {
    status: Option<ExitStatus>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
}

/// Spawn with piped stdout/stderr and a hard deadline. `wait_with_output`
/// runs on a helper thread so a child that fills the pipe cannot block the
/// deadline check; on timeout the child is SIGKILLed and reaped.
fn run_with_deadline<I, S>(args: I, deadline: Duration, capture_stdout: bool) -> Run
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(BIN);
    command.args(args).stdin(Stdio::null()).stderr(Stdio::piped());
    if capture_stdout {
        command.stdout(Stdio::piped());
    } else {
        command.stdout(Stdio::null());
    }

    let child = match command.spawn() {
        Ok(child) => child,
        Err(err) => panic!("failed to spawn {BIN}: {err}"),
    };
    let pid = child.id() as i32;
    let (tx, rx) = mpsc::channel();
    let _ = thread::spawn(move || {
        let result = child.wait_with_output();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(deadline) {
        Ok(Ok(output)) => Run {
            status: Some(output.status),
            stdout: output.stdout,
            stderr: output.stderr,
            timed_out: false,
        },
        Ok(Err(err)) => panic!("failed to collect child output: {err}"),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
            let mut stderr = Vec::new();
            if let Ok(Ok(output)) = rx.recv_timeout(Duration::from_secs(1)) {
                stderr = output.stderr;
            }
            Run {
                status: None,
                stdout: Vec::new(),
                stderr,
                timed_out: true,
            }
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("child collector thread disconnected"),
    }
}

fn base(extra: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = ["--local-only", "--pipe", "--logo", "none", "--structure", "OS:Kernel"]
        .iter()
        .map(|arg| (*arg).to_string())
        .collect();
    args.extend(extra.iter().map(|arg| (*arg).to_string()));
    args
}

fn assert_clean(run: &Run, label: &str) {
    assert!(!run.timed_out, "{label}: exceeded the {DEADLINE:?} deadline");
    let status = match run.status {
        Some(status) => status,
        None => panic!("{label}: no exit status"),
    };
    match status.code() {
        Some(code) => assert!(
            code != 134 && code != 139,
            "{label}: signal-like exit code {code}, stderr: {}",
            String::from_utf8_lossy(&run.stderr)
        ),
        None => panic!("{label}: terminated by signal {:?}", status.signal()),
    }
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(!stderr.contains("panicked at"), "{label}: panic output: {stderr}");
    assert!(
        std::str::from_utf8(&run.stdout).is_ok(),
        "{label}: stdout was not valid UTF-8"
    );
}

#[test]
fn adversarial_case_table_never_panics_or_hangs() {
    let long = "x".repeat(10_000);
    let wide_structure = ["OS", "Kernel", "CPU", "Memory", "Disk"].join(":");
    let huge_structure = format!("OS:{long}");

    let extras: &[&[&str]] = &[
        &["--separator", ""],
        &["--separator", "a"], // placeholder replaced below for the 10k case
        &["--separator", "\x1b[31m"],
        &["--separator", "\u{1f4a9}"],
        &["--separator", "\x01\x07\x08\x1b"],
        &["--separator", " => ", "--separator", ""],
        &["--key-width", ""],
        &["--key-width", "-1"],
        &["--key-width", "0"],
        &["--key-width", "65535"],
        &["--key-width", "9999999999"],
        &["--key-width", "1.5"],
        &["--key-padding-left", "-5"],
        &["--key-padding-left", "18446744073709551616"],
        &["--key-padding-left", "4", "--key-padding-left", "9999999999"],
        &["--key-type", ""],
        &["--key-type", "bogus"],
        &["--key-type", "none", "--key-type", "both"],
        &["--color-keys", ""],
        &["--color-keys", "red"],
        &["--color-keys", "bogus"],
        &["--color-keys", "@999999"],
        &["--color-output", "#12345678"],
        &["--color", ""],
        &["--color", "bogus"],
        &["--bright-color", ""],
        &["--bright-color", "maybe"],
        &["--bright-color", "true", "--bright-color", "false"],
        &["--pipe", ""],
        &["--pipe", "false"],
        &["--no-buffer", ""],
        &["--no-buffer", "bogus"],
        &["--percentage-type", "bogus"],
        &["--percent-type", "-1"],
        &["--percent-type", "255"],
        &["--percent-ndigits", "-1"],
        &["--percent-ndigits", "9999999999"],
        &["--bar-width", "-1"],
        &["--bar-width", "0"],
        &["--bar-width", "255"],
        &["--bar-width", "9999999999"],
        &["--bar-char-elapsed", ""],
        &["--bar-char-elapsed", "\u{1f4a9}\u{1f4a9}"],
        &["--size-binary-prefix", "BOGUS"],
        &["--size-binary-prefix", "SI", "--size-binary-prefix", "IEC"],
        &["--size-ndigits", "10"],
        &["--size-ndigits", "0"],
        &["--temp-unit", "bogus"],
        &["--temp-unit", "K", "--temp-unit", "C"],
        &["--temp-ndigits", "9999999999"],
        &["--structure", ""],
        &["--structure", ":"],
        &["--structure", "Bogus:OS:\u{1f4a9}:Kernel"],
        &["--structure", "OS:Kernel"],
        &["--structure", "OS:Kernel"],
        &["--logo", "bogus"],
        &["--logo", ""],
        &["--format", "bogus"],
        &["--format", "json"],
        &["--json", "--separator", "\u{1f4a9}"],
        &["--local-only"],
        &["--color-keys", "red", "--key-width", "9999999999"],
        &["--key-type", "bogus", "--separator", "=>"],
        &["--separator", "a", "--percent-type", "255"],
        &["--temp-unit", "F", "--bar-width", "255", "--key-type", "none"],
        &["--structure", "OS:Kernel", "--key-width", "-1", "--color", "bogus"],
        &["--bright-color", "true", "--no-buffer", "bogus", "--temp-ndigits", "9999999999"],
    ];
    assert!(
        extras.len() >= 60,
        "expected at least 60 adversarial cases, got {}",
        extras.len()
    );

    for (index, extra) in extras.iter().enumerate() {
        let args = match index {
            1 => base(&["--separator", long.as_str()]),
            53 => base(&[wide_structure.as_str()]),
            54 => base(&[huge_structure.as_str()]),
            63 => base(&["--separator", long.as_str(), "--percent-type", "255"]),
            _ => base(extra),
        };
        let mut labeled = Vec::with_capacity(args.len() + 1);
        labeled.push(format!("case[{index}]"));
        labeled.extend(args);
        let run = run_with_deadline(labeled.iter(), DEADLINE, true);
        assert_clean(&run, &format!("case[{index}] {extra:?}"));
    }
}

/// Tiny deterministic LCG (numerical Recipes constants). No `rand`
/// dependency and no clock: a given seed always yields the same argv.
struct Lcg(u64);

impl Lcg {
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 32
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let index = (self.next_u64() % items.len() as u64) as usize;
        &items[index]
    }
}

#[test]
fn lcg_generated_flag_values_never_panic_or_hang() {
    const KEYS: &[&str] = &[
        "--separator",
        "--key-width",
        "--key-padding-left",
        "--key-type",
        "--color-keys",
        "--color-output",
        "--color",
        "--bright-color",
        "--percent-type",
        "--percent-ndigits",
        "--bar-width",
        "--bar-char-elapsed",
        "--size-binary-prefix",
        "--size-ndigits",
        "--temp-unit",
        "--temp-ndigits",
        "--no-buffer",
        "--pipe",
        "--structure",
        "--local-only",
        "--json",
        "--format",
    ];
    const VALUES: &[&str] = &[
        "",
        "0",
        "1",
        "-1",
        "20",
        "255",
        "9999999999",
        "65536",
        "1.5",
        "red",
        "bogus",
        "SI",
        "IEC",
        "C",
        "F",
        "K",
        "none",
        "both",
        "icon",
        "string",
        "true",
        "false",
        "maybe",
        "\u{1f4a9}",
        "=>",
        "\x1b[31m",
        "\x01\x07",
    ];

    let mut rng = Lcg(0x5eed_1234_5678_9abc);
    for index in 0..200 {
        let mut args = base(&[]);
        let pairs = 1 + usize::try_from(rng.next_u64() % 3).unwrap_or(0);
        for _ in 0..pairs {
            let key = *rng.pick(KEYS);
            let value = *rng.pick(VALUES);
            args.push(key.to_string());
            args.push(value.to_string());
        }
        let run = run_with_deadline(args.iter(), DEADLINE, true);
        assert_clean(&run, &format!("lcg[{index}] {args:?}"));
    }
}

#[test]
fn every_listed_module_runs_inside_a_joined_structure() {
    let list_args = ["--local-only", "--pipe", "--logo", "none", "--list-modules"];
    let list = run_with_deadline(list_args.iter().copied(), DEADLINE, true);
    assert_clean(&list, "--list-modules");
    let listing = match String::from_utf8(list.stdout) {
        Ok(text) => text,
        Err(err) => panic!("--list-modules stdout was not UTF-8: {err}"),
    };

    let mut modules = Vec::new();
    for line in listing.lines() {
        let mut fields = line.split_whitespace();
        let number = fields.next().unwrap_or("");
        if !number.trim_end_matches(':').trim_end_matches(')').is_empty()
            && number.trim_end_matches(')').chars().all(|ch| ch.is_ascii_digit())
            && let Some(name) = fields.next()
        {
            modules.push(name.trim_end_matches(':').to_string());
        }
    }
    assert!(modules.len() > 20, "module list looked truncated: {listing:?}");

    // `command` is config-only and fastfetch refuses it in --structure.
    modules.retain(|name| !name.eq_ignore_ascii_case("command"));
    let mut active_modules = 0usize;
    for module in &modules {
        let args = base(&["--structure", module]);
        let run = run_with_deadline(args.iter(), DEADLINE, true);
        assert_clean(&run, &format!("module {module}"));
        let printed = String::from_utf8_lossy(&run.stdout);
        if printed.lines().any(|line| !line.trim().is_empty()) {
            active_modules += 1;
        }
    }

    let joined = base(&["--structure", &modules.join(":")]);
    let run = run_with_deadline(joined.iter(), DEADLINE, true);
    assert_clean(&run, "all modules joined");
    let text = match String::from_utf8(run.stdout) {
        Ok(text) => text,
        Err(err) => panic!("joined stdout was not UTF-8: {err}"),
    };
    let emitted = text.lines().filter(|line| !line.trim().is_empty()).count();
    assert!(emitted > 0, "joined structure printed no module rows");
    assert!(
        emitted >= active_modules,
        "joined structure printed {emitted} rows but {active_modules} modules printed alone"
    );
}

#[test]
fn watch_flags_are_bounded_by_a_short_deadline() {
    let args = [
        "--local-only",
        "--pipe",
        "--watch",
        "1",
        "--dynamic-interval",
        "1",
        "--structure",
        "OS:Kernel",
        "--logo",
        "none",
    ];
    let started = Instant::now();
    // stdout goes to /dev/null: a 1 ms watch loop would otherwise fill
    // the pipe (and the collector's Vec) with repeated frames.
    let run = run_with_deadline(args.iter().copied(), Duration::from_secs(2), false);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(!stderr.contains("panicked at"), "watch mode panicked: {stderr}");
    match run.status {
        Some(status) => match status.code() {
            Some(code) => assert!(code != 134 && code != 139, "watch mode exited with {code}"),
            None => panic!("watch mode was killed by signal {:?}", status.signal()),
        },
        None => assert!(run.timed_out, "watch mode had no status and did not time out"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "watch mode outlived its deadline by too much"
    );
}

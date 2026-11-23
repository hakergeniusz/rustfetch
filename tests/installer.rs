//! Installer contract tests: the same install.sh must behave identically
//! when piped into or launched from POSIX sh, bash and zsh (Linux and macOS),
//! and must keep macOS-compatible fallbacks plus broad distro hints.

use std::path::Path;
use std::process::Command;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn installer_path() -> String {
    Path::new(ROOT).join("install.sh").to_string_lossy().into_owned()
}

fn run(shell: &str, extra: &[&str]) -> (String, String, i32) {
    let mut args = vec![installer_path()];
    args.extend(extra.iter().copied().map(str::to_string));
    let child = match Command::new(shell).args(&args).output() {
        Ok(child) => child,
        Err(err) => panic!("spawn {shell} failed: {err}"),
    };
    (
        String::from_utf8_lossy(&child.stdout).into_owned(),
        String::from_utf8_lossy(&child.stderr).into_owned(),
        child.status.code().unwrap_or(-1),
    )
}

fn available<'a>(shells: &'a [&'a str]) -> Vec<&'a str> {
    shells
        .iter()
        .filter_map(|shell| {
            let found = Command::new(shell).arg("--version").output().is_ok();
            if found { Some(*shell) } else { None }
        })
        .collect()
}

#[test]
fn plan_is_identical_across_sh_bash_and_zsh() {
    let shells = available(&["sh", "bash", "zsh"]);
    assert!(shells.contains(&"sh"), "POSIX sh must be available");
    assert!(shells.len() >= 2, "need at least two shells to compare");

    let (baseline, _, rc) = run("sh", &["--print-plan"]);
    assert_eq!(rc, 0, "sh plan failed");
    assert!(baseline.contains("[*] Mode") && baseline.contains("[*] Repository"));

    for shell in shells {
        let (stdout, stderr, rc) = run(shell, &["--print-plan"]);
        assert_eq!(rc, 0, "{shell} plan failed: {stderr}");
        assert_eq!(stdout, baseline, "{shell} produced a different plan than sh");
        assert!(!stderr.contains("bash-only"), "{shell} tried to parse the bash payload");
    }
}

#[test]
fn absolute_shell_paths_are_treated_as_pipes_not_local_files() {
    for shell in ["/bin/sh", "/bin/bash", "/bin/zsh"] {
        if !Path::new(shell).exists() {
            continue;
        }
        let (stdout, stderr, rc) = run(shell, &["--print-plan"]);
        assert_eq!(rc, 0, "{shell} failed: {stderr}");
        assert!(stdout.contains("[*] Mode"), "{shell} did not reach the payload");
    }
}

#[test]
fn command_string_one_liner_works_in_sh_and_zsh() {
    let script = std::fs::read_to_string(Path::new(ROOT).join("install.sh"));
    let script = match script {
        Ok(script) => script,
        Err(err) => panic!("read install.sh: {err}"),
    };
    for shell in available(&["sh", "zsh"]) {
        let child = match Command::new(shell).arg("-c").arg(&script).arg("--").arg("--print-plan").output() {
            Ok(child) => child,
            Err(err) => panic!("spawn {shell}: {err}"),
        };
        assert!(child.status.success(), "{shell} command-string mode failed");
        let stdout = String::from_utf8_lossy(&child.stdout);
        assert!(stdout.contains("[*] Mode       : remote"));
    }
}

#[test]
fn installer_keeps_macos_compatible_fallbacks() {
    let script = match std::fs::read_to_string(Path::new(ROOT).join("install.sh")) {
        Ok(script) => script,
        Err(err) => panic!("read install.sh: {err}"),
    };
    assert!(script.contains("shasum -a 256"), "macOS has no sha256sum");
    assert!(script.contains("/bin/bash"), "macOS bash fallback missing");
    assert!(script.contains("mktemp -d"), "needs a portable temp dir");
    assert!(script.contains("*/zsh"), "absolute zsh path not recognised as a shell");
}

#[test]
fn installer_hints_cover_major_distro_families() {
    let script = match std::fs::read_to_string(Path::new(ROOT).join("install.sh")) {
        Ok(script) => script,
        Err(err) => panic!("read install.sh: {err}"),
    };
    for family in [
        "debian", "ubuntu", "fedora", "rhel", "arch", "manjaro", "alpine",
        "suse", "gentoo", "void", "nixos", "solus", "rustup.rs",
    ] {
        assert!(script.contains(family), "distro hint for {family} was dropped");
    }
}

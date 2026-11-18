//! `shell` module: the shell from the parent process chain (fastfetch's
//! getShellInfo), falling back to $SHELL. Versions come from the shell
//! binary's own embedded version string where possible (zsh, bash), so no
//! process is spawned on common setups.

use crate::{instance::Instance, modules::Row, util};

/// fastfetch's wrapper processes between terminal and shell that must not be
/// taken for the shell itself.
const WRAPPERS: &[&str] = &[
    "sh",
    "sudo",
    "su",
    "strace",
    "gdb",
    "lldb",
    "lldb-mi",
    "login",
    "ltrace",
    "perf",
    "guake-wrapped",
    "time",
    "clifm",
    "valgrind",
    "fastfetch",
    "flashfetch",
    "rustfetch",
    "mise",
    "mise-shim",
    "atuin",
    "proot",
    "script",
    "run-parts",
];

/// comm and ppid from /proc/<pid>/stat; comm may contain spaces, so both
/// ends are located and ppid is parsed after the closing paren. Shared with
/// the initsystem module for the pid 1 lookup.
pub(crate) fn process_basic_info(pid: u32) -> Option<(String, u32)> {
    let stat = util::read_file(&format!("/proc/{pid}/stat"))?;
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = stat[open + 1..close].to_string();
    let ppid = stat[close + 1..].split_whitespace().nth(1)?.parse().ok()?;
    Some((comm, ppid))
}

fn is_wrapper(comm: &str) -> bool {
    WRAPPERS.contains(&comm)
        || comm.contains("debug")
        || comm.contains("command-not-")
        || comm.ends_with(".sh")
}

/// fastfetch's pretty-name specials (setShellInfoDetails).
fn pretty_name(comm: &str, exe_name: Option<&str>) -> String {
    match comm {
        "pwsh" => "PowerShell".to_string(),
        "nu" => "nushell".to_string(),
        "oil.ovm" => "Oils".to_string(),
        "busybox" => "ash".to_string(),
        _ => match exe_name {
            Some(exe_name) => exe_name.to_string(),
            None => comm.to_string(),
        },
    }
}

/// The shell process on the parent chain, with its exe path when readable.
fn detect_shell() -> Option<(String, Option<String>)> {
    let user_shell = std::env::var("SHELL").ok()?;
    let user_shell_name = user_shell.rsplit('/').next().unwrap_or("").to_string();

    let mut pid = std::process::id();
    while pid > 1 {
        let Some((comm, ppid)) = process_basic_info(pid) else {
            break;
        };
        if comm == user_shell_name || !is_wrapper(&comm) {
            let exe = std::fs::read_link(format!("/proc/{pid}/exe"))
                .ok()
                .map(|path| path.to_string_lossy().into_owned());
            let exe_name =
                exe.as_ref().and_then(|path| path.rsplit('/').next().map(str::to_string));
            let name = pretty_name(&comm, exe_name.filter(|exe| exe.starts_with(&comm)).as_deref());
            return Some((name, exe));
        }
        pid = ppid;
    }
    Some((pretty_name(&user_shell_name, Some(&user_shell)), Some(user_shell)))
}

fn extract_zsh_version(line: &str) -> Option<String> {
    let rest = line.strip_prefix("zsh-")?;
    let end = rest.find('-')?;
    Some(rest[..end].to_string())
}

fn extract_bash_version(line: &str) -> Option<String> {
    let rest = line.strip_prefix("@(#)Bash version ")?;
    let end = rest.find('(')?;
    Some(rest[..end].to_string())
}

/// getExeVersionGeneral: `--version` output's last space-separated token.
fn version_spawn_last(path: &str) -> Option<String> {
    let output = std::process::Command::new(path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let first = util::first_line(&String::from_utf8_lossy(&output.stdout)).trim().to_string();
    first.split_whitespace().next_back().map(str::to_string)
}

/// Per-shell version resolution, mirroring fastfetch's dispatch.
fn shell_version(name: &str, exe: Option<&str>) -> Option<String> {
    match (name, exe) {
        ("zsh", Some(exe)) => {
            util::binary_extract_strings(exe, "zsh-0.0-0".len(), extract_zsh_version)
                .or_else(|| version_spawn_last(exe))
        },
        ("bash", Some(exe)) => util::binary_extract_strings(
            exe,
            "@(#)Bash version 0.0.0(0) release GNU".len(),
            extract_bash_version,
        )
        .or_else(|| version_spawn_last(exe)),
        _ => None,
    }
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some((name, exe)) = detect_shell() else {
        return Vec::new();
    };

    let value = match shell_version(&name, exe.as_deref()) {
        Some(version) => format!("{name} {version}"),
        None => name,
    };
    vec![Row::new("Shell", value)]
}

#[cfg(test)]
mod tests {
    use super::{extract_bash_version, extract_zsh_version, is_wrapper};

    #[test]
    fn extracts_embedded_versions() {
        // zsh embeds "zsh-<version>-<release>"; the dash is part of the format
        assert_eq!(extract_zsh_version("zsh-5.9.2-0"), Some("5.9.2".to_string()));
        assert_eq!(extract_zsh_version("zsh-5.9.2-dev-something"), Some("5.9.2".to_string()));
        assert_eq!(extract_zsh_version("zsh-5.9.2"), None);
        assert_eq!(extract_zsh_version("unrelated string"), None);

        assert_eq!(
            extract_bash_version("@(#)Bash version 5.2.26(1)-release GNU"),
            Some("5.2.26".to_string())
        );
        assert_eq!(extract_bash_version("no marker here"), None);
    }

    #[test]
    fn wrappers_are_skipped() {
        assert!(is_wrapper("sudo"));
        assert!(is_wrapper("ld-debug-wrapper"));
        assert!(is_wrapper("run-script.sh"));
        assert!(!is_wrapper("zsh"));
        assert!(!is_wrapper("fish"));
    }
}

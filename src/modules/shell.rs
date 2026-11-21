//! `shell` module: the shell from the parent process chain (fastfetch's
//! getShellInfo: walk up from our parent until the user's login shell or any
//! non-wrapper process), falling back to the passwd shell. Versions come
//! from the shell binary's own embedded version string where possible (zsh,
//! bash), so no process is spawned on common setups.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::{terminal, Row},
    platform,
    util,
};

pub struct ShellResult {
    pub process_name: String,
    pub exe: String,
    pub exe_name: String,
    pub exe_path: String,
    pub pretty_name: String,
    pub version: String,
    pub pid: u32,
    pub ppid: u32,
    /// The found process's controlling terminal device minor, -1 when the
    /// walk failed.
    pub tty: i32,
}

impl Default for ShellResult {
    fn default() -> Self {
        Self {
            process_name: String::new(),
            exe: String::new(),
            exe_name: String::new(),
            exe_path: String::new(),
            pretty_name: String::new(),
            version: String::new(),
            pid: 0,
            ppid: 0,
            tty: -1,
        }
    }
}

/// fastfetch's setShellInfoDetails.
fn set_details(process_name: &str, exe_name: &str) -> String {
    match process_name {
        "pwsh" => "PowerShell".to_string(),
        "nu" => "nushell".to_string(),
        "oil.ovm" => "Oils".to_string(),
        "busybox" => "ash".to_string(),
        _ => exe_name.to_string(),
    }
}

/// ffDetectShell. Memoized like upstream's cache entry.
pub fn detect_full() -> &'static ShellResult {
    static CACHE: std::sync::OnceLock<ShellResult> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        let mut result = ShellResult::default();

        let mut pid = unsafe { libc::getppid() } as u32;
        // fastfetch's FFTS_IGNORE_PARENT test hook.
        if std::env::var("FFTS_IGNORE_PARENT").as_deref() == Ok("1")
            && let Some((_, parent, _)) = terminal::process_basic_info_full(pid)
        {
            pid = parent;
        }

        let user_shell = platform::platform().user_shell.clone();
        let user_shell_name = user_shell.rsplit('/').next().unwrap_or("").to_string();

        while pid > 1 {
            let Some((comm, parent, tty)) = terminal::process_basic_info_full(pid) else {
                break;
            };
            if comm != user_shell_name && terminal::shell_is_skipped(&comm) {
                pid = parent;
                continue;
            }
            result.pid = pid;
            result.ppid = parent;
            result.tty = tty;
            (result.exe, result.exe_name, result.exe_path) =
                terminal::process_exe_info(pid, &comm);
            result.process_name = comm;
            break;
        }

        // getUserShellFromEnv: when the walk failed, the passwd shell is it.
        if result.process_name.is_empty() && !user_shell.is_empty() {
            result.exe = user_shell.clone();
            result.exe_name = user_shell.rsplit('/').next().unwrap_or("").to_string();
            result.process_name = result.exe_name.clone();
        }

        if !result.process_name.is_empty() {
            result.pretty_name = set_details(&result.process_name, &result.exe_name);
            let exe_for_version = if result.exe_path.is_empty() {
                result.exe.clone()
            } else {
                result.exe_path.clone()
            };
            result.version = shell_version(&result.exe_name, Some(&exe_for_version))
                .unwrap_or_default();
        }
        result
    })
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

/// Per-shell version resolution, mirroring fastfetch's dispatch (keyed on
/// the exe name). Port more getters as needed.
fn shell_version(exe_name: &str, exe: Option<&str>) -> Option<String> {
    match (exe_name, exe) {
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

pub fn run(instance: &Instance) -> Vec<Row> {
    let result = detect_full();
    if result.process_name.is_empty() {
        return Vec::new();
    }

    let Some(format) = crate::modules::entry_format(instance.entry) else {
        let value = if result.version.is_empty() {
            result.pretty_name.clone()
        } else {
            format!("{} {}", result.pretty_name, result.version)
        };
        return vec![Row::new(instance.display_name("shell"), value)];
    };

    // FF_PRINT_FORMAT_CHECKED's arg list. exe-name is a nullable
    // const char* upstream; the port keeps it as an empty string when
    // unset. pid is uint32_t and tty is int32_t (-1 when the walk fails).
    let args = [
        crate::format_args::FormatArg { name: "process-name", value: crate::format_args::ArgValue::Str(&result.process_name) },
        crate::format_args::FormatArg { name: "exe", value: crate::format_args::ArgValue::Str(&result.exe) },
        crate::format_args::FormatArg { name: "exe-name", value: crate::format_args::ArgValue::Str(&result.exe_name) },
        crate::format_args::FormatArg { name: "version", value: crate::format_args::ArgValue::Str(&result.version) },
        crate::format_args::FormatArg { name: "pid", value: crate::format_args::ArgValue::UInt(result.pid) },
        crate::format_args::FormatArg { name: "pretty-name", value: crate::format_args::ArgValue::Str(&result.pretty_name) },
        crate::format_args::FormatArg { name: "exe-path", value: crate::format_args::ArgValue::Str(&result.exe_path) },
        crate::format_args::FormatArg { name: "tty", value: crate::format_args::ArgValue::Int(result.tty) },
    ];
    let rendered = crate::format_args::parse_format_string(format, &args, args.len() as u32, &crate::format_args::EngineEnv::new(instance));
    vec![Row::new(instance.display_name("shell"), rendered)]
}

/// ffGenerateShellJsonResult: exe first, tty null when the walk failed.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let result = detect_full();
    if result.process_name.is_empty() {
        return JsonOutcome::Error("Couldn't detect shell".to_string());
    }
    let mut obj = Json::obj();
    obj.push("exe", result.exe.clone());
    obj.push("exeName", result.exe_name.clone());
    obj.push("exePath", result.exe_path.clone());
    obj.push("pid", result.pid as u64);
    obj.push("ppid", result.ppid as u64);
    obj.push("processName", result.process_name.clone());
    obj.push("prettyName", result.pretty_name.clone());
    obj.push("version", result.version.clone());
    if result.tty >= 0 {
        obj.push("tty", result.tty as i64);
    } else {
        obj.push("tty", Json::Null);
    }
    JsonOutcome::Result(obj)
}

#[cfg(test)]
mod tests {
    use super::{extract_bash_version, extract_zsh_version};

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
}

//! `initsystem` module: pid 1's name, ported from initsystem_linux.c. The
//! version comes from the binary's embedded strings for systemd (its banner
//! contains " running in "), else `--version` spawns for systemd, dinit and
//! shepherd.

use std::sync::Mutex;
use std::sync::OnceLock;

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

/// The resolved name and version of pid 1, memoized so parallel module
/// threads don't duplicate the binary scan.
/// The resolved pid 1 name and its optional version.
type InitResult = Option<InitSystemResult>;
type VersionExtractor = Box<dyn Fn(&str) -> Option<String>>;

static RESULT: OnceLock<Mutex<InitResult>> = OnceLock::new();

fn result_cell() -> &'static Mutex<InitResult> {
    RESULT.get_or_init(|| Mutex::new(None))
}

/// extractSystemdVersion: "systemd 255.5-1.arch running in system mode".
fn extract_systemd_version(line: &str) -> Option<String> {
    let rest = line.strip_prefix("systemd ")?;
    let end = rest.find(" running in ")?;
    Some(rest[..end].to_string())
}

/// First spawned --version line, reduced per init system.
fn version_spawn(name: &str, exe: &str) -> Option<String> {
    let (program, extract): (&str, VersionExtractor) = match name {
        "systemd" => (
            if exe.ends_with("/systemd") {
                exe
            } else {
                "systemctl"
            },
            // "systemd 255 (255.5-1.arch)..." → text between parens
            Box::new(|output: &str| {
                let start = output.find('(')? + 1;
                let end = output[start..].find(')')? + start;
                Some(output[start..end].to_string())
            }),
        ),
        "dinit" => (
            if exe.ends_with("/dinit") {
                exe
            } else {
                "dinit"
            },
            // "Dinit version 0.18.0." → last space-separated token
            Box::new(|output: &str| {
                let line = util::first_line(output).trim();
                let line = line.strip_suffix('.').unwrap_or(line);
                line.rsplit(' ').next().map(str::to_string)
            }),
        ),
        "shepherd" => (
            if exe.ends_with("/shepherd") {
                exe
            } else {
                "shepherd"
            },
            // The first line may lack the version; then use the next one.
            Box::new(|output: &str| {
                let line = if output.starts_with("shepherd") {
                    util::first_line(output)
                } else {
                    let rest = output.split_once('\n')?.1;
                    util::first_line(rest)
                };
                line.rsplit(' ').next().map(str::to_string)
            }),
        ),
        _ => return None,
    };
    let output = std::process::Command::new(program).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    extract(&String::from_utf8_lossy(&output.stdout))
}

pub struct InitSystemResult {
    pub name: String,
    pub exe: String,
    pub version: String,
    pub pid: u64,
}

fn detect() -> Option<InitSystemResult> {
    let (comm, _) = super::terminal::process_basic_info(1)?;

    // ffProcessGetInfoLinux: arg0 from the cmdline (systemd's is /sbin/init);
    // /proc/1/exe itself is not readable for a user.
    let (mut exe, _, _) = super::terminal::process_exe_info(1, &comm);
    let mut name = comm;
    if exe.starts_with('/') {
        // In some old systems, /sbin/init is a symlink.
        if let Ok(real) = std::fs::canonicalize(&exe) {
            exe = real.to_string_lossy().into_owned();
            name = exe.rsplit('/').next().unwrap_or(&exe).to_string();
        }
    }

    let version = match name.as_str() {
        "systemd" => if exe.is_empty() {
            None
        } else {
            util::binary_extract_strings(
                &exe,
                "systemd 0.0 running in x".len(),
                extract_systemd_version,
            )
        }
        .or_else(|| version_spawn(&name, &exe)),
        "dinit" | "shepherd" => version_spawn(&name, &exe),
        _ => None,
    };
    Some(InitSystemResult {
        name,
        exe,
        version: version.unwrap_or_default(),
        pid: 1,
    })
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let mut cell = match result_cell().lock() {
        Ok(cell) => cell,
        Err(poisoned) => poisoned.into_inner(),
    };
    if cell.is_none() {
        *cell = detect();
    }
    let Some(result) = cell.as_ref() else {
        return Vec::new();
    };

    let value = if result.version.is_empty() {
        result.name.clone()
    } else {
        format!("{} {}", result.name, result.version)
    };
    vec![Row::new(instance.display_name("initsystem"), value)]
}

/// ffGenerateInitSystemJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let mut cell = match result_cell().lock() {
        Ok(cell) => cell,
        Err(poisoned) => poisoned.into_inner(),
    };
    if cell.is_none() {
        *cell = detect();
    }
    match cell.as_ref() {
        Some(result) => {
            let mut obj = Json::obj();
            obj.push("name", result.name.clone());
            obj.push("exe", result.exe.clone());
            obj.push("version", result.version.clone());
            obj.push("pid", result.pid);
            JsonOutcome::Result(obj)
        },
        // ffDetectInitSystem's failure ("Failed to detect the init system
        // process") prints an error row, hidden by default.
        None => JsonOutcome::Error("Failed to detect the init system process".to_string()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn extracts_systemd_banner_version() {
        assert_eq!(
            super::extract_systemd_version("systemd 255.5-1.arch running in system mode"),
            Some("255.5-1.arch".to_string())
        );
        assert_eq!(super::extract_systemd_version("systemd no banner here"), None);
        assert_eq!(super::extract_systemd_version("unrelated"), None);
    }
}

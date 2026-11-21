//! `lm` module: login (display) manager, ported from detection/lm/lm_linux.c.
//! The service name comes from the systemd session's Service property over
//! the system D-Bus, then the private session files, then a scan of
//! root-owned processes without a loginuid; the version spawns per-service
//! helpers.

use crate::{
    dbus,
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

#[derive(Default)]
pub struct LMResult {
    pub service: String,
    pub pretty_name: String,
    pub version: String,
}



/// testLms: the known login manager process names, checked case-insensitively.
const TEST_LMS: &[&str] = &[
    "atrium", "cdm", "entrance", "gdm", "gdm3", "greetd", "lemurs", "lightdm", "lxdm", "ly",
    "plasmalogin", "sddm", "slim", "tbsm", "xdm", "xenodm",
];

/// detectBySystemdDbus: the standard way to query the systemd session
/// service name; requires no $XDG_SESSION_ID.
fn detect_by_systemd_dbus(result: &mut LMResult) -> Option<&'static str> {
    match dbus::get_property_string_on_bus(
        dbus::DBUS_BUS_SYSTEM,
        "org.freedesktop.login1",
        "/org/freedesktop/login1/session/auto",
        "org.freedesktop.login1.Session",
        "Service",
    ) {
        Some(service) => {
            result.service = service;
            None
        },
        None => Some("Failed to load system DBus"),
    }
}

/// detectBySystemdPrivate: $XDG_SESSION_ID → /run/systemd/sessions/<id>.
fn detect_by_systemd_private(result: &mut LMResult) -> Option<&'static str> {
    let mut session_id = std::env::var("XDG_SESSION_ID").unwrap_or_default();
    if session_id.is_empty() {
        // On some incorrectly configured systems, $XDG_SESSION_ID is not set.
        // This is actually buggy, and assumes current user is using DE.
        let uid = crate::platform::platform().uid;
        let Some(display) = util::read_file(&format!("/run/systemd/users/{uid}")).and_then(|content| {
            content
                .lines()
                .find_map(|line| util::parse_prop_line(line, "DISPLAY=").map(str::to_string))
        }) else {
            return Some("Failed to get $XDG_SESSION_ID");
        };
        session_id = display.to_string();
    }

    let content = util::read_file(&format!("/run/systemd/sessions/{session_id}")).and_then(|content| {
        content
            .lines()
            .find_map(|line| util::parse_prop_line(line, "SERVICE=").map(str::to_string))
    });
    match content {
        Some(service) => {
            result.service = service;
            None
        },
        None => Some("Failed to parse /run/systemd/sessions/$XDG_SESSION_ID"),
    }
}

/// detectByProcesses: root-owned processes without a loginuid whose comm is
/// a known login manager.
fn detect_by_processes(result: &mut LMResult) -> Option<&'static str> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Some("opendir(\"/proc\") failed");
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !meta.is_dir() || !name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        // Don't check for processes not owned by root (login managers run as
        // root); "no loginuid" is (uint32_t)-1.
        let Some(loginuid) = util::read_trimmed_in(&entry.path(), "loginuid") else {
            continue;
        };
        let Ok(loginuid) = loginuid.trim().parse::<i64>() else {
            continue;
        };
        if loginuid != -1 {
            continue;
        }
        let Some(comm) = util::read_trimmed_in(&entry.path(), "comm") else {
            continue;
        };
        if let Some(lm) = TEST_LMS.iter().find(|lm| lm.eq_ignore_ascii_case(&comm)) {
            result.service = (*lm).to_string();
            return None;
        }
    }
    Some("Failed to detect login manager by processes")
}

/// getGdmVersion: `gdm3 --version`, then `gdm --version`; "GDM 51.1.1".
fn get_gdm_version(result: &mut LMResult) {
    for (program, param) in [("gdm3", "--version"), ("gdm", "--version")] {
        if let Ok(output) = std::process::Command::new(program).arg(param).output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            let text = text.trim_end_matches('\n');
            if let Some((_, version)) = text.split_once(' ') {
                result.version = version.to_string();
                return;
            }
        }
    }
}

/// getSshdVersion: `sshd -V` stderr, "OpenSSH_9.0p1, OpenSSL...".
fn get_sshd_version(result: &mut LMResult) {
    if let Ok(output) = std::process::Command::new("sshd").arg("-V").output() {
        let text = String::from_utf8_lossy(&output.stderr);
        let text = text.trim_end_matches('\n');
        if let Some((head, _)) = text.split_once(',')
            && let Some((_, version)) = head.split_once('_')
        {
            result.version = version.to_string();
        }
    }
}

/// getXfwmVersion: "This is xfwm4 version 4.18.0 (revision ...)".
fn get_xfwm_version(result: &mut LMResult) {
    if let Ok(output) = std::process::Command::new("xfwm4").arg("--version").output() {
        let text = String::from_utf8_lossy(&output.stdout);
        if let Some((_, rest)) = text.split_once("version ") {
            result.version = rest.split(' ').next().unwrap_or("").to_string();
        }
    }
}

/// getLightdmVersion: `lightdm --version` stderr, "lightdm 1.30.0".
fn get_lightdm_version(result: &mut LMResult) {
    if let Ok(output) = std::process::Command::new("lightdm").arg("--version").output() {
        let text = String::from_utf8_lossy(&output.stderr);
        let text = text.trim_end_matches('\n');
        if let Some((_, version)) = text.split_once(' ') {
            result.version = version.to_string();
        }
    }
}

/// ffDetectLM: the three sources in order, then the pretty name and version.
pub fn detect() -> Result<LMResult, &'static str> {
    let mut result = LMResult::default();

    let mut error = detect_by_systemd_dbus(&mut result);
    if error.is_some() {
        error = detect_by_systemd_private(&mut result);
    }
    if error.is_some() {
        error = detect_by_processes(&mut result);
    }
    if let Some(error) = error {
        return Err(error);
    }

    result.pretty_name = match result.service.as_str() {
        service if service.eq_ignore_ascii_case("atrium") => "Atrium".to_string(),
        service if service.eq_ignore_ascii_case("cdm") => "CDM".to_string(),
        service if service.eq_ignore_ascii_case("entrance") => "Entrance".to_string(),
        service if service.eq_ignore_ascii_case("greetd") => "greetd".to_string(),
        service if service.eq_ignore_ascii_case("lemurs") => "Lemurs".to_string(),
        service if service.eq_ignore_ascii_case("lightdm") => "LightDM".to_string(),
        service if service.eq_ignore_ascii_case("lxdm") => "LXDM".to_string(),
        service if service.eq_ignore_ascii_case("ly") => "Ly".to_string(),
        service if service.eq_ignore_ascii_case("plasmalogin") => "Plasma Login".to_string(),
        service if service.eq_ignore_ascii_case("sddm") => "SDDM".to_string(),
        service if service.eq_ignore_ascii_case("slim") => "SLiM".to_string(),
        service if service.eq_ignore_ascii_case("tbsm") => "TBSM".to_string(),
        service if service.eq_ignore_ascii_case("xdm") => "XDM".to_string(),
        service if service.eq_ignore_ascii_case("xenodm") => "Xenodm".to_string(),
        service if service.starts_with("gdm") => "GDM".to_string(),
        service => service.to_string(),
    };

    if result.service.starts_with("gdm") {
        get_gdm_version(&mut result);
    } else if result.service.starts_with("sddm") {
        // The man-page reader needs zlib; without it the version stays empty.
    } else if result.service.starts_with("xfwm") {
        get_xfwm_version(&mut result);
    } else if result.service.starts_with("lightdm") {
        get_lightdm_version(&mut result);
    } else if result.service.starts_with("sshd") {
        get_sshd_version(&mut result);
    }

    Ok(result)
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(lm) = detect() else {
        // The detection error is an error row, hidden by default.
        return Vec::new();
    };
    if lm.service.is_empty() {
        return Vec::new();
    }

    let value = if lm.version.is_empty() {
        lm.pretty_name.clone()
    } else {
        format!("{} {}", lm.pretty_name, lm.version)
    };
    vec![Row::new(instance.display_name("lm"), value)]
}

/// ffGenerateLMJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(lm) => {
            if lm.service.is_empty() {
                return JsonOutcome::Error("No LM service found".to_string());
            }
            let mut obj = Json::obj();
            obj.push("service", lm.service.clone());
            obj.push("prettyName", lm.pretty_name.clone());
            obj.push("version", lm.version.clone());
            JsonOutcome::Result(obj)
        },
    }
}

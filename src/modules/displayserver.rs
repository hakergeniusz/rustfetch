//! Shared WM/DE detection, ported from detection/displayserver/linux/wmde.c
//! (ffdsDetectWMDE) plus the env-based protocol fallback. One detection
//! result feeds the DE, WM and WMTheme modules, matching upstream's
//! ffConnectDisplayServer caching.

use std::sync::OnceLock;

use crate::{platform, util};

pub const PROTOCOL_TTY: &str = "TTY";
pub const PROTOCOL_X11: &str = "X11";
pub const PROTOCOL_WAYLAND: &str = "Wayland";

/// FFDisplayServerResult's WM/DE identity fields.
pub struct DsResult {
    pub wm_process_name: String,
    pub wm_pretty_name: String,
    pub wm_protocol_name: String,
    pub de_process_name: String,
    pub de_pretty_name: String,
}

/// parseEnv: the desktop identity from the session's environment.
fn parse_env() -> Option<String> {
    for key in [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "CURRENT_DESKTOP",
        "SESSION_DESKTOP",
        "DESKTOP_SESSION",
    ] {
        if let Ok(value) = std::env::var(key)
            && !value.is_empty()
        {
            return Some(value);
        }
    }
    if std::env::var_os("KDE_FULL_SESSION").is_some()
        || std::env::var_os("KDE_SESSION_UID").is_some()
        || std::env::var_os("KDE_SESSION_VERSION").is_some()
    {
        return Some("KDE".to_string());
    }
    if std::env::var_os("GNOME_DESKTOP_SESSION_ID").is_some() {
        return Some("GNOME".to_string());
    }
    if std::env::var_os("MATE_DESKTOP_SESSION_ID").is_some() {
        return Some("Mate".to_string());
    }
    if std::env::var_os("TDE_FULL_SESSION").is_some() {
        return Some("Trinity".to_string());
    }
    if std::env::var_os("HYPRLAND_CMD").is_some() {
        return Some("Hyprland".to_string());
    }
    if std::env::var_os("SWAYSOCK").is_some() {
        return Some("Sway".to_string());
    }
    if std::env::var_os("COSMIC_VERSION").is_some() {
        return Some("COSMIC".to_string());
    }
    // WSLg: a Wayland display plus the WSLg mount.
    if std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::path::Path::new("/mnt/wslg/").is_dir()
    {
        return Some("WSLg".to_string());
    }
    None
}

/// getWMProtocolNameFromEnv: session protocol when the connector did not
/// provide one.
fn protocol_from_env() -> String {
    if let Ok(env) = std::env::var("XDG_SESSION_TYPE") {
        return match env.to_ascii_lowercase().as_str() {
            "wayland" => PROTOCOL_WAYLAND.to_string(),
            "x11" | "xorg" => PROTOCOL_X11.to_string(),
            "tty" => PROTOCOL_TTY.to_string(),
            _ => env,
        };
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("WAYLAND_SOCKET").is_some()
    {
        return PROTOCOL_WAYLAND.to_string();
    }
    if std::env::var_os("DISPLAY").is_some() {
        // XWayland also sets this
        return PROTOCOL_X11.to_string();
    }
    if std::env::var("TERM").is_ok_and(|term| term.eq_ignore_ascii_case("linux")) {
        return PROTOCOL_TTY.to_string();
    }
    String::new()
}

/// applyPrettyNameIfWM: the process-name to pretty-name table.
fn apply_pretty_name_if_wm(result: &mut DsResult, name: &str) {
    if name.is_empty() {
        return;
    }
    let pretty = if name.eq_ignore_ascii_case("kwin")
        || name.to_ascii_lowercase().starts_with("kwin_")
        || name.to_ascii_lowercase().ends_with("-kwin_wayland")
        || name.to_ascii_lowercase().ends_with("-kwin_x11")
    {
        "KWin"
    } else if name.eq_ignore_ascii_case("gnome-shell")
        || name.eq_ignore_ascii_case("gnome shell")
        || name.eq_ignore_ascii_case("gnome-session-binary")
        || name.eq_ignore_ascii_case("Mutter")
    {
        "Mutter"
    } else if name.eq_ignore_ascii_case("cinnamon")
        || name.to_ascii_lowercase().starts_with("cinnamon-")
        || name.eq_ignore_ascii_case("Muffin")
        || name.eq_ignore_ascii_case("Mutter (Muffin)")
    {
        "Muffin"
    } else if name.eq_ignore_ascii_case("sway") {
        "Sway"
    } else if name.eq_ignore_ascii_case("weston") {
        "Weston"
    } else if name.eq_ignore_ascii_case("wayfire") {
        "Wayfire"
    } else if name.eq_ignore_ascii_case("openbox") {
        "Openbox"
    } else if name.eq_ignore_ascii_case("xfwm4") {
        "Xfwm4"
    } else if name.eq_ignore_ascii_case("Marco") || name.eq_ignore_ascii_case("Metacity (Marco)") {
        "Marco"
    } else if name.eq_ignore_ascii_case("xmonad") {
        "XMonad"
    } else if name.eq_ignore_ascii_case("WSLg") {
        "WSLg"
    } else if name.eq_ignore_ascii_case("dwm") {
        "dwm"
    } else if name.eq_ignore_ascii_case("bspwm") {
        "bspwm"
    } else if name.eq_ignore_ascii_case("tinywm") {
        "TinyWM"
    } else if name.eq_ignore_ascii_case("qtile") {
        "Qtile"
    } else if name.eq_ignore_ascii_case("herbstluftwm") {
        "herbstluftwm"
    } else if name.eq_ignore_ascii_case("icewm") {
        "IceWM"
    } else if name.eq_ignore_ascii_case("dtwm") {
        "dtwm"
    } else if name.eq_ignore_ascii_case("fvwm") {
        "fvwm"
    } else if name.eq_ignore_ascii_case("ctwm") {
        "ctwm"
    } else if name.eq_ignore_ascii_case("hyprland") {
        "Hyprland"
    } else if name.eq_ignore_ascii_case("ratpoison") {
        "ratpoison"
    } else {
        return;
    };
    result.wm_pretty_name = pretty.to_string();
}

/// applyNameIfWM: a process name only counts as the WM when it maps to a
/// pretty name; then it becomes the reported process name.
fn apply_name_if_wm(result: &mut DsResult, process_name: &str) {
    apply_pretty_name_if_wm(result, process_name);
    if !result.wm_pretty_name.is_empty() {
        result.wm_process_name = process_name.to_string();
    }
}

/// applyBetterWM: the process name wins outright; an unmapped name becomes
/// its own pretty name.
fn apply_better_wm(result: &mut DsResult, process_name: &str) {
    if process_name.is_empty() {
        return;
    }
    result.wm_process_name = process_name.to_string();
    apply_pretty_name_if_wm(result, process_name);
    if result.wm_pretty_name.is_empty() {
        result.wm_pretty_name = result.wm_process_name.clone();
    }
}

/// applyPrettyNameIfDE: the desktop-identity table, which also fixes the
/// DE process name (and KDE's configured WM).
fn apply_pretty_name_if_de(result: &mut DsResult, name: &str) {
    if name.is_empty() {
        return;
    }
    if name.eq_ignore_ascii_case("KDE")
        || name.eq_ignore_ascii_case("plasma")
        || name.eq_ignore_ascii_case("plasmashell")
        || name.eq_ignore_ascii_case("plasmawayland")
    {
        result.de_process_name = "plasmashell".to_string();
        result.de_pretty_name = "KDE Plasma".to_string();
        apply_better_wm(result, &std::env::var("KDEWM").unwrap_or_default());
    } else if name.eq_ignore_ascii_case("GNOME")
        || name.eq_ignore_ascii_case("ubuntu:GNOME")
        || name.eq_ignore_ascii_case("ubuntu")
        || name.eq_ignore_ascii_case("gnome-shell")
    {
        result.de_process_name = "gnome-shell".to_string();
        result.de_pretty_name = match std::env::var("GNOME_SHELL_SESSION_MODE").as_deref() {
            Ok("classic") => "GNOME Classic".to_string(),
            _ => "GNOME".to_string(),
        };
    } else if name.eq_ignore_ascii_case("X-Cinnamon") || name.eq_ignore_ascii_case("Cinnamon") {
        result.de_process_name = "cinnamon".to_string();
        result.de_pretty_name = "Cinnamon".to_string();
    } else if name.eq_ignore_ascii_case("XFCE")
        || name.eq_ignore_ascii_case("X-XFCE")
        || name.eq_ignore_ascii_case("XFCE4")
        || name.eq_ignore_ascii_case("X-XFCE4")
        || name.eq_ignore_ascii_case("xfce4-session")
    {
        result.de_process_name = "xfce4-session".to_string();
        result.de_pretty_name = "Xfce4".to_string();
    } else if name.eq_ignore_ascii_case("MATE")
        || name.eq_ignore_ascii_case("X-MATE")
        || name.eq_ignore_ascii_case("mate-session")
    {
        result.de_process_name = "mate-session".to_string();
        result.de_pretty_name = "Mate".to_string();
    } else if name.eq_ignore_ascii_case("LXQt")
        || name.eq_ignore_ascii_case("X-LXQt")
        || name.eq_ignore_ascii_case("lxqt-session")
    {
        result.de_process_name = "lxqt-session".to_string();
        result.de_pretty_name = "LXQt".to_string();
        if result.wm_process_name.is_empty() {
            // lxqt's own session config names the window manager;
            // ffParsePropFileConfig searches the XDG config dirs.
            let config_path = std::env::var_os("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .filter(|dir| !dir.as_os_str().is_empty())
                .or_else(|| std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config")))
                .map(|dir| dir.join("lxqt/session.conf"));
            let wm = config_path
                .and_then(|path| util::read_file(path.to_str()?))
                .or_else(|| util::read_file("/etc/xdg/lxqt/session.conf"))
                .and_then(|content| {
                    content.lines().find_map(|line| {
                        let (key, value) = line.split_once('=')?;
                        (key.trim() == "window_manager").then(|| value.trim().to_string())
                    })
                })
                .unwrap_or_default();
            apply_better_wm(result, &wm);
        }
    } else if name.eq_ignore_ascii_case("Budgie")
        || name.eq_ignore_ascii_case("X-Budgie")
        || name.eq_ignore_ascii_case("budgie-desktop")
        || name.eq_ignore_ascii_case("Budgie:GNOME")
    {
        result.de_process_name = "budgie-desktop".to_string();
        result.de_pretty_name = "Budgie".to_string();
    } else if name.eq_ignore_ascii_case("dtsession") {
        result.de_process_name = "dtsession".to_string();
        result.de_pretty_name = "CDE".to_string();
    } else if name.eq_ignore_ascii_case("ukui-session") {
        result.de_process_name = "ukui-session".to_string();
        result.de_pretty_name = "UKUI".to_string();
    } else if name.to_ascii_lowercase().starts_with("unity:unity") {
        result.de_process_name = "unity-session".to_string();
        result.de_pretty_name = "Unity".to_string();
    } else if name.eq_ignore_ascii_case("Enlightenment") {
        result.de_process_name = "enlightenment_start".to_string();
        result.de_pretty_name = "Enlightenment".to_string();
    }
}

/// getFromProcesses: walk /proc for the login user's processes, naming WMs
/// and DEs by exe basename (comm fallback) until both are found.
fn get_from_processes(result: &mut DsResult) {
    let uid = platform::platform().uid;
    let Some(entries) = std::fs::read_dir("/proc").ok() else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        let path = entry.path();
        // loginuid is the identity that matters for the session.
        match util::read_trimmed_in(&path, "loginuid") {
            Some(loginuid) => {
                if loginuid.trim().parse::<u32>().is_ok_and(|value| value != uid) {
                    continue;
                }
            },
            None => continue,
        }
        // The exe link's basename wins; unreadable links fall back to comm.
        let process_name = match std::fs::read_link(path.join("exe")) {
            Ok(exe) => exe
                .file_name()
                .map(|base| base.to_string_lossy().into_owned())
                .unwrap_or_default(),
            Err(_) => util::read_trimmed_in(&path, "comm").unwrap_or_default(),
        };
        if result.de_pretty_name.is_empty() {
            apply_pretty_name_if_de(result, &process_name);
        }
        if result.wm_pretty_name.is_empty() {
            apply_name_if_wm(result, &process_name);
        }
        if !result.de_pretty_name.is_empty() && !result.wm_pretty_name.is_empty() {
            break;
        }
    }
}

/// ffdsDetectWMDE: env first, then the process walk, then the mixed
/// fallbacks that fill only the missing side.
fn detect_wmde(result: &mut DsResult) {
    let env = parse_env();

    if !result.wm_process_name.is_empty() {
        let process_name = result.wm_process_name.clone();
        apply_pretty_name_if_wm(result, &process_name);
        if result.wm_pretty_name.is_empty() {
            result.wm_pretty_name = process_name;
        }
    } else if let Some(env) = &env {
        apply_name_if_wm(result, env);
    }

    if let Some(env) = &env {
        apply_pretty_name_if_de(result, env);
    }
    if !result.de_pretty_name.is_empty() && !result.wm_pretty_name.is_empty() {
        return;
    }

    get_from_processes(result);
    let Some(env) = env else {
        return;
    };
    if (!result.wm_pretty_name.is_empty() && !result.de_pretty_name.is_empty())
        || env.is_empty()
    {
        return;
    }
    // Upstream's first two fallbacks write the same WM fields (both-empty,
    // or WM missing and different from the DE), collapsed here.
    if (result.wm_pretty_name.is_empty() && result.de_pretty_name.is_empty())
        || (result.wm_pretty_name.is_empty()
            && !result.de_process_name.eq_ignore_ascii_case(&env)
            && !result.de_pretty_name.eq_ignore_ascii_case(&env))
    {
        result.wm_process_name = env.clone();
        result.wm_pretty_name = env;
    } else if result.de_pretty_name.is_empty()
        && !result.wm_process_name.eq_ignore_ascii_case(&env)
        && !result.wm_pretty_name.eq_ignore_ascii_case(&env)
    {
        result.de_process_name = env.clone();
        result.de_pretty_name = env;
    }
}

pub fn connect() -> &'static DsResult {
    static DS: OnceLock<DsResult> = OnceLock::new();
    DS.get_or_init(|| {
        let mut result = DsResult {
            wm_process_name: String::new(),
            wm_pretty_name: String::new(),
            wm_protocol_name: String::new(),
            de_process_name: String::new(),
            de_pretty_name: String::new(),
        };
        if result.wm_protocol_name.is_empty() {
            result.wm_protocol_name = protocol_from_env();
        }
        // The TTY protocol has no WM/DE to detect.
        if !result.wm_protocol_name.eq_ignore_ascii_case(PROTOCOL_TTY) {
            detect_wmde(&mut result);
        }
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ds() -> DsResult {
        DsResult {
            wm_process_name: String::new(),
            wm_pretty_name: String::new(),
            wm_protocol_name: String::new(),
            de_process_name: String::new(),
            de_pretty_name: String::new(),
        }
    }

    #[test]
    fn wm_pretty_names_match_the_table() {
        let mut result = ds();
        apply_name_if_wm(&mut result, "gnome-shell");
        assert_eq!(result.wm_pretty_name, "Mutter");
        assert_eq!(result.wm_process_name, "gnome-shell");

        let mut result = ds();
        apply_name_if_wm(&mut result, "some-random-wm");
        assert_eq!(result.wm_pretty_name, "");
        assert_eq!(result.wm_process_name, "");
    }

    #[test]
    fn de_table_fixes_process_names() {
        let mut result = ds();
        apply_pretty_name_if_de(&mut result, "GNOME");
        assert_eq!(result.de_pretty_name, "GNOME");
        assert_eq!(result.de_process_name, "gnome-shell");

        let mut result = ds();
        apply_pretty_name_if_de(&mut result, "XFCE");
        assert_eq!(result.de_pretty_name, "Xfce4");
        assert_eq!(result.de_process_name, "xfce4-session");
    }
}

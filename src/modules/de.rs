//! `de` module: desktop environment from the shared WM/DE detection
//! (displayserver.rs), with version detection ported from
//! detection/de/de_linux.c.

use crate::{
    modules::displayserver,
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

/// Maps XDG_CURRENT_DESKTOP tokens to display names, matching fastfetch.
const DESKTOPS: &[(&str, &str)] = &[
    ("GNOME", "GNOME"),
    ("KDE", "KDE Plasma"),
    ("plasma", "KDE Plasma"),
    ("X-Cinnamon", "Cinnamon"),
    ("Cinnamon", "Cinnamon"),
    ("XFCE", "Xfce"),
    ("MATE", "MATE"),
    ("LXQt", "LXQt"),
    ("LXDE", "LXDE"),
    ("Budgie", "Budgie"),
    ("COSMIC", "COSMIC"),
    ("Hyprland", "Hyprland"),
    ("sway", "Sway"),
    ("Unity", "Unity"),
    ("Pantheon", "Pantheon"),
    ("Deepin", "Deepin"),
];

/// ffDetectDEVersion for the desktops this port supports; the GNOME and
/// Cinnamon sources match upstream, the rest spawn the DE binary like
/// upstream's fallback.
fn de_version(name: &str) -> Option<String> {
    if name == "Cinnamon"
        && let Ok(value) = std::env::var("CINNAMON_VERSION")
        && !value.is_empty()
    {
        return Some(value);
    }

    // fastfetch's getGnomeByDbus: "GNOME Shell 51.1" -> "51.1" without
    // paying for a gnome-shell process.
    if matches!(name, "GNOME" | "GNOME Classic")
        && let Some(version) = crate::dbus::get_property_string(
            "org.gnome.Shell",
            "/org/gnome/Shell",
            "org.gnome.Shell",
            "ShellVersion",
        )
    {
        return Some(version);
    }

    let (program, args): (&str, &[&str]) = match name {
        "GNOME" | "GNOME Classic" => ("gnome-shell", &["--version"]),
        "KDE Plasma" => ("plasmashell", &["--version"]),
        "Xfce4" | "Xfce" => ("xfce4-session", &["--version"]),
        "Mate" | "MATE" => ("mate-session", &["--version"]),
        "LXQt" => ("lxqt-session", &["--version"]),
        "Budgie" => ("budgie-desktop", &["--version"]),
        _ => return None,
    };

    let output = std::process::Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = util::first_line(&stdout).trim();
    // "GNOME Shell 51.1" -> "51.1"; "plasmashell 6.1.4" -> "6.1.4"
    first.split_whitespace().next_back().map(str::to_string)
}

/// The pretty DE name without version detection; shared by the qt/gtk
/// modules for their per-desktop settings sources. Returns the raw token
/// lowercased when it is not a known desktop.
pub fn detect() -> Option<&'static str> {
    let raw = std::env::var("XDG_CURRENT_DESKTOP").ok()?;
    let token = raw.split(':').next()?;
    if token.is_empty() {
        return None;
    }
    DESKTOPS
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(token))
        .map(|(_, pretty)| *pretty)
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let ds = displayserver::connect();
    if ds.de_pretty_name.is_empty() {
        // "No DE found" is an error row, hidden by default.
        return Vec::new();
    }

    // Upstream detects the version before branching on outputFormat.
    let version = de_version(&ds.de_pretty_name);
    let Some(format) = crate::modules::entry_format(instance.entry) else {
        let mut value = ds.de_pretty_name.clone();
        if let Some(version) = version.as_deref() {
            value.push(' ');
            value.push_str(version);
        }
        return vec![Row::new(instance.display_name("de"), value)];
    };

    let version = version.unwrap_or_default();
    let args = [
        crate::format_args::FormatArg { name: "process-name", value: crate::format_args::ArgValue::Str(&ds.de_process_name) },
        crate::format_args::FormatArg { name: "pretty-name", value: crate::format_args::ArgValue::Str(&ds.de_pretty_name) },
        crate::format_args::FormatArg { name: "version", value: crate::format_args::ArgValue::Str(&version) },
    ];
    let rendered = crate::format_args::parse_format_string(format, &args, args.len() as u32, &crate::format_args::EngineEnv::new(instance));
    vec![Row::new(instance.display_name("de"), rendered)]
}

/// ffGenerateDEJsonResult: process and pretty names from the shared
/// detection plus ffDetectDEVersion.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let ds = displayserver::connect();
    if ds.de_pretty_name.is_empty() {
        return JsonOutcome::Error("No DE found".to_string());
    }
    let mut result = Json::obj();
    result.push("processName", ds.de_process_name.clone());
    result.push("prettyName", ds.de_pretty_name.clone());
    result.push("version", de_version(&ds.de_pretty_name).unwrap_or_default());
    JsonOutcome::Result(result)
}

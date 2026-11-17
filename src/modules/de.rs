//! `de` module: desktop environment from XDG_CURRENT_DESKTOP, with version
//! detection ported from fastfetch's per-DE sources.

use crate::{instance::Instance, modules::Row, util};

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

/// Version string for the desktop environment, when it can be detected.
/// Mirrors fastfetch: one spawn of the DE's own binary, last token.
fn de_version(name: &str) -> Option<String> {
    if name == "Cinnamon" {
        return std::env::var("CINNAMON_VERSION").ok();
    }
    let (program, args): (&str, &[&str]) = match name {
        "GNOME" | "GNOME Classic" => ("gnome-shell", &["--version"]),
        "KDE Plasma" => ("plasmashell", &["--version"]),
        "Xfce" => ("xfce4-session", &["--version"]),
        "MATE" => ("mate-session", &["--version"]),
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

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(raw) = std::env::var("XDG_CURRENT_DESKTOP") else {
        return Vec::new();
    };

    // The variable can be a colon-separated list; take the first entry.
    let token = raw.split(':').next().unwrap_or("");

    let name = DESKTOPS
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(token))
        .map(|(_, pretty)| *pretty)
        .unwrap_or(token);

    if name.is_empty() {
        return Vec::new();
    }

    let value = match de_version(name) {
        Some(version) => format!("{name} {version}"),
        None => name.to_string(),
    };
    vec![Row::new("Desktop Environment", value)]
}

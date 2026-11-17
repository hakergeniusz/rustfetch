//! `de` module: desktop environment from XDG_CURRENT_DESKTOP.

use crate::{instance::Instance, modules::Row};

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
    vec![Row::new("DE", name.to_string())]
}

//! `wm` module: window manager detected from running process names.

use std::collections::HashSet;

use crate::{instance::Instance, modules::Row};

/// Compositors and window managers to look for, in lookup order.
const WINDOW_MANAGERS: &[&str] = &[
    "Hyprland",
    "sway",
    "wayfire",
    "river",
    "labwc",
    "niri",
    "weston",
    "kwin_wayland",
    "kwin_x11",
    "mutter",
    "gnome-shell",
    "openbox",
    "i3",
    "bspwm",
    "dwm",
    "awesome",
    "fluxbox",
    "jwm",
    "icewm",
    "mwm",
    "twm",
    "cwm",
    "spectrwm",
    "xmonad",
    "qtile",
    "enlightenment",
];

/// Processes running in /proc right now (comm names, truncated to 15 chars
/// by the kernel, exactly what we match against).
fn process_names() -> HashSet<String> {
    std::fs::read_dir("/proc")
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit())
                })
                .filter_map(|entry| {
                    std::fs::read_to_string(entry.path().join("comm"))
                        .ok()
                        .map(|comm| comm.trim().to_string())
                        .filter(|comm| !comm.is_empty())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Maps raw comm names to the names fastfetch would report.
fn pretty_name(comm: &str) -> &str {
    match comm {
        "gnome-shell" => "Mutter",
        "kwin_wayland" | "kwin_x11" => "KWin",
        other => other,
    }
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let names = process_names();
    let Some(found) = WINDOW_MANAGERS.iter().find(|name| names.contains(**name)) else {
        return Vec::new();
    };

    vec![Row::new("WM", pretty_name(found).to_string())]
}

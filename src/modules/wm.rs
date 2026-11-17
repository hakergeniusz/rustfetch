//! `wm` module: window manager detected from running process names.

use crate::{instance::Instance, modules::Row, util};

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

/// Maps raw comm names to the names fastfetch would report.
fn pretty_name(comm: &str) -> &str {
    match comm {
        "gnome-shell" => "Mutter",
        "kwin_wayland" | "kwin_x11" => "KWin",
        other => other,
    }
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let names = util::process_comms();
    let Some(found) = WINDOW_MANAGERS.iter().find(|name| names.contains(**name)) else {
        return Vec::new();
    };

    // fastfetch notes the session type on Wayland ("Mutter (Wayland)").
    let suffix = match std::env::var("XDG_SESSION_TYPE").as_deref() {
        Ok("wayland") => " (Wayland)",
        Ok("x11") => " (x11)",
        _ => "",
    };

    vec![Row::new("Window Manager", format!("{}{suffix}", pretty_name(found)))]
}

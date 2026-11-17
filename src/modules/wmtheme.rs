//! `wmtheme` module: window manager theme from GNOME settings
//! (org.gnome.desktop.wm.preferences theme).

use crate::{instance::Instance, modules::Row};

pub fn run(instance: &Instance) -> Vec<Row> {
    match super::gtk::get(instance, "org.gnome.desktop.wm.preferences", "theme") {
        Some(theme) => vec![Row::new("WM Theme", theme)],
        None => Vec::new(),
    }
}

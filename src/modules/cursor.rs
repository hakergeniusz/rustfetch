//! `cursor` module: cursor theme and size from GNOME settings, fastfetch
//! format ("Adwaita (24px)").

use crate::{instance::Instance, modules::Row};

pub fn run(instance: &Instance) -> Vec<Row> {
    let Some(theme) = super::gtk::get(instance, "org.gnome.desktop.interface", "cursor-theme") else {
        return Vec::new();
    };

    let value = match super::gtk::get(instance, "org.gnome.desktop.interface", "cursor-size") {
        Some(size) if size.parse::<u32>().is_ok() => format!("{theme} ({size}px)"),
        _ => theme,
    };
    vec![Row::new("Cursor", value)]
}

//! `theme` module: GTK theme from GNOME settings.

use crate::{instance::Instance, modules::Row};

pub fn run(instance: &Instance) -> Vec<Row> {
    match super::gtk::get(instance, "org.gnome.desktop.interface", "gtk-theme") {
        Some(theme) => vec![Row::new("Theme", theme)],
        None => Vec::new(),
    }
}

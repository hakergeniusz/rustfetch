//! `icons` module: icon theme from GNOME settings.

use crate::{instance::Instance, modules::Row};

pub fn run(instance: &Instance) -> Vec<Row> {
    match super::gtk::get(instance, "org.gnome.desktop.interface", "icon-theme") {
        Some(icons) => vec![Row::new("Icons", icons)],
        None => Vec::new(),
    }
}

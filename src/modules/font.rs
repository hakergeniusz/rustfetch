//! `font` module: desktop font from GNOME settings, fastfetch format
//! ("Adwaita Sans (11pt)").

use crate::{instance::Instance, modules::Row};

pub fn run(instance: &Instance) -> Vec<Row> {
    let Some(font) = super::gtk::get(instance, "org.gnome.desktop.interface", "font-name") else {
        return Vec::new();
    };

    let value = match super::gtk::font_name_and_pt(&font) {
        (name, Some(size)) => format!("{name} ({size}pt)"),
        (name, None) => name.to_string(),
    };
    vec![Row::new("Font", value)]
}

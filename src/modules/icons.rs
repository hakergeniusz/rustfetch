//! `icons` module: Qt (kdeglobals) and GTK (per-version) icon themes joined
//! with fastfetch's icons1/icons2 layout ("Breeze [Qt], Adwaita [GTK2/3/4]").

use crate::{instance::Instance, modules::Row};

pub fn run(instance: &Instance) -> Vec<Row> {
    let qt = super::qt::detect(instance);
    let versions = super::gtk::detect(instance);
    let gtk = super::gtk::parse_gtk(
        versions.v2.icons.as_deref(),
        versions.v3.icons.as_deref(),
        versions.v4.icons.as_deref(),
    );

    let mut value = String::new();
    if let Some(icons) = &qt.icons {
        value.push_str(icons);
        value.push_str(" [Qt]");
    }
    if let Some(icons) = gtk {
        if !value.is_empty() {
            value.push_str(", ");
        }
        value.push_str(&icons);
    }

    if value.is_empty() {
        Vec::new()
    } else {
        vec![Row::new("Icons", value)]
    }
}

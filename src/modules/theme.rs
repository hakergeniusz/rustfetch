//! `theme` module: GTK theme per version with source tags, ported from
//! fastfetch's theme module (the Qt/KDE half arrives with kdeglobals support).

use crate::{instance::Instance, modules::Row};

pub fn run(instance: &Instance) -> Vec<Row> {
    let versions = super::gtk::detect(instance);
    match super::gtk::parse_gtk(
        versions.v2.theme.as_deref(),
        versions.v3.theme.as_deref(),
        versions.v4.theme.as_deref(),
    ) {
        Some(theme) => vec![Row::new("Theme", theme)],
        None => Vec::new(),
    }
}

//! `icons` module: icon theme per version with source tags, ported from
//! fastfetch's icons module (the Qt/KDE half arrives with kdeglobals support).

use crate::{instance::Instance, modules::Row};

pub fn run(instance: &Instance) -> Vec<Row> {
    let versions = super::gtk::detect(instance);
    match super::gtk::parse_gtk(
        versions.v2.icons.as_deref(),
        versions.v3.icons.as_deref(),
        versions.v4.icons.as_deref(),
    ) {
        Some(icons) => vec![Row::new("Icons", icons)],
        None => Vec::new(),
    }
}

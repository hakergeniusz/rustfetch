//! `font` module: desktop font per version with source tags, ported from
//! fastfetch's font module ("Adwaita Sans (11pt) [GTK2/3/4]"; the Qt half
//! arrives with kdeglobals support).

use crate::{instance::Instance, modules::Row};

/// Pango-style pretty print of a GTK font name ("Adwaita Sans 11" →
/// "Adwaita Sans (11pt)").
fn pretty(font: &Option<String>) -> Option<String> {
    font.as_ref().map(|font| match super::gtk::font_name_and_pt(font) {
        (name, Some(size)) => format!("{name} ({size}pt)"),
        (name, None) => name.to_string(),
    })
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let versions = super::gtk::detect(instance);
    match super::gtk::parse_gtk(
        pretty(&versions.v2.font).as_deref(),
        pretty(&versions.v3.font).as_deref(),
        pretty(&versions.v4.font).as_deref(),
    ) {
        Some(font) => vec![Row::new("Font", font)],
        None => Vec::new(),
    }
}

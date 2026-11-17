//! `font` module: desktop font per source with tags, ported from fastfetch's
//! font module ("Adwaita Sans (11pt) [GTK2/3/4]", Qt first when present).

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
    let qt = super::qt::detect(instance);
    let versions = super::gtk::detect(instance);

    // fastfetch's generateString: the Qt font pretty-printed and tagged,
    // then the GTK values via ffParseGTK
    let mut value = String::new();
    if let Some(font) = qt.font.as_deref() {
        value.push_str(&super::terminalfont::qt_font(font));
        value.push_str(" [Qt]");
    }
    if let Some(font) = super::gtk::parse_gtk(
        pretty(&versions.v2.font).as_deref(),
        pretty(&versions.v3.font).as_deref(),
        pretty(&versions.v4.font).as_deref(),
    ) {
        if !value.is_empty() {
            value.push_str(", ");
        }
        value.push_str(&font);
    }

    if value.is_empty() {
        Vec::new()
    } else {
        vec![Row::new("Font", value)]
    }
}

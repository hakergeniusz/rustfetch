//! `theme` module: Qt (kdeglobals) and GTK (per-version) themes joined with
//! fastfetch's theme1/theme2 layout ("Breeze (Light) [Qt], Adwaita [GTK2/3/4]").

use crate::{instance::Instance, modules::Row};

/// fastfetch's theme1: widget style plus color scheme, tagged [Qt].
fn qt_theme(qt: &super::qt::QtResult) -> Option<String> {
    if qt.widget_style.is_none() && qt.color_scheme.is_none() {
        return None;
    }

    let mut theme = match (&qt.widget_style, &qt.color_scheme) {
        (Some(style), Some(scheme)) => {
            // "Breeze" + "BreezeLight" → "Breeze (Light)"
            let pretty = if scheme.to_ascii_lowercase().starts_with(&style.to_ascii_lowercase()) {
                scheme[style.len()..].trim().to_string()
            } else {
                scheme.clone()
            };
            if pretty.is_empty() {
                format!("{style} ({scheme})")
            } else {
                format!("{style} ({pretty})")
            }
        }
        (Some(style), None) => style.clone(),
        (None, Some(scheme)) => scheme.clone(),
        (None, None) => unreachable!(),
    };
    theme.push_str(" [Qt]");
    Some(theme)
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let qt = super::qt::detect(instance);
    let versions = super::gtk::detect(instance);
    let gtk = super::gtk::parse_gtk(
        versions.v2.theme.as_deref(),
        versions.v3.theme.as_deref(),
        versions.v4.theme.as_deref(),
    );

    let qt_part = qt_theme(&qt);
    let mut value = String::new();
    if let Some(theme) = &qt_part {
        value.push_str(theme);
    }
    if let Some(theme) = gtk {
        if !value.is_empty() {
            value.push_str(", ");
        }
        value.push_str(&theme);
    }

    if value.is_empty() {
        Vec::new()
    } else {
        vec![Row::new("Theme", value)]
    }
}

//! Qt/KDE settings, ported from fastfetch's qt.c: kdeglobals on Plasma,
//! lxqt.conf on LXQt, qt5ct/qt6ct configs otherwise, with the Kvantum
//! redirect. No processes are spawned.

use crate::{instance::Instance, util};

/// Qt side of the theme/icons/font values.
#[derive(Default)]
pub struct QtResult {
    pub widget_style: Option<String>,
    pub color_scheme: Option<String>,
    pub icons: Option<String>,
    pub font: Option<String>,
}

/// fastfetch's detectPlasmaFromFile: values only count inside their section.
fn detect_plasma_file(path: &std::path::Path, result: &mut QtResult) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };

    #[derive(PartialEq)]
    enum Category {
        General,
        Kde,
        Icons,
        Other,
    }
    let mut category = Category::Other;

    for line in content.lines() {
        if line.starts_with('[') {
            let name = line.trim_start_matches('[').trim_end_matches(']');
            category = if name.eq_ignore_ascii_case("General") {
                Category::General
            } else if name.eq_ignore_ascii_case("KDE") {
                Category::Kde
            } else if name.eq_ignore_ascii_case("Icons") {
                Category::Icons
            } else {
                Category::Other
            };
            continue;
        }

        match category {
            Category::Kde if result.widget_style.is_none() => {
                result.widget_style =
                    util::parse_prop_line(line, "widgetStyle =").map(str::to_string);
            }
            Category::Icons if result.icons.is_none() => {
                result.icons = util::parse_prop_line(line, "Theme =").map(str::to_string);
            }
            Category::General => {
                if result.color_scheme.is_none() {
                    result.color_scheme =
                        util::parse_prop_line(line, "ColorScheme =").map(str::to_string);
                }
                if result.font.is_none() {
                    result.font = util::parse_prop_line(line, "font =").map(str::to_string);
                }
                // Before plasma 5.23 "Font" was the key instead of "font"
                if result.font.is_none() {
                    result.font = util::parse_prop_line(line, "Font =").map(str::to_string);
                }
            }
            _ => {}
        }
    }
    true
}

fn detect_plasma() -> QtResult {
    let mut result = QtResult::default();
    let mut found = false;

    for dir in util::config_dirs() {
        if detect_plasma_file(&dir.join("kdeglobals"), &mut result) {
            found = true;
        }
        if result.widget_style.is_some()
            && result.color_scheme.is_some()
            && result.icons.is_some()
            && result.font.is_some()
        {
            return result;
        }
    }

    if !found {
        return result;
    }

    // Plasma discards unset defaults from the config; fastfetch fills them in
    result.widget_style.get_or_insert_with(|| "Breeze".to_string());
    result.color_scheme.get_or_insert_with(|| "BreezeLight".to_string());
    result.icons.get_or_insert_with(|| "Breeze".to_string());
    result.font.get_or_insert_with(|| "Noto Sans, 10".to_string());
    result
}

fn detect_lxqt() -> QtResult {
    let values = util::parse_props_in_config_dirs(
        "lxqt/lxqt.conf",
        &["style = ", "icon_theme = ", "font = "],
    )
    .1;
    QtResult {
        widget_style: values[0].clone(),
        icons: values[1].clone(),
        font: values[2].clone(),
        color_scheme: None,
    }
}

/// qt5ct fonts use \\xHHHH escapes for non-ASCII (fastfetch's #1864 note).
fn unescape_qt5ct_font(font: &str) -> String {
    let mut out = String::with_capacity(font.len());
    let mut chars = font.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'x') {
            let hex: String = chars.clone().skip(1).take(4).collect();
            if hex.chars().all(|c| c.is_ascii_hexdigit()) {
                if let Ok(codepoint) = u32::from_str_radix(&hex, 16)
                    && let Some(decoded) = char::from_u32(codepoint)
                {
                    out.push(decoded);
                }
                // the peeked 'x' plus the four hex digits
                for _ in 0..5 {
                    chars.next();
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn detect_qtct() -> QtResult {
    let Ok(theme) = std::env::var("QT_QPA_PLATFORMTHEME") else {
        return QtResult::default();
    };
    if theme != "qt5ct" && theme != "qt6ct" {
        return QtResult::default();
    }

    let values = util::parse_props_in_config_dirs(
        &format!("{theme}/{theme}.conf"),
        &["style=", "icon_theme=", "general="],
    )
    .1;
    let font = values[2].clone().map(|font| {
        if font.starts_with('@') {
            // QVariant binary encoding; nothing readable to extract
            String::new()
        } else if theme == "qt5ct" {
            unescape_qt5ct_font(&font)
        } else {
            font
        }
    });

    QtResult {
        widget_style: values[0].clone(),
        icons: values[1].clone(),
        font: font.filter(|font| !font.is_empty()),
        color_scheme: None,
    }
}

fn detect_kvantum() -> Option<String> {
    util::parse_props_in_config_dirs("Kvantum/kvantum.kvconfig", &["theme="])
        .1
        .into_iter()
        .next()
        .flatten()
}

pub fn detect(_instance: &Instance) -> QtResult {
    let mut result = match super::de::detect() {
        Some("KDE Plasma") => detect_plasma(),
        Some("LXQt") => detect_lxqt(),
        _ => detect_qtct(),
    };

    if result.widget_style.as_deref().is_some_and(|style| {
        style.eq_ignore_ascii_case("kvantum") || style.eq_ignore_ascii_case("kvantum-dark")
    }) {
        result.widget_style = detect_kvantum();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qt5ct_escapes_decode() {
        assert_eq!(unescape_qt5ct_font("Noto Sans\\x002c 10"), "Noto Sans, 10");
        assert_eq!(unescape_qt5ct_font("plain"), "plain");
    }
}

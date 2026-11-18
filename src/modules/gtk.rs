//! Per-version GTK settings (2/3/4), ported from fastfetch's gtk.c: the
//! desktop settings daemon first (GSettings/dconf read in-process), then
//! config-file fallbacks under the XDG config dirs.

use crate::{instance::Instance, util};

/// Detection result for one GTK version; all fields may be absent.
#[derive(Default)]
pub struct GtkResult {
    pub theme: Option<String>,
    pub icons: Option<String>,
    pub font: Option<String>,
    pub cursor: Option<String>,
    pub cursor_size: Option<u32>,
}

/// All three per-version results, shared by the theme/icons/font/cursor modules.
pub struct GtkVersions {
    pub v2: GtkResult,
    pub v3: GtkResult,
    pub v4: GtkResult,
}

impl GtkVersions {
    /// fastfetch's detectCursorGTK: the newest version that knows the cursor.
    pub fn cursor(&self) -> Option<&GtkResult> {
        match () {
            _ if self.v4.cursor.is_some() => Some(&self.v4),
            _ if self.v3.cursor.is_some() => Some(&self.v3),
            _ if self.v2.cursor.is_some() => Some(&self.v2),
            _ => None,
        }
    }
}

/// Reads one key from a GSettings schema (or "schema:path" relocatable
/// form), unquoting string values, memoized per (schema, key).
pub fn get(instance: &Instance, schema: &str, key: &str) -> Option<String> {
    let mut cache = instance.gnome_settings.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache_key = format!("{schema}\x1f{key}");
    if let Some(value) = cache.get(&cache_key) {
        return value.clone();
    }
    let value = crate::settings::get(schema, key);
    cache.insert(cache_key, value.clone());
    value
}

/// Splits a GTK font name like "Adwaita Sans 11" into ("Adwaita Sans", 11).
pub fn font_name_and_pt(value: &str) -> (&str, Option<u32>) {
    match value.rsplit_once(' ') {
        Some((name, size)) if size.parse::<u32>().is_ok() => (name, size.parse::<u32>().ok()),
        _ => (value, None),
    }
}

/// fastfetch's ffParseGTK: joins the per-version values with [GTKN] tags,
/// grouping equal neighbors ("Adwaita [GTK2/3/4]", "Breeze [GTK2/3], Arc [GTK4]").
pub fn parse_gtk(gtk2: Option<&str>, gtk3: Option<&str>, gtk4: Option<&str>) -> Option<String> {
    let eq = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    match (gtk2, gtk3, gtk4) {
        (Some(a), Some(b), Some(c)) => {
            if eq(a, b) && eq(a, c) {
                Some(format!("{c} [GTK2/3/4]"))
            } else if eq(a, b) {
                Some(format!("{b} [GTK2/3], {c} [GTK4]"))
            } else if eq(b, c) {
                Some(format!("{a} [GTK2], {c} [GTK3/4]"))
            } else {
                Some(format!("{a} [GTK2], {b} [GTK3], {c} [GTK4]"))
            }
        },
        (Some(a), Some(b), None) => {
            if eq(a, b) {
                Some(format!("{b} [GTK2/3]"))
            } else {
                Some(format!("{a} [GTK2], {b} [GTK3]"))
            }
        },
        (Some(a), None, Some(c)) => {
            if eq(a, c) {
                Some(format!("{c} [GTK2/4]"))
            } else {
                Some(format!("{a} [GTK2], {c} [GTK4]"))
            }
        },
        (None, Some(b), Some(c)) => {
            if eq(b, c) {
                Some(format!("{c} [GTK3/4]"))
            } else {
                Some(format!("{b} [GTK3], {c} [GTK4]"))
            }
        },
        (Some(a), None, None) => Some(format!("{a} [GTK2]")),
        (None, Some(b), None) => Some(format!("{b} [GTK3]")),
        (None, None, Some(c)) => Some(format!("{c} [GTK4]")),
        (None, None, None) => None,
    }
}

/// Detects the three GTK versions, ported from fastfetch's detectGTK: the
/// desktop settings daemon fills each result first, config files only fill
/// the still-empty fields.
pub fn detect(instance: &Instance) -> GtkVersions {
    let theme = get(instance, "org.gnome.desktop.interface", "gtk-theme");
    let icons = get(instance, "org.gnome.desktop.interface", "icon-theme");
    let font = get(instance, "org.gnome.desktop.interface", "font-name");
    let cursor = get(instance, "org.gnome.desktop.interface", "cursor-theme");
    let cursor_size = get(instance, "org.gnome.desktop.interface", "cursor-size")
        .and_then(|size| size.parse::<u32>().ok());

    let settings = GtkResult {
        theme,
        icons,
        font,
        cursor,
        cursor_size,
    };

    GtkVersions {
        v2: detect_from_config(&settings, "2"),
        v3: detect_from_config(&settings, "3"),
        v4: detect_from_config(&settings, "4"),
    }
}

/// fastfetch's detectGTKFromConfigDir: fill missing fields from a version's
/// config files, trying each config dir's candidates in fastfetch's order.
fn detect_from_config(settings: &GtkResult, version: &str) -> GtkResult {
    let mut theme = settings.theme.clone();
    let mut icons = settings.icons.clone();
    let mut font = settings.font.clone();
    let mut cursor = settings.cursor.clone();
    let mut cursor_size = settings.cursor_size.map(|size| size.to_string());

    for dir in util::config_dirs() {
        let gtk_dir = dir.join(format!("gtk-{version}.0"));
        let candidates = [
            gtk_dir.join("settings.ini"),
            gtk_dir.join("gtkrc"),
            dir.join(format!("gtkrc-{version}.0")),
            dir.join(format!(".gtkrc-{version}.0")),
        ];
        for file in candidates {
            util::parse_prop_file(
                &file,
                &mut [
                    ("gtk-theme-name =", &mut theme),
                    ("gtk-icon-theme-name =", &mut icons),
                    ("gtk-font-name =", &mut font),
                    ("gtk-cursor-theme-name =", &mut cursor),
                    ("gtk-cursor-theme-size =", &mut cursor_size),
                ],
            );
            if theme.is_some()
                && icons.is_some()
                && font.is_some()
                && cursor.is_some()
                && cursor_size.is_some()
            {
                return GtkResult {
                    theme,
                    icons,
                    font,
                    cursor,
                    cursor_size: cursor_size.and_then(|size| size.parse::<u32>().ok()),
                };
            }
        }
    }

    GtkResult {
        theme,
        icons,
        font,
        cursor,
        cursor_size: cursor_size.and_then(|size| size.parse::<u32>().ok()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_name_and_pt_splits_size() {
        assert_eq!(font_name_and_pt("Adwaita Sans 11"), ("Adwaita Sans", Some(11)));
        assert_eq!(font_name_and_pt("Mono 10.5"), ("Mono 10.5", None));
        assert_eq!(font_name_and_pt("Cantarell"), ("Cantarell", None));
    }

    #[test]
    fn parse_gtk_groups_equal_versions() {
        assert_eq!(parse_gtk(Some("A"), Some("A"), Some("A")), Some("A [GTK2/3/4]".to_string()));
        assert_eq!(
            parse_gtk(Some("A"), Some("A"), Some("B")),
            Some("A [GTK2/3], B [GTK4]".to_string())
        );
        assert_eq!(
            parse_gtk(Some("A"), Some("B"), Some("B")),
            Some("A [GTK2], B [GTK3/4]".to_string())
        );
        assert_eq!(
            parse_gtk(Some("A"), Some("B"), Some("C")),
            Some("A [GTK2], B [GTK3], C [GTK4]".to_string())
        );
        assert_eq!(parse_gtk(Some("A"), Some("a"), None), Some("a [GTK2/3]".to_string()));
        assert_eq!(parse_gtk(None, None, Some("C")), Some("C [GTK4]".to_string()));
        assert_eq!(parse_gtk(None, None, None), None);
    }
}

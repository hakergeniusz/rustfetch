//! `cursor` module: cursor theme and size, ported from fastfetch's cursor
//! detection (GTK newest-first, then env, then index.theme/Xresources files)
//! with fastfetch's print cleanup ("Adwaita (24px)").

use crate::{instance::Instance, modules::Row, util};

/// A detected cursor: theme plus optional size in px.
struct Detected {
    theme: String,
    size: Option<u32>,
}

/// fastfetch's detectCursorHyprcursor, gated on Hyprland running like
/// fastfetch gates on the WM name.
fn from_hyprcursor(comms: &std::collections::HashSet<String>) -> Option<Detected> {
    if !comms.contains("Hyprland") {
        return None;
    }
    let theme = std::env::var("HYPRCURSOR_THEME").ok().filter(|theme| !theme.is_empty())?;
    let size = std::env::var("HYPRCURSOR_SIZE").ok().and_then(|size| size.parse().ok());
    Some(Detected { theme, size })
}

/// fastfetch's detectCursorGTK: newest GTK version that knows the cursor.
fn from_gtk(instance: &Instance) -> Option<Detected> {
    let versions = super::gtk::detect(instance);
    let gtk = versions.cursor()?;
    Some(Detected {
        theme: gtk.cursor.clone()?,
        size: gtk.cursor_size,
    })
}

/// fastfetch's detectCursorFromEnv.
fn from_env() -> Option<Detected> {
    let theme = std::env::var("XCURSOR_THEME").ok().filter(|theme| !theme.is_empty())?;
    let size = std::env::var("XCURSOR_SIZE").ok().and_then(|size| size.parse().ok());
    Some(Detected { theme, size })
}

/// fastfetch's index.theme and Xresources file fallbacks, in order.
fn from_files() -> Option<Detected> {
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::PathBuf::from(home);

        let mut inherits = None;
        util::parse_prop_file(&home.join(".icons/default/index.theme"), &mut [("Inherits =", &mut inherits)]);
        if let Some(theme) = inherits {
            return Some(Detected { theme, size: None });
        }

        let mut theme = None;
        let mut size = None;
        util::parse_prop_file(
            &home.join(".Xresources"),
            &mut [("Xcursor.theme :", &mut theme), ("Xcursor.size :", &mut size)],
        );
        if let Some(theme) = theme {
            return Some(Detected {
                theme,
                size: size.and_then(|size| size.parse().ok()),
            });
        }
    }

    for dir in util::data_dirs() {
        let mut inherits = None;
        util::parse_prop_file(&dir.join("icons/default/index.theme"), &mut [("Inherits =", &mut inherits)]);
        if let Some(theme) = inherits {
            return Some(Detected { theme, size: None });
        }
    }

    None
}

/// fastfetch's print cleanup: drop a trailing "cursors"/"cursor" suffix and
/// separator characters; fall back to "default".
fn pretty_theme(theme: &str) -> String {
    let mut theme = theme.to_string();
    for suffix in ["cursors", "cursor"] {
        if theme.to_ascii_lowercase().ends_with(suffix) {
            theme.truncate(theme.len() - suffix.len());
        }
    }
    theme = theme.trim_end_matches('_').trim_end_matches('-').to_string();
    if theme.is_empty() {
        "default".to_string()
    } else {
        theme
    }
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let comms = util::process_comms();
    let detected = from_hyprcursor(&comms)
        .or_else(|| from_gtk(instance))
        .or_else(from_env)
        .or_else(from_files);

    let Some(detected) = detected else {
        return Vec::new();
    };

    let theme = pretty_theme(&detected.theme);
    let value = match detected.size {
        Some(size) if size > 0 => format!("{theme} ({size}px)"),
        _ => theme,
    };
    vec![Row::new("Cursor", value)]
}

#[cfg(test)]
mod tests {
    use super::pretty_theme;

    #[test]
    fn pretty_theme_strips_suffixes() {
        assert_eq!(pretty_theme("Adwaita"), "Adwaita");
        assert_eq!(pretty_theme("Bibata-Modern-Ice"), "Bibata-Modern-Ice");
        assert_eq!(pretty_theme("MyThemeCursors"), "MyTheme");
        assert_eq!(pretty_theme("MyTheme-cursor"), "MyTheme");
        assert_eq!(pretty_theme("cursor"), "default");
        assert_eq!(pretty_theme("capitaine-cursors"), "capitaine");
    }
}

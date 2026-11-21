//! `cursor` module: cursor theme and size, ported from detection/cursor/
//! cursor_linux.c: WSLg/TTY gates, then Plasma/LXQt config files, Hyprland's
//! hyprcursor env, GTK, XCURSOR_* env and the index.theme/Xresources
//! fallbacks. Detection keeps the raw theme/size; the print cleanup ("Adwaita
//! (24px)") is applied only for text output.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::{displayserver, Row},
    util,
};

#[derive(Default)]
pub struct CursorResult {
    pub theme: String,
    pub size: String,
    pub error: String,
}

/// fastfetch's detectCursorFromConfigFile for Plasma (kcminputrc) and LXQt:
/// defaults apply when the file exists but the keys are missing; a missing
/// file is an error.
fn from_config_file(result: &mut CursorResult, relative: &str, theme_start: &'static str, theme_default: &str, size_start: &'static str, size_default: &str) {
    let (found, values) = util::parse_props_in_config_dirs(
        relative,
        &[theme_start, size_start],
    );
    if found {
        result.theme = values[0]
            .clone()
            .filter(|theme| !theme.is_empty())
            .unwrap_or_else(|| theme_default.to_string());
        result.size = values[1]
            .clone()
            .filter(|size| !size.is_empty())
            .unwrap_or_else(|| size_default.to_string());
    }
    if result.theme.is_empty() {
        result.error = format!("Couldn't find cursor in {relative}");
    }
}

/// fastfetch's detectCursorGTK: the newest GTK version that knows the theme.
fn from_gtk(result: &mut CursorResult, instance: &Instance) -> bool {
    let versions = super::gtk::detect(instance);
    let Some(gtk) = versions.cursor() else {
        return false;
    };
    let Some(theme) = gtk.cursor.clone() else {
        return false;
    };
    result.theme = theme;
    result.size = gtk.cursor_size.clone().unwrap_or_default();
    true
}

/// fastfetch's detectCursorFromEnv.
fn from_env(result: &mut CursorResult) -> bool {
    let Ok(theme) = std::env::var("XCURSOR_THEME") else {
        return false;
    };
    if theme.is_empty() {
        return false;
    }
    result.theme = theme;
    result.size = std::env::var("XCURSOR_SIZE").unwrap_or_default();
    true
}

/// fastfetch's detectCursorHyprcursor.
fn from_hyprcursor(result: &mut CursorResult) -> bool {
    let Ok(theme) = std::env::var("HYPRCURSOR_THEME") else {
        return false;
    };
    if theme.is_empty() {
        return false;
    }
    result.theme = theme;
    result.size = std::env::var("HYPRCURSOR_SIZE").unwrap_or_default();
    true
}

/// ffDetectCursor: the gate chain, sharing upstream's stop-on-found order.
pub fn detect(instance: &Instance) -> CursorResult {
    let mut result = CursorResult::default();
    let wmde = displayserver::connect();

    if wmde.wm_pretty_name == "WSLg" {
        result.error = "WSLg uses native windows cursor".to_string();
    } else if wmde.wm_protocol_name.eq_ignore_ascii_case(displayserver::PROTOCOL_TTY) {
        result.error = "Cursor isn't supported in TTY".to_string();
    } else if wmde.de_pretty_name.eq_ignore_ascii_case("KDE Plasma") {
        from_config_file(&mut result, "kcminputrc", "cursorTheme =", "Breeze", "cursorSize =", "24");
    } else if wmde.de_pretty_name.eq_ignore_ascii_case("LXQt") {
        from_config_file(&mut result, "lxqt/session.conf", "cursor_theme =", "Adwaita", "cursor_size =", "24");
    } else if wmde.wm_pretty_name == "Hyprland" && from_hyprcursor(&mut result) {
        // Hyprland with a hyprcursor theme: nothing else to try.
    } else if !from_gtk(&mut result, instance)
        && !from_env(&mut result)
        && !home_index_theme(&mut result)
        && !from_xresources(&mut result)
        && !data_index_theme(&mut result)
    {
        result.error = "Couldn't find cursor".to_string();
    }
    result
}

/// ~/.icons/default/index.theme: a found file short-circuits the chain
/// even when it has no Inherits key.
fn home_index_theme(result: &mut CursorResult) -> bool {
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let mut theme = None;
    let found = util::parse_prop_file(
        &std::path::PathBuf::from(home).join(".icons/default/index.theme"),
        &mut [("Inherits =", &mut theme)],
    );
    if let Some(theme) = theme {
        result.theme = theme;
    }
    found
}

/// ~/.Xresources: only a non-empty theme counts as found.
fn from_xresources(result: &mut CursorResult) -> bool {
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let mut theme = None;
    let mut size = None;
    util::parse_prop_file(
        &std::path::PathBuf::from(home).join(".Xresources"),
        &mut [
            ("Xcursor.theme :", &mut theme),
            ("Xcursor.size :", &mut size),
        ],
    );
    if let Some(theme) = theme {
        result.theme = theme;
        if let Some(size) = size {
            result.size = size;
        }
        return !result.theme.is_empty();
    }
    false
}

/// icons/default/index.theme under the data dirs.
fn data_index_theme(result: &mut CursorResult) -> bool {
    for dir in util::data_dirs() {
        let mut theme = None;
        let found = util::parse_prop_file(
            &dir.join("icons/default/index.theme"),
            &mut [("Inherits =", &mut theme)],
        );
        if let Some(theme) = theme {
            result.theme = theme;
        }
        if found {
            return true;
        }
    }
    false
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
    let result = detect(instance);
    if !result.error.is_empty() {
        return Vec::new();
    }

    let theme = pretty_theme(&result.theme);
    let value = if !result.size.is_empty() && result.size != "0" {
        format!("{theme} ({}px)", result.size)
    } else {
        theme
    };
    vec![Row::new(instance.display_name("cursor"), value)]
}

/// ffGenerateCursorJsonResult: the raw, uncleaned theme and size.
pub fn json(instance: &Instance) -> JsonOutcome {
    let result = detect(instance);
    if !result.error.is_empty() {
        return JsonOutcome::Error(result.error);
    }
    let mut obj = Json::obj();
    obj.push("theme", result.theme);
    obj.push("size", result.size);
    JsonOutcome::Result(obj)
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

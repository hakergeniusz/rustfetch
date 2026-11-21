//! `wallpaper` module: file path of the current wallpaper, ported from
//! fastfetch's wallpaper_linux.c: the DE settings daemon first (GNOME family,
//! Cinnamon, MATE), then the Qt sources (Plasma's appletsrc, LXQt's pcmanfm).
//! Only the file's basename is printed.

use crate::{
    instance::Instance,
    json::JsonOutcome,
    modules::Row,
    util,
};

/// ffDetectWallpaper's per-DE settings keys.
fn from_settings_daemon(instance: &Instance) -> Option<String> {
    let (schema, key) = match super::de::detect()? {
        "GNOME" | "GNOME Classic" | "Unity" | "Budgie" => {
            ("org.gnome.desktop.background", "picture-uri")
        },
        "Cinnamon" => ("org.cinnamon.desktop.background", "picture-uri"),
        "MATE" => ("org.mate.background", "picture-filename"),
        _ => return None,
    };
    super::gtk::get(instance, schema, key)
}

/// fastfetch's Qt fallback: Plasma stores the wallpaper in the appletsrc.
fn from_kde() -> Option<String> {
    for dir in util::config_dirs() {
        let mut image = None;
        util::parse_prop_file(
            &dir.join("plasma-org.kde.plasma.desktop-appletsrc"),
            &mut [("Image =", &mut image)],
        );
        if let Some(image) = image {
            return Some(image);
        }
    }
    None
}

/// LXQt stores it in pcmanfm-qt's settings.
fn from_lxqt() -> Option<String> {
    util::parse_props_in_config_dirs("pcmanfm-qt/lxqt/settings.conf", &["Wallpaper ="])
        .1
        .into_iter()
        .next()
        .flatten()
}

fn detect(instance: &Instance) -> Option<String> {
    from_settings_daemon(instance)
        .or_else(from_kde)
        .or_else(from_lxqt)
        .map(|wallpaper| wallpaper.strip_prefix("file://").unwrap_or(&wallpaper).to_string())
}

pub fn run(instance: &Instance) -> Vec<Row> {
    detect(instance).map(|wallpaper| {
        // picture-uri is often a file:// URL; only the basename prints.
        let basename = wallpaper.rsplit('/').next().unwrap_or(&wallpaper).to_string();
        vec![Row::new(instance.display_name("wallpaper"), basename)]
    }).unwrap_or_default()
}

/// ffGenerateWallpaperJsonResult: the full path, no basename cleanup.
pub fn json(instance: &Instance) -> JsonOutcome {
    match detect(instance) {
        Some(path) => JsonOutcome::Result(path.into()),
        None => JsonOutcome::Error("Failed to detect the current wallpaper path".to_string()),
    }
}

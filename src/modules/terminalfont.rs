//! `terminalfont` module: per-terminal font detection from config files and
//! GNOME settings, ported from fastfetch's terminalfont detection. Terminals
//! whose fonts only live in their binary (kitty, wezterm, st) are not
//! detected and stay silent, like fastfetch's error rows hidden by default.

use crate::{instance::Instance, modules::Row, util};

/// fastfetch's fontInitPretty: "Name (12pt)" / "Name (11.25pt, Medium)" /
/// "Name (20px)".
fn pretty(name: &str, size: Option<&str>) -> String {
    match size {
        Some(size) if !size.is_empty() => {
            let suffix =
                if size.ends_with("pt") || size.ends_with("px") { "" } else { "pt" };
            format!("{name} ({size}{suffix})")
        }
        _ => name.to_string(),
    }
}

/// Pango font description ("Adwaita Sans 11") split into name and size.
fn pango_font(value: &str) -> String {
    let (name, size) = super::gtk::font_name_and_pt(value);
    pretty(name, size.map(|size| size.to_string()).as_deref())
}

/// Xft pattern ("Family-10", "Family:size=10:weight=bold") split.
fn xft_font(pattern: &str) -> String {
    let head = pattern.split(':').next().unwrap_or(pattern).trim();
    let (family, mut size) = match head.rsplit_once('-') {
        Some((family, tail)) if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit() || c == '.') => {
            (family.trim().to_string(), Some(tail.to_string()))
        }
        _ => (head.to_string(), None),
    };
    for pair in pattern.split(':').skip(1) {
        if let Some(value) = pair.strip_prefix("size=") {
            size = Some(value.to_string());
        } else if let Some(value) = pair.strip_prefix("pixelsize=") {
            size = Some(format!("{value}px"));
        }
    }
    pretty(&family, size.as_deref())
}

/// Qt font string (QFont::toString): family first, size second, the style
/// name last; everything between is QFont plumbing. Shared with the font
/// module's Qt half.
pub fn qt_font(data: &str) -> String {
    let mut fields = data.split(',');
    let family = fields.next().unwrap_or("").trim().to_string();
    let size = fields.next().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    // the style is the LAST field, split into words when alphabetic
    let style = data.rsplit(',').next().unwrap_or("").trim();
    let styles = if style.chars().next().is_some_and(|c| c.is_alphabetic()) && style != "Regular" {
        format!(", {}", style.split(' ').filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", "))
    } else {
        String::new()
    };
    match size {
        Some(size) => format!("{family} ({size}{styles}pt)"),
        None => family,
    }
}

/// Reads one property from a file found under any of the config dirs
/// (fastfetch's ffParsePropFileConfig).
fn parse_prop_file_config(relative: &str, query: &'static str) -> Option<String> {
    for dir in util::config_dirs() {
        let mut value = None;
        util::parse_prop_file(&dir.join(relative), &mut [(query, &mut value)]);
        if value.is_some() {
            return value;
        }
    }
    None
}

fn parse_prop_file_config_2(
    relative: &str,
    query1: &'static str,
    query2: &'static str,
) -> (Option<String>, Option<String>) {
    let mut first = None;
    let mut second = None;
    for dir in util::config_dirs() {
        util::parse_prop_file(
            &dir.join(relative),
            &mut [(query1, &mut first), (query2, &mut second)],
        );
        if first.is_some() && second.is_some() {
            break;
        }
    }
    (first, second)
}

/// fastfetch's system monospace font fallback.
fn system_monospace(instance: &Instance) -> Option<String> {
    super::gtk::get(instance, "org.gnome.desktop.interface", "monospace-font-name")
        .map(|font| pango_font(&font))
}

fn detect_konsole(instance: &Instance, rc_file: &str) -> Option<String> {
    let profile = parse_prop_file_config(rc_file, "DefaultProfile =")?;
    if profile.is_empty() {
        return None;
    }
    for dir in util::data_dirs() {
        let mut font = None;
        util::parse_prop_file(&dir.join(format!("konsole/{profile}")), &mut [("Font =", &mut font)]);
        if let Some(font) = font {
            return Some(qt_font(&font));
        }
    }
    system_monospace(instance)
}

/// GNOME-terminal-style gsettings profile: default profile id from a list
/// schema, then font/use-system-font from the relocatable profile schema.
fn detect_gsettings_profile(
    instance: &Instance,
    list_schema: &str,
    default_key: &str,
    profile_schema_prefix: &str,
    profile_path_prefix: &str,
) -> Option<String> {
    let default = super::gtk::get(instance, list_schema, default_key)?;
    if default.is_empty() {
        return None;
    }
    let schema = format!("{profile_schema_prefix}:{profile_path_prefix}{default}/");
    let font = match super::gtk::get(instance, &schema, "use-system-font").as_deref() {
        Some("false") => super::gtk::get(instance, &schema, "font"),
        _ => super::gtk::get(instance, "org.gnome.desktop.interface", "monospace-font-name"),
    };
    font.map(|font| pango_font(&font))
}

fn detect_gnome_terminal(instance: &Instance) -> Option<String> {
    detect_gsettings_profile(
        instance,
        "org.gnome.Terminal.ProfilesList",
        "default",
        "org.gnome.Terminal.Legacy.Profile",
        "/org/gnome/terminal/legacy/profiles:/:",
    )
}

fn detect_kgx(instance: &Instance) -> Option<String> {
    match super::gtk::get(instance, "org.gnome.Console", "use-system-font").as_deref() {
        Some("false") => super::gtk::get(instance, "org.gnome.Console", "custom-font"),
        _ => super::gtk::get(instance, "org.gnome.desktop.interface", "monospace-font-name"),
    }
    .map(|font| pango_font(&font))
}

fn detect_ptyxis(instance: &Instance) -> Option<String> {
    match super::gtk::get(instance, "org.gnome.Ptyxis", "use-system-font").as_deref() {
        Some("false") => super::gtk::get(instance, "org.gnome.Ptyxis", "font-name"),
        _ => super::gtk::get(instance, "org.gnome.desktop.interface", "monospace-font-name"),
    }
    .map(|font| pango_font(&font))
}

fn detect_alacritty() -> Option<String> {
    // alacritty parses configs in this order; fastfetch queries the same keys
    let mut normal = None;
    let mut family = None;
    let mut style = None;
    let mut size = None;
    let mut found = false;

    for relative in ["alacritty/alacritty.toml", "alacritty.toml", ".alacritty.toml"] {
        for dir in util::config_dirs() {
            util::parse_prop_file(
                &dir.join(relative),
                &mut [
                    ("normal =", &mut normal),
                    ("family =", &mut family),
                    ("style =", &mut style),
                    ("size =", &mut size),
                ],
            );
        }
        if normal.is_some() || family.is_some() || size.is_some() {
            found = true;
            break;
        }
    }
    if !found {
        return None;
    }

    if let Some(normal) = normal {
        family = None;
        // "{ family = "Fira Code", style = "Medium" }" → parse the fields
        let inner = normal.trim().trim_start_matches('{').trim_end_matches('}');
        for pair in inner.split(',') {
            if let Some(value) = util::parse_prop_line(pair, "family =") {
                family = Some(value.to_string());
            } else if let Some(value) = util::parse_prop_line(pair, "style =") {
                style = Some(value.to_string());
            }
        }
    }

    let family = family.unwrap_or_else(|| "monospace".to_string());
    let style = style.unwrap_or_else(|| "Regular".to_string());
    let size = size.unwrap_or_else(|| "11.25".to_string());
    // fastfetch appends the style inside the parens: "Fira Code (11.25pt, Regular)"
    Some(pretty(&family, Some(&size)).replace(')', &format!(", {style})")))
}

fn detect_ghostty() -> Option<String> {
    // the first config dir is the XDG one; includes are followed like
    // fastfetch does, capped at 16 files
    let dir = util::config_dirs().into_iter().next()?;
    let mut font_family = None;
    let mut font_size = None;
    let mut queue = vec![dir.join("ghostty/config.ghostty")];
    let mut loaded = Vec::new();
    while let Some(path) = queue.pop() {
        if loaded.contains(&path) || loaded.len() >= 16 {
            continue;
        }
        loaded.push(path.clone());
        let Some(content) = std::fs::read_to_string(&path).ok().or_else(|| {
            let without = path.with_extension("");
            std::fs::read_to_string(without).ok()
        }) else {
            continue;
        };
        for line in content.lines() {
            if let Some(value) = util::parse_prop_line(line, "font-family =") {
                if value.is_empty() {
                    font_family = None;
                } else if font_family.is_none() {
                    font_family = Some(value.to_string());
                }
            } else if let Some(value) = util::parse_prop_line(line, "font-size =") {
                font_size = Some(value.to_string());
            } else if let Some(value) = util::parse_prop_line(line, "config-file =") {
                let value = value.trim_start_matches('?').trim_matches('"').to_string();
                if value.is_empty() {
                    continue;
                }
                let include = if value.starts_with('/') {
                    std::path::PathBuf::from(value)
                } else {
                    path.parent().unwrap_or(&dir).join(value)
                };
                queue.push(include);
            }
        }
    }
    let family = font_family.unwrap_or_else(|| "JetBrainsMono Nerd Font".to_string());
    let size = font_size.unwrap_or_else(|| "12".to_string());
    Some(pretty(&family, Some(&size)))
}

fn detect_foot() -> Option<String> {
    let font = parse_prop_file_config("foot/foot.ini", "font=")?;
    if font.is_empty() {
        return Some(pretty("monospace", Some("8")));
    }
    // foot splits on commas for fallback fonts; fastfetch prints " / "
    // separated results but the primary font is what shows by default
    let primary = font.split(',').next().unwrap_or(&font).trim();
    Some(xft_font(primary))
}

fn detect_xterm() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let home = std::path::PathBuf::from(home);
    let mut font = None;
    let mut size = None;
    util::parse_prop_file(
        &home.join(".Xresources"),
        &mut [("xterm*faceName:", &mut font), ("xterm*faceSize:", &mut size)],
    );
    if font.is_none() {
        util::parse_prop_file(
            &home.join(".Xresources"),
            &mut [("xterm.vt100.faceName:", &mut font), ("xterm.vt100.faceSize:", &mut size)],
        );
    }
    Some(pretty(
        &font.unwrap_or_else(|| "fixed".to_string()),
        Some(size.unwrap_or_else(|| "8".to_string())).as_deref(),
    ))
}

fn detect_urxvt() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let home = std::path::PathBuf::from(home);
    let mut font = None;
    let found = util::parse_prop_file(&home.join(".Xresources"), &mut [("URxvt.font:", &mut font)]);
    if !found || font.is_none() {
        font = None;
        util::parse_prop_file(&home.join(".Xdefaults"), &mut [("URxvt.font:", &mut font)]);
    }
    let font = font?;
    // first entry of the comma list is the primary font
    let entry = font.split(',').next().unwrap_or(&font).trim();
    if let Some(xft) = entry.strip_prefix("xft:") {
        Some(xft_font(xft))
    } else if entry.starts_with('-') {
        // XLFD: family is the second field, point size the eighth (in 1/10 pt)
        let fields: Vec<&str> = entry.split('-').collect();
        let family = fields.get(2).copied().unwrap_or("fixed").to_string();
        let size = fields
            .get(7)
            .and_then(|point| point.parse::<u32>().ok())
            .map(|point| format!("{}.0", point as f64 / 10.0));
        Some(pretty(&family, size.as_deref()))
    } else {
        None
    }
}

fn detect_warp() -> Option<String> {
    for dir in util::config_dirs() {
        let Ok(content) = std::fs::read_to_string(dir.join("warp-terminal/user_preferences.json")) else {
            continue;
        };
        let extract = |key: &str| -> Option<String> {
            let needle = format!("\"{key}\"");
            let start = content.find(&needle)? + needle.len();
            let rest = &content[start..];
            let quote = rest.find('"')? + 1;
            let end = rest[quote..].find('"')? + quote;
            Some(rest[quote..end].to_string())
        };
        return Some(pretty(
            &extract("FontName").unwrap_or_else(|| "Hack".to_string()),
            Some(extract("FontSize").unwrap_or_else(|| "13".to_string())).as_deref(),
        ));
    }
    None
}

fn detect_cosmic_term() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let base = std::path::PathBuf::from(home).join(".config/cosmic/com.system76.CosmicTerm/v1");
    let font = util::read_trimmed_in(&base, "font_name").unwrap_or_else(|| "Noto Sans Mono".to_string());
    let size = match util::read_trimmed_in(&base, "font_size") {
        Some(size) => format!("{size}px"),
        None => "14px".to_string(),
    };
    Some(pretty(&font, Some(&size)))
}

/// Config pairs of a font name and its size, with fastfetch's defaults.
fn detect_font_size_pair(
    relative: &str,
    font_query: &'static str,
    size_query: &'static str,
    fallback_name: &str,
    fallback_size: &str,
) -> Option<String> {
    let values = util::parse_props_in_config_dirs(relative, &[font_query, size_query]).1;
    Some(pretty(
        &values[0].clone().unwrap_or_else(|| fallback_name.to_string()),
        Some(values[1].clone().unwrap_or_else(|| fallback_size.to_string())).as_deref(),
    ))
}

/// deepin-terminal: values live in two [basic.interface.*] sections, so the
/// value following each section header is the answer.
fn detect_deepin() -> Option<String> {
    for dir in util::config_dirs() {
        let content = std::fs::read_to_string(dir.join("deepin/deepin-terminal/config.conf")).ok();
        let Some(content) = content else { continue };
        let section_value = |header: &str| -> Option<String> {
            let mut lines = content.lines();
            while let Some(line) = lines.next() {
                if line.trim() == header {
                    return lines.next().and_then(|line| util::parse_prop_line(line, "value=").map(str::to_string));
                }
            }
            None
        };
        let font = section_value("[basic.interface.font]").unwrap_or_else(|| "Noto Sans Mono".to_string());
        let size = section_value("[basic.interface.font_size]").unwrap_or_else(|| "11".to_string());
        return Some(pretty(&font, Some(&size)));
    }
    None
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Some(terminal) = super::terminal::detect_raw() else {
        return Vec::new();
    };
    let name = terminal.to_ascii_lowercase();

    let value = if name.starts_with("konsole") {
        detect_konsole(instance, "konsolerc")
    } else if name.starts_with("yakuake") {
        detect_konsole(instance, "yakuakerc")
    } else if name.starts_with("gnome-terminal") {
        detect_gnome_terminal(instance)
    } else if name == "kgx" {
        detect_kgx(instance)
    } else if name.starts_with("ptyxis-agent") || name.starts_with("ptyxis") {
        detect_ptyxis(instance)
    } else if name == "tilix" {
        detect_gsettings_profile(
            instance,
            "com.gexperts.Tilix.ProfilesList",
            "default",
            "com.gexperts.Tilix.Profile",
            "/com/gexperts/Tilix/profiles/:",
        )
    } else if name == "mate-terminal" {
        detect_gsettings_profile(
            instance,
            "org.mate.terminal.global",
            "default-profile",
            "org.mate.terminal.profile",
            "/org/mate/terminal/profiles/:",
        )
    } else if name.starts_with("alacritty") {
        detect_alacritty()
    } else if name.starts_with("ghostty") {
        detect_ghostty()
    } else if name == "foot" {
        detect_foot()
    } else if name == "qterminal" {
        detect_font_size_pair("qterminal.org/qterminal.ini", "fontFamily=", "fontSize=", "monospace", "12")
    } else if name == "xterm" {
        detect_xterm()
    } else if name == "rxvt" || name == "urxvt" || name == "urxvtd" {
        detect_urxvt()
    } else if name.starts_with("terminator") {
        let (use_system, font) = parse_prop_file_config_2("terminator/config", "use_system_font =", "font =");
        match use_system.as_deref() {
            Some("True") | Some("true") | None => system_monospace(instance),
            _ => Some(pango_font(&font.unwrap_or_else(|| "Mono 10".to_string()))),
        }
    } else if name.starts_with("sakura") {
        parse_prop_file_config("sakura/sakura.conf", "font=").map(|font| pango_font(&font))
    } else if name.starts_with("termite") {
        parse_prop_file_config("termite/config", "font =").map(|font| pango_font(&font))
    } else if name.starts_with("lxterminal") {
        parse_prop_file_config("lxterminal/lxterminal.conf", "fontname =").map(|font| pango_font(&font))
    } else if name.starts_with("weston-terminal") {
        detect_font_size_pair("weston.ini", "font=", "font-size=", "DejaVu Sans Mono", "14")
    } else if name == "kmscon" {
        detect_font_size_pair("kmscon/kmscon.conf", "font-name=", "font-size=", "Hack Nerd Font", "18")
    } else if name.starts_with("deepin-terminal") {
        detect_deepin()
    } else if name.starts_with("cosmic-term") {
        detect_cosmic_term()
    } else if name == "warp" {
        detect_warp()
    } else {
        None
    };

    value.map(|font| Row::new("Terminal Font", font)).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_formats_sizes() {
        assert_eq!(pretty("Adwaita Mono", Some("11")), "Adwaita Mono (11pt)");
        assert_eq!(pretty("Fira Code", Some("11.25")), "Fira Code (11.25pt)");
        assert_eq!(pretty("Terminus", Some("20px")), "Terminus (20px)");
        assert_eq!(pretty("Just A Font", None), "Just A Font");
    }

    #[test]
    fn xft_parsing() {
        assert_eq!(xft_font("Berkeley Mono-12"), "Berkeley Mono (12pt)");
        assert_eq!(xft_font("Sarasa Term SC:size=8"), "Sarasa Term SC (8pt)");
        assert_eq!(xft_font("Term:pixelsize=16"), "Term (16px)");
        assert_eq!(xft_font("Just Family"), "Just Family");
    }

    #[test]
    fn qt_parsing() {
        assert_eq!(qt_font("Noto Sans Mono,10,-1,5,50,0,0,0,0,0"), "Noto Sans Mono (10pt)");
        assert_eq!(qt_font("Noto Sans Mono,10,bold"), "Noto Sans Mono (10, boldpt)");
        assert_eq!(qt_font("Noto Sans Mono,10,Regular"), "Noto Sans Mono (10pt)");
    }
}

//! `wmtheme` module: ported from detection/wmtheme/wmtheme_linux.c,
//! dispatching on the shared WM pretty name. Detection returns the theme
//! or the exact upstream error message, which becomes a hidden text row
//! or the JSON error field.

use crate::{
    modules::displayserver,
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

/// detectWMThemeFromSettings: one GSettings/dconf key.
fn detect_from_settings(instance: &Instance, schema: &str, key: &str) -> Result<String, String> {
    super::gtk::get(instance, schema, key)
        .filter(|theme| !theme.is_empty())
        .ok_or_else(|| "Couldn't find WM theme in DConf or GSettings".to_string())
}

/// detectGTKThemeAsWMTheme: the first of GTK4/3/2 that knows a theme.
fn detect_gtk_theme_as_wm_theme(instance: &Instance) -> Result<String, String> {
    let gtk = super::gtk::detect(instance);
    for gtk_result in [&gtk.v4, &gtk.v3, &gtk.v2] {
        if let Some(theme) = &gtk_result.theme
            && !theme.is_empty()
        {
            return Ok(theme.clone());
        }
    }
    Err("Couldn't detect GTK4/3/2 theme".to_string())
}

/// detectMutter: the user-theme extension first, then the GTK theme.
fn detect_mutter(instance: &Instance) -> Result<String, String> {
    if let Some(theme) = super::gtk::get(instance, "org.gnome.shell.extensions.user-theme", "name")
        .filter(|theme| !theme.is_empty())
    {
        return Ok(theme);
    }
    detect_gtk_theme_as_wm_theme(instance)
}

/// detectMuffin: cinnamon's theme name and WM theme combine.
fn detect_muffin(instance: &Instance) -> Result<String, String> {
    let name = super::gtk::get(instance, "org.cinnamon.theme", "name");
    let theme = super::gtk::get(instance, "org.cinnamon.desktop.wm.preferences", "theme");
    match (name, theme) {
        (None, None) => Err("Couldn't find muffin theme in GSettings / DConf".to_string()),
        (None, Some(theme)) => Ok(theme),
        (Some(name), None) => Ok(name),
        (Some(name), Some(theme)) => Ok(format!("{name} ({theme})")),
    }
}

/// detectWMThemeFromConfigFile: one `theme =` line in an XDG config file,
/// with a default when the key is missing; qml_/svg__ prefixes are stripped.
fn detect_from_config_file(config_file: &str, default_value: Option<&str>) -> Result<String, String> {
    let content = util::config_dirs()
        .iter()
        .find_map(|dir| dir.join(config_file).to_str().and_then(util::read_file))
        .ok_or_else(|| format!("Config file {config_file} doesn't exist"))?;

    let mut theme = content
        .lines()
        .find_map(|line| util::parse_prop_line(line, "theme ="))
        .unwrap_or_default()
        .to_string();

    if theme.is_empty() {
        let Some(default_value) = default_value else {
            return Err(format!("Couldn't find WM theme in {config_file}"));
        };
        return Ok(default_value.to_string());
    }
    if let Some(index) = theme.find("qml_") {
        theme = theme[index + "qml_".len()..].to_string();
    }
    if let Some(index) = theme.find("svg__") {
        theme = theme[index + "svg__".len()..].to_string();
    }
    Ok(theme)
}

/// The <name> value inside the <theme> node; the name must be a theme
/// child (it ends before </theme>).
fn openbox_theme_name(content: &str) -> Option<String> {
    let theme_start = content.find("<theme>")?;
    let theme_end = theme_start + content[theme_start..].find("</theme>")?;
    let theme_slice = &content[theme_start..theme_end];
    let name_start = theme_slice.find("<name>")? + "<name>".len();
    let name = theme_slice[name_start..].split("</name>").next()?;
    let trimmed = name.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// detectOpenbox: the <name> inside the <theme> node of openbox's rc.xml.
fn detect_openbox(de_pretty_name: &str) -> Result<String, String> {
    let config_file_subpath = match de_pretty_name {
        pretty if pretty.eq_ignore_ascii_case("LXQt") => "openbox/lxqt-rc.xml",
        pretty if pretty.eq_ignore_ascii_case("LXDE") => "openbox/lxde-rc.xml",
        _ => "openbox/rc.xml",
    };
    let path = util::config_dirs()
        .iter()
        .map(|dir| dir.join(config_file_subpath))
        .find(|path| path.exists())
        .ok_or_else(|| format!("Couldn't find config file \"{config_file_subpath}\""))?;
    let content = util::read_file(path.to_str().unwrap_or(""))
        .ok_or_else(|| format!("Couldn't read \"{}\"", path.to_string_lossy()))?;

    openbox_theme_name(&content).ok_or_else(|| {
        // Upstream distinguishes a missing node from a missing name; the
        // shared message keeps the common case (no name) exact.
        format!("Couldn't find theme name in \"{}\"", path.to_string_lossy())
    })
}

/// ffDetectWmTheme: dispatch on the WM pretty name.
pub fn detect(instance: &Instance) -> Result<String, String> {
    let wm = displayserver::connect();
    let pretty_name = &wm.wm_pretty_name;
    if pretty_name.is_empty() {
        return Err("WM Theme needs successful WM detection".to_string());
    }

    if pretty_name.eq_ignore_ascii_case("KWin") {
        return detect_from_config_file("kwinrc", Some("Breeze"));
    }
    if pretty_name.eq_ignore_ascii_case("Mutter") {
        if wm.de_pretty_name.eq_ignore_ascii_case("GNOME")
            || wm.de_pretty_name.eq_ignore_ascii_case("GNOME Classic")
        {
            return detect_mutter(instance);
        }
        return detect_gtk_theme_as_wm_theme(instance);
    }
    if pretty_name.eq_ignore_ascii_case("Muffin") {
        return detect_muffin(instance);
    }
    if pretty_name.eq_ignore_ascii_case("Marco") {
        return detect_from_settings(instance, "org.mate.Marco.general", "theme");
    }
    if pretty_name.eq_ignore_ascii_case("Openbox") {
        return detect_openbox(&wm.de_pretty_name);
    }
    if pretty_name.eq_ignore_ascii_case("Enlightenment") {
        return detect_gtk_theme_as_wm_theme(instance);
    }
    // Xfwm4 needs XFConf and cosmic-comp reads its own theme files; neither
    // reader exists in this port yet, so both report their upstream
    // not-found errors.
    Err(format!("Unknown WM: {pretty_name}"))
}

pub fn run(instance: &Instance) -> Vec<Row> {
    match detect(instance) {
        Ok(theme) => vec![Row::new("WM Theme", theme)],
        // Detection errors are hidden rows by default.
        Err(_) => Vec::new(),
    }
}

pub fn json(instance: &Instance) -> JsonOutcome {
    match detect(instance) {
        Ok(theme) => JsonOutcome::Result(Json::Str(theme)),
        Err(message) => JsonOutcome::Error(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openbox_theme_name_parses() {
        assert_eq!(
            openbox_theme_name("<openbox_config><theme><name>PiXol</name></theme></openbox_config>"),
            Some("PiXol".to_string())
        );
        // A <name> outside the theme node does not count.
        assert_eq!(
            openbox_theme_name("<theme></theme><name>Elsewhere</name>"),
            None
        );
        // An empty name is not a theme.
        assert_eq!(openbox_theme_name("<theme><name>  </name></theme>"), None);
        assert_eq!(openbox_theme_name("no nodes at all"), None);
    }
}

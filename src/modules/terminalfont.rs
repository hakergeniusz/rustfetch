//! `terminalfont` module: per-terminal font detection, ported from
//! detection/terminalfont/terminalfont.c (the common getters) and
//! terminalfont_linux.c (the platform getters). Fonts decompose into an
//! FFfont (name/size/styles/pretty); some terminals also report a fallback
//! font (ghostty, foot, urxvt). Terminals without a getter report
//! "Unknown terminal: <name>".

use crate::{
    ffont::FFfont,
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::{displayserver, Row},
    util,
};

use super::terminal::{self, TerminalResult};

#[derive(Default)]
pub struct TerminalFontResult {
    pub font: FFfont,
    pub fallback: FFfont,
    pub error: String,
}



/// ffProcessAppendStdOut: Ok(stdout) or the exact upstream error messages.
fn process_stdout(argv: &[&str]) -> Result<String, String> {
    let mut command = std::process::Command::new(argv[0]);
    command.args(&argv[1..]).env("LANG", "C.UTF-8");
    match command.output() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err("command not found".to_string())
        },
        Err(_) => Err("posix_spawnp() failed".to_string()),
        Ok(output) => match output.status.code() {
            Some(127) => Err("command not found".to_string()),
            None => Err("child process exited abnormally".to_string()),
            Some(_) => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        },
    }
}

// ---- shared sources ----

/// getSystemMonospaceFont: the DE-specific interface schema first.
fn system_monospace_font(instance: &Instance) -> Option<String> {
    let ds = displayserver::connect();
    let schema = if ds.de_pretty_name.eq_ignore_ascii_case("Cinnamon") {
        "org.cinnamon.desktop.interface:/org/cinnamon/desktop/interface/monospace-font-name"
    } else if ds.de_pretty_name.eq_ignore_ascii_case("Mate") {
        "org.mate.interface:/org/mate/interface/monospace-font-name"
    } else {
        "org.gnome.desktop.interface:/org/gnome/desktop/interface/monospace-font-name"
    };
    super::gtk::get(instance, schema, "monospace-font-name").filter(|font| !font.is_empty())
}

fn apply_system_monospace(result: &mut TerminalFontResult, instance: &Instance) {
    match system_monospace_font(instance) {
        Some(font) => result.font = FFfont::init_pango(&font),
        None => {
            result.error = "Couldn't get system monospace font name from GSettings / DConf".to_string()
        },
    }
}

/// detectKgx (gnome console): no profiles, a single custom-font key.
fn detect_kgx(result: &mut TerminalFontResult, instance: &Instance) {
    if super::gtk::get(instance, "org.gnome.Console:/org/gnome/Console", "use-system-font")
        .as_deref()
        != Some("true")
    {
        match super::gtk::get(instance, "org.gnome.Console:/org/gnome/Console", "custom-font")
            .filter(|font| !font.is_empty())
        {
            Some(font) => result.font = FFfont::init_pango(&font),
            None => {
                result.error =
                    "Couldn't get terminal font from GSettings (org.gnome.Console::custom-font)"
                        .to_string()
            },
        }
    } else {
        apply_system_monospace(result, instance);
    }
}

/// detectPtyxis.
fn detect_ptyxis(result: &mut TerminalFontResult, instance: &Instance) {
    if super::gtk::get(instance, "org.gnome.Ptyxis:/org/gnome/Ptyxis", "use-system-font")
        .as_deref()
        != Some("true")
    {
        match super::gtk::get(instance, "org.gnome.Ptyxis:/org/gnome/Ptyxis", "font-name")
            .filter(|font| !font.is_empty())
        {
            Some(font) => result.font = FFfont::init_pango(&font),
            None => {
                result.error =
                    "Couldn't get terminal font from GSettings (org.gnome.Ptyxis::font-name)"
                        .to_string()
            },
        }
    } else {
        apply_system_monospace(result, instance);
    }
}

/// detectFromGSettings: profile list schema → relocatable profile schema.
fn detect_from_gsettings(
    result: &mut TerminalFontResult,
    instance: &Instance,
    profile_path: &str,
    profile_list: &str,
    profile: &str,
    default_profile_key: &str,
) {
    let default_profile =
        super::gtk::get(instance, profile_list, default_profile_key).filter(|p| !p.is_empty());
    let Some(default_profile) = default_profile else {
        result.error = format!("Could not get default profile from gsettings: {profile_list}");
        return;
    };

    let path = format!("{profile_path}{default_profile}/");
    let schema = format!("{profile}:{path}");
    if super::gtk::get(instance, &schema, "use-system-font").as_deref() != Some("true") {
        match super::gtk::get(instance, &schema, "font").filter(|font| !font.is_empty()) {
            Some(font) => result.font = FFfont::init_pango(&font),
            None => {
                result.error =
                    format!("Couldn't get terminal font from GSettings ({profile}::{path}font)")
            },
        }
    } else {
        apply_system_monospace(result, instance);
    }
}

/// detectFromConfigFile: one Pango font property across the config dirs.
fn detect_from_config_file(result: &mut TerminalFontResult, config_file: &str, start: &'static str) {
    let mut font_name = None;
    for dir in util::config_dirs() {
        util::parse_prop_file(&dir.join(config_file), &mut [(start, &mut font_name)]);
        if !font_name.as_deref().unwrap_or("").is_empty() {
            break;
        }
    }
    match font_name.filter(|font| !font.is_empty()) {
        Some(font) => result.font = FFfont::init_pango(&font),
        None => result.error = format!("Couldn't find {start} in .config/{config_file}"),
    }
}

/// detectKonsole: default profile from the rc file, Qt font from the data
/// dirs' profile file.
fn detect_konsole(result: &mut TerminalFontResult, rc_file: &str) {
    let mut profile = None;
    let mut found = false;
    for dir in util::config_dirs() {
        found = util::parse_prop_file(&dir.join(rc_file), &mut [("DefaultProfile =", &mut profile)]);
        if found {
            break;
        }
    }
    if !found {
        result.error = format!("Configuration \".config/{rc_file}\" doesn't exist");
        return;
    }
    let Some(profile) = profile.filter(|profile| !profile.is_empty()) else {
        result.error = "Built-in profile is used".to_string();
        return;
    };

    let profile_path = format!("konsole/{profile}");
    let mut font_name = None;
    for dir in util::data_dirs() {
        util::parse_prop_file(&dir.join(&profile_path), &mut [("Font =", &mut font_name)]);
        if !font_name.as_deref().unwrap_or("").is_empty() {
            break;
        }
    }
    match font_name.filter(|font| !font.is_empty()) {
        Some(font) => result.font = FFfont::init_qt(&font),
        None => {
            result.error = format!("Couldn't find \"Font=%[^\\n]\" in \"{profile_path}\"");
        },
    }
}

/// detectXFCETerminal: the xfconf XML first, then terminalrc; the
/// use-system fallback needs XFConf, which this port doesn't read yet.
fn detect_xfce(result: &mut TerminalFontResult) {
    let mut path = "xfce4/xfconf/xfce-perchannel-xml/xfce4-terminal.xml";
    // Cut the trailing '"'/>' off the XML attribute values.
    let cut_quote =
        |value: Option<String>| match value {
            Some(value) => match value.rsplit_once('"') {
                Some((head, _)) => Some(head.to_string()),
                None => Some(value),
            },
            None => None,
        };

    let (config_found, values) = util::parse_props_in_config_dirs(
        path,
        &[
            "<property name=\"font-use-system\" type=\"bool\" value=\"",
            "<property name=\"font-name\" type=\"string\" value=\"",
        ],
    );
    let (use_sys, font_name) = if config_found {
        (cut_quote(values[0].clone()), cut_quote(values[1].clone()))
    } else {
        path = "xfce4/terminal/terminalrc";
        let (_, values) =
            util::parse_props_in_config_dirs(path, &["FontUseSystem = ", "FontName = "]);
        (values[0].clone(), values[1].clone())
    };

    let use_sys = use_sys.unwrap_or_default();
    let font_name = font_name.unwrap_or_default();
    if config_found && (use_sys.is_empty() || use_sys.eq_ignore_ascii_case("false")) {
        if font_name.is_empty() {
            result.error = format!("Couldn't find FontName in {path}");
        } else {
            result.font = FFfont::init_pango(&font_name);
        }
    } else {
        result.error = "Couldn't find xsettings::/Gtk/MonospaceFontName in XFConf".to_string();
    }
}

/// detectDeepinTerminal: the two section values with defaults applied even
/// when the config file is missing entirely.
fn detect_deepin(result: &mut TerminalFontResult) {
    let mut font_name = String::new();
    let mut font_size = String::new();

    for dir in util::config_dirs() {
        let path = dir.join("deepin/deepin-terminal/config.conf");
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let section_value = |header: &str| -> Option<String> {
            let mut lines = content.lines();
            while let Some(line) = lines.next() {
                if line == header {
                    return lines
                        .next()
                        .and_then(|line| util::parse_prop_line(line, "value="))
                        .map(str::to_string);
                }
            }
            None
        };
        if let Some(font) = section_value("[basic.interface.font]") {
            font_name = font;
        }
        if let Some(size) = section_value("[basic.interface.font_size]") {
            font_size = size;
        }
        break;
    }

    if font_name.is_empty() {
        font_name = "Noto Sans Mono".to_string();
    }
    if font_size.is_empty() {
        font_size = "11".to_string();
    }
    result.font = FFfont::init_values(&font_name, &font_size);
}

/// detectFootTerminal: a fontconfig pattern list; the first entry is the
/// font, the second the fallback. Missing config means "monospace (8pt)".
fn detect_foot(result: &mut TerminalFontResult) {
    let mut font = None;
    // File-missing and empty-value both fall back to "monospace (8pt)".
    util::config_dirs().into_iter().any(|dir| {
        util::parse_prop_file(&dir.join("foot/foot.ini"), &mut [("font=", &mut font)])
    });
    let font = font.filter(|font| !font.is_empty());
    let Some(font) = font else {
        result.font = FFfont::init_values("monospace", "8");
        return;
    };

    let mut primary = font.as_str();
    if let Some(comma) = font.find(',') {
        if comma + 1 < font.len() {
            result.fallback = FFfont::init_xft(&font[comma + 1..]);
        }
        primary = &font[..comma];
    }
    result.font = FFfont::init_xft(primary);
}

/// detectQTerminal / detectWestonTerminal / detectKmscon: one Values pair
/// with built-in defaults.
fn detect_values_pair(
    result: &mut TerminalFontResult,
    relative: &str,
    font_query: &'static str,
    size_query: &'static str,
    default_font: &str,
    default_size: &str,
) {
    let values = util::parse_props_in_config_dirs(relative, &[font_query, size_query]).1;
    let font = values[0]
        .clone()
        .filter(|font| !font.is_empty())
        .unwrap_or_else(|| default_font.to_string());
    let size = values[1]
        .clone()
        .filter(|size| !size.is_empty())
        .unwrap_or_else(|| default_size.to_string());
    result.font = FFfont::init_values(&font, &size);
}

/// detectXterm: X resources faceName/faceSize with the "8.0" default.
fn detect_xterm(result: &mut TerminalFontResult) {
    let mut font_name = None;
    let mut font_size = None;
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::PathBuf::from(home);
        util::parse_prop_file(
            &home.join(".Xresources"),
            &mut [
                ("xterm*faceName:", &mut font_name),
                ("xterm*faceSize:", &mut font_size),
            ],
        );
        if font_name.is_none() {
            util::parse_prop_file(
                &home.join(".Xresources"),
                &mut [
                    ("xterm.vt100.faceName:", &mut font_name),
                    ("xterm.vt100.faceSize:", &mut font_size),
                ],
            );
        }
    }
    let font = font_name.filter(|font| !font.is_empty()).unwrap_or_else(|| "fixed".to_string());
    let size = font_size.filter(|size| !size.is_empty()).unwrap_or_else(|| "8.0".to_string());
    result.font = FFfont::init_values(&font, &size);
}

/// detectSt: `-f` from the command line, or the "size=" strings baked into
/// the binary.
fn detect_st(result: &mut TerminalFontResult, terminal: &TerminalResult) {
    let Ok(cmdline) = std::fs::read(format!("/proc/{}/cmdline", terminal.pid)) else {
        result.error = format!("Failed to open /proc/{}/cmdline", terminal.pid);
        return;
    };

    let font;
    if let Some(position) = cmdline.windows(4).position(|window| window == b"\0-f\0") {
        // st was executed with `-f` on the command line
        let rest = &cmdline[position + 3..];
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        font = String::from_utf8_lossy(&rest[..end]).into_owned();
    } else {
        let mut found = None;
        if terminal::binary_extract_strings(&terminal.exe_path, "size=0".len(), &mut |s| {
            if s.contains("size=") {
                found = Some(s.to_string());
                false
            } else {
                true
            }
        })
        .is_none()
        {
            // The ELF scan reports open failures like upstream's libelf path.
            result.error = "open() failed".to_string();
            return;
        }
        match found {
            Some(font_string) => font = font_string,
            None => {
                result.error = "No font config found in st binary".to_string();
                return;
            },
        }
    }

    result.font = FFfont::init_xft(&font);
}

/// detectWarp: the prefs JSON object's FontName/FontSize strings.
fn detect_warp(result: &mut TerminalFontResult) {
    for dir in util::config_dirs() {
        let Ok(content) = std::fs::read_to_string(dir.join("warp-terminal/user_preferences.json"))
        else {
            continue;
        };
        // Locate the "prefs" object; a parseable file without it stops the
        // search like upstream's `return` after the first parsed file.
        let Some(prefs) = json_object(&content, "prefs") else {
            return;
        };
        let extract = |key: &str| -> Option<String> { json_string(&prefs, key) };
        result.font = FFfont::init_values(
            &extract("FontName").unwrap_or_else(|| "Hack".to_string()),
            &extract("FontSize").unwrap_or_else(|| "13".to_string()),
        );
        return;
    }
}

/// The balanced `{...}` object following `"name"` in a JSON document.
fn json_object(content: &str, name: &str) -> Option<String> {
    let key = format!("\"{name}\"");
    let start = content.find(&key)? + key.len();
    let open = content[start..].find('{')? + start;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, c) in content[open..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(content[open..=open + index].to_string());
                }
            },
            _ => {},
        }
    }
    None
}

/// A string value for `"key"` inside a JSON object; non-strings are None.
fn json_string(object: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = object.find(&needle)? + needle.len();
    let rest = object[start..].trim_start().strip_prefix(':')?.trim_start();
    if !rest.starts_with('"') {
        return None; // number/bool/null → the caller's default
    }
    let mut value = String::new();
    let mut chars = rest[1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next()? {
                'n' => value.push('\n'),
                't' => value.push('\t'),
                'r' => value.push('\r'),
                other => value.push(other),
            },
            '"' => return Some(value),
            _ => value.push(c),
        }
    }
    None
}

/// detectTerminator: use_system_font gates the system monospace font.
fn detect_terminator(result: &mut TerminalFontResult, instance: &Instance) {
    let (found, values) =
        util::parse_props_in_config_dirs("terminator/config", &["use_system_font =", "font ="]);
    let use_system = values[0].clone().unwrap_or_default();
    let font_name = values[1].clone().unwrap_or_default();
    if !found || use_system.eq_ignore_ascii_case("True") {
        apply_system_monospace(result, instance);
        return;
    }
    if font_name.is_empty() {
        result.font = FFfont::init_values("Mono", "10");
    } else {
        result.font = FFfont::init_pango(&font_name);
    }
}

/// detectUrxvt: comma-separated XLFD/xft list; the first two entries fill
/// font and fallback.
fn detect_urxvt(result: &mut TerminalFontResult) {
    let mut buffer = None;
    let mut found = false;
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::PathBuf::from(home);
        found = util::parse_prop_file(&home.join(".Xresources"), &mut [("URxvt.font:", &mut buffer)]);
        if !found {
            found = util::parse_prop_file(&home.join(".Xdefaults"), &mut [("URxvt.font:", &mut buffer)]);
        }
    }
    if !found {
        result.error = "Could not find URxvt.font in .Xresources or .Xdefaults".to_string();
        return;
    }

    let mut index = 0;
    for entry in buffer.unwrap_or_default().split(',') {
        if entry.starts_with('-') {
            let font = if index == 0 { &mut result.font } else { &mut result.fallback };
            *font = FFfont::init_xlfd(entry);
        } else if let Some(xft) = entry.strip_prefix("xft:") {
            let font = if index == 0 { &mut result.font } else { &mut result.fallback };
            *font = FFfont::init_xft(xft);
        } else {
            result.error = format!("Unknown URxvt font format: {entry}");
            continue;
        }
        index += 1;
        if index > 1 {
            break;
        }
    }
}

/// detectCosmicTerm: raw config values under $HOME/.config; the size gets
/// a "px" suffix appended (values init trims the '"' around names).
fn detect_cosmic_term(result: &mut TerminalFontResult) {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let base = std::path::PathBuf::from(home).join(".config/cosmic/com.system76.CosmicTerm/v1");

    let mut font_name =
        std::fs::read_to_string(base.join("font_name")).unwrap_or_default();
    if font_name.is_empty() {
        font_name = "Noto Sans Mono".to_string();
    }

    let mut font_size = std::fs::read_to_string(base.join("font_size")).unwrap_or_default();
    if !font_size.is_empty() {
        font_size.push_str("px");
    }
    if font_size.is_empty() {
        font_size = "14px".to_string();
    }

    result.font = FFfont::init_values(&font_name, &font_size);
}

// ---- common getters ----

/// detectAlacritty: toml files in parse order, with the `normal` inline
/// table overriding plain `family`.
fn detect_alacritty(result: &mut TerminalFontResult) {
    let mut normal = None;
    let mut family = None;
    let mut size = None;
    let mut style = String::new();

    for relative in ["alacritty/alacritty.toml", "alacritty.toml", ".alacritty.toml"] {
        let (found, values) =
            util::parse_props_in_config_dirs(relative, &["normal =", "family =", "size ="]);
        normal = values[0].clone();
        family = values[1].clone();
        size = values[2].clone();
        if found {
            break;
        }
    }

    if let Some(table) = normal.filter(|normal| !normal.is_empty()) {
        family = None; // `normal` takes precedence
        let table = table.trim().trim_end_matches('}').trim_start_matches('{').trim().to_string();
        // Assume no commas in font names.
        for line in table.replace(',', "\n").lines() {
            if let Some(value) = util::parse_prop_line(line, "family =") {
                family = Some(value.to_string());
            } else if let Some(value) = util::parse_prop_line(line, "style =") {
                style = value.to_string();
            }
        }
    }

    let family = family.filter(|family| !family.is_empty()).unwrap_or_else(|| "monospace".to_string());
    if style.is_empty() {
        style = "Regular".to_string();
    }
    let size = size.filter(|size| !size.is_empty()).unwrap_or_else(|| "11.25".to_string());
    result.font = FFfont::init_move_values(&family, &size, Some(style));
}

/// parseGhosttyConfig: one config file with `config-file` includes.
fn parse_ghostty_config(
    path: &std::path::Path,
    font_name: &mut String,
    font_name_fallback: &mut String,
    font_size: &mut String,
    config_files: &mut Vec<std::path::PathBuf>,
) {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => {
            // Try to load the config without the .ghostty extension.
            let without = match path.to_str().and_then(|p| p.strip_suffix(".ghostty")) {
                Some(stripped) => std::path::PathBuf::from(stripped),
                None => return,
            };
            match std::fs::read_to_string(&without) {
                Ok(content) => content,
                Err(_) => return,
            }
        },
    };

    for line in content.lines() {
        if let Some(value) = util::parse_prop_line(line, "font-family =") {
            // Ghostty appends to a fallback list; the first entry is the
            // primary font. An empty value resets the list.
            if value.is_empty() {
                font_name.clear();
                font_name_fallback.clear();
            } else if font_name.is_empty() {
                *font_name = value.to_string();
            } else if font_name_fallback.is_empty() {
                *font_name_fallback = value.to_string();
            }
        } else if let Some(value) = util::parse_prop_line(line, "font-size =") {
            // Latter overrides former
            *font_size = value.to_string();
        } else if let Some(value) = util::parse_prop_line(line, "config-file =") {
            let value = value.trim_start_matches('?').trim_matches('"');
            if value.is_empty() {
                continue;
            }
            let include = if value.starts_with('/') {
                std::path::PathBuf::from(value)
            } else {
                // Relative paths resolve against the including file.
                let mut absolute = path.to_path_buf();
                absolute.pop();
                absolute.push(value);
                absolute
            };

            // Each unique file loads once; 16 directives max.
            let loaded = config_files.contains(&include)
                || include == path
                || config_files.len() >= 16;
            if !loaded {
                config_files.push(include);
            }
        }
    }
}

/// detectGhostty: the XDG config first, then the collected includes.
fn detect_ghostty(result: &mut TerminalFontResult, config_subpath: &str) {
    let mut font_name = String::new();
    let mut font_name_fallback = String::new();
    let mut font_size = String::new();
    let mut config_files = Vec::new();

    if let Some(dir) = util::config_dirs().into_iter().next() {
        parse_ghostty_config(
            &dir.join(config_subpath),
            &mut font_name,
            &mut font_name_fallback,
            &mut font_size,
            &mut config_files,
        );
    }
    // Includes take effect after the whole root config was parsed.
    let mut index = 0;
    while index < config_files.len() {
        let path = config_files[index].clone();
        parse_ghostty_config(
            &path,
            &mut font_name,
            &mut font_name_fallback,
            &mut font_size,
            &mut config_files,
        );
        index += 1;
    }

    if font_name.is_empty() {
        font_name = "JetBrainsMono Nerd Font".to_string();
    }
    if font_size.is_empty() {
        font_size = "12".to_string();
    }
    result.font = FFfont::init_values(&font_name, &font_size);
    if !font_name_fallback.is_empty() {
        result.fallback = FFfont::init_values(&font_name_fallback, "");
    }
}

/// detectWezterm: `wezterm cli ls-fonts --text a`, then the quoted font in
/// the wezterm.font(...) line.
fn detect_wezterm(result: &mut TerminalFontResult, exe: &str) {
    let mut cli = exe.to_string();
    if let Some(index) = cli.rfind('-') {
        cli.truncate(index);
    }

    let output = match process_stdout(&[cli.as_str(), "ls-fonts", "--text", "a"]) {
        Ok(output) => output,
        Err(error) => {
            result.error = error;
            return;
        },
    };
    // Extract the first quoted string; without quotes the whole output.
    let font_name = match output.find('"') {
        Some(open) => {
            let after = &output[open + 1..];
            match after.find('"') {
                Some(close) => &after[..close],
                None => after,
            }
        },
        None => output.as_str(),
    };
    if font_name.is_empty() {
        return;
    }
    result.font = FFfont::init_copy(font_name);
}

/// detectTabby: a YAML config with "font: "/"fontSize: " keys.
fn detect_tabby(result: &mut TerminalFontResult) {
    let values =
        util::parse_props_in_config_dirs("tabby/config.yaml", &["font: ", "fontSize: "]);
    if !values.0 {
        return;
    }
    let font = values.1[0]
        .clone()
        .filter(|font| !font.is_empty())
        .unwrap_or_else(|| "monospace".to_string());
    let size = values.1[1]
        .clone()
        .filter(|size| !size.is_empty())
        .unwrap_or_else(|| "14".to_string());
    result.font = FFfont::init_values(&font, &size);
}

/// detectContour: `contour font-locator`, then the "Font description" line.
fn detect_contour(result: &mut TerminalFontResult, exe: &str) {
    let buf = match process_stdout(&[exe, "font-locator"]) {
        Ok(buf) => buf,
        Err(_) => {
            result.error = "`contour font-locator` failed".to_string();
            return;
        },
    };

    const MARKER: &str = "Font description      : (family=";
    let Some(index) = buf.find(MARKER) else {
        return;
    };
    let after = &buf[index + MARKER.len()..];
    let family = match after.find(" weight=") {
        Some(weight) => &after[..weight],
        None => after,
    };
    result.font = FFfont::init_copy(family);
}

/// detectRio: rio/config.toml with "Cascadia Code"/"18" defaults.
fn detect_rio(result: &mut TerminalFontResult) {
    detect_values_pair(result, "rio/config.toml", "family =", "size =", "Cascadia Code", "18");
}

/// detectKitty: the escape-sequence font query, the kitten fallback, then
/// kitty.conf with monospace/11.0 defaults.
fn detect_kitty(result: &mut TerminalFontResult, exe: &str) {
    let mut font_name = String::new();
    let mut font_size = String::new();

    let response = terminal::terminal_response(
        b"\x1bP+q6b697474792d71756572792d666f6e745f66616d696c79;6b697474792d71756572792d666f6e745f73697a65\x1b\\",
        2,
        &mut parse_kitty_font_response,
    );
    let mut decoded = false;
    if let Some((font_hex, size_hex)) = response
        && !font_hex.is_empty()
        && !size_hex.is_empty()
    {
        font_name = terminal::hex_decode(font_hex.as_bytes());
        font_size = terminal::hex_decode(size_hex.as_bytes());
        decoded = true;
    }

    if !decoded {
        match process_stdout(&[exe, "+kitten", "query-terminal"]) {
            Ok(buf) => {
                for line in buf.lines() {
                    if let Some(value) = util::parse_prop_line(line, "font_family: ") {
                        font_name = value.to_string();
                    } else if let Some(value) = util::parse_prop_line(line, "font_size: ") {
                        font_size = value.to_string();
                    }
                }
            },
            Err(_) => {
                let values = util::parse_props_in_config_dirs(
                    "kitty/kitty.conf",
                    &["font_family ", "font_size "],
                )
                .1;
                font_name = values[0]
                    .clone()
                    .filter(|font| !font.is_empty())
                    .unwrap_or_else(|| "monospace".to_string());
                font_size = values[1]
                    .clone()
                    .filter(|size| !size.is_empty())
                    .unwrap_or_else(|| "11.0".to_string());
            },
        }
    }

    result.font = FFfont::init_values(&font_name, &font_size);
}

/// kitty's two-part XTGETTCAP reply: "\eP1+r<id>=<hex>\e\\" twice.
fn parse_kitty_font_response(buf: &[u8]) -> (usize, Option<(String, String)>) {
    const PREFIX: &[u8] = b"\x1bP1+r";
    let mut payloads: Vec<&[u8]> = Vec::new();
    let mut pos = 0;
    for _ in 0..2 {
        let rest = &buf[pos.min(buf.len())..];
        if !rest.starts_with(PREFIX) {
            break;
        }
        let mut p = PREFIX.len();
        // %*[^=]: at least one character, then the literal '='.
        if p >= rest.len() || rest[p] == b'=' {
            break;
        }
        while p < rest.len() && rest[p] != b'=' {
            p += 1;
        }
        if p >= rest.len() {
            break;
        }
        p += 1;
        // %511[^\e]
        let start = p;
        while p < rest.len() && p - start < 511 && rest[p] != 0x1b {
            p += 1;
        }
        if p == start {
            break;
        }
        payloads.push(&rest[start..p]);
        if !rest[p..].starts_with(b"\x1b\\") {
            break; // the literal terminator failed; keep what's assigned
        }
        pos += p + 2;
    }

    match payloads.len() {
        2 => (
            2,
            Some((
                String::from_utf8_lossy(payloads[0]).into_owned(),
                String::from_utf8_lossy(payloads[1]).into_owned(),
            )),
        ),
        1 => (1, None),
        _ => (0, None),
    }
}

/// detectTTY: /etc/vconsole.conf, else showconsolefont.
fn detect_tty(result: &mut TerminalFontResult) {
    let mut font_name = None;
    util::parse_prop_file(std::path::Path::new("/etc/vconsole.conf"), &mut [("Font =", &mut font_name)]);
    let mut font_name = font_name.unwrap_or_default();

    if font_name.is_empty() {
        font_name = "VGA default kernel font ".to_string();
        if let Ok(output) = process_stdout(&["showconsolefont", "--info"]) {
            font_name.push_str(&output);
        }
        font_name = font_name.trim_end_matches(' ').to_string();
    }

    if !font_name.is_empty() {
        result.font = FFfont::init_copy(&font_name);
    } else {
        result.error = "Couldn't find Font in /etc/vconsole.conf".to_string();
    }
}

// ---- dispatch ----

/// detectTerminalFontCommon: the terminals with portable getters. Returns
/// false when the process name doesn't match any of them.
fn detect_common(terminal: &TerminalResult, result: &mut TerminalFontResult, _instance: &Instance) -> bool {
    let name = &terminal.process_name;
    let starts_ign = |prefix: &str| name.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase());
    if starts_ign("alacritty") {
        detect_alacritty(result);
    } else if starts_ign("wezterm-gui") {
        detect_wezterm(result, &terminal.exe);
    } else if starts_ign("tabby") {
        detect_tabby(result);
    } else if starts_ign("contour") {
        detect_contour(result, &terminal.exe);
    } else if starts_ign("ghostty") {
        detect_ghostty(result, "ghostty/config.ghostty");
    } else if starts_ign("Muxy") {
        detect_ghostty(result, "muxy/ghostty.conf");
    } else if starts_ign("rio") {
        detect_rio(result);
    } else if terminal.exe.to_ascii_lowercase().starts_with("/dev/pts/") {
        result.error = "Terminal font detection is not supported on PTS".to_string();
    } else if name.eq_ignore_ascii_case("kitty") {
        detect_kitty(result, &terminal.exe);
    } else if terminal.exe.to_ascii_lowercase().starts_with("/dev/tty") {
        detect_tty(result);
    } else {
        return false;
    }
    true
}

/// ffDetectTerminalFontPlatform: the Linux-specific getters.
fn detect_platform(terminal: &TerminalResult, result: &mut TerminalFontResult, instance: &Instance) -> bool {
    let name = &terminal.process_name;
    let starts_ign = |prefix: &str| name.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase());
    if name.eq_ignore_ascii_case("konsole") {
        detect_konsole(result, "konsolerc");
    } else if name.eq_ignore_ascii_case("yakuake") {
        detect_konsole(result, "yakuakerc");
    } else if name.eq_ignore_ascii_case("xfce4-terminal") {
        detect_xfce(result);
    } else if name.eq_ignore_ascii_case("lxterminal") {
        detect_from_config_file(result, "lxterminal/lxterminal.conf", "fontname =");
    } else if name.eq_ignore_ascii_case("tilix") {
        detect_from_gsettings(result, instance, "/com/gexperts/Tilix/profiles/", "com.gexperts.Tilix.ProfilesList", "com.gexperts.Tilix.Profile", "default");
    } else if starts_ign("gnome-terminal") {
        detect_from_gsettings(result, instance, "/org/gnome/terminal/legacy/profiles:/:", "org.gnome.Terminal.ProfilesList", "org.gnome.Terminal.Legacy.Profile", "default");
    } else if starts_ign("ptyxis-agent") {
        detect_ptyxis(result, instance);
    } else if name.eq_ignore_ascii_case("kgx") {
        detect_kgx(result, instance);
    } else if name.eq_ignore_ascii_case("mate-terminal") {
        detect_from_gsettings(result, instance, "/org/mate/terminal/profiles/", "org.mate.terminal.global", "org.mate.terminal.profile", "default-profile");
    } else if name.eq_ignore_ascii_case("deepin-terminal") {
        detect_deepin(result);
    } else if name.eq_ignore_ascii_case("foot") {
        detect_foot(result);
    } else if name.eq_ignore_ascii_case("qterminal") {
        detect_values_pair(result, "qterminal.org/qterminal.ini", "fontFamily=", "fontSize=", "monospace", "12");
    } else if name.eq_ignore_ascii_case("xterm") {
        detect_xterm(result);
    } else if name.eq_ignore_ascii_case("st") {
        detect_st(result, terminal);
    } else if name.eq_ignore_ascii_case("warp") {
        detect_warp(result);
    } else if name.eq_ignore_ascii_case("weston-terminal") {
        detect_values_pair(result, "weston.ini", "font=", "font-size=", "DejaVu Sans Mono", "14");
    } else if name.eq_ignore_ascii_case("kmscon") {
        detect_values_pair(result, "kmscon/kmscon.conf", "font-size=", "font-name=", "Hack Nerd Font", "18");
    } else if starts_ign("terminator") {
        detect_terminator(result, instance);
    } else if starts_ign("sakura") {
        detect_from_config_file(result, "sakura/sakura.conf", "font=");
    } else if starts_ign("cosmic-term") {
        detect_cosmic_term(result);
    } else if starts_ign("termite") {
        detect_from_config_file(result, "termite/config", "font =");
    } else if name.eq_ignore_ascii_case("rxvt")
        || name.eq_ignore_ascii_case("urxvt")
        || name.eq_ignore_ascii_case("urxvtd")
    {
        detect_urxvt(result);
    } else {
        return false;
    }
    true
}

/// ffDetectTerminalFont.
pub fn detect(instance: &Instance) -> TerminalFontResult {
    let terminal = terminal::detect_full();
    let mut result = TerminalFontResult::default();

    if terminal.process_name.is_empty() {
        result.error = "Terminal font needs successful terminal detection".to_string();
    } else if !detect_common(terminal, &mut result, instance) {
        detect_platform(terminal, &mut result, instance);
    }

    if result.error.is_empty() && result.font.pretty.is_empty() {
        result.error = format!("Unknown terminal: {}", terminal.process_name);
    }
    result
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let result = detect(instance);
    if !result.error.is_empty() {
        return Vec::new();
    }
    let mut value = result.font.pretty.clone();
    if !result.fallback.pretty.is_empty() {
        value.push_str(" / ");
        value.push_str(&result.fallback.pretty);
    }
    vec![Row::new("Terminal Font", value)]
}

fn font_json(font: &FFfont) -> Json {
    let mut obj = Json::obj();
    obj.push("name", font.name.clone());
    obj.push("size", font.size.clone());
    let mut styles = Json::Array(Vec::new());
    for style in &font.styles {
        styles.push_value(Json::Str(style.clone()));
    }
    obj.push("styles", styles);
    obj.push("pretty", font.pretty.clone());
    obj
}

/// ffGenerateTerminalFontJsonResult: {font, fallback} objects.
pub fn json(instance: &Instance) -> JsonOutcome {
    let result = detect(instance);
    if !result.error.is_empty() {
        return JsonOutcome::Error(result.error);
    }
    let mut obj = Json::obj();
    obj.push("font", font_json(&result.font));
    obj.push("fallback", font_json(&result.fallback));
    JsonOutcome::Result(obj)
}

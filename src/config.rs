//! JSON config loading: `--config/-c` resolution and the auto-loaded
//! `fastfetch/config.jsonc`, ported from fastfetch.c's optionParseConfigFile
//! / parseJsoncFile / parseConfigFiles and common/impl/jsonconfig.c.
//!
//! Parse and section errors exit like upstream: 477 for JSON/section
//! errors, 413 for duplicate `--config`, 414 for a config that can not be
//! found. The root sections (logo, general, display) are walked in that
//! order at load time; the `modules` array is walked into entries for the
//! module dispatch and only when the structure selection will use it.

use crate::{jsonc, platform};

/// One entry of the config's `modules` array.
pub enum Entry {
    /// A bare module name string.
    Simple(String),
    /// A module object; `opts` keeps the whole object so modules decode
    /// their own option keys from it.
    Object { type_: String, opts: Option<&'static jsonc::Value> },
}

pub struct Config {
    /// A config document was loaded (upstream's non-null configDoc): the
    /// modules array takes over the default preset, even when it is absent
    /// or empty.
    pub document_loaded: bool,
    /// `logo: null` (upstream: logo type NONE, all paddings zeroed).
    pub logo_none: bool,
    /// `logo.type: "small"` (upstream: logo type SMALL).
    pub logo_small: bool,
    /// `logo.position` (upstream's FFLogoPosition; LEFT when unset).
    pub logo_position: Option<crate::logo::PositionOpt>,
    /// `logo.padding.*` overrides (upstream's paddingTop/paddingBottom/
    /// paddingLeft/paddingRight; `--logo-padding` covers the horizontal
    /// pair on the command line).
    pub padding_top: Option<u32>,
    pub padding_bottom: Option<u32>,
    pub padding_left: Option<u32>,
    pub padding_right: Option<u32>,
    /// `logo.width` / `logo.height`: the art size floors; null resets to
    /// zero, anything that does not read as a positive integer errors.
    pub logo_width: Option<u32>,
    pub logo_height: Option<u32>,
    /// `logo.printRemaining` (yyjson_get_bool: only JSON true reads as
    /// true).
    pub logo_print_remaining: Option<bool>,
    /// `display.pipe` (applies unless `--pipe` was given on the command
    /// line, which is parsed after the config).
    pub pipe: bool,
    /// `general.playerName`, stored for the media module's custom bus.
    pub player_name: String,
    /// The modules array; empty when the key is absent. An entry list that
    /// stopped at `walk_error` keeps the entries before it.
    pub entries: Vec<Entry>,
    /// The modules-walk error, if any: remaining entries are not printed
    /// (upstream: printJsonConfig returns the error and the walk stops).
    pub walk_error: Option<String>,
}

impl Config {
    /// A "loaded but empty" config, e.g. `--config none`.
    fn empty() -> Self {
        Self {
            document_loaded: false,
            logo_none: false,
            logo_small: false,
            logo_position: None,
            padding_top: None,
            padding_bottom: None,
            padding_left: None,
            padding_right: None,
            logo_width: None,
            logo_height: None,
            logo_print_remaining: None,
            pipe: false,
            player_name: String::new(),
            entries: Vec::new(),
            walk_error: None,
        }
    }
}

fn exit_error(code: i32, message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(code)
}

/// ffPlatformPathAddAbsolute/AddHome: only directories that exist, one
/// trailing slash, deduped.
fn add_dir(dirs: &mut Vec<String>, path: &str) {
    if !std::path::Path::new(path).is_dir() {
        return;
    }
    let mut path = path.to_string();
    if !path.ends_with('/') {
        path.push('/');
    }
    if !dirs.contains(&path) {
        dirs.push(path);
    }
}

/// ffPlatform configDirs, each ending with '/'.
pub(crate) fn config_dirs() -> Vec<String> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        add_dir(&mut dirs, &dir.to_string_lossy());
    }
    let home = &platform::platform().home_dir;
    add_dir(&mut dirs, &format!("{home}.config/"));
    add_dir(&mut dirs, home);
    if let Some(list) = std::env::var_os("XDG_CONFIG_DIRS") {
        for dir in list.to_string_lossy().split(':') {
            add_dir(&mut dirs, dir);
        }
    }
    add_dir(&mut dirs, "/etc/xdg/");
    add_dir(&mut dirs, "/etc/");
    dirs
}

/// ffPlatform dataDirs, each ending with '/'.
pub(crate) fn data_dirs() -> Vec<String> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        add_dir(&mut dirs, &dir.to_string_lossy());
    }
    let home = &platform::platform().home_dir;
    add_dir(&mut dirs, &format!("{home}.local/share/"));
    // ${exePath}/../share
    let exe = &platform::platform().exe_path;
    if let Some(last_slash) = exe.rfind('/')
        && let Some(second_slash) = exe[..last_slash].rfind('/')
    {
        add_dir(&mut dirs, &format!("{}/share/", &exe[..second_slash]));
    }
    add_dir(&mut dirs, home);
    if let Some(list) = std::env::var_os("XDG_DATA_DIRS") {
        for dir in list.to_string_lossy().split(':') {
            add_dir(&mut dirs, dir);
        }
    }
    add_dir(&mut dirs, "/usr/local/share/");
    add_dir(&mut dirs, "/usr/share/");
    dirs
}

/// The `-` stdin path: strict JSON, upstream's "from stdin" error wording.
fn parse_stdin() -> jsonc::Value {
    use std::io::Read;
    let mut content = String::new();
    let _ = std::io::stdin().read_to_string(&mut content);
    jsonc::Parser::parse(&content, false).unwrap_or_else(|error| {
        exit_error(
            477,
            &format!(
                "Error: failed to parse JSON from stdin at {}: {}",
                error.pos, error.msg
            ),
        )
    })
}

/// optionParseConfigFile: the `-c` resolution chain. A returned Config
/// with `document_loaded == false` means an explicit skip (`none`).
pub fn load_explicit(value: &str, already_loaded: &mut bool) -> Config {
    if *already_loaded {
        exit_error(413, "Error: only one config file can be loaded");
    }
    *already_loaded = true;

    if value.is_empty() || value.eq_ignore_ascii_case("none") {
        return Config::empty();
    }

    if value == "-" {
        let root = parse_stdin();
        if !root.is_object() {
            exit_error(
                477,
                "JsonConfig Error: Invalid JSON config format. Root value must be an object",
            );
        }
        return build_config(leak_root(root));
    }

    // Extension rules decide the parser mode; without one, .jsonc is
    // appended for every candidate.
    let strict_json = value.to_ascii_lowercase().ends_with(".json");
    let candidate = |base: String| {
        if strict_json || base.to_ascii_lowercase().ends_with(".jsonc") || base.to_ascii_lowercase().ends_with(".json5") {
            base
        } else {
            format!("{base}.jsonc")
        }
    };

    // 1. As a path (relative to the cwd or absolute).
    let path = candidate(value.to_string());
    if let Some(doc) = try_file(&path, strict_json) {
        return doc;
    }

    // 2. configDirs + fastfetch/<value>
    for dir in config_dirs() {
        let path = candidate(format!("{dir}fastfetch/{value}"));
        if let Some(doc) = try_file(&path, strict_json) {
            return doc;
        }
    }

    // 3. dataDirs + fastfetch/presets/<value>
    for dir in data_dirs() {
        let path = candidate(format!("{dir}fastfetch/presets/{value}"));
        if let Some(doc) = try_file(&path, strict_json) {
            return doc;
        }
    }

    // 4. next to the binary, for portable installs
    let exe = &platform::platform().exe_path;
    if let Some(last_slash) = exe.rfind('/') {
        let dir = &exe[..=last_slash];
        let path = candidate(format!("{dir}{value}"));
        if let Some(doc) = try_file(&path, strict_json) {
            return doc;
        }
        let path = candidate(format!("{dir}presets/{value}"));
        if let Some(doc) = try_file(&path, strict_json) {
            return doc;
        }
    }

    exit_error(414, &format!("Error: couldn't find config: {value}"))
}

/// parseConfigFiles: the auto-loaded default config, gated on NO_CONFIG.
pub fn load_automatic(already_loaded: bool) -> Option<Config> {
    if already_loaded || std::env::var_os("NO_CONFIG").is_some() {
        return None;
    }
    for dir in config_dirs() {
        for extension in ["config.jsonc", "config.json5"] {
            if let Some(doc) = try_file(&format!("{dir}fastfetch/{extension}"), false) {
                return Some(doc);
            }
        }
    }
    None
}

/// Tries one candidate file; None means "could not open, try the next".
fn try_file(path: &str, strict: bool) -> Option<Config> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return None;
    };
    let root = jsonc::Parser::parse(&content, !strict).unwrap_or_else(|error| {
        let (row, col) = jsonc::locate_pos(&content, error.pos);
        exit_error(
            477,
            &format!(
                "Error: failed to parse JSON config file `{path}` at ({row}, {col}): {}",
                error.msg
            ),
        )
    });
    if !root.is_object() {
        exit_error(
            477,
            "JsonConfig Error: Invalid JSON config format. Root value must be an object",
        );
    }
    Some(build_config(leak_root(root)))
}

/// The parsed tree is leaked so entries can reference it &'static across
/// the job threads; one config per process.
fn leak_root(root: jsonc::Value) -> &'static jsonc::Value {
    Box::leak(Box::new(root))
}

fn build_config(root: &'static jsonc::Value) -> Config {
    let mut config = Config::empty();
    config.document_loaded = true;
    apply_logo(root, &mut config);
    apply_general(root, &mut config);
    apply_display(root, &mut config);
    let (entries, walk_error) = walk_modules(root);
    config.entries = entries;
    config.walk_error = walk_error;
    config
}

/// ffOptionsParseLogoJsonConfig. The keys we render today are the padding
/// overrides and `logo: null`; the remaining known keys are accepted so
/// their configs load, with behavior to grow in later rounds.
fn apply_logo(root: &jsonc::Value, config: &mut Config) {
    let Some(logo) = root.get("logo") else {
        return;
    };
    if logo.as_str().is_none() && !logo.is_object() {
        // yyjson_is_null: logo disabled, paddings zeroed.
        if matches!(logo, jsonc::Value::Null) {
            config.logo_none = true;
            return;
        }
        section_error("logo", None, "Property 'logo' must be an object");
    }
    let Some(entries) = object_entries(logo) else {
        section_error("logo", None, "Property 'logo' must be an object");
    };
    for (key, value) in entries {
        match key.as_str() {
            "type" => {
                let error = match value.as_str() {
                    Some(name)
                        if [
                            "auto", "builtin", "small", "file", "file-raw", "data", "data-raw",
                            "command-raw", "none",
                        ]
                        .iter()
                        .any(|known| known.eq_ignore_ascii_case(name)) =>
                    {
                        if name.eq_ignore_ascii_case("none") {
                            config.logo_none = true;
                        } else if name.eq_ignore_ascii_case("small") {
                            config.logo_small = true;
                        }
                        None
                    },
                    Some(name)
                        if ["sixel", "kitty", "kitty-direct", "kitty-icat", "iterm", "chafa", "raw"]
                            .iter()
                            .any(|known| known.eq_ignore_ascii_case(name)) =>
                    {
                        // Our port has no image logo support, like an
                        // upstream build with FF_HAVE_IMAGE_LOGO off.
                        Some(
                            "Image logo types are not supported because Fastfetch was built without image logo support",
                        )
                    },
                    _ => Some("Invalid enum value type; must be a string or integer"),
                };
                if let Some(error) = error {
                    section_error("logo", Some(key), error);
                }
            },
            "position" => {
                let error = match value {
                    jsonc::Value::Str(name)
                        if ["auto", "left", "top", "right"]
                            .iter()
                            .any(|known| known.eq_ignore_ascii_case(name)) =>
                    {
                        config.logo_position = Some(match name.to_ascii_lowercase().as_str() {
                            "auto" => crate::logo::PositionOpt::Auto,
                            "left" => crate::logo::PositionOpt::Left,
                            "right" => crate::logo::PositionOpt::Right,
                            _ => crate::logo::PositionOpt::Top,
                        });
                        None
                    },
                    jsonc::Value::Str(_) => Some("Invalid enum string"),
                    jsonc::Value::Int(_) | jsonc::Value::UInt(_) => Some("Invalid enum integer"),
                    _ => Some("Invalid enum value type; must be a string or integer"),
                };
                if let Some(error) = error {
                    section_error("logo", Some(key), error);
                }
            },
            "printRemaining" => {
                // yyjson_get_bool: only a JSON true reads as true.
                config.logo_print_remaining = Some(matches!(value, jsonc::Value::Bool(true)));
            },
            "width" | "height" => {
                // Null resets to zero; anything else reads through
                // yyjson_get_uint (negative ints bit-cast huge — upstream
                // then spins filling that width — and every non-integer
                // type reads as zero and errors).
                let slot = if key == "width" { &mut config.logo_width } else { &mut config.logo_height };
                *slot = Some(match value {
                    jsonc::Value::Null => 0,
                    jsonc::Value::UInt(amount) => *amount as u32,
                    jsonc::Value::Int(amount) => (*amount as u64) as u32,
                    _ => section_error("logo", Some(key), &format!("Logo {key} must be a positive integer")),
                });
            },
            "source" | "preserveAspectRatio" | "cache" | "recache" | "animationFrame" | "spin" | "color" => {
                // accepted; rendering support grows with the logo rounds
            },
            "padding" => {
                let Some(padding) = object_entries(value) else {
                    section_error("logo", Some(key), "Logo padding must be an object");
                };
                for (side, amount) in padding {
                    // yyjson_is_uint: only non-negative integer literals
                    // pass; negatives, reals and strings error out.
                    if !matches!(amount, jsonc::Value::UInt(_)) {
                        section_error("logo", Some(key), "Logo padding values must be positive integers");
                    }
                    let amount = amount.as_uint().unwrap_or(0) as u32;
                    match side.as_str() {
                        "top" => config.padding_top = Some(amount),
                        "bottom" => config.padding_bottom = Some(amount),
                        "left" => config.padding_left = Some(amount),
                        "right" => config.padding_right = Some(amount),
                        _ => {},
                    }
                }
            },
            _ => section_error("logo", Some(key), "Unknown logo key"),
        }
    }
}

/// ffOptionsParseGeneralJsonConfig.
fn apply_general(root: &jsonc::Value, config: &mut Config) {
    let Some(general) = root.get("general") else {
        return;
    };
    let Some(entries) = object_entries(general) else {
        section_error("general", None, "Property 'general' must be an object");
    };
    for (key, value) in entries {
        match key.as_str() {
            "thread" | "processingTimeout" | "detectVersion" | "dsForceDrm" => {},
            "playerName" => {
                if let Some(name) = value.as_str() {
                    config.player_name = name.to_string();
                }
            },
            "preload" => {
                if !value.is_object() {
                    section_error("general", Some(key), "Property 'general.preload' must be an object");
                }
            },
            "preRun" => section_error("general", Some(key), "general.preRun is removed due to security concerns."),
            _ => section_error("general", Some(key), "Unknown general property"),
        }
    }
}

/// ffOptionsParseDisplayJsonConfig; the keys that change our behavior
/// today are `pipe` and `stat` (which implies showErrors upstream). The
/// rest are accepted with upstream's type checks where they exist.
fn apply_display(root: &jsonc::Value, config: &mut Config) {
    let Some(display) = root.get("display") else {
        return;
    };
    let Some(entries) = object_entries(display) else {
        section_error("display", None, "Property 'display' must be an object");
    };
    for (key, value) in entries {
        match key.as_str() {
            "stat" => match value {
                jsonc::Value::Bool(true) => {}, // implies showErrors; rendering deferred
                jsonc::Value::Bool(false) | jsonc::Value::Null => {},
                jsonc::Value::UInt(_) => {},
                jsonc::Value::Int(i) if *i >= 0 => {},
                _ => section_error(
                    "display",
                    Some(key),
                    "display.stat must be a boolean or a positive integer",
                ),
            },
            // yyjson_get_bool accepts any value type; non-bools read as false.
            "pipe" => config.pipe = value.as_bool().unwrap_or(false),
            "showErrors" | "disableLinewrap" | "hideCursor" | "brightColor" | "noBuffer" => {},
            "separator" | "color" | "key" | "constants" => {},
            "duration" | "size" | "temp" | "percent" | "bar" | "fraction" | "freq" | "common" => {
                if !value.is_object() {
                    section_error(
                        "display",
                        Some(key),
                        &format!("display.{key} must be an object"),
                    );
                }
            },
            _ => section_error("display", Some(key), "Unknown display property"),
        }
    }
}

fn section_error(section: &str, key: Option<&str>, message: &str) -> ! {
    match key {
        Some(key) => exit_error(
            477,
            &format!("JsonConfig Error ({section}.{key}): {message}"),
        ),
        None => exit_error(477, &format!("JsonConfig Error ({section}): {message}")),
    }
}

fn object_entries(value: &jsonc::Value) -> Option<&Vec<(String, jsonc::Value)>> {
    match value {
        jsonc::Value::Object(entries) => Some(entries),
        _ => None,
    }
}

/// matchesJsonArray: a string or an array of strings, case-insensitive.
fn matches_json_array(text: &str, value: &jsonc::Value) -> bool {
    match value {
        jsonc::Value::Str(s) => text.eq_ignore_ascii_case(s),
        jsonc::Value::Array(items) => items
            .iter()
            .any(|item| item.as_str().is_some_and(|s| text.eq_ignore_ascii_case(s))),
        _ => false,
    }
}

/// printJsonConfig's modules walk. Returns the entries accepted before an
/// error (printing stops there) plus the error, if any.
fn walk_modules(root: &'static jsonc::Value) -> (Vec<Entry>, Option<String>) {
    let Some(modules) = root.get("modules") else {
        return (Vec::new(), None);
    };
    let Some(items) = object_array(modules) else {
        return (
            Vec::new(),
            Some("Property 'modules' must be an array of strings or objects".to_string()),
        );
    };

    let mut entries = Vec::new();
    for item in items {
        let (type_, opts) = match item {
            jsonc::Value::Str(name) => (name.clone(), None),
            jsonc::Value::Object(_) => {
                if let Some(conditions) = item.get("condition") {
                    let Some(condition_entries) = object_entries(conditions) else {
                        return (
                            entries,
                            Some("Property 'condition' must be an object".to_string()),
                        );
                    };
                    let sys_name = &platform::platform().sysinfo.name;
                    let arch = &platform::platform().sysinfo.architecture;
                    let mut include = true;
                    for (key, value) in condition_entries {
                        let (negated, field) = if let Some(rest) = key.strip_prefix('!') {
                            (true, rest)
                        } else {
                            (false, key.as_str())
                        };
                        let matched = match field {
                            "system" => matches_json_array(sys_name, value),
                            "arch" => matches_json_array(arch, value),
                            "succeeded" => {
                                // Whether the previous module printed
                                // successfully is only known once the run
                                // started; entries gated on it are included
                                // (upstream defers to the print pass).
                                continue;
                            },
                            _ => continue,
                        };
                        if matched == negated {
                            include = false;
                            break;
                        }
                    }
                    if !include {
                        continue;
                    }
                }
                let Some(type_) = item.get("type").and_then(jsonc::Value::as_str) else {
                    return (
                        entries,
                        Some(
                            "module object must contain a \"type\" key ( case sensitive )"
                                .to_string(),
                        ),
                    );
                };
                // An object holding only "type" behaves like a bare string.
                let has_options = object_entries(item).is_some_and(|entries| entries.len() > 1);
                (
                    type_.to_string(),
                    has_options.then_some(item as &'static jsonc::Value),
                )
            },
            _ => {
                return (
                    entries,
                    Some("modules must be an array of strings or objects".to_string()),
                );
            },
        };
        entries.push(match opts {
            Some(opts) => Entry::Object {
                type_,
                opts: Some(opts),
            },
            None => Entry::Simple(type_),
        });
    }
    (entries, None)
}

fn object_array(value: &'static jsonc::Value) -> Option<&'static Vec<jsonc::Value>> {
    match value {
        jsonc::Value::Array(items) => Some(items),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) fn for_test(content: &str) -> Config {
    let Ok(root) = jsonc::Parser::parse(content, true) else {
        panic!("test config must parse");
    };
    build_config(leak_root(root))
}

/// listConfigPaths: every configDir + "fastfetch/", marked (*) when a
/// config.jsonc exists inside.
pub(crate) fn list_config_paths() {
    for dir in config_dirs() {
        let marker = std::path::Path::new(&format!("{dir}fastfetch/config.jsonc")).is_file();
        println!("{dir}fastfetch/{}", if marker { " (*)" } else { "" });
    }
}

/// listDataPaths: every dataDir + "fastfetch/".
pub(crate) fn list_data_paths() {
    for dir in data_dirs() {
        println!("{dir}fastfetch/");
    }
}

/// listFilesRecursively: hidden entries skipped, directories recurse with
/// one more "  | " indent; pretty prints "  | " per indent level before
/// each file, autocompletion prints the path relative to the walked root.
pub(crate) fn list_files_recursively(
    root: &str,
    dir: &str,
    pretty: bool,
    indentation: u8,
    folder_name: Option<&str>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    // Entering a subdirectory announces it one indent level up.
    if pretty
        && let Some(folder_name) = folder_name
    {
        for _ in 0..indentation.saturating_sub(1) {
            print!("  | ");
        }
        println!("{folder_name}/");
    }
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        let path = format!("{dir}{name}");
        if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            list_files_recursively(root, &format!("{path}/"), pretty, indentation + 1, Some(&name));
            continue;
        }
        if pretty {
            for _ in 0..indentation {
                print!("  | ");
            }
        } else {
            // The path from the walked root to the file's directory.
            print!("{}", &dir[root.len()..]);
        }
        println!("{name}");
    }
}

/// listAvailablePresets: dataDirs + "fastfetch/presets/", then the
/// presets shipped next to the executable.
pub(crate) fn list_presets(pretty: bool) {
    for dir in data_dirs() {
        let presets = format!("{dir}fastfetch/presets/");
        list_files_recursively(&presets, &presets, pretty, 0, None);
    }
    let exe = &crate::platform::platform().exe_path;
    if let Some(last_slash) = exe.rfind('/') {
        let presets = format!("{}/presets/", &exe[..=last_slash]);
        list_files_recursively(&presets, &presets, pretty, 0, None);
    }
}

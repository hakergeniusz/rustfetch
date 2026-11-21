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
    /// `logo.preserveAspectRatio` (yyjson_get_bool: non-bools read false).
    pub preserve_aspect_ratio: Option<bool>,
    pub logo_height: Option<u32>,
    /// `logo.printRemaining` (yyjson_get_bool: only JSON true reads as
    /// true).
    pub logo_print_remaining: Option<bool>,
    /// `logo.type` (all 17 enum values; none/small also set the sticky
    /// bools).
    pub logo_type: Option<crate::logo::LogoTypeOpt>,
    /// `logo.source` (base state; the command line's `--logo`/`--file`/...
    /// overwrite it).
    pub logo_source: Option<String>,
    /// `logo.color` slot overrides: (slot 0-based, parsed SGR params).
    pub logo_colors: Vec<(usize, &'static str)>,
    /// `logo.spin.speed.perFrame` / `.perRound` / `logo.spin.tilt` /
    /// `logo.spin.angle` (yyjson numbers cast to float).
    pub spin_speed_per_frame: Option<f32>,
    pub spin_speed_per_round: Option<f32>,
    pub spin_tilt: Option<f32>,
    pub spin_angle: Option<f32>,
    /// `display.showErrors`, implied by a truthy `display.stat`.
    pub show_errors: bool,
    /// `display.disableLinewrap` / `display.hideCursor`: the terminal
    /// escape toggles, pipe-disabled. None when the key is absent.
    pub disable_linewrap: Option<bool>,
    pub hide_cursor: Option<bool>,
    /// `display.stat`: the per-module timing threshold — 0 plain, a
    /// positive number colored green/yellow/red, -1 unset. None when the
    /// key is absent.
    pub stat: Option<i32>,
    /// `display.separator` (ffStrbufSetJsonVal: strings set it, any other
    /// type clears it), the parsed `display.color` slots and
    /// `display.brightColor` (yyjson_get_bool: non-bools read false).
    pub key_value_separator: Option<String>,
    pub color_keys: Option<String>,
    pub color_title: Option<String>,
    pub color_output: Option<String>,
    pub color_separator: Option<String>,
    pub bright_color: Option<bool>,
    /// `display.key.width` / `.paddingLeft` ((uint16_t) yyjson_get_uint:
    /// negative ints bit-cast huge) and `.type` (ffJsonConfigParseEnum).
    pub key_width: Option<u16>,
    pub key_padding_left: Option<u16>,
    pub key_type: Option<u8>,
    /// `display.key.language` (optionParseLanguageString's canonical
    /// code); None keeps the default English column.
    pub key_language: Option<&'static str>,
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
            preserve_aspect_ratio: None,
            logo_height: None,
            logo_print_remaining: None,
            logo_type: None,
            logo_source: None,
            logo_colors: Vec::new(),
            spin_speed_per_frame: None,
            spin_speed_per_round: None,
            spin_tilt: None,
            spin_angle: None,
            show_errors: false,
            disable_linewrap: None,
            hide_cursor: None,
            stat: None,
            key_value_separator: None,
            color_keys: None,
            color_title: None,
            color_output: None,
            color_separator: None,
            bright_color: None,
            key_width: None,
            key_padding_left: None,
            key_type: None,
            key_language: None,
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
                // The installed fastfetch builds with image logo support, so
                // every enum name is accepted; the image types fall back to
                // the detected logo after the sizing probe when rendering.
                // ffJsonConfigParseEnum: strings match by name ("Invalid
                // enum string"), integers index the FFLogoType order 0..=16
                // ("Invalid enum integer"), and any other JSON type reports
                // the generic message.
                let parsed_error: Option<&str> = match value {
                    jsonc::Value::Int(_) | jsonc::Value::UInt(_) => {
                        let index = value.as_uint().unwrap_or(u64::MAX);
                        match (value.as_int(), index) {
                            (Some(i), _) if (0..=16).contains(&i) => None,
                            (None, index) if (0..=16).contains(&index) => None,
                            _ => Some("Invalid enum integer"),
                        }
                    },
                    jsonc::Value::Str(_) => None,
                    _ => Some("Invalid enum value type; must be a string or integer"),
                };
                if let Some(error) = parsed_error {
                    section_error("logo", Some(key), error);
                }
                let error = match value.as_str() {
                    Some(name)
                        if [
                            "auto", "builtin", "small", "file", "file-raw", "data", "data-raw",
                            "command-raw", "sixel", "kitty", "kitty-direct", "kitty-icat",
                            "iterm", "chafa", "raw", "spin", "none",
                        ]
                        .iter()
                        .any(|known| known.eq_ignore_ascii_case(name)) =>
                    {
                        let parsed = match name.to_ascii_lowercase().as_str() {
                            "auto" => crate::logo::LogoTypeOpt::Auto,
                            "builtin" => crate::logo::LogoTypeOpt::Builtin,
                            "small" => crate::logo::LogoTypeOpt::Small,
                            "file" => crate::logo::LogoTypeOpt::File,
                            "file-raw" => crate::logo::LogoTypeOpt::FileRaw,
                            "data" => crate::logo::LogoTypeOpt::Data,
                            "data-raw" => crate::logo::LogoTypeOpt::DataRaw,
                            "command-raw" => crate::logo::LogoTypeOpt::CommandRaw,
                            "sixel" => crate::logo::LogoTypeOpt::ImageSixel,
                            "kitty" => crate::logo::LogoTypeOpt::ImageKitty,
                            "kitty-direct" => crate::logo::LogoTypeOpt::ImageKittyDirect,
                            "kitty-icat" => crate::logo::LogoTypeOpt::ImageKittyIcat,
                            "iterm" => crate::logo::LogoTypeOpt::ImageIterm,
                            "chafa" => crate::logo::LogoTypeOpt::ImageChafa,
                            "raw" => crate::logo::LogoTypeOpt::ImageRaw,
                            "spin" => crate::logo::LogoTypeOpt::Spin,
                            _ => crate::logo::LogoTypeOpt::None,
                        };
                        if matches!(parsed, crate::logo::LogoTypeOpt::None) {
                            config.logo_none = true;
                        }
                        if matches!(parsed, crate::logo::LogoTypeOpt::Small) {
                            config.logo_small = true;
                        }
                        config.logo_type = Some(parsed);
                        None
                    },
                    // Integers were validated (or rejected) above; their
                    // ordinal routing happens below.
                    None => None,
                    _ => Some("Invalid enum string"),
                };
                if let Some(error) = error {
                    section_error("logo", Some(key), error);
                }
                if config.logo_type.is_none() && !matches!(value, jsonc::Value::Str(_)) {
                    // The integer path: the enum's ordinal indexes
                    // LogoTypeOpt's declaration order, FFLogoType's own.
                    let index = value.as_uint().unwrap_or(u64::MAX);
                    let ordinal = match value.as_int() {
                        Some(i) if (0..=16).contains(&i) => i as usize,
                        _ if (0..=16).contains(&index) => index as usize,
                        _ => unreachable!(),
                    };
                    let parsed = [
                        crate::logo::LogoTypeOpt::Auto,
                        crate::logo::LogoTypeOpt::Builtin,
                        crate::logo::LogoTypeOpt::Small,
                        crate::logo::LogoTypeOpt::File,
                        crate::logo::LogoTypeOpt::FileRaw,
                        crate::logo::LogoTypeOpt::Data,
                        crate::logo::LogoTypeOpt::DataRaw,
                        crate::logo::LogoTypeOpt::CommandRaw,
                        crate::logo::LogoTypeOpt::ImageSixel,
                        crate::logo::LogoTypeOpt::ImageKitty,
                        crate::logo::LogoTypeOpt::ImageKittyDirect,
                        crate::logo::LogoTypeOpt::ImageKittyIcat,
                        crate::logo::LogoTypeOpt::ImageIterm,
                        crate::logo::LogoTypeOpt::ImageChafa,
                        crate::logo::LogoTypeOpt::ImageRaw,
                        crate::logo::LogoTypeOpt::Spin,
                        crate::logo::LogoTypeOpt::None,
                    ][ordinal];
                    if matches!(parsed, crate::logo::LogoTypeOpt::None) {
                        config.logo_none = true;
                    }
                    if matches!(parsed, crate::logo::LogoTypeOpt::Small) {
                        config.logo_small = true;
                    }
                    config.logo_type = Some(parsed);
                }
            },
            "source" => {
                // ffStrbufSetJsonVal stringifies every scalar and leaves
                // null as the empty (unset) source.
                config.logo_source = match value {
                    jsonc::Value::Str(text) => Some(text.clone()),
                    jsonc::Value::Int(num) => Some(num.to_string()),
                    jsonc::Value::UInt(num) => Some(num.to_string()),
                    jsonc::Value::Bool(flag) => Some(flag.to_string()),
                    jsonc::Value::Float(num) => Some(num.to_string()),
                    jsonc::Value::Null => None,
                    _ => section_error("logo", Some(key), "Property 'source' must be a scalar"),
                };
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
            "color" => {
                // Keys "1".."9" name palette slots (strtoul: a non-number
                // reads as 0 and errors); values parse through
                // ffOptionParseColor — a non-string value reads as NULL and
                // clears the slot (a no-op here).
                let Some(entries) = object_entries(value) else {
                    section_error("logo", Some(key), "Property 'color' must be an object");
                };
                for (slot, color) in entries {
                    let index: u64 = slot.parse().unwrap_or(0);
                    if !(1..=9).contains(&index) {
                        section_error("logo", Some(key), "Keys of property 'color' must be a number between 1 to 9");
                    }
                    if let jsonc::Value::Str(text) = color {
                        // The logo section walks before display (fastfetch.c:429),
                        // so the PROP words here resolve against the init state.
                        let parsed = crate::cli::ColorState::fresh().parse_color(text);
                        if !parsed.is_empty() {
                            config.logo_colors.push(((index - 1) as usize, parsed.leak()));
                        }
                    }
                }
            },
            "spin" => {
                // ffOptionsParseLogoJsonConfig's spin block: per-key checks
                // in walk order, with the range checks shaped like C's
                // (`< min || > max`) so a NaN number passes.
                let Some(spin_entries) = object_entries(value) else {
                    section_error("logo", Some(key), "Property 'logo.spin' must be an object");
                };
                for (spin_key, spin_val) in spin_entries {
                    match spin_key.as_str() {
                        "speed" => {
                            let Some(speed_entries) = object_entries(spin_val) else {
                                section_error("logo", Some(key), "Property 'logo.spin.speed' must be an object");
                            };
                            for (speed_key, speed_val) in speed_entries {
                                match speed_key.as_str() {
                                    "perFrame" => {
                                        config.spin_speed_per_frame = Some(spin_number(
                                            speed_val,
                                            -360.0,
                                            360.0,
                                            "Property 'logo.spin.speed.perFrame' must be a number between -360 and 360",
                                            key,
                                        ));
                                    },
                                    "perRound" => {
                                        config.spin_speed_per_round = Some(spin_number(
                                            speed_val,
                                            -3600.0,
                                            3600.0,
                                            "Property 'logo.spin.speed.perRound' must be a number between -3600 and 3600",
                                            key,
                                        ));
                                    },
                                    _ => section_error("logo", Some(key), "Unknown logo.spin.speed key"),
                                }
                            }
                        },
                        "tilt" => {
                            config.spin_tilt = Some(spin_number(
                                spin_val,
                                -80.0,
                                80.0,
                                "Property 'logo.spin.tilt' must be a number between -80 and 80",
                                key,
                            ));
                        },
                        "angle" => {
                            config.spin_angle = Some(spin_number(
                                spin_val,
                                0.0,
                                360.0,
                                "Property 'logo.spin.angle' must be a number between 0 and 360",
                                key,
                            ));
                        },
                        _ => section_error("logo", Some(key), "Unknown logo.spin key"),
                    }
                }
            },
            "preserveAspectRatio" => {
                // yyjson_get_bool: any non-bool reads false.
                config.preserve_aspect_ratio = Some(value.as_bool().unwrap_or(false));
            },
            "cache" | "recache" | "animationFrame" => {
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
/// rest are accepted with upstream's type checks where they exist. The
/// color slots parse against a running ColorState, so PROP words resolve
/// in document order (upstream's shared options struct).
fn apply_display(root: &jsonc::Value, config: &mut Config) {
    let Some(display) = root.get("display") else {
        return;
    };
    let Some(entries) = object_entries(display) else {
        section_error("display", None, "Property 'display' must be an object");
    };
    let mut state = crate::cli::ColorState::fresh();
    for (key, value) in entries {
        match key.as_str() {
            "stat" => match value {
                // true = plain timing, false = unset; a non-negative
                // number is the color-change threshold and implies
                // showErrors (a null errors like upstream's is_bool/
                // is_uint chain).
                jsonc::Value::Bool(true) => {
                    config.stat = Some(0);
                    config.show_errors = true;
                },
                jsonc::Value::Bool(false) => config.stat = Some(-1),
                jsonc::Value::UInt(u) => {
                    config.stat = Some(*u as i32);
                    config.show_errors = true;
                },
                jsonc::Value::Int(i) if *i >= 0 => {
                    config.stat = Some(*i as i32);
                    config.show_errors = true;
                },
                _ => section_error(
                    "display",
                    Some(key),
                    "display.stat must be a boolean or a positive integer",
                ),
            },
            "disableLinewrap" => config.disable_linewrap = Some(value.as_bool().unwrap_or(false)),
            "hideCursor" => config.hide_cursor = Some(value.as_bool().unwrap_or(false)),
            // yyjson_get_bool accepts any value type; non-bools read as false.
            "pipe" => config.pipe = value.as_bool().unwrap_or(false),
            "showErrors" => config.show_errors = value.as_bool().unwrap_or(false),
            "brightColor" => {
                state.bright = value.as_bool().unwrap_or(false);
                config.bright_color = Some(state.bright);
            },
            "noBuffer" => {},
            // ffStrbufSetJsonVal: a string sets the separator, anything
            // else clears it.
            "separator" => {
                config.key_value_separator = Some(match value {
                    jsonc::Value::Str(text) => text.clone(),
                    _ => String::new(),
                });
            },
            "color" => match value {
                jsonc::Value::Str(text) => {
                    let parsed = state.parse_color(text);
                    state.keys = parsed.clone();
                    state.title = parsed.clone();
                    config.color_keys = Some(parsed.clone());
                    config.color_title = Some(parsed);
                },
                jsonc::Value::Object(_) => {
                    // Per-slot yyjson_obj_get lookups in upstream's fixed
                    // order; a non-string value reads as NULL and is a
                    // no-op, and unknown sub-objects are ignored.
                    for (slot, field) in [
                        ("keys", Slot::Keys),
                        ("title", Slot::Title),
                        ("output", Slot::Output),
                        ("separator", Slot::Separator),
                    ] {
                        let Some(color) = value.get(slot).and_then(|color| color.as_str()) else {
                            continue;
                        };
                        let parsed = state.parse_color(color);
                        match field {
                            Slot::Keys => {
                                state.keys = parsed.clone();
                                config.color_keys = Some(parsed);
                            },
                            Slot::Title => {
                                state.title = parsed.clone();
                                config.color_title = Some(parsed);
                            },
                            Slot::Output => {
                                state.output = parsed.clone();
                                config.color_output = Some(parsed);
                            },
                            Slot::Separator => {
                                state.separator = parsed.clone();
                                config.color_separator = Some(parsed);
                            },
                        }
                    }
                },
                _ => section_error("display", Some(key), "display.color must be either a string or an object"),
            },
            "key" => {
                let Some(_) = object_entries(value) else {
                    section_error("display", Some(key), "display.key must be an object");
                };
                // Upstream probes the sub-keys with yyjson_obj_get in a
                // fixed order (width, type, paddingLeft, language), so a
                // duplicated key's first occurrence wins.
                if let Some(width) = value.get("width") {
                    config.key_width = Some(json_get_uint16(width));
                }
                if let Some(kind) = value.get("type") {
                    config.key_type = Some(json_config_enum(
                        kind,
                        &[
                            ("none", crate::instance::KEY_TYPE_NONE),
                            ("string", crate::instance::KEY_TYPE_STRING),
                            ("icon", crate::instance::KEY_TYPE_ICON),
                            ("both", 3 | 1 << 4),
                            ("both-0", 3),
                            ("both-1", 3 | 1 << 4),
                            ("both-2", 3 | 2 << 4),
                            ("both-3", 3 | 3 << 4),
                            ("both-4", 3 | 4 << 4),
                        ],
                    ));
                }
                if let Some(padding) = value.get("paddingLeft") {
                    config.key_padding_left = Some(json_get_uint16(padding));
                }
                if let Some(language) = value.get("language") {
                    if language.as_str().is_none() && !matches!(language, jsonc::Value::Null) {
                        section_error("display", Some(key), "display.key.language must be a string or null");
                    }
                    config.key_language = Some(parse_language_string(language.as_str().unwrap_or("")));
                }
            },
            "constants" => {
                if !matches!(value, jsonc::Value::Array(_)) {
                    section_error("display", Some(key), "display.constants must be an array");
                }
            },
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

/// Which display.color object slot a sub-key fills.
enum Slot {
    Keys,
    Title,
    Output,
    Separator,
}

/// (uint16_t) yyjson_get_uint: integers read through (negative ints
/// bit-cast huge), every other type reads as zero.
fn json_get_uint16(value: &jsonc::Value) -> u16 {
    let raw = match value {
        jsonc::Value::UInt(amount) => *amount,
        jsonc::Value::Int(amount) => *amount as u64,
        _ => 0,
    };
    raw as u16
}

/// ffJsonConfigParseEnum: integers match pair values ("Invalid enum
/// integer"), strings match names case-insensitively ("Invalid enum
/// string"), anything else is a type error. The section/key pair is
/// display.key because upstream's walk leaves `*pkey` at the outer key.
fn json_config_enum(value: &jsonc::Value, pairs: &[(&str, u8)]) -> u8 {
    match value {
        jsonc::Value::Int(num) => {
            if let Some((_, code)) = pairs.iter().find(|(_, code)| *num == *code as i64) {
                *code
            } else {
                section_error("display", Some("key"), "Invalid enum integer");
            }
        },
        jsonc::Value::UInt(num) => {
            if let Some((_, code)) = pairs.iter().find(|(_, code)| *num == *code as u64) {
                *code
            } else {
                section_error("display", Some("key"), "Invalid enum integer");
            }
        },
        jsonc::Value::Str(text) => {
            if let Some((_, code)) = pairs.iter().find(|(name, _)| text.eq_ignore_ascii_case(name)) {
                *code
            } else {
                section_error("display", Some("key"), "Invalid enum string");
            }
        },
        _ => section_error("display", Some("key"), "Invalid enum value type; must be a string or integer"),
    }
}

/// optionParseLanguageString's detection + normalization + 2-char switch,
/// returning the canonical language code; the error paths match upstream
/// exactly. The resolved value feeds the display-name table
/// (Instance::display_name).
pub(crate) fn parse_language_string(value: &str) -> &'static str {
    // ffDetectLocale: LC_ALL, then LANG, then setlocale(LC_TIME, nullptr) —
    // the port's startup equivalent of `setlocale(LC_TIME, "")` reads
    // LC_TIME when LC_ALL/LANG were empty, else the "C" locale. All empty
    // is impossible after the setlocale leg, so the "Failed to detect
    // locale" exit stays unreachable here.
    let detected = value.is_empty();
    let mut language = if detected {
        ["LC_ALL", "LANG", "LC_TIME"]
            .iter()
            .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
            .unwrap_or_else(|| "C".to_string())
    } else {
        value.to_string()
    };
    // SubstrBeforeFirstC('.') (the encoding) and ('@') (the modifier,
    // Linux), then lowercase.
    language = language.split('.').next().unwrap_or("").to_string();
    language = language.split('@').next().unwrap_or("").to_string();
    language = language.to_ascii_lowercase();

    const LANGS: &[&str] = &[
        "en", "ar", "cs", "de", "es", "fi", "fr", "gl", "he", "id", "it", "ja", "ko", "pl", "pt", "ru", "tr", "uk", "vi",
    ];
    // The error label: the detected locale falls back to English silently,
    // an explicit one reports and exits 477.
    let unsupported = |language: &str| -> &'static str {
        if detected {
            return "en";
        }
        eprintln!("Error: Language '{language}' is not supported");
        std::process::exit(477);
    };
    if language.len() <= 1 {
        // chars[0] on the NUL-terminated buffer: empty reads as '\0'.
        if language == "c" {
            return "en";
        }
        return unsupported(&language);
    }
    if language.len() > 2 {
        match language.as_bytes()[2] {
            b'-' => language.replace_range(2..3, "_"),
            b'_' => {},
            _ => return unsupported(&language),
        }
    }
    // The switch matches the first two chars only: "de_DE" resolves to de,
    // while zh needs the full zh_cn/zh_tw spelling (the switch's own case,
    // not part of the LANGS tail).
    if &language[..2] == "zh" {
        return match language.as_str() {
            "zh_cn" => "zh_cn",
            "zh_tw" => "zh_tw",
            _ => unsupported(&language),
        };
    }
    let Some(lang) = LANGS.iter().find(|lang| **lang == &language[..2]) else {
        return unsupported(&language);
    };
    lang
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

/// yyjson_is_num + yyjson_get_num: integers and reals, NaN and infinity
/// included for the range checks that follow.
fn json_num(value: &jsonc::Value) -> Option<f64> {
    match value {
        jsonc::Value::Int(i) => Some(*i as f64),
        jsonc::Value::UInt(u) => Some(*u as f64),
        jsonc::Value::Float(f) => Some(*f),
        _ => None,
    }
}

/// One logo.spin numeric key: the value must read as a number and pass the
/// C-shaped range check (`< min || > max`, so a NaN passes) before it is
/// stored as a float.
fn spin_number(value: &jsonc::Value, min: f64, max: f64, message: &str, spin_key: &str) -> f32 {
    let Some(num) = json_num(value) else {
        section_error("logo", Some(spin_key), message);
    };
    if num < min || num > max {
        section_error("logo", Some(spin_key), message);
    }
    num as f32
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

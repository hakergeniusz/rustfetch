//! Module registry: every module is a name plus a function returning rows.

pub mod battery;
pub mod bios;
pub mod bluetooth;
pub mod bluetoothradio;
pub mod board;
pub mod brightness;
pub mod camera;
pub mod r#break;
pub mod chassis;
pub mod codec;
pub mod command;
pub mod colors;
pub mod custom;
pub mod cpu;
pub mod cpucache;
pub mod cpuusage;
pub mod cursor;
pub mod datetime;
pub mod de;
pub mod btrfs;
pub mod disk;
pub mod diskio;
pub mod display;
pub mod editor;
pub mod dns;
pub mod font;
pub mod gamepad;
pub mod gpu;
pub mod gtk;
pub mod host;
pub mod icons;
pub mod initsystem;
pub mod kernel;
pub mod keyboard;
pub mod lm;
pub mod loadavg;
pub mod locale;
pub mod localip;
pub mod media;
pub mod player;
pub mod memory;
pub mod monitor;
pub mod terminaltheme;
pub mod top;
pub mod mouse;
pub mod netio;
pub mod opencl;
pub mod opengl;
pub mod os;
pub mod packages;
pub mod physicaldisk;
pub mod physicalmemory;
pub mod poweradapter;
pub mod processes;
pub mod qt;
pub mod separator;
pub mod shell;
pub mod swap;
pub mod terminal;
pub mod terminalfont;
pub mod terminalsize;
pub mod theme;
pub mod tpm;
pub mod version;
pub mod title;
pub mod uptime;
pub mod users;
pub mod wallpaper;
pub mod wm;
pub mod wmtheme;

use crate::{instance::Instance, util};

/// How a row should be rendered.
#[derive(Clone, Copy)]
pub enum Kind {
    /// Styled by the renderer: `key: value` or accent-colored title.
    Value,
    /// Printed verbatim; the module owns any escape sequences.
    Raw,
}

pub struct Row {
    pub key: Option<String>,
    pub value: String,
    pub kind: Kind,
}

impl Row {
    /// Regular `key: value` line.
    pub fn new(key: &str, value: impl Into<String>) -> Self {
        Self {
            key: Some(key.to_string()),
            value: value.into(),
            kind: Kind::Value,
        }
    }

    /// Keyless line styled by the renderer (e.g. the title).
    pub fn bare(value: impl Into<String>) -> Self {
        Self {
            key: None,
            value: value.into(),
            kind: Kind::Value,
        }
    }

    /// Keyless line printed as-is (separator, color blocks).
    pub fn raw(value: impl Into<String>) -> Self {
        Self {
            key: None,
            value: value.into(),
            kind: Kind::Raw,
        }
    }
}

pub struct ModuleDef {
    pub name: &'static str,
    pub doc: &'static str,
    pub run: fn(&Instance) -> Vec<Row>,
}

/// All modules in fastfetch's ordering; `break` can't use the old macro
/// because its name is a Rust keyword.
pub const REGISTRY: &[ModuleDef] = &[
    ModuleDef { name: "title", doc: "user@host", run: title::run },
    ModuleDef { name: "separator", doc: "dashed separator line", run: separator::run },
    ModuleDef { name: "os", doc: "operating system name and architecture", run: os::run },
    ModuleDef { name: "opencl", doc: "opencl platform version", run: opencl::run },
    ModuleDef { name: "opengl", doc: "gl version via a headless context", run: opengl::run },
    ModuleDef { name: "host", doc: "device model name", run: host::run },
    ModuleDef { name: "bios", doc: "firmware version and release date", run: bios::run },
    ModuleDef { name: "bluetooth", doc: "connected bluetooth devices", run: bluetooth::run },
    ModuleDef { name: "bluetoothradio", doc: "bluetooth adapters and their versions", run: bluetoothradio::run },
    ModuleDef { name: "board", doc: "motherboard name and version", run: board::run },
    ModuleDef { name: "chassis", doc: "smbios chassis type and version", run: chassis::run },
    ModuleDef { name: "codec", doc: "hardware video acceleration codecs", run: codec::run },
    ModuleDef { name: "kernel", doc: "kernel name and release", run: kernel::run },
    ModuleDef { name: "initsystem", doc: "pid 1 name and version", run: initsystem::run },
    ModuleDef { name: "uptime", doc: "time since boot", run: uptime::run },
    ModuleDef { name: "users", doc: "users logged in with their login time", run: users::run },
    ModuleDef { name: "tpm", doc: "tpm version", run: tpm::run },
    ModuleDef { name: "version", doc: "binary version and architecture", run: version::run },
    ModuleDef { name: "loadavg", doc: "load averages of the last 1, 5 and 15 minutes", run: loadavg::run },
    ModuleDef { name: "keyboard", doc: "keyboard devices", run: keyboard::run },
    ModuleDef { name: "lm", doc: "login manager", run: lm::run },
    ModuleDef { name: "processes", doc: "number of running processes", run: processes::run },
    ModuleDef { name: "packages", doc: "installed packages by package manager", run: packages::run },
    ModuleDef { name: "shell", doc: "user shell and version", run: shell::run },
    ModuleDef { name: "editor", doc: "$VISUAL or $EDITOR and its version", run: editor::run },
    ModuleDef { name: "display", doc: "monitors: resolution, size, refresh rate", run: display::run },
    ModuleDef { name: "brightness", doc: "monitor brightness by backlight or DDC/CI", run: brightness::run },
    ModuleDef { name: "de", doc: "desktop environment", run: de::run },
    ModuleDef { name: "wm", doc: "window manager", run: wm::run },
    ModuleDef { name: "wmtheme", doc: "window manager theme", run: wmtheme::run },
    ModuleDef { name: "theme", doc: "gtk theme", run: theme::run },
    ModuleDef { name: "icons", doc: "icon theme", run: icons::run },
    ModuleDef { name: "font", doc: "desktop font", run: font::run },
    ModuleDef { name: "cursor", doc: "cursor theme and size", run: cursor::run },
    ModuleDef { name: "terminal", doc: "terminal emulator", run: terminal::run },
    ModuleDef { name: "terminalfont", doc: "terminal font", run: terminalfont::run },
    ModuleDef { name: "terminalsize", doc: "terminal dimensions in cells and pixels", run: terminalsize::run },
    ModuleDef { name: "cpu", doc: "cpu model, core count and frequency", run: cpu::run },
    ModuleDef { name: "cpucache", doc: "cpu cache levels grouped by size", run: cpucache::run },
    ModuleDef { name: "cpuusage", doc: "average cpu usage over the last 200ms", run: cpuusage::run },
    ModuleDef { name: "gpu", doc: "gpu vendor and model", run: gpu::run },
    ModuleDef { name: "gamepad", doc: "gamepad devices and battery", run: gamepad::run },
    ModuleDef { name: "mouse", doc: "mouse devices", run: mouse::run },
    ModuleDef { name: "memory", doc: "ram usage", run: memory::run },
    ModuleDef { name: "monitor", doc: "same as display, different default output", run: monitor::run },
    ModuleDef { name: "terminaltheme", doc: "terminal fg/bg colors", run: terminaltheme::run },
    ModuleDef { name: "top", doc: "processes with the highest cpu/memory/disk usage", run: top::run },
    ModuleDef { name: "swap", doc: "swap usage", run: swap::run },
    ModuleDef { name: "disk", doc: "root filesystem usage", run: disk::run },
    ModuleDef { name: "btrfs", doc: "mounted btrfs filesystem usage", run: btrfs::run },
    ModuleDef { name: "physicaldisk", doc: "physical storage devices", run: physicaldisk::run },
    ModuleDef { name: "physicalmemory", doc: "dimm modules from the udev database", run: physicalmemory::run },
    ModuleDef { name: "netio", doc: "default-route interface rx/tx rates", run: netio::run },
    ModuleDef { name: "diskio", doc: "physical disk read/write rates", run: diskio::run },
    ModuleDef { name: "localip", doc: "lan addresses of network interfaces", run: localip::run },
    ModuleDef { name: "dns", doc: "dns servers from resolv.conf", run: dns::run },
    ModuleDef { name: "datetime", doc: "local date and time", run: datetime::run },
    ModuleDef { name: "locale", doc: "system locale", run: locale::run },
    ModuleDef { name: "battery", doc: "battery charge and status", run: battery::run },
    ModuleDef { name: "poweradapter", doc: "power adapter wattage", run: poweradapter::run },
    ModuleDef { name: "camera", doc: "video capture devices", run: camera::run },
    ModuleDef { name: "wallpaper", doc: "file name of the current wallpaper", run: wallpaper::run },
    ModuleDef { name: "media", doc: "currently playing song and its progress", run: media::run },
    ModuleDef { name: "player", doc: "active media player name", run: player::run },
    ModuleDef { name: "break", doc: "an empty line", run: r#break::run },
    ModuleDef { name: "command", doc: "output of a config command", run: command::run },
    ModuleDef { name: "custom", doc: "custom text from a config format string", run: custom::run },
    ModuleDef { name: "colors", doc: "terminal color palette", run: colors::run },
];

/// Modules shown when no explicit selection is given, mirroring fastfetch's
/// default preset (CMakeLists.txt DEFAULT_STRUCTURE). A fastfetch
/// config.jsonc with a modules list overrides this.
pub const DEFAULT_PRESET: &[&str] = &[
    "title",
    "separator",
    "os",
    "host",
    "kernel",
    "uptime",
    "packages",
    "shell",
    "display",
    "de",
    "wm",
    "wmtheme",
    "theme",
    "icons",
    "font",
    "cursor",
    "terminal",
    "terminalfont",
    "cpu",
    "gpu",
    "memory",
    "swap",
    "disk",
    "localip",
    "battery",
    "poweradapter",
    "locale",
    "break",
    "colors",
];

/// fastfetch preset compatibility: a ~/.config/fastfetch/config.jsonc with a
/// "modules" array takes over the default preset. Unknown module names are
/// skipped (this port covers a subset of fastfetch's modules).
/// One dispatch unit from the module list: a registry module or a
/// config-supplied custom text / shell command row.
pub enum PresetJob {
    Module(&'static ModuleDef),
    Custom(String),
    Command {
        text: String,
        shell: String,
        param: Option<String>,
        split_lines: bool,
    },
}

fn module_job(name: &str) -> Option<PresetJob> {
    REGISTRY
        .iter()
        .find(|module| module.name.eq_ignore_ascii_case(name))
        .map(PresetJob::Module)
}

fn preset_from_fastfetch_config() -> Option<Vec<PresetJob>> {
    // fastfetch resolves the config dir via XDG_CONFIG_HOME, falling back
    // to $HOME/.config.
    let home = std::env::var_os("HOME")?;
    let mut config = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => std::path::PathBuf::from(dir),
        _ => std::path::PathBuf::from(home).join(".config"),
    };
    config.push("fastfetch/config.jsonc");
    preset_from_fastfetch_config_content(&util::read_file(config.to_str()?)?)
}

/// Parses the "modules" array out of a fastfetch config's JSONC content.
/// Array items are module name strings or objects; custom objects carry
/// their format text through, other objects run their type's module with
/// the options dropped.
fn preset_from_fastfetch_config_content(content: &str) -> Option<Vec<PresetJob>> {
    // strip comments, then take the array following "modules"
    let mut cleaned = String::with_capacity(content.len());
    let mut in_string = false;
    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            if c == '"' {
                in_string = false;
            }
            cleaned.push(c);
            continue;
        }
        if c == '"' {
            in_string = true;
            cleaned.push(c);
        } else if c == '/' && chars.peek() == Some(&'/') {
            for skipped in chars.by_ref() {
                if skipped == '\n' {
                    cleaned.push('\n');
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(skipped) = chars.next() {
                if skipped == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else {
            cleaned.push(c);
        }
    }

    let modules_pos = cleaned.find("\"modules\"")?;
    let rest = &cleaned[modules_pos..];
    let start = rest.find('[')?;
    let depth_end = rest[start..]
        .char_indices()
        .scan(0i32, |depth, (i, c)| {
            *depth += match c {
                '[' => 1,
                ']' => -1,
                _ => 0,
            };
            Some((*depth, i))
        })
        .find(|(depth, _)| *depth == 0)
        .map(|(_, i)| i + start)?;
    let array = &rest[start + 1..depth_end];

    let chars: Vec<char> = array.chars().collect();
    let mut jobs = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            ',' | ' ' | '\n' | '\t' | '\r' => i += 1,
            '"' => {
                let (name, next) = read_json_string(&chars, i);
                i = next;
                if let Some(job) = module_job(&name) {
                    jobs.push(job);
                }
            },
            '{' => {
                // Object item: {"type": "custom", "format": "..."} or
                // {"type": "<module>", ...} with the options dropped.
                let depth_end = chars[i..]
                    .iter()
                    .scan(0i32, |depth, &c| {
                        *depth += match c {
                            '{' => 1,
                            '}' => -1,
                            _ => 0,
                        };
                        Some(*depth)
                    })
                    .position(|depth| depth == 0)
                    .map(|offset| i + offset)?;
                let object: String = chars[i..=depth_end].iter().collect();
                let type_ = json_string_field(&object, "type").unwrap_or_default();
                if type_.eq_ignore_ascii_case("custom") {
                    // A custom without a format still prints its empty row.
                    let format = json_string_field(&object, "format").unwrap_or_default();
                    jobs.push(PresetJob::Custom(format));
                } else if type_.eq_ignore_ascii_case("command") {
                    // fastfetch's defaults: /bin/sh -c, splitLines off.
                    jobs.push(PresetJob::Command {
                        text: json_string_field(&object, "text").unwrap_or_default(),
                        shell: json_string_field(&object, "shell")
                            .unwrap_or_else(|| "/bin/sh".to_string()),
                        param: match json_string_field(&object, "param") {
                            Some(param) => Some(param),
                            None => Some("-c".to_string()),
                        },
                        split_lines: json_bool_field(&object, "splitLines").unwrap_or(false),
                    });
                } else if let Some(job) = module_job(&type_) {
                    jobs.push(job);
                }
                i = depth_end + 1;
            },
            _ => i += 1,
        }
    }
    (!jobs.is_empty()).then_some(jobs)
}

/// Reads a JSON string starting at the opening quote; \escapes decode.
fn read_json_string(chars: &[char], start: usize) -> (String, usize) {
    let mut out = String::new();
    let mut i = start + 1;
    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() => {
                out.push(match chars[i + 1] {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    'b' => '\u{8}',
                    'f' => '\u{c}',
                    other => other,
                });
                i += 2;
            },
            '"' => return (out, i + 1),
            c => {
                out.push(c);
                i += 1;
            },
        }
    }
    (out, i)
}

/// Finds `"name": true/false` inside a JSON object's text.
fn json_bool_field(object: &str, name: &str) -> Option<bool> {
    let needle = format!("\"{name}\"");
    let pos = object.find(&needle)? + needle.len();
    let rest = object[pos..].trim_start().strip_prefix(':')?.trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Finds `"name": "value"` inside a JSON object's text.
fn json_string_field(object: &str, name: &str) -> Option<String> {
    let needle = format!("\"{name}\"");
    let pos = object.find(&needle)? + needle.len();
    let rest = object[pos..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let chars: Vec<char> = rest.chars().collect();
    if chars.first() != Some(&'"') {
        return None;
    }
    let (value, _) = read_json_string(&chars, 0);
    Some(value)
}

pub fn list_modules() {
    for module in REGISTRY {
        println!("{:10} {}", module.name, module.doc);
    }
}

/// ffPercentAppendNum: the number is formatted like the global
/// percentNdigits=0 default and colored by the module's own green/yellow
/// thresholds, compared before rounding. Modules where a high value is good
/// (battery, physicaldisk) set green > yellow, which inverts the bands.
pub fn colorize_percent(pct: f64, green: u8, yellow: u8, color: bool) -> String {
    let text = format!("{pct:.0}");
    let code = if green <= yellow {
        if pct > yellow as f64 {
            "91"
        } else if pct > green as f64 {
            "93"
        } else {
            "32"
        }
    } else if pct < yellow as f64 {
        "91"
    } else if pct < green as f64 {
        "93"
    } else {
        "32"
    };
    if color {
        format!("\x1b[{code}m{text}%\x1b[m")
    } else {
        format!("{text}%")
    }
}

/// Runs the selected modules (or the default preset) in structure order.
///
/// Modules are independent of each other, so each one runs in its own
/// scoped thread: the slow data sources (gdbus, gsettings, process scans)
/// overlap instead of adding up. Rows are emitted in structure order.
pub fn collect(selected: &[String], instance: &Instance) -> Vec<Row> {
    let jobs: Vec<PresetJob> = if selected.is_empty() {
        preset_from_fastfetch_config().unwrap_or_else(|| {
            DEFAULT_PRESET
                .iter()
                .filter_map(|name| module_job(name))
                .collect()
        })
    } else {
        // Unknown names are skipped like fastfetch's structure parser: the
        // token becomes a hidden error row and the rest still runs. Module
        // names match case-insensitively (ffStrEqualsIgnCase).
        selected.iter().filter_map(|name| module_job(name)).collect()
    };

    let wants = |name: &str| {
        jobs.iter().any(|job| matches!(job, PresetJob::Module(module) if module.name == name))
    };

    // The rate modules sample counters twice; start the clock before the
    // parallel detection runs, like fastfetch's ffPrepare* during option
    // parsing.
    if wants("top") {
        top::prepare();
    }
    if wants("cpuusage") {
        cpuusage::prepare();
    }
    if wants("netio") {
        netio::prepare();
    }
    if wants("diskio") {
        diskio::prepare();
    }

    let results: Vec<Vec<Row>> = std::thread::scope(|scope| {
        // Structure order wins: `-s b:os:a` prints rows in exactly that
        // order, like fastfetch's structure loop. Duplicates allowed.
        jobs.iter()
            .map(|job| match job {
                PresetJob::Module(module) => scope.spawn(move || (module.run)(instance)),
                PresetJob::Custom(text) => {
                    scope.spawn(move || custom::render_rows(text, instance))
                },
                PresetJob::Command {
                    text,
                    shell,
                    param,
                    split_lines,
                } => scope.spawn(move || {
                    command::run_text(text, shell, param.as_deref(), *split_lines, instance)
                }),
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().unwrap_or_default())
            .collect()
    });
    results.into_iter().flatten().collect()
}

#[cfg(test)]
mod preset_tests {
    use super::PresetJob;

    fn job_names(jobs: &[PresetJob]) -> Vec<String> {
        jobs.iter()
            .map(|job| match job {
                PresetJob::Module(module) => module.name.to_string(),
                PresetJob::Custom(text) => format!("custom:{text}"),
                PresetJob::Command { text, .. } => format!("command:{text}"),
            })
            .collect()
    }

    #[test]
    fn fastfetch_config_preset_parses() {
        let config = r#"
// this is a JSONC comment
{
    "$schema": "https://github.com/fastfetch-cli/fastfetch/raw/dev/doc/json/schema.json",
    "logo": {"type": "small"},
    /* block comment, ignored */
    "modules": [
        "title",
        "separator",
        // comments inside the array too
        "os", "host",
        "break",
        {"type": "custom", "format": "hello {#red}world"},
        {"type": "os", "key": "ignored"},
        "colors",
    ],
}
"#;
        let Some(jobs) = super::preset_from_fastfetch_config_content(config) else {
            panic!("config with modules array should parse");
        };
        assert_eq!(
            job_names(&jobs),
            [
                "title",
                "separator",
                "os",
                "host",
                "break",
                "custom:hello {#red}world",
                "os",
                "colors",
            ]
        );

        assert!(
            super::preset_from_fastfetch_config_content("{\"other\": []}").is_none()
        );
    }

    #[test]
    fn custom_without_format_still_prints() {
        let config = r#"{"modules": [{"type": "custom"}]}"#;
        let Some(jobs) = super::preset_from_fastfetch_config_content(config) else {
            panic!("custom object should parse");
        };
        assert_eq!(job_names(&jobs), ["custom:"]);
    }

    #[test]
    fn command_job_carries_options() {
        let config = r#"{"modules": [{"type": "command", "text": "echo hi", "splitLines": true}]}"#;
        let Some(jobs) = super::preset_from_fastfetch_config_content(config) else {
            panic!("command object should parse");
        };
        match &jobs[0] {
            PresetJob::Command {
                text,
                shell,
                param,
                split_lines,
            } => {
                assert_eq!(text, "echo hi");
                assert_eq!(shell, "/bin/sh");
                assert_eq!(param.as_deref(), Some("-c"));
                assert!(split_lines);
            },
            _other => panic!("expected command job"),
        }
    }
}

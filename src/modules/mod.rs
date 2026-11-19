//! Module registry: every module is a name plus a function returning rows.

pub mod battery;
pub mod bios;
pub mod bluetooth;
pub mod bluetoothradio;
pub mod board;
pub mod bootmgr;
pub mod r#break;
pub mod brightness;
pub mod btrfs;
pub mod camera;
pub mod chassis;
pub mod codec;
pub mod colors;
pub mod command;
pub mod cpu;
pub mod cpucache;
pub mod cpuusage;
pub mod cursor;
pub mod custom;
pub mod datetime;
pub mod de;
pub mod disk;
pub mod diskio;
pub mod displayserver;
pub mod display;
pub mod dns;
pub mod editor;
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
pub mod logo;
pub mod media;
pub mod memory;
pub mod monitor;
pub mod mouse;
pub mod netio;
pub mod opencl;
pub mod opengl;
pub mod os;
pub mod packages;
pub mod physicaldisk;
pub mod physicalmemory;
pub mod player;
pub mod poweradapter;
pub mod processes;
pub mod publicip;
pub mod qt;
pub mod separator;
pub mod shell;
pub mod sound;
pub mod swap;
pub mod terminal;
pub mod terminalfont;
pub mod terminalsize;
pub mod terminaltheme;
pub mod theme;
pub mod title;
pub mod top;
pub mod tpm;
pub mod uptime;
pub mod users;
pub mod version;
pub mod vulkan;
pub mod wallpaper;
pub mod weather;
pub mod wifi;
pub mod wm;
pub mod wmtheme;
pub mod zpool;

use crate::{
    instance::Instance,
    json::JsonOutcome,
    util,
};

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

/// A module's JSON generator: modules/display/genJsonResult, returning
/// `result` on success or `error` with a message on detection failure.
/// `None` until the module's printJson is ported.
pub type JsonFn = fn(&Instance) -> JsonOutcome;

pub struct ModuleDef {
    pub name: &'static str,
    pub run: fn(&Instance) -> Vec<Row>,
    pub json: Option<JsonFn>,
}

/// All modules in fastfetch's ordering; `break` can't use the old macro
/// because its name is a Rust keyword.
pub const REGISTRY: &[ModuleDef] = &[
    ModuleDef {
        name: "title",
        run: title::run,
        json: Some(title::json),
    },
    ModuleDef {
        name: "separator",
        run: separator::run,
        json: None,
    },
    ModuleDef {
        name: "os",
        run: os::run,
        json: Some(os::json),
    },
    ModuleDef {
        name: "opencl",
        run: opencl::run,
        json: None,
    },
    ModuleDef {
        name: "opengl",
        run: opengl::run,
        json: None,
    },
    ModuleDef {
        name: "vulkan",
        run: vulkan::run,
        json: None,
    },
    ModuleDef {
        name: "host",
        run: host::run,
        json: Some(host::json),
    },
    ModuleDef {
        name: "bios",
        run: bios::run,
        json: None,
    },
    ModuleDef {
        name: "bootmgr",
        run: bootmgr::run,
        json: None,
    },
    ModuleDef {
        name: "bluetooth",
        run: bluetooth::run,
        json: None,
    },
    ModuleDef {
        name: "bluetoothradio",
        run: bluetoothradio::run,
        json: None,
    },
    ModuleDef {
        name: "board",
        run: board::run,
        json: None,
    },
    ModuleDef {
        name: "chassis",
        run: chassis::run,
        json: None,
    },
    ModuleDef {
        name: "codec",
        run: codec::run,
        json: None,
    },
    ModuleDef {
        name: "kernel",
        run: kernel::run,
        json: Some(kernel::json),
    },
    ModuleDef {
        name: "initsystem",
        run: initsystem::run,
        json: None,
    },
    ModuleDef {
        name: "uptime",
        run: uptime::run,
        json: Some(uptime::json),
    },
    ModuleDef {
        name: "users",
        run: users::run,
        json: None,
    },
    ModuleDef {
        name: "tpm",
        run: tpm::run,
        json: None,
    },
    ModuleDef {
        name: "version",
        run: version::run,
        json: None,
    },
    ModuleDef {
        name: "loadavg",
        run: loadavg::run,
        json: None,
    },
    ModuleDef {
        name: "keyboard",
        run: keyboard::run,
        json: None,
    },
    ModuleDef {
        name: "lm",
        run: lm::run,
        json: None,
    },
    ModuleDef {
        name: "processes",
        run: processes::run,
        json: None,
    },
    ModuleDef {
        name: "packages",
        run: packages::run,
        json: Some(packages::json),
    },
    ModuleDef {
        name: "shell",
        run: shell::run,
        json: Some(shell::json),
    },
    ModuleDef {
        name: "editor",
        run: editor::run,
        json: None,
    },
    ModuleDef {
        name: "display",
        run: display::run,
        json: None,
    },
    ModuleDef {
        name: "brightness",
        run: brightness::run,
        json: None,
    },
    ModuleDef {
        name: "de",
        run: de::run,
        json: Some(de::json),
    },
    ModuleDef {
        name: "wm",
        run: wm::run,
        json: Some(wm::json),
    },
    ModuleDef {
        name: "wmtheme",
        run: wmtheme::run,
        json: Some(wmtheme::json),
    },
    ModuleDef {
        name: "theme",
        run: theme::run,
        json: None,
    },
    ModuleDef {
        name: "icons",
        run: icons::run,
        json: None,
    },
    ModuleDef {
        name: "font",
        run: font::run,
        json: Some(font::json),
    },
    ModuleDef {
        name: "cursor",
        run: cursor::run,
        json: Some(cursor::json),
    },
    ModuleDef {
        name: "terminal",
        run: terminal::run,
        json: Some(terminal::json),
    },
    ModuleDef {
        name: "terminalfont",
        run: terminalfont::run,
        json: Some(terminalfont::json),
    },
    ModuleDef {
        name: "terminalsize",
        run: terminalsize::run,
        json: None,
    },
    ModuleDef {
        name: "cpu",
        run: cpu::run,
        json: Some(cpu::json),
    },
    ModuleDef {
        name: "cpucache",
        run: cpucache::run,
        json: None,
    },
    ModuleDef {
        name: "cpuusage",
        run: cpuusage::run,
        json: None,
    },
    ModuleDef {
        name: "gpu",
        run: gpu::run,
        json: None,
    },
    ModuleDef {
        name: "gamepad",
        run: gamepad::run,
        json: None,
    },
    ModuleDef {
        name: "mouse",
        run: mouse::run,
        json: None,
    },
    ModuleDef {
        name: "memory",
        run: memory::run,
        json: Some(memory::json),
    },
    ModuleDef {
        name: "monitor",
        run: monitor::run,
        json: None,
    },
    ModuleDef {
        name: "terminaltheme",
        run: terminaltheme::run,
        json: None,
    },
    ModuleDef {
        name: "top",
        run: top::run,
        json: None,
    },
    ModuleDef {
        name: "swap",
        run: swap::run,
        json: Some(swap::json),
    },
    ModuleDef {
        name: "disk",
        run: disk::run,
        json: Some(disk::json),
    },
    ModuleDef {
        name: "btrfs",
        run: btrfs::run,
        json: None,
    },
    ModuleDef {
        name: "physicaldisk",
        run: physicaldisk::run,
        json: None,
    },
    ModuleDef {
        name: "physicalmemory",
        run: physicalmemory::run,
        json: None,
    },
    ModuleDef {
        name: "netio",
        run: netio::run,
        json: None,
    },
    ModuleDef {
        name: "diskio",
        run: diskio::run,
        json: None,
    },
    ModuleDef {
        name: "publicip",
        run: publicip::run,
        json: None,
    },
    ModuleDef {
        name: "localip",
        run: localip::run,
        json: None,
    },
    ModuleDef {
        name: "dns",
        run: dns::run,
        json: None,
    },
    ModuleDef {
        name: "wifi",
        run: wifi::run,
        json: None,
    },
    ModuleDef {
        name: "sound",
        run: sound::run,
        json: None,
    },
    ModuleDef {
        name: "zpool",
        run: zpool::run,
        json: None,
    },
    ModuleDef {
        name: "datetime",
        run: datetime::run,
        json: None,
    },
    ModuleDef {
        name: "locale",
        run: locale::run,
        json: None,
    },
    ModuleDef {
        name: "battery",
        run: battery::run,
        json: Some(battery::json),
    },
    ModuleDef {
        name: "poweradapter",
        run: poweradapter::run,
        json: Some(poweradapter::json),
    },
    ModuleDef {
        name: "camera",
        run: camera::run,
        json: None,
    },
    ModuleDef {
        name: "wallpaper",
        run: wallpaper::run,
        json: None,
    },
    ModuleDef {
        name: "media",
        run: media::run,
        json: None,
    },
    ModuleDef {
        name: "player",
        run: player::run,
        json: None,
    },
    ModuleDef {
        name: "weather",
        run: weather::run,
        json: None,
    },
    ModuleDef {
        name: "break",
        run: r#break::run,
        json: None,
    },
    ModuleDef {
        name: "command",
        run: command::run,
        json: None,
    },
    ModuleDef {
        name: "custom",
        run: custom::run,
        json: None,
    },
    ModuleDef {
        name: "colors",
        run: colors::run,
        json: None,
    },
    ModuleDef {
        name: "logo",
        run: logo::run,
        json: None,
    },
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
    /// A structure/config token that selects no module. Text mode hides it
    /// (the hidden "<no implementation provided>" error row); JSON mode
    /// emits {"type": <token as written>, "error": "Unknown module type"}.
    Unknown(String),
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
                jobs.push(module_job(&name).unwrap_or(PresetJob::Unknown(name)));
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
                } else {
                    jobs.push(module_job(&type_).unwrap_or(PresetJob::Unknown(type_)));
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

/// The `--list-modules` table: module names and descriptions are the
/// upstream ffModuleInfos data (MIT), in upstream registration order.
const MODULE_LIST: &[(&str, &str)] = &[
    ("Battery", "Print battery information"),
    ("BIOS", "Print first-stage bootloader information (name, version, release date, etc.)"),
    ("Bluetooth", "List connected Bluetooth devices"),
    ("BluetoothRadio", "List Bluetooth radios (supported versions, vendors, etc.)"),
    ("Board", "Print motherboard name and other information"),
    ("Bootmgr", "Print second-stage bootloader information (name, firmware, etc.)"),
    ("Break", "Print an empty line"),
    ("Brightness", "Print the current brightness level of your monitors"),
    ("Btrfs", "Print Linux BTRFS volumes"),
    ("Camera", "Print available cameras"),
    ("Chassis", "Print chassis type information (desktop, laptop, etc.)"),
    ("Codec", "Print hardware video acceleration codec types (decode / encode)"),
    ("Command", "Run custom shell scripts"),
    ("Colors", "Display the terminal's 16-color palette"),
    ("CPU", "Print CPU name, frequency, etc."),
    ("CPUCache", "Print CPU cache sizes"),
    ("CPUUsage", "Print CPU usage. Collecting data takes some time"),
    ("Cursor", "Print cursor style name"),
    ("Custom", "Print a custom string, with or without key"),
    ("DateTime", "Print the current date and time"),
    ("DE", "Print desktop environment name"),
    ("Display", "Print resolutions, refresh rates, etc"),
    ("Disk", "Print partitions, space usage, file system, etc"),
    ("DiskIO", "Print physical disk I/O throughput"),
    ("DNS", "Print configured DNS servers"),
    ("Editor", "Print information about the default editor ($VISUAL or $EDITOR)"),
    ("Font", "Print system font names"),
    ("Gamepad", "List connected gamepads"),
    ("GPU", "Print GPU names, memory sizes, types, etc"),
    ("Host", "Print your computer's product name"),
    ("Icons", "Print icon style name"),
    ("InitSystem", "Print init system (pid 1) name and version"),
    ("Kernel", "Print system kernel version"),
    ("Keyboard", "List connected keyboards"),
    ("LM", "Print login manager (desktop manager) name and version"),
    ("Loadavg", "Print system load averages"),
    ("Locale", "Print system locale name"),
    ("LocalIp", "List local IP addresses (IPv4 or IPv6), MAC addresses, etc"),
    ("Logo", "Query built-in logo for JSON output"),
    ("Media", "Print the name of the currently playing song"),
    ("Memory", "Print system memory usage information"),
    ("Monitor", "Same as Display module, but with a different default output format"),
    ("Mouse", "List connected mice"),
    ("NetIO", "Print network I/O throughput"),
    ("OpenCL", "Print the highest OpenCL version supported by the GPU"),
    ("OpenGL", "Print the highest OpenGL version supported by the GPU"),
    ("OS", "Print the OS or Linux distribution name and version"),
    ("Packages", "List installed package managers and count of installed packages"),
    ("PhysicalDisk", "Print physical disk information"),
    ("PhysicalMemory", "Print system physical memory devices"),
    ("Player", "Print the music player name that is currently active"),
    ("PowerAdapter", "Print power adapter name and charging watts"),
    ("Processes", "Print number of running processes and threads"),
    ("PublicIp", "Print your public IP address and related information"),
    ("Separator", "Print a separator line"),
    ("Shell", "Print the current shell name and version"),
    ("Sound", "Print sound devices, volume levels, etc"),
    ("Swap", "Print swap (paging file) space usage"),
    ("Terminal", "Print the current terminal name and version"),
    ("TerminalFont", "Print the font name and size used by the current terminal"),
    ("TerminalSize", "Print the current terminal size"),
    ("TerminalTheme", "Print the current terminal theme (foreground and background colors)"),
    ("Title", "Print the title, including your username and hostname"),
    ("Top", "Print processes with the highest CPU, memory or disk I/O usage"),
    ("Theme", "Print the current desktop environment theme"),
    ("TPM", "Print information about the Trusted Platform Module (TPM) security device"),
    ("Uptime", "Print how long the system has been running"),
    ("Users", "Print users who are currently logged in"),
    ("Version", "Print the Fastfetch version and build information"),
    ("Vulkan", "Print the highest Vulkan version supported by the GPU"),
    ("Wallpaper", "Print the file path of the current wallpaper"),
    ("Weather", "Print weather information"),
    ("WM", "Print the window manager name and version"),
    ("Wifi", "Print connected Wi-Fi info (SSID, connection and security protocol)"),
    ("WMTheme", "Print the current window manager theme"),
    ("Zpool", "Print ZFS storage pools"),
];

pub fn list_modules() {
    // printf("%d)%s%-14s: %s\n", count, count > 9 ? " " : "  ", name, description)
    for (index, (name, description)) in MODULE_LIST.iter().enumerate() {
        let gap = if index >= 9 { " " } else { "  " };
        println!("{}){gap}{name:<14}: {description}", index + 1);
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

/// The module list both output formats dispatch on: the fastfetch config's
/// modules when present, the -s structure when given, else the default
/// preset. Unknown tokens become Unknown jobs (hidden rows in text mode,
/// "Unknown module type" items in JSON mode); empty -s tokens are unknown
/// tokens too, like fastfetch's getdelim walk.
fn build_jobs(selected: &[String], structure_given: bool) -> Vec<PresetJob> {
    let mut jobs: Vec<PresetJob> = if !structure_given && selected.is_empty() {
        preset_from_fastfetch_config().unwrap_or_else(|| {
            DEFAULT_PRESET.iter().filter_map(|name| module_job(name)).collect()
        })
    } else {
        // Module names match case-insensitively (ffStrEqualsIgnCase); anything
        // else keeps the token verbatim for the JSON path.
        selected
            .iter()
            .map(|name| module_job(name).unwrap_or(PresetJob::Unknown(name.clone())))
            .collect()
    };
    // The structureDisabled check runs before parsing each token, and
    // ffStrbufSeparatedContainIgnCaseNS answers "an empty list contains
    // only the empty segment" — with no --structure-disabled, that check
    // passes exactly for empty tokens, which are therefore never parsed.
    jobs.retain(|job| !matches!(job, PresetJob::Unknown(token) if token.is_empty()));
    jobs
}

/// The canonical `type` string of a JSON item ("cpu" → "CPU"), from the
/// same ffModuleInfos table --list-modules uses.
fn canonical_type(name: &str) -> &'static str {
    MODULE_LIST
        .iter()
        .find(|(canonical, _)| canonical.eq_ignore_ascii_case(name))
        .map(|(canonical, _)| *canonical)
        .unwrap_or("")
}

/// One `{type, result|error}` entry of --format json output.
pub struct JsonItem {
    pub type_name: String,
    pub outcome: JsonOutcome,
}

fn prepare_rate_modules(jobs: &[PresetJob]) {
    let wants = |name: &str| {
        jobs.iter()
            .any(|job| matches!(job, PresetJob::Module(module) if module.name == name))
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
}

/// Runs the selected modules (or the default preset when no structure was
/// given) in structure order.
///
/// Modules are independent of each other, so each one runs in its own
/// scoped thread: the slow data sources (gdbus, gsettings, process scans)
/// overlap instead of adding up. Rows are emitted in structure order.
pub fn collect(selected: &[String], structure_given: bool, instance: &Instance) -> Vec<Row> {
    let jobs = build_jobs(selected, structure_given);
    prepare_rate_modules(&jobs);

    let results: Vec<Vec<Row>> = std::thread::scope(|scope| {
        // Structure order wins: `-s b:os:a` prints rows in exactly that
        // order, like fastfetch's structure loop. Duplicates allowed.
        // Unknown tokens are the hidden "<no implementation provided>"
        // error rows and never print.
        jobs.iter()
            .map(|job| match job {
                PresetJob::Module(module) => scope.spawn(move || (module.run)(instance)),
                // The hidden "<no implementation provided>" error row.
                PresetJob::Unknown(_) => scope.spawn(std::vec::Vec::new),
                PresetJob::Custom(text) => scope.spawn(move || custom::render_rows(text, instance)),
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

/// --format json: one {type, result|error} item per structure entry, in
/// structure order. Unknown tokens print their verbatim spelling.
pub fn collect_json(selected: &[String], structure_given: bool, instance: &Instance) -> Vec<JsonItem> {
    let jobs = build_jobs(selected, structure_given);
    prepare_rate_modules(&jobs);

    std::thread::scope(|scope| {
        jobs.iter()
            .map(|job| match job {
                PresetJob::Module(module) => scope.spawn(move || JsonItem {
                    type_name: canonical_type(module.name).to_string(),
                    // A module without a ported generator still prints the
                    // fixed upstream error for generator-less modules.
                    outcome: module.json.map_or(JsonOutcome::Unsupported, |json| (json)(instance)),
                }),
                PresetJob::Unknown(token) => scope.spawn(move || JsonItem {
                    type_name: token.clone(),
                    outcome: JsonOutcome::Error("Unknown module type".to_string()),
                }),
                PresetJob::Custom(text) => {
                    scope.spawn(move || JsonItem {
                        type_name: "Custom".to_string(),
                        outcome: custom::json(text, instance),
                    })
                },
                PresetJob::Command {
                    text,
                    shell,
                    param,
                    split_lines,
                } => scope.spawn(move || JsonItem {
                    type_name: "Command".to_string(),
                    outcome: command::json(text, shell, param.as_deref(), *split_lines, instance),
                }),
            })
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .collect()
    })
}

#[cfg(test)]
mod preset_tests {
    use super::{module_job, PresetJob, REGISTRY, MODULE_LIST};

    fn job_names(jobs: &[PresetJob]) -> Vec<String> {
        jobs.iter()
            .map(|job| match job {
                PresetJob::Module(module) => module.name.to_string(),
                PresetJob::Unknown(token) => format!("unknown:{token}"),
                PresetJob::Custom(text) => format!("custom:{text}"),
                PresetJob::Command { text, .. } => format!("command:{text}"),
            })
            .collect()
    }

    #[test]
    fn module_list_covers_the_registry() {
        // --list-modules and the registry must stay in lockstep: every
        // listed name selects a module, and no module goes unlisted.
        assert_eq!(MODULE_LIST.len(), REGISTRY.len());
        let mut listed: Vec<String> =
            MODULE_LIST.iter().map(|(name, _)| name.to_ascii_lowercase()).collect();
        let mut registered: Vec<&str> = REGISTRY.iter().map(|module| module.name).collect();
        listed.sort_unstable();
        registered.sort_unstable();
        assert_eq!(listed, registered);
        for (name, _) in MODULE_LIST {
            assert!(module_job(name).is_some(), "{name} does not select a module");
        }
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

        assert!(super::preset_from_fastfetch_config_content("{\"other\": []}").is_none());
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

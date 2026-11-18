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
pub mod colors;
pub mod cpu;
pub mod cpucache;
pub mod cpuusage;
pub mod cursor;
pub mod datetime;
pub mod de;
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
fn preset_from_fastfetch_config() -> Option<Vec<&'static str>> {
    let mut config = std::path::PathBuf::from(std::env::var_os("HOME")?);
    config.push(".config/fastfetch/config.jsonc");
    preset_from_fastfetch_config_content(&util::read_file(config.to_str()?)?)
}

/// Parses the "modules" array out of a fastfetch config's JSONC content.
fn preset_from_fastfetch_config_content(content: &str) -> Option<Vec<&'static str>> {
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

    let names: Vec<&'static str> = array
        .split(',')
        .filter_map(|item| {
            let name = item.trim().trim_matches('"');
            REGISTRY
                .iter()
                .find(|module| module.name == name)
                .map(|module| module.name)
        })
        .collect();
    (!names.is_empty()).then_some(names)
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

/// Runs the selected modules (or the default preset) in registry order.
///
/// Modules are independent of each other, so each one runs in its own
/// scoped thread: the slow data sources (gdbus, gsettings, process scans)
/// overlap instead of adding up. Rows are still emitted in registry order.
pub fn collect(selected: &[String], instance: &Instance) -> Vec<Row> {
    let names: Vec<&str> = if selected.is_empty() {
        preset_from_fastfetch_config().unwrap_or_else(|| DEFAULT_PRESET.to_vec())
    } else {
        // Unknown names are skipped like fastfetch's structure parser: the
        // token becomes a hidden error row and the rest still runs.
        selected
            .iter()
            .filter(|name| REGISTRY.iter().any(|module| module.name == name.as_str()))
            .map(String::as_str)
            .collect()
    };

    // The rate modules sample counters twice; start the clock before the
    // parallel detection runs, like fastfetch's ffPrepare* during option
    // parsing.
    if names.contains(&"top") {
        top::prepare();
    }
    if names.contains(&"cpuusage") {
        cpuusage::prepare();
    }
    if names.contains(&"netio") {
        netio::prepare();
    }
    if names.contains(&"diskio") {
        diskio::prepare();
    }

    let results: Vec<Vec<Row>> = std::thread::scope(|scope| {
        REGISTRY
            .iter()
            .filter(|module| names.contains(&module.name))
            .map(|module| scope.spawn(move || (module.run)(instance)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().unwrap_or_default())
            .collect()
    });
    results.into_iter().flatten().collect()
}

#[cfg(test)]
mod preset_tests {
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
        "colors",
    ],
}
"#;
        let Some(names) = super::preset_from_fastfetch_config_content(config) else {
            panic!("config with modules array should parse");
        };
        assert_eq!(names, ["title", "separator", "os", "host", "break", "colors"]);

        assert_eq!(
            super::preset_from_fastfetch_config_content("{\"other\": []}"),
            None
        );
    }
}

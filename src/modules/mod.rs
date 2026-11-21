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
};

/// How a row should be rendered.
#[derive(Clone, Copy)]
pub enum Kind {
    /// Styled by the renderer: `key: value` or accent-colored title.
    Value,
    /// ffPrintError's row: the standard key, the message wrapped in the
    /// bold-red error modifier outside pipe mode.
    Error,
    /// Printed verbatim; the module owns any escape sequences.
    Raw,
    /// A placeholder whose detection (and its /dev/tty queries, like
    /// upstream's print-time ffDetect calls) runs when the renderer
    /// reaches the row's position, expanding into the real rows — or
    /// none, when the error row is hidden. The module name rides along
    /// because the entry's common args stamp onto the expanded rows.
    Late(fn(&Instance) -> Vec<Row>, &'static str),
}

pub struct Row {
    pub key: Option<String>,
    pub value: String,
    pub kind: Kind,
    /// ffOptionInitModuleArg's default keyIcon for the module, stamped by
    /// collect(); empty when the module has none. Printed when
    /// display.keyType carries the ICON bit.
    pub key_icon: &'static str,
    /// ffPrintLogoAndKey's {index} format argument: the module's own row
    /// counter (0 when it doesn't number rows, 1-based where it does).
    pub key_index: u32,
    /// The common module args the entry set (ffJsonConfigParseModuleArgs),
    /// stamped by collect(); None when the entry set none or the module
    /// lacks the generic parse (logo, separator, break).
    pub common: Option<CommonArgs>,
    /// The custom key expanded for this row (the {index}/{icon}/
    /// {module-name} form); set only on rows that take a custom key.
    /// Rows without one keep their built-in key text.
    pub key_override: Option<String>,
}

impl Row {
    /// Regular `key: value` line.
    pub fn new(key: &str, value: impl Into<String>) -> Self {
        Self {
            key: Some(key.to_string()),
            value: value.into(),
            kind: Kind::Value,
            key_icon: "",
            key_index: 0,
            common: None,
            key_override: None,
        }
    }

    /// ffPrintError's row: the standard key writer plus the bold-red
    /// wrapped message.
    pub fn error(key: &str, value: impl Into<String>) -> Self {
        Self {
            key: Some(key.to_string()),
            value: value.into(),
            kind: Kind::Error,
            key_icon: "",
            key_index: 0,
            common: None,
            key_override: None,
        }
    }

    /// Keyless line styled by the renderer (e.g. the title).
    pub fn bare(value: impl Into<String>) -> Self {
        Self {
            key: None,
            value: value.into(),
            kind: Kind::Value,
            key_icon: "",
            key_index: 0,
            common: None,
            key_override: None,
        }
    }

    /// Keyless line printed as-is (separator, color blocks).
    pub fn raw(value: impl Into<String>) -> Self {
        Self {
            key: None,
            value: value.into(),
            kind: Kind::Raw,
            key_icon: "",
            key_index: 0,
            common: None,
            key_override: None,
        }
    }

    /// A detection deferred to print time, like fastfetch's printModule
    /// entry points that detect as they print.
    pub fn late(late: fn(&Instance) -> Vec<Row>, module: &'static str) -> Self {
        Self {
            key: None,
            value: String::new(),
            kind: Kind::Late(late, module),
            key_icon: "",
            key_index: 0,
            common: None,
            key_override: None,
        }
    }

    /// ffOptionInitModuleArg's default keyIcon: every keyed row the module
    /// produced prints its icon when display.keyType carries the ICON bit.
    /// Keyless rows stand in for the magic `" "` key and print none, like
    /// upstream's hidden key block.
    pub fn stamped(mut self, icon: &'static str) -> Self {
        if self.key.is_some() {
            self.key_icon = icon;
        }
        self
    }

    /// The module's row counter for the {index} format argument of a
    /// custom key; 0-based where upstream passes a 0 moduleIndex, 1-based
    /// where it pre-increments.
    pub fn with_index(mut self, index: u32) -> Self {
        self.key_index = index;
        self
    }
}

/// The common module args of one entry (ffJsonConfigParseModuleArgs's
/// FFModuleArgs subset the renderer consumes; the `format` outputFormat is
/// the separate format-engine work). Colors hold ffOptionParseColor's SGR
/// parameters.
#[derive(Clone, Default)]
pub(crate) struct CommonArgs {
    /// The raw `key` string: the magic `" "` hides the whole key block,
    /// the empty string falls back to the module name.
    pub key: Option<String>,
    pub key_color: Option<String>,
    pub key_width: u32,
    pub key_icon: Option<String>,
    pub output_color: Option<String>,
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
        json: Some(opencl::json),
    },
    ModuleDef {
        name: "opengl",
        run: opengl::run,
        json: Some(opengl::json),
    },
    ModuleDef {
        name: "vulkan",
        run: vulkan::run,
        json: Some(vulkan::json),
    },
    ModuleDef {
        name: "host",
        run: host::run,
        json: Some(host::json),
    },
    ModuleDef {
        name: "bios",
        run: bios::run,
        json: Some(bios::json),
    },
    ModuleDef {
        name: "bootmgr",
        run: bootmgr::run,
        json: Some(bootmgr::json),
    },
    ModuleDef {
        name: "bluetooth",
        run: bluetooth::run,
        json: Some(bluetooth::json),
    },
    ModuleDef {
        name: "bluetoothradio",
        run: bluetoothradio::run,
        json: Some(bluetoothradio::json),
    },
    ModuleDef {
        name: "board",
        run: board::run,
        json: Some(board::json),
    },
    ModuleDef {
        name: "chassis",
        run: chassis::run,
        json: Some(chassis::json),
    },
    ModuleDef {
        name: "codec",
        run: codec::run,
        json: Some(codec::json),
    },
    ModuleDef {
        name: "kernel",
        run: kernel::run,
        json: Some(kernel::json),
    },
    ModuleDef {
        name: "initsystem",
        run: initsystem::run,
        json: Some(initsystem::json),
    },
    ModuleDef {
        name: "uptime",
        run: uptime::run,
        json: Some(uptime::json),
    },
    ModuleDef {
        name: "users",
        run: users::run,
        json: Some(users::json),
    },
    ModuleDef {
        name: "tpm",
        run: tpm::run,
        json: Some(tpm::json),
    },
    ModuleDef {
        name: "version",
        run: version::run,
        json: Some(version::json),
    },
    ModuleDef {
        name: "loadavg",
        run: loadavg::run,
        json: Some(loadavg::json),
    },
    ModuleDef {
        name: "keyboard",
        run: keyboard::run,
        json: Some(keyboard::json),
    },
    ModuleDef {
        name: "lm",
        run: lm::run,
        json: Some(lm::json),
    },
    ModuleDef {
        name: "processes",
        run: processes::run,
        json: Some(processes::json),
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
        json: Some(editor::json),
    },
    ModuleDef {
        name: "display",
        run: display::run,
        json: Some(display::json),
    },
    ModuleDef {
        name: "brightness",
        run: brightness::run,
        json: Some(brightness::json),
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
        json: Some(theme::json),
    },
    ModuleDef {
        name: "icons",
        run: icons::run,
        json: Some(icons::json),
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
        json: Some(terminalsize::json),
    },
    ModuleDef {
        name: "cpu",
        run: cpu::run,
        json: Some(cpu::json),
    },
    ModuleDef {
        name: "cpucache",
        run: cpucache::run,
        json: Some(cpucache::json),
    },
    ModuleDef {
        name: "cpuusage",
        run: cpuusage::run,
        json: Some(cpuusage::json),
    },
    ModuleDef {
        name: "gpu",
        run: gpu::run,
        json: Some(gpu::json),
    },
    ModuleDef {
        name: "gamepad",
        run: gamepad::run,
        json: Some(gamepad::json),
    },
    ModuleDef {
        name: "mouse",
        run: mouse::run,
        json: Some(mouse::json),
    },
    ModuleDef {
        name: "memory",
        run: memory::run,
        json: Some(memory::json),
    },
    ModuleDef {
        name: "monitor",
        run: monitor::run,
        json: Some(monitor::json),
    },
    ModuleDef {
        name: "terminaltheme",
        run: terminaltheme::run,
        json: Some(terminaltheme::json),
    },
    ModuleDef {
        name: "top",
        run: top::run,
        json: Some(top::json),
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
        json: Some(btrfs::json),
    },
    ModuleDef {
        name: "physicaldisk",
        run: physicaldisk::run,
        json: Some(physicaldisk::json),
    },
    ModuleDef {
        name: "physicalmemory",
        run: physicalmemory::run,
        json: Some(physicalmemory::json),
    },
    ModuleDef {
        name: "netio",
        run: netio::run,
        json: Some(netio::json),
    },
    ModuleDef {
        name: "diskio",
        run: diskio::run,
        json: Some(diskio::json),
    },
    ModuleDef {
        name: "publicip",
        run: publicip::run,
        json: Some(publicip::json),
    },
    ModuleDef {
        name: "localip",
        run: localip::run,
        json: Some(localip::json),
    },
    ModuleDef {
        name: "dns",
        run: dns::run,
        json: Some(dns::json),
    },
    ModuleDef {
        name: "wifi",
        run: wifi::run,
        json: Some(wifi::json),
    },
    ModuleDef {
        name: "sound",
        run: sound::run,
        json: Some(sound::json),
    },
    ModuleDef {
        name: "zpool",
        run: zpool::run,
        json: Some(zpool::json),
    },
    ModuleDef {
        name: "datetime",
        run: datetime::run,
        json: Some(datetime::json),
    },
    ModuleDef {
        name: "locale",
        run: locale::run,
        json: Some(locale::json),
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
        json: Some(camera::json),
    },
    ModuleDef {
        name: "wallpaper",
        run: wallpaper::run,
        json: Some(wallpaper::json),
    },
    ModuleDef {
        name: "media",
        run: media::run,
        json: Some(media::json),
    },
    ModuleDef {
        name: "player",
        run: player::run,
        json: Some(player::json),
    },
    ModuleDef {
        name: "weather",
        run: weather::run,
        json: Some(weather::json),
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
        json: Some(logo::json),
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
    ModuleOpts {
        def: &'static ModuleDef,
        opts: &'static crate::jsonc::Value,
    },
    /// A structure/config token that selects no module. Text mode prints
    /// ffPrintError's showErrors-gated "<no implementation provided>" row
    /// under the verbatim token; JSON mode emits
    /// {"type": <token as written>, "error": "Unknown module type"}.
    Unknown(String),
    Custom {
        format: String,
        opts: Option<&'static crate::jsonc::Value>,
    },
    Command {
        text: String,
        shell: String,
        param: Option<String>,
        split_lines: bool,
        opts: Option<&'static crate::jsonc::Value>,
    },
}

fn module_job(name: &str) -> Option<PresetJob> {
    REGISTRY
        .iter()
        .find(|module| module.name.eq_ignore_ascii_case(name))
        .map(PresetJob::Module)
}

/// Shared empty options object for object entries that only carry "type".
static EMPTY_OBJECT: crate::jsonc::Value = crate::jsonc::Value::Object(Vec::new());

/// Maps one config modules-array entry onto a job. Object entries with
/// type `custom`/`command` decode their text options here; every other
/// object entry keeps its options for the module to read off the
/// per-job instance.
fn entry_job(entry: &crate::config::Entry) -> PresetJob {
    match entry {
        crate::config::Entry::Simple(name) => {
            module_job(name).unwrap_or(PresetJob::Unknown(name.clone()))
        },
        crate::config::Entry::Object { type_, opts } => {
            let opts_value = (*opts).unwrap_or(&EMPTY_OBJECT);
            match type_.to_ascii_lowercase().as_str() {
                "custom" => {
                    // A custom without a format still prints its empty row.
                    let format = opts_value
                        .get("format")
                        .and_then(crate::jsonc::Value::as_str)
                        .unwrap_or_default();
                    PresetJob::Custom {
                        format: format.to_string(),
                        opts: *opts,
                    }
                },
                "command" => {
                    // fastfetch's defaults: /bin/sh -c, splitLines off.
                    PresetJob::Command {
                        text: opts_value
                            .get("text")
                            .and_then(crate::jsonc::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        shell: opts_value
                            .get("shell")
                            .and_then(crate::jsonc::Value::as_str)
                            .unwrap_or("/bin/sh")
                            .to_string(),
                        param: Some(
                            opts_value
                                .get("param")
                                .and_then(crate::jsonc::Value::as_str)
                                .unwrap_or("-c")
                                .to_string(),
                        ),
                        split_lines: opts_value
                            .get("splitLines")
                            .and_then(crate::jsonc::Value::as_bool)
                            .unwrap_or(false),
                        opts: *opts,
                    }
                },
                _ => match module_job(type_) {
                    Some(PresetJob::Module(def)) => PresetJob::ModuleOpts { def, opts: opts_value },
                    Some(other) => other,
                    None => PresetJob::Unknown(type_.clone()),
                },
            }
        },
    }
}

/// The jobs for a loaded config document: the modules array takes over
/// the structure, an absent array prints nothing, and a walk error
/// truncates the list at the entry that failed.
fn preset_jobs_from_config(config: &crate::config::Config) -> Vec<PresetJob> {
    config.entries.iter().map(entry_job).collect()
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

/// FASTFETCH_DATATEXT_STRUCTURE: the default preset as one colon string.
pub const DEFAULT_STRUCTURE_TEXT: &str = "Title:Separator:OS:Host:Kernel:Uptime:Packages:Shell:Display:DE:WM:WMTheme:Theme:Icons:Font:Cursor:Terminal:TerminalFont:CPU:GPU:Memory:Swap:Disk:LocalIp:Battery:PowerAdapter:Locale:Break:Colors";

/// listModules: the pretty numbered listing, or the shell-completion
/// "Name:description" form for `--list-modules autocompletion`.
pub fn list_modules(pretty: bool) {
    if !pretty {
        for (name, description) in MODULE_LIST {
            println!("{name}:{description}");
        }
        return;
    }
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
/// ffTempsAppendNum with the display defaults: 1 digit, no space before
/// °C, threshold colors (dark-terminal light-yellow/light-red variants).
pub(crate) fn temps_num(celsius: f64, green: u8, yellow: u8, color: bool) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    if color {
        let (yellow_code, red_code) = if terminal_light() { ("33", "31") } else { ("93", "91") };
        let code = if green <= yellow {
            if celsius > yellow as f64 {
                red_code
            } else if celsius > green as f64 {
                yellow_code
            } else {
                "32"
            }
        } else if celsius < yellow as f64 {
            red_code
        } else if celsius < green as f64 {
            yellow_code
        } else {
            "32"
        };
        let _ = write!(out, "\x1b[{code}m");
    }
    let _ = write!(out, "{celsius:.1}°C");
    if color {
        out.push_str("\x1b[m");
    }
    out
}

/// FFColorRangeConfig: the green/yellow temperature thresholds. Every
/// temps module defaults to {60, 80}; physicaldisk overrides to {50, 70}.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TempRange {
    pub green: u8,
    pub yellow: u8,
}

impl Default for TempRange {
    fn default() -> Self {
        Self { green: 60, yellow: 80 }
    }
}

/// ffTempsParseJsonObject: the shared `temp` option value — a boolean
/// toggles the temperature read, null disables it, and an object sets the
/// green/yellow thresholds (integers 0-100; any other shape exits 480,
/// like upstream's usage errors).
pub(crate) fn parse_temp(value: &crate::jsonc::Value, range: &mut TempRange) -> bool {
    if let Some(enabled) = value.as_bool() {
        return enabled;
    }
    if matches!(value, crate::jsonc::Value::Null) {
        return false;
    }
    let crate::jsonc::Value::Object(entries) = value else {
        eprintln!("Error: usage: temp must be an object or a boolean");
        std::process::exit(480);
    };

    // yyjson_is_int: sint/uint only, a real is a usage error.
    let int_val = |value: &crate::jsonc::Value| match value {
        crate::jsonc::Value::Int(int) => Some(*int),
        crate::jsonc::Value::UInt(uint) => Some(*uint as i64),
        _ => None,
    };
    for (key, field) in [("green", &mut range.green), ("yellow", &mut range.yellow)] {
        let Some(raw) = entries.iter().rev().find(|(name, _)| name == key).map(|(_, v)| v) else {
            continue;
        };
        let Some(num) = int_val(raw) else {
            eprintln!("Error: usage: temp.{key} must be an integer between 0 and 100");
            std::process::exit(480);
        };
        if !(0..=100).contains(&num) {
            eprintln!("Error: usage: temp.{key} must be between 0 and 100");
            std::process::exit(480);
        }
        *field = num as u8;
    }
    true
}

/// terminalLightTheme: the env-only terminal theme detection's background
/// darkness (upstream forces the env path at display init for speed).
pub(crate) fn terminal_light() -> bool {
    use std::sync::OnceLock;
    static LIGHT: OnceLock<bool> = OnceLock::new();
    *LIGHT.get_or_init(crate::modules::terminaltheme::terminal_is_light_by_env)
}

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
fn build_jobs(selected: &[String], structure_given: bool, config: Option<&crate::config::Config>) -> Vec<PresetJob> {
    let mut jobs: Vec<PresetJob> = if structure_given {
        // Module names match case-insensitively (ffStrEqualsIgnCase); anything
        // else keeps the token verbatim for the JSON path. A `-s` structure
        // ignores the config's modules entirely, like upstream's
        // `structure.length == 0 && configDoc` gate.
        selected
            .iter()
            .map(|name| module_job(name).unwrap_or(PresetJob::Unknown(name.clone())))
            .collect()
    } else if let Some(config) = config.filter(|config| config.document_loaded) {
        preset_jobs_from_config(config)
    } else {
        DEFAULT_PRESET.iter().filter_map(|name| module_job(name)).collect()
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

/// One `{type, result|error}` entry of --format json output. `stat` is
/// the per-module timing appended as the third key when display.stat is
/// on (yyjson_mut_obj_add_real on the array's last module).
pub struct JsonItem {
    pub type_name: String,
    pub outcome: JsonOutcome,
    pub stat: Option<f64>,
}

fn prepare_rate_modules(jobs: &[PresetJob]) {
    let wants = |name: &str| {
        jobs.iter().any(|job| {
            matches!(
                job,
                PresetJob::Module(module) if module.name == name
            ) || matches!(
                job,
                PresetJob::ModuleOpts { def, .. } if def.name == name
            )
        })
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

    // The network modules send their requests here too, once per module
    // instance in config order (`-s` jobs carry default options): a second
    // publicip or weather instance is an upstream hard error (exit 1).
    for job in jobs {
        let (name, entry) = match job {
            PresetJob::Module(module) => (module.name, None),
            PresetJob::ModuleOpts { def, opts } => (def.name, Some(*opts)),
            _ => continue,
        };
        match name {
            "publicip" => publicip::prepare_from_entry(entry),
            "weather" => weather::prepare_from_entry(entry),
            _ => {},
        }
    }
}

/// Runs the selected modules (or the default preset when no structure was
/// given) in structure order.
///
/// Like upstream, jobs run one after another on the calling thread: the
/// dlopen-heavy backends (NVML, VA-API, NSS) and glibc's loader/exit locks
/// deadlock when initialized concurrently, and upstream's watchdog-thread
/// network timeouts assume a single-threaded walk. Rows are emitted in
/// structure order.
/// Stamps the module's default key icon onto the rows it produced, like
/// ffOptionInitModuleArg filling moduleArgs.keyIcon before ffPrintLogoAndKey.
fn stamp_icons(rows: &mut [Row], name: &str) {
    let icon = crate::module_icons::icon_for(name);
    if icon.is_empty() {
        return;
    }
    for row in rows.iter_mut() {
        if row.key.is_some() {
            row.key_icon = icon;
        }
    }
}

/// ffJsonConfigParseModuleArgs: the common args one module entry carries.
/// `key`/`keyIcon` are ffStrbufSetJsonVal (strings set, anything else
/// clears), `keyColor`/`outputColor` run ffOptionParseColor on the string
/// value (non-strings clear; invalid colors exit 479 at the module's
/// parse, before its rows print), `keyWidth` is the u32 reinterpretation
/// of yyjson_get_uint. Pairs are visited in document order, so a
/// duplicate key wins last, and `type`/`condition` are consumed skips.
/// The `format` outputFormat is the separate format-engine work and is
/// left in the entry for the modules' own parses to ignore.
pub(crate) fn parse_common_args(opts: Option<&crate::jsonc::Value>, instance: &Instance) -> CommonArgs {
    let colors = crate::cli::ColorState::from_instance(instance);
    let crate::jsonc::Value::Object(entries) = opts.unwrap_or(&crate::jsonc::Value::Null) else {
        return CommonArgs::default();
    };
    let mut args = CommonArgs::default();
    for (name, value) in entries {
        match name.as_str() {
            "type" | "condition" => {},
            "key" => {
                args.key = value.as_str().map(str::to_owned);
            },
            "keyIcon" => {
                args.key_icon = value.as_str().map(str::to_owned);
            },
            "keyColor" => {
                args.key_color = Some(
                    value
                        .as_str()
                        .map_or_else(String::new, |code| colors.parse_color_deferred(code, &instance.pending_exit)),
                );
            },
            "outputColor" => {
                args.output_color = Some(
                    value
                        .as_str()
                        .map_or_else(String::new, |code| colors.parse_color_deferred(code, &instance.pending_exit)),
                );
            },
            "keyWidth" => {
                args.key_width = match value {
                    crate::jsonc::Value::Int(int) => *int as u32,
                    crate::jsonc::Value::UInt(uint) => *uint as u32,
                    _ => 0,
                };
            },
            _ => {},
        }
    }
    args
}

/// The key-path arg lists: ffPrintLogoAndKey's DEFAULT sites expand the
/// custom key against {index, icon, module-name}; the self-expanding
/// modules prebuild theirs against a module-specific list in which only
/// the trailing {icon}/{module-name} entries are known here — every
/// module-specific name and position (and thus any positional) is the
/// format-engine work and stays verbatim.
enum KeyArgs<'a> {
    Common { index: u32, icon: &'a str, module_name: &'a str },
    Prebuilt { icon: &'a str, module_name: &'a str },
}

/// Modules whose print prebuilds their key (their ffPrintLogoAndKey call
/// passes the built text with FF_PRINT_TYPE_NO_CUSTOM_KEY): the common
/// slots (keyColor/keyWidth/keyIcon/outputColor) still apply, but the
/// key template expands against the module's own arg list, so {index}
/// has no meaning and {} does not count into it.
const TYPE_K_KEY_MODULES: &[&str] = &[
    "battery",
    "bios",
    "bluetoothradio",
    "brightness",
    "btrfs",
    "codec",
    "cpucache",
    "disk",
    "diskio",
    "display",
    "loadavg",
    "localip",
    "monitor",
    "netio",
    "os",
    "physicaldisk",
    "swap",
    "zpool",
];

/// ffParseFormatString (format.c) over a custom key: the spec grammar the
/// key paths exercise — `{}`/`{0}` sequential positionals, numeric and
/// named args (case-insensitive), `{#color}` through ffOptionParseColor
/// and `{#}` as a reset (pipe drops both), `{{` escapes, an unmatched
/// trailing brace expanding like any spec, `{-}` stopping the expansion,
/// and the truncation specs `:` (trim-right-space), `<`/`>`/`|`
/// (pad/align) with negative lengths taking an ellipsis. Conditionals
/// {?}/{/}, {$env}/{$constants} and `~` ranges are the format-engine
/// work and stay verbatim; Type K module-specific args likewise;
/// invalid placeholders mirror upstream's appendInvalidPlaceholder
/// bytes. The TTY expansion closes with a reset. An invalid `{#color}`
/// defers its exit(479) into `pending` like the common-arg colors.
fn expand_key(
    template: &str,
    args: &KeyArgs<'_>,
    instance: &Instance,
) -> String {
    let colors = crate::cli::ColorState::from_instance(instance);
    let pipe = instance.pipe;
    let num_args = match args {
        KeyArgs::Common { .. } => 3,
        KeyArgs::Prebuilt { .. } => 0,
    };
    let named = |name: &str| -> Option<String> {
        let icon_module = |icon: &str, module_name: &str| -> Option<String> {
            if name.eq_ignore_ascii_case("icon") {
                Some(icon.to_string())
            } else if name.eq_ignore_ascii_case("module-name") {
                Some(module_name.to_string())
            } else {
                None
            }
        };
        match args {
            KeyArgs::Common { index, icon, module_name } => {
                if name.eq_ignore_ascii_case("index") {
                    Some(index.to_string())
                } else {
                    icon_module(icon, module_name)
                }
            },
            KeyArgs::Prebuilt { icon, module_name } => icon_module(icon, module_name),
        }
    };
    let positional = |i: u32| -> Option<String> {
        match args {
            KeyArgs::Common { index, icon, module_name } => match i {
                1 => Some(index.to_string()),
                2 => Some(icon.to_string()),
                3 => Some(module_name.to_string()),
                _ => None,
            },
            KeyArgs::Prebuilt { .. } => None,
        }
    };
    // strToInt32: strtoll base 10 clamped to i32. No digits convert only
    // when the whole value is empty (valid 0); otherwise leftover
    // characters make the placeholder invalid, like upstream's *pEnd.
    let str_to_i32 = |value: &str| -> Option<i32> {
        let bytes = value.as_bytes();
        let mut offset = 0;
        while offset < bytes.len() && matches!(bytes[offset], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') {
            offset += 1;
        }
        let negative = offset < bytes.len() && bytes[offset] == b'-';
        if offset < bytes.len() && (bytes[offset] == b'+' || bytes[offset] == b'-') {
            offset += 1;
        }
        let start = offset;
        while offset < bytes.len() && bytes[offset].is_ascii_digit() {
            offset += 1;
        }
        if offset == start {
            return value.is_empty().then_some(0);
        }
        if offset != bytes.len() {
            return None;
        }
        let magnitude = value[start..offset].parse::<i128>().unwrap_or(i128::from(i64::MAX));
        let signed = if negative { -magnitude } else { magnitude };
        Some(signed.clamp(i32::MIN as i128, i32::MAX as i128) as i32)
    };
    // ffStrbufSubstrBefore is byte-based; the port cuts at the nearest
    // char boundary below the length so its Strings stay valid UTF-8.
    let byte_floor = |value: &str, mut end: usize| -> usize {
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        end
    };
    let mut out = String::new();
    let mut arg_counter = 0u32;
    let mut rest = template;
    while !rest.is_empty() {
        let Some(open) = rest.find('{') else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        if after.starts_with('{') {
            out.push('{');
            rest = &after[1..];
            continue;
        }
        // An unmatched trailing brace turns the remainder into the spec;
        // the closing brace only prints when it was actually found.
        let (spec, terminated) = match after.find('}') {
            Some(close) => (&after[..close], true),
            None => (after, false),
        };
        rest = after.find('}').map_or("", |close| &after[close + 1..]);
        if spec.len() == 1 {
            match spec.as_bytes()[0] {
                // {-} stops the whole expansion; the TTY reset still closes.
                b'-' => break,
                b'#' => {
                    if !pipe {
                        out.push_str("\x1b[m");
                    }
                    continue;
                },
                // Conditionals defer to the format engine: an unmatched
                // end token prints verbatim, a matched one skips.
                b'?' | b'/' => {
                    out.push('{');
                    out.push_str(spec);
                    if terminated {
                        out.push('}');
                    }
                    continue;
                },
                // Everything else, including single digits, is a
                // positional arg lookup.
                _ => {},
            }
        }
        if let Some(color) = spec.strip_prefix('#') {
            if !pipe {
                out.push_str("\x1b[");
                out.push_str(&colors.parse_color_deferred(color, &instance.pending_exit));
                out.push('m');
            }
            continue;
        }
        if spec.starts_with('$') || spec.starts_with('?') || spec.starts_with('/') {
            // Constants/env vars and conditionals defer to the format
            // engine; upstream would resolve or skip them.
            out.push('{');
            out.push_str(spec);
            if terminated {
                out.push('}');
            }
            continue;
        }
        let invalid = |out: &mut String| {
            out.push('{');
            out.push_str(spec);
            if terminated {
                out.push('}');
            }
        };
        let sep_at = spec.find([':', '<', '>', '|', '~']);
        let (head, sep, tail) = match sep_at {
            Some(at) => (&spec[..at], spec.as_bytes()[at], Some(&spec[at + 1..])),
            None => (spec, 0, None),
        };
        let resolved = if head.is_empty() {
            arg_counter += 1;
            positional(arg_counter)
        } else if head.as_bytes()[0].is_ascii_digit() {
            // strtoul's whole-string parse and tail check; "0" falls back
            // to the sequential counter.
            match head.parse::<u32>() {
                Ok(0) => {
                    arg_counter += 1;
                    positional(arg_counter)
                },
                Ok(n) if n <= num_args => positional(n),
                _ => None,
            }
        } else if head.as_bytes()[0].is_ascii_alphabetic() {
            named(head)
        } else {
            None
        };
        let Some(value) = resolved else {
            invalid(&mut out);
            continue;
        };
        let Some(sep) = (sep != 0).then_some(sep) else {
            out.push_str(&value);
            continue;
        };
        let Some(tail) = tail else {
            out.push_str(&value);
            continue;
        };
        if sep == b'~' {
            // Substring ranges are the format engine's skipAnsiEscape
            // work; they stay verbatim here.
            invalid(&mut out);
            continue;
        }
        let Some(mut trunc) = str_to_i32(tail) else {
            invalid(&mut out);
            continue;
        };
        let mut ellipsis = false;
        if trunc < 0 {
            ellipsis = true;
            trunc = if trunc == i32::MIN { i32::MAX } else { -trunc };
        }
        let trunc = trunc as u32;
        let value_len = value.len() as u32;
        if value_len == trunc {
            out.push_str(&value);
        } else if value_len > trunc {
            let end = if sep == b':' {
                let end = byte_floor(&value, trunc as usize);
                value[..end].trim_end_matches(|c: char| c.is_ascii_whitespace()).len()
            } else {
                byte_floor(&value, if ellipsis { trunc.saturating_sub(1) } else { trunc } as usize)
            };
            out.push_str(&value[..end]);
            if ellipsis {
                out.push('…');
            }
        } else if sep == b':' {
            out.push_str(&value);
        } else {
            let padding = trunc - value_len;
            if sep == b'<' {
                out.push_str(&value);
                out.push_str(&" ".repeat(padding as usize));
            } else if sep == b'>' {
                out.push_str(&" ".repeat(padding as usize));
                out.push_str(&value);
            } else {
                let half = padding / 2;
                out.push_str(&" ".repeat(half as usize));
                out.push_str(&value);
                out.push_str(&" ".repeat((padding - half) as usize));
            }
        }
    }
    if !pipe {
        out.push_str("\x1b[m");
    }
    out
}

/// Upstream error sites that pass FF_PRINT_TYPE_NO_CUSTOM_KEY: their
/// error rows keep the built-in key text under a custom key, while the
/// colors/width/icon slots still apply (only the text is skipped).
const ERROR_FIXED_KEY: &[&str] = &["break", "codec", "colors", "diskio", "dns", "logo", "separator", "title"];

/// Stamps one job's common args onto the rows it produced: every row
/// carries the colors/width/icon slots, and rows that print a key get the
/// custom key expanded with ffPrintLogoAndKey's {index}/{icon}/
/// {module-name} args. Keyless rows never take a key override; error rows
/// of the NO_CUSTOM_KEY sites keep their text.
pub(crate) fn stamp_common(rows: &mut [Row], args: &CommonArgs, module: &str, instance: &Instance) {
    // Keyed rows carry the slots even when empty — an empty-string
    // keyColor explicitly clears back to the display default, which the
    // renderer resolves through the same Some-iff-set rule upstream uses.
    // Keyless rows (the title) never pass through ffPrintLogoAndKey
    // upstream and so carry no module args at all; the custom module's
    // bare rows get theirs from stamp_custom instead.
    let has_custom = args.key.as_deref().is_some_and(|key| !key.is_empty() && key != " ");
    let error_fixed = ERROR_FIXED_KEY.contains(&module);
    let module_name = crate::module_display_names::display_name_for(module);
    let default_icon = crate::module_icons::icon_for(module);
    for row in rows.iter_mut() {
        // The late placeholder carries the entry so the renderer can
        // stamp the rows its detection expands into.
        if row.key.is_none() && !matches!(row.kind, Kind::Late(..)) {
            continue;
        }
        row.common = Some(args.clone());
        if has_custom && !(error_fixed && matches!(row.kind, Kind::Error)) {
            let icon = args.key_icon.as_deref().unwrap_or(default_icon);
            let key_args = if TYPE_K_KEY_MODULES.contains(&module) {
                KeyArgs::Prebuilt { icon, module_name }
            } else {
                KeyArgs::Common { index: row.key_index, icon, module_name }
            };
            row.key_override = Some(expand_key(args.key.as_deref().unwrap_or_default(), &key_args, instance));
        }
    }
}

/// The custom module's rows: its default key is the magic `" "`, so a
/// custom key turns the bare row into a keyed one.
fn stamp_custom(rows: &mut [Row], args: &CommonArgs, instance: &Instance) {
    let module_name = crate::module_display_names::display_name_for("custom");
    let icon = args
        .key_icon
        .as_deref()
        .unwrap_or_else(|| crate::module_icons::icon_for("custom"));
    for row in rows.iter_mut() {
        row.common = Some(args.clone());
        // The default and the magic key print bare; the empty string
        // falls back to the module name like every other module.
        let key = match args.key.as_deref() {
            None | Some(" ") => continue,
            Some("") => module_name.to_string(),
            Some(custom) => {
                let key_args = KeyArgs::Common { index: row.key_index, icon, module_name };
                expand_key(custom, &key_args, instance)
            },
        };
        row.key = Some(key);
    }
}

pub fn collect(
    selected: &[String],
    structure_given: bool,
    config: Option<&crate::config::Config>,
    instance: &Instance,
    stats: &mut Vec<(usize, f64)>,
) -> Vec<Row> {
    let jobs = build_jobs(selected, structure_given, config);
    prepare_rate_modules(&jobs);

    let mut rows = Vec::new();
    // display.stat: one boundary/ms entry per module, the boundary being
    // the row count when the module finished. The printer emits the
    // timing line when it reaches the boundary.
    let stat_on = instance.stat >= 0;
    // Structure order wins: `-s b:os:a` prints rows in exactly that
    // order, like fastfetch's structure loop. Duplicates allowed.
    // Unknown tokens are the hidden "<no implementation provided>"
    // error rows and never print.
    for job in &jobs {
        let started = if stat_on { crate::spin::time_get_tick() } else { 0.0 };
        match job {
            PresetJob::Module(module) => {
                let mut produced = (module.run)(instance);
                stamp_icons(&mut produced, module.name);
                rows.extend(produced);
            },
            PresetJob::ModuleOpts { def, opts } => {
                // logo/separator/break have no ffJsonConfigParseModuleArgs
                // arm upstream; their entries carry only their own keys.
                let args = if matches!(def.name, "logo" | "separator" | "break") {
                    CommonArgs::default()
                } else {
                    parse_common_args(Some(opts), instance)
                };
                // A deferred color exit stops the structure loop right
                // here, before the failing module prints anything.
                if instance.pending_exit.borrow().is_some() {
                    break;
                }
                // Each object entry runs with its own options: the job
                // gets a private instance carrying the entry.
                let mut job_instance = instance.clone();
                job_instance.entry = Some(opts);
                let mut produced = (def.run)(&job_instance);
                stamp_icons(&mut produced, def.name);
                stamp_common(&mut produced, &args, def.name, &job_instance);
                // A {#color} failure inside the key template fires after
                // run(): upstream's print dies in ffPrintLogoAndKey, so
                // this module's rows never print either.
                if job_instance.pending_exit.borrow().is_some() {
                    // The state was set on the per-entry clone; lift it to
                    // the outer instance for the renderer's exit point.
                    *instance.pending_exit.borrow_mut() = job_instance.pending_exit.borrow().clone();
                    break;
                }
                rows.extend(produced);
            },
            // ffPrintError(line, 0, nullptr, FF_PRINT_TYPE_NO_CUSTOM_KEY,
            // "<no implementation provided>"): visible only with
            // display.showErrors. The moduleArgs are null here, so the
            // row carries no key icon and no common args.
            PresetJob::Unknown(token) => {
                if instance.show_errors {
                    rows.push(Row::error(token, "<no implementation provided>"));
                }
            },
            // The custom module's default key is the magic " ", so its
            // rows are keyless unless the entry sets a key.
            PresetJob::Custom { format, opts } => {
                let args = parse_common_args(*opts, instance);
                if instance.pending_exit.borrow().is_some() {
                    break;
                }
                let mut produced = custom::render_rows(format, instance);
                stamp_icons(&mut produced, "custom");
                stamp_custom(&mut produced, &args, instance);
                if instance.pending_exit.borrow().is_some() {
                    break;
                }
                rows.extend(produced);
            },
            PresetJob::Command {
                text,
                shell,
                param,
                split_lines,
                opts,
            } => {
                let args = parse_common_args(*opts, instance);
                if instance.pending_exit.borrow().is_some() {
                    break;
                }
                let mut produced = command::run_text(
                    text,
                    shell,
                    param.as_deref(),
                    *split_lines,
                    instance,
                );
                stamp_icons(&mut produced, "command");
                stamp_common(&mut produced, &args, "command", instance);
                if instance.pending_exit.borrow().is_some() {
                    break;
                }
                rows.extend(produced);
            },
        }
        if stat_on {
            stats.push((rows.len(), crate::spin::time_get_tick() - started));
        }
    }
    // ffPrintError's gate: module parse/runtime error rows stay hidden
    // unless display.showErrors. JSON mode is unaffected — its errors
    // ride the result objects.
    if !instance.show_errors {
        rows.retain(|row| !matches!(row.kind, Kind::Error));
    }
    rows
}

/// --format json: one {type, result|error} item per structure entry, in
/// structure order. Unknown tokens print their verbatim spelling.
pub fn collect_json(
    selected: &[String],
    structure_given: bool,
    config: Option<&crate::config::Config>,
    instance: &Instance,
) -> Vec<JsonItem> {
    let jobs = build_jobs(selected, structure_given, config);
    prepare_rate_modules(&jobs);

    let mut items = Vec::new();
    let stat_on = instance.stat >= 0;
    for job in &jobs {
        let started = if stat_on { crate::spin::time_get_tick() } else { 0.0 };
        let mut item = match job {
            PresetJob::Module(module) => JsonItem {
                type_name: canonical_type(module.name).to_string(),
                // A module without a ported generator still prints the
                // fixed upstream error for generator-less modules.
                outcome: module.json.map_or(JsonOutcome::Unsupported, |json| (json)(instance)),
                stat: None,
            },
            PresetJob::ModuleOpts { def, opts } => {
                let mut instance = instance.clone();
                instance.entry = Some(opts);
                // The parse pass runs before generateJsonResult in JSON
                // mode too, so common-arg color errors abort before any
                // of the document is written (nothing precedes the dump).
                if !matches!(def.name, "logo" | "separator" | "break") {
                    parse_common_args(Some(opts), &instance);
                    if let Some((code, message)) = instance.pending_exit.borrow_mut().take() {
                        eprintln!("{message}");
                        std::process::exit(code);
                    }
                }
                JsonItem {
                    type_name: canonical_type(def.name).to_string(),
                    outcome: def.json.map_or(JsonOutcome::Unsupported, |json| (json)(&instance)),
                    stat: None,
                }
            },
            PresetJob::Unknown(token) => JsonItem {
                type_name: token.clone(),
                outcome: JsonOutcome::Error("Unknown module type".to_string()),
                stat: None,
            },
            PresetJob::Custom { format, opts } => {
                parse_common_args(*opts, instance);
                if let Some((code, message)) = instance.pending_exit.borrow_mut().take() {
                    eprintln!("{message}");
                    std::process::exit(code);
                }
                JsonItem {
                    type_name: "Custom".to_string(),
                    outcome: custom::json(format, instance),
                    stat: None,
                }
            },
            PresetJob::Command {
                text,
                shell,
                param,
                split_lines,
                opts,
            } => {
                parse_common_args(*opts, instance);
                if let Some((code, message)) = instance.pending_exit.borrow_mut().take() {
                    eprintln!("{message}");
                    std::process::exit(code);
                }
                JsonItem {
                    type_name: "Command".to_string(),
                    outcome: command::json(text, shell, param.as_deref(), *split_lines, instance),
                    stat: None,
                }
            },
        };
        if stat_on {
            item.stat = Some(crate::spin::time_get_tick() - started);
        }
        items.push(item);
    }
    items
}

#[cfg(test)]
mod preset_tests {
    use super::{module_job, PresetJob, REGISTRY, MODULE_LIST};

    fn job_names(jobs: &[PresetJob]) -> Vec<String> {
        jobs.iter()
            .map(|job| match job {
                PresetJob::Module(module) => module.name.to_string(),
                PresetJob::ModuleOpts { def, .. } => format!("opts:{}", def.name),
                PresetJob::Unknown(token) => format!("unknown:{token}"),
                PresetJob::Custom { format, .. } => format!("custom:{format}"),
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
        let config = crate::config::for_test(config);
        assert!(config.document_loaded);
        let jobs = super::preset_jobs_from_config(&config);
        assert_eq!(
            job_names(&jobs),
            [
                "title",
                "separator",
                "os",
                "host",
                "break",
                "custom:hello {#red}world",
                "opts:os",
                "colors",
            ]
        );

        // A loaded document with no modules key prints nothing (upstream:
        // printJsonConfig returns early) instead of the default preset.
        let config = crate::config::for_test(r#"{"logo": null}"#);
        assert!(config.document_loaded);
        assert!(config.logo_none);
        assert!(super::preset_jobs_from_config(&config).is_empty());
    }

    #[test]
    fn custom_without_format_still_prints() {
        let config = crate::config::for_test(r#"{"modules": [{"type": "custom"}]}"#);
        let jobs = super::preset_jobs_from_config(&config);
        // An object carrying only "type" behaves like the bare string; the
        // custom module's default (empty) format prints the same empty row.
        assert_eq!(job_names(&jobs), ["custom"]);
    }

    #[test]
    fn command_job_carries_options() {
        let config =
            crate::config::for_test(r#"{"modules": [{"type": "command", "text": "echo hi", "splitLines": true}]}"#);
        let jobs = super::preset_jobs_from_config(&config);
        match &jobs[0] {
            PresetJob::Command {
                text,
                shell,
                param,
                split_lines,
                ..
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

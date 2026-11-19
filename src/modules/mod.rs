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
            let opts = (*opts).unwrap_or(&EMPTY_OBJECT);
            match type_.to_ascii_lowercase().as_str() {
                "custom" => {
                    // A custom without a format still prints its empty row.
                    let format = opts
                        .get("format")
                        .and_then(crate::jsonc::Value::as_str)
                        .unwrap_or_default();
                    PresetJob::Custom(format.to_string())
                },
                "command" => {
                    // fastfetch's defaults: /bin/sh -c, splitLines off.
                    PresetJob::Command {
                        text: opts
                            .get("text")
                            .and_then(crate::jsonc::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        shell: opts
                            .get("shell")
                            .and_then(crate::jsonc::Value::as_str)
                            .unwrap_or("/bin/sh")
                            .to_string(),
                        param: Some(
                            opts.get("param")
                                .and_then(crate::jsonc::Value::as_str)
                                .unwrap_or("-c")
                                .to_string(),
                        ),
                        split_lines: opts
                            .get("splitLines")
                            .and_then(crate::jsonc::Value::as_bool)
                            .unwrap_or(false),
                    }
                },
                _ => match module_job(type_) {
                    Some(PresetJob::Module(def)) => PresetJob::ModuleOpts { def, opts },
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

/// One `{type, result|error}` entry of --format json output.
pub struct JsonItem {
    pub type_name: String,
    pub outcome: JsonOutcome,
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
}

/// Runs the selected modules (or the default preset when no structure was
/// given) in structure order.
///
/// Modules are independent of each other, so each one runs in its own
/// scoped thread: the slow data sources (gdbus, gsettings, process scans)
/// overlap instead of adding up. Rows are emitted in structure order.
pub fn collect(
    selected: &[String],
    structure_given: bool,
    config: Option<&crate::config::Config>,
    instance: &Instance,
) -> Vec<Row> {
    let jobs = build_jobs(selected, structure_given, config);
    prepare_rate_modules(&jobs);

    let results: Vec<Vec<Row>> = std::thread::scope(|scope| {
        // Structure order wins: `-s b:os:a` prints rows in exactly that
        // order, like fastfetch's structure loop. Duplicates allowed.
        // Unknown tokens are the hidden "<no implementation provided>"
        // error rows and never print.
        jobs.iter()
            .map(|job| match job {
                PresetJob::Module(module) => scope.spawn(move || (module.run)(instance)),
                PresetJob::ModuleOpts { def, opts } => {
                    // Each object entry runs with its own options: the job
                    // thread gets a private instance carrying the entry.
                    let mut instance = instance.clone();
                    instance.entry = Some(opts);
                    scope.spawn(move || (def.run)(&instance))
                },
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
pub fn collect_json(
    selected: &[String],
    structure_given: bool,
    config: Option<&crate::config::Config>,
    instance: &Instance,
) -> Vec<JsonItem> {
    let jobs = build_jobs(selected, structure_given, config);
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
                PresetJob::ModuleOpts { def, opts } => {
                    let mut instance = instance.clone();
                    instance.entry = Some(opts);
                    scope.spawn(move || JsonItem {
                        type_name: canonical_type(def.name).to_string(),
                        outcome: def.json.map_or(JsonOutcome::Unsupported, |json| (json)(&instance)),
                    })
                },
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
                PresetJob::ModuleOpts { def, .. } => format!("opts:{}", def.name),
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

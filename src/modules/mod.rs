//! Module registry: every module is a name plus a function returning rows.

pub mod battery;
pub mod bios;
pub mod board;
pub mod colors;
pub mod cpu;
pub mod cursor;
pub mod de;
pub mod disk;
pub mod display;
pub mod font;
pub mod gpu;
pub mod gtk;
pub mod host;
pub mod icons;
pub mod kernel;
pub mod lm;
pub mod loadavg;
pub mod locale;
pub mod localip;
pub mod memory;
pub mod os;
pub mod packages;
pub mod physicaldisk;
pub mod processes;
pub mod separator;
pub mod shell;
pub mod swap;
pub mod terminal;
pub mod terminalsize;
pub mod theme;
pub mod title;
pub mod uptime;
pub mod wm;
pub mod wmtheme;

use crate::instance::Instance;

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

macro_rules! modules {
    ($(($name:ident, $doc:literal)),+ $(,)?) => {
        pub const REGISTRY: &[ModuleDef] = &[
            $(ModuleDef { name: stringify!($name), doc: $doc, run: $name::run }),+
        ];
    };
}

modules![
    (title, "user@host"),
    (separator, "dashed separator line"),
    (os, "operating system name and architecture"),
    (host, "device model name"),
    (bios, "firmware version and release date"),
    (board, "motherboard name and version"),
    (kernel, "kernel name and release"),
    (uptime, "time since boot"),
    (loadavg, "load averages of the last 1, 5 and 15 minutes"),
    (lm, "login manager"),
    (processes, "number of running processes"),
    (packages, "installed packages by package manager"),
    (shell, "user shell and version"),
    (display, "monitors: resolution, size, refresh rate"),
    (de, "desktop environment"),
    (wm, "window manager"),
    (wmtheme, "window manager theme"),
    (theme, "gtk theme"),
    (icons, "icon theme"),
    (font, "desktop font"),
    (cursor, "cursor theme and size"),
    (terminal, "terminal emulator"),
    (terminalsize, "terminal dimensions in cells and pixels"),
    (cpu, "cpu model, core count and frequency"),
    (gpu, "gpu vendor and model"),
    (memory, "ram usage"),
    (swap, "swap usage"),
    (disk, "root filesystem usage"),
    (physicaldisk, "physical storage devices"),
    (localip, "lan addresses of network interfaces"),
    (locale, "system locale"),
    (battery, "battery charge and status"),
    (colors, "terminal color palette"),
];

/// Modules shown when no explicit selection is given, mirroring fastfetch's
/// default output.
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
    "cpu",
    "gpu",
    "memory",
    "swap",
    "disk",
    "localip",
    "locale",
    "battery",
    "colors",
];

pub fn list_modules() {
    for module in REGISTRY {
        println!("{:10} {}", module.name, module.doc);
    }
}

/// Percent values colored like fastfetch: green when usage is low, yellow in
/// the middle band, red when high. Plain text without colors.
pub fn colorize_percent(pct: u64, color: bool) -> String {
    let code = if pct < 25 {
        "32"
    } else if pct < 75 {
        "93"
    } else {
        "91"
    };
    if color {
        format!("\x1b[{code}m{pct}%\x1b[0m")
    } else {
        format!("{pct}%")
    }
}

/// Runs the selected modules (or the default preset) in registry order.
///
/// Modules are independent of each other, so each one runs in its own
/// scoped thread: the slow data sources (gdbus, gsettings, process scans)
/// overlap instead of adding up. Rows are still emitted in registry order.
pub fn collect(selected: &[String], instance: &Instance) -> Vec<Row> {
    let names: Vec<&str> = if selected.is_empty() {
        DEFAULT_PRESET.to_vec()
    } else {
        let unknown: Vec<&str> = selected
            .iter()
            .filter(|name| REGISTRY.iter().all(|module| module.name != name.as_str()))
            .map(String::as_str)
            .collect();
        if !unknown.is_empty() {
            eprintln!("rustfetch: unknown module(s): {}", unknown.join(", "));
            std::process::exit(2);
        }
        selected.iter().map(String::as_str).collect()
    };

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

//! Runtime context shared by all modules: color capability and distro identity.

use std::collections::HashMap;
use std::io::IsTerminal;
use std::sync::{Mutex, OnceLock};

use crate::{cli::Cli, util};

/// The title/key colors fastfetch derives from the logo definition
/// (colorTitle = colors[0], colorKeys = colors[1]); set even when the
/// logo itself is hidden with --logo none.
#[derive(Clone, Copy)]
pub struct LogoColors {
    pub title: Option<&'static str>,
    pub key: Option<&'static str>,
}

pub struct Instance {
    /// display.c:957 — `!isatty(stdout) || NO_COLOR is set (even empty)`,
    /// or an explicit `--pipe` / config `display.pipe`. Gates {#color}
    /// tokens in custom formats even in JSON mode.
    pub pipe: bool,
    /// True when ANSI escape sequences may be printed (interactive tty,
    /// colors not disabled via `--pipe` or `NO_COLOR`; unlike many tools,
    /// fastfetch does not honor TERM=dumb).
    pub color: bool,
    /// Logo state mirrored from the CLI: the NONE type hides the logo
    /// entirely, SMALL selects the compact variant, and the source names
    /// a builtin logo or a file.
    pub logo_none: bool,
    pub logo_small: bool,
    pub logo_source: Option<String>,
    /// `ID=` from /etc/os-release, lowercased.
    pub distro_id: Option<String>,
    /// `PRETTY_NAME=` from /etc/os-release, tried by the logo detection
    /// after the id (upstream's os->name).
    pub distro_name: Option<String>,
    /// `ID_LIKE=` entries from /etc/os-release, lowercased.
    pub distro_like: Vec<String>,
    /// The title/key colors fastfetch derives from the logo definition
    /// (colorTitle = colors[0], colorKeys = colors[1]); set even when the
    /// logo itself is hidden with --logo none.
    pub logo_colors: OnceLock<LogoColors>,
    /// `logo.padding.top` from the config; blank key-column lines inserted
    /// above the logo block.
    pub padding_top: Option<u32>,
    /// The current config entry's object (per-job: each module job gets a
    /// clone carrying its own entry), read by modules with config options.
    pub entry: Option<&'static crate::jsonc::Value>,
    /// Memoized GSettings/dconf reads per (schema, key), filled lazily by the
    /// GTK modules. A Mutex because modules run in parallel threads.
    pub gnome_settings: Mutex<HashMap<String, Option<String>>>,
}

impl Clone for Instance {
    fn clone(&self) -> Self {
        Self {
            pipe: self.pipe,
            color: self.color,
            logo_colors: self.logo_colors.clone(),
            distro_id: self.distro_id.clone(),
            distro_name: self.distro_name.clone(),
            distro_like: self.distro_like.clone(),
            logo_none: self.logo_none,
            logo_small: self.logo_small,
            logo_source: self.logo_source.clone(),
            padding_top: self.padding_top,
            entry: self.entry,
            gnome_settings: Mutex::new(self.gnome_settings.lock().unwrap_or_else(|e| e.into_inner()).clone()),
        }
    }
}

impl Instance {
    pub fn new(cli: &Cli, config_pipe: bool, padding_top: Option<u32>) -> Self {
        // display.c:957 — --pipe defaults to !isatty(stdout) || NO_COLOR
        // (getenv, so an empty value counts too); --pipe false overrides;
        // config display.pipe applies unless the command line said
        // something (the config loads before the option pass).
        let pipe = cli.pipe
            .unwrap_or(if config_pipe {
                true
            } else {
                !std::io::stdout().is_terminal() || std::env::var_os("NO_COLOR").is_some()
            });
        let color = !pipe && !cli.format_json;

        let mut distro_id = None;
        let mut distro_name = None;
        let mut distro_like = Vec::new();
        if let Some(content) = util::read_file("/etc/os-release") {
            for (key, value) in util::parse_kv(&content) {
                match key {
                    "ID" => distro_id = Some(value.to_lowercase()),
                    "PRETTY_NAME" => distro_name = Some(value.to_string()),
                    "ID_LIKE" => {
                        distro_like = value.split_whitespace().map(|v| v.to_lowercase()).collect();
                    },
                    _ => {},
                }
            }
        }

        Self {
            pipe,
            color,
            distro_id,
            distro_name,
            distro_like,
            logo_none: cli.logo_none,
            logo_small: cli.logo_small,
            logo_source: cli.logo_source.clone(),
            padding_top,
            entry: None,
            gnome_settings: Mutex::new(HashMap::new()),
            logo_colors: OnceLock::new(),
        }
    }
}

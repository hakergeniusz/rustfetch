//! Runtime context shared by all modules: color capability and distro identity.

use std::collections::HashMap;
use std::io::IsTerminal;
use std::sync::Mutex;

use crate::{cli::Cli, cli::Format, util};

pub struct Instance {
    /// True when ANSI escape sequences may be printed (interactive tty, colors
    /// not disabled via `--pipe`, `NO_COLOR` or `TERM=dumb`).
    pub color: bool,
    /// `ID=` from /etc/os-release, lowercased.
    pub distro_id: Option<String>,
    /// `ID_LIKE=` entries from /etc/os-release, lowercased.
    pub distro_like: Vec<String>,
    /// Memoized GSettings/dconf reads per (schema, key), filled lazily by the
    /// GTK modules. A Mutex because modules run in parallel threads.
    pub gnome_settings: Mutex<HashMap<String, Option<String>>>,
}

impl Instance {
    pub fn new(cli: &Cli) -> Self {
        let color = !cli.pipe
            && cli.format == Format::Standard
            && std::env::var_os("NO_COLOR").is_none()
            && std::env::var("TERM").is_ok_and(|term| term != "dumb")
            && std::io::stdout().is_terminal();

        let mut distro_id = None;
        let mut distro_like = Vec::new();
        if let Some(content) = util::read_file("/etc/os-release") {
            for (key, value) in util::parse_kv(&content) {
                match key {
                    "ID" => distro_id = Some(value.to_lowercase()),
                    "ID_LIKE" => {
                        distro_like = value.split_whitespace().map(|v| v.to_lowercase()).collect();
                    },
                    _ => {},
                }
            }
        }

        Self {
            color,
            distro_id,
            distro_like,
            gnome_settings: Mutex::new(HashMap::new()),
        }
    }
}

//! Runtime context shared by all modules: color capability and distro identity.

use std::cell::Cell;
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
    /// The text logo's rendering inputs (FFOptionsLogo): paddings,
    /// width/height floors and the printRemaining gate, assembled in
    /// main.rs from the config keys and the command line.
    pub logo_options: crate::logo::LogoOptions,
    /// The `--logo-color-N` / config `logo.color` palette overrides,
    /// config first and the command line last (upstream's one
    /// FFOptionsLogo struct mutated in parse order).
    pub logo_color_overrides: &'static [(usize, &'static str)],
    /// display.showErrors: gates error messages that are hidden by
    /// default (--show-errors, config display.showErrors, --stat).
    pub show_errors: bool,
    /// display.stat: the per-module timing (0 plain, >0 the color-change
    /// threshold in ms, -1 off). collect/collect_json emit one stat line
    /// or "stat" field per module when it is on.
    pub stat: i32,
    /// instance.state.logoSpin: the spin logo's per-process state, filled
    /// by the logo load and turned by the watch loop's redraw.
    pub spin: std::cell::RefCell<Option<crate::spin::SpinState>>,
    /// The current config entry's object (per-job: each module job gets a
    /// clone carrying its own entry), read by modules with config options.
    pub entry: Option<&'static crate::jsonc::Value>,
    /// instance.state.titleFqdn: set when the title module parses the
    /// `fqdn` option; the separator's default length reads it.
    pub title_fqdn: Cell<bool>,
    /// general.playerName: the media module's explicit MPRIS bus, CLI
    /// `--player-name` over the config value.
    pub player_name: String,
    /// ffStart's reset already went out (the image printers flush it
    /// ahead of their raw dumps); the renderer skips its own.
    pub ff_started: Cell<bool>,
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
            logo_options: self.logo_options,
            logo_color_overrides: self.logo_color_overrides,
            show_errors: self.show_errors,
            stat: self.stat,
            // The spin state belongs to the render loop; module jobs get
            // none.
            spin: std::cell::RefCell::new(None),
            entry: self.entry,
            ff_started: Cell::new(self.ff_started.get()),
            player_name: self.player_name.clone(),
            title_fqdn: Cell::new(self.title_fqdn.get()),
            gnome_settings: Mutex::new(self.gnome_settings.lock().unwrap_or_else(|e| e.into_inner()).clone()),
        }
    }
}

impl Instance {
    pub fn new(
        cli: &Cli,
        config_pipe: bool,
        show_errors: bool,
        stat: i32,
        player_name: String,
        logo_options: crate::logo::LogoOptions,
        logo_color_overrides: &'static [(usize, &'static str)],
    ) -> Self {
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
            logo_options,
            logo_color_overrides,
            show_errors,
            stat,
            spin: std::cell::RefCell::new(None),
            entry: None,
            title_fqdn: Cell::new(false),
            player_name,
            ff_started: Cell::new(false),
            gnome_settings: Mutex::new(HashMap::new()),
            logo_colors: OnceLock::new(),
        }
    }
}

//! Runtime context shared by all modules: color capability and distro identity.

use std::cell::{Cell, RefCell};
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

/// FF_MODULE_KEY_TYPE (common/option.h): the NONE gate hides the whole
/// key block; the STRING bit prints the key text shifted by the value's
/// high nibble; the ICON bit prints the module icon glyph (the icon
/// table lands with the key-icon commit).
pub const KEY_TYPE_NONE: u8 = 0;
pub const KEY_TYPE_STRING: u8 = 1 << 0;
pub const KEY_TYPE_ICON: u8 = 1 << 1;
pub const KEY_TYPE_SPACE_SHIFT: u8 = 4;

/// The display-surface options the key printer and the title read — the
/// FFOptionsDisplay fields ffPrintLogoAndKey touches. Defaults mirror
/// ffOptionsInitDisplay: bright bolt, ": " separator, STRING key type,
/// empty colors, zero key geometry.
#[derive(Clone)]
pub struct DisplaySurface {
    /// display.brightColor: gates the `\e[1m` bolt ahead of the key and
    /// title colors (ffOptionsInitDisplay's terminalLightTheme check is
    /// compiled out of the reference build).
    pub bright_color: bool,
    /// display.keyValueSeparator, printed verbatim after the key.
    pub key_value_separator: String,
    /// display.colorKeys / colorTitle / colorOutput / colorSeparator:
    /// parsed SGR parameter strings; empty prints no escape.
    pub color_keys: String,
    pub color_title: String,
    pub color_output: String,
    pub color_separator: String,
    /// display.keyWidth: a positive value moves the value to column
    /// keyWidth + logoWidth with `\e[{n}G`, in pipe mode too.
    pub key_width: u16,
    /// display.keyPaddingLeft: the space run before the key, in pipe
    /// mode too.
    pub key_padding_left: u16,
    /// display.keyType: the NONE gate around the key block, the STRING
    /// bit's key text, and the (pending) icon bits.
    pub key_type: u8,
    /// display.keyLanguage: optionParseLanguageString's canonical code;
    /// None keeps the default English column. Selects the display-name
    /// table column (FF_MODULE_GET_DISPLAY_NAME's offsetof delta).
    pub key_language: Option<&'static str>,
}

impl Default for DisplaySurface {
    fn default() -> Self {
        Self {
            bright_color: true,
            key_value_separator: ": ".to_string(),
            color_keys: String::new(),
            color_title: String::new(),
            color_output: String::new(),
            color_separator: String::new(),
            key_width: 0,
            key_padding_left: 0,
            key_type: KEY_TYPE_STRING,
            key_language: None,
        }
    }
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
    /// display.disableLinewrap / display.hideCursor, pipe-disabled
    /// (ffStart's globals): `\e[?7l`/`\e[?25l` before output and the
    /// matching restore at ffFinish.
    pub disable_linewrap: bool,
    pub hide_cursor: bool,
    /// The display-surface options (separator, colors, key geometry):
    /// assembled in main.rs from the config and the command line.
    pub display: DisplaySurface,
    /// Memoized GSettings/dconf reads per (schema, key), filled lazily by the
    /// GTK modules. A Mutex because modules run in parallel threads.
    pub gnome_settings: Mutex<HashMap<String, Option<String>>>,
    /// A color-parse exit(479) deferred out of the common-args parse:
    /// upstream parses lazily at each module's print, so earlier modules
    /// keep printing before the abort. collect() stops collecting and the
    /// renderer exits with this code after the rows so far print, stderr
    /// message last. JSON mode reads and exits on it directly (nothing
    /// precedes the doc).
    pub pending_exit: RefCell<Option<(i32, String)>>,
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
            disable_linewrap: self.disable_linewrap,
            hide_cursor: self.hide_cursor,
            display: self.display.clone(),
            player_name: self.player_name.clone(),
            title_fqdn: Cell::new(self.title_fqdn.get()),
            gnome_settings: Mutex::new(self.gnome_settings.lock().unwrap_or_else(|e| e.into_inner()).clone()),
            pending_exit: self.pending_exit.clone(),
        }
    }
}

impl Instance {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cli: &Cli,
        config_pipe: bool,
        show_errors: bool,
        stat: i32,
        player_name: String,
        disable_linewrap: bool,
        hide_cursor: bool,
        display: DisplaySurface,
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
            // ffStart's globals (init.c:112): both toggles are computed
            // pipe-disabled, gating the ffStart prefixes and the ffFinish
            // restores alike.
            disable_linewrap: disable_linewrap && !pipe,
            hide_cursor: hide_cursor && !pipe,
            display,
            ff_started: Cell::new(false),
            gnome_settings: Mutex::new(HashMap::new()),
            logo_colors: OnceLock::new(),
            pending_exit: std::cell::RefCell::new(None),
        }
    }

    /// The effective key color: ffLogoApplyColors fills the display slot
    /// from the logo only when it is empty, so a CLI/config color wins.
    pub fn key_color(&self) -> Option<&str> {
        if !self.display.color_keys.is_empty() {
            Some(&self.display.color_keys)
        } else {
            self.logo_colors.get().and_then(|colors| colors.key)
        }
    }

    /// The effective title color, resolved the same way as the key's.
    pub fn title_color(&self) -> Option<&str> {
        if !self.display.color_title.is_empty() {
            Some(&self.display.color_title)
        } else {
            self.logo_colors.get().and_then(|colors| colors.title)
        }
    }

    /// FF_MODULE_GET_DISPLAY_NAME: the module's display name in the
    /// resolved key language (default keys, interpolated key prefixes,
    /// error rows and the {module-name} arg all read it).
    pub fn display_name(&self, module: &str) -> &'static str {
        crate::module_display_names::display_name_for(self.display.key_language, module)
    }
}

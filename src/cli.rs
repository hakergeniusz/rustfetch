//! CLI parsing for rustfetch: a port of fastfetch.c's two-pass argv walk
//! (`parseCommand`, then `parseOption`) and of common/impl/option.c's
//! parse helpers, so the value-extraction rule (a following token that
//! starts with `-` is not a value), the usage-error strings, and the exit
//! codes match the reference binary byte for byte.

use crate::config::{self, Config};

/// The reference binary's `--help` text, embedded verbatim: upstream
/// prints a hand-maintained document, not a generated one.
const HELP: &str = include_str!("cli_help.txt");

/// `--list-features`: the reference build's compiled-in features.
const LIST_FEATURES: &str = include_str!("list_features.txt");

/// `--help-raw`: the reference's JSON help document.
const HELP_RAW: &str = include_str!("help_raw.txt");

/// A JSON value from HELP_RAW. Objects keep insertion order, which the
/// `Options:` lines of a specific-help entry depend on. This is a small
/// reader for the one fixed document fastfetch ships; it is not a general
/// purpose JSON implementation and adds no dependency.
enum HelpJson {
    #[allow(dead_code)] // HELP_RAW has no `null`, but the reader accepts it
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<HelpJson>),
    Obj(Vec<(String, HelpJson)>),
}

impl HelpJson {
    fn get(&self, key: &str) -> Option<&HelpJson> {
        match self {
            Self::Obj(entries) => entries.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(text) => Some(text),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&[HelpJson]> {
        match self {
            Self::Arr(items) => Some(items),
            _ => None,
        }
    }

    fn as_object(&self) -> Option<&[(String, HelpJson)]> {
        match self {
            Self::Obj(entries) => Some(entries),
            _ => None,
        }
    }
}

struct HelpJsonParser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> HelpJsonParser<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, bytes: text.as_bytes(), pos: 0 }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len()
            && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\n' | b'\r')
        {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn consume(&mut self, token: &str) -> bool {
        if self.text.get(self.pos..self.pos + token.len()) == Some(token) {
            self.pos += token.len();
            true
        } else {
            false
        }
    }

    fn parse(&mut self) -> Option<HelpJson> {
        let value = self.value()?;
        self.skip_ws();
        if self.pos == self.bytes.len() { Some(value) } else { None }
    }

    fn value(&mut self) -> Option<HelpJson> {
        self.skip_ws();
        match self.peek()? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string().map(HelpJson::Str),
            b't' if self.consume("true") => Some(HelpJson::Bool(true)),
            b'f' if self.consume("false") => Some(HelpJson::Bool(false)),
            b'n' if self.consume("null") => Some(HelpJson::Null),
            b'-' | b'0'..=b'9' => self.number().map(HelpJson::Num),
            _ => None,
        }
    }

    fn object(&mut self) -> Option<HelpJson> {
        self.pos += 1; // `{`
        let mut entries = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Some(HelpJson::Obj(entries));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return None;
            }
            self.pos += 1;
            let value = self.value()?;
            entries.push((key, value));
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    return Some(HelpJson::Obj(entries));
                },
                _ => return None,
            }
        }
    }

    fn array(&mut self) -> Option<HelpJson> {
        self.pos += 1; // `[`
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Some(HelpJson::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    return Some(HelpJson::Arr(items));
                },
                _ => return None,
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        if self.peek() != Some(b'"') {
            return None;
        }
        self.pos += 1;
        let mut out = String::new();
        let mut segment = self.pos;
        loop {
            match *self.bytes.get(self.pos)? {
                b'"' => {
                    out.push_str(self.text.get(segment..self.pos)?);
                    self.pos += 1;
                    return Some(out);
                },
                b'\\' => {
                    out.push_str(self.text.get(segment..self.pos)?);
                    self.pos += 1;
                    let escape = *self.bytes.get(self.pos)?;
                    self.pos += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..=0xDBFF).contains(&first) {
                                if self.text.get(self.pos..self.pos + 2) != Some("\\u") {
                                    return None;
                                }
                                self.pos += 2;
                                let second = self.hex4()?;
                                if !(0xDC00..=0xDFFF).contains(&second) {
                                    return None;
                                }
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else {
                                first
                            };
                            out.push(char::from_u32(code)?);
                        },
                        _ => return None,
                    }
                    segment = self.pos;
                },
                _ => self.pos += 1,
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.text.get(self.pos..self.pos + 4)?;
        let value = u32::from_str_radix(digits, 16).ok()?;
        self.pos += 4;
        Some(value)
    }

    fn number(&mut self) -> Option<String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        self.text.get(start..self.pos).map(str::to_string)
    }
}

/// fastfetch.c's FFdata fields that belong to the command pass but have
/// nowhere to live on `Cli`: a struct literal in another module's tests
/// constructs Cli field by field, so the generation state is kept here in
/// thread-local storage (one argv walk per thread).
#[derive(Clone, Default)]
pub(crate) struct GenConfigState {
    /// The `--gen-config` path, or None when the flag was absent.
    pub(crate) path: Option<String>,
    /// The `--structure-disabled` token list.
    pub(crate) structure_disabled: Option<String>,
}

thread_local! {
    static GEN_CONFIG_STATE: std::cell::RefCell<GenConfigState> = std::cell::RefCell::new(GenConfigState::default());
}

/// The state produced by the most recent `Cli::parse_from` on this thread.
pub(crate) fn gen_config_state() -> GenConfigState {
    GEN_CONFIG_STATE.with(|state| state.borrow().clone())
}

pub struct Cli {
    /// `-s/--structure`: the colon-separated module list, kept verbatim.
    pub structure: Option<String>,

    /// Logo state (upstream's FFLogoType + source): `-l none` sets the
    /// NONE type, `-l small` the SMALL type, any other value only sets
    /// the source; later flags overwrite only their own part. The
    /// config's `logo: null` starts at NONE.
    pub logo_none: bool,
    pub logo_small: bool,
    pub logo_source: Option<String>,
    /// `--logo-position auto|left|right|top` (options/logo.c's enum, default
    /// LEFT); the config's `logo.position` is the base value.
    pub logo_position: Option<crate::logo::PositionOpt>,
    /// `--logo-width` / `--logo-height`: floors on the parsed art size
    /// (ffOptionParseUInt32, 0 leaves the option unset).
    pub logo_width: Option<u32>,
    /// `--logo-preserve-aspect-ratio <?bool>`: the iterm image payload's
    /// preserveAspectRatio flag.
    pub logo_preserve_aspect_ratio: Option<bool>,
    pub logo_height: Option<u32>,
    /// `--logo-padding` sets both horizontal paddings; the four sided
    /// flags set one each. The vertical pair feeds the cache build, the
    /// horizontal pair the key column and raw dumps.
    pub logo_padding_top: Option<u32>,
    pub logo_padding_bottom: Option<u32>,
    pub logo_padding_left: Option<u32>,
    pub logo_padding_right: Option<u32>,
    /// `--logo-print-remaining <?bool>`: gates the closing
    /// ffLogoPrintRemaining (the watch rounds' call is not gated).
    pub logo_print_remaining: Option<bool>,
    /// The explicit logo type: `--logo-type`'s enum or the source-type
    /// setters (`--file`, `--data`, `--raw`, the image flags). The sticky
    /// `-l none`/`-l small` bools predate this and win for their values.
    pub logo_type: Option<crate::logo::LogoTypeOpt>,
    /// `--logo-animation-frame <int32>`: the image-animation frame index
    /// (parsed like upstream; no image renderer consumes it yet).
    pub logo_animation_frame: Option<i32>,
    /// `--logo-color-<1-9>` overrides: (slot 0-based, parsed SGR params).
    /// The strings are leaked so the palette can be `&'static`.
    pub logo_colors: Vec<(usize, &'static str)>,
    /// rustfetch-only HyFetch flags; `logo.hyfetch` is the config half.
    /// `--hyfetch-preset` is a comma-separated candidate list, the other
    /// two override their config fields per-field (CLI last).
    pub hyfetch_preset: Option<String>,
    pub hyfetch_mode: Option<crate::hyfetch::AnsiMode>,
    pub hyfetch_lightness: Option<f64>,
    /// `--logo-spin-speed-per-frame <float>`: degrees per watch refresh.
    pub logo_spin_speed_per_frame: Option<f32>,
    /// `--logo-spin-speed-per-round <float>`: seconds per revolution.
    pub logo_spin_speed_per_round: Option<f32>,
    /// `--logo-spin-tilt <float>`: the slab's fixed lean in degrees.
    pub logo_spin_tilt: Option<f32>,
    /// `--logo-spin-angle <float>`: the first frame's yaw in degrees.
    pub logo_spin_angle: Option<f32>,
    /// `--show-errors <?bool>`: unhides the load errors that are silent
    /// by default. `--stat true|<number>` implies it.
    pub show_errors: Option<bool>,
    /// `--stat <?bool|ms>`: per-module timing. `true` or a number (0 =
    /// plain, a positive number = the color-change threshold in ms) times
    /// every module; anything else resets it to unset (-1). The config's
    /// display.stat merges the same way, CLI last.
    pub stat: Option<i32>,
    /// `--pipe <?bool>`: Some when the flag was given, with upstream's
    /// boolean parsing (empty/true/yes/on/1 → true, anything else false).
    pub pipe: Option<bool>,
    /// The refresh interval in ms (`instance.state.dynamicInterval`):
    /// `--dynamic-interval <ms>` verbatim, `-w/--watch [seconds]` times
    /// 1000 (no value → 1000). Zero disables the watch loop.
    pub watch_interval: u32,
    /// JSON output enable state: `-j/--json` and `--format json` enable it
    /// monotonically (upstream's enableJsonOutput); nothing disables it,
    /// so `--format json -j false` still prints JSON.
    pub format_json: bool,

    /// `--player-name <str>`: the media module's explicit MPRIS bus.
    pub player_name: Option<String>,
    /// `--disable-linewrap <?bool>`: `\e[?7l` before output, `\e[?7h` at
    /// the end; both pipe-disabled.
    pub disable_linewrap: Option<bool>,
    /// `--hide-cursor <?bool>`: `\e[?25l` before output, `\e[?25h` at the
    /// end; both pipe-disabled.
    pub hide_cursor: Option<bool>,
    /// `--separator <str>`: display.keyValueSeparator, printed verbatim
    /// after every key.
    pub key_value_separator: Option<String>,
    /// `--color` / `--color-{keys,title,output,separator}`: the parsed
    /// SGR parameter strings (upstream's display.color* slots).
    pub color_keys: Option<String>,
    pub color_title: Option<String>,
    pub color_output: Option<String>,
    pub color_separator: Option<String>,
    /// `--key-width` / `--key-padding-left`: `(uint16_t)
    /// ffOptionParseUInt32` — the `\e[{n}G` jump and the leading spaces.
    pub key_width: Option<u16>,
    pub key_padding_left: Option<u16>,
    /// `--key-type`: display.keyType (FF_MODULE_KEY_TYPE bits).
    pub key_type: Option<u8>,
    /// `--key-language`: optionParseLanguageString's canonical code; a
    /// missing value auto-detects (upstream wraps nullptr in an empty
    /// FFstrbuf, whose length 0 is the `detected` flag).
    pub key_language: Option<&'static str>,
    /// `--duration-abbreviation` / `--duration-space-before-unit`: the
    /// ffDurationAppendNum text toggles.
    pub duration_abbreviation: Option<bool>,
    pub duration_space_before_unit: Option<crate::format::SpaceBeforeUnit>,
    /// `--size-binary-prefix` / `--size-ndigits` (rejects > 9 with exit
    /// 479) / `--size-max-prefix` / `--size-space-before-unit`: the
    /// ffSizeAppendNum knobs.
    pub size_binary_prefix: Option<crate::format::SizeBinaryPrefix>,
    pub size_ndigits: Option<u32>,
    pub size_max_prefix: Option<u8>,
    pub size_space_before_unit: Option<crate::format::SpaceBeforeUnit>,
    /// `--freq-ndigits` (ffOptionParseInt32, cast to int8_t at the merge)
    /// / `--freq-space-before-unit`.
    pub freq_ndigits: Option<i32>,
    pub freq_space_before_unit: Option<crate::format::SpaceBeforeUnit>,
    /// `--temp-unit` / `--temp-ndigits` / the `--temp-color-*` trio /
    /// `--temp-space-before-unit`: the ffTempsAppendNum knobs (the
    /// ndigits cast truncates to uint8_t, no range check).
    pub temp_unit: Option<crate::format::TemperatureUnit>,
    pub temp_ndigits: Option<u32>,
    pub temp_color_green: Option<String>,
    pub temp_color_yellow: Option<String>,
    pub temp_color_red: Option<String>,
    pub temp_space_before_unit: Option<crate::format::SpaceBeforeUnit>,
    /// `--percent-type` (uint8 truncating cast) / `--percent-ndigits` /
    /// the `--percent-color-*` trio / `--percent-space-before-unit` /
    /// `--percent-width`: the ffPercentAppendNum knobs.
    pub percent_type: Option<u32>,
    pub percent_ndigits: Option<u32>,
    pub percent_color_green: Option<String>,
    pub percent_color_yellow: Option<String>,
    pub percent_color_red: Option<String>,
    pub percent_space_before_unit: Option<crate::format::SpaceBeforeUnit>,
    pub percent_width: Option<u32>,
    /// `--fraction-ndigits` (ffOptionParseInt32, cast to int8_t at the
    /// merge) / `--fraction-trailing-zeros`: the ffStrbufAppendDouble
    /// knobs (localip's speed is the only consumer).
    pub fraction_ndigits: Option<i32>,
    pub fraction_trailing_zeros: Option<crate::format::FractionTrailingZeros>,
    /// The `--bar-*` strings, width and colors of ffPercentAppendBar;
    /// `--bar-color-elapsed` keeps the literal "auto" (the band-selection
    /// mode) verbatim.
    pub bar_char_elapsed: Option<String>,
    pub bar_char_total: Option<String>,
    pub bar_border_left: Option<String>,
    pub bar_border_right: Option<String>,
    pub bar_border_left_elapsed: Option<String>,
    pub bar_border_right_elapsed: Option<String>,
    pub bar_color_elapsed: Option<String>,
    pub bar_color_total: Option<String>,
    pub bar_color_border: Option<String>,
    pub bar_width: Option<u32>,
    /// `--bright-color <?bool>`: the bolt ahead of the key/title colors.
    pub bright_color: Option<bool>,
    /// The config document loaded during the command pass: upstream
    /// processes `-c` before the option pass, so its errors (413/414)
    /// fire in argv order ahead of unknown-option errors.
    pub config: Option<Config>,
    /// Whether `-c` consumed a config, gating the automatic config search.
    pub config_loaded: bool,
}

impl Cli {
    pub fn parse() -> Cli {
        let argv: Vec<String> = std::env::args().collect();
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        Self::parse_from(&refs)
    }

    /// Walks argv twice, like upstream: the command pass exits on the
    /// informative options and loads `-c`, then the option pass parses
    /// the rest and rejects unknown keys. Both passes reject positional
    /// tokens and skip consumed values.
    pub fn parse_from(argv: &[&str]) -> Cli {
        let mut handled = vec![false; argv.len()];
        let mut gen_config_path: Option<String> = None;
        let mut structure_disabled: Option<String> = None;
        let mut cli = Self {
            structure: None,
            logo_none: false,
            logo_small: false,
            logo_source: None,
            logo_position: None,
            logo_width: None,
            logo_preserve_aspect_ratio: None,
            logo_height: None,
            logo_padding_top: None,
            logo_padding_bottom: None,
            logo_padding_left: None,
            logo_padding_right: None,
            logo_print_remaining: None,
            logo_type: None,
            logo_animation_frame: None,
            logo_colors: Vec::new(),
            hyfetch_preset: None,
            hyfetch_mode: None,
            hyfetch_lightness: None,
            logo_spin_speed_per_frame: None,
            logo_spin_speed_per_round: None,
            logo_spin_tilt: None,
            logo_spin_angle: None,
            show_errors: None,
            stat: None,
            player_name: None,
            disable_linewrap: None,
            hide_cursor: None,
            key_value_separator: None,
            color_keys: None,
            color_title: None,
            color_output: None,
            color_separator: None,
            key_width: None,
            key_padding_left: None,
            key_type: None,
            key_language: None,
            duration_abbreviation: None,
            duration_space_before_unit: None,
            size_binary_prefix: None,
            size_ndigits: None,
            size_max_prefix: None,
            size_space_before_unit: None,
            freq_ndigits: None,
            freq_space_before_unit: None,
            temp_unit: None,
            temp_ndigits: None,
            temp_color_green: None,
            temp_color_yellow: None,
            temp_color_red: None,
            temp_space_before_unit: None,
            percent_type: None,
            percent_ndigits: None,
            percent_color_green: None,
            percent_color_yellow: None,
            percent_color_red: None,
            percent_space_before_unit: None,
            percent_width: None,
            fraction_ndigits: None,
            fraction_trailing_zeros: None,
            bar_char_elapsed: None,
            bar_char_total: None,
            bar_border_left: None,
            bar_border_right: None,
            bar_border_left_elapsed: None,
            bar_border_right_elapsed: None,
            bar_color_elapsed: None,
            bar_color_total: None,
            bar_color_border: None,
            bar_width: None,
            bright_color: None,
            pipe: None,
            watch_interval: 0,
            format_json: false,
            config: None,
            config_loaded: false,
        };

        // Pass 1: parseCommand. Unhandled keys stay for the option pass.
        let mut i = 1;
        while i < argv.len() {
            let key = argv[i];
            if key.is_empty() {
                i += 1;
                continue;
            }
            if !key.starts_with('-') {
                invalid_option(key);
            }
            let value = next_value(argv, i);
            match key.to_ascii_lowercase().as_str() {
                "-h" | "--help" => print_help(value),
                "-v" | "--version" => {
                    // The self-identification is rustfetch's own; set
                    // RUSTFETCH_PARITY_VERSION to emit the pinned upstream
                    // string for byte-for-byte parity suites.
                    if std::env::var_os("RUSTFETCH_PARITY_VERSION").is_some() {
                        println!("fastfetch 2.71.0 (x86_64)");
                    } else {
                        println!("rustfetch {} (x86_64)", env!("CARGO_PKG_VERSION"));
                    }
                    std::process::exit(0);
                },
                "--version-raw" => {
                    if std::env::var_os("RUSTFETCH_PARITY_VERSION").is_some() {
                        println!("2.71.0");
                    } else {
                        println!("{}", env!("CARGO_PKG_VERSION"));
                    }
                    std::process::exit(0);
                },
                "--list-modules" => {
                    // pretty unless the value is "autocompletion"
                    crate::modules::list_modules(!value.is_some_and(|value| value.eq_ignore_ascii_case("autocompletion")));
                    std::process::exit(0);
                },
                "--list-presets" => {
                    crate::config::list_presets(!value.is_some_and(|value| value.eq_ignore_ascii_case("autocompletion")));
                    std::process::exit(0);
                },
                "--list-config-paths" => {
                    crate::config::list_config_paths();
                    std::process::exit(0);
                },
                "--list-data-paths" => {
                    crate::config::list_data_paths();
                    std::process::exit(0);
                },
                "--list-features" => {
                    print!("{LIST_FEATURES}");
                    std::process::exit(0);
                },
                "--list-logos" => {
                    match value {
                        Some(value) if value.eq_ignore_ascii_case("autocompletion") => {
                            crate::logo::list_logos_builtin(1, true);
                        },
                        Some(value) if value.eq_ignore_ascii_case("builtin") => {
                            crate::logo::list_logos_builtin(1, false);
                        },
                        Some(value) if value.eq_ignore_ascii_case("custom") => {
                            crate::logo::list_logos_custom();
                        },
                        Some(value) => {
                            eprintln!("Error: unsupported logo type: {value}");
                            std::process::exit(415);
                        },
                        None => {
                            println!("Builtin logos:");
                            crate::logo::list_logos_builtin(1, false);
                            println!("\nCustom logos:");
                            crate::logo::list_logos_custom();
                        },
                    }
                    std::process::exit(0);
                },
                "--help-raw" => {
                    print!("{HELP_RAW}");
                    std::process::exit(0);
                },
                key if key.to_ascii_lowercase().starts_with("--print-") => {
                    let subkey = &key["--print-".len()..];
                    if subkey.to_ascii_lowercase().ends_with("structure") {
                        println!("{}", crate::modules::DEFAULT_STRUCTURE_TEXT);
                    } else if subkey.eq_ignore_ascii_case("logos") {
                        // The option struct holds whatever was parsed up to
                        // this argv position (config keys via -c included).
                        let cfg = cli.config.as_ref();
                        let opts = crate::logo::LogoOptions {
                            padding_top: cli.logo_padding_top.or(cfg.and_then(|config| config.padding_top)).unwrap_or(0),
                            padding_bottom: cli.logo_padding_bottom.or(cfg.and_then(|config| config.padding_bottom)).unwrap_or(0),
                            padding_left: cli.logo_padding_left.or(cfg.and_then(|config| config.padding_left)).unwrap_or(0),
                            padding_right: cli.logo_padding_right.or(cfg.and_then(|config| config.padding_right)).unwrap_or(4),
                            width: cli.logo_width.or(cfg.and_then(|config| config.logo_width)).unwrap_or(0),
                            height: cli.logo_height.or(cfg.and_then(|config| config.logo_height)).unwrap_or(0),
                            print_remaining: true,
                            ..Default::default()
                        };
                        crate::logo::print_logos(&opts);
                    } else {
                        eprintln!("Error: unsupported print option: {key}");
                        std::process::exit(415);
                    }
                    std::process::exit(0);
                },
                "-c" | "--config" => {
                    let Some(value) = value else {
                        usage_exit(key, "<config>", 413);
                    };
                    let config = config::load_explicit(value, &mut cli.config_loaded);
                    // logo: null / logo.type: "small" set the type at
                    // doc-load time, so a later `-l arch` only fills the
                    // source of a hidden or small logo.
                    if config.logo_none {
                        cli.logo_none = true;
                    }
                    if config.logo_small {
                        cli.logo_small = true;
                    }
                    if config.logo_position.is_some() {
                        cli.logo_position = config.logo_position;
                    }
                    cli.config = Some(config);
                    handled[i] = true;
                    if value_was_consumed(argv, i) {
                        handled[i + 1] = true;
                    }
                },
                "--gen-config" => {
                    // fastfetch.c's genConfigCommon/generateConfigFile:
                    // a second --gen-config or a previously enabled JSON
                    // document both hit the duplicate-flag error.
                    if gen_config_path.is_some() || cli.format_json {
                        gen_config_duplicate();
                    }
                    gen_config_path = Some(match value {
                        Some(value) => value.to_string(),
                        None => default_gen_config_path(),
                    });
                    handled[i] = true;
                    if value_was_consumed(argv, i) {
                        handled[i + 1] = true;
                    }
                },
                "-j" | "--json" => {
                    // enableJsonOutput only: `-j false` is a no-op.
                    if parse_boolean(value) {
                        if gen_config_path.is_some() {
                            gen_config_duplicate();
                        }
                        cli.format_json = true;
                    }
                    handled[i] = true;
                    if value_was_consumed(argv, i) {
                        handled[i + 1] = true;
                    }
                },
                "--format" => {
                    if format_enum(key, value) {
                        if gen_config_path.is_some() {
                            gen_config_duplicate();
                        }
                        cli.format_json = true;
                    }
                    handled[i] = true;
                    if value_was_consumed(argv, i) {
                        handled[i + 1] = true;
                    }
                },
                // parseCommand's watch options; upstream blanks the key so
                // parseOption never sees them again.
                "--dynamic-interval" => {
                    cli.watch_interval = parse_uint32(key, value);
                    handled[i] = true;
                    if value_was_consumed(argv, i) {
                        handled[i + 1] = true;
                    }
                },
                "-w" | "--watch" => {
                    cli.watch_interval = match value {
                        None => 1000,
                        Some(seconds) => parse_uint32(key, Some(seconds)).wrapping_mul(1000),
                    };
                    handled[i] = true;
                    if value_was_consumed(argv, i) {
                        handled[i + 1] = true;
                    }
                },
                key if key.to_ascii_lowercase().starts_with("--print-") => {
                    eprintln!("Error: unsupported print option: {key}");
                    std::process::exit(415);
                },
                key if key.to_ascii_lowercase().starts_with("--list-") => {
                    eprintln!("Error: unsupported list option: {key}");
                    std::process::exit(415);
                },
                _ => {},
            }
            if value_was_consumed(argv, i) {
                i += 1;
            }
            i += 1;
        }

        // fastfetch.c:924 — the automatic config search runs between the
        // two argv passes, so the option pass sees the config's colors
        // and the PROP color words resolve against them (NO_CONFIG and
        // the already-loaded gate are checked inside).
        // parseConfigFiles (fastfetch.c:737) returns immediately when
        // data->genConfigPath is set: config generation never auto-loads a
        // config from the config dirs, even though an explicit -c in the
        // command pass has already been parsed.
        if cli.config.is_none() && !gen_config_path.as_deref().is_some_and(|path| !path.is_empty()) {
            cli.config = config::load_automatic(cli.config_loaded);
        }

        // The display colors the option pass parses against: upstream's
        // one FFOptionsDisplay, seeded by the config walk and mutated in
        // argv order by the --color*/--bright-color/--logo-color-N arms.
        let mut state = ColorState::from_config(cli.config.as_ref());

        // Pass 2: parseOption.
        let mut i = 1;
        while i < argv.len() {
            let key = argv[i];
            if key.is_empty() || handled[i] {
                i += 1;
                continue;
            }
            if !key.starts_with('-') {
                invalid_option(key);
            }
            let value = next_value(argv, i);
            match key.to_ascii_lowercase().as_str() {
                "-s" | "--structure" => cli.structure = Some(parse_string(key, value)),
                "--structure-disabled" => structure_disabled = Some(parse_string(key, value)),
                "-l" | "--logo" => {
                    let Some(value) = value else {
                        usage_exit(key, "<none|small|logo-source>", 477);
                    };
                    if value.eq_ignore_ascii_case("none") {
                        cli.logo_none = true;
                    } else if value.eq_ignore_ascii_case("small") {
                        cli.logo_small = true;
                    } else {
                        cli.logo_source = Some(value.to_string());
                    }
                },
                "--pipe" => cli.pipe = Some(parse_boolean(value)),
                "--no-buffer" => {
                    // Upstream calls setvbuf(stdout, NULL, _IONBF, 0). Rust's
                    // stdout is a LineWriter, so it already flushes on every
                    // newline and again at process exit; the rendered output
                    // is byte-identical in --pipe mode. Fully unbuffered
                    // behaviour would need a per-print flush hook on the
                    // render path, which lives outside this file, so here we
                    // only accept the flag with the shared optional-bool
                    // parse and flush anything currently pending.
                    let _disabled = parse_boolean(value);
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                },
                "--logo-position" => cli.logo_position = Some(position_enum(key, value)),
                "--logo-width" => cli.logo_width = Some(parse_uint32(key, value)),
                "--logo-height" => cli.logo_height = Some(parse_uint32(key, value)),
                "--logo-padding" => {
                    let padding = parse_uint32(key, value);
                    cli.logo_padding_left = Some(padding);
                    cli.logo_padding_right = Some(padding);
                },
                "--logo-padding-top" => cli.logo_padding_top = Some(parse_uint32(key, value)),
                "--logo-padding-bottom" => cli.logo_padding_bottom = Some(parse_uint32(key, value)),
                "--logo-padding-left" => cli.logo_padding_left = Some(parse_uint32(key, value)),
                "--logo-padding-right" => cli.logo_padding_right = Some(parse_uint32(key, value)),
                "--logo-print-remaining" => cli.logo_print_remaining = Some(parse_boolean(value)),
                key if {
                    let lower = key.to_ascii_lowercase();
                    lower.starts_with("--logo-color-") && lower.len() == 14
                } => {
                    // options/logo.c:152 — the digit at key[13] names the
                    // palette slot; anything outside 1..=9 exits 472.
                    let digit = key.as_bytes()[13] as char;
                    let index = digit.to_digit(10).unwrap_or(0) as i32 - 1;
                    if !(0..9).contains(&index) {
                        eprintln!("Error: invalid --color-[1-9] index: {digit}");
                        std::process::exit(472);
                    }
                    let Some(value) = value else {
                        usage_exit(key, "<str>", 477);
                    };
                    let parsed = state.parse_color(value);
                    if !parsed.is_empty() {
                        cli.logo_colors.push((index as usize, parsed.leak()));
                    }
                },
                "--hyfetch-preset" => cli.hyfetch_preset = Some(parse_string(key, value)),
                "--hyfetch-mode" => cli.hyfetch_mode = Some(hyfetch_mode_enum(key, value)),
                "--hyfetch-lightness" => cli.hyfetch_lightness = Some(hyfetch_lightness(key, value)),
                "--logo-type" => cli.logo_type = Some(logo_type_enum(key, value)),
                "--file" => {
                    cli.logo_source = Some(parse_string(key, value));
                    cli.logo_type = Some(crate::logo::LogoTypeOpt::File);
                },
                "--file-raw" => {
                    cli.logo_source = Some(parse_string(key, value));
                    cli.logo_type = Some(crate::logo::LogoTypeOpt::FileRaw);
                },
                "--data" => {
                    cli.logo_source = Some(parse_string(key, value));
                    cli.logo_type = Some(crate::logo::LogoTypeOpt::Data);
                },
                "--data-raw" => {
                    cli.logo_source = Some(parse_string(key, value));
                    cli.logo_type = Some(crate::logo::LogoTypeOpt::DataRaw);
                },
                "--raw" => {
                    cli.logo_source = Some(parse_string(key, value));
                    cli.logo_type = Some(crate::logo::LogoTypeOpt::ImageRaw);
                },
                "--sixel" => {
                    cli.logo_source = Some(parse_string(key, value));
                    cli.logo_type = Some(crate::logo::LogoTypeOpt::ImageSixel);
                },
                "--kitty" => {
                    cli.logo_source = Some(parse_string(key, value));
                    cli.logo_type = Some(crate::logo::LogoTypeOpt::ImageKitty);
                },
                "--kitty-direct" => {
                    cli.logo_source = Some(parse_string(key, value));
                    cli.logo_type = Some(crate::logo::LogoTypeOpt::ImageKittyDirect);
                },
                "--kitty-icat" => {
                    cli.logo_source = Some(parse_string(key, value));
                    cli.logo_type = Some(crate::logo::LogoTypeOpt::ImageKittyIcat);
                },
                "--iterm" => {
                    cli.logo_source = Some(parse_string(key, value));
                    cli.logo_type = Some(crate::logo::LogoTypeOpt::ImageIterm);
                },
                "--chafa" => {
                    cli.logo_source = Some(parse_string(key, value));
                    cli.logo_type = Some(crate::logo::LogoTypeOpt::ImageChafa);
                },
                "--chafa-fg-only" => {
                    parse_boolean(value);
                },
                "--chafa-symbols" => {
                    parse_string(key, value);
                },
                "--chafa-canvas-mode" => {
                    enum_pairs(key, value, &[
                        "TRUECOLOR", "INDEXED_256", "INDEXED_240", "INDEXED_16",
                        "FGBG_BGFG", "FGBG", "INDEXED_8", "INDEXED_16_8",
                    ]);
                },
                "--chafa-color-space" => {
                    enum_pairs(key, value, &["RGB", "DIN99D"]);
                },
                "--chafa-dither-mode" => {
                    enum_pairs(key, value, &["NONE", "ORDERED", "DIFFUSION"]);
                },
                "--logo-cache" => {
                    // FF_LOGO_CACHE_ON/OFF/REGEN: this port caches nothing.
                    if let Some(v) = value && v.eq_ignore_ascii_case("regen") { } else { parse_boolean(value); }
                },
                "--logo-recache" => {
                    eprintln!("--logo-recache has been replaced by --logo-cache regen");
                    std::process::exit(477);
                },
                "--logo-separate" => {
                    eprintln!("--logo-separate has been renamed to --logo-position");
                    std::process::exit(477);
                },
                "--logo-preserve-aspect-ratio" => {
                    cli.logo_preserve_aspect_ratio = Some(parse_boolean(value));
                },
                "--logo-animation-frame" => cli.logo_animation_frame = Some(parse_int32(key, value)),
                "--logo-spin-speed-per-frame" => {
                    cli.logo_spin_speed_per_frame = Some(parse_float(key, value, -360.0, 360.0));
                },
                "--logo-spin-speed-per-round" => {
                    cli.logo_spin_speed_per_round = Some(parse_float(key, value, -3600.0, 3600.0));
                },
                "--logo-spin-tilt" => {
                    cli.logo_spin_tilt = Some(parse_float(key, value, -80.0, 80.0));
                },
                "--logo-spin-angle" => {
                    cli.logo_spin_angle = Some(parse_float(key, value, 0.0, 360.0));
                },
                "--show-errors" => {
                    cli.show_errors = Some(parse_boolean(value));
                },
                "--player-name" => {
                    cli.player_name = Some(parse_string(key, value));
                },
                "--disable-linewrap" => {
                    cli.disable_linewrap = Some(parse_boolean(value));
                },
                "--hide-cursor" => {
                    cli.hide_cursor = Some(parse_boolean(value));
                },
                "--separator" => {
                    cli.key_value_separator = Some(parse_string(key, value));
                },
                "--color" => {
                    // optionCheckString + ffOptionParseColor: the parse
                    // replaces both slots at once.
                    let value = parse_string(key, value);
                    let parsed = state.parse_color(&value);
                    state.keys = parsed.clone();
                    state.title = parsed.clone();
                    cli.color_keys = Some(parsed.clone());
                    cli.color_title = Some(parsed);
                },
                key if key.to_ascii_lowercase().starts_with("--color-") => {
                    let subkey = &key.to_ascii_lowercase()["--color-".len()..];
                    match subkey {
                        "keys" => {
                            let value = parse_string(key, value);
                            let parsed = state.parse_color(&value);
                            state.keys = parsed.clone();
                            cli.color_keys = Some(parsed);
                        },
                        "title" => {
                            let value = parse_string(key, value);
                            let parsed = state.parse_color(&value);
                            state.title = parsed.clone();
                            cli.color_title = Some(parsed);
                        },
                        "output" => {
                            let value = parse_string(key, value);
                            let parsed = state.parse_color(&value);
                            state.output = parsed.clone();
                            cli.color_output = Some(parsed);
                        },
                        "separator" => {
                            let value = parse_string(key, value);
                            let parsed = state.parse_color(&value);
                            state.separator = parsed.clone();
                            cli.color_separator = Some(parsed);
                        },
                        // Unknown subkeys return false upstream, falling
                        // through to the module-option scan.
                        _ => {
                            if !parse_module_options(key, value) {
                                eprintln!("Error: unknown option: {key}");
                                std::process::exit(400);
                            }
                        },
                    }
                },
                key if key.to_ascii_lowercase().starts_with("--key-") => {
                    let subkey = &key.to_ascii_lowercase()["--key-".len()..];
                    match subkey {
                        "width" => cli.key_width = Some(parse_uint32(key, value) as u16),
                        "type" => cli.key_type = Some(key_type_enum(key, value)),
                        "padding-left" => cli.key_padding_left = Some(parse_uint32(key, value) as u16),
                        "language" => {
                            cli.key_language = Some(crate::config::parse_language_string(value.unwrap_or("")));
                        },
                        _ => {
                            if !parse_module_options(key, value) {
                                eprintln!("Error: unknown option: {key}");
                                std::process::exit(400);
                            }
                        },
                    }
                },
                "--bright-color" => {
                    let bright = parse_boolean(value);
                    state.bright = bright;
                    cli.bright_color = Some(bright);
                },
                key if key.to_ascii_lowercase().starts_with("--duration-") => {
                    let subkey = &key.to_ascii_lowercase()["--duration-".len()..];
                    match subkey {
                        "abbreviation" => cli.duration_abbreviation = Some(parse_boolean(value)),
                        "space-before-unit" => {
                            cli.duration_space_before_unit = Some(space_before_unit_enum(key, value));
                        },
                        _ => {
                            if !parse_module_options(key, value) {
                                eprintln!("Error: unknown option: {key}");
                                std::process::exit(400);
                            }
                        },
                    }
                },
                key if key.to_ascii_lowercase().starts_with("--size-") => {
                    let subkey = &key.to_ascii_lowercase()["--size-".len()..];
                    match subkey {
                        "binary-prefix" => {
                            cli.size_binary_prefix = Some(size_binary_prefix_enum(key, value));
                        },
                        "ndigits" => {
                            let num = parse_uint32(key, value);
                            if num > 9 {
                                eprintln!("Error: {key} must be between 0 and 9");
                                std::process::exit(479);
                            }
                            cli.size_ndigits = Some(num);
                        },
                        "max-prefix" => {
                            const PAIRS: &[(&str, i32)] = &[
                                ("B", 0), ("kB", 1), ("MB", 2), ("GB", 3), ("TB", 4),
                                ("PB", 5), ("EB", 6), ("ZB", 7), ("YB", 8),
                            ];
                            cli.size_max_prefix = Some(parse_enum(key, value, PAIRS) as u8);
                        },
                        "space-before-unit" => {
                            cli.size_space_before_unit = Some(space_before_unit_enum(key, value));
                        },
                        _ => {
                            if !parse_module_options(key, value) {
                                eprintln!("Error: unknown option: {key}");
                                std::process::exit(400);
                            }
                        },
                    }
                },
                key if key.to_ascii_lowercase().starts_with("--freq-") => {
                    let subkey = &key.to_ascii_lowercase()["--freq-".len()..];
                    match subkey {
                        "ndigits" => cli.freq_ndigits = Some(parse_int32(key, value)),
                        "space-before-unit" => {
                            cli.freq_space_before_unit = Some(space_before_unit_enum(key, value));
                        },
                        _ => {
                            if !parse_module_options(key, value) {
                                eprintln!("Error: unknown option: {key}");
                                std::process::exit(400);
                            }
                        },
                    }
                },
                key if key.to_ascii_lowercase().starts_with("--temp-") => {
                    let subkey = &key.to_ascii_lowercase()["--temp-".len()..];
                    match subkey {
                        "unit" => cli.temp_unit = Some(temp_unit_enum(key, value)),
                        "ndigits" => cli.temp_ndigits = Some(parse_uint32(key, value)),
                        "color-green" => {
                            cli.temp_color_green = Some(value.map_or_else(String::new, |v| state.parse_color(v)));
                        },
                        "color-yellow" => {
                            cli.temp_color_yellow = Some(value.map_or_else(String::new, |v| state.parse_color(v)));
                        },
                        "color-red" => {
                            cli.temp_color_red = Some(value.map_or_else(String::new, |v| state.parse_color(v)));
                        },
                        "space-before-unit" => {
                            cli.temp_space_before_unit = Some(space_before_unit_enum(key, value));
                        },
                        _ => {
                            if !parse_module_options(key, value) {
                                eprintln!("Error: unknown option: {key}");
                                std::process::exit(400);
                            }
                        },
                    }
                },
                key if key.eq_ignore_ascii_case("--fraction-ndigits") => {
                    cli.fraction_ndigits = Some(parse_int32(key, value));
                },
                key if key.eq_ignore_ascii_case("--fraction-trailing-zeros") => {
                    cli.fraction_trailing_zeros = Some(fraction_trailing_zeros_enum(key, value));
                },
                key if key.to_ascii_lowercase().starts_with("--percent-") => {
                    let subkey = &key.to_ascii_lowercase()["--percent-".len()..];
                    match subkey {
                        "type" => cli.percent_type = Some(parse_uint32(key, value)),
                        "ndigits" => cli.percent_ndigits = Some(parse_uint32(key, value)),
                        "color-green" => {
                            cli.percent_color_green = Some(value.map_or_else(String::new, |v| state.parse_color(v)));
                        },
                        "color-yellow" => {
                            cli.percent_color_yellow = Some(value.map_or_else(String::new, |v| state.parse_color(v)));
                        },
                        "color-red" => {
                            cli.percent_color_red = Some(value.map_or_else(String::new, |v| state.parse_color(v)));
                        },
                        "space-before-unit" => {
                            cli.percent_space_before_unit = Some(space_before_unit_enum(key, value));
                        },
                        "width" => cli.percent_width = Some(parse_uint32(key, value)),
                        _ => {
                            if !parse_module_options(key, value) {
                                eprintln!("Error: unknown option: {key}");
                                std::process::exit(400);
                            }
                        },
                    }
                },
                key if key.to_ascii_lowercase().starts_with("--bar-") => {
                    let subkey = &key.to_ascii_lowercase()["--bar-".len()..];
                    match subkey {
                        "char-elapsed" => cli.bar_char_elapsed = Some(parse_string(key, value)),
                        "char-total" => cli.bar_char_total = Some(parse_string(key, value)),
                        "width" => cli.bar_width = Some(parse_uint32(key, value)),
                        "border-left" => cli.bar_border_left = Some(parse_string(key, value)),
                        "border-right" => cli.bar_border_right = Some(parse_string(key, value)),
                        "border-left-elapsed" => cli.bar_border_left_elapsed = Some(parse_string(key, value)),
                        "border-right-elapsed" => cli.bar_border_right_elapsed = Some(parse_string(key, value)),
                        // A missing value clears; "auto" re-enters the
                        // band-selection mode; anything else parses.
                        "color-elapsed" => {
                            cli.bar_color_elapsed = Some(match value {
                                None => String::new(),
                                Some(v) if v.eq_ignore_ascii_case("auto") => "auto".to_string(),
                                Some(v) => state.parse_color(v),
                            });
                        },
                        "color-total" => {
                            cli.bar_color_total = Some(value.map_or_else(String::new, |v| state.parse_color(v)));
                        },
                        "color-border" => {
                            cli.bar_color_border = Some(value.map_or_else(String::new, |v| state.parse_color(v)));
                        },
                        _ => {
                            if !parse_module_options(key, value) {
                                eprintln!("Error: unknown option: {key}");
                                std::process::exit(400);
                            }
                        },
                    }
                },
                "--stat" => {
                    // A truthy value or a full strtoul number times the
                    // modules and implies showErrors; anything else (bare
                    // flag, "false", garbage) resets the timing to unset.
                    if parse_boolean(value) {
                        cli.stat = Some(0);
                        cli.show_errors = Some(true);
                    } else if let Some(num) = value.and_then(strtoul_full) {
                        cli.stat = Some(num as i32);
                        cli.show_errors = Some(true);
                    } else {
                        cli.stat = Some(-1);
                    }
                },
                _ => {
                    if !parse_module_options(key, value) {
                        eprintln!("Error: unknown option: {key}");
                        std::process::exit(400);
                    }
                },
            }
            if value_was_consumed(argv, i) {
                i += 1;
            }
            i += 1;
        }

        GEN_CONFIG_STATE.with(|state| {
            *state.borrow_mut() = GenConfigState {
                path: gen_config_path,
                structure_disabled,
            };
        });
        cli
    }
}

/// modules.c registry order, which ffParseModuleOptions' prefix scan and
/// ffMigrateCommandOptionToJsonc's exact-name lookup both follow.
const MODULES: &[&str] = &[
    "Battery", "BIOS", "Bluetooth", "BluetoothRadio", "Board", "Bootmgr", "Break",
    "Brightness", "Btrfs",
    "Camera", "Chassis", "Codec", "Command", "Colors", "CPU", "CPUCache", "CPUUsage",
    "Cursor", "Custom",
    "DateTime", "DE", "Display", "Disk", "DiskIO", "DNS",
    "Editor", "Font", "Gamepad", "GPU", "Host", "Icons", "InitSystem",
    "Kernel", "Keyboard",
    "LM", "Loadavg", "Locale", "LocalIp", "Logo",
    "Media", "Memory", "Monitor", "Mouse",
    "NetIO",
    "OpenCL", "OpenGL", "OS",
    "Packages", "PhysicalDisk", "PhysicalMemory", "Player", "PowerAdapter", "Processes",
    "PublicIp",
    "Separator", "Shell", "Sound", "Swap",
    "Terminal", "TerminalFont", "TerminalSize", "TerminalTheme", "Title", "Top", "Theme",
    "TPM",
    "Uptime", "Users",
    "Version", "Vulkan",
    "Wallpaper", "Weather", "WM", "Wifi", "WMTheme",
    "Zpool",
];

/// parseStructureCommand's exact, case-insensitive module-name match:
/// returns the registered spelling, which genJsonConfig lowercases.
pub(crate) fn canonical_module_name(token: &str) -> Option<&'static str> {
    MODULES.iter().copied().find(|name| name.eq_ignore_ascii_case(token))
}

/// commandoption.c ffParseModuleOptions: the per-module CLI options were
/// removed; a key whose prefix names a registered module is rejected with
/// the JSON-config migration hint instead of "unknown option".
fn parse_module_options(key: &str, value: Option<&str>) -> bool {
    let bytes = key.as_bytes();
    if !(key.starts_with("--") && bytes.get(2).is_some_and(|b| b.is_ascii_alphabetic())) {
        return false;
    }
    // An empty value token counts as absent in the example line.
    let value = value.filter(|v| !v.is_empty());
    let rest = &key[2..];
    for name in MODULES.iter().filter(|name| {
        name.as_bytes()[0] == bytes[2].to_ascii_uppercase()
    }) {
        // ffOptionTestPrefix: the module name must be a case-insensitive
        // prefix, followed by nothing or one '-'.
        if rest.len() < name.len() || !rest[..name.len()].eq_ignore_ascii_case(name) {
            continue;
        }
        let after = &rest[name.len()..];
        let sub_key = match after.as_bytes().first() {
            None => "",
            Some(b'-') => &after[1..],
            Some(_) => continue,
        };
        if sub_key.is_empty() || sub_key.starts_with('-') {
            eprintln!("Error: unknown module key {key}");
            std::process::exit(477);
        }
        let mut json_key = String::new();
        let mut flag = false;
        for &b in sub_key.as_bytes() {
            if b == b'-' {
                if flag {
                    eprintln!("Error: invalid double `-` in module key {key}");
                    std::process::exit(477);
                }
                flag = true;
            } else {
                if !b.is_ascii_alphanumeric() {
                    use std::io::Write;
                    let mut err = std::io::stderr().lock();
                    let _ = write!(err, "Error: invalid character `");
                    let _ = err.write_all(&[b]);
                    let _ = writeln!(err, "` in module key {key}");
                    std::process::exit(477);
                }
                if flag {
                    flag = false;
                    json_key.push(b.to_ascii_uppercase() as char);
                } else {
                    json_key.push(b as char);
                }
            }
        }
        let module_name = name.to_ascii_lowercase();
        let (open, close) = if value.is_some() { ("\"", "\"") } else { ("", "") };
        eprintln!("Error: Unsupported module option: {key}");
        eprintln!("       Support of module options has been removed. Please add the flag to the JSON config instead.");
        eprintln!(
            "       Example (demonstration only): `{{ \"modules\": [ {{ \"type\": \"{module_name}\", \"{json_key}\": {}{}{} }} ] }}`",
            open,
            value.unwrap_or("true"),
            close,
        );
        eprintln!("       See <https://github.com/fastfetch-cli/fastfetch/wiki/Configuration> for more information.");
        std::process::exit(477);
    }
    false
}

/// upstream parseArguments: the next token is a value unless it is
/// missing, starts with `-` (`-` alone aliases /dev/stdin), or is a
/// negative number (`--logo-spin-tilt -30`).
fn next_value<'a>(argv: &[&'a str], i: usize) -> Option<&'a str> {
    let next = *argv.get(i + 1)?;
    if next.len() > 1 && next.starts_with('-') && !is_negative_number(next) {
        None
    } else {
        Some(next)
    }
}

/// Whether the token after argv[i] is consumed as a value by the walk.
fn value_was_consumed(argv: &[&str], i: usize) -> bool {
    next_value(argv, i).is_some()
}

/// `-30`, `-.5`: a negative number is the value of the previous option,
/// not the name of the next one.
fn is_negative_number(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.first() == Some(&b'-')
        && (bytes.get(1).is_some_and(|c| c.is_ascii_digit())
            || (bytes.get(1) == Some(&b'.') && bytes.get(2).is_some_and(|c| c.is_ascii_digit())))
}

/// ffOptionParseBoolean: absent or empty → true; case-insensitive
/// true/yes/on/1 → true; anything else → false, silently.
fn parse_boolean(value: Option<&str>) -> bool {
    match value {
        None => true,
        Some(value) => {
            value.is_empty()
                || matches!(value.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1")
        },
    }
}

fn usage_exit(key: &str, message: &str, code: i32) -> ! {
    eprintln!("Error: usage: {key} {message}");
    std::process::exit(code)
}

/// fastfetch.c's "duplicated `--gen-config` or `--format json` flags"
/// (exit(477), i.e. 221 in the shell).
fn gen_config_duplicate() -> ! {
    eprintln!("Error: duplicated `--gen-config` or `--format json` flags found");
    std::process::exit(477)
}

/// setupGenConfigPath with no path: configDirs[0] + "fastfetch/config.jsonc".
fn default_gen_config_path() -> String {
    match crate::config::config_dirs().first() {
        Some(dir) => format!("{dir}fastfetch/config.jsonc"),
        None => {
            eprintln!("Error: No config directory found to generate config file in. Use --gen-config <path> to specify a path");
            std::process::exit(477)
        },
    }
}

/// ffOptionParseString: the `-s` value.
fn parse_string(key: &str, value: Option<&str>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => usage_exit(key, "<str>", 477),
    }
}

/// --logo-type's ffOptionParseEnum pairs (options/logo.c:125).
fn logo_type_enum(key: &str, value: Option<&str>) -> crate::logo::LogoTypeOpt {
    use crate::logo::LogoTypeOpt as T;
    let Some(value) = value else {
        usage_exit(key, "<value>", 476);
    };
    let value = value.to_ascii_lowercase();
    match value.as_str() {
        "auto" => T::Auto,
        "builtin" => T::Builtin,
        "small" => T::Small,
        "file" => T::File,
        "file-raw" => T::FileRaw,
        "data" => T::Data,
        "data-raw" => T::DataRaw,
        "command-raw" => T::CommandRaw,
        "sixel" => T::ImageSixel,
        "kitty" => T::ImageKitty,
        "kitty-direct" => T::ImageKittyDirect,
        "kitty-icat" => T::ImageKittyIcat,
        "iterm" => T::ImageIterm,
        "chafa" => T::ImageChafa,
        "raw" => T::ImageRaw,
        "spin" => T::Spin,
        "none" => T::None,
        _ => {
            eprintln!("Error: unknown {key} value: {value}");
            std::process::exit(478)
        },
    }
}

/// ffOptionParseEnum over a name list, used for the parse-only chafa
/// suboptions: a missing value exits 476, an unknown name 478, and the
/// matched index is dropped (no renderer consumes it yet).
fn enum_pairs(key: &str, value: Option<&str>, names: &[&str]) {
    let Some(value) = value else {
        usage_exit(key, "<value>", 476);
    };
    if names.iter().any(|name| name.eq_ignore_ascii_case(value)) {
        return;
    }
    eprintln!("Error: unknown {key} value: {value}");
    std::process::exit(478)
}

/// --key-type's ffOptionParseEnum pairs (display.c:749): FF_MODULE_KEY_TYPE
/// values, `both` aliasing BOTH_1.
/// ffOptionParseEnum: case-insensitive pair match; a missing value exits
/// 476, an unknown one 478.
fn parse_enum(key: &str, value: Option<&str>, pairs: &[(&str, i32)]) -> i32 {
    let Some(value) = value else {
        usage_exit(key, "<value>", 476);
    };
    if let Some((_, code)) = pairs.iter().find(|(name, _)| value.eq_ignore_ascii_case(name)) {
        return *code;
    }
    eprintln!("Error: unknown {key} value: {value}");
    std::process::exit(478)
}

fn space_before_unit_enum(key: &str, value: Option<&str>) -> crate::format::SpaceBeforeUnit {
    const PAIRS: &[(&str, i32)] = &[("default", 0), ("always", 1), ("never", 2)];
    match parse_enum(key, value, PAIRS) {
        1 => crate::format::SpaceBeforeUnit::Always,
        2 => crate::format::SpaceBeforeUnit::Never,
        _ => crate::format::SpaceBeforeUnit::Default,
    }
}

fn size_binary_prefix_enum(key: &str, value: Option<&str>) -> crate::format::SizeBinaryPrefix {
    const PAIRS: &[(&str, i32)] = &[("iec", 0), ("si", 1), ("jedec", 2)];
    match parse_enum(key, value, PAIRS) {
        1 => crate::format::SizeBinaryPrefix::Si,
        2 => crate::format::SizeBinaryPrefix::Jedec,
        _ => crate::format::SizeBinaryPrefix::Iec,
    }
}

/// ffOptionParseEnum for --fraction-trailing-zeros.
fn fraction_trailing_zeros_enum(key: &str, value: Option<&str>) -> crate::format::FractionTrailingZeros {
    const PAIRS: &[(&str, i32)] =
        &[("default", 0), ("always", 1), ("never", 2)];
    match parse_enum(key, value, PAIRS) {
        1 => crate::format::FractionTrailingZeros::Always,
        2 => crate::format::FractionTrailingZeros::Never,
        _ => crate::format::FractionTrailingZeros::Default,
    }
}

fn temp_unit_enum(key: &str, value: Option<&str>) -> crate::format::TemperatureUnit {
    const PAIRS: &[(&str, i32)] = &[
        ("DEFAULT", 0),
        ("D", 0),
        ("CELSIUS", 1),
        ("C", 1),
        ("FAHRENHEIT", 2),
        ("F", 2),
        ("KELVIN", 3),
        ("K", 3),
    ];
    match parse_enum(key, value, PAIRS) {
        1 => crate::format::TemperatureUnit::Celsius,
        2 => crate::format::TemperatureUnit::Fahrenheit,
        3 => crate::format::TemperatureUnit::Kelvin,
        _ => crate::format::TemperatureUnit::Default,
    }
}

fn key_type_enum(key: &str, value: Option<&str>) -> u8 {    const PAIRS: &[(&str, u8)] = &[
        ("none", crate::instance::KEY_TYPE_NONE),
        ("string", crate::instance::KEY_TYPE_STRING),
        ("icon", crate::instance::KEY_TYPE_ICON),
        ("both", 3 | 1 << 4),
        ("both-0", 3),
        ("both-1", 3 | 1 << 4),
        ("both-2", 3 | 2 << 4),
        ("both-3", 3 | 3 << 4),
        ("both-4", 3 | 4 << 4),
    ];
    let Some(value) = value else {
        usage_exit(key, "<value>", 476);
    };
    if let Some((_, code)) = PAIRS.iter().find(|(name, _)| value.eq_ignore_ascii_case(name)) {
        return *code;
    }
    eprintln!("Error: unknown {key} value: {value}");
    std::process::exit(478)
}

/// ffOptionParseInt32: strtol base 10 with the strict tail check — a
/// missing value exits 480, trailing junk or an out-of-range value 479.
fn parse_int32(key: &str, value: Option<&str>) -> i32 {
    let Some(value) = value else {
        usage_exit(key, "<num>", 480);
    };
    let invalid = || -> ! {
        eprintln!("Error: usage: {key} <num>");
        std::process::exit(479)
    };
    let text = value.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let digits = text
        .strip_prefix(['+', '-'])
        .unwrap_or(text)
        .len()
        .saturating_sub(
            text.strip_prefix(['+', '-'])
                .unwrap_or(text)
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .len(),
        );
    if digits == 0 || digits != text.strip_prefix(['+', '-']).unwrap_or(text).len() {
        invalid();
    }
    match text.parse::<i64>() {
        Ok(num) if (i32::MIN as i64..=i32::MAX as i64).contains(&num) => num as i32,
        _ => invalid(),
    }
}

/// logoParseFloat: strtod with the strict tail check and an inclusive
/// range; a missing value prints the usage form, a bad value the bounds.
/// logoParseFloat (options/logo.c:28): strtod with a range. NaN passes the
/// `<`/`>` checks like upstream; the value returns for the spin options to
/// store. (glibc's hex-float literals are not emulated.)
fn parse_float(key: &str, value: Option<&str>, min: f64, max: f64) -> f32 {
    let Some(value) = value else {
        usage_exit(key, "<number>", 477);
    };
    let parsed: f64 = match value.trim().parse() {
        Ok(num) if !value.trim().is_empty() => num,
        _ => {
            eprintln!("Error: {key} must be a number between {min} and {max}");
            std::process::exit(477)
        },
    };
    if parsed < min || parsed > max {
        eprintln!("Error: {key} must be a number between {min} and {max}");
        std::process::exit(477)
    }
    parsed as f32
}

/// strtoul base 10 with full consumption (upstream's `*end == '\0'`):
/// optional C whitespace and sign, then digits; ERANGE saturates. Returns
/// the value truncated to u32 (the negative wrap included), like
/// `(uint32_t) strtoul(...)`.
fn strtoul_full(value: &str) -> Option<u32> {
    let trimmed = value.trim_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let magnitude: u64 = digits.parse().unwrap_or(u64::MAX); // ERANGE saturates
    let raw = if negative { magnitude.wrapping_neg() } else { magnitude };
    Some(raw as u32)
}

/// The display colors the PROP color words (`keys`, `title`, `output`,
/// `separator`) resolve against: upstream parses every color option
/// against the one live FFOptionsDisplay, which the config walk (logo,
/// then display) fills before the command line mutates it in argv order.
pub(crate) struct ColorState {
    pub(crate) keys: String,
    pub(crate) title: String,
    pub(crate) output: String,
    pub(crate) separator: String,
    pub(crate) bright: bool,
}

impl ColorState {
    /// ffOptionsInitDisplay's parse-time state: empty slots, bright on.
    pub(crate) fn fresh() -> Self {
        Self {
            keys: String::new(),
            title: String::new(),
            output: String::new(),
            separator: String::new(),
            bright: true,
        }
    }

    /// The state after the config walk: the config's parsed color slots
    /// (None = untouched defaults).
    pub(crate) fn from_config(config: Option<&Config>) -> Self {
        let Some(config) = config else {
            return Self::fresh();
        };
        Self {
            keys: config.color_keys.clone().unwrap_or_default(),
            title: config.color_title.clone().unwrap_or_default(),
            output: config.color_output.clone().unwrap_or_default(),
            separator: config.color_separator.clone().unwrap_or_default(),
            bright: config.bright_color.unwrap_or(true),
        }
    }

    /// Module-time parses (separator.c's outputColor) resolve against the
    /// final display state, like upstream's option-time struct reads.
    pub(crate) fn from_instance(instance: &crate::instance::Instance) -> Self {
        Self {
            keys: instance.display.color_keys.clone(),
            title: instance.display.color_title.clone(),
            output: instance.display.color_output.clone(),
            separator: instance.display.color_separator.clone(),
            bright: instance.display.bright_color,
        }
    }

    /// ffOptionParseColorNoClear: the SGR parameter string a color slot
    /// holds. ANSI escapes pass through with every trailing `m` trimmed,
    /// named colors and modifiers prefix-match case-insensitively and
    /// accumulate, @256 and #RGB run through strtoul (C-whitespace skip,
    /// sign, ERANGE clamp, u32 truncation before the checks), an unmatched
    /// letter rejects with the unconsumed tail, and any other character is
    /// appended verbatim — plain digits therefore survive untouched. The
    /// display-property words read the state's current slot, prefixed
    /// `1;` when the bright bolt applies.
    pub(crate) fn parse_color(&self, value: &str) -> String {
        self.parse_color_impl(value, None)
    }

    /// The common-args parse variant: an invalid code does not abort on
    /// the spot — it records the exit(479) and its stderr message in
    /// `pending`, so collect() can stop there and the renderer lets the
    /// rows before the failing module print before the message lands
    /// (upstream parses lazily at each module's print).
    pub(crate) fn parse_color_deferred(
        &self,
        value: &str,
        pending: &std::cell::RefCell<Option<(i32, String)>>,
    ) -> String {
        self.parse_color_impl(value, Some(pending))
    }

    fn parse_color_impl(
        &self,
        value: &str,
        pending: Option<&std::cell::RefCell<Option<(i32, String)>>>,
    ) -> String {
        let fail = |message: String| -> String {
            match pending {
                Some(cell) => {
                    // First failure wins: upstream exits at the first
                    // invalid code, so only its message can ever print.
                    let mut slot = cell.borrow_mut();
                    if slot.is_none() {
                        *slot = Some((479, message.clone()));
                    }
                },
                None => {
                    eprintln!("{message}");
                    std::process::exit(479);
                },
            }
            String::new()
        };
        if value.is_empty() {
            return String::new();
        }
        if value.starts_with('\x1b') && value.len() > 1 && value.as_bytes()[1] == b'[' {
            return value[2..].trim_end_matches('m').to_string();
        }
        const NAMED: &[(&str, &str)] = &[
            ("reset_", "0;"),
            ("bold_", "1;"),
            ("bright_", "1;"),
            ("dim_", "2;"),
            ("italic_", "3;"),
            ("underline_", "4;"),
            ("blink_", "5;"),
            ("inverse_", "7;"),
            ("hidden_", "8;"),
            ("strike_", "9;"),
            ("black", "30"),
            ("red", "31"),
            ("green", "32"),
            ("yellow", "33"),
            ("blue", "34"),
            ("magenta", "35"),
            ("cyan", "36"),
            ("white", "37"),
            ("default", "39"),
            ("light_black", "90"),
            ("light_red", "91"),
            ("light_green", "92"),
            ("light_yellow", "93"),
            ("light_blue", "94"),
            ("light_magenta", "95"),
            ("light_cyan", "96"),
            ("light_white", "97"),
        ];
        let mut out = String::new();
        let mut rest = value;
        while !rest.is_empty() {
            if rest.as_bytes()[0].is_ascii_alphabetic() {
                if let Some((code, consumed)) = NAMED
                    .iter()
                    .find(|(name, _)| rest.len() >= name.len() && rest[..name.len()].eq_ignore_ascii_case(name))
                    .map(|(name, code)| (*code, name.len()))
                {
                    out.push_str(code);
                    rest = &rest[consumed..];
                    continue;
                }
                // The display color properties: the word resolves against the
                // state's current slot — `1;` when the bright bolt applies,
                // then the slot's parsed parameters, or the default 39 when
                // it is empty.
                let mut matched = false;
                for (name, honor_bright) in [("keys", true), ("title", true), ("output", false), ("separator", false)] {
                    if rest.len() >= name.len() && rest[..name.len()].eq_ignore_ascii_case(name) {
                        if honor_bright && self.bright {
                            out.push_str("1;");
                        }
                        let current = match name {
                            "keys" => &self.keys,
                            "title" => &self.title,
                            "output" => &self.output,
                            _ => &self.separator,
                        };
                        if current.is_empty() {
                            out.push_str("39");
                        } else {
                            out.push_str(current);
                        }
                        rest = &rest[name.len()..];
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    return fail(format!("Error: invalid color code found: {rest}"));
                }
                continue;
            }
            if let Some(after) = rest.strip_prefix('@') {
                // Xterm 256 color through strtoul base 10: C whitespace, an
                // optional sign, digits; ERANGE clamps to u64::MAX, the u32
                // truncation happens before the 255 check, and the canonical
                // value is appended (leading zeros drop).
                let bytes = after.as_bytes();
                let mut offset = 0;
                while offset < bytes.len() && matches!(bytes[offset], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') {
                    offset += 1;
                }
                let negative = offset < bytes.len() && bytes[offset] == b'-';
                if offset < bytes.len() && (bytes[offset] == b'+' || bytes[offset] == b'-') {
                    offset += 1;
                }
                let digit_start = offset;
                while offset < bytes.len() && bytes[offset].is_ascii_digit() {
                    offset += 1;
                }
                if offset == digit_start {
                    return fail(format!("Error: invalid 256 color code found: {after}"));
                }
                // glibc returns ULONG_MAX on ERANGE before the sign negates.
                let color = match after[digit_start..offset].parse::<u64>() {
                    Ok(magnitude) => (if negative { magnitude.wrapping_neg() } else { magnitude }) as u32,
                    Err(_) => u32::MAX,
                };
                if color > 255 {
                    return fail(format!("Error: invalid 256 color code found: {after}"));
                }
                out.push_str("38;5;");
                out.push_str(&color.to_string());
                rest = &after[offset..];
                continue;
            }
            if let Some(after) = rest.strip_prefix('#') {
                // RGB through strtoul base 16: the consumed span — sign and
                // 0x prefix included — drives the length checks (3 expands
                // 4-bit channels, 6 is used as-is), and the u32 truncation
                // happens before them.
                let bytes = after.as_bytes();
                let mut offset = 0;
                while offset < bytes.len() && matches!(bytes[offset], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') {
                    offset += 1;
                }
                let negative = offset < bytes.len() && bytes[offset] == b'-';
                if offset < bytes.len() && (bytes[offset] == b'+' || bytes[offset] == b'-') {
                    offset += 1;
                }
                let digit_start = offset;
                let mut hex_start = offset;
                if offset + 2 < bytes.len()
                    && bytes[offset] == b'0'
                    && (bytes[offset + 1] | 32) == b'x'
                    && bytes[offset + 2].is_ascii_hexdigit()
                {
                    hex_start = offset + 2;
                    offset += 2;
                }
                while offset < bytes.len() && bytes[offset].is_ascii_hexdigit() {
                    offset += 1;
                }
                if offset == digit_start {
                    return fail(format!("Error: invalid RGB color code found: {after}"));
                }
                // glibc returns ULONG_MAX on ERANGE before the sign negates;
                // a 6-hex-digit span can never overflow, so the clamp branch
                // is dead here but kept symmetric.
                let value = match u64::from_str_radix(&after[hex_start..offset], 16) {
                    Ok(magnitude) => (if negative { magnitude.wrapping_neg() } else { magnitude }) as u32,
                    Err(_) => u32::MAX,
                };
                if offset > 6 {
                    return fail(format!("Error: RGB color code too long: {after}"));
                } else if offset == 3 {
                    let rgb = ((value & 0xF00) >> 8) * 0x110000
                        + ((value & 0x0F0) >> 4) * 0x001100
                        + (value & 0x00F) * 0x000011;
                    out.push_str(&format!("38;2;{};{};{}", rgb >> 16, (rgb >> 8) & 0xFF, rgb & 0xFF));
                } else if offset != 6 {
                    return fail(format!("Error: invalid RGB color code length: {after}"));
                } else {
                    out.push_str(&format!("38;2;{};{};{}", value >> 16, (value >> 8) & 0xFF, value & 0xFF));
                }
                rest = &after[offset..];
                continue;
            }
            let next = rest.chars().next().unwrap_or('\0');
            out.push(next);
            rest = &rest[next.len_utf8()..];
        }
        out
    }
}

/// ffOptionParseUInt32: strtoul base 10 with the strict tail check — a
/// missing value exits 480, a leading `-`, trailing junk (end not at the
/// terminator) or a value above u32::MAX exits 479. Leading whitespace
/// and a lone sign parse to zero like strtoul's empty conversion.
fn parse_uint32(key: &str, value: Option<&str>) -> u32 {
    let Some(value) = value else {
        usage_exit(key, "<num>", 480);
    };
    let invalid = || -> ! {
        eprintln!("Error: usage: {key} <num>");
        std::process::exit(479)
    };
    if value.starts_with('-') {
        invalid();
    }
    let text = value.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let text = text.strip_prefix('+').unwrap_or(text);
    if text.is_empty() {
        return 0;
    }
    let digits = text.len() - text.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || digits != text.len() {
        invalid();
    }
    match text.parse::<u64>() {
        Ok(num) if num <= u32::MAX as u64 => num as u32,
        _ => invalid(),
    }
}

/// --format: `default`/`json` (case-insensitive), with
/// ffOptionParseEnum's messages.
fn format_enum(key: &str, value: Option<&str>) -> bool {
    let Some(value) = value else {
        usage_exit(key, "<value>", 476);
    };
    if value.eq_ignore_ascii_case("json") {
        return true;
    }
    if value.eq_ignore_ascii_case("default") {
        return false;
    }
    eprintln!("Error: unknown {key} value: {value}");
    std::process::exit(478)
}

/// rustfetch-only `--hyfetch-mode`: the same case-insensitive ffOptionParseEnum
/// path as the other enum flags, with three aliases per ANSI family.
fn hyfetch_mode_enum(key: &str, value: Option<&str>) -> crate::hyfetch::AnsiMode {
    const PAIRS: &[(&str, i32)] = &[
        ("24bit", 0),
        ("rgb", 0),
        ("truecolor", 0),
        ("8bit", 1),
        ("ansi256", 1),
        ("256color", 1),
    ];
    match parse_enum(key, value, PAIRS) {
        1 => crate::hyfetch::AnsiMode::Ansi256,
        _ => crate::hyfetch::AnsiMode::Rgb,
    }
}

/// rustfetch-only `--hyfetch-lightness`: a finite number in 0..=1,
/// rejected with the rustfetch config exit code rather than a silent clamp.
fn hyfetch_lightness(key: &str, value: Option<&str>) -> f64 {
    let Some(value) = value else {
        usage_exit(key, "<0.0..1.0>", 477);
    };
    match value.parse::<f64>() {
        Ok(number) if number.is_finite() && (0.0..=1.0).contains(&number) => number,
        _ => usage_exit(key, "<0.0..1.0>", 477),
    }
}

/// --logo-position's ffOptionParseEnum pairs (options/logo.c:196).
fn position_enum(key: &str, value: Option<&str>) -> crate::logo::PositionOpt {
    use crate::logo::PositionOpt;
    let Some(value) = value else {
        usage_exit(key, "<value>", 476);
    };
    match value.to_ascii_lowercase().as_str() {
        "auto" => PositionOpt::Auto,
        "left" => PositionOpt::Left,
        "right" => PositionOpt::Right,
        "top" => PositionOpt::Top,
        _ => {
            eprintln!("Error: unknown {key} value: {value}");
            std::process::exit(478)
        },
    }
}

fn invalid_option(key: &str) -> ! {
    eprintln!("Error: invalid option: {key}. An option must start with `-`");
    std::process::exit(400)
}

/// printCommandFormatHelpJson (`-h format-json`): every module's format
/// variables as a yyjson pretty document, keyed `<module>Format`.
fn print_format_help_json() -> ! {
    let mut root = crate::json::Json::obj();
    for (name, args) in crate::module_formats::MODULE_FORMATS {
        if args.is_empty() {
            continue;
        }
        let mut key = name.to_ascii_lowercase();
        key.push_str("Format");
        let mut content =
            format!("Output format for the `{name}` module. See Wiki for formatting syntax\n");
        for (index, (variable, desc)) in args.iter().enumerate() {
            content.push_str(&format!("    {}. {{{variable}}}: {desc}\n", index + 1));
        }
        // ffStrbufTrimRight(&content, '\n')
        while content.ends_with('\n') {
            content.pop();
        }
        let mut object = crate::json::Json::obj();
        object.push("description", content);
        object.push("type", "string");
        root.push(&key, object);
    }
    // yyjson_mut_write_fp(stdout, doc, YYJSON_WRITE_PRETTY, ...) without a
    // trailing newline, followed by putchar('\n').
    print!("{}", crate::json::to_pretty_indent(&root, "    "));
    std::process::exit(0);
}

/// printCommandFormatHelp (`-h <module>-format`).
fn print_module_format_help(command: &str) {
    let type_name = command[..command.len() - "-format".len()].to_ascii_lowercase();
    let Some((name, args)) = crate::module_formats::MODULE_FORMATS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&type_name))
    else {
        eprintln!("Error: Module '{type_name}' is not supported");
        return;
    };
    if args.is_empty() {
        eprintln!("Error: Module '{name}' doesn't support output formatting");
        return;
    }
    println!("-- In config file: {{ \"type\": \"{type_name}\", \"format\": \"{{<format-variable>}}\" }}");
    println!("Sets the format string for {name} output.");
    println!("To see how a format string is constructed, take a look at https://github.com/fastfetch-cli/fastfetch/wiki/Format-String-Guide.");
    println!("Descriptions which end with a '*' are available in key format too.");
    println!("The following variables are passed:");
    let mut max_width = 20usize;
    for (variable, _) in *args {
        max_width = max_width.max(variable.len() + 2);
    }
    for (variable, desc) in *args {
        println!("{:>width$}: {desc}", format!("{{{variable}}}"), width = max_width);
    }
}

/// Every non-pseudo option object in the embedded help document, in order.
fn help_options(doc: &HelpJson) -> Vec<&HelpJson> {
    let mut options = Vec::new();
    if let Some(categories) = doc.as_object() {
        for (_, value) in categories {
            if let Some(items) = value.as_array() {
                options.extend(items.iter());
            }
        }
    }
    options
}

/// `-x, --long <arg>` / `--long <?arg>`: the type words are already the
/// upstream ones (`num`, `bool`, `enum`, `color`, `structure`, `str`,
/// `string`, `path`, `data`, `logo`, `command`, `config`).
fn arg_suffix(arg: &HelpJson) -> Option<String> {
    let kind = arg.get("type")?.as_str()?;
    let optional = arg.get("optional").and_then(HelpJson::as_bool).unwrap_or(false);
    Some(if optional { format!("<?{kind}>") } else { format!("<{kind}>") })
}

fn default_text(arg: &HelpJson) -> Option<String> {
    let default = arg.get("default")?;
    // `structure`'s JSON default is a pointer to `--print-structure`; the
    // reference expands it to the compiled-in module list.
    if arg.get("type").and_then(HelpJson::as_str) == Some("structure") {
        return Some(crate::modules::DEFAULT_STRUCTURE_TEXT.to_string());
    }
    match default {
        HelpJson::Str(text) => Some(text.clone()),
        HelpJson::Bool(value) => Some(value.to_string()),
        HelpJson::Num(number) => Some(number.clone()),
        _ => None,
    }
}

/// Generate the exact `-h <flag>` entry for a long option name from HELP_RAW.
/// Pseudo entries are skipped so they keep the "No specific help" answer.
fn specific_help(command: &str) -> Option<String> {
    let mut parser = HelpJsonParser::new(HELP_RAW);
    let doc = parser.parse()?;
    let options = help_options(&doc);
    let entry = options.iter().copied().find(|option| {
        let pseudo = option.get("pseudo").and_then(HelpJson::as_bool).unwrap_or(false);
        !pseudo
            && option.get("long").and_then(HelpJson::as_str)
                .is_some_and(|name| name.eq_ignore_ascii_case(command))
    })?;

    // Upstream aligns every `Options:` key to the longest enum key in the
    // whole option table (12 in this build), not to the current option's
    // own longest key.
    let key_width = options
        .iter()
        .filter_map(|option| option.get("arg").and_then(|arg| arg.get("enum")).and_then(HelpJson::as_object))
        .flatten()
        .map(|(key, _)| key.len())
        .max()
        .unwrap_or(0);

    let mut out = String::new();
    if let Some(desc) = entry.get("desc").and_then(HelpJson::as_str) {
        out.push_str(desc);
    }
    out.push('\n');

    out.push_str("     Usage: ");
    if let Some(short) = entry.get("short").and_then(HelpJson::as_str) {
        out.push('-');
        out.push_str(short);
        out.push_str(", ");
    }
    if let Some(long) = entry.get("long").and_then(HelpJson::as_str) {
        out.push_str("--");
        out.push_str(long);
    }
    if let Some(arg) = entry.get("arg")
        && let Some(suffix) = arg_suffix(arg)
    {
        out.push(' ');
        out.push_str(&suffix);
    }
    out.push('\n');

    if let Some(default) = entry.get("arg").and_then(default_text) {
        out.push_str("   Default: ");
        out.push_str(&default);
        out.push('\n');
    }

    if let Some(pairs) = entry.get("arg").and_then(|arg| arg.get("enum")).and_then(HelpJson::as_object) {
        out.push_str("   Options:\n");
        for (key, value) in pairs {
            let desc = value.as_str().unwrap_or("");
            out.push_str(&format!("{key:>key_width$}: {desc}\n"));
        }
    }

    if let Some(remark) = entry.get("remark") {
        let lines: Vec<&str> = match remark {
            HelpJson::Str(text) => vec![text],
            HelpJson::Arr(items) => items.iter().filter_map(HelpJson::as_str).collect(),
            _ => Vec::new(),
        };
        for (index, line) in lines.iter().enumerate() {
            if index == 0 {
                out.push_str("    Remark: ");
            } else {
                out.push_str("            ");
            }
            out.push_str(line);
            out.push('\n');
        }
    }

    Some(out)
}

/// `-h` without a command prints the full help; `-h <command>` prints the
/// option's entry, or the reference's no-help error (which still exits 0).
fn print_help(command: Option<&str>) -> ! {
    match command {
        None => {
            print!("{}", HELP);
            std::process::exit(0);
        },
        Some(command) => {
            if command.eq_ignore_ascii_case("format-json") {
                print_format_help_json();
            }
            // <module>-format: an English letter followed by the suffix,
            // like fastfetch's argv walk check.
            if command.len() > "-format".len()
                && command.as_bytes()[0].is_ascii_alphabetic()
                && command[command.len() - "-format".len()..].eq_ignore_ascii_case("-format")
            {
                print_module_format_help(command);
                std::process::exit(0);
            }
            match specific_help(command) {
                Some(text) => print!("{text}"),
                None => eprintln!("Error: No specific help for command '{command}' provided"),
            }
            std::process::exit(0);
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_value_skips_dash_prefixed_tokens() {
        // `-` alone aliases /dev/stdin, so it is a value.
        assert_eq!(next_value(&["rf", "-c", "-"], 1), Some("-"));
        assert_eq!(next_value(&["rf", "-c", "all"], 1), Some("all"));
        assert_eq!(next_value(&["rf", "-c", "-s", "x"], 1), None);
        assert_eq!(next_value(&["rf", "-c"], 1), None);
        // A negative number is a value (`--logo-spin-tilt -30`).
        assert_eq!(next_value(&["rf", "-l", "-30"], 1), Some("-30"));
        assert_eq!(next_value(&["rf", "-l", "-.5"], 1), Some("-.5"));
        // But `-x` is the next option.
        assert_eq!(next_value(&["rf", "-l", "-x"], 1), None);
    }

    #[test]
    fn parse_from_reads_the_hyfetch_flags() {
        // These flags are rustfetch-only, absent from the verbatim help.
        let cli = Cli::parse_from(&[
            "rf",
            "--hyfetch-preset",
            "transgender,nonbinary",
            "--hyfetch-mode",
            "8BIT",
            "--hyfetch-lightness",
            "0.5",
        ]);
        assert_eq!(cli.hyfetch_preset.as_deref(), Some("transgender,nonbinary"));
        assert_eq!(cli.hyfetch_mode, Some(crate::hyfetch::AnsiMode::Ansi256));
        assert_eq!(cli.hyfetch_lightness, Some(0.5));

        let cli = Cli::parse_from(&["rf", "--hyfetch-mode", "truecolor"]);
        assert_eq!(cli.hyfetch_mode, Some(crate::hyfetch::AnsiMode::Rgb));
        assert!(cli.hyfetch_preset.is_none());
        assert!(cli.hyfetch_lightness.is_none());
    }

    #[test]
    fn parse_boolean_is_the_strict_set() {
        assert!(parse_boolean(None));
        assert!(parse_boolean(Some("")));
        for word in ["true", "TRUE", "yes", "Yes", "on", "1"] {
            assert!(parse_boolean(Some(word)), "{word}");
        }
        for word in ["false", "abc", "-", "0", "no", "off"] {
            assert!(!parse_boolean(Some(word)), "{word}");
        }
    }

    #[test]
    fn parse_from_reads_the_option_set() {
        let cli = Cli::parse_from(&["rf", "-s", "os:cpu", "--pipe", "false", "-l", "none", "-j"]);
        assert_eq!(cli.structure.as_deref(), Some("os:cpu"));
        assert_eq!(cli.pipe, Some(false));
        assert!(cli.logo_none);
        assert!(cli.format_json);

        // "none"/"small" set the type; a source flag only fills the
        // source, like upstream's FFLogoType mutations.
        let cli = Cli::parse_from(&["rf", "-l", "arch", "-l", "small"]);
        assert!(cli.logo_small);
        assert_eq!(cli.logo_source.as_deref(), Some("arch"));
        let cli = Cli::parse_from(&["rf", "-l", "small", "-l", "arch"]);
        assert!(cli.logo_small);
        assert_eq!(cli.logo_source.as_deref(), Some("arch"));
        let cli = Cli::parse_from(&["rf", "-l", "arch", "-l", "none"]);
        assert!(cli.logo_none);
        assert_eq!(cli.logo_source.as_deref(), Some("arch"));

        // A missing boolean value defaults to true.
        let cli = Cli::parse_from(&["rf", "--pipe"]);
        assert_eq!(cli.pipe, Some(true));

        // Nothing enables JSON except -j/--json true and --format json.
        let cli = Cli::parse_from(&["rf", "-j", "false", "--format", "json"]);
        assert!(cli.format_json);
        let cli = Cli::parse_from(&["rf", "--format", "default", "-s", "os"]);
        assert!(!cli.format_json);
        assert_eq!(cli.structure.as_deref(), Some("os"));
    }

    #[test]
    fn parse_from_skips_values_of_earlier_options() {
        // `-s`'s value must not surface as a positional token.
        let cli = Cli::parse_from(&["rf", "-s", "os", "--pipe", "true"]);
        assert_eq!(cli.structure.as_deref(), Some("os"));
        assert_eq!(cli.pipe, Some(true));
    }

    #[test]
    fn specific_help_generates_upstream_entries() {
        assert_eq!(
            specific_help("wmi-timeout").as_deref(),
            Some("Set the timeout (ms) for WMI queries\n     Usage: --wmi-timeout <num>\n   Default: 5000\n    Remark: Windows only\n")
        );
        assert_eq!(
            specific_help("no-buffer").as_deref(),
            Some("Specify whether stdout buffering should be disabled\n     Usage: --no-buffer <?bool>\n   Default: false\n")
        );
        assert_eq!(
            specific_help("structure").as_deref(),
            Some("Set the structure of the fetch\n     Usage: -s, --structure <structure>\n   Default: Title:Separator:OS:Host:Kernel:Uptime:Packages:Shell:Display:DE:WM:WMTheme:Theme:Icons:Font:Cursor:Terminal:TerminalFont:CPU:GPU:Memory:Swap:Disk:LocalIp:Battery:PowerAdapter:Locale:Break:Colors\n    Remark: Must be a colon-separated list of module keys. Use \"fastfetch --list-modules\" to see available options\n")
        );
        // Pseudo entries still answer the no-help error.
        assert!(specific_help("logo-color-1").is_none());
        assert!(specific_help("logo-color-[1-9]").is_none());
    }
}

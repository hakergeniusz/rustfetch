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

/// `-h <command>` entries for the options this port parses, verbatim.
/// The reference knows a hundred more commands; those answer the same
/// "No specific help" error as an unknown name.
const COMMAND_HELP: &[(&str, &str)] = &[
    (
        "help",
        "Show this help message or help for a specific command\n     Usage: -h, --help <?command>\n",
    ),
    ("version", "Show the full Fastfetch version\n     Usage: -v, --version\n"),
    (
        "version-raw",
        "Show the raw version string (major.minor.patch)\n     Usage: --version-raw\n",
    ),
    (
        "config",
        "Specify the config file or preset to load\n     Usage: -c, --config <config>\n    Remark: The file is searched in the order shown by \"fastfetch --list-config-paths\". Use \"-\" to read config from stdin, or \"none\" to disable further config loading. See also https://github.com/fastfetch-cli/fastfetch/wiki/Configuration for details\n",
    ),
    (
        "structure",
        "Set the structure of the fetch\n     Usage: -s, --structure <structure>\n   Default: Title:Separator:OS:Host:Kernel:Uptime:Packages:Shell:Display:DE:WM:WMTheme:Theme:Icons:Font:Cursor:Terminal:TerminalFont:CPU:GPU:Memory:Swap:Disk:LocalIp:Battery:PowerAdapter:Locale:Break:Colors\n    Remark: Must be a colon-separated list of module keys. Use \"fastfetch --list-modules\" to see available options\n",
    ),
    (
        "json",
        "Enable or disable JSON output\n     Usage: -j, --json <?bool>\n   Default: false\n    Remark: Shortcut for `--format json`\n",
    ),
    (
        "format",
        "Set the output format\n     Usage: --format <enum>\n   Default: default\n   Options:\n     default: Default format\n        json: JSON format\n",
    ),
    (
        "pipe",
        "Disable colors\n     Usage: --pipe <?bool>\n   Default: false\n    Remark: Auto-detected based on isatty(1) by default\n",
    ),
    (
        "logo",
        "Set the logo source. Use \"none\" to disable logo output\n     Usage: -l, --logo <logo>\n    Remark: Use either a built-in logo name or a path to an image file. See also https://github.com/fastfetch-cli/fastfetch/wiki/Logo-options\n",
    ),
    ("list-modules", "List available modules\n     Usage: --list-modules\n"),
];

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
        let mut cli = Self {
            structure: None,
            logo_none: false,
            logo_small: false,
            logo_source: None,
            logo_position: None,
            logo_width: None,
            logo_height: None,
            logo_padding_top: None,
            logo_padding_bottom: None,
            logo_padding_left: None,
            logo_padding_right: None,
            logo_print_remaining: None,
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
                    println!("fastfetch 2.71.0 (x86_64)");
                    std::process::exit(0);
                },
                "--version-raw" => {
                    println!("2.71.0");
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
                "-j" | "--json" => {
                    // enableJsonOutput only: `-j false` is a no-op.
                    if parse_boolean(value) {
                        cli.format_json = true;
                    }
                    handled[i] = true;
                    if value_was_consumed(argv, i) {
                        handled[i + 1] = true;
                    }
                },
                "--format" => {
                    if format_enum(key, value) {
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
                _ => {
                    eprintln!("Error: unknown option: {key}");
                    std::process::exit(400);
                },
            }
            if value_was_consumed(argv, i) {
                i += 1;
            }
            i += 1;
        }

        cli
    }
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

/// ffOptionParseString: the `-s` value.
fn parse_string(key: &str, value: Option<&str>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => usage_exit(key, "<str>", 477),
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
            let entry = COMMAND_HELP
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(command));
            match entry {
                Some((_, text)) => print!("{text}"),
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
}

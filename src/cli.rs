//! CLI parsing for rustfetch: a port of fastfetch.c's two-pass argv walk
//! (`parseCommand`, then `parseOption`) and of common/impl/option.c's
//! parse helpers, so the value-extraction rule (a following token that
//! starts with `-` is not a value), the usage-error strings, and the exit
//! codes match the reference binary byte for byte.

use crate::config::{self, Config};

/// The reference binary's `--help` text, embedded verbatim: upstream
/// prints a hand-maintained document, not a generated one.
const HELP: &str = include_str!("cli_help.txt");

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
    /// `--pipe <?bool>`: Some when the flag was given, with upstream's
    /// boolean parsing (empty/true/yes/on/1 → true, anything else false).
    pub pipe: Option<bool>,
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
            pipe: None,
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
                    crate::modules::list_modules();
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

fn invalid_option(key: &str) -> ! {
    eprintln!("Error: invalid option: {key}. An option must start with `-`");
    std::process::exit(400)
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

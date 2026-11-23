//! Command-line parsing. Pure logic, compiled and tested on every target.

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub version: Option<String>,
    pub install_dir: Option<String>,
    pub uninstall: bool,
    pub no_path: bool,
    pub help: bool,
}

pub const HELP: &str = "\
rustfetch-setup - rustfetch installer for Windows (no PowerShell required)

USAGE:
    rustfetch-setup.exe [OPTIONS]

OPTIONS:
    --version vX.Y.Z   Pin a release tag instead of using the latest release.
                       A leading 'v' is optional (e.g. 0.2.8 works too).
    --install-dir DIR  Install into DIR instead of
                       %LOCALAPPDATA%\\Programs\\rustfetch.
                       Environment variables like %LOCALAPPDATA% are expanded.
    --no-path          Do not add the install directory to the user PATH.
    --uninstall        Remove rustfetch.exe and its user PATH entry.
    --help, -h, /?     Show this help and exit.

EXAMPLES:
    rustfetch-setup.exe
    rustfetch-setup.exe --version v0.2.8 --install-dir D:\\Tools
    rustfetch-setup.exe --uninstall

Notes: double-clicking the EXE runs the default install. No administrator
rights are needed. An internet connection is required to download the binary
and its SHA-256 sidecar from GitHub Releases.
";

fn take_value(args: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    match args.get(*index) {
        Some(value) if !value.trim().is_empty() => Ok(value.clone()),
        Some(_) => Err(format!("{} needs a non-empty value.", flag)),
        None => Err(format!("{} needs a value but none was given.", flag)),
    }
}

/// Parse the arguments (excluding the program name).
pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut opts = Options::default();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--help" | "-h" | "/?" => opts.help = true,
            "--uninstall" => opts.uninstall = true,
            "--no-path" => opts.no_path = true,
            "--version" | "-v" => {
                opts.version = Some(take_value(args, &mut i, arg)?);
            }
            "--install-dir" => {
                opts.install_dir = Some(take_value(args, &mut i, arg)?);
            }
            _ => {
                if let Some(value) = arg.strip_prefix("--version=") {
                    if value.trim().is_empty() {
                        return Err("--version= needs a non-empty value.".to_string());
                    }
                    opts.version = Some(value.to_string());
                } else if let Some(value) = arg.strip_prefix("--install-dir=") {
                    if value.trim().is_empty() {
                        return Err("--install-dir= needs a non-empty value.".to_string());
                    }
                    opts.install_dir = Some(value.to_string());
                } else {
                    return Err(format!(
                        "Unknown option '{}'. Run rustfetch-setup.exe --help for usage.",
                        arg
                    ));
                }
            }
        }
        i += 1;
    }
    Ok(opts)
}

#[cfg(test)]
mod tests {
    use super::{parse, Options};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn empty_args_are_defaults() {
        assert_eq!(parse(&args(&[])), Ok(Options::default()));
    }

    #[test]
    fn parses_version_and_install_dir() {
        let expected = Options {
            version: Some("v1.2.3".to_string()),
            install_dir: Some("D:\\Tools".to_string()),
            ..Options::default()
        };
        assert_eq!(
            parse(&args(&["--version", "v1.2.3", "--install-dir", "D:\\Tools"])),
            Ok(expected)
        );
    }

    #[test]
    fn parses_equals_form_and_flags() {
        let expected = Options {
            version: Some("0.2.8".to_string()),
            install_dir: Some("%LOCALAPPDATA%\\Tools".to_string()),
            no_path: true,
            uninstall: true,
            help: true,
        };
        assert_eq!(
            parse(&args(&[
                "--version=0.2.8",
                "--install-dir=%LOCALAPPDATA%\\Tools",
                "--no-path",
                "--uninstall",
                "--help",
            ])),
            Ok(expected)
        );
    }

    #[test]
    fn rejects_unknown_and_missing_values() {
        assert!(parse(&args(&["--bogus"])).is_err());
        assert!(parse(&args(&["--version"])).is_err());
        assert!(parse(&args(&["--install-dir", " "])).is_err());
        assert!(parse(&args(&["--version="])).is_err());
    }
}

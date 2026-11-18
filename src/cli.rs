//! CLI definition for rustfetch.

use clap::{Parser, ValueEnum};

/// Output format, mirroring fastfetch's `--format`.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Format {
    /// Two-column terminal layout with logo and colors.
    #[default]
    Standard,
    /// One JSON object per module, no logo or colors.
    Json,
}

#[derive(Parser, Debug)]
#[command(
    name = "rustfetch",
    version,
    about = "Fastfetch, rewritten in Rust",
    after_help = "Run `rustfetch -s <module>:<module>` to only show the given modules."
)]
pub struct Cli {
    /// Only show these modules, e.g. `rustfetch -s os cpu memory`
    pub modules: Vec<String>,

    /// List all available modules and exit
    #[arg(short, long)]
    pub list_modules: bool,

    /// Logo to use: a distro id (e.g. `arch`), `none`, or a path to a text file
    #[arg(long)]
    pub logo: Option<String>,

    /// Set padding around the logo (like fastfetch: applies to both sides,
    /// while the default puts 4 spaces on the right only)
    #[arg(long)]
    pub padding: Option<u8>,

    /// Only show these modules, like fastfetch's `-s os cpu`
    #[arg(short, long)]
    pub structure: Option<String>,

    /// Disable colors, logo and layout (for scripting); a value re-enables
    /// it, like fastfetch's `--pipe false`
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    pub pipe: Option<bool>,

    /// Output format
    #[arg(long, value_enum, default_value = "standard")]
    pub format: Format,
}

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
    after_help = "Run `rustfetch <module> ...` to only show the given modules."
)]
pub struct Cli {
    /// Only show these modules, e.g. `rustfetch os cpu memory`
    pub modules: Vec<String>,

    /// List all available modules and exit
    #[arg(short, long)]
    pub list_modules: bool,

    /// Logo to use: a distro id (e.g. `arch`), `none`, or a path to a text file
    #[arg(long)]
    pub logo: Option<String>,

    /// Use the small variant of the logo when available
    #[arg(short, long)]
    pub small: bool,

    /// Spaces between the logo and the info column
    #[arg(short, long, default_value_t = 2)]
    pub padding: u8,

    /// Disable colors, logo and layout (for scripting)
    #[arg(long)]
    pub pipe: bool,

    /// Output format
    #[arg(long, value_enum, default_value = "standard")]
    pub format: Format,
}

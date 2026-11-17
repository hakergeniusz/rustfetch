//! `separator` module: dashes matching the title width, like fastfetch.

use crate::{instance::Instance, modules::Row, util};

pub fn run(_instance: &Instance) -> Vec<Row> {
    // fastfetch sizes the separator to the title (user@host) length.
    let title_length = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
        .chars()
        .count()
        + 1
        + util::read_trimmed("/etc/hostname")
            .unwrap_or_else(|| "unknown".to_string())
            .chars()
            .count();

    vec![Row::raw("-".repeat(title_length.max(4)))]
}

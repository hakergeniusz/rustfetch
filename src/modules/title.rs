//! `title` module: user@host.

use crate::{instance::Instance, modules::Row, util};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    let host = util::read_trimmed("/etc/hostname").unwrap_or_else(|| "unknown".to_string());

    vec![Row::bare(format!("{user}@{host}"))]
}

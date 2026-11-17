//! `os` module: pretty distro name and architecture.

use crate::{instance::Instance, modules::Row, util};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(content) = util::read_file("/etc/os-release") else {
        return Vec::new();
    };

    let name = util::parse_kv(&content)
        .into_iter()
        .find(|(key, _)| *key == "PRETTY_NAME")
        .map(|(_, value)| value.to_string());

    match name {
        Some(name) => vec![Row::new(
            "OS",
            format!("{name} ({})", std::env::consts::ARCH),
        )],
        None => Vec::new(),
    }
}

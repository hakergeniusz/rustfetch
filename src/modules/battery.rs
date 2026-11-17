//! `battery` module: charge and status of all batteries from sysfs.

use crate::{instance::Instance, modules::Row, util};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") else {
        return Vec::new();
    };

    let mut batteries = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        if !entry.file_name().to_string_lossy().starts_with("BAT") {
            continue;
        }
        let Some(capacity) = util::read_trimmed_in(&entry.path(), "capacity") else {
            continue;
        };
        let status = util::read_trimmed_in(&entry.path(), "status").unwrap_or_default();
        batteries.push(format!("{capacity}% [{status}]"));
    }

    if batteries.is_empty() {
        return Vec::new();
    }
    vec![Row::new("Battery", batteries.join(", "))]
}

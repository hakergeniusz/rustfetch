//! `poweradapter` module: AC adapters from /sys/class/power_supply, ported
//! from fastfetch (type Mains, online, input_power_limit → "{watts}W").

use crate::{instance::Instance, modules::Row, util};

fn adapter(dir: &std::path::Path) -> Option<Row> {
    if util::read_trimmed_in(dir, "present").as_deref() == Some("0") {
        return None;
    }
    if util::read_trimmed_in(dir, "type").as_deref() != Some("Mains") {
        return None;
    }
    if util::read_trimmed_in(dir, "scope").as_deref() == Some("Device") {
        return None;
    }
    if util::read_trimmed_in(dir, "online").as_deref() != Some("1") {
        return None;
    }
    let limit = util::read_trimmed_in(dir, "input_power_limit")?;
    let microwatts: f64 = limit.parse().ok()?;
    let watts = (microwatts / 1e6 + 0.5) as i64;
    Some(Row::new("Power Adapter", format!("{watts}W")))
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|entry| adapter(&entry.path()))
        .collect()
}

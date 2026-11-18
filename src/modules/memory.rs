//! `memory` and `swap` modules: usage from /proc/meminfo.

use crate::{instance::Instance, modules::Row, modules::colorize_percent, util};

/// Reads "<key>: <value> kB" from meminfo content.
fn meminfo_value(content: &str, key: &str) -> Option<u64> {
    content.lines().find_map(|line| {
        let (name, value) = util::parse_colon_line(line)?;
        if name != key {
            return None;
        }
        value.split_whitespace().next()?.parse::<u64>().ok()
    })
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Some(content) = util::read_file("/proc/meminfo") else {
        return Vec::new();
    };

    let Some(total) = meminfo_value(&content, "MemTotal") else {
        return Vec::new();
    };
    let available =
        meminfo_value(&content, "MemAvailable").or_else(|| meminfo_value(&content, "MemFree"));
    let Some(available) = available else {
        return Vec::new();
    };

    let used = total.saturating_sub(available);
    let pct = used as f64 * 100.0 / total as f64;

    vec![Row::new(
        "Memory",
        format!(
            "{} / {} ({})",
            util::fmt_bytes(used * 1024),
            util::fmt_bytes(total * 1024),
            colorize_percent(pct, 50, 80, instance.color)
        ),
    )]
}

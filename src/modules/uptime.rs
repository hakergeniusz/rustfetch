//! `uptime` module: time since boot, fastfetch style ("2 days, 3 hours, 4 mins").

use crate::{instance::Instance, modules::Row, util};

fn plural(count: u64, unit: &str) -> String {
    if count == 1 {
        format!("{count} {unit}")
    } else {
        format!("{count} {unit}s")
    }
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(content) = util::read_file("/proc/uptime") else {
        return Vec::new();
    };
    let Some(seconds) = util::first_line(&content)
        .split_whitespace()
        .next()
        .and_then(|token| token.parse::<f64>().ok())
        .map(|value| value as u64)
    else {
        return Vec::new();
    };

    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(plural(days, "day"));
    }
    if hours > 0 {
        parts.push(plural(hours, "hour"));
    }
    parts.push(plural(minutes, "min"));

    vec![Row::new("Uptime", parts.join(", "))]
}

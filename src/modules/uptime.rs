//! `uptime` module: time since boot, fastfetch style ("2 days, 3 hours, 4 mins").
//! ffDetectUptime reads /proc/uptime; the module rounds its milliseconds
//! into whole seconds ((ms + 500) / 1000) before ffDurationAppendNum.

use crate::{instance::Instance, modules::Row, util};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(content) = util::read_file("/proc/uptime") else {
        return Vec::new();
    };
    let Some(total_seconds) = util::first_line(&content)
        .split_whitespace()
        .next()
        .and_then(|token| token.parse::<f64>().ok())
        .map(|value| {
            // ffDetectUptime: uptime = (uint64)(sec * 1000), and the module
            // prints ffDurationAppendNum((uptime + 500) / 1000).
            let milliseconds = (value * 1000.0) as u64;
            (milliseconds + 500) / 1000
        })
    else {
        return Vec::new();
    };

    vec![Row::new("Uptime", util::duration_parts(total_seconds).join(", "))]
}

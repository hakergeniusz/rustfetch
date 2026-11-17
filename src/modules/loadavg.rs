//! `loadavg` module: system load averages from /proc/loadavg.

use crate::{instance::Instance, modules::Row, util};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(content) = util::read_file("/proc/loadavg") else {
        return Vec::new();
    };

    let averages: Vec<&str> = util::first_line(&content).split_whitespace().take(3).collect();
    if averages.len() < 3 {
        return Vec::new();
    }

    vec![Row::new("Load Average", averages.join(", "))]
}

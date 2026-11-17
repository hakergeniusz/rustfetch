//! `cpu` module: model, logical core count and current frequency from
//! /proc/cpuinfo.

use crate::{instance::Instance, modules::Row, util};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(content) = util::read_file("/proc/cpuinfo") else {
        return Vec::new();
    };

    let mut name = None;
    let mut cores = 0u64;
    let mut max_mhz = 0f64;

    for line in content.lines() {
        match util::parse_colon_line(line) {
            Some(("processor", _)) => cores += 1,
            Some(("model name", value)) if name.is_none() => name = Some(value.to_string()),
            Some(("cpu MHz", value)) => {
                if let Ok(mhz) = value.parse::<f64>() {
                    max_mhz = max_mhz.max(mhz);
                }
            },
            _ => {},
        }
    }

    let Some(name) = name else {
        return Vec::new();
    };

    // 3.7 GHz on desktop parts, whole GHz on very fast ones.
    let freq = if max_mhz > 0.0 {
        let ghz = max_mhz / 1000.0;
        let digits = if ghz >= 100.0 { 0 } else if ghz >= 10.0 { 1 } else { 2 };
        format!(" @ {ghz:.digits$} GHz")
    } else {
        String::new()
    };

    vec![Row::new("CPU", format!("{name} ({cores}){freq}"))]
}

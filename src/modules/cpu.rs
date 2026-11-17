//! `cpu` module: model, logical core count and current frequency from
//! /proc/cpuinfo.

use crate::{instance::Instance, modules::Row, util};

/// Maximum boost frequency across cores, in kHz, from cpufreq sysfs.
fn max_boost_khz() -> u64 {
    std::fs::read_dir("/sys/devices/system/cpu")
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    name.starts_with("cpu") && name[3..].chars().all(|c| c.is_ascii_digit())
                })
                .filter_map(|entry| {
                    util::read_trimmed_in(&entry.path().join("cpufreq"), "cpuinfo_max_freq")
                })
                .filter_map(|value| value.parse::<u64>().ok())
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

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

    // Prefer cpufreq's boost frequency; /proc/cpuinfo only reports current.
    let mhz = {
        let boost_khz = max_boost_khz();
        if boost_khz > 0 {
            boost_khz as f64 / 1000.0
        } else {
            max_mhz
        }
    };

    // 3.7 GHz on desktop parts, whole GHz on very fast ones.
    let freq = if mhz > 0.0 {
        let ghz = mhz / 1000.0;
        let digits = if ghz >= 100.0 {
            0
        } else if ghz >= 10.0 {
            1
        } else {
            2
        };
        format!(" @ {ghz:.digits$} GHz")
    } else {
        String::new()
    };

    vec![Row::new("CPU", format!("{name} ({cores}){freq}"))]
}

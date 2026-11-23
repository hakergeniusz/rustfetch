//! `loadavg` module: system load averages from /proc/loadavg.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

pub fn run(instance: &Instance) -> Vec<Row> {
    let Some(content) = util::read_file("/proc/loadavg") else {
        return Vec::new();
    };

    let averages: Vec<&str> = util::first_line(&content).split_whitespace().take(3).collect();
    if averages.len() < 3 {
        return Vec::new();
    }

    vec![Row::new(instance.display_name("loadavg"), averages.join(", "))]
}

/// ffDetectLoadavg: /proc/loadavg's first three fields, else sysinfo().
fn detect() -> Result<[f64; 3], &'static str> {
    if let Some(content) = util::read_file("/proc/loadavg") {
        let values: Vec<f64> = util::first_line(&content)
            .split_whitespace()
            .take(3)
            .filter_map(|token| token.parse().ok())
            .collect();
        if values.len() == 3 {
            return Ok([values[0], values[1], values[2]]);
        }
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let mut sysinfo: libc::sysinfo = unsafe { std::mem::zeroed() };
        // SAFETY: sysinfo only writes into the caller-provided struct.
        if unsafe { libc::sysinfo(&mut sysinfo) } != 0 {
            return Err("both /proc/loadavg and sysinfo() failed");
        }
        let divisor = (1u64 << libc::SI_LOAD_SHIFT) as f64;
        Ok([
            sysinfo.loads[0] as f64 / divisor,
            sysinfo.loads[1] as f64 / divisor,
            sysinfo.loads[2] as f64 / divisor,
        ])
    }
    // BSDs have neither /proc/loadavg nor sysinfo(2) in libc; expose the
    // error path so the module type-checks (and simply prints nothing).
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        Err("no /proc/loadavg and sysinfo() is Linux-only")
    }
}

/// ffGenerateLoadavgJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(result) => {
            let mut arr = Json::Array(Vec::new());
            for value in result {
                arr.push_value(Json::F64(value));
            }
            JsonOutcome::Result(arr)
        },
    }
}

//! `uptime` module: time since boot, fastfetch style ("2 days, 3 hours, 4 mins").
//! ffDetectUptime prefers /proc/uptime and falls back to CLOCK_BOOTTIME;
//! bootTime is now minus uptime. The module rounds its milliseconds into
//! whole seconds ((ms + 500) / 1000) before ffDurationAppendNum.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

pub struct UptimeResult {
    /// Milliseconds since boot.
    pub uptime: u64,
    /// now - uptime, in milliseconds.
    pub boot_time: u64,
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

pub fn detect() -> Result<UptimeResult, String> {
    let sec = util::read_file("/proc/uptime").and_then(|content| {
        util::first_line(&content)
            .split_whitespace()
            .next()
            .and_then(|token| token.parse::<f64>().ok())
    });
    if let Some(sec) = sec {
        let uptime = (sec * 1000.0) as u64;
        return Ok(UptimeResult { uptime, boot_time: now_millis().saturating_sub(uptime) });
    }

    // clock_gettime(CLOCK_BOOTTIME) fallback.
    let mut timespec = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut timespec) } != 0 {
        return Err("clock_gettime(CLOCK_BOOTTIME) failed".to_string());
    }
    let uptime = (timespec.tv_sec.max(0) as u64) * 1000 + (timespec.tv_nsec as u64) / 1_000_000;
    Ok(UptimeResult { uptime, boot_time: now_millis().saturating_sub(uptime) })
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(uptime) = detect() else {
        return Vec::new();
    };
    // The module prints ffDurationAppendNum((uptime + 500) / 1000).
    let total_seconds = (uptime.uptime + 500) / 1000;
    vec![Row::new("Uptime", util::duration_parts(total_seconds).join(", "))]
}

/// ffGenerateUptimeJsonResult: raw milliseconds plus the formatted boot
/// time.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error),
        Ok(uptime) => {
            let mut result = Json::obj();
            result.push("uptime", uptime.uptime);
            result.push("bootTime", util::time_to_full_str(uptime.boot_time));
            JsonOutcome::Result(result)
        },
    }
}

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

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(uptime) = detect() else {
        return Vec::new();
    };
    // The module prints ffDurationAppendNum((uptime + 500) / 1000).
    let total_seconds = (uptime.uptime + 500) / 1000;
    let Some(format) = crate::modules::entry_format(instance.entry) else {
        let mut value = String::new();
        crate::format::duration_append_num(&mut value, total_seconds, &instance.display_format);
        return vec![Row::new(instance.display_name("uptime"), value)];
    };

    // The custom-format branch splits the raw milliseconds and renders
    // through FF_PRINT_FORMAT_CHECKED's arg list: the split fields as
    // UINT, the short boot time as STRING, the calendar age as
    // UINT/UINT/DOUBLE and the formatted duration as the last STRING.
    let mut rest = uptime.uptime;
    let milliseconds = (rest % 1000) as u32;
    rest /= 1000;
    let seconds = (rest % 60) as u32;
    rest /= 60;
    let minutes = (rest % 60) as u32;
    rest /= 60;
    let hours = (rest % 24) as u32;
    let days = (rest / 24) as u32;

    let age = util::time_get_age(uptime.boot_time, now_millis());
    let boot_time = util::time_to_short_str(uptime.boot_time);
    let mut formatted = String::new();
    crate::format::duration_append_num(&mut formatted, total_seconds, &instance.display_format);

    let args = [
        crate::format_args::FormatArg { name: "days", value: crate::format_args::ArgValue::UInt(days) },
        crate::format_args::FormatArg { name: "hours", value: crate::format_args::ArgValue::UInt(hours) },
        crate::format_args::FormatArg { name: "minutes", value: crate::format_args::ArgValue::UInt(minutes) },
        crate::format_args::FormatArg { name: "seconds", value: crate::format_args::ArgValue::UInt(seconds) },
        crate::format_args::FormatArg { name: "milliseconds", value: crate::format_args::ArgValue::UInt(milliseconds) },
        crate::format_args::FormatArg { name: "boot-time", value: crate::format_args::ArgValue::Str(&boot_time) },
        crate::format_args::FormatArg { name: "years", value: crate::format_args::ArgValue::UInt(age.years) },
        crate::format_args::FormatArg { name: "days-of-year", value: crate::format_args::ArgValue::UInt(age.days_of_year) },
        crate::format_args::FormatArg { name: "years-fraction", value: crate::format_args::ArgValue::Double(age.years_fraction) },
        crate::format_args::FormatArg { name: "formatted", value: crate::format_args::ArgValue::Str(&formatted) },
    ];
    let rendered = crate::format_args::parse_format_string(format, &args, args.len() as u32, &crate::format_args::EngineEnv::new(instance));
    vec![Row::new(instance.display_name("uptime"), rendered)]
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

//! `memory` module: usage from /proc/meminfo, ported from
//! detection/memory/memory_linux.c. MemAvailable is sanity-checked against
//! MemTotal (recomputed from free+buffers+cached+sReclaimable-shmem when
//! unreasonable), and the ZFS ARC size is subtracted from used memory.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

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

pub struct MemoryResult {
    pub total: u64,
    pub used: u64,
}

pub fn detect() -> Result<MemoryResult, String> {
    let Some(content) = util::read_file("/proc/meminfo") else {
        return Err("ffReadFileData(\"/proc/meminfo\", ARRAY_SIZE(buf)-1, buf)".to_string());
    };
    let Some(total_kb) = meminfo_value(&content, "MemTotal") else {
        return Err("MemTotal not found in /proc/meminfo".to_string());
    };

    let available_kb = match meminfo_value(&content, "MemAvailable") {
        Some(value) if value != 0 && value < total_kb => value,
        // MemAvailable can be unreasonable (#1988): recompute it.
        _ => {
            let mem_free = meminfo_value(&content, "MemFree").unwrap_or(0);
            let buffers = meminfo_value(&content, "Buffers").unwrap_or(0);
            let cached = meminfo_value(&content, "Cached").unwrap_or(0);
            let s_reclaimable = meminfo_value(&content, "SReclaimable").unwrap_or(0);
            let shmem = meminfo_value(&content, "Shmem").unwrap_or(0);
            mem_free
                .wrapping_add(buffers)
                .wrapping_add(cached)
                .wrapping_add(s_reclaimable)
                .wrapping_sub(shmem)
        },
    };

    let used_bytes = total_kb.wrapping_sub(available_kb) * 1024;

    // Upstream also subtracts the ZFS ARC size from /proc/spl/kstat/zfs/
    // arcstats here, but its strtoull offset math lands on the "size"
    // label rather than the value column, so the parse always yields 0
    // and the adjustment never fires; ported as the same no-op.

    Ok(MemoryResult { total: total_kb * 1024, used: used_bytes })
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(memory) = detect() else {
        // Detection errors are hidden rows by default.
        return Vec::new();
    };

    let percent = super::percent_config(instance.entry, crate::format::PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 });
    let ctx = super::print_ctx(instance);
    let df = &instance.display_format;
    let percent_type_flags = if percent.type_flags == 0 { df.percent_type } else { percent.type_flags };

    let mut used_pretty = String::new();
    crate::format::size_append_num(&mut used_pretty, memory.used, df);
    let mut total_pretty = String::new();
    crate::format::size_append_num(&mut total_pretty, memory.total, df);

    if memory.total == 0 {
        return vec![Row::new(instance.display_name("memory"), "Disabled")];
    }
    let percentage = memory.used as f64 / memory.total as f64 * 100.0;

    let mut value = String::new();
    if percent_type_flags & crate::format::percent_type::BAR_BIT != 0 {
        crate::format::percent_append_bar(&mut value, percentage, percent, &ctx, df);
        value.push(' ');
    }
    if percent_type_flags & crate::format::percent_type::HIDE_OTHERS_BIT == 0 {
        value.push_str(&used_pretty);
        value.push_str(" / ");
        value.push_str(&total_pretty);
        value.push(' ');
    }
    if percent_type_flags & crate::format::percent_type::NUM_BIT != 0 {
        let parentheses = !value.is_empty();
        crate::format::percent_append_num(&mut value, percentage, percent, parentheses, &ctx, df);
    }
    // ffStrbufTrimRight(&str, ' ').
    while value.ends_with(' ') {
        value.pop();
    }
    vec![Row::new(instance.display_name("memory"), value)]
}

/// ffGenerateMemoryJsonResult: raw byte counts.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error),
        Ok(memory) => {
            let mut result = Json::obj();
            result.push("total", memory.total);
            result.push("used", memory.used);
            JsonOutcome::Result(result)
        },
    }
}

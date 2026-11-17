//! `swap` module: usage from /proc/meminfo; hidden when no swap is configured.

use crate::{instance::Instance, modules::Row, util};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(content) = util::read_file("/proc/meminfo") else {
        return Vec::new();
    };
    let total = content
        .lines()
        .find_map(|line| {
            let (name, value) = util::parse_colon_line(line)?;
            (name == "SwapTotal").then(|| value.split_whitespace().next()?.parse::<u64>().ok())?
        });
    let free = content
        .lines()
        .find_map(|line| {
            let (name, value) = util::parse_colon_line(line)?;
            (name == "SwapFree").then(|| value.split_whitespace().next()?.parse::<u64>().ok())?
        });

    let (Some(total), Some(free)) = (total, free) else {
        return Vec::new();
    };
    if total == 0 {
        return Vec::new();
    }

    let used = total.saturating_sub(free);
    let pct = util::percent(used, total).unwrap_or(0);

    vec![Row::new(
        "Swap",
        format!("{} / {} ({pct}%)", util::fmt_bytes(used * 1024), util::fmt_bytes(total * 1024)),
    )]
}

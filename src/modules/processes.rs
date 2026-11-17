//! `processes` module: running process and thread counts from /proc,
//! skipping kernel threads like fastfetch (PF_KTHREAD flag in stat field 9).

use crate::{instance::Instance, modules::Row, util};

/// kernel include/linux/sched.h
const PF_KTHREAD: u64 = 0x0020_0000;

/// (flags, num_threads) are stat fields 9 and 20; after the comm, field 3
/// starts, so they sit at token indexes 6 and 17.
pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };

    let mut processes = 0u64;
    let mut threads = 0u64;

    for entry in entries.filter_map(Result::ok) {
        if !entry.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let stat_path = entry.path().join("stat");
        let Some(stat) = stat_path.to_str().and_then(util::read_file) else {
            continue;
        };
        let Some(close) = stat.rfind(')') else {
            continue;
        };
        let tokens: Vec<&str> = stat[close + 2..].split_whitespace().collect();
        if tokens.len() < 18 {
            continue;
        }

        let Ok(flags) = tokens[6].parse::<u64>() else {
            continue;
        };
        if flags & PF_KTHREAD != 0 {
            continue;
        }

        processes += 1;
        threads += tokens[17].parse::<u64>().unwrap_or(0);
    }

    if processes == 0 {
        return Vec::new();
    }

    let value = if threads > processes {
        format!("{processes} ({threads} threads)")
    } else {
        processes.to_string()
    };
    vec![Row::new("Processes", value)]
}

//! `top` module: the processes with the highest CPU / memory / disk usage,
//! ported from top_linux.c. /proc is scanned twice — a baseline during
//! prepare() and the second sample after the 250ms wait; the cpu percent
//! is the CPU-time delta over the actual sampling interval.

use std::sync::{Mutex, OnceLock};

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

const PF_KTHREAD: u64 = 0x0020_0000;
const N_PROCESSES: u32 = 5;
const WAIT_TIME_MS: u64 = 250;

#[derive(Clone)]
struct Snapshot {
    name: String,
    cpu_time_ms: u64,
    mem_bytes: u64,
    bytes_read: u64,
    bytes_written: u64,
    start_time: u64,
    pid: u32,
    /// Only shown with a custom showTypes; the default output omits it.
    #[allow(dead_code)]
    threads: u32,
}

/// Fields 3..22 of /proc/pid/stat after the "(comm)" part; kernel tasks
/// (PF_KTHREAD in field 9) are skipped like parseStat does.
fn parse_stat(buffer: &str) -> Option<Snapshot> {
    let open = buffer.find('(')?;
    let close = buffer.rfind(')')?;
    if close <= open || close + 2 >= buffer.len() {
        return None;
    }
    let name = buffer[open + 1..close].to_string();
    let tokens: Vec<&str> = buffer[close + 2..].split(' ').collect();
    // tokens[i] is field i + 3.
    if tokens.len() < 20 {
        return None;
    }
    let flags: u64 = tokens[6].parse().ok()?;
    if flags & PF_KTHREAD != 0 {
        return None;
    }
    let utime: u64 = tokens[11].parse().ok()?;
    let stime: u64 = tokens[12].parse().ok()?;
    let threads: u32 = tokens[17].parse().ok()?;
    let start_time: u64 = tokens[19].parse().ok()?;
    Some(Snapshot {
        name,
        cpu_time_ms: utime + stime,
        mem_bytes: 0,
        bytes_read: 0,
        bytes_written: 0,
        start_time,
        threads,
        pid: 0,
    })
}

/// The statm second field is the resident set size in pages.
fn parse_statm(statm: &str) -> Option<u64> {
    statm.split_whitespace().nth(1)?.parse().ok()
}

/// read_bytes / write_bytes out of /proc/pid/io.
fn parse_io(io: &str) -> (u64, u64) {
    let mut read = 0;
    let mut written = 0;
    for line in io.lines() {
        if let Some(value) = line.strip_prefix("read_bytes: ") {
            read = value.trim().parse().unwrap_or(0);
        } else if let Some(value) = line.strip_prefix("write_bytes: ") {
            written = value.trim().parse().unwrap_or(0);
        }
    }
    (read, written)
}

/// ffTopGetProcessSnapshot: one pass over /proc.
fn scan() -> Vec<Snapshot> {
    let clk_tck = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as u64;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };

    let mut snapshots = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Ok(pid) = name.to_string_lossy().parse::<u32>() else {
            continue;
        };
        if pid == 0 {
            continue;
        }
        let path = entry.path();
        let Some(stat) = std::fs::read_to_string(path.join("stat")).ok() else {
            continue;
        };
        let Some(mut snapshot) = parse_stat(&stat) else {
            continue;
        };
        snapshot.pid = pid;
        if let Some(statm) = std::fs::read_to_string(path.join("statm")).ok()
            && let Some(pages) = parse_statm(&statm)
        {
            snapshot.mem_bytes = pages * page_size;
        }
        if let Ok(io) = std::fs::read_to_string(path.join("io")) {
            let (read, written) = parse_io(&io);
            snapshot.bytes_read = read;
            snapshot.bytes_written = written;
        }
        snapshot.cpu_time_ms = snapshot.cpu_time_ms * 1000 / clk_tck.max(1);
        snapshots.push(snapshot);
    }
    snapshots
}

type Baseline = Option<(Vec<Snapshot>, std::time::Instant)>;
static BASELINE: OnceLock<Mutex<Baseline>> = OnceLock::new();

/// ffPrepareTopProcesses: the first sample, taken before the modules run.
pub fn prepare() {
    let lock = BASELINE.get_or_init(|| Mutex::new(None));
    let mut baseline = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if baseline.is_none() {
        *baseline = Some((scan(), std::time::Instant::now()));
    }
}

/// One compared-and-rated process after the two-sample measurement.
struct TopProcess {
    process: Snapshot,
    cpu_percent: f64,
    bytes_read: u64,
    bytes_written: u64,
}

/// ffDetectTopProcesses: the baseline sample, the wait, the second sample,
/// the per-process deltas; the baseline of the next run is the second
/// sample. Sorted cpu-desc (ties by ascending pid), truncated to the top 5.
fn top_processes() -> Vec<TopProcess> {
    prepare();
    let lock = BASELINE.get_or_init(|| Mutex::new(None));
    let (first, started) = {
        let baseline = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match baseline.as_ref() {
            Some(baseline) => (baseline.0.clone(), baseline.1),
            None => return Vec::new(),
        }
    };

    let elapsed = started.elapsed().as_millis() as u64;
    if elapsed < WAIT_TIME_MS {
        std::thread::sleep(std::time::Duration::from_millis(WAIT_TIME_MS - elapsed));
    }
    let second = scan();
    let elapsed = (started.elapsed().as_millis() as u64).max(1);

    // A process must still exist with monotonic counters and the same
    // start time, or it is dropped from the comparison.
    let mut results: Vec<TopProcess> = Vec::new();
    for old in &first {
        let Some(new) = second.iter().find(|new| new.pid == old.pid) else {
            continue;
        };
        if new.start_time != old.start_time
            || new.cpu_time_ms < old.cpu_time_ms
            || new.bytes_read < old.bytes_read
            || new.bytes_written < old.bytes_written
        {
            continue;
        }
        results.push(TopProcess {
            cpu_percent: (new.cpu_time_ms - old.cpu_time_ms) as f64 / elapsed as f64 * 100.0,
            bytes_read: (new.bytes_read - old.bytes_read) * 1000 / elapsed,
            bytes_written: (new.bytes_written - old.bytes_written) * 1000 / elapsed,
            process: new.clone(),
        });
    }

    // The baseline of the next run is the second sample.
    {
        let mut baseline = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *baseline = Some((second, std::time::Instant::now()));
    }

    // Default sort: cpu desc, ties by ascending pid; keep the top 5.
    results.sort_by(|a, b| {
        let order = if b.cpu_percent < a.cpu_percent {
            std::cmp::Ordering::Less
        } else if b.cpu_percent > a.cpu_percent {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        };
        order.then(a.process.pid.cmp(&b.process.pid))
    });
    results.truncate(N_PROCESSES as usize);
    results
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let results = top_processes();
    if results.is_empty() {
        // "No processes found" is an error row, hidden by default.
        return Vec::new();
    }

    let total = results.len();
    results
        .iter()
        .enumerate()
        .map(|(index, top)| {
            let process = &top.process;
            let key = if total == 1 {
                "Top Processes".to_string()
            } else {
                format!("{} {}", instance.display_name("top"), index + 1)
            };
            let key_index = if total == 1 { 0 } else { index as u32 + 1 };
            let name = if process.name.is_empty() {
                "<unknown>".to_string()
            } else {
                process.name.clone()
            };
            let mut value = format!("{name} ({})", process.pid);
            value.push_str(" - CPU ");
            value.push_str(&super::colorize_percent(top.cpu_percent, 50, 80, instance.color));
            value.push_str(" - MEM ");
            value.push_str(&util::fmt_bytes(process.mem_bytes));
            value.push_str(" - DSK ");
            value.push_str(&util::fmt_bytes(top.bytes_read));
            value.push_str("/s / ");
            value.push_str(&util::fmt_bytes(top.bytes_written));
            value.push_str("/s");
            Row::new(&key, value).with_index(key_index)
        })
        .collect()
}

/// ffGenerateTopJsonResult with the default showTypes (CPU | memory | disk).
pub fn json(_instance: &Instance) -> JsonOutcome {
    let results = top_processes();
    let mut arr = Json::Array(Vec::new());
    for top in &results {
        let mut obj = Json::obj();
        obj.push("name", top.process.name.clone());
        obj.push("pid", top.process.pid as u64);
        obj.push("cpuPercent", top.cpu_percent);
        obj.push("mem", top.process.mem_bytes);
        obj.push("bytesRead", top.bytes_read);
        obj.push("bytesWritten", top.bytes_written);
        obj.push("startTime", top.process.start_time);
        arr.push_value(obj);
    }
    JsonOutcome::Result(arr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_stat_fields() {
        let stat = "982655 (ffmpeg) S 982600 982655 982655 0 -1 4194560 \
            12345 0 0 0 100 50 0 0 0 0 16 0 123456789 1234567 5632 \
            18446744073709551615 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 \
            0 0 0 0 0 0 0 0 0 0\n";
        let Some(snapshot) = parse_stat(stat) else {
            panic!("stat must parse");
        };
        assert_eq!(snapshot.name, "ffmpeg");
        assert_eq!(snapshot.cpu_time_ms, 150); // utime 100 + stime 50 ticks
        assert_eq!(snapshot.threads, 16);
        assert_eq!(snapshot.start_time, 123456789);
    }

    #[test]
    fn skips_kernel_threads_and_malformed() {
        // PF_KTHREAD (0x200000) set in flags (field 9).
        let kthread = "42 (kworker) S 1 1 1 0 -1 6291456 0 0 0 0 0 0 0 0 0 0 1 0 1 1 1 1 1 1 1\n";
        assert!(parse_stat(kthread).is_none());
        assert!(parse_stat("12 (x)").is_none());
    }

    #[test]
    fn parses_statm_and_io() {
        assert_eq!(parse_statm("1234 5678 5632 100 0 1234 0"), Some(5678));
        let io = "rchar: 1\nwchar: 2\nsyscr: 3\nsyscw: 4\nread_bytes: 512\nwrite_bytes: 1024\n";
        assert_eq!(parse_io(io), (512, 1024));
        assert_eq!(parse_io("no data"), (0, 0));
    }

    #[test]
    fn sorts_by_cpu_desc_then_pid_asc() {
        let snapshot = |name: &str, pid: u32| Snapshot {
            name: name.into(),
            cpu_time_ms: 0,
            mem_bytes: 0,
            bytes_read: 0,
            bytes_written: 0,
            start_time: 1,
            threads: 0,
            pid,
        };
        let mut results = [
            TopProcess { process: snapshot("b", 20), cpu_percent: 5.0, bytes_read: 0, bytes_written: 0 },
            TopProcess { process: snapshot("a", 10), cpu_percent: 10.0, bytes_read: 0, bytes_written: 0 },
            TopProcess { process: snapshot("c", 2), cpu_percent: 10.0, bytes_read: 0, bytes_written: 0 },
        ];
        results.sort_by(|a, b| {
            let order = if b.cpu_percent < a.cpu_percent {
                std::cmp::Ordering::Less
            } else if b.cpu_percent > a.cpu_percent {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            };
            order.then(a.process.pid.cmp(&b.process.pid))
        });
        assert_eq!(
            results.iter().map(|tp| tp.process.pid).collect::<Vec<_>>(),
            [2, 10, 20]
        );
    }
}

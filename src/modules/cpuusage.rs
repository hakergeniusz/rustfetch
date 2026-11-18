//! `cpuusage` module: average cpu usage, ported from cpuusage.c's
//! sample-delta scheme: /proc/stat is read once at startup (prepare) and
//! again after the 200ms window, and the in-use/total time delta per core is
//! averaged into one percentage.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{instance::Instance, modules::Row, modules::colorize_percent, util};

const WAIT_MS: u64 = 200;

/// (in-use, total) jiffies per cpu from the second line on; the aggregate
/// "cpu" line and anything the "cpuN" pattern doesn't match stop the parse.
fn times_from(content: &str) -> Vec<(u64, u64)> {
    let mut times = Vec::new();
    for line in content.lines().skip(1) {
        let mut tokens = line.split_whitespace();
        let Some(label) = tokens.next() else {
            break;
        };
        let Some(index) = label.strip_prefix("cpu") else {
            break;
        };
        if index.parse::<u32>().is_err() {
            break;
        }
        let values: Vec<u64> = tokens.take(7).filter_map(|t| t.parse().ok()).collect();
        if values.len() != 7 {
            break;
        }
        let [user, nice, system, idle, iowait, irq, softirq] = values[..] else {
            break;
        };
        let in_use = user + nice + system + irq + softirq;
        times.push((in_use, in_use + idle + iowait));
    }
    times
}

fn read_times() -> Option<Vec<(u64, u64)>> {
    let times = times_from(&util::read_file("/proc/stat")?);
    (!times.is_empty()).then_some(times)
}

/// One (in-use, total) pair per cpu plus the sample instant.
type CpuTimes = Vec<(u64, u64)>;

static BASELINE: Mutex<Option<(CpuTimes, Instant)>> = Mutex::new(None);

/// ffPrepareCPUUsage: take the first sample as early as possible so the
/// 200ms window overlaps the other modules' detection.
pub(crate) fn prepare() {
    let mut baseline = match BASELINE.lock() {
        Ok(baseline) => baseline,
        Err(poisoned) => poisoned.into_inner(),
    };
    if baseline.is_none()
        && let Some(times) = read_times()
    {
        *baseline = Some((times, Instant::now()));
    }
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    // The baseline guard is released before any retry sleeps below.
    let times1 = {
        let mut baseline = match BASELINE.lock() {
            Ok(baseline) => baseline,
            Err(poisoned) => poisoned.into_inner(),
        };
        match baseline.take() {
            Some((times1, start)) => {
                let elapsed = start.elapsed();
                if elapsed.as_millis() < WAIT_MS as u128 {
                    std::thread::sleep(Duration::from_millis(WAIT_MS) - elapsed);
                }
                Some(times1)
            },
            None => {
                let Some(times1) = read_times() else {
                    return Vec::new();
                };
                std::thread::sleep(Duration::from_millis(WAIT_MS));
                Some(times1)
            },
        }
    };
    let Some(times1) = times1 else {
        return Vec::new();
    };

    // A core's time can stand still between two quick samples; retry like
    // fastfetch does before giving up.
    let mut times2 = None;
    for _ in 0..=3 {
        match read_times() {
            Some(times) => {
                if times.iter().zip(&times1).all(|((_, total2), (_, total1))| total2 > total1) {
                    times2 = Some(times);
                    break;
                }
                std::thread::sleep(Duration::from_millis(WAIT_MS));
            },
            None => return Vec::new(),
        }
    }
    let Some(times2) = times2 else {
        return Vec::new();
    };
    if times1.len() != times2.len() {
        return Vec::new();
    }

    let mut sum = 0.0;
    for ((in_use1, total1), (in_use2, total2)) in times1.iter().zip(&times2) {
        sum += (*in_use2 - *in_use1) as f64 / (*total2 - *total1) as f64 * 100.0;
    }
    let avg = sum / times2.len() as f64;
    vec![Row::new(
        "CPU Usage",
        colorize_percent(avg, 50, 80, _instance.color),
    )]
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_per_core_times() {
        let stat = "cpu  100 0 200 300 0 50 0 0 0\n\
                    cpu0 10 2 20 40 5 6 7 0 0\n\
                    cpu1 11 3 21 41 6 7 8 0 0\n\
                    intr 123\n";
        let times = super::times_from(stat);
        assert_eq!(times.len(), 2);
        // cpu0: in_use = 10+2+20+6+7 = 45, total = 45+40+5 = 90
        assert_eq!(times[0], (45, 90));
        assert_eq!(times[1], (50, 97));
    }
}

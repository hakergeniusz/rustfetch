//! `cpuusage` module: average cpu usage, ported from cpuusage.c's
//! sample-delta scheme: /proc/stat is read once at startup (prepare) and
//! again after the 200ms window, and the in-use/total time delta per core is
//! averaged into one percentage.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::{colorize_percent, Row},
    util,
};

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

fn detect() -> Result<Vec<f64>, &'static str> {
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
                    return Err("No CPU cores found");
                };
                std::thread::sleep(Duration::from_millis(WAIT_MS));
                Some(times1)
            },
        }
    };
    let Some(times1) = times1 else {
        return Err("No CPU cores found");
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
            None => return Err("No CPU cores found"),
        }
    }
    let Some(times2) = times2 else {
        return Err("CPU time did not increase. Try increasing wait time.");
    };
    if times1.len() != times2.len() {
        return Err("Unexpected CPU usage result");
    }

    Ok(times1
        .iter()
        .zip(&times2)
        .map(|((in_use1, total1), (in_use2, total2))| {
            (*in_use2 - *in_use1) as f64 / (*total2 - *total1) as f64 * 100.0
        })
        .collect())
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(percentages) = detect() else {
        return Vec::new();
    };
    if percentages.is_empty() {
        return Vec::new();
    }

    let avg = percentages.iter().sum::<f64>() / percentages.len() as f64;
    vec![Row::new(
        instance.display_name("cpuusage"),
        colorize_percent(avg, 50, 80, instance.color),
    )]
}

/// ffGenerateCPUUsageJsonResult: one percentage per core.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(percentages) => {
            let mut arr = Json::Array(Vec::new());
            for percent in percentages {
                arr.push_value(Json::F64(percent));
            }
            JsonOutcome::Result(arr)
        },
    }
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

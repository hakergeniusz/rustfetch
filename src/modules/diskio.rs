//! `diskio` module: per-device read/write rates, ported from diskio_linux.c.
//! Physical disks are the /sys/block entries that expose a `device` dir;
//! counters come from the block stat file (sectors scaled by 512) and the
//! rate is the delta over the 250ms sample window.

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{instance::Instance, modules::Row, util};

const WAIT_MS: u64 = 250;

#[derive(Clone)]
struct DiskIoSample {
    name: String,
    dev_path: String,
    bytes_read: u64,
    bytes_written: u64,
    read_count: u64,
    write_count: u64,
}

/// "vendor model" or the block device name; nvme namespaces with multiple
/// namespaces get a " - N" suffix.
fn device_name(device_dir: &Path, dev_name: &str) -> String {
    let mut name = util::read_trimmed_in(device_dir, "vendor").unwrap_or_default();
    if !name.is_empty() {
        name.push(' ');
    }
    name.push_str(&util::read_trimmed_in(device_dir, "model").unwrap_or_default());

    if name.trim().is_empty() {
        return dev_name.to_string();
    }
    let name = name.trim().to_string();

    if let Some(rest) = dev_name.strip_prefix("nvme")
        && let Some((devid, nsid)) = rest.split_once('n')
        && let (Ok(devid), Ok(nsid)) = (devid.parse::<u32>(), nsid.parse::<u32>())
    {
        let multi_ns = nsid > 1 || device_dir.join(format!("nvme{devid}n2")).exists();
        if multi_ns {
            return format!("{name} - {nsid}");
        }
    }
    name
}

fn get_counters() -> Option<Vec<DiskIoSample>> {
    let block_dir = Path::new("/sys/block");
    let mut result = Vec::new();
    for entry in std::fs::read_dir(block_dir).ok()? {
        let entry = entry.ok()?;
        let dev_name = entry.file_name().to_string_lossy().into_owned();
        if dev_name.starts_with('.') {
            continue;
        }
        let device_dir = entry.path().join("device");
        if !device_dir.is_dir() {
            continue; // virtual device
        }

        let stat = util::read_trimmed_in(&entry.path(), "stat")?;
        let fields: Vec<u64> = stat.split_whitespace().filter_map(|t| t.parse().ok()).collect();
        if fields.len() < 7 {
            continue;
        }
        // I/Os, merges, sectors, ticks, ... — reads at [0]/[2], writes at [4]/[6].
        result.push(DiskIoSample {
            name: device_name(&device_dir, &dev_name),
            dev_path: format!("/dev/{dev_name}"),
            bytes_read: fields[2] * 512,
            bytes_written: fields[6] * 512,
            read_count: fields[0],
            write_count: fields[4],
        });
    }
    Some(result)
}

static BASELINE: Mutex<Option<(Vec<DiskIoSample>, Instant)>> = Mutex::new(None);

/// ffPrepareDiskIO: take the first counter snapshot early.
pub(crate) fn prepare() {
    let mut baseline = match BASELINE.lock() {
        Ok(baseline) => baseline,
        Err(poisoned) => poisoned.into_inner(),
    };
    if baseline.is_none()
        && let Some(counters) = get_counters()
    {
        *baseline = Some((counters, Instant::now()));
    }
}

fn apply_rates(samples: &mut [DiskIoSample], prev: &[DiskIoSample], elapsed_ms: u64) -> bool {
    if samples.len() != prev.len() {
        return false;
    }
    for (curr, prev) in samples.iter_mut().zip(prev) {
        if curr.dev_path != prev.dev_path {
            return false;
        }
        let rate = |value: u64, prev: u64| {
            (value.saturating_sub(prev) as f64 * 1000.0 / elapsed_ms as f64) as u64
        };
        curr.bytes_read = rate(curr.bytes_read, prev.bytes_read);
        curr.bytes_written = rate(curr.bytes_written, prev.bytes_written);
        curr.read_count = rate(curr.read_count, prev.read_count);
        curr.write_count = rate(curr.write_count, prev.write_count);
    }
    true
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let times1 = {
        let mut baseline = match BASELINE.lock() {
            Ok(baseline) => baseline,
            Err(poisoned) => poisoned.into_inner(),
        };
        match baseline.take() {
            Some((counters, start)) => {
                let elapsed = start.elapsed();
                if elapsed.as_millis() < WAIT_MS as u128 {
                    std::thread::sleep(Duration::from_millis(WAIT_MS) - elapsed);
                }
                Some((counters, start.elapsed().as_millis() as u64))
            },
            None => {
                let Some(counters) = get_counters() else {
                    return Vec::new();
                };
                std::thread::sleep(Duration::from_millis(WAIT_MS));
                Some((counters, WAIT_MS))
            },
        }
    };
    let Some((counters1, elapsed_ms)) = times1 else {
        return Vec::new();
    };
    if counters1.is_empty() {
        return Vec::new();
    }

    let mut counters2 = match get_counters() {
        Some(counters) => counters,
        None => return vec![Row::new("DiskIO", "opendir(\"/sys/block/\") == nullptr")],
    };
    if !apply_rates(&mut counters2, &counters1, elapsed_ms) {
        return Vec::new();
    }

    counters2.sort_by(|a, b| a.name.cmp(&b.name));

    counters2
        .iter()
        .map(|dev| {
            let key_name = if dev.name.is_empty() {
                dev.dev_path.clone()
            } else {
                dev.name.clone()
            };
            Row::new(
                &format!("Disk I/O ({key_name})"),
                format!(
                    "{}/s (R) - {}/s (W)",
                    util::fmt_bytes(dev.bytes_read),
                    util::fmt_bytes(dev.bytes_written)
                ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_devices_and_nvme_namespaces() {
        use std::path::PathBuf;
        // No vendor/model files: the block device name stands in.
        assert_eq!(super::device_name(&PathBuf::from("/nonexistent"), "sda"), "sda");
    }

    #[test]
    fn rates_are_deltas_over_elapsed() {
        let prev = super::DiskIoSample {
            name: "Samsung SSD".to_string(),
            dev_path: "/dev/sda".to_string(),
            bytes_read: 1000,
            bytes_written: 0,
            read_count: 10,
            write_count: 0,
        };
        let mut curr = prev.clone();
        curr.bytes_read = 3000;
        let mut samples = [curr.clone()];
        assert!(super::apply_rates(&mut samples, &[prev], 1000));
        assert_eq!(samples[0].bytes_read, 2000);
        assert_eq!(samples[0].bytes_written, 0);
    }
}

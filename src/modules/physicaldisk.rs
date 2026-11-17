//! `physicaldisk` module: storage devices from /sys/class/block, keyed by
//! model like fastfetch ("Physical Disk (WDC PC SN530...): 476.94 GiB
//! [SSD, Fixed]"). Virtual devices (zram, dm) are tagged Virtual.

use crate::{instance::Instance, modules::Row, util};

/// KernelRAM-ish block devices that have no device model at all; zram and
/// device-mapper stay visible as Virtual, the rest is plumbing.
const EXCLUDED_PREFIXES: &[&str] = &["loop", "ram", "md", "sr", "fd", "nbd"];

fn read_u64(path: &std::path::Path, file: &str) -> Option<u64> {
    util::read_trimmed_in(path, file)?.parse().ok()
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(entries) = std::fs::read_dir("/sys/class/block") else {
        return Vec::new();
    };

    let mut disks = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if EXCLUDED_PREFIXES.iter().any(|prefix| name.starts_with(prefix)) {
            continue;
        }
        // partitions carry a `partition` marker; whole disks do not
        if entry.path().join("partition").exists() {
            continue;
        }
        let Some(size) = read_u64(&entry.path(), "size") else {
            continue;
        };

        let device = entry.path().join("device");
        let model = util::read_trimmed_in(&device, "model");
        let rotational = read_u64(&entry.path(), "queue/rotational").unwrap_or(1);
        let removable = read_u64(&entry.path(), "removable").unwrap_or(0);

        let mut types = Vec::new();
        match &model {
            Some(_) => types.push(if rotational == 1 { "HDD" } else { "SSD" }),
            None => types.push("Virtual"),
        };
        types.push(if removable == 1 { "Removable" } else { "Fixed" });

        let value = format!("{} [{}]", util::fmt_bytes(size * 512), types.join(", "));
        let key_name = model.unwrap_or(name);
        disks.push((key_name, value));
    }
    disks.sort();

    disks
        .into_iter()
        .map(|(name, value)| Row::new(&format!("Physical Disk ({name})"), value))
        .collect()
}

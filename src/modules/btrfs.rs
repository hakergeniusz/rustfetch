//! `btrfs` module: mounted btrfs filesystems from /sys/fs/btrfs, ported
//! from btrfs_linux.c. Only what the default output prints is parsed:
//! the label, the per-device sizes and the data/metadata/system
//! allocation profiles; features and generation are custom-format only.

use std::path::Path;

use crate::{
    instance::Instance,
    modules::{colorize_percent, Row},
    util,
};

/// The UUID directory name length ("00000000-0000-0000-0000-000000000000").
const UUID_LEN: usize = 36;

/// Profile probe order with the redundancy factor each contributes to
/// the used/allocated totals; raid5/6 count once like upstream.
const PROFILES: [(&str, u64); 9] = [
    ("single", 1),
    ("dup", 2),
    ("raid0", 1),
    ("raid1", 2),
    ("raid10", 2),
    ("raid1c3", 3),
    ("raid1c4", 4),
    ("raid5", 1),
    ("raid6", 1),
];

#[derive(Debug)]
struct Allocation {
    total: u64,
    used: u64,
    copies: u64,
}

#[derive(Debug)]
struct Volume {
    label: String,
    total_size: u64,
    allocations: [Allocation; 3],
}

/// Upstream only keeps names whose length equals a UUID's.
fn is_uuid(name: &str) -> bool {
    name.len() == UUID_LEN
}

fn read_u64(path: &Path) -> Option<u64> {
    util::read_file(path.to_str()?)?
        .trim()
        .parse()
        .ok()
}

/// The first profile directory present under allocation/<type>/.
fn detect_profile(type_dir: &Path) -> u64 {
    for (name, copies) in PROFILES {
        if type_dir.join(name).exists() {
            return copies;
        }
    }
    1 // "unknown" behaves like single
}

/// Missing files count as zero, like upstream's ignored error return.
fn detect_allocation(root: &Path) -> [Allocation; 3] {
    let alloc_dir = root.join("allocation");
    ["data", "metadata", "system"].map(|type_| {
        let type_dir = alloc_dir.join(type_);
        Allocation {
            total: read_u64(&type_dir.join("total_bytes")).unwrap_or(0),
            used: read_u64(&type_dir.join("bytes_used")).unwrap_or(0),
            copies: detect_profile(&type_dir),
        }
    })
}

fn detect() -> Option<Vec<Volume>> {
    let mut volumes = Vec::new();
    for entry in std::fs::read_dir("/sys/fs/btrfs").ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || !is_uuid(&name) {
            continue;
        }
        let root = entry.path();

        // Devices are symlinks into /sys/devices; each exposes size in
        // 512-byte sectors.
        let mut total_size = 0;
        if let Ok(devices) = std::fs::read_dir(root.join("devices")) {
            for device in devices.flatten() {
                if device.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                if let Some(size) = read_u64(&device.path().join("size")) {
                    total_size += size * 512;
                }
            }
        }

        let label = util::read_file(root.join("label").to_str()?)
            .unwrap_or_default()
            .trim_end()
            .to_string();
        let allocations = detect_allocation(&root);

        volumes.push(Volume {
            label,
            total_size,
            allocations,
        });
    }
    Some(volumes)
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Some(volumes) = detect() else {
        // "opendir(\"/sys/fs/btrfs\") == nullptr" is an error row, hidden
        // by default.
        return Vec::new();
    };
    if volumes.is_empty() {
        // "No btrfs drive found" is an error row, hidden by default.
        return Vec::new();
    }

    volumes
        .iter()
        .map(|volume| {
            let key = if volume.label.is_empty() {
                "BTRFS".to_string()
            } else {
                format!("BTRFS ({})", volume.label)
            };
            let mut used = 0;
            let mut allocated = 0;
            for allocation in &volume.allocations {
                used += allocation.used * allocation.copies;
                allocated += allocation.total * allocation.copies;
            }
            let total = volume.total_size;
            let used_pct = if total > 0 { used as f64 / total as f64 * 100.0 } else { 0.0 };
            let allocated_pct = if total > 0 {
                allocated as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            Row::new(
                &key,
                format!(
                    "{} / {} ({}, {} allocated)",
                    util::fmt_bytes(used),
                    util::fmt_bytes(total),
                    colorize_percent(used_pct, 50, 80, instance.color),
                    colorize_percent(allocated_pct, 50, 80, instance.color),
                ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_uuid_length_names_only() {
        assert!(super::is_uuid("35b43b06-4724-4371-93f6-44bf4d729e06"));
        assert!(!super::is_uuid("features"));
        assert!(!super::is_uuid("35b43b06-4724-4371-93f6-44bf4d729e0"));
    }

    #[test]
    fn profiles_carry_upstream_factors() {
        assert_eq!(
            super::PROFILES,
            [
                ("single", 1),
                ("dup", 2),
                ("raid0", 1),
                ("raid1", 2),
                ("raid10", 2),
                ("raid1c3", 3),
                ("raid1c4", 4),
                ("raid5", 1),
                ("raid6", 1),
            ]
        );
    }

    #[test]
    fn totals_multiply_by_copies() {
        let allocations = [
            super::Allocation { total: 426283892736, used: 346880049152, copies: 1 },
            super::Allocation { total: 11811160064, used: 8458993664, copies: 2 },
            super::Allocation { total: 8388608, used: 65536, copies: 2 },
        ];
        let mut used = 0;
        let mut allocated = 0;
        for allocation in &allocations {
            used += allocation.used * allocation.copies;
            allocated += allocation.total * allocation.copies;
        }
        assert_eq!(used, 363798167552); // 338.81 GiB
        assert_eq!(allocated, 449922990080); // 419 GiB
    }
}

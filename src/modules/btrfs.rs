//! `btrfs` module: mounted btrfs filesystems from /sys/fs/btrfs, ported
//! from btrfs_linux.c. The UUID-named directories carry the label, the
//! per-device sizes, the enabled features and the data/metadata/system
//! allocation profiles.

use std::path::Path;

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::{Row, colorize_percent},
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
    /// "data" / "metadata" / "system".
    type_: &'static str,
    /// "single" / "dup" / "raidN" / "unknown".
    profile: &'static str,
    copies: u64,
    total: u64,
    used: u64,
}

#[derive(Debug)]
struct Volume {
    uuid: String,
    name: String,
    devices: String,
    features: String,
    generation: u32,
    node_size: u32,
    sector_size: u32,
    total_size: u64,
    /// global_rsv_reserved (used) and global_rsv_size (total).
    global_reservation_used: u64,
    global_reservation_total: u64,
    allocations: [Allocation; 3],
}

/// Upstream only keeps names whose length equals a UUID's.
fn is_uuid(name: &str) -> bool {
    name.len() == UUID_LEN
}

fn read_u64(path: &Path) -> Option<u64> {
    util::read_file(path.to_str()?)?.trim().parse().ok()
}

/// The first profile directory present under allocation/<type>/, with the
/// redundancy factor each contributes.
fn detect_profile(type_dir: &Path) -> (&'static str, u64) {
    for (name, copies) in PROFILES {
        if type_dir.join(name).is_dir() {
            return (name, copies);
        }
    }
    ("unknown", 1)
}

/// Missing files count as zero, like upstream's ignored error return.
fn detect_allocation(root: &Path) -> [Allocation; 3] {
    let alloc_dir = root.join("allocation");
    ["data", "metadata", "system"].map(|type_| {
        let type_dir = alloc_dir.join(type_);
        let (profile, copies) = detect_profile(&type_dir);
        Allocation {
            type_,
            profile,
            copies,
            total: read_u64(&type_dir.join("total_bytes")).unwrap_or(0),
            used: read_u64(&type_dir.join("bytes_used")).unwrap_or(0),
        }
    })
}

/// The comma-joined directory entry names, readdir order (dot entries
/// skipped), like upstream's enumerateDevices/enumerateFeatures.
fn join_dir_names(dir: &Path) -> String {
    let mut joined = String::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if !joined.is_empty() {
                joined.push(',');
            }
            joined.push_str(&name);
        }
    }
    joined
}

fn detect() -> Result<Vec<Volume>, &'static str> {
    let Ok(entries) = std::fs::read_dir("/sys/fs/btrfs") else {
        return Err("opendir(\"/sys/fs/btrfs\") == nullptr");
    };

    let mut volumes = Vec::new();
    for entry in entries.flatten() {
        let uuid = entry.file_name().to_string_lossy().into_owned();
        if uuid.starts_with('.') || !is_uuid(&uuid) {
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

        let name = util::read_file(root.join("label").to_str().unwrap_or(""))
            .unwrap_or_default()
            .trim_end()
            .to_string();
        let allocations = detect_allocation(&root);
        // global_rsv_reserved is the used part of the reservation,
        // global_rsv_size its total.
        let global_reservation_used =
            read_u64(&root.join("allocation/global_rsv_reserved")).unwrap_or(0);
        let global_reservation_total =
            read_u64(&root.join("allocation/global_rsv_size")).unwrap_or(0);

        volumes.push(Volume {
            uuid,
            name,
            devices: join_dir_names(&root.join("devices")),
            features: join_dir_names(&root.join("features")),
            generation: read_u64(&root.join("generation")).unwrap_or(0) as u32,
            node_size: read_u64(&root.join("nodesize")).unwrap_or(0) as u32,
            sector_size: read_u64(&root.join("sectorsize")).unwrap_or(0) as u32,
            total_size,
            global_reservation_used,
            global_reservation_total,
            allocations,
        });
    }
    Ok(volumes)
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(volumes) = detect() else {
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
            let key = if volume.name.is_empty() {
                "BTRFS".to_string()
            } else {
                format!("{} ({})", instance.display_name("btrfs"), volume.name)
            };
            let mut used = 0;
            let mut allocated = 0;
            for allocation in &volume.allocations {
                used += allocation.used * allocation.copies;
                allocated += allocation.total * allocation.copies;
            }
            let total = volume.total_size;
            let used_pct = if total > 0 {
                used as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            let allocated_pct = if total > 0 {
                allocated as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            let mut used_pretty = String::new();
            crate::format::size_append_num(&mut used_pretty, used, &instance.display_format);
            let mut total_pretty = String::new();
            crate::format::size_append_num(&mut total_pretty, total, &instance.display_format);
            Row::new(
                &key,
                format!(
                    "{} / {} ({}, {} allocated)",
                    used_pretty,
                    total_pretty,
                    colorize_percent(used_pct, 50, 80, instance.color),
                    colorize_percent(allocated_pct, 50, 80, instance.color),
                ),
            )
        })
        .collect()
}

/// ffGenerateBtrfsJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(volumes) => {
            let mut arr = Json::Array(Vec::new());
            for volume in &volumes {
                let mut obj = Json::obj();
                obj.push("name", volume.name.clone());
                obj.push("uuid", volume.uuid.clone());
                obj.push("devices", volume.devices.clone());
                obj.push("features", volume.features.clone());
                obj.push("generation", volume.generation as u64);
                obj.push("nodeSize", volume.node_size as u64);
                obj.push("sectorSize", volume.sector_size as u64);
                obj.push("totalSize", volume.total_size);
                let mut reservation = Json::obj();
                reservation.push("used", volume.global_reservation_used);
                reservation.push("total", volume.global_reservation_total);
                obj.push("globalReservation", reservation);

                let mut allocation = Json::Array(Vec::new());
                for alloc in &volume.allocations {
                    let mut item = Json::obj();
                    item.push("type", alloc.type_);
                    item.push("profile", alloc.profile);
                    item.push("copies", alloc.copies);
                    item.push("used", alloc.used);
                    item.push("total", alloc.total);
                    allocation.push_value(item);
                }
                obj.push("allocation", allocation);
                arr.push_value(obj);
            }
            JsonOutcome::Result(arr)
        },
    }
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
            super::Allocation { type_: "data", profile: "single", copies: 1, total: 426283892736, used: 346880049152 },
            super::Allocation { type_: "metadata", profile: "dup", copies: 2, total: 11811160064, used: 8458993664 },
            super::Allocation { type_: "system", profile: "dup", copies: 2, total: 8388608, used: 65536 },
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

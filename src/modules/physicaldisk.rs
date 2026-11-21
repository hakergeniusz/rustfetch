//! `physicaldisk` module: whole-disk devices from /sys/block, ported from
//! physicaldisk_linux.c. Zero-size entries (upstream's UNUSED hideType
//! default) are skipped; devices without a `device/` sysfs dir are Virtual.
//! The name is "vendor model" with the NVMe multi-namespace suffix, the
//! interconnect comes from the device symlink or the transport file.

use std::path::Path;

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

const HDD: u8 = 1 << 0;
const SSD: u8 = 1 << 1;
const VIRTUAL: u8 = 1 << 2;
const FIXED: u8 = 1 << 3;
const REMOVABLE: u8 = 1 << 4;
const READWRITE: u8 = 1 << 5;
const READONLY: u8 = 1 << 6;
const UNUSED: u8 = 1 << 7;

/// FF_PHYSICALDISK_TEMP_UNSET; set only when the config asks for temp.
const TEMP_UNSET: f64 = f64::MIN;

/// Module options: temp off by default, tempConfig {50, 70}.
#[derive(Default)]
pub(crate) struct PhysicalDiskOptions {
    temp: bool,
    temp_range: super::TempRange,
}

impl PhysicalDiskOptions {
    fn from_instance(instance: &Instance) -> Self {
        let mut options = Self { temp: false, temp_range: super::TempRange { green: 50, yellow: 70 } };
        let Some(entry) = instance.entry else {
            return options;
        };
        if let Some(value) = entry.get("temp") {
            options.temp = super::parse_temp(value, &mut options.temp_range);
        }
        options
    }
}

#[derive(Default)]
struct Disk {
    name: String,
    interconnect: String,
    serial: String,
    dev_path: String,
    revision: String,
    type_bits: u8,
    size: u64,
    /// The UNSET sentinel unless the config asked for the temperature.
    temperature: f64,
}

/// The first byte of a sysfs attribute; upstream reads exactly one char.
fn first_byte(dir: &Path, file: &str) -> Option<u8> {
    std::fs::read(dir.join(file)).ok()?.first().copied()
}

/// A right-trimmed sysfs string attribute.
fn attr_trimmed_right(dir: &Path, file: &str) -> Option<String> {
    let content = util::read_file(dir.join(file).to_str()?)?;
    Some(content.trim_end().to_string())
}

/// detectNvmeTemp over the hwmonN/temp1_input siblings (millidegrees).
fn detect_nvme_temp(device: &Path) -> f64 {
    for c in b'0'..=b'9' {
        let path = device.join(format!("hwmon{}/temp1_input", c as char));
        if let Some(content) = util::read_file(path.to_str().unwrap_or(""))
            && let Ok(temp) = content.trim().parse::<f64>()
        {
            return if temp > 0.0 && temp < 10_000_000.0 { temp / 1000.0 } else { TEMP_UNSET };
        }
    }
    TEMP_UNSET
}

/// The NVMe multi-namespace suffix: nvme0n1 on a drive with several
/// namespaces (nsid > 1, or a sibling nvme0n2) reports " - 1".
fn append_nvme_namespace_suffix(name: &mut String, dev_name: &str) {
    // sscanf(devName, "nvme%dn%d", &devid, &nsid) == 2
    let Some((devid, nsid)) = dev_name
        .strip_prefix("nvme")
        .and_then(|rest| rest.split_once('n'))
    else {
        return;
    };
    let (Ok(devid), Ok(nsid)) = (devid.parse::<u32>(), nsid.parse::<u32>()) else {
        return;
    };
    let multi_ns = nsid > 1 || Path::new(&format!("/dev/nvme{devid}n2")).exists();
    if multi_ns {
        name.push_str(&format!(" - {nsid}"));
    }
}

fn parse_physical_disk(block_dir: &Path, dev_name: &str, options: &PhysicalDiskOptions) -> Option<Disk> {
    let size = first_byte_read_u64(block_dir, "size").unwrap_or(0) * 512;

    // The UNUSED hideType is on by default.
    if size == 0 {
        return None;
    }

    let device = block_dir.join("device");
    let has_device = device.is_dir();

    let mut name = if has_device {
        let mut name = attr_trimmed_right(&device, "vendor").unwrap_or_default();
        if !name.is_empty() {
            name.push(' ');
        }
        name.push_str(&attr_trimmed_right(&device, "model").unwrap_or_default());
        if name.trim_end().is_empty() {
            dev_name.to_string()
        } else {
            name.trim_end().to_string()
        }
    } else {
        dev_name.to_string()
    };
    if dev_name.starts_with("nvme") {
        append_nvme_namespace_suffix(&mut name, dev_name);
    }

    let mut type_bits = 0u8;
    if !has_device {
        type_bits |= VIRTUAL;
    }

    let mut disk = Disk {
        name,
        dev_path: format!("/dev/{dev_name}"),
        type_bits,
        size,
        temperature: TEMP_UNSET,
        ..Disk::default()
    };

    let mut is_virtio = false;
    if has_device {
        if dev_name.starts_with("nvme") {
            disk.interconnect = "NVMe".to_string();
        } else if dev_name.starts_with("mmcblk") {
            disk.interconnect = "MMC".to_string();
        } else if let Ok(real) = std::fs::canonicalize(&device) {
            let real = real.to_string_lossy();
            if real.contains("/usb") {
                disk.interconnect = "USB".to_string();
            } else if real.contains("/ata") {
                disk.interconnect = "ATA".to_string();
            } else if real.contains("/scsi") {
                disk.interconnect = "SCSI".to_string();
            } else if real.contains("/nvme") {
                disk.interconnect = "NVMe".to_string();
            } else if real.contains("/virtio") {
                disk.interconnect = "VirtIO".to_string();
                is_virtio = true; // VirtIO devices are virtual, but we still want to report it
            } else if let Some(transport) = attr_trimmed_right(&device, "transport") {
                disk.interconnect = transport;
            }
        }
    } else {
        disk.interconnect = "Virtual".to_string();
    }

    if has_device && !is_virtio {
        if options.temp {
            disk.temperature = detect_nvme_temp(&device);
        }

        // An unreadable rotational file keeps the '1' initializer: HDD.
        if first_byte(block_dir, "queue/rotational") != Some(b'0') {
            disk.type_bits |= HDD;
        } else {
            disk.type_bits |= SSD;
        }

        if let Some(serial) = util::read_file(device.join("serial").to_str().unwrap_or("")) {
            disk.serial = serial.trim().to_string();
        }

        if let Some(revision) = attr_trimmed_right(&device, "firmware_rev")
            .or_else(|| attr_trimmed_right(&device, "rev"))
        {
            disk.revision = revision;
        }

        // The temperature stays unset: --temp is not exposed by this CLI.
    }

    if first_byte(block_dir, "removable") == Some(b'1') {
        disk.type_bits |= REMOVABLE;
    } else if first_byte(block_dir, "removable").is_some() {
        disk.type_bits |= FIXED;
    }

    if first_byte(block_dir, "ro") == Some(b'1') {
        disk.type_bits |= READONLY;
    } else if first_byte(block_dir, "ro").is_some() {
        disk.type_bits |= READWRITE;
    }

    Some(disk)
}

/// strtoull over the first whitespace-terminated token of "size".
fn first_byte_read_u64(dir: &Path, file: &str) -> Option<u64> {
    let content = util::read_file(dir.join(file).to_str()?)?;
    content.split_whitespace().next()?.parse().ok()
}

/// ffDetectPhysicalDisk over /sys/block.
fn detect(options: &PhysicalDiskOptions) -> Result<Vec<Disk>, &'static str> {
    let Ok(entries) = std::fs::read_dir("/sys/block/") else {
        return Err("opendir(\"/sys/block/\") == nullptr");
    };

    let mut result = Vec::new();
    for entry in entries.flatten() {
        let dev_name = entry.file_name().to_string_lossy().into_owned();
        if dev_name.starts_with('.') {
            continue;
        }
        if let Some(disk) = parse_physical_disk(&entry.path(), &dev_name, options) {
            result.push(disk);
        }
    }
    Ok(result)
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let options = PhysicalDiskOptions::from_instance(instance);
    let Ok(mut result) = detect(&options) else {
        return Vec::new();
    };
    result.sort_by(|a, b| a.name.cmp(&b.name));

    result
        .iter()
        .map(|dev| {
            let key_name = if dev.name.is_empty() { &dev.dev_path } else { &dev.name };
            let physical_type = if dev.type_bits & VIRTUAL != 0 {
                "Virtual"
            } else if dev.type_bits & HDD != 0 {
                "HDD"
            } else if dev.type_bits & SSD != 0 {
                "SSD"
            } else {
                ""
            };
            let removable_type = if dev.type_bits & REMOVABLE != 0 {
                "Removable"
            } else if dev.type_bits & FIXED != 0 {
                "Fixed"
            } else {
                ""
            };
            let read_only_type = if dev.type_bits & READONLY != 0 { "Read-only" } else { "" };

            let mut value = String::new();
            crate::format::size_append_num(&mut value, dev.size, &instance.display_format);
            if !physical_type.is_empty() || !removable_type.is_empty() || !read_only_type.is_empty()
            {
                value.push_str(" [");
                let mut first = true;
                for part in [physical_type, removable_type, read_only_type] {
                    if part.is_empty() {
                        continue;
                    }
                    if !first {
                        value.push_str(", ");
                    }
                    value.push_str(part);
                    first = false;
                }
                value.push(']');
            }

            if dev.temperature != TEMP_UNSET && !value.is_empty() {
                value.push_str(" - ");
                value.push_str(&super::temps_num(dev.temperature, options.temp_range.green, options.temp_range.yellow, instance.color));
            }
            Row::new(&format!("{} ({key_name})", instance.display_name("physicaldisk")), value)
        })
        .collect()
}

/// ffGeneratePhysicalDiskJsonResult; unlike the text path it does not sort,
/// keeping the sysfs scan order.
pub fn json(instance: &Instance) -> JsonOutcome {
    let options = PhysicalDiskOptions::from_instance(instance);
    let Ok(result) = detect(&options) else {
        return JsonOutcome::Error("opendir(\"/sys/block/\") == nullptr".to_string());
    };

    let mut arr = Json::Array(Vec::new());
    for dev in &result {
        let mut obj = Json::obj();
        obj.push("name", dev.name.clone());
        obj.push("devPath", dev.dev_path.clone());
        obj.push("interconnect", dev.interconnect.clone());

        if dev.type_bits & VIRTUAL != 0 {
            obj.push("kind", "Virtual");
        } else if dev.type_bits & HDD != 0 {
            obj.push("kind", "HDD");
        } else if dev.type_bits & SSD != 0 {
            obj.push("kind", "SSD");
        } else {
            obj.push("kind", Json::Null);
        }

        obj.push("size", dev.size);
        obj.push("serial", dev.serial.clone());

        if dev.type_bits & REMOVABLE != 0 {
            obj.push("removable", true);
        } else if dev.type_bits & FIXED != 0 {
            obj.push("removable", false);
        } else {
            obj.push("removable", Json::Null);
        }

        if dev.type_bits & READONLY != 0 {
            obj.push("readOnly", true);
        } else if dev.type_bits & READWRITE != 0 {
            obj.push("readOnly", false);
        } else {
            obj.push("readOnly", Json::Null);
        }

        obj.push("unknown", dev.type_bits & UNUSED != 0);
        obj.push("revision", dev.revision.clone());

        if dev.temperature != TEMP_UNSET {
            obj.push("temperature", dev.temperature);
        } else {
            obj.push("temperature", Json::Null);
        }
        arr.push_value(obj);
    }
    JsonOutcome::Result(arr)
}

#[cfg(test)]
mod tests {
    #[test]
    fn nvme_suffix_only_for_multi_namespace() {
        // nsid 1 without a sibling namespace: no suffix.
        let mut name = String::new();
        super::append_nvme_namespace_suffix(&mut name, "nvme9n1");
        assert_eq!(name, "");

        // nsid > 1 always suffixed, even without a sibling check.
        let mut name = String::new();
        super::append_nvme_namespace_suffix(&mut name, "nvme0n3");
        assert_eq!(name, " - 3");
    }
}

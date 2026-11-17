//! `gpu` module: GPUs from /sys/class/drm named via pci.ids, with subsystem
//! refinement. No processes spawned, no giant dependencies.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::{instance::Instance, modules::Row, util};

const PCI_IDS_PATHS: &[&str] =
    &["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids", "/var/lib/usbutils/pci.ids"];

/// PCI vendor ids known to host GPUs, including virtual machine adapters.
/// Values match pci.ids casing (`10de`), matching what normalize_id produces.
const GPU_VENDORS: &[&str] = &["1002", "8086", "10de", "1022", "15ad", "1234", "1af4", "1b36"];

/// sysfs ids come as `0x10de` while pci.ids keys are `10de`.
fn normalize_id(raw: &str) -> String {
    let lowered = raw.to_ascii_lowercase();
    lowered.strip_prefix("0x").unwrap_or(&lowered).to_string()
}

/// Parsed lookup tables from pci.ids.
struct PciNames {
    vendors: HashMap<String, String>,
    devices: HashMap<(String, String), String>,
    subsystems: HashMap<(String, String, String, String), String>,
}

impl PciNames {
    fn load() -> Option<Self> {
        let content = PCI_IDS_PATHS.iter().find_map(|path| util::read_file(path))?;

        let mut vendors = HashMap::new();
        let mut devices = HashMap::new();
        let mut subsystems = HashMap::new();
        let mut vendor = String::new();
        let mut device = String::new();

        let push_id = |id: &str, name: &str, map: &mut HashMap<String, String>| {
            if id.len() == 4 && id.chars().all(|c| c.is_ascii_hexdigit()) {
                map.insert(id.to_ascii_lowercase(), name.to_string());
            }
        };

        for line in content.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("\t\t") {
                // subsystem line: "<subvendor> <subdevice>  name"
                if let Some((ids, name)) = rest.split_once("  ") {
                    let mut fields = ids.split(' ');
                    let (Some(subvendor), Some(subdevice)) = (fields.next(), fields.next()) else {
                        continue;
                    };
                    subsystems.insert(
                        (vendor.clone(), device.clone(), subvendor.to_ascii_lowercase(), subdevice.to_ascii_lowercase()),
                        name.to_string(),
                    );
                }
            } else if let Some(rest) = line.strip_prefix('\t') {
                // device line: "<device-id>  name"
                if let Some((id, name)) = rest.split_once("  ") {
                    if id.len() == 4 && id.chars().all(|c| c.is_ascii_hexdigit()) {
                        device = id.to_ascii_lowercase();
                        devices.insert((vendor.clone(), device.clone()), name.to_string());
                    }
                }
            } else if let Some((id, name)) = line.split_once("  ") {
                vendor = id.to_ascii_lowercase();
                push_id(id, name, &mut vendors);
            }
        }

        Some(Self { vendors, devices, subsystems })
    }

    fn device_name(&self, vendor: &str, device: &str) -> Option<&String> {
        self.devices.get(&(vendor.to_string(), device.to_string()))
    }

    fn vendor_name(&self, vendor: &str) -> Option<&String> {
        self.vendors.get(vendor)
    }

    fn subsystem_name(
        &self,
        vendor: &str,
        device: &str,
        subvendor: &str,
        subdevice: &str,
    ) -> Option<&String> {
        self.subsystems.get(&(vendor.to_string(), device.to_string(), subvendor.to_string(), subdevice.to_string()))
    }
}

/// Paths of /sys/class/drm cardN/device for real PCI devices.
fn gpu_cards() -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut cards: Vec<std::path::PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("card") && name[4..].chars().all(|c| c.is_ascii_digit())
        })
        .map(|entry| entry.path().join("device"))
        .filter(|device| device.join("vendor").exists())
        .collect();
    cards.sort();
    cards
}

/// Short vendor name: pci.ids names AMD as "Advanced Micro Devices, Inc.
/// [AMD/ATI]" and NVIDIA as "NVIDIA Corporation"; both read better short.
fn short_vendor(name: &str) -> &str {
    if let Some(start) = name.find('[') {
        let inner = &name[start + 1..];
        if let Some(end) = inner.find(']') {
            let alias = &inner[..end];
            if let Some((alias, _)) = alias.split_once('/') {
                return alias;
            }
            return alias;
        }
    }
    for suffix in [" Corporation", " Inc.", " Ltd.", " Ltd"] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            return stripped;
        }
    }
    name
}

/// pci.ids device names like "GA106 [GeForce RTX 3050 OEM]" carry the
/// marketing card name in brackets; subsystem names are already bare.
fn bracketed(name: &str) -> Option<String> {
    let start = name.find('[')? + 1;
    let inner = &name[start..];
    let end = inner.rfind(']')?;
    let inner = &inner[..end];
    (!inner.is_empty()).then(|| inner.to_string())
}

fn describe_card(
    lookup: Option<&PciNames>,
    vendor_id: &str,
    device_id: &str,
    device: &Path,
) -> String {
    let subvendor = util::read_trimmed_in(device, "subsystem_vendor").map(|v| normalize_id(&v));
    let subdevice = util::read_trimmed_in(device, "subsystem_device").map(|v| normalize_id(&v));

    if let Some(lookup) = lookup {
        let subsystem = match (&subvendor, &subdevice) {
            (Some(subvendor), Some(subdevice)) => lookup.subsystem_name(vendor_id, device_id, subvendor, subdevice),
            _ => None,
        };
        if let (Some(vendor_name), Some(device_name)) =
            (lookup.vendor_name(vendor_id), lookup.device_name(vendor_id, device_id))
        {
            let device_display =
                subsystem.cloned().or_else(|| bracketed(device_name)).unwrap_or_else(|| device_name.clone());
            return format!("{} {}", short_vendor(vendor_name), device_display);
        }
    }

    format!("{vendor_id} {device_id}")
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let lookup = PciNames::load();
    let mut rows = Vec::new();
    let mut seen = HashSet::new();

    for device in gpu_cards() {
        let Some(vendor_id) = util::read_trimmed_in(&device, "vendor").map(|v| normalize_id(&v)) else {
            continue;
        };
        if !GPU_VENDORS.contains(&vendor_id.as_str()) {
            continue;
        }
        let Some(device_id) = util::read_trimmed_in(&device, "device").map(|v| normalize_id(&v)) else {
            continue;
        };

        let name = describe_card(lookup.as_ref(), &vendor_id, &device_id, &device);
        if seen.insert(name.clone()) {
            rows.push(Row::new("GPU", name));
        }
    }
    rows
}

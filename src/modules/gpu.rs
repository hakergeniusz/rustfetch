//! `gpu` module: GPUs from /sys/class/drm (DRM cards) with the
//! /sys/bus/pci/devices fallback, ported from detection/gpu/gpu_linux.c and
//! gpu_pci.c. Names come from pci.ids (bracket content preferred), types
//! from the vendor heuristics. The driver-specific backends (NVML etc.) and
//! the Vulkan/OpenCL/EGL/OpenGL fallbacks are not ported: they need shared
//! libraries and only run when native detection finds nothing.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

const PCI_IDS_PATHS: &[&str] = &[
    "/usr/share/hwdata/pci.ids",
    "/usr/share/misc/pci.ids",
    "/usr/local/share/hwdata/pci.ids",
];

// FF_GPU_TYPE values.
const GPU_TYPE_UNKNOWN: u8 = 0;
const GPU_TYPE_INTEGRATED: u8 = 1;
const GPU_TYPE_DISCRETE: u8 = 2;

/// ffGPUPciAddr2Id.
const fn pci_addr_to_id(domain: u64, bus: u64, device: u64, function: u64) -> u64 {
    (domain << 16) | (bus << 8) | (device << 3) | function
}

/// ffGPUGetVendorString: the short vendor name for a PCI vendor id.
const fn vendor_string(vendor_id: u16) -> Option<&'static str> {
    Some(match vendor_id {
        0x106b => "Apple",
        0x1002 | 0x1022 | 0x1dd8 => "AMD",
        0x8086 | 0x8087 | 0x03e7 => "Intel",
        0x0955 | 0x10de | 0x12d2 => "NVIDIA",
        0x1ed5 => "Moore Threads",
        0x17cb | 0x5143 => "Qualcomm",
        0x14c3 => "MTK",
        0x15ad => "VMware",
        0x1af4 => "RedHat",
        0x1ab8 => "Parallels",
        0x1414 => "Microsoft",
        0x108e => "Oracle",
        0x182f | 0x14e4 => "Broadcom",
        0x0014 => "Loongson",
        0x0731 => "Jingjia Micro",
        0x19e5 => "Huawei",
        0x1d17 => "Zhaoxin",
        0x1234 => "QEMU",
        _ => return None,
    })
}

pub struct GpuResult {
    pub index: Option<u32>,
    pub core_count: Option<i32>,
    pub core_usage: Option<f64>,
    pub dedicated_total: Option<u64>,
    pub dedicated_used: Option<u64>,
    pub shared_total: Option<u64>,
    pub shared_used: Option<u64>,
    /// Empty means null in the JSON result.
    pub memory_type: String,
    pub driver: String,
    pub name: String,
    pub temperature: Option<f64>,
    pub gpu_type: u8,
    pub vendor: String,
    pub platform_api: String,
    /// 0 means unset (FF_GPU_FREQUENCY_UNSET).
    pub frequency: u32,
    pub device_id: u64,
    /// (gen, lanes) for max and current; all-zero means unset.
    pub pcie_max: (u16, u16),
    pub pcie_curr: (u16, u16),
}

impl Default for GpuResult {
    fn default() -> Self {
        Self {
            index: None,
            core_count: None,
            core_usage: None,
            dedicated_total: None,
            dedicated_used: None,
            shared_total: None,
            shared_used: None,
            memory_type: String::new(),
            driver: String::new(),
            name: String::new(),
            temperature: None,
            gpu_type: GPU_TYPE_UNKNOWN,
            vendor: String::new(),
            platform_api: String::new(),
            frequency: 0,
            device_id: 0,
            pcie_max: (0, 0),
            pcie_curr: (0, 0),
        }
    }
}

/// detectDriverFromSysfs: the driver symlink's basename, with the nvidia
/// open/proprietary remark and the module version appended.
fn detect_driver(device_dir: &std::path::Path, driver: &mut String) {
    let Ok(target) = std::fs::read_link(device_dir.join("driver")) else {
        return;
    };
    *driver = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    if driver == "nvidia"
        && let Some(version) = util::read_file("/proc/driver/nvidia/version")
    {
        if version.contains(" Open ") {
            driver.push_str(" (open source)");
        } else {
            driver.push_str(" (proprietary)");
        }
    }

    if let Some(version) = util::read_trimmed_in(device_dir, "driver/module/version") {
        driver.push(' ');
        driver.push_str(version.trim_end());
    } else if driver == "zx"
        && let Some(version) = util::read_trimmed_in(device_dir, "zx_info/driver_version")
    {
        driver.push(' ');
        driver.push_str(version.trim_end());
    }
}

/// The pci.ids lookup: vendor name (only fills an empty vendor), then the
/// device line with its bracket content preferred.
fn fill_vendor_and_name(gpu: &mut GpuResult, subclass: u8, vendor_id: u16, device_id: u16) {
    let Some(content) = PCI_IDS_PATHS.iter().find_map(|path| util::read_file(path)) else {
        return;
    };

    let needle = format!("\n{vendor_id:04x}  ");
    let Some(start) = content.find(&needle) else {
        return;
    };
    let start = start + needle.len();
    let vendor_end = content[start..].find('\n').map(|pos| start + pos).unwrap_or(content.len());

    if gpu.vendor.is_empty() {
        gpu.vendor = content[start..vendor_end].to_string();
    }

    // Device lines are `\tXXXX  name`; ids only grow inside the block, so
    // the walk stops at the first id greater than ours.
    let device_key = format!("{device_id:04x}");
    let mut rest = &content[vendor_end + 1..];
    while rest.starts_with('\t') || rest.starts_with('#') {
        let line_end = rest.find('\n').map(|pos| pos + 1).unwrap_or(rest.len());
        let line = &rest[..line_end];
        if line.starts_with('\t') && line.len() >= 8 {
            let id = &line[1..5];
            if id.as_bytes() > device_key.as_bytes() {
                break;
            }
            if id == device_key && line.as_bytes()[5] == b' ' && line.as_bytes()[6] == b' ' {
                let name = &line[7..line.len().saturating_sub(1)];
                if name.ends_with(']')
                    && let Some(opening) = name.rfind('[')
                {
                    let inner = &name[opening + 1..name.len() - 1];
                    if !inner.is_empty() {
                        gpu.name = inner.to_string();
                    }
                }
                if gpu.name.is_empty() {
                    gpu.name = name.to_string();
                }
                break;
            }
        }
        rest = &rest[line_end..];
    }

    if gpu.name.is_empty() {
        let subclass_str = match subclass {
            0 => " (VGA compatible)",
            1 => " (XGA compatible)",
            2 => " (3D)",
            _ => "",
        };
        gpu.name = format!(
            "{} Device {device_id:04X}{subclass_str}",
            if gpu.vendor.is_empty() { "Unknown" } else { &gpu.vendor }
        );
    }
}

/// ffGPUDetectTypeByVendorAndName: the vendor/name heuristics for GPUs the
/// specific detectors left unknown.
fn detect_type_by_vendor_and_name(gpu: &mut GpuResult) {
    if gpu.gpu_type != GPU_TYPE_UNKNOWN {
        return;
    }
    match gpu.vendor.as_str() {
        "NVIDIA" => {
            let lowered = gpu.name.to_ascii_lowercase();
            if ["geforce", "quadro", "tesla"]
                .iter()
                .any(|prefix| lowered.starts_with(prefix))
            {
                gpu.gpu_type = GPU_TYPE_DISCRETE;
            }
        },
        "Moore Threads" => {
            if gpu.name.to_ascii_lowercase().starts_with("mtt ") {
                gpu.gpu_type = GPU_TYPE_DISCRETE;
            }
        },
        "Intel" => {
            gpu.gpu_type = if gpu.device_id == pci_addr_to_id(0, 0, 2, 0) {
                GPU_TYPE_INTEGRATED
            } else {
                GPU_TYPE_DISCRETE
            };
        },
        "VMware" | "Parallels" => gpu.gpu_type = GPU_TYPE_INTEGRATED,
        _ => {},
    }
}

/// pciDetectIntelSpecific: the address-based type and the current max
/// frequency (the hwmon temperature needs --temp, which is off by default).
fn pci_detect_intel_specific(gpu: &mut GpuResult, device_dir: &std::path::Path, drm_key: Option<&str>) {
    gpu.gpu_type = if gpu.device_id == pci_addr_to_id(0, 0, 2, 0) {
        GPU_TYPE_INTEGRATED
    } else {
        GPU_TYPE_DISCRETE
    };

    let Some(drm_key) = drm_key else {
        return;
    };
    let path = if gpu.driver == "xe" {
        device_dir.join("tile0/gt0/freq0/max_freq")
    } else {
        device_dir.join(format!("drm/{drm_key}/gt_max_freq_mhz"))
    };
    if let Some(value) = util::read_trimmed_in(&path, "") {
        gpu.frequency = value.trim().parse().unwrap_or(0);
    }
}

/// pciDetectAmdSpecific: the APU check via in1_input; memory accounting
/// needs --driver-specific (off by default) and the hwmon temperature needs
/// --temp.
fn pci_detect_amd_specific(gpu: &mut GpuResult, device_dir: &std::path::Path) {
    if !device_dir.join("hwmon").exists() {
        return;
    }
    if gpu.driver != "amdgpu" {
        return;
    }
    if device_dir.join("hwmon/in1_input").exists() {
        gpu.gpu_type = GPU_TYPE_INTEGRATED;
    } else {
        gpu.gpu_type = GPU_TYPE_DISCRETE;
    }
}

/// detectPci: one display-class PCI device from its modalias.
fn detect_pci(gpus: &mut Vec<GpuResult>, modalias: &str, device_dir: &std::path::Path, mut drm_key: Option<String>) {
    // v%8xd%8xsv%8xsd%8xbc%2xsc%2x
    let Some(rest) = modalias.strip_prefix("pci:v") else {
        return;
    };
    fn take_hex(text: &str, width: usize) -> Option<(u32, &str)> {
        let value = text.get(..width)?;
        let value = u32::from_str_radix(value, 16).ok()?;
        Some((value, &text[width..]))
    }

    let Some((vendor_id, rest)) = take_hex(rest, 8) else { return };
    let Some(rest) = rest.strip_prefix('d') else { return };
    let Some((device_id, rest)) = take_hex(rest, 8) else { return };
    let Some(rest) = rest.strip_prefix("sv") else { return };
    let Some((_sub_vendor, rest)) = take_hex(rest, 8) else { return };
    let Some(rest) = rest.strip_prefix("sd") else { return };
    let Some((_sub_device, rest)) = take_hex(rest, 8) else { return };
    let Some(rest) = rest.strip_prefix("bc") else { return };
    let Some((class_id, rest)) = take_hex(rest, 2) else { return };
    let Some(rest) = rest.strip_prefix("sc") else { return };
    let Some((subclass_id, _)) = take_hex(rest, 2) else { return };
    let class_id = class_id as u8;
    let subclass_id = subclass_id as u8;

    if class_id != 0x03 {
        return; // Not a GPU device
    }

    let pci_addr = device_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut parts = pci_addr.split([':', '.']);
    let Some(domain) = parts.next().and_then(|v| u64::from_str_radix(v, 16).ok()) else { return };
    let Some(bus) = parts.next().and_then(|v| u64::from_str_radix(v, 16).ok()) else { return };
    let Some(device) = parts.next().and_then(|v| u64::from_str_radix(v, 16).ok()) else { return };
    let Some(function) = parts.next().and_then(|v| u64::from_str_radix(v, 16).ok()) else { return };

    if function > 0 && subclass_id == 0x80 {
        return; // Likely an auxiliary display controller (#2034)
    }

    let mut gpu = GpuResult {
        vendor: vendor_string(vendor_id as u16).unwrap_or("").to_string(),
        device_id: pci_addr_to_id(domain, bus, device, function),
        ..GpuResult::default()
    };

    if drm_key.is_none() {
        // The PCI fallback looks for a DRM card under the device dir.
        if let Ok(entries) = std::fs::read_dir(device_dir.join("drm")) {
            for entry in entries.flatten() {
                let entry_name = entry.file_name();
                let entry_name = entry_name.to_string_lossy();
                if entry_name.starts_with("card") {
                    drm_key = Some(entry_name.into_owned());
                    break;
                }
            }
        }
    }
    if let Some(drm_key) = &drm_key {
        gpu.platform_api = format!("DRM ({drm_key})");
    }

    detect_driver(device_dir, &mut gpu.driver);

    match gpu.vendor.as_str() {
        "AMD" => pci_detect_amd_specific(&mut gpu, device_dir),
        "Intel" => pci_detect_intel_specific(&mut gpu, device_dir, drm_key.as_deref()),
        "NVIDIA" if gpu.driver == "nouveau" => {
            // The temperature needs --temp; nouveau memory needs
            // --driver-specific; neither is exposed by this CLI.
        },
        "Zhaoxin" if gpu.driver.starts_with("zx") => {
            gpu.gpu_type = GPU_TYPE_INTEGRATED;
            if let Some(value) = util::read_trimmed_in(device_dir, "zx_info/eclk") {
                gpu.frequency = value.trim().parse().unwrap_or(0);
            }
        },
        // ffGPUDetectDriverSpecific needs --temp or --driver-specific; both
        // default off, so nothing runs here.
        _ => {},
    }

    if gpu.name.is_empty() {
        fill_vendor_and_name(&mut gpu, subclass_id, vendor_id as u16, device_id as u16);
    }
    detect_type_by_vendor_and_name(&mut gpu);

    gpus.push(gpu);
}

/// drmDetectGPUs: the DRM card directories.
fn detect_drm(gpus: &mut Vec<GpuResult>) -> Result<(), &'static str> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return Err("Failed to open `/sys/class/drm/`");
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("card") || name[4..].contains('-') {
            continue;
        }
        let device_dir = entry.path().join("device");
        let Some(modalias) = util::read_trimmed_in(&device_dir, "modalias") else {
            continue;
        };
        if modalias.starts_with("pci:") {
            detect_pci(gpus, &modalias, &device_dir, Some(name.into_owned()));
        } else if modalias.starts_with("of:") {
            // Open Firmware GPUs (embedded ARM boards) are not ported yet.
        }
    }
    Ok(())
}

/// pciDetectGPUs: every PCI device, without a DRM key until the device dir
/// exposes one.
fn detect_pci_bus(gpus: &mut Vec<GpuResult>) -> Result<(), &'static str> {
    let Ok(entries) = std::fs::read_dir("/sys/bus/pci/devices/") else {
        return Err("Failed to open `/sys/bus/pci/devices/`");
    };
    for entry in entries.flatten() {
        let device_dir = entry.path();
        let Some(modalias) = util::read_trimmed_in(&device_dir, "modalias") else {
            continue;
        };
        if modalias.starts_with("pci:") {
            detect_pci(gpus, &modalias, &device_dir, None);
        }
    }
    Ok(())
}

/// ffDetectGPU: WSL2's /dev/dxg first (an exact no-op off WSL), then the
/// DRM cards, then the whole PCI bus. The Vulkan/OpenCL/EGL/OpenGL
/// fallbacks need shared libraries and are not ported.
pub fn detect() -> Result<Vec<GpuResult>, &'static str> {
    let mut gpus = Vec::new();

    // ffGPUDetectWsl2 fails fast without /dev/dxg.
    if std::path::Path::new("/dev/dxg").exists() {
        return Err("Failed to enumerate adapters with D3DKMTEnumAdapters2");
    }

    detect_drm(&mut gpus)?;
    if gpus.is_empty() {
        detect_pci_bus(&mut gpus)?;
    }
    if gpus.is_empty() {
        return Err("GPU detection failed");
    }
    Ok(gpus)
}

/// The text value: "{vendor} {name}", the vendor dropped when the name
/// already starts with it, plus the [type] tag.
fn item_value(gpu: &GpuResult) -> String {
    let mut value = String::new();
    if !gpu.vendor.is_empty() && !gpu.name.to_ascii_lowercase().starts_with(&gpu.vendor.to_ascii_lowercase()) {
        value.push_str(&gpu.vendor);
        value.push(' ');
    }
    value.push_str(&gpu.name);
    if let Some(core_count) = gpu.core_count {
        value.push_str(&format!(" ({core_count})"));
    }
    if gpu.frequency > 0 {
        value.push_str(&format!(" @ {}", util::fmt_freq(gpu.frequency)));
    }
    match gpu.gpu_type {
        GPU_TYPE_INTEGRATED => value.push_str(" [Integrated]"),
        GPU_TYPE_DISCRETE => value.push_str(" [Discrete]"),
        _ => {},
    }
    value
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(gpus) = detect() else {
        return Vec::new();
    };
    gpus.iter()
        .map(|gpu| Row::new("GPU", item_value(gpu)))
        .collect()
}

/// ffGenerateGPUJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(gpus) => {
            let mut arr = Json::Array(Vec::new());
            for gpu in &gpus {
                let mut obj = Json::obj();
                match gpu.index {
                    Some(index) => obj.push("index", index as u64),
                    None => obj.push("index", Json::Null),
                }
                match gpu.core_count {
                    Some(count) => obj.push("coreCount", count as i64),
                    None => obj.push("coreCount", Json::Null),
                }
                match gpu.core_usage {
                    Some(usage) => obj.push("coreUsage", usage),
                    None => obj.push("coreUsage", Json::Null),
                }

                let mut memory = Json::obj();
                let mut dedicated = Json::obj();
                match gpu.dedicated_total {
                    Some(total) => dedicated.push("total", total),
                    None => dedicated.push("total", Json::Null),
                }
                match gpu.dedicated_used {
                    Some(used) => dedicated.push("used", used),
                    None => dedicated.push("used", Json::Null),
                }
                memory.push("dedicated", dedicated);
                let mut shared = Json::obj();
                match gpu.shared_total {
                    Some(total) => shared.push("total", total),
                    None => shared.push("total", Json::Null),
                }
                match gpu.shared_used {
                    Some(used) => shared.push("used", used),
                    None => shared.push("used", Json::Null),
                }
                memory.push("shared", shared);
                if gpu.memory_type.is_empty() {
                    memory.push("type", Json::Null);
                } else {
                    memory.push("type", gpu.memory_type.clone());
                }
                obj.push("memory", memory);

                obj.push("driver", gpu.driver.clone());
                obj.push("name", gpu.name.clone());
                match gpu.temperature {
                    Some(temperature) => obj.push("temperature", temperature),
                    None => obj.push("temperature", Json::Null),
                }
                match gpu.gpu_type {
                    GPU_TYPE_INTEGRATED => obj.push("type", "Integrated"),
                    GPU_TYPE_DISCRETE => obj.push("type", "Discrete"),
                    _ => obj.push("type", Json::Null),
                }
                obj.push("vendor", gpu.vendor.clone());
                obj.push("platformApi", gpu.platform_api.clone());
                if gpu.frequency != 0 {
                    obj.push("frequency", gpu.frequency as u64);
                } else {
                    obj.push("frequency", Json::Null);
                }
                obj.push("deviceId", gpu.device_id);
                if gpu.pcie_max == (0, 0) && gpu.pcie_curr == (0, 0) {
                    obj.push("pcieSpeed", Json::Null);
                } else {
                    let mut pcie = Json::obj();
                    let mut max = Json::obj();
                    max.push("gen", gpu.pcie_max.0 as u64);
                    max.push("lanes", gpu.pcie_max.1 as u64);
                    pcie.push("max", max);
                    let mut current = Json::obj();
                    current.push("gen", gpu.pcie_curr.0 as u64);
                    current.push("lanes", gpu.pcie_curr.1 as u64);
                    pcie.push("current", current);
                    obj.push("pcieSpeed", pcie);
                }
                arr.push_value(obj);
            }
            JsonOutcome::Result(arr)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pci_addr_to_id_matches_ffgpu() {
        // 0000:01:00.0, the reference machine's GPU address.
        assert_eq!(pci_addr_to_id(0, 1, 0, 0), 256);
        // Intel's reserved integrated graphics address.
        assert_eq!(pci_addr_to_id(0, 0, 2, 0), 16);
    }

    #[test]
    fn vendor_strings_match_ffgpugetvendorstring() {
        assert_eq!(vendor_string(0x10de), Some("NVIDIA"));
        assert_eq!(vendor_string(0x1002), Some("AMD"));
        assert_eq!(vendor_string(0x8086), Some("Intel"));
        assert_eq!(vendor_string(0x1234), Some("QEMU"));
        assert_eq!(vendor_string(0x9999), None);
    }

    #[test]
    fn modalias_and_names_resolve_like_ffgpu() {
        // The vendor is set from ffGPUGetVendorString before the lookup.
        let mut gpu = GpuResult { vendor: "NVIDIA".to_string(), ..GpuResult::default() };
        fill_vendor_and_name(&mut gpu, 0, 0x10de, 0x2508);
        assert_eq!(gpu.vendor, "NVIDIA");
        assert_eq!(gpu.name, "GeForce RTX 3050 OEM");

        // A synthesized name when pci.ids has no entry.
        let mut gpu = GpuResult { vendor: "NVIDIA".to_string(), ..GpuResult::default() };
        fill_vendor_and_name(&mut gpu, 0, 0x10de, 0xffff);
        assert_eq!(gpu.name, "NVIDIA Device FFFF (VGA compatible)");
    }
}

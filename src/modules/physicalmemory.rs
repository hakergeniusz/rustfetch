//! `physicalmemory` module: DIMM information from the udev database,
//! ported from physicalmemory_linux.c. systemd >= 248 decodes the SMBIOS
//! memory records into the world-readable /run/udev/data/+dmi:id; the
//! direct SMBIOS table fallback needs privileged access and is not ported.

use crate::{instance::Instance, modules::Row, util};

struct Device {
    installed: bool,
    size: u64,
    max_speed: u32,
    running_speed: u32,
    type_: String,
    form_factor: String,
    locator: String,
    vendor: String,
    ecc: bool,
}

impl Device {
    fn new() -> Self {
        // Doubles as parse-time state: absent PRESENT / SIZE means "present,
        // unknown size", so it starts true and is only ever cleared.
        Self {
            installed: true,
            size: 0,
            max_speed: 0,
            running_speed: 0,
            type_: String::new(),
            form_factor: String::new(),
            locator: String::new(),
            vendor: String::new(),
            ecc: false,
        }
    }
}

/// Splits "E:MEMORY_DEVICE_0_SIZE=4294967296" into (0, "SIZE", "...").
fn parse_header(line: &str) -> Option<(u32, &str, &str)> {
    let rest = line.strip_prefix("E:MEMORY_DEVICE_")?;
    let eq = rest.find('=')?;
    let (head, value) = (&rest[..eq], &rest[eq + 1..]);
    let (index, field) = head.split_once('_')?;
    let index: u32 = index.parse().ok()?;
    (index < 256).then_some((index, field, value))
}

/// The placeholder strings ffIsSmbiosValueSet rejects, case-insensitively.
fn is_placeholder(value: &str) -> bool {
    let lower = value.to_lowercase();
    lower.is_empty()
        || lower.starts_with("to be filled")
        || lower.starts_with("to be set")
        || lower.starts_with("oem")
        || lower.starts_with("o.e.m.")
        || lower.starts_with("system product")
        || lower.starts_with("unknown product")
        || matches!(
            lower.as_str(),
            "none"
                | "system name"
                | "system version"
                | "system sku#"
                | "default string"
                | "undefined"
                | "not specified"
                | "not applicable"
                | "not defined"
                | "not available"
                | "invalid"
                | "type1productconfigid"
                | "tbd by oem"
                | "no enclosure"
                | "chassis version"
                | "all series"
                | "n/a"
                | "unknown"
                | "standard"
        )
        // Some SMBIOS implementations use "0x0000" to indicate unset.
        || (lower.starts_with("0x0") && lower[2..].chars().all(|c| c == '0'))
}

/// ffUdevSetString: decode \xHH escapes, drop the "<OUT OF SPEC>" prefix,
/// then the SMBIOS placeholder cleanup.
fn udev_set_string(value: &str) -> String {
    let mut text = decode_hex_escapes(value);
    if text.starts_with("<OUT OF SPEC>") {
        text.clear();
    }
    let trimmed = text.trim_end();
    if is_placeholder(trimmed) {
        String::new()
    } else {
        trimmed.to_string()
    }
}

/// ffStrbufDecodeHexEscapeSequences: udev escapes disallowed bytes as \xHH.
fn decode_hex_escapes(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut read = 0;
    while read < bytes.len() {
        if read + 3 < bytes.len() && bytes[read] == b'\\' && bytes[read + 1] == b'x' {
            let hi = (bytes[read + 2] as char).to_digit(16);
            let lo = (bytes[read + 3] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push(((hi << 4) | lo) as u8);
                read += 4;
                continue;
            }
        }
        out.push(bytes[read]);
        read += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `locator` is the "bank/device" join; the properties arrive in no
/// guaranteed order, so the bank prepends and the device appends.
fn append_locator(locator: &mut String, value: &str, bank: bool) {
    let part = udev_set_string(value);
    if part.is_empty() {
        return;
    }
    if !locator.is_empty() {
        if bank {
            locator.insert(0, '/');
        } else {
            locator.push('/');
        }
    }
    if bank {
        locator.insert_str(0, &part);
    } else {
        locator.push_str(&part);
    }
}

/// getVendorString: the JEDEC vendor ids fastfetch maps.
fn vendor_name(vendor_id: u32) -> Option<&'static str> {
    Some(match vendor_id {
        0x017A => "Apacer",
        0x0198 => "Kingston",
        0x029E => "Corsair",
        0x04CB => "A-DATA",
        0x04CD => "G-Skill",
        0x059B | 0x859B => "Crucial",
        0x00CE | 0x80CE | 0xCE00 => "Samsung",
        0x014F => "Transcend",
        0x2C00 | 0x802C => "Micron",
        0xAD00 | 0x80AD => "SK Hynix",
        0x5105 | 0x8551 => "Qimonda",
        0x02FE => "Elpida",
        0x0467 => "Ramaxel",
        _ => return None,
    })
}

/// FFPhysicalMemoryUpdateVendorString: "Micron"-style ids decode to names.
fn update_vendor_string(vendor: &mut String) {
    if vendor.is_empty() {
        return;
    }
    if vendor.eq_ignore_ascii_case("Unknown") {
        vendor.clear();
        return;
    }
    let id = if vendor.starts_with("0x") {
        if vendor.len() < 6 {
            return;
        }
        vendor[2..6].to_string()
    } else {
        if vendor.len() < 4 {
            return;
        }
        vendor[..4].to_string()
    };
    let Ok(vendor_id) = u32::from_str_radix(&id, 16) else {
        return;
    };
    if let Some(name) = vendor_name(vendor_id) {
        *vendor = name.to_string();
    }
}

/// The udev database is authoritative: as soon as it contains any
/// MEMORY_DEVICE_* property, the set it describes is complete.
fn detect() -> Option<Vec<Device>> {
    let content = util::read_file("/run/udev/data/+dmi:id")?;
    let mut devices: Vec<Option<Device>> = Vec::new();
    let mut total_widths = [0u16; 256];
    let mut data_widths = [0u16; 256];
    let mut seen = [false; 256];
    let mut found = false;

    for line in content.lines() {
        let Some((index, field, value)) = parse_header(line) else {
            continue;
        };
        while devices.len() <= index as usize {
            devices.push(None);
        }
        found = true;
        seen[index as usize] = true;
        let device = devices[index as usize].get_or_insert_with(Device::new);

        match field {
            "SIZE" => {
                device.size = value.parse().unwrap_or(0);
                if device.size == 0 {
                    device.installed = false;
                }
            },
            "PRESENT" => {
                if value.trim().parse::<u64>().unwrap_or(0) == 0 {
                    device.installed = false;
                }
            },
            "TOTAL_WIDTH" => total_widths[index as usize] = value.trim().parse().unwrap_or(0),
            "DATA_WIDTH" => data_widths[index as usize] = value.trim().parse().unwrap_or(0),
            "SPEED_MTS" => device.max_speed = value.trim().parse().unwrap_or(0),
            "CONFIGURED_SPEED_MTS" => device.running_speed = value.trim().parse().unwrap_or(0),
            "FORM_FACTOR" => device.form_factor = udev_set_string(value),
            "BANK_LOCATOR" => append_locator(&mut device.locator, value, true),
            "LOCATOR" => append_locator(&mut device.locator, value, false),
            "TYPE" => device.type_ = udev_set_string(value),
            "MANUFACTURER" => device.vendor = udev_set_string(value),
            _ => {},
        }
    }

    if !found {
        return None;
    }

    // Finalize and drop sparse / empty slots.
    let mut result = Vec::new();
    for (index, device) in devices.into_iter().enumerate() {
        let Some(device) = device else {
            continue;
        };
        if !seen[index] || !device.installed {
            continue;
        }
        let mut device = device;
        device.ecc = total_widths[index] > 0
            && data_widths[index] > 0
            && total_widths[index] > data_widths[index];
        if device.type_.is_empty() {
            device.type_ = "Unknown".to_string();
        }
        update_vendor_string(&mut device.vendor);
        result.push(device);
    }
    Some(result)
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(devices) = detect() else {
        // "Failed to get SMBIOS data" is an error row, hidden by default.
        return Vec::new();
    };
    if devices.is_empty() {
        // "No physical memory devices detected" is an error row, hidden by default.
        return Vec::new();
    }

    devices
        .iter()
        .enumerate()
        .map(|(index, device)| {
            let index = if devices.len() == 1 { 0 } else { index + 1 };
            let key = if index == 0 {
                "Physical Memory".to_string()
            } else {
                format!("Physical Memory {index}")
            };
            let mut value = String::new();
            if device.installed {
                value.push_str(&util::fmt_bytes(device.size));
                value.push_str(" - ");
                value.push_str(&device.type_);
                if device.max_speed > 0 {
                    value.push_str(&format!("-{}", device.max_speed));
                }
                if device.running_speed > 0 && device.running_speed != device.max_speed {
                    value.push_str(&format!(" @ {} MT/s", device.running_speed));
                }
                if !device.vendor.is_empty() {
                    value.push_str(&format!(" ({})", device.vendor));
                }
                if device.ecc {
                    value.push_str(" - ECC");
                }
            } else {
                value.push_str("Empty");
                if !device.form_factor.is_empty() {
                    value.push_str(&format!(" - {}", device.form_factor));
                }
                if !device.locator.is_empty() {
                    value.push_str(&format!(" ({})", device.locator));
                }
            }
            Row::new(&key, value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_udev_headers() {
        assert_eq!(
            parse_header("E:MEMORY_DEVICE_0_SIZE=4294967296"),
            Some((0, "SIZE", "4294967296"))
        );
        assert_eq!(
            parse_header("E:MEMORY_DEVICE_12_SPEED_MTS=3200"),
            Some((12, "SPEED_MTS", "3200"))
        );
        assert_eq!(
            parse_header("E:MEMORY_DEVICE_0_SPEED_MTS=3200"),
            Some((0, "SPEED_MTS", "3200"))
        );
        assert_eq!(parse_header("E:OTHER=1"), None);
        assert_eq!(parse_header("E:MEMORY_DEVICE_300_SIZE=1"), None);
    }

    #[test]
    fn filters_placeholders() {
        assert!(is_placeholder("To Be Filled By O.E.M."));
        assert!(is_placeholder("Unknown"));
        assert!(is_placeholder("0x0000"));
        assert!(is_placeholder("N/A"));
        assert!(!is_placeholder("Micron Technology"));
        assert!(!is_placeholder("0x10"));
    }

    #[test]
    fn decodes_udev_escapes() {
        assert_eq!(decode_hex_escapes("Micron\\x20Technology"), "Micron Technology");
        assert_eq!(decode_hex_escapes("plain"), "plain");
    }

    #[test]
    fn maps_vendors_and_joins_locators() {
        assert_eq!(vendor_name(0x2C00), Some("Micron"));
        assert_eq!(vendor_name(0x1234), None);
        let mut vendor = "0x2C00".to_string();
        update_vendor_string(&mut vendor);
        assert_eq!(vendor, "Micron");
        let mut locator = String::new();
        append_locator(&mut locator, "BANK 2", true);
        append_locator(&mut locator, "DIMM2", false);
        assert_eq!(locator, "BANK 2/DIMM2");
    }
}

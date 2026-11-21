//! `physicalmemory` module: DIMM records from the raw SMBIOS table, ported
//! from physicalmemory_linux.c. Type-17 structures are walked from the
//! first occurrence to the end-of-table marker; the record offsets below
//! include the 4-byte header, matching the upstream packed struct.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    smbios::SmbiosTable,
    util,
};

const FF_SMBIOS_TYPE_MEMORY_DEVICE: u8 = 17;

/// offsetof(FFSmbiosMemoryDevice, Speed) and ..., ConfiguredMemorySpeed),
/// the version gates for the 2.3+ and 2.7+ fields.
const OFF_SPEED: usize = 0x15;
const OFF_CONFIGURED_SPEED: usize = 0x20;

struct Device {
    installed: bool,
    size: u64,
    max_speed: u32,
    running_speed: u32,
    type_: String,
    form_factor: String,
    locator: String,
    vendor: String,
    serial: String,
    part_number: String,
    ecc: bool,
}

/// The string fields SMBIOS leaves at placeholder values print empty.
fn clean_up_smbios_value(table: &SmbiosTable, start: usize, index: u8) -> String {
    let Some(value) = table.locate_string(start, index) else {
        return String::new();
    };
    let value = value.trim_end();
    if super::host::is_smbios_value_set(value) {
        value.to_string()
    } else {
        String::new()
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
    if vendor == "Unknown" {
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

const FORM_FACTOR_NAMES: [&str; 20] = [
    "", // 0x00 placeholder
    "Other",
    "Unknown",
    "SIMM",
    "SIP",
    "Chip",
    "DIP",
    "ZIP",
    "Proprietary Card",
    "DIMM",
    "TSOP",
    "Row of chips",
    "RIMM",
    "SODIMM",
    "SRIMM",
    "FBDIMM",
    "Die",
    "CAMM",
    "CUDIMM",
    "CSODIMM",
];

const MEMORY_TYPE_NAMES: [&str; 38] = [
    "", // 0x00 placeholder
    "Other",
    "Unknown",
    "DRAM",
    "EDRAM",
    "VRAM",
    "SRAM",
    "RAM",
    "ROM",
    "FLASH",
    "EEPROM",
    "FEPROM",
    "EPROM",
    "CDRAM",
    "3DRAM",
    "SDRAM",
    "SGRAM",
    "RDRAM",
    "DDR",
    "DDR2",
    "DDR2 FB-DIMM",
    "Reserved",
    "Reserved",
    "Reserved",
    "DDR3",
    "FBD2",
    "DDR4",
    "LPDDR",
    "LPDDR2",
    "LPDDR3",
    "LPDDR4",
    "Logical non-volatile device",
    "HBM",
    "HBM2",
    "DDR5",
    "LPDDR5",
    "HBM3",
    "MRDIMM",
];

/// parsePhysicalDisk-equivalent for one type-17 structure.
fn parse_device(table: &SmbiosTable, offset: usize) -> Option<Device> {
    let size_raw = table.u16_at(offset, 0x0C);
    let installed = size_raw != 0;

    // showEmptySlots defaults to false.
    if !installed {
        return None;
    }

    let total_width = table.u16_at(offset, 0x08);
    let data_width = table.u16_at(offset, 0x0A);
    let ecc = total_width != 0xFFFF && data_width != 0xFFFF && total_width > data_width;

    let mut size = 0u64;
    if size_raw != 0xFFFF {
        if size_raw == 0x7FFF {
            size = (table.u32_at(offset, 0x1C) as u64 & !(1u64 << 31)) * 1024 * 1024;
        } else if size_raw & (1 << 15) != 0 {
            size = u64::from(size_raw & !(1 << 15)) * 1024;
        } else {
            size = u64::from(size_raw) * 1024 * 1024;
        }
    }

    let strings = offset + table.header_length(offset);
    let bank = table.locate_string(strings, table.u8_at(offset, 0x11));
    let device = table.locate_string(strings, table.u8_at(offset, 0x10));
    let locator = match (bank, device) {
        (Some(bank), Some(device)) => format!("{bank}/{device}"),
        (Some(bank), None) => bank.to_string(),
        (None, Some(device)) => device.to_string(),
        (None, None) => String::new(),
    };

    let form_factor_raw = table.u8_at(offset, 0x0E);
    let form_factor = if form_factor_raw > 0 && (form_factor_raw as usize) < FORM_FACTOR_NAMES.len()
    {
        FORM_FACTOR_NAMES[form_factor_raw as usize].to_string()
    } else {
        format!("Unknown ({form_factor_raw})")
    };

    let memory_type_raw = table.u8_at(offset, 0x12);
    let type_ = if memory_type_raw > 0 && (memory_type_raw as usize) < MEMORY_TYPE_NAMES.len() {
        MEMORY_TYPE_NAMES[memory_type_raw as usize].to_string()
    } else {
        format!("Unknown ({memory_type_raw})")
    };

    let mut device = Device {
        installed,
        size,
        max_speed: 0,
        running_speed: 0,
        type_,
        form_factor,
        locator,
        vendor: String::new(),
        serial: String::new(),
        part_number: String::new(),
        ecc,
    };

    if table.header_length(offset) > OFF_SPEED {
        let speed = table.u16_at(offset, 0x15);
        if speed != 0 {
            device.max_speed = if speed == 0xFFFF {
                table.u32_at(offset, 0x54)
            } else {
                u32::from(speed)
            };
        }

        device.vendor = clean_up_smbios_value(table, strings, table.u8_at(offset, 0x17));
        update_vendor_string(&mut device.vendor);
        device.serial = clean_up_smbios_value(table, strings, table.u8_at(offset, 0x18));
        device.part_number = clean_up_smbios_value(table, strings, table.u8_at(offset, 0x1A));
    }

    if table.header_length(offset) > OFF_CONFIGURED_SPEED {
        let configured = table.u16_at(offset, 0x20);
        if configured != 0 {
            device.running_speed = if configured == 0xFFFF {
                table.u32_at(offset, 0x58)
            } else {
                u32::from(configured)
            };
        }
    }

    Some(device)
}

/// ffDetectPhysicalMemory.
fn detect() -> Result<Vec<Device>, &'static str> {
    let table = SmbiosTable::load().ok_or("Failed to get SMBIOS data")?;
    if table.first_offset(FF_SMBIOS_TYPE_MEMORY_DEVICE).is_none() {
        return Err("Memory device is not found in SMBIOS data");
    }
    Ok(table
        .walk(FF_SMBIOS_TYPE_MEMORY_DEVICE)
        .iter()
        .filter_map(|&offset| parse_device(&table, offset))
        .collect())
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(devices) = detect() else {
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
                format!("{} {index}", instance.display_name("physicalmemory"))
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

/// ffGeneratePhysicalMemoryJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(devices) => {
            let mut arr = Json::Array(Vec::new());
            for device in &devices {
                let mut obj = Json::obj();
                obj.push("size", device.size);
                obj.push("installed", device.installed);
                obj.push("maxSpeed", device.max_speed as u64);
                obj.push("runningSpeed", device.running_speed as u64);
                obj.push("type", device.type_.clone());
                obj.push("locator", device.locator.clone());
                obj.push("formFactor", device.form_factor.clone());
                obj.push("vendor", device.vendor.clone());
                obj.push("serial", device.serial.clone());
                obj.push("partNumber", device.part_number.clone());
                obj.push("ecc", device.ecc);
                arr.push_value(obj);
            }
            JsonOutcome::Result(arr)
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn maps_vendors_from_ids() {
        let mut vendor = "0x2C00".to_string();
        super::update_vendor_string(&mut vendor);
        assert_eq!(vendor, "Micron");
        let mut vendor = "Unknown".to_string();
        super::update_vendor_string(&mut vendor);
        assert_eq!(vendor, "");
        let mut vendor = "Micron Technology".to_string();
        super::update_vendor_string(&mut vendor);
        assert_eq!(vendor, "Micron Technology"); // not a bare id
        let mut vendor = "0x2C".to_string();
        super::update_vendor_string(&mut vendor);
        assert_eq!(vendor, "0x2C"); // too short
    }

    #[test]
    fn memory_type_and_form_factor_tables_cover_enums() {
        assert_eq!(super::MEMORY_TYPE_NAMES[0x1A], "DDR4");
        assert_eq!(super::MEMORY_TYPE_NAMES[0x22], "DDR5");
        assert_eq!(super::FORM_FACTOR_NAMES[0x09], "DIMM");
        assert_eq!(super::FORM_FACTOR_NAMES[0x0D], "SODIMM");
    }
}

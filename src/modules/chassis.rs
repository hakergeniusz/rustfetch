//! `chassis` module: the SMBIOS chassis type, ported from chassis_linux.c.
//! The DMI files under /sys/devices/virtual/dmi/id (falling back to
//! /sys/class/dmi/id) name the type via the SMBIOS 7.4.1 table, with the
//! Asahi devicetree files as the non-SMBIOS fallback. The version is
//! appended when set; unset values print nothing (the OEM didn't set it).

use crate::{instance::Instance, modules::Row, util};

/// SMBIOS 7.4.1 System Enclosure or Chassis Types.
fn chassis_type_to_string(type_: u32) -> Option<&'static str> {
    Some(match type_ & 0b0111_1111 {
        0x01 => "Other",
        0x02 => "Unknown",
        0x03 => "Desktop",
        0x04 => "Low Profile Desktop",
        0x05 => "Pizza Box",
        0x06 => "Mini Tower",
        0x07 => "Tower",
        0x08 => "Portable",
        0x09 => "Laptop",
        0x0A => "Notebook",
        0x0B => "Hand Held",
        0x0C => "Docking Station",
        0x0D => "All in One",
        0x0E => "Sub Notebook",
        0x0F => "Space-saving",
        0x10 => "Lunch Box",
        0x11 => "Main Server Chassis",
        0x12 => "Expansion Chassis",
        0x13 => "SubChassis",
        0x14 => "Bus Expansion Chassis",
        0x15 => "Peripheral Chassis",
        0x16 => "RAID Chassis",
        0x17 => "Rack Mount Chassis",
        0x18 => "Sealed-case PC",
        0x19 => "Multi-system chassis",
        0x1A => "Compact PCI",
        0x1B => "Advanced TCA",
        0x1C => "Blade",
        0x1D => "Blade Enclosure",
        0x1E => "Tablet",
        0x1F => "Convertible",
        0x20 => "Detachable",
        0x21 => "IoT Gateway",
        0x22 => "Embedded PC",
        0x23 => "Mini PC",
        0x24 => "Stick PC",
        _ => return None,
    })
}

/// ffGetSmbiosValue: the first readable file whose content is an actual
/// value (not "None" or empty).
fn smbios_value(path: &str) -> Option<String> {
    util::read_trimmed(path).filter(|value| is_set(value))
}

fn is_set(value: &str) -> bool {
    !value.is_empty() && !value.eq_ignore_ascii_case("none")
}

fn detect() -> Option<(String, String)> {
    // The /sys/class/dmi/id files need root; /sys/devices/virtual/dmi/id are
    // world-readable aliases with serials filtered.
    let mut type_ = smbios_value("/sys/devices/virtual/dmi/id/chassis_type")
        .or_else(|| smbios_value("/sys/class/dmi/id/chassis_type"))
        .unwrap_or_default();
    if let Some(name) = type_.parse::<u32>().ok().and_then(chassis_type_to_string) {
        type_ = name.to_string();
    }

    if type_.is_empty() {
        // Available on Asahi Linux: a big-endian u32 under the smbios node.
        let mut vendor = String::new();
        if let Ok(bytes) =
            std::fs::read("/sys/firmware/devicetree/base/smbios/smbios/chassis/chassis-type")
            && bytes.len() == 4
        {
            let chassis_type = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if let Some(name) = chassis_type_to_string(chassis_type) {
                type_ = name.to_string();
            }
            if let Some(manufacturer) = util::read_trimmed(
                "/sys/firmware/devicetree/base/smbios/smbios/chassis/manufacturer",
            ) {
                vendor = manufacturer.trim_end_matches('\0').trim_end().to_string();
            }
        } else if let Some(devtype) =
            util::read_trimmed("/sys/firmware/devicetree/base/chassis-type")
        {
            let devtype = devtype.trim_end_matches('\0');
            let mut chars = devtype.chars();
            if let Some(first) = chars.next() {
                type_ = first.to_uppercase().collect::<String>() + chars.as_str();
            }
        }
        return Some((type_, vendor));
    }

    let version = smbios_value("/sys/devices/virtual/dmi/id/chassis_version").unwrap_or_default();
    Some((type_, version))
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some((type_, version)) = detect() else {
        return Vec::new();
    };
    if type_.is_empty() {
        // "chassis_type is not set by O.E.M." is an error row, hidden by
        // default.
        return Vec::new();
    }

    let value = if version.is_empty() {
        type_
    } else {
        format!("{type_} ({version})")
    };
    vec![Row::new("Chassis", value)]
}

#[cfg(test)]
mod tests {
    #[test]
    fn maps_smbios_types() {
        assert_eq!(super::chassis_type_to_string(3), Some("Desktop"));
        assert_eq!(super::chassis_type_to_string(9), Some("Laptop"));
        assert_eq!(super::chassis_type_to_string(10), Some("Notebook"));
        assert_eq!(super::chassis_type_to_string(35), Some("Mini PC"));
        assert_eq!(super::chassis_type_to_string(37), None);
        // The high bit (chassis state byte) is masked off.
        assert_eq!(super::chassis_type_to_string(0x80 | 3), Some("Desktop"));
    }
}

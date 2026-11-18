//! `bootmgr` module: the current UEFI boot entry, ported from
//! detection/bootmgr/bootmgr_linux.c. Reads BootCurrent, Boot#### and
//! SecureBoot from efivars; the entry's description and the first file
//! path device node form the value. Requires mounted, readable efivars
//! (root-owned efivars files make the error row, hidden by default).

use crate::{instance::Instance, modules::Row};

const EFIVARS_PREFIX: &str = "/sys/firmware/efi/efivars/";
const EFI_GLOBAL_GUID: &str = "8be4df61-93ca-11d2-aa0d-00e098032b8c";
const LOAD_OPTION_SIZE: usize = 8; // attributes u32 + file path list length u16

/// evBits from bootmgr.c: extracts `mask` at `shift` as u8.
fn ev_bits(val: u16, mask: u8, shift: u8) -> u8 {
    ((val & ((mask as u16) << shift)) >> shift) as u8
}

/// ffEfiUcs2ToUtf8: UCS-2 (no surrogate pairs) to UTF-8, NUL terminated.
fn ucs2_to_utf8(chars: &[u8]) -> String {
    let mut result = String::new();
    for unit in chars.as_chunks::<2>().0 {
        let value = u16::from_le_bytes(*unit);
        if value == 0 {
            break;
        }
        if value <= 0x7F {
            result.push(value as u8 as char);
        } else if value <= 0x7FF {
            result.push((0xC0u8 | ev_bits(value, 0x1F, 6)) as char);
            result.push((0x80u8 | ev_bits(value, 0x3F, 0)) as char);
        } else {
            result.push((0xE0u8 | ev_bits(value, 0xF, 12)) as char);
            result.push((0x80u8 | ev_bits(value, 0x3F, 6)) as char);
            result.push((0x80u8 | ev_bits(value, 0x3F, 0)) as char);
        }
    }
    result
}

/// ffEfiFillLoadOption: description and first file path media device node.
/// The slice starts at the option's Attributes field; Description lives at
/// offset 6 (attributes u32 + file path list length u16).
fn fill_load_option(option: &[u8]) -> (String, String) {
    let description = &option[6.min(option.len())..];
    let desc_len = description
        .as_chunks::<2>()
        .0
        .iter()
        .position(|unit| u16::from_le_bytes(*unit) == 0)
        .unwrap_or(description.len() / 2);

    let mut name = String::new();
    if desc_len > 0 {
        name = ucs2_to_utf8(&description[..desc_len * 2]);
    }

    // Walk the device path list after the NUL terminator of the description.
    let mut firmware = String::new();
    let mut cursor = (desc_len + 1) * 2;
    while cursor + 4 <= description.len() {
        let node_type = description[cursor];
        let sub_type = description[cursor + 1];
        let length =
            u16::from_le_bytes([description[cursor + 2], description[cursor + 3]]) as usize;
        if length == 0 || cursor + length > description.len() {
            break;
        }
        if node_type == 4 && sub_type == 4 {
            // File Path Media Device Path.
            firmware = ucs2_to_utf8(&description[cursor + 4..cursor + length]);
            break;
        }
        if node_type == 0x7F {
            // End of Hardware Device Path.
            break;
        }
        cursor += length;
    }
    (name, firmware)
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(current) = std::fs::read(format!("{EFIVARS_PREFIX}BootCurrent-{EFI_GLOBAL_GUID}"))
    else {
        // "Failed to read efivar: BootCurrent" is an error row, hidden by
        // default.
        return Vec::new();
    };
    if current.len() != 6 {
        return Vec::new();
    }
    let order = u16::from_le_bytes([current[4], current[5]]);

    let Ok(boot_entry) =
        std::fs::read(format!("{EFIVARS_PREFIX}Boot{order:04X}-{EFI_GLOBAL_GUID}"))
    else {
        // "Failed to read efivar: Boot####" is an error row, hidden by
        // default.
        return Vec::new();
    };
    if boot_entry.len() < 5 + LOAD_OPTION_SIZE || boot_entry.len() >= 2048 {
        return Vec::new();
    }

    let (name, firmware) = fill_load_option(&boot_entry[4..]);
    let firmware_name = firmware.rsplit('\\').next().unwrap_or_default();

    let mut value = name;
    if !firmware_name.is_empty() {
        value.push_str(" - ");
        value.push_str(firmware_name);
    }
    vec![Row::new("Boot Manager", value)]
}

#[cfg(test)]
mod tests {
    #[test]
    fn converts_ucs2() {
        assert_eq!(super::ucs2_to_utf8(&[b's', 0, b'd', 0, b'c', 0, 0, 0]), "sdc");
        // é (0xE9) encodes as 0xC3 0xA9.
        assert_eq!(
            super::ucs2_to_utf8(&[0xE9, 0, 0, 0]),
            [0xC3u8, 0xA9u8].map(|b| b as char).iter().collect::<String>()
        );
    }

    #[test]
    fn parses_load_option() {
        // Attributes u32, list length u16, "Boot" UCS-2 + NUL, then a file
        // path node (type 4 subtype 4) with "\EFI\BOOT" in UCS-2.
        let data = vec![
            1, 0, 0, 0, // attributes
            0x14, 0, // file path list length
            b'B', 0, b'o', 0, b'o', 0, b't', 0, 0, 0, // description
            4, 4, 0x0E, 0, // node: type 4, subtype 4, length 14
            b'\\', 0, b'E', 0, b'F', 0, b'I', 0, 0, 0, // path data: "\EFI"
        ];
        let (name, firmware) = super::fill_load_option(&data);
        assert_eq!(name, "Boot");
        assert_eq!(firmware, "\\EFI");
    }
}

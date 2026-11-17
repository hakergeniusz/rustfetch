//! `gpu` module: GPUs from /sys/class/drm named via pci.ids, with subsystem
//! refinement. No processes spawned, no giant dependencies; only the needed
//! vendor blocks of pci.ids are scanned.

use std::collections::HashSet;

use crate::{instance::Instance, modules::Row, util};

const PCI_IDS_PATHS: &[&str] = &[
    "/usr/share/hwdata/pci.ids",
    "/usr/share/misc/pci.ids",
    "/var/lib/usbutils/pci.ids",
];

/// PCI vendor ids known to host GPUs, including virtual machine adapters.
/// Values match pci.ids casing (`10de`), matching what normalize_id produces.
const GPU_VENDORS: &[&str] = &[
    "1002", "8086", "10de", "1022", "15ad", "1234", "1af4", "1b36",
];

/// sysfs ids come as `0x10de` while pci.ids keys are `10de`.
fn normalize_id(raw: &str) -> String {
    let lowered = raw.to_ascii_lowercase();
    lowered.strip_prefix("0x").unwrap_or(&lowered).to_string()
}

fn is_hex_id(id: &str) -> bool {
    id.len() == 4 && id.chars().all(|c| c.is_ascii_hexdigit())
}

/// One DRM card with its PCI identity read from sysfs.
struct Card {
    vendor: String,
    device: String,
    subvendor: Option<String>,
    subdevice: Option<String>,
}

fn read_cards() -> Vec<Card> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut cards: Vec<Card> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("card") && name[4..].chars().all(|c| c.is_ascii_digit())
        })
        .map(|entry| entry.path().join("device"))
        .filter(|device| device.join("vendor").exists())
        .filter_map(|device| {
            let vendor = util::read_trimmed_in(&device, "vendor").map(|v| normalize_id(&v))?;
            if !GPU_VENDORS.contains(&vendor.as_str()) {
                return None;
            }
            let dev = util::read_trimmed_in(&device, "device").map(|v| normalize_id(&v))?;
            Some(Card {
                vendor,
                device: dev,
                subvendor: util::read_trimmed_in(&device, "subsystem_vendor").map(|v| normalize_id(&v)),
                subdevice: util::read_trimmed_in(&device, "subsystem_device").map(|v| normalize_id(&v)),
            })
        })
        .collect();
    cards.sort_by(|a, b| a.vendor.cmp(&b.vendor).then(a.device.cmp(&b.device)));
    cards
}

/// The section of pci.ids covering one vendor: the vendor line plus every
/// indented device/subsystem line under it.
fn vendor_block<'a>(content: &'a str, vendor: &str) -> Option<&'a str> {
    let start = content
        .find(&format!("\n{vendor}  "))
        .map(|pos| pos + 1)
        .or_else(|| content.starts_with(&format!("{vendor}  ")).then_some(0))?;
    let rest = &content[start..];

    // The block ends at the first unindented, non-comment line after the
    // vendor line; pci.ids scatters `#` comments inside vendor sections.
    let mut end = rest.find('\n').map(|pos| pos + 1).unwrap_or(rest.len());
    for line in rest[end..].lines() {
        if !line.starts_with('\t') && !line.starts_with('#') {
            break;
        }
        end += line.len() + 1;
    }
    Some(&rest[..end])
}

/// Device and subsystem names inside a vendor block, as `(&str, Option<&str>)`.
fn device_names<'a>(
    block: &'a str,
    device_id: &str,
    subvendor: Option<&str>,
    subdevice: Option<&str>,
) -> (Option<&'a str>, Option<&'a str>) {
    let mut lines = block.lines();
    let _vendor_line = lines.next();

    let mut device_name = None;
    for line in lines {
        let Some(rest) = line.strip_prefix('\t') else {
            continue;
        };
        if let Some(deeper) = rest.strip_prefix('\t') {
            // subsystem line under the device we already matched
            if device_name.is_some()
                && let Some((ids, name)) = deeper.split_once("  ")
            {
                let mut fields = ids.split(' ');
                if fields.next() == subvendor && fields.next() == subdevice {
                    return (device_name, Some(name));
                }
            }
        } else if let Some((id, name)) = rest.split_once("  ") {
            if id == device_id && is_hex_id(id) {
                device_name = Some(name);
            } else if device_name.is_some() {
                break; // walked past our device
            }
        }
    }
    (device_name, None)
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

fn describe(card: &Card, block: Option<&str>) -> String {
    if let Some(block) = block {
        let vendor = block.lines().next().and_then(|line| line.split_once("  ")).map(|(_, name)| name);
        let (device, subsystem) =
            device_names(block, &card.device, card.subvendor.as_deref(), card.subdevice.as_deref());

        if let (Some(vendor), Some(device)) = (vendor, device) {
            let display = subsystem
                .map(str::to_string)
                .or_else(|| bracketed(device))
                .unwrap_or_else(|| device.to_string());
            return format!("{} {}", short_vendor(vendor), display);
        }
    }

    format!("{} {}", card.vendor, card.device)
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let cards = read_cards();
    if cards.is_empty() {
        return Vec::new();
    }

    let content = PCI_IDS_PATHS.iter().find_map(|path| util::read_file(path));
    let mut rows = Vec::new();
    let mut seen = HashSet::new();

    for card in &cards {
        let block = content.as_deref().and_then(|content| vendor_block(content, &card.vendor));
        let name = describe(card, block);
        if seen.insert(name.clone()) {
            rows.push(Row::new("GPU", name));
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_block_slices_vendor_section() {
        let content = "# comment\n1002  AMD\n\t7300  card\n10de  NVIDIA\n\t2508  GA106 [GeForce RTX 3050 OEM]\n\t\t174b a630  sub\n2000  next\n";
        let block = vendor_block(content, "10de").expect("block found");
        assert!(block.starts_with("10de  NVIDIA"), "got: {block:?}");
        assert!(block.contains("2508"), "got: {block:?}");
        assert!(!block.contains("2000"), "got: {block:?}");
    }

    #[test]
    fn device_names_match_subsystem() {
        let content = "10de  NVIDIA\n\t2508  GA106 [GeForce RTX 3050 OEM]\n\t\t174b a630  sub\n";
        let (device, subsystem) = device_names(content, "2508", Some("174b"), Some("a630"));
        assert_eq!(device, Some("GA106 [GeForce RTX 3050 OEM]"));
        assert_eq!(subsystem, Some("sub"));
    }
}

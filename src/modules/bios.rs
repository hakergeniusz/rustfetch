//! `bios` module: firmware version and release date from the DMI tables,
//! keyed "BIOS (UEFI)" or "BIOS" like fastfetch.

use crate::{instance::Instance, modules::Row, util};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(version) = util::read_trimmed("/sys/class/dmi/id/bios_version") else {
        return Vec::new();
    };

    let key = if std::path::Path::new("/sys/firmware/efi").exists() {
        "BIOS (UEFI)"
    } else {
        "BIOS"
    };

    let mut value = version;
    let release = util::read_trimmed("/sys/class/dmi/id/bios_release")
        .or_else(|| util::read_trimmed("/sys/class/dmi/id/bios_date"));
    if let Some(release) = release {
        value.push_str(&format!(" ({release})"));
    }

    vec![Row::new(key, value)]
}

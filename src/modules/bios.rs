//! `bios` module: firmware information from the DMI tables, ported from
//! bios_linux.c (U-Boot and devicetree fallbacks included). The JSON adds
//! the release, vendor and the UEFI/BIOS type.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::{host, Row},
    util,
};

pub struct BiosResult {
    pub date: String,
    pub release: String,
    pub vendor: String,
    pub version: String,
    pub firmware_type: String,
}

/// ffDetectBios.
pub fn detect() -> BiosResult {
    let mut result = BiosResult {
        date: String::new(),
        release: String::new(),
        vendor: String::new(),
        version: String::new(),
        firmware_type: String::new(),
    };

    if !host::get_smbios_value(
        "/sys/devices/virtual/dmi/id/bios_date",
        "/sys/class/dmi/id/bios_date",
    )
    .is_empty()
    {
        result.date = host::get_smbios_value("/sys/devices/virtual/dmi/id/bios_date", "/sys/class/dmi/id/bios_date");
        result.release = host::get_smbios_value("/sys/devices/virtual/dmi/id/bios_release", "/sys/class/dmi/id/bios_release");
        result.vendor = host::get_smbios_value("/sys/devices/virtual/dmi/id/bios_vendor", "/sys/class/dmi/id/bios_vendor");
        result.version = host::get_smbios_value("/sys/devices/virtual/dmi/id/bios_version", "/sys/class/dmi/id/bios_version");
    } else if let Some(version) = util::read_file("/proc/device-tree/chosen/u-boot,version") {
        result.version = version.trim_end_matches('\0').to_string();
        result.vendor = "U-Boot".to_string();
    }

    result.firmware_type = if std::path::Path::new("/sys/firmware/efi/").is_dir()
        || std::path::Path::new("/sys/firmware/acpi/tables/UEFI").is_file()
    {
        "UEFI".to_string()
    } else {
        "BIOS".to_string()
    };
    result
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let bios = detect();
    if bios.version.is_empty() {
        return Vec::new();
    }

    let key = if bios.firmware_type == "UEFI" {
        "BIOS (UEFI)"
    } else {
        "BIOS"
    };

    let mut value = bios.version;
    let release = if bios.release.is_empty() {
        bios.date.clone()
    } else {
        bios.release.clone()
    };
    if !release.is_empty() {
        value.push_str(&format!(" ({release})"));
    }

    vec![Row::new(key, value)]
}

/// ffGenerateBiosJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let bios = detect();
    let mut obj = Json::obj();
    obj.push("date", bios.date);
    obj.push("release", bios.release);
    obj.push("vendor", bios.vendor);
    obj.push("version", bios.version);
    obj.push("type", bios.firmware_type);
    JsonOutcome::Result(obj)
}

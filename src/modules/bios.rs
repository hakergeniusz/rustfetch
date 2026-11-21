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

pub fn run(instance: &Instance) -> Vec<Row> {
    let bios = detect();
    if bios.version.is_empty() {
        return Vec::new();
    }

    // bios.c maps an ignCase "BIOS" type to "Legacy" (empty would be
    // "Unknown"; the detection here always yields UEFI or BIOS) and always
    // interpolates: "%s (%s)". The mapping happens in the key branch,
    // before FF_PRINT_FORMAT_CHECKED, so the custom-format `type` argument
    // reads the same mapped value.
    let bios_type = if bios.firmware_type.eq_ignore_ascii_case("BIOS") {
        "Legacy"
    } else {
        &bios.firmware_type
    };
    let key = format!("{} ({bios_type})", instance.display_name("bios"));

    let Some(format) = crate::modules::entry_format(instance.entry) else {
        let mut value = bios.version;
        let release = if bios.release.is_empty() {
            bios.date.clone()
        } else {
            bios.release.clone()
        };
        if !release.is_empty() {
            value.push_str(&format!(" ({release})"));
        }

        return vec![Row::new(&key, value)];
    };

    // Upstream FF_PRINT_FORMAT_CHECKED passes the five bios strbufs as
    // FF_ARG_TYPE_STRBUF values: date, release, vendor, version and the
    // mapped type above.
    let args = [
        crate::format_args::FormatArg { name: "date", value: crate::format_args::ArgValue::Str(&bios.date) },
        crate::format_args::FormatArg { name: "release", value: crate::format_args::ArgValue::Str(&bios.release) },
        crate::format_args::FormatArg { name: "vendor", value: crate::format_args::ArgValue::Str(&bios.vendor) },
        crate::format_args::FormatArg { name: "version", value: crate::format_args::ArgValue::Str(&bios.version) },
        crate::format_args::FormatArg { name: "type", value: crate::format_args::ArgValue::Str(bios_type) },
    ];
    let rendered = crate::format_args::parse_format_string(format, &args, args.len() as u32, &crate::format_args::EngineEnv::new(instance));
    vec![Row::new(&key, rendered)]
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

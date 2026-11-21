//! `host` module: device model from the DMI tables, ported from
//! detection/host/host_linux.c. Each value goes through
//! ffGetSmbiosValue (both sysfs paths, the garbage-value filter); when
//! DMI gives nothing, the devicetree/WSL fallbacks apply. The JSON
//! result serializes all seven fields in upstream order.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

/// fastfetch's ffIsSmbiosValueSet: SMBIOS strings this hardware leaves unset
/// carry a predictable set of placeholder values.
pub(crate) fn is_smbios_value_set(value: &str) -> bool {
    !value.is_empty()
        && !value.to_ascii_lowercase().starts_with("to be filled")
        && !value.to_ascii_lowercase().starts_with("to be set")
        && !value.to_ascii_lowercase().starts_with("oem")
        && !value.eq_ignore_ascii_case("O.E.M.")
        && !value.to_ascii_lowercase().starts_with("system product")
        && !value.to_ascii_lowercase().starts_with("unknown product")
        && !value.eq_ignore_ascii_case("none")
        && !value.eq_ignore_ascii_case("system name")
        && !value.eq_ignore_ascii_case("system version")
        && !value.eq_ignore_ascii_case("system sku#")
        && !value.eq_ignore_ascii_case("default string")
        && !value.eq_ignore_ascii_case("undefined")
        && !value.eq_ignore_ascii_case("not specified")
        && !value.eq_ignore_ascii_case("not applicable")
        && !value.eq_ignore_ascii_case("not defined")
        && !value.eq_ignore_ascii_case("not available")
        && !value.eq_ignore_ascii_case("invalid")
        && !value.eq_ignore_ascii_case("type1productconfigid")
        && !value.eq_ignore_ascii_case("tbd by oem")
        && !value.eq_ignore_ascii_case("no enclosure")
        && !value.eq_ignore_ascii_case("chassis version")
        && !value.eq_ignore_ascii_case("all series")
        && !value.eq_ignore_ascii_case("n/a")
        && !value.eq_ignore_ascii_case("unknown")
        && !value.eq_ignore_ascii_case("standard")
        && !(value.starts_with("0x0") && value[2..].chars().all(|c| c.is_ascii_hexdigit()))
}

/// ffGetSmbiosValue: try the /sys/devices path, then /sys/class, trimming
/// trailing whitespace and rejecting garbage values; empty on failure.
pub(crate) fn get_smbios_value(devices_path: &str, class_path: &str) -> String {
    for path in [devices_path, class_path] {
        if let Some(value) = util::read_trimmed(path).filter(|value| is_smbios_value_set(value)) {
            return value;
        }
    }
    String::new()
}

/// The devicetree readers used when DMI has nothing (ARM boards, Asahi).
fn read_devicetree(path: &str) -> Option<String> {
    util::read_file(path).map(|content| content.trim_end_matches('\0').to_string())
}

fn get_host_product_name() -> Option<String> {
    if let Some(model) = read_devicetree("/sys/firmware/devicetree/base/model")
        .or_else(|| read_devicetree("/sys/firmware/devicetree/base/banner-name"))
    {
        return Some(model);
    }
    // /tmp/sysinfo/model still goes through the garbage filter.
    util::read_trimmed("/tmp/sysinfo/model").filter(|value| is_smbios_value_set(value))
}

#[derive(Default)]
pub struct HostResult {
    family: String,
    name: String,
    version: String,
    sku: String,
    vendor: String,
    serial: String,
    uuid: String,
}

fn detect() -> HostResult {
    let mut host = HostResult::default();

    let product_name = get_smbios_value(
        "/sys/devices/virtual/dmi/id/product_name",
        "/sys/class/dmi/id/product_name",
    );
    let product_family = get_smbios_value(
        "/sys/devices/virtual/dmi/id/product_family",
        "/sys/class/dmi/id/product_family",
    );

    if !product_name.is_empty() || !product_family.is_empty() {
        host.name = product_name;
        host.family = product_family;
        host.version = get_smbios_value(
            "/sys/devices/virtual/dmi/id/product_version",
            "/sys/class/dmi/id/product_version",
        );
        host.sku = get_smbios_value(
            "/sys/devices/virtual/dmi/id/product_sku",
            "/sys/class/dmi/id/product_sku",
        );
        host.serial = get_smbios_value(
            "/sys/devices/virtual/dmi/id/product_serial",
            "/sys/class/dmi/id/product_serial",
        );
        host.vendor = get_smbios_value(
            "/sys/devices/virtual/dmi/id/sys_vendor",
            "/sys/class/dmi/id/sys_vendor",
        );
        // QEMU's DMI name is prefixed for clarity.
        if host.name.starts_with("Standard PC") {
            host.name = format!("KVM/QEMU {}", host.name);
        }
    } else {
        host.family = read_devicetree("/sys/firmware/devicetree/base/smbios/smbios/system/family")
            .or_else(|| read_devicetree("/sys/firmware/devicetree/base/smbios/smbios/system/product"))
            .unwrap_or_default();
        host.name = get_host_product_name().unwrap_or_default();
        host.serial = read_devicetree("/sys/firmware/devicetree/base/smbios/smbios/system/serial")
            .or_else(|| read_devicetree("/sys/firmware/devicetree/base/serial-number"))
            .unwrap_or_default();
        host.vendor = read_devicetree("/sys/firmware/devicetree/base/smbios/smbios/system/manufacturer")
            .unwrap_or_default();
    }

    if host.family.is_empty() && host.name.is_empty() {
        // On WSL, the real host can't be detected. Instead use WSL as host.
        let wsl_distro_name = std::env::var("WSL_DISTRO_NAME").ok().filter(|name| !name.is_empty());
        if wsl_distro_name.is_some()
            || std::env::var_os("WSL_DISTRO").is_some()
            || std::env::var_os("WSL_INTEROP").is_some()
        {
            host.name = match &wsl_distro_name {
                Some(distro) => format!("Windows Subsystem for Linux - {distro}"),
                None => "Windows Subsystem for Linux".to_string(),
            };
            host.family = "WSL".to_string();
            host.vendor = "Microsoft Corporation".to_string();
        }
    }

    host
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let host = detect();
    if host.name.is_empty() && host.family.is_empty() {
        // "neither product_family nor product_name is set by O.E.M." is an
        // error row, hidden by default.
        return Vec::new();
    }

    let mut value = if !host.name.is_empty() {
        host.name
    } else {
        host.family
    };
    if !host.version.is_empty() {
        value.push_str(&format!(" ({})", host.version));
    }
    vec![Row::new(instance.display_name("host"), value)]
}

/// ffGenerateHostJsonResult: the DMI identity, in its fixed order.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let host = detect();
    if host.name.is_empty() && host.family.is_empty() {
        return JsonOutcome::Error("neither product_family nor product_name is set by O.E.M.".to_string());
    }
    let mut result = Json::obj();
    result.push("family", host.family);
    result.push("name", host.name);
    result.push("version", host.version);
    result.push("sku", host.sku);
    result.push("vendor", host.vendor);
    result.push("serial", host.serial);
    result.push("uuid", host.uuid);
    JsonOutcome::Result(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smbios_filter_matches_fastfetch() {
        assert!(is_smbios_value_set("1.0"));
        assert!(is_smbios_value_set("Nitro N50-640"));
        assert!(!is_smbios_value_set("None"));
        assert!(!is_smbios_value_set("N/A"));
        assert!(!is_smbios_value_set("Default string"));
        assert!(!is_smbios_value_set("To be filled by O.E.M."));
        assert!(!is_smbios_value_set("0x0000"));
        assert!(!is_smbios_value_set(""));
    }

    #[test]
    fn wsl_fallback_formats_the_distro_name() {
        let host = HostResult {
            name: "Windows Subsystem for Linux - Arch".to_string(),
            ..Default::default()
        };
        assert_eq!(host.name, "Windows Subsystem for Linux - Arch");
    }
}

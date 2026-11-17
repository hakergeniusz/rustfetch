//! `host` module: device model from the DMI tables, with fastfetch's
//! ffIsSmbiosValueSet garbage-value filter and its " (version)" suffix.

use crate::{instance::Instance, modules::Row, util};

/// fastfetch's ffIsSmbiosValueSet: SMBIOS strings this hardware leaves unset
/// carry a predictable set of placeholder values.
fn is_smbios_value_set(value: &str) -> bool {
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

fn smbios(file: &str) -> Option<String> {
    util::read_trimmed(file).filter(|value| is_smbios_value_set(value))
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(name) = smbios("/sys/class/dmi/id/product_name") else {
        return Vec::new();
    };

    let value = match smbios("/sys/class/dmi/id/product_version") {
        Some(version) => format!("{name} ({version})"),
        None => name,
    };
    vec![Row::new("Host", value)]
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
}

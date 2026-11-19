//! `bluetooth radio` module: the local adapters, ported from
//! bluetoothradio_linux.c. Every /sys/class/bluetooth/hciN entry (connected
//! devices excluded by the ':' in their names) answers a
//! Properties.GetAll for org.bluez.Adapter1 on the system bus; enabled
//! radios print as "Bluetooth 5.3 (Intel)" style rows.

use std::sync::OnceLock;

use crate::{
    dbus,
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

/// ffBluetoothRadioGetVendor: the Bluetooth SIG company ids fastfetch maps.
fn vendor_name(manufacturer_id: u32) -> &'static str {
    match manufacturer_id {
        0 => "Ericsson",
        1 => "Nokia",
        2 => "Intel",
        3 => "IBM",
        4 => "Toshiba",
        5 => "3Com",
        6 => "Microsoft",
        7 => "Lucent",
        8 => "Motorola",
        9 => "Infineon",
        10 => "CSR",
        11 => "Silicon-Wave",
        12 => "Digi-Answer",
        13 => "Ti",
        14 => "Parthus",
        15 => "Broadcom",
        16 => "Mitel",
        17 => "Widcomm",
        18 => "Zeevo",
        19 => "Atmel",
        20 => "Mitsubishi",
        21 => "RTX Telecom",
        22 => "KC Technology",
        23 => "Newlogic",
        24 => "Transilica",
        25 => "Rohde-Schwarz",
        26 => "TTPCom",
        27 => "Signia",
        28 => "Conexant",
        29 => "Qualcomm",
        30 => "Inventel",
        31 => "AVM Berlin",
        32 => "BandSpeed",
        33 => "Mansella",
        34 => "NEC",
        35 => "Wave Plus Technology",
        36 => "Alcatel",
        37 => "Philips Semiconductor",
        38 => "C Technologies",
        39 => "Open Interface",
        40 => "RF Micro Devices",
        41 => "Hitachi",
        42 => "Symbol Technologies",
        43 => "Tenovis",
        44 => "Macronix International",
        72 => "Marvell",
        76 => "Apple",
        89 => "Nordic Semiconductor",
        283 => "Aruba Networks",
        65535 => "Internal Use",
        _ => "Unknown",
    }
}

/// The LMP version table of printDevice; negative values are the unset
/// sentinel and print without a version.
fn lmp_version_str(version: i32) -> Option<&'static str> {
    Some(match version {
        0 => "1.0b",
        1 => "1.1",
        2 => "1.2",
        3 => "2.0",
        4 => "2.1",
        5 => "3.0",
        6 => "4.0",
        7 => "4.1",
        8 => "4.2",
        9 => "5.0",
        10 => "5.1",
        11 => "5.2",
        12 => "5.3",
        13 => "5.4",
        14 => "6.0",
        15 => "6.1",
        16 => "6.2",
        17 => "6.3",
        _ => return None,
    })
}

#[derive(Clone)]
struct Radio {
    name: String,
    address: String,
    enabled: bool,
    discoverable: bool,
    connectable: bool,
    lmp_version: i32,
    /// Never published by BlueZ over D-Bus; upstream leaves it INT_MIN.
    lmp_subversion: i32,
    vendor: String,
}

fn detect_radio(hci: &str) -> Radio {
    let mut radio = Radio {
        name: String::new(),
        address: String::new(),
        enabled: false,
        discoverable: false,
        connectable: false,
        lmp_version: i32::MIN,
        lmp_subversion: i32::MIN,
        vendor: "Unknown".to_string(),
    };
    let props = dbus::get_all_properties_on_bus(
        1,
        "org.bluez",
        &format!("/org/bluez/{hci}"),
        "org.bluez.Adapter1",
    );
    if let Some(props) = props {
        for (key, value) in props {
            let value = value.strip_prefix('<').and_then(|s| s.strip_suffix('>')).unwrap_or(&value);
            // The rendered keys carry their quotes; trim before matching.
            match key.trim_matches('\'') {
                "Alias" => radio.name = util::string_value(value),
                "Address" => radio.address = util::string_value(value),
                "Manufacturer" => {
                    if let Ok(id) = value.parse::<u32>() {
                        radio.vendor = vendor_name(id).to_string();
                    }
                },
                "Version" => {
                    if let Ok(version) = value.parse::<i32>() {
                        radio.lmp_version = version;
                    }
                },
                "Powered" => radio.enabled = value == "true",
                "Discoverable" => radio.discoverable = value == "true",
                "Pairable" => radio.connectable = value == "true",
                _ => {},
            }
        }
    }
    radio
}

/// detectBluetooth: a missing sysfs dir is the only hard error; a failed
/// Properties.GetAll still appends the (mostly empty) radio, like upstream
/// discarding detectBluetoothRoot's status.
fn detect() -> Result<Vec<Radio>, &'static str> {
    static RADIOS: OnceLock<Result<Vec<Radio>, &'static str>> = OnceLock::new();
    RADIOS
        .get_or_init(|| {
            let Ok(entries) = std::fs::read_dir("/sys/class/bluetooth") else {
                return Err("Failed to open /sys/class/bluetooth");
            };
            Ok(entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| !name.starts_with('.') && !name.contains(':'))
                .map(|hci| detect_radio(&hci))
                .collect())
        })
        .clone()
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let radios = detect().unwrap_or_default();
    let enabled: Vec<&Radio> = radios.iter().filter(|radio| radio.enabled).collect();
    if enabled.is_empty() {
        // "Bluetooth radios found but none enabled" / "No devices detected"
        // are error rows, hidden by default.
        return Vec::new();
    }

    // The key is name-based, like fastfetch's "%s (%s)".

    enabled
        .iter()
        .map(|radio| {
            let value = match lmp_version_str(radio.lmp_version) {
                Some(version) => format!("Bluetooth {version} ({})", radio.vendor),
                None => radio.vendor.clone(),
            };
            Row::new(&format!("Bluetooth Radio ({})", radio.name), value)
        })
        .collect()
}

/// ffGenerateBluetoothRadioJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(radios) => {
            // lmpVersion/lmpSubversion keep their INT_MIN unset sentinel as
            // JSON null.
            let lmp = |version: i32| {
                if version == i32::MIN {
                    Json::Null
                } else {
                    Json::I64(version as i64)
                }
            };
            let mut arr = Json::Array(Vec::new());
            for radio in &radios {
                let mut obj = Json::obj();
                obj.push("name", radio.name.clone());
                obj.push("address", radio.address.clone());
                obj.push("lmpVersion", lmp(radio.lmp_version));
                obj.push("lmpSubversion", lmp(radio.lmp_subversion));
                obj.push("vendor", radio.vendor.clone());
                obj.push("enabled", radio.enabled);
                obj.push("discoverable", radio.discoverable);
                obj.push("connectable", radio.connectable);
                arr.push_value(obj);
            }
            JsonOutcome::Result(arr)
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn maps_lmp_versions_and_vendors() {
        assert_eq!(super::lmp_version_str(12), Some("5.3"));
        assert_eq!(super::lmp_version_str(9), Some("5.0"));
        assert_eq!(super::lmp_version_str(-1), None);
        assert_eq!(super::vendor_name(2), "Intel");
        assert_eq!(super::vendor_name(15), "Broadcom");
        assert_eq!(super::vendor_name(9999), "Unknown");
    }
}

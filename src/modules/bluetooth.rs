//! `bluetooth` module: BlueZ devices over the system bus, ported from
//! bluetooth_linux.c. org.freedesktop.DBus.ObjectManager.GetManagedObjects
//! is walked for object paths containing /dev_; the Device1 interface yields
//! name, address, icon, connected state and the stack bits (Class = seen
//! over BR/EDR, Appearance = LE-only), Battery1 the percentage, and RSSI is
//! mapped onto the 0..100 signal-quality scale. Unpaired devices and
//! disconnected ones (unless configured) are skipped; by default only
//! connected devices print.

use std::sync::OnceLock;

use crate::{
    dbus,
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    modules::colorize_percent,
    util,
};

/// ffRssiToSignalQuality: -50 dBm and above is a perfect link, -100 and
/// below none, linear in between.
fn rssi_to_signal_quality(rssi: i64) -> f64 {
    if rssi >= -50 {
        100.0
    } else if rssi <= -100 {
        0.0
    } else {
        (rssi + 100) as f64 * 2.0
    }
}

#[derive(Default)]
struct Device {
    name: String,
    address: String,
    /// The Device1 Icon property, upstream's `type` field.
    r#type: String,
    connected: bool,
    battery: u8,
    classic: bool,
    le: bool,
    /// RSSI mapped onto 0..100; unset when the property is absent.
    signal_quality: Option<f64>,
}

/// connectedDevices: /sys/class/bluetooth entries whose name contains a ':'
/// are connected devices (hci0:1 style names).
fn connected_devices() -> u32 {
    let Ok(entries) = std::fs::read_dir("/sys/class/bluetooth") else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().contains(':'))
        .count() as u32
}

/// One property of Device1 or Battery1.
fn apply_property(device: &mut Device, key: &str, value: &str) -> bool {
    let value = value.strip_prefix('<').and_then(|s| s.strip_suffix('>')).unwrap_or(value);
    match key {
        "Name" => device.name = util::string_value(value),
        "Address" => device.address = util::string_value(value),
        // BlueZ derives this from the Class of Device whenever there is one
        // and only falls back to the LE appearance otherwise, so `type`
        // describes a classic-seen device by its CoD.
        "Icon" => device.r#type = util::string_value(value),
        "Class" => device.classic = true,
        "Appearance" => device.le = true,
        "Percentage" => device.battery = value.parse::<i64>().unwrap_or(0).clamp(0, 255) as u8,
        "Connected" => device.connected = value == "true",
        // int16 dBm, written only from discovery reports and invalidated
        // when the session ends, so it says nothing about connection state.
        "RSSI" => device.signal_quality = value.parse::<i64>().ok().map(rssi_to_signal_quality),
        "Paired" => return value != "false", // false = drop the device
        _ => {},
    }
    true
}

fn walk_device(path: &str, interfaces: &str) -> Option<Device> {
    // We don't want adapter objects.
    if !path.contains("/dev_") {
        return None;
    }
    let mut device = Device::default();
    for (interface, props) in util::dict_entries(interfaces) {
        if !interface.contains(".Device") && !interface.contains(".Battery") {
            continue;
        }
        for (key, value) in util::dict_entries(&props) {
            if !apply_property(&mut device, &key, &value) {
                return None; // unpaired
            }
        }
    }
    Some(device)
}

fn detect() -> Result<Vec<Device>, &'static str> {
    // Only connected devices by default: skip the bus round trip when the
    // kernel reports none.
    let connected_count = connected_devices();
    if connected_count == 0 {
        return Ok(Vec::new());
    }

    static REPLY: OnceLock<Option<String>> = OnceLock::new();
    let reply = REPLY.get_or_init(|| {
        dbus::call_method_printed_on_bus(
            1, // DBUS_BUS_SYSTEM
            "org.bluez",
            "/",
            "org.freedesktop.DBus.ObjectManager",
            "GetManagedObjects",
        )
    });
    let Some(reply) = reply else {
        return Err("Failed to call GetManagedObjects");
    };

    // gdbus-style arg-list parens around the dict.
    let inner = reply.strip_prefix('(').and_then(|s| s.strip_suffix(')')).unwrap_or(reply);
    let mut devices = Vec::new();
    for (path, interfaces) in util::dict_entries(inner) {
        let path = util::unquote(&path);
        if let Some(device) = walk_device(&path, &interfaces)
            && device.connected
        {
            let mut device = device;
            if device.name.is_empty() {
                device.name = "Unknown Device".to_string();
            }
            devices.push(device);
            if devices.len() == connected_count as usize {
                break;
            }
        }
    }
    Ok(devices)
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let devices = detect().unwrap_or_default();
    if devices.is_empty() {
        // "No bluetooth devices found" is an error row, hidden by default.
        return Vec::new();
    }

    devices
        .iter()
        .enumerate()
        .map(|(index, device)| {
            let index = if devices.len() == 1 { 0 } else { index + 1 };
            let key = if index == 0 {
                "Bluetooth".to_string()
            } else {
                format!("Bluetooth {index}")
            };

            let mut value = device.name.clone();
            // The battery percent shares the gamepad config {50, 20} and
            // takes parentheses when a name is already on the line.
            if device.battery > 0 && device.battery <= 100 {
                let pct = colorize_percent(device.battery as f64, 50, 20, instance.color);
                if value.is_empty() {
                    value = pct;
                } else {
                    // ffPercentAppendNum takes parentheses when the name is
                    // already on the line.
                    value.push_str(&format!(" ({pct})"));
                }
            }
            if !device.connected {
                value.push_str(" [disconnected]");
            }
            Row::new(&key, value)
        })
        .collect()
}

/// ffGenerateBluetoothJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(devices) => {
            let mut arr = Json::Array(Vec::new());
            for device in &devices {
                let mut obj = Json::obj();
                obj.push("address", device.address.clone());
                obj.push("battery", device.battery as u64);
                obj.push("connected", device.connected);

                let mut device_types = Json::Array(Vec::new());
                if device.classic {
                    device_types.push_value("Classic");
                }
                if device.le {
                    device_types.push_value("Low Energy");
                }
                obj.push("deviceType", device_types);

                obj.push("name", device.name.clone());
                match device.signal_quality {
                    Some(quality) => obj.push("signalQuality", quality),
                    None => obj.push("signalQuality", Json::Null),
                }
                obj.push("type", device.r#type.clone());
                arr.push_value(obj);
            }
            JsonOutcome::Result(arr)
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::util;

    #[test]
    fn maps_rssi_onto_quality() {
        assert_eq!(super::rssi_to_signal_quality(-30), 100.0);
        assert_eq!(super::rssi_to_signal_quality(-63), 74.0);
        assert_eq!(super::rssi_to_signal_quality(-100), 0.0);
        assert_eq!(super::rssi_to_signal_quality(-120), 0.0);
    }

    #[test]
    fn parses_a_managed_objects_walk() {
        let reply = "({'/org/bluez/hci0': {'org.bluez.Adapter1': {'Address': <'04:CF:4B:33:C5:C9'>}}, \
                     '/org/bluez/hci0/dev_03_21_8B_91_16_4D': {'org.bluez.Device1': {'Address': <'03:21:8B:91:16:4D'>, 'Name': <'JBL TUNE160BT'>, 'Connected': <true>, 'Class': <7078148>}, 'org.bluez.Battery1': {'Percentage': <100>}}})";
        let inner = reply.strip_prefix('(').and_then(|s| s.strip_suffix(')')).unwrap_or(reply);
        let entries = util::dict_entries(inner);
        assert_eq!(entries.len(), 2);
        let (path, interfaces) = &entries[1];
        assert_eq!(util::unquote(path), "/org/bluez/hci0/dev_03_21_8B_91_16_4D");

        let mut device = super::Device::default();
        for (interface, props) in util::dict_entries(interfaces) {
            assert!(interface.contains(".Device") || interface.contains(".Battery"));
            for (key, value) in util::dict_entries(&props) {
                assert!(super::apply_property(&mut device, &key, &value));
            }
        }
        assert_eq!(device.name, "JBL TUNE160BT");
        assert!(device.connected);
        assert!(device.classic);
        assert!(!device.le);
        assert_eq!(device.battery, 100);
    }
}

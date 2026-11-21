//! `gamepad` module: joystick devices, ported from gamepad_linux.c. The
//! /sys/class/input/jsN entries name themselves through device/name and get
//! their battery from the first power_supply below the device that exposes
//! capacity (percentage) or capacity_level (Critical/Low/Normal/High/Full).

use std::path::Path;

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

fn battery_percent(device_path: &Path) -> u8 {
    let Ok(supplies) = std::fs::read_dir(device_path.join("device/power_supply")) else {
        return 0;
    };
    for supply in supplies.flatten() {
        if supply.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        // Percentage number (tested with a PS4 controller)
        if let Some(capacity) = util::read_trimmed_in(&supply.path(), "capacity")
            && let Ok(pct) = capacity.parse::<u8>()
        {
            return pct;
        }
        // Level name (tested with an NS Pro controller)
        if let Some(level) = util::read_trimmed_in(&supply.path(), "capacity_level") {
            let pct = match level.chars().next() {
                Some('C') => 1,   // Critical
                Some('L') => 25,  // Low
                Some('N') => 50,  // Normal
                Some('H') => 75,  // High
                Some('F') => 100, // Full
                _ => 0,
            };
            if pct > 0 {
                return pct;
            }
        }
    }
    0
}

pub struct GamepadDevice {
    pub name: String,
    pub serial: String,
    pub battery: u8,
}

fn detect() -> Vec<GamepadDevice> {
    let Ok(entries) = std::fs::read_dir("/sys/class/input") else {
        return Vec::new();
    };
    let mut devices = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("js") || name.chars().nth(2).is_none_or(|c| !c.is_ascii_digit()) {
            continue;
        }
        let Some(device_name) = util::read_trimmed_in(&entry.path(), "device/name") else {
            continue;
        };
        devices.push(GamepadDevice {
            battery: battery_percent(&entry.path()),
            serial: util::read_trimmed_in(&entry.path(), "device/uniq").unwrap_or_default(),
            name: device_name,
        });
    }
    devices
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let percent = super::percent_config(instance.entry, crate::format::PercentModuleConfig { green: 50, yellow: 20, type_flags: 0 });
    let ctx = super::print_ctx(instance);
    let df = &instance.display_format;
    let percent_type_flags = if percent.type_flags == 0 { df.percent_type } else { percent.type_flags };
    let devices = detect();
    if devices.is_empty() {
        // "No gamepad found" is an error row, hidden by default.
        return Vec::new();
    }

    devices
        .iter()
        .enumerate()
        .map(|(index, device)| {
            let index = if devices.len() == 1 { 0 } else { index + 1 };
            let key = if index == 0 {
                "Gamepad".to_string()
            } else {
                format!("{} {index}", instance.display_name("gamepad"))
            };

            // showBatteryLevel: the uint8_t battery's 0 means unknown.
            let show_battery = device.battery > 0 && device.battery <= 100;
            let mut value = String::new();
            if show_battery && percent_type_flags & crate::format::percent_type::BAR_BIT != 0 {
                crate::format::percent_append_bar(&mut value, device.battery as f64, percent, &ctx, df);
                value.push(' ');
            }
            if percent_type_flags & crate::format::percent_type::HIDE_OTHERS_BIT == 0 {
                value.push_str(&device.name);
            }
            if show_battery && percent_type_flags & crate::format::percent_type::NUM_BIT != 0 {
                if !value.is_empty() {
                    value.push(' ');
                }
                let parentheses = !value.is_empty();
                crate::format::percent_append_num(&mut value, device.battery as f64, percent, parentheses, &ctx, df);
            }
            Row::new(&key, value).with_index(index as u32)
        })
        .collect()
}

/// ffGenerateGamepadJsonResult: 0 means "no battery reported".
pub fn json(_instance: &Instance) -> JsonOutcome {
    let devices = detect();
    let mut arr = Json::Array(Vec::new());
    for device in &devices {
        let mut obj = Json::obj();
        obj.push("serial", device.serial.clone());
        obj.push("name", device.name.clone());
        obj.push("battery", device.battery as u64);
        obj.push("ignored", false);
        arr.push_value(obj);
    }
    JsonOutcome::Result(arr)
}

#[cfg(test)]
mod tests {
    #[test]
    fn js_names_need_a_digit() {
        assert!("js0".chars().nth(2).is_some_and(|c| c.is_ascii_digit()));
        assert!("js12".chars().nth(2).is_some_and(|c| c.is_ascii_digit()));
        assert!(!"json".chars().nth(2).is_some_and(|c| c.is_ascii_digit()));
    }
}

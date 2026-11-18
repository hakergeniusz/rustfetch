//! `gamepad` module: joystick devices, ported from gamepad_linux.c. The
//! /sys/class/input/jsN entries name themselves through device/name and get
//! their battery from the first power_supply below the device that exposes
//! capacity (percentage) or capacity_level (Critical/Low/Normal/High/Full).

use std::path::Path;

use crate::{instance::Instance, modules::Row, modules::colorize_percent, util};

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

fn detect() -> Vec<(String, u8)> {
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
        devices.push((device_name, battery_percent(&entry.path())));
    }
    devices
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let devices = detect();
    if devices.is_empty() {
        // "No gamepad found" is an error row, hidden by default.
        return Vec::new();
    }

    devices
        .iter()
        .enumerate()
        .map(|(index, (name, battery))| {
            let index = if devices.len() == 1 { 0 } else { index + 1 };
            let key = if index == 0 {
                "Gamepad".to_string()
            } else {
                format!("Gamepad {index}")
            };

            let mut value = name.clone();
            // The gamepad percent config is the inverted {50, 20}.
            if *battery > 0 && *battery <= 100 {
                let pct = colorize_percent(*battery as f64, 50, 20, instance.color);
                if value.is_empty() {
                    value = pct;
                } else {
                    value.push_str(&format!(" ({pct})"));
                }
            }
            Row::new(&key, value)
        })
        .collect()
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

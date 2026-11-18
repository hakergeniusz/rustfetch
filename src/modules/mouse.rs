//! `mouse` module: pointing devices, ported from mouse_linux.c. The
//! /sys/class/input/mouseN entries name themselves through device/name; the
//! serial from uniq only feeds the JSON format upstream.

use crate::{instance::Instance, modules::Row, util};

fn detect() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir("/sys/class/input") else {
        return Vec::new();
    };
    let mut devices = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("mouse")
            || name[5..].chars().next().is_none_or(|c| !c.is_ascii_digit())
        {
            continue;
        }
        if let Some(device_name) = util::read_trimmed_in(&entry.path(), "device/name") {
            devices.push(device_name);
        }
    }
    devices
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let devices = detect();
    if devices.is_empty() {
        // "No devices detected" is an error row, hidden by default.
        return Vec::new();
    }

    devices
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let index = if devices.len() > 1 { index + 1 } else { 0 };
            let key = if index == 0 {
                "Mouse".to_string()
            } else {
                format!("Mouse {index}")
            };
            Row::new(&key, name.clone())
        })
        .collect()
}

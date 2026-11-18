//! `keyboard` module: keyboards, ported from keyboard_linux.c. Entries in
//! /proc/bus/input/devices qualify when a "kbd" handler is attached, EV has
//! the auto-repeat bit (filters power buttons and PC speakers) and the last
//! KEY word has KEY_A (filters media remotes); the name comes from the
//! "N: Name=" line.

use crate::{instance::Instance, modules::Row};

const EV_REP: u64 = 20;
const KEY_A: u64 = 30;

fn detect() -> Vec<String> {
    detect_of(&std::fs::read_to_string("/proc/bus/input/devices").unwrap_or_default())
}

fn detect_of(content: &str) -> Vec<String> {
    let mut devices = Vec::new();
    let mut name = String::new();
    let mut skip_entry = false;

    for line in content.lines() {
        if line.is_empty() {
            if !skip_entry && !name.is_empty() {
                devices.push(name.clone());
            }
            name.clear();
            skip_entry = false;
            continue;
        }
        if skip_entry {
            continue;
        }

        if let Some(rest) = line.strip_prefix("N: Name=") {
            name = rest.trim().trim_matches('"').to_string();
        } else if let Some(rest) = line.strip_prefix("H: Handlers=") {
            if !rest.split(' ').any(|handler| handler == "kbd") {
                // No kbd handler: not a keyboard.
                skip_entry = true;
            }
        } else if let Some(rest) = line.strip_prefix("B: EV=") {
            let bits = u64::from_str_radix(rest.trim(), 16).unwrap_or(0);
            if bits & (1 << EV_REP) == 0 {
                skip_entry = true;
            }
        } else if let Some(rest) = line.strip_prefix("B: KEY=") {
            // The bitmap is space-separated hex longs, MSB first; KEY_A sits
            // in the last (least significant) word.
            let last_word = rest.trim().rsplit(' ').next().unwrap_or(rest.trim());
            let bits = u64::from_str_radix(last_word, 16).unwrap_or(0);
            if bits & (1 << KEY_A) == 0 {
                skip_entry = true;
            }
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

    let rows = devices
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let index = if devices.len() > 1 { index + 1 } else { 0 };
            let key = if index == 0 {
                "Keyboard".to_string()
            } else {
                format!("Keyboard {index}")
            };
            Row::new(&key, name.clone())
        })
        .collect();
    rows
}

#[cfg(test)]
mod tests {
    #[test]
    fn filters_pseudo_keyboards() {
        let devices = "\
N: Name=\"AT Translated Set 2 keyboard\"
P: Phys=isa0060/serio0/input0
H: Handlers=sysrq kbd event0
B: EV=120013
B: KEY=402000000 3803078f800d001 feffffdfffefffff fffffffffffffffe

N: Name=\"Power Button\"
P: Phys=LNXPWRBN/button/input0
H: Handlers=kbd event1
B: EV=3
B: KEY=10000000000000 0

N: Name=\"Media Remote\"
H: Handlers=rfkill kbd event2 leds
B: EV=13
B: KEY=fff00000000 0 0 0

N: Name=\"Real Keyboard\"
H: Handlers=kbd event3
B: EV=120013
B: KEY=c 0 0 40000000

";
        assert_eq!(
            super::detect_of(devices),
            ["AT Translated Set 2 keyboard", "Real Keyboard"]
        );
    }
}

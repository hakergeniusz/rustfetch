//! `battery` module: port of detection/battery/battery_linux.c and
//! modules/battery/battery.c. Batteries are enumerated from
//! /sys/class/power_supply: entries of type "Battery" (skipping absent
//! ones, "Mains" supplies — which set the shared AC-connected bit — and
//! "Device"-scoped batteries). The default value is
//! "<capacity>% [(<duration> remaining)] [<statuses>]"; a "Full" or
//! "Not charging" battery carries no status bits, so no brackets print
//! at all. The `temp` option (shared ffTempsParseJsonObject semantics)
//! enables the sysfs temperature read, in deci-degrees divided by 10.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

// FFBatteryStatus bits, in the printer's append order.
const STATUS_UNKNOWN: u8 = 1 << 0;
const STATUS_AC_CONNECTED: u8 = 1 << 1;
const STATUS_USB_CONNECTED: u8 = 1 << 2;
const STATUS_WIRELESS_CONNECTED: u8 = 1 << 3;
const STATUS_CHARGING: u8 = 1 << 4;
const STATUS_DISCHARGING: u8 = 1 << 5;
const STATUS_CRITICAL: u8 = 1 << 6;

pub struct BatteryResult {
    pub manufacturer: String,
    pub manufacture_date: String,
    pub model_name: String,
    pub technology: String,
    pub serial: String,
    pub status: u8,
    pub capacity: f64,
    pub cycle_count: u32,
    /// Seconds until empty, -1 if unknown (upstream int32_t timeRemaining).
    pub time_remaining: i32,
    /// FF_BATTERY_TEMP_UNSET (-DBL_MAX) unless `temp` asked for the read.
    pub temperature: f64,
}

/// FF_BATTERY_TEMP_UNSET: -DBL_MAX, never a real temperature.
const TEMP_UNSET: f64 = -f64::MAX;

/// The FFBatteryOptions our detection and rendering consume: the shared
/// `temp` flag plus its green/yellow thresholds.
pub(crate) struct BatteryOptions {
    temp: bool,
    temp_range: super::TempRange,
}

impl BatteryOptions {
    fn from_instance(instance: &Instance) -> Self {
        let mut options = Self { temp: false, temp_range: super::TempRange::default() };
        let Some(entry) = instance.entry else {
            return options;
        };
        if let Some(value) = entry.get("temp") {
            options.temp = super::parse_temp(value, &mut options.temp_range);
        }
        options
    }
}

/// ffStrbufToSInt: the leading integer or the default.
fn to_i64(text: &str, default: i64) -> i64 {
    text.trim().parse().unwrap_or(default)
}

/// parseBattery: one power_supply directory; Mains supplies return None
/// after updating `ac_connected`.
fn parse_battery(
    path: &std::path::Path,
    id: &str,
    ac_connected: &mut bool,
    options: &BatteryOptions,
) -> Option<BatteryResult> {
    // present == '0' means the device exists but is not populated.
    if util::read_trimmed_in(path, "present").is_some_and(|text| text == "0") {
        return None;
    }

    // type must exist and be "Battery"; "Mains" supplies only report AC.
    let battery_type = util::read_trimmed_in(path, "type")?;
    if battery_type == "Mains" {
        if !*ac_connected && util::read_trimmed_in(path, "online").is_some_and(|text| text == "1") {
            *ac_connected = true;
        }
        return None;
    }
    if battery_type != "Battery" {
        return None;
    }

    // Device-scoped batteries (e.g. peripherals) are not the laptop's.
    if util::read_trimmed_in(path, "scope").is_some_and(|scope| scope == "Device") {
        return None;
    }

    // capacity must exist.
    let capacity = util::read_trimmed_in(path, "capacity")?.parse().unwrap_or(0.0);

    let manufacturer = match util::read_trimmed_in(path, "manufacturer") {
        Some(text) => text,
        // asahi's SMC battery has no manufacturer file.
        None if id == "macsmc-battery" => "Apple Inc.".to_string(),
        None => String::new(),
    };
    let model_name = util::read_trimmed_in(path, "model_name").unwrap_or_default();
    let technology = util::read_trimmed_in(path, "technology").unwrap_or_default();

    let status_text = util::read_trimmed_in(path, "status").unwrap_or_default();
    let mut status = 0u8;
    let mut time_remaining = -1i32;

    // Unknown, Charging, Discharging, Not charging, Full — only the first
    // three map to status bits ("Full" and "Not charging" print none).
    if status_text == "Discharging" {
        status |= STATUS_DISCHARGING;
        if let Some(text) = util::read_trimmed_in(path, "time_to_empty_now") {
            time_remaining = to_i64(&text, 0) as i32;
        } else if let Some(text) = util::read_trimmed_in(path, "charge_now") {
            let charge_now = to_i64(&text, 0);
            if let Some(text) = (charge_now > 0)
                .then(|| util::read_trimmed_in(path, "current_now"))
                .flatten()
            {
                // Missing values default to INT64_MIN, whose absolute value
                // overflows back to itself and stays negative.
                let mut current_now = to_i64(&text, i64::MIN);
                if current_now < 0 {
                    current_now = current_now.wrapping_neg();
                }
                if current_now > 0 {
                    time_remaining = ((charge_now * 3600) / current_now) as i32;
                }
            }
        }
    } else if status_text == "Charging" {
        status |= STATUS_CHARGING;
    } else if status_text == "Unknown" {
        status |= STATUS_UNKNOWN;
    }

    if util::read_trimmed_in(path, "capacity_level").is_some_and(|level| level == "Critical") {
        status |= STATUS_CRITICAL;
    }

    let serial = util::read_trimmed_in(path, "serial_number").unwrap_or_default();

    let cycle_count = {
        let count = util::read_trimmed_in(path, "cycle_count")
            .map(|text| to_i64(&text, 0))
            .unwrap_or(0);
        if count < 0 || count > u32::MAX as i64 {
            0
        } else {
            count as u32
        }
    };

    let manufacture_date = (|| {
        let year = util::read_trimmed_in(path, "manufacture_year").map(|text| to_i64(&text, 0))?;
        if year <= 0 {
            return None;
        }
        let month = util::read_trimmed_in(path, "manufacture_month").map(|text| to_i64(&text, 0))?;
        if month <= 0 {
            return None;
        }
        let day = util::read_trimmed_in(path, "manufacture_day").map(|text| to_i64(&text, 0))?;
        if day <= 0 {
            return None;
        }
        Some(format!("{year:04}-{month:02}-{day:02}"))
    })()
    .unwrap_or_default();

    // options->temp gates the sysfs read: "temp" holds deci-degrees.
    let temperature = if options.temp {
        match util::read_trimmed_in(path, "temp") {
            Some(text) => match text.parse::<f64>() {
                Ok(value) => value / 10.0,
                Err(_) => TEMP_UNSET,
            },
            None => TEMP_UNSET,
        }
    } else {
        TEMP_UNSET
    };

    Some(BatteryResult {
        manufacturer,
        manufacture_date,
        model_name,
        technology,
        serial,
        status,
        capacity,
        cycle_count,
        time_remaining,
        temperature,
    })
}

fn detect(options: &BatteryOptions) -> Result<Vec<BatteryResult>, &'static str> {
    let mut results = Vec::new();
    let mut ac_connected = false;
    let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") else {
        return Err("opendir(\"/sys/class/power_supply/\") == nullptr");
    };
    for entry in entries.filter_map(Result::ok) {
        let id = entry.file_name().to_string_lossy().into_owned();
        if id.starts_with('.') {
            continue;
        }
        if let Some(battery) = parse_battery(&entry.path(), &id, &mut ac_connected, options) {
            results.push(battery);
        }
    }
    if ac_connected {
        for battery in &mut results {
            battery.status |= STATUS_AC_CONNECTED;
        }
    }
    Ok(results)
}

/// The status text pieces, in ffPrintBattery's append order.
fn status_texts(status: u8) -> Vec<&'static str> {
    [
        (STATUS_AC_CONNECTED, "AC Connected"),
        (STATUS_USB_CONNECTED, "USB Connected"),
        (STATUS_WIRELESS_CONNECTED, "Wireless Connected"),
        (STATUS_CHARGING, "Charging"),
        (STATUS_DISCHARGING, "Discharging"),
        (STATUS_CRITICAL, "Critical"),
        (STATUS_UNKNOWN, "Unknown"),
    ]
    .into_iter()
    .filter(|(bit, _)| status & bit != 0)
    .map(|(_, text)| text)
    .collect()
}

/// printBattery's default format for one battery; the USB and wireless
/// bits are unreachable on Linux detection but kept for parity.
fn item_value(battery: &BatteryResult, options: &BatteryOptions, color: bool, df: &crate::format::DisplayFormat) -> String {
    let mut value = String::new();
    if battery.capacity >= 0.0 {
        // ffPercentAppendNum with the battery's { green: 50, yellow: 20 }
        // thresholds (a high value is good) and the default NUM_COLOR bit.
        value.push_str(&super::colorize_percent(battery.capacity, 50, 20, color));
        if battery.time_remaining > 0 {
            value.push_str(" (");
            crate::format::duration_append_num(
                &mut value,
                battery.time_remaining as u32 as u64,
                df,
            );
            value.push_str(" remaining)");
        }
    }

    if battery.status != 0 {
        let statuses = status_texts(battery.status).join(", ");
        if !value.is_empty() {
            value.push_str(" [");
            value.push_str(&statuses);
            value.push(']');
        } else {
            value.push_str(&statuses);
        }
    }

    if battery.temperature != TEMP_UNSET {
        if !value.is_empty() {
            value.push_str(" - ");
        }
        value.push_str(&super::temps_num(
            battery.temperature,
            options.temp_range.green,
            options.temp_range.yellow,
            color,
        ));
    }

    value
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let options = BatteryOptions::from_instance(instance);
    let Ok(results) = detect(&options) else {
        // "opendir(...) == nullptr" is an error row, hidden by default.
        return Vec::new();
    };
    if results.is_empty() {
        // "No batteries found" is an error row, hidden by default.
        return Vec::new();
    }
    results
        .iter()
        .map(|battery| {
            let key = if battery.model_name.is_empty() {
                "Battery".to_string()
            } else {
                format!("{} ({})", instance.display_name("battery"), battery.model_name)
            };
            Row::new(&key, item_value(battery, &options, instance.color, &instance.display_format))
        })
        .collect()
}

/// ffGenerateBatteryJsonResult: the status array uses the generator's
/// append order; the temperature is real only when detection filled it
/// and a non-positive time remaining prints null.
pub fn json(instance: &Instance) -> JsonOutcome {
    let options = BatteryOptions::from_instance(instance);
    match detect(&options) {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(results) => {
            let mut arr = Json::Array(Vec::new());
            for battery in &results {
                let mut obj = Json::obj();
                obj.push("capacity", battery.capacity);
                obj.push("manufacturer", battery.manufacturer.clone());
                obj.push("manufactureDate", battery.manufacture_date.clone());
                obj.push("modelName", battery.model_name.clone());
                obj.push("technology", battery.technology.clone());
                obj.push("serial", battery.serial.clone());
                if battery.temperature != TEMP_UNSET {
                    obj.push("temperature", battery.temperature);
                } else {
                    obj.push("temperature", Json::Null);
                }
                obj.push("cycleCount", battery.cycle_count as u64);
                if battery.time_remaining > 0 {
                    obj.push("timeRemaining", battery.time_remaining as i64);
                } else {
                    obj.push("timeRemaining", Json::Null);
                }
                let mut status = Json::Array(Vec::new());
                for text in status_texts(battery.status) {
                    status.push_value(Json::Str(text.to_string()));
                }
                obj.push("status", status);
                arr.push_value(obj);
            }
            JsonOutcome::Result(arr)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battery(status: u8, capacity: f64, time_remaining: i32) -> BatteryResult {
        BatteryResult {
            manufacturer: String::new(),
            manufacture_date: String::new(),
            model_name: String::new(),
            technology: String::new(),
            serial: String::new(),
            status,
            capacity,
            cycle_count: 0,
            time_remaining,
            temperature: TEMP_UNSET,
        }
    }

    #[test]
    fn temp_option_value_shapes() {
        // ffTempsParseJsonObject: boolean, null, thresholds object, and the
        // two usage errors.
        let mut range = super::super::TempRange::default();
        let Ok(true_value) = crate::jsonc::Parser::parse("true", false) else { panic!("parse failed") };
        let Ok(null_value) = crate::jsonc::Parser::parse("null", false) else { panic!("parse failed") };
        let Ok(object_value) =
            crate::jsonc::Parser::parse(r#"{"green": 30, "yellow": 90}"#, false)
        else {
            panic!("parse failed")
        };
        assert!(super::super::parse_temp(&true_value, &mut range));
        assert!(!super::super::parse_temp(&null_value, &mut range));
        assert!(super::super::parse_temp(&object_value, &mut range));
        assert_eq!((range.green, range.yellow), (30, 90));
    }

    #[test]
    fn temperature_prints_after_the_statuses() {
        let options = super::BatteryOptions { temp: true, temp_range: super::super::TempRange::default() };
        let mut hot = battery(STATUS_DISCHARGING, 85.0, -1);
        hot.temperature = 95.5;
        assert_eq!(item_value(&hot, &options, false, &Default::default()), "85% [Discharging] - 95.5°C");
        // The capacity block prints even when no status bit is recognized.
        hot.status = 0;
        assert_eq!(item_value(&hot, &options, false, &Default::default()), "85% - 95.5°C");
        // A battery with no other info prints the bare temperature.
        hot.capacity = -1.0;
        assert_eq!(item_value(&hot, &options, false, &Default::default()), "95.5°C");
        // The unset sentinel prints nothing, even with temp enabled.
        let cold = battery(0, 50.0, -1);
        assert_eq!(item_value(&cold, &options, false, &Default::default()), "50%");
    }

    #[test]
    fn status_maps_sysfs_strings_to_bits() {
        assert_eq!(status_texts(STATUS_AC_CONNECTED), vec!["AC Connected"]);
        assert_eq!(
            status_texts(STATUS_AC_CONNECTED | STATUS_DISCHARGING | STATUS_CRITICAL),
            vec!["AC Connected", "Discharging", "Critical"]
        );
        // "Full" and "Not charging" map to no bit at all.
        assert_eq!(status_texts(0), Vec::<&str>::new());
    }

    #[test]
    fn value_matches_the_default_format() {
        let options = super::BatteryOptions { temp: false, temp_range: super::super::TempRange::default() };
        // A discharging battery with a known time remaining.
        assert_eq!(
            item_value(&battery(STATUS_DISCHARGING, 85.0, 95_400), &options, false, &Default::default()),
            "85% (1 day, 2 hours, 30 mins remaining) [Discharging]"
        );
        // A full battery has no status bits, so no brackets print.
        assert_eq!(item_value(&battery(0, 100.0, -1), &options, false, &Default::default()), "100%");
        // The status text alone still prints, without brackets (capacity 0
        // still passes the capacity >= 0 gate).
        assert_eq!(
            item_value(&battery(STATUS_AC_CONNECTED, 0.0, -1), &options, false, &Default::default()),
            "0% [AC Connected]"
        );
    }
}

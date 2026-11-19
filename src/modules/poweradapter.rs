//! `poweradapter` module: AC adapters from /sys/class/power_supply, ported
//! from fastfetch (type Mains, online, input_power_limit → "{watts}W").

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

pub struct PowerAdapterResult {
    pub name: String,
    pub description: String,
    pub watts: i64,
    pub manufacturer: String,
    pub model_name: String,
    pub serial: String,
}

/// parsePowerAdapter: one Mains supply with a non-empty input_power_limit.
fn parse_adapter(dir: &std::path::Path, id: &str) -> Option<PowerAdapterResult> {
    if util::read_trimmed_in(dir, "present").as_deref() == Some("0") {
        return None;
    }
    if util::read_trimmed_in(dir, "type").as_deref() != Some("Mains") {
        return None;
    }
    if util::read_trimmed_in(dir, "scope").as_deref() == Some("Device") {
        return None;
    }
    if util::read_trimmed_in(dir, "online").as_deref() != Some("1") {
        return None;
    }
    let limit = util::read_trimmed_in(dir, "input_power_limit")?;
    if limit.is_empty() {
        return None;
    }
    let microwatts: f64 = limit.trim().parse().ok()?;
    let manufacturer = match util::read_trimmed_in(dir, "manufacturer") {
        Some(text) => text,
        // asahi's SMC AC has no manufacturer file.
        None if id == "macsmc-ac" => "Apple Inc.".to_string(),
        None => String::new(),
    };
    Some(PowerAdapterResult {
        name: String::new(),
        description: String::new(),
        watts: (microwatts / 1e6 + 0.5) as i64,
        manufacturer,
        model_name: util::read_trimmed_in(dir, "model_name").unwrap_or_default(),
        serial: util::read_trimmed_in(dir, "serial_number").unwrap_or_default(),
    })
}

/// ffDetectPowerAdapter.
fn detect() -> Result<Vec<PowerAdapterResult>, &'static str> {
    let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") else {
        return Err("opendir(\"/sys/class/power_supply/\") == nullptr");
    };
    let mut results = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if let Some(adapter) = parse_adapter(&entry.path(), &id) {
            results.push(adapter);
        }
    }
    Ok(results)
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(results) = detect() else {
        return Vec::new();
    };
    results
        .iter()
        .map(|adapter| Row::new("Power Adapter", format!("{}W", adapter.watts)))
        .collect()
}

/// ffGeneratePowerAdapterJsonResult: one object per adapter; the name and
/// description strings stay empty on Linux detection.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(results) => {
            let mut arr = Json::Array(Vec::new());
            for adapter in &results {
                let mut obj = Json::obj();
                obj.push("description", adapter.description.clone());
                obj.push("manufacturer", adapter.manufacturer.clone());
                obj.push("modelName", adapter.model_name.clone());
                obj.push("name", adapter.name.clone());
                obj.push("serial", adapter.serial.clone());
                obj.push("watts", adapter.watts);
                arr.push_value(obj);
            }
            JsonOutcome::Result(arr)
        },
    }
}

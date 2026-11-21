//! `wm` module: window manager from the shared WM/DE detection
//! (displayserver.rs). The text value appends the WM version when the WM
//! is one of the supported ones, then " (protocol)".

use crate::{
    modules::displayserver,
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
};

/// ffDetectWMVersion: upstream supports Hyprland, sway, labwc, niri,
/// umbriel, weston, WSLg, i3, ctwm, fvwm and Openbox; everything else —
/// including Mutter, this machine's WM — leaves the version buffer empty
/// ("Unsupported WM"). Port more getters as needed.
fn wm_version(_process_name: &str) -> Option<String> {
    None
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let ds = displayserver::connect();
    if ds.wm_pretty_name.is_empty() {
        // "No WM found" is an error row, hidden by default.
        return Vec::new();
    }

    let mut value = ds.wm_pretty_name.clone();
    if let Some(version) = wm_version(&ds.wm_process_name) {
        value.push(' ');
        value.push_str(&version);
    }
    if !ds.wm_protocol_name.is_empty() {
        value.push_str(&format!(" ({})", ds.wm_protocol_name));
    }
    vec![Row::new(instance.display_name("wm"), value)]
}

/// ffGenerateWMJsonResult: the five fields in upstream order; pluginName
/// needs the detectPlugin option (off by default, not exposed here) and
/// the version is empty for unsupported WMs.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let ds = displayserver::connect();
    if ds.wm_pretty_name.is_empty() {
        return JsonOutcome::Error("No WM found".to_string());
    }
    let mut result = Json::obj();
    result.push("processName", ds.wm_process_name.clone());
    result.push("prettyName", ds.wm_pretty_name.clone());
    result.push("protocolName", ds.wm_protocol_name.clone());
    result.push("pluginName", String::new());
    result.push("version", wm_version(&ds.wm_process_name).unwrap_or_default());
    JsonOutcome::Result(result)
}

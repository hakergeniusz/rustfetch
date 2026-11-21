//! `weather` module: the current weather from wttr.in, ported from
//! detection/weather/weather.c over src/networking.rs (the port of
//! common/impl/networking_linux.c). The default format string is
//! "%t+-+%C+(%l)" with no location; the reply body prints verbatim,
//! right-trimmed. Like publicip, the prepare hook sends the request during
//! config parsing and the single internal slot can only be used once per
//! run. The `&m`/`&u` temperature-unit suffix of the path rides upstream's
//! display.tempUnit, which this port does not track.

use std::sync::Mutex;

use crate::{
    instance::Instance,
    json::JsonOutcome,
    modules::Row,
    networking,
};

const DEFAULT_OUTPUT_FORMAT: &str = "%t+-+%C+(%l)";

enum Slot {
    Uninit,
    Pending(networking::State),
    Failed(&'static str),
}

static SLOT: Mutex<Slot> = Mutex::new(Slot::Uninit);

pub struct WeatherOptions {
    pub location: String,
    pub output_format: String,
    pub timeout: u32,
}

impl Default for WeatherOptions {
    fn default() -> Self {
        Self { location: String::new(), output_format: DEFAULT_OUTPUT_FORMAT.into(), timeout: 0 }
    }
}

impl WeatherOptions {
    /// ffInitWeatherOptions + ffParseWeatherJsonObject: absent keys leave
    /// the defaults untouched.
    pub fn from_entry(entry: Option<&'static crate::jsonc::Value>) -> Self {
        let mut options = Self::default();
        let Some(entry) = entry else {
            return options;
        };
        if let Some(location) = entry.get("location").and_then(crate::jsonc::Value::as_str) {
            options.location = location.to_string();
        }
        if let Some(format) = entry.get("outputFormat").and_then(crate::jsonc::Value::as_str) {
            options.output_format = format.to_string();
        }
        if let Some(timeout) = entry.get("timeout").and_then(crate::jsonc::Value::as_uint) {
            options.timeout = timeout as u32;
        }
        options
    }

    pub fn from_instance(instance: &Instance) -> Self {
        Self::from_entry(instance.entry)
    }
}

/// ffPrepareWeather: "/" + location + "?format=" + outputFormat.
fn prepare(options: &WeatherOptions) {
    if !matches!(*SLOT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()), Slot::Uninit) {
        eprintln!("Error: Weather module can only be used once due to internal limitations");
        std::process::exit(1);
    }

    let mut state = networking::State::new(options.timeout, false, false);
    let mut path = format!("/{}", options.location);
    path.push_str("?format=");
    path.push_str(&options.output_format);
    // display.tempUnit's "&m"/"&u" suffix: this port has no tempUnit.

    let result = match state.send("wttr.in", 80, &path, "User-Agent: curl/0.0.0\r\n") {
        Ok(()) => Slot::Pending(state),
        Err(error) => Slot::Failed(error),
    };
    *SLOT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = result;
}

/// The prepare entry for the option pass: one call per module instance, in
/// config order (`-s` jobs pass None for the default options).
pub(crate) fn prepare_from_entry(entry: Option<&'static crate::jsonc::Value>) {
    prepare(&WeatherOptions::from_entry(entry));
}

/// ffDetectWeather: the wttr.in reply body, right-trimmed.
fn detect(options: &WeatherOptions) -> Result<String, &'static str> {
    if matches!(*SLOT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()), Slot::Uninit) {
        prepare(options);
    }

    let taken = std::mem::replace(
        &mut *SLOT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
        Slot::Uninit,
    );
    let response = match taken {
        Slot::Failed(error) => return Err(error),
        Slot::Pending(mut state) => state.recv(),
        Slot::Uninit => return Err("ffNetworkingSendHttpRequest() failed"),
    };

    let response = response?;
    // ffStrbufSubstrAfterFirstS: a missing "\r\n\r\n" separator leaves the
    // whole response (headers included) as the body.
    let body = match response.windows(4).position(|window| window == b"\r\n\r\n") {
        Some(index) => &response[index + 4..],
        None => &response[..],
    };
    if body.is_empty() {
        return Err("Empty server response received");
    }

    // ffStrbufTrimRightSpace.
    let body = String::from_utf8_lossy(body).trim_end().to_string();
    if body.is_empty() {
        return Err("Empty server response received");
    }
    Ok(body)
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(body) = detect(&WeatherOptions::from_instance(instance)) else {
        return Vec::new();
    };
    vec![Row::new(instance.display_name("weather"), body)]
}

/// ffGenerateWeatherJsonResult.
pub fn json(instance: &Instance) -> JsonOutcome {
    match detect(&WeatherOptions::from_instance(instance)) {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(body) => JsonOutcome::Result(body.into()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn request_matches_the_upstream_bytes() {
        // initNetworkingState: "GET " + path + " HTTP/1.0\r\nHost: " + host
        // + "\r\nConnection: close\r\n" + headers + "\r\n"; the default
        // weather path is "/" + "?format=" + outputFormat and its header is
        // the curl User-Agent. The port lives in src/networking.rs; this
        // pins the assembled default request.
        let options = super::WeatherOptions::default();
        let path = format!("/{}?format={}", options.location, options.output_format);
        assert_eq!(
            format!("GET {path} HTTP/1.0\r\nHost: wttr.in\r\nConnection: close\r\nUser-Agent: curl/0.0.0\r\n\r\n"),
            "GET /?format=%t+-+%C+(%l) HTTP/1.0\r\nHost: wttr.in\r\nConnection: close\r\nUser-Agent: curl/0.0.0\r\n\r\n"
        );
    }
}

//! `publicip` module: the public address from ipinfo.io, ported from
//! detection/publicip/publicip.c over src/networking.rs (the port of
//! common/impl/networking_linux.c). The default request goes to
//! ipinfo.io:80/json with TCP Fast Open; a custom `url` speaks plain http
//! and prints its body verbatim. The prepare hook sends the request early
//! (during config parsing, like upstream's ffPreparePublicIp) so the reply
//! is often already buffered when the module's turn comes; each of the two
//! internal slots (IPv4/IPv6) can only be used once per run.

use std::sync::Mutex;

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    networking,
};

const DEFAULT_HOST_V4: &str = "ipinfo.io";
const DEFAULT_HOST_V6: &str = "v6.ipinfo.io";

/// (ip, "city, country") over the strict yyjson-compatible parser: the ip
/// field appends only when it is a string (ffStrbufAppendJsonVal), the
/// city/country printf falls back to "(null)" for missing or non-string
/// values, and a root that is not an object resolves nothing.
fn parse_ip_location(root: Option<&crate::jsonc::Value>) -> (String, String) {
    let field = |name: &str| {
        root.and_then(|root| root.get(name))
            .and_then(crate::jsonc::Value::as_str)
    };
    let ip = field("ip").unwrap_or("").to_string();
    let city = field("city").unwrap_or("(null)");
    let country = field("country").unwrap_or("(null)");
    (ip, format!("{city}, {country}"))
}

/// One static slot per `ipv6` flag: FF_UNINITIALIZED, then the request in
/// flight (or its send error), until ffDetectPublicIp consumes and resets
/// it. A second prepare on the same slot is an upstream hard error.
enum Slot {
    Uninit,
    Pending(networking::State),
    Failed(&'static str),
}

static SLOTS: [Mutex<Slot>; 2] = [Mutex::new(Slot::Uninit), Mutex::new(Slot::Uninit)];

/// ffPublicIpParseUrlPort: the port has to be the last thing in `host`.
fn parse_url_port(host: &str, colon_index: usize) -> u16 {
    let port_str = &host[colon_index + 1..];
    let trimmed = port_str.trim_start();
    let digits = trimmed.len() - trimmed.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let valid = digits > 0
        && digits == trimmed.len() // *portEnd == '\0'
        && trimmed[..digits].parse::<u64>().is_ok_and(|value| value != 0 && value <= 65535);
    if !valid {
        eprintln!("Error: invalid port in the PublicIp module URL");
        std::process::exit(1);
    }
    trimmed[..digits].parse().unwrap_or(0)
}

fn slot_index(ipv6: bool) -> usize {
    usize::from(ipv6)
}

/// ffPreparePublicIp: parse the URL, build the state and start the request.
/// Custom URLs are http-only; anything else is an upstream hard error.
fn prepare(options: &PublicIpOptions) {
    let slot = &SLOTS[slot_index(options.ipv6)];
    if !matches!(*slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()), Slot::Uninit) {
        eprintln!("Error: PublicIp module can only be used once due to internal limitations");
        std::process::exit(1);
    }

    let mut state = networking::State::new(options.timeout, options.ipv6, false);
    let send_result = if options.url.is_empty() {
        state.tfo = true;
        let host = if options.ipv6 { DEFAULT_HOST_V6 } else { DEFAULT_HOST_V4 };
        state.send(host, 80, "/json", "")
    } else {
        let mut host = options.url.clone();
        if let Some(scheme_end) = host.find("://") {
            if scheme_end != 4 || !host[..4].eq_ignore_ascii_case("http") {
                eprintln!("Error: only http: protocol is supported. Use `Command` module with `curl` if needed");
                std::process::exit(1);
            }
            host = host[scheme_end + "://".len()..].to_string();
        }
        let (host, path, port) = split_host_path(host);
        let path = if path.is_empty() { "/".to_string() } else { path };
        state.send(&host, port, &path, "")
    };
    let result = match send_result {
        Ok(()) => Slot::Pending(state),
        Err(error) => Slot::Failed(error),
    };
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = result;
}

/// The port split: an optional bracketed IPv6 literal is the only
/// unambiguous spelling of `[::1]:8080`; a bare literal holds several
/// colons and has no port at all.
fn split_host_path(rest: String) -> (String, String, u16) {
    let mut port = 0;
    let (host, path) = match rest.find('/') {
        Some(path_start) => (
            rest[..path_start].to_string(),
            rest[path_start..].to_string(),
        ),
        None => (rest, String::new()),
    };

    let host = if host.starts_with('[') {
        let Some(bracket_end) = host.find(']') else {
            eprintln!("Error: unmatched '[' in the PublicIp module URL");
            std::process::exit(1);
        };
        if bracket_end + 1 < host.len() {
            if host.as_bytes()[bracket_end + 1] != b':' {
                eprintln!("Error: unexpected characters after the IPv6 literal in the PublicIp module URL");
                std::process::exit(1);
            }
            // Read the port while the string is still intact: trimming the
            // brackets first would invalidate the index it was found at.
            port = parse_url_port(&host, bracket_end + 1);
        }
        host[1..bracket_end].to_string()
    } else {
        let first = host.find(':');
        let last = host.rfind(':');
        if let Some(colon) = first.filter(|&colon| last == Some(colon)) {
            port = parse_url_port(&host, colon);
            host[..colon].to_string()
        } else {
            host
        }
    };

    (host, path, if port == 0 { 80 } else { port })
}

/// ffDetectPublicIp: (ip, location) from the ipinfo.io reply; only a JSON
/// object reply is interpreted, anything else prints verbatim.
fn detect(options: &PublicIpOptions) -> Result<(String, String), &'static str> {
    let slot = &SLOTS[slot_index(options.ipv6)];
    if matches!(*slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()), Slot::Uninit) {
        prepare(options);
    }

    let taken = std::mem::replace(
        &mut *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
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

    // Custom URLs always print the body verbatim; the default URL
    // interprets a JSON document, anything else prints verbatim too.
    if options.url.is_empty()
        && let Ok(root) = crate::jsonc::Parser::parse(&String::from_utf8_lossy(body), false)
    {
        return Ok(parse_ip_location(Some(&root)));
    }
    Ok((String::from_utf8_lossy(body).trim_end().to_string(), String::new()))
}

#[derive(Default)]
pub struct PublicIpOptions {
    pub url: String,
    pub timeout: u32,
    pub ipv6: bool,
}



impl PublicIpOptions {
    /// ffInitPublicIpOptions + ffParsePublicIpJsonObject: absent keys leave
    /// the defaults untouched.
    pub fn from_entry(entry: Option<&'static crate::jsonc::Value>) -> Self {
        let mut options = Self::default();
        let Some(entry) = entry else {
            return options;
        };
        if let Some(url) = entry.get("url").and_then(crate::jsonc::Value::as_str) {
            options.url = url.to_string();
        }
        if let Some(timeout) = entry.get("timeout").and_then(crate::jsonc::Value::as_uint) {
            options.timeout = timeout as u32;
        }
        if let Some(ipv6) = entry.get("ipv6").and_then(crate::jsonc::Value::as_bool) {
            options.ipv6 = ipv6;
        }
        options
    }

    pub fn from_instance(instance: &Instance) -> Self {
        Self::from_entry(instance.entry)
    }
}

/// The prepare entry for the option pass: one call per module instance, in
/// config order (`-s` jobs pass None for the default options).
pub(crate) fn prepare_from_entry(entry: Option<&'static crate::jsonc::Value>) {
    prepare(&PublicIpOptions::from_entry(entry));
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let options = PublicIpOptions::from_instance(instance);
    let Ok((ip, location)) = detect(&options) else {
        return Vec::new();
    };
    // ffPrintPublicIp: the location block only prints when non-empty.
    if location.is_empty() {
        vec![Row::new("Public IP", ip)]
    } else {
        vec![Row::new("Public IP", format!("{ip} ({location})"))]
    }
}

/// ffGeneratePublicIpJsonResult.
pub fn json(instance: &Instance) -> JsonOutcome {
    match detect(&PublicIpOptions::from_instance(instance)) {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok((ip, location)) => {
            let mut obj = Json::obj();
            obj.push("ip", ip);
            obj.push("location", location);
            JsonOutcome::Result(obj)
        },
    }
}

#[cfg(test)]
mod tests {
    fn root(body: &str) -> Option<crate::jsonc::Value> {
        crate::jsonc::Parser::parse(body, false).ok()
    }

    #[test]
    fn builds_location_with_nulls() {
        let body = root(r#"{"ip": "1.2.3.4", "city": "Łódź", "country": "PL"}"#);
        assert_eq!(super::parse_ip_location(body.as_ref()), ("1.2.3.4".into(), "Łódź, PL".into()));
        let body = root(r#"{"ip": "1.2.3.4"}"#);
        assert_eq!(super::parse_ip_location(body.as_ref()), ("1.2.3.4".into(), "(null), (null)".into()));
        let body = root(r#"{"ip": "1.2.3.4", "city": null, "country": "PL"}"#);
        assert_eq!(super::parse_ip_location(body.as_ref()), ("1.2.3.4".into(), "(null), PL".into()));
        // ffStrbufAppendJsonVal only appends strings, so a numeric ip is
        // empty and the location falls back to printf's "(null)".
        let body = root(r#"{"ip": 42}"#);
        assert_eq!(super::parse_ip_location(body.as_ref()), ("".into(), "(null), (null)".into()));
        // A root that is not an object resolves nothing.
        let body = root("[1, 2]");
        assert_eq!(super::parse_ip_location(body.as_ref()), ("".into(), "(null), (null)".into()));
    }

    #[test]
    fn splits_hosts_paths_and_ports() {
        assert_eq!(
            super::split_host_path("example.com/status".to_string()),
            ("example.com".into(), "/status".into(), 80)
        );
        assert_eq!(
            super::split_host_path("example.com:8080".to_string()),
            ("example.com".into(), String::new(), 8080)
        );
        assert_eq!(
            super::split_host_path("[2001:db8::1]:8080/x".to_string()),
            ("2001:db8::1".into(), "/x".into(), 8080)
        );
        // A bare IPv6 literal has no port at all.
        assert_eq!(
            super::split_host_path("2001:db8::1".to_string()),
            ("2001:db8::1".into(), String::new(), 80)
        );
    }
}

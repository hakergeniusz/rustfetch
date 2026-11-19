//! `publicip` module: the public address from ipinfo.io, ported from
//! detection/publicip/publicip.c with the raw-socket HTTP client of
//! common/impl/networking_linux.c (the TCP Fast Open fast path reduces to
//! connect+send with std::net). The default request is a blocking
//! HTTP/1.0 GET to ipinfo.io:80 /json, exactly like fastfetch's
//! timeout=0 default; the reply's "ip", "city" and "country" fields form
//! the value. Custom URLs upstream only arrive through the JSON config,
//! which this port does not parse for module options.

use std::io::{Read, Write};
use std::net::TcpStream;

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
};

const REQUEST: &str = "GET /json HTTP/1.0\r\nHost: ipinfo.io\r\nConnection: close\r\n\r\n";
const MAX_RESPONSE_SIZE: usize = 1024 * 1024;

/// json_string_field from the module registry: finds `"name": "value"`.
fn json_string_field(object: &str, name: &str) -> Option<String> {
    let needle = format!("\"{name}\"");
    let pos = object.find(&needle)? + needle.len();
    let rest = object[pos..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut value = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next() {
                Some('n') => value.push('\n'),
                Some('t') => value.push('\t'),
                Some('r') => value.push('\r'),
                Some('b') => value.push('\u{8}'),
                Some('f') => value.push('\u{c}'),
                Some(escaped) => value.push(escaped),
                None => break,
            },
            other => value.push(other),
        }
    }
    Some(value)
}

/// The http response body after the "\r\n\r\n" header separator; when the
/// separator is missing, ffStrbufSubstrAfterFirstS leaves the buffer
/// untouched, so the whole response (headers included) is the body.
fn response_body(response: &str) -> &str {
    match response.find("\r\n\r\n") {
        Some(index) => &response[index + 4..],
        None => response,
    }
}

/// (ip, "city, country") with yyjson_get_str's "(null)" default for missing
/// or non-string values, as glibc printf would print them.
fn parse_ip_location(body: &str) -> (String, String) {
    let ip = json_string_field(body, "ip").unwrap_or_default();
    let city = json_string_field(body, "city").unwrap_or_else(|| "(null)".into());
    let country = json_string_field(body, "country").unwrap_or_else(|| "(null)".into());
    (ip, format!("{city}, {country}"))
}

/// ffDetectPublicIp: (ip, location) from the ipinfo.io reply; only a JSON
/// object reply is interpreted, anything else prints verbatim.
fn detect() -> Result<(String, String), &'static str> {
    let Ok(mut stream) = TcpStream::connect("ipinfo.io:80") else {
        return Err("connect() failed");
    };
    if stream.write_all(REQUEST.as_bytes()).is_err() {
        return Err("connect() failed");
    }

    let mut response = Vec::new();
    if stream.take((MAX_RESPONSE_SIZE + 1) as u64).read_to_end(&mut response).is_err()
        || response.len() > MAX_RESPONSE_SIZE
    {
        return Err("Response too large");
    }
    let response = String::from_utf8_lossy(&response);
    let body = response_body(&response);
    if body.is_empty() {
        return Err("Empty server response received");
    }

    if body.starts_with('{') {
        Ok(parse_ip_location(body))
    } else {
        // Custom servers may answer with plain text; the ip field carries it.
        Ok((body.trim_end().to_string(), String::new()))
    }
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok((ip, location)) = detect() else {
        return Vec::new();
    };
    vec![Row::new("Public IP", format!("{ip} ({location})"))]
}

/// ffGeneratePublicIpJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
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
    #[test]
    fn splits_headers_and_bodies() {
        assert_eq!(super::response_body("HTTP/1.0 200 OK\r\n\r\nip"), "ip");
        // Missing separator keeps everything.
        assert_eq!(super::response_body("HTTP/1.0 200 OK"), "HTTP/1.0 200 OK");
    }

    #[test]
    fn builds_location_with_nulls() {
        let body = r#"{"ip": "1.2.3.4", "city": "Łódź", "country": "PL"}"#;
        assert_eq!(super::parse_ip_location(body), ("1.2.3.4".into(), "Łódź, PL".into()));
        let body = r#"{"ip": "1.2.3.4"}"#;
        assert_eq!(super::parse_ip_location(body), ("1.2.3.4".into(), "(null), (null)".into()));
        let body = r#"{"ip": "1.2.3.4", "city": null, "country": "PL"}"#;
        assert_eq!(super::parse_ip_location(body), ("1.2.3.4".into(), "(null), PL".into()));
    }

    #[test]
    fn decodes_escapes() {
        assert_eq!(
            super::json_string_field(r#"{"city": "a\"b\\c"}"#, "city"),
            Some(r#"a"b\c"#.to_string())
        );
    }
}

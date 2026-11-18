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

use crate::{instance::Instance, modules::Row};

const REQUEST: &str = "GET /json HTTP/1.0\r\nHost: ipinfo.io\r\nConnection: close\r\n\r\n";
const MAX_RESPONSE_SIZE: usize = 1024 * 1024;

/// yyjson_get_str in a glibc printf: a missing or non-string JSON value
/// prints as "(null)".
fn string_or_null(json: &str, key: &str) -> Option<String> {
    json_string_field(json, key)
}

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

/// The value shown for a parsed ipinfo.io reply: `ip (city, country)`.
fn parsed_value(body: &str) -> String {
    let ip = json_string_field(body, "ip").unwrap_or_default();
    let city = string_or_null(body, "city").unwrap_or_else(|| "(null)".into());
    let country = string_or_null(body, "country").unwrap_or_else(|| "(null)".into());
    format!("{ip} ({city}, {country})")
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(mut stream) = TcpStream::connect("ipinfo.io:80") else {
        // "getaddrinfo() failed" / "connect() failed" are error rows,
        // hidden by default.
        return Vec::new();
    };
    if stream.write_all(REQUEST.as_bytes()).is_err() {
        return Vec::new();
    }

    let mut response = Vec::new();
    if stream.take((MAX_RESPONSE_SIZE + 1) as u64).read_to_end(&mut response).is_err()
        || response.len() > MAX_RESPONSE_SIZE
    {
        // "Response too large" is an error row, hidden by default.
        return Vec::new();
    }
    let response = String::from_utf8_lossy(&response);
    let body = response_body(&response);
    if body.is_empty() {
        // "Empty server response received" is an error row, hidden by
        // default.
        return Vec::new();
    }

    // Only a JSON object reply is interpreted; anything else (custom
    // servers, header-only responses) prints verbatim, right-trimmed.
    let value = if body.starts_with('{') {
        parsed_value(body)
    } else {
        body.trim_end().to_string()
    };
    vec![Row::new("Public IP", value)]
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
        assert_eq!(super::parsed_value(body), "1.2.3.4 (Łódź, PL)");
        let body = r#"{"ip": "1.2.3.4"}"#;
        assert_eq!(super::parsed_value(body), "1.2.3.4 ((null), (null))");
        let body = r#"{"ip": "1.2.3.4", "city": null, "country": "PL"}"#;
        assert_eq!(super::parsed_value(body), "1.2.3.4 ((null), PL)");
    }

    #[test]
    fn decodes_escapes() {
        assert_eq!(
            super::json_string_field(r#"{"city": "a\"b\\c"}"#, "city"),
            Some(r#"a"b\c"#.to_string())
        );
    }
}

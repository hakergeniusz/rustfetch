//! `weather` module: the current weather from wttr.in, ported from
//! detection/weather/weather.c over the raw-socket HTTP client of
//! common/impl/networking_linux.c (the same blocking HTTP/1.0 pattern as
//! the publicip module; timeout=0 is the upstream default). The default
//! format string is "%t+-+%C+(%l)" with no location and the default
//! temperature unit, so no &m/&u suffix is appended; the reply body prints
//! verbatim, right-trimmed.

use std::io::{Read, Write};
use std::net::TcpStream;

use crate::{instance::Instance, modules::Row};

const REQUEST: &str = "GET /?format=%t+-+%C+(%l) HTTP/1.0\r\nHost: wttr.in\r\nConnection: close\r\nUser-Agent: curl/0.0.0\r\n\r\n";
const MAX_RESPONSE_SIZE: usize = 1024 * 1024;

/// The http response body after the "\r\n\r\n" header separator; when the
/// separator is missing, ffStrbufSubstrAfterFirstS leaves the buffer
/// untouched, so the whole response (headers included) is the body.
fn response_body(response: &str) -> &str {
    match response.find("\r\n\r\n") {
        Some(index) => &response[index + 4..],
        None => response,
    }
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(mut stream) = TcpStream::connect("wttr.in:80") else {
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
    // ffStrbufTrimRightSpace.
    let body = response_body(&response).trim_end();
    if body.is_empty() {
        // "Empty server response received" is an error row, hidden by
        // default.
        return Vec::new();
    }

    vec![Row::new("Weather", body.to_string())]
}

#[cfg(test)]
mod tests {
    #[test]
    fn splits_headers_and_bodies() {
        assert_eq!(super::response_body("HTTP/1.0 200 OK\r\n\r\n+12°C"), "+12°C");
        // Missing separator keeps everything.
        assert_eq!(super::response_body("HTTP/1.0 200 OK"), "HTTP/1.0 200 OK");
    }

    #[test]
    fn request_matches_the_upstream_bytes() {
        // initNetworkingState: "GET " + path + " HTTP/1.0\r\nHost: " + host
        // + "\r\nConnection: close\r\n" + headers + "\r\n"; weather's path
        // is "/" + "?format=" + outputFormat and its header is the curl
        // User-Agent.
        assert_eq!(
            super::REQUEST,
            "GET /?format=%t+-+%C+(%l) HTTP/1.0\r\nHost: wttr.in\r\nConnection: close\r\nUser-Agent: curl/0.0.0\r\n\r\n"
        );
    }
}

//! `dns` module: nameservers from resolv.conf, ported from dns_linux.c. The
//! system file first; a lone 127.0.0.53 or 127.0.0.1 entry is replaced by the
//! systemd-resolved / NetworkManager files, and empty results fall back to
//! the resolvconf, dnsmasq and openresolv locations. IPv4 addresses print
//! before IPv6 ones.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

/// The nameserver entries of one resolv.conf file; None when it can't be
/// read (which lets the caller fall through to the next candidate).
fn from_conf(path: &str) -> Option<Vec<String>> {
    let content = util::read_file(path)?;
    Some(parse_resolv_content(&content))
}

fn parse_resolv_content(content: &str) -> Vec<String> {
    let mut servers = Vec::new();
    for line in content.lines() {
        let Some(rest) = line.strip_prefix("nameserver") else {
            continue;
        };
        let rest = rest.trim_start_matches([' ', '\t']);
        if rest.is_empty() {
            continue;
        }
        let server = rest.split('#').next().unwrap_or(rest).trim_end();
        servers.push(server.to_string());
    }
    servers
}

fn detect() -> Result<Vec<String>, &'static str> {
    let mut servers = from_conf("/etc/resolv.conf").ok_or("fopen(path, r) failed")?;

    // Managed by systemd-resolved or NetworkManager: their own resolv.conf
    // lists the real servers.
    if servers.len() == 1 {
        match servers[0].as_str() {
            "127.0.0.53" => {
                if let Some(real) = from_conf("/run/systemd/resolve/resolv.conf") {
                    servers = real;
                }
            },
            "127.0.0.1" => {
                if let Some(real) = from_conf("/var/run/NetworkManager/resolv.conf") {
                    servers = real;
                }
            },
            _ => {},
        }
    }

    if servers.is_empty() {
        for path in [
            "/run/resolvconf/resolv.conf",
            "/var/run/dnsmasq/resolv.conf",
            "/etc/resolv.conf.openresolv",
        ] {
            if let Some(fallback) = from_conf(path)
                && !fallback.is_empty()
            {
                return Ok(fallback);
            }
        }
    }

    Ok(servers)
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(servers) = detect() else {
        // ffPrintError is gated behind --show-errors, off by default.
        return Vec::new();
    };
    if servers.is_empty() {
        return Vec::new();
    }

    // IPv4 addresses first, then IPv6.
    let mut value = Vec::new();
    for server in &servers {
        if server.contains('.') {
            value.push(server.clone());
        }
    }
    for server in &servers {
        if server.contains(':') {
            value.push(server.clone());
        }
    }
    vec![Row::new(instance.display_name("dns"), value.join(" "))]
}



/// ffGenerateDNSJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(servers) => {
            let mut arr = Json::Array(Vec::new());
            for server in servers {
                arr.push_value(Json::Str(server));
            }
            JsonOutcome::Result(arr)
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_nameserver_lines() {
        let conf = "\
# comment
nameserver 127.0.0.53
nameserver\t1.1.1.1 # cloudflare
nameserver 2606:4700:4700::1111
search lan
nameserver
";
        assert_eq!(
            super::parse_resolv_content(conf),
            ["127.0.0.53", "1.1.1.1", "2606:4700:4700::1111"]
        );
    }
}

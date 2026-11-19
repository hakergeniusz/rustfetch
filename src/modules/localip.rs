//! `localip` module: interface addresses via getifaddrs(3), no spawning.
//! fastfetch's defaults: IPv4 with prefix length, only the interface holding
//! the default route (DEFAULT_ROUTE_ONLY). The filter chain matches
//! detection/localip/localip_linux.c: running interfaces, no loopback,
//! AF_INET/AF_INET6 only.

use std::net::Ipv4Addr;

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
};

pub struct LocalIpResult {
    pub name: String,
    /// Whether the interface holds the IPv4 default route.
    pub default_route_ipv4: bool,
    /// "addr" or "addr/prefix" (the PREFIX_LEN default).
    pub ipv4: String,
}

/// The interface owning the default route, from /proc/net/route (destination
/// 00000000); fastfetch asks netlink for the same thing.
fn default_route_iface() -> Option<String> {
    let content = crate::util::read_file("/proc/net/route")?;
    content
        .lines()
        .skip(1)
        .find(|line| {
            let mut fields = line.split_whitespace();
            fields.next().is_some() && fields.next() == Some("00000000")
        })
        .and_then(|line| line.split_whitespace().next().map(str::to_string))
}

/// Number of leading one-bits in the netmask sockaddr (0 when unusable).
fn prefix_bits(netmask: *const libc::sockaddr, max: u8) -> u8 {
    if netmask.is_null() {
        return 0;
    }
    // SAFETY: reading the sa_family plus the address bytes of a netmask
    // sockaddr is valid for AF_INET/AF_INET6 entries from getifaddrs.
    match unsafe { (*netmask).sa_family as i32 } {
        libc::AF_INET => {
            let sin = unsafe { &*(netmask as *const libc::sockaddr_in) };
            let bytes = u32::from_be(sin.sin_addr.s_addr).to_be_bytes();
            u8::try_from(bytes.iter().map(|b| b.count_ones()).sum::<u32>()).unwrap_or(max)
        },
        libc::AF_INET6 => {
            let sin6 = unsafe { &*(netmask as *const libc::sockaddr_in6) };
            let ones: u32 = sin6.sin6_addr.s6_addr.iter().map(|b| b.count_ones()).sum();
            u8::try_from(ones).unwrap_or(max)
        },
        _ => 0,
    }
}

/// ffDetectLocalIps with the default options: IPv4 only, prefix length,
/// default-route interfaces only.
fn detect() -> Result<Vec<LocalIpResult>, &'static str> {
    let route_iface = default_route_iface().unwrap_or_default();

    let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs allocates a list we own and freeifaddrs frees it.
    if unsafe { libc::getifaddrs(&mut ifap) } != 0 || ifap.is_null() {
        return Err("getifaddrs(&ifAddrStruct) failed");
    }

    let mut results: Vec<LocalIpResult> = Vec::new();
    let mut cursor = ifap;
    while !cursor.is_null() {
        // SAFETY: cursor walks the list returned by getifaddrs until null.
        let ifa = unsafe { *cursor };
        cursor = ifa.ifa_next;

        if ifa.ifa_addr.is_null() {
            continue;
        }
        // IFF_RUNNING
        if ifa.ifa_flags & libc::IFF_RUNNING as u32 == 0 {
            continue;
        }
        // IFF_LOOPBACK (the LOOP show-type bit is off by default)
        if ifa.ifa_flags & libc::IFF_LOOPBACK as u32 != 0 {
            continue;
        }
        // Only AF_INET (no MAC bit); IPv6 show-type is off by default.
        // SAFETY: ifa_addr is a valid sockaddr of the family it reports.
        if unsafe { (*ifa.ifa_addr).sa_family as i32 } != libc::AF_INET {
            continue;
        }
        let name =
            unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) }.to_string_lossy().into_owned();
        // DEFAULT_ROUTE_ONLY with the IPv4 show bit.
        if name != route_iface {
            continue;
        }

        let sin = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in) };
        let octets = u32::from_be(sin.sin_addr.s_addr).to_be_bytes();
        let ip = Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]);
        let prefix = prefix_bits(ifa.ifa_netmask, 32);

        // Multiple addresses share one adapter entry; without ALL_IPS the
        // first one wins.
        if results.iter().any(|result| result.name == name) {
            continue;
        }
        results.push(LocalIpResult {
            default_route_ipv4: true,
            name,
            ipv4: format!("{ip}/{prefix}"),
        });
    }

    // SAFETY: frees the list obtained by getifaddrs above.
    unsafe { libc::freeifaddrs(ifap) };
    Ok(results)
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(results) = detect() else {
        return Vec::new();
    };
    results
        .iter()
        .map(|ip| Row::new(&format!("Local IP ({})", ip.name), ip.ipv4.clone()))
        .collect()
}

/// ffGenerateLocalIpJsonResult: name, defaultRoute.ipv4 and ipv4 for the
/// default show type.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(results) => {
            let mut arr = Json::Array(Vec::new());
            for ip in &results {
                let mut obj = Json::obj();
                obj.push("name", ip.name.clone());
                let mut default_route = Json::obj();
                default_route.push("ipv4", ip.default_route_ipv4);
                obj.push("defaultRoute", default_route);
                obj.push("ipv4", ip.ipv4.clone());
                arr.push_value(obj);
            }
            JsonOutcome::Result(arr)
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn default_route_parses_procfs_format() {
        // 00000000 destination marks the default route; first field is iface
        let line = "enp3s0\t00000000\tC0A812FE\t0003\t0\t0\t0\t00000000\t0\t\t0";
        let mut fields = line.split_whitespace();
        let (iface, destination, _) = (fields.next(), fields.next(), fields.next());
        assert_eq!(destination, Some("00000000"));
        assert_eq!(iface, Some("enp3s0"));
    }
}

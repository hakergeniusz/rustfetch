//! `localip` module: interface addresses via getifaddrs(3), no spawning.
//! Keys follow fastfetch: "Local IP (iface)" for IPv4 and
//! "Local IP (v6 iface)" for IPv6.

use std::net::Ipv4Addr;

use crate::{instance::Instance, modules::Row};

/// Virtual plumbing interfaces that are not real networks.
const SKIPPED: &[&str] = &[
    "docker", "br-", "veth", "virbr", "vboxnet", "wg", "tun", "tap",
];

fn is_skipped(name: &str) -> bool {
    name == "lo" || SKIPPED.iter().any(|prefix| name.starts_with(prefix))
}

/// Number of leading one-bits in the netmask sockaddr (0 when unusable).
fn prefix_bits(netmask: *const libc::sockaddr, max: u8) -> u8 {
    if netmask.is_null() {
        return 0;
    }
    // SAFETY: reading the sa_family plus the address bytes of a netmask
    // returned by getifaddrs is safe for the lifetime of the list.
    let family = unsafe { (*netmask).sa_family as i32 };
    match family {
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

pub fn run(_instance: &Instance) -> Vec<Row> {
    let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs allocates a list we own and freeifaddrs frees it.
    if unsafe { libc::getifaddrs(&mut ifap) } != 0 || ifap.is_null() {
        return Vec::new();
    }

    let mut rows = Vec::new();
    let mut cursor = ifap;
    while !cursor.is_null() {
        // SAFETY: cursor walks the list returned by getifaddrs until null.
        let ifa = unsafe { *cursor };
        cursor = ifa.ifa_next;

        let name = unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) }.to_string_lossy().into_owned();
        if is_skipped(&name) || ifa.ifa_addr.is_null() {
            continue;
        }

        // SAFETY: ifa_addr is a valid sockaddr of the family it reports.
        let family = unsafe { (*ifa.ifa_addr).sa_family as i32 };
        if family == libc::AF_INET {
            let sin = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in) };
            let octets = u32::from_be(sin.sin_addr.s_addr).to_be_bytes();
            let ip = Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]);
            if ip.is_link_local() {
                continue;
            }
            let prefix = prefix_bits(ifa.ifa_netmask, 32);
            rows.push(Row::new(&format!("Local IP ({name})"), format!("{ip}/{prefix}")));
        } else if family == libc::AF_INET6 {
            let sin6 = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in6) };
            let ip = std::net::Ipv6Addr::from(sin6.sin6_addr.s6_addr);
            if ip.is_unicast_link_local() || ip.is_loopback() || ip.is_unspecified() {
                continue;
            }
            let prefix = prefix_bits(ifa.ifa_netmask, 128);
            rows.push(Row::new(&format!("Local IP (v6 {name})"), format!("{ip}/{prefix}")));
        }
    }

    // SAFETY: frees the list obtained by getifaddrs above.
    unsafe { libc::freeifaddrs(ifap) };
    rows
}

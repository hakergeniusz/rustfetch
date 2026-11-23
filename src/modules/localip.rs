//! `localip` module: interface addresses via getifaddrs(3), no spawning.
//! Ported from detection/localip/localip_linux.c with the showType bit
//! gating of modules/localip/localip.c: IPv4 with prefix length and
//! default-route-only are the defaults; MAC/MTU/speed/flags/loop/IPv6/allIps
//! arrive through the JSON config.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::OnceLock;

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
};

/// Call `ioctl(2)` with a request cast to the type the target expects.
///
/// glibc declares the request as `c_ulong`, while bionic (Android) declares it
/// as `c_int`; `as _` lets the compiler pick the right type on each target.
#[inline]
unsafe fn portable_ioctl<T>(fd: libc::c_int, request: libc::c_ulong, arg: *mut T) -> libc::c_int {
    // SAFETY: the caller guarantees `fd` and `arg` are valid for `request`.
    unsafe { libc::ioctl(fd, request as _, arg) }
}

/// FFLocalIpType bits.
mod show {
    pub const LOOP: u16 = 1 << 0;
    pub const IPV4: u16 = 1 << 1;
    pub const IPV6: u16 = 1 << 2;
    pub const MAC: u16 = 1 << 3;
    pub const PREFIX_LEN: u16 = 1 << 4;
    pub const MTU: u16 = 1 << 5;
    pub const SPEED: u16 = 1 << 6;
    pub const FLAGS: u16 = 1 << 7;
    pub const COMPACT: u16 = 1 << 10;
    pub const DEFAULT_ROUTE_ONLY: u16 = 1 << 11;
    pub const ALL_IPS: u16 = 1 << 12;
}

/// FFLocalIpIpv6Type bits (Linux: no PREFERRED without SIOCGIFAFLAG_IN6).
mod v6 {
    pub const GUA: u8 = 0b0000_0001;
    pub const ULA: u8 = 0b0000_0010;
    pub const LLA: u8 = 0b0000_0100;
    pub const UNKNOWN: u8 = 0b0000_1000;
    pub const SECONDARY: u8 = 0b0100_0000;
    pub const AUTO: u8 = 0b1111_1111;
}

/// Module options, ported from ffInitLocalIpOptions/ffParseLocalIpJsonObject.
pub(crate) struct LocalIpOptions {
    show_type: u16,
    /// FFLocalIpIpv6Type: v6::AUTO for "auto", else the type bits to show.
    ipv6_type: u8,
    name_prefix: String,
}

impl Default for LocalIpOptions {
    fn default() -> Self {
        Self {
            show_type: show::IPV4 | show::PREFIX_LEN | show::DEFAULT_ROUTE_ONLY,
            ipv6_type: v6::AUTO,
            name_prefix: String::new(),
        }
    }
}

impl LocalIpOptions {
    fn from_instance(instance: &Instance) -> Self {
        let Some(entry) = instance.entry else {
            return Self::default();
        };
        let mut options = Self::default();
        // Absent keys leave the default untouched, like upstream's
        // per-key `if (yyjson_get_bool(val))` acting only on present keys.
        let mut bit = |key: &str, bit: u16| {
            let Some(value) = entry.get(key).and_then(crate::jsonc::Value::as_bool) else {
                return;
            };
            if value {
                options.show_type |= bit;
            } else {
                options.show_type &= !bit;
            }
        };
        bit("showIpv4", show::IPV4);
        bit("showMac", show::MAC);
        bit("showLoop", show::LOOP);
        bit("showPrefixLen", show::PREFIX_LEN);
        bit("showMtu", show::MTU);
        bit("showSpeed", show::SPEED);
        bit("showFlags", show::FLAGS);
        bit("compact", show::COMPACT);
        bit("defaultRouteOnly", show::DEFAULT_ROUTE_ONLY);
        bit("showAllIps", show::ALL_IPS);

        // showIpv6 doubles as the ipv6Type setter: a bool toggles the bit
        // with type AUTO, a string selects the type and enables the bit.
        if let Some(enabled) = entry.get("showIpv6").and_then(crate::jsonc::Value::as_bool) {
            options.ipv6_type = v6::AUTO;
            if enabled {
                options.show_type |= show::IPV6;
            } else {
                options.show_type &= !show::IPV6;
            }
        } else if let Some(kind) = entry.get("showIpv6").and_then(crate::jsonc::Value::as_str) {
            options.ipv6_type = match kind {
                "gua" => v6::GUA,
                "ula" => v6::ULA,
                "lla" => v6::LLA,
                "unknown" => v6::UNKNOWN,
                _ => v6::AUTO,
            };
            options.show_type |= show::IPV6;
        }

        if let Some(prefix) = entry.get("namePrefix").and_then(crate::jsonc::Value::as_str) {
            options.name_prefix = prefix.to_string();
        }
        options
    }
}

pub struct LocalIpResult {
    pub name: String,
    pub ipv4: String,
    pub ipv6: String,
    pub mac: String,
    pub flags: String,
    /// -1 like upstream when unset; MTU in bytes, speed in Mbps.
    pub mtu: i32,
    pub speed: i32,
    /// FFLocalIpType bits for the route families the interface carries.
    pub default_route: u16,
}

/// The interface owning the IPv4 default route, from /proc/net/route
/// (destination 00000000); fastfetch asks netlink for the same thing.
fn default_route_iface_v4() -> Option<String> {
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

/// The interface owning the IPv6 default route, from /proc/net/ipv6_route
/// (all-zero destination with prefix 00; the device is the last field).
/// Upstream asks netlink and picks by metric; the procfs table's ::/0 rows
/// carry the metric in the 4th field, so the lowest wins.
fn default_route_iface_v6() -> Option<String> {
    let content = crate::util::read_file("/proc/net/ipv6_route")?;
    let mut best: Option<(u32, String)> = None;
    for line in content.lines() {
        let mut fields = line.split_whitespace();
        let dest = fields.next()?;
        let prefix = fields.next()?;
        if dest != "00000000000000000000000000000000" || prefix != "00" {
            continue;
        }
        let metric = fields.next().and_then(|m| u32::from_str_radix(m, 16).ok()).unwrap_or(u32::MAX);
        let device = fields.nth(5)?; // field 9 of 10
        if best.as_ref().is_none_or(|(best_metric, _)| metric < *best_metric) {
            best = Some((metric, device.to_string()));
        }
    }
    best.map(|(_, device)| device)
}

/// The GUA/ULA addresses of /proc/net/if_inet6 whose kernel flags don't
/// mark them secondary (deprecated/temporary/tentative/dadfailed/optimistic).
fn stable_ipv6_addresses() -> &'static Vec<[u8; 16]> {
    static ADDRESSES: OnceLock<Vec<[u8; 16]>> = OnceLock::new();
    ADDRESSES.get_or_init(|| {
        let Some(content) = crate::util::read_file("/proc/net/if_inet6") else {
            return Vec::new();
        };
        let mut addresses = Vec::new();
        for line in content.lines() {
            let mut fields = line.split_whitespace();
            let Some(hex) = fields.next() else { continue };
            if hex.len() != 32 {
                continue;
            }
            let Ok(addr) = u128::from_str_radix(hex, 16) else { continue };
            let bytes = addr.to_be_bytes();
            // Only global-unicast and unique-local entries are tracked.
            if !is_global(&bytes) && !is_unique_local(&bytes) {
                continue;
            }
            let Some(flags) = fields.nth(3).and_then(|f| u32::from_str_radix(f, 16).ok()) else {
                continue;
            };
            // IFA_F_DEPRECATED|TEMPORARY|TENTATIVE|DADFAILED|OPTIMISTIC
            const SECONDARY_FLAGS: u32 = 0x10 | 0x40 | 0x20 | 0x8 | 0x100;
            if flags & SECONDARY_FLAGS == 0 {
                addresses.push(bytes);
            }
        }
        addresses
    })
}

/// IN6_IS_ADDR_GLOBAL: 2000::/3.
fn is_global(bytes: &[u8; 16]) -> bool {
    bytes[0] & 0xE0 == 0x20
}

/// IN6_IS_ADDR_UNIQUE_LOCAL: fc00::/7.
fn is_unique_local(bytes: &[u8; 16]) -> bool {
    bytes[0] & 0xFE == 0xFC
}

/// IN6_IS_ADDR_LINKLOCAL: fe80::/10.
fn is_link_local(bytes: &[u8; 16]) -> bool {
    bytes[0] == 0xFE && bytes[1] & 0xC0 == 0x80
}

/// getIpv6Type (Linux branch): the address classification plus the
/// SECONDARY bit when the address is absent from the stable if_inet6 set.
fn get_ipv6_type(addr: &[u8; 16]) -> u8 {
    let kind = if is_global(addr) {
        v6::GUA
    } else if is_unique_local(addr) {
        v6::ULA
    } else if is_link_local(addr) {
        v6::LLA
    } else {
        // Unknown types return early: they never get the secondary bit.
        return v6::UNKNOWN;
    };
    // The if_inet6 list only tracks global/unique-local addresses, so every
    // other address (link-local included) counts as secondary.
    if !stable_ipv6_addresses().contains(addr) {
        return kind | v6::SECONDARY;
    }
    kind
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

/// appendIpv4: "addr" or "addr/prefix", comma-joined for multiple entries.
fn append_ipv4(
    show_type: u16,
    buffer: &mut String,
    addr: *const libc::sockaddr,
    netmask: *const libc::sockaddr,
) {
    if addr.is_null() {
        return;
    }
    // SAFETY: ifa_addr is an AF_INET sockaddr for this entry.
    let sin = unsafe { &*(addr as *const libc::sockaddr_in) };
    let octets = u32::from_be(sin.sin_addr.s_addr).to_be_bytes();
    let mut text = Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]).to_string();
    if show_type & show::PREFIX_LEN != 0 {
        let prefix = prefix_bits(netmask, 32);
        if prefix != 0 {
            text.push_str(&format!("/{prefix}"));
        }
    }
    if !buffer.is_empty() {
        buffer.push(',');
    }
    buffer.push_str(&text);
}

/// appendIpv6: compressed inet_ntop form, optional /prefix.
fn append_ipv6(
    show_type: u16,
    buffer: &mut String,
    addr: *const libc::sockaddr,
    netmask: *const libc::sockaddr,
) {
    if addr.is_null() {
        return;
    }
    // SAFETY: ifa_addr is an AF_INET6 sockaddr for this entry.
    let sin6 = unsafe { &*(addr as *const libc::sockaddr_in6) };
    let mut text = Ipv6Addr::from(sin6.sin6_addr.s6_addr).to_string();
    if show_type & show::PREFIX_LEN != 0 {
        let prefix = prefix_bits(netmask, 128);
        if prefix != 0 {
            text.push_str(&format!("/{prefix}"));
        }
    }
    if !buffer.is_empty() {
        buffer.push(',');
    }
    buffer.push_str(&text);
}

/// The ifa_name field as an owned string.
fn ifa_name_of(entry: &libc::ifaddrs) -> String {
    // SAFETY: ifa_name points into the getifaddrs list's NUL-terminated name.
    unsafe { std::ffi::CStr::from_ptr(entry.ifa_name) }
        .to_string_lossy()
        .into_owned()
}

/// The NI flag names in upstream's table order, matching IFF_* values.
const NI_FLAGS: &[(u32, &str)] = &[
    (libc::IFF_UP as u32, "UP"),
    (libc::IFF_BROADCAST as u32, "BROADCAST"),
    (libc::IFF_DEBUG as u32, "DEBUG"),
    (libc::IFF_LOOPBACK as u32, "LOOPBACK"),
    (libc::IFF_POINTOPOINT as u32, "POINTOPOINT"),
    (libc::IFF_RUNNING as u32, "RUNNING"),
    (libc::IFF_NOARP as u32, "NOARP"),
    (libc::IFF_PROMISC as u32, "PROMISC"),
    (libc::IFF_ALLMULTI as u32, "ALLMULTI"),
    (libc::IFF_MULTICAST as u32, "MULTICAST"),
    // Linux-only interface flag bits; BSD libc does not define these.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    (libc::IFF_NOTRAILERS as u32, "NOTRAILERS"),
    #[cfg(any(target_os = "linux", target_os = "android"))]
    (libc::IFF_MASTER as u32, "MASTER"),
    #[cfg(any(target_os = "linux", target_os = "android"))]
    (libc::IFF_SLAVE as u32, "SLAVE"),
    #[cfg(any(target_os = "linux", target_os = "android"))]
    (libc::IFF_PORTSEL as u32, "PORTSEL"),
    #[cfg(any(target_os = "linux", target_os = "android"))]
    (libc::IFF_AUTOMEDIA as u32, "AUTOMEDIA"),
    #[cfg(any(target_os = "linux", target_os = "android"))]
    (libc::IFF_DYNAMIC as u32, "DYNAMIC"),
    (0x10000, "LOWER_UP"),
    (0x20000, "DORMANT"),
    (0x40000, "ECHO"),
];

/// ffLocalIpFillNIFlags: comma-joined names of the set flags.
fn fill_ni_flags(flag: u32) -> String {
    let mut buffer = String::new();
    let mut flag = flag;
    for &(bit, name) in NI_FLAGS {
        if flag == 0 {
            break;
        }
        if flag & bit != 0 {
            if !buffer.is_empty() {
                buffer.push(',');
            }
            buffer.push_str(name);
            flag &= !bit;
        }
    }
    buffer
}

/// ffDetectLocalIps with the showType gating of the C port.
fn detect(options: &LocalIpOptions) -> Result<Vec<LocalIpResult>, &'static str> {
    let show = options.show_type;
    let route_v4 = if show & show::IPV4 != 0 { default_route_iface_v4().unwrap_or_default() } else { String::new() };
    let route_v6 = if show & show::IPV6 != 0 { default_route_iface_v6().unwrap_or_default() } else { String::new() };

    let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs allocates a list we own and freeifaddrs frees it.
    if unsafe { libc::getifaddrs(&mut ifap) } != 0 || ifap.is_null() {
        return Err("getifaddrs(&ifAddrStruct) failed");
    }

    struct Adapter {
        /// The first (or AF_PACKET) ifaddrs entry: name, flags and MAC source.
        mac: libc::ifaddrs,
        ipv4: Vec<*mut libc::ifaddrs>,
        ipv6: Vec<*mut libc::ifaddrs>,
    }

    let mut adapters: Vec<Adapter> = Vec::new();
    let mut cursor = ifap;
    while !cursor.is_null() {
        // SAFETY: cursor walks the list returned by getifaddrs until null.
        let node = cursor;
        let ifa: libc::ifaddrs = unsafe { *node };
        cursor = ifa.ifa_next;

        if ifa.ifa_addr.is_null() {
            continue;
        }
        if ifa.ifa_flags & libc::IFF_RUNNING as u32 == 0 {
            continue;
        }
        if ifa.ifa_flags & libc::IFF_LOOPBACK as u32 != 0 && show & show::LOOP == 0 {
            continue;
        }
        if !options.name_prefix.is_empty() && !ifa_name_of(&ifa).starts_with(&options.name_prefix) {
            continue;
        }
        // SAFETY: ifa_addr is a valid sockaddr of the family it reports.
        let family = unsafe { (*ifa.ifa_addr).sa_family as i32 };
        if show & show::MAC == 0 && family != libc::AF_INET && family != libc::AF_INET6 {
            continue;
        }
        if show & show::DEFAULT_ROUTE_ONLY != 0 {
            let name = ifa_name_of(&ifa);
            let is_v4_default = show & show::IPV4 != 0 && route_v4 == name;
            let is_v6_default = show & show::IPV6 != 0 && route_v6 == name;
            if !is_v4_default && !is_v6_default {
                continue;
            }
        }

        let name = ifa_name_of(&ifa);
        let adapter = match adapters
            .iter_mut()
            .find(|a| ifa_name_of(&a.mac) == name)
        {
            Some(adapter) => adapter,
            None => {
                adapters.push(Adapter {
                    // SAFETY: ifa copies the entry's fields; ifa_name stays a
                    // pointer into the list freed after detection.
                    // Shallow-copy the whole libc entry: BSD ifaddrs has no
                    // `ifa_ifu` union field (it uses `ifa_dstaddr`). Only
                    // name/flags/addr are read from this copy, and ifa_next
                    // is cleared so it is never walked as a list.
                    mac: {
                        let mut copy = ifa;
                        copy.ifa_next = std::ptr::null_mut();
                        copy
                    },
                    ipv4: Vec::new(),
                    ipv6: Vec::new(),
                });
                let pushed = adapters.len() - 1;
                &mut adapters[pushed]
            },
        };

        match family {
            // The node pointer stays valid until freeifaddrs below.
            libc::AF_INET if show & show::IPV4 != 0 => adapter.ipv4.push(node),
            libc::AF_INET6 if show & show::IPV6 != 0 => adapter.ipv6.push(node),
            // MAC addresses come from AF_PACKET's sockaddr_ll on Linux only.
            #[cfg(any(target_os = "linux", target_os = "android"))]
            libc::AF_PACKET => adapter.mac = ifa,
            _ => {},
        }
    }

    // SAFETY: the adapters hold raw entries into the list; both are freed
    // together below.
    let mut results: Vec<LocalIpResult> = Vec::new();
    for adapter in &adapters {
        if adapter.ipv4.is_empty() && adapter.ipv6.is_empty() && show & show::MAC == 0 {
            continue;
        }

        let mut item = LocalIpResult {
            name: ifa_name_of(&adapter.mac),
            ipv4: String::new(),
            ipv6: String::new(),
            mac: String::new(),
            flags: String::new(),
            mtu: -1,
            speed: -1,
            default_route: 0,
        };

        if show & show::FLAGS != 0 {
            item.flags = fill_ni_flags(adapter.mac.ifa_flags);
        }

        if show & show::IPV4 != 0 {
            let is_default_route = route_v4 == ifa_name_of(&adapter.mac);
            if is_default_route {
                item.default_route |= show::IPV4;
            }
            // DEFAULT_ROUTE_ONLY skips this family's addresses but the
            // remaining sections (v6, MAC) still run, like upstream's goto.
            if !(show & show::DEFAULT_ROUTE_ONLY != 0 && !is_default_route) {
                if show & show::ALL_IPS != 0 {
                    for entry in &adapter.ipv4 {
                        // SAFETY: entries point into the getifaddrs list.
                        let ifa = unsafe { **entry };
                        append_ipv4(show, &mut item.ipv4, ifa.ifa_addr, ifa.ifa_netmask);
                    }
                } else if let Some(first) = adapter.ipv4.first() {
                    // Upstream prefers the route's source address;
                    // /proc/net/route has none, so the first address wins.
                    let ifa = unsafe { **first };
                    append_ipv4(show, &mut item.ipv4, ifa.ifa_addr, ifa.ifa_netmask);
                }
            }
        }

        if show & show::IPV6 != 0 {
            let is_default_route = route_v6 == ifa_name_of(&adapter.mac);
            if is_default_route {
                item.default_route |= show::IPV6;
            }
            let collect_v6 = show & show::DEFAULT_ROUTE_ONLY == 0 || is_default_route;
            if collect_v6 && options.ipv6_type == v6::AUTO {
                if show & show::ALL_IPS != 0 {
                    for entry in &adapter.ipv6 {
                        let ifa = unsafe { **entry };
                        append_ipv6(show, &mut item.ipv6, ifa.ifa_addr, ifa.ifa_netmask);
                    }
                } else {
                    // AUTO: preferred (not tracked on Linux), else the first
                    // global-unicast non-secondary, else the first unique-local
                    // non-secondary, else the first address.
                    let types: Vec<u8> = adapter
                        .ipv6
                        .iter()
                        .map(|entry| {
                            let ifa = unsafe { **entry };
                            // SAFETY: ifa_addr is an AF_INET6 sockaddr here.
                            let sin6 = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in6) };
                            get_ipv6_type(&sin6.sin6_addr.s6_addr)
                        })
                        .collect();
                    let mut selected = None;
                    for (index, kind) in types.iter().enumerate() {
                        if kind & v6::SECONDARY == 0 && kind & v6::GUA != 0 {
                            selected = Some(index);
                            break;
                        }
                        if kind & v6::SECONDARY == 0 && kind & v6::ULA != 0 && selected.is_none() {
                            selected = Some(index);
                        }
                    }
                    if selected.is_none() && !adapter.ipv6.is_empty() {
                        selected = Some(0);
                    }
                    if let Some(index) = selected {
                        let ifa = unsafe { *adapter.ipv6[index] };
                        append_ipv6(show, &mut item.ipv6, ifa.ifa_addr, ifa.ifa_netmask);
                    }
                }
            } else if collect_v6 {
                for entry in &adapter.ipv6 {
                    let ifa = unsafe { **entry };
                    let sin6 = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in6) };
                    let kind = get_ipv6_type(&sin6.sin6_addr.s6_addr);
                    if kind & options.ipv6_type == 0 {
                        continue;
                    }
                    if show & show::ALL_IPS != 0 || kind & v6::SECONDARY == 0 {
                        append_ipv6(show, &mut item.ipv6, ifa.ifa_addr, ifa.ifa_netmask);
                        if show & show::ALL_IPS == 0 {
                            break;
                        }
                    }
                }
            }
        }

        #[cfg(any(target_os = "linux", target_os = "android"))]
        if show & show::MAC != 0 && !adapter.mac.ifa_addr.is_null() {
            // SAFETY: upstream reinterprets the entry's ifa_addr as a
            // sockaddr_ll without checking the family, so an AF_INET entry's
            // zero padding prints as 00:00:00:00:00:00 (tun devices).
            let sll = unsafe { &*(adapter.mac.ifa_addr as *const libc::sockaddr_ll) };
            item.mac = format!(
                "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                sll.sll_addr[0],
                sll.sll_addr[1],
                sll.sll_addr[2],
                sll.sll_addr[3],
                sll.sll_addr[4],
                sll.sll_addr[5]
            );
        }

        results.push(item);
    }

    // MTU and speed via the same socket ioctls upstream uses.
    if show & (show::MTU | show::SPEED) != 0 {
        // SAFETY: socket() returns an fd or -1; close() owns it.
        let sockfd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) };
        if sockfd > 0 {
            for iface in &mut results {
                let mut ifr: libc::ifreq = unsafe { std::mem::zeroed() };
                let bytes = iface.name.as_bytes();
                if bytes.len() < ifr.ifr_name.len() {
                    // SAFETY: ifr_name is IFNAMSIZ bytes of writable storage.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            bytes.as_ptr(),
                            ifr.ifr_name.as_mut_ptr().cast(),
                            bytes.len(),
                        );
                    }
                }

                #[repr(C)]
                struct EthtoolCmd {
                    cmd: u32,
                    supported: u32,
                    advertising: u32,
                    speed: u16,
                    duplex: u8,
                    port: u8,
                    phy_address: u8,
                    transceiver: u8,
                    autoneg: u8,
                    mdio_support: u8,
                    maxtxpkt: u32,
                    maxrxpkt: u32,
                    speed_hi: u16,
                    lp_advertising: u32,
                }
                const ETHTOOL_GSET: u32 = 0x00000001;
                const SIOCETHTOOL: u64 = 0x8946;

                // SIOCGIFMTU and ifr_ifru.ifru_mtu are Linux-only libc items.
                #[cfg(any(target_os = "linux", target_os = "android"))]
                if show & show::MTU != 0 {
                    // SAFETY: ifreq ioctl with an owned buffer.
                    if unsafe { portable_ioctl(sockfd, libc::SIOCGIFMTU, &mut ifr as *mut _) } == 0 {
                        iface.mtu = unsafe { ifr.ifr_ifru.ifru_mtu };
                    }
                }
                if show & show::SPEED != 0 {
                    let mut edata = EthtoolCmd {
                        cmd: ETHTOOL_GSET,
                        supported: 0,
                        advertising: 0,
                        speed: 0,
                        duplex: 0,
                        port: 0,
                        phy_address: 0,
                        transceiver: 0,
                        autoneg: 0,
                        mdio_support: 0,
                        maxtxpkt: 0,
                        maxrxpkt: 0,
                        speed_hi: 0,
                        lp_advertising: 0,
                    };
                    ifr.ifr_ifru.ifru_data = (&mut edata as *mut EthtoolCmd).cast();
                    // SAFETY: SIOCETHTOOL fills the ethtool_cmd buffer.
                    if unsafe { portable_ioctl(sockfd, SIOCETHTOOL as libc::c_ulong, &mut ifr as *mut _) } == 0 {
                        iface.speed = ((edata.speed_hi as i32) << 16) | edata.speed as i32;
                    }
                }
            }
            // SAFETY: the socket opened above.
            unsafe { libc::close(sockfd) };
        }
    }

    // SAFETY: frees the list obtained by getifaddrs above.
    unsafe { libc::freeifaddrs(ifap) };
    Ok(results)
}

/// appendSpeed: Tbps/Gbps through ffStrbufAppendDouble with the
/// display.fraction options (DEFAULT strips the trailing zero, so 2.5
/// Gbps never prints "2.50"), Mbps as the raw integer.
fn append_speed(speed: i32, buffer: &mut String, df: &crate::format::DisplayFormat) {
    if speed >= 1_000_000 {
        crate::format::fraction_append_num(buffer, speed as f64 / 1e6, df);
        buffer.push_str(" Tbps");
    } else if speed >= 1000 {
        crate::format::fraction_append_num(buffer, speed as f64 / 1e3, df);
        buffer.push_str(" Gbps");
    } else {
        buffer.push_str(&format!("{speed} Mbps"));
    }
}

/// printIp: addresses, " (mac)", " [Speed X / MTU Y]", " <flags>", " *".
fn print_ip(ip: &LocalIpResult, mark_default_route: bool, df: &crate::format::DisplayFormat) -> String {
    let mut buffer = String::new();
    if !ip.ipv4.is_empty() {
        buffer.push_str(&ip.ipv4);
    }
    if !ip.ipv6.is_empty() {
        if !buffer.is_empty() {
            buffer.push(' ');
        }
        buffer.push_str(&ip.ipv6);
    }
    if !ip.mac.is_empty() {
        if !buffer.is_empty() {
            buffer.push_str(&format!(" ({})", ip.mac));
        } else {
            buffer.push_str(&ip.mac);
        }
    }
    if ip.mtu > 0 || ip.speed > 0 {
        let flag = !buffer.is_empty();
        if flag {
            buffer.push_str(" [");
        }
        if ip.speed > 0 {
            if ip.mtu > 0 {
                buffer.push_str("Speed ");
            }
            append_speed(ip.speed, &mut buffer, df);
            if ip.mtu > 0 {
                buffer.push_str(" / MTU ");
            }
        }
        if ip.mtu > 0 {
            buffer.push_str(&format!("{}", ip.mtu));
        }
        if flag {
            buffer.push(']');
        }
    }
    if !ip.flags.is_empty() {
        let flag = !buffer.is_empty();
        if flag {
            buffer.push_str(" <");
        }
        buffer.push_str(&ip.flags);
        if flag {
            buffer.push('>');
        }
    }
    if mark_default_route && ip.default_route != 0 {
        buffer.push_str(" *");
    }
    buffer
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let options = LocalIpOptions::from_instance(instance);
    let show = options.show_type;
    let Ok(mut results) = detect(&options) else {
        return Vec::new();
    };
    if results.is_empty() {
        // "Failed to detect any IPs" is hidden by default.
        return Vec::new();
    }
    results.sort_by(|a, b| a.name.cmp(&b.name));

    if show & show::COMPACT != 0 {
        let joined = results
            .iter()
            .map(|ip| print_ip(ip, false, &instance.display_format))
            .collect::<Vec<_>>()
            .join(" - ");
        return vec![Row::new(instance.display_name("localip"), joined)];
    }

    results
        .iter()
        .enumerate()
        .map(|(index, ip)| {
            let name = if ip.name.is_empty() {
                format!("unknown {}", index + 1)
            } else {
                ip.name.clone()
            };
            let mark_default = show & show::DEFAULT_ROUTE_ONLY == 0;
            Row::new(&format!("{} ({name})", instance.display_name("localip")), print_ip(ip, mark_default, &instance.display_format))
                .with_key_args(vec![
                    crate::modules::RowKeyArg::uint("index", crate::modules::numbered_index(index, results.len())),
                    crate::modules::RowKeyArg::text("ifname", ip.name.clone()),
                    crate::modules::RowKeyArg::text("mac", ip.mac.clone()),
                ])
        })
        .collect()
}

/// ffGenerateLocalIpJsonResult: fields gated by the showType bits.
pub fn json(instance: &Instance) -> JsonOutcome {
    let options = LocalIpOptions::from_instance(instance);
    let show = options.show_type;
    match detect(&options) {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(results) => {
            let mut arr = Json::Array(Vec::new());
            for ip in &results {
                let mut obj = Json::obj();
                obj.push("name", ip.name.clone());
                if show & (show::IPV4 | show::IPV6) != 0 {
                    let mut default_route = Json::obj();
                    if show & show::IPV4 != 0 {
                        default_route.push("ipv4", ip.default_route & show::IPV4 != 0);
                    }
                    if show & show::IPV6 != 0 {
                        default_route.push("ipv6", ip.default_route & show::IPV6 != 0);
                    }
                    obj.push("defaultRoute", default_route);
                }
                if show & show::IPV4 != 0 {
                    obj.push("ipv4", ip.ipv4.clone());
                }
                if show & show::IPV6 != 0 {
                    obj.push("ipv6", ip.ipv6.clone());
                }
                if show & show::MAC != 0 {
                    obj.push("mac", ip.mac.clone());
                }
                if show & show::MTU != 0 {
                    obj.push("mtu", Json::I64(ip.mtu as i64));
                }
                if show & show::SPEED != 0 {
                    obj.push("speed", Json::I64(ip.speed as i64));
                }
                if show & show::FLAGS != 0 {
                    obj.push("flags", ip.flags.clone());
                }
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

    #[test]
    fn ni_flags_join_in_table_order() {
        assert_eq!(
            super::fill_ni_flags(libc::IFF_UP as u32 | libc::IFF_RUNNING as u32 | 0x10000),
            "UP,RUNNING,LOWER_UP"
        );
    }

    #[test]
    fn ipv6_types_classify_prefixes() {
        // Addresses absent from /proc/net/if_inet6 are secondary; link-local
        // addresses always are, since the list only tracks global/ULA ones.
        assert_eq!(super::get_ipv6_type(&super::Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1).octets()), super::v6::GUA | super::v6::SECONDARY);
        assert_eq!(super::get_ipv6_type(&super::Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1).octets()), super::v6::ULA | super::v6::SECONDARY);
        assert_eq!(super::get_ipv6_type(&super::Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1).octets()), super::v6::LLA | super::v6::SECONDARY);
        assert_eq!(super::get_ipv6_type(&super::Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1).octets()), super::v6::UNKNOWN);
    }

    #[test]
    fn formats_speed_units() {
        let df = Default::default();
        let mut buffer = String::new();
        super::append_speed(100, &mut buffer, &df);
        assert_eq!(buffer, "100 Mbps");
        buffer.clear();
        super::append_speed(2500, &mut buffer, &df);
        // DEFAULT strips the trailing zero: "2.5", not "2.50".
        assert_eq!(buffer, "2.5 Gbps");
        buffer.clear();
        super::append_speed(2_000_000, &mut buffer, &df);
        assert_eq!(buffer, "2 Tbps");
        let always = crate::format::DisplayFormat {
            fraction_trailing_zeros: crate::format::FractionTrailingZeros::Always,
            ..Default::default()
        };
        buffer.clear();
        super::append_speed(2500, &mut buffer, &always);
        assert_eq!(buffer, "2.50 Gbps");
    }
}

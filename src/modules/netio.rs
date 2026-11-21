//! `netio` module: rx/tx rates of the default-route interface, ported from
//! netio_linux.c and netif_linux.c. Counters come from /sys/class/net, the
//! default route is found by dumping the kernel's IPv4 route table over a
//! netlink socket (lowest metric wins), and the rate is the byte delta over
//! the 250ms sample window.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

const WAIT_MS: u64 = 250;

/// One interface's counters, in the field order the rate loop walks.
#[derive(Clone)]
struct NetIoSample {
    name: String,
    /// Only rendered when defaultRouteOnly is off; the default config keeps
    /// it set but unused.
    #[allow(dead_code)]
    default_route: bool,
    rx_bytes: u64,
    tx_bytes: u64,
    rx_packets: u64,
    tx_packets: u64,
    rx_errors: u64,
    tx_errors: u64,
    rx_dropped: u64,
    tx_dropped: u64,
}

/// The default-route cache, upstream's ffCacheGet entry — the only cache
/// ffCacheInvalidateAll registers, so the watch loop's per-round
/// re-detection reduces to dropping this.
static ROUTE_CACHE: Mutex<Option<Option<String>>> = Mutex::new(None);

/// ffNetifGetDefaultRouteImplV4: dump AF_INET routes over netlink and keep
/// the lowest-metric default route's interface.
fn default_route_ifname() -> Option<String> {
    let mut route = ROUTE_CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if route.is_none() {
        *route = Some(unsafe { default_route_ifname_impl() });
    }
    route.clone().flatten()
}

/// ffCacheInvalidateAll: the default-route entries are its only
/// registrants, so the watch loop's per-round re-detection reduces to
/// dropping this cache.
pub(crate) fn invalidate_route_cache() {
    *ROUTE_CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

unsafe fn default_route_ifname_impl() -> Option<String> {
    use libc::{RT_SCOPE_HOST, RTA_GATEWAY, RTA_OIF, RTA_PRIORITY, RTN_LOCAL};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct RtMsg {
        rtm_family: u8,
        rtm_dst_len: u8,
        rtm_src_len: u8,
        rtm_tos: u8,
        rtm_table: u8,
        rtm_protocol: u8,
        rtm_scope: u8,
        rtm_type: u8,
        rtm_flags: libc::c_uint,
    }

    let sock_fd = unsafe {
        libc::socket(libc::AF_NETLINK, libc::SOCK_RAW | libc::SOCK_CLOEXEC, libc::NETLINK_ROUTE)
    };
    if sock_fd < 0 {
        return None;
    }

    let mut bind_addr: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    bind_addr.nl_family = libc::AF_NETLINK as u16;
    if unsafe {
        libc::bind(
            sock_fd,
            (&bind_addr as *const libc::sockaddr_nl).cast(),
            std::mem::size_of::<libc::sockaddr_nl>() as u32,
        )
    } < 0
    {
        unsafe { libc::close(sock_fd) };
        return None;
    }

    let mut port_addr: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    let mut port_len = std::mem::size_of::<libc::sockaddr_nl>() as u32;
    let pid = if unsafe {
        libc::getsockname(sock_fd, (&mut port_addr as *mut libc::sockaddr_nl).cast(), &mut port_len)
    } < 0
    {
        unsafe { libc::close(sock_fd) };
        return None;
    } else {
        port_addr.nl_pid
    };

    // nlmsghdr + rtmsg + rtattr(RTA_TABLE) + u32 table, packed.
    #[repr(C, packed)]
    struct Request {
        // Only sent over the netlink socket; Rust never reads the fields.
        #[allow(dead_code)]
        nlh: libc::nlmsghdr,
        rtm: RtMsg,
        rta: libc::rtattr,
        table: u32,
    }
    let req = Request {
        nlh: libc::nlmsghdr {
            nlmsg_len: std::mem::size_of::<Request>() as u32,
            nlmsg_type: libc::RTM_GETROUTE,
            nlmsg_flags: (libc::NLM_F_REQUEST | libc::NLM_F_DUMP) as u16,
            nlmsg_seq: 1,
            nlmsg_pid: pid,
        },
        rtm: RtMsg {
            rtm_family: libc::AF_INET as u8,
            rtm_dst_len: 0,
            rtm_src_len: 0,
            rtm_tos: 0,
            rtm_table: libc::RT_TABLE_UNSPEC,
            rtm_protocol: libc::RTPROT_UNSPEC,
            rtm_scope: libc::RT_SCOPE_UNIVERSE,
            rtm_type: libc::RTN_UNSPEC,
            rtm_flags: 0,
        },
        rta: libc::rtattr {
            rta_len: std::mem::size_of::<libc::rtattr>() as u16 + 4,
            rta_type: libc::RTA_TABLE,
        },
        table: libc::RT_TABLE_MAIN as u32,
    };

    let mut dest_addr: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    dest_addr.nl_family = libc::AF_NETLINK as u16;
    let sent = unsafe {
        libc::sendto(
            sock_fd,
            (&req as *const Request).cast(),
            std::mem::size_of::<Request>(),
            0,
            (&dest_addr as *const libc::sockaddr_nl).cast(),
            std::mem::size_of::<libc::sockaddr_nl>() as u32,
        )
    };
    if sent != std::mem::size_of::<Request>() as isize {
        unsafe { libc::close(sock_fd) };
        return None;
    }

    // Dump replies are multi-part; a receive timeout guards against a
    // missing NLMSG_DONE.
    let timeout = libc::timeval {
        tv_sec: 1,
        tv_usec: 0,
    };
    unsafe {
        libc::setsockopt(
            sock_fd,
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            (&timeout as *const libc::timeval).cast(),
            std::mem::size_of::<libc::timeval>() as u32,
        )
    };

    let mut buffer = vec![0u8; 16 * 1024];
    let mut min_metric = u32::MAX;
    let mut best_ifindex: u32 = 0;

    loop {
        let received = unsafe {
            libc::recvfrom(
                sock_fd,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if received <= 0 {
            break;
        }
        let received = received as usize;

        // Walk nlmsghdr chain, 4-byte aligned.
        let mut offset = 0usize;
        let mut done = false;
        while offset + std::mem::size_of::<libc::nlmsghdr>() <= received {
            let nlh =
                unsafe { buffer.as_ptr().add(offset).cast::<libc::nlmsghdr>().read_unaligned() };
            if nlh.nlmsg_len < std::mem::size_of::<libc::nlmsghdr>() as u32
                || offset + nlh.nlmsg_len as usize > received
            {
                break;
            }
            if nlh.nlmsg_seq != 1 || nlh.nlmsg_pid != pid {
                offset += (nlh.nlmsg_len as usize + 3) & !3;
                continue;
            }
            if nlh.nlmsg_type == libc::NLMSG_DONE as u16 {
                done = true;
                break;
            }
            if nlh.nlmsg_type == libc::NLMSG_ERROR as u16 || nlh.nlmsg_type != libc::RTM_NEWROUTE {
                offset += (nlh.nlmsg_len as usize + 3) & !3;
                continue;
            }

            let rtm = unsafe {
                buffer
                    .as_ptr()
                    .add(offset + std::mem::size_of::<libc::nlmsghdr>())
                    .cast::<RtMsg>()
                    .read_unaligned()
            };
            if rtm.rtm_family != libc::AF_INET as u8
                || rtm.rtm_dst_len != 0
                || rtm.rtm_scope == RT_SCOPE_HOST
                || rtm.rtm_type == RTN_LOCAL
            {
                offset += (nlh.nlmsg_len as usize + 3) & !3;
                continue;
            }

            let attrs_start =
                offset + std::mem::size_of::<libc::nlmsghdr>() + std::mem::size_of::<RtMsg>();
            let attrs_len = nlh.nlmsg_len as usize
                - std::mem::size_of::<libc::nlmsghdr>()
                - std::mem::size_of::<RtMsg>();

            let mut ifindex = 0u32;
            let mut metric = 0u32;
            let mut skip_route = false;
            let mut attr_offset = 0usize;
            while attr_offset + std::mem::size_of::<libc::rtattr>() <= attrs_len {
                let rta = unsafe {
                    buffer
                        .as_ptr()
                        .add(attrs_start + attr_offset)
                        .cast::<libc::rtattr>()
                        .read_unaligned()
                };
                if (rta.rta_len as usize) < std::mem::size_of::<libc::rtattr>()
                    || attr_offset + rta.rta_len as usize > attrs_len
                {
                    break;
                }
                if (rta.rta_len as usize - std::mem::size_of::<libc::rtattr>()) >= 4 {
                    let data = unsafe {
                        buffer
                            .as_ptr()
                            .add(attrs_start + attr_offset + std::mem::size_of::<libc::rtattr>())
                            .cast::<u32>()
                            .read_unaligned()
                    };
                    match rta.rta_type {
                        RTA_OIF => ifindex = data,
                        RTA_GATEWAY => {
                            if data == 0 {
                                skip_route = true;
                            }
                        },
                        RTA_PRIORITY => {
                            if data >= min_metric {
                                skip_route = true;
                            }
                            metric = data;
                        },
                        _ => {},
                    }
                }
                attr_offset += (rta.rta_len as usize + 3) & !3;
            }

            if !skip_route && ifindex != 0 && metric < min_metric {
                min_metric = metric;
                best_ifindex = ifindex;
                if min_metric == 0 {
                    done = true;
                    break;
                }
            }
            offset += (nlh.nlmsg_len as usize + 3) & !3;
        }
        if done {
            break;
        }
    }
    unsafe { libc::close(sock_fd) };

    if min_metric == u32::MAX || best_ifindex == 0 {
        return None;
    }
    let mut name_buf = [0i8; libc::IF_NAMESIZE];
    let name = unsafe { libc::if_indextoname(best_ifindex, name_buf.as_mut_ptr()) };
    if name.is_null() {
        return None;
    }
    Some(unsafe { std::ffi::CStr::from_ptr(name) }.to_string_lossy().into_owned())
}

fn read_u64(dir: &std::path::Path, file: &str) -> u64 {
    util::read_trimmed_in(dir, file).and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn get_data(net_dir: &std::path::Path, if_name: &str, default_route: bool) -> Option<NetIoSample> {
    let dir = net_dir.join(if_name);
    let operstate = util::read_trimmed_in(&dir, "operstate")?;
    // 'u' matches both "up" and "unknown"
    if !operstate.starts_with('u') {
        return None;
    }
    Some(NetIoSample {
        name: if_name.to_string(),
        default_route,
        rx_bytes: read_u64(&dir, "statistics/rx_bytes"),
        tx_bytes: read_u64(&dir, "statistics/tx_bytes"),
        rx_packets: read_u64(&dir, "statistics/rx_packets"),
        tx_packets: read_u64(&dir, "statistics/tx_packets"),
        rx_errors: read_u64(&dir, "statistics/rx_errors"),
        tx_errors: read_u64(&dir, "statistics/tx_errors"),
        rx_dropped: read_u64(&dir, "statistics/rx_dropped"),
        tx_dropped: read_u64(&dir, "statistics/tx_dropped"),
    })
}

fn get_counters() -> Option<Vec<NetIoSample>> {
    let net_dir = std::path::Path::new("/sys/class/net");
    let default_route = default_route_ifname().unwrap_or_default();
    // NetIO defaults to the default-route interface only.
    if default_route.is_empty() {
        return Some(Vec::new());
    }
    let sample = get_data(net_dir, &default_route, true)?;
    Some(vec![sample])
}

static BASELINE: Mutex<Option<(Vec<NetIoSample>, Instant)>> = Mutex::new(None);

/// ffPrepareNetIO: take the first counter snapshot early.
pub(crate) fn prepare() {
    let mut baseline = match BASELINE.lock() {
        Ok(baseline) => baseline,
        Err(poisoned) => poisoned.into_inner(),
    };
    if baseline.is_none()
        && let Some(counters) = get_counters()
    {
        *baseline = Some((counters, Instant::now()));
    }
}

/// The wrapper walks the 8-byte counter fields and turns deltas into
/// per-second rates over the elapsed window.
fn apply_rates(samples: &mut [NetIoSample], prev: &[NetIoSample], elapsed_ms: u64) -> bool {
    if samples.len() != prev.len() {
        return false;
    }
    for (curr, prev) in samples.iter_mut().zip(prev) {
        if curr.name != prev.name {
            return false;
        }
        let rate = |value: u64, prev: u64| {
            (value.saturating_sub(prev) as f64 * 1000.0 / elapsed_ms as f64) as u64
        };
        curr.rx_bytes = rate(curr.rx_bytes, prev.rx_bytes);
        curr.tx_bytes = rate(curr.tx_bytes, prev.tx_bytes);
        curr.rx_packets = rate(curr.rx_packets, prev.rx_packets);
        curr.tx_packets = rate(curr.tx_packets, prev.tx_packets);
        curr.rx_errors = rate(curr.rx_errors, prev.rx_errors);
        curr.tx_errors = rate(curr.tx_errors, prev.tx_errors);
        curr.rx_dropped = rate(curr.rx_dropped, prev.rx_dropped);
        curr.tx_dropped = rate(curr.tx_dropped, prev.tx_dropped);
    }
    true
}

/// ffDetectNetIO: the two-sample window with the per-second deltas.
fn detect() -> Result<Vec<NetIoSample>, &'static str> {
    let times1 = {
        let mut baseline = match BASELINE.lock() {
            Ok(baseline) => baseline,
            Err(poisoned) => poisoned.into_inner(),
        };
        match baseline.take() {
            Some((counters, start)) => {
                let elapsed = start.elapsed();
                if elapsed.as_millis() < WAIT_MS as u128 {
                    std::thread::sleep(Duration::from_millis(WAIT_MS) - elapsed);
                }
                Some((counters, start.elapsed().as_millis() as u64))
            },
            None => {
                let Some(counters) = get_counters() else {
                    return Err("opendir(\"/sys/class/net\") == nullptr");
                };
                std::thread::sleep(Duration::from_millis(WAIT_MS));
                Some((counters, WAIT_MS))
            },
        }
    };
    let Some((counters1, elapsed_ms)) = times1 else {
        return Err("No network interfaces found");
    };
    if counters1.is_empty() {
        return Err("No network interfaces found");
    }

    let mut counters2 = match get_counters() {
        Some(counters) => counters,
        None => return Err("opendir(\"/sys/class/net\") == nullptr"),
    };
    if !apply_rates(&mut counters2, &counters1, elapsed_ms) {
        return Err("Different number of network interfaces. Network change?");
    }

    Ok(counters2)
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(counters2) = detect() else {
        return Vec::new();
    };

    counters2
        .iter()
        .map(|inf| {
            let name = if inf.name.is_empty() {
                "unknown".to_string()
            } else {
                inf.name.clone()
            };
            Row::new(
                &format!("{} ({name})", instance.display_name("netio")),
                format!(
                    "{}/s (IN) - {}/s (OUT)",
                    util::fmt_bytes(inf.rx_bytes),
                    util::fmt_bytes(inf.tx_bytes)
                ),
            )
        })
        .collect()
}



/// ffGenerateNetIOJsonResult: the per-second deltas over the default set of
/// interfaces (defaultRouteOnly is a module option not exposed here, so all
/// /sys/class/net entries appear).
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(samples) => {
            let mut arr = Json::Array(Vec::new());
            for inf in &samples {
                let mut obj = Json::obj();
                obj.push("name", inf.name.clone());
                obj.push("defaultRoute", inf.default_route);
                obj.push("txBytes", inf.tx_bytes);
                obj.push("rxBytes", inf.rx_bytes);
                obj.push("txPackets", inf.tx_packets);
                obj.push("rxPackets", inf.rx_packets);
                obj.push("rxErrors", inf.rx_errors);
                obj.push("txErrors", inf.tx_errors);
                obj.push("rxDrops", inf.rx_dropped);
                obj.push("txDrops", inf.tx_dropped);
                arr.push_value(obj);
            }
            JsonOutcome::Result(arr)
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rates_are_deltas_over_elapsed() {
        let prev = super::NetIoSample {
            name: "wlan0".to_string(),
            default_route: true,
            rx_bytes: 1000,
            tx_bytes: 500,
            rx_packets: 0,
            tx_packets: 0,
            rx_errors: 0,
            tx_errors: 0,
            rx_dropped: 0,
            tx_dropped: 0,
        };
        let mut curr = prev.clone();
        curr.rx_bytes = 3000;
        curr.tx_bytes = 2500;
        let mut samples = [curr.clone()];
        assert!(super::apply_rates(&mut samples, &[prev], 500));
        // 2000 bytes over 0.5s
        assert_eq!(samples[0].rx_bytes, 4000);
        assert_eq!(samples[0].tx_bytes, 4000);
    }
}

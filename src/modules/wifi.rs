//! `wifi` module: wireless interface state, ported from
//! detection/wifi/wifi_linux.c and modules/wifi/wifi.c. Interfaces are
//! enumerated with if_nameindex(3) and filtered on the /sys phy80211
//! directory; connected state comes from an nl80211 GET_SCAN dump for the
//! associated BSS, with GET_INTERFACE (channel width) and GET_STATION
//! (signal, rates) follow-ups, then wireless-extension ioctls fill the
//! gaps. The default value is "ssid - protocol - band GHz - security
//! (signal%)" with the security derived from RSN/WPA information
//! elements; interfaces without an SSID print their /sys operstate.

use std::ffi::{CStr, c_char, c_int, c_void};

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

/// Call `ioctl(2)` with a request cast to the type the target expects.
///
/// glibc declares the request as `c_ulong`, while bionic (Android) declares it
/// as `c_int`; `as _` lets the compiler pick the right type on each target.
#[inline]
unsafe fn portable_ioctl<T>(fd: c_int, request: libc::c_ulong, arg: *mut T) -> c_int {
    // SAFETY: the caller guarantees `fd` and `arg` are valid for `request`.
    unsafe { libc::ioctl(fd, request as _, arg) }
}

// ---- nl80211 / genetlink constants (linux/nl80211.h, linux/genetlink.h) ----

const AF_NETLINK: c_int = 16;
const NETLINK_GENERIC: c_int = 16;
const SOCK_RAW_CLOEXEC: c_int = libc::SOCK_RAW | libc::SOCK_CLOEXEC;

const GENL_ID_CTRL: u16 = 16;
const CTRL_CMD_GETFAMILY: u8 = 3;
const CTRL_CMD_NEWFAMILY: u8 = 1;
const CTRL_ATTR_FAMILY_ID: u16 = 1;
const CTRL_ATTR_FAMILY_NAME: u16 = 2;

const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLM_F_REQUEST: u16 = 1;
const NLM_F_ACK: u16 = 4;
const NLM_F_DUMP: u16 = 0x300;
const NLA_TYPE_MASK: u16 = 0x3fff;

const NL80211_CMD_GET_INTERFACE: u8 = 5;
const NL80211_CMD_GET_STATION: u8 = 17;
const NL80211_CMD_GET_SCAN: u8 = 32;
const NL80211_ATTR_IFINDEX: u16 = 3;
const NL80211_ATTR_STA_INFO: u16 = 21;
const NL80211_ATTR_BSS: u16 = 47;
const NL80211_ATTR_CHANNEL_WIDTH: u16 = 159;

const NL80211_BSS_BSSID: u16 = 1;
const NL80211_BSS_FREQUENCY: u16 = 2;
const NL80211_BSS_CAPABILITY: u16 = 5;
const NL80211_BSS_INFORMATION_ELEMENTS: u16 = 6;
const NL80211_BSS_SIGNAL_MBM: u16 = 7;
const NL80211_BSS_STATUS: u16 = 9;
const NL80211_BSS_BEACON_IES: u16 = 11;
const NL80211_BSS_STATUS_ASSOCIATED: u32 = 1;

const NL80211_STA_INFO_SIGNAL: u16 = 7;
const NL80211_STA_INFO_TX_BITRATE: u16 = 8;
const NL80211_STA_INFO_RX_BITRATE: u16 = 14;

const NL80211_RATE_INFO_BITRATE: u16 = 1;
const NL80211_RATE_INFO_MCS: u16 = 2;
const NL80211_RATE_INFO_40_MHZ_WIDTH: u16 = 3;
const NL80211_RATE_INFO_BITRATE32: u16 = 5;
const NL80211_RATE_INFO_VHT_MCS: u16 = 6;
const NL80211_RATE_INFO_80_MHZ_WIDTH: u16 = 8;
const NL80211_RATE_INFO_80P80_MHZ_WIDTH: u16 = 9;
const NL80211_RATE_INFO_160_MHZ_WIDTH: u16 = 10;
const NL80211_RATE_INFO_10_MHZ_WIDTH: u16 = 11;
const NL80211_RATE_INFO_5_MHZ_WIDTH: u16 = 12;
const NL80211_RATE_INFO_HE_MCS: u16 = 13;
const NL80211_RATE_INFO_320_MHZ_WIDTH: u16 = 18;
const NL80211_RATE_INFO_EHT_MCS: u16 = 19;
const NL80211_RATE_INFO_S1G_MCS: u16 = 23;
const NL80211_RATE_INFO_1_MHZ_WIDTH: u16 = 25;
const NL80211_RATE_INFO_2_MHZ_WIDTH: u16 = 26;
const NL80211_RATE_INFO_4_MHZ_WIDTH: u16 = 27;
const NL80211_RATE_INFO_8_MHZ_WIDTH: u16 = 28;
const NL80211_RATE_INFO_16_MHZ_WIDTH: u16 = 29;
const NL80211_RATE_INFO_UHR_MCS: u16 = 30;

// ---- wireless extension constants (linux/wireless.h) ----

const SIOCGIWNAME: libc::c_ulong = 0x8b01;
const SIOCGIWFREQ: libc::c_ulong = 0x8b05;
const SIOCGIWESSID: libc::c_ulong = 0x8b07;
const SIOCGIWSTATS: libc::c_ulong = 0x8b1d;
const SIOCGIWAP: libc::c_ulong = 0x8b15;
const SIOCGIWRATE: libc::c_ulong = 0x8b20;
const SIOCGIWENCODEEXT: libc::c_ulong = 0x8b2d;
const IW_ESSID_MAX_SIZE: usize = 32;
const IW_ENCODE_ALG_WEP: u16 = 1;
const IW_ENCODE_ALG_TKIP: u16 = 2;
const IW_ENCODE_ALG_CCMP: u16 = 3;
const IW_ENCODE_ALG_PMK: u16 = 4;
const IW_ENCODE_ALG_AES_CMAC: u16 = 5;
const IFF_UP: u32 = 1;

/// ffRssiToSignalQuality: linear RSSI to percent, clamped.
fn rssi_to_quality(rssi: i32) -> f64 {
    if rssi >= -50 {
        100.0
    } else if rssi <= -100 {
        0.0
    } else {
        ((rssi + 100) * 2) as f64
    }
}

/// ffWifiFreqToChannel: 0 for unknown / non-standard frequencies.
fn freq_to_channel(frequency: u16) -> u16 {
    if frequency == 2484 {
        return 14;
    }
    if (2412..=2472).contains(&frequency) && (frequency - 2407).is_multiple_of(5) {
        return (frequency - 2407) / 5;
    }
    if (4910..=4980).contains(&frequency) && (frequency - 4000).is_multiple_of(5) {
        return (frequency - 4000) / 5;
    }
    if (5000..=5895).contains(&frequency) && (frequency - 5000).is_multiple_of(5) {
        return (frequency - 5000) / 5;
    }
    if frequency == 5935 {
        return 2;
    }
    if (5955..=7115).contains(&frequency) && (frequency - 5950).is_multiple_of(5) {
        return (frequency - 5950) / 5;
    }
    0
}

/// ffWifiChannelWidthToMhz.
fn channel_width_to_mhz(width: u32) -> u16 {
    match width {
        0 | 1 => 20, // 20_NOHT, 20
        2 => 40,     // 40
        3 | 4 => 80, // 80, 80P80
        5 => 160,    // 160
        6 => 5,      // 5
        7 => 10,     // 10
        8 => 1,      // 1
        9 => 2,      // 2
        10 => 4,     // 4
        11 => 8,     // 8
        12 => 16,    // 16
        13 => 320,   // 320
        _ => 0,
    }
}

#[derive(Default)]
struct WifiItem {
    /// The interface name, upstream's inf.description.
    inf_description: String,
    inf_status: String,
    status: String,
    ssid: String,
    bssid: String,
    protocol: String,
    security: String,
    signal_quality: Option<f64>,
    rx_rate: Option<f64>,
    tx_rate: Option<f64>,
    channel: u16,
    channel_width: u16,
    frequency: u16,
}

// ---- netlink plumbing ----

/// sockaddr_nl; not in this libc version for Linux, defined per <linux/netlink.h>.
#[repr(C)]
struct SockaddrNl {
    family: u16,
    pad: u16,
    pid: u32,
    groups: u32,
}

struct NlContext {
    sock_fd: c_int,
    family_id: u16,
    port_id: u32,
    seq: u32,
}

impl NlContext {
    const UNINITIALIZED: c_int = -1;
    const FAILED: c_int = -2;

    /// ffWifiNlInit: raw netlink socket bound to the generic family with a
    /// 250 ms receive timeout, then the nl80211 family id query.
    fn init(&mut self) -> bool {
        // SAFETY: plain socket/bind/setsockopt/getsockname on PODs.
        unsafe {
            let fd = libc::socket(AF_NETLINK, SOCK_RAW_CLOEXEC, NETLINK_GENERIC);
            if fd < 0 {
                return false;
            }
            self.sock_fd = fd;

            let addr = SockaddrNl {
                family: AF_NETLINK as u16,
                pad: 0,
                pid: 0,
                groups: 0,
            };
            if libc::bind(
                fd,
                (&addr as *const SockaddrNl).cast(),
                std::mem::size_of_val(&addr) as u32,
            ) < 0
            {
                libc::close(fd);
                self.sock_fd = Self::UNINITIALIZED;
                return false;
            }

            let timeout = libc::timeval {
                tv_sec: 0,
                tv_usec: 250_000,
            };
            if libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                &timeout as *const libc::timeval as *const c_void,
                std::mem::size_of_val(&timeout) as u32,
            ) < 0
            {
                libc::close(fd);
                self.sock_fd = Self::UNINITIALIZED;
                return false;
            }

            let mut found = SockaddrNl {
                family: 0,
                pad: 0,
                pid: 0,
                groups: 0,
            };
            let mut found_len = std::mem::size_of_val(&found) as u32;
            if libc::getsockname(fd, (&mut found as *mut SockaddrNl).cast(), &mut found_len) < 0 {
                libc::close(fd);
                self.sock_fd = Self::UNINITIALIZED;
                return false;
            }
            self.port_id = found.pid;
        }
        self.get_family_id()
    }

    /// ffWifiNlGetFamilyId.
    fn get_family_id(&mut self) -> bool {
        self.seq += 1;
        let seq = self.seq;
        let mut request = Vec::with_capacity(64);
        put_nlmsg(&mut request, GENL_ID_CTRL, NLM_F_REQUEST | NLM_F_ACK, seq, self.port_id);
        put_genl(&mut request, CTRL_CMD_GETFAMILY, 1);
        put_attr(&mut request, CTRL_ATTR_FAMILY_NAME, b"nl80211\0"); // sizeof("nl80211")
        patch_len(&mut request);

        // SAFETY: sendto/recvfrom on our own socket with sized buffers.
        unsafe {
            let addr = SockaddrNl {
                family: AF_NETLINK as u16,
                pad: 0,
                pid: 0,
                groups: 0,
            };
            if libc::sendto(
                self.sock_fd,
                request.as_ptr().cast(),
                request.len(),
                0,
                (&addr as *const SockaddrNl).cast(),
                std::mem::size_of_val(&addr) as u32,
            ) != request.len() as isize
            {
                return false;
            }
        }

        let mut buffer = vec![0u8; 8192];
        loop {
            // SAFETY: recvfrom into our buffer.
            let received = unsafe {
                libc::recvfrom(
                    self.sock_fd,
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if received < 0 {
                return false;
            }
            let mut found = false;
            for_each_nlmsg(&buffer[..received as usize], &mut |payload, msg_type, seq| {
                if seq != self.seq {
                    // Stale reply still queued from an earlier exchange.
                    return None;
                }
                if msg_type != GENL_ID_CTRL || payload.len() < 4 || payload[0] != CTRL_CMD_NEWFAMILY
                {
                    return None;
                }
                for attr in each_attr(&payload[4..]) {
                    if attr.attr_type == CTRL_ATTR_FAMILY_ID && attr.payload.len() >= 2 {
                        self.family_id = u16::from_ne_bytes([attr.payload[0], attr.payload[1]]);
                        found = true;
                        return Some(());
                    }
                }
                None
            });
            if found {
                return true;
            }
        }
    }

    /// One netlink request/response round trip; the handler receives each
    /// message matching our sequence and returns Some(value) to stop, None
    /// to keep receiving (multipart dumps until DONE). Replies from
    /// earlier exchanges still queued are skipped by sequence.
    fn exchange<T>(
        &mut self,
        msg_type: u16,
        flags: u16,
        cmd: u8,
        attrs: &[(u16, Vec<u8>)],
        recv_len: usize,
        handler: &mut dyn FnMut(&[u8], u16) -> Option<T>,
    ) -> Option<T> {
        self.seq += 1;
        let seq = self.seq;

        let mut request = Vec::with_capacity(64);
        put_nlmsg(&mut request, msg_type, flags, seq, self.port_id);
        put_genl(&mut request, cmd, 0);
        for (attr_type, data) in attrs {
            put_attr(&mut request, *attr_type, data);
        }
        patch_len(&mut request);

        // SAFETY: sendto/recvfrom on our own socket with sized buffers.
        let sent = unsafe {
            let addr = SockaddrNl {
                family: AF_NETLINK as u16,
                pad: 0,
                pid: 0,
                groups: 0,
            };
            libc::sendto(
                self.sock_fd,
                request.as_ptr().cast(),
                request.len(),
                0,
                (&addr as *const SockaddrNl).cast(),
                std::mem::size_of_val(&addr) as u32,
            )
        };
        if sent != request.len() as isize {
            return None;
        }

        let mut buffer = vec![0u8; recv_len];
        loop {
            // SAFETY: recvfrom into our buffer.
            let received = unsafe {
                libc::recvfrom(
                    self.sock_fd,
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if received <= 0 {
                return None;
            }
            let mut stop = None;
            for_each_nlmsg(&buffer[..received as usize], &mut |payload, msg_type, msg_seq| {
                if msg_seq != seq {
                    return None;
                }
                if let Some(value) = handler(payload, msg_type) {
                    stop = Some(value);
                    return Some(());
                }
                None
            });
            if stop.is_some() {
                return stop;
            }
        }
    }
}

struct NlAttrView<'a> {
    attr_type: u16,
    payload: &'a [u8],
}

/// Native-endian scalar reads with a zero fallback for short payloads;
/// the call sites check the length first, like the upstream guards.
fn read_u32(data: &[u8]) -> u32 {
    u32::from_ne_bytes([data[0], data[1], data[2], data[3]])
}

fn read_i32(data: &[u8]) -> i32 {
    i32::from_ne_bytes([data[0], data[1], data[2], data[3]])
}

fn read_u16(data: &[u8]) -> u16 {
    u16::from_ne_bytes([data[0], data[1]])
}

/// Iterate NLA-aligned attributes inside a buffer (ffWifiNlAttrOk/Next).
fn each_attr(mut remaining: &[u8]) -> impl Iterator<Item = NlAttrView<'_>> {
    std::iter::from_fn(move || {
        if remaining.len() < 4 {
            return None;
        }
        let len = u16::from_ne_bytes([remaining[0], remaining[1]]) as usize;
        let attr_type = u16::from_ne_bytes([remaining[2], remaining[3]]);
        if len < 4 || len > remaining.len() {
            remaining = &[];
            return None;
        }
        let payload = &remaining[4..len];
        let aligned = (len + 3) & !3;
        remaining = &remaining[aligned.min(remaining.len())..];
        Some(NlAttrView {
            attr_type: attr_type & NLA_TYPE_MASK,
            payload,
        })
    })
}

/// The per-message handler: (payload, type, sequence); Some stops the walk.
type MessageHandler<'a> = &'a mut dyn FnMut(&[u8], u16, u32) -> Option<()>;

/// Iterate NLMSG-aligned messages in a reply; a Some from the callback
/// stops the walk. The callback also receives the message sequence, so
/// exchanges can skip stale replies still queued from earlier dumps.
fn for_each_nlmsg(buffer: &[u8], handle: MessageHandler<'_>) {
    let mut offset = 0usize;
    while offset + 16 <= buffer.len() {
        let len = u32::from_ne_bytes([
            buffer[offset],
            buffer[offset + 1],
            buffer[offset + 2],
            buffer[offset + 3],
        ]) as usize;
        if len < 16 || offset + len > buffer.len() {
            return;
        }
        let msg_type = u16::from_ne_bytes([buffer[offset + 4], buffer[offset + 5]]);
        let seq = u32::from_ne_bytes([
            buffer[offset + 8],
            buffer[offset + 9],
            buffer[offset + 10],
            buffer[offset + 11],
        ]);
        if handle(&buffer[offset + 16..offset + len], msg_type, seq).is_some() {
            return;
        }
        offset += (len + 3) & !3;
    }
}

fn put_nlmsg(buffer: &mut Vec<u8>, msg_type: u16, flags: u16, seq: u32, pid: u32) {
    buffer.extend_from_slice(&0u32.to_ne_bytes()); // length, patched last
    buffer.extend_from_slice(&msg_type.to_ne_bytes());
    buffer.extend_from_slice(&flags.to_ne_bytes());
    buffer.extend_from_slice(&seq.to_ne_bytes());
    buffer.extend_from_slice(&pid.to_ne_bytes());
}

/// Fix the nlmsghdr length once the payload is complete.
fn patch_len(buffer: &mut [u8]) {
    let len = buffer.len() as u32;
    buffer[..4].copy_from_slice(&len.to_ne_bytes());
}

fn put_genl(buffer: &mut Vec<u8>, cmd: u8, version: u8) {
    buffer.push(cmd);
    buffer.push(version);
    buffer.extend_from_slice(&[0, 0]);
}

fn put_attr(buffer: &mut Vec<u8>, attr_type: u16, data: &[u8]) {
    let len = 4 + data.len();
    buffer.extend_from_slice(&(len as u16).to_ne_bytes());
    buffer.extend_from_slice(&attr_type.to_ne_bytes());
    buffer.extend_from_slice(data);
    let padding = (4 - len % 4) % 4;
    buffer.extend(std::iter::repeat_n(0u8, padding));
}

// ---- detection stages ----

/// ffWifiParseBitrateFromRateInfo: rate in Mbps plus protocol/width from a
/// nested NL80211_RATE_INFO structure.
fn parse_rate_info(
    rate_attr: &NlAttrView,
    protocol: &mut String,
    channel_width: &mut u16,
) -> Option<f64> {
    let mut rate: Option<f64> = None;
    let mut width = 0u16;
    for info in each_attr(rate_attr.payload) {
        match info.attr_type {
            NL80211_RATE_INFO_40_MHZ_WIDTH => width = 40,
            NL80211_RATE_INFO_80_MHZ_WIDTH | NL80211_RATE_INFO_80P80_MHZ_WIDTH => width = 80,
            NL80211_RATE_INFO_160_MHZ_WIDTH => width = 160,
            NL80211_RATE_INFO_320_MHZ_WIDTH => width = 320,
            NL80211_RATE_INFO_10_MHZ_WIDTH => width = 10,
            NL80211_RATE_INFO_5_MHZ_WIDTH => width = 5,
            NL80211_RATE_INFO_1_MHZ_WIDTH => width = 1,
            NL80211_RATE_INFO_2_MHZ_WIDTH => width = 2,
            NL80211_RATE_INFO_4_MHZ_WIDTH => width = 4,
            NL80211_RATE_INFO_8_MHZ_WIDTH => width = 8,
            NL80211_RATE_INFO_16_MHZ_WIDTH => width = 16,
            NL80211_RATE_INFO_UHR_MCS => *protocol = "802.11bn (Wi-Fi 8)".to_string(),
            NL80211_RATE_INFO_S1G_MCS => *protocol = "802.11ah (Wi-Fi HaLow)".to_string(),
            NL80211_RATE_INFO_EHT_MCS => *protocol = "802.11be (Wi-Fi 7)".to_string(),
            NL80211_RATE_INFO_HE_MCS => *protocol = "802.11ax (Wi-Fi 6)".to_string(),
            NL80211_RATE_INFO_VHT_MCS => *protocol = "802.11ac (Wi-Fi 5)".to_string(),
            NL80211_RATE_INFO_MCS => *protocol = "802.11n (Wi-Fi 4)".to_string(),
            NL80211_RATE_INFO_BITRATE32 => {
                if info.payload.len() >= 4 {
                    rate = Some(read_u32(info.payload) as f64 / 10.0);
                }
            },
            NL80211_RATE_INFO_BITRATE if info.payload.len() >= 2 && rate.is_none() => {
                rate = Some(read_u16(info.payload) as f64 / 10.0);
            },
            _ => {},
        }
    }
    let rate = rate?;
    if *channel_width == 0 {
        *channel_width = width;
    }
    Some(rate)
}

#[derive(Default)]
struct SecurityFlags {
    privacy: bool,
    wep: bool,
    wpa: bool,
    wpa2: bool,
    wpa3: bool,
    owe: bool,
    eap: bool,
}

/// ffWifiApplySecurityFlags.
fn apply_security_flags(item: &mut WifiItem, sec: &SecurityFlags) {
    let mut security = String::new();
    if sec.wep {
        security.push_str("WEP/");
    }
    if sec.wpa {
        security.push_str("WPA/");
    }
    if sec.wpa2 {
        security.push_str("WPA2/");
    }
    if sec.wpa3 {
        security.push_str("WPA3/");
    }
    if sec.owe {
        security.push_str("OWE/");
    }
    if sec.eap {
        security.push_str("802.1X/");
    }

    if security.is_empty() {
        item.security = if sec.privacy { "WEP" } else { "Insecure" }.to_string();
    } else {
        security.pop(); // trim the trailing '/'
        item.security = security;
    }
}

/// ffWifiParseRsnIe: RSN information element (id 48).
fn parse_rsn_ie(ie: &[u8], sec: &mut SecurityFlags) {
    if ie.len() < 8 {
        // version(2) + group cipher suite(4) + pairwise count(2)
        return;
    }
    sec.wpa2 = true;
    let mut pos = 2usize; // version field length
    if pos + 4 > ie.len() {
        return;
    }
    pos += 4; // skip group cipher suite selector
    if pos + 2 > ie.len() {
        return;
    }
    let pairwise_count = u16::from_le_bytes([ie[pos], ie[pos + 1]]) as usize;
    pos += 2;
    let pairwise_len = pairwise_count * 4; // each suite selector is 4 bytes
    if pos + pairwise_len > ie.len() {
        return;
    }
    pos += pairwise_len;
    if pos + 2 > ie.len() {
        return;
    }
    let akm_count = u16::from_le_bytes([ie[pos], ie[pos + 1]]) as usize;
    pos += 2;

    let mut i = 0;
    while i < akm_count && pos + 4 <= ie.len() {
        let akm = &ie[pos..pos + 4];
        if akm[0] == 0x00 && akm[1] == 0x0f && akm[2] == 0xac {
            // RSN OUI 00:0f:ac
            match akm[3] {
                1 | 5 | 11 | 12 => sec.eap = true, // 802.1X variants
                8 => sec.wpa3 = true,              // SAE (WPA3-Personal)
                18 => sec.owe = true,              // OWE
                _ => {},
            }
        }
        i += 1;
        pos += 4;
    }

    if sec.owe {
        sec.wpa2 = false;
    }
}

/// ffWifiParseWpaVendorIe: Microsoft WPA vendor information element (221).
fn parse_wpa_vendor_ie(ie: &[u8], sec: &mut SecurityFlags) {
    if ie.len() < 8 {
        return;
    }
    if !(ie[0] == 0x00 && ie[1] == 0x50 && ie[2] == 0xf2 && ie[3] == 0x01) {
        // Microsoft WPA OUI/type
        return;
    }
    sec.wpa = true;
    let mut pos = 4usize;
    if pos + 2 > ie.len() {
        return;
    }
    pos += 2; // skip WPA version
    if pos + 4 > ie.len() {
        return;
    }
    pos += 4; // skip multicast cipher suite selector
    if pos + 2 > ie.len() {
        return;
    }
    let pairwise_count = u16::from_le_bytes([ie[pos], ie[pos + 1]]) as usize;
    pos += 2 + pairwise_count * 4; // count field(2) + N unicast selectors(4)
    if pos + 2 > ie.len() {
        return;
    }
    let akm_count = u16::from_le_bytes([ie[pos], ie[pos + 1]]) as usize;
    pos += 2;

    let mut i = 0;
    while i < akm_count && pos + 4 <= ie.len() {
        let akm = &ie[pos..pos + 4];
        if akm[0] == 0x00 && akm[1] == 0x50 && akm[2] == 0xf2 && akm[3] == 1 {
            // WPA Enterprise (802.1X)
            sec.eap = true;
        }
        i += 1;
        pos += 4;
    }
}

/// ffWifiParseInformationElements: SSID, RSN and WPA vendor elements.
fn parse_information_elements(ies: &[u8], item: &mut WifiItem, sec: &mut SecurityFlags) {
    let mut pos = 0usize;
    while pos + 2 <= ies.len() {
        let id = ies[pos];
        let len = ies[pos + 1] as usize;
        pos += 2;
        if pos + len > ies.len() {
            break;
        }
        let ie = &ies[pos..pos + len];
        match id {
            0 => item.ssid = String::from_utf8_lossy(ie).into_owned(), // SSID
            48 => parse_rsn_ie(ie, sec),                               // RSN
            221 => parse_wpa_vendor_ie(ie, sec),                       // vendor (WPA)
            _ => {},
        }
        pos += len;
    }
}

/// ffWifiIsBssAssociated.
fn is_bss_associated(bss_attr: &NlAttrView) -> bool {
    for attr in each_attr(bss_attr.payload) {
        if attr.attr_type == NL80211_BSS_STATUS && attr.payload.len() >= 4 {
            return read_u32(attr.payload) == NL80211_BSS_STATUS_ASSOCIATED;
        }
    }
    false
}

/// ffWifiParseBssAttr.
fn parse_bss_attr(bss_attr: &NlAttrView, item: &mut WifiItem) {
    let mut sec = SecurityFlags::default();
    for attr in each_attr(bss_attr.payload) {
        match attr.attr_type {
            NL80211_BSS_BSSID if attr.payload.len() >= 6 => {
                item.bssid = attr.payload[..6]
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(":");
            },
            NL80211_BSS_FREQUENCY if attr.payload.len() >= 4 => {
                item.frequency = read_u32(attr.payload) as u16;
                item.channel = freq_to_channel(item.frequency);
            },
            NL80211_BSS_SIGNAL_MBM if attr.payload.len() >= 4 => {
                // mBm (100 * dBm) => dBm
                let mbm = read_i32(attr.payload);
                item.signal_quality = Some(rssi_to_quality(mbm / 100));
            },
            NL80211_BSS_CAPABILITY if attr.payload.len() >= 2 => {
                // IEEE 802.11 capability bit 4: privacy
                let capability = read_u16(attr.payload);
                sec.privacy = capability & (1 << 4) != 0;
            },
            NL80211_BSS_INFORMATION_ELEMENTS | NL80211_BSS_BEACON_IES => {
                parse_information_elements(attr.payload, item, &mut sec);
            },
            _ => {},
        }
    }
    apply_security_flags(item, &sec);
}

/// ffWifiFetchScanInfo: the GET_SCAN dump looking for the associated BSS.
fn fetch_scan_info(ctx: &mut NlContext, item: &mut WifiItem, if_index: u32) -> bool {
    let family_id = ctx.family_id;
    let mut outcome = None;
    ctx.exchange(
        family_id,
        NLM_F_REQUEST | NLM_F_DUMP | NLM_F_ACK,
        NL80211_CMD_GET_SCAN,
        &[(NL80211_ATTR_IFINDEX, if_index.to_ne_bytes().to_vec())],
        1024 * 16,
        &mut |payload, msg_type| {
            if msg_type == NLMSG_DONE {
                outcome = Some(false);
                return Some(());
            }
            if msg_type == NLMSG_ERROR {
                if payload.len() >= 4 {
                    let error = read_i32(payload);
                    if error != 0 {
                        outcome = Some(false);
                        return Some(());
                    }
                }
                return None;
            }
            if msg_type != family_id || payload.len() < 4 {
                return None;
            }
            for attr in each_attr(&payload[4..]) {
                if attr.attr_type != NL80211_ATTR_BSS {
                    continue;
                }
                if !is_bss_associated(&attr) {
                    continue;
                }
                parse_bss_attr(&attr, item);
                item.status = "connected".to_string();
                outcome = Some(true);
                return Some(());
            }
            None
        },
    );
    outcome.unwrap_or(false)
}

/// ffWifiFetchInterfaceInfo: the GET_INTERFACE channel width query.
fn fetch_interface_info(ctx: &mut NlContext, item: &mut WifiItem, if_index: u32) -> bool {
    let family_id = ctx.family_id;
    let mut outcome = None;
    ctx.exchange(
        family_id,
        NLM_F_REQUEST | NLM_F_ACK,
        NL80211_CMD_GET_INTERFACE,
        &[(NL80211_ATTR_IFINDEX, if_index.to_ne_bytes().to_vec())],
        8192,
        &mut |payload, msg_type| {
            if msg_type == NLMSG_ERROR {
                outcome = Some(false);
                return Some(());
            }
            if msg_type != family_id || payload.len() < 4 {
                return None;
            }
            for attr in each_attr(&payload[4..]) {
                if attr.attr_type == NL80211_ATTR_CHANNEL_WIDTH && attr.payload.len() >= 4 {
                    let width = read_u32(attr.payload);
                    item.channel_width = channel_width_to_mhz(width);
                    outcome = Some(item.channel_width != 0);
                    return Some(());
                }
            }
            outcome = Some(false);
            Some(())
        },
    );
    outcome.unwrap_or(false)
}

/// ffWifiParseStationInfo.
fn parse_station_info(sta_info_attr: &NlAttrView, item: &mut WifiItem) {
    for attr in each_attr(sta_info_attr.payload) {
        match attr.attr_type {
            NL80211_STA_INFO_SIGNAL
                if !attr.payload.is_empty() && item.signal_quality.is_none() =>
            {
                let rssi = attr.payload[0] as i8 as i32;
                item.signal_quality = Some(rssi_to_quality(rssi));
            },
            NL80211_STA_INFO_TX_BITRATE if item.tx_rate.is_none() || item.channel_width == 0 => {
                if let Some(tx) =
                    parse_rate_info(&attr, &mut item.protocol, &mut item.channel_width)
                {
                    item.tx_rate = Some(tx);
                }
            },
            NL80211_STA_INFO_RX_BITRATE if item.rx_rate.is_none() || item.channel_width == 0 => {
                if let Some(rx) =
                    parse_rate_info(&attr, &mut item.protocol, &mut item.channel_width)
                {
                    item.rx_rate = Some(rx);
                }
            },
            _ => {},
        }
    }
}

/// ffWifiFetchStationInfo: the GET_STATION dump.
fn fetch_station_info(ctx: &mut NlContext, item: &mut WifiItem, if_index: u32) -> bool {
    let family_id = ctx.family_id;
    let mut got_station = false;
    ctx.exchange(
        family_id,
        NLM_F_REQUEST | NLM_F_DUMP | NLM_F_ACK,
        NL80211_CMD_GET_STATION,
        &[(NL80211_ATTR_IFINDEX, if_index.to_ne_bytes().to_vec())],
        8192,
        &mut |payload, msg_type| {
            if msg_type == NLMSG_DONE || msg_type == NLMSG_ERROR {
                return Some(());
            }
            if msg_type != family_id || payload.len() < 4 {
                return None;
            }
            for attr in each_attr(&payload[4..]) {
                if attr.attr_type == NL80211_ATTR_STA_INFO {
                    parse_station_info(&attr, item);
                    got_station = true;
                }
            }
            None
        },
    );
    got_station
}

/// detectWithNetlink.
fn detect_with_netlink(ctx: &mut NlContext, item: &mut WifiItem, if_index: u32) {
    if ctx.sock_fd < 0 {
        if ctx.sock_fd == NlContext::UNINITIALIZED {
            if !ctx.init() {
                ctx.sock_fd = NlContext::FAILED; // permanent failure sentinel
                return;
            }
        } else {
            return;
        }
    }

    if fetch_scan_info(ctx, item, if_index) {
        item.status = "connected".to_string();
        fetch_interface_info(ctx, item, if_index);
        fetch_station_info(ctx, item, if_index);
    } else {
        item.status = "disconnected".to_string();
    }
}

// ---- wireless extension ioctls ----

#[repr(C)]
#[derive(Clone, Copy)]
struct IwPoint {
    pointer: *mut c_void,
    length: u16,
    flags: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct IwParam {
    value: i32,
    fixed: u8,
    disabled: u8,
    flags: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct IwFreq {
    m: i32,
    e: u16,
    i: u8,
    flags: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SockaddrData {
    family: u16,
    data: [u8; 14],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct IwQuality {
    qual: u8,
    level: i8,
    noise: i8,
    updated: u8,
}

/// iw_statistics: status u16, qual at 2, u32-aligned discard at 8, miss at 28.
#[repr(C)]
struct IwStatistics {
    status: u16,
    qual: IwQuality,
    _pad: [u8; 2],
    discard: [u8; 20],
    miss: [u8; 4],
}

/// iw_encode_ext: ext_flags, tx_seq[8], rx_seq[8], sockaddr, alg, key_len.
#[repr(C)]
struct IwEncodeExt {
    ext_flags: u32,
    tx_seq: [u8; 8],
    rx_seq: [u8; 8],
    addr: SockaddrData,
    alg: u16,
    key_len: u16,
}

#[repr(C)]
union IwreqData {
    essid: IwPoint,
    name: IwParam,
    freq: IwFreq,
    bitrate: IwParam,
    ap_addr: SockaddrData,
    data: IwPoint,
    raw: [u8; 16],
}

#[repr(C)]
struct Iwreq {
    ifr_name: [c_char; 16],
    u: IwreqData,
}

struct IoctlContext {
    sock_fd: c_int,
}

impl IoctlContext {
    const UNINITIALIZED: c_int = -1;
    const FAILED: c_int = -2;

    fn socket_or_skip(&mut self) -> Option<c_int> {
        if self.sock_fd < 0 {
            if self.sock_fd != Self::UNINITIALIZED {
                return None;
            }
            // SAFETY: plain AF_INET datagram socket creation.
            let sock =
                unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
            if sock < 0 {
                self.sock_fd = Self::FAILED;
                return None;
            }
            self.sock_fd = sock;
        }
        Some(self.sock_fd)
    }
}

/// One wireless ioctl round trip: build the iwreq, run the ioctl, map the
/// result through `post`.
fn iw_ioctl<T>(
    fd: c_int,
    if_name: &str,
    request: libc::c_ulong,
    prepare: impl FnOnce(&mut Iwreq),
    post: impl FnOnce(&Iwreq) -> T,
) -> Option<T> {
    let mut iwr = Iwreq {
        ifr_name: [0; 16],
        u: IwreqData { raw: [0; 16] },
    };
    for (target, byte) in iwr.ifr_name.iter_mut().zip(if_name.as_bytes()) {
        *target = *byte as c_char;
    }
    prepare(&mut iwr);
    // SAFETY: the kernel writes into the iwreq's own union and the
    // caller-provided external buffers during the call.
    if unsafe { portable_ioctl(fd, request, &mut iwr as *mut Iwreq) } < 0 {
        return None;
    }
    Some(post(&iwr))
}

/// detectWithIoctl: fill detection gaps with wireless-extension ioctls.
fn detect_with_ioctl(ctx: &mut IoctlContext, item: &mut WifiItem, if_name: &str) {
    let Some(fd) = ctx.socket_or_skip() else {
        return;
    };

    if item.ssid.is_empty() {
        let mut essid = [0u8; IW_ESSID_MAX_SIZE + 1];
        let pointer = essid.as_mut_ptr().cast::<c_void>();
        let connected = iw_ioctl(
            fd,
            if_name,
            SIOCGIWESSID,
            |iwr| {
                iwr.u.essid = IwPoint {
                    pointer,
                    length: (IW_ESSID_MAX_SIZE + 1) as u16,
                    flags: 0,
                };
            },
            |_| {
                // SAFETY: the kernel wrote at most `length` bytes here.
                let bytes =
                    unsafe { std::slice::from_raw_parts(pointer as *const u8, essid.len()) };
                let len = bytes.iter().position(|&byte| byte == 0).unwrap_or(bytes.len());
                item.ssid = String::from_utf8_lossy(&bytes[..len]).into_owned();
                true
            },
        );
        if connected == Some(true) {
            item.status = "connected".to_string();
        }
    }

    if item.protocol.is_empty() {
        let protocol = iw_ioctl(
            fd,
            if_name,
            SIOCGIWNAME,
            |iwr| iwr.u.raw = [0; 16],
            |iwr| {
                // The kernel writes the driver name string into u.name's bytes.
                let token: &[u8] = unsafe { &iwr.u.raw };
                let mut token: &[u8] =
                    &token[..token.iter().position(|&b| b == 0).unwrap_or(token.len())];
                if token.starts_with(b"IEEE ") {
                    token = &token["IEEE ".len()..];
                }
                if let Some(rest) = token.strip_prefix(b"802.11") {
                    let mut rest = rest;
                    if rest.first() == Some(&b' ') {
                        rest = &rest[1..];
                    }
                    if !rest.is_empty() {
                        let suffix: String =
                            rest.iter().map(|byte| byte.to_ascii_lowercase() as char).collect();
                        return match suffix.as_str() {
                            "n" => "802.11n (Wi-Fi 4)".to_string(),
                            "ac" => "802.11ac (Wi-Fi 5)".to_string(),
                            "ax" => "802.11ax (Wi-Fi 6)".to_string(),
                            "be" => "802.11be (Wi-Fi 7)".to_string(),
                            "bn" => "802.11bn (Wi-Fi 8)".to_string(),
                            _ => format!("802.11{suffix}"),
                        };
                    }
                }
                String::new()
            },
        );
        if let Some(protocol) = protocol {
            item.protocol = protocol;
        }
    }

    if item.bssid.is_empty() {
        let bssid = iw_ioctl(
            fd,
            if_name,
            SIOCGIWAP,
            |iwr| {
                iwr.u.ap_addr = SockaddrData {
                    family: 0,
                    data: [0; 14],
                }
            },
            |iwr| {
                // SAFETY: reading the sockaddr the kernel wrote back.
                let ap: SockaddrData = unsafe { iwr.u.ap_addr };
                ap.data[..6]
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(":")
            },
        );
        if let Some(mut bssid) = bssid {
            bssid.pop(); // ffStrbufTrimRight(&bssid, ':')
            item.bssid = bssid;
        }
    }

    if item.tx_rate.is_none() {
        let tx_rate = iw_ioctl(
            fd,
            if_name,
            SIOCGIWRATE,
            |iwr| {
                iwr.u.bitrate = IwParam {
                    value: 0,
                    fixed: 0,
                    disabled: 0,
                    flags: 0,
                }
            },
            |iwr| {
                // SAFETY: reading the param the kernel wrote back.
                let bitrate: IwParam = unsafe { iwr.u.bitrate };
                bitrate.value
            },
        );
        if let Some(value) = tx_rate.filter(|value| *value > 0) {
            // bps => Mbps
            item.tx_rate = Some(value as f64 / 1_000_000.0);
        }
    }

    if item.frequency == 0 && item.channel == 0 {
        let freq = iw_ioctl(
            fd,
            if_name,
            SIOCGIWFREQ,
            |iwr| {
                iwr.u.freq = IwFreq {
                    m: 0,
                    e: 0,
                    i: 0,
                    flags: 0,
                }
            },
            |iwr| {
                // SAFETY: reading the freq the kernel wrote back.
                let freq: IwFreq = unsafe { iwr.u.freq };
                freq
            },
        );
        if let Some(freq) = freq {
            if freq.e == 0 && freq.m <= 1000 {
                // The kernel may return the direct channel number.
                item.channel = freq.m as u16;
            } else {
                // Normalize the exponent to 10^6 (MHz).
                let (mut m, mut e) = (freq.m, freq.e);
                while e < 6 {
                    m /= 10;
                    e += 1;
                }
                while e > 6 {
                    m *= 10;
                    e -= 1;
                }
                item.frequency = m as u16;
                item.channel = freq_to_channel(item.frequency);
            }
        }
    }

    if item.signal_quality.is_none() {
        let mut stats = IwStatistics {
            status: 0,
            qual: IwQuality {
                qual: 0,
                level: 0,
                noise: 0,
                updated: 0,
            },
            _pad: [0; 2],
            discard: [0; 20],
            miss: [0; 4],
        };
        let pointer = (&mut stats as *mut IwStatistics).cast::<c_void>();
        let quality = iw_ioctl(
            fd,
            if_name,
            SIOCGIWSTATS,
            |iwr| {
                iwr.u.data = IwPoint {
                    pointer,
                    length: std::mem::size_of::<IwStatistics>() as u16,
                    flags: 0,
                };
            },
            |_| {
                let level = stats.qual.level as i32;
                if level >= -50 {
                    100.0
                } else if level <= -100 {
                    0.0
                } else {
                    ((level + 100) * 2) as f64
                }
            },
        );
        if let Some(quality) = quality {
            item.signal_quality = Some(quality);
        }
    }

    if item.security.is_empty() {
        let mut ext = IwEncodeExt {
            ext_flags: 0,
            tx_seq: [0; 8],
            rx_seq: [0; 8],
            addr: SockaddrData {
                family: 0,
                data: [0; 14],
            },
            alg: 0,
            key_len: 0,
        };
        let pointer = (&mut ext as *mut IwEncodeExt).cast::<c_void>();
        let security = iw_ioctl(
            fd,
            if_name,
            SIOCGIWENCODEEXT,
            |iwr| {
                iwr.u.data = IwPoint {
                    pointer,
                    length: std::mem::size_of::<IwEncodeExt>() as u16,
                    flags: 0,
                };
            },
            |_| ext.alg,
        );
        if let Some(alg) = security {
            item.security = match alg {
                IW_ENCODE_ALG_WEP => "WEP".to_string(),
                IW_ENCODE_ALG_TKIP => "TKIP".to_string(),
                IW_ENCODE_ALG_CCMP => "CCMP".to_string(),
                IW_ENCODE_ALG_PMK => "PMK".to_string(),
                IW_ENCODE_ALG_AES_CMAC => "CMAC".to_string(),
                other => format!("Unknown ({other})"),
            };
        }
    }
}

// ---- assembly ----

/// ffDetectWifi.
fn detect() -> Vec<WifiItem> {
    let mut result = Vec::new();

    // SAFETY: if_nameindex returns a null-terminated list freed below.
    let infs = unsafe { libc::if_nameindex() };
    if infs.is_null() {
        return result;
    }

    let mut nl = NlContext {
        sock_fd: NlContext::UNINITIALIZED,
        family_id: 0,
        port_id: 0,
        seq: 0,
    };
    let mut ic = IoctlContext {
        sock_fd: IoctlContext::UNINITIALIZED,
    };

    unsafe {
        let mut cursor = infs;
        while (*cursor).if_index != 0 || !(*cursor).if_name.is_null() {
            let if_index = (*cursor).if_index;
            let if_name = CStr::from_ptr((*cursor).if_name).to_string_lossy().into_owned();

            // Not a wifi interface (no phy80211 directory).
            if !std::path::Path::new(&format!("/sys/class/net/{if_name}/phy80211")).is_dir() {
                cursor = cursor.add(1);
                continue;
            }

            let mut item =
                WifiItem { inf_description: if_name.clone(), ..WifiItem::default() };

            match util::read_file(&format!("/sys/class/net/{if_name}/operstate")) {
                Some(state) if !state.is_empty() && state.as_bytes()[0] == b'u' => {
                    item.inf_status = "up".to_string();
                    detect_with_netlink(&mut nl, &mut item, if_index);
                    detect_with_ioctl(&mut ic, &mut item, &if_name);
                },
                Some(state) if !state.is_empty() => {
                    item.status = "disconnected".to_string();
                    match util::read_file(&format!("/sys/class/net/{if_name}/flags")) {
                        Some(flags) => {
                            // /sys flags are hexadecimal.
                            let value =
                                u32::from_str_radix(flags.trim().trim_start_matches("0x"), 16)
                                    .unwrap_or(0);
                            item.inf_status =
                                if value & IFF_UP != 0 { "up" } else { "down" }.to_string();
                        },
                        None => item.inf_status = "unknown".to_string(),
                    }
                },
                _ => {
                    item.inf_status = "unknown".to_string();
                    item.status = "disconnected".to_string();
                },
            }

            result.push(item);
            cursor = cursor.add(1);
        }

        libc::if_freenameindex(infs);
        if nl.sock_fd >= 0 {
            libc::close(nl.sock_fd);
        }
        if ic.sock_fd >= 0 {
            libc::close(ic.sock_fd);
        }
    }

    result
}

/// The band label from the WLAN channel list (wikipedia).
fn band_str(frequency: u16) -> &'static str {
    if frequency > 58000 {
        "60"
    } else if frequency > 40000 {
        "45"
    } else if frequency > 5900 {
        "6"
    } else if frequency > 5100 {
        "5"
    } else if frequency > 4900 {
        "4.9"
    } else if frequency > 3600 {
        "3.65"
    } else if frequency > 2000 {
        "2.4"
    } else if frequency > 800 {
        "0.9"
    } else {
        ""
    }
}

/// The default-format value for one interface. The bar prints before the
/// ssid; the number's parentheses open only when the buffer is non-empty
/// (hidden by --percent-type hide-others with no bar, upstream prints a
/// bare number).
fn item_value(
    item: &WifiItem,
    percent: crate::format::PercentModuleConfig,
    ctx: &crate::format::PrintCtx,
    df: &crate::format::DisplayFormat,
) -> String {
    if !item.ssid.is_empty() {
        let percent_type_flags =
            if percent.type_flags == 0 { df.percent_type } else { percent.type_flags };
        let mut buffer = String::new();
        if let Some(quality) = item.signal_quality
            && percent_type_flags & crate::format::percent_type::BAR_BIT != 0
        {
            crate::format::percent_append_bar(&mut buffer, quality, percent, ctx, df);
            buffer.push(' ');
        }
        if percent_type_flags & crate::format::percent_type::HIDE_OTHERS_BIT == 0 {
            buffer.push_str(&item.ssid);
            if !item.protocol.is_empty() {
                buffer.push_str(" - ");
                buffer.push_str(&item.protocol);
            }
            let band = band_str(item.frequency);
            if !band.is_empty() {
                buffer.push_str(" - ");
                buffer.push_str(band);
                if df.freq_space_before_unit != crate::format::SpaceBeforeUnit::Never {
                    buffer.push(' ');
                }
                buffer.push_str("GHz");
            }
            if !item.security.is_empty() {
                buffer.push_str(" - ");
                buffer.push_str(&item.security);
            }
            buffer.push(' ');
        }
        if let Some(quality) = item.signal_quality
            && percent_type_flags & crate::format::percent_type::NUM_BIT != 0
        {
            let parentheses = !buffer.is_empty();
            crate::format::percent_append_num(&mut buffer, quality, percent, parentheses, ctx, df);
        }
        while buffer.ends_with(' ') {
            buffer.pop();
        }
        buffer
    } else {
        item.inf_status.clone()
    }
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let percent = super::percent_config(
        instance.entry,
        crate::format::PercentModuleConfig { green: 75, yellow: 50, type_flags: 0 },
    );
    let ctx = super::print_ctx(instance);
    let items = detect();
    if items.is_empty() {
        // "No Wifi interfaces found" is an error row, hidden by default.
        return Vec::new();
    }
    let single = items.len() == 1;
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let key = if single {
                "Wi-Fi".to_string()
            } else {
                format!("{} {}", instance.display_name("wifi"), index + 1)
            };
            Row::new(&key, item_value(&item, percent, &ctx, &instance.display_format))
                .with_index(if single { 0 } else { index as u32 + 1 })
        })
        .collect()
}

/// ffGenerateWifiJsonResult; rates and quality carry the -DBL_MAX sentinel
/// as null.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let mut arr = Json::Array(Vec::new());
    for item in &detect() {
        let mut obj = Json::obj();

        let mut inf = Json::obj();
        inf.push("description", item.inf_description.clone());
        inf.push("status", item.inf_status.clone());
        obj.push("inf", inf);

        let mut conn = Json::obj();
        conn.push("status", item.status.clone());
        conn.push("ssid", item.ssid.clone());
        conn.push("bssid", item.bssid.clone());
        conn.push("protocol", item.protocol.clone());
        conn.push("security", item.security.clone());
        match item.signal_quality {
            Some(quality) => conn.push("signalQuality", quality),
            None => conn.push("signalQuality", Json::Null),
        }
        match item.rx_rate {
            Some(rate) => conn.push("rxRate", rate),
            None => conn.push("rxRate", Json::Null),
        }
        match item.tx_rate {
            Some(rate) => conn.push("txRate", rate),
            None => conn.push("txRate", Json::Null),
        }
        conn.push("channel", item.channel as u64);
        conn.push("channelWidth", item.channel_width as u64);
        conn.push("frequency", item.frequency as u64);
        obj.push("conn", conn);

        arr.push_value(obj);
    }
    JsonOutcome::Result(arr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freq_to_channel_matches_upstream() {
        assert_eq!(freq_to_channel(2412), 1);
        assert_eq!(freq_to_channel(2472), 13);
        assert_eq!(freq_to_channel(2484), 14);
        assert_eq!(freq_to_channel(5180), 36);
        assert_eq!(freq_to_channel(5805), 161);
        assert_eq!(freq_to_channel(5935), 2);
        assert_eq!(freq_to_channel(5955), 1);
        assert_eq!(freq_to_channel(7115), 233);
        assert_eq!(freq_to_channel(3000), 0);
    }

    #[test]
    fn channel_widths_map_to_mhz() {
        assert_eq!(channel_width_to_mhz(0), 20);
        assert_eq!(channel_width_to_mhz(1), 20);
        assert_eq!(channel_width_to_mhz(3), 80);
        assert_eq!(channel_width_to_mhz(4), 80);
        assert_eq!(channel_width_to_mhz(13), 320);
        assert_eq!(channel_width_to_mhz(14), 0);
    }

    #[test]
    fn rssi_quality_clamps_linearly() {
        assert_eq!(rssi_to_quality(-40), 100.0);
        assert_eq!(rssi_to_quality(-50), 100.0);
        assert_eq!(rssi_to_quality(-51), 98.0);
        assert_eq!(rssi_to_quality(-75), 50.0);
        assert_eq!(rssi_to_quality(-100), 0.0);
        assert_eq!(rssi_to_quality(-101), 0.0);
    }

    #[test]
    fn band_labels() {
        assert_eq!(band_str(5180), "5");
        assert_eq!(band_str(2437), "2.4");
        assert_eq!(band_str(60000), "60");
        assert_eq!(band_str(0), "");
    }

    #[test]
    fn security_flags_render_like_upstream() {
        let mut item = WifiItem::default();
        apply_security_flags(
            &mut item,
            &SecurityFlags {
                privacy: true,
                wpa2: true,
                ..Default::default()
            },
        );
        assert_eq!(item.security, "WPA2");
        let mut item = WifiItem::default();
        apply_security_flags(
            &mut item,
            &SecurityFlags {
                privacy: true,
                ..Default::default()
            },
        );
        assert_eq!(item.security, "WEP");
        let mut item = WifiItem::default();
        apply_security_flags(&mut item, &SecurityFlags::default());
        assert_eq!(item.security, "Insecure");
        let mut item = WifiItem::default();
        apply_security_flags(
            &mut item,
            &SecurityFlags {
                wep: true,
                wpa: true,
                wpa3: true,
                eap: true,
                ..Default::default()
            },
        );
        assert_eq!(item.security, "WEP/WPA/WPA3/802.1X");
    }

    #[test]
    fn rsn_and_wpa_ies_are_recognized() {
        let mut sec = SecurityFlags::default();
        // RSN IE: version 1, group cipher CCMP, 1 pairwise, 1 AKM = SAE (8).
        let rsn = [
            1, 0, // version
            0x00, 0x0f, 0xac, 0x04, // group cipher CCMP
            1, 0, // pairwise count
            0x00, 0x0f, 0xac, 0x04, // pairwise CCMP
            1, 0, // AKM count
            0x00, 0x0f, 0xac, 0x08, // SAE
        ];
        parse_rsn_ie(&rsn, &mut sec);
        assert!(sec.wpa3);
        assert!(!sec.eap);

        let mut sec = SecurityFlags::default();
        // WPA vendor IE: OUI 00:50:f2 type 1, version 1, multicast cipher
        // TKIP, 1 unicast, 1 AKM = 802.1X.
        let wpa = [
            0x00, 0x50, 0xf2, 0x01, // OUI + type
            1, 0, // version
            0x00, 0x50, 0xf2, 0x02, // multicast TKIP
            1, 0, // unicast count
            0x00, 0x50, 0xf2, 0x02, 1, 0, // AKM count
            0x00, 0x50, 0xf2, 0x01, // 802.1X
        ];
        parse_wpa_vendor_ie(&wpa, &mut sec);
        assert!(sec.wpa);
        assert!(sec.eap);
    }

    #[test]
    fn value_assembles_like_the_default_format() {
        let item = WifiItem {
            inf_status: "up".into(),
            status: "connected".into(),
            ssid: "home".into(),
            protocol: "802.11ac (Wi-Fi 5)".into(),
            security: "WPA2".into(),
            signal_quality: Some(52.0),
            frequency: 5180,
            ..Default::default()
        };
        let percent = crate::format::PercentModuleConfig { green: 75, yellow: 50, type_flags: 0 };
        let ctx = crate::format::PrintCtx { pipe: true, output_color: String::new() };
        assert_eq!(
            item_value(&item, percent, &ctx, &Default::default()),
            "home - 802.11ac (Wi-Fi 5) - 5 GHz - WPA2 (52%)"
        );
        let disconnected = WifiItem {
            inf_status: "down".into(),
            ..Default::default()
        };
        assert_eq!(item_value(&disconnected, percent, &ctx, &Default::default()), "down");
        // No security: no trailing separator.
        let open = WifiItem {
            ssid: "cafe".into(),
            signal_quality: Some(100.0),
            ..Default::default()
        };
        assert_eq!(item_value(&open, percent, &ctx, &Default::default()), "cafe (100%)");
    }

    #[test]
    fn attr_iteration_is_aligned_and_bounded() {
        // attr(len 6, payload [1,2] + pad), attr(len 4, empty), then a
        // truncated header that stops the walk.
        let data = [6u8, 0, 5, 0, 1, 2, 0, 0, 4, 0, 7, 0, 9];
        let attrs: Vec<_> = each_attr(&data).collect();
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0].payload, &[1, 2]);
        assert_eq!(attrs[1].payload, &[]);
    }

    #[test]
    fn protocol_name_mapping() {
        // The ioctl protocol mapping: IEEE prefix, 802.11 prefix, lowercase.
        let map = |name: &[u8]| -> String {
            let mut token: &[u8] = name;
            if token.starts_with(b"IEEE ") {
                token = &token["IEEE ".len()..];
            }
            if let Some(rest) = token.strip_prefix(b"802.11") {
                let mut rest = rest;
                if rest.first() == Some(&b' ') {
                    rest = &rest[1..];
                }
                if !rest.is_empty() {
                    let suffix: String =
                        rest.iter().map(|byte| byte.to_ascii_lowercase() as char).collect();
                    return match suffix.as_str() {
                        "n" => "802.11n (Wi-Fi 4)".to_string(),
                        "ac" => "802.11ac (Wi-Fi 5)".to_string(),
                        "ax" => "802.11ax (Wi-Fi 6)".to_string(),
                        "be" => "802.11be (Wi-Fi 7)".to_string(),
                        "bn" => "802.11bn (Wi-Fi 8)".to_string(),
                        _ => format!("802.11{suffix}"),
                    };
                }
            }
            String::new()
        };
        assert_eq!(map(b"IEEE 802.11AX"), "802.11ax (Wi-Fi 6)");
        assert_eq!(map(b"IEEE 802.11ac"), "802.11ac (Wi-Fi 5)");
        assert_eq!(map(b"802.11ad"), "802.11ad");
        assert_eq!(map(b"IEEE 802.11"), "");
    }
}

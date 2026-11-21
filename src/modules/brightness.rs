//! `brightness` module: monitor brightness, ported from brightness_linux.c.
//! Backlight devices under /sys/class/backlight first; when they cover fewer
//! monitors than the display server reports, external displays are probed
//! over DDC/CI through /dev/i2c-*. The probed bus numbers are cached in the
//! same file fastfetch uses (~/.cache/fastfetch/brightness/i2c_cache.txt).

use std::path::PathBuf;

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

const I2C_RDWR: libc::c_ulong = 0x0707;
const I2C_M_RD: u16 = 0x0001;
const DDC_EDID_ADDR: u16 = 0x50;
const DDC_CI_ADDR: u16 = 0x37;
const DDC_CI_WRITE_ADDR: u16 = DDC_CI_ADDR << 1;
const DDC_CI_RESPONSE_CHECKSUM_ADDR: u8 = 0x50;
const VCP_RESPONSE_BUFFER_SIZE: usize = 12;

#[repr(C)]
struct I2cMsg {
    addr: u16,
    flags: u16,
    len: u16,
    buf: *mut u8,
}

#[repr(C)]
struct I2cRdwrIoctlData {
    msgs: *mut I2cMsg,
    nmsgs: u32,
}

/// XOR checksum over the (write) address and payload.
fn ddc_checksum(address: u8, data: &[u8]) -> u8 {
    data.iter().fold(address, |checksum, &byte| checksum ^ byte)
}

/// GetVCP Feature 0x10 (luminance) request.
fn build_get_vcp_request() -> [u8; 5] {
    let mut request = [0u8; 5];
    request[0] = 0x51; // VCP_COMMAND
    request[1] = 0x80 | 2; // MAKE_HEADER(2)
    request[2] = 0x01; // GET_VCP
    request[3] = 0x10; // LUMINANCE_OPCODE
    request[4] = ddc_checksum(DDC_CI_WRITE_ADDR as u8, &request[..4]);
    request
}

fn validate_get_vcp_response(response: &[u8; VCP_RESPONSE_BUFFER_SIZE]) -> bool {
    if response[0] != DDC_CI_WRITE_ADDR as u8
        || response[1] != (0x80 | 8)
        || response[2] != 0x02
        || response[3] != 0x00
        || response[4] != 0x10
        || response[5] != 0x00
    {
        return false;
    }
    ddc_checksum(DDC_CI_RESPONSE_CHECKSUM_ADDR, &response[..10]) == response[10]
}

unsafe fn i2c_rdwr(fd: libc::c_int, msgs: &mut [I2cMsg]) -> bool {
    let mut data = I2cRdwrIoctlData {
        msgs: msgs.as_mut_ptr(),
        nmsgs: msgs.len() as u32,
    };
    (unsafe { libc::ioctl(fd, I2C_RDWR, &mut data as *mut I2cRdwrIoctlData) }) == msgs.len() as i32
}

/// One DDC/CI GetVCP round trip: write the request, pause, read the reply.
unsafe fn read_ddc_response(fd: libc::c_int) -> Option<[u8; VCP_RESPONSE_BUFFER_SIZE]> {
    let request = build_get_vcp_request();

    let mut write_msg = I2cMsg {
        addr: DDC_CI_ADDR,
        flags: 0,
        len: request.len() as u16,
        buf: request.as_ptr() as *mut u8,
    };
    if !unsafe { i2c_rdwr(fd, std::slice::from_mut(&mut write_msg)) } {
        return None;
    }

    std::thread::sleep(std::time::Duration::from_millis(10));

    let mut response = [0u8; VCP_RESPONSE_BUFFER_SIZE];
    let mut read_msg = I2cMsg {
        addr: DDC_CI_ADDR,
        flags: I2C_M_RD,
        len: response.len() as u16,
        buf: response.as_mut_ptr(),
    };
    if !unsafe { i2c_rdwr(fd, std::slice::from_mut(&mut read_msg)) } {
        return None;
    }
    validate_get_vcp_response(&response).then_some(response)
}

/// EDID over DDC: set the offset byte, then read 128 bytes.
unsafe fn read_edid(fd: libc::c_int) -> Option<[u8; 128]> {
    let mut offset = [0u8];
    let mut edid = [0u8; 128];
    let mut msgs = [
        I2cMsg {
            addr: DDC_EDID_ADDR,
            flags: 0,
            len: 1,
            buf: offset.as_mut_ptr(),
        },
        I2cMsg {
            addr: DDC_EDID_ADDR,
            flags: I2C_M_RD,
            len: edid.len() as u16,
            buf: edid.as_mut_ptr(),
        },
    ];
    unsafe { i2c_rdwr(fd, &mut msgs) }.then_some(edid)
}

/// ffEdidGetName: the model-name descriptor block, else vendor+model id.
fn edid_name(edid: &[u8; 128]) -> String {
    let mut i = 0x36;
    while i < 0x7E {
        if edid[i] == 0 && edid[i + 3] == 0xFC {
            let mut j = 0;
            while j < 13 {
                if edid[i + 5 + j] == 0x0A {
                    return String::from_utf8_lossy(&edid[i + 5..i + 5 + j]).into_owned();
                }
                j += 1;
            }
        }
        i += 0x12;
    }

    // Vendor + model: three letters from the PNP id bits and the model hex.
    let vendor: [char; 3] = [
        ((((edid[8] >> 2) & 0x1F) as u16) + b'A' as u16 - 1) as u8 as char,
        ((((((edid[8] & 0x3) as u16) << 3) | (((edid[9] & 0xE0) as u16) >> 5)) + b'A' as u16 - 1)
            as u8) as char,
        ((((edid[9] & 0x1F) as u16) + b'A' as u16 - 1) as u8) as char,
    ];
    let model = edid[10] as u32 + ((edid[11] as u32) << 8);
    format!("{}{:04X}", vendor.iter().collect::<String>(), model)
}

struct Brightness {
    name: String,
    current: f64,
    max: f64,
    /// The backlight and DDC/CI backends both report a zero minimum.
    min: f64,
    builtin: bool,
}

fn detect_with_backlight(result: &mut Vec<Brightness>) {
    let Ok(entries) = std::fs::read_dir("/sys/class/backlight") else {
        return;
    };
    for entry in entries.flatten() {
        let entry_name = entry.file_name().to_string_lossy().into_owned();
        if entry_name.starts_with('.') {
            continue;
        }
        let Some(current) =
            util::read_trimmed_in(&entry.path(), "brightness").and_then(|v| v.parse::<f64>().ok())
        else {
            continue;
        };
        let Some(max) = util::read_trimmed_in(&entry.path(), "max_brightness")
            .and_then(|v| v.parse::<f64>().ok())
        else {
            continue;
        };

        // Monitor name: the EDID behind the device, else the DRM connector
        // name (cardN-XYZ → XYZ), else the sysfs entry name.
        let mut name = entry_name.clone();
        if let Ok(device) = std::fs::canonicalize(entry.path().join("device")) {
            let named = std::fs::read(device.join("edid"))
                .ok()
                .filter(|data| data.len() >= 128)
                .map(|data| {
                    let mut edid = [0u8; 128];
                    edid.copy_from_slice(&data[..128]);
                    edid_name(&edid)
                });
            name = match named {
                Some(name) => name,
                None => {
                    let connector =
                        device.to_string_lossy().rsplit('/').next().unwrap_or("").to_string();
                    if connector.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                        entry_name.clone()
                    } else if connector.starts_with("card")
                        && connector.chars().nth(4).is_some_and(|c| c.is_ascii_digit())
                    {
                        connector
                            .split_once('-')
                            .map_or(connector.clone(), |(_, rest)| rest.to_string())
                    } else {
                        connector
                    }
                },
            };
        }
        result.push(Brightness {
            name,
            current,
            max,
            min: 0.0,
            builtin: true,
        });
    }
}

/// The bus numbers probed successfully last time, shared with fastfetch's
/// own cache file.
fn cache_path() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CACHE_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".cache"),
    };
    Some(base.join("fastfetch/brightness/i2c_cache.txt"))
}

fn load_cache() -> Vec<i32> {
    let Some(path) = cache_path() else {
        return Vec::new();
    };
    let Some(content) = util::read_file(path.to_string_lossy().as_ref()) else {
        return Vec::new();
    };
    let mut tokens = content.split_whitespace();
    let Some(count) = tokens.next().and_then(|c| c.parse::<usize>().ok()) else {
        return Vec::new();
    };
    let devices: Vec<i32> = tokens
        .filter_map(|dev| dev.parse::<i32>().ok())
        .filter(|dev| (0..100).contains(dev))
        .take(count)
        .collect();
    if devices.len() == count {
        devices
    } else {
        Vec::new()
    }
}

fn save_cache(devices: &[i32]) {
    if devices.is_empty() {
        return;
    }
    let Some(path) = cache_path() else {
        return;
    };
    let content = format!(
        "{} {}",
        devices.len(),
        devices.iter().map(|dev| dev.to_string()).collect::<Vec<_>>().join(" ")
    );
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, content);
}

/// Scan /dev/i2c-0..99 for displays answering the luminance GetVCP.
fn scan_i2c_devices() -> Vec<i32> {
    let mut devices = Vec::new();
    for bus in 0..100i32 {
        let Some(fd) = open_bus(bus) else {
            continue;
        };
        let compatible = unsafe { read_ddc_response(fd) }.is_some();
        unsafe { libc::close(fd) };
        if compatible {
            devices.push(bus);
        }
    }
    devices
}

fn open_bus(bus: i32) -> Option<libc::c_int> {
    let cpath = std::ffi::CString::new(format!("/dev/i2c-{bus}")).ok()?;
    let fd = unsafe { libc::open(cpath.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
    (fd >= 0).then_some(fd)
}

/// Probe one cached/scan bus; a valid luminance response adds a result.
unsafe fn probe_bus(bus: i32, result: &mut Vec<Brightness>) -> bool {
    let Some(fd) = open_bus(bus) else {
        return false;
    };
    let probe = (|| {
        let response = unsafe { read_ddc_response(fd)? };
        let current = ((response[8] as u32) << 8) + response[9] as u32;
        let max = ((response[6] as u32) << 8) + response[7] as u32;
        if max == 0 || current > max {
            return None;
        }
        let name = unsafe { read_edid(fd) }
            .map(|edid| edid_name(&edid))
            .unwrap_or_else(|| format!("I2C-{bus}"));
        Some(Brightness {
            name,
            current: current as f64,
            max: max as f64,
            min: 0.0,
            builtin: false,
        })
    })();
    unsafe { libc::close(fd) };
    match probe {
        Some(display) => {
            result.push(display);
            true
        },
        None => false,
    }
}

fn detect_with_ddcci(result: &mut Vec<Brightness>) -> bool {
    let mut devices = load_cache();
    if devices.is_empty() {
        devices = scan_i2c_devices();
    }
    let mut validated: Vec<i32> = Vec::new();
    for &bus in &devices {
        if unsafe { probe_bus(bus, result) } {
            validated.push(bus);
        }
    }

    if validated.is_empty() {
        // The cache can outlive the monitor it was built for: rescan once.
        devices = scan_i2c_devices();
        for &bus in &devices {
            if unsafe { probe_bus(bus, result) } {
                validated.push(bus);
            }
        }
    }

    if !validated.is_empty() {
        save_cache(&validated);
    }
    !validated.is_empty()
}

/// ffDetectBrightness: backlight entries, then DDC/CI when they cover fewer
/// monitors than the display server reports.
fn detect() -> Vec<Brightness> {
    let mut result = Vec::new();
    detect_with_backlight(&mut result);
    if result.len() < super::display::display_count() {
        detect_with_ddcci(&mut result);
    }
    result
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let percent = super::percent_config(instance.entry, crate::format::PercentModuleConfig { green: 100, yellow: 100, type_flags: 0 });
    let ctx = super::print_ctx(instance);
    let df = &instance.display_format;
    let percent_type_flags = if percent.type_flags == 0 { df.percent_type } else { percent.type_flags };
    let result = detect();

    if result.is_empty() {
        // ffPrintError is gated behind --show-errors, off by default.
        return Vec::new();
    }

    result
        .iter()
        .enumerate()
        .map(|(i, display)| {
            // Brightness hardcodes green=100/yellow=100: the number is
            // green (red only past 100, impossible here).
            let pct = display.current / display.max * 100.0;
            let mut value = String::new();
            if percent_type_flags & crate::format::percent_type::BAR_BIT != 0 {
                crate::format::percent_append_bar(&mut value, pct, percent, &ctx, df);
            }
            if percent_type_flags & crate::format::percent_type::NUM_BIT != 0 {
                if !value.is_empty() {
                    value.push(' ');
                }
                let parentheses = !value.is_empty();
                crate::format::percent_append_num(&mut value, pct, percent, parentheses, &ctx, df);
            }
            value.push_str(if display.builtin { " [Built-in]" } else { " [External]" });
            Row::new(
                &format!("{} ({})", instance.display_name("brightness"), display.name),
                value,
            )
            .with_key_args(vec![
                crate::modules::RowKeyArg::uint("index", crate::modules::numbered_index(i, result.len())),
                crate::modules::RowKeyArg::text("name", display.name.clone()),
            ])
        })
        .collect()
}

/// ffGenerateBrightnessJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let result = detect();
    let mut arr = Json::Array(Vec::new());
    for display in &result {
        let mut obj = Json::obj();
        obj.push("name", display.name.clone());
        obj.push("max", display.max);
        obj.push("min", display.min);
        obj.push("current", display.current);
        obj.push("builtin", display.builtin);
        arr.push_value(obj);
    }
    JsonOutcome::Result(arr)
}

#[cfg(test)]
mod tests {
    #[test]
    fn builds_and_validates_ddc_frames() {
        let request = super::build_get_vcp_request();
        assert_eq!(request[..4], [0x51, 0x82, 0x01, 0x10]);
        assert_eq!(request[4], super::ddc_checksum(0x6E, &request[..4]));

        let mut response = [0u8; 12];
        response[0] = 0x6E;
        response[1] = 0x80 | 8;
        response[2] = 0x02;
        response[3] = 0x00;
        response[4] = 0x10;
        response[5] = 0x00;
        response[6] = 0x02; // max = 0x0201 = 513
        response[7] = 0x01;
        response[8] = 0x00; // current = 0x00FE = 254
        response[9] = 0xFE;
        response[10] = super::ddc_checksum(0x50, &response[..10]);
        assert!(super::validate_get_vcp_response(&response));

        response[10] ^= 1;
        assert!(!super::validate_get_vcp_response(&response));
    }

    #[test]
    fn reads_edid_model_names() {
        let mut edid = [0u8; 128];
        // Monitor descriptor at 0x36, tag 0xFC at +3, name at +5.
        edid[0x36] = 0;
        edid[0x39] = 0xFC;
        edid[0x3B] = b'X';
        edid[0x3C] = b'V';
        edid[0x3D] = b'2';
        edid[0x3E] = b'7';
        edid[0x3F] = b'1';
        edid[0x40] = b'U';
        edid[0x41] = 0x0A;
        assert_eq!(super::edid_name(&edid), "XV271U");

        // "ABC" vendor, model 0.
        let mut edid = [0u8; 128];
        edid[8] = 0x04;
        edid[9] = 0x43;
        assert_eq!(super::edid_name(&edid), "ABC0000");
    }
}

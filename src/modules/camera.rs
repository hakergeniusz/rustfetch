//! `camera` module: video capture devices, ported from camera_linux.c.
//! /dev/video0..9 are opened and queried with the VIDIOC_QUERYCAP and
//! VIDIOC_G_FMT ioctls; only devices with the capture capability are listed.

use std::ffi::CString;

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

const VIDIOC_QUERYCAP: libc::c_ulong = 0x8068_5600;
const VIDIOC_G_FMT: libc::c_ulong = 0xc0d0_5604;
const V4L2_BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
const V4L2_CAP_VIDEO_CAPTURE: u32 = 0x0000_0001;

/// struct v4l2_capability as of videodev2.h (104 bytes).
#[repr(C)]
struct V4l2Capability {
    driver: [u8; 16],
    card: [u8; 32],
    bus_info: [u8; 32],
    kernel_version: u32,
    capabilities: u32,
    device_caps: u32,
    reserved: [u32; 3],
}

/// struct v4l2_format (208 bytes): type, then the pixel format union member.
#[repr(C)]
struct V4l2Format {
    type_: u32,
    _padding: u32,
    pix: [u8; 200],
}

/// fmt.fmt.pix width/height/colorspace offsets inside the union member.
fn pix_u32(fmt: &V4l2Format, offset: usize) -> u32 {
    u32::from_le_bytes([
        fmt.pix[offset],
        fmt.pix[offset + 1],
        fmt.pix[offset + 2],
        fmt.pix[offset + 3],
    ])
}

fn colorspace_name(colorspace: u32) -> &'static str {
    match colorspace {
        1 => "SMPTE 170M",
        2 => "SMPTE 240M",
        4 => "BT.878",
        5 => "NTSC",
        6 => "EBU 3213",
        7 => "JPEG",
        3 | 8 => "sRGB",
        9 => "Adobe RGB",
        10 => "BT.2020",
        11 => "RAW",
        12 => "DCI-P3",
        _ => "",
    }
}

fn c_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

pub struct CameraDevice {
    pub name: String,
    pub id: String,
    pub colorspace: String,
    pub width: u32,
    pub height: u32,
}

fn detect() -> Result<Vec<CameraDevice>, &'static str> {
    let mut result = Vec::new();
    for i in 0..=9u8 {
        let path = format!("/dev/video{i}");
        let Ok(cpath) = CString::new(path.clone()) else {
            break;
        };
        let fd = unsafe { libc::open(cpath.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
        if fd < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if errno == libc::ENOENT {
                break;
            }
            if errno == libc::ENXIO {
                continue; // the node exists but no driver is bound
            }
            return Err("Failed to open /dev/videoN");
        }

        let mut cap = V4l2Capability {
            driver: [0; 16],
            card: [0; 32],
            bus_info: [0; 32],
            kernel_version: 0,
            capabilities: 0,
            device_caps: 0,
            reserved: [0; 3],
        };
        let mut fmt = V4l2Format {
            type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
            _padding: 0,
            pix: [0; 200],
        };
        let captured = unsafe { portable_ioctl(fd, VIDIOC_QUERYCAP, &mut cap as *mut _) } >= 0
            && cap.capabilities & V4L2_CAP_VIDEO_CAPTURE != 0
            && unsafe { portable_ioctl(fd, VIDIOC_G_FMT, &mut fmt as *mut _) } >= 0;
        unsafe { libc::close(fd) };
        if !captured {
            continue;
        }

        result.push(CameraDevice {
            id: c_string(&cap.bus_info),
            colorspace: colorspace_name(pix_u32(&fmt, 24)).to_string(),
            width: pix_u32(&fmt, 0),
            height: pix_u32(&fmt, 4),
            name: c_string(&cap.card),
        });
    }
    Ok(result)
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(cameras) = detect() else {
        return Vec::new();
    };
    if cameras.is_empty() {
        // "No camera found" is an error row, hidden by default.
        return Vec::new();
    }

    cameras
        .iter()
        .enumerate()
        .map(|(index, dev)| {
            let name = &dev.name;
            let colorspace = &dev.colorspace;
            let (width, height) = (dev.width, dev.height);
            let index = if cameras.len() == 1 { 0 } else { index + 1 };
            let key = if index == 0 {
                "Camera".to_string()
            } else {
                format!("{} {index}", instance.display_name("camera"))
            };
            let mut value = name.clone();
            if !colorspace.is_empty() {
                value.push_str(&format!(" - {colorspace}"));
            }
            if width > 0 && height > 0 {
                value.push_str(&format!(" ({width}x{height} px)"));
            }
            Row::new(&key, value).with_index(index as u32)
        })
        .collect()
}



/// ffGenerateCameraJsonResult: vendor is never filled on Linux detection.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(cameras) => {
            let mut arr = Json::Array(Vec::new());
            for dev in &cameras {
                let mut obj = Json::obj();
                obj.push("name", dev.name.clone());
                obj.push("vendor", "");
                obj.push("colorSpace", dev.colorspace.clone());
                obj.push("id", dev.id.clone());
                obj.push("width", dev.width as u64);
                obj.push("height", dev.height as u64);
                arr.push_value(obj);
            }
            JsonOutcome::Result(arr)
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn maps_colorspace_names() {
        assert_eq!(super::colorspace_name(1), "SMPTE 170M");
        assert_eq!(super::colorspace_name(8), "sRGB");
        assert_eq!(super::colorspace_name(9), "Adobe RGB");
        assert_eq!(super::colorspace_name(0), "");
    }

    #[test]
    fn truncates_c_strings() {
        let bytes = b"USB Camera\0rest";
        assert_eq!(super::c_string(bytes), "USB Camera");
    }
}

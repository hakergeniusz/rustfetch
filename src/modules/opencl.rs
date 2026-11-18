//! `opencl` module: the OpenCL platform version, ported from
//! detection/opencl/opencl.c. libOpenCL is dlopened and the newest
//! platform's CL_PLATFORM_VERSION string is printed without its "OpenCL "
//! prefix ("3.0 CUDA"). The per-GPU details upstream collects only feed
//! custom format strings, which this port does not implement.

use std::ffi::{c_int, c_uint, c_void};

use crate::{instance::Instance, modules::Row};

const CL_SUCCESS: c_int = 0;
const CL_PLATFORM_VERSION: c_uint = 0x0901;

type ClGetPlatformIDsFn = unsafe extern "C" fn(c_uint, *mut *mut c_void, *mut c_uint) -> c_int;
type ClGetPlatformInfoFn =
    unsafe extern "C" fn(*mut c_void, c_uint, usize, *mut c_void, *mut usize) -> c_int;

/// The newest platform version wins, compared as raw strings.
fn pick_newest(current: &str, candidate: &str) -> bool {
    candidate > current
}

/// Strips a case-insensitive "OpenCL " prefix and the surrounding spaces.
fn version_pretty(buffer: &str) -> &str {
    let mut text = buffer;
    if text.get(..7).is_some_and(|prefix| prefix.eq_ignore_ascii_case("OpenCL ")) {
        text = &text[7..];
    }
    text.trim_matches(' ')
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(library) = super::codec::load_library("OpenCL", 1) else {
        // "libOpenCL.so.1 failed" is an error row, hidden by default.
        return Vec::new();
    };
    let Some(get_platform_ids) = super::codec::load_symbol(library, "clGetPlatformIDs") else {
        return Vec::new();
    };
    let Some(get_platform_info) = super::codec::load_symbol(library, "clGetPlatformInfo") else {
        return Vec::new();
    };
    let get_platform_ids: ClGetPlatformIDsFn = unsafe { std::mem::transmute(get_platform_ids) };
    let get_platform_info: ClGetPlatformInfoFn = unsafe { std::mem::transmute(get_platform_info) };

    // SAFETY: the handles outlive the copied symbols and are never closed.
    unsafe {
        let mut platforms: [*mut c_void; 32] = [std::ptr::null_mut(); 32];
        let mut num_platforms = 0;
        if (get_platform_ids)(32, platforms.as_mut_ptr(), &mut num_platforms) != CL_SUCCESS {
            return Vec::new();
        }

        let mut version = String::new();
        let mut buffer = [0u8; 1024];
        for platform in platforms.iter().take(num_platforms as usize) {
            let mut size = 0usize;
            if (get_platform_info)(
                *platform,
                CL_PLATFORM_VERSION,
                buffer.len(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            ) != CL_SUCCESS
            {
                // "clGetPlatformInfo() failed" is an error row, hidden by default.
                return Vec::new();
            }
            let len = size.saturating_sub(1).min(buffer.len());
            let text = String::from_utf8_lossy(&buffer[..len]).into_owned();
            if version.is_empty() || pick_newest(&version, &text) {
                version = version_pretty(&text).to_string();
            }
        }
        if version.is_empty() {
            // "clGetPlatformIDs returned 0 platforms" is an error row, hidden
            // by default.
            return Vec::new();
        }
        vec![Row::new("OpenCL", version)]
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn picks_the_newest_version_string() {
        assert!(super::pick_newest("3.0", "3.1 CUDA"));
        assert!(!super::pick_newest("3.1 CUDA", "2.1 OpenCL"));
    }

    #[test]
    fn strips_prefix_and_spaces() {
        assert_eq!(super::version_pretty("OpenCL 3.0 CUDA"), "3.0 CUDA");
        assert_eq!(super::version_pretty("opencl 2.1"), "2.1");
        assert_eq!(super::version_pretty("1.2"), "1.2");
        assert_eq!(super::version_pretty("OpenCL  3.0 "), "3.0");
    }
}

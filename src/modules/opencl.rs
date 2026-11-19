//! `opencl` module: the OpenCL platform and its GPU devices, ported from
//! detection/opencl/opencl.c. libOpenCL is dlopened; the newest platform's
//! CL_PLATFORM_VERSION string (without its "OpenCL " prefix) forms the
//! version and every CL_DEVICE_TYPE_GPU device of every platform joins the
//! result, with vendor id, compute units, clock and memory filled in.

use std::ffi::{c_int, c_uint, c_ulong, c_void};

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::{Row, gpu::GpuResult},
};

const CL_SUCCESS: c_int = 0;
const CL_PLATFORM_VERSION: c_uint = 0x0901;
const CL_PLATFORM_NAME: c_uint = 0x0902;
const CL_PLATFORM_VENDOR: c_uint = 0x0903;
const CL_DEVICE_VENDOR_ID: c_uint = 0x1001;
const CL_DEVICE_MAX_COMPUTE_UNITS: c_uint = 0x1002;
const CL_DEVICE_MAX_CLOCK_FREQUENCY: c_uint = 0x100C;
const CL_DEVICE_GLOBAL_MEM_SIZE: c_uint = 0x101F;
const CL_DEVICE_NAME: c_uint = 0x102B;
const CL_DEVICE_VENDOR: c_uint = 0x102C;
const CL_DEVICE_VERSION: c_uint = 0x102F;
const CL_DRIVER_VERSION: c_uint = 0x102D;
const CL_DEVICE_HOST_UNIFIED_MEMORY: c_uint = 0x1035;
const CL_DEVICE_TYPE_GPU: c_ulong = 1 << 2;

type ClGetPlatformIDsFn = unsafe extern "C" fn(c_uint, *mut *mut c_void, *mut c_uint) -> c_int;
type ClGetPlatformInfoFn =
    unsafe extern "C" fn(*mut c_void, c_uint, usize, *mut c_void, *mut usize) -> c_int;
type ClGetDeviceIDsFn =
    unsafe extern "C" fn(*mut c_void, c_ulong, c_uint, *mut *mut c_void, *mut c_uint) -> c_int;
type ClGetDeviceInfoFn =
    unsafe extern "C" fn(*mut c_void, c_uint, usize, *mut c_void, *mut usize) -> c_int;

/// Strips a case-insensitive "OpenCL " prefix and the surrounding spaces.
fn version_pretty(buffer: &str) -> &str {
    let mut text = buffer;
    if text.get(..7).is_some_and(|prefix| prefix.eq_ignore_ascii_case("OpenCL ")) {
        text = &text[7..];
    }
    text.trim_matches(' ')
}

struct OpenClResult {
    version: String,
    name: String,
    vendor: String,
    gpus: Vec<GpuResult>,
}

/// openCLHandleData: pick the newest platform, then walk its GPU devices.
fn handle_data(
    get_platform_ids: ClGetPlatformIDsFn,
    get_platform_info: ClGetPlatformInfoFn,
    get_device_ids: ClGetDeviceIDsFn,
    get_device_info: ClGetDeviceInfoFn,
) -> Result<OpenClResult, &'static str> {
    // SAFETY: the dlopen-ed OpenCL implementation fills our arrays; handles
    // stay valid until process exit.
    unsafe {
        let mut platforms: [*mut c_void; 32] = [std::ptr::null_mut(); 32];
        let mut num_platforms = 0;
        let ret = (get_platform_ids)(32, platforms.as_mut_ptr(), &mut num_platforms);
        if ret != CL_SUCCESS {
            return Err(match ret {
                -1 /* CL_PLATFORM_NOT_FOUND_KHR when defined */ => {
                    "clGetPlatformIDs() failed: CL_PLATFORM_NOT_FOUND_KHR"
                },
                -2 => "clGetPlatformIDs() failed: CL_INVALID_VALUE",
                -6 => "clGetPlatformIDs() failed: CL_OUT_OF_HOST_MEMORY",
                _ => "clGetPlatformIDs() failed: unknown error",
            });
        }
        if num_platforms == 0 {
            return Err("clGetPlatformIDs returned 0 platforms");
        }

        let mut result = OpenClResult {
            version: String::new(),
            name: String::new(),
            vendor: String::new(),
            gpus: Vec::new(),
        };
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
                return Err("clGetPlatformInfo() failed");
            }
            let text = String::from_utf8_lossy(&buffer[..size.saturating_sub(1).min(buffer.len())])
                .into_owned();

            // Use the newest supported OpenCL version.
            if result.version.as_str() < text.as_str() {
                result.version = version_pretty(&text).to_string();

                let mut size = 0usize;
                if (get_platform_info)(
                    *platform,
                    CL_PLATFORM_NAME,
                    buffer.len(),
                    buffer.as_mut_ptr().cast(),
                    &mut size,
                ) == CL_SUCCESS
                {
                    result.name =
                        String::from_utf8_lossy(&buffer[..size.saturating_sub(1).min(buffer.len())])
                            .into_owned();
                }
                let mut size = 0usize;
                if (get_platform_info)(
                    *platform,
                    CL_PLATFORM_VENDOR,
                    buffer.len(),
                    buffer.as_mut_ptr().cast(),
                    &mut size,
                ) == CL_SUCCESS
                {
                    result.vendor =
                        String::from_utf8_lossy(&buffer[..size.saturating_sub(1).min(buffer.len())])
                            .into_owned();
                }
            }

            let mut device_ids: [*mut c_void; 32] = [std::ptr::null_mut(); 32];
            let mut num_devices = device_ids.len() as c_uint;
            if (get_device_ids)(
                *platform,
                CL_DEVICE_TYPE_GPU,
                device_ids.len() as c_uint,
                device_ids.as_mut_ptr(),
                &mut num_devices,
            ) != CL_SUCCESS
            {
                continue;
            }

            for device in device_ids.iter().take(num_devices as usize) {
                let mut size = 0usize;
                if (get_device_info)(
                    *device,
                    CL_DEVICE_NAME,
                    buffer.len(),
                    buffer.as_mut_ptr().cast(),
                    &mut size,
                ) != CL_SUCCESS
                {
                    continue;
                }
                let mut gpu = GpuResult {
                    name: String::from_utf8_lossy(
                        &buffer[..size.saturating_sub(1).min(buffer.len())],
                    )
                    .into_owned(),
                    device_id: *device as usize as u64,
                    ..GpuResult::default()
                };

                let mut size = 0usize;
                if (get_device_info)(
                    *device,
                    CL_DEVICE_VERSION,
                    buffer.len(),
                    buffer.as_mut_ptr().cast(),
                    &mut size,
                ) == CL_SUCCESS
                {
                    gpu.platform_api = String::from_utf8_lossy(
                        &buffer[..size.saturating_sub(1).min(buffer.len())],
                    )
                    .trim_end()
                    .to_string();
                } else {
                    gpu.platform_api = "OpenCL".to_string();
                }

                let mut vendor_id = 0u32;
                if (get_device_info)(
                    *device,
                    CL_DEVICE_VENDOR_ID,
                    std::mem::size_of::<u32>(),
                    (&mut vendor_id as *mut u32).cast(),
                    std::ptr::null_mut(),
                ) == CL_SUCCESS
                {
                    gpu.vendor = super::gpu::vendor_string(vendor_id as u16)
                        .unwrap_or_default()
                        .to_string();
                }
                if gpu.vendor.is_empty() {
                    let mut size = 0usize;
                    if (get_device_info)(
                        *device,
                        CL_DEVICE_VENDOR,
                        buffer.len(),
                        buffer.as_mut_ptr().cast(),
                        &mut size,
                    ) == CL_SUCCESS
                    {
                        gpu.vendor = String::from_utf8_lossy(
                            &buffer[..size.saturating_sub(1).min(buffer.len())],
                        )
                        .into_owned();
                    }
                }

                let mut size = 0usize;
                if (get_device_info)(
                    *device,
                    CL_DRIVER_VERSION,
                    buffer.len(),
                    buffer.as_mut_ptr().cast(),
                    &mut size,
                ) == CL_SUCCESS
                {
                    let text = String::from_utf8_lossy(
                        &buffer[..size.saturating_sub(1).min(buffer.len())],
                    );
                    // Everything after the first space, else the whole string.
                    gpu.driver = match text.split_once(' ') {
                        Some((_, version)) => version.to_string(),
                        None => text.into_owned(),
                    };
                }

                let mut compute_units = 0u32;
                if (get_device_info)(
                    *device,
                    CL_DEVICE_MAX_COMPUTE_UNITS,
                    std::mem::size_of::<u32>(),
                    (&mut compute_units as *mut u32).cast(),
                    std::ptr::null_mut(),
                ) == CL_SUCCESS
                {
                    gpu.core_count = Some(compute_units as i32);
                }

                let mut clock = 0u32;
                if (get_device_info)(
                    *device,
                    CL_DEVICE_MAX_CLOCK_FREQUENCY,
                    std::mem::size_of::<u32>(),
                    (&mut clock as *mut u32).cast(),
                    std::ptr::null_mut(),
                ) == CL_SUCCESS
                {
                    gpu.frequency = clock;
                }

                let mut unified = 0u32;
                if (get_device_info)(
                    *device,
                    CL_DEVICE_HOST_UNIFIED_MEMORY,
                    std::mem::size_of::<u32>(),
                    (&mut unified as *mut u32).cast(),
                    std::ptr::null_mut(),
                ) == CL_SUCCESS
                {
                    gpu.gpu_type = if unified != 0 {
                        super::gpu::GPU_TYPE_INTEGRATED
                    } else {
                        super::gpu::GPU_TYPE_DISCRETE
                    };

                    let mut mem_size = 0u64;
                    if (get_device_info)(
                        *device,
                        CL_DEVICE_GLOBAL_MEM_SIZE,
                        std::mem::size_of::<u64>(),
                        (&mut mem_size as *mut u64).cast(),
                        std::ptr::null_mut(),
                    ) == CL_SUCCESS
                    {
                        if gpu.gpu_type == super::gpu::GPU_TYPE_INTEGRATED {
                            gpu.shared_total = Some(mem_size);
                        } else {
                            gpu.dedicated_total = Some(mem_size);
                        }
                    }
                }

                result.gpus.push(gpu);
            }
        }
        Ok(result)
    }
}

/// detectOpenCL: load libOpenCL and run the platform walk.
fn detect() -> Result<OpenClResult, &'static str> {
    let Some(library) = super::codec::load_library("OpenCL", 1) else {
        return Err("dlopen(libOpenCL.so) failed");
    };
    let Some(raw) = super::codec::load_symbol(library, "clGetPlatformIDs") else {
        return Err("dlsym clGetPlatformIDs failed");
    };
    let get_platform_ids: ClGetPlatformIDsFn = unsafe { std::mem::transmute(raw) };
    let Some(raw) = super::codec::load_symbol(library, "clGetPlatformInfo") else {
        return Err("dlsym clGetPlatformInfo failed");
    };
    let get_platform_info: ClGetPlatformInfoFn = unsafe { std::mem::transmute(raw) };
    let Some(raw) = super::codec::load_symbol(library, "clGetDeviceIDs") else {
        return Err("dlsym clGetDeviceIDs failed");
    };
    let get_device_ids: ClGetDeviceIDsFn = unsafe { std::mem::transmute(raw) };
    let Some(raw) = super::codec::load_symbol(library, "clGetDeviceInfo") else {
        return Err("dlsym clGetDeviceInfo failed");
    };
    let get_device_info: ClGetDeviceInfoFn = unsafe { std::mem::transmute(raw) };
    handle_data(get_platform_ids, get_platform_info, get_device_ids, get_device_info)
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(result) = detect() else {
        // The detection error rows are hidden by default.
        return Vec::new();
    };
    vec![Row::new("OpenCL", result.version)]
}

/// ffGenerateOpenCLJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(result) => {
            let mut obj = Json::obj();
            obj.push("version", result.version);
            obj.push("name", result.name);
            obj.push("vendor", result.vendor);

            let mut gpus = Json::Array(Vec::new());
            for gpu in &result.gpus {
                gpus.push_value(super::gpu::gpu_opencl_json(gpu));
            }
            obj.push("gpus", gpus);
            JsonOutcome::Result(obj)
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn strips_prefix_and_spaces() {
        assert_eq!(super::version_pretty("OpenCL 3.0 CUDA"), "3.0 CUDA");
        assert_eq!(super::version_pretty("opencl 2.1"), "2.1");
        assert_eq!(super::version_pretty("1.2"), "1.2");
        assert_eq!(super::version_pretty("OpenCL  3.0 "), "3.0");
    }

    #[test]
    fn driver_version_takes_the_second_word() {
        // Ported from the strchr split: everything after the first space.
        let text = "CUDA 620.70.06";
        assert_eq!(text.split_once(' ').map(|(_, v)| v).unwrap_or(text), "620.70.06");
        assert_eq!("620.70.06".split_once(' ').map(|(_, v)| v).unwrap_or("620.70.06"), "620.70.06");
    }
}

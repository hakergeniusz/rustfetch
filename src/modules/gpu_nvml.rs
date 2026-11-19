//! NVML backend for the proprietary NVIDIA driver, ported from
//! detection/gpu/gpu_nvidia.c. Loaded and initialized once; a failed load
//! or init is remembered, so later GPUs skip it silently like upstream's
//! "loading nvml library failed" path (the caller ignores the error).

use std::ffi::{c_char, c_int, c_uint, c_void};
use std::sync::OnceLock;

use super::{GpuResult, GPU_TYPE_DISCRETE};

const NVML_SUCCESS: c_int = 0;
// NVML_CLOCK_GRAPHICS, NVML_TEMPERATURE_GPU.
const CLOCK_GRAPHICS: c_int = 0;
/// NVML_STRUCT_VERSION(Memory, 2): sizeof(nvmlMemory_v2_t) | (2 << 24).
const MEMORY_V2_VERSION: c_uint = 40 | (2 << 24);
/// NVML_STRUCT_VERSION(Temperature, 1).
const TEMPERATURE_V1_VERSION: c_uint = 12 | (1 << 24);
/// Brands mapping to FF_GPU_TYPE_DISCRETE: QUADRO, TESLA, NVS, GEFORCE,
/// TITAN, QUADRO_RTX, NVIDIA_RTX.
const DISCRETE_BRANDS: [u32; 7] = [1, 2, 3, 5, 6, 12, 13];

#[repr(C)]
struct MemoryV2 {
    version: c_uint,
    total: u64,
    reserved: u64,
    free: u64,
    used: u64,
}

#[repr(C)]
struct MemoryV1 {
    version: c_uint,
    total: u64,
    free: u64,
    used: u64,
}

#[repr(C)]
struct TemperatureV1 {
    version: c_uint,
    sensor_type: c_int,
    temperature: c_int,
}

#[repr(C)]
struct Utilization {
    gpu: c_uint,
    memory: c_uint,
}

type HandleFn = unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> c_int;
type DeviceFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
type ClockFn = unsafe extern "C" fn(*mut c_void, c_int, *mut c_uint) -> c_int;

struct Api {
    get_handle_by_bus_id: HandleFn,
    get_brand: DeviceFn,
    get_index: DeviceFn,
    get_temperature: unsafe extern "C" fn(*mut c_void, *mut TemperatureV1) -> c_int,
    get_memory_v2: DeviceFn,
    get_memory_v1: DeviceFn,
    get_core_count: DeviceFn,
    get_max_clock: ClockFn,
    get_utilization: DeviceFn,
    get_max_pcie_gen: DeviceFn,
    get_max_pcie_width: DeviceFn,
    get_curr_pcie_gen: DeviceFn,
    get_curr_pcie_width: DeviceFn,
    get_name: unsafe extern "C" fn(*mut c_void, *mut c_char, c_uint) -> c_int,
}

unsafe fn sym(handle: *mut c_void, name: &str) -> Option<*mut c_void> {
    let Ok(name) = std::ffi::CString::new(name) else {
        return None;
    };
    let ptr = unsafe { libc::dlsym(handle, name.as_ptr()) };
    (!ptr.is_null()).then_some(ptr)
}

/// Interprets a dlsym'd pointer as the function type the call site expects.
unsafe fn fntype<T>(ptr: *mut c_void) -> T {
    assert!(!ptr.is_null());
    assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<*mut c_void>());
    unsafe { std::mem::transmute_copy::<*mut c_void, T>(&ptr) }
}

/// Loads libnvidia-ml and initializes it once; None after any failure.
fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<&'static Api>> = OnceLock::new();
    *API.get_or_init(|| unsafe {
        let handle = ["libnvidia-ml.so.1", "libnvidia-ml.so"].iter().find_map(|name| {
            let Ok(name) = std::ffi::CString::new(*name) else {
                return None;
            };
            let handle = libc::dlopen(name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
            (!handle.is_null()).then_some(handle)
        })?;
        macro_rules! need {
            ($name:literal) => {
                sym(handle, $name)?
            };
        }
        let init_v2: unsafe extern "C" fn() -> c_int = fntype(need!("nvmlInit_v2"));
        if init_v2() != NVML_SUCCESS {
            return None;
        }
        // Upstream registers nvmlShutdown via atexit; process teardown is
        // not observable, so it is skipped here.
        Some(&*Box::leak(Box::new(Api {
            get_handle_by_bus_id: fntype(need!("nvmlDeviceGetHandleByPciBusId_v2")),
            get_brand: fntype(need!("nvmlDeviceGetBrand")),
            get_index: fntype(need!("nvmlDeviceGetIndex")),
            get_temperature: fntype(need!("nvmlDeviceGetTemperatureV")),
            get_memory_v2: fntype(need!("nvmlDeviceGetMemoryInfo_v2")),
            get_memory_v1: fntype(need!("nvmlDeviceGetMemoryInfo")),
            get_core_count: fntype(need!("nvmlDeviceGetNumGpuCores")),
            get_max_clock: fntype(need!("nvmlDeviceGetMaxClockInfo")),
            get_utilization: fntype(need!("nvmlDeviceGetUtilizationRates")),
            get_max_pcie_gen: fntype(need!("nvmlDeviceGetMaxPcieLinkGeneration")),
            get_max_pcie_width: fntype(need!("nvmlDeviceGetMaxPcieLinkWidth")),
            get_curr_pcie_gen: fntype(need!("nvmlDeviceGetCurrPcieLinkGeneration")),
            get_curr_pcie_width: fntype(need!("nvmlDeviceGetCurrPcieLinkWidth")),
            get_name: fntype(need!("nvmlDeviceGetName")),
        })))
    })
}

/// ffDetectNvidiaGpuInfo with the bus-id condition. The error return is
/// deliberately dropped: upstream's gpu_linux.c caller ignores it too.
pub(crate) fn detect(
    gpu: &mut GpuResult,
    bus_id: (u64, u64, u64, u64),
    temp: bool,
    driver_specific: bool,
) {
    let Some(api) = api() else {
        return;
    };
    unsafe {
        let bus_id_str = format!("{:04x}:{:02x}:{:02x}.{}", bus_id.0, bus_id.1, bus_id.2, bus_id.3);
        let Ok(bus_id_str) = std::ffi::CString::new(bus_id_str) else {
            return;
        };
        let mut device: *mut c_void = std::ptr::null_mut();
        if (api.get_handle_by_bus_id)(bus_id_str.as_ptr(), &mut device) != NVML_SUCCESS {
            return;
        }

        // result.type: NVML brands that mean discrete
        let mut brand: u32 = 0;
        if (api.get_brand)(device, (&mut brand as *mut u32).cast()) == NVML_SUCCESS
            && DISCRETE_BRANDS.contains(&brand)
        {
            gpu.gpu_type = GPU_TYPE_DISCRETE;
        }

        // result.index
        let mut index: c_uint = 0;
        if (api.get_index)(device, (&mut index as *mut c_uint).cast()) == NVML_SUCCESS {
            gpu.index = Some(index);
        }

        // result.temp
        if temp {
            let mut request = TemperatureV1 {
                version: TEMPERATURE_V1_VERSION,
                sensor_type: 0, // NVML_TEMPERATURE_GPU
                temperature: 0,
            };
            if (api.get_temperature)(device, &mut request) == NVML_SUCCESS {
                gpu.temperature = Some(f64::from(request.temperature));
            }
        }

        // result.memory: dedicated
        if driver_specific {
            let mut memory = MemoryV2 {
                version: MEMORY_V2_VERSION,
                total: 0,
                reserved: 0,
                free: 0,
                used: 0,
            };
            if (api.get_memory_v2)(device, (&mut memory as *mut MemoryV2).cast()) == NVML_SUCCESS {
                gpu.dedicated_total = Some(memory.used + memory.free);
                gpu.dedicated_used = Some(memory.used);
            } else {
                let mut memory_v1 = MemoryV1 {
                    version: 0,
                    total: 0,
                    free: 0,
                    used: 0,
                };
                if (api.get_memory_v1)(device, (&mut memory_v1 as *mut MemoryV1).cast())
                    == NVML_SUCCESS
                {
                    gpu.dedicated_total = Some(memory_v1.total);
                    gpu.dedicated_used = Some(memory_v1.used);
                }
            }
        }

        // result.coreCount
        if driver_specific {
            let mut cores: c_uint = 0;
            if (api.get_core_count)(device, (&mut cores as *mut c_uint).cast()) == NVML_SUCCESS {
                gpu.core_count = Some(cores as i32);
            }
        }

        // result.frequency
        if driver_specific {
            let mut mhz: c_uint = 0;
            if (api.get_max_clock)(device, CLOCK_GRAPHICS, &mut mhz) == NVML_SUCCESS {
                gpu.frequency = mhz;
            }
        }

        // result.coreUsage
        if driver_specific {
            let mut utilization = Utilization { gpu: 0, memory: 0 };
            if (api.get_utilization)(device, (&mut utilization as *mut Utilization).cast())
                == NVML_SUCCESS
            {
                gpu.core_usage = Some(f64::from(utilization.gpu));
            }
        }

        // result.name: NVML's name replaces the pci.ids one
        let mut name = [0u8; 96]; // NVML_DEVICE_NAME_V2_BUFFER_SIZE
        if (api.get_name)(device, name.as_mut_ptr().cast(), name.len() as c_uint)
            == NVML_SUCCESS
        {
            let end = name.iter().position(|&byte| byte == 0).unwrap_or(name.len());
            if let Ok(nvml_name) = std::str::from_utf8(&name[..end]) {
                gpu.name = nvml_name.to_string();
            }
        }

        // result.psMax / result.psCurr
        if driver_specific {
            let mut value: c_uint = 0;
            if (api.get_max_pcie_gen)(device, (&mut value as *mut c_uint).cast()) == NVML_SUCCESS {
                gpu.pcie_max.0 = value as u16;
            }
            if (api.get_max_pcie_width)(device, (&mut value as *mut c_uint).cast()) == NVML_SUCCESS
            {
                gpu.pcie_max.1 = value as u16;
            }
            if (api.get_curr_pcie_gen)(device, (&mut value as *mut c_uint).cast()) == NVML_SUCCESS {
                gpu.pcie_curr.0 = value as u16;
            }
            if (api.get_curr_pcie_width)(device, (&mut value as *mut c_uint).cast()) == NVML_SUCCESS
            {
                gpu.pcie_curr.1 = value as u16;
            }
        }
    }
}

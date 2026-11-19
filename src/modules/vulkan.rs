//! `vulkan` module: the Vulkan API version, driver and physical devices,
//! ported from detection/vulkan/vulkan.c. A 1.1 instance (1.0 when the
//! runtime is older) enumerates physical devices; the newest device API
//! version wins and its driver name/info form the value, while every
//! non-CPU device joins the result with its memory heaps. Software
//! rasterizers (VK_PHYSICAL_DEVICE_TYPE_CPU) are skipped; when only those
//! remain the instance version prints with a " [Software only]" suffix.

use std::ffi::{CStr, c_char, c_int, c_uint, c_void};

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::{Row, gpu::{GpuResult, GPU_TYPE_DISCRETE, GPU_TYPE_INTEGRATED, GPU_TYPE_UNKNOWN, gpu_vulkan_json}},
};

const VK_SUCCESS: c_int = 0;
const VK_STRUCTURE_TYPE_APPLICATION_INFO: c_int = 0;
const VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO: c_int = 1;
// Values per vulkan_core.h; Properties2 is 1000059001, not 1000059000.
const VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2: c_int = 1000059001;
const VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DRIVER_PROPERTIES_KHR: c_int = 1000196000;
const VK_PHYSICAL_DEVICE_TYPE_INTEGRATED: u32 = 1;
const VK_PHYSICAL_DEVICE_TYPE_DISCRETE: u32 = 2;
const VK_PHYSICAL_DEVICE_TYPE_VIRTUAL: u32 = 3;
const VK_PHYSICAL_DEVICE_TYPE_CPU: u32 = 4;
const VK_MEMORY_HEAP_DEVICE_LOCAL_BIT: u32 = 1 << 0;
const VK_MAX_MEMORY_HEAPS: usize = 64;

/// VK_MAKE_VERSION.
const fn make_version(major: u32, minor: u32, patch: u32) -> c_uint {
    (major << 22) | (minor << 12) | patch
}

const VK_API_VERSION_1_0: c_uint = make_version(1, 0, 0);
const VK_API_VERSION_1_1: c_uint = make_version(1, 1, 0);

type VkGetInstanceProcAddrFn = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
type VkCreateInstanceFn =
    unsafe extern "C" fn(*const VkInstanceCreateInfo, *const c_void, *mut *mut c_void) -> c_int;
type VkDestroyInstanceFn = unsafe extern "C" fn(*mut c_void, *const c_void);
type VkEnumeratePhysicalDevicesFn =
    unsafe extern "C" fn(*mut c_void, *mut c_uint, *mut *mut c_void) -> c_int;
type VkGetPhysicalDeviceProperties2Fn =
    unsafe extern "C" fn(*mut c_void, *mut VkPhysicalDeviceProperties2);
type VkGetPhysicalDevicePropertiesFn =
    unsafe extern "C" fn(*mut c_void, *mut VkPhysicalDevicePropertiesBlob);
type VkGetPhysicalDeviceMemoryPropertiesFn =
    unsafe extern "C" fn(*mut c_void, *mut VkPhysicalDeviceMemoryProperties);
type VkEnumerateInstanceVersionFn = unsafe extern "C" fn(*mut c_uint) -> c_int;

#[repr(C)]
struct VkApplicationInfo {
    s_type: c_int,
    p_next: *const c_void,
    p_application_name: *const c_char,
    application_version: c_uint,
    p_engine_name: *const c_char,
    engine_version: c_uint,
    api_version: c_uint,
}

#[repr(C)]
struct VkInstanceCreateInfo {
    s_type: c_int,
    p_next: *const c_void,
    flags: c_uint,
    p_application_info: *const VkApplicationInfo,
    enabled_layer_count: c_uint,
    pp_enabled_layer_names: *const *const c_char,
    enabled_extension_count: c_uint,
    pp_enabled_extension_names: *const *const c_char,
}

/// The blob upstream declares as VkPhysicalDeviceProperties; only
/// apiVersion (offset 0) and deviceType (offset 16) are read.
/// VkPhysicalDeviceProperties up to deviceName; the tail the module never
/// reads is left out (upstream zero-fills the whole struct, which only
/// matters for fields this port does not touch).
#[repr(C)]
struct VkPhysicalDevicePropertiesBlob {
    api_version: c_uint,
    driver_version: c_uint,
    vendor_id: c_uint,
    device_id: c_uint,
    device_type: c_uint,
    device_name: [u8; 256],
    /// The limits/sparse fields the driver writes past deviceName.
    tail: [u8; 764],
}

#[repr(C)]
struct VkPhysicalDeviceProperties2 {
    s_type: c_int,
    p_next: *mut c_void,
    properties: VkPhysicalDevicePropertiesBlob,
}

#[repr(C)]
struct VkMemoryHeap {
    size: u64,
    flags: u32,
}

#[repr(C)]
struct VkPhysicalDeviceMemoryProperties {
    memory_type_count: u32,
    memory_types: [u8; 256], // not read
    memory_heap_count: u32,
    memory_heaps: [VkMemoryHeap; VK_MAX_MEMORY_HEAPS],
}

#[repr(C)]
struct VkPhysicalDeviceDriverProperties {
    s_type: c_int,
    p_next: *mut c_void,
    driver_id: c_uint,
    driver_name: [u8; 256],
    driver_info: [u8; 256],
    conformance_version: [u8; 4],
}

/// VK_VERSION_* unpacked into (major, minor, patch).
fn decode_version(version: c_uint) -> (u32, u32, u32) {
    (version >> 22, (version >> 12) & 0x3FF, version & 0xFFF)
}

/// ffVersionToPretty: zero trailing components are dropped ("2.0.0" is "2").
fn version_pretty(version: (u32, u32, u32)) -> String {
    let mut out = String::new();
    if version.0 > 0 || version.1 > 0 || version.2 > 0 {
        out.push_str(&version.0.to_string());
    }
    if version.1 > 0 || version.2 > 0 {
        out.push('.');
        out.push_str(&version.1.to_string());
    }
    if version.2 > 0 {
        out.push('.');
        out.push_str(&version.2.to_string());
    }
    out
}

/// applyDriverName: "name [info]", dropping multiline infos.
fn driver_string(properties: &VkPhysicalDeviceDriverProperties) -> String {
    let name = CStr::from_bytes_until_nul(&properties.driver_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.is_empty() {
        return String::new();
    }
    let mut driver = name;
    let info = CStr::from_bytes_until_nul(&properties.driver_info)
        .map(|info| info.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !info.is_empty() && !info.contains('\n') {
        driver.push_str(" [");
        driver.push_str(&info);
        driver.push(']');
    }
    driver
}

/// (major, minor, patch) ordering, like ffVersionCompare.
fn version_newer(candidate: (u32, u32, u32), current: (u32, u32, u32)) -> bool {
    candidate > current
}

pub struct VulkanResult {
    api_version: String,
    conformance_version: String,
    instance_version: String,
    driver: String,
    gpus: Vec<GpuResult>,
}

/// ffVersionToPretty applied to every result version.
fn finalize(result: &mut VulkanResult, instance: (u32, u32, u32), api: (u32, u32, u32), conformance: (u32, u32, u32)) {
    result.instance_version = version_pretty(instance);
    result.api_version = version_pretty(api);
    result.conformance_version = version_pretty(conformance);
}

/// detectVulkan.
fn detect() -> Result<VulkanResult, &'static str> {
    let Some(vulkan) = super::codec::load_library("vulkan", 2) else {
        return Err("dlopen(libvulkan.so) failed");
    };
    let Some(raw) = super::codec::load_symbol(vulkan, "vkGetInstanceProcAddr") else {
        return Err("dlsym vkGetInstanceProcAddr failed");
    };
    let vk_get_proc_addr: VkGetInstanceProcAddrFn = unsafe { std::mem::transmute(raw) };
    let Some(raw) = super::codec::load_symbol(vulkan, "vkCreateInstance") else {
        return Err("dlsym vkCreateInstance failed");
    };
    let vk_create_instance: VkCreateInstanceFn = unsafe { std::mem::transmute(raw) };
    let Some(raw) = super::codec::load_symbol(vulkan, "vkDestroyInstance") else {
        return Err("dlsym vkDestroyInstance failed");
    };
    let vk_destroy_instance: VkDestroyInstanceFn = unsafe { std::mem::transmute(raw) };
    let Some(raw) = super::codec::load_symbol(vulkan, "vkEnumeratePhysicalDevices") else {
        return Err("dlsym vkEnumeratePhysicalDevices failed");
    };
    let vk_enumerate_devices: VkEnumeratePhysicalDevicesFn = unsafe { std::mem::transmute(raw) };

    // Driver loaders print messages to stdout; suppress them like upstream.
    let Some(mut suppressed) = super::opengl::SuppressedIo::new() else {
        return Err("Failed to suppress stdio");
    };

    let outcome = (|| -> Result<VulkanResult, &'static str> {
        let mut result = VulkanResult {
            api_version: String::new(),
            conformance_version: String::new(),
            instance_version: String::new(),
            driver: String::new(),
            gpus: Vec::new(),
        };

        // SAFETY: the loader's function pointers are valid for the lifetime
        // of the dlopen-ed library; the instance is destroyed below.
        unsafe {
            // vkEnumerateInstanceVersion exists on 1.1+ only.
            let mut instance_version = (0u32, 0u32, 0u32);
            let enumerate_version_name = c"vkEnumerateInstanceVersion";
            let raw = (vk_get_proc_addr)(std::ptr::null_mut(), enumerate_version_name.as_ptr());
            if !raw.is_null() {
                let enumerate_version: VkEnumerateInstanceVersionFn = std::mem::transmute(raw);
                let mut version = 0u32;
                if (enumerate_version)(&mut version) == VK_SUCCESS {
                    instance_version = decode_version(version);
                }
            }

            let project_version = make_version(0, 1, 0); // rustfetch 0.1.0
            let app_name = c"rustfetch";
            let engine_name = c"vulkanPrintGPUs";
            let application_info = VkApplicationInfo {
                s_type: VK_STRUCTURE_TYPE_APPLICATION_INFO,
                p_next: std::ptr::null(),
                p_application_name: app_name.as_ptr(),
                application_version: project_version,
                p_engine_name: engine_name.as_ptr(),
                engine_version: project_version,
                // Requesting 1.1 unlocks VkPhysicalDeviceDriverProperties.
                api_version: if instance_version.1 >= 1 {
                    VK_API_VERSION_1_1
                } else {
                    VK_API_VERSION_1_0
                },
            };
            let create_info = VkInstanceCreateInfo {
                s_type: VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
                p_next: std::ptr::null(),
                flags: 0,
                p_application_info: &application_info,
                enabled_layer_count: 0,
                pp_enabled_layer_names: std::ptr::null(),
                enabled_extension_count: 0,
                pp_enabled_extension_names: std::ptr::null(),
            };
            let mut instance: *mut c_void = std::ptr::null_mut();
            let res = (vk_create_instance)(&create_info, std::ptr::null(), &mut instance);
            if res != VK_SUCCESS {
                return Err(match res {
                    -1 => "ffvkCreateInstance() failed: VK_ERROR_OUT_OF_HOST_MEMORY",
                    -2 => "ffvkCreateInstance() failed: VK_ERROR_OUT_OF_DEVICE_MEMORY",
                    -3 => "ffvkCreateInstance() failed: VK_ERROR_INITIALIZATION_FAILED",
                    -6 => "ffvkCreateInstance() failed: VK_ERROR_LAYER_NOT_PRESENT",
                    -7 => "ffvkCreateInstance() failed: VK_ERROR_EXTENSION_NOT_PRESENT",
                    -9 => "ffvkCreateInstance() failed: VK_ERROR_INCOMPATIBLE_DRIVER",
                    _ => "ffvkCreateInstance() failed: unknown error",
                });
            }

            // If instance creation succeeded but vkEnumerateInstanceVersion
            // did not, the implementation is 1.0.
            if instance_version == (0, 0, 0) {
                instance_version = (1, 0, 0);
            }

            let mut devices: [*mut c_void; 128] = [std::ptr::null_mut(); 128];
            let mut device_count = devices.len() as c_uint;
            let res = (vk_enumerate_devices)(instance, &mut device_count, devices.as_mut_ptr());
            if res != VK_SUCCESS {
                (vk_destroy_instance)(instance, std::ptr::null());
                return Err(match res {
                    -1 => "ffvkEnumeratePhysicalDevices() failed: VK_ERROR_OUT_OF_HOST_MEMORY",
                    -2 => "ffvkEnumeratePhysicalDevices() failed: VK_ERROR_OUT_OF_DEVICE_MEMORY",
                    -3 => "ffvkEnumeratePhysicalDevices() failed: VK_ERROR_INITIALIZATION_FAILED",
                    5 => "ffvkEnumeratePhysicalDevices() failed: VK_INCOMPLETE",
                    _ => "ffvkEnumeratePhysicalDevices() failed",
                });
            }

            let props2_name = c"vkGetPhysicalDeviceProperties2";
            let props2_raw = (vk_get_proc_addr)(instance, props2_name.as_ptr());
            let props1_name = c"vkGetPhysicalDeviceProperties";
            let props1_raw = (vk_get_proc_addr)(instance, props1_name.as_ptr());
            let memory_name = c"vkGetPhysicalDeviceMemoryProperties";
            let memory_raw = (vk_get_proc_addr)(instance, memory_name.as_ptr());
            if memory_raw.is_null() {
                (vk_destroy_instance)(instance, std::ptr::null());
                return Err("vkGetPhysicalDeviceMemoryProperties is not available");
            }
            let get_memory_properties: VkGetPhysicalDeviceMemoryPropertiesFn =
                std::mem::transmute(memory_raw);

            let mut max_api = (0u32, 0u32, 0u32);
            let mut max_conformance = (0u32, 0u32, 0u32);
            for device in devices.iter().take(device_count as usize) {
                let mut driver_properties = VkPhysicalDeviceDriverProperties {
                    s_type: VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DRIVER_PROPERTIES_KHR,
                    p_next: std::ptr::null_mut(),
                    driver_id: 0,
                    driver_name: [0; 256],
                    driver_info: [0; 256],
                    conformance_version: [0; 4],
                };
                let mut properties = VkPhysicalDeviceProperties2 {
                    s_type: VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2,
                    p_next: &mut driver_properties as *mut _ as *mut c_void,
                    properties: std::mem::zeroed(),
                };
                if !props2_raw.is_null() {
                    let get_properties2: VkGetPhysicalDeviceProperties2Fn =
                        std::mem::transmute(props2_raw);
                    (get_properties2)(*device, &mut properties);
                } else {
                    let get_properties: VkGetPhysicalDevicePropertiesFn =
                        std::mem::transmute(props1_raw);
                    (get_properties)(*device, &mut properties.properties);
                }
                let blob = &properties.properties;

                // Software rasterizers are not physical GPUs.
                if blob.device_type == VK_PHYSICAL_DEVICE_TYPE_CPU {
                    continue;
                }

                let device_api = decode_version(blob.api_version);
                if version_newer(device_api, max_api) {
                    max_api = device_api;
                    result.driver = driver_string(&driver_properties);
                }

                // The conformance version rides the Properties2 chain only;
                // VkConformanceVersion is {major, minor, subminor, patch} and
                // upstream compares major/minor/patch.
                if !props2_raw.is_null() {
                    let device_conformance = (
                        u32::from(driver_properties.conformance_version[0]),
                        u32::from(driver_properties.conformance_version[1]),
                        u32::from(driver_properties.conformance_version[3]),
                    );
                    if version_newer(device_conformance, max_conformance) {
                        max_conformance = device_conformance;
                    }
                }

                // #456: one entry per deviceId.
                if result
                    .gpus
                    .iter()
                    .any(|gpu| gpu.device_id == u64::from(blob.device_id))
                {
                    continue;
                }

                let mut gpu = GpuResult {
                    platform_api: format!("Vulkan {}", version_pretty(device_api)),
                    device_id: u64::from(blob.device_id),
                    name: CStr::from_bytes_until_nul(&blob.device_name)
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    gpu_type: match blob.device_type {
                        VK_PHYSICAL_DEVICE_TYPE_DISCRETE => GPU_TYPE_DISCRETE,
                        // Virtual GPUs are hypervisor devices; integrated.
                        VK_PHYSICAL_DEVICE_TYPE_INTEGRATED | VK_PHYSICAL_DEVICE_TYPE_VIRTUAL => {
                            GPU_TYPE_INTEGRATED
                        },
                        _ => GPU_TYPE_UNKNOWN,
                    },
                    vendor: super::gpu::vendor_string(blob.vendor_id as u16)
                        .unwrap_or_default()
                        .to_string(),
                    // The driver field carries the raw driverInfo, unlike the
                    // module-level driver's "name [info]" form.
                    driver: CStr::from_bytes_until_nul(&driver_properties.driver_info)
                        .map(|info| info.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    dedicated_total: Some(0),
                    shared_total: Some(0),
                    ..GpuResult::default()
                };

                let mut memory_properties: VkPhysicalDeviceMemoryProperties =
                    std::mem::zeroed();
                (get_memory_properties)(*device, &mut memory_properties);
                for heap in memory_properties.memory_heaps
                    .iter()
                    .take(memory_properties.memory_heap_count as usize)
                {
                    let vmem = if gpu.gpu_type == GPU_TYPE_DISCRETE
                        && heap.flags & VK_MEMORY_HEAP_DEVICE_LOCAL_BIT != 0
                    {
                        &mut gpu.dedicated_total
                    } else {
                        &mut gpu.shared_total
                    };
                    *vmem = Some(vmem.unwrap_or(0) + heap.size);
                }

                result.gpus.push(gpu);
            }

            (vk_destroy_instance)(instance, std::ptr::null());

            finalize(&mut result, instance_version, max_api, max_conformance);
            Ok(result)
        }
    })();
    suppressed.restore();
    outcome
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(result) = detect() else {
        // The detection error rows are hidden by default.
        return Vec::new();
    };

    let mut value = if result.api_version.is_empty() {
        // No non-CPU device: both the device api version and the driver
        // stay empty, so the instance version prints with the marker.
        format!("{} [Software only]", result.instance_version)
    } else {
        let mut value = result.api_version;
        if !result.driver.is_empty() {
            value.push_str(" - ");
            value.push_str(&result.driver);
        }
        value
    };
    let _ = &mut value;
    vec![Row::new("Vulkan", value)]
}

/// ffGenerateVulkanJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(result) => {
            let mut obj = Json::obj();
            obj.push("apiVersion", result.api_version);
            obj.push("conformanceVersion", result.conformance_version);
            obj.push("instanceVersion", result.instance_version);
            obj.push("driver", result.driver);
            let mut gpus = Json::Array(Vec::new());
            for gpu in &result.gpus {
                gpus.push_value(gpu_vulkan_json(gpu));
            }
            obj.push("gpus", gpus);
            JsonOutcome::Result(obj)
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn decodes_versions() {
        let encoded = super::make_version(1, 4, 357);
        assert_eq!(super::decode_version(encoded), (1, 4, 357));
    }

    #[test]
    fn pretty_drops_trailing_zeros() {
        assert_eq!(super::version_pretty((1, 4, 357)), "1.4.357");
        assert_eq!(super::version_pretty((1, 0, 0)), "1");
        assert_eq!(super::version_pretty((2, 0, 0)), "2");
        assert_eq!(super::version_pretty((0, 0, 0)), "");
    }
}

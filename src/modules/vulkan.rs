//! `vulkan` module: the Vulkan API version and driver, ported from
//! detection/vulkan/vulkan.c. A 1.1 instance (1.0 when the runtime is
//! older) enumerates physical devices; the newest device API version
//! wins and its driver name/info form the value. Software rasterizers
//! (VK_PHYSICAL_DEVICE_TYPE_CPU) are skipped; when only those remain the
//! instance version prints with a " [Software only]" suffix. Memory
//! totals and per-GPU details upstream collects only feed custom formats,
//! which this port does not implement.

use std::ffi::{CStr, c_char, c_int, c_uint, c_void};

use crate::{instance::Instance, modules::Row};

const VK_SUCCESS: c_int = 0;
const VK_STRUCTURE_TYPE_APPLICATION_INFO: c_int = 0;
const VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO: c_int = 1;
// Values per vulkan_core.h; Properties2 is 1000059001, not 1000059000.
const VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2: c_int = 1000059001;
const VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DRIVER_PROPERTIES_KHR: c_int = 1000196000;
const VK_PHYSICAL_DEVICE_TYPE_CPU: u32 = 4;

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
#[repr(C)]
struct VkPhysicalDevicePropertiesBlob {
    api_version: c_uint,
    driver_version: c_uint,
    vendor_id: c_uint,
    device_id: c_uint,
    device_type: c_uint,
    rest: [u8; 1020],
}

#[repr(C)]
struct VkPhysicalDeviceProperties2 {
    s_type: c_int,
    p_next: *mut c_void,
    properties: VkPhysicalDevicePropertiesBlob,
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

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(vulkan) = super::codec::load_library("vulkan", 2) else {
        // "libvulkan.so.2 failed" is an error row, hidden by default.
        return Vec::new();
    };
    let Some(get_proc_addr_raw) = super::codec::load_symbol(vulkan, "vkGetInstanceProcAddr") else {
        return Vec::new();
    };
    let Some(create_instance_raw) = super::codec::load_symbol(vulkan, "vkCreateInstance") else {
        return Vec::new();
    };
    let Some(destroy_instance_raw) = super::codec::load_symbol(vulkan, "vkDestroyInstance") else {
        return Vec::new();
    };
    let Some(enumerate_devices_raw) =
        super::codec::load_symbol(vulkan, "vkEnumeratePhysicalDevices")
    else {
        return Vec::new();
    };
    let vk_get_proc_addr: VkGetInstanceProcAddrFn =
        unsafe { std::mem::transmute(get_proc_addr_raw) };
    let vk_create_instance: VkCreateInstanceFn =
        unsafe { std::mem::transmute(create_instance_raw) };
    let vk_destroy_instance: VkDestroyInstanceFn =
        unsafe { std::mem::transmute(destroy_instance_raw) };
    let vk_enumerate_devices: VkEnumeratePhysicalDevicesFn =
        unsafe { std::mem::transmute(enumerate_devices_raw) };

    // Driver loaders print messages to stdout; suppress them like upstream.
    let Some(mut suppressed) = super::opengl::SuppressedIo::new() else {
        return Vec::new();
    };

    let outcome = (|| -> Option<(String, String)> {
        unsafe {
            // vkEnumerateInstanceVersion exists on 1.1+ only.
            let mut instance_version = 0u32;
            let enumerate_version_name = c"vkEnumerateInstanceVersion";
            let raw = (vk_get_proc_addr)(std::ptr::null_mut(), enumerate_version_name.as_ptr());
            if !raw.is_null() {
                let enumerate_version: VkEnumerateInstanceVersionFn = std::mem::transmute(raw);
                let mut version = 0u32;
                if (enumerate_version)(&mut version) == VK_SUCCESS {
                    instance_version = version;
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
                api_version: if (instance_version >> 12) & 0x3FF >= 1 {
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
            if (vk_create_instance)(&create_info, std::ptr::null(), &mut instance) != VK_SUCCESS {
                return None;
            }

            let mut devices: [*mut c_void; 128] = [std::ptr::null_mut(); 128];
            let mut device_count = devices.len() as c_uint;
            if (vk_enumerate_devices)(instance, &mut device_count, devices.as_mut_ptr())
                != VK_SUCCESS
            {
                (vk_destroy_instance)(instance, std::ptr::null());
                return None;
            }

            let props2_name = c"vkGetPhysicalDeviceProperties2";
            let props2_raw = (vk_get_proc_addr)(instance, props2_name.as_ptr());
            let props1_name = c"vkGetPhysicalDeviceProperties";
            let props1_raw = (vk_get_proc_addr)(instance, props1_name.as_ptr());
            let memory_name = c"vkGetPhysicalDeviceMemoryProperties";
            let memory_raw = (vk_get_proc_addr)(instance, memory_name.as_ptr());
            if memory_raw.is_null() {
                (vk_destroy_instance)(instance, std::ptr::null());
                return None;
            }

            let mut max_api = (0u32, 0u32, 0u32);
            let mut max_driver = String::new();
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

                // Software rasterizers are not physical GPUs.
                if properties.properties.device_type == VK_PHYSICAL_DEVICE_TYPE_CPU {
                    continue;
                }

                let api = properties.properties.api_version;
                let device_api = (api >> 22, (api >> 12) & 0x3FF, api & 0xFFF);
                if version_newer(device_api, max_api) {
                    max_api = device_api;
                    max_driver = driver_string(&driver_properties);
                }
            }

            (vk_destroy_instance)(instance, std::ptr::null());

            if max_api == (0, 0, 0) {
                // No non-CPU device: the instance version with a marker
                // suffix. 0 means the enumerate call was unavailable;
                // upstream falls back to "1.0".
                let version = if instance_version == 0 {
                    (1, 0, 0)
                } else {
                    decode_version(instance_version)
                };
                Some((version_pretty(version), String::new()))
            } else {
                Some((version_pretty(max_api), max_driver))
            }
        }
    })();
    suppressed.restore();

    match outcome {
        Some((api, driver)) => {
            let mut value = api;
            if !driver.is_empty() {
                value.push_str(" - ");
                value.push_str(&driver);
            }
            vec![Row::new("Vulkan", value)]
        },
        None => Vec::new(),
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

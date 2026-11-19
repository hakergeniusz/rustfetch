//! `codec` module: hardware video acceleration capabilities, ported from
//! codec_linux.c. libva and libva-drm are dlopened and interrogated over
//! each /dev/dri/renderD node (VA-API over DRM); the profiles map onto
//! codec types and the entrypoints decide decoder vs encoder. The VDPAU
//! fallback needs a live X11 display and is not ported.

use std::ffi::{CString, c_char, c_int, c_void};

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
};

const VA_STATUS_SUCCESS: c_int = 0;
const VA_CONFIG_ATTRIB_RT_FORMAT: u32 = 0;
const VA_ATTRIB_NOT_SUPPORTED: u32 = 0x8000_0000;

const ENTRYPOINT_VLD: c_int = 1;
const ENTRYPOINT_IDCT: c_int = 3;
const ENTRYPOINT_MOCOMP: c_int = 4;
const ENTRYPOINT_ENCSLICE: c_int = 6;
const ENTRYPOINT_ENCSLICE_LP: c_int = 8;
const ENTRYPOINT_FEI: c_int = 11;

type Display = *mut c_void;

type VaInitializeFn = unsafe extern "C" fn(Display, *mut c_int, *mut c_int) -> c_int;
type VaTerminateFn = unsafe extern "C" fn(Display) -> c_int;
type VaMaxNumFn = unsafe extern "C" fn(Display) -> c_int;
type VaQueryProfilesFn = unsafe extern "C" fn(Display, *mut c_int, *mut c_int) -> c_int;
type VaQueryEntrypointsFn = unsafe extern "C" fn(Display, c_int, *mut c_int, *mut c_int) -> c_int;
type VaGetAttributesFn =
    unsafe extern "C" fn(Display, c_int, c_int, *mut VaConfigAttrib, c_int) -> c_int;
type VaGetDisplayDrmFn = unsafe extern "C" fn(c_int) -> Display;
type VaQueryVendorStringFn = unsafe extern "C" fn(Display) -> *const c_char;

#[repr(C)]
struct VaConfigAttrib {
    type_: u32,
    value: u32,
}

/// The FFCodecType bitfield, in print order.
#[derive(Clone, Copy, PartialEq, Eq)]
struct CodecType(u32);

const H263: CodecType = CodecType(1 << 2);
const MJPEG: CodecType = CodecType(1 << 3);
const MPEG2: CodecType = CodecType(1 << 5);
const DIVX_XVID: CodecType = CodecType(1 << 6);
const H264: CodecType = CodecType(1 << 7);
const VC1: CodecType = CodecType(1 << 10);
const VP8: CodecType = CodecType(1 << 11);
const HEVC: CodecType = CodecType(1 << 12);
const VP9: CodecType = CodecType(1 << 13);
const AV1: CodecType = CodecType(1 << 14);
const VVC: CodecType = CodecType(1 << 15);

/// Every type bit VA-API can produce, in ffCodecTypeToString print order.
const VA_TYPES: [CodecType; 11] = [
    H263, MJPEG, MPEG2, DIVX_XVID, H264, VC1, VP8, HEVC, VP9, AV1, VVC,
];

/// ffCodecTypeToString.
fn codec_type_name(type_: CodecType) -> &'static str {
    match type_ {
        H263 => "H.263",
        MJPEG => "MJPEG",
        MPEG2 => "MPEG-2",
        DIVX_XVID => "DivX / Xvid",
        H264 => "H.264",
        VC1 => "VC-1",
        VP8 => "VP8",
        HEVC => "HEVC / H.265",
        VP9 => "VP9",
        AV1 => "AV1",
        VVC => "VVC / H.266",
        _ => "Unknown",
    }
}

/// vaProfileToType: the numeric VAProfile values fastfetch maps.
fn va_profile_to_type(profile: c_int) -> CodecType {
    CodecType(match profile {
        // VAProfileVP9Profile0..3
        19..=22 => 1 << 13,
        // The HEVC family (Main, Main10, Main12, 422, 444, SCC variants).
        17 | 18 | 23..=31 | 34 => 1 << 12,
        32 | 33 | 39 => 1 << 14,                      // AV1Profile0..2
        37 | 38 => 1 << 15,                           // VVCMain10 / VVCMultilayerMain10
        11 => 1 << 2,                                 // H263Baseline
        12 => 1 << 3,                                 // JPEGBaseline
        0 | 1 => 1 << 5,                              // MPEG2Simple / MPEG2Main
        2..=4 => 1 << 6,                              // MPEG4 Simple / AdvancedSimple / Main
        5 | 6 | 7 | 13 | 15 | 16 | 36 | 40 => 1 << 7, // H264 family
        8..=10 => 1 << 10,                            // VC1Simple / Main / Advanced
        14 => 1 << 11,                                // VP8Version0_3
        _ => 0,
    })
}

/// vaGetEntrypointType.
fn entrypoint_show_type(entrypoint: c_int) -> u8 {
    match entrypoint {
        ENTRYPOINT_VLD | ENTRYPOINT_IDCT | ENTRYPOINT_MOCOMP => 1, // decoder
        ENTRYPOINT_ENCSLICE | ENTRYPOINT_ENCSLICE_LP | ENTRYPOINT_FEI => 2, // encoder
        _ => 0,
    }
}

/// ffLibraryLoadSingle: plain path first, then ".N" suffixes downwards.
pub(crate) fn load_library(name: &str, max_version: i32) -> Option<*mut c_void> {
    let plain = CString::new(format!("lib{name}.so")).ok()?;
    let handle = unsafe { libc::dlopen(plain.as_ptr(), libc::RTLD_LAZY | libc::RTLD_NODELETE) };
    if !handle.is_null() {
        return Some(handle);
    }
    for version in (0..=max_version).rev() {
        let path = CString::new(format!("lib{name}.so.{version}")).ok()?;
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_LAZY | libc::RTLD_NODELETE) };
        if !handle.is_null() {
            return Some(handle);
        }
    }
    None
}

pub(crate) fn load_symbol(handle: *mut c_void, name: &str) -> Option<*mut c_void> {
    let symbol = CString::new(name).ok()?;
    let pointer = unsafe { libc::dlsym(handle, symbol.as_ptr()) };
    (!pointer.is_null()).then_some(pointer)
}

struct VaApi {
    /// Handles stay open for the lifetime of the borrowed symbols.
    _libva: *mut c_void,
    _libva_drm: *mut c_void,
    va_initialize: VaInitializeFn,
    va_terminate: VaTerminateFn,
    va_max_num_profiles: VaMaxNumFn,
    va_max_num_entrypoints: VaMaxNumFn,
    va_query_config_profiles: VaQueryProfilesFn,
    va_query_config_entrypoints: VaQueryEntrypointsFn,
    va_get_config_attributes: VaGetAttributesFn,
    va_query_vendor_string: VaQueryVendorStringFn,
    va_get_display_drm: VaGetDisplayDrmFn,
}

impl VaApi {
    /// The load order of detectCodecByVaDrm; both libraries must open.
    fn load() -> Option<Self> {
        let libva = load_library("va", 2)?;
        let libva_drm = load_library("va-drm", 2)?;
        // SAFETY: the handles outlive the copied symbols and are never closed.
        Some(unsafe { Self::from_handles(libva, libva_drm)? })
    }

    unsafe fn from_handles(libva: *mut c_void, libva_drm: *mut c_void) -> Option<Self> {
        unsafe {
            Some(Self {
                _libva: libva,
                _libva_drm: libva_drm,
                va_initialize: std::mem::transmute::<*mut c_void, VaInitializeFn>(load_symbol(
                    libva,
                    "vaInitialize",
                )?),
                va_terminate: std::mem::transmute::<*mut c_void, VaTerminateFn>(load_symbol(
                    libva,
                    "vaTerminate",
                )?),
                va_max_num_profiles: std::mem::transmute::<*mut c_void, VaMaxNumFn>(load_symbol(
                    libva,
                    "vaMaxNumProfiles",
                )?),
                va_max_num_entrypoints: std::mem::transmute::<*mut c_void, VaMaxNumFn>(
                    load_symbol(libva, "vaMaxNumEntrypoints")?,
                ),
                va_query_config_profiles: std::mem::transmute::<*mut c_void, VaQueryProfilesFn>(
                    load_symbol(libva, "vaQueryConfigProfiles")?,
                ),
                va_query_config_entrypoints: std::mem::transmute::<*mut c_void, VaQueryEntrypointsFn>(
                    load_symbol(libva, "vaQueryConfigEntrypoints")?,
                ),
                va_get_config_attributes: std::mem::transmute::<*mut c_void, VaGetAttributesFn>(
                    load_symbol(libva, "vaGetConfigAttributes")?,
                ),
                va_query_vendor_string: std::mem::transmute::<*mut c_void, VaQueryVendorStringFn>(
                    load_symbol(libva, "vaQueryVendorString")?,
                ),
                va_get_display_drm: std::mem::transmute::<*mut c_void, VaGetDisplayDrmFn>(
                    load_symbol(libva_drm, "vaGetDisplayDRM")?,
                ),
            })
        }
    }
}

/// vaProfileHasOutput: some entrypoint of the wanted kind whose RT format
/// attribute is supported.
unsafe fn profile_has_output(
    api: &VaApi,
    display: Display,
    entrypoints: &mut [c_int],
    profile: c_int,
    want: u8,
) -> bool {
    unsafe {
        let mut num_entrypoints = entrypoints.len() as c_int;
        if (api.va_query_config_entrypoints)(
            display,
            profile,
            entrypoints.as_mut_ptr(),
            &mut num_entrypoints,
        ) != VA_STATUS_SUCCESS
        {
            return false;
        }
        for entrypoint in entrypoints[..num_entrypoints as usize].iter().copied() {
            if entrypoint_show_type(entrypoint) != want {
                continue;
            }
            let mut attrib = VaConfigAttrib {
                type_: VA_CONFIG_ATTRIB_RT_FORMAT,
                value: 0,
            };
            if (api.va_get_config_attributes)(display, profile, entrypoint, &mut attrib, 1)
                == VA_STATUS_SUCCESS
                && attrib.value != VA_ATTRIB_NOT_SUPPORTED
                && attrib.value != 0
            {
                return true;
            }
        }
        false
    }
}

struct CodecResult {
    gpu: String,
    decoders: CodecType,
    encoders: CodecType,
    /// The detection backend that produced the entry.
    platform_api: &'static str,
}

/// One successful display: the codec bitmask split by entrypoint kind.
unsafe fn query_display(api: &VaApi, display: Display) -> Option<CodecResult> {
    unsafe {
        let max_profiles = (api.va_max_num_profiles)(display);
        let max_entrypoints = (api.va_max_num_entrypoints)(display);
        if max_profiles <= 0 || max_entrypoints <= 0 {
            return None;
        }
        let mut profiles = vec![0 as c_int; max_profiles as usize];
        let mut num_profiles = max_profiles;
        if (api.va_query_config_profiles)(display, profiles.as_mut_ptr(), &mut num_profiles)
            != VA_STATUS_SUCCESS
        {
            return None;
        }

        let mut entrypoints = vec![0 as c_int; max_entrypoints as usize];
        let mut decoders = CodecType(0);
        let mut encoders = CodecType(0);
        for profile in profiles[..num_profiles as usize].iter().copied() {
            let type_ = va_profile_to_type(profile);
            if decoders.0 & type_.0 == 0
                && profile_has_output(api, display, &mut entrypoints, profile, 1)
            {
                decoders.0 |= type_.0;
            }
            if encoders.0 & type_.0 == 0
                && profile_has_output(api, display, &mut entrypoints, profile, 2)
            {
                encoders.0 |= type_.0;
            }
        }
        if decoders.0 == 0 && encoders.0 == 0 {
            return None;
        }
        let vendor = (api.va_query_vendor_string)(display);
        let gpu = if vendor.is_null() {
            String::new()
        } else {
            // SAFETY: vaQueryVendorString returns a NUL-terminated static
            // string for an initialized display.
            std::ffi::CStr::from_ptr(vendor).to_string_lossy().into_owned()
        };
        Some(CodecResult { gpu, decoders, encoders, platform_api: "VA-API (DRM)" })
    }
}

/// detectCodecByVaDrm: every renderD node answers one VA-API session.
fn detect_by_va_drm(api: &VaApi) -> Result<Vec<CodecResult>, &'static str> {
    let Ok(entries) = std::fs::read_dir("/dev/dri") else {
        return Err("opendir(/dev/dri/) failed");
    };
    let mut results = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("renderD") {
            continue;
        }
        let Some(path) = CString::new(entry.path().as_os_str().as_encoded_bytes()).ok() else {
            continue;
        };
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
        if fd < 0 {
            continue;
        }
        unsafe {
            let display = (api.va_get_display_drm)(fd);
            if !display.is_null() {
                let (mut major, mut minor) = (0, 0);
                if (api.va_initialize)(display, &mut major, &mut minor) == VA_STATUS_SUCCESS
                    && let Some(codec) = query_display(api, display)
                {
                    results.push(codec);
                }
                (api.va_terminate)(display);
            }
            libc::close(fd);
        }
    }
    if results.is_empty() {
        return Err("No DRM device could initialize VA-API");
    }
    Ok(results)
}

/// ffDetectCodecNative reduced to the DRM path: load the libraries, walk
/// /dev/dri, and surface the upstream error when nothing initializes.
fn detect() -> Result<Vec<CodecResult>, &'static str> {
    let Some(api) = VaApi::load() else {
        return Err("dlopen(libva.so) failed");
    };
    detect_by_va_drm(&api)
}

/// Module options: showType both, splitGPU off (ffInitCodecOptions).
#[derive(Default)]
pub(crate) struct CodecOptions {
    split_gpu: bool,
    show_encoder: bool,
    show_decoder: bool,
}

impl CodecOptions {
    fn from_instance(instance: &Instance) -> Self {
        let mut options = Self { show_encoder: true, show_decoder: true, ..Self::default() };
        let Some(entry) = instance.entry else {
            return options;
        };
        options.split_gpu = entry.get("splitGPU").and_then(crate::jsonc::Value::as_bool).unwrap_or(false);
        match entry.get("showType").and_then(crate::jsonc::Value::as_str) {
            Some("encoder") => {
                options.show_encoder = true;
                options.show_decoder = false;
            },
            Some("decoder") => {
                options.show_encoder = false;
                options.show_decoder = true;
            },
            Some("both") => {
                options.show_encoder = true;
                options.show_decoder = true;
            },
            _ => {},
        }
        options
    }
}

/// printCodecLine: "Codec (Direction - gpu)" when the device string is
/// non-empty, "Codec (Direction)" otherwise; empty lists print "None".
fn codec_row(direction: &str, gpu: &str, types: CodecType) -> Row {
    let mut value = String::new();
    for type_ in VA_TYPES {
        if types.0 & type_.0 != 0 {
            if !value.is_empty() {
                value.push_str(", ");
            }
            value.push_str(codec_type_name(type_));
        }
    }
    if value.is_empty() {
        value.push_str("None");
    }
    let key = if gpu.is_empty() {
        format!("Codec ({direction})")
    } else {
        format!("Codec ({direction} - {gpu})")
    };
    Row::new(&key, value)
}

/// ffPrintCodec: with splitGPU, one Encoder/Decoder row per device; else
/// the devices merged with an empty gpu string.
pub fn run(instance: &Instance) -> Vec<Row> {
    let options = CodecOptions::from_instance(instance);
    let Ok(results) = detect() else {
        // The load/detection error rows are hidden by default.
        return Vec::new();
    };
    if results.is_empty() {
        // "No hardware video acceleration found" is hidden by default.
        return Vec::new();
    }

    let mut rows = Vec::new();
    if options.split_gpu {
        for result in &results {
            if options.show_encoder {
                rows.push(codec_row("Encoder", &result.gpu, result.encoders));
            }
            if options.show_decoder {
                rows.push(codec_row("Decoder", &result.gpu, result.decoders));
            }
        }
    } else {
        let mut merged = CodecResult {
            gpu: String::new(),
            decoders: CodecType(0),
            encoders: CodecType(0),
            platform_api: "",
        };
        for result in &results {
            merged.decoders.0 |= result.decoders.0;
            merged.encoders.0 |= result.encoders.0;
            merged.platform_api = result.platform_api;
        }
        if options.show_encoder {
            rows.push(codec_row("Encoder", &merged.gpu, merged.encoders));
        }
        if options.show_decoder {
            rows.push(codec_row("Decoder", &merged.gpu, merged.decoders));
        }
    }
    rows
}

/// ffGenerateCodecJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(results) => {
            let mut arr = Json::Array(Vec::new());
            for result in &results {
                let mut obj = Json::obj();
                obj.push("gpu", result.gpu.clone());

                let mut encoders = Json::Array(Vec::new());
                let mut decoders = Json::Array(Vec::new());
                for type_ in VA_TYPES {
                    if result.encoders.0 & type_.0 != 0 {
                        encoders.push_value(codec_type_name(type_));
                    }
                    if result.decoders.0 & type_.0 != 0 {
                        decoders.push_value(codec_type_name(type_));
                    }
                }
                obj.push("encoders", encoders);
                obj.push("decoders", decoders);
                obj.push("platformApi", result.platform_api);
                arr.push_value(obj);
            }
            JsonOutcome::Result(arr)
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn maps_va_profiles_and_entrypoints() {
        assert_eq!(super::va_profile_to_type(19).0, 1 << 13); // VP9Profile0
        assert_eq!(super::va_profile_to_type(17).0, 1 << 12); // HEVCMain
        assert_eq!(super::va_profile_to_type(0).0, 1 << 5); // MPEG2Simple
        assert_eq!(super::va_profile_to_type(11).0, 1 << 2); // H263Baseline
        assert_eq!(super::va_profile_to_type(999).0, 0);
        assert_eq!(super::entrypoint_show_type(1), 1); // VLD decodes
        assert_eq!(super::entrypoint_show_type(6), 2); // EncSlice encodes
        assert_eq!(super::entrypoint_show_type(10), 0); // VideoProc is neither
    }

    #[test]
    fn joins_codec_names() {
        let names: Vec<&str> = super::VA_TYPES
            .iter()
            .filter(|t| super::AV1.0 & t.0 != 0 || super::H264.0 & t.0 != 0)
            .map(|t| super::codec_type_name(*t))
            .collect();
        assert_eq!(names, ["H.264", "AV1"]);
    }
}

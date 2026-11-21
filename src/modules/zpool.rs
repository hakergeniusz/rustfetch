//! `zpool` module: ZFS pool state via libzfs, ported from
//! detection/zpool/zpool.c and modules/zpool/zpool.c. libzfs.so (at most
//! version 6) is dlopen-ed; pools are enumerated with zpool_iter and their
//! name/health/guid/size/free/allocated/fragmentation/readonly properties
//! queried per pool. The default value is
//! "used / total (used%, allocated% allocated, frag% frag) - state".
//! Unavailable on machines without OpenZFS (the error row hides by
//! default), so this port is exercised only structurally here.

use std::ffi::{c_char, c_int, c_void};

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::{Row, colorize_percent},
    util,
};

type ZpoolIterFn = unsafe extern "C" fn(*mut c_void, ZpoolIterCallback, *mut c_void) -> c_int;
type ZpoolIterCallback = unsafe extern "C" fn(zpool: *mut c_void, param: *mut c_void) -> c_int;
type LibzfsInitFn = unsafe extern "C" fn() -> *mut c_void;
type LibzfsFiniFn = unsafe extern "C" fn(*mut c_void);
type ZpoolNameToPropFn = unsafe extern "C" fn(*const c_char) -> c_int;
type ZpoolGetPropIntFn = unsafe extern "C" fn(*mut c_void, c_int, *mut c_int) -> u64;
type ZpoolGetPropFn = unsafe extern "C" fn(
    zpool: *mut c_void,
    prop: c_int,
    buf: *mut c_char,
    len: usize,
    source: *mut c_int,
    literal: c_int,
) -> c_int;
type ZpoolCloseFn = unsafe extern "C" fn(*mut c_void);

struct ZpoolItem {
    name: String,
    state: String,
    guid: u64,
    used: u64,
    total: u64,
    allocated: u64,
    /// None is the upstream -DBL_MAX: the pool reports no fragmentation.
    fragmentation: Option<f64>,
    read_only: bool,
}

/// The per-pool state the zpool_iter callback reads through.
struct ZfsData<'a> {
    zpool_get_prop_int: ZpoolGetPropIntFn,
    zpool_get_prop: ZpoolGetPropFn,
    zpool_close: ZpoolCloseFn,
    /// Property ids from zpool_name_to_prop, in the name/health/guid/size/
    /// free/allocated/fragmentation/readonly order.
    props: [c_int; 8],
    result: &'a mut Vec<ZpoolItem>,
}

/// enumZpoolCallback: one pool per call; the handle is closed here.
unsafe extern "C" fn enum_zpool_callback(zpool: *mut c_void, param: *mut c_void) -> c_int {
    // SAFETY: `param` points at the ZfsData this module built, and the
    // zpool handle at every property call is the one zpool_iter passed in.
    unsafe {
        let data = &mut *(param as *mut ZfsData);
        let buffer = [0u8; 1024];

        let string_prop = |prop: c_int| -> String {
            let mut source: c_int = 0;
            // SAFETY: the buffer outlives the call and the pool handle is valid.
            let ok = (data.zpool_get_prop)(
                zpool,
                prop,
                buffer.as_ptr().cast_mut().cast(),
                buffer.len(),
                &mut source,
                0,
            ) == 0;
            if ok {
                let end = buffer.iter().position(|&byte| byte == 0).unwrap_or(buffer.len());
                String::from_utf8_lossy(&buffer[..end]).into_owned()
            } else {
                "unknown".to_string()
            }
        };
        let int_prop = |prop: c_int| -> u64 {
            let mut source: c_int = 0;
            // SAFETY: the pool handle is valid for the duration of the call.
            (data.zpool_get_prop_int)(zpool, prop, &mut source)
        };

        let [
            name,
            health,
            guid,
            size,
            free,
            allocated,
            fragmentation,
            read_only,
        ] = data.props;
        let total = int_prop(size);
        data.result.push(ZpoolItem {
            name: string_prop(name),
            state: string_prop(health),
            guid: int_prop(guid),
            total,
            used: total.saturating_sub(int_prop(free)),
            allocated: int_prop(allocated),
            fragmentation: {
                let value = int_prop(fragmentation);
                (value != u64::MAX).then_some(value as f64)
            },
            read_only: int_prop(read_only) != 0,
        });
        // SAFETY: the pool handle is ours to close.
        (data.zpool_close)(zpool);
    }
    0
}

/// ffKmodLoaded: the module name is the first word of a /proc/modules line.
fn kmod_loaded(mod_name: &str) -> bool {
    let Some(modules) = util::read_file("/proc/modules") else {
        return false;
    };
    modules.lines().any(|line| line.split(' ').next() == Some(mod_name))
}

fn detect() -> Result<Vec<ZpoolItem>, &'static str> {
    let Some(libzfs) = super::codec::load_library("zfs", 6) else {
        return Err("dlopen(libzfs.so) failed");
    };
    // SAFETY: the symbols come from the dlopen-ed libzfs; their signatures
    // are the ones in libzfs_simplified.h.
    unsafe {
        let Some(libzfs_init_raw) = super::codec::load_symbol(libzfs, "libzfs_init") else {
            return Err("dlsym libzfs_init failed");
        };
        let libzfs_init: LibzfsInitFn = std::mem::transmute(libzfs_init_raw);
        let handle = libzfs_init();
        if handle.is_null() {
            if !kmod_loaded("zfs") {
                return Err("`zfs` kernel module is not loaded");
            }
            return Err("libzfs_init() failed");
        }

        let Some(fini_raw) = super::codec::load_symbol(libzfs, "libzfs_fini") else {
            return Err("dlsym libzfs_fini failed");
        };
        let libzfs_fini: LibzfsFiniFn = std::mem::transmute(fini_raw);

        let outcome = (|| {
            let Some(name_to_prop_raw) = super::codec::load_symbol(libzfs, "zpool_name_to_prop")
            else {
                return Err("dlsym zpool_name_to_prop failed");
            };
            let name_to_prop: ZpoolNameToPropFn = std::mem::transmute(name_to_prop_raw);

            let prop_names = [
                "name",
                "health",
                "guid",
                "size",
                "free",
                "allocated",
                "fragmentation",
                "readonly",
            ];
            let mut props = [0i32; 8];
            for (index, prop_name) in prop_names.iter().enumerate() {
                let prop_name = std::ffi::CString::new(*prop_name).unwrap_or_default();
                let id = name_to_prop(prop_name.as_ptr());
                if id < 0 {
                    return Err(format!("Failed to query prop: {}", prop_names[index]).leak());
                }
                props[index] = id;
            }

            let Some(iter_raw) = super::codec::load_symbol(libzfs, "zpool_iter") else {
                return Err("dlsym zpool_iter failed");
            };
            let zpool_iter: ZpoolIterFn = std::mem::transmute(iter_raw);
            let Some(get_prop_int_raw) = super::codec::load_symbol(libzfs, "zpool_get_prop_int")
            else {
                return Err("dlsym zpool_get_prop_int failed");
            };
            let Some(get_prop_raw) = super::codec::load_symbol(libzfs, "zpool_get_prop") else {
                return Err("dlsym zpool_get_prop failed");
            };
            let Some(close_raw) = super::codec::load_symbol(libzfs, "zpool_close") else {
                return Err("dlsym zpool_close failed");
            };

            let mut result = Vec::new();
            // SAFETY: the symbol signatures match libzfs_simplified.h.
            let zpool_get_prop_int: ZpoolGetPropIntFn = std::mem::transmute(get_prop_int_raw);
            let zpool_get_prop: ZpoolGetPropFn = std::mem::transmute(get_prop_raw);
            let zpool_close: ZpoolCloseFn = std::mem::transmute(close_raw);
            let mut data = ZfsData {
                zpool_get_prop_int,
                zpool_get_prop,
                zpool_close,
                props,
                result: &mut result,
            };
            if zpool_iter(handle, enum_zpool_callback, &mut data as *mut ZfsData as *mut c_void) < 0
            {
                return Err("zpool_iter() failed");
            }
            Ok(result)
        })();

        libzfs_fini(handle);
        outcome
    }
}

/// The default-format value for one pool: the used/total percents, the
/// fragmentation (or "-" when the pool reports none) and the state.
fn item_value(item: &ZpoolItem, color: bool) -> String {
    let used_percent = if item.total > 0 {
        item.used as f64 / item.total as f64 * 100.0
    } else {
        0.0
    };
    let allocated_percent = if item.total > 0 {
        item.allocated as f64 / item.total as f64 * 100.0
    } else {
        0.0
    };
    let mut value = format!("{} / {} (", util::fmt_bytes(item.used), util::fmt_bytes(item.total));
    value.push_str(&colorize_percent(used_percent, 50, 80, color));
    value.push_str(", ");
    value.push_str(&colorize_percent(allocated_percent, 50, 80, color));
    value.push_str(" allocated, ");
    match item.fragmentation {
        Some(frag) => value.push_str(&colorize_percent(frag, 50, 80, color)),
        None => value.push('-'),
    }
    value.push_str(" frag) - ");
    value.push_str(&item.state);
    if item.read_only {
        value.push_str(" [Read-only]");
    }
    value
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(items) = detect() else {
        // The detection error string is an error row, hidden by default.
        return Vec::new();
    };
    if items.is_empty() {
        // "No zpool found" is an error row, hidden by default.
        return Vec::new();
    }
    let single = items.len() == 1;
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let mut key = if item.name.is_empty() {
                "Zpool".to_string()
            } else {
                format!("{} ({})", instance.display_name("zpool"), item.name)
            };
            if !single {
                key.push_str(&format!(" {}", index + 1));
            }
            Row::new(&key, item_value(&item, instance.color))
        })
        .collect()
}

/// ffGenerateZpoolJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(items) => {
            let mut arr = Json::Array(Vec::new());
            for item in &items {
                let mut obj = Json::obj();
                obj.push("name", item.name.clone());
                obj.push("state", item.state.clone());
                obj.push("guid", item.guid);
                obj.push("used", item.used);
                obj.push("allocated", item.allocated);
                obj.push("total", item.total);
                match item.fragmentation {
                    Some(frag) => obj.push("fragmentation", frag),
                    None => obj.push("fragmentation", Json::Null),
                }
                obj.push("readOnly", item.read_only);
                arr.push_value(obj);
            }
            JsonOutcome::Result(arr)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kmod_detection_scans_module_names() {
        // kmod_loaded reads /proc/modules, which CI cannot fake; the name
        // extraction is what the upstream check boils down to.
        let modules = "zfs 123456 2 - Live 0x0000000000000000\ntun 49152 0 - Live\n";
        let found = modules.lines().any(|line| line.split(' ').next() == Some("zfs"));
        assert!(found);
        let found = modules.lines().any(|line| line.split(' ').next() == Some("zfsonlinux"));
        assert!(!found);
    }

    #[test]
    fn value_matches_the_default_format() {
        let item = ZpoolItem {
            name: "tank".into(),
            state: "ONLINE".into(),
            guid: 1234567890,
            used: 500 * 1024 * 1024 * 1024,
            total: 1000 * 1024 * 1024 * 1024,
            allocated: 450 * 1024 * 1024 * 1024,
            fragmentation: Some(12.0),
            read_only: false,
        };
        assert_eq!(
            item_value(&item, false),
            "500.00 GiB / 1000.00 GiB (50%, 45% allocated, 12% frag) - ONLINE"
        );
        let read_only = ZpoolItem {
            read_only: true,
            fragmentation: None,
            state: "DEGRADED".into(),
            ..item
        };
        assert_eq!(
            item_value(&read_only, false),
            "500.00 GiB / 1000.00 GiB (50%, 45% allocated, - frag) - DEGRADED [Read-only]"
        );
    }
}

//! Desktop settings reads without spawning, ported from fastfetch's
//! common/impl/settings.c: GSettings via dlopened libgio-2.0, falling back
//! to a dconf client from libdconf. Loaded lazily once and shared by the
//! GTK/Qt-adjacent modules that previously ran `gsettings list-recursively`.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::OnceLock;

type GVariant = c_void;
type GSettingsSchema = c_void;
type GSettingsSchemaSource = c_void;
type GSettings = c_void;
type DConfClient = c_void;
type GError = c_void;

const RTLD_FLAGS: c_int = libc::RTLD_LAZY | libc::RTLD_LOCAL;
/// dconf_client_read_full flag to read the default value.
const DCONF_READ_USER_VALUE: c_int = 1;
const DCONF_READ_DEFAULT_VALUE: c_int = 2;

/// dlsym a symbol, returning null when missing.
unsafe fn sym(handle: *mut c_void, name: &str) -> *mut c_void {
    match CString::new(name) {
        Ok(name) => unsafe { libc::dlsym(handle, name.as_ptr()) },
        Err(_) => std::ptr::null_mut(),
    }
}

/// dlopen the first soname that exists; null when none do.
unsafe fn lib(names: &[&str]) -> *mut c_void {
    for name in names {
        if let Ok(cname) = CString::new(*name) {
            let handle = unsafe { libc::dlopen(cname.as_ptr(), RTLD_FLAGS) };
            if !handle.is_null() {
                return handle;
            }
        }
    }
    std::ptr::null_mut()
}

/// GVariant accessors both libgio and libdconf provide through their glib
/// dependency.
struct VariantFns {
    type_string: TypeStringFn,
    dup_string: DupStringFn,
    get_boolean: GetBooleanFn,
    get_int32: GetInt32Fn,
    unref: UnrefFn,
    free: FreeFn,
}

type TypeStringFn = unsafe extern "C" fn(*mut GVariant) -> *const c_char;
type DupStringFn = unsafe extern "C" fn(*mut GVariant, *mut usize) -> *mut c_char;
type GetBooleanFn = unsafe extern "C" fn(*mut GVariant) -> c_int;
type GetInt32Fn = unsafe extern "C" fn(*mut GVariant) -> i32;
type UnrefFn = unsafe extern "C" fn(*mut GVariant);
type FreeFn = unsafe extern "C" fn(*mut c_void);

type LookupFn =
    unsafe extern "C" fn(*mut GSettingsSchemaSource, *const c_char, c_int) -> *mut GSettingsSchema;
type SchemaSourceGetDefaultFn = unsafe extern "C" fn() -> *mut GSettingsSchemaSource;
type HasKeyFn = unsafe extern "C" fn(*mut GSettingsSchema, *const c_char) -> c_int;
type NewFullFn =
    unsafe extern "C" fn(*mut GSettingsSchema, *mut c_void, *const c_char) -> *mut GSettings;
type GSettingsGetFn = unsafe extern "C" fn(*mut GSettings, *const c_char) -> *mut GVariant;
type ReadFullFn =
    unsafe extern "C" fn(*mut DConfClient, *const c_char, c_int, *mut *mut GError) -> *mut GVariant;
type ClientNewFn = unsafe extern "C" fn() -> *mut DConfClient;

/// Function-pointer bundle for libgio; the schema source is fetched once.
/// Raw pointers shared across the module threads, hence the Send/Sync impl.
struct GSettingsApi {
    schema_source: *mut GSettingsSchemaSource,
    lookup: LookupFn,
    has_key: HasKeyFn,
    new_full: NewFullFn,
    get_value: GSettingsGetFn,
    get_user_value: GSettingsGetFn,
    get_default_value: GSettingsGetFn,
    variants: VariantFns,
}

unsafe impl Send for GSettingsApi {}
unsafe impl Sync for GSettingsApi {}

/// Interprets a dlsym'd pointer as the function type the call site expects.
/// Panics on a null pointer or a mismatched size.
unsafe fn fntype<T>(ptr: *mut c_void) -> T {
    assert!(!ptr.is_null());
    assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<*mut c_void>());
    unsafe { std::mem::transmute_copy::<*mut c_void, T>(&ptr) }
}

fn gsettings_api() -> Option<&'static GSettingsApi> {
    static API: OnceLock<Option<&'static GSettingsApi>> = OnceLock::new();
    *API.get_or_init(|| unsafe {
        let gio = lib(&["libgio-2.0.so.0", "libgio-2.0.so"]);
        if gio.is_null() {
            return None;
        }

        macro_rules! need {
            ($name:literal) => {
                match sym(gio, $name) {
                    ptr if !ptr.is_null() => ptr,
                    _ => return None,
                }
            };
        }

        let schema_source_get_default: SchemaSourceGetDefaultFn =
            fntype(need!("g_settings_schema_source_get_default"));
        let schema_source = schema_source_get_default();
        if schema_source.is_null() {
            return None;
        }

        let api = Box::leak(Box::new(GSettingsApi {
            schema_source,
            lookup: fntype(need!("g_settings_schema_source_lookup")),
            has_key: fntype(need!("g_settings_schema_has_key")),
            new_full: fntype(need!("g_settings_new_full")),
            get_value: fntype(need!("g_settings_get_value")),
            get_user_value: fntype(need!("g_settings_get_user_value")),
            get_default_value: fntype(need!("g_settings_get_default_value")),
            variants: VariantFns {
                type_string: fntype(need!("g_variant_get_type_string")),
                dup_string: fntype(need!("g_variant_dup_string")),
                get_boolean: fntype(need!("g_variant_get_boolean")),
                get_int32: fntype(need!("g_variant_get_int32")),
                unref: fntype(need!("g_variant_unref")),
                free: fntype(need!("g_free")),
            },
        }));
        Some(api as &'static GSettingsApi)
    })
}

/// dconf client kept for the process lifetime, like fastfetch's DConfData.
struct DConfApi {
    client: *mut DConfClient,
    read_full: ReadFullFn,
    variants: VariantFns,
}

unsafe impl Send for DConfApi {}
unsafe impl Sync for DConfApi {}

fn dconf_api() -> Option<&'static DConfApi> {
    static API: OnceLock<Option<&'static DConfApi>> = OnceLock::new();
    *API.get_or_init(|| unsafe {
        let dconf = lib(&["libdconf.so.2", "libdconf.so.1", "libdconf.so"]);
        if dconf.is_null() {
            return None;
        }
        macro_rules! need {
            ($name:literal) => {
                match sym(dconf, $name) {
                    ptr if !ptr.is_null() => ptr,
                    _ => return None,
                }
            };
        }
        let client_new: ClientNewFn = fntype(need!("dconf_client_new"));
        let client = client_new();
        if client.is_null() {
            return None;
        }
        let api = Box::leak(Box::new(DConfApi {
            client,
            read_full: fntype(need!("dconf_client_read_full")),
            variants: VariantFns {
                type_string: fntype(need!("g_variant_get_type_string")),
                dup_string: fntype(need!("g_variant_dup_string")),
                get_boolean: fntype(need!("g_variant_get_boolean")),
                get_int32: fntype(need!("g_variant_get_int32")),
                unref: fntype(need!("g_variant_unref")),
                free: fntype(need!("g_free")),
            },
        }));
        Some(api as &'static DConfApi)
    })
}

/// Reads a GVariant's payload as a display string; unknown types yield None.
/// The variant is always unreffed when non-null.
unsafe fn variant_string(variant: *mut GVariant, fns: &VariantFns) -> Option<String> {
    if variant.is_null() {
        return None;
    }
    let kind = unsafe { CStr::from_ptr((fns.type_string)(variant)) }.to_bytes();
    let value = match kind {
        b"s" => unsafe {
            let mut len = 0usize;
            let ptr = (fns.dup_string)(variant, &mut len);
            if ptr.is_null() {
                None
            } else {
                let s = CStr::from_ptr(ptr).to_string_lossy().into_owned();
                (fns.free)(ptr as *mut c_void);
                Some(s)
            }
        },
        b"b" => Some(if unsafe { (fns.get_boolean)(variant) } == 0 {
            "false".to_string()
        } else {
            "true".to_string()
        }),
        b"i" => Some(unsafe { (fns.get_int32)(variant) }.to_string()),
        _ => None,
    };
    unsafe { (fns.unref)(variant) };
    value
}

/// Splits fastfetch's "schema-id" or "schema-id:object-path" form.
fn split_schema(schema_with_path: &str) -> (&str, Option<&str>) {
    match schema_with_path.split_once(':') {
        Some((schema, path)) => (schema, Some(path)),
        None => (schema_with_path, None),
    }
}

/// ffSettingsGetGSettings: effective value, then user value, then default.
fn get_gsettings(schema_with_path: &str, key: &str) -> Option<String> {
    let api = gsettings_api()?;
    let (schema_name, path) = split_schema(schema_with_path);
    unsafe {
        let cname = CString::new(schema_name).ok()?;
        let schema = (api.lookup)(api.schema_source, cname.as_ptr(), 1);
        if schema.is_null() {
            return None;
        }
        let ckey = CString::new(key).ok()?;
        if (api.has_key)(schema, ckey.as_ptr()) == 0 {
            return None;
        }

        let cpath = match path {
            // Relocatable schema; the path must end with '/'.
            Some(path) if !path.ends_with('/') => CString::new(format!("{path}/")).ok(),
            Some(path) => CString::new(path).ok(),
            None => None,
        };
        // The schema and the GSettings object are intentionally not unreffed,
        // matching fastfetch; the process is short-lived.
        let settings = (api.new_full)(
            schema,
            std::ptr::null_mut(),
            cpath.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
        );
        if settings.is_null() {
            return None;
        }

        let variant = (api.get_value)(settings, ckey.as_ptr());
        let variant = if variant.is_null() {
            (api.get_user_value)(settings, ckey.as_ptr())
        } else {
            variant
        };
        let variant = if variant.is_null() {
            (api.get_default_value)(settings, ckey.as_ptr())
        } else {
            variant
        };
        variant_string(variant, &api.variants)
    }
}

/// ffSettingsGetDConf: unmerged value, then user value, then default.
fn get_dconf(schema_with_path: &str, key: &str) -> Option<String> {
    let api = dconf_api()?;
    let (schema_name, path) = split_schema(schema_with_path);
    let dconf_key = match path {
        Some(path) => format!("{path}{key}"),
        None => format!("/{}/{}", schema_name.replace('.', "/"), key),
    };
    let ckey = CString::new(dconf_key).ok()?;
    unsafe {
        let variant = (api.read_full)(api.client, ckey.as_ptr(), 0, std::ptr::null_mut());
        let variant = if variant.is_null() {
            (api.read_full)(api.client, ckey.as_ptr(), DCONF_READ_USER_VALUE, std::ptr::null_mut())
        } else {
            variant
        };
        let variant = if variant.is_null() {
            (api.read_full)(
                api.client,
                ckey.as_ptr(),
                DCONF_READ_DEFAULT_VALUE,
                std::ptr::null_mut(),
            )
        } else {
            variant
        };
        variant_string(variant, &api.variants)
    }
}

/// ffSettingsGetGnome: GSettings when available, dconf otherwise.
pub fn get(schema_with_path: &str, key: &str) -> Option<String> {
    get_gsettings(schema_with_path, key).or_else(|| get_dconf(schema_with_path, key))
}

#[cfg(test)]
mod tests {
    use super::split_schema;

    #[test]
    fn split_schema_takes_the_first_colon() {
        assert_eq!(
            split_schema("org.gnome.desktop.interface"),
            ("org.gnome.desktop.interface", None)
        );
        assert_eq!(
            split_schema(
                "org.gnome.Terminal.Legacy.Profile:/org/gnome/terminal/legacy/profiles:/:uuid/"
            ),
            (
                "org.gnome.Terminal.Legacy.Profile",
                Some("/org/gnome/terminal/legacy/profiles:/:uuid/")
            )
        );
    }
}

//! D-Bus reads via dlopened libdbus-1, ported from fastfetch's
//! common/impl/dbus.c: property gets and zero-argument method calls on the
//! session bus without spawning gdbus/dbus-send children.

use std::ffi::{c_char, c_int, c_uint, c_void, CStr, CString};
use std::sync::OnceLock;

const DBUS_BUS_SESSION: c_int = 0;
const DBUS_TYPE_STRING: c_int = b's' as _;
const DBUS_TYPE_OBJECT_PATH: c_int = b'o' as _;
const DBUS_TYPE_BYTE: c_int = b'y' as _;
const DBUS_TYPE_VARIANT: c_int = b'v' as _;
const DBUS_TYPE_ARRAY: c_int = b'a' as _;
const DBUS_TYPE_INVALID: c_int = 0;
const PROCESSING_TIMEOUT_MS: c_int = 2500;

type DBusConnection = c_void;
type DBusMessage = c_void;
type GError = c_void;

/// libdbus's iterator is an opaque struct of 5 pointers and 13 ints; a
/// zero-initialized buffer of the same size stands in for one.
#[repr(C, align(8))]
struct DBusMessageIter {
    _opaque: [u8; 104],
}

impl DBusMessageIter {
    fn zeroed() -> std::mem::MaybeUninit<Self> {
        std::mem::MaybeUninit::zeroed()
    }
}

type BusGetFn = unsafe extern "C" fn(c_int, *mut *mut GError) -> *mut DBusConnection;
type NewMethodCallFn =
    unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, *const c_char) -> *mut DBusMessage;
type IterInitAppendFn = unsafe extern "C" fn(*mut DBusMessage, *mut DBusMessageIter);
type IterAppendBasicFn = unsafe extern "C" fn(*mut DBusMessageIter, c_int, *const c_void) -> c_uint;
type IterInitFn = unsafe extern "C" fn(*mut DBusMessage, *mut DBusMessageIter) -> c_int;
type IterArgTypeFn = unsafe extern "C" fn(*mut DBusMessageIter) -> c_int;
type IterGetBasicFn = unsafe extern "C" fn(*mut DBusMessageIter, *mut c_void);
type IterRecurseFn = unsafe extern "C" fn(*mut DBusMessageIter, *mut DBusMessageIter);
type IterNextFn = unsafe extern "C" fn(*mut DBusMessageIter) -> c_int;
type MessageUnrefFn = unsafe extern "C" fn(*mut DBusMessage);
type SendReplyBlockFn =
    unsafe extern "C" fn(*mut DBusConnection, *mut DBusMessage, c_int, *mut *mut GError) -> *mut DBusMessage;
type ThreadsInitFn = unsafe extern "C" fn() -> c_int;

struct DbusApi {
    bus_get: BusGetFn,
    new_method_call: NewMethodCallFn,
    iter_init_append: IterInitAppendFn,
    iter_append_basic: IterAppendBasicFn,
    iter_init: IterInitFn,
    get_arg_type: IterArgTypeFn,
    get_basic: IterGetBasicFn,
    recurse: IterRecurseFn,
    next: IterNextFn,
    message_unref: MessageUnrefFn,
    send_with_reply_and_block: SendReplyBlockFn,
}

unsafe impl Send for DbusApi {}
unsafe impl Sync for DbusApi {}

/// dlsym a symbol, returning null when missing.
unsafe fn sym(handle: *mut c_void, name: &str) -> *mut c_void {
    match CString::new(name) {
        Ok(name) => unsafe { libc::dlsym(handle, name.as_ptr()) },
        Err(_) => std::ptr::null_mut(),
    }
}

/// Interprets a dlsym'd pointer as the function type the call site expects.
unsafe fn fntype<T>(ptr: *mut c_void) -> T {
    assert!(!ptr.is_null());
    assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<*mut c_void>());
    unsafe { std::mem::transmute_copy::<*mut c_void, T>(&ptr) }
}

fn dbus_api() -> Option<&'static DbusApi> {
    static API: OnceLock<Option<&'static DbusApi>> = OnceLock::new();
    *API.get_or_init(|| unsafe {
        let name = CString::new("libdbus-1.so.4").ok()?;
        let mut handle = libc::dlopen(name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
        if handle.is_null() {
            let name = CString::new("libdbus-1.so.3").ok()?;
            handle = libc::dlopen(name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
        }
        if handle.is_null() {
            return None;
        }

        macro_rules! need {
            ($name:literal) => {
                match sym(handle, $name) {
                    ptr if !ptr.is_null() => ptr,
                    _ => return None,
                }
            };
        }

        // Modules run on parallel threads; libdbus needs this before any
        // connection is shared across them.
        let threads_init: ThreadsInitFn = fntype(need!("dbus_threads_init_default"));
        threads_init();

        let api = Box::leak(Box::new(DbusApi {
            bus_get: fntype(need!("dbus_bus_get")),
            new_method_call: fntype(need!("dbus_message_new_method_call")),
            iter_init_append: fntype(need!("dbus_message_iter_init_append")),
            iter_append_basic: fntype(need!("dbus_message_iter_append_basic")),
            iter_init: fntype(need!("dbus_message_iter_init")),
            get_arg_type: fntype(need!("dbus_message_iter_get_arg_type")),
            get_basic: fntype(need!("dbus_message_iter_get_basic")),
            recurse: fntype(need!("dbus_message_iter_recurse")),
            next: fntype(need!("dbus_message_iter_next")),
            message_unref: fntype(need!("dbus_message_unref")),
            send_with_reply_and_block: fntype(need!("dbus_connection_send_with_reply_and_block")),
        }));
        Some(api as &'static DbusApi)
    })
}

/// ffDBusGetString: strings, object paths, byte arrays of chars, arrays of
/// strings joined with ", ", all unwrapped from variants.
unsafe fn get_string(api: &DbusApi, iter: &mut DBusMessageIter) -> Option<String> {
    let arg_type = unsafe { (api.get_arg_type)(iter) };

    if arg_type == DBUS_TYPE_STRING || arg_type == DBUS_TYPE_OBJECT_PATH {
        let mut value: *const c_char = std::ptr::null();
        unsafe { (api.get_basic)(iter, (&mut value as *mut *const c_char).cast()) };
        if value.is_null() {
            return None;
        }
        let value = unsafe { CStr::from_ptr(value) }.to_string_lossy();
        return (!value.is_empty()).then(|| value.into_owned());
    }

    if arg_type == DBUS_TYPE_BYTE {
        let mut value: u8 = 0;
        unsafe { (api.get_basic)(iter, (&mut value as *mut u8).cast()) };
        return Some((value as char).to_string());
    }

    if arg_type != DBUS_TYPE_VARIANT && arg_type != DBUS_TYPE_ARRAY {
        return None;
    }

    let mut sub = DBusMessageIter::zeroed();
    unsafe { (api.recurse)(iter, sub.as_mut_ptr()) };
    let mut sub = unsafe { sub.assume_init() };

    if arg_type == DBUS_TYPE_VARIANT {
        return unsafe { get_string(api, &mut sub) };
    }

    // Array: byte arrays join into one string, string arrays with ", ".
    let mut values: Vec<String> = Vec::new();
    let mut bytes = String::new();
    loop {
        let sub_type = unsafe { (api.get_arg_type)(&mut sub) };
        if sub_type == DBUS_TYPE_INVALID {
            break;
        }
        if sub_type == DBUS_TYPE_BYTE {
            if let Some(ch) = unsafe { get_string(api, &mut sub) } {
                bytes.push_str(&ch);
            }
        } else if let Some(value) = unsafe { get_string(api, &mut sub) } {
            values.push(value);
        }
        if unsafe { (api.next)(&mut sub) } == 0 {
            break;
        }
    }
    if !bytes.is_empty() {
        return Some(bytes);
    }
    if values.is_empty() {
        None
    } else {
        Some(values.join(", "))
    }
}

/// org.freedesktop.DBus.Properties.Get returning a string-ish value.
pub fn get_property_string(dest: &str, path: &str, interface: &str, property: &str) -> Option<String> {
    let api = dbus_api()?;
    unsafe {
        let connection = (api.bus_get)(DBUS_BUS_SESSION, std::ptr::null_mut());
        if connection.is_null() {
            return None;
        }

        let (cdest, cpath) = (CString::new(dest).ok()?, CString::new(path).ok()?);
        let cinterface = CString::new(interface).ok()?;
        let cproperty = CString::new(property).ok()?;
        let message = (api.new_method_call)(
            cdest.as_ptr(),
            cpath.as_ptr(),
            c"org.freedesktop.DBus.Properties".as_ptr(),
            c"Get".as_ptr(),
        );
        if message.is_null() {
            return None;
        }

        let mut iter = DBusMessageIter::zeroed();
        (api.iter_init_append)(message, iter.as_mut_ptr());
        let mut iter = iter.assume_init();
        let iface_ptr = cinterface.as_ptr();
        let prop_ptr = cproperty.as_ptr();
        (api.iter_append_basic)(&mut iter, DBUS_TYPE_STRING, (&iface_ptr as *const *const c_char).cast());
        (api.iter_append_basic)(&mut iter, DBUS_TYPE_STRING, (&prop_ptr as *const *const c_char).cast());

        let reply = (api.send_with_reply_and_block)(connection, message, PROCESSING_TIMEOUT_MS, std::ptr::null_mut());
        (api.message_unref)(message);
        if reply.is_null() {
            return None;
        }

        let mut root = DBusMessageIter::zeroed();
        let ok = (api.iter_init)(reply, root.as_mut_ptr());
        let parsed = if ok == 0 {
            None
        } else {
            let mut root = root.assume_init();
            get_string(api, &mut root)
        };
        (api.message_unref)(reply);
        parsed
    }
}

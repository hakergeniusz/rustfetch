//! `opengl` module: the GL_VERSION string from a headless context, ported
//! from detection/opengl (EGL path). libEGL is dlopened, a 1x1 pbuffer
//! surface gets made current and glGetString(GL_VERSION) answers; stdout
//! and stderr are suppressed while the driver initializes, exactly like
//! FF_SUPPRESS_IO. The GLX fallback needs a full X11 ABI surface and is
//! not ported; EGL surfaceless covers headless and Wayland hosts.

use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};

use crate::{instance::Instance, modules::Row};

const EGL_TRUE: c_uint = 1;
const EGL_NONE: c_int = 0x3038;
const EGL_WIDTH: c_int = 0x3057;
const EGL_HEIGHT: c_int = 0x3056;
const EGL_OPENGL_API: c_uint = 0x30A2;
const EGL_CONTEXT_CLIENT_VERSION: c_int = 0x3098;
const EGL_PLATFORM_SURFACELESS_MESA: c_uint = 0x31DD;

const GL_VERSION: c_uint = 0x1F02;

type EglGetProcAddressFn = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type EglQueryStringFn = unsafe extern "C" fn(*mut c_void, c_int) -> *const c_char;
type EglInitializeFn = unsafe extern "C" fn(*mut c_void, *mut c_int, *mut c_int) -> c_uint;
type EglBindApiFn = unsafe extern "C" fn(c_uint) -> c_uint;
type EglGetConfigsFn =
    unsafe extern "C" fn(*mut c_void, *mut *mut c_void, c_int, *mut c_int) -> c_uint;
type EglCreatePbufferSurfaceFn =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_int) -> *mut c_void;
type EglCreateContextFn =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *const c_int) -> *mut c_void;
type EglMakeCurrentFn =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> c_uint;
type EglDestroyFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_uint;
type EglTerminateFn = unsafe extern "C" fn(*mut c_void) -> c_uint;
type EglGetDisplayFn = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type EglGetPlatformDisplayFn =
    unsafe extern "C" fn(c_uint, *mut c_void, *const c_int) -> *mut c_void;
type GlGetStringFn = unsafe extern "C" fn(c_uint) -> *const c_char;

/// ffSuppressIO: redirect fd 1/2 to /dev/null while the driver talks, then
/// restore. Safe here because rendering happens after all modules ran.
pub(crate) struct SuppressedIo {
    restored: bool,
    orig_out: c_int,
    orig_err: c_int,
    null_file: c_int,
}

impl SuppressedIo {
    pub(crate) fn new() -> Option<Self> {
        let orig_out = unsafe { libc::dup(1) };
        let orig_err = unsafe { libc::dup(2) };
        let null_file =
            unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC) };
        if orig_out < 0 || orig_err < 0 || null_file < 0 {
            return None;
        }
        unsafe {
            libc::fflush(std::ptr::null_mut());
            libc::dup2(null_file, 1);
            libc::dup2(null_file, 2);
        }
        Some(Self {
            restored: false,
            orig_out,
            orig_err,
            null_file,
        })
    }

    pub(crate) fn restore(&mut self) {
        if self.restored {
            return;
        }
        self.restored = true;
        unsafe {
            libc::fflush(std::ptr::null_mut());
            libc::dup2(self.orig_out, 1);
            libc::dup2(self.orig_err, 2);
            libc::close(self.orig_out);
            libc::close(self.orig_err);
            libc::close(self.null_file);
        }
    }
}

impl Drop for SuppressedIo {
    fn drop(&mut self) {
        self.restore();
    }
}

fn gl_string(get_string: GlGetStringFn, name: c_uint) -> Option<String> {
    unsafe {
        let pointer = (get_string)(name);
        if pointer.is_null() {
            return None;
        }
        Some(CStr::from_ptr(pointer).to_string_lossy().into_owned())
    }
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(egl) = super::codec::load_library("EGL", 1) else {
        // "libEGL.so.1 failed" is an error row, hidden by default.
        return Vec::new();
    };
    let sym = |name: &str| super::codec::load_symbol(egl, name);
    let Some(get_proc_address) = sym("eglGetProcAddress") else {
        return Vec::new();
    };
    // SAFETY: the library handle stays open for the borrowed symbols.
    let egl_get_proc_address: EglGetProcAddressFn =
        unsafe { std::mem::transmute(get_proc_address) };
    let load = |name: &str| -> Option<*mut c_void> { sym(name) };

    let Some(egl_query_string) = load("eglQueryString") else {
        return Vec::new();
    };
    let Some(egl_initialize) = load("eglInitialize") else {
        return Vec::new();
    };
    let Some(egl_bind_api) = load("eglBindAPI") else {
        return Vec::new();
    };
    let Some(egl_get_configs) = load("eglGetConfigs") else {
        return Vec::new();
    };
    let Some(egl_create_pbuffer_surface) = load("eglCreatePbufferSurface") else {
        return Vec::new();
    };
    let Some(egl_create_context) = load("eglCreateContext") else {
        return Vec::new();
    };
    let Some(egl_make_current) = load("eglMakeCurrent") else {
        return Vec::new();
    };
    let Some(egl_destroy_context) = load("eglDestroyContext") else {
        return Vec::new();
    };
    let Some(egl_destroy_surface) = load("eglDestroySurface") else {
        return Vec::new();
    };
    let Some(egl_terminate) = load("eglTerminate") else {
        return Vec::new();
    };
    let Some(egl_get_display) = load("eglGetDisplay") else {
        return Vec::new();
    };

    // EGL_VERSION string only feeds custom formats; symbol stays checked.
    let _egl_query_string: EglQueryStringFn = unsafe { std::mem::transmute(egl_query_string) };
    let egl_initialize: EglInitializeFn = unsafe { std::mem::transmute(egl_initialize) };
    let egl_bind_api: EglBindApiFn = unsafe { std::mem::transmute(egl_bind_api) };
    let egl_get_configs: EglGetConfigsFn = unsafe { std::mem::transmute(egl_get_configs) };
    let egl_create_pbuffer_surface: EglCreatePbufferSurfaceFn =
        unsafe { std::mem::transmute(egl_create_pbuffer_surface) };
    let egl_create_context: EglCreateContextFn = unsafe { std::mem::transmute(egl_create_context) };
    let egl_make_current: EglMakeCurrentFn = unsafe { std::mem::transmute(egl_make_current) };
    let egl_destroy_context: EglDestroyFn = unsafe { std::mem::transmute(egl_destroy_context) };
    let egl_destroy_surface: EglDestroyFn = unsafe { std::mem::transmute(egl_destroy_surface) };
    let egl_terminate: EglTerminateFn = unsafe { std::mem::transmute(egl_terminate) };
    let egl_get_display: EglGetDisplayFn = unsafe { std::mem::transmute(egl_get_display) };

    let gl_string_name = CString::new("glGetString").unwrap_or_default();
    let gl_string_ptr = gl_string_name.as_ptr();
    let Some(gl_string_raw) = sym_if_nonnull(unsafe { (egl_get_proc_address)(gl_string_ptr) })
    else {
        return Vec::new();
    };
    let get_gl_string: GlGetStringFn = unsafe { std::mem::transmute(gl_string_raw) };

    // Driver initialization prints to stdout/stderr; silence it.
    let Some(mut suppressed) = SuppressedIo::new() else {
        return Vec::new();
    };

    #[inline]
    fn sym_if_nonnull(pointer: *mut c_void) -> Option<*mut c_void> {
        (!pointer.is_null()).then_some(pointer)
    }

    let version = unsafe {
        // Prefer the surfaceless MESA platform, then the default display.
        let mut display: *mut c_void = std::ptr::null_mut();
        for name in ["eglGetPlatformDisplay", "eglGetPlatformDisplayEXT"] {
            let Ok(name) = CString::new(name) else {
                break;
            };
            let Some(pointer) = sym_if_nonnull((egl_get_proc_address)(name.as_ptr())) else {
                continue;
            };
            let get_platform_display: EglGetPlatformDisplayFn = std::mem::transmute(pointer);
            display = (get_platform_display)(
                EGL_PLATFORM_SURFACELESS_MESA,
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            if !display.is_null() {
                break;
            }
        }
        if display.is_null() {
            display = (egl_get_display)(std::ptr::null_mut());
        }
        if display.is_null() {
            return Vec::new();
        }

        let mut major = 0;
        let mut minor = 0;
        if (egl_initialize)(display, &mut major, &mut minor) != EGL_TRUE {
            (egl_terminate)(display);
            return Vec::new();
        }

        // Prefer desktop OpenGL; failure means the default API (usually GLES).
        let gles = (egl_bind_api)(EGL_OPENGL_API) != EGL_TRUE;

        let mut config: *mut c_void = std::ptr::null_mut();
        let mut config_count = 0;
        (egl_get_configs)(display, &mut config, 1, &mut config_count);
        if config_count == 0 {
            (egl_terminate)(display);
            return Vec::new();
        }

        let surface = (egl_create_pbuffer_surface)(
            display,
            config,
            [EGL_WIDTH, 1, EGL_HEIGHT, 1, EGL_NONE].as_ptr(),
        );
        if surface.is_null() {
            (egl_terminate)(display);
            return Vec::new();
        }

        let mut context = (egl_create_context)(
            display,
            config,
            std::ptr::null_mut(),
            [
                EGL_CONTEXT_CLIENT_VERSION,
                if gles { 2 } else { 1 },
                EGL_NONE,
            ]
            .as_ptr(),
        );
        if context.is_null() && gles {
            // Some ANGLE builds support GLES 1.1 only.
            context =
                (egl_create_context)(display, config, std::ptr::null_mut(), [EGL_NONE].as_ptr());
        }
        if context.is_null() {
            (egl_destroy_surface)(display, surface);
            (egl_terminate)(display);
            return Vec::new();
        }

        let version = if (egl_make_current)(display, surface, surface, context) != EGL_TRUE {
            None
        } else {
            gl_string(get_gl_string, GL_VERSION)
        };

        (egl_make_current)(
            display,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        (egl_destroy_context)(display, context);
        (egl_destroy_surface)(display, surface);
        (egl_terminate)(display);
        version
    };
    suppressed.restore();

    match version {
        Some(version) => vec![Row::new("OpenGL", version)],
        None => Vec::new(),
    }
}

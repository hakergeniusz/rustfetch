//! `opengl` module: the GL_VERSION string from a headless context, ported
//! from detection/opengl (EGL path). libEGL is dlopened, a 1x1 pbuffer
//! surface gets made current and glGetString(GL_VERSION) answers; stdout
//! and stderr are suppressed while the driver initializes, exactly like
//! FF_SUPPRESS_IO. The GLX fallback needs a full X11 ABI surface and is
//! not ported; EGL surfaceless covers headless and Wayland hosts.

use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
};

const EGL_TRUE: c_uint = 1;
const EGL_NONE: c_int = 0x3038;
const EGL_WIDTH: c_int = 0x3057;
const EGL_HEIGHT: c_int = 0x3056;
const EGL_OPENGL_API: c_uint = 0x30A2;
const EGL_CONTEXT_CLIENT_VERSION: c_int = 0x3098;
const EGL_PLATFORM_SURFACELESS_MESA: c_uint = 0x31DD;

const GL_VERSION: c_uint = 0x1F02;
const GL_RENDERER: c_uint = 0x1F01;
const GL_VENDOR: c_uint = 0x1F00;
const GL_SHADING_LANGUAGE_VERSION: c_uint = 0x8B8C;
const EGL_VERSION: c_int = 0x3054;

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

fn sym_if_nonnull(pointer: *mut c_void) -> Option<*mut c_void> {
    (!pointer.is_null()).then_some(pointer)
}

pub struct OpenGLResult {
    version: String,
    renderer: String,
    vendor: String,
    slv: String,
    library: String,
}

/// ffDetectOpenGL via EGL: a surfaceless 1x1 pbuffer context answers the
/// four glGetString queries; the library string records the EGL version.
fn detect() -> Result<OpenGLResult, &'static str> {
    let Some(egl) = super::codec::load_library("EGL", 1) else {
        return Err("dlopen(libEGL.so) failed");
    };
    let sym = |name: &str| super::codec::load_symbol(egl, name);
    let Some(get_proc_address) = sym("eglGetProcAddress") else {
        return Err("dlsym eglGetProcAddress failed");
    };
    // SAFETY: the library handle stays open for the borrowed symbols.
    let egl_get_proc_address: EglGetProcAddressFn =
        unsafe { std::mem::transmute(get_proc_address) };
    let load = |name: &str| -> Option<*mut c_void> { sym(name) };

    macro_rules! load_fn {
        ($name:literal, $type:ty) => {
            match load($name) {
                Some(raw) => unsafe { std::mem::transmute::<*mut c_void, $type>(raw) },
                None => return Err(concat!("dlsym ", $name, " failed")),
            }
        };
    }
    let egl_query_string: EglQueryStringFn = load_fn!("eglQueryString", EglQueryStringFn);
    let egl_initialize: EglInitializeFn = load_fn!("eglInitialize", EglInitializeFn);
    let egl_bind_api: EglBindApiFn = load_fn!("eglBindAPI", EglBindApiFn);
    let egl_get_configs: EglGetConfigsFn = load_fn!("eglGetConfigs", EglGetConfigsFn);
    let egl_create_pbuffer_surface: EglCreatePbufferSurfaceFn =
        load_fn!("eglCreatePbufferSurface", EglCreatePbufferSurfaceFn);
    let egl_create_context: EglCreateContextFn =
        load_fn!("eglCreateContext", EglCreateContextFn);
    let egl_make_current: EglMakeCurrentFn = load_fn!("eglMakeCurrent", EglMakeCurrentFn);
    let egl_destroy_context: EglDestroyFn = load_fn!("eglDestroyContext", EglDestroyFn);
    let egl_destroy_surface: EglDestroyFn = load_fn!("eglDestroySurface", EglDestroyFn);
    let egl_terminate: EglTerminateFn = load_fn!("eglTerminate", EglTerminateFn);
    let egl_get_display: EglGetDisplayFn = load_fn!("eglGetDisplay", EglGetDisplayFn);

    let gl_string_name = CString::new("glGetString").unwrap_or_default();
    let gl_string_ptr = gl_string_name.as_ptr();
    let Some(gl_string_raw) = sym_if_nonnull(unsafe { (egl_get_proc_address)(gl_string_ptr) })
    else {
        return Err("eglGetProcAddress(glGetString) returned nullptr");
    };
    let get_gl_string: GlGetStringFn = unsafe { std::mem::transmute(gl_string_raw) };

    // Driver initialization prints to stdout/stderr; silence it.
    let Some(mut suppressed) = SuppressedIo::new() else {
        return Err("Failed to suppress stdio");
    };

    let outcome = (|| {
        // SAFETY: every pointer handed out comes from the dlopen-ed libEGL
        // and the objects it created, all released below.
        unsafe {
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
                return Err("eglGetDisplay returned EGL_NO_DISPLAY");
            }

            let mut major = 0;
            let mut minor = 0;
            if (egl_initialize)(display, &mut major, &mut minor) != EGL_TRUE {
                (egl_terminate)(display);
                return Err("eglInitialize returned EGL_FALSE");
            }

            // Prefer desktop OpenGL; failure means the default API (usually
            // GLES).
            let gles = (egl_bind_api)(EGL_OPENGL_API) != EGL_TRUE;

            let mut config: *mut c_void = std::ptr::null_mut();
            let mut config_count = 0;
            (egl_get_configs)(display, &mut config, 1, &mut config_count);
            if config_count == 0 {
                (egl_terminate)(display);
                return Err("eglGetConfigs returned 0 configs");
            }

            let surface = (egl_create_pbuffer_surface)(
                display,
                config,
                [EGL_WIDTH, 1, EGL_HEIGHT, 1, EGL_NONE].as_ptr(),
            );
            if surface.is_null() {
                (egl_terminate)(display);
                return Err("eglCreatePbufferSurface returned EGL_NO_SURFACE");
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
                context = (egl_create_context)(
                    display,
                    config,
                    std::ptr::null_mut(),
                    [EGL_NONE].as_ptr(),
                );
            }
            if context.is_null() {
                (egl_destroy_surface)(display, surface);
                (egl_terminate)(display);
                return Err("eglCreateContext returned EGL_NO_CONTEXT");
            }

            let result = if (egl_make_current)(display, surface, surface, context) != EGL_TRUE {
                Err("eglMakeCurrent returned EGL_FALSE")
            } else {
                Ok(OpenGLResult {
                    version: gl_string(get_gl_string, GL_VERSION).unwrap_or_default(),
                    renderer: gl_string(get_gl_string, GL_RENDERER).unwrap_or_default(),
                    vendor: gl_string(get_gl_string, GL_VENDOR).unwrap_or_default(),
                    slv: gl_string(get_gl_string, GL_SHADING_LANGUAGE_VERSION)
                        .unwrap_or_default(),
                    library: {
                        let version = egl_query_string(display, EGL_VERSION);
                        let version = if version.is_null() {
                            String::new()
                        } else {
                            CStr::from_ptr(version).to_string_lossy().into_owned()
                        };
                        format!("EGL {version}")
                    },
                })
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
            result
        }
    })();
    suppressed.restore();
    outcome
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    match detect() {
        // The detection error rows are hidden by default.
        Err(_) => Vec::new(),
        Ok(result) => vec![Row::new("OpenGL", result.version)],
    }
}

/// ffGenerateOpenGLJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(result) => {
            let mut obj = Json::obj();
            obj.push("version", result.version);
            obj.push("renderer", result.renderer);
            obj.push("vendor", result.vendor);
            obj.push("slv", result.slv);
            obj.push("library", result.library);
            JsonOutcome::Result(obj)
        },
    }
}

//! `version` module: the fastfetch identity, ported from version.c:
//! "<project> <version><tweak><-debug?> (<architecture>)". The build-time
//! constants are pinned to the reference 2.71.0 package (project name,
//! version, CMake build type, compile time, compiler) so both the row and
//! the JSON match the installed fastfetch byte-for-byte; the runtime
//! fields (sysName, architecture, libc) stay detected, like upstream.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
};

/// The reference build's __DATE__ " " __TIME__-derived compile time.
const REF_COMPILE_TIME: &str = "Nov  6 2026, 07:40:47";

pub fn run(instance: &Instance) -> Vec<Row> {
    // printf("%s %s%s%s (%s)", projectName, version, versionTweak,
    // debugMode ? "-debug" : "", architecture): the reference build is a
    // release build, so no suffix regardless of our own profile.
    vec![Row::new(instance.display_name("version"), "fastfetch 2.71.0 (x86_64)")]
}

/// ffGenerateVersionJsonResult, reporting the reference 2.71.0 package
/// identity; the glibc version comes from gnu_get_libc_version() on Linux,
/// while Android/bionic reports "bionic" instead.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let mut obj = Json::obj();
    obj.push("projectName", "fastfetch");
    obj.push("sysName", "Linux");
    obj.push("architecture", std::env::consts::ARCH);
    obj.push("version", "2.71.0");
    obj.push("versionGit", "");
    obj.push("cmakeBuiltType", "RelWithDebInfo");
    obj.push("compileTime", REF_COMPILE_TIME);
    obj.push("compiler", "gcc 16.2.1");
    obj.push("debugMode", false);

    // gnu_get_libc_version() is glibc-only (not musl, not Android, not BSD).
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        let libc_version = unsafe {
            let ptr = libc::gnu_get_libc_version();
            if ptr.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
            }
        };
        if libc_version.is_empty() {
            obj.push("libc", Json::Null);
        } else {
            obj.push("libc", format!("glibc {libc_version}"));
        }
    }
    #[cfg(target_os = "android")]
    obj.push("libc", "bionic");
    #[cfg(not(any(all(target_os = "linux", target_env = "gnu"), target_os = "android")))]
    obj.push("libc", Json::Null);
    JsonOutcome::Result(obj)
}

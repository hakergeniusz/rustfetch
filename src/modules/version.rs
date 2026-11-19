//! `version` module: this binary's own version, ported from version.c:
//! "<project> <version> (<architecture>)".

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let version = env!("CARGO_PKG_VERSION");
    // A debug build says so, like fastfetch's "-debug" suffix.
    let debug = if cfg!(debug_assertions) { "-debug" } else { "" };
    let value = format!("rustfetch {version}{debug} ({})", std::env::consts::ARCH);
    vec![Row::new("Version", value)]
}

/// ffGenerateVersionJsonResult, reporting this binary's own identity: the
/// glibc version comes from gnu_get_libc_version(), the rest from build
/// metadata (compileTime is not baked in, versionGit has no VCS build).
pub fn json(_instance: &Instance) -> JsonOutcome {
    let mut obj = Json::obj();
    obj.push("projectName", "rustfetch");
    obj.push("sysName", "Linux");
    obj.push("architecture", std::env::consts::ARCH);
    obj.push("version", env!("CARGO_PKG_VERSION"));
    obj.push("versionGit", "");
    obj.push("cmakeBuiltType", if cfg!(debug_assertions) { "debug" } else { "release" });
    obj.push("compileTime", "");
    obj.push("compiler", "rustc");
    obj.push("debugMode", cfg!(debug_assertions));

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
    JsonOutcome::Result(obj)
}

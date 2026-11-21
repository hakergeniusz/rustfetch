//! `kernel` module: kernel type and release from uname, which
//! ffGenerateKernelJsonResult serializes with the page size.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    platform,
};

pub fn run(instance: &Instance) -> Vec<Row> {
    let sysinfo = &platform::platform().sysinfo;
    vec![Row::new(instance.display_name("kernel"), format!("{} {}", sysinfo.name, sysinfo.release))]
}

pub fn json(_instance: &Instance) -> JsonOutcome {
    let sysinfo = &platform::platform().sysinfo;
    let mut result = Json::obj();
    result.push("architecture", sysinfo.architecture.clone());
    result.push("name", sysinfo.name.clone());
    result.push("release", sysinfo.release.clone());
    result.push("version", sysinfo.version.clone());
    result.push("pageSize", sysinfo.page_size);
    JsonOutcome::Result(result)
}

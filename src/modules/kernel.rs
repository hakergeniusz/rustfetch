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
    let Some(format) = crate::modules::entry_format(instance.entry) else {
        return vec![Row::new(instance.display_name("kernel"), format!("{} {}", sysinfo.name, sysinfo.release))];
    };

    // The custom-format branch follows FF_PRINT_FORMAT_CHECKED's arg list:
    // sysname/release/version/arch straight from uname, with the page size
    // formatted by ffSizeAppendNum (the C source builds this arg only here).
    let mut page_size = String::new();
    crate::format::size_append_num(&mut page_size, sysinfo.page_size, &instance.display_format);
    let args = [
        crate::format_args::FormatArg { name: "sysname", value: crate::format_args::ArgValue::Str(&sysinfo.name) },
        crate::format_args::FormatArg { name: "release", value: crate::format_args::ArgValue::Str(&sysinfo.release) },
        crate::format_args::FormatArg { name: "version", value: crate::format_args::ArgValue::Str(&sysinfo.version) },
        crate::format_args::FormatArg { name: "arch", value: crate::format_args::ArgValue::Str(&sysinfo.architecture) },
        crate::format_args::FormatArg { name: "page-size", value: crate::format_args::ArgValue::Str(&page_size) },
    ];
    let rendered = crate::format_args::parse_format_string(format, &args, args.len() as u32, &crate::format_args::EngineEnv::new(instance));
    vec![Row::new(instance.display_name("kernel"), rendered)]
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

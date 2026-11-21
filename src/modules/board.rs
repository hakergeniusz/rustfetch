//! `board` module: motherboard information from the DMI tables, ported from
//! board_linux.c with the devicetree fallbacks.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::{host, Row},
    util,
};

pub struct BoardResult {
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub serial: String,
}

/// ffDetectBoard.
pub fn detect() -> BoardResult {
    let mut result = BoardResult {
        name: String::new(),
        vendor: String::new(),
        version: String::new(),
        serial: String::new(),
    };

    if !host::get_smbios_value(
        "/sys/devices/virtual/dmi/id/board_name",
        "/sys/class/dmi/id/board_name",
    )
    .is_empty()
    {
        result.name = host::get_smbios_value("/sys/devices/virtual/dmi/id/board_name", "/sys/class/dmi/id/board_name");
        result.serial = host::get_smbios_value("/sys/devices/virtual/dmi/id/board_serial", "/sys/class/dmi/id/board_serial");
        result.vendor = host::get_smbios_value("/sys/devices/virtual/dmi/id/board_vendor", "/sys/class/dmi/id/board_vendor");
        result.version = host::get_smbios_value("/sys/devices/virtual/dmi/id/board_version", "/sys/class/dmi/id/board_version");
    } else if let Some(name) = util::read_file("/sys/firmware/devicetree/base/smbios/smbios/baseboard/product") {
        result.name = name.trim_end_matches('\0').to_string();
        if let Some(vendor) = util::read_file("/sys/firmware/devicetree/base/smbios/smbios/baseboard/manufacturer") {
            result.vendor = vendor.trim_end_matches('\0').to_string();
        }
    } else if let Some(name) = util::read_file("/sys/firmware/devicetree/base/board") {
        result.name = name.trim_end().to_string();
    } else if let Some(vendor) = util::read_file("/sys/firmware/devicetree/base/compatible") {
        // "vendor,model" splits into the two fields.
        let vendor = vendor.trim_end_matches('\0');
        if let Some((vendor_part, name)) = vendor.split_once(',') {
            result.vendor = vendor_part.to_string();
            result.name = name.trim_end().to_string();
        }
    }

    result
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let board = detect();
    if board.name.is_empty() {
        // "board_name is not set." is an error row, hidden by default.
        return Vec::new();
    }

    let Some(format) = crate::modules::entry_format(instance.entry) else {
        let mut value = board.name;
        if !board.version.is_empty()
            && !board.version.eq_ignore_ascii_case("none")
        {
            value.push_str(&format!(" ({})", board.version));
        }
        return vec![Row::new(instance.display_name("board"), value)];
    };

    // Upstream FF_PRINT_FORMAT_CHECKED passes the four board strbufs as
    // FF_ARG_TYPE_STRBUF values. Borrow them here, before the default path
    // would move `name`/`version` into the composed `value`.
    let args = [
        crate::format_args::FormatArg { name: "name", value: crate::format_args::ArgValue::Str(&board.name) },
        crate::format_args::FormatArg { name: "vendor", value: crate::format_args::ArgValue::Str(&board.vendor) },
        crate::format_args::FormatArg { name: "version", value: crate::format_args::ArgValue::Str(&board.version) },
        crate::format_args::FormatArg { name: "serial", value: crate::format_args::ArgValue::Str(&board.serial) },
    ];
    let rendered = crate::format_args::parse_format_string(format, &args, args.len() as u32, &crate::format_args::EngineEnv::new(instance));
    vec![Row::new(instance.display_name("board"), rendered)]
}

/// ffGenerateBoardJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let board = detect();
    if board.name.is_empty() {
        return JsonOutcome::Error("board_name is not set.".to_string());
    }
    let mut obj = Json::obj();
    obj.push("name", board.name);
    obj.push("vendor", board.vendor);
    obj.push("version", board.version);
    obj.push("serial", board.serial);
    JsonOutcome::Result(obj)
}

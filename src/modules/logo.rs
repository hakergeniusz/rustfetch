//! `logo` module: port of modules/logo/logo.c, whose text-mode printer
//! always reports "Supported in JSON format only" — the logo itself is
//! rendered outside the module system (see crate::logo). The error row is
//! hidden by default, so selecting the module prints just the logo, like
//! fastfetch. The JSON generator exposes the builtin logo table.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    logo_entries::self,
    modules::Row,
};

pub fn run(_instance: &Instance) -> Vec<Row> {
    // ffPrintLogo: the text-mode printer is this one error row; collect's
    // showErrors gate hides it by default.
    vec![Row::error("Logo", "Supported in JSON format only")]
}

use crate::logo::{get_builtin, get_builtin_detected, Size};

/// ffGenerateLogoJsonResult: the builtin logo table as JSON. The logo
/// state maps onto upstream's FFLogoType: NONE — like every other type
/// this port can not render — is rejected up front, SMALL picks the
/// compact variant, and any other state looks the source up in the
/// builtin table (or the detected distro when no source is set).
pub fn json(instance: &Instance) -> JsonOutcome {
    if instance.logo_none {
        return JsonOutcome::Error(
            "Only 'builtin' and 'small' logo types are supported".to_string(),
        );
    }
    let small = instance.logo_small;
    let size = if small { Size::Small } else { Size::Unknown };
    let source = instance.logo_source.as_deref();
    let entry = match source {
        // "?" is ffLogoGetBuiltinForName's alias for the unknown logo.
        Some("?") => &logo_entries::UNKNOWN_ENTRY,
        Some(name) => match get_builtin(name, size) {
            Some(entry) => entry,
            None => {
                return JsonOutcome::Error(
                    "No built-in logo found for the specified name/size".to_string(),
                )
            },
        },
        None => get_builtin_detected(instance, size),
    };

    let mut obj = Json::obj();
    // fastfetch_encode_c_string strips one trailing newline from the art.
    obj.push("lines", entry.art.strip_suffix('\n').unwrap_or(entry.art));

    let mut names = Json::Array(Vec::new());
    for name in entry.names {
        names.push_value(*name);
    }
    obj.push("names", names);

    let mut colors = Json::Array(Vec::new());
    for color in entry.colors {
        colors.push_value(*color);
    }
    obj.push("colors", colors);

    // yyjson_mut_obj_add_str skips null values entirely, so unset
    // colorKeys/colorTitle stay absent.
    if let Some(color_keys) = entry.color_keys {
        obj.push("colorKeys", color_keys);
    }
    if let Some(color_title) = entry.color_title {
        obj.push("colorTitle", color_title);
    }

    // FF_LOGO_LINE_TYPE_NORMAL is 0, so a bit test would always be false;
    // the "normal" case is an equality test, like the text renderer's.
    let mut types = Json::Array(Vec::new());
    if entry.line_type == 0 {
        types.push_value("normal");
    }
    if entry.line_type & 1 != 0 {
        types.push_value("small");
    }
    if entry.line_type & 2 != 0 {
        types.push_value("alter");
    }
    obj.push("type", types);

    JsonOutcome::Result(obj)
}

//! `logo` module: port of modules/logo/logo.c, whose text-mode printer
//! always reports "Supported in JSON format only" — the logo itself is
//! rendered outside the module system (see crate::logo). The error row is
//! hidden by default, so selecting the module prints just the logo, like
//! fastfetch. The JSON generator exposes the builtin logo table.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    logo_entries::{self, LogoEntry},
    modules::Row,
};

pub fn run(_instance: &Instance) -> Vec<Row> {
    // "Supported in JSON format only" is an error row, hidden by default.
    Vec::new()
}

/// FFLogoSize reduced to the values the JSON generator can produce
/// (Normal exists upstream but is only reachable via --logo-type).
#[derive(Clone, Copy, PartialEq)]
enum Size {
    #[allow(dead_code)]
    Normal,
    Small,
    Unknown,
}

/// ffLogoGetBuiltin: only entries whose first name shares the requested
/// name's letter are examined (upstream's per-letter tables), filtered by
/// the size's line type, then matched case-insensitively; with small size
/// a name also matches a "_small"-suffixed entry name.
fn find_entry(name: &str, size: Size) -> Option<&'static LogoEntry> {
    let first = name.chars().next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let first = first.to_ascii_lowercase();
    for entry in logo_entries::LOGO_ENTRIES {
        let Some(candidate) = entry.names.first() else { continue };
        let Some(bucket) = candidate.chars().next() else { continue };
        if bucket.to_ascii_lowercase() != first {
            continue;
        }
        let type_ok = match size {
            Size::Normal => entry.line_type == 0,
            Size::Small => entry.line_type == 1,
            Size::Unknown => true,
        };
        if !type_ok {
            continue;
        }
        if logo_has_name(entry, name, size == Size::Small) {
            return Some(entry);
        }
    }
    None
}

/// logoHasName.
fn logo_has_name(entry: &LogoEntry, name: &str, small: bool) -> bool {
    for logo_name in entry.names {
        if small
            && let Some(stripped) = logo_name.strip_suffix("_small")
            && stripped.len() == name.len()
            && stripped.eq_ignore_ascii_case(name)
        {
            return true;
        }
        if logo_name.eq_ignore_ascii_case(name) {
            return true;
        }
    }
    false
}

/// ffLogoGetBuiltinDetected: the os id, then the idLike tokens, then the
/// unknown logo. Upstream also tries os->name (PRETTY_NAME) and the kernel
/// name between them; neither is reachable when the id matches and they
/// are not ported here.
fn detected_entry(instance: &Instance, size: Size) -> &'static LogoEntry {
    if let Some(id) = &instance.distro_id
        && let Some(entry) = find_entry(id, size)
    {
        return entry;
    }
    for like in &instance.distro_like {
        if let Some(entry) = find_entry(like, size) {
            return entry;
        }
    }
    &logo_entries::UNKNOWN_ENTRY
}

/// ffGenerateLogoJsonResult: the builtin logo table as JSON. The CLI's
/// --logo values map onto upstream's logo options: `none` is the NONE
/// type (rejected like upstream), `small` is the SMALL type, any other
/// value is a builtin name looked up in any variant.
pub fn json(instance: &Instance) -> JsonOutcome {
    let source = instance.logo_source.as_deref();
    if source.is_some_and(|value| value.eq_ignore_ascii_case("none")) {
        return JsonOutcome::Error(
            "Only 'builtin' and 'small' logo types are supported".to_string(),
        );
    }
    let small = source.is_some_and(|value| value.eq_ignore_ascii_case("small"));
    let size = if small { Size::Small } else { Size::Unknown };
    let entry = match source.filter(|_| !small) {
        // "?" is ffLogoGetBuiltinForName's alias for the unknown logo.
        Some("?") => &logo_entries::UNKNOWN_ENTRY,
        Some(name) => match find_entry(name, size) {
            Some(entry) => entry,
            None => {
                return JsonOutcome::Error(
                    "No built-in logo found for the specified name/size".to_string(),
                )
            },
        },
        None => detected_entry(instance, size),
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

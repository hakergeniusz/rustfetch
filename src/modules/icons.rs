//! `icons` module: Qt (kdeglobals) and GTK (per-version) icon themes joined
//! with fastfetch's icons1/icons2 layout ("Breeze [Qt], Adwaita [GTK2/3/4]").

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::{displayserver, Row},
};

/// ffDetectIcons: the TTY gate, the Qt part as icons1, the grouped GTK
/// themes as icons2.
fn detect(instance: &Instance) -> Result<(String, String), &'static str> {
    let ds = displayserver::connect();
    if ds.wm_protocol_name.eq_ignore_ascii_case(displayserver::PROTOCOL_TTY) {
        return Err("Icons aren't supported in TTY");
    }

    let qt = super::qt::detect(instance);
    let versions = super::gtk::detect(instance);
    let icons1 = qt
        .icons
        .as_ref()
        .map(|icons| format!("{icons} [Qt]"))
        .unwrap_or_default();
    let icons2 = super::gtk::parse_gtk(
        versions.v2.icons.as_deref(),
        versions.v3.icons.as_deref(),
        versions.v4.icons.as_deref(),
    )
    .unwrap_or_default();

    if icons1.is_empty() && icons2.is_empty() {
        return Err("No icons could be found");
    }
    Ok((icons1, icons2))
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok((icons1, icons2)) = detect(instance) else {
        return Vec::new();
    };

    let mut value = icons1;
    if !icons2.is_empty() {
        if !value.is_empty() {
            value.push_str(", ");
        }
        value.push_str(&icons2);
    }

    if value.is_empty() {
        Vec::new()
    } else {
        vec![Row::new(instance.display_name("icons"), value)]
    }
}

/// ffGenerateIconsJsonResult.
pub fn json(instance: &Instance) -> JsonOutcome {
    match detect(instance) {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok((icons1, icons2)) => {
            let mut obj = Json::obj();
            obj.push("icons1", icons1);
            obj.push("icons2", icons2);
            JsonOutcome::Result(obj)
        },
    }
}

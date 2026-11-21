//! `theme` module: Qt (kdeglobals) and GTK (per-version) themes joined with
//! fastfetch's theme1/theme2 layout ("Breeze (Light) [Qt], Adwaita [GTK2/3/4]").

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::{displayserver, Row},
};

/// fastfetch's theme1: widget style plus color scheme, tagged [Qt].
fn qt_theme(qt: &super::qt::QtResult) -> Option<String> {
    if qt.widget_style.is_none() && qt.color_scheme.is_none() {
        return None;
    }

    let mut theme = match (&qt.widget_style, &qt.color_scheme) {
        (Some(style), Some(scheme)) => {
            // "Breeze" + "BreezeLight" → "Breeze (Light)"
            let pretty = if scheme.to_ascii_lowercase().starts_with(&style.to_ascii_lowercase()) {
                scheme[style.len()..].trim().to_string()
            } else {
                scheme.clone()
            };
            if pretty.is_empty() {
                format!("{style} ({scheme})")
            } else {
                format!("{style} ({pretty})")
            }
        },
        (Some(style), None) => style.clone(),
        (None, Some(scheme)) => scheme.clone(),
        (None, None) => unreachable!(),
    };
    theme.push_str(" [Qt]");
    Some(theme)
}

/// ffDetectTheme: the TTY gate, the Qt part as theme1 and the grouped GTK
/// themes as theme2 (which may legitimately be empty).
fn detect(instance: &Instance) -> Result<(String, String), &'static str> {
    let ds = displayserver::connect();
    if ds.wm_protocol_name.eq_ignore_ascii_case(displayserver::PROTOCOL_TTY) {
        return Err("Theme isn't supported in TTY");
    }

    let qt = super::qt::detect(instance);
    let versions = super::gtk::detect(instance);
    let theme1 = qt_theme(&qt).unwrap_or_default();
    let theme2 = super::gtk::parse_gtk(
        versions.v2.theme.as_deref(),
        versions.v3.theme.as_deref(),
        versions.v4.theme.as_deref(),
    )
    .unwrap_or_default();

    if theme1.is_empty() && theme2.is_empty() {
        return Err("No themes found");
    }
    Ok((theme1, theme2))
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok((theme1, theme2)) = detect(instance) else {
        return Vec::new();
    };

    let Some(format) = crate::modules::entry_format(instance.entry) else {
        let mut value = theme1;
        if !theme2.is_empty() {
            if !value.is_empty() {
                value.push_str(", ");
            }
            value.push_str(&theme2);
        }

        return if value.is_empty() {
            Vec::new()
        } else {
            vec![Row::new(instance.display_name("theme"), value)]
        };
    };

    // Upstream FF_PRINT_FORMAT_CHECKED passes the two detected strbufs as
    // `theme1` and `theme2` string args.
    let args = [
        crate::format_args::FormatArg { name: "theme1", value: crate::format_args::ArgValue::Str(&theme1) },
        crate::format_args::FormatArg { name: "theme2", value: crate::format_args::ArgValue::Str(&theme2) },
    ];
    let rendered = crate::format_args::parse_format_string(format, &args, args.len() as u32, &crate::format_args::EngineEnv::new(instance));
    vec![Row::new(instance.display_name("theme"), rendered)]
}

/// ffGenerateThemeJsonResult.
pub fn json(instance: &Instance) -> JsonOutcome {
    match detect(instance) {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok((theme1, theme2)) => {
            let mut obj = Json::obj();
            obj.push("theme1", theme1);
            obj.push("theme2", theme2);
            JsonOutcome::Result(obj)
        },
    }
}

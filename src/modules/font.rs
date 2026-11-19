//! `font` module: desktop font per source with tags, ported from fastfetch's
//! font module: Qt first ("... [Qt]") then the GTK fonts via ffParseGTK.
//! The JSON result exposes the per-source pretty fonts.

use crate::{
    ffont::FFfont,
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::{displayserver, Row},
};

pub struct FontResult {
    pub display: String,
    /// [qt, gtk2, gtk3, gtk4] pretty fonts.
    pub fonts: [String; 4],
}

/// ffDetectFont: TTY gate, then the Qt and GTK pretty fonts; an empty set
/// is an error, like ffDetectFont's final check.
pub fn detect(instance: &Instance) -> Result<FontResult, &'static str> {
    let ds = displayserver::connect();
    if ds.wm_protocol_name.eq_ignore_ascii_case(displayserver::PROTOCOL_TTY) {
        return Err("Font isn't supported in TTY");
    }

    let qt = super::qt::detect(instance);
    let versions = super::gtk::detect(instance);

    let fonts = [
        FFfont::init_qt(qt.font.as_deref().unwrap_or("")).pretty,
        FFfont::init_pango(versions.v2.font.as_deref().unwrap_or("")).pretty,
        FFfont::init_pango(versions.v3.font.as_deref().unwrap_or("")).pretty,
        FFfont::init_pango(versions.v4.font.as_deref().unwrap_or("")).pretty,
    ];

    // generateString: the Qt font tagged and followed by ", " when any GTK
    // font exists, then the grouped GTK values.
    let mut display = String::new();
    if !fonts[0].is_empty() {
        display.push_str(&fonts[0]);
        display.push_str(" [Qt]");
        if fonts[1..].iter().any(|font| !font.is_empty()) {
            display.push_str(", ");
        }
    }
    if let Some(gtk) = super::gtk::parse_gtk(
        (!fonts[1].is_empty()).then_some(fonts[1].as_str()),
        (!fonts[2].is_empty()).then_some(fonts[2].as_str()),
        (!fonts[3].is_empty()).then_some(fonts[3].as_str()),
    ) {
        display.push_str(&gtk);
    }

    if fonts.iter().all(|font| font.is_empty()) {
        return Err("No fonts found");
    }
    Ok(FontResult { display, fonts })
}

pub fn run(instance: &Instance) -> Vec<Row> {
    match detect(instance) {
        Ok(font) => vec![Row::new("Font", font.display)],
        Err(_) => Vec::new(),
    }
}

/// ffGenerateFontJsonResult: {display, fonts[4]} or the detection error.
pub fn json(instance: &Instance) -> JsonOutcome {
    match detect(instance) {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(font) => {
            let mut result = Json::obj();
            result.push("display", font.display);
            let mut arr = Json::Array(Vec::new());
            for value in font.fonts {
                arr.push_value(Json::Str(value));
            }
            result.push("fonts", arr);
            JsonOutcome::Result(result)
        },
    }
}

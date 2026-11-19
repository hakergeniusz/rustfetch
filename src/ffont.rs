//! FFfont, ported from common/impl/font.c: decomposes a font into name,
//! size and styles plus the combined "Name (size[pt], styles)" pretty form.
//! Shared by the font and terminalfont modules and their JSON generators.

/// One parsed font. `size` keeps the raw parsed text ("11", "11.25pt",
/// "20px"); styles may legitimately contain empty entries, which upstream's
/// Qt parser produces for consecutive spaces.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct FFfont {
    pub pretty: String,
    pub name: String,
    pub size: String,
    pub styles: Vec<String>,
}

/// ffStrbufAppendSUntilC: append up to (excluding) `until`, or the whole
/// value when absent; returns the position after the delimiter or None.
fn append_until_c<'a>(buffer: &mut String, value: &'a str, until: char) -> Option<&'a str> {
    match value.find(until) {
        Some(index) => {
            buffer.push_str(&value[..index]);
            Some(&value[index + until.len_utf8()..])
        },
        None => {
            buffer.push_str(value);
            None
        },
    }
}

/// sscanf("%lf") == 1: a leading double with optional whitespace, sign,
/// digits, fraction and exponent — trailing junk allowed.
fn sscanf_double(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let mut digits = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    // The exponent is not consumed; sscanf only needs one leading double.
    let _ = i;
    true
}

/// isalpha in the C locale: ASCII letters only.
fn c_is_alpha_first(word: &[u8]) -> bool {
    word.first().is_some_and(|b| b.is_ascii_alphabetic())
}

const PANGO_STYLE_KEYWORDS: [&str; 14] = [
    "Ultra", "Extra", "Semi", "Demi", "Normal", "Roman", "Oblique", "Italic", "Thin", "Light",
    "Bold", "Black", "Condensed", "Expanded",
];

/// strbufAppendNSExcludingC: append the word minus `exclude` bytes.
fn append_excluding(buffer: &mut String, word: &str, exclude: char) {
    buffer.extend(word.chars().filter(|c| *c != exclude));
}

/// fontPangoParseWord: one Pango word routed into size, styles or the name.
/// `alternative` is the style entry that prefix keywords already claimed,
/// as its index into `styles`.
fn pango_parse_word(data: &[u8], pos: &mut usize, font: &mut FFfont, alternative: Option<usize>) {
    while *pos < data.len() && matches!(data[*pos], b' ' | b'\t' | b',') {
        *pos += 1;
    }
    let start = *pos;
    while *pos < data.len()
        && !matches!(data[*pos], b' ' | b'\t' | b',' | b'`' | b'\\')
    {
        *pos += 1;
    }
    if *pos == start {
        return;
    }
    let word = String::from_utf8_lossy(&data[start..*pos]).into_owned();
    let word_bytes = &data[start..*pos];

    // The last word (or one cut off by a backtick/backslash) is a size
    // candidate when it parses as a number, "px" suffix stripped.
    if *pos >= data.len() || matches!(data[*pos], b'`' | b'\\') {
        font.size.push_str(&word);
        if let Some(stripped) = font.size.strip_suffix("px") {
            let stripped_len = stripped.len();
            font.size.truncate(stripped_len);
        }
        if sscanf_double(&font.size) {
            return;
        }
        font.size.clear();
    }

    if PANGO_STYLE_KEYWORDS
        .iter()
        .any(|keyword| starts_ign_case(word_bytes, keyword))
    {
        let index = match alternative {
            Some(index) => index,
            None => {
                font.styles.push(String::new());
                font.styles.len() - 1
            },
        };
        append_excluding(&mut font.styles[index], &word, '-');

        // "Semi Bold" merges into one style entry via the next word;
        // "Semi-Bold" does not (the upstream check looks for a literal
        // "Semi " at the word start).
        let combined = ["Ultra", "Extra", "Semi", "Demi"]
            .iter()
            .any(|keyword| word.eq_ignore_ascii_case(keyword) && data.get(*pos) == Some(&b' '));
        if combined {
            let mut next = *pos;
            pango_parse_word(data, &mut next, font, Some(index));
            *pos = next;
        }
        return;
    }

    if let Some(index) = alternative {
        append_excluding(&mut font.styles[index], &word, '-');
        return;
    }

    if !font.name.is_empty() {
        font.name.push(' ');
    }
    font.name.push_str(&word);
}

/// ffStrStartsWithIgnCase on byte slices.
fn starts_ign_case(bytes: &[u8], prefix: &str) -> bool {
    bytes.len() >= prefix.len()
        && bytes[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

impl FFfont {
/// ffFontInitPango: parse "Adwaita Sans 11" / "Fira Code Semi-Bold 10.5".
pub fn init_pango(data: &str) -> FFfont {
    let mut font = FFfont::default();
    let bytes = data.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() && !matches!(bytes[pos], b'`' | b'\\') {
        pango_parse_word(bytes, &mut pos, &mut font, None);
    }
    font.init_pretty();
    font
}

/// ffFontInitQt: QFont::toString — family, size, then the style name in
/// the last field split into words.
pub fn init_qt(data: &str) -> FFfont {
    let mut font = FFfont::default();

    // Family
    let Some(rest) = append_until_c(&mut font.name, data, ',') else {
        font.name = font.name.trim_matches(' ').to_string();
        font.init_pretty();
        return font;
    };
    font.name = font.name.trim_matches(' ').to_string();
    let mut rest = rest;

    // Size
    let mut size = String::new();
    let Some(rest2) = append_until_c(&mut size, rest, ',') else {
        font.size = size.trim_matches(' ').to_string();
        font.init_pretty();
        return font;
    };
    font.size = size.trim_matches(' ').to_string();
    rest = rest2;

    // Style: the LAST field; words split on single spaces (empty entries
    // kept, like the upstream list).
    let Some(style_start) = rest.rfind(',') else {
        font.init_pretty();
        return font;
    };
    let style = &rest[style_start + 1..];
    if c_is_alpha_first(style.as_bytes()) {
        font.styles.extend(style.split(' ').map(str::to_string));
    }

    font.init_pretty();
    font
}

/// ffFontInitValues: raw name ('"' trimmed) and size.
pub fn init_values(name: &str, size: &str) -> FFfont {
    let mut font = FFfont {
        name: name.trim_matches('"').to_string(),
        size: size.to_string(),
        ..FFfont::default()
    };
    font.init_pretty();
    font
}

/// ffFontInitCopy: a bare font name.
pub fn init_copy(name: &str) -> FFfont {
    Self::init_values(name, "")
}

/// ffFontInitMoveValues: pre-split name, size and one style string.
pub fn init_move_values(name: &str, size: &str, style: Option<String>) -> FFfont {
    let mut font = FFfont {
        name: name.to_string(),
        size: size.to_string(),
        styles: style.into_iter().collect(),
        ..FFfont::default()
    };
    font.init_pretty();
    font
}

/// ffFontInitXlfd: X logical font description; family is field 1, the
/// size prefers pointsize (decipoints) over pixelsize, styles come from
/// the weight/slant/setwidth/addstyle fields except "normal".
pub fn init_xlfd(xlfd: &str) -> FFfont {
    let mut font = FFfont::default();

    // Skip the leading '-' so the field indexes align with the docs.
    let body = xlfd.strip_prefix('-').unwrap_or(xlfd);
    let mut start = body;
    for field in 0..14u8 {
        let (chunk, next) = match start.find('-') {
            Some(index) => (&start[..index], Some(&start[index + 1..])),
            None => (start, None),
        };
        if !chunk.is_empty() {
            match field {
                1 => font.name.push_str(chunk),
                7 => {
                    // pointsize, in decipoints
                    if let Some(deci_pt) = chunk.parse::<i64>().ok().filter(|&v| v > 0) {
                        font.size.clear();
                        if deci_pt % 10 == 0 {
                            font.size = format!("{}pt", deci_pt / 10);
                        } else {
                            font.size = format!("{}.{}pt", deci_pt / 10, deci_pt % 10);
                        }
                    }
                },
                6 => {
                    // pixelsize, only when pointsize hasn't claimed the size
                    if font.size.is_empty()
                        && let Some(px) = chunk.parse::<i64>().ok().filter(|&v| v > 0)
                    {
                        font.size = format!("{px}px");
                    }
                },
                2..=5 if !(chunk.len() == 6 && chunk.eq_ignore_ascii_case("normal")) => {
                    font.styles.push(chunk.to_string());
                },
                _ => {},
            }
        }
        let Some(next) = next else { break };
        start = next;
    }

    font.init_pretty();
    font
}

/// ffFontInitXft: fontconfig pattern — "Family-10", the first of several
/// comma-separated families, plus size/pixelsize/style/weight/slant/width
/// keys after ':'.
pub fn init_xft(xft: &str) -> FFfont {
    let mut font = FFfont::default();
    let data = xft.as_bytes();
    let mut p = 0;

    while p < data.len() && matches!(data[p], b' ' | b'\t') {
        p += 1;
    }
    let head_start = p;
    while p < data.len() && data[p] != b':' {
        p += 1;
    }
    let mut head_end = p;
    while head_end > head_start && matches!(data[head_end - 1], b' ' | b'\t') {
        head_end -= 1;
    }
    // Multiple families: take the first one.
    if let Some(comma) = data[head_start..head_end].iter().position(|&b| b == b',') {
        head_end = head_start + comma;
        while head_end > head_start && matches!(data[head_end - 1], b' ' | b'\t') {
            head_end -= 1;
        }
    }
    let head = &data[head_start..head_end];

    // A trailing "-<number>" head suffix is the size.
    let mut dash_pos = None;
    let mut q = head_end;
    while q > head_start {
        q -= 1;
        if head[q - head_start] == b'-' && q + 1 < head_end && data[q + 1].is_ascii_digit() {
            dash_pos = Some(q);
            break;
        }
    }

    if let Some(dash) = dash_pos {
        let size_start = dash + 1;
        let mut ok = true;
        let mut seen_digit = false;
        for &b in &data[size_start..head_end] {
            if b.is_ascii_digit() {
                seen_digit = true;
            } else if b != b'.' {
                ok = false;
                break;
            }
        }
        if ok && seen_digit {
            let mut name_end = dash;
            while name_end > head_start && matches!(data[name_end - 1], b' ' | b'\t') {
                name_end -= 1;
            }
            if name_end > head_start {
                font.name
                    .push_str(&String::from_utf8_lossy(&data[head_start..name_end]));
            }
            if head_end > size_start {
                font.size
                    .push_str(&String::from_utf8_lossy(&data[size_start..head_end]));
            }
        } else if head_end > head_start {
            font.name.push_str(&String::from_utf8_lossy(head));
        }
    } else if head_end > head_start {
        font.name.push_str(&String::from_utf8_lossy(head));
    }

    font.name = font.name.trim_matches(' ').to_string();
    font.name = font.name.trim_matches('"').to_string();

    // key=value fields after ':'
    while p < data.len() && data[p] == b':' {
        p += 1;
        let key_start = p;
        while p < data.len() && data[p] != b'=' && data[p] != b':' {
            p += 1;
        }
        if p >= data.len() || data[p] != b'=' {
            continue;
        }
        p += 1;

        // Value: until the next ':', backslash-escapes taken literally.
        let mut value_bytes = Vec::new();
        while p < data.len() && data[p] != b':' {
            if data[p] == b'\\' && p + 1 < data.len() {
                p += 1;
            }
            value_bytes.push(data[p]);
            p += 1;
        }
        let value = String::from_utf8_lossy(&value_bytes)
            .trim_matches(' ')
            .trim_matches('"')
            .to_string();
        if value.is_empty() {
            continue;
        }

        // The key ended at the '=' we consumed.
        let equals = key_start
            + data[key_start..]
                .iter()
                .position(|&b| b == b'=')
                .unwrap_or(p - key_start);
        let key_slice = std::str::from_utf8(&data[key_start..equals]).unwrap_or("");

        let size_empty = font.size.is_empty();
        if (key_slice.len() == 4 && key_slice.eq_ignore_ascii_case("size"))
            || (key_slice.len() == 9
                && (key_slice.eq_ignore_ascii_case("pixelsize")
                    || key_slice.eq_ignore_ascii_case("pointsize")))
        {
            if size_empty && value.as_bytes().first().is_some_and(|b| b.is_ascii_digit()) {
                font.size.push_str(&value);
                font.size.push_str(if key_slice.len() == 9 && key_slice.eq_ignore_ascii_case("pixelsize") {
                    "px"
                } else {
                    "pt"
                });
            }
        } else if key_slice.len() == 5 && key_slice.eq_ignore_ascii_case("style") {
            // A style may contain several words.
            for word in value.split([' ', '\t', ',']) {
                if !word.is_empty() {
                    font.styles.push(word.to_string());
                }
            }
        } else if (key_slice.len() == 6 && key_slice.eq_ignore_ascii_case("weight"))
            || (key_slice.len() == 5
                && (key_slice.eq_ignore_ascii_case("slant") || key_slice.eq_ignore_ascii_case("width")))
        {
            // "Semi-Bold" → "SemiBold" to align with the other parsers.
            let mut style = String::new();
            append_excluding(&mut style, &value, '-');
            let style = style.trim_matches(' ').to_string();
            if !style.is_empty() {
                font.styles.push(style);
            }
        }
    }

    font.init_pretty();
    font
}

    /// fontInitPretty: the combined "Name (size[pt], styles)" form, falling
    /// back to "default" when only size/styles were parsed.
    fn init_pretty(&mut self) {
        self.pretty = self.name.clone();
        if self.size.is_empty() && self.styles.is_empty() {
            return;
        }
        if self.pretty.is_empty() {
            self.pretty.push_str("default");
        }
        self.pretty.push_str(" (");

        if !self.size.is_empty() {
            self.pretty.push_str(&self.size);
            if !self.size.ends_with("pt") && !self.size.ends_with("px") {
                self.pretty.push_str("pt");
            }
            if !self.styles.is_empty() {
                self.pretty.push_str(", ");
            }
        }

        for (index, style) in self.styles.iter().enumerate() {
            self.pretty.push_str(style);
            if index < self.styles.len() - 1 {
                self.pretty.push_str(", ");
            }
        }

        self.pretty.push(')');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qt_decomposes_family_size_and_styles() {
        let font = FFfont::init_qt("Noto Sans Mono,10,-1,5,50,0,0,0,0,0");
        assert_eq!(font.name, "Noto Sans Mono");
        assert_eq!(font.size, "10");
        assert!(font.styles.is_empty());
        assert_eq!(font.pretty, "Noto Sans Mono (10pt)");

        let font = FFfont::init_qt("Fira Code,11,-1,5,50,0,0,0,0,0,Bold");
        assert_eq!(font.styles, vec!["Bold".to_string()]);
        assert_eq!(font.pretty, "Fira Code (11pt, Bold)");

        // Consecutive spaces keep empty entries, like the upstream list.
        let font = FFfont::init_qt("F,10,x,Bold  Italic");
        assert_eq!(font.styles, vec!["Bold".to_string(), "".to_string(), "Italic".to_string()]);
    }

    #[test]
    fn pango_splits_name_size_and_styles() {
        let font = FFfont::init_pango("Adwaita Sans 11");
        assert_eq!(font.name, "Adwaita Sans");
        assert_eq!(font.size, "11");
        assert!(font.styles.is_empty());
        assert_eq!(font.pretty, "Adwaita Sans (11pt)");

        let font = FFfont::init_pango("Fira Code Semi-Bold Italic 10.5");
        assert_eq!(font.name, "Fira Code");
        assert_eq!(font.size, "10.5");
        assert_eq!(font.styles, vec!["SemiBold".to_string(), "Italic".to_string()]);
        assert_eq!(font.pretty, "Fira Code (10.5pt, SemiBold, Italic)");

        // A non-numeric last word stays part of the name.
        let font = FFfont::init_pango("Just A Font");
        assert_eq!(font.name, "Just A Font");
        assert_eq!(font.size, "");
        assert_eq!(font.pretty, "Just A Font");

        // Sizes keep non-"pt" text verbatim in the pretty form.
        let font = FFfont::init_pango("Terminus 20px");
        assert_eq!(font.size, "20");
        assert_eq!(font.pretty, "Terminus (20pt)");
    }

    #[test]
    fn values_trims_quotes() {
        let font = FFfont::init_values("\"Cascadia Code\"", "18");
        assert_eq!(font.name, "Cascadia Code");
        assert_eq!(font.pretty, "Cascadia Code (18pt)");
        assert_eq!(FFfont::init_copy("Just A Name").pretty, "Just A Name");
    }

    #[test]
    fn move_values_keeps_one_style() {
        let font = FFfont::init_move_values("Fira Code", "11.25", Some("Regular".to_string()));
        assert_eq!(font.styles, vec!["Regular".to_string()]);
        assert_eq!(font.pretty, "Fira Code (11.25pt, Regular)");
    }

    #[test]
    fn xlfd_prefers_pointsize_and_collects_styles() {
        let font = FFfont::init_xlfd("-misc-DejaVu Sans Mono-medium-r-normal--0-120-75-75-m-0-ISO10646-1");
        assert_eq!(font.name, "DejaVu Sans Mono");
        assert_eq!(font.size, "12pt");
        assert_eq!(font.styles, vec!["medium".to_string(), "r".to_string()]);
        assert_eq!(font.pretty, "DejaVu Sans Mono (12pt, medium, r)");

        let font = FFfont::init_xlfd("-misc-Fixed-medium-r-normal--16-125-75-75-c-80-ISO10646-1");
        assert_eq!(font.size, "12.5pt");
        assert_eq!(font.name, "Fixed");

        // Without pointsize, pixelsize is used.
        let font = FFfont::init_xlfd("-misc-Fixed-medium-r-normal--16-0-75-75-c-80-ISO10646-1");
        assert_eq!(font.size, "16px");
    }

    #[test]
    fn xft_parses_head_and_keys() {
        let font = FFfont::init_xft("Berkeley Mono-12");
        assert_eq!(font.name, "Berkeley Mono");
        assert_eq!(font.size, "12");
        assert_eq!(font.pretty, "Berkeley Mono (12pt)");

        let font = FFfont::init_xft("Sarasa Term SC:size=8");
        assert_eq!(font.size, "8pt");
        assert_eq!(font.name, "Sarasa Term SC");

        let font = FFfont::init_xft("Term:pixelsize=16:style=Bold Italic");
        assert_eq!(font.size, "16px");
        assert_eq!(font.styles, vec!["Bold".to_string(), "Italic".to_string()]);

        let font = FFfont::init_xft("Fira Code-12:style=Regular:weight=Bold");
        assert_eq!(font.styles, vec!["Regular".to_string(), "Bold".to_string()]);
        assert_eq!(font.size, "12");
    }

    #[test]
    fn pretty_falls_back_to_default_name() {
        let font = FFfont::init_values("", "11");
        assert_eq!(font.pretty, "default (11pt)");
        let font = FFfont {
            name: String::new(),
            size: String::new(),
            styles: vec!["Bold".to_string()],
            pretty: String::new(),
        };
        let mut font = font;
        font.init_pretty();
        assert_eq!(font.pretty, "default (Bold)");
    }
}

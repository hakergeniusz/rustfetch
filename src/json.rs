//! JSON value tree plus a writer byte-compatible with the way fastfetch
//! serializes its result document: `yyjson_mut_write_fp(stdout, doc,
//! YYJSON_WRITE_INF_AND_NAN_AS_NULL | YYJSON_WRITE_PRETTY_TWO_SPACES |
//! YYJSON_WRITE_NEWLINE_AT_END)` (fastfetch.c). Two-space indentation, keys
//! separated by ": ", empty containers on a single line, a newline at the
//! very end.
//!
//! Reals follow yyjson's ECMAScript-style shortest formatter, which differs
//! from Rust's Display in three ways: integral doubles keep a trailing ".0"
//! (the writer is expected to be run per-field against upstream's
//! `yyjson_mut_obj_add_*` calls, so `add_real` and `add_uint` stay
//! distinguishable), NaN and infinity print as `null`, and magnitudes whose
//! decimal point sits outside (-6, 21] switch to scientific notation
//! without a plus sign or leading zeros ("2e34", "1e-7").

use std::fmt::Write as _;

/// One JSON value; objects keep insertion order, like yyjson's mut doc.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    // Null results come from every module generator with an optional
    // detection field (temperature, pcieSpeed, ...).
    #[allow(dead_code)]
    Null,
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

/// What a module's JSON generator produced for its `{type, ...}` item.
/// Mirrors genJsonResult: generators add `result` on success or `error`
/// with a message on failure; modules without a generator print the
/// fixed "Unsupported for JSON format" error.
pub enum JsonOutcome {
    Unsupported,
    Error(String),
    Result(Json),
}

impl Json {
    pub fn obj() -> Self {
        Json::Object(Vec::new())
    }

    /// Appends a key to an object, preserving call order.
    pub fn push(&mut self, key: &str, value: impl Into<Json>) {
        if let Json::Object(entries) = self {
            entries.push((key.to_string(), value.into()));
        }
    }

    /// Appends a key only when the option carries a value; absent options
    /// stay absent, like upstream skipping the obj_add call entirely.
    #[allow(dead_code)] // used as module generators migrate off `json: None`
    pub fn push_option(&mut self, key: &str, value: Option<impl Into<Json>>) {
        if let Some(value) = value {
            self.push(key, value.into());
        }
    }

    /// Appends an element to an array, mirroring yyjson_mut_arr_add_val.
    pub fn push_value(&mut self, value: impl Into<Json>) {
        if let Json::Array(items) = self {
            items.push(value.into());
        }
    }
}

impl From<&str> for Json {
    fn from(value: &str) -> Self {
        Json::Str(value.to_string())
    }
}

impl From<String> for Json {
    fn from(value: String) -> Self {
        Json::Str(value)
    }
}

impl From<bool> for Json {
    fn from(value: bool) -> Self {
        Json::Bool(value)
    }
}

impl From<u64> for Json {
    fn from(value: u64) -> Self {
        Json::U64(value)
    }
}

impl From<i64> for Json {
    fn from(value: i64) -> Self {
        Json::I64(value)
    }
}

impl From<f64> for Json {
    fn from(value: f64) -> Self {
        Json::F64(value)
    }
}

/// yyjson's pretty-two-spaces serialization, including the trailing newline
/// (YYJSON_WRITE_NEWLINE_AT_END).
pub fn to_pretty(value: &Json) -> String {
    to_pretty_indent(value, "  ")
}

/// yyjson's plain pretty serialization (YYJSON_WRITE_PRETTY, four spaces):
/// `-h format-json` writes its document with it, unlike the two-space
/// result writer.
pub fn to_pretty_indent(value: &Json, unit: &str) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0, unit);
    out.push('\n');
    out
}

fn indent(out: &mut String, level: usize, unit: &str) {
    for _ in 0..level {
        out.push_str(unit);
    }
}

fn write_value(out: &mut String, value: &Json, level: usize, unit: &str) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::I64(number) => {
            let _ = write!(out, "{number}");
        },
        Json::U64(number) => {
            let _ = write!(out, "{number}");
        },
        Json::F64(number) => out.push_str(&write_f64(*number)),
        Json::Str(text) => write_string(out, text),
        Json::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                indent(out, level + 1, unit);
                write_value(out, item, level + 1, unit);
                if index + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            indent(out, level, unit);
            out.push(']');
        },
        Json::Object(entries) => {
            if entries.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            for (index, (key, item)) in entries.iter().enumerate() {
                indent(out, level + 1, unit);
                write_string(out, key);
                out.push_str(": ");
                write_value(out, item, level + 1, unit);
                if index + 1 < entries.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            indent(out, level, unit);
            out.push('}');
        },
    }
}

/// write_str: `"` and `\` and the control shorthands, other controls as
/// uppercase \u00XX, everything else (including DEL and non-ASCII) raw.
fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            },
            c => out.push(c),
        }
    }
    out.push('"');
}

/// write_f64_raw's algorithmic branch: shortest digits (Rust's `{:e}` gives
/// exactly those), then either positional with the ".0" marker or scientific
/// without a plus sign.
fn write_f64(value: f64) -> String {
    if !value.is_finite() {
        // YYJSON_WRITE_INF_AND_NAN_AS_NULL.
        return "null".to_string();
    }
    if value == 0.0 {
        // Zero keeps its sign and the decimal-point marker.
        return if value.is_sign_negative() { "-0.0" } else { "0.0" }.to_string();
    }

    // "d[.ddd]e±X" with the shortest round-tripping digits; {:e} always
    // produces the 'e' separator and an i32 exponent.
    let scientific = format!("{:e}", value.abs());
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        return scientific;
    };
    let Ok(exponent) = exponent.parse::<i32>() else {
        return scientific;
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    // Position of the decimal point relative to the first digit.
    let dot_ofs = exponent + 1;

    let mut out = String::new();
    if value < 0.0 {
        out.push('-');
    }
    // (-6, 21] is written positionally; anything else is scientific.
    if dot_ofs > -6 && dot_ofs <= 21 {
        if dot_ofs <= 0 {
            out.push_str("0.");
            for _ in 0..-dot_ofs {
                out.push('0');
            }
            out.push_str(&digits);
        } else if dot_ofs as usize >= digits.len() {
            out.push_str(&digits);
            for _ in 0..(dot_ofs as usize - digits.len()) {
                out.push('0');
            }
            out.push_str(".0");
        } else {
            out.push_str(&digits[..dot_ofs as usize]);
            out.push('.');
            out.push_str(&digits[dot_ofs as usize..]);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let _ = write!(out, "e{}", dot_ofs - 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reals_match_yyjson_ecmascript_format() {
        assert_eq!(write_f64(96.0), "96.0");
        assert_eq!(write_f64(0.0), "0.0");
        assert_eq!(write_f64(-0.0), "-0.0");
        assert_eq!(write_f64(179.877), "179.877");
        assert_eq!(write_f64(59.95055010525479), "59.95055010525479");
        assert_eq!(write_f64(-3.5), "-3.5");
        assert_eq!(write_f64(1000.0), "1000.0");
        assert_eq!(write_f64(0.5), "0.5");
        assert_eq!(write_f64(0.05), "0.05");
        assert_eq!(write_f64(0.000005), "0.000005");
        // Outside (-6, 21]: scientific, no plus sign, no leading zeros.
        assert_eq!(write_f64(0.0000005), "5e-7");
        assert_eq!(write_f64(1e-7), "1e-7");
        assert_eq!(write_f64(1e21), "1e21");
        assert_eq!(write_f64(2.0e34), "2e34");
        // INF_AND_NAN_AS_NULL.
        assert_eq!(write_f64(f64::NAN), "null");
        assert_eq!(write_f64(f64::INFINITY), "null");
        assert_eq!(write_f64(f64::NEG_INFINITY), "null");
    }

    #[test]
    fn strings_escape_like_yyjson() {
        let mut out = String::new();
        write_string(&mut out, "a\"b\\c\nd\u{8}e\u{c}f\u{1}g\u{7f}héllo");
        assert_eq!(out, "\"a\\\"b\\\\c\\nd\\be\\ff\\u0001g\u{7f}héllo\"");
    }

    #[test]
    fn pretty_layout_matches_yyjson_two_spaces() {
        let mut item = Json::obj();
        item.push("type", "Uptime");
        let mut result = Json::obj();
        result.push("uptime", 3673440u64);
        result.push("bootTime", "2026-11-18T20:37:36.116+0100");
        item.push("result", result);
        let mut array = Json::Array(vec![]);
        array.push_value(item);
        assert_eq!(
            to_pretty(&array),
            r#"[
  {
    "type": "Uptime",
    "result": {
      "uptime": 3673440,
      "bootTime": "2026-11-18T20:37:36.116+0100"
    }
  }
]
"#
        );
    }

    #[test]
    fn empty_containers_stay_compact() {
        let mut object = Json::obj();
        object.push("result", Json::Array(vec![]));
        object.push("nested", Json::obj());
        assert_eq!(to_pretty(&object), "{\n  \"result\": [],\n  \"nested\": {}\n}\n");
    }
}
